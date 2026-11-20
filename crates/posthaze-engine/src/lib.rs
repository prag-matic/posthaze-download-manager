//! Core headless HTTP/HTTPS download engine for Posthaze Download Manager.

#![deny(missing_docs)]

mod model;
mod probe;
mod queue;
mod store;
mod throttle;
mod transfer;
/// Dedicated blocking `.part` file writer used by `DownloadEngine` and benchmarks.
pub mod writer;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, broadcast, mpsc, watch};

use crate::model::{PlannedSegment, SegmentCheckpoint};
use crate::probe::{ProbeResult, probe_url_with_headers};
use crate::queue::QueueCoordinator;
use crate::store::DownloadStore;
use crate::throttle::BandwidthLimiter;
use crate::transfer::{
    ControlSignal, LiveSegments, MultiSegmentResult, execute_multi_segment,
    execute_multi_segment_from_checkpoints, execute_single_stream, plan_segments,
};
use crate::writer::PartFileWriter;

pub use crate::model::*;
pub use crate::store::DownloadRecord;

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
struct ActiveControl {
    control_tx: watch::Sender<ControlSignal>,
    done_rx: watch::Receiver<bool>,
}

/// Control and observation handle for an active `Download`.
#[derive(Debug)]
pub struct DownloadHandle {
    id: DownloadId,
    control_tx: watch::Sender<ControlSignal>,
    progress_rx: watch::Receiver<ProgressSnapshot>,
    join_handle: tokio::task::JoinHandle<Result<DownloadOutcome, EngineError>>,
}

impl DownloadHandle {
    /// Returns the persistent [`DownloadId`] for this `Download`.
    pub fn id(&self) -> &DownloadId {
        &self.id
    }

    /// Signals this `Download` to pause gracefully, flushing buffered `Chunk`s and saving a `Checkpoint`.
    pub fn pause(&self) {
        let _ = self.control_tx.send(ControlSignal::Pause);
    }

    /// Signals this `Download` to cancel immediately and delete its `.part` file.
    pub fn cancel(&self) {
        let _ = self.control_tx.send(ControlSignal::Cancel);
    }

    /// Subscribes to live [`ProgressSnapshot`] updates for this `Download`.
    pub fn subscribe(&self) -> watch::Receiver<ProgressSnapshot> {
        self.progress_rx.clone()
    }

    /// Awaits completion of this `Download` and returns its [`DownloadOutcome`].
    ///
    /// This method is cancellation-safe until it resolves once, allowing callers to invoke
    /// [`DownloadHandle::pause`] or [`DownloadHandle::cancel`] if a `tokio::select!` branch wins.
    pub async fn wait(&mut self) -> Result<DownloadOutcome, EngineError> {
        (&mut self.join_handle)
            .await
            .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }
}

/// Headless HTTP/HTTPS download engine.
#[derive(Debug, Clone)]
pub struct DownloadEngine {
    client: Arc<std::sync::RwLock<reqwest::Client>>,
    limiter: Arc<BandwidthLimiter>,
    store: Option<DownloadStore>,
    active: Arc<Mutex<HashMap<DownloadId, ActiveControl>>>,
    queue: Arc<QueueCoordinator>,
    ephemeral_settings: Arc<Mutex<Settings>>,
    event_tx: broadcast::Sender<EngineEvent>,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine {
    fn with_store(store: Option<DownloadStore>, settings: Settings) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let client = build_http_client(settings.proxy_mode, &settings.proxy_url)
            .unwrap_or_else(|_| build_http_client(ProxyMode::System, "").unwrap());
        Self {
            client: Arc::new(std::sync::RwLock::new(client)),
            limiter: Arc::new(BandwidthLimiter::new(settings.download_speed_limit_kib)),
            store,
            active: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(QueueCoordinator::new(settings.max_concurrent_downloads)),
            ephemeral_settings: Arc::new(Mutex::new(settings)),
            event_tx,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        self.client
            .read()
            .expect("http client rwlock poisoned")
            .clone()
    }

    /// Constructs a new ephemeral [`DownloadEngine`] without SQLite persistence.
    pub fn new() -> Self {
        Self::with_store(None, Settings::default())
    }

    /// Opens or creates a SQLite-backed [`DownloadEngine`] at `db_path`.
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let store = DownloadStore::open(db_path.as_ref()).await?;
        let settings = store.get_settings().await?;
        Ok(Self::with_store(Some(store), settings))
    }

    /// Constructs an ephemeral in-memory SQLite-backed [`DownloadEngine`].
    pub async fn in_memory() -> Result<Self, EngineError> {
        let store = DownloadStore::in_memory().await?;
        let settings = store.get_settings().await?;
        Ok(Self::with_store(Some(store), settings))
    }

    /// Subscribes to the engine-wide [`EngineEvent`] broadcast stream.
    ///
    /// Subscribers that fall behind the `1024`-event buffer (`RecvError::Lagged`) should
    /// reconcile their view via [`DownloadEngine::list_downloads`].
    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    /// Probes `url` without creating any database record or `.part` file.
    pub async fn probe(
        &self,
        url: impl Into<String>,
        output_dir: impl AsRef<Path>,
    ) -> Result<PreflightInfo, EngineError> {
        self.probe_with_headers(url, output_dir, &[]).await
    }

    /// Probes `url` with custom HTTP request `headers` without creating any database record or `.part` file.
    pub async fn probe_with_headers(
        &self,
        url: impl Into<String>,
        output_dir: impl AsRef<Path>,
        headers: &[(String, String)],
    ) -> Result<PreflightInfo, EngineError> {
        let original_url = url.into();
        let client = self.http_client();
        let probed = probe_url_with_headers(&client, &original_url, headers).await?;
        let disambiguated_output_path =
            disambiguate_output_path(output_dir.as_ref(), &probed.suggested_filename);
        Ok(PreflightInfo {
            original_url,
            final_url: probed.final_url,
            suggested_filename: probed.suggested_filename,
            disambiguated_output_path,
            content_length: probed.content_length,
            supports_ranges: probed.supports_ranges,
            etag: probed.etag,
            last_modified: probed.last_modified,
        })
    }

    /// Starts a `Download` asynchronously and returns a [`DownloadHandle`].
    pub async fn start(&self, request: DownloadRequest) -> Result<DownloadHandle, EngineError> {
        let id = DownloadId::new();
        if let Some(store) = &self.store {
            store.insert_queued(&id, &request).await?;
        }
        Ok(self.spawn_fresh_download(id, request).await)
    }

