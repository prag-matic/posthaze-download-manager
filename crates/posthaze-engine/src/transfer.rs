use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::BytesMut;
use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::RANGE;
use tokio::task::JoinSet;

use crate::EngineError;
use crate::writer::PartFileWriter;

pub const MIN_SEGMENT_SIZE: u64 = 2 * 1024 * 1024;
pub const CHUNK_BUFFER_SIZE: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpec {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
struct ActiveSegment {
    id: usize,
    start: u64,
    cursor: AtomicU64,
    end: AtomicU64,
}

impl ActiveSegment {
    fn new(id: usize, start: u64, end: u64) -> Arc<Self> {
        Arc::new(Self {
            id,
            start,
            cursor: AtomicU64::new(start),
            end: AtomicU64::new(end),
        })
    }

    fn remaining(&self) -> u64 {
        let end = self.end.load(Ordering::Acquire);
        let cur = self.cursor.load(Ordering::Acquire);
        end.saturating_sub(cur)
    }
}

pub fn plan_segments(total_bytes: u64, max_workers: usize) -> Vec<SegmentSpec> {
    if total_bytes == 0 {
        return Vec::new();
    }

    let count = ((total_bytes / MIN_SEGMENT_SIZE).max(1) as usize).min(max_workers.max(1));
    let base = total_bytes / count as u64;
    let rem = total_bytes % count as u64;

    let mut segments = Vec::with_capacity(count);
    let mut cursor = 0u64;
    for i in 0..count {
        let len = base + if (i as u64) < rem { 1 } else { 0 };
        let end = cursor + len;
        segments.push(SegmentSpec { start: cursor, end });
        cursor = end;
    }
    segments
}

pub async fn execute_single_stream(
    client: &reqwest::Client,
    url: &str,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    active_workers: &AtomicUsize,
) -> Result<u64, EngineError> {
    active_workers.store(1, Ordering::Relaxed);
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        active_workers.store(0, Ordering::Relaxed);
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: resp.url().to_string(),
        });
    }

    let result = stream_unbounded_to_writer(resp, writer, downloaded).await;
    active_workers.store(0, Ordering::Relaxed);
    result
}

pub async fn execute_multi_segment(
    client: &reqwest::Client,
    url: &str,
    total_bytes: u64,
    max_workers: usize,
    writer: &Arc<PartFileWriter>,
    downloaded: &Arc<AtomicU64>,
    active_workers: &Arc<AtomicUsize>,
) -> Result<(u64, usize), EngineError> {
    let initial_specs = plan_segments(total_bytes, max_workers);
    if initial_specs.is_empty() {
        return Ok((0, 0));
    }

    let worker_count = initial_specs.len();
    let next_id = Arc::new(AtomicUsize::new(worker_count));
    let active_segments: Arc<Mutex<Vec<Arc<ActiveSegment>>>> = Arc::new(Mutex::new(
        initial_specs
            .iter()
            .enumerate()
            .map(|(id, spec)| ActiveSegment::new(id, spec.start, spec.end))
            .collect(),
    ));

    let initial_list = {
        let guard = active_segments.lock().expect("active_segments poisoned");
        guard.clone()
    };

    let mut join_set = JoinSet::new();
    for initial_seg in initial_list {
        let client = client.clone();
        let url = url.to_string();
        let writer = Arc::clone(writer);
        let downloaded = Arc::clone(downloaded);
        let active_workers = Arc::clone(active_workers);
        let active_segments = Arc::clone(&active_segments);
        let next_id = Arc::clone(&next_id);

        join_set.spawn(async move {
            active_workers.fetch_add(1, Ordering::Relaxed);
            let res = run_worker_loop(
                &client,
                &url,
                initial_seg,
                worker_count == 1,
                &writer,
                &downloaded,
                &active_segments,
                &next_id,
            )
            .await;
            active_workers.fetch_sub(1, Ordering::Relaxed);
            res
        });
    }

    let mut total_written = 0u64;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok(bytes)) => total_written += bytes,
            Ok(Err(err)) => {
                join_set.abort_all();
                return Err(err);
            }
            Err(join_err) => {
                join_set.abort_all();
                return Err(EngineError::Writer(format!(
                    "worker task failed: {join_err}"
                )));
            }
        }
    }

    Ok((total_written, worker_count))
}

