use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

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

    let result = stream_body_to_writer(resp, 0, None, writer, downloaded).await;
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
    let segments = plan_segments(total_bytes, max_workers);
    if segments.is_empty() {
        return Ok((0, 0));
    }

    let worker_count = segments.len();
    let mut join_set = JoinSet::new();

    for seg in segments {
        let client = client.clone();
        let url = url.to_string();
        let writer = Arc::clone(writer);
        let downloaded = Arc::clone(downloaded);
        let active_workers = Arc::clone(active_workers);

        join_set.spawn(async move {
            active_workers.fetch_add(1, Ordering::Relaxed);
            let res = run_segment_worker(
                &client,
                &url,
                seg,
                worker_count == 1,
                &writer,
                &downloaded,
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

async fn run_segment_worker(
    client: &reqwest::Client,
    url: &str,
    seg: SegmentSpec,
    allow_full_200: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
) -> Result<u64, EngineError> {
    let expected_len = seg.end - seg.start;
    let range_header = format!("bytes={}-{}", seg.start, seg.end - 1);
    let resp = client.get(url).header(RANGE, range_header).send().await?;
    let status = resp.status();

    if !(status == StatusCode::PARTIAL_CONTENT || (allow_full_200 && status == StatusCode::OK)) {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: resp.url().to_string(),
        });
    }

    let written =
        stream_body_to_writer(resp, seg.start, Some(expected_len), writer, downloaded).await?;
    if written < expected_len {
        return Err(EngineError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "segment [{}, {}) truncated: got {written} of {expected_len} bytes",
                seg.start, seg.end
            ),
        )));
    }
    Ok(written)
}

async fn stream_body_to_writer(
    resp: reqwest::Response,
    start_offset: u64,
    max_bytes: Option<u64>,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
) -> Result<u64, EngineError> {
    let mut written = 0u64;
    let mut write_offset = start_offset;
    let mut buffer = BytesMut::with_capacity(CHUNK_BUFFER_SIZE);
    let mut stream = resp.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        let slice = match max_bytes {
            Some(limit) => {
                let remaining = limit.saturating_sub(written) as usize;
                if remaining == 0 {
                    break;
                }
                if chunk.len() > remaining {
                    chunk.slice(..remaining)
                } else {
                    chunk
                }
            }
            None => chunk,
        };

        let len = slice.len() as u64;
        written += len;
        downloaded.fetch_add(len, Ordering::Relaxed);
        buffer.extend_from_slice(&slice);

        if buffer.len() >= CHUNK_BUFFER_SIZE {
            let block = buffer.split().freeze();
            let block_len = block.len() as u64;
            writer.write_at(write_offset, block).await?;
            write_offset += block_len;
        }

        if let Some(limit) = max_bytes
            && written >= limit
        {
            break;
        }
    }

    if !buffer.is_empty() {
        let tail = buffer.freeze();
        writer.write_at(write_offset, tail).await?;
    }

    Ok(written)
}
