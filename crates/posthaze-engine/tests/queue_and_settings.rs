mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState, EngineError, Settings};
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};

#[tokio::test]
async fn settings_persist_across_engine_reopen_and_validate_bounds() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let defaults = engine.get_settings().await.unwrap();
    assert_eq!(defaults.max_workers, 8);
    assert_eq!(defaults.max_concurrent_downloads, 3);
    assert!(!defaults.sync_on_complete);

    let custom_dir = dir.path().join("downloads");
    let updated = Settings {
        default_output_dir: custom_dir.clone(),
        max_workers: 12,
        max_concurrent_downloads: 5,
        sync_on_complete: true,
    };
    engine.update_settings(updated.clone()).await.unwrap();
    assert_eq!(engine.get_settings().await.unwrap(), updated);

    // Invalid bounds must be rejected without clobbering stored settings.
    let invalid_workers = Settings {
        max_workers: 0,
        ..updated.clone()
    };
    assert!(engine.update_settings(invalid_workers).await.is_err());

    let invalid_conc = Settings {
        max_concurrent_downloads: 0,
        ..updated.clone()
    };
    assert!(engine.update_settings(invalid_conc).await.is_err());
    assert_eq!(engine.get_settings().await.unwrap(), updated);

    drop(engine);

    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(reopened.get_settings().await.unwrap(), updated);
    assert_eq!(
        reopened.get_settings().await.unwrap().default_output_dir,
        PathBuf::from(&custom_dir)
    );
}

