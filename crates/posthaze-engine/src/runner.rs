//! Unified `Download` execution pipeline for fresh and resumed `Download`s.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, broadcast, mpsc, watch};

use crate::model::{
    DownloadId, DownloadOutcome, DownloadRequest, DownloadState, EngineError, EngineEvent,
    FailureKind, PlannedSegment, ProgressSnapshot, SegmentCheckpoint, SegmentProgress,
};
use crate::probe::{ProbeResult, probe_url_with_headers};
use crate::queue::QueueCoordinator;
use crate::store::{DownloadRecord, DownloadStore};
use crate::throttle::BandwidthLimiter;
use crate::transfer::{
    ControlSignal, LiveSegments, MultiSegmentResult, execute_multi_segment,
    execute_multi_segment_from_checkpoints, execute_single_stream, plan_segments,
};
use crate::writer::PartFileWriter;
use crate::{DownloadHandle, disambiguate_output_path, make_part_path, remove_file_if_exists};

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(50);

/// Control handle stored in `DownloadEngine::active` for a queued or active `Download`.
#[derive(Debug, Clone)]
pub(crate) struct ActiveControl {
    pub control_tx: watch::Sender<ControlSignal>,
    pub done_rx: watch::Receiver<bool>,
}

/// Execution mode for a queued `Download`.
pub(crate) enum TransferMode {
    Fresh(DownloadRequest),
    Resume {
        record: DownloadRecord,
        segments: Vec<SegmentCheckpoint>,
    },
}

/// Dependencies required to admit and run a `Download` through the `Queue`.
pub(crate) struct RunnerContext {
    pub client: reqwest::Client,
    pub limiter: Arc<BandwidthLimiter>,
    pub store: Option<DownloadStore>,
    pub active: Arc<Mutex<HashMap<DownloadId, ActiveControl>>>,
    pub queue: Arc<QueueCoordinator>,
    pub event_tx: broadcast::Sender<EngineEvent>,
}

/// Transfer strategy selected from server `ProbeResult` capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferStrategy {
    MultiSegment(u64),
    SingleStream,
}

/// Executes a single `Download` directly without `Queue` admission (used by `DownloadEngine::download`).
pub(crate) async fn run_direct_download(
    client: reqwest::Client,
    limiter: Arc<BandwidthLimiter>,
    store: Option<DownloadStore>,
    event_tx: broadcast::Sender<EngineEvent>,
    id: DownloadId,
    request: DownloadRequest,
    progress_tx: Option<watch::Sender<ProgressSnapshot>>,
) -> Result<DownloadOutcome, EngineError> {
    let (_control_tx, control_rx) = watch::channel(ControlSignal::Run);
    let sink = ProgressSink::new(id.clone(), progress_tx, event_tx);
    execute_download(
        client,
        limiter,
        store,
        id,
        TransferMode::Fresh(request),
        sink,
        control_rx,
    )
    .await
}

/// Admits a fresh or resumed `Download` into the `Queue`, wires channels/events, and spawns execution.
pub(crate) async fn spawn_queued_download(
    ctx: RunnerContext,
    id: DownloadId,
    mode: TransferMode,
) -> DownloadHandle {
    let (queued_bytes, queued_total, queued_part_path) = match &mode {
        TransferMode::Fresh(_) => (0, None, None),
        TransferMode::Resume { record, .. } => (
            record.bytes_downloaded,
            record.content_length,
            record.part_path.clone(),
        ),
    };

    let (progress_tx, progress_rx) = watch::channel(ProgressSnapshot::status(
        DownloadState::Queued,
        queued_bytes,
        queued_total,
        0,
    ));
    let (control_tx, control_rx) = watch::channel(ControlSignal::Run);
    let (done_tx, done_rx) = watch::channel(false);

    {
        let mut active = ctx.active.lock().await;
        active.insert(
            id.clone(),
            ActiveControl {
                control_tx: control_tx.clone(),
                done_rx,
            },
        );
    }

    let task_id = id.clone();
    let sink = ProgressSink::new(id.clone(), Some(progress_tx), ctx.event_tx.clone());

    ctx.queue.enqueue(id.clone());
    let _ = ctx.event_tx.send(EngineEvent::StateChanged {
        id: id.clone(),
        state: DownloadState::Queued,
        error: None,
        failure_kind: None,
    });
    let _ = ctx.event_tx.send(EngineEvent::QueueUpdated);

    let join_handle = tokio::spawn(async move {
        let mut control_rx = control_rx;
        let res = match ctx.queue.wait_for_slot(&task_id, &mut control_rx).await {
            Ok(_slot) => {
                let res = execute_download(
                    ctx.client,
                    ctx.limiter,
                    ctx.store,
                    task_id.clone(),
                    mode,
                    sink,
                    control_rx,
                )
                .await;
                ctx.active.lock().await.remove(&task_id);
                res
            }
            Err(err) => {
                ctx.active.lock().await.remove(&task_id);
                finalize_queued_interruption(
                    ctx.store.as_ref(),
                    &task_id,
                    queued_part_path.as_deref(),
                    queued_bytes,
                    queued_total,
                    &sink,
                    err,
                )
                .await
            }
        };
        let _ = ctx.event_tx.send(EngineEvent::QueueUpdated);
        let _ = done_tx.send(true);
        res
    });

    DownloadHandle {
        id,
        control_tx,
        progress_rx,
        join_handle,
    }
}

