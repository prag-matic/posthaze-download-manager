mod e2e_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use posthaze_engine::{DownloadEngine, DownloadState};
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use e2e_common::{
    ServeGuard, assert_matches_bench_pattern, configure_db_output_dir, extract_download_id,
    run_cli, run_cli_async, seed_paused_download, send_native_host_json, spawn_mock_http_server,
};

// ============================================================================
// EA7: CLI download, list, resume, cancel, delete, --debug / --stats-out
// ============================================================================

#[test]
fn ea7_download_happy_path_and_byte_exact_output() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let srv = ServeGuard::spawn(&["--size", "256K", "--port", "0"]);

    let out = run_cli(
        &["download", "--db", db_path.to_str().unwrap(), &srv.url],
        Some(dir.path()),
    );
    assert!(
        out.status.success(),
        "download failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let id = extract_download_id(&stdout);
    assert!(!id.is_empty());
    assert!(stdout.contains("Download Summary"), "stdout: {stdout}");

    let downloaded = std::fs::read(dir.path().join("bench.bin")).unwrap();
    assert_matches_bench_pattern(&downloaded, srv.size_bytes as usize);
}

#[test]
fn ea7_download_output_connections_and_sync_on_complete() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("custom_4m.bin");
    let srv = ServeGuard::spawn(&["--size", "4M", "--port", "0"]);

    let out = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "-c",
            "4",
            "--sync-on-complete",
            &srv.url,
        ],
        None,
    );
    assert!(
        out.status.success(),
        "download failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Range support:  true"), "stdout: {stdout}");
    assert!(stdout.contains("4194304 bytes"), "stdout: {stdout}");

    let downloaded = std::fs::read(&out_file).unwrap();
    assert_matches_bench_pattern(&downloaded, srv.size_bytes as usize);
}

#[test]
fn ea7_download_debug_and_stats_out_json_validation() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("tracked.bin");
    let stats_file = dir.path().join("custom.stats.json");
    let srv = ServeGuard::spawn(&["--size", "1M", "--port", "0"]);

    let out = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "--debug",
            "--stats-out",
            stats_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(
        out.status.success(),
        "download failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Stats saved:"), "stdout: {stdout}");

    let stats_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats_json["status"], "completed");
    assert!(stats_json["error"].is_null());
    assert_eq!(stats_json["bytes_downloaded"], 1024 * 1024);
    assert_eq!(stats_json["content_length"], 1024 * 1024);
    assert_eq!(stats_json["supports_ranges"], true);
    assert!(stats_json["workers_used"].as_u64().unwrap() >= 1);
    assert!(stats_json["total_ms"].as_f64().unwrap() >= 0.0);
    assert!(!stats_json["samples"].as_array().unwrap().is_empty());

    // Also verify `--debug` without `--stats-out` writes `<final_path>.stats.json`.
    let out2_file = dir.path().join("auto_stats.bin");
    let out2 = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out2_file.to_str().unwrap(),
            "--debug",
            &srv.url,
        ],
        None,
    );
    assert!(out2.status.success());
    let default_stats = dir.path().join("auto_stats.bin.stats.json");
    assert!(default_stats.is_file(), "expected {}", default_stats.display());
}

#[test]
fn ea7_list_empty_and_populated_table_format() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let empty_list = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    assert!(empty_list.status.success());
    let empty_stdout = String::from_utf8_lossy(&empty_list.stdout);
    assert!(
        empty_stdout.contains("No downloads recorded in"),
        "stdout: {empty_stdout}"
    );

    let srv = ServeGuard::spawn(&["--size", "64K", "--port", "0"]);
    let out_file = dir.path().join("listed.bin");
    let dl = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(dl.status.success());
    let id = extract_download_id(&String::from_utf8_lossy(&dl.stdout));

    let pop_list = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    assert!(pop_list.status.success());
    let pop_stdout = String::from_utf8_lossy(&pop_list.stdout);
    assert!(pop_stdout.contains("ID"), "stdout: {pop_stdout}");
    assert!(pop_stdout.contains("STATE"), "stdout: {pop_stdout}");
    assert!(pop_stdout.contains("PROGRESS"), "stdout: {pop_stdout}");
    assert!(pop_stdout.contains("TARGET / URL"), "stdout: {pop_stdout}");
    assert!(pop_stdout.contains(&id), "stdout: {pop_stdout}");
    assert!(pop_stdout.contains("Completed"), "stdout: {pop_stdout}");
    assert!(pop_stdout.contains("listed.bin"), "stdout: {pop_stdout}");
}

