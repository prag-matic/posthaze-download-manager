mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::MockResponse;
use posthaze_engine::{DownloadEngine, DownloadId, DownloadRequest, DownloadState, EngineError};
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn probe_returns_preflight_without_mutating_db_and_disambiguates_collisions() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let payload = vec![0x41u8; 4096];
    let payload_srv = Arc::new(payload.clone());
    let base_url = common::spawn_mock_server(move |method, _path, _range| {
        let len = payload_srv.len();
        MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".to_string(), len.to_string()),
                ("Accept-Ranges".to_string(), "bytes".to_string()),
                ("ETag".to_string(), "\"v1-preflight\"".to_string()),
                (
                    "Last-Modified".to_string(),
                    "Wed, 19 Nov 2026 10:00:00 GMT".to_string(),
                ),
            ],
            body: if method == "HEAD" {
                Vec::new()
            } else {
                payload_srv.as_ref().clone()
            },
        }
    })
    .await;

    let url = format!("{base_url}/archive.tar.gz");
    let info1 = engine.probe(&url, dir.path()).await.unwrap();
    assert_eq!(info1.original_url, url);
    assert_eq!(info1.final_url, url);
    assert_eq!(info1.suggested_filename, "archive.tar.gz");
    assert_eq!(
        info1.disambiguated_output_path,
        dir.path().join("archive.tar.gz")
    );
    assert_eq!(info1.content_length, Some(4096));
    assert!(info1.supports_ranges);
    assert_eq!(info1.etag.as_deref(), Some("\"v1-preflight\""));
    assert_eq!(
        info1.last_modified.as_deref(),
        Some("Wed, 19 Nov 2026 10:00:00 GMT")
    );

    // Probing must not create a DB record or .part file.
    assert!(engine.list_downloads().await.unwrap().is_empty());
    assert!(!dir.path().join("archive.tar.gz.part").exists());

    // Create an existing final file and an existing (1).part file; probe must skip to (2).
    std::fs::write(dir.path().join("archive.tar.gz"), b"existing").unwrap();
    std::fs::write(dir.path().join("archive.tar (1).gz.part"), b"partial").unwrap();

    let info2 = engine.probe(&url, dir.path()).await.unwrap();
    assert_eq!(
        info2.disambiguated_output_path,
        dir.path().join("archive.tar (2).gz")
    );

    // Starting without explicit output_path must also disambiguate to (2) instead of overwriting.
    let mut handle = engine
        .start(DownloadRequest::new(&url, dir.path()))
        .await
        .unwrap();
    let outcome = handle.wait().await.unwrap();
    assert_eq!(outcome.final_path, dir.path().join("archive.tar (2).gz"));
    assert_eq!(std::fs::read(&outcome.final_path).unwrap(), payload);
    assert_eq!(
        std::fs::read(dir.path().join("archive.tar.gz")).unwrap(),
        b"existing"
    );

    // Extensionless collision disambiguates as `<name> (1)`.
    std::fs::write(dir.path().join("README"), b"existing").unwrap();
    let extless = engine
        .probe(&format!("{base_url}/README"), dir.path())
        .await
        .unwrap();
    assert_eq!(extless.disambiguated_output_path, dir.path().join("README (1)"));
}

#[tokio::test]
async fn engine_pause_by_id_pauses_active_and_rejects_invalid_states() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let total_len = 256 * 1024usize;
    let payload: Vec<u8> = (0..total_len).map(|i| (i % 251) as u8).collect();
    let payload_srv = Arc::new(payload);

    let base_url = common::spawn_raw_server(move |method, _path, range, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"p1\"\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = range
                .as_deref()
                .map(|r| common::parse_byte_range(r, payload_srv.len()))
                .unwrap_or((0, payload_srv.len() - 1));
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nETag: \"p1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(4096) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut req = DownloadRequest::new(format!("{base_url}/pausable.bin"), dir.path());
    req.max_workers = 1;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();

    let mut rx = handle.subscribe();
    rx.wait_for(|s| s.state == DownloadState::Active && s.bytes_downloaded > 0)
        .await
        .unwrap();

    engine.pause(&id).await.unwrap();
    assert!(matches!(handle.wait().await, Err(EngineError::Paused)));

    let rec = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(rec.state, DownloadState::Paused);
    assert!(rec.part_path.unwrap().exists());

    // Pausing an already-paused or unknown download returns explicit errors.
    assert!(matches!(
        engine.pause(&id).await,
        Err(EngineError::InvalidState(_))
    ));
    assert!(matches!(
        engine.pause(&DownloadId::new()).await,
        Err(EngineError::NotFound(_))
    ));
}