async fn execute_download(
    client: reqwest::Client,
    limiter: Arc<BandwidthLimiter>,
    store: Option<DownloadStore>,
    id: DownloadId,
    mode: TransferMode,
    sink: ProgressSink,
    mut control_rx: watch::Receiver<ControlSignal>,
) -> Result<DownloadOutcome, EngineError> {
    let total_start = Instant::now();
    let probe_start = Instant::now();

    let (probe_url, probe_headers, prior_progress, resume_prev_state) = match &mode {
        TransferMode::Fresh(request) => (request.url.as_str(), request.headers.as_slice(), (0, None), None),
        TransferMode::Resume { record, .. } => (
            record.url.as_str(),
            record.headers.as_slice(),
            (record.bytes_downloaded, record.content_length),
            Some(record.state),
        ),
    };

    let probe = probe_with_control(
        &client,
        &store,
        &sink,
        &mut control_rx,
        &id,
        probe_url,
        probe_headers,
        prior_progress,
        resume_prev_state,
    )
    .await?;
    let probe_duration = probe_start.elapsed();

    if let TransferMode::Resume { record, .. } = &mode
        && let Err(err) = validate_resume_probe(&id, record, &probe)
    {
        return Err(fail_download(
            &store,
            &sink,
            &id,
            record.bytes_downloaded,
            record.content_length,
            err,
        )
        .await);
    }

    let (final_path, part_path, max_workers, sync_on_complete, headers, initial_cp_map, initial_bytes, resume_segments) =
        match mode {
            TransferMode::Fresh(request) => {
                let final_path = request.output_path.unwrap_or_else(|| {
                    disambiguate_output_path(&request.output_dir, &probe.suggested_filename)
                });
                let part_path = make_part_path(&final_path);
                (
                    final_path,
                    part_path,
                    request.max_workers,
                    request.sync_on_complete,
                    request.headers,
                    HashMap::new(),
                    0,
                    None,
                )
            }
            TransferMode::Resume { record, segments } => {
                let final_path = record.output_path.ok_or_else(|| {
                    EngineError::InvalidState("missing output_path for resumed download".to_string())
                })?;
                let part_path = record.part_path.ok_or_else(|| {
                    EngineError::InvalidState("missing part_path for resumed download".to_string())
                })?;
                let initial_cp_map: HashMap<usize, u64> = segments
                    .iter()
                    .map(|s| (s.segment_index, s.cursor))
                    .collect();
                let initial_bytes = compute_initial_bytes(&segments);
                (
                    final_path,
                    part_path,
                    record.max_workers,
                    record.sync_on_complete,
                    record.headers,
                    initial_cp_map,
                    initial_bytes,
                    Some(segments),
                )
            }
        };

    let checkpointed_cursors = Arc::new(std::sync::Mutex::new(initial_cp_map));
    let live_segments_slot: Arc<std::sync::Mutex<Option<LiveSegments>>> =
        Arc::new(std::sync::Mutex::new(None));

    let transfer_start = Instant::now();
    let (cp_tx, cp_handle) =
        spawn_checkpoint_task(&store, &id, Arc::clone(&checkpointed_cursors));

    let writer_res = if resume_segments.is_some() {
        PartFileWriter::spawn_existing_with_sync(part_path.clone(), sync_on_complete, Some(cp_tx))
    } else {
        PartFileWriter::spawn_with_checkpoint(
            part_path.clone(),
            probe.content_length,
            sync_on_complete,
            Some(cp_tx),
        )
    };
    let writer = match writer_res {
        Ok(writer) => Arc::new(writer),
        Err(err) => {
            cp_handle.abort();
            return Err(
                fail_download(&store, &sink, &id, initial_bytes, probe.content_length, err).await,
            );
        }
    };

    if let Some(store) = &store {
        let activate_res = if resume_segments.is_some() {
            store.mark_resumed_active(&id, &probe.final_url).await
        } else {
            let planned_segments = plan_initial_segments(&probe, max_workers);
            store
                .record_probed_and_activate(&id, &probe, &final_path, &part_path, &planned_segments)
                .await
        };
        if let Err(err) = activate_res {
            cp_handle.abort();
            return Err(
                fail_download(&None, &sink, &id, initial_bytes, probe.content_length, err).await,
            );
        }
    }

    let downloaded = Arc::new(AtomicU64::new(initial_bytes));
    let active_workers = Arc::new(AtomicUsize::new(0));

    sink.emit_state(DownloadState::Active, None, None);
    sink.send(ProgressSnapshot::status(
        DownloadState::Active,
        initial_bytes,
        probe.content_length,
        0,
    ));

    let reporter = spawn_progress_reporter(
        sink.clone(),
        Arc::clone(&downloaded),
        Arc::clone(&active_workers),
        probe.content_length,
        Arc::clone(&live_segments_slot),
        Arc::clone(&checkpointed_cursors),
        initial_bytes,
    );

    let persist = store.as_ref().map(|s| (id.clone(), s.clone()));
    let transfer_result = if let Some(segments) = resume_segments {
        execute_multi_segment_from_checkpoints(
            &client,
            &probe.final_url,
            &headers,
            Some(&limiter),
            segments,
            max_workers,
            &writer,
            &downloaded,
            &active_workers,
            control_rx,
            persist,
            Some(&live_segments_slot),
        )
        .await
    } else {
        match select_transfer_strategy(&probe) {
            TransferStrategy::MultiSegment(total_bytes) => {
                execute_multi_segment(
                    &client,
                    &probe.final_url,
                    &headers,
                    Some(&limiter),
                    total_bytes,
                    max_workers,
                    &writer,
                    &downloaded,
                    &active_workers,
                    control_rx,
                    persist,
                    Some(&live_segments_slot),
                )
                .await
            }
            TransferStrategy::SingleStream => execute_single_stream(
                &client,
                &probe.final_url,
                &headers,
                Some(limiter.as_ref()),
                &writer,
                &downloaded,
                &active_workers,
                control_rx,
            )
            .await
            .map(|bytes| MultiSegmentResult {
                bytes_written: bytes,
                workers_used: 1,
                splits_performed: 0,
                retries_performed: 0,
            }),
        }
    };

    reporter.abort();

    let out = finalize_transfer(
        &store,
        &sink,
        &id,
        &probe,
        final_path,
        &part_path,
        writer,
        &downloaded,
        transfer_result,
        probe_duration,
        transfer_start,
        total_start,
    )
    .await;
    let _ = cp_handle.await;
    out
}

