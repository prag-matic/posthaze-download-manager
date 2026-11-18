use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use posthaze_engine::{DownloadEngine, DownloadRequest};

async fn spawn_throttled_range_server(
    payload: Arc<Vec<u8>>,
    throttle_first_request: bool,
    range_requests: Arc<AtomicUsize>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let range_requests = Arc::clone(&range_requests);

            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
                    return;
                }

                let mut range_header = None;
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.trim().eq_ignore_ascii_case("range")
                    {
                        range_header = Some(value.trim().to_string());
                    }
                }

                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("/");

                if path != "/throttled.bin" {
                    let _ = write_half
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
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

                // Throttle only the very first Range request (req_idx == 0) so it would take ~6s
                // without work stealing, while stolen sub-ranges stream at full speed.
                if throttle_first_request && req_idx == 0 {
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
            });
        }
    });

    format!("http://{addr}")
}

fn parse_byte_range(header: &str, total_len: usize) -> (usize, usize) {
    let spec = header.trim().strip_prefix("bytes=").unwrap();
    let (s, e) = spec.split_once('-').unwrap();
    let start: usize = s.parse().unwrap();
    let end: usize = if e.is_empty() {
        total_len - 1
    } else {
        e.parse::<usize>().unwrap().min(total_len - 1)
    };
    (start, end)
}

#[tokio::test]
async fn steals_work_from_throttled_worker_and_verifies_blake3() {
    // 12 MiB with 2 Workers -> initial segments are 6 MiB each.
    // Worker 0 is throttled (~1 MiB/s -> ~6s alone), Worker 1 finishes immediately and steals.
    let payload: Vec<u8> = (0..12 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let range_requests = Arc::new(AtomicUsize::new(0));

    let base_url =
        spawn_throttled_range_server(Arc::new(payload), true, Arc::clone(&range_requests)).await;

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

    // Without work stealing, 6 MiB at 64 KiB / 60ms takes ~5.76s.
    // With work stealing, Worker 1 bisects Segment 0 repeatedly so it finishes much faster.
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
