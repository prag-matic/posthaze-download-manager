use std::sync::Arc;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use posthaze_engine::{DownloadEngine, DownloadRequest};

struct MockResponse {
    status: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn spawn_mock_server<F>(handler: F) -> String
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

#[tokio::test]
async fn falls_back_to_range_get_when_head_is_blocked() {
    let payload: Vec<u8> = (0..128 * 1024).map(|i| (i % 241) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, range| match (method, path, range) {
        ("HEAD", "/blocked.bin", _) => MockResponse {
            status: "405 Method Not Allowed",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
        ("GET", "/blocked.bin", Some("bytes=0-0")) => MockResponse {
            status: "206 Partial Content",
            headers: vec![
                ("Content-Length".into(), "1".into()),
                (
                    "Content-Range".into(),
                    format!("bytes 0-0/{}", payload_clone.len()),
                ),
                ("ETag".into(), "\"range-fallback\"".into()),
            ],
            body: vec![payload_clone[0]],
        },
        ("GET", "/blocked.bin", None) => MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_clone.len().to_string())],
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
            DownloadRequest::new(format!("{base_url}/blocked.bin"), temp_dir.path()),
            None,
        )
        .await
        .expect("download should succeed via Range GET fallback");

    assert!(outcome.supports_ranges);
    assert_eq!(outcome.content_length, Some(payload.len() as u64));
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(outcome.etag.as_deref(), Some("\"range-fallback\""));

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn falls_back_to_range_get_when_head_omits_range_and_length() {
    let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 173) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, range| match (method, path, range) {
        ("HEAD", "/minimal-head.bin", _) => MockResponse {
            status: "200 OK",
            headers: Vec::new(),
            body: Vec::new(),
        },
        ("GET", "/minimal-head.bin", Some("bytes=0-0")) => MockResponse {
            status: "206 Partial Content",
            headers: vec![
                ("Content-Length".into(), "1".into()),
                (
                    "Content-Range".into(),
                    format!("bytes 0-0/{}", payload_clone.len()),
                ),
            ],
            body: vec![payload_clone[0]],
        },
        ("GET", "/minimal-head.bin", None) => MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_clone.len().to_string())],
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
            DownloadRequest::new(format!("{base_url}/minimal-head.bin"), temp_dir.path()),
            None,
        )
        .await
        .expect("download should succeed");

    assert!(outcome.supports_ranges);
    assert_eq!(outcome.content_length, Some(payload.len() as u64));

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn downloads_chunked_unknown_length_in_single_stream_mode() {
    let payload: Vec<u8> = (0..90 * 1024).map(|i| (i % 211) as u8).collect();
    let expected_hash = blake3::hash(&payload);

    let mut chunked_body = Vec::new();
    for piece in payload.chunks(16 * 1024) {
        chunked_body.extend_from_slice(format!("{:x}\r\n", piece.len()).as_bytes());
        chunked_body.extend_from_slice(piece);
        chunked_body.extend_from_slice(b"\r\n");
    }
    chunked_body.extend_from_slice(b"0\r\n\r\n");

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD", "/stream.bin") => MockResponse {
            status: "200 OK",
            headers: vec![("Transfer-Encoding".into(), "chunked".into())],
            body: Vec::new(),
        },
        ("GET", "/stream.bin") => MockResponse {
            status: "200 OK",
            headers: vec![("Transfer-Encoding".into(), "chunked".into())],
            body: chunked_body.clone(),
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
            DownloadRequest::new(format!("{base_url}/stream.bin"), temp_dir.path()),
            None,
        )
        .await
        .expect("chunked download should succeed without pre-allocation errors");

    assert!(!outcome.supports_ranges);
    assert_eq!(outcome.content_length, None);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}
