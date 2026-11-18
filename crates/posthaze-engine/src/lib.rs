//! Core headless HTTP/HTTPS download engine for Posthaze Download Manager.

mod probe;
mod writer;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use thiserror::Error;
use tokio::sync::watch;

use crate::probe::probe_url;
use crate::writer::PartFileWriter;

/// Lifecycle state of a [`Download`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Probing,
    Active,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Errors produced by [`DownloadEngine`].
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unexpected HTTP status {status} for {url}")]
    HttpStatus { status: u16, url: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("file writer task failed: {0}")]
    Writer(String),
}

/// Request parameters for starting a [`Download`].
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub output_path: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub max_workers: usize,
}

impl DownloadRequest {
    /// Creates a new request targeting `output_dir` with default settings.
    pub fn new(url: impl Into<String>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            output_path: None,
            output_dir: output_dir.into(),
            max_workers: 8,
        }
    }
}

/// Snapshot of a [`Download`]'s progress and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub active_workers: usize,
}

/// Result returned when a [`Download`] completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub final_path: PathBuf,
    pub bytes_downloaded: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub final_url: String,
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
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Executes a single [`Download`] to completion, streaming progress updates via `progress_tx`.
    pub async fn download(
        &self,
        request: DownloadRequest,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    ) -> Result<DownloadOutcome, EngineError> {
        emit_progress(
            &progress_tx,
            ProgressSnapshot {
                state: DownloadState::Probing,
                bytes_downloaded: 0,
                total_bytes: None,
                active_workers: 0,
            },
        );

        let probe = match probe_url(&self.client, &request.url).await {
            Ok(probe) => probe,
            Err(err) => {
                emit_failed(&progress_tx, 0, None);
                return Err(err);
            }
        };

        let final_path = request
            .output_path
            .unwrap_or_else(|| request.output_dir.join(&probe.suggested_filename));
        let part_path = make_part_path(&final_path);

        let writer = PartFileWriter::spawn(part_path, probe.content_length)?;

        emit_progress(
            &progress_tx,
            ProgressSnapshot {
                state: DownloadState::Active,
                bytes_downloaded: 0,
                total_bytes: probe.content_length,
                active_workers: 1,
            },
        );

        let resp = self.client.get(&probe.final_url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            emit_failed(&progress_tx, 0, probe.content_length);
            return Err(EngineError::HttpStatus {
                status: status.as_u16(),
                url: probe.final_url,
            });
        }

        let mut offset = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(err) => {
                    emit_failed(&progress_tx, offset, probe.content_length);
                    return Err(err.into());
                }
            };
            let len = chunk.len() as u64;
            writer.write_at(offset, chunk).await?;
            offset += len;

            emit_progress(
                &progress_tx,
                ProgressSnapshot {
                    state: DownloadState::Active,
                    bytes_downloaded: offset,
                    total_bytes: probe.content_length,
                    active_workers: 1,
                },
            );
        }

        writer.finish(&final_path).await?;

        emit_progress(
            &progress_tx,
            ProgressSnapshot {
                state: DownloadState::Completed,
                bytes_downloaded: offset,
                total_bytes: probe.content_length.or(Some(offset)),
                active_workers: 0,
            },
        );

        Ok(DownloadOutcome {
            final_path,
            bytes_downloaded: offset,
            etag: probe.etag,
            last_modified: probe.last_modified,
            final_url: probe.final_url,
        })
    }
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
