//! Core headless HTTP/HTTPS download engine for Posthaze Download Manager.

#![deny(missing_docs)]

mod probe;
mod store;
mod transfer;
/// Dedicated blocking `.part` file writer used by `DownloadEngine` and benchmarks.
pub mod writer;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::{Mutex, watch};

use crate::probe::{ProbeResult, probe_url};
use crate::store::DownloadStore;
use crate::transfer::{
    ControlSignal, MultiSegmentResult, SegmentCheckpoint, TransferFailure, execute_multi_segment,
    execute_multi_segment_from_checkpoints, execute_single_stream,
};
use crate::writer::PartFileWriter;

pub use crate::store::DownloadRecord;

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(50);

/// Persistent identifier for a `Download` (UUID v4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DownloadId(String);

impl DownloadId {
    /// Generates a new random UUID v4 [`DownloadId`].
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Returns the string representation of this [`DownloadId`].
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DownloadId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DownloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for DownloadId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for DownloadId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Lifecycle state of a `Download`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    /// Waiting to start.
    Queued,
    /// Inspecting server capabilities and metadata (`HEAD` / `Range` probe).
    Probing,
    /// Actively transferring `Segment`s and writing `Chunk`s to `.part`.
    Active,
    /// Paused by user or scheduler.
    Paused,
    /// All `Segment`s flushed, synced, and renamed to the final path.
    Completed,
    /// Transfer aborted due to an unrecoverable error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

impl DownloadState {
    /// Returns the canonical database/display string for this state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Probing => "Probing",
            Self::Active => "Active",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

impl FromStr for DownloadState {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Queued" => Ok(Self::Queued),
            "Probing" => Ok(Self::Probing),
            "Active" => Ok(Self::Active),
            "Paused" => Ok(Self::Paused),
            "Completed" => Ok(Self::Completed),
            "Failed" => Ok(Self::Failed),
            "Cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// Errors produced by [`DownloadEngine`].
#[derive(Debug, Error)]
pub enum EngineError {
    /// Underlying `reqwest` transport or protocol error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Server returned a non-success HTTP status code.
    #[error("unexpected HTTP status {status} for {url}")]
    HttpStatus {
        /// HTTP status code.
        status: u16,
        /// Effective URL that returned the status.
        url: String,
    },

    /// Local filesystem or OS I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Dedicated `.part` writer task failed or panicked.
    #[error("file writer task failed: {0}")]
    Writer(String),

    /// SQLite persistence error.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// SQLite schema migration error.
    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// Requested operation is invalid for the current `Download` state.
    #[error("invalid download state: {0}")]
    InvalidState(String),

    /// No `Download` record exists for the requested [`DownloadId`].
    #[error("download not found: {0}")]
    NotFound(String),

    /// Server `Validator` or `Content-Length` changed since the `Download` was paused.
    #[error("remote resource changed: {0}")]
    ResourceChanged(String),

    /// `Download` was paused before completion.
    #[error("download paused")]
    Paused,

    /// `Download` was cancelled before completion.
    #[error("download cancelled")]
    Cancelled,

    /// Background `Download` task failed to join.
    #[error("download task join error: {0}")]
    TaskJoin(String),
}

/// Request parameters for starting a `Download`.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// Source HTTP/HTTPS URL.
    pub url: String,
    /// Explicit destination file path (overrides `output_dir` + resolved filename).
    pub output_path: Option<PathBuf>,
    /// Target directory used when `output_path` is `None`.
    pub output_dir: PathBuf,
    /// Maximum concurrent `Worker`s allowed for multi-`Segment` transfers.
    pub max_workers: usize,
    /// Whether to call `File::sync_all()` (`FlushFileBuffers`) before renaming `.part`.
    pub sync_on_complete: bool,
}

impl DownloadRequest {
    /// Creates a new request targeting `output_dir` with default settings.
    pub fn new(url: impl Into<String>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            output_path: None,
            output_dir: output_dir.into(),
            max_workers: 8,
            sync_on_complete: false,
        }
    }
}

/// Snapshot of a `Download`'s progress and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    /// Current lifecycle state.
    pub state: DownloadState,
    /// Total bytes received so far.
    pub bytes_downloaded: u64,
    /// Expected total size from `Content-Length`, if known.
    pub total_bytes: Option<u64>,
    /// Number of currently active `Worker`s.
    pub active_workers: usize,
}

