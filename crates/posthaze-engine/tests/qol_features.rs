mod common;

use std::sync::Arc;
use tempfile::TempDir;

use common::{MockResponse, spawn_mock_server};
use posthaze_engine::{
    ChecksumAlgorithm, DeleteFileMode, DownloadEngine, DownloadRequest, parse_expected_checksum,
};

#[tokio::test]
async fn compute_file_checksums_and_delete_with_mode_work_end_to_end() {
    let payload = Arc::new(b"hello world".to_vec());
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, path, _| {
        if path != "/hello.txt" && path != "/hello-2.txt" {
            return MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            };
        }
        if method == "HEAD" {
            return MockResponse {
                status: "200 OK",
                headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
                body: Vec::new(),
            };
        }
        MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
            body: payload_srv.as_ref().clone(),
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let folder_a = dir.path().join("folder-a");
    let folder_b = dir.path().join("folder-b");
    let folder_c = dir.path().join("folder-c");
    std::fs::create_dir_all(&folder_a).unwrap();
    std::fs::create_dir_all(&folder_b).unwrap();
    std::fs::create_dir_all(&folder_c).unwrap();

    let engine = DownloadEngine::open(&db_path).await.unwrap();

    let mut handle1 = engine
        .start(DownloadRequest::new(
            format!("{base_url}/hello.txt"),
            &folder_a,
        ))
        .await
        .unwrap();
    let id1 = handle1.id().clone();
    let outcome1 = handle1.wait().await.unwrap();
    assert!(outcome1.final_path.exists());

    // Multi-algorithm checksum verification
    let checksums = engine.compute_file_checksums(&id1).await.unwrap();
    assert_eq!(
        checksums.blake3,
        blake3::hash(payload.as_slice()).to_hex().to_string()
    );
    assert_eq!(
        checksums.sha256,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    assert_eq!(
        checksums.sha1,
        "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed"
    );
    assert_eq!(checksums.md5, "5eb63bbbe01eeed093cb22bb8f5acdc3");
    assert_eq!(checksums.crc32, "0d4a1185");
    assert_eq!(checksums.sha512.len(), 128);

    let (algo, hex) = parse_expected_checksum(&format!("sha256:{}", checksums.sha256)).unwrap();
    assert_eq!(algo, Some(ChecksumAlgorithm::Sha256));
    assert_eq!(
        checksums.match_expected(algo, &hex),
        Some(ChecksumAlgorithm::Sha256)
    );
    assert_eq!(
        checksums.match_expected(None, &checksums.blake3),
        Some(ChecksumAlgorithm::Blake3)
    );
    assert_eq!(checksums.match_expected(None, &"0".repeat(64)), None);

    // Duplicate URL lookup
    let found = engine
        .find_download_by_url(&format!("{base_url}/hello.txt"))
        .await
        .unwrap()
        .expect("record should be indexed by URL");
    assert_eq!(found.id, id1);

    // Move completed file into folder_c and enqueue a paused transfer in folder_b
    let moved_path = engine.move_completed_file(&id1, &folder_c).await.unwrap();
    assert!(moved_path.exists());

    let id2 = engine
        .enqueue_paused(DownloadRequest::new(
            format!("{base_url}/hello-2.txt"),
            &folder_b,
        ))
        .await
        .unwrap();

    // Recent folder history should contain all three confirmed/moved directories without duplicates.
    let recent = engine.list_recent_folders(10).await.unwrap();
    assert_eq!(recent.len(), 3);
    assert!(recent.contains(&folder_a));
    assert!(recent.contains(&folder_b));
    assert!(recent.contains(&folder_c));

    // Delete entry without deleting file on disk.
    engine.delete_with_mode(&id1, None).await.unwrap();
    assert!(
        moved_path.exists(),
        "file must remain on disk when file_mode is None"
    );

    // Resume second download, complete it, and delete permanently without touching Recycle Bin.
    let mut handle2 = engine.resume(&id2).await.unwrap();
    let outcome2 = handle2.wait().await.unwrap();
    assert!(outcome2.final_path.exists());
    engine
        .delete_with_mode(&id2, Some(DeleteFileMode::Permanent))
        .await
        .unwrap();
    assert!(
        !outcome2.final_path.exists(),
        "Permanent delete must remove disk file"
    );
}

#[tokio::test]
async fn reserved_paths_protect_concurrent_duplicates_and_folder_history_prunes_cleanly() {
    let payload = Arc::new(b"duplicate-guard-payload".to_vec());
    let payload_srv = Arc::clone(&payload);

    let base_url = spawn_mock_server(move |method, path, _| {
        if path != "/pack.zip" {
            return MockResponse {
                status: "404 Not Found",
                headers: vec![("Content-Length".into(), "0".into())],
                body: Vec::new(),
            };
        }
        if method == "HEAD" {
            return MockResponse {
                status: "200 OK",
                headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
                body: Vec::new(),
            };
        }
        MockResponse {
            status: "200 OK",
            headers: vec![("Content-Length".into(), payload_srv.len().to_string())],
            body: payload_srv.as_ref().clone(),
        }
    })
    .await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let save_dir = dir.path().join("Saves");
    let extra_dir = dir.path().join("TempGone");
    std::fs::create_dir_all(&save_dir).unwrap();
    std::fs::create_dir_all(&extra_dir).unwrap();

    let engine = DownloadEngine::open(&db_path).await.unwrap();

    // Two paused downloads claiming the exact same explicit path must receive distinct paths
    // even before any .part file touches disk.
    let explicit_target = save_dir.join("pack.zip");
    let mut req1 = DownloadRequest::new(format!("{base_url}/pack.zip#part1"), &save_dir);
    req1.output_path = Some(explicit_target.clone());
    let dup1 = engine.enqueue_paused(req1).await.unwrap();

    let mut req2 = DownloadRequest::new(format!("{base_url}/pack.zip"), &save_dir);
    req2.output_path = Some(explicit_target.clone());
    let dup2 = engine.enqueue_paused(req2).await.unwrap();

    let rec1 = engine.get_download(&dup1).await.unwrap().unwrap();
    let rec2 = engine.get_download(&dup2).await.unwrap().unwrap();
    assert_eq!(rec1.output_path.as_ref(), Some(&explicit_target));
    assert_eq!(
        rec2.output_path,
        Some(save_dir.join("pack (1).zip")),
        "second queued/paused copy must disambiguate cleanly without colliding"
    );

    // Rotating token URL + disambiguated `(1)` filename still finds the ongoing/paused duplicate!
    let found_rotated = engine
        .find_duplicate_download(
            &format!("{base_url}/pack.zip?token=rotated"),
            Some("pack (2).zip"),
            None,
            Some(&save_dir),
        )
        .await
        .unwrap()
        .expect("canonical filename fallback should match paused copy despite (n) suffix and rotating URL");
    assert!(matches!(
        found_rotated.state,
        posthaze_engine::DownloadState::Paused
    ));

    // Fragment-stripped URL match finds paused duplicate; distinct query token does not match.
    let found = engine
        .find_download_by_url(&format!("{base_url}/pack.zip"))
        .await
        .unwrap()
        .expect("fragment-stripped URL should match paused entry");
    assert!(matches!(
        found.state,
        posthaze_engine::DownloadState::Paused
    ));
    assert!(
        engine
            .find_download_by_url(&format!("{base_url}/pack.zip?token=other"))
            .await
            .unwrap()
            .is_none()
    );

    // Folder history records trailing-slash / deleted folders cleanly and supports clear_recent_folders.
    let mut req_extra = DownloadRequest::new(format!("{base_url}/pack.zip?v=2"), &extra_dir);
    req_extra.output_path = Some(extra_dir.join("pack2.zip"));
    let _ = engine.enqueue_paused(req_extra).await.unwrap();

    std::fs::remove_dir_all(&extra_dir).unwrap();
    let listed = engine.list_recent_folders(10).await.unwrap();
    assert_eq!(listed, vec![save_dir]);

    engine.clear_recent_folders().await.unwrap();
    assert!(engine.list_recent_folders(10).await.unwrap().is_empty());
}