#[tokio::test]
async fn queue_enforces_max_concurrent_downloads_in_fifo_order() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 2;
    engine.update_settings(settings).await.unwrap();

    let payload = vec![0x5Au8; 64 * 1024];
    let payload_len = payload.len();
    let active_requests = Arc::new(AtomicUsize::new(0));
    let max_observed_concurrent = Arc::new(AtomicUsize::new(0));
    let admitted_order = Arc::new(Mutex::new(Vec::<String>::new()));
    let release_gate = Arc::new(Notify::new());

    let active_srv = Arc::clone(&active_requests);
    let max_srv = Arc::clone(&max_observed_concurrent);
    let order_srv = Arc::clone(&admitted_order);
    let gate_srv = Arc::clone(&release_gate);
    let payload_srv = payload.clone();

    let base_url = common::spawn_raw_server(move |method, path, _range, mut write_half| {
        let active_srv = Arc::clone(&active_srv);
        let max_srv = Arc::clone(&max_srv);
        let order_srv = Arc::clone(&order_srv);
        let gate_srv = Arc::clone(&gate_srv);
        let payload_srv = payload_srv.clone();
        async move {
            if method == "HEAD" {
                let mut guard = order_srv.lock().await;
                if !guard.contains(&path) {
                    guard.push(path.clone());
                }
                drop(guard);

                let current = active_srv.fetch_add(1, Ordering::SeqCst) + 1;
                max_srv.fetch_max(current, Ordering::SeqCst);

                // Hold HEAD until test releases one wave.
                gate_srv.notified().await;

                active_srv.fetch_sub(1, Ordering::SeqCst);

                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {payload_len}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {payload_len}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            let _ = write_half.write_all(&payload_srv).await;
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut handles = Vec::new();
    for i in 1..=5 {
        let mut req = DownloadRequest::new(format!("{base_url}/file-{i}.bin"), dir.path());
        req.max_workers = 1;
        handles.push(engine.start(req).await.unwrap());
    }

    // Give the first 2 Downloads time to enter HEAD and block on the gate.
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(active_requests.load(Ordering::SeqCst), 2);
    assert_eq!(max_observed_concurrent.load(Ordering::SeqCst), 2);

    // Remaining 3 must still be Queued.
    for h in &handles[2..] {
        assert_eq!(h.subscribe().borrow().state, DownloadState::Queued);
    }

    // Periodically release gate until all 5 complete.
    let releaser_gate = Arc::clone(&release_gate);
    let releaser = tokio::spawn(async move {
        for _ in 0..10 {
            releaser_gate.notify_waiters();
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    });

    for (idx, mut h) in handles.into_iter().enumerate() {
        let outcome = h.wait().await.unwrap();
        assert_eq!(
            blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
            blake3::hash(&payload),
            "file {} hash mismatch",
            idx + 1
        );
    }
    releaser.abort();

    assert!(max_observed_concurrent.load(Ordering::SeqCst) <= 2);
    let order = admitted_order.lock().await.clone();
    assert_eq!(
        order,
        vec![
            "/file-1.bin",
            "/file-2.bin",
            "/file-3.bin",
            "/file-4.bin",
            "/file-5.bin"
        ]
    );
}

#[tokio::test]
async fn update_settings_wakes_queued_downloads_immediately() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 1;
    engine.update_settings(settings.clone()).await.unwrap();

    let payload = vec![0x33u8; 32 * 1024];
    let payload_len = payload.len();
    let head_hits = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());

    let hits_srv = Arc::clone(&head_hits);
    let gate_srv = Arc::clone(&gate);
    let payload_srv = payload.clone();

    let base_url = common::spawn_raw_server(move |method, _path, _range, mut write_half| {
        let hits_srv = Arc::clone(&hits_srv);
        let gate_srv = Arc::clone(&gate_srv);
        let payload_srv = payload_srv.clone();
        async move {
            if method == "HEAD" {
                hits_srv.fetch_add(1, Ordering::SeqCst);
                gate_srv.notified().await;
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {payload_len}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {payload_len}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            let _ = write_half.write_all(&payload_srv).await;
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut handles = Vec::new();
    for i in 1..=3 {
        let mut req = DownloadRequest::new(format!("{base_url}/wake-{i}.bin"), dir.path());
        req.max_workers = 1;
        handles.push(engine.start(req).await.unwrap());
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(head_hits.load(Ordering::SeqCst), 1);

    // Raise concurrency from 1 -> 3 while #1 is still blocked in HEAD.
    settings.max_concurrent_downloads = 3;
    engine.update_settings(settings).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(head_hits.load(Ordering::SeqCst), 3);

    gate.notify_waiters();
    for mut h in handles {
        let outcome = h.wait().await.unwrap();
        assert_eq!(std::fs::read(&outcome.final_path).unwrap(), payload);
    }
}

#[tokio::test]
async fn queued_download_supports_immediate_pause_resume_and_cancel() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 1;
    engine.update_settings(settings).await.unwrap();

    let payload = vec![0x77u8; 32 * 1024];
    let payload_len = payload.len();
    let blocker_gate = Arc::new(Notify::new());

    let gate_srv = Arc::clone(&blocker_gate);
    let payload_srv = payload.clone();

    let base_url = common::spawn_raw_server(move |method, path, _range, mut write_half| {
        let gate_srv = Arc::clone(&gate_srv);
        let payload_srv = payload_srv.clone();
        async move {
            if method == "HEAD" && path == "/blocker.bin" {
                gate_srv.notified().await;
            }
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {payload_len}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if method != "HEAD" {
                let _ = write_half.write_all(&payload_srv).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut h1 = engine
        .start(DownloadRequest::new(format!("{base_url}/blocker.bin"), dir.path()))
        .await
        .unwrap();
    let mut h2 = engine
        .start(DownloadRequest::new(format!("{base_url}/paused-in-queue.bin"), dir.path()))
        .await
        .unwrap();
    let mut h3 = engine
        .start(DownloadRequest::new(format!("{base_url}/cancelled-in-queue.bin"), dir.path()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(h2.subscribe().borrow().state, DownloadState::Queued);
    assert_eq!(h3.subscribe().borrow().state, DownloadState::Queued);

    // Pause h2 and cancel h3 while h1 still holds the single concurrency slot.
    h2.pause();
    assert!(matches!(h2.wait().await, Err(EngineError::Paused)));
    assert_eq!(
        engine.get_download(h2.id()).await.unwrap().unwrap().state,
        DownloadState::Paused
    );

    engine.cancel(h3.id()).await.unwrap();
    assert!(matches!(h3.wait().await, Err(EngineError::Cancelled)));
    assert_eq!(
        engine.get_download(h3.id()).await.unwrap().unwrap().state,
        DownloadState::Cancelled
    );

    // Release blocker h1 and resume h2.
    blocker_gate.notify_waiters();
    assert!(h1.wait().await.is_ok());

    let mut resumed_h2 = engine.resume(h2.id()).await.unwrap();
    let outcome = resumed_h2.wait().await.unwrap();
    assert_eq!(std::fs::read(&outcome.final_path).unwrap(), payload);
}

#[tokio::test]
async fn open_reconciles_stale_queued_downloads_to_paused() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let payload = vec![0xABu8; 16 * 1024];
    let payload_len = payload.len();
    let gate = Arc::new(Notify::new());

    let gate_srv = Arc::clone(&gate);
    let payload_srv = payload.clone();

    let base_url = common::spawn_raw_server(move |method, path, _range, mut write_half| {
        let gate_srv = Arc::clone(&gate_srv);
        let payload_srv = payload_srv.clone();
        async move {
            if method == "HEAD" && path == "/hold.bin" {
                gate_srv.notified().await;
            }
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {payload_len}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n"
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if method != "HEAD" {
                let _ = write_half.write_all(&payload_srv).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let db_for_crash = db_path.clone();
    let out_for_crash = dir.path().to_path_buf();
    let url_for_crash = base_url.clone();

    let (id1, id2) = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (id1, id2) = rt.block_on(async {
            let eng = DownloadEngine::open(&db_for_crash).await.unwrap();
            let mut s = eng.get_settings().await.unwrap();
            s.max_concurrent_downloads = 1;
            eng.update_settings(s).await.unwrap();

            let h1 = eng
                .start(DownloadRequest::new(format!("{url_for_crash}/hold.bin"), &out_for_crash))
                .await
                .unwrap();
            let h2 = eng
                .start(DownloadRequest::new(format!("{url_for_crash}/stale-queued.bin"), &out_for_crash))
                .await
                .unwrap();

            tokio::time::sleep(Duration::from_millis(80)).await;
            (h1.id().clone(), h2.id().clone())
        });
        rt.shutdown_background();
        (id1, id2)
    })
    .await
    .unwrap();

    // Release gate so any lingering sockets close cleanly, then reopen DB.
    gate.notify_waiters();

    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let rec1 = reopened.get_download(&id1).await.unwrap().unwrap();
    let rec2 = reopened.get_download(&id2).await.unwrap().unwrap();
    assert_eq!(rec1.state, DownloadState::Failed);
    assert_eq!(rec2.state, DownloadState::Paused);

    // Reconciled Paused (formerly Queued) download can be resumed to completion.
    let mut resumed = reopened.resume(&id2).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    assert_eq!(std::fs::read(&outcome.final_path).unwrap(), payload);
}