/// Result and timing summary returned when a `Download` completes.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadOutcome {
    /// Final destination file path after `.part` rename.
    pub final_path: PathBuf,
    /// Total bytes written to disk.
    pub bytes_downloaded: u64,
    /// Server-reported `Content-Length`, if known.
    pub content_length: Option<u64>,
    /// Whether the server advertised byte-range support.
    pub supports_ranges: bool,
    /// Number of `Worker`s used during the transfer.
    pub workers_used: usize,
    /// Number of dynamic `Segment` splits (work steals) performed.
    pub splits_performed: usize,
    /// Number of transient `Worker` retries performed.
    pub retries_performed: usize,
    /// Time spent in `Probing`.
    pub probe_duration: Duration,
    /// Time spent transferring `Chunk`s and finalizing the `.part` file.
    pub transfer_duration: Duration,
    /// End-to-end wall-clock duration.
    pub total_duration: Duration,
    /// Server `ETag` header, if present.
    pub etag: Option<String>,
    /// Server `Last-Modified` header, if present.
    pub last_modified: Option<String>,
    /// Final effective URL after redirects.
    pub final_url: String,
}

impl DownloadOutcome {
    /// Average transfer throughput in bytes per second.
    pub fn average_bytes_per_sec(&self) -> f64 {
        let secs = self.transfer_duration.as_secs_f64();
        if secs <= 0.0 {
            0.0
        } else {
            self.bytes_downloaded as f64 / secs
        }
    }
}

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
    pub async fn wait(self) -> Result<DownloadOutcome, EngineError> {
        self.join_handle
            .await
            .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }
}

