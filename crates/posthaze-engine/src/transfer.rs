use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderMap, RANGE};
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::model::SegmentCheckpoint;
use crate::probe::{apply_custom_headers, status_to_error};
use crate::throttle::BandwidthLimiter;
use crate::writer::{PartFileWriter, PooledBuffer};
use crate::EngineError;

pub const MIN_SEGMENT_SIZE: u64 = 2 * 1024 * 1024;
const SPLIT_ALIGN: u64 = 4096;
pub const CHUNK_BUFFER_SIZE: usize = 512 * 1024;
const MAX_SEGMENT_RETRIES: u32 = 5;
const BASE_BACKOFF_MS: u64 = 100;
const MAX_BACKOFF_MS: u64 = 5_000;

const SHED_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const SHED_GRACE: Duration = Duration::from_secs(2);
const SHED_BEST_AFTER_TIME: Duration = Duration::from_millis(100);
const SHED_BEST_AFTER_BYTES: u64 = 256 * 1024;
const SHED_RATIO: u64 = 4;
const SHED_LEFT_SECS: f64 = 2.0;
const LINK_WINDOW: Duration = Duration::from_secs(1);
const MAX_SHEDS_PER_SEGMENT: u32 = 3;

#[derive(Clone, Copy)]
struct TransferContext<'a> {
    client: &'a reqwest::Client,
    url: &'a str,
    headers: &'a [(String, String)],
    limiter: Option<&'a BandwidthLimiter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlSignal {
    Run,
    Pause,
    Cancel,
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

#[derive(Debug)]
pub(crate) struct SegmentPool {
    pub(crate) segments: Vec<Arc<ActiveSegment>>,
    pub(crate) active_workers: usize,
}

impl SegmentPool {
    pub(crate) fn len(&self) -> usize {
        self.segments.len()
    }

    pub(crate) fn snapshot_checkpoints(&self) -> Vec<SegmentCheckpoint> {
        self.segments
            .iter()
            .map(|seg| {
                let cursor = seg.cursor.load(Ordering::Acquire).max(seg.start);
                let end = seg.end.load(Ordering::Acquire);
                let completed = seg.completed.load(Ordering::Acquire) || cursor >= end;
                SegmentCheckpoint {
                    segment_index: seg.id,
                    start: seg.start,
                    cursor,
                    end: Some(end),
                    completed,
                }
            })
            .collect()
    }
}

pub(crate) type LiveSegments = Arc<Mutex<SegmentPool>>;

#[derive(Debug)]
struct LinkWindow {
    window_start: Instant,
    bytes: u64,
    link_bps: u64,
}

#[derive(Debug)]
struct SpeedMonitor {
    best_worker_bps: AtomicU64,
    link: Mutex<LinkWindow>,
}

impl SpeedMonitor {
    fn new() -> Self {
        Self {
            best_worker_bps: AtomicU64::new(0),
            link: Mutex::new(LinkWindow {
                window_start: Instant::now(),
                bytes: 0,
                link_bps: 0,
            }),
        }
    }

    fn record_bytes(&self, len: u64) {
        let mut guard = self.link.lock().unwrap_or_else(|e| e.into_inner());
        guard.bytes += len;
        let elapsed = guard.window_start.elapsed();
        if elapsed >= LINK_WINDOW {
            let bps = ((guard.bytes as f64) / elapsed.as_secs_f64()).round() as u64;
            guard.link_bps = bps;
            guard.bytes = 0;
            guard.window_start = Instant::now();
        }
    }

    fn update_best_worker(&self, bps: u64) {
        self.best_worker_bps.fetch_max(bps, Ordering::Relaxed);
    }

