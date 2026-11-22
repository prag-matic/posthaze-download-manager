mod e2e_common;

use serde_json::json;
use tempfile::TempDir;

use e2e_common::{
    ServeGuard, assert_matches_bench_pattern, configure_db_output_dir, extract_download_id,
    run_cli, send_native_host_json,
};

#[test]
fn tier4_scenario_full_cli_operator_lifecycle() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("operator.db");
    let out_file = dir.path().join("artifact.bin");
    let stats_file = dir.path().join("artifact.stats.json");

    let srv = ServeGuard::spawn(&["--size", "3M", "--port", "0"]);

    let dl = run_cli(
        &[
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "-c",
            "4",
            "--debug",
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
    let id = extract_download_id(&String::from_utf8_lossy(&dl.stdout));

    assert_matches_bench_pattern(&std::fs::read(&out_file).unwrap(), 3 * 1024 * 1024);

    let list = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    assert!(list.status.success());
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list_stdout.contains(&id), "list missing id: {list_stdout}");
    assert!(list_stdout.contains("Completed"), "list missing Completed: {list_stdout}");

    let stats: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats["status"], "completed");
    assert_eq!(stats["bytes_downloaded"], 3 * 1024 * 1024);
    assert_eq!(stats["supports_ranges"], true);
    let samples = stats["samples"].as_array().expect("samples must be array");
    assert!(!samples.is_empty(), "samples should not be empty");
    assert!(samples.last().unwrap()["bytes_downloaded"].is_u64());

    let del = run_cli(
        &["delete", &id, "--delete-file", "--db", db_path.to_str().unwrap()],
        None,
    );
    assert!(del.status.success());
    assert!(!out_file.exists(), "output file should be removed");

    let list_after = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    let after_stdout = String::from_utf8_lossy(&list_after.stdout);
    assert!(after_stdout.contains("No downloads recorded"), "after: {after_stdout}");
    assert!(!after_stdout.contains(&id));
}

#[tokio::test]
async fn tier4_scenario_multi_file_cli_and_native_host_batch_workflow() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("batch.db");
    configure_db_output_dir(&db_path, dir.path()).await;

    let srv = ServeGuard::spawn(&["--size", "512K", "--port", "0"]);
    let filenames = ["batch_1.bin", "batch_2.bin", "batch_3.bin"];

    for name in filenames {
        let (status, reply, _) = send_native_host_json(
            &db_path,
            &json!({
                "type": "enqueue",
                "url": srv.url,
                "filename": name
            }),
        );
        assert!(status.success());
        assert_eq!(reply["ok"], true);
        if reply["mode"] == "preflight" {
            // Skip headless DB batch assertions if an external user UI owns the pipe.
            return;
        }
        assert_eq!(reply["mode"], "queued-headless");
    }

    let list = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    assert!(list.status.success());
    let list_stdout = String::from_utf8_lossy(&list.stdout);

    let mut ids = Vec::new();
    for name in filenames {
        let id = list_stdout
            .lines()
            .find(|l| l.contains("Paused") && l.contains(name))
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or_else(|| panic!("missing paused item {name} in:\n{list_stdout}"))
            .to_string();
        ids.push(id);
    }

    // Resume first two to byte-exact completion.
    for (idx, name) in filenames[..2].iter().enumerate() {
        let res = run_cli(&["resume", &ids[idx], "--db", db_path.to_str().unwrap()], None);
        assert!(
            res.status.success(),
            "resume {name} failed: {}",
            String::from_utf8_lossy(&res.stderr)
        );
        assert_matches_bench_pattern(&std::fs::read(dir.path().join(name)).unwrap(), 512 * 1024);
    }

    // Cancel and delete the third item cleanly.
    let cancel = run_cli(&["cancel", &ids[2], "--db", db_path.to_str().unwrap()], None);
    assert!(cancel.status.success());
    let del3 = run_cli(&["delete", &ids[2], "--db", db_path.to_str().unwrap()], None);
    assert!(del3.status.success());

    let list_mid = run_cli(&["list", "--db", db_path.to_str().unwrap()], None);
    let mid_stdout = String::from_utf8_lossy(&list_mid.stdout);
    assert!(mid_stdout.contains(&ids[0]));
    assert!(mid_stdout.contains(&ids[1]));
    assert!(!mid_stdout.contains(&ids[2]));
    assert!(!mid_stdout.contains("Paused"));

    // Clean up completed items and files.
    for idx in 0..2 {
        let del = run_cli(
            &[
                "delete",
                &ids[idx],
                "--delete-file",
                "--db",
                db_path.to_str().unwrap(),
            ],
            None,
        );
        assert!(del.status.success());
        assert!(!dir.path().join(filenames[idx]).exists());
    }
}

#[test]
fn tier4_scenario_loopback_fault_injection_and_flaky_benchmark_regression() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("flaky.db");
    let out_file = dir.path().join("fault_checked.bin");
    let stats_file = dir.path().join("fault_checked.stats.json");
    let bench_report = dir.path().join("flaky_bench.json");

    let srv = ServeGuard::spawn(&[
        "--size",
        "2M",
        "--per-conn-mbps",
        "40",
        "--drop-pct",
        "15",
        "--error-503-pct",
        "15",
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
        "fault-injected download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );
    assert_matches_bench_pattern(&std::fs::read(&out_file).unwrap(), 2 * 1024 * 1024);

    let stats: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats_file).unwrap()).unwrap();
    assert_eq!(stats["status"], "completed");
    assert_eq!(stats["bytes_downloaded"], 2 * 1024 * 1024);

    let bench = run_cli(
        &[
            "bench",
            "--scenarios",
            "flaky",
            "--no-external",
            "--sizes",
            "1M",
            "-c",
            "4",
            "--runs",
            "1",
            "--report",
            bench_report.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        bench.status.success(),
        "flaky bench failed: {}",
        String::from_utf8_lossy(&bench.stderr)
    );

    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&bench_report).unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["scenario"], "flaky");
    assert_eq!(rows[0]["tool"], "posthaze");
    assert_eq!(rows[0]["size_bytes"], 1024 * 1024);
    assert_eq!(rows[0]["connections"], 4);
    assert!(rows[0]["median_mbps"].as_f64().unwrap() > 0.0);
}
