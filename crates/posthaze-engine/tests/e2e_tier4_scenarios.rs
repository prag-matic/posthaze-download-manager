mod e2e_common;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use e2e_common::{
    MockResponse, blake3_hex, create_pre_0010_db, deterministic_payload, file_blake3_hex,
    parse_byte_range, spawn_http_proxy_server, spawn_mock_server, spawn_raw_server,
};
use posthaze_engine::{
    CollisionPolicy, DownloadEngine, DownloadRequest, DownloadState, EngineError, EngineEvent,
    ProxyMode, Settings, categorize_subfolder, resolve_target_path,
};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

// Scenario 1: Mixed-size multi-file queue (0B, 100 KiB single-stream, 4 MiB .zip,
// 6 MiB .mp4, 8 MiB .bin) under bounded concurrency + categorization + BLAKE3 + move.
#[tokio::test]
async fn tier4_scenario_mixed_size_bounded_queue_categorization_and_blake3() {
    let files: Vec<(&str, Vec<u8>, bool)> = vec![
        ("empty.txt", Vec::new(), false),
        ("readme.bin", deterministic_payload(100 * 1024), false),
        ("archive.zip", deterministic_payload(4 * 1024 * 1024), true),
        ("clip.mp4", deterministic_payload(6 * 1024 * 1024), true),
        ("dataset.bin", deterministic_payload(8 * 1024 * 1024), true),
    ];

    let mut expected = HashMap::new();
    let mut server_map = HashMap::new();
    for (name, bytes, ranges) in &files {
        expected.insert(name.to_string(), blake3_hex(bytes));
        server_map.insert(format!("/{name}"), (Arc::new(bytes.clone()), *ranges));
    }
    let server_map = Arc::new(server_map);

    let base_url = spawn_mock_server(move |method, path, range| {
        let Some((data, supports_ranges)) = server_map.get(path) else {
            return MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            };
        };
        if method == "HEAD" {
            return MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), data.len().to_string()),
                    (
                        "Accept-Ranges".into(),
                        if *supports_ranges { "bytes" } else { "none" }.into(),
                    ),
                ],
                body: Vec::new(),
            };
        }
        if *supports_ranges && let Some(r) = range {
            let (s, e) = parse_byte_range(r, data.len());
            let slice = data[s..=e].to_vec();
            return MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), slice.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes {s}-{e}/{}", data.len()),
                    ),
                ],
                body: slice,
            };
        }
        MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), data.len().to_string())],
            body: data.as_ref().clone(),
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        max_concurrent_downloads: 2,
        max_workers: 4,
        auto_categorize_folders: true,
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let mut handles = Vec::new();
    for (name, _, _) in &files {
        let target_dir = match categorize_subfolder(name) {
            Some(cat) => dir.path().join(cat),
            None => dir.path().to_path_buf(),
        };
        std::fs::create_dir_all(&target_dir).unwrap();
        let resolved = resolve_target_path(&target_dir, name, CollisionPolicy::Rename);
        assert!(!resolved.needs_overwrite_prompt);

        let mut req = DownloadRequest::new(format!("{base_url}/{name}"), &target_dir);
        req.output_path = Some(resolved.path);
        req.max_workers = 4;
        handles.push((name.to_string(), engine.start(req).await.unwrap()));
    }

    for (name, mut h) in handles {
        let id = h.id().clone();
        let outcome = h.wait().await.unwrap();
        assert_eq!(
            engine.compute_file_blake3(&id).await.unwrap(),
            expected[&name]
        );
        assert_eq!(file_blake3_hex(&outcome.final_path), expected[&name]);
        if name == "archive.zip" {
            assert!(outcome.final_path.starts_with(dir.path().join("Archives")));
        } else if name == "clip.mp4" {
            assert!(outcome.final_path.starts_with(dir.path().join("Video")));
        }
    }

    assert_eq!(engine.list_downloads().await.unwrap().len(), 5);
}

