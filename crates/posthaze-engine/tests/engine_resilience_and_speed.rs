mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

use common::{MockResponse, parse_byte_range, spawn_mock_server, spawn_raw_server};
use posthaze_engine::{DownloadEngine, DownloadRequest, EngineError, FailureKind};

#[tokio::test]
async fn honors_retry_after_on_429_and_recovers() {
    let payload: Vec<u8> = (0..256 * 1024).map(|i| (i % 241) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_srv = Arc::new(payload);
    let get_attempts = Arc::new(AtomicUsize::new(0));
    let get_attempts_srv = Arc::clone(&get_attempts);

    let base_url = spawn_mock_server(move |method, path, _| {
        if path != "/rate-limited.bin" {
            return MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            };
        }
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
        let attempt = get_attempts_srv.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            MockResponse {
                status: "429 Too Many Requests",
                headers: vec![
                    ("Content-Length".into(), "0".into()),
                    ("Retry-After".into(), "1".into()),
                ],
                body: Vec::new(),
            }
        } else {
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), payload_srv.len().to_string()),
                    (
                        "Content-Range".into(),
                        format!("bytes 0-{}/{}", payload_srv.len() - 1, payload_srv.len()),
                    ),
                ],
                body: payload_srv.as_ref().clone(),
            }
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/rate-limited.bin"), dir.path());
    req.max_workers = 1;

    let start = Instant::now();
    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("should recover after honoring Retry-After");
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_millis(900),
        "expected Retry-After: 1 to delay >= ~1s, elapsed: {elapsed:?}"
    );
    assert_eq!(get_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        expected_hash
    );
}