/// Resolves whether a persisted `Paused` or `Failed` `Download` resumes from `Checkpoint`s or starts fresh.
pub(crate) fn resolve_resume_mode(
    record: DownloadRecord,
    segments: Vec<SegmentCheckpoint>,
    fallback_output_dir: PathBuf,
) -> Result<TransferMode, EngineError> {
    let not_started_yet = record.part_path.is_none() && segments.is_empty();
    if not_started_yet {
        let output_dir = record.output_dir.unwrap_or(fallback_output_dir);
        return Ok(TransferMode::Fresh(DownloadRequest {
            url: record.url,
            output_path: record.output_path,
            output_dir,
            max_workers: record.max_workers,
            sync_on_complete: record.sync_on_complete,
            headers: record.headers,
        }));
    }

    if !record.supports_ranges {
        return Err(EngineError::InvalidState(
            "cannot resume download without server byte-range support".to_string(),
        ));
    }

    let Some(part_path) = &record.part_path else {
        return Err(EngineError::InvalidState(
            "cannot resume download without a recorded .part file".to_string(),
        ));
    };
    if !part_path.exists() {
        return Err(EngineError::InvalidState(format!(
            "missing .part file for resume: {}",
            part_path.display()
        )));
    }
    if record.output_path.is_none() {
        return Err(EngineError::InvalidState(
            "missing output_path for resumed download".to_string(),
        ));
    }
    if segments.is_empty() {
        return Err(EngineError::InvalidState(
            "cannot resume download with no persisted segments".to_string(),
        ));
    }

    Ok(TransferMode::Resume { record, segments })
}