    /// Stages a new `Download` in [`DownloadState::Paused`] without starting network transfer.
    ///
    /// The staged record can be started later via [`Self::resume`].
    pub async fn enqueue_paused(&self, request: DownloadRequest) -> Result<DownloadId, EngineError> {
        let Some(store) = &self.store else {
            return Err(EngineError::InvalidState(
                "cannot stage paused download without persistence".to_string(),
            ));
        };
        let id = DownloadId::new();
        store.insert_paused(&id, &request).await?;
        let _ = self.event_tx.send(EngineEvent::StateChanged {
            id: id.clone(),
            state: DownloadState::Paused,
            error: None,
        });
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(id)
    }

    async fn spawn_fresh_download(&self, id: DownloadId, request: DownloadRequest) -> DownloadHandle {
        let (progress_tx, progress_rx) = watch::channel(ProgressSnapshot::status(
            DownloadState::Queued,
            0,
            None,
            0,
        ));
        let (control_tx, control_rx) = watch::channel(ControlSignal::Run);
        let (done_tx, done_rx) = watch::channel(false);

        {
            let mut active = self.active.lock().await;
            active.insert(
                id.clone(),
                ActiveControl {
                    control_tx: control_tx.clone(),
                    done_rx,
                },
            );
        }

        let client = self.http_client();
        let limiter = Arc::clone(&self.limiter);
        let store = self.store.clone();
        let active_map = Arc::clone(&self.active);
        let queue = Arc::clone(&self.queue);
        let event_tx = self.event_tx.clone();
        let task_id = id.clone();
        let sink = ProgressSink::new(id.clone(), Some(progress_tx), event_tx.clone());

        queue.enqueue(id.clone());
        let _ = event_tx.send(EngineEvent::StateChanged {
            id: id.clone(),
            state: DownloadState::Queued,
            error: None,
        });
        let _ = event_tx.send(EngineEvent::QueueUpdated);

        let join_handle = tokio::spawn(async move {
            let mut control_rx = control_rx;
            let res = match queue.wait_for_slot(&task_id, &mut control_rx).await {
                Ok(_slot) => {
                    let res = run_download(
                        client,
                        limiter,
                        store,
                        task_id.clone(),
                        request,
                        sink,
                        control_rx,
                    )
                    .await;
                    active_map.lock().await.remove(&task_id);
                    res
                }
                Err(err) => {
                    active_map.lock().await.remove(&task_id);
                    finalize_queued_interruption(
                        store.as_ref(),
                        &task_id,
                        None,
                        0,
                        None,
                        &sink,
                        err,
                    )
                    .await
                }
            };
            let _ = event_tx.send(EngineEvent::QueueUpdated);
            let _ = done_tx.send(true);
            res
        });

        DownloadHandle {
            id,
            control_tx,
            progress_rx,
            join_handle,
        }
    }

    /// Resumes a persisted `Paused` or resumable `Failed` `Download` by [`DownloadId`].
    pub async fn resume(&self, id: &DownloadId) -> Result<DownloadHandle, EngineError> {
        {
            let active = self.active.lock().await;
            if active.contains_key(id) {
                return Err(EngineError::InvalidState(format!(
                    "download {id} is already active"
                )));
            }
        }

        let Some(store) = &self.store else {
            return Err(EngineError::NotFound(id.to_string()));
        };

        let record = store
            .get_download(id)
            .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
            .ok_or_else(|| EngineError::NotFound(id.to_string()))?;

        match record.state {
            DownloadState::Paused | DownloadState::Failed => {}
            other => {
                return Err(EngineError::InvalidState(format!(
                    "cannot resume download in {} state",
                    other.as_str()
                )));
            }
        }

        let not_started_yet =
            record.part_path.is_none() && store.get_segments(id).await?.is_empty();

        let (segments, fresh_request) = if not_started_yet {
            let fallback_dir = match &record.output_dir {
                Some(dir) => dir.clone(),
                None => store.get_settings().await?.default_output_dir,
            };
            let req = DownloadRequest {
                url: record.url.clone(),
                output_path: record.output_path.clone(),
                output_dir: fallback_dir,
                max_workers: record.max_workers,
                sync_on_complete: record.sync_on_complete,
                headers: record.headers.clone(),
            };
            (Vec::new(), Some(req))
        } else {
            if !record.supports_ranges {
                return Err(EngineError::InvalidState(
                    "cannot resume download without server byte-range support".to_string(),
                ));
            }

            let Some(part_path) = &record.part_path else {
                return Err(EngineError::InvalidState(
                    "cannot resume download without a recorded .part file".to_string(),
                ));
            };
            if !part_path.exists() {
                return Err(EngineError::InvalidState(format!(
                    "missing .part file for resume: {}",
                    part_path.display()
                )));
            }

            let segments = store.get_segments(id).await?;
            if segments.is_empty() {
                return Err(EngineError::InvalidState(
                    "cannot resume download with no persisted segments".to_string(),
                ));
            }
            (segments, None)
        };

        store.set_state(id, DownloadState::Queued).await?;

        let (progress_tx, progress_rx) = watch::channel(ProgressSnapshot::status(
            DownloadState::Queued,
            record.bytes_downloaded,
            record.content_length,
            0,
        ));
        let (control_tx, control_rx) = watch::channel(ControlSignal::Run);
        let (done_tx, done_rx) = watch::channel(false);

        {
            let mut active = self.active.lock().await;
            active.insert(
                id.clone(),
                ActiveControl {
                    control_tx: control_tx.clone(),
                    done_rx,
                },
            );
        }

        let client = self.http_client();
        let limiter = Arc::clone(&self.limiter);
        let store = store.clone();
        let active_map = Arc::clone(&self.active);
        let queue = Arc::clone(&self.queue);
        let event_tx = self.event_tx.clone();
        let task_id = id.clone();
        let sink = ProgressSink::new(id.clone(), Some(progress_tx), event_tx.clone());

        queue.enqueue(id.clone());
        let _ = event_tx.send(EngineEvent::StateChanged {
            id: id.clone(),
            state: DownloadState::Queued,
            error: None,
        });
        let _ = event_tx.send(EngineEvent::QueueUpdated);

        let join_handle = tokio::spawn(async move {
            let mut control_rx = control_rx;
            let res = match queue.wait_for_slot(&task_id, &mut control_rx).await {
                Ok(_slot) => {
                    let res = if let Some(req) = fresh_request {
                        run_download(
                            client,
                            limiter,
                            Some(store),
                            task_id.clone(),
                            req,
                            sink,
                            control_rx,
                        )
                        .await
                    } else {
                        run_resumed_download(
                            client,
                            limiter,
                            store,
                            task_id.clone(),
                            record,
                            segments,
                            sink,
                            control_rx,
                        )
                        .await
                    };
                    active_map.lock().await.remove(&task_id);
                    res
                }
                Err(err) => {
                    active_map.lock().await.remove(&task_id);
                    finalize_queued_interruption(
                        Some(&store),
                        &task_id,
                        record.part_path.as_deref(),
                        record.bytes_downloaded,
                        record.content_length,
                        &sink,
                        err,
                    )
                    .await
                }
            };
            let _ = event_tx.send(EngineEvent::QueueUpdated);
            let _ = done_tx.send(true);
            res
        });

        Ok(DownloadHandle {
            id: id.clone(),
            control_tx,
            progress_rx,
            join_handle,
        })
    }

