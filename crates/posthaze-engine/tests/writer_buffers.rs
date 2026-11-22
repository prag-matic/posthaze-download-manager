mod common;

use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use common::parse_byte_range;
use posthaze_engine::{
    DownloadEngine, DownloadRequest, DownloadState, EngineError, ProgressSnapshot,
};

async fn spawn_chunked_stream_server(
    payload: Arc<Vec<u8>>,
    supports_ranges: bool,
    chunk_pattern: Vec<usize>,
    per_chunk_delay: Option<Duration>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let chunk_pattern = chunk_pattern.clone();
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
                let range_hdr = req_str.lines().find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("range") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });

                if method == "HEAD" {
                    let resp = if supports_ranges {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"odd-v1\"\r\nConnection: close\r\n\r\n",
                            payload.len()
                        )
                    } else {
                        "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
                    };
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                if method == "GET" {
                    let (headers, slice) = match (supports_ranges, range_hdr.as_deref()) {
                        (true, Some(r)) => {
                            let (start, end) = parse_byte_range(r, payload.len());
                            let slice = &payload[start..=end];
                            let hdr = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"odd-v1\"\r\nConnection: close\r\n\r\n",
                                slice.len(),
                                payload.len()
                            );
                            (hdr, slice)
                        }
                        _ => {
                            let hdr = if supports_ranges {
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"odd-v1\"\r\nConnection: close\r\n\r\n",
                                    payload.len()
                                )
                            } else {
                                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
                            };
                            (hdr, payload.as_slice())
                        }
                    };
                    if stream.write_all(headers.as_bytes()).await.is_err() {
                        return;
                    }

                    let mut offset = 0usize;
                    let mut idx = 0usize;
                    while offset < slice.len() {
                        let step = chunk_pattern[idx % chunk_pattern.len()]
                            .max(1)
                            .min(slice.len() - offset);
                        idx += 1;
                        if stream.write_all(&slice[offset..offset + step]).await.is_err() {
                            return;
                        }
                        offset += step;
                        if let Some(delay) = per_chunk_delay {
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn single_stream_multi_mib_odd_chunks_straddling_512k_boundary_verifies_blake3() {
    // > 3 MiB payload streamed without Content-Length/Accept-Ranges in odd-sized TCP writes
    // that repeatedly straddle and exceed the 512 KiB CHUNK_BUFFER_SIZE boundary.
    let payload: Vec<u8> = (0..(3 * 1024 * 1024 + 137_913))
        .map(|i| ((i * 13 + 5) % 251) as u8)
        .collect();
    let expected_hash = blake3::hash(&payload);
    let base_url = spawn_chunked_stream_server(
        Arc::new(payload.clone()),
        false,
        vec![17_001, 257_321, 700_003, 111_111, 1, 614_400],
        None,
    )
    .await;

    let temp_dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();
    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/single-odd.bin"), temp_dir.path()),
            None,
        )
        .await
        .expect("single-stream odd-chunk download should succeed");

    assert!(!outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 1);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert!(!temp_dir.path().join("single-odd.bin.part").exists());

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn multi_segment_odd_chunks_and_oversized_frames_verify_blake3() {
    // 9 MiB + odd tail across 4 concurrent Workers with TCP reads both smaller and larger than 512 KiB.
    let payload: Vec<u8> = (0..(9 * 1024 * 1024 + 73_123))
        .map(|i| ((i * 29 + 11) % 251) as u8)
        .collect();
    let expected_hash = blake3::hash(&payload);
    let base_url = spawn_chunked_stream_server(
        Arc::new(payload.clone()),
        true,
        vec![13_337, 300_001, 650_009, 99_991],
        None,
    )
    .await;

    let temp_dir = TempDir::new().unwrap();
    let mut req = DownloadRequest::new(format!("{base_url}/multi-odd.bin"), temp_dir.path());
    req.max_workers = 4;

    let engine = DownloadEngine::new();
    let outcome = engine
        .download(req, None)
        .await
        .expect("multi-segment odd-chunk download should succeed");

    assert!(outcome.supports_ranges);
    assert_eq!(outcome.workers_used, 4);
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert!(!temp_dir.path().join("multi-odd.bin.part").exists());

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn pause_flushes_odd_tail_and_resume_completes_blake3_exact() {
    let payload: Vec<u8> = (0..(5 * 1024 * 1024 + 19_457))
        .map(|i| ((i * 7 + 19) % 251) as u8)
        .collect();
    let expected_hash = blake3::hash(&payload);
    let base_url = spawn_chunked_stream_server(
        Arc::new(payload.clone()),
        true,
        vec![190_001, 110_003],
        Some(Duration::from_millis(25)),
    )
    .await;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/pause-odd.bin"), temp_dir.path());
    req.max_workers = 2;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx: watch::Receiver<ProgressSnapshot> = handle.subscribe();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            {
                let snap = rx.borrow();
                if snap.state == DownloadState::Active && snap.bytes_downloaded >= 600_000 {
                    break;
                }
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("download should become active and stream > 512 KiB");

    handle.pause();
    let err = handle.wait().await.unwrap_err();
    assert!(matches!(err, EngineError::Paused), "got {err:?}");

    let mut resumed = engine.resume(&id).await.unwrap();
    let outcome = resumed
        .wait()
        .await
        .expect("resumed download should complete");
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}

#[tokio::test]
async fn cancel_mid_transfer_exits_cleanly_and_deletes_part() {
    let payload: Vec<u8> = (0..(4 * 1024 * 1024 + 555))
        .map(|i| ((i * 3 + 1) % 251) as u8)
        .collect();
    let base_url = spawn_chunked_stream_server(
        Arc::new(payload),
        true,
        vec![128 * 1024],
        Some(Duration::from_millis(35)),
    )
    .await;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut req = DownloadRequest::new(format!("{base_url}/cancel-odd.bin"), temp_dir.path());
    req.max_workers = 2;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

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
    .expect("download should become active");

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
}
