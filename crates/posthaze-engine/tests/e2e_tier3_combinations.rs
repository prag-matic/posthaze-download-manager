mod e2e_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use e2e_common::{
    MockResponse, blake3_hex, deterministic_payload, file_blake3_hex, parse_byte_range,
    spawn_http_proxy_server, spawn_mock_server, spawn_raw_server,
};
use posthaze_engine::writer::PartFileWriter;
use posthaze_engine::{
    DownloadEngine, DownloadRequest, DownloadState, EngineError, EngineEvent, ProxyMode, Settings,
};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};

// 1. EA1 x EA4: Multi-segment transfer + speed limit + live ProgressSnapshot
#[tokio::test]
async fn tier3_multi_segment_plus_speed_limit_plus_progress_snapshot() {
    let payload = Arc::new(deterministic_payload(5 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, _, range| {
        if method == "HEAD" {
            return MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), payload_srv.len().to_string()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            };
        }
        let (start, end) = parse_byte_range(range.unwrap_or("bytes=0-"), payload_srv.len());
        let slice = payload_srv[start..=end].to_vec();
        MockResponse {
            status: "206 Partial Content",
            headers: vec![
                ("Content-Length".into(), slice.len().to_string()),
                (
                    "Content-Range".into(),
                    format!("bytes {start}-{end}/{}", payload_srv.len()),
                ),
            ],
            body: slice,
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        download_speed_limit_kib: 4096, // 4 MiB/s across 3 segments
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/throttled-multi.bin"), dir.path());
    req.max_workers = 3;

    let started = Instant::now();
    let mut handle = engine.start(req).await.unwrap();
    let mut rx = handle.subscribe();
    let mut saw_multi = false;
    let watcher = tokio::spawn(async move {
        loop {
            let snap = rx.borrow().clone();
            if snap.state == DownloadState::Active && snap.segments.len() >= 2 {
                saw_multi = true;
            }
            if snap.state == DownloadState::Completed || rx.changed().await.is_err() {
                break;
            }
        }
        saw_multi
    });

    let outcome = handle.wait().await.unwrap();
    assert!(watcher.await.unwrap());
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(outcome.workers_used, 3);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

// 2. EA2 x EA3: enqueue_paused + update_settings concurrency bump + FIFO wakeup
#[tokio::test]
async fn tier3_enqueue_paused_plus_update_settings_concurrency_bump_plus_fifo() {
    let payload = Arc::new(deterministic_payload(32 * 1024));
    let expected_hash = blake3_hex(&payload);
    let gate = Arc::new(Notify::new());
    let head_hits = Arc::new(AtomicUsize::new(0));
    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    let payload_srv = Arc::clone(&payload);
    let gate_srv = Arc::clone(&gate);
    let hits_srv = Arc::clone(&head_hits);
    let order_srv = Arc::clone(&order);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let gate_srv = Arc::clone(&gate_srv);
        let hits_srv = Arc::clone(&hits_srv);
        let order_srv = Arc::clone(&order_srv);
        async move {
            if req.method == "HEAD" {
                hits_srv.fetch_add(1, Ordering::SeqCst);
                order_srv.lock().await.push(req.path.clone());
                gate_srv.notified().await;
            }
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if req.method != "HEAD" {
                let _ = write_half.write_all(&payload_srv).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 1;
    engine.update_settings(settings.clone()).await.unwrap();

    let id1 = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/p1.bin"), dir.path()))
        .await
        .unwrap();
    let id2 = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/p2.bin"), dir.path()))
        .await
        .unwrap();
    let id3 = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/p3.bin"), dir.path()))
        .await
        .unwrap();

    let mut h1 = engine.resume(&id1).await.unwrap();
    let mut h2 = engine.resume(&id2).await.unwrap();
    let mut h3 = engine.resume(&id3).await.unwrap();

    tokio::time::timeout(Duration::from_secs(3), async {
        while head_hits.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(head_hits.load(Ordering::SeqCst), 1);

    // Raise concurrency from 1 -> 3 while h1 is blocked in HEAD.
    settings.max_concurrent_downloads = 3;
    engine.update_settings(settings).await.unwrap();

    tokio::time::timeout(Duration::from_secs(3), async {
        while head_hits.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    gate.notify_waiters();
    for h in [&mut h1, &mut h2, &mut h3] {
        let out = h.wait().await.unwrap();
        assert_eq!(file_blake3_hex(&out.final_path), expected_hash);
    }
    assert_eq!(*order.lock().await, vec!["/p1.bin", "/p2.bin", "/p3.bin"]);
}

// 3. EA1 x EA3: Manual proxy + custom headers + multi-segment Range download
#[tokio::test]
async fn tier3_manual_proxy_plus_custom_headers_plus_multi_segment_range() {
    let payload = Arc::new(deterministic_payload(5 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let seen_headers = Arc::new(Mutex::new(Vec::<Option<String>>::new()));

    let payload_srv = Arc::clone(&payload);
    let headers_srv = Arc::clone(&seen_headers);
    let target_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let headers_srv = Arc::clone(&headers_srv);
        async move {
            headers_srv
                .lock()
                .await
                .push(req.header("x-api-key").map(str::to_string));
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, payload_srv.len()))
                .unwrap_or((0, payload_srv.len() - 1));
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            let _ = write_half.write_all(slice).await;
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let (proxy_url, proxy_hits) = spawn_http_proxy_server(&target_url).await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        proxy_mode: ProxyMode::Manual,
        proxy_url,
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let mut req = DownloadRequest::new(format!("{target_url}/proxied-multi.bin"), dir.path());
    req.max_workers = 2;
    req.headers = vec![("X-Api-Key".to_string(), "key-999".to_string())];

    let outcome = engine.download(req, None).await.unwrap();
    assert_eq!(outcome.workers_used, 2);
    assert!(proxy_hits.load(Ordering::SeqCst) >= 3);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);

    let captured = seen_headers.lock().await;
    assert!(captured.len() >= 3);
    assert!(captured.iter().all(|v| v.as_deref() == Some("key-999")));
}

// 4. EA2 x EA5 x EA6: Pause + reopen DB + restart_fresh + rename_completed_file + BLAKE3
#[tokio::test]
async fn tier3_pause_reopen_db_plus_restart_fresh_plus_rename_and_blake3() {
    let payload_v1 = Arc::new(deterministic_payload(2 * 1024 * 1024));
    let payload_v2 = Arc::new(deterministic_payload(2 * 1024 * 1024 + 321));
    let v2_hash = blake3_hex(&payload_v2);
    let switch_v2 = Arc::new(AtomicBool::new(false));

    let v1_srv = Arc::clone(&payload_v1);
    let v2_srv = Arc::clone(&payload_v2);
    let flag_srv = Arc::clone(&switch_v2);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let data = if flag_srv.load(Ordering::SeqCst) {
            Arc::clone(&v2_srv)
        } else {
            Arc::clone(&v1_srv)
        };
        let slow = !flag_srv.load(Ordering::SeqCst);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    data.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, data.len()))
                .unwrap_or((0, data.len() - 1));
            let slice = &data[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                data.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                if slow {
                    tokio::time::sleep(Duration::from_millis(12)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let id = {
        let engine = DownloadEngine::open(&db_path).await.unwrap();
        let mut handle = engine
            .start(DownloadRequest::new(format!("{base_url}/pkg.bin"), dir.path()))
            .await
            .unwrap();
        let id = handle.id().clone();
        tokio::time::timeout(Duration::from_secs(5), async {
            handle
                .subscribe()
                .wait_for(|s| s.bytes_downloaded >= 256 * 1024)
                .await
                .unwrap();
        })
        .await
        .unwrap();
        engine.pause(&id).await.unwrap();
        let _ = handle.wait().await;
        id
    };

    switch_v2.store(true, Ordering::SeqCst);
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let mut fresh_handle = reopened.restart_fresh(&id).await.unwrap();
    let outcome = fresh_handle.wait().await.unwrap();
    assert_eq!(outcome.bytes_downloaded, payload_v2.len() as u64);

    let renamed = reopened
        .rename_completed_file(&id, "pkg-v2.bin")
        .await
        .unwrap();
    assert_eq!(reopened.compute_file_blake3(&id).await.unwrap(), v2_hash);
    assert_eq!(file_blake3_hex(&renamed), v2_hash);
}

// 5. EA1 x EA5: Concurrent multi-download Checkpoint writes + simultaneous list queries
#[tokio::test]
async fn tier3_concurrent_checkpoints_plus_simultaneous_list_queries() {
    let payload = Arc::new(deterministic_payload(10 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, payload_srv.len()))
                .unwrap_or((0, payload_srv.len() - 1));
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(256 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(3)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(&dir.path().join("state.db"))
        .await
        .unwrap();

    let mut handles = Vec::new();
    for i in 1..=3 {
        let mut req = DownloadRequest::new(format!("{base_url}/c-{i}.bin"), dir.path());
        req.max_workers = 4;
        handles.push(engine.start(req).await.unwrap());
    }

    let reader_engine = engine.clone();
    let ids: Vec<_> = handles.iter().map(|h| h.id().clone()).collect();
    let reader = tokio::spawn(async move {
        for _ in 0..50 {
            let list = reader_engine.list_downloads().await.unwrap();
            assert_eq!(list.len(), 3);
            let _ = reader_engine.list_segment_counts().await.unwrap();
            for id in &ids {
                let _ = reader_engine.list_segments(id).await.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    for mut h in handles {
        let out = h.wait().await.unwrap();
        assert_eq!(file_blake3_hex(&out.final_path), expected_hash);
    }
    reader.await.unwrap();
}

// 6. F10/F11 x EA1 x EA2: Constrained 2-worker Tokio runtime with concurrent multi-segment transfers
#[test]
fn tier3_constrained_two_worker_tokio_runtime_concurrent_multi_segment() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(16)
        .thread_keep_alive(Duration::from_secs(5))
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let payload = Arc::new(deterministic_payload(6 * 1024 * 1024));
        let expected_hash = blake3_hex(&payload);
        let payload_srv = Arc::clone(&payload);

        let base_url = spawn_mock_server(move |method, _, range| {
            if method == "HEAD" {
                return MockResponse {
                    status: "200 OK",
                    headers: vec![
                        ("Content-Length".into(), payload_srv.len().to_string()),
                        ("Accept-Ranges".into(), "bytes".into()),
                    ],
                    body: Vec::new(),
                };
            }
            let (start, end) = parse_byte_range(range.unwrap_or("bytes=0-"), payload_srv.len());
            let slice = payload_srv[start..=end].to_vec();
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), slice.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes {start}-{end}/{}", payload_srv.len()),
                    ),
                ],
                body: slice,
            }
        })
        .await;

        let dir = TempDir::new().unwrap();
        let engine = DownloadEngine::open(&dir.path().join("rt2.db"))
            .await
            .unwrap();

        let mut handles = Vec::new();
        for i in 1..=3 {
            let mut req = DownloadRequest::new(format!("{base_url}/rt-{i}.bin"), dir.path());
            req.max_workers = 4;
            handles.push(engine.start(req).await.unwrap());
        }

        for mut h in handles {
            let out = h.wait().await.unwrap();
            assert_eq!(file_blake3_hex(&out.final_path), expected_hash);
        }
    });
}

// 7. EA3 x EA6: Redirect + Content-Disposition filename + collision disambiguation
#[tokio::test]
async fn tier3_redirect_plus_content_disposition_plus_collision_disambiguation() {
    let payload = Arc::new(deterministic_payload(64 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, path, _| match path {
        "/redir" => MockResponse {
            status: "302 Found",
            headers: vec![
                ("Location".into(), "/files/blob".into()),
                ("Content-Length".into(), "0".into()),
            ],
            body: Vec::new(),
        },
        "/files/blob" => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_srv.len().to_string()),
                (
                    "Content-Disposition".into(),
                    "attachment; filename=\"bundle.zip\"".into(),
                ),
            ],
            body: if method == "HEAD" {
                Vec::new()
            } else {
                payload_srv.as_ref().clone()
            },
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("bundle.zip"), b"existing").unwrap();

    let engine = DownloadEngine::in_memory().await.unwrap();
    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/redir"), dir.path()),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.final_path, dir.path().join("bundle (1).zip"));
    assert_eq!(outcome.final_url, format!("{base_url}/files/blob"));
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

// 8. EA2 x EA3: Pause in-flight + proxy hot-swap + resume through new proxy
#[tokio::test]
async fn tier3_pause_in_flight_plus_proxy_hotswap_plus_resume() {
    let payload = Arc::new(deterministic_payload(3 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let slow = Arc::new(AtomicBool::new(true));

    let payload_srv = Arc::clone(&payload);
    let slow_srv = Arc::clone(&slow);
    let target_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let slow_srv = Arc::clone(&slow_srv);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"px1\"\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, payload_srv.len()))
                .unwrap_or((0, payload_srv.len() - 1));
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"px1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                if slow_srv.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(12)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let (proxy_url, proxy_hits) = spawn_http_proxy_server(&target_url).await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{target_url}/swap.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        handle
            .subscribe()
            .wait_for(|s| s.bytes_downloaded >= 256 * 1024)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    engine.pause(&id).await.unwrap();
    let _ = handle.wait().await;
    assert_eq!(proxy_hits.load(Ordering::SeqCst), 0);

    // Hot-swap to Manual proxy and resume.
    slow.store(false, Ordering::Relaxed);
    let mut settings = engine.get_settings().await.unwrap();
    settings.proxy_mode = ProxyMode::Manual;
    settings.proxy_url = proxy_url;
    engine.update_settings(settings).await.unwrap();

    let mut resumed = engine.resume(&id).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    assert!(proxy_hits.load(Ordering::SeqCst) >= 2);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

// 9. EA1 x EA3: HEAD 403 fallback + dynamic split + sync_on_complete
#[tokio::test]
async fn tier3_head_fallback_403_plus_dynamic_split_plus_sync_on_complete() {
    let payload = Arc::new(deterministic_payload(8 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            if req.method == "HEAD" {
                let _ = write_half
                    .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, payload_srv.len()))
                .unwrap_or((0, payload_srv.len() - 1));
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            let slow = start == 0 && slice.len() > 1024;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                if slow {
                    tokio::time::sleep(Duration::from_millis(12)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/fallback-split.bin"), dir.path());
    req.max_workers = 2;
    req.sync_on_complete = true;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(outcome.splits_performed >= 1);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

// 10. EA1 x EA2 x EA4: PartFileWriter backpressure + engine cancel cleanup + Event stream
#[tokio::test]
async fn tier3_writer_backpressure_plus_cancel_cleanup_and_event_stream() {
    let dir = TempDir::new().unwrap();
    let part_path = dir.path().join("bp-check.part");
    let final_path = dir.path().join("bp-check.bin");

    // Exercise PartFileWriter directly with 20 chunks (10 MiB).
    let chunk = Bytes::from(vec![0x55u8; 512 * 1024]);
    let writer = PartFileWriter::spawn(part_path, Some(20 * 512 * 1024)).unwrap();
    for i in 0..20usize {
        writer
            .write_at((i * 512 * 1024) as u64, chunk.clone())
            .await
            .unwrap();
    }
    writer.finish(&final_path).await.unwrap();
    assert_eq!(std::fs::metadata(&final_path).unwrap().len(), 20 * 512 * 1024);

    // Pair with slow download cancellation verifying .part removal and Cancelled event.
    let payload = Arc::new(deterministic_payload(2 * 1024 * 1024));
    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if req.method != "HEAD" {
                for c in payload_srv.chunks(32 * 1024) {
                    if write_half.write_all(c).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut events = engine.subscribe_events();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/abort.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();

    tokio::time::timeout(Duration::from_secs(5), async {
        handle
            .subscribe()
            .wait_for(|s| s.state == DownloadState::Active && s.bytes_downloaded > 0)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    engine.cancel(&id).await.unwrap();
    assert!(matches!(handle.wait().await, Err(EngineError::Cancelled)));
    assert!(!dir.path().join("abort.bin.part").exists());

    let mut saw_cancelled_event = false;
    while let Ok(ev) = events.try_recv() {
        if matches!(
            ev,
            EngineEvent::StateChanged {
                id: ref eid,
                state: DownloadState::Cancelled,
                ..
            } if *eid == id
        ) {
            saw_cancelled_event = true;
        }
    }
    assert!(saw_cancelled_event);
}
