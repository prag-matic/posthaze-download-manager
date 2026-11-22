mod e2e_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use e2e_common::{
    MockResponse, blake3_hex, deterministic_payload, file_blake3_hex, parse_byte_range,
    spawn_mock_server, spawn_raw_server,
};
use posthaze_engine::writer::PartFileWriter;
use posthaze_engine::{
    CollisionPolicy, DownloadEngine, DownloadId, DownloadRequest, DownloadState, EngineError,
    EngineEvent, FailureKind, ProxyMode, Settings, resolve_target_path,
};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Notify;

// ============================================================================
// EA1: Boundaries (0B, 1B, 512 KiB, 512 KiB + 1, Missing Content-Length, Errors)
// ============================================================================

#[tokio::test]
async fn tier2_ea1_zero_byte_download_range_and_non_range() {
    let base_url = spawn_mock_server(|_, path, _| match path {
        "/zero-range.bin" => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), "0".into()),
                ("Accept-Ranges".into(), "bytes".into()),
            ],
            body: Vec::new(),
        },
        "/zero-stream.bin" => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), "0".into()),
                ("Accept-Ranges".into(), "none".into()),
            ],
            body: Vec::new(),
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
    for path in ["/zero-range.bin", "/zero-stream.bin"] {
        let outcome = engine
            .download(
                DownloadRequest::new(format!("{base_url}{path}"), dir.path()),
                None,
            )
            .await
            .unwrap();
        assert_eq!(outcome.bytes_downloaded, 0);
        assert_eq!(std::fs::metadata(&outcome.final_path).unwrap().len(), 0);
        assert_eq!(file_blake3_hex(&outcome.final_path), blake3_hex(&[]));
    }
}

