mod e2e_common;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use e2e_common::{
    MockResponse, blake3_hex, create_pre_0010_db, deterministic_payload, file_blake3_hex,
    parse_byte_range, spawn_http_proxy_server, spawn_keepalive_range_server, spawn_mock_server,
    spawn_raw_server,
};
use posthaze_engine::writer::PartFileWriter;
use posthaze_engine::{
    CollisionPolicy, DeleteDefaultAction, DownloadEngine, DownloadId, DownloadRequest,
    DownloadState, EngineError, EngineEvent, ProxyMode, Settings, categorize_subfolder,
    disambiguate_output_path, resolve_target_path,
};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};

// ============================================================================
// EA1: Multi-segment, Single-stream, PartFileWriter, Dynamic Splitting
// ============================================================================

#[tokio::test]
async fn tier1_ea1_multi_segment_range_download_matches_blake3() {
    let payload = Arc::new(deterministic_payload(6 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let range_gets = Arc::new(AtomicUsize::new(0));

    let payload_srv = Arc::clone(&payload);
    let gets_srv = Arc::clone(&range_gets);
    let base_url = spawn_mock_server(move |method, path, range| {
        if path != "/multi.bin" {
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
        gets_srv.fetch_add(1, Ordering::SeqCst);
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
    let mut req = DownloadRequest::new(format!("{base_url}/multi.bin"), dir.path());
    req.max_workers = 3;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 3);
    assert!(range_gets.load(Ordering::SeqCst) >= 3);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea1_single_stream_non_range_download_matches_blake3() {
    let payload = Arc::new(deterministic_payload(1536 * 1024));
    let expected_hash = blake3_hex(&payload);
    let get_count = Arc::new(AtomicUsize::new(0));

    let payload_srv = Arc::clone(&payload);
    let gets_srv = Arc::clone(&get_count);
    let base_url = spawn_mock_server(move |method, path, range| {
        if path != "/stream.bin" {
            return MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            };
        }
        if method == "GET" {
            if range == Some("bytes=0-0") {
                // Probe fallback sends `Range: bytes=0-0` when HEAD returns `Accept-Ranges: none`.
                return MockResponse {
                    status: "200 OK",
                    headers: vec![
                        ("Content-Length".into(), payload_srv.len().to_string()),
                        ("Accept-Ranges".into(), "none".into()),
                    ],
                    body: payload_srv.as_ref().clone(),
                };
            }
            assert!(range.is_none(), "single-stream transfer GET must not send Range");
            gets_srv.fetch_add(1, Ordering::SeqCst);
        }
        MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_srv.len().to_string()),
                ("Accept-Ranges".into(), "none".into()),
            ],
            body: if method == "HEAD" {
                Vec::new()
            } else {
                payload_srv.as_ref().clone()
            },
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();
    let mut req = DownloadRequest::new(format!("{base_url}/stream.bin"), dir.path());
    req.max_workers = 8;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(!outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 1);
    assert_eq!(get_count.load(Ordering::SeqCst), 1);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea1_part_file_writer_spawn_and_interleaved_offsets() {
    let dir = TempDir::new().unwrap();
    let part_path = dir.path().join("interleaved.bin.part");
    let final_path = dir.path().join("interleaved.bin");

    let chunk_size = 512 * 1024usize;
    let total = chunk_size * 5 + 12345;
    let payload = deterministic_payload(total);

    let writer = PartFileWriter::spawn(part_path.clone(), Some(total as u64)).unwrap();

    // Write chunks in scrambled non-sequential order across 512 KiB boundaries.
    let offsets = [
        chunk_size * 3,
        0,
        chunk_size * 5,
        chunk_size * 2,
        chunk_size,
        chunk_size * 4,
    ];
    for offset in offsets {
        let end = (offset + chunk_size).min(total);
        writer
            .write_at(offset as u64, Bytes::copy_from_slice(&payload[offset..end]))
            .await
            .unwrap();
    }

    writer.finish(&final_path).await.unwrap();
    assert!(!part_path.exists());
    assert_eq!(file_blake3_hex(&final_path), blake3_hex(&payload));
}

#[tokio::test]
async fn tier1_ea1_part_file_writer_spawn_with_sync_durability() {
    let dir = TempDir::new().unwrap();
    let part_path = dir.path().join("synced.bin.part");
    let final_path = dir.path().join("synced.bin");

    let payload = deterministic_payload(2 * 1024 * 1024 + 777);
    let writer =
        PartFileWriter::spawn_with_sync(part_path.clone(), Some(payload.len() as u64), true)
            .unwrap();

    for (idx, slice) in payload.chunks(512 * 1024).enumerate().rev() {
        let offset = (idx * 512 * 1024) as u64;
        writer
            .write_at(offset, Bytes::copy_from_slice(slice))
            .await
            .unwrap();
    }

    writer.finish(&final_path).await.unwrap();
    assert!(!part_path.exists());
    assert_eq!(file_blake3_hex(&final_path), blake3_hex(&payload));
}

#[tokio::test]
async fn tier1_ea1_dynamic_segment_splitting_work_steals_slow_worker() {
    let total_len = 8 * 1024 * 1024usize;
    let payload = Arc::new(deterministic_payload(total_len));
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
            if write_half.write_all(hdr.as_bytes()).await.is_err() {
                return;
            }
            let slow = start == 0;
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
    let mut req = DownloadRequest::new(format!("{base_url}/steal.bin"), dir.path());
    req.max_workers = 2;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(
        outcome.splits_performed >= 1,
        "expected dynamic split, got {}",
        outcome.splits_performed
    );
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea1_backpressure_writer_handles_24_concurrent_512kib_chunks() {
    let dir = TempDir::new().unwrap();
    let part_path = dir.path().join("bp.bin.part");
    let final_path = dir.path().join("bp.bin");

    let chunk_size = 512 * 1024usize;
    let total = chunk_size * 24; // 12 MiB (> 16 bounded channel slots)
    let payload = Arc::new(deterministic_payload(total));
    let expected_hash = blake3_hex(&payload);

    let writer = Arc::new(PartFileWriter::spawn(part_path, Some(total as u64)).unwrap());

    let mut tasks = Vec::new();
    for seg in 0..4usize {
        let w = Arc::clone(&writer);
        let p = Arc::clone(&payload);
        tasks.push(tokio::spawn(async move {
            for idx in 0..6usize {
                let offset = (seg * 6 + idx) * chunk_size;
                w.write_at(
                    offset as u64,
                    Bytes::copy_from_slice(&p[offset..offset + chunk_size]),
                )
                .await
                .unwrap();
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }

    Arc::try_unwrap(writer)
        .expect("all writer tasks finished")
        .finish(&final_path)
        .await
        .unwrap();
    assert_eq!(file_blake3_hex(&final_path), expected_hash);
}

// ============================================================================
// EA2: Lifecycle Control & FIFO Queue Scheduling
// ============================================================================

#[tokio::test]
async fn tier1_ea2_start_and_wait_completes_download() {
    let payload = Arc::new(deterministic_payload(512 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/a.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().state,
        DownloadState::Completed
    );
}

#[tokio::test]
async fn tier1_ea2_enqueue_paused_stages_without_network_then_resumes() {
    let payload = Arc::new(deterministic_payload(256 * 1024));
    let expected_hash = blake3_hex(&payload);
    let hits = Arc::new(AtomicUsize::new(0));

    let payload_srv = Arc::clone(&payload);
    let hits_srv = Arc::clone(&hits);
    let base_url = spawn_mock_server(move |method, _, _| {
        hits_srv.fetch_add(1, Ordering::SeqCst);
        MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
            body: if method == "HEAD" {
                Vec::new()
            } else {
                payload_srv.as_ref().clone()
            },
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/staged.bin"), dir.path());
    req.headers = vec![("X-Stage".into(), "1".into())];

    let id = engine.enqueue_paused(req).await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let rec = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(rec.state, DownloadState::Paused);
    assert_eq!(rec.headers, vec![("X-Stage".into(), "1".into())]);

    let mut handle = engine.resume(&id).await.unwrap();
    let outcome = handle.wait().await.unwrap();
    assert!(hits.load(Ordering::SeqCst) >= 2);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea2_pause_and_resume_persists_checkpoint_and_completes() {
    let payload = Arc::new(deterministic_payload(4 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let slow_flag = Arc::new(AtomicBool::new(true));

    let payload_srv = Arc::clone(&payload);
    let slow_srv = Arc::clone(&slow_flag);
    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let slow_srv = Arc::clone(&slow_srv);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"p1\"\r\nConnection: close\r\n\r\n",
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"p1\"\r\nConnection: close\r\n\r\n",
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

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/pausable.bin"), dir.path());
    req.max_workers = 2;

    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        rx.wait_for(|s| s.bytes_downloaded >= 512 * 1024)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    engine.pause(&id).await.unwrap();
    assert!(matches!(handle.wait().await, Err(EngineError::Paused)));

    let paused_rec = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(paused_rec.state, DownloadState::Paused);
    assert!(paused_rec.bytes_downloaded >= 512 * 1024);
    assert!(paused_rec.part_path.as_ref().unwrap().exists());

    slow_flag.store(false, Ordering::Relaxed);
    let mut resumed = engine.resume(&id).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea2_cancel_removes_part_file_and_marks_cancelled() {
    let payload = Arc::new(deterministic_payload(4 * 1024 * 1024));
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
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/cancel.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        rx.wait_for(|s| s.state == DownloadState::Active && s.bytes_downloaded > 0)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    let part_path = engine.get_download(&id).await.unwrap().unwrap().part_path.unwrap();
    assert!(part_path.exists());

    engine.cancel(&id).await.unwrap();
    assert!(matches!(handle.wait().await, Err(EngineError::Cancelled)));
    assert!(!part_path.exists());
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().state,
        DownloadState::Cancelled
    );
}

#[tokio::test]
async fn tier1_ea2_delete_with_and_without_file_removal() {
    let payload = Arc::new(deterministic_payload(32 * 1024));
    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let mut h1 = engine
        .start(DownloadRequest::new(format!("{base_url}/keep.bin"), dir.path()))
        .await
        .unwrap();
    let id1 = h1.id().clone();
    let out1 = h1.wait().await.unwrap().final_path;

    let mut h2 = engine
        .start(DownloadRequest::new(format!("{base_url}/remove.bin"), dir.path()))
        .await
        .unwrap();
    let id2 = h2.id().clone();
    let out2 = h2.wait().await.unwrap().final_path;

    engine.delete(&id1, false).await.unwrap();
    assert!(engine.get_download(&id1).await.unwrap().is_none());
    assert!(out1.exists(), "delete(false) must keep file on disk");

    engine.delete(&id2, true).await.unwrap();
    assert!(engine.get_download(&id2).await.unwrap().is_none());
    assert!(!out2.exists(), "delete(true) must delete file from disk");
}

#[tokio::test]
async fn tier1_ea2_restart_fresh_clears_old_progress_and_redownloads() {
    let v1 = Arc::new(deterministic_payload(2 * 1024 * 1024));
    let v2 = Arc::new(deterministic_payload(2 * 1024 * 1024 + 555));
    let v2_hash = blake3_hex(&v2);
    let use_v2 = Arc::new(AtomicBool::new(false));

    let v1_srv = Arc::clone(&v1);
    let v2_srv = Arc::clone(&v2);
    let flag_srv = Arc::clone(&use_v2);
    let base_url = spawn_raw_server(move |req, mut write_half| {
        let active_payload = if flag_srv.load(Ordering::SeqCst) {
            Arc::clone(&v2_srv)
        } else {
            Arc::clone(&v1_srv)
        };
        let slow = !flag_srv.load(Ordering::SeqCst);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    active_payload.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let (start, end) = req
                .range
                .as_deref()
                .map(|r| parse_byte_range(r, active_payload.len()))
                .unwrap_or((0, active_payload.len() - 1));
            let slice = &active_payload[start..=end];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                active_payload.len()
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
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/fresh.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        rx.wait_for(|s| s.bytes_downloaded >= 256 * 1024)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    engine.pause(&id).await.unwrap();
    let _ = handle.wait().await;

    use_v2.store(true, Ordering::SeqCst);
    let mut restarted = engine.restart_fresh(&id).await.unwrap();
    let outcome = restarted.wait().await.unwrap();
    assert_eq!(outcome.bytes_downloaded, v2.len() as u64);
    assert_eq!(file_blake3_hex(&outcome.final_path), v2_hash);
}

#[tokio::test]
async fn tier1_ea2_fifo_max_concurrent_downloads_scheduling() {
    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 2;
    engine.update_settings(settings).await.unwrap();

    let payload = Arc::new(deterministic_payload(32 * 1024));
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());

    let payload_srv = Arc::clone(&payload);
    let order_srv = Arc::clone(&order);
    let active_srv = Arc::clone(&active);
    let max_srv = Arc::clone(&max_seen);
    let gate_srv = Arc::clone(&gate);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let order_srv = Arc::clone(&order_srv);
        let active_srv = Arc::clone(&active_srv);
        let max_srv = Arc::clone(&max_srv);
        let gate_srv = Arc::clone(&gate_srv);
        async move {
            if req.method == "HEAD" {
                {
                    let mut g = order_srv.lock().await;
                    if !g.contains(&req.path) {
                        g.push(req.path.clone());
                    }
                }
                let cur = active_srv.fetch_add(1, Ordering::SeqCst) + 1;
                max_srv.fetch_max(cur, Ordering::SeqCst);
                gate_srv.notified().await;
                active_srv.fetch_sub(1, Ordering::SeqCst);

                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                    payload_srv.len()
                );
                let _ = write_half.write_all(hdr.as_bytes()).await;
                let _ = write_half.shutdown().await;
                return;
            }
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            let _ = write_half.write_all(&payload_srv).await;
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut handles = Vec::new();
    for i in 1..=4 {
        let h = engine
            .start(DownloadRequest::new(
                format!("{base_url}/fifo-{i}.bin"),
                dir.path(),
            ))
            .await
            .unwrap();
        handles.push(h);
    }

    tokio::time::timeout(Duration::from_secs(3), async {
        while active.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(handles[2].subscribe().borrow().state, DownloadState::Queued);
    assert_eq!(handles[3].subscribe().borrow().state, DownloadState::Queued);

    let releaser_gate = Arc::clone(&gate);
    let releaser = tokio::spawn(async move {
        for _ in 0..20 {
            releaser_gate.notify_waiters();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    for mut h in handles {
        let outcome = h.wait().await.unwrap();
        assert_eq!(file_blake3_hex(&outcome.final_path), blake3_hex(&payload));
    }
    releaser.abort();

    assert!(max_seen.load(Ordering::SeqCst) <= 2);
    assert_eq!(
        *order.lock().await,
        vec!["/fifo-1.bin", "/fifo-2.bin", "/fifo-3.bin", "/fifo-4.bin"]
    );
}

// ============================================================================
// EA3: HTTP, Redirects, Custom Headers, Proxy Routing & Settings
// ============================================================================

#[tokio::test]
async fn tier1_ea3_probe_returns_metadata_without_creating_db_or_part() {
    let base_url = spawn_mock_server(|method, path, _| {
        assert_eq!(method, "HEAD");
        if path == "/asset" {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), "4096".into()),
                    ("Accept-Ranges".into(), "bytes".into()),
                    ("ETag".into(), "\"meta-v1\"".into()),
                    ("Last-Modified".into(), "Mon, 01 Jan 2026 00:00:00 GMT".into()),
                    (
                        "Content-Disposition".into(),
                        "attachment; filename=\"installer.zip\"".into(),
                    ),
                ],
                body: Vec::new(),
            }
        } else {
            MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            }
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let info = engine
        .probe(format!("{base_url}/asset"), dir.path())
        .await
        .unwrap();

    assert_eq!(info.suggested_filename, "installer.zip");
    assert_eq!(info.disambiguated_output_path, dir.path().join("installer.zip"));
    assert_eq!(info.content_length, Some(4096));
    assert!(info.supports_ranges);
    assert_eq!(info.etag.as_deref(), Some("\"meta-v1\""));
    assert_eq!(
        info.last_modified.as_deref(),
        Some("Mon, 01 Jan 2026 00:00:00 GMT")
    );
    assert!(engine.list_downloads().await.unwrap().is_empty());
    assert!(!dir.path().join("installer.zip.part").exists());
}

#[tokio::test]
async fn tier1_ea3_probe_with_headers_and_download_send_custom_headers() {
    let payload = Arc::new(deterministic_payload(64 * 1024));
    let expected_hash = blake3_hex(&payload);
    let seen_tokens = Arc::new(Mutex::new(Vec::<(String, Option<String>)>::new()));

    let payload_srv = Arc::clone(&payload);
    let tokens_srv = Arc::clone(&seen_tokens);
    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let tokens_srv = Arc::clone(&tokens_srv);
        async move {
            let auth = req.header("authorization").map(str::to_string);
            tokens_srv.lock().await.push((req.method.clone(), auth));
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
    let headers = vec![("Authorization".to_string(), "Bearer secret-123".to_string())];

    let info = engine
        .probe_with_headers(format!("{base_url}/auth.bin"), dir.path(), &headers)
        .await
        .unwrap();
    assert_eq!(info.content_length, Some(payload.len() as u64));

    let mut req = DownloadRequest::new(format!("{base_url}/auth.bin"), dir.path());
    req.headers = headers;
    let outcome = engine.download(req, None).await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);

    let captured = seen_tokens.lock().await;
    assert!(captured.len() >= 3);
    assert!(
        captured
            .iter()
            .all(|(_, tok)| tok.as_deref() == Some("Bearer secret-123"))
    );
}

#[tokio::test]
async fn tier1_ea3_follows_http_302_redirect_and_records_final_url() {
    let payload = Arc::new(deterministic_payload(128 * 1024));
    let expected_hash = blake3_hex(&payload);

    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_mock_server(move |method, path, _| match path {
        "/redirect" => MockResponse {
            status: "302 Found",
            headers: vec![
                ("Location".into(), "/cdn/resolved.bin".into()),
                ("Content-Length".into(), "0".into()),
            ],
            body: Vec::new(),
        },
        "/cdn/resolved.bin" => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_srv.len().to_string()),
                ("Accept-Ranges".into(), "none".into()),
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
    let engine = DownloadEngine::in_memory().await.unwrap();
    let expected_final = format!("{base_url}/cdn/resolved.bin");

    let probed = engine
        .probe(format!("{base_url}/redirect"), dir.path())
        .await
        .unwrap();
    assert_eq!(probed.final_url, expected_final);
    assert_eq!(probed.suggested_filename, "resolved.bin");

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/redirect"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(outcome.final_url, expected_final);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().final_url.as_deref(),
        Some(expected_final.as_str())
    );
}

#[tokio::test]
async fn tier1_ea3_proxy_mode_manual_routes_through_http_proxy() {
    let payload = Arc::new(deterministic_payload(128 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let target_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![
            ("Content-Length".into(), payload_srv.len().to_string()),
            ("Accept-Ranges".into(), "none".into()),
        ],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let (proxy_url, proxy_hits) = spawn_http_proxy_server(&target_url).await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        proxy_mode: ProxyMode::Manual,
        proxy_url: proxy_url.clone(),
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let info = engine
        .probe(format!("{target_url}/proxied.bin"), dir.path())
        .await
        .unwrap();
    assert_eq!(info.content_length, Some(payload.len() as u64));

    let outcome = engine
        .download(
            DownloadRequest::new(format!("{target_url}/proxied.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();

    assert!(proxy_hits.load(Ordering::SeqCst) >= 3);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea3_proxy_mode_none_and_settings_persist_across_reopen() {
    let payload = Arc::new(deterministic_payload(32 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let custom = Settings {
        default_output_dir: dir.path().join("out"),
        max_workers: 5,
        max_concurrent_downloads: 2,
        sync_on_complete: true,
        proxy_mode: ProxyMode::None,
        proxy_url: String::new(),
        download_speed_limit_kib: 2048,
        auto_categorize_folders: true,
        collision_policy: CollisionPolicy::Overwrite,
        delete_action_default: DeleteDefaultAction::Remove,
        auto_retry_count: 2,
        ..Settings::default()
    };

    {
        let engine = DownloadEngine::open(&db_path).await.unwrap();
        engine.update_settings(custom.clone()).await.unwrap();
    }

    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(reopened.get_settings().await.unwrap(), custom);
    let outcome = reopened
        .download(
            DownloadRequest::new(format!("{base_url}/direct.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
#[ignore = "post-M2: F6 proxy-only hot-swap"]
async fn tier1_ea3_non_proxy_settings_update_reuses_keepalive_connection() {
    let payload = Arc::new(deterministic_payload(64 * 1024));
    let expected_hash = blake3_hex(&payload);
    let (base_url, conn_count, req_count) = spawn_keepalive_range_server(payload).await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let mut req1 = DownloadRequest::new(format!("{base_url}/k1.bin"), dir.path());
    req1.max_workers = 1;
    let out1 = engine.download(req1, None).await.unwrap();
    assert_eq!(file_blake3_hex(&out1.final_path), expected_hash);
    let conns_after_first = conn_count.load(Ordering::SeqCst);
    assert_eq!(conns_after_first, 1);

    // Update unrelated settings (no proxy_mode/proxy_url change).
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 4;
    settings.download_speed_limit_kib = 0;
    engine.update_settings(settings).await.unwrap();

    let mut req2 = DownloadRequest::new(format!("{base_url}/k2.bin"), dir.path());
    req2.max_workers = 1;
    let out2 = engine.download(req2, None).await.unwrap();
    assert_eq!(file_blake3_hex(&out2.final_path), expected_hash);

    assert!(req_count.load(Ordering::SeqCst) >= 4);
    assert_eq!(
        conn_count.load(Ordering::SeqCst),
        1,
        "unrelated settings update must preserve warm keep-alive TCP connection"
    );
}

// ============================================================================
// EA4: Progress Telemetry, Broadcast Events & Throttling
// ============================================================================

#[tokio::test]
async fn tier1_ea4_handle_subscribe_watch_tracks_progress_to_completion() {
    let payload = Arc::new(deterministic_payload(4 * 1024 * 1024));
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
            for chunk in slice.chunks(128 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(18)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/watch.bin"), dir.path());
    req.max_workers = 2;

    let mut handle = engine.start(req).await.unwrap();
    let mut rx = handle.subscribe();
    let mut saw_active_segments = false;
    let mut prev_bytes = 0u64;

    let watcher = tokio::spawn(async move {
        loop {
            let snap = rx.borrow().clone();
            assert!(snap.bytes_downloaded >= prev_bytes);
            prev_bytes = snap.bytes_downloaded;
            if snap.state == DownloadState::Active && snap.segments.len() >= 2 {
                saw_active_segments = true;
            }
            if snap.state == DownloadState::Completed {
                return (saw_active_segments, snap);
            }
            if rx.changed().await.is_err() {
                return (saw_active_segments, rx.borrow().clone());
            }
        }
    });

    let outcome = handle.wait().await.unwrap();
    let (saw_segs, final_snap) = watcher.await.unwrap();
    assert!(saw_segs);
    assert_eq!(final_snap.state, DownloadState::Completed);
    assert_eq!(final_snap.bytes_downloaded, payload.len() as u64);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea4_subscribe_events_receives_all_event_variants() {
    let payload = Arc::new(deterministic_payload(192 * 1024));
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
                for chunk in payload_srv.chunks(64 * 1024) {
                    let _ = write_half.write_all(chunk).await;
                    tokio::time::sleep(Duration::from_millis(35)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut rx = engine.subscribe_events();

    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 4;
    engine.update_settings(settings).await.unwrap();

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/ev.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    handle.wait().await.unwrap();

    let mut flags = [false; 6];
    while let Ok(ev) = rx.try_recv() {
        match ev {
            EngineEvent::SettingsUpdated(s) if s.max_concurrent_downloads == 4 => flags[0] = true,
            EngineEvent::QueueUpdated => flags[1] = true,
            EngineEvent::StateChanged {
                id: eid,
                state: DownloadState::Probing,
                ..
            } if eid == id => flags[2] = true,
            EngineEvent::StateChanged {
                id: eid,
                state: DownloadState::Active,
                ..
            } if eid == id => flags[3] = true,
            EngineEvent::ProgressUpdated { id: eid, .. } if eid == id => flags[4] = true,
            EngineEvent::StateChanged {
                id: eid,
                state: DownloadState::Completed,
                ..
            } if eid == id => flags[5] = true,
            _ => {}
        }
    }
    assert_eq!(flags, [true; 6], "missing expected EngineEvents: {flags:?}");
}

#[tokio::test]
async fn tier1_ea4_speed_limit_throttles_transfer_duration() {
    let payload = Arc::new(deterministic_payload(128 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![
            ("Content-Length".into(), payload_srv.len().to_string()),
            ("Accept-Ranges".into(), "none".into()),
        ],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        download_speed_limit_kib: 64,
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let started = Instant::now();
    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/throttled.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();

    // 128 KiB at 64 KiB/s (with initial burst) should still require > 700ms.
    assert!(
        started.elapsed() >= Duration::from_millis(700),
        "expected throttled duration >= 700ms, got {:?}",
        started.elapsed()
    );
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea4_multiple_event_subscribers_receive_independent_streams() {
    let payload = Arc::new(deterministic_payload(32 * 1024));
    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut sub1 = engine.subscribe_events();
    let mut sub2 = engine.subscribe_events();

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/multi-sub.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    handle.wait().await.unwrap();

    for sub in [&mut sub1, &mut sub2] {
        let mut saw_completed = false;
        while let Ok(ev) = sub.try_recv() {
            if matches!(
                ev,
                EngineEvent::StateChanged {
                    id: ref eid,
                    state: DownloadState::Completed,
                    ..
                } if *eid == id
            ) {
                saw_completed = true;
            }
        }
        assert!(saw_completed);
    }
}

#[tokio::test]
async fn tier1_ea4_terminal_events_delivered_on_pause_and_cancel() {
    let payload = Arc::new(deterministic_payload(2 * 1024 * 1024));
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
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut rx = engine.subscribe_events();

    let mut h_pause = engine
        .start(DownloadRequest::new(format!("{base_url}/p.bin"), dir.path()))
        .await
        .unwrap();
    let id_pause = h_pause.id().clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        h_pause
            .subscribe()
            .wait_for(|s| s.state == DownloadState::Active && s.bytes_downloaded > 0)
            .await
            .unwrap();
    })
    .await
    .unwrap();
    engine.pause(&id_pause).await.unwrap();
    let _ = h_pause.wait().await;

    let mut h_cancel = engine
        .start(DownloadRequest::new(format!("{base_url}/c.bin"), dir.path()))
        .await
        .unwrap();
    let id_cancel = h_cancel.id().clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        h_cancel
            .subscribe()
            .wait_for(|s| s.state == DownloadState::Active)
            .await
            .unwrap();
    })
    .await
    .unwrap();
    engine.cancel(&id_cancel).await.unwrap();
    let _ = h_cancel.wait().await;

    let mut saw_paused = false;
    let mut saw_cancelled = false;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            EngineEvent::StateChanged {
                id,
                state: DownloadState::Paused,
                ..
            } if id == id_pause => saw_paused = true,
            EngineEvent::StateChanged {
                id,
                state: DownloadState::Cancelled,
                ..
            } if id == id_cancel => saw_cancelled = true,
            _ => {}
        }
    }
    assert!(saw_paused);
    assert!(saw_cancelled);
}

#[tokio::test]
#[ignore = "post-M2: F7 unchanged-tick suppression"]
async fn tier1_ea4_suppresses_unchanged_progress_ticks_during_stall() {
    let payload = Arc::new(deterministic_payload(128 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);
    let release_tail = Arc::new(Notify::new());
    let release_srv = Arc::clone(&release_tail);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let release_srv = Arc::clone(&release_srv);
        async move {
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if req.method != "HEAD" {
                let _ = write_half.write_all(&payload_srv[..64 * 1024]).await;
                let _ = write_half.flush().await;
                release_srv.notified().await;
                let _ = write_half.write_all(&payload_srv[64 * 1024..]).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut events = engine.subscribe_events();

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/stall.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();

    // Wait until the first 64 KiB arrives, then hold steady for 700 ms.
    tokio::time::timeout(Duration::from_secs(5), async {
        handle
            .subscribe()
            .wait_for(|s| s.bytes_downloaded == 64 * 1024)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    // Drain events emitted up to stall start.
    while events.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(700)).await;

    let mut unchanged_ticks = 0usize;
    while let Ok(ev) = events.try_recv() {
        if let EngineEvent::ProgressUpdated {
            id: eid,
            snapshot,
        } = ev
            && eid == id
            && snapshot.state == DownloadState::Active
            && snapshot.bytes_downloaded == 64 * 1024
        {
            unchanged_ticks += 1;
        }
    }

    release_tail.notify_waiters();
    let outcome = handle.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
    assert!(
        unchanged_ticks <= 2,
        "expected suppressed unchanged progress ticks during 700ms stall, got {unchanged_ticks}"
    );
}

// ============================================================================
// EA5: SQLite Persistence, Migrations & Crash Recovery
// ============================================================================

// Note: SQLite `PRAGMA synchronous` is per-connection rather than persisted in the DB header,
// and `DownloadStore` is `pub(crate)`, so an external `sqlx` read connection observes its own
// default `synchronous` setting. This opaque-box test verifies persistent `journal_mode = wal`,
// required schema tables, and concurrent non-blocking WAL reads/writes while the engine is open.
#[tokio::test]
async fn tier1_ea5_open_fresh_db_configures_wal_and_schema() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("nested").join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();
    assert!(db_path.exists());

    // Exercise engine write/read on the live WAL database while an external reader inspects it.
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_workers = 5;
    engine.update_settings(settings).await.unwrap();

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&db_path))
        .await
        .unwrap();

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode;")
        .fetch_one(&pool)
        .await
        .unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let download_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloads")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert!(tables.contains(&"downloads".to_string()));
    assert!(tables.contains(&"segments".to_string()));
    assert!(tables.contains(&"settings".to_string()));
    assert_eq!(download_rows, 0);
    assert_eq!(engine.get_settings().await.unwrap().max_workers, 5);
    pool.close().await;
    drop(engine);
}

#[tokio::test]
async fn tier1_ea5_in_memory_engine_supports_downloads_and_queries() {
    let payload = Arc::new(deterministic_payload(64 * 1024));
    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/mem.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(file_blake3_hex(&outcome.final_path), blake3_hex(&payload));
    assert_eq!(engine.list_downloads().await.unwrap().len(), 1);
    assert_eq!(engine.list_segments(&id).await.unwrap().len(), 1);
    assert_eq!(engine.list_segment_counts().await.unwrap().get(&id), Some(&1));
}

#[tokio::test]
async fn tier1_ea5_list_segments_and_list_segment_counts_reflect_multi_segment() {
    let payload = Arc::new(deterministic_payload(6 * 1024 * 1024));
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
    let mut req = DownloadRequest::new(format!("{base_url}/segs.bin"), dir.path());
    req.max_workers = 3;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    handle.wait().await.unwrap();

    let segs = engine.list_segments(&id).await.unwrap();
    assert_eq!(segs.len(), 3);
    assert!(segs.iter().all(|s| s.completed));
    let counts = engine.list_segment_counts().await.unwrap();
    assert_eq!(counts.get(&id), Some(&3));
}

#[tokio::test]
async fn tier1_ea5_crash_simulation_and_checkpoint_resume_recovers_byte_exact() {
    let payload = Arc::new(deterministic_payload(10 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let stall_tail = Arc::new(AtomicBool::new(true));
    let entered_stall = Arc::new(AtomicBool::new(false));
    let stall_wake = Arc::new(Notify::new());

    let payload_srv = Arc::clone(&payload);
    let stall_srv = Arc::clone(&stall_tail);
    let entered_srv = Arc::clone(&entered_stall);
    let wake_srv = Arc::clone(&stall_wake);
    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let stall_srv = Arc::clone(&stall_srv);
        let entered_srv = Arc::clone(&entered_srv);
        let wake_srv = Arc::clone(&wake_srv);
        async move {
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"cp1\"\r\nConnection: close\r\n\r\n",
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"cp1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
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
    let db_path = dir.path().join("state.db");

    let id = {
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
            let id = rt.block_on(async {
                let engine = DownloadEngine::open(&db).await.unwrap();
                let mut req = DownloadRequest::new(format!("{url}/cp.bin"), &out);
                req.max_workers = 1;
                let handle = engine.start(req).await.unwrap();
                let id = handle.id().clone();

                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if entered.load(Ordering::Acquire)
                            && let Some(rec) = engine.get_download(&id).await.unwrap()
                            && rec.bytes_downloaded >= 8 * 1024 * 1024
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();

                drop(handle);
                drop(engine);
                id
            });
            rt.shutdown_timeout(Duration::from_millis(200));
            id
        })
        .await
        .unwrap()
    };

    stall_tail.store(false, Ordering::Release);
    stall_wake.notify_waiters();
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let reconciled = reopened.get_download(&id).await.unwrap().unwrap();
    assert_eq!(reconciled.state, DownloadState::Paused);
    assert!(reconciled.bytes_downloaded >= 8 * 1024 * 1024);

    let mut resumed = reopened.resume(&id).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier1_ea5_pre_0010_synthetic_db_upgrades_cleanly_preserving_rows() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("legacy.db");
    let legacy_id_str = "11111111-2222-4333-8444-555555555555";
    create_pre_0010_db(&db_path, Some(legacy_id_str)).await;

    let engine = DownloadEngine::open(&db_path).await.unwrap();
    let legacy_id = DownloadId::from(legacy_id_str.to_string());

    let settings = engine.get_settings().await.unwrap();
    assert_eq!(settings.max_workers, 6);
    assert_eq!(settings.max_concurrent_downloads, 4);
    assert!(settings.sync_on_complete);

    let rec = engine.get_download(&legacy_id).await.unwrap().unwrap();
    assert_eq!(rec.state, DownloadState::Paused);
    assert_eq!(rec.bytes_downloaded, 1024);
    let segs = engine.list_segments(&legacy_id).await.unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].cursor, 1024);
}

#[tokio::test]
#[ignore = "post-M2: F9 composite index"]
async fn tier1_ea5_migration_0010_creates_composite_index_and_drops_prefix_index() {
    let dir = TempDir::new().unwrap();
    let fresh_db = dir.path().join("fresh.db");
    let upgraded_db = dir.path().join("upgraded.db");

    create_pre_0010_db(&upgraded_db, Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")).await;

    for db in [&fresh_db, &upgraded_db] {
        let engine = DownloadEngine::open(db).await.unwrap();
        drop(engine);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(db))
            .await
            .unwrap();

        let index_rows = sqlx::query(
            "SELECT name, sql FROM sqlite_master WHERE type = 'index' AND tbl_name = 'segments'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let names: Vec<String> = index_rows
            .iter()
            .map(|r| r.get::<String, _>("name"))
            .collect();
        assert!(
            !names.iter().any(|n| n == "idx_segments_download_id"),
            "legacy single-column prefix index must be dropped in {names:?}"
        );
        let has_unique_composite = index_rows.iter().any(|r| {
            let sql: Option<String> = r.get("sql");
            sql.as_deref().is_some_and(|s| {
                let low = s.to_ascii_lowercase();
                low.contains("unique")
                    && low.contains("download_id")
                    && low.contains("segment_index")
            })
        });
        assert!(
            has_unique_composite,
            "expected unique composite index on segments(download_id, segment_index)"
        );

        let plan_rows = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT cursor_offset FROM segments WHERE download_id = 'x' AND segment_index = 0",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let detail = plan_rows
            .iter()
            .map(|r| r.get::<String, _>("detail"))
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            detail.to_ascii_uppercase().contains("USING")
                && detail.to_ascii_lowercase().contains("index"),
            "expected query plan to use composite index, got: {detail}"
        );

        let ordered_plan_rows = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT segment_index FROM segments WHERE download_id = 'x' ORDER BY segment_index ASC",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let ordered_detail = ordered_plan_rows
            .iter()
            .map(|r| r.get::<String, _>("detail"))
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            ordered_detail.to_ascii_uppercase().contains("USING")
                && ordered_detail.to_ascii_lowercase().contains("index")
                && !ordered_detail.to_ascii_uppercase().contains("TEMP B-TREE"),
            "expected ordered query plan to use composite index leading with download_id without TEMP B-TREE, got: {ordered_detail}"
        );
        pool.close().await;
    }
}

// ============================================================================
// EA6: Paths, Collision Policies, Categorization, Post-Completion & DownloadId
// ============================================================================

#[test]
fn tier1_ea6_categorize_subfolder_maps_all_documented_categories() {
    assert_eq!(categorize_subfolder("bundle.ZIP"), Some("Archives"));
    assert_eq!(categorize_subfolder("disk.iso"), Some("Archives"));
    assert_eq!(categorize_subfolder("movie.Mp4"), Some("Video"));
    assert_eq!(categorize_subfolder("clip.mkv"), Some("Video"));
    assert_eq!(categorize_subfolder("song.flac"), Some("Audio"));
    assert_eq!(categorize_subfolder("voice.opus"), Some("Audio"));
    assert_eq!(categorize_subfolder("report.pdf"), Some("Documents"));
    assert_eq!(categorize_subfolder("notes.md"), Some("Documents"));
    assert_eq!(categorize_subfolder("installer.exe"), Some("Programs"));
    assert_eq!(categorize_subfolder("script.ps1"), Some("Programs"));
    assert_eq!(categorize_subfolder("raw.bin"), None);
    assert_eq!(categorize_subfolder("README"), None);
}

#[test]
fn tier1_ea6_disambiguate_output_path_avoids_existing_and_part_collisions() {
    let dir = TempDir::new().unwrap();
    let p0 = disambiguate_output_path(dir.path(), "archive.tar.gz");
    assert_eq!(p0, dir.path().join("archive.tar.gz"));

    std::fs::write(&p0, b"existing").unwrap();
    let p1 = disambiguate_output_path(dir.path(), "archive.tar.gz");
    assert_eq!(p1, dir.path().join("archive.tar (1).gz"));

    // Simulate an active download holding a `.part` file for `(1)`.
    std::fs::write(dir.path().join("archive.tar (1).gz.part"), b"partial").unwrap();
    let p2 = disambiguate_output_path(dir.path(), "archive.tar.gz");
    assert_eq!(p2, dir.path().join("archive.tar (2).gz"));
}

#[test]
fn tier1_ea6_resolve_target_path_policies_rename_overwrite_ask() {
    let dir = TempDir::new().unwrap();
    let clean = resolve_target_path(dir.path(), "item.bin", CollisionPolicy::Ask);
    assert_eq!(clean.path, dir.path().join("item.bin"));
    assert!(!clean.needs_overwrite_prompt);

    std::fs::write(dir.path().join("item.bin"), b"data").unwrap();

    let rename = resolve_target_path(dir.path(), "item.bin", CollisionPolicy::Rename);
    assert_eq!(rename.path, dir.path().join("item (1).bin"));
    assert!(!rename.needs_overwrite_prompt);

    let overwrite = resolve_target_path(dir.path(), "item.bin", CollisionPolicy::Overwrite);
    assert_eq!(overwrite.path, dir.path().join("item.bin"));
    assert!(!overwrite.needs_overwrite_prompt);

    let ask = resolve_target_path(dir.path(), "item.bin", CollisionPolicy::Ask);
    assert_eq!(ask.path, dir.path().join("item.bin"));
    assert!(ask.needs_overwrite_prompt);
}

#[tokio::test]
async fn tier1_ea6_rename_completed_file_updates_disk_and_sqlite() {
    let payload = Arc::new(deterministic_payload(48 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/orig.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(engine.compute_file_blake3(&id).await.unwrap(), expected_hash);

    let renamed = engine
        .rename_completed_file(&id, "renamed-artifact.bin")
        .await
        .unwrap();
    assert_eq!(renamed, dir.path().join("renamed-artifact.bin"));
    assert!(renamed.exists());
    assert!(!outcome.final_path.exists());
    assert_eq!(file_blake3_hex(&renamed), expected_hash);
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().output_path,
        Some(renamed)
    );
}

#[tokio::test]
async fn tier1_ea6_move_completed_file_creates_target_dir_and_updates_sqlite() {
    let payload = Arc::new(deterministic_payload(48 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, _, _| MockResponse {
        status: "200 OK",
        headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
        body: if method == "HEAD" {
            Vec::new()
        } else {
            payload_srv.as_ref().clone()
        },
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/pkg.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    let dest_dir = dir.path().join("nested").join("archive");
    let moved = engine.move_completed_file(&id, &dest_dir).await.unwrap();
    assert_eq!(moved, dest_dir.join("pkg.bin"));
    assert!(moved.exists());
    assert!(!outcome.final_path.exists());
    assert_eq!(file_blake3_hex(&moved), expected_hash);

    let rec = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(rec.output_path, Some(moved));
    assert_eq!(rec.output_dir, Some(dest_dir));
}

#[test]
fn tier1_ea6_download_id_public_api_contract() {
    let id1 = DownloadId::new();
    let id2 = DownloadId::default();
    assert_ne!(id1, id2);
    assert_eq!(id1.as_str(), id1.to_string());
    let as_ref: &str = id1.as_ref();
    assert_eq!(as_ref, id1.as_str());

    let cloned = id1.clone();
    assert_eq!(id1, cloned);

    let roundtrip = DownloadId::from(id1.as_str().to_string());
    assert_eq!(roundtrip, id1);

    let mut map = HashMap::new();
    map.insert(id1.clone(), 42usize);
    assert_eq!(map.get(&roundtrip), Some(&42));

    let mut set = HashSet::new();
    set.insert(id1);
    assert!(set.contains(&cloned));
}
