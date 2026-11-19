mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use common::parse_byte_range;
use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState};

/// Spawns a range server that serves the first 9 MiB quickly, then stalls subsequent
/// reads when `stall_tail` is true so we can observe an 8 MiB periodic `Checkpoint`
/// and simulate an ungraceful process crash.
async fn spawn_stall_after_9mib_server(
    payload: Arc<Vec<u8>>,
    stall_tail: Arc<AtomicBool>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let stall_tail = Arc::clone(&stall_tail);

            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = req_str.lines().next().unwrap_or("");
                let method = first_line.split_whitespace().next().unwrap_or("");
                let range_hdr = req_str.lines().find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("range") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });

                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"crash-v1\"\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                if method == "GET" {
                    let (start, end) = match range_hdr.as_deref() {
                        Some(r) => parse_byte_range(r, payload.len()),
                        None => (0, payload.len().saturating_sub(1)),
                    };
                    let slice = &payload[start..=end];
                    let header = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"crash-v1\"\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        payload.len()
                    );
                    if stream.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }

                    let stall_point = 9 * 1024 * 1024usize;
                    let mut pos = 0usize;
                    for chunk in slice.chunks(256 * 1024) {
                        if stall_tail.load(Ordering::Relaxed) && start + pos >= stall_point {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            return;
                        }
                        if stream.write_all(chunk).await.is_err() {
                            return;
                        }
                        pos += chunk.len();
                    }
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn periodic_checkpoint_and_startup_crash_reconciliation_recovers_download() {
    // 10 MiB payload with 1 Worker so we deterministically cross the 8 MiB checkpoint threshold.
    let payload: Vec<u8> = (0..10 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let stall_tail = Arc::new(AtomicBool::new(true));

    let base_url =
        spawn_stall_after_9mib_server(Arc::new(payload), Arc::clone(&stall_tail)).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/crash.bin"), dir.path());
    req.max_workers = 1;
    let handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();

    // Poll get_download until the 8 MiB writer-acked Checkpoint lands in SQLite.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(rec) = engine.get_download(&id).await.unwrap()
                && rec.state == DownloadState::Active
                && rec.bytes_downloaded >= 8 * 1024 * 1024
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("periodic 8 MiB checkpoint should persist bytes_downloaded while Active");

    // Simulate ungraceful termination by dropping handle & engine without pause().
    drop(handle);
    drop(engine);

    // Disable stall and reopen DB: startup crash reconciliation must mark Active -> Paused.
    stall_tail.store(false, Ordering::Relaxed);
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let reconciled = reopened.get_download(&id).await.unwrap().unwrap();
    assert_eq!(reconciled.state, DownloadState::Paused);
    assert!(reconciled.bytes_downloaded >= 8 * 1024 * 1024);

    // Resume from the reconciled checkpoint and verify BLAKE3.
    let mut resumed = reopened.resume(&id).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn startup_reconciles_probing_and_non_range_active_to_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = req_str.lines().next().unwrap_or("");
                let mut parts = first_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("");

                if path == "/stall-head" {
                    // Never respond to HEAD so the download stays in Probing.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return;
                }

                if method == "HEAD" && path == "/no-range" {
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }

                if method == "GET" && path == "/no-range" {
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\nabc")
                        .await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            });
        }
    });
    let base_url = format!("http://{addr}");

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let probing_handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/stall-head"),
            dir.path(),
        ))
        .await
        .unwrap();
    let probing_id = probing_handle.id().clone();

    let non_range_handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/no-range"),
            dir.path(),
        ))
        .await
        .unwrap();
    let non_range_id = non_range_handle.id().clone();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = engine.get_download(&probing_id).await.unwrap().unwrap();
            let nr = engine.get_download(&non_range_id).await.unwrap().unwrap();
            if p.state == DownloadState::Probing && nr.state == DownloadState::Active {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    drop(probing_handle);
    drop(non_range_handle);
    drop(engine);

    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(
        reopened.get_download(&probing_id).await.unwrap().unwrap().state,
        DownloadState::Failed
    );
    assert_eq!(
        reopened.get_download(&non_range_id).await.unwrap().unwrap().state,
        DownloadState::Failed
    );
}