#[allow(clippy::too_many_arguments)]
async fn run_worker_loop(
    client: &reqwest::Client,
    url: &str,
    mut current_seg: Arc<ActiveSegment>,
    allow_full_200: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    active_segments: &Mutex<Vec<Arc<ActiveSegment>>>,
    next_id: &AtomicUsize,
) -> Result<u64, EngineError> {
    let mut worker_total = 0u64;

    loop {
        let seg_bytes =
            run_segment_worker(client, url, &current_seg, allow_full_200, writer, downloaded)
                .await?;
        worker_total += seg_bytes;

        remove_active_segment(active_segments, current_seg.id);
        match steal_largest_segment(active_segments, next_id) {
            Some(stolen) => current_seg = stolen,
            None => break,
        }
    }

    Ok(worker_total)
}

fn remove_active_segment(active_segments: &Mutex<Vec<Arc<ActiveSegment>>>, seg_id: usize) {
    let mut guard = active_segments.lock().expect("active_segments poisoned");
    guard.retain(|s| s.id != seg_id);
}

fn steal_largest_segment(
    active_segments: &Mutex<Vec<Arc<ActiveSegment>>>,
    next_id: &AtomicUsize,
) -> Option<Arc<ActiveSegment>> {
    let mut guard = active_segments.lock().expect("active_segments poisoned");

    let target = guard
        .iter()
        .max_by_key(|seg| seg.remaining())
        .cloned()?;

    let cur = target.cursor.load(Ordering::Acquire);
    let old_end = target.end.load(Ordering::Acquire);
    let remaining = old_end.saturating_sub(cur);
    if remaining <= MIN_SEGMENT_SIZE {
        return None;
    }

    let mid = cur + remaining / 2;
    if target
        .end
        .compare_exchange(old_end, mid, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }

    let cur_after = target.cursor.load(Ordering::Acquire);
    let steal_start = cur_after.max(mid);
    if steal_start >= old_end {
        return None;
    }

    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let new_seg = ActiveSegment::new(id, steal_start, old_end);
    guard.push(Arc::clone(&new_seg));
    Some(new_seg)
}

async fn run_segment_worker(
    client: &reqwest::Client,
    url: &str,
    seg: &ActiveSegment,
    allow_full_200: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
) -> Result<u64, EngineError> {
    let initial_end = seg.end.load(Ordering::Acquire);
    if seg.start >= initial_end {
        return Ok(0);
    }

    let range_header = format!("bytes={}-{}", seg.start, initial_end - 1);
    let resp = client.get(url).header(RANGE, range_header).send().await?;
    let status = resp.status();

    if !(status == StatusCode::PARTIAL_CONTENT || (allow_full_200 && status == StatusCode::OK)) {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: resp.url().to_string(),
        });
    }

    let mut cur = seg.start;
    let mut write_offset = seg.start;
    let mut buffer = BytesMut::with_capacity(CHUNK_BUFFER_SIZE);
    let mut stream = resp.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        let limit = seg.end.load(Ordering::Acquire);
        let remaining = limit.saturating_sub(cur) as usize;
        if remaining == 0 {
            break;
        }

        let slice = if chunk.len() > remaining {
            chunk.slice(..remaining)
        } else {
            chunk
        };

        let len = slice.len() as u64;
        cur += len;
        seg.cursor.store(cur, Ordering::Release);
        downloaded.fetch_add(len, Ordering::Relaxed);
        buffer.extend_from_slice(&slice);

        if buffer.len() >= CHUNK_BUFFER_SIZE {
            let block = buffer.split().freeze();
            let block_len = block.len() as u64;
            writer.write_at(write_offset, block).await?;
            write_offset += block_len;
        }

        if cur >= seg.end.load(Ordering::Acquire) {
            break;
        }
    }

    if !buffer.is_empty() {
        let tail = buffer.freeze();
        writer.write_at(write_offset, tail).await?;
    }

    let final_end = seg.end.load(Ordering::Acquire);
    if cur < final_end {
        return Err(EngineError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "segment [{}, {}) truncated: reached {cur} of {final_end}",
                seg.start, final_end
            ),
        )));
    }

    Ok(cur - seg.start)
}

async fn stream_unbounded_to_writer(
    resp: reqwest::Response,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
) -> Result<u64, EngineError> {
    let mut written = 0u64;
    let mut write_offset = 0u64;
    let mut buffer = BytesMut::with_capacity(CHUNK_BUFFER_SIZE);
    let mut stream = resp.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        let len = chunk.len() as u64;
        written += len;
        downloaded.fetch_add(len, Ordering::Relaxed);
        buffer.extend_from_slice(&chunk);

        if buffer.len() >= CHUNK_BUFFER_SIZE {
            let block = buffer.split().freeze();
            let block_len = block.len() as u64;
            writer.write_at(write_offset, block).await?;
            write_offset += block_len;
        }
    }

    if !buffer.is_empty() {
        let tail = buffer.freeze();
        writer.write_at(write_offset, tail).await?;
    }

    Ok(written)
}