#[tokio::test]
async fn ea7_resume_paused_download_from_db() {
    let payload: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 241) as u8).collect();
    let throttle = Arc::new(AtomicBool::new(true));
    let base_url = spawn_mock_http_server(Arc::new(payload.clone()), Arc::clone(&throttle), false).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("resumed.bin");

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let (id, part_path) =
        seed_paused_download(&engine, &format!("{base_url}/resumed.bin"), &out_file).await;
    assert!(part_path.exists());
    drop(engine);

    throttle.store(false, Ordering::Relaxed);
    let res = run_cli_async(
        &["resume", id.as_str(), "--db", db_path.to_str().unwrap()],
        None,
    )
    .await;
    assert!(
        res.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    assert_eq!(std::fs::read(&out_file).unwrap(), payload);
    assert!(!part_path.exists());
}

#[tokio::test]
async fn ea7_cancel_persisted_download_removes_part_file() {
    let payload: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 199) as u8).collect();
    let throttle = Arc::new(AtomicBool::new(true));
    let base_url = spawn_mock_http_server(Arc::new(payload), Arc::clone(&throttle), false).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("to_cancel.bin");

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let (id, part_path) =
        seed_paused_download(&engine, &format!("{base_url}/to_cancel.bin"), &out_file).await;
    assert!(part_path.exists());
    drop(engine);

    let cancel = run_cli_async(
        &["cancel", id.as_str(), "--db", db_path.to_str().unwrap()],
        None,
    )
    .await;
    assert!(cancel.status.success());
    let stdout = String::from_utf8_lossy(&cancel.stdout);
    assert!(stdout.contains(&format!("Cancelled download {id}")));
    assert!(!part_path.exists());
    assert!(!out_file.exists());
}