#[tokio::test]
async fn tier2_ea1_single_byte_download_with_high_max_workers() {
    let payload = Arc::new(vec![0xA7u8]);
    let payload_srv = Arc::clone(&payload);
    let base_url = spawn_mock_server(move |method, _, range| {
        if method == "HEAD" {
            return MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), "1".into()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            };
        }
        if let Some(r) = range {
            let (s, e) = parse_byte_range(r, 1);
            MockResponse {
                status: "206 Partial Content",
                headers: vec![
                    ("Content-Length".into(), "1".into()),
                    ("Content-Range".into(), format!("bytes {s}-{e}/1")),
                ],
                body: payload_srv[s..=e].to_vec(),
            }
        } else {
            MockResponse {
                status: "200 OK",
                headers: vec![("Content-Length".into(), "1".into())],
                body: payload_srv.as_ref().clone(),
            }
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/one-byte.bin"), dir.path());
    req.max_workers = 16;

    let outcome = engine.download(req, None).await.unwrap();
    assert_eq!(outcome.bytes_downloaded, 1);
    assert_eq!(outcome.workers_used, 1);
    assert_eq!(std::fs::read(&outcome.final_path).unwrap(), vec![0xA7u8]);
}

#[tokio::test]
async fn tier2_ea1_exact_512kib_and_512kib_plus_1_chunk_boundaries() {
    let exact = Arc::new(deterministic_payload(512 * 1024));
    let plus_one = Arc::new(deterministic_payload(512 * 1024 + 1));

    let exact_srv = Arc::clone(&exact);
    let plus_srv = Arc::clone(&plus_one);
    let base_url = spawn_mock_server(move |method, path, _| {
        let data = if path == "/exact.bin" {
            &exact_srv
        } else {
            &plus_srv
        };
        MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), data.len().to_string()),
                ("Accept-Ranges".into(), "none".into()),
            ],
            body: if method == "HEAD" {
                Vec::new()
            } else {
                data.as_ref().clone()
            },
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let out_exact = engine
        .download(
            DownloadRequest::new(format!("{base_url}/exact.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(out_exact.bytes_downloaded, 512 * 1024);
    assert_eq!(file_blake3_hex(&out_exact.final_path), blake3_hex(&exact));

    let out_plus = engine
        .download(
            DownloadRequest::new(format!("{base_url}/plus1.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(out_plus.bytes_downloaded, 512 * 1024 + 1);
    assert_eq!(
        file_blake3_hex(&out_plus.final_path),
        blake3_hex(&plus_one)
    );
}

#[tokio::test]
async fn tier2_ea1_missing_content_length_and_no_ranges_streams_until_eof() {
    let payload = Arc::new(deterministic_payload(700 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            let hdr = "HTTP/1.1 200 OK\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n";
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if req.method != "HEAD" {
                for chunk in payload_srv.chunks(64 * 1024) {
                    let _ = write_half.write_all(chunk).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/unbounded.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.content_length, None);
    assert!(!outcome.supports_ranges);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea1_head_405_falls_back_to_range_get_probe_and_completes() {
    let payload = Arc::new(deterministic_payload(3 * 1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, _, range| {
        if method == "HEAD" {
            return MockResponse {
                status: "405 Method Not Allowed",
                headers: vec![("Content-Length".into(), "0".into())],
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
    let mut req = DownloadRequest::new(format!("{base_url}/no-head.bin"), dir.path());
    req.max_workers = 2;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 2);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea1_part_file_writer_errors_when_parent_is_a_regular_file() {
    let dir = TempDir::new().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"file").unwrap();

    let bad_part = blocker.join("child.part");
    let err = PartFileWriter::spawn(bad_part, Some(1024))
        .expect_err("spawning writer under non-directory parent must fail");
    assert!(matches!(err, EngineError::Io(_)));

    // Also verify finish() fails cleanly if destination parent doesn't exist.
    let ok_part = dir.path().join("valid.part");
    let writer = PartFileWriter::spawn(ok_part, Some(4)).unwrap();
    writer.write_at(0, Bytes::from_static(b"abcd")).await.unwrap();
    let missing_dest = dir.path().join("missing_subdir").join("out.bin");
    let finish_err = writer.finish(&missing_dest).await.unwrap_err();
    assert!(matches!(finish_err, EngineError::Io(_)));
}

// ============================================================================
// EA2: Boundaries (NotFound, InvalidState, ResourceChanged, Queue Interrupts)
// ============================================================================

#[tokio::test]
async fn tier2_ea2_nonexistent_download_id_returns_not_found() {
    let engine = DownloadEngine::in_memory().await.unwrap();
    let ghost = DownloadId::new();

    assert!(matches!(engine.resume(&ghost).await, Err(EngineError::NotFound(_))));
    assert!(matches!(engine.pause(&ghost).await, Err(EngineError::NotFound(_))));
    assert!(matches!(engine.cancel(&ghost).await, Err(EngineError::NotFound(_))));
    assert!(matches!(engine.delete(&ghost, false).await, Err(EngineError::NotFound(_))));
    assert!(matches!(engine.restart_fresh(&ghost).await, Err(EngineError::NotFound(_))));
    assert!(matches!(engine.list_segments(&ghost).await, Err(EngineError::NotFound(_))));
    assert!(matches!(
        engine.compute_file_blake3(&ghost).await,
        Err(EngineError::NotFound(_))
    ));
    assert!(engine.get_download(&ghost).await.unwrap().is_none());
}

#[tokio::test]
async fn tier2_ea2_invalid_state_transitions_return_invalid_state() {
    let payload = Arc::new(deterministic_payload(16 * 1024));
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
        .start(DownloadRequest::new(format!("{base_url}/done.bin"), dir.path()))
        .await
        .unwrap();
    let completed_id = handle.id().clone();
    handle.wait().await.unwrap();

    assert!(matches!(
        engine.resume(&completed_id).await,
        Err(EngineError::InvalidState(_))
    ));
    assert!(matches!(
        engine.pause(&completed_id).await,
        Err(EngineError::InvalidState(_))
    ));
    assert!(matches!(
        engine.cancel(&completed_id).await,
        Err(EngineError::InvalidState(_))
    ));
    assert!(matches!(
        engine.restart_fresh(&completed_id).await,
        Err(EngineError::InvalidState(_))
    ));

    let paused_id = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/p.bin"), dir.path()))
        .await
        .unwrap();
    assert!(matches!(
        engine.pause(&paused_id).await,
        Err(EngineError::InvalidState(_))
    ));
}

#[tokio::test]
async fn tier2_ea2_enqueue_paused_on_ephemeral_engine_returns_invalid_state() {
    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();
    let err = engine
        .enqueue_paused(DownloadRequest::new("http://127.0.0.1:9/x.bin", dir.path()))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::InvalidState(_)));
}

#[tokio::test]
async fn tier2_ea2_resume_fails_with_resource_changed_on_etag_or_length_change() {
    let payload = Arc::new(deterministic_payload(2 * 1024 * 1024));
    let mutate_etag = Arc::new(AtomicBool::new(false));
    let payload_srv = Arc::clone(&payload);
    let mutate_srv = Arc::clone(&mutate_etag);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let mutate_srv = Arc::clone(&mutate_srv);
        async move {
            let etag = if mutate_srv.load(Ordering::SeqCst) {
                "\"v2\""
            } else {
                "\"v1\""
            };
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(12)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/mut.bin"), dir.path()))
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

    mutate_etag.store(true, Ordering::SeqCst);
    let mut resumed = engine.resume(&id).await.unwrap();
    let err = resumed.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::ResourceChanged(_)));
    assert_eq!(err.failure_kind(), FailureKind::ResourceChanged);

    let rec = engine.get_download(&id).await.unwrap().unwrap();
    assert_eq!(rec.state, DownloadState::Failed);
    assert_eq!(rec.failure_kind(), Some(FailureKind::ResourceChanged));

    // Also verify Content-Length mismatch (without ETag) triggers EngineError::ResourceChanged.
    let mutate_len = Arc::new(AtomicBool::new(false));
    let payload_len_srv = Arc::clone(&payload);
    let mutate_len_srv = Arc::clone(&mutate_len);
    let len_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_len_srv);
        let mutate_srv = Arc::clone(&mutate_len_srv);
        async move {
            let advertised_len = if mutate_srv.load(Ordering::SeqCst) {
                payload_srv.len() + 4096
            } else {
                payload_srv.len()
            };
            if req.method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {advertised_len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{advertised_len}\r\nConnection: close\r\n\r\n",
                slice.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(12)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut len_handle = engine
        .start(DownloadRequest::new(format!("{len_url}/mut-len.bin"), dir.path()))
        .await
        .unwrap();
    let len_id = len_handle.id().clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        len_handle
            .subscribe()
            .wait_for(|s| s.bytes_downloaded >= 256 * 1024)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    engine.pause(&len_id).await.unwrap();
    let _ = len_handle.wait().await;

    mutate_len.store(true, Ordering::SeqCst);
    let mut len_resumed = engine.resume(&len_id).await.unwrap();
    let len_err = len_resumed.wait().await.unwrap_err();
    assert!(matches!(len_err, EngineError::ResourceChanged(_)));
    assert_eq!(len_err.failure_kind(), FailureKind::ResourceChanged);

    let len_rec = engine.get_download(&len_id).await.unwrap().unwrap();
    assert_eq!(len_rec.state, DownloadState::Failed);
    assert_eq!(len_rec.failure_kind(), Some(FailureKind::ResourceChanged));
}

#[tokio::test]
async fn tier2_ea2_pause_and_cancel_while_queued_under_concurrency_1() {
    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 1;
    engine.update_settings(settings).await.unwrap();

    let payload = Arc::new(deterministic_payload(32 * 1024));
    let gate = Arc::new(Notify::new());
    let payload_srv = Arc::clone(&payload);
    let gate_srv = Arc::clone(&gate);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let gate_srv = Arc::clone(&gate_srv);
        async move {
            if req.method == "HEAD" && req.path == "/hold.bin" {
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

    let mut h1 = engine
        .start(DownloadRequest::new(format!("{base_url}/hold.bin"), dir.path()))
        .await
        .unwrap();
    let mut h2 = engine
        .start(DownloadRequest::new(format!("{base_url}/q-pause.bin"), dir.path()))
        .await
        .unwrap();
    let mut h3 = engine
        .start(DownloadRequest::new(format!("{base_url}/q-cancel.bin"), dir.path()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(h2.subscribe().borrow().state, DownloadState::Queued);
    assert_eq!(h3.subscribe().borrow().state, DownloadState::Queued);

    h2.pause();
    assert!(matches!(h2.wait().await, Err(EngineError::Paused)));
    engine.cancel(h3.id()).await.unwrap();
    assert!(matches!(h3.wait().await, Err(EngineError::Cancelled)));

    gate.notify_waiters();
    h1.wait().await.unwrap();

    let mut h2_resumed = engine.resume(h2.id()).await.unwrap();
    let out = h2_resumed.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&out.final_path), blake3_hex(&payload));
}

// ============================================================================
// EA3: HTTP & Proxy Boundaries (Validation, Status Codes, Retries, FailureKind)
// ============================================================================

#[tokio::test]
async fn tier2_ea3_settings_validate_rejects_zero_workers_and_zero_concurrency() {
    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(&dir.path().join("state.db")).await.unwrap();
    let valid = Settings {
        default_output_dir: dir.path().to_path_buf(),
        max_workers: 4,
        max_concurrent_downloads: 2,
        ..Settings::default()
    };
    engine.update_settings(valid.clone()).await.unwrap();

    let zero_workers = Settings {
        max_workers: 0,
        ..valid.clone()
    };
    assert!(matches!(zero_workers.validate(), Err(EngineError::InvalidState(_))));
    assert!(matches!(
        engine.update_settings(zero_workers).await,
        Err(EngineError::InvalidState(_))
    ));

    let zero_conc = Settings {
        max_concurrent_downloads: 0,
        ..valid.clone()
    };
    assert!(matches!(zero_conc.validate(), Err(EngineError::InvalidState(_))));
    assert!(matches!(
        engine.update_settings(zero_conc).await,
        Err(EngineError::InvalidState(_))
    ));

    assert_eq!(engine.get_settings().await.unwrap(), valid);
}

#[tokio::test]
async fn tier2_ea3_manual_proxy_empty_whitespace_or_invalid_url_rejected() {
    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(&dir.path().join("state.db")).await.unwrap();
    let valid = Settings {
        default_output_dir: dir.path().to_path_buf(),
        proxy_mode: ProxyMode::None,
        ..Settings::default()
    };
    engine.update_settings(valid.clone()).await.unwrap();

    for bad_proxy in ["", "   \t ", "not a valid proxy url ://???"] {
        let candidate = Settings {
            proxy_mode: ProxyMode::Manual,
            proxy_url: bad_proxy.to_string(),
            ..valid.clone()
        };
        assert!(
            matches!(
                engine.update_settings(candidate).await,
                Err(EngineError::InvalidState(_))
            ),
            "expected InvalidState for proxy_url={bad_proxy:?}"
        );
    }

    assert_eq!(engine.get_settings().await.unwrap(), valid);
}

#[tokio::test]
async fn tier2_ea3_http_404_fails_immediately_non_resumable_without_retry() {
    let get_hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = Arc::clone(&get_hits);
    let base_url = spawn_mock_server(move |method, _, _| {
        if method == "HEAD" {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), (2 * 1024 * 1024).to_string()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            }
        } else {
            hits_srv.fetch_add(1, Ordering::SeqCst);
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
    let mut req = DownloadRequest::new(format!("{base_url}/404.bin"), dir.path());
    req.max_workers = 1;

    let err = engine.download(req, None).await.unwrap_err();
    assert!(matches!(err, EngineError::HttpStatus { status: 404, .. }));
    assert_eq!(err.failure_kind(), FailureKind::NonResumable);
    assert_eq!(get_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tier2_ea3_http_503_exhausts_retries_with_resumable_failure_kind() {
    let get_hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = Arc::clone(&get_hits);
    let base_url = spawn_mock_server(move |method, _, _| {
        if method == "HEAD" {
            MockResponse {
                status: "200 OK",
                headers: vec![
                    ("Content-Length".into(), (1024 * 1024).to_string()),
                    ("Accept-Ranges".into(), "bytes".into()),
                ],
                body: Vec::new(),
            }
        } else {
            hits_srv.fetch_add(1, Ordering::SeqCst);
            MockResponse {
                status: "503 Service Unavailable",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            }
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/503.bin"), dir.path());
    req.max_workers = 1;

    let err = engine.download(req, None).await.unwrap_err();
    assert!(matches!(err, EngineError::HttpStatus { status: 503, .. }));
    assert_eq!(err.failure_kind(), FailureKind::Resumable);
    assert_eq!(get_hits.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn tier2_ea3_http_500_transient_recovers_within_retry_budget() {
    let payload = Arc::new(deterministic_payload(1024 * 1024));
    let expected_hash = blake3_hex(&payload);
    let attempts = Arc::new(AtomicUsize::new(0));

    let payload_srv = Arc::clone(&payload);
    let attempts_srv = Arc::clone(&attempts);
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
        let n = attempts_srv.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            return MockResponse {
                status: "500 Internal Server Error",
                headers: vec![("Content-Length".into(), "0".into())],
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
    let mut req = DownloadRequest::new(format!("{base_url}/500-recover.bin"), dir.path());
    req.max_workers = 1;

    let outcome = engine.download(req, None).await.unwrap();
    assert!(outcome.retries_performed >= 2);
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea3_unreachable_manual_proxy_fails_probe_cleanly() {
    // Bind and immediately drop a TCP listener to guarantee an unused closed port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed_port = listener.local_addr().unwrap().port();
    drop(listener);

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        proxy_mode: ProxyMode::Manual,
        proxy_url: format!("http://127.0.0.1:{closed_port}"),
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let err = engine
        .probe("http://127.0.0.1:80/file.bin", dir.path())
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Http(_)));
}

// ============================================================================
// EA4: Progress & Event Broadcast Boundaries
// ============================================================================

#[tokio::test]
async fn tier2_ea4_slow_broadcast_consumer_observes_lagged_without_blocking_engine() {
    let engine = DownloadEngine::in_memory().await.unwrap();
    let mut slow_rx = engine.subscribe_events();

    let mut settings = engine.get_settings().await.unwrap();
    // 600 update_settings calls emit 1200 events (SettingsUpdated + QueueUpdated),
    // overflowing both 256 (post-M2) and 1024 (pre-M2) broadcast capacities.
    for i in 1..=600u64 {
        settings.download_speed_limit_kib = i;
        engine.update_settings(settings.clone()).await.unwrap();
    }

    let mut saw_lagged = false;
    loop {
        match slow_rx.recv().await {
            Err(RecvError::Lagged(skipped)) => {
                assert!(skipped > 0);
                saw_lagged = true;
                break;
            }
            Ok(_) => continue,
            Err(RecvError::Closed) => break,
        }
    }
    assert!(saw_lagged, "expected slow subscriber to observe RecvError::Lagged");

    // Drain remaining buffered messages before verifying fresh recovery delivery.
    while let Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) =
        slow_rx.try_recv()
    {}

    // Subscriber recovers and continues receiving subsequent events.
    settings.download_speed_limit_kib = 0;
    engine.update_settings(settings).await.unwrap();
    let mut saw_recovery_event = false;
    while let Ok(ev) = slow_rx.try_recv() {
        if matches!(ev, EngineEvent::SettingsUpdated(ref s) if s.download_speed_limit_kib == 0) {
            saw_recovery_event = true;
        }
    }
    assert!(saw_recovery_event);
}

#[tokio::test]
async fn tier2_ea4_dropped_event_and_watch_subscribers_do_not_block_completion() {
    let payload = Arc::new(deterministic_payload(256 * 1024));
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
    let ev_rx = engine.subscribe_events();
    drop(ev_rx);

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/nodrop.bin"), dir.path()))
        .await
        .unwrap();
    let watch_rx = handle.subscribe();
    drop(watch_rx);

    let outcome = handle.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea4_zero_speed_limit_means_unlimited_throughput() {
    let payload = Arc::new(deterministic_payload(2 * 1024 * 1024));
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
    let mut settings = engine.get_settings().await.unwrap();
    settings.download_speed_limit_kib = 0;
    engine.update_settings(settings).await.unwrap();

    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/fast.bin"), dir.path()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea4_dynamic_speed_limit_lift_mid_transfer_completes_byte_exact() {
    let payload = Arc::new(deterministic_payload(512 * 1024));
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
    let mut settings = engine.get_settings().await.unwrap();
    settings.download_speed_limit_kib = 64;
    engine.update_settings(settings.clone()).await.unwrap();

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/lift.bin"), dir.path()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        handle
            .subscribe()
            .wait_for(|s| s.bytes_downloaded >= 64 * 1024)
            .await
            .unwrap();
    })
    .await
    .unwrap();

    settings.download_speed_limit_kib = 0;
    engine.update_settings(settings).await.unwrap();

    let outcome = tokio::time::timeout(Duration::from_secs(5), handle.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea4_terminal_snapshot_zeroes_active_workers_and_matches_total_bytes() {
    let payload = Arc::new(deterministic_payload(128 * 1024));
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
        .start(DownloadRequest::new(format!("{base_url}/term.bin"), dir.path()))
        .await
        .unwrap();
    let rx = handle.subscribe();
    handle.wait().await.unwrap();

    let snap = rx.borrow().clone();
    assert_eq!(snap.state, DownloadState::Completed);
    assert_eq!(snap.active_workers, 0);
    assert_eq!(snap.bytes_downloaded, payload.len() as u64);
    assert_eq!(snap.total_bytes, Some(payload.len() as u64));
}

// ============================================================================
// EA5: SQLite & Persistence Boundaries
// ============================================================================

#[tokio::test]
async fn tier2_ea5_concurrent_checkpoint_writes_and_list_reads_have_no_lock_timeouts() {
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
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(&dir.path().join("concurrent.db"))
        .await
        .unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/heavy.bin"), dir.path());
    req.max_workers = 4;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();

    let reader_engine = engine.clone();
    let reader_id = id.clone();
    let reader_task = tokio::spawn(async move {
        for _ in 0..40 {
            let _ = reader_engine.list_downloads().await.unwrap();
            let _ = reader_engine.list_segments(&reader_id).await.unwrap();
            let _ = reader_engine.list_segment_counts().await.unwrap();
            let _ = reader_engine.get_settings().await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    let outcome = handle.wait().await.unwrap();
    reader_task.await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), expected_hash);
}

#[tokio::test]
async fn tier2_ea5_startup_reconciles_probing_and_non_range_active_to_failed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let stop_server = Arc::new(Notify::new());
    let stop_srv = Arc::clone(&stop_server);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let stop_conn = Arc::clone(&stop_srv);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("");
                if path == "/hang-head" {
                    let _ = tokio::time::timeout(Duration::from_secs(30), stop_conn.notified()).await;
                } else if method == "HEAD" {
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n")
                        .await;
                } else {
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nConnection: close\r\n\r\nhi")
                        .await;
                    let _ = tokio::time::timeout(Duration::from_secs(30), stop_conn.notified()).await;
                }
            });
        }
    });
    let base_url = format!("http://{addr}");

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");

    let (id_probe, id_stream) = {
        let db = db_path.clone();
        let out = dir.path().to_path_buf();
        let url = base_url.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let ids = rt.block_on(async {
                let engine = DownloadEngine::open(&db).await.unwrap();

                let h_probe = engine
                    .start(DownloadRequest::new(format!("{url}/hang-head"), &out))
                    .await
                    .unwrap();
                let h_stream = engine
                    .start(DownloadRequest::new(format!("{url}/hang-body"), &out))
                    .await
                    .unwrap();
                let id_probe = h_probe.id().clone();
                let id_stream = h_stream.id().clone();

                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let p = engine.get_download(&id_probe).await.unwrap().unwrap();
                        let s = engine.get_download(&id_stream).await.unwrap().unwrap();
                        if p.state == DownloadState::Probing && s.state == DownloadState::Active {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(15)).await;
                    }
                })
                .await
                .unwrap();

                drop(h_probe);
                drop(h_stream);
                drop(engine);
                (id_probe, id_stream)
            });
            rt.shutdown_timeout(Duration::from_millis(200));
            ids
        })
        .await
        .unwrap()
    };

    stop_server.notify_waiters();
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let r_probe = reopened.get_download(&id_probe).await.unwrap().unwrap();
    let r_stream = reopened.get_download(&id_stream).await.unwrap().unwrap();
    assert_eq!(r_probe.state, DownloadState::Failed);
    assert_eq!(r_stream.state, DownloadState::Failed);
    assert_eq!(r_probe.failure_kind(), Some(FailureKind::Resumable));
}

#[tokio::test]
async fn tier2_ea5_startup_reconciles_queued_to_paused_and_allows_resume() {
    let payload = Arc::new(deterministic_payload(16 * 1024));
    let payload_srv = Arc::clone(&payload);
    let gate = Arc::new(Notify::new());
    let gate_srv = Arc::clone(&gate);

    let base_url = spawn_raw_server(move |req, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        let gate_srv = Arc::clone(&gate_srv);
        async move {
            if req.method == "HEAD" && req.path == "/blocker.bin" {
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
    let db_path = dir.path().join("state.db");
    let id_queued = {
        let db = db_path.clone();
        let out = dir.path().to_path_buf();
        let url = base_url.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let qid = rt.block_on(async {
                let eng = DownloadEngine::open(&db).await.unwrap();
                let mut s = eng.get_settings().await.unwrap();
                s.max_concurrent_downloads = 1;
                eng.update_settings(s).await.unwrap();

                let _h1 = eng
                    .start(DownloadRequest::new(format!("{url}/blocker.bin"), &out))
                    .await
                    .unwrap();
                let h2 = eng
                    .start(DownloadRequest::new(format!("{url}/queued.bin"), &out))
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(80)).await;
                h2.id().clone()
            });
            rt.shutdown_background();
            qid
        })
        .await
        .unwrap()
    };

    gate.notify_waiters();
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(
        reopened.get_download(&id_queued).await.unwrap().unwrap().state,
        DownloadState::Paused
    );

    let mut resumed = reopened.resume(&id_queued).await.unwrap();
    let outcome = resumed.wait().await.unwrap();
    assert_eq!(file_blake3_hex(&outcome.final_path), blake3_hex(&payload));
}

#[tokio::test]
async fn tier2_ea5_cascade_delete_removes_persisted_segments() {
    let payload = Arc::new(deterministic_payload(4 * 1024 * 1024));
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
    let mut req = DownloadRequest::new(format!("{base_url}/del.bin"), dir.path());
    req.max_workers = 2;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    assert_eq!(engine.list_segments(&id).await.unwrap().len(), 2);
    assert_eq!(engine.list_segment_counts().await.unwrap().get(&id), Some(&2));

    engine.delete(&id, true).await.unwrap();
    assert!(!outcome.final_path.exists());
    assert!(!engine.list_segment_counts().await.unwrap().contains_key(&id));
    assert!(matches!(
        engine.list_segments(&id).await,
        Err(EngineError::NotFound(_))
    ));
}

#[tokio::test]
async fn tier2_ea5_large_queue_100_persisted_rows_list_and_counts() {
    let dir = TempDir::new().unwrap();
    let engine = DownloadEngine::open(&dir.path().join("many.db")).await.unwrap();

    let mut ids = Vec::with_capacity(100);
    for i in 0..100 {
        let id = engine
            .enqueue_paused(DownloadRequest::new(
                format!("http://127.0.0.1:9/file-{i}.bin"),
                dir.path(),
            ))
            .await
            .unwrap();
        ids.push(id);
    }

    let listed = engine.list_downloads().await.unwrap();
    assert_eq!(listed.len(), 100);
    // Newest-first ordering (`created_at DESC, rowid DESC`) puts the last inserted row first.
    assert_eq!(listed.first().unwrap().id, *ids.last().unwrap());
    assert_eq!(listed.last().unwrap().id, *ids.first().unwrap());
    assert!(engine.list_segment_counts().await.unwrap().is_empty());
}

// ============================================================================
// EA6: Paths, Sanitization, Collisions & Post-Completion Error Boundaries
// ============================================================================

#[tokio::test]
async fn tier2_ea6_sanitizes_windows_reserved_names_and_rfc5987_filenames() {
    let payload = Arc::new(b"safe-content".to_vec());
    let expected_hash = blake3_hex(&payload);
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, path, _| {
        let disp = match path {
            "/reserved" => "attachment; filename=\"../../CON.txt\"",
            "/utf8" => "attachment; filename=\"fallback.bin\"; filename*=UTF-8''hello%20world%3F.bin",
            _ => "attachment; filename=\"ok.bin\"",
        };
        MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_srv.len().to_string()),
                ("Content-Disposition".into(), disp.to_string()),
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
    let engine = DownloadEngine::in_memory().await.unwrap();

    let out_reserved = engine
        .download(
            DownloadRequest::new(format!("{base_url}/reserved"), dir.path()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(out_reserved.final_path, dir.path().join("_CON.txt"));
    assert_eq!(file_blake3_hex(&out_reserved.final_path), expected_hash);

    let out_utf8 = engine
        .download(
            DownloadRequest::new(format!("{base_url}/utf8"), dir.path()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(out_utf8.final_path, dir.path().join("hello world_.bin"));
    assert_eq!(file_blake3_hex(&out_utf8.final_path), expected_hash);
}

#[test]
fn tier2_ea6_collision_policy_ask_flags_prompt_on_part_file_collision() {
    let dir = TempDir::new().unwrap();
    let res_clean = resolve_target_path(dir.path(), "archive.zip", CollisionPolicy::Ask);
    assert!(!res_clean.needs_overwrite_prompt);

    std::fs::write(dir.path().join("archive.zip.part"), b"partial").unwrap();
    let res_part = resolve_target_path(dir.path(), "archive.zip", CollisionPolicy::Ask);
    assert!(res_part.needs_overwrite_prompt);
    assert_eq!(res_part.path, dir.path().join("archive.zip"));
}

#[tokio::test]
async fn tier2_ea6_rename_completed_file_rejects_invalid_colliding_and_paused() {
    let payload = Arc::new(deterministic_payload(8 * 1024));
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

    // No-op same name returns Ok.
    assert_eq!(
        engine.rename_completed_file(&id, "a.bin").await.unwrap(),
        outcome.final_path
    );

    // Invalid names rejected.
    for bad in ["", "   ", "."] {
        assert!(matches!(
            engine.rename_completed_file(&id, bad).await,
            Err(EngineError::InvalidState(_))
        ));
    }

    // Existing collision rejected.
    std::fs::write(dir.path().join("taken.bin"), b"exists").unwrap();
    assert!(matches!(
        engine.rename_completed_file(&id, "taken.bin").await,
        Err(EngineError::InvalidState(_))
    ));

    // Paused download rejected.
    let paused_id = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/b.bin"), dir.path()))
        .await
        .unwrap();
    assert!(matches!(
        engine.rename_completed_file(&paused_id, "c.bin").await,
        Err(EngineError::InvalidState(_))
    ));
}

#[tokio::test]
async fn tier2_ea6_move_completed_file_rejects_collision_and_non_completed() {
    let payload = Arc::new(deterministic_payload(8 * 1024));
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
        .start(DownloadRequest::new(format!("{base_url}/item.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();

    // No-op move to current directory returns Ok.
    assert_eq!(
        engine.move_completed_file(&id, dir.path()).await.unwrap(),
        outcome.final_path
    );

    let dest = dir.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("item.bin"), b"collision").unwrap();
    assert!(matches!(
        engine.move_completed_file(&id, &dest).await,
        Err(EngineError::InvalidState(_))
    ));

    let paused_id = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/p.bin"), dir.path()))
        .await
        .unwrap();
    assert!(matches!(
        engine.move_completed_file(&paused_id, &dest).await,
        Err(EngineError::InvalidState(_))
    ));
}

#[tokio::test]
async fn tier2_ea6_compute_file_blake3_rejects_paused_and_missing_disk_file() {
    let payload = Arc::new(deterministic_payload(8 * 1024));
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

    let paused_id = engine
        .enqueue_paused(DownloadRequest::new(format!("{base_url}/p.bin"), dir.path()))
        .await
        .unwrap();
    assert!(matches!(
        engine.compute_file_blake3(&paused_id).await,
        Err(EngineError::InvalidState(_))
    ));

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/gone.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    let outcome = handle.wait().await.unwrap();
    std::fs::remove_file(&outcome.final_path).unwrap();

    assert!(matches!(
        engine.compute_file_blake3(&id).await,
        Err(EngineError::Io(_))
    ));
}


