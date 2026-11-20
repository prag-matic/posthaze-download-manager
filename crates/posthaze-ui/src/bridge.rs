//! Dedicated background multi-thread Tokio runtime bridge for `DownloadEngine` (ADR-0010).

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use posthaze_engine::{
    DownloadEngine, DownloadId, DownloadRecord, DownloadRequest, EngineError, EngineEvent,
    PreflightInfo, SegmentProgress, Settings,
};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{broadcast, oneshot};

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
        delete_file: bool,
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
        delete_file: bool,
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
        let runtime = Builder::new_multi_thread()
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

    /// Computes the `BLAKE3` hex digest of a completed `Download`'s file.
    pub fn compute_blake3(&self, id: DownloadId) -> oneshot::Receiver<Result<String, EngineError>> {
        self.spawn_task(|engine| async move { engine.compute_file_blake3(&id).await })
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
                EngineCommand::Delete { id, delete_file } => {
                    engine.delete(&id, delete_file).await?;
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
                EngineCommand::DeleteMany { ids, delete_file } => {
                    for id in ids {
                        let _ = engine.delete(&id, delete_file).await;
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
}
