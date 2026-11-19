use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;

async fn spawn_range_http_server(payload: Arc<Vec<u8>>, throttle: Arc<AtomicBool>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let throttle = Arc::clone(&throttle);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let method = req
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                let range_hdr = req.lines().find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("range") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });

                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"cli-v1\"\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                } else if method == "GET" {
                    if let Some(r) = range_hdr {
                        let r = r.strip_prefix("bytes=").unwrap_or(&r);
                        let (s, e) = r.split_once('-').unwrap();
                        let start: usize = s.parse().unwrap();
                        let end: usize = e.parse::<usize>().unwrap().min(payload.len() - 1);
                        let slice = &payload[start..=end];
                        let header = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"cli-v1\"\r\nConnection: close\r\n\r\n",
                            slice.len(),
                            payload.len()
                        );
                        if stream.write_all(header.as_bytes()).await.is_err() {
                            return;
                        }
                        let slow = throttle.load(Ordering::Relaxed);
                        for chunk in slice.chunks(64 * 1024) {
                            if stream.write_all(chunk).await.is_err() {
                                return;
                            }
                            if slow {
                                tokio::time::sleep(Duration::from_millis(35)).await;
                            }
                        }
                    } else {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"cli-v1\"\r\nConnection: close\r\n\r\n",
                            payload.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(&payload).await;
                    }
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn cli_download_and_list_persist_to_custom_db() {
    let payload = b"posthaze cli persistence test payload".to_vec();
    let base_url =
        spawn_range_http_server(Arc::new(payload.clone()), Arc::new(AtomicBool::new(false))).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("nested").join("state.db");
    let out_path = dir.path().join("out.bin");
    let bin = env!("CARGO_BIN_EXE_posthaze-cli");

    let dl = Command::new(bin)
        .args([
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            &format!("{base_url}/out.bin"),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        dl.status.success(),
        "download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );
    assert_eq!(std::fs::read(&out_path).unwrap(), payload);

    let list = Command::new(bin)
        .args(["list", "--db", db_path.to_str().unwrap()])
        .output()
        .await
        .unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("Completed"), "stdout was: {stdout}");
    assert!(stdout.contains("out.bin"), "stdout was: {stdout}");
}

async fn seed_paused_download(
    engine: &DownloadEngine,
    url: &str,
    out_path: &std::path::Path,
) -> posthaze_engine::DownloadId {
    let mut req = DownloadRequest::new(url, out_path.parent().unwrap());
    req.output_path = Some(out_path.to_path_buf());
    req.max_workers = 2;

    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if rx.borrow().state == DownloadState::Active && rx.borrow().bytes_downloaded > 0 {
                break;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    handle.pause();
    let _ = handle.wait().await;
    id
}

#[tokio::test]
async fn cli_resume_and_cancel_subcommands_work_end_to_end() {
    let payload: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let throttle = Arc::new(AtomicBool::new(true));
    let base_url = spawn_range_http_server(Arc::new(payload.clone()), Arc::clone(&throttle)).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let resume_out = dir.path().join("resume.bin");
    let cancel_out = dir.path().join("cancel.bin");

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let resume_id = seed_paused_download(&engine, &format!("{base_url}/r.bin"), &resume_out).await;
    let cancel_id = seed_paused_download(&engine, &format!("{base_url}/c.bin"), &cancel_out).await;
    let cancel_part = engine
        .get_download(&cancel_id)
        .await
        .unwrap()
        .unwrap()
        .part_path
        .unwrap();
    assert!(cancel_part.exists());
    drop(engine);

    throttle.store(false, Ordering::Relaxed);
    let bin = env!("CARGO_BIN_EXE_posthaze-cli");

    let resume_out_cmd = Command::new(bin)
        .args([
            "resume",
            resume_id.as_str(),
            "--db",
            db_path.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        resume_out_cmd.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&resume_out_cmd.stderr)
    );
    assert_eq!(std::fs::read(&resume_out).unwrap(), payload);

    let cancel_cmd = Command::new(bin)
        .args([
            "cancel",
            cancel_id.as_str(),
            "--db",
            db_path.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        cancel_cmd.status.success(),
        "cancel failed: {}",
        String::from_utf8_lossy(&cancel_cmd.stderr)
    );
    assert!(!cancel_part.exists(), ".part file should be deleted");

    let list_cmd = Command::new(bin)
        .args(["list", "--db", db_path.to_str().unwrap()])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&list_cmd.stdout);
    assert!(stdout.contains("Completed"), "stdout: {stdout}");
    assert!(stdout.contains("Cancelled"), "stdout: {stdout}");

    // Delete the cancelled record (without --delete-file) and completed record (with --delete-file).
    let del_cancelled = Command::new(bin)
        .args([
            "delete",
            cancel_id.as_str(),
            "--db",
            db_path.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        del_cancelled.status.success(),
        "delete cancelled failed: {}",
        String::from_utf8_lossy(&del_cancelled.stderr)
    );

    let del_completed = Command::new(bin)
        .args([
            "delete",
            resume_id.as_str(),
            "--delete-file",
            "--db",
            db_path.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        del_completed.status.success(),
        "delete completed failed: {}",
        String::from_utf8_lossy(&del_completed.stderr)
    );
    assert!(!resume_out.exists(), "completed file should be removed");

    let list_after_del = Command::new(bin)
        .args(["list", "--db", db_path.to_str().unwrap()])
        .output()
        .await
        .unwrap();
    let stdout_after = String::from_utf8_lossy(&list_after_del.stdout);
    assert!(!stdout_after.contains(cancel_id.as_str()));
    assert!(!stdout_after.contains(resume_id.as_str()));
}
