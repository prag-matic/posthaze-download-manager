mod common;

use std::sync::Arc;
use tempfile::TempDir;

use common::{MockResponse, parse_byte_range, spawn_mock_server};
use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState};

#[tokio::test]
async fn persists_completed_download_across_engine_reopen() {
    let payload: Vec<u8> = (0..5 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_arc = Arc::new(payload);

    let base_url = spawn_mock_server(move |method, path, range| match (method, path, range) {
        ("HEAD", "/archive.bin", _) => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_arc.len().to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("ETag".into(), "\"v1-abc\"".into()),
                ("Last-Modified".into(), "Wed, 21 Oct 2015 07:28:00 GMT".into()),
            ],
            body: Vec::new(),
        },
        ("GET", "/archive.bin", Some(r)) => {
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

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let url = format!("{base_url}/archive.bin");

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let mut req = DownloadRequest::new(url.clone(), dir.path());
    req.max_workers = 2;

    let handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let rx = handle.subscribe();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(rx.borrow().state, DownloadState::Completed);
    let written = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&written), expected_hash);

    drop(engine);

    // Reopen SQLite database in a fresh engine instance and verify persisted record.
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let record = reopened
        .get_download(&id)
        .await
        .unwrap()
        .expect("download record should persist across reopen");

    assert_eq!(record.id, id);
    assert_eq!(record.url, url);
    assert_eq!(record.final_url.as_deref(), Some(url.as_str()));
    assert_eq!(record.output_path.as_deref(), Some(outcome.final_path.as_path()));
    assert!(record.part_path.is_some());
    assert_eq!(record.state, DownloadState::Completed);
    assert_eq!(record.content_length, Some(written.len() as u64));
    assert_eq!(record.bytes_downloaded, written.len() as u64);
    assert!(record.supports_ranges);
    assert_eq!(record.etag.as_deref(), Some("\"v1-abc\""));
    assert_eq!(
        record.last_modified.as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );

    let all = reopened.list_downloads().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, id);
}

#[tokio::test]
async fn in_memory_engine_starts_and_lists_download() {
    let payload = b"hello posthaze in-memory".to_vec();
    let payload_arc = Arc::new(payload.clone());

    let base_url = spawn_mock_server(move |method, path, _| match (method, path) {
        ("HEAD", "/hello.txt") => MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_arc.len().to_string())],
            body: Vec::new(),
        },
        ("GET", "/hello.txt") => MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_arc.len().to_string())],
            body: payload_arc.as_ref().clone(),
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/hello.txt"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(std::fs::read(&outcome.final_path).unwrap(), payload);
    let list = engine.list_downloads().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].state, DownloadState::Completed);
}

#[tokio::test]
async fn persists_failed_state_on_http_error() {
    let base_url = spawn_mock_server(|_, _, _| MockResponse {
        status: "404 Not Found",
        headers: vec![("Content-Length".into(), "0".into())],
        body: Vec::new(),
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/missing.bin"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let err = handle.wait().await.unwrap_err();
    assert!(err.to_string().contains("404"));

    let record = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Failed);
    assert!(record.error_message.is_some());
}