    async fn signal_and_wait_active(&self, id: &DownloadId, signal: ControlSignal) -> bool {
        let live = {
            let active = self.active.lock().await;
            active.get(id).cloned()
        };
        if let Some(ctrl) = live {
            let _ = ctrl.control_tx.send(signal);
            let mut done_rx = ctrl.done_rx;
            let _ = done_rx.wait_for(|&done| done).await;
            true
        } else {
            false
        }
    }

    async fn require_download_record(
        &self,
        id: &DownloadId,
    ) -> Result<(&DownloadStore, DownloadRecord), EngineError> {
        let Some(store) = &self.store else {
            return Err(EngineError::NotFound(id.to_string()));
        };
        let record = store
            .get_download(id)
            .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
            .ok_or_else(|| EngineError::NotFound(id.to_string()))?;
        Ok((store, record))
    }

    /// Cancels an active or persisted (`Paused`/`Failed`/`Queued`) `Download`, deleting its `.part` file.
    pub async fn cancel(&self, id: &DownloadId) -> Result<(), EngineError> {
        if self.signal_and_wait_active(id, ControlSignal::Cancel).await {
            return Ok(());
        }

        let (store, record) = self.require_download_record(id).await?;

        match record.state {
            DownloadState::Completed => {
                return Err(EngineError::InvalidState(
                    "cannot cancel a completed download".to_string(),
                ));
            }
            DownloadState::Cancelled => return Ok(()),
            DownloadState::Queued
            | DownloadState::Probing
            | DownloadState::Active
            | DownloadState::Paused
            | DownloadState::Failed => {}
        }

        if let Some(part_path) = record.part_path {
            remove_file_if_exists(&part_path).await?;
        }
        store.mark_cancelled(id).await?;
        let _ = self.event_tx.send(EngineEvent::StateChanged {
            id: id.clone(),
            state: DownloadState::Cancelled,
            error: None,
        });
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
    }

    /// Gracefully pauses an in-flight `Active` or `Queued` `Download` by [`DownloadId`].
    pub async fn pause(&self, id: &DownloadId) -> Result<(), EngineError> {
        if self.signal_and_wait_active(id, ControlSignal::Pause).await {
            if let Some(store) = &self.store
                && let Some(rec) = store.get_download(id).await?
                && rec.state != DownloadState::Paused
            {
                return Err(EngineError::InvalidState(format!(
                    "download finished in {} state before pause took effect",
                    rec.state.as_str()
                )));
            }
            return Ok(());
        }

        let (_, record) = self.require_download_record(id).await?;

        Err(EngineError::InvalidState(format!(
            "cannot pause download in {} state",
            record.state.as_str()
        )))
    }

    /// Deletes a `Download` record (stopping any active `Download`), its `.part` file, and optionally its final file.
    pub async fn delete(&self, id: &DownloadId, delete_file: bool) -> Result<(), EngineError> {
        let _ = self.signal_and_wait_active(id, ControlSignal::Cancel).await;
        let (store, record) = self.require_download_record(id).await?;

        if let Some(part_path) = &record.part_path {
            remove_file_if_exists(part_path).await?;
        }
        if delete_file && let Some(output_path) = &record.output_path {
            remove_file_if_exists(output_path).await?;
        }
        store.delete_download(id).await?;
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
    }

    /// Clears stale `.part` / `Segment` state for a `Paused`, `Failed`, or `Cancelled` `Download` and restarts from byte `0`.
    pub async fn restart_fresh(&self, id: &DownloadId) -> Result<DownloadHandle, EngineError> {
        {
            let active = self.active.lock().await;
            if active.contains_key(id) {
                return Err(EngineError::InvalidState(format!(
                    "download {id} is already active"
                )));
            }
        }

        let (store, record) = self.require_download_record(id).await?;

        match record.state {
            DownloadState::Paused | DownloadState::Failed | DownloadState::Cancelled => {}
            other => {
                return Err(EngineError::InvalidState(format!(
                    "cannot restart download in {} state",
                    other.as_str()
                )));
            }
        }

        if let Some(part_path) = &record.part_path {
            remove_file_if_exists(part_path).await?;
        }
        store.reset_for_fresh_restart(id).await?;

        let fallback_dir = match &record.output_dir {
            Some(dir) => dir.clone(),
            None => store.get_settings().await?.default_output_dir,
        };
        let req = DownloadRequest {
            url: record.url,
            output_path: record.output_path,
            output_dir: fallback_dir,
            max_workers: record.max_workers,
            sync_on_complete: record.sync_on_complete,
            headers: record.headers,
        };

        Ok(self.spawn_fresh_download(id.clone(), req).await)
    }

    /// Lists all persisted `DownloadRecord`s ordered newest-first.
    pub async fn list_downloads(&self) -> Result<Vec<DownloadRecord>, EngineError> {
        match &self.store {
            Some(store) => store.list_downloads().await,
            None => Ok(Vec::new()),
        }
    }

    /// Fetches a single persisted `DownloadRecord` by [`DownloadId`].
    pub async fn get_download(&self, id: &DownloadId) -> Result<Option<DownloadRecord>, EngineError> {
        match &self.store {
            Some(store) => store.get_download(id).await,
            None => Ok(None),
        }
    }

    /// Loads persisted `Segment` `Checkpoint`s for a `Download` as [`SegmentProgress`] entries.
    pub async fn list_segments(&self, id: &DownloadId) -> Result<Vec<SegmentProgress>, EngineError> {
        let (store, _) = self.require_download_record(id).await?;
        let checkpoints = store.get_segments(id).await?;
        Ok(checkpoints
            .into_iter()
            .map(|cp| SegmentProgress {
                id: cp.segment_index,
                start: cp.start,
                cursor: cp.cursor,
                checkpointed_cursor: cp.cursor,
                end: cp.end,
                completed: cp.completed,
            })
            .collect())
    }