#[tokio::test]
async fn worker_failure_isolation_completes_when_single_worker_hits_403() {
    // 8 MiB file with max_workers = 4 (2 MiB each). The very first GET for Segment 1
    // ([2 MiB, 4 MiB)) returns non-transient 403 Forbidden, forcing that worker to retire
    // and release its segment instead of aborting the download; surviving workers claim and finish it.
    let payload: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_srv = Arc::new(payload);
    let fail_seg1_once = Arc::new(AtomicBool::new(true));
    let fail_seg1_srv = Arc::clone(&fail_seg1_once);

    let base_url = spawn_raw_server(move |method, path, range_header, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let fail_seg1_srv = Arc::clone(&fail_seg1_srv);
        async move {
            if path != "/capped.bin" {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                return;
            }
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let (start, end) =
                parse_byte_range(range_header.as_deref().unwrap_or("bytes=0-"), payload_srv.len());
            if start == 2 * 1024 * 1024 && fail_seg1_srv.swap(false, Ordering::SeqCst) {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = write_half.shutdown().await;
                return;
            }

            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            if write_half.write_all(hdr.as_bytes()).await.is_ok() {
                for chunk in slice.chunks(256 * 1024) {
                    if write_half.write_all(chunk).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/capped.bin"), dir.path());
    req.max_workers = 4;

    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("surviving workers should finish segment abandoned by retired worker");

    assert!(!fail_seg1_once.load(Ordering::SeqCst));
    assert_eq!(outcome.bytes_downloaded, 8 * 1024 * 1024);
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        expected_hash
    );
}

#[tokio::test]
async fn slow_worker_shedding_reconnects_degraded_socket() {
    // 8 MiB file with 2 workers: Worker 0's first GET stalls (~10 KiB/s), while Worker 1 and
    // any reconnected GET for Segment 0 run at full speed. Shedding should drop the slow socket
    // around ~2s and finish well below 5s.
    let payload: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 239) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_srv = Arc::new(payload);
    let seg0_reqs = Arc::new(AtomicUsize::new(0));
    let seg0_reqs_srv = Arc::clone(&seg0_reqs);

    let base_url = spawn_raw_server(move |method, path, range_header, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let seg0_reqs_srv = Arc::clone(&seg0_reqs_srv);
        async move {
            if path != "/shed.bin" {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                return;
            }
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let (start, end) =
                parse_byte_range(range_header.as_deref().unwrap_or("bytes=0-"), payload_srv.len());
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            if write_half.write_all(hdr.as_bytes()).await.is_err() {
                return;
            }

            if start < 1024 * 1024 && seg0_reqs_srv.fetch_add(1, Ordering::SeqCst) == 0 {
                // Keep socket 0 alive very slowly so best_worker_bps is driven by Worker 1 (delayed
                // briefly so Worker 0 doesn't finish before Worker 1 starts, keeping both active).
                for chunk in slice.chunks(4 * 1024) {
                    if write_half.write_all(chunk).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(400)).await;
                }
            } else {
                // Throttle Worker 1 slightly (> 2.5s total? No, Worker 1 just needs to be active for
                // >= 2s while Worker 0 is slow, so 300ms * 10 chunks = ~3s at ~1.3 MiB/s vs 10 KiB/s).
                for chunk in slice.chunks(400 * 1024) {
                    if write_half.write_all(chunk).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/shed.bin"), dir.path());
    req.max_workers = 2;

    let start = Instant::now();
    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("shedding should recover from degraded socket");
    let elapsed = start.elapsed();

    assert!(
        seg0_reqs.load(Ordering::SeqCst) >= 2,
        "expected Worker 0 socket to be shed and reconnected at least once, got {} requests",
        seg0_reqs.load(Ordering::SeqCst)
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "expected shedding to finish well under 10s, took {elapsed:?}"
    );
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        expected_hash
    );
}

#[tokio::test]
async fn probe_via_get_and_preflight_probe_avoid_head_round_trips() {
    let payload: Vec<u8> = (0..6 * 1024 * 1024).map(|i| (i % 199) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_srv = Arc::new(payload);
    let head_count = Arc::new(AtomicUsize::new(0));
    let get_count = Arc::new(AtomicUsize::new(0));
    let head_srv = Arc::clone(&head_count);
    let get_srv = Arc::clone(&get_count);

    let base_url = spawn_mock_server(move |method, path, range| {
        if path != "/fast.bin" {
            return MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            };
        }
        if method == "HEAD" {
            head_srv.fetch_add(1, Ordering::SeqCst);
            return MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), payload_srv.len().to_string()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            };
        }
        get_srv.fetch_add(1, Ordering::SeqCst);
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
    let engine = DownloadEngine::new();

    // 1. probe_via_get = true sends 0 HEAD requests and streams Worker 0 from the initial GET.
    let mut req = DownloadRequest::new(format!("{base_url}/fast.bin"), dir.path());
    req.max_workers = 3;
    req.probe_via_get = true;

    let out1 = engine.download(req, None).await.unwrap();
    assert_eq!(head_count.load(Ordering::SeqCst), 0);
    assert_eq!(get_count.load(Ordering::SeqCst), 3);
    assert_eq!(
        blake3::hash(&std::fs::read(&out1.final_path).unwrap()),
        expected_hash
    );

    // 2. Pre-Flight probe reuse skips re-probing during DownloadEngine::download().
    let preflight = engine
        .probe(format!("{base_url}/fast.bin"), dir.path())
        .await
        .unwrap();
    assert_eq!(head_count.load(Ordering::SeqCst), 1);
    head_count.store(0, Ordering::SeqCst);
    get_count.store(0, Ordering::SeqCst);

    let mut req2 = DownloadRequest::new(format!("{base_url}/fast.bin"), dir.path());
    req2.max_workers = 3;
    req2.preflight_probe = Some(preflight);
    let out2 = engine.download(req2, None).await.unwrap();
    assert_eq!(head_count.load(Ordering::SeqCst), 0);
    assert_eq!(get_count.load(Ordering::SeqCst), 3);
    assert_eq!(
        blake3::hash(&std::fs::read(&out2.final_path).unwrap()),
        expected_hash
    );
}

#[tokio::test]
async fn lock_free_splits_persist_on_pause_and_resume_blake3_exact() {
    // 12 MiB file with 2 workers; Worker 0 is throttled after 1 MiB so Worker 1 steals dynamically
    // without blocking on SQLite, then we pause and verify SQLite persisted >= 3 segments.
    let payload: Vec<u8> = (0..12 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_srv = Arc::new(payload);
    let slow_first = Arc::new(AtomicBool::new(true));
    let slow_first_srv = Arc::clone(&slow_first);
    let req_idx = Arc::new(AtomicUsize::new(0));
    let req_idx_srv = Arc::clone(&req_idx);

    let base_url = spawn_raw_server(move |method, path, range_header, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let slow_first_srv = Arc::clone(&slow_first_srv);
        let req_idx_srv = Arc::clone(&req_idx_srv);
        async move {
            if path != "/split-pause.bin" {
                let _ = write_half
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                return;
            }
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"sp-v1\"\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }

            let idx = req_idx_srv.fetch_add(1, Ordering::SeqCst);
            let (start, end) =
                parse_byte_range(range_header.as_deref().unwrap_or("bytes=0-"), payload_srv.len());
            let slice = &payload_srv[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"sp-v1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            if write_half.write_all(hdr.as_bytes()).await.is_err() {
                return;
            }
            let is_slow = idx == 0 && slow_first_srv.load(Ordering::Relaxed);
            for chunk in slice.chunks(128 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                if is_slow {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(dir.path().join("state.db")).await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/split-pause.bin"), dir.path());
    req.max_workers = 2;

    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            {
                let snap = rx.borrow();
                if snap.segments.len() >= 3 && snap.bytes_downloaded >= 7 * 1024 * 1024 {
                    break;
                }
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("expected dynamic split to occur before pause");

    handle.pause();
    let err = handle.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::Paused));

    let persisted_segs = engine.list_segments(&id).await.unwrap();
    assert!(
        persisted_segs.len() >= 3,
        "expected pause finalizer to upsert split segments, got {}",
        persisted_segs.len()
    );

    slow_first.store(false, Ordering::Relaxed);
    let mut resumed = engine.resume(&id).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        expected_hash
    );
}

#[cfg(windows)]
#[tokio::test]
async fn disk_preflight_rejects_excessive_content_length_before_allocating() {
    let base_url = spawn_mock_server(move |method, _, _| {
        if method == "HEAD" {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), "10000000000000000".into()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            }
        } else {
            MockResponse {
                status: "500 Internal Server Error",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            }
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();
    let err = engine
        .download(
            DownloadRequest::new(format!("{base_url}/huge.bin"), dir.path()),
            None,
        )
        .await
        .expect_err("10 PB file must fail disk preflight");

    assert!(matches!(err, EngineError::InsufficientDiskSpace { .. }));
    assert_eq!(err.failure_kind(), FailureKind::NonResumable);
    assert!(!dir.path().join("huge.bin.part").exists());
}
