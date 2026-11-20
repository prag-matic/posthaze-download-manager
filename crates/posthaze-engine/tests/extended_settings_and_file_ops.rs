mod common;

use std::sync::Arc;
use std::time::Instant;

use common::MockResponse;
use posthaze_engine::{
    CollisionPolicy, DeleteDefaultAction, DownloadEngine, DownloadRequest, ProxyMode, Settings,
    categorize_subfolder,
};
use tempfile::tempdir;

#[tokio::test]
async fn extended_settings_round_trip_and_categorize_helpers() {
    assert_eq!(categorize_subfolder("bundle.tar.gz"), Some("Archives"));
    assert_eq!(categorize_subfolder("clip.mp4"), Some("Video"));
    assert_eq!(categorize_subfolder("track.flac"), Some("Audio"));
    assert_eq!(categorize_subfolder("manual.pdf"), Some("Documents"));
    assert_eq!(categorize_subfolder("setup.msi"), Some("Programs"));
    assert_eq!(categorize_subfolder("blob.bin"), None);

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let custom = Settings {
        default_output_dir: dir.path().join("dl"),
        max_workers: 6,
        max_concurrent_downloads: 4,
        sync_on_complete: true,
        proxy_mode: ProxyMode::None,
        proxy_url: String::new(),
        download_speed_limit_kib: 1024,
        auto_categorize_folders: true,
        collision_policy: CollisionPolicy::Overwrite,
        delete_action_default: DeleteDefaultAction::DeleteFile,
        auto_retry_count: 3,
        completion_cmd_enabled: true,
        completion_cmd_path: "MpCmdRun.exe".to_string(),
        completion_cmd_args: "-Scan -ScanType 3 -File \"%path%\"".to_string(),
        notify_on_added: true,
        notify_on_completed: true,
        notify_on_failed: false,
        notify_only_inactive: false,
        ui_click_row_opens_inspector: false,
        ui_show_add_paused: false,
        ui_zoom_percent: 110,
        ui_theme: "cyanotype".to_string(),
        ui_table_density: posthaze_engine::TableDensity::Compact,
        ui_show_speed_col: false,
        ui_show_eta_col: true,
        ui_show_segs_col: false,
        ui_show_toolbar_labels: false,
        ui_show_scrollbar_minimap: false,
        ui_show_titlebar_telemetry: false,
        ui_start_sidebar_collapsed: true,
        ui_accent_override: Some("emerald".to_string()),
        ui_lock_file_extension: false,
        ui_close_to_tray: false,
        launch_on_startup: true,
    };

    {
        let engine = DownloadEngine::open(&db_path).await.unwrap();
        engine.update_settings(custom.clone()).await.unwrap();
        assert_eq!(engine.get_settings().await.unwrap(), custom);
    }

    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    assert_eq!(reopened.get_settings().await.unwrap(), custom);
}

#[tokio::test]
async fn file_integrity_rename_move_and_throttled_download_work() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let engine = DownloadEngine::open(&db_path).await.unwrap();

    // 64 KiB/s limit on a 64 KiB payload should take >= ~0.5s while verifying exact BLAKE3.
    let settings = Settings {
        default_output_dir: dir.path().to_path_buf(),
        download_speed_limit_kib: 64,
        ..Settings::default()
    };
    engine.update_settings(settings).await.unwrap();

    let payload = vec![0x3Cu8; 64 * 1024];
    let expected_hash = blake3::hash(&payload).to_hex().to_string();
    let payload_srv = Arc::new(payload);

    let base_url = common::spawn_mock_server(move |method, _path, _range| {
        let len = payload_srv.len();
        MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".to_string(), len.to_string()),
                ("Accept-Ranges".to_string(), "none".to_string()),
            ],
            body: if method == "HEAD" {
                Vec::new()
            } else {
                payload_srv.as_ref().clone()
            },
        }
    })
    .await;

    let req = DownloadRequest::new(format!("{base_url}/sample.bin"), dir.path());
    let started = Instant::now();
    let mut handle = engine.start(req).await.unwrap();
    let outcome = handle.wait().await.unwrap();
    assert!(started.elapsed().as_millis() >= 350);

    let id = handle.id().clone();
    let digest = engine.compute_file_blake3(&id).await.unwrap();
    assert_eq!(digest, expected_hash);

    let renamed = engine
        .rename_completed_file(&id, "renamed.bin")
        .await
        .unwrap();
    assert!(renamed.exists());
    assert!(!outcome.final_path.exists());
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().output_path,
        Some(renamed.clone())
    );

    let moved_dir = dir.path().join("moved");
    let moved = engine.move_completed_file(&id, &moved_dir).await.unwrap();
    assert!(moved.exists());
    assert!(!renamed.exists());
    assert_eq!(
        engine.get_download(&id).await.unwrap().unwrap().output_path,
        Some(moved)
    );
}