    /// Reads the persisted [`Settings`] for this engine.
    pub async fn get_settings(&self) -> Result<Settings, EngineError> {
        match &self.store {
            Some(store) => store.get_settings().await,
            None => Ok(self.ephemeral_settings.lock().await.clone()),
        }
    }

    /// Validates and persists updated [`Settings`] for this engine.
    pub async fn update_settings(&self, settings: Settings) -> Result<(), EngineError> {
        if settings.max_workers == 0 {
            return Err(EngineError::InvalidState(
                "max_workers must be at least 1".to_string(),
            ));
        }
        if settings.max_concurrent_downloads == 0 {
            return Err(EngineError::InvalidState(
                "max_concurrent_downloads must be at least 1".to_string(),
            ));
        }
        let rebuilt_client = build_http_client(settings.proxy_mode, &settings.proxy_url)?;
        match &self.store {
            Some(store) => store.update_settings(&settings).await?,
            None => {
                *self.ephemeral_settings.lock().await = settings.clone();
            }
        }
        *self.client.write().expect("http client rwlock poisoned") = rebuilt_client;
        self.limiter.set_limit_kib(settings.download_speed_limit_kib);
        self.queue.set_max_concurrent(settings.max_concurrent_downloads);
        let _ = self.event_tx.send(EngineEvent::SettingsUpdated(settings));
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
    }

    /// Computes the `BLAKE3` hex digest of a `Completed` `Download`'s output file.
    pub async fn compute_file_blake3(&self, id: &DownloadId) -> Result<String, EngineError> {
        let (_, record) = self.require_download_record(id).await?;
        if record.state != DownloadState::Completed {
            return Err(EngineError::InvalidState(
                "file integrity is only available for completed downloads".to_string(),
            ));
        }
        let path = record.output_path.ok_or_else(|| {
            EngineError::NotFound("completed download has no output path".to_string())
        })?;
        tokio::task::spawn_blocking(move || -> Result<String, EngineError> {
            use std::io::Read as _;
            let mut file = std::fs::File::open(&path)?;
            let mut hasher = blake3::Hasher::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hasher.finalize().to_hex().to_string())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    async fn prepare_completed_file_op(
        &self,
        id: &DownloadId,
    ) -> Result<(DownloadStore, PathBuf), EngineError> {
        let (store, record) = self.require_download_record(id).await?;
        if record.state != DownloadState::Completed {
            return Err(EngineError::InvalidState(
                "file operation is only available for completed downloads".to_string(),
            ));
        }
        let path = record.output_path.ok_or_else(|| {
            EngineError::NotFound("completed download has no output path".to_string())
        })?;
        Ok((store.clone(), path))
    }

    /// Renames a `Completed` `Download`'s output file on disk and updates SQLite.
    pub async fn rename_completed_file(
        &self,
        id: &DownloadId,
        new_filename: &str,
    ) -> Result<PathBuf, EngineError> {
        let (store, old_path) = self.prepare_completed_file_op(id).await?;
        let parent = old_path
            .parent()
            .ok_or_else(|| EngineError::InvalidState("missing parent directory".to_string()))?;
        let sanitized = probe::sanitize_filename(new_filename).ok_or_else(|| {
            EngineError::InvalidState("filename cannot be empty or invalid".to_string())
        })?;
        let new_path = parent.join(&sanitized);
        if new_path == old_path {
            return Ok(new_path);
        }
        if new_path.exists() {
            return Err(EngineError::InvalidState(format!(
                "target file already exists: {}",
                new_path.display()
            )));
        }
        tokio::fs::rename(&old_path, &new_path).await?;
        store
            .update_completed_file_paths(id, &new_path, None)
            .await?;
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(new_path)
    }

    /// Moves a `Completed` `Download`'s output file to `target_dir` and updates SQLite.
    pub async fn move_completed_file(
        &self,
        id: &DownloadId,
        target_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, EngineError> {
        let (store, old_path) = self.prepare_completed_file_op(id).await?;
        let file_name = old_path
            .file_name()
            .ok_or_else(|| EngineError::InvalidState("missing filename".to_string()))?
            .to_os_string();
        let target_dir = target_dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&target_dir).await?;
        let new_path = target_dir.join(file_name);
        if new_path == old_path {
            return Ok(new_path);
        }
        if new_path.exists() {
            return Err(EngineError::InvalidState(format!(
                "target file already exists: {}",
                new_path.display()
            )));
        }
        if let Err(rename_err) = tokio::fs::rename(&old_path, &new_path).await {
            // Cross-volume move fallback: copy -> fsync -> remove original.
            if tokio::fs::copy(&old_path, &new_path).await.is_err() {
                return Err(EngineError::Io(rename_err));
            }
            tokio::fs::remove_file(&old_path).await?;
        }
        store
            .update_completed_file_paths(id, &new_path, Some(&target_dir))
            .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?;
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(new_path)
    }

    /// Executes a single `Download` to completion, streaming progress updates via `progress_tx`.
    pub async fn download(
        &self,
        request: DownloadRequest,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    ) -> Result<DownloadOutcome, EngineError> {
        let id = DownloadId::new();
        if let Some(store) = &self.store {
            store.insert_queued(&id, &request).await?;
        }
        let (_control_tx, control_rx) = watch::channel(ControlSignal::Run);
        let sink = ProgressSink::new(id.clone(), progress_tx, self.event_tx.clone());
        run_download(
            self.http_client(),
            Arc::clone(&self.limiter),
            self.store.clone(),
            id,
            request,
            sink,
            control_rx,
        )
        .await
    }
}

fn build_http_client(proxy_mode: ProxyMode, proxy_url: &str) -> Result<reqwest::Client, EngineError> {
    let mut builder = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .connect_timeout(Duration::from_secs(15))
        .tcp_nodelay(true);

    match proxy_mode {
        ProxyMode::System => {}
        ProxyMode::None => {
            builder = builder.no_proxy();
        }
        ProxyMode::Manual => {
            let trimmed = proxy_url.trim();
            if trimmed.is_empty() {
                return Err(EngineError::InvalidState(
                    "manual proxy mode requires a proxy URL".to_string(),
                ));
            }
            let proxy = reqwest::Proxy::all(trimmed).map_err(|err| {
                EngineError::InvalidState(format!("invalid proxy URL '{trimmed}': {err}"))
            })?;
            builder = builder.proxy(proxy);
        }
    }

    builder.build().map_err(EngineError::Http)
}

async fn remove_file_if_exists(path: &Path) -> Result<(), EngineError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(EngineError::Io(err)),
    }
}

