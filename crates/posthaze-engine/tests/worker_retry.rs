mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use common::{MockResponse, parse_byte_range, spawn_mock_server, spawn_raw_server};
use posthaze_engine::{DownloadEngine, DownloadRequest, EngineError};

#[tokio::test]
async fn recovers_from_503_and_mid_segment_drop_resuming_from_cursor() {
    let payload: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload = Arc::new(payload);
    let seg0_starts = Arc::new(Mutex::new(Vec::new()));
    let seg0_starts_srv = Arc::clone(&seg0_starts);
    let seg0_attempts = Arc::new(AtomicUsize::new(0));

    let base_url = spawn_raw_server(move |method, path, range_header, mut write_half| {
        let payload = Arc::clone(&payload);
        let seg0_attempts = Arc::clone(&seg0_attempts);
        let seg0_starts = Arc::clone(&seg0_starts_srv);
        async move {
            if path != "/flaky.bin" {
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

            let (start, end) = parse_byte_range(
                range_header.as_deref().unwrap_or("bytes=0-"),
                payload.len(),
            );

            // Inject failures only for Segment 0's lower half (start < 2 MiB).
            if start < 2 * 1024 * 1024 {
                seg0_starts.lock().unwrap().push(start);
                let attempt = seg0_attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    let _ = write_half
                        .write_all(
                            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                    let _ = write_half.shutdown().await;
                    return;
                }
                if attempt == 1 {
                    let slice = &payload[start..=end];
                    let hdr = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        payload.len()
                    );
                    let _ = write_half.write_all(hdr.as_bytes()).await;
                    let _ = write_half.write_all(&slice[..512 * 1024]).await;
                    let _ = write_half.shutdown().await;
                    return;
                }
            }

            let slice = &payload[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            let _ = write_half.write_all(slice).await;
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/flaky.bin"), temp_dir.path());
    req.max_workers = 2;

    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("download should recover from 503 and mid-stream drop");

    let starts = seg0_starts.lock().unwrap().clone();
    assert!(
        starts.len() >= 3,
        "expected at least 3 attempts on segment 0, got {starts:?}"
    );
    assert_eq!(starts[0], 0);
    assert_eq!(starts[1], 0);
    assert!(
        starts[2] >= 512 * 1024,
        "expected retry after drop to resume from cursor >= 512 KiB, got {}",
        starts[2]
    );

    assert_eq!(outcome.bytes_downloaded, 8 * 1024 * 1024);
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn fails_cleanly_when_max_retries_exhausted() {
    let get_count = Arc::new(AtomicUsize::new(0));
    let get_count_srv = Arc::clone(&get_count);
    let base_url = spawn_mock_server(move |method, _path, _range| {
        if method == "HEAD" {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), (1024 * 1024).to_string()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            }
        } else {
            get_count_srv.fetch_add(1, Ordering::SeqCst);
            MockResponse {
                status: "503 Service Unavailable",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            }
        }
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/dead.bin"), temp_dir.path());
    req.max_workers = 1;

    let engine = DownloadEngine::new();
    let err = engine
        .download(req, None)
        .await
        .expect_err("download should fail after exhausting retries");

    assert!(
        matches!(err, EngineError::HttpStatus { status: 503, .. }),
        "expected HttpStatus 503, got {err:?}"
    );
    assert_eq!(
        get_count.load(Ordering::SeqCst),
        6,
        "expected 1 initial attempt + 5 retries = 6 GET requests"
    );
}
