use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use posthaze_engine::{DownloadEngine, DownloadRequest, EngineError};

async fn spawn_flaky_range_server(payload: Arc<Vec<u8>>, seg0_starts: Arc<Mutex<Vec<usize>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seg0_attempts = Arc::new(AtomicUsize::new(0));

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let seg0_attempts = Arc::clone(&seg0_attempts);
            let seg0_starts = Arc::clone(&seg0_starts);

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

                if path != "/flaky.bin" {
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

                let (start, end) = parse_byte_range(
                    range_header.as_deref().unwrap_or("bytes=0-"),
                    payload.len(),
                );

                // Inject failures only for Segment 0's lower half (start < 2 MiB) so work-stealing
                // on [2 MiB, 4 MiB) does not race the fault-injection counter.
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
                        // Send 206 claiming full range, write 512 KiB, then abruptly drop connection.
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
            });
        }
    });

    format!("http://{addr}")
}

async fn spawn_always_503_server(total_len: usize, get_count: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let get_count = Arc::clone(&get_count);
            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
                    return;
                }
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let method = request_line.split_whitespace().next().unwrap_or("");
                if method == "HEAD" {
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {total_len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
                    );
                    let _ = write_half.write_all(hdr.as_bytes()).await;
                } else {
                    get_count.fetch_add(1, Ordering::SeqCst);
                    let _ = write_half
                        .write_all(
                            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
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
async fn recovers_from_503_and_mid_segment_drop_resuming_from_cursor() {
    let payload: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let seg0_starts = Arc::new(Mutex::new(Vec::new()));

    let base_url = spawn_flaky_range_server(Arc::new(payload), Arc::clone(&seg0_starts)).await;

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
    let base_url = spawn_always_503_server(1024 * 1024, Arc::clone(&get_count)).await;

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
