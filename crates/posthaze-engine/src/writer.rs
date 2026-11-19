use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::EngineError;

const WRITER_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug)]
struct WriteChunk {
    offset: u64,
    data: Bytes,
}

/// Dedicated blocking file writer for a single `.part` file.
pub struct PartFileWriter {
    part_path: PathBuf,
    tx: mpsc::Sender<WriteChunk>,
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
        if let Some(parent) = part_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part_path)?;

        if let Some(len) = total_bytes {
            file.set_len(len)?;
        }

        let (tx, mut rx) = mpsc::channel::<WriteChunk>(WRITER_CHANNEL_CAPACITY);
        let handle = tokio::task::spawn_blocking(move || {
            let mut cursor = 0u64;
            while let Some(chunk) = rx.blocking_recv() {
                if chunk.offset != cursor {
                    file.seek(SeekFrom::Start(chunk.offset))?;
                    cursor = chunk.offset;
                }
                file.write_all(&chunk.data)?;
                cursor += chunk.data.len() as u64;
            }
            file.flush()?;
            if sync_on_complete {
                file.sync_all()?;
            }
            Ok(())
        });

        Ok(Self {
            part_path,
            tx,
            handle,
        })
    }

    /// Sends a `Chunk` to be written at `offset`.
    pub async fn write_at(&self, offset: u64, data: Bytes) -> Result<(), EngineError> {
        self.tx
            .send(WriteChunk { offset, data })
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
        self.handle.abort();
        drop(self.tx);
        let _ = self.handle.await;
    }
}
