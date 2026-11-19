mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use common::{MockResponse, parse_byte_range, spawn_mock_server};
use posthaze_engine::{
    DownloadEngine, DownloadId, DownloadRequest, DownloadState, EngineError, ProgressSnapshot,
};
use tokio::sync::watch;

async fn wait_until_active_with_bytes(rx: &mut watch::Receiver<ProgressSnapshot>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            {
                let snap = rx.borrow();
                if snap.state == DownloadState::Active && snap.bytes_downloaded > 0 {
                    break;
                }
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("download should become active and receive bytes");
}

#[allow(clippy::type_complexity)]
async fn spawn_resumable_server(
    payload: Arc<Vec<u8>>,
    etag: Arc<Mutex<Option<String>>>,
    last_modified: Arc<Mutex<Option<String>>>,
    throttle: Arc<AtomicBool>,
    recorded_ranges: Arc<Mutex<Vec<String>>>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let etag = Arc::clone(&etag);
            let last_modified = Arc::clone(&last_modified);
            let throttle = Arc::clone(&throttle);
            let recorded_ranges = Arc::clone(&recorded_ranges);

            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = req_str.lines().next().unwrap_or("");
                let method = first_line.split_whitespace().next().unwrap_or("");
                let range_hdr = req_str.lines().find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("range") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });

                let mut extra_headers = String::new();
                if let Some(e) = etag.lock().unwrap().as_ref() {
                    extra_headers.push_str(&format!("ETag: {e}\r\n"));
                }
                if let Some(lm) = last_modified.lock().unwrap().as_ref() {
                    extra_headers.push_str(&format!("Last-Modified: {lm}\r\n"));
                }

                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n{extra_headers}Connection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                if method == "GET" {
                    if let Some(r) = &range_hdr {
                        recorded_ranges.lock().unwrap().push(r.clone());
                    }
                    let (start, end) = match range_hdr.as_deref() {
                        Some(r) => parse_byte_range(r, payload.len()),
                        None => (0, payload.len().saturating_sub(1)),
                    };
                    let slice = &payload[start..=end];
                    let header = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\n{extra_headers}Connection: close\r\n\r\n",
                        slice.len(),
                        payload.len()
                    );
                    if stream.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    let is_throttled = throttle.load(Ordering::Relaxed);
                    for chunk in slice.chunks(64 * 1024) {
                        if stream.write_all(chunk).await.is_err() {
                            return;
                        }
                        if is_throttled {
                            tokio::time::sleep(Duration::from_millis(35)).await;
                        }
                    }
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn resumes_paused_download_across_engine_restart_and_verifies_blake3() {
    let payload: Vec<u8> = (0..6 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let throttle = Arc::new(AtomicBool::new(true));
    let recorded_ranges = Arc::new(Mutex::new(Vec::new()));

    let base_url = spawn_resumable_server(
        Arc::new(payload),
        Arc::new(Mutex::new(Some("\"v1\"".into()))),
        Arc::new(Mutex::new(Some("Wed, 21 Oct 2015 07:28:00 GMT".into()))),
        Arc::clone(&throttle),
        Arc::clone(&recorded_ranges),
    )
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/asset.bin"), dir.path());
    req.max_workers = 2;

    let handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    wait_until_active_with_bytes(&mut rx).await;

    handle.pause();
    let err = handle.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::Paused));
    drop(engine);

    // Clear recorded ranges and disable throttling before resuming in a new engine instance.
    recorded_ranges.lock().unwrap().clear();
    throttle.store(false, Ordering::Relaxed);

    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let resumed_handle = reopened.resume(&id).await.unwrap();
    let outcome = resumed_handle.wait().await.unwrap();

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);

    let resumed_ranges = recorded_ranges.lock().unwrap().clone();
    assert!(!resumed_ranges.is_empty());
    assert!(
        resumed_ranges.iter().any(|r| !r.starts_with("bytes=0-")),
        "expected resumed Range requests to start past 0, got {resumed_ranges:?}"
    );

    let record = reopened.get_download(&id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Completed);
}

#[tokio::test]
async fn fails_with_resource_changed_when_etag_changes_and_keeps_part() {
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let etag = Arc::new(Mutex::new(Some("\"v1\"".into())));
    let throttle = Arc::new(AtomicBool::new(true));

    let base_url = spawn_resumable_server(
        Arc::new(payload),
        Arc::clone(&etag),
        Arc::new(Mutex::new(None)),
        throttle,
        Arc::new(Mutex::new(Vec::new())),
    )
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let handle = engine
        .start(DownloadRequest::new(format!("{base_url}/v.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    wait_until_active_with_bytes(&mut rx).await;

    handle.pause();
    let _ = handle.wait().await;

    let part_path = engine
        .get_download(&id)
        .await
        .unwrap()
        .unwrap()
        .part_path
        .unwrap();
    assert!(part_path.exists());

    // Remote file ETag changes.
    *etag.lock().unwrap() = Some("\"v2\"".into());

    let resumed = engine.resume(&id).await.unwrap();
    let err = resumed.wait().await.unwrap_err();
    assert!(
        matches!(err, EngineError::ResourceChanged(_)),
        "expected ResourceChanged, got {err:?}"
    );
    assert!(
        part_path.exists(),
        ".part must remain intact on ResourceChanged"
    );
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
}

#[tokio::test]
async fn resumes_without_etag_or_last_modified_when_content_length_matches() {
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 239) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let throttle = Arc::new(AtomicBool::new(true));

    let base_url = spawn_resumable_server(
        Arc::new(payload),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::clone(&throttle),
        Arc::new(Mutex::new(Vec::new())),
    )
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/noval.bin"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    wait_until_active_with_bytes(&mut rx).await;

    handle.pause();
    let _ = handle.wait().await;

    throttle.store(false, Ordering::Relaxed);
    let resumed = engine.resume(&id).await.unwrap();
    let outcome = resumed.wait().await.unwrap();

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn rejects_resume_for_invalid_states_missing_id_or_missing_part() {
    let payload = b"small payload".to_vec();
    let payload_arc = Arc::new(payload);
    let base_url = spawn_mock_server(move |method, path, _| match (method, path) {
        ("HEAD", "/done.bin") => MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_arc.len().to_string())],
            body: Vec::new(),
        },
        ("GET", "/done.bin") => MockResponse {
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
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    // Non-existent ID.
    let missing_err = engine.resume(&DownloadId::new()).await.unwrap_err();
    assert!(matches!(missing_err, EngineError::NotFound(_)));

    // Completed download.
    let handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/done.bin"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let _ = handle.wait().await.unwrap();
    let completed_err = engine.resume(&id).await.unwrap_err();
    assert!(matches!(completed_err, EngineError::InvalidState(_)));
}