    fn shed_bar(&self, active_workers: usize) -> Option<u64> {
        let best = self.best_worker_bps.load(Ordering::Relaxed);
        if best == 0 || active_workers <= 1 {
            return None;
        }
        let link = {
            let guard = self.link.lock().unwrap_or_else(|e| e.into_inner());
            if guard.link_bps > 0 {
                guard.link_bps
            } else {
                let elapsed = guard.window_start.elapsed().as_secs_f64();
                if elapsed > 0.1 {
                    ((guard.bytes as f64) / elapsed).round() as u64
                } else {
                    0
                }
            }
        };
        let fair_share = link.max(best) / active_workers as u64;
        Some(best.min(fair_share.max(1)))
    }
}

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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_single_stream(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    limiter: Option<&BandwidthLimiter>,
    expected_content_length: Option<u64>,
    initial_response: Option<reqwest::Response>,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    active_workers: &AtomicUsize,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<u64, EngineError> {
    if let Some(sig_err) = check_control_signal(&control_rx) {
        return Err(sig_err);
    }

    active_workers.store(1, Ordering::Relaxed);
    let resp = match initial_response {
        Some(resp) => resp,
        None => {
            tokio::select! {
                biased;
                _ = control_rx.changed() => {
                    active_workers.store(0, Ordering::Relaxed);
                    return Err(signal_to_error(*control_rx.borrow()));
                }
                res = apply_custom_headers(client.get(url), headers).send() => res?,
            }
        }
    };
    let status = resp.status();
    if !status.is_success() {
        active_workers.store(0, Ordering::Relaxed);
        return Err(status_to_error(
            status,
            resp.headers(),
            resp.url().to_string(),
        ));
    }

    if status == StatusCode::OK
        && is_html_content_type(resp.headers())
        && expected_content_length.is_some()
        && resp.content_length() != expected_content_length
    {
        active_workers.store(0, Ordering::Relaxed);
        return Err(build_html_error(resp).await);
    }

    let result = stream_unbounded_to_writer(resp, limiter, writer, downloaded, control_rx).await;
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
    limiter: Option<&Arc<BandwidthLimiter>>,
    total_bytes: u64,
    max_workers: usize,
    initial_response: Option<reqwest::Response>,
    writer: &Arc<PartFileWriter>,
    downloaded: &Arc<AtomicU64>,
    active_workers: &Arc<AtomicUsize>,
    control_rx: watch::Receiver<ControlSignal>,
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
        limiter,
        checkpoints,
        max_workers,
        initial_response,
        writer,
        downloaded,
        active_workers,
        control_rx,
        live_segments_slot,
    )
    .await
}

enum WorkerExit {
    Finished,
    Retired(EngineError),
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_multi_segment_from_checkpoints(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    limiter: Option<&Arc<BandwidthLimiter>>,
    checkpoints: Vec<SegmentCheckpoint>,
    max_workers: usize,
    mut initial_response: Option<reqwest::Response>,
    writer: &Arc<PartFileWriter>,
    downloaded: &Arc<AtomicU64>,
    active_workers: &Arc<AtomicUsize>,
    control_rx: watch::Receiver<ControlSignal>,
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
    let speed_monitor = Arc::new(SpeedMonitor::new());

    let segments_vec: Vec<Arc<ActiveSegment>> = checkpoints
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
        .collect();

    let segment_pool: LiveSegments = Arc::new(Mutex::new(SegmentPool {
        segments: segments_vec,
        active_workers: 0,
    }));

    if let Some(slot) = live_segments_slot {
        *slot.lock().unwrap() = Some(Arc::clone(&segment_pool));
    }

    let incomplete_list: Vec<Arc<ActiveSegment>> = {
        let guard = segment_pool.lock().expect("segment_pool poisoned");
        guard
            .segments
            .iter()
            .filter(|seg| !seg.completed.load(Ordering::Acquire))
            .cloned()
            .collect()
    };

    if incomplete_list.is_empty() {
        let total_written = sum_segment_bytes(&segment_pool);
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
        let Some(stolen) = steal_largest_segment(&segment_pool, &next_id) else {
            break;
        };
        stolen.claimed.store(true, Ordering::Release);
        splits_performed.fetch_add(1, Ordering::Relaxed);
        initial_work.push(stolen);
    }

    let worker_count = initial_work.len();
    {
        let mut guard = segment_pool.lock().expect("segment_pool poisoned");
        guard.active_workers = worker_count;
    }

    let mut join_set = JoinSet::new();
    for (worker_idx, initial_seg) in initial_work.into_iter().enumerate() {
        let client = client.clone();
        let url = url.to_string();
        let writer = Arc::clone(writer);
        let downloaded = Arc::clone(downloaded);
        let active_workers = Arc::clone(active_workers);
        let segment_pool = Arc::clone(&segment_pool);
        let next_id = Arc::clone(&next_id);
        let splits_performed = Arc::clone(&splits_performed);
        let retries_performed = Arc::clone(&retries_performed);
        let speed_monitor = Arc::clone(&speed_monitor);
        let control_rx = control_rx.clone();
        let headers: Arc<[(String, String)]> = headers.to_vec().into();
        let limiter = limiter.cloned();
        let pre_opened = if worker_idx == 0 {
            initial_response.take()
        } else {
            None
        };

        join_set.spawn(async move {
            active_workers.fetch_add(1, Ordering::Relaxed);
            let tctx = TransferContext {
                client: &client,
                url: &url,
                headers: &headers,
                limiter: limiter.as_deref(),
            };
            let res = run_worker_loop(
                tctx,
                initial_seg,
                worker_count == 1,
                pre_opened,
                &writer,
                &downloaded,
                &segment_pool,
                &next_id,
                &splits_performed,
                &retries_performed,
                &speed_monitor,
                control_rx,
            )
            .await;
            active_workers.fetch_sub(1, Ordering::Relaxed);
            res
        });
    }

    let mut last_retired_err: Option<EngineError> = None;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok(WorkerExit::Finished)) => {}
            Ok(Ok(WorkerExit::Retired(err))) => {
                last_retired_err = Some(err);
            }
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

