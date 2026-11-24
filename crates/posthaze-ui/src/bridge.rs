//! Dedicated background multi-thread Tokio runtime bridge for `DownloadEngine` (ADR-0010).

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use posthaze_engine::{
    DeleteFileMode, DownloadEngine, DownloadId, DownloadRecord, DownloadRequest, DownloadState,
    EngineError, EngineEvent, FileChecksums, PreflightInfo, SegmentProgress, Settings,
};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{broadcast, oneshot};

/// Bundled duplicate-check and folder-history context loaded for Pre-Flight windows.
#[derive(Debug, Clone)]
pub(crate) struct PreflightContext {
    pub duplicate: Option<DownloadRecord>,
    pub duplicate_file_exists: bool,
    pub recent_folders: Vec<PathBuf>,
}

/// Live conflict check snapshot run right before confirming a Pre-Flight download.
#[derive(Debug, Clone)]
pub(crate) struct ConfirmConflictCheck {
    pub duplicate: Option<DownloadRecord>,
    pub reserved_paths: Vec<PathBuf>,
}

/// Consistent point-in-time read of all `Download` records, `Settings`, and persisted `Segment` counts.
#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub records: Vec<DownloadRecord>,
    pub settings: Settings,
    pub segment_counts: HashMap<DownloadId, usize>,
}

/// Typed mutating commands executed through [`EngineBridge`].
#[derive(Debug, Clone)]
pub enum EngineCommand {
    Start {
        request: DownloadRequest,
    },
    EnqueuePaused {
        request: DownloadRequest,
    },
    Pause(DownloadId),
    Resume(DownloadId),
    Cancel(DownloadId),
    RestartFresh(DownloadId),
    Delete {
        id: DownloadId,
        file_mode: Option<DeleteFileMode>,
    },
    UpdateSettings(Settings),
    RenameFile {
        id: DownloadId,
        new_name: String,
    },
    MoveFile {
        id: DownloadId,
        target_dir: PathBuf,
    },
    ResumeMany(Vec<DownloadId>),
    PauseMany(Vec<DownloadId>),
    /// Delete multiple `Download`s from the `Queue` (and optionally their files on disk).
    DeleteMany {
        ids: Vec<DownloadId>,
        file_mode: Option<DeleteFileMode>,
    },
}

/// Outcome of executing an [`EngineCommand`], carrying the refreshed [`EngineSnapshot`].
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub snapshot: EngineSnapshot,
    pub created_id: Option<DownloadId>,
}

/// Bridges a background multi-thread Tokio [`DownloadEngine`] into GPUI's async executor.
#[derive(Clone)]
pub struct EngineBridge {
    runtime: Arc<Runtime>,
    engine: DownloadEngine,
}

