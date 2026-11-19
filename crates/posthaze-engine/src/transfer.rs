use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::BytesMut;
use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::RANGE;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::probe::apply_custom_headers;
use crate::store::DownloadStore;
use crate::writer::PartFileWriter;
use crate::{DownloadId, EngineError};

pub const MIN_SEGMENT_SIZE: u64 = 2 * 1024 * 1024;
pub const CHUNK_BUFFER_SIZE: usize = 512 * 1024;
const MAX_SEGMENT_RETRIES: u32 = 5;
const BASE_BACKOFF_MS: u64 = 100;
const MAX_BACKOFF_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlSignal {
    Run,
    Pause,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentCheckpoint {
    pub segment_index: usize,
    pub start: u64,
    pub cursor: u64,
    pub end: Option<u64>,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpec {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
pub(crate) struct ActiveSegment {
    id: usize,
    start: u64,
    cursor: AtomicU64,
    end: AtomicU64,
    completed: AtomicBool,
    claimed: AtomicBool,
}

impl ActiveSegment {
    pub(crate) fn snapshot_progress(
        &self,
        cp_map: &std::collections::HashMap<usize, u64>,
    ) -> crate::SegmentProgress {
        let cursor = self.cursor.load(Ordering::Relaxed).max(self.start);
        let end = self.end.load(Ordering::Relaxed);
        let flushed = cp_map.get(&self.id).copied().unwrap_or(self.start);
        crate::SegmentProgress {
            id: self.id,
            start: self.start,
            cursor,
            checkpointed_cursor: flushed.max(self.start).min(cursor),
            end: Some(end),
            completed: self.completed.load(Ordering::Relaxed),
        }
    }
}

pub(crate) type LiveSegments = Arc<Mutex<Vec<Arc<ActiveSegment>>>>;

impl ActiveSegment {
    fn new(id: usize, start: u64, end: u64) -> Arc<Self> {
        Self::from_checkpoint(id, start, start, end, false)
    }

    fn from_checkpoint(id: usize, start: u64, cursor: u64, end: u64, completed: bool) -> Arc<Self> {
        Arc::new(Self {
            id,
            start,
            cursor: AtomicU64::new(cursor),
            end: AtomicU64::new(end),
            completed: AtomicBool::new(completed),
            claimed: AtomicBool::new(false),
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
    headers: &[(String, String)],
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    active_workers: &AtomicUsize,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<u64, EngineError> {
    if let Some(sig_err) = check_control_signal(&control_rx) {
        return Err(sig_err);
    }

    active_workers.store(1, Ordering::Relaxed);
    let resp = tokio::select! {
        biased;
        _ = control_rx.changed() => {
            active_workers.store(0, Ordering::Relaxed);
            return Err(signal_to_error(*control_rx.borrow()));
        }
        res = apply_custom_headers(client.get(url), headers).send() => res?,
    };
    let status = resp.status();
    if !status.is_success() {
        active_workers.store(0, Ordering::Relaxed);
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: resp.url().to_string(),
        });
    }

    let result = stream_unbounded_to_writer(resp, writer, downloaded, control_rx).await;
    active_workers.store(0, Ordering::Relaxed);
    result
}

/// Summary metrics returned by a multi-`Segment` transfer.
pub struct MultiSegmentResult {
    pub bytes_written: u64,
    pub workers_used: usize,
    pub splits_performed: usize,
    pub retries_performed: usize,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_multi_segment(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    total_bytes: u64,
    max_workers: usize,
    writer: &Arc<PartFileWriter>,
    downloaded: &Arc<AtomicU64>,
    active_workers: &Arc<AtomicUsize>,
    control_rx: watch::Receiver<ControlSignal>,
    persist: Option<(DownloadId, DownloadStore)>,
    live_segments_slot: Option<&Arc<Mutex<Option<LiveSegments>>>>,
) -> Result<MultiSegmentResult, EngineError> {
    let initial_specs = plan_segments(total_bytes, max_workers);
    let checkpoints: Vec<SegmentCheckpoint> = initial_specs
        .into_iter()
        .enumerate()
        .map(|(segment_index, spec)| SegmentCheckpoint {
            segment_index,
            start: spec.start,
            cursor: spec.start,
            end: Some(spec.end),
            completed: false,
        })
        .collect();

    execute_multi_segment_from_checkpoints(
        client,
        url,
        headers,
        checkpoints,
        max_workers,
        writer,
        downloaded,
        active_workers,
        control_rx,
        persist,
        live_segments_slot,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_multi_segment_from_checkpoints(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    checkpoints: Vec<SegmentCheckpoint>,
    max_workers: usize,
    writer: &Arc<PartFileWriter>,
    downloaded: &Arc<AtomicU64>,
    active_workers: &Arc<AtomicUsize>,
    control_rx: watch::Receiver<ControlSignal>,
    persist: Option<(DownloadId, DownloadStore)>,
    live_segments_slot: Option<&Arc<Mutex<Option<LiveSegments>>>>,
) -> Result<MultiSegmentResult, EngineError> {
    if checkpoints.is_empty() {
        return Ok(MultiSegmentResult {
            bytes_written: 0,
            workers_used: 0,
            splits_performed: 0,
            retries_performed: 0,
        });
    }

    let max_idx = checkpoints
        .iter()
        .map(|c| c.segment_index)
        .max()
        .unwrap_or(0);
    let next_id = Arc::new(AtomicUsize::new(max_idx + 1));
    let splits_performed = Arc::new(AtomicUsize::new(0));
    let retries_performed = Arc::new(AtomicUsize::new(0));

    let active_segments: Arc<Mutex<Vec<Arc<ActiveSegment>>>> = Arc::new(Mutex::new(
        checkpoints
            .iter()
            .map(|c| {
                let end = c.end.unwrap_or(c.cursor);
                ActiveSegment::from_checkpoint(
                    c.segment_index,
                    c.start,
                    c.cursor,
                    end,
                    c.completed || c.cursor >= end,
                )
            })
            .collect(),
    ));

    if let Some(slot) = live_segments_slot {
        *slot.lock().unwrap() = Some(Arc::clone(&active_segments));
    }

    let incomplete_list: Vec<Arc<ActiveSegment>> = {
        let guard = active_segments.lock().expect("active_segments poisoned");
        guard
            .iter()
            .filter(|seg| !seg.completed.load(Ordering::Acquire))
            .cloned()
            .collect()
    };

    if incomplete_list.is_empty() {
        let total_written = sum_segment_bytes(&active_segments);
        return Ok(MultiSegmentResult {
            bytes_written: total_written,
            workers_used: 0,
            splits_performed: 0,
            retries_performed: 0,
        });
    }

    let worker_budget = max_workers.max(1);
    let mut initial_work: Vec<Arc<ActiveSegment>> = Vec::with_capacity(worker_budget);
    for seg in incomplete_list.into_iter().take(worker_budget) {
        seg.claimed.store(true, Ordering::Release);
        initial_work.push(seg);
    }

    while initial_work.len() < worker_budget {
        let Some((target_idx, new_end, stolen)) = steal_largest_segment(&active_segments, &next_id)
        else {
            break;
        };
        if let Some((download_id, store)) = &persist {
            let stolen_end = stolen.end.load(Ordering::Acquire);
            store
                .split_segment(
                    download_id,
                    target_idx,
                    new_end,
                    stolen.id,
                    stolen.start,
                    stolen_end,
                )
                .await?;
        }
        stolen.claimed.store(true, Ordering::Release);
        splits_performed.fetch_add(1, Ordering::Relaxed);
        initial_work.push(stolen);
    }

    let worker_count = initial_work.len();
    let mut join_set = JoinSet::new();
    for initial_seg in initial_work {
        let client = client.clone();
        let url = url.to_string();
        let writer = Arc::clone(writer);
        let downloaded = Arc::clone(downloaded);
        let active_workers = Arc::clone(active_workers);
        let active_segments = Arc::clone(&active_segments);
        let next_id = Arc::clone(&next_id);
        let splits_performed = Arc::clone(&splits_performed);
        let retries_performed = Arc::clone(&retries_performed);
        let control_rx = control_rx.clone();
        let persist = persist.clone();
        let headers: Arc<[(String, String)]> = headers.to_vec().into();

        join_set.spawn(async move {
            active_workers.fetch_add(1, Ordering::Relaxed);
            let res = run_worker_loop(
                &client,
                &url,
                &headers,
                initial_seg,
                worker_count == 1,
                &writer,
                &downloaded,
                &active_segments,
                &next_id,
                &splits_performed,
                &retries_performed,
                control_rx,
                persist,
            )
            .await;
            active_workers.fetch_sub(1, Ordering::Relaxed);
            res
        });
    }

    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                if matches!(err, EngineError::Paused) {
                    while join_set.join_next().await.is_some() {}
                } else {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                }
                return Err(err);
            }
            Err(join_err) => {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                return Err(EngineError::Writer(format!("worker task failed: {join_err}")));
            }
        }
    }

    Ok(MultiSegmentResult {
        bytes_written: sum_segment_bytes(&active_segments),
        workers_used: worker_count,
        splits_performed: splits_performed.load(Ordering::Relaxed),
        retries_performed: retries_performed.load(Ordering::Relaxed),
    })
}

fn sum_segment_bytes(active_segments: &Mutex<Vec<Arc<ActiveSegment>>>) -> u64 {
    let guard = active_segments.lock().expect("active_segments poisoned");
    guard
        .iter()
        .map(|s| s.cursor.load(Ordering::Acquire).saturating_sub(s.start))
        .sum()
}

#[allow(clippy::too_many_arguments)]
async fn run_worker_loop(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    mut current_seg: Arc<ActiveSegment>,
    allow_full_200: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    active_segments: &Mutex<Vec<Arc<ActiveSegment>>>,
    next_id: &AtomicUsize,
    splits_performed: &AtomicUsize,
    retries_performed: &AtomicUsize,
    mut control_rx: watch::Receiver<ControlSignal>,
    persist: Option<(DownloadId, DownloadStore)>,
) -> Result<u64, EngineError> {
    let mut worker_total = 0u64;

    loop {
        let seg_bytes = run_segment_worker(
            client,
            url,
            headers,
            &current_seg,
            allow_full_200,
            writer,
            downloaded,
            retries_performed,
            &mut control_rx,
        )
        .await?;
        worker_total += seg_bytes;

        current_seg.completed.store(true, Ordering::Release);
        if let Some(unclaimed) = claim_unclaimed_segment(active_segments) {
            current_seg = unclaimed;
            continue;
        }
        match steal_largest_segment(active_segments, next_id) {
            Some((target_idx, new_end, stolen)) => {
                if let Some((download_id, store)) = &persist {
                    let stolen_end = stolen.end.load(Ordering::Acquire);
                    store
                        .split_segment(
                            download_id,
                            target_idx,
                            new_end,
                            stolen.id,
                            stolen.start,
                            stolen_end,
                        )
                        .await?;
                }
                stolen.claimed.store(true, Ordering::Release);
                splits_performed.fetch_add(1, Ordering::Relaxed);
                current_seg = stolen;
            }
            None => break,
        }
    }

    Ok(worker_total)
}

fn claim_unclaimed_segment(
    active_segments: &Mutex<Vec<Arc<ActiveSegment>>>,
) -> Option<Arc<ActiveSegment>> {
    let guard = active_segments.lock().expect("active_segments poisoned");
    for seg in guard.iter() {
        if !seg.completed.load(Ordering::Acquire) && !seg.claimed.swap(true, Ordering::AcqRel) {
            return Some(Arc::clone(seg));
        }
    }
    None
}

fn steal_largest_segment(
    active_segments: &Mutex<Vec<Arc<ActiveSegment>>>,
    next_id: &AtomicUsize,
) -> Option<(usize, u64, Arc<ActiveSegment>)> {
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
    Some((target.id, steal_start, new_seg))
}

#[allow(clippy::too_many_arguments)]
async fn run_segment_worker(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    seg: &ActiveSegment,
    allow_full_200: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    retries_performed: &AtomicUsize,
    control_rx: &mut watch::Receiver<ControlSignal>,
) -> Result<u64, EngineError> {
    let mut retries = 0u32;

    loop {
        if let Some(sig_err) = check_control_signal(control_rx) {
            return Err(sig_err);
        }

        let attempt_start = seg.cursor.load(Ordering::Acquire);
        let end_snapshot = seg.end.load(Ordering::Acquire);
        if attempt_start >= end_snapshot {
            return Ok(attempt_start.saturating_sub(seg.start));
        }

        match run_segment_attempt(
            client,
            url,
            headers,
            seg,
            attempt_start,
            end_snapshot,
            allow_full_200,
            writer,
            downloaded,
            control_rx,
        )
        .await
        {
            Ok(()) => {
                let final_cur = seg.cursor.load(Ordering::Acquire);
                return Ok(final_cur.saturating_sub(seg.start));
            }
            Err(err) => {
                if matches!(err, EngineError::Paused | EngineError::Cancelled) {
                    return Err(err);
                }

                let made_progress = seg.cursor.load(Ordering::Acquire) > attempt_start;
                if made_progress {
                    retries = 0;
                } else {
                    retries += 1;
                }

                if !is_transient_error(&err) || retries > MAX_SEGMENT_RETRIES {
                    return Err(err);
                }

                retries_performed.fetch_add(1, Ordering::Relaxed);
                let delay = backoff_with_jitter(retries.saturating_sub(1));
                tokio::select! {
                    biased;
                    _ = control_rx.changed() => {
                        if let Some(sig_err) = check_control_signal(control_rx) {
                            return Err(sig_err);
                        }
                    }
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_segment_attempt(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    seg: &ActiveSegment,
    attempt_start: u64,
    end_snapshot: u64,
    allow_full_200: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    control_rx: &mut watch::Receiver<ControlSignal>,
) -> Result<(), EngineError> {
    let range_header = format!("bytes={attempt_start}-{}", end_snapshot - 1);
    let resp = tokio::select! {
        biased;
        _ = control_rx.changed() => {
            if let Some(sig_err) = check_control_signal(control_rx) {
                return Err(sig_err);
            }
            unreachable!("Run signal does not trigger abort");
        }
        res = apply_custom_headers(client.get(url).header(RANGE, range_header), headers).send() => res?,
    };
    let status = resp.status();

    if !(status == StatusCode::PARTIAL_CONTENT
        || (allow_full_200 && attempt_start == 0 && status == StatusCode::OK))
    {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: resp.url().to_string(),
        });
    }

    let mut cur = attempt_start;
    let mut write_offset = attempt_start;
    let mut buffer = BytesMut::with_capacity(CHUNK_BUFFER_SIZE * 2);
    let mut stream = resp.bytes_stream();
    let mut stream_err: Option<EngineError> = None;

    loop {
        let next_item = tokio::select! {
            biased;
            _ = control_rx.changed() => {
                match *control_rx.borrow() {
                    ControlSignal::Run => continue,
                    ControlSignal::Pause => {
                        stream_err = Some(EngineError::Paused);
                        break;
                    }
                    ControlSignal::Cancel => return Err(EngineError::Cancelled),
                }
            }
            item = stream.next() => item,
        };

        let Some(item) = next_item else {
            break;
        };

        let chunk = match item {
            Ok(chunk) => chunk,
            Err(err) => {
                stream_err = Some(err.into());
                break;
            }
        };

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
            buffer.reserve(CHUNK_BUFFER_SIZE);
            let block_len = block.len() as u64;
            writer.write_segment_at(seg.id, write_offset, block).await?;
            write_offset += block_len;
        }

        if cur >= seg.end.load(Ordering::Acquire) {
            break;
        }
    }

    if !buffer.is_empty() {
        let tail = buffer.freeze();
        writer.write_segment_at(seg.id, write_offset, tail).await?;
    }

    if let Some(err) = stream_err {
        return Err(err);
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

    Ok(())
}

fn is_transient_error(err: &EngineError) -> bool {
    match err {
        EngineError::Http(_) => true,
        EngineError::HttpStatus { status, .. } => matches!(*status, 408 | 429 | 500..=599),
        EngineError::Io(io_err) => io_err.kind() == std::io::ErrorKind::UnexpectedEof,
        EngineError::Writer(_)
        | EngineError::Database(_)
        | EngineError::Migration(_)
        | EngineError::InvalidState(_)
        | EngineError::NotFound(_)
        | EngineError::ResourceChanged(_)
        | EngineError::Paused
        | EngineError::Cancelled
        | EngineError::TaskJoin(_) => false,
    }
}

fn check_control_signal(control_rx: &watch::Receiver<ControlSignal>) -> Option<EngineError> {
    match *control_rx.borrow() {
        ControlSignal::Run => None,
        sig => Some(signal_to_error(sig)),
    }
}

fn signal_to_error(sig: ControlSignal) -> EngineError {
    match sig {
        ControlSignal::Run => unreachable!("Run is not an interruption"),
        ControlSignal::Pause => EngineError::Paused,
        ControlSignal::Cancel => EngineError::Cancelled,
    }
}

fn backoff_with_jitter(retry_index: u32) -> Duration {
    let exp_ms = BASE_BACKOFF_MS
        .saturating_mul(1u64 << retry_index.min(10))
        .min(MAX_BACKOFF_MS);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as u64);
    let jitter_ms = nanos % (exp_ms / 2).max(1);
    Duration::from_millis(exp_ms + jitter_ms)
}

async fn stream_unbounded_to_writer(
    resp: reqwest::Response,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<u64, EngineError> {
    let mut written = 0u64;
    let mut write_offset = 0u64;
    let mut buffer = BytesMut::with_capacity(CHUNK_BUFFER_SIZE * 2);
    let mut stream = resp.bytes_stream();
    let mut interrupted: Option<EngineError> = None;

    loop {
        let next_item = tokio::select! {
            biased;
            _ = control_rx.changed() => {
                match *control_rx.borrow() {
                    ControlSignal::Run => continue,
                    ControlSignal::Pause => {
                        interrupted = Some(EngineError::Paused);
                        break;
                    }
                    ControlSignal::Cancel => return Err(EngineError::Cancelled),
                }
            }
            item = stream.next() => item,
        };

        let Some(item) = next_item else {
            break;
        };

        let chunk = item?;
        let len = chunk.len() as u64;
        written += len;
        downloaded.fetch_add(len, Ordering::Relaxed);
        buffer.extend_from_slice(&chunk);

        if buffer.len() >= CHUNK_BUFFER_SIZE {
            let block = buffer.split().freeze();
            buffer.reserve(CHUNK_BUFFER_SIZE);
            let block_len = block.len() as u64;
            writer.write_at(write_offset, block).await?;
            write_offset += block_len;
        }
    }

    if !buffer.is_empty() {
        let tail = buffer.freeze();
        writer.write_at(write_offset, tail).await?;
    }

    if let Some(err) = interrupted {
        return Err(err);
    }

    Ok(written)
}