    let has_incomplete = {
        let guard = segment_pool.lock().expect("segment_pool poisoned");
        guard
            .segments
            .iter()
            .any(|seg| !seg.completed.load(Ordering::Acquire) && seg.remaining() > 0)
    };
    if has_incomplete {
        return Err(last_retired_err.unwrap_or_else(|| {
            EngineError::Writer("workers exited with incomplete segments".to_string())
        }));
    }

    Ok(MultiSegmentResult {
        bytes_written: sum_segment_bytes(&segment_pool),
        workers_used: worker_count,
        splits_performed: splits_performed.load(Ordering::Relaxed),
        retries_performed: retries_performed.load(Ordering::Relaxed),
    })
}

fn sum_segment_bytes(segment_pool: &Mutex<SegmentPool>) -> u64 {
    let guard = segment_pool.lock().expect("segment_pool poisoned");
    guard
        .segments
        .iter()
        .map(|s| s.cursor.load(Ordering::Acquire).saturating_sub(s.start))
        .sum()
}

fn is_fatal_download_error(err: &EngineError) -> bool {
    matches!(
        err,
        EngineError::Paused
            | EngineError::Cancelled
            | EngineError::Writer(_)
            | EngineError::UnexpectedHtmlResponse { .. }
            | EngineError::ResourceChanged(_)
            | EngineError::HttpStatus { status: 200, .. }
    )
}

