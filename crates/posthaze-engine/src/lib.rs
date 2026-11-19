//! Core headless HTTP/HTTPS download engine for Posthaze Download Manager.

#![deny(missing_docs)]

mod probe;
mod queue;
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
use tokio::sync::{Mutex, broadcast, mpsc, watch};

use crate::probe::{ProbeResult, probe_url};
use crate::queue::QueueCoordinator;
use crate::store::DownloadStore;
use crate::transfer::{
    ControlSignal, LiveSegments, MultiSegmentResult, SegmentCheckpoint, execute_multi_segment,
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

/// Persisted application configuration for [`DownloadEngine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Default target directory for new `Download`s when no explicit path is provided.
    pub default_output_dir: PathBuf,
    /// Default maximum concurrent `Worker`s per multi-`Segment` `Download`.
    pub max_workers: usize,
    /// Maximum number of `Download`s admitted simultaneously by the `Queue`.
    pub max_concurrent_downloads: usize,
    /// Whether completed `Download`s call `File::sync_all()` before renaming `.part`.
    pub sync_on_complete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_output_dir: default_downloads_dir(),
            max_workers: 8,
            max_concurrent_downloads: 3,
            sync_on_complete: false,
        }
    }
}

fn default_downloads_dir() -> PathBuf {
    if let Some(profile) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let downloads = PathBuf::from(&profile).join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
        return PathBuf::from(profile);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Non-mutating metadata returned by [`DownloadEngine::probe`] before starting a `Download`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightInfo {
    /// Original URL supplied to the probe.
    pub original_url: String,
    /// Effective URL after following HTTP redirects.
    pub final_url: String,
    /// Sanitized filename derived from `Content-Disposition` or the URL path.
    pub suggested_filename: String,
    /// Collision-free target path inside the requested output directory.
    pub disambiguated_output_path: PathBuf,
    /// Server-reported `Content-Length`, if known.
    pub content_length: Option<u64>,
    /// Whether the server advertised byte-range (`206 Partial Content`) support.
    pub supports_ranges: bool,
    /// Server `ETag` `Validator`, if present.
    pub etag: Option<String>,
    /// Server `Last-Modified` `Validator`, if present.
    pub last_modified: Option<String>,
}

/// Live or persisted `Segment` progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentProgress {
    /// Zero-based `Segment` index within the `Download`.
    pub id: usize,
    /// Inclusive start byte offset of this `Segment`.
    pub start: u64,
    /// Current network progress offset (`>= start`).
    pub cursor: u64,
    /// Highest writer-acknowledged `Checkpoint` offset flushed to disk (`start <= checkpointed_cursor <= cursor`).
    pub checkpointed_cursor: u64,
    /// Exclusive end byte offset of this `Segment`, or `None` for an unbounded stream.
    pub end: Option<u64>,
    /// Whether this `Segment` has finished receiving all bytes.
    pub completed: bool,
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
    /// Smoothed instantaneous throughput in bytes per second.
    pub bytes_per_sec: u64,
    /// Estimated remaining seconds until completion, if `total_bytes` and speed are known.
    pub eta_secs: Option<u64>,
    /// Per-`Segment` progress and disk `Checkpoint` offsets.
    pub segments: Vec<SegmentProgress>,
}

impl Default for ProgressSnapshot {
    fn default() -> Self {
        Self {
            state: DownloadState::Queued,
            bytes_downloaded: 0,
            total_bytes: None,
            active_workers: 0,
            bytes_per_sec: 0,
            eta_secs: None,
            segments: Vec::new(),
        }
    }
}

impl ProgressSnapshot {
    fn status(
        state: DownloadState,
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
        active_workers: usize,
    ) -> Self {
        Self {
            state,
            bytes_downloaded,
            total_bytes,
            active_workers,
            bytes_per_sec: 0,
            eta_secs: None,
            segments: Vec::new(),
        }
    }
}

/// Engine-wide event stream consumed by desktop UI views and observers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    /// A `Download` transitioned to a new [`DownloadState`].
    StateChanged {
        /// Identifier of the `Download`.
        id: DownloadId,
        /// New lifecycle state.
        state: DownloadState,
        /// Optional failure message when `state` is [`DownloadState::Failed`].
        error: Option<String>,
    },
    /// Periodic or terminal [`ProgressSnapshot`] update for a `Download`.
    ProgressUpdated {
        /// Identifier of the `Download`.
        id: DownloadId,
        /// Latest progress and per-`Segment` snapshot.
        snapshot: ProgressSnapshot,
    },
    /// `Queue` membership or concurrency slot assignment changed.
    QueueUpdated,
    /// Persisted [`Settings`] were updated.
    SettingsUpdated(Settings),
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
    client: reqwest::Client,
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
        Self {
            client: build_http_client(),
            store,
            active: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(QueueCoordinator::new(settings.max_concurrent_downloads)),
            ephemeral_settings: Arc::new(Mutex::new(settings)),
            event_tx,
        }
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
        let original_url = url.into();
        let probed = probe_url(&self.client, &original_url).await?;
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

        let client = self.client.clone();
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
            .await?
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

        let client = self.client.clone();
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
            .await?
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
        match &self.store {
            Some(store) => store.update_settings(&settings).await?,
            None => {
                *self.ephemeral_settings.lock().await = settings.clone();
            }
        }
        self.queue.set_max_concurrent(settings.max_concurrent_downloads);
        let _ = self.event_tx.send(EngineEvent::SettingsUpdated(settings));
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
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
            self.client.clone(),
            self.store.clone(),
            id,
            request,
            sink,
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
        res = probe_url(client, url) => match res {
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
                            .await?
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

async fn run_resumed_download(
    client: reqwest::Client,
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

    if let Some(store) = &store
        && let Err(err) = store
            .record_probed_and_activate(&id, &probe, &final_path, &part_path, request.max_workers)
            .await
    {
        if let Some(h) = cp_handle {
            h.abort();
        }
        return Err(fail_download(&None, &sink, &id, 0, probe.content_length, err).await);
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
