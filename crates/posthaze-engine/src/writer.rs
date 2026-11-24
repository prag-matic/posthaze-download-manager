use std::collections::HashMap;
use std::fs::OpenOptions;
#[cfg(not(windows))]
use std::io::{Seek, SeekFrom};
use std::io::Write;
#[cfg(windows)]
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::EngineError;
use crate::transfer::CHUNK_BUFFER_SIZE;

/// Maximum number of `Chunk`s buffered in the per-`Download` writer channel (16 MiB at 512 KiB/`Chunk`).
///
/// Chosen at `32` (within the `16..=32` `Chunk` spec target, down 4x from `128` / 64 MiB) so up to
/// `c=32` concurrent `Worker`s can keep one `Chunk` in flight without channel starvation while
/// preserving ADR-0005 backpressure.
pub(crate) const WRITER_CHANNEL_CAPACITY: usize = 32;
const _: () = assert!(WRITER_CHANNEL_CAPACITY >= 16 && WRITER_CHANNEL_CAPACITY <= 32);

const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(2);
const CHECKPOINT_BYTES: u64 = 8 * 1024 * 1024;

/// Minimum pre-allocated `.part` size (`5 MiB`) before enabling NTFS sparse mode to avoid
/// synchronous zero-fill stalls when upper `Segment`s write far into the file.
pub(crate) const SPARSE_MIN_SIZE: u64 = 5 * 1024 * 1024;

#[cfg(windows)]
fn set_sparse(file: &std::fs::File, sparse: bool) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FILE_SET_SPARSE_BUFFER, FSCTL_SET_SPARSE};

    let input = FILE_SET_SPARSE_BUFFER { SetSparse: sparse };
    let mut returned = 0u32;
    // SAFETY: `file` owns the handle and outlives this synchronous `DeviceIoControl` call;
    // `input` is a valid stack `FILE_SET_SPARSE_BUFFER` and `returned` is a valid `u32`.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_SPARSE,
            (&raw const input).cast(),
            size_of::<FILE_SET_SPARSE_BUFFER>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    ok != 0
}

#[cfg(not(windows))]
fn set_sparse(_file: &std::fs::File, _sparse: bool) -> bool {
    false
}

pub(crate) type CheckpointSender = mpsc::Sender<Vec<(usize, u64)>>;

/// Non-blocking, bounded free-list of fixed-capacity `Chunk` buffers shared within a single `Download`.
#[derive(Debug)]
pub(crate) struct ChunkBufferPool {
    buffers: Mutex<Vec<Vec<u8>>>,
    max_retained: usize,
    buffer_capacity: usize,
}

impl ChunkBufferPool {
    pub(crate) fn new(max_retained: usize, buffer_capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            buffers: Mutex::new(Vec::with_capacity(max_retained)),
            max_retained,
            buffer_capacity,
        })
    }

    /// Acquires a cleared `Vec<u8>` buffer of capacity `buffer_capacity` without ever blocking or awaiting.
    pub(crate) fn acquire(self: &Arc<Self>) -> PooledBuffer {
        let mut buf = {
            let mut guard = self.buffers.lock().unwrap_or_else(|e| e.into_inner());
            guard.pop()
        }
        .unwrap_or_else(|| Vec::with_capacity(self.buffer_capacity));
        buf.clear();
        PooledBuffer {
            buf,
            pool: Arc::clone(self),
        }
    }

    fn release(&self, mut buf: Vec<u8>) {
        if buf.capacity() != self.buffer_capacity {
            return;
        }
        buf.clear();
        let overflow = {
            let mut guard = self.buffers.lock().unwrap_or_else(|e| e.into_inner());
            if guard.len() < self.max_retained {
                guard.push(buf);
                None
            } else {
                Some(buf)
            }
        };
        drop(overflow);
    }

    pub(crate) fn clear(&self) {
        let drained = {
            let mut guard = self.buffers.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *guard)
        };
        drop(drained);
    }

    #[cfg(test)]
    pub(crate) fn retained_len(&self) -> usize {
        self.buffers.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// RAII handle to a `Chunk` buffer leased from [`ChunkBufferPool`]; returns the buffer on drop.
#[derive(Debug)]
pub(crate) struct PooledBuffer {
    buf: Vec<u8>,
    pool: Arc<ChunkBufferPool>,
}

impl PooledBuffer {
    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    pub(crate) fn extend_from_slice(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let buf = std::mem::take(&mut self.buf);
        if buf.capacity() == self.pool.buffer_capacity {
            self.pool.release(buf);
        }
    }
}

#[derive(Debug)]
enum ChunkData {
    Bytes(Bytes),
    Pooled(PooledBuffer),
}

impl ChunkData {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Bytes(b) => b.as_ref(),
            Self::Pooled(p) => p.as_slice(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Bytes(b) => b.len(),
            Self::Pooled(p) => p.len(),
        }
    }
}

