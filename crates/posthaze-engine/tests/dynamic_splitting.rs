mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use common::{parse_byte_range, spawn_raw_server};
use posthaze_engine::{DownloadEngine, DownloadRequest};

#[tokio::test]
async fn steals_work_from_throttled_worker_and_verifies_blake3() {
    // 12 MiB with 2 Workers -> initial segments are 6 MiB each.
    // Worker 0 is throttled (~1 MiB/s -> ~6s alone), Worker 1 finishes immediately and steals.
    let payload: Vec<u8> = (0..12 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload = Arc::new(payload);
    let range_requests = Arc::new(AtomicUsize::new(0));
    let range_requests_srv = Arc::clone(&range_requests);

    let base_url = spawn_raw_server(move |method, path, range_header, mut write_half| {
        let payload = Arc::clone(&payload);
        let range_requests = Arc::clone(&range_requests_srv);
        async move {
            if path != "/throttled.bin" {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                return;
            }

            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let req_idx = range_requests.fetch_add(1, Ordering::SeqCst);
            let (start, end) = parse_byte_range(
                range_header.as_deref().unwrap_or("bytes=0-"),
                payload.len(),
            );
            let slice = &payload[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload.len()
            );
            if write_half.write_all(hdr.as_bytes()).await.is_err() {
                return;
            }

            if req_idx == 0 {
                for chunk in slice.chunks(64 * 1024) {
                    if write_half.write_all(chunk).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(60)).await;
                }
            } else {
                let _ = write_half.write_all(slice).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/throttled.bin"), temp_dir.path());
    req.max_workers = 2;

    let start = Instant::now();
    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("download should succeed with work stealing");
    let elapsed = start.elapsed();

    assert!(
        range_requests.load(Ordering::SeqCst) > 2,
        "expected more than 2 range requests due to dynamic splitting, got {}",
        range_requests.load(Ordering::SeqCst)
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "expected work stealing to finish well under 3s, took {elapsed:?}"
    );
    assert_eq!(outcome.bytes_downloaded, 12 * 1024 * 1024);

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}
