mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use common::{MockResponse, parse_byte_range, spawn_mock_server, spawn_raw_server};
use posthaze_engine::{DownloadEngine, DownloadRequest, EngineError};

#[tokio::test]
async fn handles_zero_byte_download_in_range_and_non_range_modes() {
    let base_url = spawn_mock_server(|method, path, _range| match (method, path) {
        ("HEAD" | "GET", "/empty-range.bin") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), "0".into()),
                ("Accept-Ranges".into(), "bytes".into()),
            ],
            body: Vec::new(),
        },
        ("HEAD" | "GET", "/empty-plain.bin") => MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();

    for name in ["empty-range.bin", "empty-plain.bin"] {
        let outcome = engine
            .download(
                DownloadRequest::new(format!("{base_url}/{name}"), temp_dir.path()),
                None,
            )
            .await
            .expect("zero-byte download should succeed");
        assert_eq!(outcome.bytes_downloaded, 0);
        assert_eq!(std::fs::metadata(&outcome.final_path).unwrap().len(), 0);
    }
}

#[tokio::test]
async fn prefixes_windows_reserved_device_names() {
    let payload = b"reserved-name-safe".to_vec();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD" | "GET", "/reserved") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_clone.len().to_string()),
                (
                    "Content-Disposition".into(),
                    "attachment; filename=\"CON.txt\"".into(),
                ),
            ],
            body: payload_clone.clone(),
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();
    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/reserved"), temp_dir.path()),
            None,
        )
        .await
        .expect("download should sanitize CON.txt");

    assert_eq!(outcome.final_path, temp_dir.path().join("_CON.txt"));
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn fails_immediately_on_non_transient_404_without_retrying() {
    let get_count = Arc::new(AtomicUsize::new(0));
    let get_count_srv = Arc::clone(&get_count);
    let base_url = spawn_mock_server(move |method, _path, _range| {
        if method == "HEAD" {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), (4 * 1024 * 1024).to_string()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            }
        } else {
            get_count_srv.fetch_add(1, Ordering::SeqCst);
            MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            }
        }
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/missing.bin"), temp_dir.path());
    req.max_workers = 1;

    let engine = DownloadEngine::new();
    let err = engine
        .download(req, None)
        .await
        .expect_err("404 should fail immediately");

    assert!(matches!(err, EngineError::HttpStatus { status: 404, .. }));
    assert_eq!(get_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chaos_high_concurrency_with_drops_503s_and_work_stealing() {
    let payload: Vec<u8> = (0..16 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload = Arc::new(payload);
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_srv = Arc::clone(&counter);

    let base_url = spawn_raw_server(move |method, path, range_header, mut write_half| {
        let payload = Arc::clone(&payload);
        let counter = Arc::clone(&counter_srv);
        async move {
            if path != "/chaos.bin" {
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

            let req_no = counter.fetch_add(1, Ordering::SeqCst);
            // Inject a 503 on request #0 and a mid-stream drop on request #2.
            if req_no == 0 {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = write_half.shutdown().await;
                return;
            }

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

            if req_no == 2 && slice.len() > 256 * 1024 {
                let _ = write_half.write_all(&slice[..256 * 1024]).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let _ = write_half.write_all(slice).await;
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/chaos.bin"), temp_dir.path());
    req.max_workers = 8;

    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("chaos download should recover and complete");

    assert_eq!(outcome.bytes_downloaded, 16 * 1024 * 1024);
    assert!(outcome.retries_performed >= 2);
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn falls_back_to_single_stream_when_range_get_returns_200_ok() {
    let payload: Vec<u8> = (0..(4 * 1024 * 1024 + 123))
        .map(|i| ((i * 31 + 7) % 251) as u8)
        .collect();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD", "/ignore-range.bin") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_clone.len().to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("Content-Type".into(), "application/octet-stream".into()),
            ],
            body: Vec::new(),
        },
        ("GET", "/ignore-range.bin") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_clone.len().to_string()),
                ("Content-Type".into(), "application/octet-stream".into()),
            ],
            body: payload_clone.clone(),
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/ignore-range.bin"), temp_dir.path());
    req.max_workers = 4;

    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("download should fall back to single-stream and succeed");

    assert!(!outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 1);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn rejects_html_200_response_instead_of_file_bytes() {
    let html_body =
        b"<html><head><title>Link Expired</title></head><body>Please refresh</body></html>"
            .to_vec();
    let html_clone = html_body.clone();

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD", "/download.aspx") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), (5 * 1024 * 1024).to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("Content-Type".into(), "application/octet-stream".into()),
            ],
            body: Vec::new(),
        },
        ("GET", "/download.aspx") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), html_clone.len().to_string()),
                ("Content-Type".into(), "text/html; charset=utf-8".into()),
            ],
            body: html_clone.clone(),
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/download.aspx"), temp_dir.path());
    req.max_workers = 4;

    let engine = DownloadEngine::new();
    let err = engine
        .download(req, None)
        .await
        .expect_err("HTML 200 response should fail with diagnostic error");

    match err {
        EngineError::UnexpectedHtmlResponse { detail, .. } => {
            assert!(
                detail.contains("Link Expired"),
                "expected HTML title in detail, got: {detail}"
            );
        }
        other => panic!("expected UnexpectedHtmlResponse, got: {other:?}"),
    }
}

#[tokio::test]
async fn skip_probe_downloads_single_use_endpoint_with_one_request() {
    let payload = b"single-use-signed-token-payload-bytes".to_vec();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_srv = Arc::clone(&request_count);

    let base_url = spawn_mock_server(move |method, path, range| {
        let n = request_count_srv.fetch_add(1, Ordering::SeqCst) + 1;
        if method == "GET" && path == "/once.aspx" && n == 1 && range.is_none() {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), payload_clone.len().to_string()),
                    ("Content-Type".into(), "application/octet-stream".into()),
                ],
                body: payload_clone.clone(),
            }
        } else {
            MockResponse {
                status: "410 Gone",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            }
        }
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/once.aspx"), temp_dir.path());
    req.output_path = Some(temp_dir.path().join("episode.mkv"));
    req.known_content_length = Some(payload.len() as u64);
    req.max_workers = 1;
    req.skip_probe = true;

    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("skip_probe should complete single-use URL in one GET");

    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(outcome.workers_used, 1);
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        expected_hash
    );
}