// Scenario 2: Mid-transfer crash simulation (drop engine after >8 MiB checkpoint)
// -> reopen DB -> reconcile Active & Queued -> resume both to byte-exact BLAKE3.
#[tokio::test]
async fn tier4_scenario_mid_transfer_crash_reopen_resume_to_completion() {
    let payload_big = Arc::new(deterministic_payload(12 * 1024 * 1024));
    let payload_small = Arc::new(deterministic_payload(256 * 1024));
    let hash_big = blake3_hex(&payload_big);
    let hash_small = blake3_hex(&payload_small);
    let stall_after_9m = Arc::new(AtomicBool::new(true));
    let entered_stall = Arc::new(AtomicBool::new(false));
    let stall_wake = Arc::new(Notify::new());

    let big_srv = Arc::clone(&payload_big);
    let small_srv = Arc::clone(&payload_small);
    let stall_srv = Arc::clone(&stall_after_9m);
    let entered_srv = Arc::clone(&entered_stall);
    let wake_srv = Arc::clone(&stall_wake);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let big_srv = Arc::clone(&big_srv);
        let small_srv = Arc::clone(&small_srv);
        let stall_srv = Arc::clone(&stall_srv);
        let entered_srv = Arc::clone(&entered_srv);
        let wake_srv = Arc::clone(&wake_srv);
        async move {
            if req.path == "/small.bin" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                    small_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                if req.method != "HEAD" {
                    let _ = write_half.write_all(&small_srv).await;
                }
                let _ = write_half.shutdown().await;
                return;
            }
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"sc2\"\r\nConnection: close\r\n\r\n",
                    big_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, big_srv.len()))
                .unwrap_or((0, big_srv.len() - 1));
            let slice = &big_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"sc2\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                big_srv.len()
            );
            if write_half.write_all(hdr.as_bytes()).await.is_err() {
                return;
            }
            let mut pos = 0usize;
            for chunk in slice.chunks(256 * 1024) {
                if stall_srv.load(Ordering::Acquire) && start + pos >= 9 * 1024 * 1024 {
                    entered_srv.store(true, Ordering::Release);
                    while stall_srv.load(Ordering::Acquire) {
                        let _ = tokio::time::timeout(
                            Duration::from_millis(50),
                            wake_srv.notified(),
                        )
                        .await;
                    }
                    return;
                }
                if write_half.write_all(chunk).await.is_err() {
                    return;
                }
                pos += chunk.len();
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("crash.db");

    let (id_big, id_small) = {
        let db = db_path.clone();
        let out = dir.path().to_path_buf();
        let url = base_url.clone();
        let entered = Arc::clone(&entered_stall);
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let ids = rt.block_on(async {
                let engine = DownloadEngine::open(&db).await.unwrap();
                let mut settings = engine.get_settings().await.unwrap();
                settings.max_concurrent_downloads = 1;
                engine.update_settings(settings).await.unwrap();

                let mut req_big = DownloadRequest::new(format!("{url}/big.bin"), &out);
                req_big.max_workers = 1;
                let h_big = engine.start(req_big).await.unwrap();
                let h_small = engine
                    .start(DownloadRequest::new(format!("{url}/small.bin"), &out))
                    .await
                    .unwrap();
                let id_big = h_big.id().clone();
                let id_small = h_small.id().clone();

                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if entered.load(Ordering::Acquire)
                            && let Some(rec) = engine.get_download(&id_big).await.unwrap()
                            && rec.bytes_downloaded >= 8 * 1024 * 1024
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();

                drop(h_big);
                drop(h_small);
                drop(engine);
                (id_big, id_small)
            });
            rt.shutdown_timeout(Duration::from_millis(200));
            ids
        })
        .await
        .unwrap()
    };

    stall_after_9m.store(false, Ordering::Release);
    stall_wake.notify_waiters();
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(
        reopened.get_download(&id_big).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
    assert_eq!(
        reopened.get_download(&id_small).await.unwrap().unwrap().state,
        DownloadState::Paused
    );

    let mut r_big = reopened.resume(&id_big).await.unwrap();
    let mut r_small = reopened.resume(&id_small).await.unwrap();
    let out_big = r_big.wait().await.unwrap();
    let out_small = r_small.wait().await.unwrap();

    assert_eq!(file_blake3_hex(&out_big.final_path), hash_big);
    assert_eq!(file_blake3_hex(&out_small.final_path), hash_small);
}

