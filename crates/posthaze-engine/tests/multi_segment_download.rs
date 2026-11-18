use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use posthaze_engine::{DownloadEngine, DownloadRequest};

struct MockResponse {
    status: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn spawn_range_server<F>(handler: F) -> String
where
    F: Fn(&str, &str, Option<&str>) -> MockResponse + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handler = Arc::new(handler);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let handler = Arc::clone(&handler);
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
                let resp = handler(method, path, range_header.as_deref());

                let mut raw = format!("HTTP/1.1 {}\r\n", resp.status);
                for (k, v) in &resp.headers {
                    raw.push_str(&format!("{k}: {v}\r\n"));
                }
                raw.push_str("Connection: close\r\n\r\n");

                let _ = write_half.write_all(raw.as_bytes()).await;
                if method != "HEAD" {
                    let _ = write_half.write_all(&resp.body).await;
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
async fn downloads_multi_segment_payload_and_verifies_blake3() {
    // 10 MiB payload -> with max_workers = 4 and 2 MiB floor, uses 4 concurrent Workers.
    let payload: Vec<u8> = (0..10 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_arc = Arc::new(payload);
    let range_requests = Arc::new(AtomicUsize::new(0));
    let range_requests_clone = Arc::clone(&range_requests);

    let base_url = spawn_range_server(move |method, path, range| match (method, path, range) {
        ("HEAD", "/big.bin", _) => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_arc.len().to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
            ],
            body: Vec::new(),
        },
        ("GET", "/big.bin", Some(r)) => {
            range_requests_clone.fetch_add(1, Ordering::Relaxed);
            let (start, end) = parse_byte_range(r, payload_arc.len());
            let slice = payload_arc[start..=end].to_vec();
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), slice.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes {start}-{end}/{}", payload_arc.len()),
                    ),
                ],
                body: slice,
            }
        }
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/big.bin"), temp_dir.path());
    req.max_workers = 4;

    let engine = DownloadEngine::new();
    let outcome = engine.download(req, None).await.expect("multi-segment download should succeed");

    assert!(outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 4);
    assert_eq!(range_requests.load(Ordering::Relaxed), 4);
    assert_eq!(outcome.bytes_downloaded, 10 * 1024 * 1024);
    assert!(!temp_dir.path().join("big.bin.part").exists());

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn downloads_multi_segment_after_head_blocked_range_probe_fallback() {
    // 8 MiB payload with HEAD returning 405 -> falls back to Range: bytes=0-0, then 4 Workers.
    let payload: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 239) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_arc = Arc::new(payload);

    let base_url = spawn_range_server(move |method, path, range| match (method, path, range) {
        ("HEAD", "/cdn.bin", _) => MockResponse {
            status: "405 Method Not Allowed",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
        ("GET", "/cdn.bin", Some(r)) => {
            let (start, end) = parse_byte_range(r, payload_arc.len());
            let slice = payload_arc[start..=end].to_vec();
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), slice.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes {start}-{end}/{}", payload_arc.len()),
                    ),
                ],
                body: slice,
            }
        }
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/cdn.bin"), temp_dir.path());
    req.max_workers = 4;

    let engine = DownloadEngine::new();
    let outcome = engine.download(req, None).await.expect("download should succeed");

    assert!(outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 4);
    assert_eq!(outcome.bytes_downloaded, 8 * 1024 * 1024);

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn clamps_workers_to_one_when_below_min_segment_floor() {
    let payload: Vec<u8> = (0..512 * 1024).map(|i| (i % 197) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_arc = Arc::new(payload);

    let base_url = spawn_range_server(move |method, path, range| match (method, path, range) {
        ("HEAD", "/small.bin", _) => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_arc.len().to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
            ],
            body: Vec::new(),
        },
        ("GET", "/small.bin", Some(r)) => {
            let (start, end) = parse_byte_range(r, payload_arc.len());
            let slice = payload_arc[start..=end].to_vec();
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), slice.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes {start}-{end}/{}", payload_arc.len()),
                    ),
                ],
                body: slice,
            }
        }
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/small.bin"), temp_dir.path());
    req.max_workers = 8;

    let engine = DownloadEngine::new();
    let outcome = engine.download(req, None).await.expect("download should succeed");

    assert_eq!(outcome.workers_used, 1);
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}