pub(crate) fn select_transfer_strategy(probe: &ProbeResult) -> TransferStrategy {
    match (probe.supports_ranges, probe.content_length) {
        (true, Some(total_bytes)) => TransferStrategy::MultiSegment(total_bytes),
        _ => TransferStrategy::SingleStream,
    }
}

pub(crate) fn plan_initial_segments(probe: &ProbeResult, max_workers: usize) -> Vec<PlannedSegment> {
    match (probe.supports_ranges, probe.content_length) {
        (true, Some(total)) if total > 0 => plan_segments(total, max_workers)
            .into_iter()
            .map(|s| PlannedSegment {
                start: s.start,
                end: Some(s.end),
            })
            .collect(),
        _ => vec![PlannedSegment {
            start: 0,
            end: probe.content_length,
        }],
    }
}

pub(crate) fn compute_initial_bytes(segments: &[SegmentCheckpoint]) -> u64 {
    segments
        .iter()
        .map(|s| s.cursor.saturating_sub(s.start))
        .sum()
}

pub(crate) fn validate_resume_probe(
    id: &DownloadId,
    record: &DownloadRecord,
    probe: &ProbeResult,
) -> Result<(), EngineError> {
    if record.content_length != probe.content_length {
        return Err(EngineError::ResourceChanged(format!(
            "Content-Length changed from {:?} to {:?}",
            record.content_length, probe.content_length
        )));
    }

    if let Some(prev_etag) = &record.etag
        && probe.etag.as_deref() != Some(prev_etag.as_str())
    {
        return Err(EngineError::ResourceChanged(format!(
            "ETag changed from {prev_etag} to {:?}",
            probe.etag
        )));
    }

    if let Some(prev_lm) = &record.last_modified
        && probe.last_modified.as_deref() != Some(prev_lm.as_str())
    {
        return Err(EngineError::ResourceChanged(format!(
            "Last-Modified changed from {prev_lm} to {:?}",
            probe.last_modified
        )));
    }

    if record.etag.is_none()
        && record.last_modified.is_none()
        && probe.etag.is_none()
        && probe.last_modified.is_none()
    {
        tracing::warn!(
            download_id = %id,
            "resuming download without ETag or Last-Modified validator; relying on Content-Length match"
        );
    }

    if !probe.supports_ranges {
        return Err(EngineError::InvalidState(
            "server no longer supports byte ranges for resume".to_string(),
        ));
    }

    Ok(())
}

#[derive(Clone)]
struct ProgressSink {
    id: DownloadId,
    watch_tx: Option<watch::Sender<ProgressSnapshot>>,
    event_tx: broadcast::Sender<EngineEvent>,
}

impl ProgressSink {
    fn new(
        id: DownloadId,
        watch_tx: Option<watch::Sender<ProgressSnapshot>>,
        event_tx: broadcast::Sender<EngineEvent>,
    ) -> Self {
        Self {
            id,
            watch_tx,
            event_tx,
        }
    }

    fn send(&self, snapshot: ProgressSnapshot) {
        if let Some(tx) = &self.watch_tx {
            let _ = tx.send(snapshot.clone());
        }
        let _ = self.event_tx.send(EngineEvent::ProgressUpdated {
            id: self.id.clone(),
            snapshot,
        });
    }

    fn emit_state(
        &self,
        state: DownloadState,
        error: Option<String>,
        failure_kind: Option<FailureKind>,
    ) {
        let _ = self.event_tx.send(EngineEvent::StateChanged {
            id: self.id.clone(),
            state,
            error,
            failure_kind,
        });
    }
}

async fn fail_download(
    store: &Option<DownloadStore>,
    sink: &ProgressSink,
    id: &DownloadId,
    bytes: u64,
    total: Option<u64>,
    err: EngineError,
) -> EngineError {
    let err_msg = err.to_string();
    let kind = err.failure_kind();
    if let Some(store) = store {
        let _ = store.mark_failed(id, bytes, &err_msg).await;
    }
    sink.emit_state(DownloadState::Failed, Some(err_msg), Some(kind));
    emit_failed(sink, bytes, total);
    err
}

