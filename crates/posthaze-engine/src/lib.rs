//! Core headless HTTP/HTTPS download engine for Posthaze Download Manager.

#![deny(missing_docs)]

mod probe;
mod transfer;
/// Dedicated blocking `.part` file writer used by `DownloadEngine` and benchmarks.
pub mod writer;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::watch;

use crate::probe::probe_url;
use crate::transfer::{execute_multi_segment, execute_single_stream};
use crate::writer::PartFileWriter;

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(50);

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

/// Headless HTTP/HTTPS download engine.
#[derive(Debug, Clone)]
pub struct DownloadEngine {
    client: reqwest::Client,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine {
    /// Constructs a new [`DownloadEngine`].
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("posthaze/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Executes a single `Download` to completion, streaming progress updates via `progress_tx`.
    pub async fn download(
        &self,
        request: DownloadRequest,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    ) -> Result<DownloadOutcome, EngineError> {
        let total_start = Instant::now();
        emit_progress(
            &progress_tx,
            ProgressSnapshot {
                state: DownloadState::Probing,
                bytes_downloaded: 0,
                total_bytes: None,
                active_workers: 0,
            },
        );

        let probe_start = Instant::now();
        let probe = match probe_url(&self.client, &request.url).await {
            Ok(probe) => probe,
            Err(err) => {
                emit_failed(&progress_tx, 0, None);
                return Err(err);
            }
        };
        let probe_duration = probe_start.elapsed();

        let final_path = request
            .output_path
            .unwrap_or_else(|| request.output_dir.join(&probe.suggested_filename));
        let part_path = make_part_path(&final_path);

        let transfer_start = Instant::now();
        let writer = Arc::new(PartFileWriter::spawn_with_sync(
            part_path,
            probe.content_length,
            request.sync_on_complete,
        )?);
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

        let transfer_result = match (probe.supports_ranges, probe.content_length) {
            (true, Some(total_bytes)) => {
                execute_multi_segment(
                    &self.client,
                    &probe.final_url,
                    total_bytes,
                    request.max_workers,
                    &writer,
                    &downloaded,
                    &active_workers,
                )
                .await
            }
            _ => execute_single_stream(
                &self.client,
                &probe.final_url,
                &writer,
                &downloaded,
                &active_workers,
            )
            .await
            .map(|bytes| crate::transfer::MultiSegmentResult {
                bytes_written: bytes,
                workers_used: 1,
                splits_performed: 0,
                retries_performed: 0,
            }),
        };

        if let Some(handle) = reporter {
            handle.abort();
        }

        let res = match transfer_result {
            Ok(res) => res,
            Err(err) => {
                emit_failed(
                    &progress_tx,
                    downloaded.load(Ordering::Relaxed),
                    probe.content_length,
                );
                return Err(err);
            }
        };

        let writer = Arc::try_unwrap(writer).map_err(|_| {
            EngineError::Writer("writer handle still referenced after workers exited".to_string())
        })?;
        writer.finish(&final_path).await?;

        let transfer_duration = transfer_start.elapsed();
        let total_duration = total_start.elapsed();

        emit_progress(
            &progress_tx,
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
            etag: probe.etag,
            last_modified: probe.last_modified,
            final_url: probe.final_url,
        })
    }
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
