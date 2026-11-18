mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

use common::{MockResponse, parse_byte_range, spawn_mock_server};
use posthaze_engine::{DownloadEngine, DownloadRequest};

#[tokio::test]
async fn downloads_multi_segment_payload_and_verifies_blake3() {
    // 10 MiB payload -> with max_workers = 4 and 2 MiB floor, uses 4 concurrent Workers.
    let payload: Vec<u8> = (0..10 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_arc = Arc::new(payload);
    let range_requests = Arc::new(AtomicUsize::new(0));
    let range_requests_clone = Arc::clone(&range_requests);

    let base_url = spawn_mock_server(move |method, path, range| match (method, path, range) {
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
    let outcome = engine
        .download(req, None)
        .await
        .expect("multi-segment download should succeed");

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

    let base_url = spawn_mock_server(move |method, path, range| match (method, path, range) {
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
    let outcome = engine
        .download(req, None)
        .await
        .expect("download should succeed");

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

    let base_url = spawn_mock_server(move |method, path, range| match (method, path, range) {
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
    let outcome = engine
        .download(req, None)
        .await
        .expect("download should succeed");

    assert_eq!(outcome.workers_used, 1);
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}