#[allow(clippy::too_many_arguments)]
async fn run_worker_loop(
    tctx: TransferContext<'_>,
    mut current_seg: Arc<ActiveSegment>,
    allow_full_200: bool,
    mut pre_opened: Option<reqwest::Response>,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    segment_pool: &Mutex<SegmentPool>,
    next_id: &AtomicUsize,
    splits_performed: &AtomicUsize,
    retries_performed: &AtomicUsize,
    speed_monitor: &SpeedMonitor,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<WorkerExit, EngineError> {
    loop {
        let attempt_res = run_segment_worker(
            tctx,
            &current_seg,
            allow_full_200,
            pre_opened.take(),
            writer,
            downloaded,
            retries_performed,
            segment_pool,
            speed_monitor,
            &mut control_rx,
        )
        .await;

        match attempt_res {
            Ok(_) => {
                current_seg.completed.store(true, Ordering::Release);
                match next_segment_or_exit(segment_pool, next_id, splits_performed) {
                    Some(next_seg) => current_seg = next_seg,
                    None => return Ok(WorkerExit::Finished),
                }
            }
            Err(err) => {
                if is_fatal_download_error(&err) {
                    return Err(err);
                }
                if try_retire_segment(segment_pool, &current_seg) {
                    tracing::warn!(
                        segment_id = current_seg.id,
                        error = %err,
                        "worker retired after exhausting retries; releasing segment to surviving workers"
                    );
                    return Ok(WorkerExit::Retired(err));
                }
                return Err(err);
            }
        }
    }
}

fn try_retire_segment(segment_pool: &Mutex<SegmentPool>, seg: &ActiveSegment) -> bool {
    let mut guard = segment_pool.lock().expect("segment_pool poisoned");
    if guard.active_workers > 1 {
        seg.claimed.store(false, Ordering::Release);
        guard.active_workers -= 1;
        true
    } else {
        false
    }
}

fn next_segment_or_exit(
    segment_pool: &Mutex<SegmentPool>,
    next_id: &AtomicUsize,
    splits_performed: &AtomicUsize,
) -> Option<Arc<ActiveSegment>> {
    let mut guard = segment_pool.lock().expect("segment_pool poisoned");

    for seg in &guard.segments {
        if !seg.completed.load(Ordering::Acquire) && !seg.claimed.swap(true, Ordering::AcqRel) {
            return Some(Arc::clone(seg));
        }
    }

    if let Some(stolen) = steal_largest_in_pool(&mut guard, next_id) {
        stolen.claimed.store(true, Ordering::Release);
        splits_performed.fetch_add(1, Ordering::Relaxed);
        return Some(stolen);
    }

    guard.active_workers = guard.active_workers.saturating_sub(1);
    None
}

fn steal_largest_segment(
    segment_pool: &Mutex<SegmentPool>,
    next_id: &AtomicUsize,
) -> Option<Arc<ActiveSegment>> {
    let mut guard = segment_pool.lock().expect("segment_pool poisoned");
    steal_largest_in_pool(&mut guard, next_id)
}

fn steal_largest_in_pool(
    guard: &mut SegmentPool,
    next_id: &AtomicUsize,
) -> Option<Arc<ActiveSegment>> {
    let target = guard
        .segments
        .iter()
        .max_by_key(|seg| seg.remaining())
        .cloned()?;

    let cur = target.cursor.load(Ordering::Acquire);
    let old_end = target.end.load(Ordering::Acquire);
    let remaining = old_end.saturating_sub(cur);
    if remaining <= MIN_SEGMENT_SIZE {
        return None;
    }

    let raw_mid = cur + remaining / 2;
    let mid = if remaining >= 2 * SPLIT_ALIGN {
        let aligned = (raw_mid / SPLIT_ALIGN) * SPLIT_ALIGN;
        if aligned > cur && aligned < old_end {
            aligned
        } else {
            raw_mid
        }
    } else {
        raw_mid
    };

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
    guard.segments.push(Arc::clone(&new_seg));
    Some(new_seg)
}

enum AttemptAbort {
    Engine(EngineError),
    ShedSlow,
}

impl From<EngineError> for AttemptAbort {
    fn from(value: EngineError) -> Self {
        Self::Engine(value)
    }
}

impl From<reqwest::Error> for AttemptAbort {
    fn from(value: reqwest::Error) -> Self {
        Self::Engine(EngineError::Http(value))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_segment_worker(
    tctx: TransferContext<'_>,
    seg: &ActiveSegment,
    allow_full_200: bool,
    mut pre_opened: Option<reqwest::Response>,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    retries_performed: &AtomicUsize,
    segment_pool: &Mutex<SegmentPool>,
    speed_monitor: &SpeedMonitor,
    control_rx: &mut watch::Receiver<ControlSignal>,
) -> Result<u64, EngineError> {
    let mut retries = 0u32;
    let mut shed_count = 0u32;

    loop {
        if let Some(sig_err) = check_control_signal(control_rx) {
            return Err(sig_err);
        }

        let attempt_start = seg.cursor.load(Ordering::Acquire);
        let end_snapshot = seg.end.load(Ordering::Acquire);
        if attempt_start >= end_snapshot {
            return Ok(attempt_start.saturating_sub(seg.start));
        }

        let initial_resp = if attempt_start == 0 {
            pre_opened.take()
        } else {
            pre_opened = None;
            None
        };

        match run_segment_attempt(
            tctx,
            seg,
            attempt_start,
            end_snapshot,
            allow_full_200,
            initial_resp,
            shed_count < MAX_SHEDS_PER_SEGMENT,
            writer,
            downloaded,
            segment_pool,
            speed_monitor,
            control_rx,
        )
        .await
        {
            Ok(()) => {
                let final_cur = seg.cursor.load(Ordering::Acquire);
                return Ok(final_cur.saturating_sub(seg.start));
            }
            Err(AttemptAbort::ShedSlow) => {
                shed_count += 1;
                retries_performed.fetch_add(1, Ordering::Relaxed);
                tracing::debug!(
                    segment_id = seg.id,
                    shed_count,
                    cursor = seg.cursor.load(Ordering::Acquire),
                    "shedding slow connection and reconnecting"
                );
                continue;
            }
            Err(AttemptAbort::Engine(err)) => {
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
                let delay = match &err {
                    EngineError::Throttled { retry_after, .. } => *retry_after,
                    _ => backoff_with_jitter(retries.saturating_sub(1)),
                };
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
    tctx: TransferContext<'_>,
    seg: &ActiveSegment,
    attempt_start: u64,
    end_snapshot: u64,
    allow_full_200: bool,
    initial_resp: Option<reqwest::Response>,
    allow_shed: bool,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    segment_pool: &Mutex<SegmentPool>,
    speed_monitor: &SpeedMonitor,
    control_rx: &mut watch::Receiver<ControlSignal>,
) -> Result<(), AttemptAbort> {
    let resp = match initial_resp {
        Some(resp) => resp,
        None => {
            let range_header = format!("bytes={attempt_start}-{}", end_snapshot - 1);
            tokio::select! {
                biased;
                _ = control_rx.changed() => {
                    if let Some(sig_err) = check_control_signal(control_rx) {
                        return Err(sig_err.into());
                    }
                    unreachable!("Run signal does not trigger abort");
                }
                res = apply_custom_headers(tctx.client.get(tctx.url).header(RANGE, range_header), tctx.headers).send() => res?,
            }
        }
    };
    let status = resp.status();

    if !(status == StatusCode::PARTIAL_CONTENT
        || (allow_full_200 && attempt_start == 0 && status == StatusCode::OK))
    {
        if status == StatusCode::OK && is_html_content_type(resp.headers()) {
            return Err(build_html_error(resp).await.into());
        }
        return Err(status_to_error(status, resp.headers(), resp.url().to_string()).into());
    }

    let mut cur = attempt_start;
    let mut write_offset = attempt_start;
    let mut buffer: Option<PooledBuffer> = None;
    let mut stream = resp.bytes_stream();
    let mut stream_err: Option<AttemptAbort> = None;

    let attempt_started = Instant::now();
    let mut disk_stall = Duration::ZERO;
    let mut attempt_bytes = 0u64;
    let can_shed = allow_shed && tctx.limiter.is_none_or(|l| l.is_unlimited());
    let mut shed_ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + SHED_CHECK_INTERVAL,
        SHED_CHECK_INTERVAL,
    );
    shed_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        let next_item = tokio::select! {
            biased;
            _ = control_rx.changed() => {
                match *control_rx.borrow() {
                    ControlSignal::Run => continue,
                    ControlSignal::Pause => {
                        stream_err = Some(EngineError::Paused.into());
                        break;
                    }
                    ControlSignal::Cancel => return Err(EngineError::Cancelled.into()),
                }
            }
            _ = shed_ticker.tick(), if can_shed => {
                if should_shed_worker(
                    attempt_started,
                    disk_stall,
                    attempt_bytes,
                    seg.remaining(),
                    segment_pool,
                    speed_monitor,
                ) {
                    stream_err = Some(AttemptAbort::ShedSlow);
                    break;
                }
                continue;
            }
            item = stream.next() => item,
        };

        let Some(item) = next_item else {
            break;
        };

        let chunk = match item {
            Ok(chunk) => chunk,
            Err(err) => {
                stream_err = Some(EngineError::Http(err).into());
                break;
            }
        };

        let limit = seg.end.load(Ordering::Acquire);
        let remaining = limit.saturating_sub(cur) as usize;
        if remaining == 0 {
            break;
        }

        let slice = if chunk.len() > remaining {
            &chunk[..remaining]
        } else {
            &chunk[..]
        };
        if slice.is_empty() {
            continue;
        }

        let len = slice.len() as u64;
        if let Some(lim) = tctx.limiter {
            lim.acquire(len).await;
        }
        cur += len;
        attempt_bytes += len;
        seg.cursor.store(cur, Ordering::Release);
        downloaded.fetch_add(len, Ordering::Relaxed);
        speed_monitor.record_bytes(len);

        let net_elapsed = attempt_started.elapsed().saturating_sub(disk_stall);
        if net_elapsed >= SHED_BEST_AFTER_TIME && attempt_bytes >= SHED_BEST_AFTER_BYTES {
            let bps = ((attempt_bytes as f64) / net_elapsed.as_secs_f64()).round() as u64;
            speed_monitor.update_best_worker(bps);
        }

        let write_start = Instant::now();
        append_and_flush_chunks(writer, seg.id, &mut write_offset, &mut buffer, slice).await?;
        disk_stall += write_start.elapsed();

        if cur >= seg.end.load(Ordering::Acquire) {
            break;
        }
    }

    flush_tail_chunk(writer, seg.id, write_offset, &mut buffer).await?;

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
        ))
        .into());
    }

    Ok(())
}