impl EngineBridge {
    /// Starts the dedicated Tokio runtime and opens [`DownloadEngine`] at `db_path`.
    pub fn open(db_path: PathBuf) -> Result<Self, EngineError> {
        let worker_threads = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 4))
            .unwrap_or(2);
        let runtime = Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .max_blocking_threads(16)
            .thread_keep_alive(std::time::Duration::from_secs(5))
            .enable_all()
            .thread_name("posthaze-engine-rt")
            .build()
            .map_err(EngineError::Io)?;
        let engine = runtime.block_on(DownloadEngine::open(db_path))?;
        Ok(Self {
            runtime: Arc::new(runtime),
            engine,
        })
    }

    /// Subscribes to engine-wide [`EngineEvent`] notifications.
    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.engine.subscribe_events()
    }

    /// Returns a handle to the background Tokio runtime.
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// Returns a clone of the engine's proxy-configured HTTP client.
    pub fn http_client(&self) -> reqwest::Client {
        self.engine.http_client()
    }

    /// Reads persisted [`Settings`] synchronously on the engine Tokio runtime.
    ///
    /// Used before opening the first GPUI window so frame 1 starts with the saved
    /// theme and `WindowBackgroundAppearance` without an opaque flash.
    pub fn load_settings_sync(&self) -> Result<Settings, EngineError> {
        self.runtime.block_on(self.engine.get_settings())
    }

    /// Loads a consistent [`EngineSnapshot`] of records, settings, and persisted `Segment`s.
    pub fn load_snapshot(&self) -> oneshot::Receiver<Result<EngineSnapshot, EngineError>> {
        self.spawn_task(|engine| async move { read_snapshot(&engine).await })
    }

    /// Loads persisted [`SegmentProgress`] entries for a single `Download`.
    pub fn load_segments(
        &self,
        id: DownloadId,
    ) -> oneshot::Receiver<Result<(DownloadId, Vec<SegmentProgress>), EngineError>> {
        self.spawn_task(|engine| async move {
            let segs = engine.list_segments(&id).await?;
            Ok((id, segs))
        })
    }

    /// Loads the latest persisted [`DownloadRecord`] for a single `DownloadId`.
    pub fn load_download(
        &self,
        id: DownloadId,
    ) -> oneshot::Receiver<Result<Option<DownloadRecord>, EngineError>> {
        self.spawn_task(|engine| async move { engine.get_download(&id).await })
    }

    /// Runs a non-mutating pre-flight probe with optional custom headers.
    pub fn probe(
        &self,
        url: String,
        output_dir: PathBuf,
        headers: Vec<(String, String)>,
    ) -> oneshot::Receiver<Result<PreflightInfo, EngineError>> {
        self.spawn_task(|engine| async move {
            engine.probe_with_headers(url, output_dir, &headers).await
        })
    }

    /// Computes standard + `BLAKE3` digests of a completed `Download`'s file in one pass.
    pub fn compute_checksums(
        &self,
        id: DownloadId,
    ) -> oneshot::Receiver<Result<FileChecksums, EngineError>> {
        self.spawn_task(|engine| async move { engine.compute_file_checksums(&id).await })
    }

    /// Lists up to `limit` most recently used existing destination folders.
    pub fn recent_folders(
        &self,
        limit: usize,
    ) -> oneshot::Receiver<Result<Vec<PathBuf>, EngineError>> {
        self.spawn_task(|engine| async move { engine.list_recent_folders(limit).await })
    }

    /// Clears all persisted destination folders from history.
    pub fn clear_folder_history(&self) -> oneshot::Receiver<Result<(), EngineError>> {
        self.spawn_task(|engine| async move { engine.clear_recent_folders().await })
    }

    /// Loads duplicate-URL/file info and verified recent folders together.
    pub(crate) fn preflight_context(
        &self,
        urls: Vec<String>,
        filename: Option<String>,
        content_length: Option<u64>,
        output_dir: Option<PathBuf>,
        folder_limit: usize,
    ) -> oneshot::Receiver<Result<PreflightContext, EngineError>> {
        self.spawn_task(move |engine| async move {
            let duplicate = pick_best_duplicate(
                &engine,
                &urls,
                filename.as_deref(),
                content_length,
                output_dir.as_deref(),
            )
            .await?;
            let dup_path = duplicate.as_ref().and_then(|d| d.output_path.clone());
            let duplicate_file_exists = match dup_path {
                Some(path) => tokio::task::spawn_blocking(move || path.is_file())
                    .await
                    .unwrap_or(false),
                None => false,
            };
            let recent_folders = engine.list_recent_folders(folder_limit).await?;
            Ok(PreflightContext {
                duplicate,
                duplicate_file_exists,
                recent_folders,
            })
        })
    }

    /// Re-checks live duplicate URLs/files and reserved paths immediately before queueing a download.
    pub(crate) fn check_confirm_conflicts(
        &self,
        urls: Vec<String>,
        filename: Option<String>,
        content_length: Option<u64>,
        output_dir: Option<PathBuf>,
    ) -> oneshot::Receiver<Result<ConfirmConflictCheck, EngineError>> {
        self.spawn_task(move |engine| async move {
            let duplicate = pick_best_duplicate(
                &engine,
                &urls,
                filename.as_deref(),
                content_length,
                output_dir.as_deref(),
            )
            .await?;
            let reserved_paths = engine.list_reserved_output_paths().await?;
            Ok(ConfirmConflictCheck {
                duplicate,
                reserved_paths,
            })
        })
    }

    /// Executes a typed [`EngineCommand`] and returns the refreshed [`EngineSnapshot`].
    pub fn execute(
        &self,
        command: EngineCommand,
    ) -> oneshot::Receiver<Result<CommandOutcome, EngineError>> {
        self.spawn_task(|engine| async move {
            let created_id = match command {
                EngineCommand::Start { request } => {
                    let handle = engine.start(request).await?;
                    Some(handle.id().clone())
                }
                EngineCommand::EnqueuePaused { request } => {
                    Some(engine.enqueue_paused(request).await?)
                }
                EngineCommand::Pause(id) => {
                    engine.pause(&id).await?;
                    None
                }
                EngineCommand::Resume(id) => {
                    let _ = engine.resume(&id).await?;
                    None
                }
                EngineCommand::Cancel(id) => {
                    engine.cancel(&id).await?;
                    None
                }
                EngineCommand::RestartFresh(id) => {
                    let _ = engine.restart_fresh(&id).await?;
                    None
                }
                EngineCommand::Delete { id, file_mode } => {
                    engine.delete_with_mode(&id, file_mode).await?;
                    None
                }
                EngineCommand::UpdateSettings(settings) => {
                    engine.update_settings(settings).await?;
                    None
                }
                EngineCommand::RenameFile { id, new_name } => {
                    let _ = engine.rename_completed_file(&id, &new_name).await?;
                    None
                }
                EngineCommand::MoveFile { id, target_dir } => {
                    let _ = engine.move_completed_file(&id, target_dir).await?;
                    None
                }
                EngineCommand::ResumeMany(ids) => {
                    for id in ids {
                        let _ = engine.resume(&id).await;
                    }
                    None
                }
                EngineCommand::PauseMany(ids) => {
                    for id in ids {
                        let _ = engine.pause(&id).await;
                    }
                    None
                }
                EngineCommand::DeleteMany { ids, file_mode } => {
                    for id in ids {
                        let _ = engine.delete_with_mode(&id, file_mode).await;
                    }
                    None
                }
            };

            let snapshot = read_snapshot(&engine).await?;
            Ok(CommandOutcome {
                snapshot,
                created_id,
            })
        })
    }

    fn spawn_task<F, Fut, T>(&self, make_fut: F) -> oneshot::Receiver<T>
    where
        F: FnOnce(DownloadEngine) -> Fut,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let engine = self.engine.clone();
        let (tx, rx) = oneshot::channel();
        let fut = make_fut(engine);
        self.runtime.spawn(async move {
            let out = fut.await;
            let _ = tx.send(out);
        });
        rx
    }
}