async fn cancel_download(
    store: Option<&DownloadStore>,
    sink: &ProgressSink,
    id: &DownloadId,
    part_path: Option<&Path>,
    bytes: u64,
    total: Option<u64>,
) -> EngineError {
    if let Some(part) = part_path {
        let _ = remove_file_if_exists(part).await;
    }
    if let Some(store) = store {
        let _ = store.mark_cancelled(id).await;
    }
    sink.emit_state(DownloadState::Cancelled, None, None);
    sink.send(ProgressSnapshot::status(
        DownloadState::Cancelled,
        bytes,
        total,
        0,
    ));
    EngineError::Cancelled
}

#[allow(clippy::too_many_arguments)]
async fn probe_with_control(
    client: &reqwest::Client,
    store: &Option<DownloadStore>,
    sink: &ProgressSink,
    control_rx: &mut watch::Receiver<ControlSignal>,
    id: &DownloadId,
    url: &str,
    headers: &[(String, String)],
    (initial_bytes, initial_total): (u64, Option<u64>),
    resume_prev_state: Option<DownloadState>,
) -> Result<ProbeResult, EngineError> {
    if let Some(store) = store
        && let Err(err) = store.set_state(id, DownloadState::Probing).await
    {
        return Err(fail_download(&None, sink, id, initial_bytes, initial_total, err).await);
    }
    sink.emit_state(DownloadState::Probing, None, None);
    sink.send(ProgressSnapshot::status(
        DownloadState::Probing,
        initial_bytes,
        initial_total,
        0,
    ));

    if *control_rx.borrow() == ControlSignal::Cancel {
        return Err(
            cancel_download(store.as_ref(), sink, id, None, initial_bytes, initial_total).await,
        );
    }

    tokio::select! {
        biased;
        _ = control_rx.changed() => {
            let sig = *control_rx.borrow();
            match sig {
                ControlSignal::Cancel => {
                    Err(cancel_download(store.as_ref(), sink, id, None, initial_bytes, initial_total).await)
                }
                ControlSignal::Pause => {
                    if let Some(prev) = resume_prev_state {
                        if let Some(store) = store {
                            let _ = store.set_state(id, prev).await;
                        }
                        sink.emit_state(prev, None, None);
                        sink.send(ProgressSnapshot::status(prev, initial_bytes, initial_total, 0));
                        Err(EngineError::Paused)
                    } else {
                        Err(fail_download(
                            store,
                            sink,
                            id,
                            initial_bytes,
                            initial_total,
                            EngineError::InvalidState(
                                "cannot pause non-range download".to_string(),
                            ),
                        )
                        .await)
                    }
                }
                ControlSignal::Run => unreachable!("Run does not wake select as interruption"),
            }
        }
        res = probe_url_with_headers(client, url, headers) => match res {
            Ok(probe) => Ok(probe),
            Err(err) => Err(fail_download(store, sink, id, initial_bytes, initial_total, err).await),
        }
    }
}

fn spawn_checkpoint_task(
    store: &Option<DownloadStore>,
    id: &DownloadId,
    checkpointed_cursors: Arc<std::sync::Mutex<HashMap<usize, u64>>>,
) -> (crate::writer::CheckpointSender, tokio::task::JoinHandle<()>) {
    let store = store.clone();
    let (tx, mut rx) = mpsc::channel::<Vec<(usize, u64)>>(4);
    let task_id = id.clone();
    let handle = tokio::spawn(async move {
        while let Some(cursors) = rx.recv().await {
            {
                let mut map = checkpointed_cursors.lock().unwrap();
                for &(seg_idx, cursor) in &cursors {
                    map.entry(seg_idx)
                        .and_modify(|prev| *prev = (*prev).max(cursor))
                        .or_insert(cursor);
                }
            }
            if let Some(store) = &store
                && let Err(err) = store.apply_writer_checkpoint(&task_id, &cursors).await
            {
                tracing::warn!(download_id = %task_id, error = %err, "failed to persist writer checkpoint");
            }
        }
    });
    (tx, handle)
}

