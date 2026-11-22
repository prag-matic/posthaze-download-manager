mod e2e_common;

use serde_json::json;
use tempfile::TempDir;

use e2e_common::{
    ServeGuard, assert_matches_bench_pattern, configure_db_output_dir, run_cli,
    send_native_host_json,
};

#[test]
fn tier3_serve_no_range_with_download_c8_completes_single_stream_byte_exact() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("no_range.bin");
    let stats_file = dir.path().join("no_range.stats.json");

    let srv = ServeGuard::spawn(&["--size", "2M", "--no-range", "--port", "0"]);
    let dl = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "-c",
            "8",
            "--stats-out",
            stats_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(
        dl.status.success(),
        "download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );
    assert_matches_bench_pattern(&std::fs::read(&out_file).unwrap(), 2 * 1024 * 1024);

    let stats: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats["status"], "completed");
    assert_eq!(stats["supports_ranges"], false);
    assert_eq!(stats["workers_used"], 1);
}

#[test]
fn tier3_serve_fault_injection_503_and_drops_with_download_stats_out() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let out_file = dir.path().join("flaky.bin");
    let stats_file = dir.path().join("flaky.stats.json");

    let srv = ServeGuard::spawn(&[
        "--size",
        "2M",
        "--error-503-pct",
        "20",
        "--drop-pct",
        "10",
        "--port",
        "0",
    ]);
    let dl = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "-c",
            "4",
            "--stats-out",
            stats_file.to_str().unwrap(),
            &srv.url,
        ],
        None,
    );
    assert!(
        dl.status.success(),
        "flaky download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );
    assert_matches_bench_pattern(&std::fs::read(&out_file).unwrap(), 2 * 1024 * 1024);

    let stats: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats["status"], "completed");
    assert_eq!(stats["bytes_downloaded"], 2 * 1024 * 1024);
    assert!(stats["retries_performed"].is_u64());
}

#[tokio::test]
async fn tier3_native_host_enqueue_list_resume_delete_roundtrip() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    configure_db_output_dir(&db_path, dir.path()).await;

    let srv = ServeGuard::spawn(&["--size", "512K", "--port", "0"]);
    let (nh_status, reply, _) = send_native_host_json(
        &db_path,
        &json!({
            "type": "enqueue",
            "url": srv.url,
            "filename": "nh_roundtrip.bin"
        }),
    );
    assert!(nh_status.success());
    assert_eq!(reply["ok"], true);
    if reply["mode"] == "preflight" {
        // A desktop posthaze-ui instance intercepted the global Windows named pipe.
        return;
    }
    assert_eq!(reply["mode"], "queued-headless");

    let list_before = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    assert!(list_before.status.success());
    let list_stdout = String::from_utf8_lossy(&list_before.stdout);
    let id = list_stdout
        .lines()
        .find(|l| l.contains("Paused") && l.contains("nh_roundtrip.bin"))
        .and_then(|l| l.split_whitespace().next())
        .expect("paused enqueued download not found in list output")
        .to_string();

    let res = run_cli(&["resume", &id, "--db", db_path.to_str().unwrap()], None);
    assert!(
        res.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );

    let target = dir.path().join("nh_roundtrip.bin");
    assert_matches_bench_pattern(&std::fs::read(&target).unwrap(), 512 * 1024);

    let del = run_cli(
        &["delete", &id, "--delete-file", "--db", db_path.to_str().unwrap()],
        None,
    );
    assert!(del.status.success());
    assert!(!target.exists());

    let list_after = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    assert!(
        String::from_utf8_lossy(&list_after.stdout).contains("No downloads recorded"),
        "stdout: {}",
        String::from_utf8_lossy(&list_after.stdout)
    );
}

#[test]
fn tier3_bench_compare_db_and_compare_sync_json_rows() {
    let dir = TempDir::new().unwrap();
    let report = dir.path().join("bench_compare.json");

    let out = run_cli(
        &[
            "bench",
            "--no-external",
            "--compare-db",
            "--compare-sync",
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

    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(rows.len(), 6, "expected 3 tools x 2 connection counts: {rows:?}");

    for c in [1_u64, 2_u64] {
        for tool in ["posthaze", "posthaze (db)", "posthaze (sync)"] {
            let row = rows
                .iter()
                .find(|r| r["tool"] == tool && r["connections"] == c && r["size_bytes"] == 1024 * 1024)
                .unwrap_or_else(|| panic!("missing row for tool={tool}, c={c} in {rows:?}"));
            assert_eq!(row["scenario"], "custom", "unexpected scenario in {row}");
            assert_eq!(row["runs"], 1, "unexpected runs in {row}");
            assert!(
                row["median_mbps"].as_f64().is_some_and(|v| v > 0.0),
                "expected median_mbps > 0.0 in {row}"
            );
            assert!(
                row["median_transfer_s"].as_f64().is_some_and(|v| v > 0.0),
                "expected median_transfer_s > 0.0 in {row}"
            );
        }
    }
}