#[derive(Debug)]
struct WriteChunk {
    segment_index: usize,
    offset: u64,
    data: ChunkData,
}

#[cfg(any(windows, test))]
fn write_all_with_seek_write<F>(
    mut offset: u64,
    mut buf: &[u8],
    mut seek_write_fn: F,
) -> std::io::Result<()>
where
    F: FnMut(&[u8], u64) -> std::io::Result<usize>,
{
    while !buf.is_empty() {
        match seek_write_fn(buf, offset) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "failed to write whole buffer via seek_write",
                ));
            }
            Ok(n) => {
                let Some(rest) = buf.get(n..) else {
                    return Err(std::io::Error::other(
                        "seek_write reported more bytes written than buffer length",
                    ));
                };
                buf = rest;
                offset = offset.checked_add(n as u64).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "file offset overflow during seek_write",
                    )
                })?;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

trait PositionalSink: Send + 'static {
    fn write_all_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
    fn sync_all(&self) -> std::io::Result<()>;
    fn clear_sparse(&self) {}
}

struct PositionalFile {
    file: std::fs::File,
    #[cfg(not(windows))]
    cursor: u64,
}

impl PositionalFile {
    fn new(file: std::fs::File) -> Self {
        Self {
            file,
            #[cfg(not(windows))]
            cursor: 0,
        }
    }
}

impl PositionalSink for PositionalFile {
    fn write_all_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            write_all_with_seek_write(offset, buf, |slice, pos| self.file.seek_write(slice, pos))
        }
        #[cfg(not(windows))]
        {
            if offset != self.cursor {
                self.file.seek(SeekFrom::Start(offset))?;
                self.cursor = offset;
            }
            self.file.write_all(buf)?;
            self.cursor = offset.checked_add(buf.len() as u64).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "file offset overflow during write",
                )
            })?;
            Ok(())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }

    fn sync_all(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }

    fn clear_sparse(&self) {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
            if let Ok(meta) = self.file.metadata()
                && (meta.file_attributes() & FILE_ATTRIBUTE_SPARSE_FILE) != 0
            {
                let _ = set_sparse(&self.file, false);
            }
        }
    }
}