#[allow(clippy::too_many_arguments)]
async fn finalize_transfer(
    store: &Option<DownloadStore>,
    sink: &ProgressSink,
    id: &DownloadId,
    probe: &ProbeResult,
    final_path: PathBuf,
    part_path: &Path,
    writer: Arc<PartFileWriter>,
    downloaded: &AtomicU64,
    transfer_result: Result<MultiSegmentResult, EngineError>,
    probe_duration: Duration,
    transfer_start: Instant,
    total_start: Instant,
) -> Result<DownloadOutcome, EngineError> {
    let res = match transfer_result {
        Ok(res) => res,
        Err(failure_err) => {
            let bytes = downloaded.load(Ordering::Relaxed);
            match failure_err {
                EngineError::Paused => {
                    if !probe.supports_ranges {
                        if let Ok(w) = Arc::try_unwrap(writer) {
                            w.abort().await;
                        }
                        return Err(fail_download(
                            store,
                            sink,
                            id,
                            bytes,
                            probe.content_length,
                            EngineError::InvalidState(
                                "cannot pause non-range download".to_string(),
                            ),
                        )
                        .await);
                    }
                    let writer_cursors = match Arc::try_unwrap(writer) {
                        Ok(w) => match w.flush_and_close().await {
                            Ok(cursors) => cursors,
                            Err(flush_err) => {
                                return Err(fail_download(
                                    store,
                                    sink,
                                    id,
                                    bytes,
                                    probe.content_length,
                                    flush_err,
                                )
                                .await);
                            }
                        },
                        Err(_) => {
                            return Err(fail_download(
                                store,
                                sink,
                                id,
                                bytes,
                                probe.content_length,
                                EngineError::Writer(
                                    "writer handle still referenced on pause".to_string(),
                                ),
                            )
                            .await);
                        }
                    };
                    let persisted_bytes = if let Some(store) = store {
                        store
                            .finalize_writer_checkpoint(
                                id,
                                DownloadState::Paused,
                                None,
                                &writer_cursors,
                            )
                            .await
                            .map_err(|e| EngineError::TaskJoin(e.to_string()))?
                    } else {
                        bytes
                    };
                    sink.emit_state(DownloadState::Paused, None, None);
                    sink.send(ProgressSnapshot::status(
                        DownloadState::Paused,
                        persisted_bytes,
                        probe.content_length,
                        0,
                    ));
                    return Err(EngineError::Paused);
                }
                EngineError::Cancelled => {
                    if let Ok(w) = Arc::try_unwrap(writer) {
                        w.abort().await;
                    }
                    return Err(cancel_download(
                        store.as_ref(),
                        sink,
                        id,
                        Some(part_path),
                        bytes,
                        probe.content_length,
                    )
                    .await);
                }
                err => {
                    let mut acked_bytes = bytes;
                    if probe.supports_ranges
                        && let Ok(w) = Arc::try_unwrap(writer)
                        && let Ok(writer_cursors) = w.flush_and_close().await
                    {
                        if let Some(store) = store
                            && let Ok(persisted) = store
                                .finalize_writer_checkpoint(
                                    id,
                                    DownloadState::Failed,
                                    Some(&err.to_string()),
                                    &writer_cursors,
                                )
                                .await
                        {
                            acked_bytes = persisted;
                        }
                        let kind = err.failure_kind();
                        sink.emit_state(DownloadState::Failed, Some(err.to_string()), Some(kind));
                        emit_failed(sink, acked_bytes, probe.content_length);
                        return Err(err);
                    }
                    if let Some(store) = store {
                        let _ = store.mark_failed(id, bytes, &err.to_string()).await;
                    }
                    let kind = err.failure_kind();
                    sink.emit_state(DownloadState::Failed, Some(err.to_string()), Some(kind));
                    emit_failed(sink, bytes, probe.content_length);
                    return Err(err);
                }
            }
        }
    };

    let writer = match Arc::try_unwrap(writer) {
        Ok(writer) => writer,
        Err(_) => {
            return Err(fail_download(
                store,
                sink,
                id,
                res.bytes_written,
                probe.content_length,
                EngineError::Writer(
                    "writer handle still referenced after workers exited".to_string(),
                ),
            )
            .await);
        }
    };

    if let Err(err) = writer.finish(&final_path).await {
        return Err(fail_download(
            store,
            sink,
            id,
            res.bytes_written,
            probe.content_length,
            err,
        )
        .await);
    }

    if let Some(store) = store {
        store.mark_completed(id, res.bytes_written).await?;
    }

    let transfer_duration = transfer_start.elapsed();
    let total_duration = total_start.elapsed();

    sink.emit_state(DownloadState::Completed, None, None);
    sink.send(ProgressSnapshot::status(
        DownloadState::Completed,
        res.bytes_written,
        probe.content_length.or(Some(res.bytes_written)),
        0,
    ));

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
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
        final_url: probe.final_url.clone(),
    })
}