#[test]
fn ea7_delete_preserves_or_removes_file_with_flag() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let srv = ServeGuard::spawn(&["--size", "64K", "--port", "0"]);

    let keep_file = dir.path().join("keep.bin");
    let dl1 = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            keep_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(dl1.status.success());
    let id1 = extract_download_id(&String::from_utf8_lossy(&dl1.stdout));

    let rm_file = dir.path().join("remove.bin");
    let dl2 = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            rm_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(dl2.status.success());
    let id2 = extract_download_id(&String::from_utf8_lossy(&dl2.stdout));

    let del1 = run_cli(
        &["delete", &id1, "--db", db_path.to_str().unwrap()],
        None,
    );
    assert!(del1.status.success());
    assert!(String::from_utf8_lossy(&del1.stdout).contains(&format!("Deleted download {id1}")));
    assert!(keep_file.is_file(), "file should be preserved without --delete-file");

    let del2 = run_cli(
        &[
            "delete",
            &id2,
            "--delete-file",
            "--db",
            db_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(del2.status.success());
    assert!(!rm_file.exists(), "file should be removed with --delete-file");

    let list = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(!list_stdout.contains(&id1));
    assert!(!list_stdout.contains(&id2));
}

// ============================================================================
// EA8: serve, bench, native-host
// ============================================================================

async fn raw_http_request(addr: &str, request: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("missing HTTP header terminator");
    let headers = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = raw[split + 4..].to_vec();
    (headers, body)
}

#[tokio::test]
async fn ea8_serve_ephemeral_port_handles_head_get_and_range() {
    let srv = ServeGuard::spawn(&["--size", "256K", "--port", "0"]);
    assert_eq!(srv.size_bytes, 256 * 1024);

    let (head_hdrs, head_body) = raw_http_request(
        &srv.addr,
        "HEAD /bench.bin HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head_hdrs.starts_with("HTTP/1.1 200 OK"), "{head_hdrs}");
    assert!(head_hdrs.contains("Content-Length: 262144"), "{head_hdrs}");
    assert!(head_hdrs.contains("Accept-Ranges: bytes"), "{head_hdrs}");
    assert!(head_body.is_empty());

    let (range_hdrs, range_body) = raw_http_request(
        &srv.addr,
        "GET /bench.bin HTTP/1.1\r\nHost: localhost\r\nRange: bytes=1024-4095\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        range_hdrs.starts_with("HTTP/1.1 206 Partial Content"),
        "{range_hdrs}"
    );
    assert!(
        range_hdrs.contains("Content-Range: bytes 1024-4095/262144"),
        "{range_hdrs}"
    );
    assert_eq!(range_body.len(), 3072);
    for (i, &b) in range_body.iter().enumerate() {
        assert_eq!(b, e2e_common::expected_bench_byte(1024 + i));
    }

    let (full_hdrs, full_body) = raw_http_request(
        &srv.addr,
        "GET /bench.bin HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(full_hdrs.starts_with("HTTP/1.1 200 OK"), "{full_hdrs}");
    assert_matches_bench_pattern(&full_body, 256 * 1024);
}

#[test]
fn ea8_bench_no_external_json_report_validation() {
    let dir = TempDir::new().unwrap();
    let report = dir.path().join("bench_out.json");

    let out = run_cli(
        &[
            "bench",
            "--no-external",
            "--sizes",
            "1M",
            "-c",
            "1,2",
            "--runs",
            "1",
            "--report",
            report.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        out.status.success(),
        "bench failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Report saved to"), "stdout: {stdout}");

    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["scenario"], "custom");
    assert_eq!(rows[0]["tool"], "posthaze");
    assert_eq!(rows[0]["size_bytes"], 1024 * 1024);
    assert_eq!(rows[0]["connections"], 1);
    assert_eq!(rows[0]["runs"], 1);
    assert!(rows[0]["median_mbps"].as_f64().unwrap() > 0.0);
    assert_eq!(rows[1]["connections"], 2);
    assert!(rows[1]["median_transfer_s"].as_f64().unwrap() > 0.0);
}

#[test]
fn ea8_bench_markdown_report_writes_table() {
    let dir = TempDir::new().unwrap();
    let report = dir.path().join("bench_out.md");

    let out = run_cli(
        &[
            "bench",
            "--no-external",
            "--sizes",
            "512K",
            "-c",
            "1",
            "--runs",
            "1",
            "--report",
            report.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        out.status.success(),
        "bench failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let md = std::fs::read_to_string(&report).unwrap();
    assert!(md.contains("| Scenario | Tool | Size (MiB) | Connections | Runs |"));
    assert!(md.contains("| custom | posthaze | 0.5 | 1 | 1 |"), "md: {md}");
}

#[test]
fn ea8_native_host_ping_4byte_le_framing() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let (status, reply, _stderr) = send_native_host_json(&db_path, &json!({"type": "ping"}));
    assert!(status.success());
    assert_eq!(reply["ok"], true, "unexpected ping reply: {reply}");
    let mode = reply["mode"].as_str().unwrap_or("");
    assert!(
        mode == "host-only" || mode == "pong",
        "unexpected ping mode: {reply}"
    );
}

#[tokio::test]
async fn ea8_native_host_enqueue_4byte_le_framing() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    configure_db_output_dir(&db_path, dir.path()).await;

    let (status, reply, _stderr) = send_native_host_json(
        &db_path,
        &json!({
            "type": "enqueue",
            "url": "http://127.0.0.1:9999/archive.bin",
            "filename": "archive.bin",
            "headers": [["Referer", "http://127.0.0.1:9999/"]]
        }),
    );
    assert!(status.success());
    assert_eq!(reply["ok"], true, "unexpected enqueue reply: {reply}");

    let mode = reply["mode"].as_str().unwrap_or("");
    assert!(
        mode == "queued-headless" || mode == "preflight",
        "unexpected enqueue mode: {reply}"
    );
    if mode == "queued-headless" {
        let engine = DownloadEngine::open(&db_path).await.unwrap();
        let rows = engine.list_downloads().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, DownloadState::Paused);
        assert_eq!(rows[0].url, "http://127.0.0.1:9999/archive.bin");
        assert_eq!(
            rows[0].output_path.as_deref(),
            Some(dir.path().join("archive.bin").as_path())
        );
    }
}
