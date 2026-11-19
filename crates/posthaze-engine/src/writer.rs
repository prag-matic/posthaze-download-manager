use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::EngineError;

const WRITER_CHANNEL_CAPACITY: usize = 128;
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(2);
const CHECKPOINT_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) type CheckpointSender = mpsc::Sender<Vec<(usize, u64)>>;

#[derive(Debug)]
struct WriteChunk {
    segment_index: usize,
    offset: u64,
    data: Bytes,
}

/// Dedicated blocking file writer for a single `.part` file.
pub struct PartFileWriter {
    part_path: PathBuf,
    tx: mpsc::Sender<WriteChunk>,
    aborted: Arc<AtomicBool>,
    handle: JoinHandle<Result<(), std::io::Error>>,
}

impl PartFileWriter {
    /// Opens/creates `part_path`, optionally pre-allocates `total_bytes`, and spawns the writer task
    /// (defaulting to `flush()` on completion).
    pub fn spawn(part_path: PathBuf, total_bytes: Option<u64>) -> Result<Self, EngineError> {
        Self::spawn_with_sync(part_path, total_bytes, false)
    }

    /// Opens/creates `part_path`, optionally pre-allocates `total_bytes`, and spawns the writer task
    /// with configurable `sync_all()` durability on completion.
    pub fn spawn_with_sync(
        part_path: PathBuf,
        total_bytes: Option<u64>,
        sync_on_complete: bool,
    ) -> Result<Self, EngineError> {
        Self::spawn_with_checkpoint(part_path, total_bytes, sync_on_complete, None)
    }

    /// Opens/creates `part_path`, optionally pre-allocates `total_bytes`, and spawns the writer task
    /// with periodic writer-acknowledged `Checkpoint`s.
    pub(crate) fn spawn_with_checkpoint(
        part_path: PathBuf,
        total_bytes: Option<u64>,
        sync_on_complete: bool,
        checkpoint_tx: Option<mpsc::Sender<Vec<(usize, u64)>>>,
    ) -> Result<Self, EngineError> {
        if let Some(parent) = part_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part_path)?;

        if let Some(len) = total_bytes {
            file.set_len(len)?;
        }

        Ok(Self::from_open_file(
            part_path,
            file,
            sync_on_complete,
            checkpoint_tx,
        ))
    }

    /// Opens an existing `.part` file without truncating and spawns the writer task.
    pub(crate) fn spawn_existing_with_sync(
        part_path: PathBuf,
        sync_on_complete: bool,
        checkpoint_tx: Option<mpsc::Sender<Vec<(usize, u64)>>>,
    ) -> Result<Self, EngineError> {
        let file = OpenOptions::new().write(true).open(&part_path)?;
        Ok(Self::from_open_file(
            part_path,
            file,
            sync_on_complete,
            checkpoint_tx,
        ))
    }

    fn from_open_file(
        part_path: PathBuf,
        mut file: std::fs::File,
        sync_on_complete: bool,
        checkpoint_tx: Option<mpsc::Sender<Vec<(usize, u64)>>>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<WriteChunk>(WRITER_CHANNEL_CAPACITY);
        let aborted = Arc::new(AtomicBool::new(false));
        let aborted_flag = Arc::clone(&aborted);
        let handle = tokio::task::spawn_blocking(move || {
            let mut cursor = 0u64;
            let mut seg_cursors: HashMap<usize, u64> = HashMap::new();
            let mut bytes_since_checkpoint = 0u64;
            let mut last_checkpoint = Instant::now();

            while let Some(chunk) = rx.blocking_recv() {
                if aborted_flag.load(Ordering::Acquire) {
                    return Ok(());
                }
                if chunk.offset != cursor {
                    file.seek(SeekFrom::Start(chunk.offset))?;
                    cursor = chunk.offset;
                }
                file.write_all(&chunk.data)?;
                let chunk_len = chunk.data.len() as u64;
                cursor += chunk_len;

                if checkpoint_tx.is_some() {
                    let end = chunk.offset + chunk_len;
                    seg_cursors
                        .entry(chunk.segment_index)
                        .and_modify(|cur| *cur = (*cur).max(end))
                        .or_insert(end);
                    bytes_since_checkpoint += chunk_len;

                    if bytes_since_checkpoint >= CHECKPOINT_BYTES
                        || last_checkpoint.elapsed() >= CHECKPOINT_INTERVAL
                    {
                        file.flush()?;
                        if let Some(cp_tx) = &checkpoint_tx {
                            let snapshot: Vec<(usize, u64)> =
                                seg_cursors.iter().map(|(&k, &v)| (k, v)).collect();
                            let _ = cp_tx.try_send(snapshot);
                        }
                        bytes_since_checkpoint = 0;
                        last_checkpoint = Instant::now();
                    }
                }
            }
            if aborted_flag.load(Ordering::Acquire) {
                return Ok(());
            }
            file.flush()?;
            if sync_on_complete {
                file.sync_all()?;
            }
            Ok(())
        });

        Self {
            part_path,
            tx,
            aborted,
            handle,
        }
    }

    /// Sends a `Chunk` to be written at `offset`.
    pub async fn write_at(&self, offset: u64, data: Bytes) -> Result<(), EngineError> {
        self.write_segment_at(0, offset, data).await
    }

    /// Sends a `Chunk` belonging to `segment_index` to be written at `offset`.
    pub(crate) async fn write_segment_at(
        &self,
        segment_index: usize,
        offset: u64,
        data: Bytes,
    ) -> Result<(), EngineError> {
        self.tx
            .send(WriteChunk {
                segment_index,
                offset,
                data,
            })
            .await
            .map_err(|_| EngineError::Writer("writer channel closed unexpectedly".to_string()))
    }

    /// Closes the writer channel, waits for flush/sync, and atomically renames `.part` to `final_path`.
    pub async fn finish(self, final_path: &Path) -> Result<(), EngineError> {
        let part_path = self.part_path.clone();
        self.flush_and_close().await?;
        tokio::fs::rename(&part_path, final_path).await?;
        Ok(())
    }

    /// Drains and flushes all buffered `Chunk`s to `.part` without renaming it.
    pub(crate) async fn flush_and_close(self) -> Result<(), EngineError> {
        drop(self.tx);
        let write_result = self
            .handle
            .await
            .map_err(|err| EngineError::Writer(format!("writer task panicked: {err}")))?;
        write_result?;
        Ok(())
    }

    /// Aborts the writer task immediately without draining buffered `Chunk`s.
    pub(crate) async fn abort(self) {
        self.aborted.store(true, Ordering::Release);
        self.handle.abort();
        drop(self.tx);
        let _ = self.handle.await;
    }
}