// Scenario 3: Live queue settings hot-swap (speed limit + proxy switch + concurrency bump)
#[tokio::test]
async fn tier4_scenario_live_settings_hotswap_during_active_queue() {
    let payload = Arc::new(deterministic_payload(256 * 1024));
    let expected_hash = blake3_hex(&payload);
    let gate = Arc::new(Notify::new());
    let head_hits = Arc::new(AtomicUsize::new(0));

    let payload_srv = Arc::clone(&payload);
    let gate_srv = Arc::clone(&gate);
    let hits_srv = Arc::clone(&head_hits);
    let target_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let gate_srv = Arc::clone(&gate_srv);
        let hits_srv = Arc::clone(&hits_srv);
        async move {
            if req.method == "HEAD" && req.path == "/item-1.bin" {
                hits_srv.fetch_add(1, Ordering::SeqCst);
                gate_srv.notified().await;
            } else if req.method == "HEAD" {
                hits_srv.fetch_add(1, Ordering::SeqCst);
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

    let (proxy_url, proxy_hits) = spawn_http_proxy_server(&target_url).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("hotswap.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let mut events = engine.subscribe_events();

    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 1;
    settings.download_speed_limit_kib = 128;
    settings.proxy_mode = ProxyMode::None;
    engine.update_settings(settings.clone()).await.unwrap();

    let mut handles = Vec::new();
    for i in 1..=4 {
        handles.push(
            engine
                .start(DownloadRequest::new(
                    format!("{target_url}/item-{i}.bin"),
                    dir.path(),
                ))
                .await
                .unwrap(),
        );
    }

    tokio::time::timeout(Duration::from_secs(3), async {
        while head_hits.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(handles[1].subscribe().borrow().state, DownloadState::Queued);

    // Hot-swap: lift speed limit, bump concurrency to 4, switch proxy to Manual.
    settings.max_concurrent_downloads = 4;
    settings.download_speed_limit_kib = 0;
    settings.proxy_mode = ProxyMode::Manual;
    settings.proxy_url = proxy_url.clone();
    engine.update_settings(settings.clone()).await.unwrap();

    // Items 2..4 wake up immediately when concurrency increases.
    tokio::time::timeout(Duration::from_secs(3), async {
        while head_hits.load(Ordering::SeqCst) < 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    // New probe & download started after hot-swap must route through the Manual proxy.
    let post_swap_info = engine
        .probe(format!("{target_url}/item-5.bin"), dir.path())
        .await
        .unwrap();
    assert_eq!(post_swap_info.content_length, Some(payload.len() as u64));
    let mut handle_5 = engine
        .start(DownloadRequest::new(
            format!("{target_url}/item-5.bin"),
            dir.path(),
        ))
        .await
        .unwrap();

    gate.notify_waiters();
    for mut h in handles {
        let out = h.wait().await.unwrap();
        assert_eq!(file_blake3_hex(&out.final_path), expected_hash);
    }
    let out_5 = handle_5.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&out_5.final_path), expected_hash);

    assert!(proxy_hits.load(Ordering::SeqCst) >= 3);

    let mut saw_settings_event = false;
    while let Ok(ev) = events.try_recv() {
        if matches!(ev, EngineEvent::SettingsUpdated(ref s) if s.max_concurrent_downloads == 4 && s.proxy_mode == ProxyMode::Manual) {
            saw_settings_event = true;
        }
    }
    assert!(saw_settings_event);

    drop(engine);
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(reopened.get_settings().await.unwrap(), settings);
}

// Scenario 4: Flaky/work-stealing chaos server (503 bursts + mid-body TCP drops + slow worker)
#[tokio::test]
async fn tier4_scenario_flaky_work_stealing_chaos_server_recovers_byte_exact() {
    let payload = Arc::new(deterministic_payload(12 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let get_seq = Arc::new(AtomicUsize::new(0));

    let payload_srv = Arc::clone(&payload);
    let seq_srv = Arc::clone(&get_seq);
    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let seq_srv = Arc::clone(&seq_srv);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"chaos\"\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let seq = seq_srv.fetch_add(1, Ordering::SeqCst);
            if seq == 0 || seq == 3 {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
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
            if write_half.write_all(hdr.as_bytes()).await.is_err() {
                return;
            }

            // Abrupt mid-body drop on seq == 2.
            if seq == 2 && slice.len() > 256 * 1024 {
                let _ = write_half.write_all(&slice[..256 * 1024]).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let slow = start == 0 && slice.len() > 1024 * 1024;
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
    let engine = DownloadEngine::open(&dir.path().join("chaos.db"))
        .await
        .unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/chaos-12m.bin"), dir.path());
    req.max_workers = 4;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(outcome.retries_performed >= 3);
    assert!(outcome.splits_performed >= 1);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

// Scenario 5: Legacy DB upgrade + staged pause + ETag mismatch restart_fresh + rename/move/delete
#[tokio::test]
async fn tier4_scenario_legacy_upgrade_pause_restart_rename_move_and_delete() {
    let payload_v1 = Arc::new(deterministic_payload(2 * 1024 * 1024));
    let payload_v2 = Arc::new(deterministic_payload(2 * 1024 * 1024 + 99));
    let hash_v2 = blake3_hex(&payload_v2);
    let version = Arc::new(AtomicUsize::new(1));

    let v1_srv = Arc::clone(&payload_v1);
    let v2_srv = Arc::clone(&payload_v2);
    let ver_srv = Arc::clone(&version);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let ver = ver_srv.load(Ordering::SeqCst);
        let data = if ver == 1 {
            Arc::clone(&v1_srv)
        } else {
            Arc::clone(&v2_srv)
        };
        let etag = if ver == 1 { "\"r1\"" } else { "\"r2\"" };
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                slice.len(),
                data.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                if ver == 1 {
                    tokio::time::sleep(Duration::from_millis(12)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("legacy_session.db");
    create_pre_0010_db(&db_path, Some("deadbeef-0000-4000-8000-000000000001")).await;

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/release.bin"), dir.path()))
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

    // Server updates release -> resume fails with ResourceChanged -> restart_fresh succeeds.
    version.store(2, Ordering::SeqCst);
    let mut fail_resume = engine.resume(&id).await.unwrap();
    assert!(matches!(
        fail_resume.wait().await,
        Err(EngineError::ResourceChanged(_))
    ));

    let mut fresh = engine.restart_fresh(&id).await.unwrap();
    let _ = fresh.wait().await.unwrap();

    let renamed = engine
        .rename_completed_file(&id, "release-r2.bin")
        .await
        .unwrap();
    let archive_dir = dir.path().join("archived");
    let moved = engine
        .move_completed_file(&id, &archive_dir)
        .await
        .unwrap();
    assert!(!renamed.exists());
    assert_eq!(engine.compute_file_blake3(&id).await.unwrap(), hash_v2);
    assert_eq!(file_blake3_hex(&moved), hash_v2);

    engine.delete(&id, true).await.unwrap();
    assert!(!moved.exists());
    assert!(engine.get_download(&id).await.unwrap().is_none());
}
