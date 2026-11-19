mod common;

use tempfile::TempDir;
use tokio::sync::watch;

use common::{MockResponse, spawn_mock_server};
use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState, ProgressSnapshot};

#[tokio::test]
async fn downloads_single_stream_and_verifies_blake3_and_metadata() {
    let payload: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD" | "GET", "/redirect") => MockResponse {
            status: "302 Found",
            headers: vec![
                ("Location".into(), "/files/actual.bin".into()),
                ("Content-Length".into(), "0".into()),
            ],
            body: Vec::new(),
        },
        ("HEAD" | "GET", "/files/actual.bin") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_clone.len().to_string()),
                (
                    "Content-Disposition".into(),
                    "attachment; filename=\"resolved-name.bin\"".into(),
                ),
                ("ETag".into(), "\"abc123\"".into()),
                (
                    "Last-Modified".into(),
                    "Wed, 21 Oct 2015 07:28:00 GMT".into(),
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
    let (tx, rx) = watch::channel(ProgressSnapshot {
        state: DownloadState::Queued,
        ..Default::default()
    });

    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/redirect"), temp_dir.path()),
            Some(tx),
        )
        .await
        .expect("download should succeed");

    assert_eq!(
        outcome.final_path,
        temp_dir.path().join("resolved-name.bin")
    );
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(outcome.etag.as_deref(), Some("\"abc123\""));
    assert_eq!(
        outcome.last_modified.as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );
    assert_eq!(outcome.final_url, format!("{base_url}/files/actual.bin"));

    let snapshot = rx.borrow().clone();
    assert_eq!(snapshot.state, DownloadState::Completed);
    assert_eq!(snapshot.bytes_downloaded, payload.len() as u64);

    let part_path = temp_dir.path().join("resolved-name.bin.part");
    assert!(!part_path.exists(), ".part file must be renamed away");

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn prefers_rfc5987_filename_and_sanitizes_windows_illegal_chars() {
    let payload = b"rfc5987-payload-bytes".to_vec();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD" | "GET", "/item") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_clone.len().to_string()),
                (
                    "Content-Disposition".into(),
                    "attachment; filename=\"fallback.bin\"; filename*=UTF-8''My%20Video%3A%201080p%2A.mp4"
                        .into(),
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
            DownloadRequest::new(format!("{base_url}/item"), temp_dir.path()),
            None,
        )
        .await
        .expect("download should succeed");

    assert_eq!(
        outcome.final_path,
        temp_dir.path().join("My Video_ 1080p_.mp4")
    );
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn percent_decodes_url_filename_and_coalesces_across_chunk_boundary() {
    // 600 KiB exceeds the 128 KiB coalescing buffer multiple times.
    let payload: Vec<u8> = (0..600 * 1024).map(|i| (i % 199) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path, _range| match (method, path) {
        ("HEAD" | "GET", "/My%20Archive%20v1.zip") => MockResponse {
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
            DownloadRequest::new(format!("{base_url}/My%20Archive%20v1.zip"), temp_dir.path()),
            None,
        )
        .await
        .expect("download should succeed");

    assert_eq!(
        outcome.final_path,
        temp_dir.path().join("My Archive v1.zip")
    );
    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}