/// Dedicated blocking file writer for a single `.part` file.
#[derive(Debug)]
pub struct PartFileWriter {
    part_path: PathBuf,
    tx: mpsc::Sender<WriteChunk>,
    aborted: Arc<AtomicBool>,
    clear_sparse_on_close: Arc<AtomicBool>,
    pool: Arc<ChunkBufferPool>,
    handle: JoinHandle<Result<HashMap<usize, u64>, std::io::Error>>,
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
            if len >= SPARSE_MIN_SIZE {
                let _ = set_sparse(&file, true);
            }
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
        file: std::fs::File,
        sync_on_complete: bool,
        checkpoint_tx: Option<mpsc::Sender<Vec<(usize, u64)>>>,
    ) -> Self {
        Self::from_sink(
            part_path,
            PositionalFile::new(file),
            sync_on_complete,
            checkpoint_tx,
        )
    }

    fn from_sink<S: PositionalSink>(
        part_path: PathBuf,
        mut sink: S,
        sync_on_complete: bool,
        checkpoint_tx: Option<mpsc::Sender<Vec<(usize, u64)>>>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<WriteChunk>(WRITER_CHANNEL_CAPACITY);
        let aborted = Arc::new(AtomicBool::new(false));
        let aborted_flag = Arc::clone(&aborted);
        let clear_sparse_on_close = Arc::new(AtomicBool::new(false));
        let clear_sparse_flag = Arc::clone(&clear_sparse_on_close);
        let pool = ChunkBufferPool::new(WRITER_CHANNEL_CAPACITY, CHUNK_BUFFER_SIZE);

        let handle = tokio::task::spawn_blocking(move || {
            let mut seg_cursors: HashMap<usize, u64> = HashMap::new();
            let mut bytes_since_checkpoint = 0u64;
            let mut last_checkpoint = Instant::now();

            while let Some(chunk) = rx.blocking_recv() {
                if aborted_flag.load(Ordering::Acquire) {
                    return Ok(seg_cursors);
                }

                sink.write_all_at(chunk.offset, chunk.data.as_slice())?;
                let chunk_len = chunk.data.len() as u64;
                let end = chunk.offset + chunk_len;
                let segment_index = chunk.segment_index;
                // Return any pooled buffer immediately before flushing or blocking on next Chunk.
                drop(chunk);

                seg_cursors
                    .entry(segment_index)
                    .and_modify(|cur| *cur = (*cur).max(end))
                    .or_insert(end);

                if checkpoint_tx.is_some() {
                    bytes_since_checkpoint += chunk_len;

                    if bytes_since_checkpoint >= CHECKPOINT_BYTES
                        || last_checkpoint.elapsed() >= CHECKPOINT_INTERVAL
                    {
                        sink.flush()?;
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
                return Ok(seg_cursors);
            }
            if clear_sparse_flag.load(Ordering::Acquire) {
                sink.clear_sparse();
            }
            sink.flush()?;
            if sync_on_complete {
                sink.sync_all()?;
            }
            Ok(seg_cursors)
        });

        Self {
            part_path,
            tx,
            aborted,
            clear_sparse_on_close,
            pool,
            handle,
        }
    }

    /// Acquires a recycled `Chunk`-sized buffer from this writer's per-`Download` pool.
    pub(crate) fn acquire_buffer(&self) -> PooledBuffer {
        self.pool.acquire()
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
        self.send_chunk(segment_index, offset, ChunkData::Bytes(data))
            .await
    }

    /// Sends a pooled `Chunk` belonging to `segment_index` to be written at `offset`.
    pub(crate) async fn write_segment_pooled(
        &self,
        segment_index: usize,
        offset: u64,
        data: PooledBuffer,
    ) -> Result<(), EngineError> {
        self.send_chunk(segment_index, offset, ChunkData::Pooled(data))
            .await
    }

    async fn send_chunk(
        &self,
        segment_index: usize,
        offset: u64,
        data: ChunkData,
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
        self.clear_sparse_on_close.store(true, Ordering::Release);
        let _ = self.flush_and_close().await?;
        tokio::fs::rename(&part_path, final_path).await?;
        Ok(())
    }

    /// Drains and flushes all buffered `Chunk`s to `.part` without renaming it, returning
    /// the highest writer-acknowledged byte offset per `Segment` index.
    pub(crate) async fn flush_and_close(self) -> Result<HashMap<usize, u64>, EngineError> {
        drop(self.tx);
        let write_result = self
            .handle
            .await
            .map_err(|err| EngineError::Writer(format!("writer task panicked: {err}")))?;
        self.pool.clear();
        Ok(write_result?)
    }

    /// Aborts the writer task immediately without draining buffered `Chunk`s.
    pub(crate) async fn abort(self) {
        self.aborted.store(true, Ordering::Release);
        self.handle.abort();
        drop(self.tx);
        let _ = self.handle.await;
        self.pool.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tempfile::TempDir;

    type RecordedWrites = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

    struct MockSink {
        gate_rx: Option<std::sync::mpsc::Receiver<()>>,
        gate_once: bool,
        write_started_tx: Option<tokio::sync::oneshot::Sender<()>>,
        writes: RecordedWrites,
        fail_after: Option<usize>,
        write_count: usize,
        flushed: Arc<AtomicUsize>,
    }

    impl PositionalSink for MockSink {
        fn write_all_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<()> {
            if let Some(tx) = self.write_started_tx.take() {
                let _ = tx.send(());
            }
            if self.gate_once {
                if let Some(rx) = self.gate_rx.take() {
                    let _ = rx.recv();
                }
            } else if let Some(rx) = &self.gate_rx {
                let _ = rx.recv();
            }
            self.write_count += 1;
            if let Some(limit) = self.fail_after
                && self.write_count > limit
            {
                return Err(std::io::Error::other("simulated disk failure"));
            }
            self.writes.lock().unwrap().push((offset, buf.to_vec()));
            Ok(())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn sync_all(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn writer_channel_capacity_is_bounded_between_16_and_32() {
        const {
            assert!(WRITER_CHANNEL_CAPACITY >= 16 && WRITER_CHANNEL_CAPACITY <= 32);
        }
        assert!((16..=32).contains(&WRITER_CHANNEL_CAPACITY));
    }

    #[test]
    fn seek_write_loop_handles_partial_writes_and_interrupted() {
        let mut target = [0u8; 32];
        let mut calls = 0usize;
        let payload = b"hello-seek-write";

        write_all_with_seek_write(10, payload, |slice, offset| {
            calls += 1;
            match calls {
                1 => {
                    assert_eq!(offset, 10);
                    let n = 3.min(slice.len());
                    target[offset as usize..offset as usize + n].copy_from_slice(&slice[..n]);
                    Ok(n)
                }
                2 => {
                    assert_eq!(offset, 13);
                    Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
                }
                3 => {
                    assert_eq!(offset, 13);
                    let n = 5.min(slice.len());
                    target[offset as usize..offset as usize + n].copy_from_slice(&slice[..n]);
                    Ok(n)
                }
                _ => {
                    assert_eq!(offset, 18);
                    target[offset as usize..offset as usize + slice.len()].copy_from_slice(slice);
                    Ok(slice.len())
                }
            }
        })
        .expect("partial writes + Interrupted should complete");

        assert_eq!(calls, 4);
        assert_eq!(&target[10..10 + payload.len()], payload);
        assert!(target[..10].iter().all(|&b| b == 0));
        assert!(target[10 + payload.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    fn seek_write_loop_returns_write_zero_on_ok_zero() {
        let err = write_all_with_seek_write(0, b"abc", |_, _| Ok(0))
            .expect_err("Ok(0) must map to WriteZero");
        assert_eq!(err.kind(), std::io::ErrorKind::WriteZero);
    }

    #[test]
    fn seek_write_loop_handles_empty_buffer_and_offset_overflow() {
        let mut called = false;
        write_all_with_seek_write(u64::MAX, &[], |_, _| {
            called = true;
            Ok(0)
        })
        .expect("empty buffer must be a no-op");
        assert!(!called);

        let err = write_all_with_seek_write(u64::MAX - 1, b"abcd", |slice, _| {
            Ok(3.min(slice.len()))
        })
        .expect_err("offset overflow must return InvalidInput");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn chunk_buffer_pool_reuses_buffers_and_caps_retained_count() {
        let pool = ChunkBufferPool::new(2, CHUNK_BUFFER_SIZE);
        assert_eq!(pool.retained_len(), 0);

        let mut b1 = pool.acquire();
        assert_eq!(b1.capacity(), CHUNK_BUFFER_SIZE);
        let ptr1 = b1.as_slice().as_ptr();
        b1.extend_from_slice(b"abc");
        drop(b1);
        assert_eq!(pool.retained_len(), 1);

        let b1_reused = pool.acquire();
        assert_eq!(b1_reused.len(), 0);
        assert_eq!(b1_reused.capacity(), CHUNK_BUFFER_SIZE);
        assert_eq!(b1_reused.as_slice().as_ptr(), ptr1);
        assert_eq!(pool.retained_len(), 0);

        let b2 = pool.acquire();
        let b3 = pool.acquire();
        drop(b1_reused);
        drop(b2);
        drop(b3);
        assert_eq!(pool.retained_len(), 2);

        // Oversized buffer (reallocated beyond capacity) must not be retained.
        let mut oversized = pool.acquire();
        let extra = vec![0u8; CHUNK_BUFFER_SIZE + 64];
        oversized.extend_from_slice(&extra);
        assert!(oversized.capacity() > CHUNK_BUFFER_SIZE);
        pool.clear();
        drop(oversized);
        assert_eq!(pool.retained_len(), 0);
    }

    #[test]
    fn chunk_buffer_pool_tolerates_poisoned_mutex() {
        let pool = ChunkBufferPool::new(2, 1024);
        let pool_clone = Arc::clone(&pool);
        let _ = std::thread::spawn(move || {
            let _guard = pool_clone.buffers.lock().unwrap();
            panic!("intentional poison");
        })
        .join();

        let mut buf = pool.acquire();
        buf.extend_from_slice(b"ok");
        drop(buf);
        assert_eq!(pool.retained_len(), 1);
        pool.clear();
        assert_eq!(pool.retained_len(), 0);
    }

    #[tokio::test]
    async fn bounded_channel_enforces_backpressure_and_unblocks_on_drain() {
        let (gate_tx, gate_rx) = std::sync::mpsc::channel();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let flushed = Arc::new(AtomicUsize::new(0));
        let sink = MockSink {
            gate_rx: Some(gate_rx),
            gate_once: false,
            write_started_tx: None,
            writes: Arc::clone(&writes),
            fail_after: None,
            write_count: 0,
            flushed,
        };
        let dir = TempDir::new().unwrap();
        let writer = PartFileWriter::from_sink(dir.path().join("bp.part"), sink, false, None);

        // First send is dequeued by the writer task and blocks inside sink.write_all_at;
        // the remaining WRITER_CHANNEL_CAPACITY sends fill the bounded channel.
        for i in 0..=WRITER_CHANNEL_CAPACITY {
            writer
                .write_at((i * 4) as u64, Bytes::from_static(b"abcd"))
                .await
                .expect("writes up to channel capacity + active slot must not block");
        }

        let writer = Arc::new(writer);
        let writer_clone = Arc::clone(&writer);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let blocked_send = tokio::spawn(async move {
            let _ = started_tx.send(());
            writer_clone
                .write_at(
                    ((WRITER_CHANNEL_CAPACITY + 1) * 4) as u64,
                    Bytes::from_static(b"efgh"),
                )
                .await
        });
        started_rx.await.unwrap();

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(
            !blocked_send.is_finished(),
            "send beyond WRITER_CHANNEL_CAPACITY must stall on backpressure"
        );

        // Release two writes so the blocked send enters the channel and completes.
        gate_tx.send(()).unwrap();
        gate_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), blocked_send)
            .await
            .expect("blocked send should unblock as channel drains")
            .unwrap()
            .unwrap();

        for _ in 0..WRITER_CHANNEL_CAPACITY {
            gate_tx.send(()).unwrap();
        }
        let writer = Arc::try_unwrap(writer).expect("single owner remains");
        writer.flush_and_close().await.unwrap();
        assert_eq!(writes.lock().unwrap().len(), WRITER_CHANNEL_CAPACITY + 2);
    }

    #[tokio::test]
    async fn interleaved_write_at_and_pooled_writes_produce_blake3_exact_file() {
        let dir = TempDir::new().unwrap();
        let part_path = dir.path().join("interleaved.bin.part");
        let final_path = dir.path().join("interleaved.bin");

        let mut expected = vec![0u8; CHUNK_BUFFER_SIZE * 3 + 1234];
        for (i, byte) in expected.iter_mut().enumerate() {
            *byte = ((i * 31 + 7) % 251) as u8;
        }
        let expected_hash = blake3::hash(&expected);

        let writer = PartFileWriter::spawn(part_path, Some(expected.len() as u64)).unwrap();

        // Interleave pooled full chunk, unpooled Bytes slice, pooled tail, and out-of-order segment.
        let mut p2 = writer.acquire_buffer();
        p2.extend_from_slice(&expected[CHUNK_BUFFER_SIZE * 2..CHUNK_BUFFER_SIZE * 3]);
        assert_eq!(p2.capacity(), CHUNK_BUFFER_SIZE);
        writer
            .write_segment_pooled(2, (CHUNK_BUFFER_SIZE * 2) as u64, p2)
            .await
            .unwrap();

        writer
            .write_at(
                CHUNK_BUFFER_SIZE as u64,
                Bytes::copy_from_slice(&expected[CHUNK_BUFFER_SIZE..CHUNK_BUFFER_SIZE * 2]),
            )
            .await
            .unwrap();

        let mut p0 = writer.acquire_buffer();
        p0.extend_from_slice(&expected[..CHUNK_BUFFER_SIZE]);
        writer.write_segment_pooled(0, 0, p0).await.unwrap();

        let mut p_tail = writer.acquire_buffer();
        p_tail.extend_from_slice(&expected[CHUNK_BUFFER_SIZE * 3..]);
        assert_eq!(p_tail.capacity(), CHUNK_BUFFER_SIZE);
        writer
            .write_segment_pooled(3, (CHUNK_BUFFER_SIZE * 3) as u64, p_tail)
            .await
            .unwrap();

        writer.finish(&final_path).await.unwrap();
        let actual = std::fs::read(&final_path).unwrap();
        assert_eq!(blake3::hash(&actual), expected_hash);
    }

    #[tokio::test]
    async fn abort_drop_and_writer_error_never_deadlock_with_pooled_buffers() {
        // 1. Writer I/O error drains/drops queued pooled buffers and fails subsequent sends cleanly.
        let writes = Arc::new(Mutex::new(Vec::new()));
        let sink = MockSink {
            gate_rx: None,
            gate_once: false,
            write_started_tx: None,
            writes,
            fail_after: Some(1),
            write_count: 0,
            flushed: Arc::new(AtomicUsize::new(0)),
        };
        let dir = TempDir::new().unwrap();
        let writer = PartFileWriter::from_sink(dir.path().join("err.part"), sink, false, None);
        let pool = Arc::clone(&writer.pool);

        let mut b1 = writer.acquire_buffer();
        b1.extend_from_slice(b"first");
        writer.write_segment_pooled(0, 0, b1).await.unwrap();

        let mut hit_err = false;
        for i in 1..=(WRITER_CHANNEL_CAPACITY + 8) {
            let mut b = writer.acquire_buffer();
            b.extend_from_slice(b"next");
            if writer
                .write_segment_pooled(0, (i * 4) as u64, b)
                .await
                .is_err()
            {
                hit_err = true;
                break;
            }
        }
        let close_res = tokio::time::timeout(Duration::from_secs(2), writer.flush_and_close())
            .await
            .expect("flush_and_close after io error must not deadlock");
        assert!(hit_err || close_res.is_err());
        assert_eq!(pool.retained_len(), 0);

        // 2. Abort while write #0 is in-flight and channel is full of pooled buffers completes promptly.
        let (gate_tx, gate_rx) = std::sync::mpsc::channel();
        let (write_started_tx, write_started_rx) = tokio::sync::oneshot::channel();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let blocked_sink = MockSink {
            gate_rx: Some(gate_rx),
            gate_once: true,
            write_started_tx: Some(write_started_tx),
            writes: Arc::clone(&writes),
            fail_after: None,
            write_count: 0,
            flushed: Arc::new(AtomicUsize::new(0)),
        };
        let writer =
            PartFileWriter::from_sink(dir.path().join("abort.part"), blocked_sink, false, None);
        let pool = Arc::clone(&writer.pool);

        // Send write #0 and confirm the blocking writer task is paused inside MockSink::write_all_at.
        let mut first = writer.acquire_buffer();
        first.extend_from_slice(b"active");
        writer.write_segment_pooled(0, 0, first).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), write_started_rx)
            .await
            .expect("writer task should enter write_all_at for chunk #0")
            .unwrap();

        // Fill all WRITER_CHANNEL_CAPACITY slots with pooled chunks while chunk #0 is in-flight.
        for i in 1..=WRITER_CHANNEL_CAPACITY {
            let mut b = writer.acquire_buffer();
            b.extend_from_slice(b"queued");
            writer
                .write_segment_pooled(0, (i * 6) as u64, b)
                .await
                .unwrap();
        }

        // Start abort() and confirm `aborted` is visible before releasing the one-shot gate,
        // eliminating any TOCTOU window where chunk #1 could be written or stall on `gate_rx`.
        let aborted_flag = Arc::clone(&writer.aborted);
        let abort_task = tokio::spawn(writer.abort());
        tokio::time::timeout(Duration::from_secs(2), async {
            while !aborted_flag.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("abort task should mark writer aborted");
        drop(gate_tx);

        tokio::time::timeout(Duration::from_secs(2), abort_task)
            .await
            .expect("abort must not deadlock")
            .expect("abort task must not panic");
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert_eq!(pool.retained_len(), 0);

        // 3. Dropping PartFileWriter directly while holding leased PooledBuffer never deadlocks.
        let writer = PartFileWriter::spawn(dir.path().join("drop.part"), Some(1024)).unwrap();
        let mut leased = writer.acquire_buffer();
        leased.extend_from_slice(b"leased");
        let mut queued = writer.acquire_buffer();
        queued.extend_from_slice(b"queued");
        writer.write_segment_pooled(0, 0, queued).await.unwrap();
        drop(writer);
        drop(leased);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn sparse_attribute_set_for_large_part_and_cleared_on_finish() {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;

        let dir = TempDir::new().unwrap();
        let part_path = dir.path().join("sparse.bin.part");
        let final_path = dir.path().join("sparse.bin");
        let len = SPARSE_MIN_SIZE + 4096;

        let writer = PartFileWriter::spawn(part_path.clone(), Some(len)).unwrap();
        let part_attrs = std::fs::metadata(&part_path).unwrap().file_attributes();
        // On NTFS volumes, `.part` should be marked sparse before writing upper segments;
        // on non-NTFS volumes (e.g. FAT32/exFAT), FSCTL_SET_SPARSE is a safe no-op.
        writer
            .write_segment_at(0, len - 16, Bytes::from_static(b"0123456789abcdef"))
            .await
            .unwrap();
        writer.finish(&final_path).await.unwrap();

        let final_meta = std::fs::metadata(&final_path).unwrap();
        assert_eq!(final_meta.len(), len);
        if (part_attrs & FILE_ATTRIBUTE_SPARSE_FILE) != 0 {
            assert_eq!(
                final_meta.file_attributes() & FILE_ATTRIBUTE_SPARSE_FILE,
                0,
                "final completed file should have NTFS sparse attribute cleared"
            );
        }
    }
}