#[derive(Clone)]
struct ProgressSink {
    id: DownloadId,
    watch_tx: Option<watch::Sender<ProgressSnapshot>>,
    event_tx: broadcast::Sender<EngineEvent>,
}

impl ProgressSink {
    fn new(
        id: DownloadId,
        watch_tx: Option<watch::Sender<ProgressSnapshot>>,
        event_tx: broadcast::Sender<EngineEvent>,
    ) -> Self {
        Self {
            id,
            watch_tx,
            event_tx,
        }
    }

    fn send(&self, snapshot: ProgressSnapshot) {
        if let Some(tx) = &self.watch_tx {
            let _ = tx.send(snapshot.clone());
        }
        let _ = self.event_tx.send(EngineEvent::ProgressUpdated {
            id: self.id.clone(),
            snapshot,
        });
    }

    fn emit_state(&self, state: DownloadState, error: Option<String>) {
        let _ = self.event_tx.send(EngineEvent::StateChanged {
            id: self.id.clone(),
            state,
            error,
        });
    }
}

async fn fail_download(
    store: &Option<DownloadStore>,
    sink: &ProgressSink,
    id: &DownloadId,
    bytes: u64,
    total: Option<u64>,
    err: EngineError,
) -> EngineError {
    let err_msg = err.to_string();
    if let Some(store) = store {
        let _ = store.mark_failed(id, bytes, &err_msg).await;
    }
    sink.emit_state(DownloadState::Failed, Some(err_msg));
    emit_failed(sink, bytes, total);
    err
}

#[allow(clippy::too_many_arguments)]
async fn probe_with_control(
    client: &reqwest::Client,
    store: &Option<DownloadStore>,
    sink: &ProgressSink,
    control_rx: &mut watch::Receiver<ControlSignal>,
    id: &DownloadId,
    url: &str,
    headers: &[(String, String)],
    (initial_bytes, initial_total): (u64, Option<u64>),
    resume_prev_state: Option<DownloadState>,
) -> Result<ProbeResult, EngineError> {
    if let Some(store) = store
        && let Err(err) = store.set_state(id, DownloadState::Probing).await
    {
        return Err(fail_download(&None, sink, id, initial_bytes, initial_total, err).await);
    }
    sink.emit_state(DownloadState::Probing, None);
    emit_progress(
        sink,
        ProgressSnapshot::status(DownloadState::Probing, initial_bytes, initial_total, 0),
    );

    if *control_rx.borrow() == ControlSignal::Cancel {
        if let Some(store) = store {
            let _ = store.mark_cancelled(id).await;
        }
        sink.emit_state(DownloadState::Cancelled, None);
        emit_progress(
            sink,
            ProgressSnapshot::status(DownloadState::Cancelled, initial_bytes, initial_total, 0),
        );
        return Err(EngineError::Cancelled);
    }

    tokio::select! {
        biased;
        _ = control_rx.changed() => {
            let sig = *control_rx.borrow();
            match sig {
                ControlSignal::Cancel => {
                    if let Some(store) = store {
                        let _ = store.mark_cancelled(id).await;
                    }
                    sink.emit_state(DownloadState::Cancelled, None);
                    emit_progress(
                        sink,
                        ProgressSnapshot::status(
                            DownloadState::Cancelled,
                            initial_bytes,
                            initial_total,
                            0,
                        ),
                    );
                    Err(EngineError::Cancelled)
                }
                ControlSignal::Pause => {
                    if let Some(prev) = resume_prev_state {
                        if let Some(store) = store {
                            let _ = store.set_state(id, prev).await;
                        }
                        sink.emit_state(prev, None);
                        emit_progress(
                            sink,
                            ProgressSnapshot::status(prev, initial_bytes, initial_total, 0),
                        );
                        Err(EngineError::Paused)
                    } else {
                        Err(fail_download(
                            store,
                            sink,
                            id,
                            initial_bytes,
                            initial_total,
                            EngineError::InvalidState(
                                "cannot pause non-range download".to_string(),
                            ),
                        )
                        .await)
                    }
                }
                ControlSignal::Run => unreachable!("Run does not wake select as interruption"),
            }
        }
        res = probe_url_with_headers(client, url, headers) => match res {
            Ok(probe) => Ok(probe),
            Err(err) => Err(fail_download(store, sink, id, initial_bytes, initial_total, err).await),
        }
    }
}

fn validate_resume_probe(
    id: &DownloadId,
    record: &DownloadRecord,
    probe: &ProbeResult,
) -> Result<(), EngineError> {
    if record.content_length != probe.content_length {
        return Err(EngineError::ResourceChanged(format!(
            "Content-Length changed from {:?} to {:?}",
            record.content_length, probe.content_length
        )));
    }

    if let Some(prev_etag) = &record.etag
        && probe.etag.as_deref() != Some(prev_etag.as_str())
    {
        return Err(EngineError::ResourceChanged(format!(
            "ETag changed from {prev_etag} to {:?}",
            probe.etag
        )));
    }

    if let Some(prev_lm) = &record.last_modified
        && probe.last_modified.as_deref() != Some(prev_lm.as_str())
    {
        return Err(EngineError::ResourceChanged(format!(
            "Last-Modified changed from {prev_lm} to {:?}",
            probe.last_modified
        )));
    }

    if record.etag.is_none()
        && record.last_modified.is_none()
        && probe.etag.is_none()
        && probe.last_modified.is_none()
    {
        tracing::warn!(
            download_id = %id,
            "resuming download without ETag or Last-Modified validator; relying on Content-Length match"
        );
    }

    if !probe.supports_ranges {
        return Err(EngineError::InvalidState(
            "server no longer supports byte ranges for resume".to_string(),
        ));
    }

    Ok(())
}

fn spawn_checkpoint_task(
    store: &Option<DownloadStore>,
    id: &DownloadId,
    checkpointed_cursors: Arc<std::sync::Mutex<HashMap<usize, u64>>>,
) -> (
    Option<crate::writer::CheckpointSender>,
    Option<tokio::task::JoinHandle<()>>,
) {
    let store = store.clone();
    let (tx, mut rx) = mpsc::channel::<Vec<(usize, u64)>>(4);
    let task_id = id.clone();
    let handle = tokio::spawn(async move {
        while let Some(cursors) = rx.recv().await {
            {
                let mut map = checkpointed_cursors.lock().unwrap();
                for &(seg_idx, cursor) in &cursors {
                    map.entry(seg_idx)
                        .and_modify(|prev| *prev = (*prev).max(cursor))
                        .or_insert(cursor);
                }
            }
            if let Some(store) = &store
                && let Err(err) = store.apply_writer_checkpoint(&task_id, &cursors).await
            {
                tracing::warn!(download_id = %task_id, error = %err, "failed to persist writer checkpoint");
            }
        }
    });
    (Some(tx), Some(handle))
}