fn should_shed_worker(
    attempt_started: Instant,
    disk_stall: Duration,
    attempt_bytes: u64,
    remaining_bytes: u64,
    segment_pool: &Mutex<SegmentPool>,
    speed_monitor: &SpeedMonitor,
) -> bool {
    let net_elapsed = attempt_started.elapsed().saturating_sub(disk_stall);
    if net_elapsed < SHED_GRACE || remaining_bytes == 0 {
        return false;
    }

    let active = segment_pool
        .lock()
        .expect("segment_pool poisoned")
        .active_workers;
    let Some(shed_bar) = speed_monitor.shed_bar(active) else {
        return false;
    };

    let worker_bps = ((attempt_bytes as f64) / net_elapsed.as_secs_f64()).round() as u64;
    if worker_bps >= shed_bar / SHED_RATIO {
        return false;
    }

    let est_remaining_secs = remaining_bytes as f64 / worker_bps.max(1) as f64;
    est_remaining_secs > SHED_LEFT_SECS
}

fn is_transient_error(err: &EngineError) -> bool {
    match err {
        EngineError::Http(_) | EngineError::Throttled { .. } => true,
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
        | EngineError::TaskJoin(_)
        | EngineError::InsufficientDiskSpace { .. }
        | EngineError::FileTooLargeForFilesystem { .. }
        | EngineError::UnexpectedHtmlResponse { .. } => false,
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

async fn append_and_flush_chunks(
    writer: &PartFileWriter,
    segment_index: usize,
    write_offset: &mut u64,
    buffer: &mut Option<PooledBuffer>,
    data: &[u8],
) -> Result<(), EngineError> {
    let mut offset_in_slice = 0;
    while offset_in_slice < data.len() {
        let active = buffer.get_or_insert_with(|| writer.acquire_buffer());
        let space = CHUNK_BUFFER_SIZE - active.len();
        let take = (data.len() - offset_in_slice).min(space);
        active.extend_from_slice(&data[offset_in_slice..offset_in_slice + take]);
        offset_in_slice += take;

        if active.len() == CHUNK_BUFFER_SIZE {
            let full = buffer.take().expect("buffer just populated");
            let block_len = full.len() as u64;
            writer
                .write_segment_pooled(segment_index, *write_offset, full)
                .await?;
            *write_offset += block_len;
        }
    }
    Ok(())
}

async fn flush_tail_chunk(
    writer: &PartFileWriter,
    segment_index: usize,
    write_offset: u64,
    buffer: &mut Option<PooledBuffer>,
) -> Result<(), EngineError> {
    if let Some(tail) = buffer.take()
        && !tail.is_empty()
    {
        writer
            .write_segment_pooled(segment_index, write_offset, tail)
            .await?;
    }
    Ok(())
}

async fn stream_unbounded_to_writer(
    resp: reqwest::Response,
    limiter: Option<&BandwidthLimiter>,
    writer: &PartFileWriter,
    downloaded: &AtomicU64,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<u64, EngineError> {
    let mut written = 0u64;
    let mut write_offset = 0u64;
    let mut buffer: Option<PooledBuffer> = None;
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
        if chunk.is_empty() {
            continue;
        }
        let len = chunk.len() as u64;
        if let Some(lim) = limiter {
            lim.acquire(len).await;
        }
        written += len;
        downloaded.fetch_add(len, Ordering::Relaxed);
        append_and_flush_chunks(writer, 0, &mut write_offset, &mut buffer, &chunk).await?;
    }

    flush_tail_chunk(writer, 0, write_offset, &mut buffer).await?;

    if let Some(err) = interrupted {
        return Err(err);
    }

    Ok(written)
}

fn is_html_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").trim())
        .is_some_and(|mime| {
            mime.eq_ignore_ascii_case("text/html")
                || mime.eq_ignore_ascii_case("application/xhtml+xml")
        })
}

async fn build_html_error(resp: reqwest::Response) -> EngineError {
    let url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_string();

    let mut body_bytes = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(Ok(chunk)) = stream.next().await {
        let remaining = 4096usize.saturating_sub(body_bytes.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        body_bytes.extend_from_slice(&chunk[..take]);
        if body_bytes.len() >= 4096 {
            break;
        }
    }

    let text = String::from_utf8_lossy(&body_bytes);
    let title = extract_html_title(&text);
    let detail = match title {
        Some(t) if !t.is_empty() => format!("Content-Type {content_type}; title: \"{t}\""),
        _ => format!("Content-Type {content_type}"),
    };

    EngineError::UnexpectedHtmlResponse { url, detail }
}

fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start_tag = lower.find("<title")?;
    let after_open = &lower[start_tag..];
    let gt = after_open.find('>')?;
    let content_start = start_tag + gt + 1;
    let end_rel = lower[content_start..].find("</title>")?;
    let raw = &html[content_start..content_start + end_rel];
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed.chars().take(120).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn append_and_flush_chunks_caps_buffer_capacity_and_writes_exact_bytes() {
        let dir = TempDir::new().unwrap();
        let part_path = dir.path().join("chunk_cap.bin.part");
        let final_path = dir.path().join("chunk_cap.bin");

        // Construct odd-sized slices including empty slices and an oversized (> CHUNK_BUFFER_SIZE) slice.
        let total = CHUNK_BUFFER_SIZE * 3 + 77_777;
        let payload: Vec<u8> = (0..total).map(|i| ((i * 17 + 3) % 251) as u8).collect();
        let expected_hash = blake3::hash(&payload);

        let writer = PartFileWriter::spawn(part_path, Some(payload.len() as u64)).unwrap();
        let mut buffer: Option<PooledBuffer> = None;
        let mut write_offset = 0u64;

        let slice_sizes = [
            0usize,
            1,
            19_999,
            CHUNK_BUFFER_SIZE - 10_000,
            0,
            CHUNK_BUFFER_SIZE + 123_456, // oversized single HTTP frame
            333_333,
        ];
        let mut pos = 0usize;
        let mut idx = 0usize;
        while pos < payload.len() {
            let step = slice_sizes[idx % slice_sizes.len()].min(payload.len() - pos);
            idx += 1;
            let slice = &payload[pos..pos + step];
            pos += step;

            append_and_flush_chunks(&writer, 0, &mut write_offset, &mut buffer, slice)
                .await
                .unwrap();

            if let Some(active) = &buffer {
                assert!(
                    active.capacity() <= CHUNK_BUFFER_SIZE,
                    "pooled buffer capacity {} exceeded CHUNK_BUFFER_SIZE {}",
                    active.capacity(),
                    CHUNK_BUFFER_SIZE
                );
                assert!(active.len() < CHUNK_BUFFER_SIZE);
            }
        }

        flush_tail_chunk(&writer, 0, write_offset, &mut buffer)
            .await
            .unwrap();
        assert!(buffer.is_none());

        writer.finish(&final_path).await.unwrap();
        let actual = std::fs::read(&final_path).unwrap();
        assert_eq!(blake3::hash(&actual), expected_hash);
    }
}