/// Headless HTTP/HTTPS download engine.
#[derive(Debug, Clone)]
pub struct DownloadEngine {
    client: reqwest::Client,
    store: Option<DownloadStore>,
    active: Arc<Mutex<HashMap<DownloadId, ActiveControl>>>,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine {
    /// Constructs a new ephemeral [`DownloadEngine`] without SQLite persistence.
    pub fn new() -> Self {
        Self {
            client: build_http_client(),
            store: None,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Opens or creates a SQLite-backed [`DownloadEngine`] at `db_path`.
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let store = DownloadStore::open(db_path.as_ref()).await?;
        Ok(Self {
            client: build_http_client(),
            store: Some(store),
            active: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Constructs an ephemeral in-memory SQLite-backed [`DownloadEngine`].
    pub async fn in_memory() -> Result<Self, EngineError> {
        let store = DownloadStore::in_memory().await?;
        Ok(Self {
            client: build_http_client(),
            store: Some(store),
            active: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Starts a `Download` asynchronously and returns a [`DownloadHandle`].
    pub async fn start(&self, request: DownloadRequest) -> Result<DownloadHandle, EngineError> {
        let id = DownloadId::new();
        if let Some(store) = &self.store {
            store.insert_queued(&id, &request).await?;
        }

        let (progress_tx, progress_rx) = watch::channel(ProgressSnapshot {
            state: DownloadState::Queued,
            bytes_downloaded: 0,
            total_bytes: None,
            active_workers: 0,
        });
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

        let client = self.client.clone();
        let store = self.store.clone();
        let active_map = Arc::clone(&self.active);
        let task_id = id.clone();

        let join_handle = tokio::spawn(async move {
            let res = run_download(
                client,
                store,
                task_id.clone(),
                request,
                Some(progress_tx),
                control_rx,
            )
            .await;
            active_map.lock().await.remove(&task_id);
            let _ = done_tx.send(true);
            res
        });

        Ok(DownloadHandle {
            id,
            control_tx,
            progress_rx,
            join_handle,
        })
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
            .await?
            .ok_or_else(|| EngineError::NotFound(id.to_string()))?;

        match record.state {
            DownloadState::Paused => {}
            DownloadState::Failed if record.supports_ranges || record.part_path.is_none() => {}
            other => {
                return Err(EngineError::InvalidState(format!(
                    "cannot resume download in {} state",
                    other.as_str()
                )));
            }
        }

        if let Some(part_path) = &record.part_path
            && !part_path.exists()
        {
            return Err(EngineError::InvalidState(format!(
                "missing .part file for resume: {}",
                part_path.display()
            )));
        }

        let segments = store.get_segments(id).await?;

        let (progress_tx, progress_rx) = watch::channel(ProgressSnapshot {
            state: DownloadState::Queued,
            bytes_downloaded: record.bytes_downloaded,
            total_bytes: record.content_length,
            active_workers: 0,
        });
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

        let client = self.client.clone();
        let store = store.clone();
        let active_map = Arc::clone(&self.active);
        let task_id = id.clone();

        let join_handle = tokio::spawn(async move {
            let res = run_resumed_download(
                client,
                store,
                task_id.clone(),
                record,
                segments,
                Some(progress_tx),
                control_rx,
            )
            .await;
            active_map.lock().await.remove(&task_id);
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

    /// Cancels an active or persisted (`Paused`/`Failed`/`Queued`) `Download`, deleting its `.part` file.
    pub async fn cancel(&self, id: &DownloadId) -> Result<(), EngineError> {
        let live = {
            let active = self.active.lock().await;
            active.get(id).cloned()
        };

        if let Some(ctrl) = live {
            let _ = ctrl.control_tx.send(ControlSignal::Cancel);
            let mut done_rx = ctrl.done_rx;
            let _ = done_rx.wait_for(|&done| done).await;
            return Ok(());
        }

        let Some(store) = &self.store else {
            return Err(EngineError::NotFound(id.to_string()));
        };

        let record = store
            .get_download(id)
            .await?
            .ok_or_else(|| EngineError::NotFound(id.to_string()))?;

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
            remove_part_file(&part_path).await?;
        }
        store.mark_cancelled(id).await?;
        Ok(())
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
        run_download(
            self.client.clone(),
            self.store.clone(),
            id,
            request,
            progress_tx,
            control_rx,
        )
        .await
    }
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("posthaze/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .tcp_nodelay(true)
        .build()
        .unwrap_or_default()
}

async fn remove_part_file(part_path: &Path) -> Result<(), EngineError> {
    match tokio::fs::remove_file(part_path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(EngineError::Io(err)),
    }
}

async fn fail_download(
    store: &Option<DownloadStore>,
    progress_tx: &Option<watch::Sender<ProgressSnapshot>>,
    id: &DownloadId,
    bytes: u64,
    total: Option<u64>,
    err: EngineError,
) -> EngineError {
    if let Some(store) = store {
        let _ = store.mark_failed(id, bytes, &err.to_string()).await;
    }
    emit_failed(progress_tx, bytes, total);
    err
}

async fn probe_with_control(
    client: &reqwest::Client,
    store: &Option<DownloadStore>,
    progress_tx: &Option<watch::Sender<ProgressSnapshot>>,
    control_rx: &mut watch::Receiver<ControlSignal>,
    id: &DownloadId,
    url: &str,
    (initial_bytes, initial_total): (u64, Option<u64>),
) -> Result<ProbeResult, EngineError> {
    if let Some(store) = store
        && let Err(err) = store.set_state(id, DownloadState::Probing).await
    {
        return Err(fail_download(&None, progress_tx, id, initial_bytes, initial_total, err).await);
    }
    emit_progress(
        progress_tx,
        ProgressSnapshot {
            state: DownloadState::Probing,
            bytes_downloaded: initial_bytes,
            total_bytes: initial_total,
            active_workers: 0,
        },
    );

    if *control_rx.borrow() == ControlSignal::Cancel {
        if let Some(store) = store {
            let _ = store.mark_cancelled(id).await;
        }
        emit_progress(
            progress_tx,
            ProgressSnapshot {
                state: DownloadState::Cancelled,
                bytes_downloaded: initial_bytes,
                total_bytes: initial_total,
                active_workers: 0,
            },
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
                    emit_progress(
                        progress_tx,
                        ProgressSnapshot {
                            state: DownloadState::Cancelled,
                            bytes_downloaded: initial_bytes,
                            total_bytes: initial_total,
                            active_workers: 0,
                        },
                    );
                    Err(EngineError::Cancelled)
                }
                ControlSignal::Pause => {
                    if let Some(store) = store {
                        let _ = store.save_paused(id, initial_bytes, &[]).await;
                    }
                    emit_progress(
                        progress_tx,
                        ProgressSnapshot {
                            state: DownloadState::Paused,
                            bytes_downloaded: initial_bytes,
                            total_bytes: initial_total,
                            active_workers: 0,
                        },
                    );
                    Err(EngineError::Paused)
                }
                ControlSignal::Run => unreachable!("Run does not wake select as interruption"),
            }
        }
        res = probe_url(client, url) => match res {
            Ok(probe) => Ok(probe),
            Err(err) => Err(fail_download(store, progress_tx, id, initial_bytes, initial_total, err).await),
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

#[allow(clippy::too_many_arguments)]
async fn finalize_transfer(
    store: &Option<DownloadStore>,
    progress_tx: &Option<watch::Sender<ProgressSnapshot>>,
    id: &DownloadId,
    probe: &ProbeResult,
    final_path: PathBuf,
    part_path: &Path,
    writer: Arc<PartFileWriter>,
    downloaded: &AtomicU64,
    transfer_result: Result<MultiSegmentResult, TransferFailure>,
    probe_duration: Duration,
    transfer_start: Instant,
    total_start: Instant,
) -> Result<DownloadOutcome, EngineError> {
    let res = match transfer_result {
        Ok(res) => res,
        Err(failure) => {
            let bytes = downloaded.load(Ordering::Relaxed);
            match failure.error {
                EngineError::Paused => {
                    match Arc::try_unwrap(writer) {
                        Ok(w) => {
                            if let Err(flush_err) = w.flush_and_close().await {
                                return Err(fail_download(
                                    store,
                                    progress_tx,
                                    id,
                                    bytes,
                                    probe.content_length,
                                    flush_err,
                                )
                                .await);
                            }
                        }
                        Err(_) => {
                            return Err(fail_download(
                                store,
                                progress_tx,
                                id,
                                bytes,
                                probe.content_length,
                                EngineError::Writer(
                                    "writer handle still referenced on pause".to_string(),
                                ),
                            )
                            .await);
                        }
                    }
                    if let Some(store) = store {
                        store.save_paused(id, bytes, &failure.segments).await?;
                    }
                    emit_progress(
                        progress_tx,
                        ProgressSnapshot {
                            state: DownloadState::Paused,
                            bytes_downloaded: bytes,
                            total_bytes: probe.content_length,
                            active_workers: 0,
                        },
                    );
                    return Err(EngineError::Paused);
                }
                EngineError::Cancelled => {
                    if let Ok(w) = Arc::try_unwrap(writer) {
                        w.abort().await;
                    }
                    let _ = remove_part_file(part_path).await;
                    if let Some(store) = store {
                        let _ = store.mark_cancelled(id).await;
                    }
                    emit_progress(
                        progress_tx,
                        ProgressSnapshot {
                            state: DownloadState::Cancelled,
                            bytes_downloaded: bytes,
                            total_bytes: probe.content_length,
                            active_workers: 0,
                        },
                    );
                    return Err(EngineError::Cancelled);
                }
                err => {
                    let mut segments_to_save = failure.segments;
                    if probe.supports_ranges {
                        if let Ok(w) = Arc::try_unwrap(writer) {
                            if w.flush_and_close().await.is_err() {
                                segments_to_save.clear();
                            }
                        } else {
                            segments_to_save.clear();
                        }
                    }
                    if let Some(store) = store {
                        let _ = store
                            .save_failed_with_segments(
                                id,
                                bytes,
                                &err.to_string(),
                                &segments_to_save,
                            )
                            .await;
                    }
                    emit_failed(progress_tx, bytes, probe.content_length);
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
                progress_tx,
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
            progress_tx,
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

    emit_progress(
        progress_tx,
        ProgressSnapshot {
            state: DownloadState::Completed,
            bytes_downloaded: res.bytes_written,
            total_bytes: probe.content_length.or(Some(res.bytes_written)),
            active_workers: 0,
        },
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

async fn run_resumed_download(
    client: reqwest::Client,
    store: DownloadStore,
    id: DownloadId,
    record: DownloadRecord,
    segments: Vec<SegmentCheckpoint>,
    progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<DownloadOutcome, EngineError> {
    let (Some(final_path), Some(part_path)) = (record.output_path.clone(), record.part_path.clone())
    else {
        let mut req = DownloadRequest::new(record.url, PathBuf::from("."));
        req.output_path = record.output_path;
        req.max_workers = record.max_workers;
        req.sync_on_complete = record.sync_on_complete;
        return run_download(client, Some(store), id, req, progress_tx, control_rx).await;
    };

    if segments.is_empty() {
        let mut req = DownloadRequest::new(record.url, PathBuf::from("."));
        req.output_path = Some(final_path);
        req.max_workers = record.max_workers;
        req.sync_on_complete = record.sync_on_complete;
        return run_download(client, Some(store), id, req, progress_tx, control_rx).await;
    }

    let total_start = Instant::now();
    let store_opt = Some(store.clone());
    let probe_start = Instant::now();
    let probe = probe_with_control(
        &client,
        &store_opt,
        &progress_tx,
        &mut control_rx,
        &id,
        &record.url,
        (record.bytes_downloaded, record.content_length),
    )
    .await?;
    let probe_duration = probe_start.elapsed();

    if let Err(err) = validate_resume_probe(&id, &record, &probe) {
        return Err(fail_download(
            &store_opt,
            &progress_tx,
            &id,
            record.bytes_downloaded,
            record.content_length,
            err,
        )
        .await);
    }

    let transfer_start = Instant::now();
    let writer = match PartFileWriter::spawn_existing_with_sync(
        part_path.clone(),
        record.sync_on_complete,
    ) {
        Ok(writer) => Arc::new(writer),
        Err(err) => {
            return Err(fail_download(
                &store_opt,
                &progress_tx,
                &id,
                record.bytes_downloaded,
                probe.content_length,
                err,
            )
            .await);
        }
    };

    if let Err(err) = store.mark_resumed_active(&id, &probe.final_url).await {
        return Err(fail_download(
            &None,
            &progress_tx,
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

    emit_progress(
        &progress_tx,
        ProgressSnapshot {
            state: DownloadState::Active,
            bytes_downloaded: initial_bytes,
            total_bytes: probe.content_length,
            active_workers: 0,
        },
    );

    let reporter = spawn_progress_reporter(
        progress_tx.clone(),
        Arc::clone(&downloaded),
        Arc::clone(&active_workers),
        probe.content_length,
    );

    let transfer_result = execute_multi_segment_from_checkpoints(
        &client,
        &probe.final_url,
        segments,
        record.max_workers,
        &writer,
        &downloaded,
        &active_workers,
        control_rx,
        Some((id.clone(), store.clone())),
    )
    .await;

    if let Some(handle) = reporter {
        handle.abort();
    }

    finalize_transfer(
        &store_opt,
        &progress_tx,
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
    .await
}

async fn run_download(
    client: reqwest::Client,
    store: Option<DownloadStore>,
    id: DownloadId,
    request: DownloadRequest,
    progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<DownloadOutcome, EngineError> {
    let total_start = Instant::now();
    let probe_start = Instant::now();
    let probe = probe_with_control(
        &client,
        &store,
        &progress_tx,
        &mut control_rx,
        &id,
        &request.url,
        (0, None),
    )
    .await?;
    let probe_duration = probe_start.elapsed();

    let final_path = request
        .output_path
        .unwrap_or_else(|| request.output_dir.join(&probe.suggested_filename));
    let part_path = make_part_path(&final_path);

    let transfer_start = Instant::now();
    let writer = match PartFileWriter::spawn_with_sync(
        part_path.clone(),
        probe.content_length,
        request.sync_on_complete,
    ) {
        Ok(writer) => Arc::new(writer),
        Err(err) => {
            return Err(
                fail_download(&store, &progress_tx, &id, 0, probe.content_length, err).await,
            );
        }
    };

    if let Some(store) = &store
        && let Err(err) = store
            .record_probed_and_activate(&id, &probe, &final_path, &part_path, request.max_workers)
            .await
    {
        return Err(fail_download(&None, &progress_tx, &id, 0, probe.content_length, err).await);
    }

    let downloaded = Arc::new(AtomicU64::new(0));
    let active_workers = Arc::new(AtomicUsize::new(0));

    emit_progress(
        &progress_tx,
        ProgressSnapshot {
            state: DownloadState::Active,
            bytes_downloaded: 0,
            total_bytes: probe.content_length,
            active_workers: 0,
        },
    );

    let reporter = spawn_progress_reporter(
        progress_tx.clone(),
        Arc::clone(&downloaded),
        Arc::clone(&active_workers),
        probe.content_length,
    );

    let persist = store.as_ref().map(|s| (id.clone(), s.clone()));
    let transfer_result = match (probe.supports_ranges, probe.content_length) {
        (true, Some(total_bytes)) => {
            execute_multi_segment(
                &client,
                &probe.final_url,
                total_bytes,
                request.max_workers,
                &writer,
                &downloaded,
                &active_workers,
                control_rx,
                persist,
            )
            .await
        }
        _ => execute_single_stream(
            &client,
            &probe.final_url,
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
        })
        .map_err(|error| TransferFailure {
            error,
            segments: Vec::new(),
        }),
    };

    if let Some(handle) = reporter {
        handle.abort();
    }

    finalize_transfer(
        &store,
        &progress_tx,
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
    .await
}

fn spawn_progress_reporter(
    progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    downloaded: Arc<AtomicU64>,
    active_workers: Arc<AtomicUsize>,
    total_bytes: Option<u64>,
) -> Option<tokio::task::JoinHandle<()>> {
    let tx = progress_tx?;
    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROGRESS_EMIT_INTERVAL);
        loop {
            interval.tick().await;
            let _ = tx.send(ProgressSnapshot {
                state: DownloadState::Active,
                bytes_downloaded: downloaded.load(Ordering::Relaxed),
                total_bytes,
                active_workers: active_workers.load(Ordering::Relaxed),
            });
        }
    }))
}

fn make_part_path(final_path: &Path) -> PathBuf {
    let mut os: OsString = final_path.as_os_str().to_os_string();
    os.push(".part");
    PathBuf::from(os)
}

fn emit_progress(tx: &Option<watch::Sender<ProgressSnapshot>>, snapshot: ProgressSnapshot) {
    if let Some(tx) = tx {
        let _ = tx.send(snapshot);
    }
}

fn emit_failed(tx: &Option<watch::Sender<ProgressSnapshot>>, bytes: u64, total: Option<u64>) {
    emit_progress(
        tx,
        ProgressSnapshot {
            state: DownloadState::Failed,
            bytes_downloaded: bytes,
            total_bytes: total,
            active_workers: 0,
        },
    );
}