#[allow(clippy::too_many_arguments)]
async fn finalize_transfer(
    store: &Option<DownloadStore>,
    sink: &ProgressSink,
    id: &DownloadId,
    probe: &ProbeResult,
    final_path: PathBuf,
    part_path: &Path,
    writer: Arc<PartFileWriter>,
    downloaded: &AtomicU64,
    transfer_result: Result<MultiSegmentResult, EngineError>,
    probe_duration: Duration,
    transfer_start: Instant,
    total_start: Instant,
) -> Result<DownloadOutcome, EngineError> {
    let res = match transfer_result {
        Ok(res) => res,
        Err(failure_err) => {
            let bytes = downloaded.load(Ordering::Relaxed);
            match failure_err {
                EngineError::Paused => {
                    if !probe.supports_ranges {
                        if let Ok(w) = Arc::try_unwrap(writer) {
                            w.abort().await;
                        }
                        return Err(fail_download(
                            store,
                            sink,
                            id,
                            bytes,
                            probe.content_length,
                            EngineError::InvalidState(
                                "cannot pause non-range download".to_string(),
                            ),
                        )
                        .await);
                    }
                    let writer_cursors = match Arc::try_unwrap(writer) {
                        Ok(w) => match w.flush_and_close().await {
                            Ok(cursors) => cursors,
                            Err(flush_err) => {
                                return Err(fail_download(
                                    store,
                                    sink,
                                    id,
                                    bytes,
                                    probe.content_length,
                                    flush_err,
                                )
                                .await);
                            }
                        },
                        Err(_) => {
                            return Err(fail_download(
                                store,
                                sink,
                                id,
                                bytes,
                                probe.content_length,
                                EngineError::Writer(
                                    "writer handle still referenced on pause".to_string(),
                                ),
                            )
                            .await);
                        }
                    };
                    let persisted_bytes = if let Some(store) = store {
                        store
                            .finalize_writer_checkpoint(
                                id,
                                DownloadState::Paused,
                                None,
                                &writer_cursors,
                            )
                            .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
                    } else {
                        bytes
                    };
                    sink.emit_state(DownloadState::Paused, None);
                    emit_progress(
                        sink,
                        ProgressSnapshot::status(
                            DownloadState::Paused,
                            persisted_bytes,
                            probe.content_length,
                            0,
                        ),
                    );
                    return Err(EngineError::Paused);
                }
                EngineError::Cancelled => {
                    if let Ok(w) = Arc::try_unwrap(writer) {
                        w.abort().await;
                    }
                    let _ = remove_file_if_exists(part_path).await;
                    if let Some(store) = store {
                        let _ = store.mark_cancelled(id).await;
                    }
                    sink.emit_state(DownloadState::Cancelled, None);
                    emit_progress(
                        sink,
                        ProgressSnapshot::status(
                            DownloadState::Cancelled,
                            bytes,
                            probe.content_length,
                            0,
                        ),
                    );
                    return Err(EngineError::Cancelled);
                }
                err => {
                    let mut acked_bytes = bytes;
                    if probe.supports_ranges
                        && let Ok(w) = Arc::try_unwrap(writer)
                        && let Ok(writer_cursors) = w.flush_and_close().await
                    {
                        if let Some(store) = store
                            && let Ok(persisted) = store
                                .finalize_writer_checkpoint(
                                    id,
                                    DownloadState::Failed,
                                    Some(&err.to_string()),
                                    &writer_cursors,
                                )
                                .await
                        {
                            acked_bytes = persisted;
                        }
                        sink.emit_state(DownloadState::Failed, Some(err.to_string()));
                        emit_failed(sink, acked_bytes, probe.content_length);
                        return Err(err);
                    }
                    if let Some(store) = store {
                        let _ = store.mark_failed(id, bytes, &err.to_string()).await;
                    }
                    sink.emit_state(DownloadState::Failed, Some(err.to_string()));
                    emit_failed(sink, bytes, probe.content_length);
                    return Err(err);
                }
            }
        }
    };

    let writer = match Arc::try_unwrap(writer) {
        Ok(writer) => writer,
        Err(_) => {
            return Err(fail_download(
                store,
                sink,
                id,
                res.bytes_written,
                probe.content_length,
                EngineError::Writer(
                    "writer handle still referenced after workers exited".to_string(),
                ),
            )
            .await);
        }
    };

    if let Err(err) = writer.finish(&final_path).await {
        return Err(fail_download(
            store,
            sink,
            id,
            res.bytes_written,
            probe.content_length,
            err,
        )
        .await);
    }

    if let Some(store) = store {
        store.mark_completed(id, res.bytes_written).await?;
    }

    let transfer_duration = transfer_start.elapsed();
    let total_duration = total_start.elapsed();

    sink.emit_state(DownloadState::Completed, None);
    emit_progress(
        sink,
        ProgressSnapshot::status(
            DownloadState::Completed,
            res.bytes_written,
            probe.content_length.or(Some(res.bytes_written)),
            0,
        ),
    );

    Ok(DownloadOutcome {
        final_path,
        bytes_downloaded: res.bytes_written,
        content_length: probe.content_length,
        supports_ranges: probe.supports_ranges,
        workers_used: res.workers_used,
        splits_performed: res.splits_performed,
        retries_performed: res.retries_performed,
        probe_duration,
        transfer_duration,
        total_duration,
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
        final_url: probe.final_url.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_resumed_download(
    client: reqwest::Client,
    limiter: Arc<BandwidthLimiter>,
    store: DownloadStore,
    id: DownloadId,
    record: DownloadRecord,
    segments: Vec<SegmentCheckpoint>,
    sink: ProgressSink,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<DownloadOutcome, EngineError> {
    let final_path = record.output_path.clone().ok_or_else(|| {
        EngineError::InvalidState("missing output_path for resumed download".to_string())
    })?;
    let part_path = record.part_path.clone().ok_or_else(|| {
        EngineError::InvalidState("missing part_path for resumed download".to_string())
    })?;

    let total_start = Instant::now();
    let store_opt = Some(store.clone());
    let probe_start = Instant::now();
    let probe = probe_with_control(
        &client,
        &store_opt,
        &sink,
        &mut control_rx,
        &id,
        &record.url,
        &record.headers,
        (record.bytes_downloaded, record.content_length),
        Some(record.state),
    )
    .await?;
    let probe_duration = probe_start.elapsed();

    if let Err(err) = validate_resume_probe(&id, &record, &probe) {
        return Err(fail_download(
            &store_opt,
            &sink,
            &id,
            record.bytes_downloaded,
            record.content_length,
            err,
        )
        .await);
    }

    let initial_cp_map: HashMap<usize, u64> = segments
        .iter()
        .map(|s| (s.segment_index, s.cursor))
        .collect();
    let checkpointed_cursors = Arc::new(std::sync::Mutex::new(initial_cp_map));
    let live_segments_slot: Arc<std::sync::Mutex<Option<LiveSegments>>> =
        Arc::new(std::sync::Mutex::new(None));

    let transfer_start = Instant::now();
    let (cp_tx, cp_handle) =
        spawn_checkpoint_task(&store_opt, &id, Arc::clone(&checkpointed_cursors));
    let writer = match PartFileWriter::spawn_existing_with_sync(
        part_path.clone(),
        record.sync_on_complete,
        cp_tx,
    ) {
        Ok(writer) => Arc::new(writer),
        Err(err) => {
            if let Some(h) = cp_handle {
                h.abort();
            }
            return Err(fail_download(
                &store_opt,
                &sink,
                &id,
                record.bytes_downloaded,
                probe.content_length,
                err,
            )
            .await);
        }
    };

    if let Err(err) = store.mark_resumed_active(&id, &probe.final_url).await {
        if let Some(h) = cp_handle {
            h.abort();
        }
        return Err(fail_download(
            &None,
            &sink,
            &id,
            record.bytes_downloaded,
            probe.content_length,
            err,
        )
        .await);
    }

    let initial_bytes: u64 = segments
        .iter()
        .map(|s| s.cursor.saturating_sub(s.start))
        .sum();
    let downloaded = Arc::new(AtomicU64::new(initial_bytes));
    let active_workers = Arc::new(AtomicUsize::new(0));

    sink.emit_state(DownloadState::Active, None);
    emit_progress(
        &sink,
        ProgressSnapshot::status(
            DownloadState::Active,
            initial_bytes,
            probe.content_length,
            0,
        ),
    );

    let reporter = spawn_progress_reporter(
        sink.clone(),
        Arc::clone(&downloaded),
        Arc::clone(&active_workers),
        probe.content_length,
        Arc::clone(&live_segments_slot),
        Arc::clone(&checkpointed_cursors),
        initial_bytes,
    );

    let transfer_result = execute_multi_segment_from_checkpoints(
        &client,
        &probe.final_url,
        &record.headers,
        Some(&limiter),
        segments,
        record.max_workers,
        &writer,
        &downloaded,
        &active_workers,
        control_rx,
        Some((id.clone(), store.clone())),
        Some(&live_segments_slot),
    )
    .await;

    if let Some(handle) = reporter {
        handle.abort();
    }

    let out = finalize_transfer(
        &store_opt,
        &sink,
        &id,
        &probe,
        final_path,
        &part_path,
        writer,
        &downloaded,
        transfer_result,
        probe_duration,
        transfer_start,
        total_start,
    )
    .await;
    if let Some(h) = cp_handle {
        let _ = h.await;
    }
    out
}

async fn run_download(
    client: reqwest::Client,
    limiter: Arc<BandwidthLimiter>,
    store: Option<DownloadStore>,
    id: DownloadId,
    request: DownloadRequest,
    sink: ProgressSink,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<DownloadOutcome, EngineError> {
    let total_start = Instant::now();
    let probe_start = Instant::now();
    let probe = probe_with_control(
        &client,
        &store,
        &sink,
        &mut control_rx,
        &id,
        &request.url,
        &request.headers,
        (0, None),
        None,
    )
    .await?;
    let probe_duration = probe_start.elapsed();

    let final_path = request
        .output_path
        .unwrap_or_else(|| disambiguate_output_path(&request.output_dir, &probe.suggested_filename));
    let part_path = make_part_path(&final_path);

    let checkpointed_cursors = Arc::new(std::sync::Mutex::new(HashMap::new()));
    let live_segments_slot: Arc<std::sync::Mutex<Option<LiveSegments>>> =
        Arc::new(std::sync::Mutex::new(None));

    let transfer_start = Instant::now();
    let (cp_tx, cp_handle) =
        spawn_checkpoint_task(&store, &id, Arc::clone(&checkpointed_cursors));
    let writer = match PartFileWriter::spawn_with_checkpoint(
        part_path.clone(),
        probe.content_length,
        request.sync_on_complete,
        cp_tx,
    ) {
        Ok(writer) => Arc::new(writer),
        Err(err) => {
            if let Some(h) = cp_handle {
                h.abort();
            }
            return Err(
                fail_download(&store, &sink, &id, 0, probe.content_length, err).await,
            );
        }
    };

    if let Some(store) = &store {
        let planned_segments: Vec<PlannedSegment> =
            match (probe.supports_ranges, probe.content_length) {
                (true, Some(total)) if total > 0 => plan_segments(total, request.max_workers)
                    .into_iter()
                    .map(|s| PlannedSegment {
                        start: s.start,
                        end: Some(s.end),
                    })
                    .collect(),
                _ => vec![PlannedSegment {
                    start: 0,
                    end: probe.content_length,
                }],
            };
        if let Err(err) = store
            .record_probed_and_activate(&id, &probe, &final_path, &part_path, &planned_segments)
            .await
        {
            if let Some(h) = cp_handle {
                h.abort();
            }
            return Err(fail_download(&None, &sink, &id, 0, probe.content_length, err).await);
        }
    }

    let downloaded = Arc::new(AtomicU64::new(0));
    let active_workers = Arc::new(AtomicUsize::new(0));

    sink.emit_state(DownloadState::Active, None);
    emit_progress(
        &sink,
        ProgressSnapshot::status(DownloadState::Active, 0, probe.content_length, 0),
    );

    let reporter = spawn_progress_reporter(
        sink.clone(),
        Arc::clone(&downloaded),
        Arc::clone(&active_workers),
        probe.content_length,
        Arc::clone(&live_segments_slot),
        Arc::clone(&checkpointed_cursors),
        0,
    );

    let persist = store.as_ref().map(|s| (id.clone(), s.clone()));
    let transfer_result = match (probe.supports_ranges, probe.content_length) {
        (true, Some(total_bytes)) => {
            execute_multi_segment(
                &client,
                &probe.final_url,
                &request.headers,
                Some(&limiter),
                total_bytes,
                request.max_workers,
                &writer,
                &downloaded,
                &active_workers,
                control_rx,
                persist,
                Some(&live_segments_slot),
            )
            .await
        }
        _ => execute_single_stream(
            &client,
            &probe.final_url,
            &request.headers,
            Some(limiter.as_ref()),
            &writer,
            &downloaded,
            &active_workers,
            control_rx,
        )
        .await
        .map(|bytes| MultiSegmentResult {
            bytes_written: bytes,
            workers_used: 1,
            splits_performed: 0,
            retries_performed: 0,
        }),
    };

    if let Some(handle) = reporter {
        handle.abort();
    }

    let out = finalize_transfer(
        &store,
        &sink,
        &id,
        &probe,
        final_path,
        &part_path,
        writer,
        &downloaded,
        transfer_result,
        probe_duration,
        transfer_start,
        total_start,
    )
    .await;
    if let Some(h) = cp_handle {
        let _ = h.await;
    }
    out
}

fn spawn_progress_reporter(
    sink: ProgressSink,
    downloaded: Arc<AtomicU64>,
    active_workers: Arc<AtomicUsize>,
    total_bytes: Option<u64>,
    live_segments_slot: Arc<std::sync::Mutex<Option<LiveSegments>>>,
    checkpointed_cursors: Arc<std::sync::Mutex<HashMap<usize, u64>>>,
    initial_bytes: u64,
) -> Option<tokio::task::JoinHandle<()>> {
    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROGRESS_EMIT_INTERVAL);
        let mut last_sample_time = Instant::now();
        let mut last_sample_bytes = initial_bytes;
        let mut ema_bps: f64 = 0.0;

        loop {
            interval.tick().await;
            let bytes = downloaded.load(Ordering::Relaxed);
            let now = Instant::now();
            let elapsed = now.duration_since(last_sample_time);
            if elapsed >= Duration::from_millis(200) {
                let delta = bytes.saturating_sub(last_sample_bytes);
                let inst_bps = delta as f64 / elapsed.as_secs_f64();
                ema_bps = if ema_bps == 0.0 {
                    inst_bps
                } else {
                    0.35 * inst_bps + 0.65 * ema_bps
                };
                last_sample_time = now;
                last_sample_bytes = bytes;
            }
            let bytes_per_sec = ema_bps.round() as u64;
            let eta_secs = total_bytes.and_then(|total| {
                if bytes >= total {
                    Some(0)
                } else if bytes_per_sec > 0 {
                    Some((total - bytes).div_ceil(bytes_per_sec))
                } else {
                    None
                }
            });

            let segments = collect_segment_progress(
                &live_segments_slot,
                &checkpointed_cursors,
                bytes,
                total_bytes,
            );

            sink.send(ProgressSnapshot {
                state: DownloadState::Active,
                bytes_downloaded: bytes,
                total_bytes,
                active_workers: active_workers.load(Ordering::Relaxed),
                bytes_per_sec,
                eta_secs,
                segments,
            });
        }
    }))
}

fn collect_segment_progress(
    live_segments_slot: &Arc<std::sync::Mutex<Option<LiveSegments>>>,
    checkpointed_cursors: &Arc<std::sync::Mutex<HashMap<usize, u64>>>,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
) -> Vec<SegmentProgress> {
    let live_arc = live_segments_slot.lock().unwrap().clone();
    let cp_map = checkpointed_cursors.lock().unwrap();

    if let Some(live_segments) = live_arc {
        let segs = live_segments.lock().unwrap();
        let mut out: Vec<SegmentProgress> =
            segs.iter().map(|seg| seg.snapshot_progress(&cp_map)).collect();
        out.sort_by_key(|s| s.id);
        out
    } else {
        let flushed = cp_map.get(&0).copied().unwrap_or(0);
        vec![SegmentProgress {
            id: 0,
            start: 0,
            cursor: bytes_downloaded,
            checkpointed_cursor: flushed.min(bytes_downloaded),
            end: total_bytes,
            completed: false,
        }]
    }
}

fn make_part_path(final_path: &Path) -> PathBuf {
    let mut os: OsString = final_path.as_os_str().to_os_string();
    os.push(".part");
    PathBuf::from(os)
}

fn disambiguate_output_path(output_dir: &Path, filename: &str) -> PathBuf {
    let initial = output_dir.join(filename);
    if !initial.exists() && !make_part_path(&initial).exists() {
        return initial;
    }

    let file_path = Path::new(filename);
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(filename);
    let ext = file_path.extension().and_then(|e| e.to_str());

    let mut counter: usize = 1;
    loop {
        let numbered = match ext {
            Some(ext) if !ext.is_empty() => format!("{stem} ({counter}).{ext}"),
            _ => format!("{stem} ({counter})"),
        };
        let candidate = output_dir.join(numbered);
        if !candidate.exists() && !make_part_path(&candidate).exists() {
            return candidate;
        }
        counter += 1;
    }
}

fn emit_progress(sink: &ProgressSink, snapshot: ProgressSnapshot) {
    sink.send(snapshot);
}

async fn finalize_queued_interruption(
    store: Option<&DownloadStore>,
    id: &DownloadId,
    part_path: Option<&Path>,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
    sink: &ProgressSink,
    err: EngineError,
) -> Result<DownloadOutcome, EngineError> {
    match err {
        EngineError::Paused => {
            if let Some(store) = store {
                let _ = store.set_state(id, DownloadState::Paused).await;
            }
            sink.emit_state(DownloadState::Paused, None);
            sink.send(ProgressSnapshot::status(
                DownloadState::Paused,
                bytes_downloaded,
                total_bytes,
                0,
            ));
            Err(EngineError::Paused)
        }
        EngineError::Cancelled => {
            if let Some(part) = part_path {
                let _ = remove_file_if_exists(part).await;
            }
            if let Some(store) = store {
                let _ = store.mark_cancelled(id).await;
            }
            sink.emit_state(DownloadState::Cancelled, None);
            sink.send(ProgressSnapshot::status(
                DownloadState::Cancelled,
                bytes_downloaded,
                total_bytes,
                0,
            ));
            Err(EngineError::Cancelled)
        }
        other => Err(other),
    }
}

fn emit_failed(sink: &ProgressSink, bytes: u64, total: Option<u64>) {
    emit_progress(
        sink,
        ProgressSnapshot::status(DownloadState::Failed, bytes, total, 0),
    );
}