#[tokio::test]
async fn engine_delete_removes_record_part_and_respects_delete_file_flag() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let payload = vec![0x99u8; 64 * 1024];
    let payload_srv = Arc::new(payload.clone());
    let base_url = common::spawn_raw_server(move |method, _path, range, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"d1\"\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = range
                .as_deref()
                .map(|r| common::parse_byte_range(r, payload_srv.len()))
                .unwrap_or((0, payload_srv.len() - 1));
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nETag: \"d1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(4096) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(8)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    // 1. Completed download deleted with delete_file = false keeps final file.
    let mut h1 = engine
        .start(DownloadRequest::new(format!("{base_url}/keep.bin"), dir.path()))
        .await
        .unwrap();
    let out1 = h1.wait().await.unwrap();
    engine.delete(h1.id(), false).await.unwrap();
    assert!(engine.get_download(h1.id()).await.unwrap().is_none());
    assert!(out1.final_path.exists());

    // 2. Completed download deleted with delete_file = true removes final file.
    let mut h2 = engine
        .start(DownloadRequest::new(format!("{base_url}/remove.bin"), dir.path()))
        .await
        .unwrap();
    let out2 = h2.wait().await.unwrap();
    engine.delete(h2.id(), true).await.unwrap();
    assert!(engine.get_download(h2.id()).await.unwrap().is_none());
    assert!(!out2.final_path.exists());

    // 3. Paused download deleted with delete_file = false still cleans up .part.
    let mut req3 = DownloadRequest::new(format!("{base_url}/partial.bin"), dir.path());
    req3.max_workers = 1;
    let mut h3 = engine.start(req3).await.unwrap();
    h3.subscribe()
        .wait_for(|s| s.state == DownloadState::Active && s.bytes_downloaded > 0)
        .await
        .unwrap();
    engine.pause(h3.id()).await.unwrap();
    let _ = h3.wait().await;
    let part3 = engine
        .get_download(h3.id())
        .await
        .unwrap()
        .unwrap()
        .part_path
        .unwrap();
    assert!(part3.exists());

    engine.delete(h3.id(), false).await.unwrap();
    assert!(engine.get_download(h3.id()).await.unwrap().is_none());
    assert!(!part3.exists());

    // 4. Deleting missing ID returns NotFound.
    assert!(matches!(
        engine.delete(&DownloadId::new(), false).await,
        Err(EngineError::NotFound(_))
    ));
}

#[tokio::test]
async fn engine_restart_fresh_clears_stale_part_and_recovers_from_resource_changed() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let v1: Vec<u8> = vec![0x11u8; 128 * 1024];
    let v2: Vec<u8> = (0..(96 * 1024)).map(|i| (i % 199) as u8).collect();
    let version = Arc::new(AtomicUsize::new(1));

    let v1_srv = Arc::new(v1);
    let v2_srv = Arc::new(v2.clone());
    let ver_srv = Arc::clone(&version);

    let base_url = common::spawn_raw_server(move |method, _path, range, mut write_half| {
        let v1_srv = Arc::clone(&v1_srv);
        let v2_srv = Arc::clone(&v2_srv);
        let ver_srv = Arc::clone(&ver_srv);
        async move {
            let ver = ver_srv.load(Ordering::SeqCst);
            let (body, etag) = if ver == 1 {
                (v1_srv, "\"etag-v1\"")
            } else {
                (v2_srv, "\"etag-v2\"")
            };
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = range
                .as_deref()
                .map(|r| common::parse_byte_range(r, body.len()))
                .unwrap_or((0, body.len() - 1));
            let slice = &body[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                slice.len(),
                body.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(4096) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                if ver == 1 {
                    tokio::time::sleep(Duration::from_millis(8)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut req = DownloadRequest::new(format!("{base_url}/firmware.bin"), dir.path());
    req.max_workers = 1;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    handle
        .subscribe()
        .wait_for(|s| s.state == DownloadState::Active && s.bytes_downloaded > 0)
        .await
        .unwrap();
    engine.pause(&id).await.unwrap();
    let _ = handle.wait().await;

    // Flip remote resource to v2; resume must fail with ResourceChanged.
    version.store(2, Ordering::SeqCst);
    let mut resumed = engine.resume(&id).await.unwrap();
    assert!(matches!(
        resumed.wait().await,
        Err(EngineError::ResourceChanged(_))
    ));
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().state,
        DownloadState::Failed
    );

    // restart_fresh discards stale v1 .part and downloads v2 cleanly from byte 0.
    let mut fresh = engine.restart_fresh(&id).await.unwrap();
    let outcome = fresh.wait().await.unwrap();
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        blake3::hash(&v2)
    );
}
