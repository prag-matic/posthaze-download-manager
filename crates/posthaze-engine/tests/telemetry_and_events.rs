mod common;

use std::sync::Arc;
use std::time::Duration;

use posthaze_engine::{
    DownloadEngine, DownloadRequest, DownloadState, EngineEvent, SegmentProgress,
};
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn progress_snapshot_reports_speed_eta_and_segment_checkpoints() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    // 6 MiB payload so 2+ segments are planned (> 2 MiB MIN_SEGMENT_SIZE).
    let total_len = 6 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..total_len).map(|i| (i % 251) as u8).collect();
    let payload_srv = Arc::new(payload);

    let base_url = common::spawn_raw_server(move |method, _path, range, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"t1\"\r\nConnection: close\r\n\r\n",
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nETag: \"t1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            for chunk in slice.chunks(64 * 1024) {
                if write_half.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(8)).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut req = DownloadRequest::new(format!("{base_url}/telemetry.bin"), dir.path());
    req.max_workers = 2;
    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

    let mut saw_nonzero_speed = false;
    let mut saw_two_segments = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snap = rx.borrow().clone();
            for seg in &snap.segments {
                assert!(seg.start <= seg.checkpointed_cursor);
                assert!(seg.checkpointed_cursor <= seg.cursor);
            }
            if snap.state == DownloadState::Active && snap.segments.len() >= 2 {
                saw_two_segments = true;
            }
            if snap.state == DownloadState::Active && snap.bytes_per_sec > 0 {
                saw_nonzero_speed = true;
                assert!(snap.eta_secs.is_some());
            }
            if saw_nonzero_speed && saw_two_segments && snap.bytes_downloaded >= 512 * 1024 {
                break;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap();

    assert!(saw_two_segments);
    assert!(saw_nonzero_speed);

    engine.pause(&id).await.unwrap();
    let _ = handle.wait().await;

    let persisted_segments: Vec<SegmentProgress> = engine.list_segments(&id).await.unwrap();
    assert!(persisted_segments.len() >= 2);
    for seg in &persisted_segments {
        assert_eq!(seg.cursor, seg.checkpointed_cursor);
    }
}

#[tokio::test]
async fn subscribe_events_delivers_lifecycle_progress_queue_and_settings_events() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    let mut rx1 = engine.subscribe_events();
    let mut rx2 = engine.subscribe_events();

    let mut settings = engine.get_settings().await.unwrap();
    settings.max_concurrent_downloads = 4;
    engine.update_settings(settings.clone()).await.unwrap();

    let payload = vec![0x42u8; 256 * 1024];
    let payload_srv = Arc::new(payload);
    let base_url = common::spawn_raw_server(move |method, _path, _range, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            let hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n",
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
            if method != "HEAD" {
                for chunk in payload_srv.chunks(64 * 1024) {
                    let _ = write_half.write_all(chunk).await;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await;

    let mut handle = engine
        .start(DownloadRequest::new(format!("{base_url}/events.bin"), dir.path()))
        .await
        .unwrap();
    let id = handle.id().clone();
    handle.wait().await.unwrap();

    let mut saw_settings = false;
    let mut saw_queue = false;
    let mut saw_probing = false;
    let mut saw_active = false;
    let mut saw_progress = false;
    let mut saw_completed = false;

    while let Ok(ev) = rx1.try_recv() {
        match ev {
            EngineEvent::SettingsUpdated(s) if s.max_concurrent_downloads == 4 => {
                saw_settings = true;
            }
            EngineEvent::QueueUpdated => saw_queue = true,
            EngineEvent::StateChanged {
                id: ev_id,
                state: DownloadState::Probing,
                ..
            } if ev_id == id => saw_probing = true,
            EngineEvent::StateChanged {
                id: ev_id,
                state: DownloadState::Active,
                ..
            } if ev_id == id => saw_active = true,
            EngineEvent::StateChanged {
                id: ev_id,
                state: DownloadState::Completed,
                ..
            } if ev_id == id => saw_completed = true,
            EngineEvent::ProgressUpdated { id: ev_id, .. } if ev_id == id => {
                saw_progress = true;
            }
            _ => {}
        }
    }

    assert!(saw_settings);
    assert!(saw_queue);
    assert!(saw_probing);
    assert!(saw_active);
    assert!(saw_progress);
    assert!(saw_completed);

    // Second subscriber also receives events independently.
    assert!(rx2.try_recv().is_ok());
}

#[tokio::test]
async fn dynamic_segment_split_appears_live_in_progress_snapshot() {
    let dir = tempdir().unwrap();
    let engine = DownloadEngine::in_memory().await.unwrap();

    // 8 MiB payload with max_workers = 2 -> initial 2 segments of 4 MiB each.
    // Throttle range starting at byte 0 so worker 1 finishes [4 MiB..8 MiB) fast and steals [0..4 MiB).
    let total_len = 8 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..total_len).map(|i| (i % 251) as u8).collect();
    let payload_srv = Arc::new(payload.clone());

    let base_url = common::spawn_raw_server(move |method, _path, range, mut write_half| {
        let payload_srv = Arc::clone(&payload_srv);
        async move {
            if method == "HEAD" {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"split-1\"\r\nConnection: close\r\n\r\n",
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
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nETag: \"split-1\"\r\nConnection: close\r\n\r\n",
                slice.len(),
                payload_srv.len()
            );
            let _ = write_half.write_all(hdr.as_bytes()).await;
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

    let mut req = DownloadRequest::new(format!("{base_url}/split.bin"), dir.path());
    req.max_workers = 2;
    let mut handle = engine.start(req).await.unwrap();
    let mut rx = handle.subscribe();

    let mut max_segments_seen = 0usize;
    let watcher = tokio::spawn(async move {
        loop {
            let len = rx.borrow().segments.len();
            max_segments_seen = max_segments_seen.max(len);
            if rx.changed().await.is_err() {
                break;
            }
        }
        max_segments_seen
    });

    let outcome = handle.wait().await.unwrap();
    let observed_max = watcher.await.unwrap();
    assert!(
        observed_max >= 3,
        "expected dynamic split (>=3 segments) in ProgressSnapshot, saw {observed_max}"
    );
    assert_eq!(
        blake3::hash(&std::fs::read(&outcome.final_path).unwrap()),
        blake3::hash(&payload)
    );
}