async fn pick_best_duplicate(
    engine: &DownloadEngine,
    urls: &[String],
    filename: Option<&str>,
    content_length: Option<u64>,
    output_dir: Option<&std::path::Path>,
) -> Result<Option<DownloadRecord>, EngineError> {
    let rank = |s: DownloadState| match s {
        DownloadState::Active | DownloadState::Probing => 0u8,
        DownloadState::Queued => 1,
        DownloadState::Paused => 2,
        DownloadState::Failed => 3,
        DownloadState::Completed => 4,
        DownloadState::Cancelled => 5,
    };
    let mut best: Option<DownloadRecord> = None;
    let mut seen = Vec::with_capacity(urls.len());
    for url in urls {
        let trimmed = url.trim();
        if trimmed.is_empty() || seen.iter().any(|u: &String| u == trimmed) {
            continue;
        }
        seen.push(trimmed.to_string());
        if let Some(rec) = engine.find_download_by_url(trimmed).await? {
            let take = match &best {
                None => true,
                Some(cur) => {
                    (rank(rec.state), std::cmp::Reverse(rec.updated_at))
                        < (rank(cur.state), std::cmp::Reverse(cur.updated_at))
                }
            };
            if take {
                best = Some(rec);
            }
        }
    }
    if best.is_none()
        && let Some(first_url) = seen.first()
    {
        best = engine
            .find_duplicate_download(first_url, filename, content_length, output_dir)
            .await?;
    }
    Ok(best)
}

async fn read_snapshot(engine: &DownloadEngine) -> Result<EngineSnapshot, EngineError> {
    let records = engine.list_downloads().await?;
    let settings = engine.get_settings().await?;
    let segment_counts = engine.list_segment_counts().await?;
    Ok(EngineSnapshot {
        records,
        settings,
        segment_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_update_settings_returns_atomic_snapshot() {
        let temp_dir = std::env::temp_dir().join(format!("posthaze_bridge_test_{}", DownloadId::new()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("state.db");

        let bridge = EngineBridge::open(db_path).expect("bridge should open");
        let initial = bridge
            .load_snapshot()
            .blocking_recv()
            .expect("channel ok")
            .expect("snapshot ok");
        assert!(initial.records.is_empty());
        assert!(initial.segment_counts.is_empty());

        let mut updated = initial.settings.clone();
        updated.max_concurrent_downloads = 5;
        let outcome = bridge
            .execute(EngineCommand::UpdateSettings(updated))
            .blocking_recv()
            .expect("channel ok")
            .expect("update settings ok");
        assert_eq!(outcome.created_id, None);
        assert_eq!(outcome.snapshot.settings.max_concurrent_downloads, 5);

        drop(bridge);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn load_settings_sync_reads_persisted_theme_and_accent_on_reopen() {
        let temp_dir =
            std::env::temp_dir().join(format!("posthaze_bridge_sync_test_{}", DownloadId::new()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("state.db");

        {
            let bridge = EngineBridge::open(db_path.clone()).expect("bridge should open");
            let mut settings = bridge.load_settings_sync().expect("initial settings ok");
            settings.ui_theme = "mica-system".to_string();
            settings.ui_accent_override = Some("blue".to_string());
            bridge
                .execute(EngineCommand::UpdateSettings(settings))
                .blocking_recv()
                .expect("channel ok")
                .expect("update settings ok");
        }

        let reopened = EngineBridge::open(db_path).expect("bridge should reopen");
        let loaded = reopened
            .load_settings_sync()
            .expect("persisted settings should load synchronously");
        assert_eq!(loaded.ui_theme, "mica-system");
        assert_eq!(loaded.ui_accent_override.as_deref(), Some("blue"));

        drop(reopened);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
