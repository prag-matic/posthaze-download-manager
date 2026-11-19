mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use common::{MockResponse, parse_byte_range, spawn_mock_server};
use posthaze_engine::{
    DownloadEngine, DownloadRequest, DownloadState, EngineError, ProgressSnapshot,
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

/// Spawns a slow streaming HTTP server that drips `chunk_size` bytes every `delay`
/// so tests can deterministically `pause()` or `cancel()` mid-transfer.
async fn spawn_slow_range_server(
    payload: Arc<Vec<u8>>,
    chunk_size: usize,
    delay: Duration,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
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
                let mut parts = first_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let _path = parts.next().unwrap_or("");
                let range_hdr = req_str.lines().find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("range") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });

                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"slow-v1\"\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                if method == "GET" {
                    let (start, end) = match range_hdr.as_deref() {
                        Some(r) => parse_byte_range(r, payload.len()),
                        None => (0, payload.len().saturating_sub(1)),
                    };
                    let slice = &payload[start..=end];
                    let header = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        payload.len()
                    );
                    if stream.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in slice.chunks(chunk_size) {
                        if stream.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(delay).await;
                    }
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn pause_live_download_preserves_part_and_persists_paused_state() {
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let base_url =
        spawn_slow_range_server(Arc::new(payload), 64 * 1024, Duration::from_millis(40)).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/slow.bin"), dir.path());
    req.max_workers = 2;

    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

    // Wait until transfer is Active and has received at least some bytes.
    wait_until_active_with_bytes(&mut rx).await;

    handle.pause();
    let err = handle.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::Paused), "got {err:?}");

    let record = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Paused);
    assert!(record.bytes_downloaded > 0);
    let part_path = record.part_path.expect("part_path should be recorded");
    assert!(part_path.exists(), ".part file must be preserved on pause");
}

#[tokio::test]
async fn cancel_live_download_deletes_part_and_marks_cancelled() {
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let base_url =
        spawn_slow_range_server(Arc::new(payload), 64 * 1024, Duration::from_millis(40)).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/cancel_live.bin"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

    wait_until_active_with_bytes(&mut rx).await;

    let part_path = engine
        .get_download(&id)
        .await
        .unwrap()
        .unwrap()
        .part_path
        .unwrap();
    assert!(part_path.exists());

    handle.cancel();
    let err = handle.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::Cancelled), "got {err:?}");

    assert!(!part_path.exists(), ".part file must be deleted on cancel");
    let record = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Cancelled);
}

#[tokio::test]
async fn cancel_persisted_paused_download_deletes_part_and_marks_cancelled() {
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let base_url =
        spawn_slow_range_server(Arc::new(payload), 64 * 1024, Duration::from_millis(40)).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/cancel_persisted.bin"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

    wait_until_active_with_bytes(&mut rx).await;

    handle.pause();
    let _ = handle.wait().await;
    drop(engine);

    // Reopen in a new engine instance and cancel the persisted Paused download.
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let before = reopened.get_download(&id).await.unwrap().unwrap();
    assert_eq!(before.state, DownloadState::Paused);
    let part_path = before.part_path.unwrap();
    assert!(part_path.exists());

    reopened.cancel(&id).await.unwrap();

    assert!(!part_path.exists(), ".part file must be deleted on cancel(id)");
    let after = reopened.get_download(&id).await.unwrap().unwrap();
    assert_eq!(after.state, DownloadState::Cancelled);
}

#[tokio::test]
async fn resumable_failed_download_retains_part_file() {
    let total = 1024 * 1024usize;
    let gets = Arc::new(AtomicUsize::new(0));
    let gets_clone = Arc::clone(&gets);

    let base_url = spawn_mock_server(move |method, path, _| match (method, path) {
        ("HEAD", "/flaky.bin") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), total.to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
                ("ETag".into(), "\"flaky-v1\"".into()),
            ],
            body: Vec::new(),
        },
        ("GET", "/flaky.bin") => {
            gets_clone.fetch_add(1, Ordering::Relaxed);
            MockResponse {
                status: "403 Forbidden",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
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
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut handle = engine
        .start(DownloadRequest::new(
            format!("{base_url}/flaky.bin"),
            dir.path(),
        ))
        .await
        .unwrap();
    let id = handle.id().clone();
    let err = handle.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::HttpStatus { status: 403, .. }));

    let record = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Failed);
    assert!(record.supports_ranges);
    let part_path = record.part_path.unwrap();
    assert!(
        part_path.exists(),
        "resumable Failed download must retain its .part file"
    );

    // Cancelling the persisted Failed download deletes .part and marks Cancelled.
    engine.cancel(&id).await.unwrap();
    assert!(!part_path.exists());
    let cancelled = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(cancelled.state, DownloadState::Cancelled);
}
