use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::EngineError;

const WRITER_CHANNEL_CAPACITY: usize = 64;

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
    /// Opens/creates `part_path`, optionally pre-allocates `total_bytes`, and spawns the writer task.
    pub fn spawn(part_path: PathBuf, total_bytes: Option<u64>) -> Result<Self, EngineError> {
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
            while let Some(chunk) = rx.blocking_recv() {
                file.seek(SeekFrom::Start(chunk.offset))?;
                file.write_all(&chunk.data)?;
            }
            file.sync_all()?;
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
        drop(self.tx);
        let write_result = self
            .handle
            .await
            .map_err(|err| EngineError::Writer(format!("writer task panicked: {err}")))?;
        write_result?;
        tokio::fs::rename(&self.part_path, final_path).await?;
        Ok(())
    }
}
