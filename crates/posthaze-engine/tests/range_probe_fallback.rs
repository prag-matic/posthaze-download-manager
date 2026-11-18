mod common;

use tempfile::TempDir;

use common::{MockResponse, parse_byte_range, spawn_mock_server};
use posthaze_engine::{DownloadEngine, DownloadRequest};

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
        ("GET", "/blocked.bin", Some(range_val)) => {
            if range_val == "bytes=0-0" {
                MockResponse {
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
                }
            } else {
                let (s, e) = parse_byte_range(range_val, payload_clone.len());
                let slice = payload_clone[s..=e].to_vec();
                MockResponse {
                    status: "206 Partial Content",
                    headers: vec![
                        ("Content-Length".into(), slice.len().to_string()),
                        (
                            "Content-Range".into(),
                            format!("bytes {s}-{e}/{}", payload_clone.len()),
                        ),
                    ],
                    body: slice,
                }
            }
        }
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
        ("GET", "/minimal-head.bin", Some(range_val)) => {
            let (s, e) = parse_byte_range(range_val, payload_clone.len());
            let slice = payload_clone[s..=e].to_vec();
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), slice.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes {s}-{e}/{}", payload_clone.len()),
                    ),
                ],
                body: slice,
            }
        }
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