fn spawn_progress_reporter(
    sink: ProgressSink,
    downloaded: Arc<AtomicU64>,
    active_workers: Arc<AtomicUsize>,
    total_bytes: Option<u64>,
    live_segments_slot: Arc<std::sync::Mutex<Option<LiveSegments>>>,
    checkpointed_cursors: Arc<std::sync::Mutex<HashMap<usize, u64>>>,
    initial_bytes: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROGRESS_EMIT_INTERVAL);
        let mut last_sample_time = Instant::now();
        let mut last_sample_bytes = initial_bytes;
        let mut ema_bps: f64 = 0.0;

        loop {
            interval.tick().await;
            let bytes = downloaded.load(Ordering::Relaxed);
            let now = Instant::now();
            let elapsed = now.duration_since(last_sample_time);
            if elapsed >= Duration::from_millis(200) {
                let delta = bytes.saturating_sub(last_sample_bytes);
                let inst_bps = delta as f64 / elapsed.as_secs_f64();
                ema_bps = if ema_bps == 0.0 {
                    inst_bps
                } else {
                    0.35 * inst_bps + 0.65 * ema_bps
                };
                last_sample_time = now;
                last_sample_bytes = bytes;
            }
            let bytes_per_sec = ema_bps.round() as u64;
            let eta_secs = total_bytes.and_then(|total| {
                if bytes >= total {
                    Some(0)
                } else if bytes_per_sec > 0 {
                    Some((total - bytes).div_ceil(bytes_per_sec))
                } else {
                    None
                }
            });

            let segments = collect_segment_progress(
                &live_segments_slot,
                &checkpointed_cursors,
                bytes,
                total_bytes,
            );

            sink.send(ProgressSnapshot {
                state: DownloadState::Active,
                bytes_downloaded: bytes,
                total_bytes,
                active_workers: active_workers.load(Ordering::Relaxed),
                bytes_per_sec,
                eta_secs,
                segments,
            });
        }
    })
}

fn collect_segment_progress(
    live_segments_slot: &Arc<std::sync::Mutex<Option<LiveSegments>>>,
    checkpointed_cursors: &Arc<std::sync::Mutex<HashMap<usize, u64>>>,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
) -> Vec<SegmentProgress> {
    let live_arc = live_segments_slot.lock().unwrap().clone();
    let cp_map = checkpointed_cursors.lock().unwrap();

    if let Some(live_segments) = live_arc {
        let segs = live_segments.lock().unwrap();
        let mut out: Vec<SegmentProgress> =
            segs.iter().map(|seg| seg.snapshot_progress(&cp_map)).collect();
        out.sort_by_key(|s| s.id);
        out
    } else {
        let flushed = cp_map.get(&0).copied().unwrap_or(0);
        vec![SegmentProgress {
            id: 0,
            start: 0,
            cursor: bytes_downloaded,
            checkpointed_cursor: flushed.min(bytes_downloaded),
            end: total_bytes,
            completed: false,
        }]
    }
}

async fn finalize_queued_interruption(
    store: Option<&DownloadStore>,
    id: &DownloadId,
    part_path: Option<&Path>,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
    sink: &ProgressSink,
    err: EngineError,
) -> Result<DownloadOutcome, EngineError> {
    match err {
        EngineError::Paused => {
            if let Some(store) = store {
                let _ = store.set_state(id, DownloadState::Paused).await;
            }
            sink.emit_state(DownloadState::Paused, None, None);
            sink.send(ProgressSnapshot::status(
                DownloadState::Paused,
                bytes_downloaded,
                total_bytes,
                0,
            ));
            Err(EngineError::Paused)
        }
        EngineError::Cancelled => {
            Err(cancel_download(store, sink, id, part_path, bytes_downloaded, total_bytes).await)
        }
        other => Err(other),
    }
}

