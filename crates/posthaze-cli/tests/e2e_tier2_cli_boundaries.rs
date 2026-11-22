mod e2e_common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::json;
use tempfile::TempDir;

use e2e_common::{
    ServeGuard, assert_matches_bench_pattern, encode_native_frame, run_cli, run_cli_async,
    run_native_host_raw, send_native_host_json, spawn_mock_http_server,
};

// ============================================================================
// EA7 Boundaries: download, resume, cancel, delete, DB errors
// ============================================================================

#[tokio::test]
async fn ea7_boundary_download_http_404_exits_1_and_writes_failed_stats_json() {
    let base_url =
        spawn_mock_http_server(Arc::new(Vec::new()), Arc::new(AtomicBool::new(false)), true).await;
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("missing.bin");
    let stats_file = dir.path().join("failed.stats.json");

    let out = run_cli_async(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "--stats-out",
            stats_file.to_str().unwrap(),
            &format!("{base_url}/missing.bin"),
        ],
        None,
    )
    .await;
    assert_eq!(out.status.code(), Some(1));
    assert!(!out_file.exists(), "output file should not exist on 404");

    let stats: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats["status"], "failed");
    assert!(stats["error"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(stats["final_path"].is_null());
}

#[test]
fn ea7_boundary_download_connections_zero_clamps_and_completes() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("clamped.bin");
    let stats_file = dir.path().join("clamped.stats.json");
    let srv = ServeGuard::spawn(&["--size", "256K", "--port", "0"]);

    let out = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "-c",
            "0",
            "--stats-out",
            stats_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(
        out.status.success(),
        "download -c 0 failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_matches_bench_pattern(&std::fs::read(&out_file).unwrap(), 256 * 1024);

    let stats: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats["status"], "completed");
    assert_eq!(stats["workers_used"], 1);
}

#[test]
fn ea7_boundary_resume_nonexistent_id_exits_1() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let out = run_cli(
        &[
            "resume",
            "00000000-0000-0000-0000-000000000000",
            "--db",
            db_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Failed to resume download"), "stderr: {stderr}");
}

#[test]
fn ea7_boundary_cancel_nonexistent_id_exits_1() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let out = run_cli(
        &[
            "cancel",
            "00000000-0000-0000-0000-000000000000",
            "--db",
            db_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Failed to cancel download"), "stderr: {stderr}");
}

#[test]
fn ea7_boundary_delete_nonexistent_id_exits_1() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let out = run_cli(
        &[
            "delete",
            "00000000-0000-0000-0000-000000000000",
            "--db",
            db_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Failed to delete download"), "stderr: {stderr}");
}

#[test]
fn ea7_boundary_download_and_list_with_invalid_db_path_exit_1() {
    let dir = TempDir::new().unwrap();
    // Create a regular file where a parent directory is expected so SQLite cannot open/create DB.
    let blocking_file = dir.path().join("not_a_directory");
    std::fs::write(&blocking_file, b"occupied").unwrap();
    let bad_db = blocking_file.join("state.db");

    let dl = run_cli(
        &[
            "download",
            "--db",
            bad_db.to_str().unwrap(),
            "http://127.0.0.1:9/file.bin",
        ],
        Some(dir.path()),
    );
    assert_eq!(dl.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&dl.stderr).contains("Failed to open database"),
        "stderr: {}",
        String::from_utf8_lossy(&dl.stderr)
    );

    let list = run_cli(&["list", "--db", bad_db.to_str().unwrap()], None);
    assert_eq!(list.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&list.stderr).contains("Failed to open database"),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
}

// ============================================================================
// EA8 Boundaries: serve, bench, native-host framing & URL validation
// ============================================================================

#[test]
fn ea8_boundary_serve_invalid_size_exits_1() {
    let out = run_cli(&["serve", "--size", "not-a-size"], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Invalid --size"), "stderr: {stderr}");
}

#[test]
fn ea8_boundary_bench_unknown_tool_exits_1() {
    let out = run_cli(&["bench", "--tools", "bogus"], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Unknown tool 'bogus'"), "stderr: {stderr}");
}

#[test]
fn ea8_boundary_bench_invalid_size_exits_1() {
    let out = run_cli(&["bench", "--no-external", "--sizes", "bad"], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Invalid --size 'bad'"), "stderr: {stderr}");
}

#[test]
fn ea8_boundary_native_host_zero_byte_frame_length_exits_1() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let (status, stdout, _stderr) = run_native_host_raw(&db_path, &0_u32.to_le_bytes());
    assert_eq!(status.code(), Some(1));
    let reply = e2e_common::decode_native_frame(&stdout).expect("expected error reply frame");
    assert_eq!(reply["ok"], false);
    assert!(
        reply["error"]
            .as_str()
            .is_some_and(|e| e.contains("invalid native messaging frame length: 0")),
        "reply: {reply}"
    );
}

#[test]
fn ea8_boundary_native_host_oversized_frame_length_exits_1() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let oversized_len = (1024_u32 * 1024_u32) + 1;

    let (status, stdout, _stderr) = run_native_host_raw(&db_path, &oversized_len.to_le_bytes());
    assert_eq!(status.code(), Some(1));
    let reply = e2e_common::decode_native_frame(&stdout).expect("expected error reply frame");
    assert_eq!(reply["ok"], false);
    assert!(
        reply["error"]
            .as_str()
            .is_some_and(|e| e.contains("invalid native messaging frame length")),
        "reply: {reply}"
    );
}

#[test]
fn ea8_boundary_native_host_clean_eof_exits_0_with_empty_stdout() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let (status, stdout, _stderr) = run_native_host_raw(&db_path, &[]);
    assert!(status.success());
    assert!(stdout.is_empty(), "expected empty stdout on EOF, got {} bytes", stdout.len());
}

#[test]
fn ea8_boundary_native_host_non_http_url_and_invalid_json_exit_1() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let (status, reply, _stderr) = send_native_host_json(
        &db_path,
        &json!({
            "type": "enqueue",
            "url": "ftp://example.com/file.bin"
        }),
    );
    assert_eq!(status.code(), Some(1));
    assert_eq!(reply["ok"], false);
    assert!(
        reply["error"]
            .as_str()
            .is_some_and(|e| e.contains("Only http:// and https:// URLs are supported")),
        "reply: {reply}"
    );

    let bad_json_frame = encode_native_frame(b"{not valid json");
    let (status2, stdout2, _) = run_native_host_raw(&db_path, &bad_json_frame);
    assert_eq!(status2.code(), Some(1));
    let reply2 = e2e_common::decode_native_frame(&stdout2).unwrap();
    assert_eq!(reply2["ok"], false);
    assert!(
        reply2["error"]
            .as_str()
            .is_some_and(|e| e.contains("invalid JSON request")),
        "reply2: {reply2}"
    );
}