fn emit_failed(sink: &ProgressSink, bytes: u64, total: Option<u64>) {
    sink.send(ProgressSnapshot::status(
        DownloadState::Failed,
        bytes,
        total,
        0,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_probe(
        content_length: Option<u64>,
        supports_ranges: bool,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> ProbeResult {
        ProbeResult {
            final_url: "https://example.com/file.bin".to_string(),
            suggested_filename: "file.bin".to_string(),
            content_length,
            supports_ranges,
            etag: etag.map(str::to_string),
            last_modified: last_modified.map(str::to_string),
        }
    }

    fn sample_record(
        content_length: Option<u64>,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> DownloadRecord {
        DownloadRecord {
            id: DownloadId::from("dl-1".to_string()),
            url: "https://example.com/file.bin".to_string(),
            final_url: Some("https://example.com/file.bin".to_string()),
            output_dir: None,
            output_path: Some(PathBuf::from("file.bin")),
            part_path: Some(PathBuf::from("file.bin.part")),
            state: DownloadState::Paused,
            content_length,
            supports_ranges: true,
            etag: etag.map(str::to_string),
            last_modified: last_modified.map(str::to_string),
            max_workers: 4,
            sync_on_complete: false,
            headers: Vec::new(),
            bytes_downloaded: 100,
            error_message: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn validate_resume_probe_rejects_mismatched_validators_and_range_loss() {
        let id = DownloadId::from("dl-1".to_string());
        let record = sample_record(Some(1000), Some("\"v1\""), Some("Mon, 01 Jan 2026"));

        let err = validate_resume_probe(&id, &record, &sample_probe(Some(2000), true, Some("\"v1\""), Some("Mon, 01 Jan 2026")))
            .unwrap_err();
        assert!(matches!(err, EngineError::ResourceChanged(_)));

        let err = validate_resume_probe(&id, &record, &sample_probe(Some(1000), true, Some("\"v2\""), Some("Mon, 01 Jan 2026")))
            .unwrap_err();
        assert!(matches!(err, EngineError::ResourceChanged(_)));

        let err = validate_resume_probe(&id, &record, &sample_probe(Some(1000), true, Some("\"v1\""), Some("Tue, 02 Jan 2026")))
            .unwrap_err();
        assert!(matches!(err, EngineError::ResourceChanged(_)));

        let err = validate_resume_probe(&id, &record, &sample_probe(Some(1000), false, Some("\"v1\""), Some("Mon, 01 Jan 2026")))
            .unwrap_err();
        assert!(matches!(err, EngineError::InvalidState(_)));

        assert!(validate_resume_probe(&id, &record, &sample_probe(Some(1000), true, Some("\"v1\""), Some("Mon, 01 Jan 2026"))).is_ok());
    }

    #[test]
    fn select_transfer_strategy_and_plan_segments_cover_range_and_non_range() {
        assert_eq!(
            select_transfer_strategy(&sample_probe(Some(4096), true, None, None)),
            TransferStrategy::MultiSegment(4096)
        );
        assert_eq!(
            select_transfer_strategy(&sample_probe(Some(4096), false, None, None)),
            TransferStrategy::SingleStream
        );
        assert_eq!(
            select_transfer_strategy(&sample_probe(None, true, None, None)),
            TransferStrategy::SingleStream
        );

        let non_range_planned = plan_initial_segments(&sample_probe(Some(4096), false, None, None), 4);
        assert_eq!(
            non_range_planned,
            vec![PlannedSegment {
                start: 0,
                end: Some(4096)
            }]
        );

        let multi_planned = plan_initial_segments(&sample_probe(Some(8 * 1024 * 1024), true, None, None), 4);
        assert_eq!(multi_planned.len(), 4);
    }

    #[test]
    fn compute_initial_bytes_sums_segment_progress_safely() {
        let segments = vec![
            SegmentCheckpoint {
                segment_index: 0,
                start: 0,
                cursor: 500,
                end: Some(1000),
                completed: false,
            },
            SegmentCheckpoint {
                segment_index: 1,
                start: 1000,
                cursor: 1250,
                end: Some(2000),
                completed: false,
            },
        ];
        assert_eq!(compute_initial_bytes(&segments), 750);
    }

    #[test]
    fn resolve_resume_mode_chooses_fresh_when_not_started_and_validates_resume() {
        let mut not_started = sample_record(None, None, None);
        not_started.part_path = None;
        let mode = resolve_resume_mode(not_started, Vec::new(), PathBuf::from("downloads")).unwrap();
        assert!(matches!(mode, TransferMode::Fresh(req) if req.output_dir == PathBuf::from("downloads")));

        let mut no_ranges = sample_record(Some(1000), None, None);
        no_ranges.supports_ranges = false;
        assert!(matches!(
            resolve_resume_mode(no_ranges, Vec::new(), PathBuf::from("downloads")),
            Err(EngineError::InvalidState(_))
        ));
    }
}
