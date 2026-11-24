//! Core headless HTTP/HTTPS download engine for Posthaze Download Manager.

#![deny(missing_docs)]

mod disk;
mod model;
mod probe;
mod queue;
mod runner;
mod store;
mod throttle;
mod transfer;
/// Dedicated blocking `.part` file writer used by `DownloadEngine` and benchmarks.
pub mod writer;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, broadcast, watch};

use crate::probe::probe_url_with_headers;
use crate::queue::QueueCoordinator;
use crate::runner::{
    ActiveControl, RunnerContext, TransferMode, resolve_resume_mode, run_direct_download,
    spawn_queued_download,
};
use crate::store::DownloadStore;
use crate::throttle::BandwidthLimiter;
use crate::transfer::ControlSignal;

pub use crate::model::*;
pub use crate::store::DownloadRecord;

/// Control and observation handle for an active `Download`.
#[derive(Debug)]
pub struct DownloadHandle {
    id: DownloadId,
    control_tx: watch::Sender<ControlSignal>,
    progress_rx: watch::Receiver<ProgressSnapshot>,
    join_handle: tokio::task::JoinHandle<Result<DownloadOutcome, EngineError>>,
}

impl DownloadHandle {
    /// Returns the persistent [`DownloadId`] for this `Download`.
    pub fn id(&self) -> &DownloadId {
        &self.id
    }

    /// Signals this `Download` to pause gracefully, flushing buffered `Chunk`s and saving a `Checkpoint`.
    pub fn pause(&self) {
        let _ = self.control_tx.send(ControlSignal::Pause);
    }

    /// Signals this `Download` to cancel immediately and delete its `.part` file.
    pub fn cancel(&self) {
        let _ = self.control_tx.send(ControlSignal::Cancel);
    }

    /// Subscribes to live [`ProgressSnapshot`] updates for this `Download`.
    pub fn subscribe(&self) -> watch::Receiver<ProgressSnapshot> {
        self.progress_rx.clone()
    }

    /// Awaits completion of this `Download` and returns its [`DownloadOutcome`].
    ///
    /// This method is cancellation-safe until it resolves once, allowing callers to invoke
    /// [`DownloadHandle::pause`] or [`DownloadHandle::cancel`] if a `tokio::select!` branch wins.
    pub async fn wait(&mut self) -> Result<DownloadOutcome, EngineError> {
        (&mut self.join_handle)
            .await
            .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }
}

#[derive(Debug, Clone)]
struct HttpClientState {
    client: reqwest::Client,
    proxy_mode: ProxyMode,
    proxy_url: String,
    rebuilds: u64,
}

/// Headless HTTP/HTTPS download engine.
#[derive(Debug, Clone)]
pub struct DownloadEngine {
    client: Arc<std::sync::RwLock<HttpClientState>>,
    limiter: Arc<BandwidthLimiter>,
    store: Option<DownloadStore>,
    active: Arc<Mutex<HashMap<DownloadId, ActiveControl>>>,
    queue: Arc<QueueCoordinator>,
    ephemeral_settings: Arc<Mutex<Settings>>,
    event_tx: broadcast::Sender<EngineEvent>,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine {
    fn with_store(store: Option<DownloadStore>, settings: Settings) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (client, proxy_mode, proxy_url) =
            match build_http_client(settings.proxy_mode, &settings.proxy_url) {
                Ok(client) => (
                    client,
                    settings.proxy_mode,
                    settings.proxy_url.trim().to_string(),
                ),
                Err(_) => (
                    build_http_client(ProxyMode::System, "").unwrap(),
                    ProxyMode::System,
                    String::new(),
                ),
            };
        Self {
            client: Arc::new(std::sync::RwLock::new(HttpClientState {
                client,
                proxy_mode,
                proxy_url,
                rebuilds: 0,
            })),
            limiter: Arc::new(BandwidthLimiter::new(settings.download_speed_limit_kib)),
            store,
            active: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(QueueCoordinator::new(settings.max_concurrent_downloads)),
            ephemeral_settings: Arc::new(Mutex::new(settings)),
            event_tx,
        }
    }

    /// Returns a clone of the engine's currently active, proxy-configured HTTP client.
    pub fn http_client(&self) -> reqwest::Client {
        self.client
            .read()
            .expect("http client rwlock poisoned")
            .client
            .clone()
    }

    /// Returns how many times the shared HTTP client has been rebuilt due to proxy changes.
    #[doc(hidden)]
    pub fn http_client_rebuild_count(&self) -> u64 {
        self.client
            .read()
            .expect("http client rwlock poisoned")
            .rebuilds
    }

    /// Constructs a new ephemeral [`DownloadEngine`] without SQLite persistence.
    pub fn new() -> Self {
        Self::with_store(None, Settings::default())
    }

    /// Opens or creates a SQLite-backed [`DownloadEngine`] at `db_path`.
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let store = DownloadStore::open(db_path.as_ref()).await?;
        let settings = store.get_settings().await?;
        Ok(Self::with_store(Some(store), settings))
    }

    /// Constructs an ephemeral in-memory SQLite-backed [`DownloadEngine`].
    pub async fn in_memory() -> Result<Self, EngineError> {
        let store = DownloadStore::in_memory().await?;
        let settings = store.get_settings().await?;
        Ok(Self::with_store(Some(store), settings))
    }

    /// Subscribes to the engine-wide [`EngineEvent`] broadcast stream.
    ///
    /// Subscribers that fall behind the `1024`-event buffer (`RecvError::Lagged`) should
    /// reconcile their view via [`DownloadEngine::list_downloads`].
    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    /// Probes `url` without creating any database record or `.part` file.
    pub async fn probe(
        &self,
        url: impl Into<String>,
        output_dir: impl AsRef<Path>,
    ) -> Result<PreflightInfo, EngineError> {
        self.probe_with_headers(url, output_dir, &[]).await
    }

    /// Probes `url` with custom HTTP request `headers` without creating any database record or `.part` file.
    pub async fn probe_with_headers(
        &self,
        url: impl Into<String>,
        output_dir: impl AsRef<Path>,
        headers: &[(String, String)],
    ) -> Result<PreflightInfo, EngineError> {
        let original_url = url.into();
        let client = self.http_client();
        let probed = probe_url_with_headers(&client, &original_url, headers).await?;
        let reserved = if let Some(store) = &self.store {
            store.list_reserved_output_paths(None).await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let disambiguated_output_path = disambiguate_output_path_with_reserved(
            output_dir.as_ref(),
            &probed.suggested_filename,
            &reserved,
        );
        Ok(PreflightInfo {
            original_url,
            final_url: probed.final_url,
            suggested_filename: probed.suggested_filename,
            disambiguated_output_path,
            content_length: probed.content_length,
            supports_ranges: probed.supports_ranges,
            etag: probed.etag,
            last_modified: probed.last_modified,
            server_digest: probed.server_digest,
        })
    }

    /// Starts a `Download` asynchronously and returns a [`DownloadHandle`].
    pub async fn start(&self, mut request: DownloadRequest) -> Result<DownloadHandle, EngineError> {
        let id = DownloadId::new();
        if let Some(store) = &self.store {
            reserve_non_conflicting_output_path(store, &mut request).await;
            store.insert_queued(&id, &request).await?;
            let folder = request
                .output_path
                .as_deref()
                .and_then(|p| p.parent())
                .unwrap_or(&request.output_dir);
            let _ = store.record_folder_history(folder).await;
        }
        Ok(self.spawn_fresh_download(id, request).await)
    }

    /// Stages a new `Download` in [`DownloadState::Paused`] without starting network transfer.
    ///
    /// The staged record can be started later via [`Self::resume`].
    pub async fn enqueue_paused(
        &self,
        mut request: DownloadRequest,
    ) -> Result<DownloadId, EngineError> {
        let Some(store) = &self.store else {
            return Err(EngineError::InvalidState(
                "cannot stage paused download without persistence".to_string(),
            ));
        };
        let id = DownloadId::new();
        reserve_non_conflicting_output_path(store, &mut request).await;
        store.insert_paused(&id, &request).await?;
        let folder = request
            .output_path
            .as_deref()
            .and_then(|p| p.parent())
            .unwrap_or(&request.output_dir);
        let _ = store.record_folder_history(folder).await;
        let _ = self.event_tx.send(EngineEvent::StateChanged {
            id: id.clone(),
            state: DownloadState::Paused,
            error: None,
            failure_kind: None,
        });
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(id)
    }

    fn runner_context(&self, store: Option<DownloadStore>) -> RunnerContext {
        RunnerContext {
            client: self.http_client(),
            limiter: Arc::clone(&self.limiter),
            store,
            active: Arc::clone(&self.active),
            queue: Arc::clone(&self.queue),
            event_tx: self.event_tx.clone(),
        }
    }

    async fn spawn_fresh_download(&self, id: DownloadId, request: DownloadRequest) -> DownloadHandle {
        spawn_queued_download(
            self.runner_context(self.store.clone()),
            id,
            TransferMode::Fresh(request),
        )
        .await
    }

    /// Resumes a persisted `Paused` or resumable `Failed` `Download` by [`DownloadId`].
    pub async fn resume(&self, id: &DownloadId) -> Result<DownloadHandle, EngineError> {
        {
            let active = self.active.lock().await;
            if active.contains_key(id) {
                return Err(EngineError::InvalidState(format!(
                    "download {id} is already active"
                )));
            }
        }

        let Some(store) = &self.store else {
            return Err(EngineError::NotFound(id.to_string()));
        };

        let record = store
            .get_download(id)
            .await
            .map_err(|e| EngineError::TaskJoin(e.to_string()))?
            .ok_or_else(|| EngineError::NotFound(id.to_string()))?;

        match record.state {
            DownloadState::Paused | DownloadState::Failed => {}
            other => {
                return Err(EngineError::InvalidState(format!(
                    "cannot resume download in {} state",
                    other.as_str()
                )));
            }
        }

        let segments = store.get_segments(id).await?;
        let fallback_dir = if record.output_dir.is_none() && record.part_path.is_none() && segments.is_empty() {
            store.get_settings().await?.default_output_dir
        } else {
            PathBuf::new()
        };
        let mode = resolve_resume_mode(record, segments, fallback_dir)?;

        store.set_state(id, DownloadState::Queued).await?;
        Ok(spawn_queued_download(self.runner_context(Some(store.clone())), id.clone(), mode).await)
    }

    async fn signal_and_wait_active(&self, id: &DownloadId, signal: ControlSignal) -> bool {
        let live = {
            let active = self.active.lock().await;
            active.get(id).cloned()
        };
        if let Some(ctrl) = live {
            let _ = ctrl.control_tx.send(signal);
            let mut done_rx = ctrl.done_rx;
            let _ = done_rx.wait_for(|&done| done).await;
            true
        } else {
            false
        }
    }

    async fn require_download_record(
        &self,
        id: &DownloadId,
    ) -> Result<(&DownloadStore, DownloadRecord), EngineError> {
        let Some(store) = &self.store else {
            return Err(EngineError::NotFound(id.to_string()));
        };
        let record = store
            .get_download(id)
            .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
            .ok_or_else(|| EngineError::NotFound(id.to_string()))?;
        Ok((store, record))
    }

    /// Cancels an active or persisted (`Paused`/`Failed`/`Queued`) `Download`, deleting its `.part` file.
    pub async fn cancel(&self, id: &DownloadId) -> Result<(), EngineError> {
        if self.signal_and_wait_active(id, ControlSignal::Cancel).await {
            return Ok(());
        }

        let (store, record) = self.require_download_record(id).await?;

        match record.state {
            DownloadState::Completed => {
                return Err(EngineError::InvalidState(
                    "cannot cancel a completed download".to_string(),
                ));
            }
            DownloadState::Cancelled => return Ok(()),
            DownloadState::Queued
            | DownloadState::Probing
            | DownloadState::Active
            | DownloadState::Paused
            | DownloadState::Failed => {}
        }

        if let Some(part_path) = record.part_path {
            remove_file_if_exists(&part_path).await?;
        }
        store.mark_cancelled(id).await?;
        let _ = self.event_tx.send(EngineEvent::StateChanged {
            id: id.clone(),
            state: DownloadState::Cancelled,
            error: None,
            failure_kind: None,
        });
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
    }

    /// Gracefully pauses an in-flight `Active` or `Queued` `Download` by [`DownloadId`].
    pub async fn pause(&self, id: &DownloadId) -> Result<(), EngineError> {
        if self.signal_and_wait_active(id, ControlSignal::Pause).await {
            if let Some(store) = &self.store
                && let Some(rec) = store.get_download(id).await?
                && rec.state != DownloadState::Paused
            {
                return Err(EngineError::InvalidState(format!(
                    "download finished in {} state before pause took effect",
                    rec.state.as_str()
                )));
            }
            return Ok(());
        }

        let (_, record) = self.require_download_record(id).await?;

        Err(EngineError::InvalidState(format!(
            "cannot pause download in {} state",
            record.state.as_str()
        )))
    }

    /// Deletes a `Download` record (stopping any active `Download`), its `.part` file, and optionally its final file permanently.
    pub async fn delete(&self, id: &DownloadId, delete_file: bool) -> Result<(), EngineError> {
        self.delete_with_mode(id, delete_file.then_some(DeleteFileMode::Permanent))
            .await
    }

    /// Deletes a `Download` record (stopping any active `Download`), its `.part` file, and optionally
    /// removes its completed output file using `file_mode` (`RecycleBin` or `Permanent`).
    pub async fn delete_with_mode(
        &self,
        id: &DownloadId,
        file_mode: Option<DeleteFileMode>,
    ) -> Result<(), EngineError> {
        let _ = self.signal_and_wait_active(id, ControlSignal::Cancel).await;
        let (store, record) = self.require_download_record(id).await?;

        if let Some(part_path) = &record.part_path {
            remove_file_if_exists(part_path).await?;
        }
        if let Some(mode) = file_mode
            && let Some(output_path) = record.output_path
        {
            match mode {
                DeleteFileMode::Permanent => {
                    remove_file_if_exists(&output_path).await?;
                }
                DeleteFileMode::RecycleBin => {
                    tokio::task::spawn_blocking(move || disk::move_to_recycle_bin(&output_path))
                        .await
                        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
                }
            }
        }
        store.delete_download(id).await?;
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
    }

    /// Clears stale `.part` / `Segment` state for a `Paused`, `Failed`, or `Cancelled` `Download` and restarts from byte `0`.
    pub async fn restart_fresh(&self, id: &DownloadId) -> Result<DownloadHandle, EngineError> {
        {
            let active = self.active.lock().await;
            if active.contains_key(id) {
                return Err(EngineError::InvalidState(format!(
                    "download {id} is already active"
                )));
            }
        }

        let (store, record) = self.require_download_record(id).await?;

        match record.state {
            DownloadState::Paused | DownloadState::Failed | DownloadState::Cancelled => {}
            other => {
                return Err(EngineError::InvalidState(format!(
                    "cannot restart download in {} state",
                    other.as_str()
                )));
            }
        }

        if let Some(part_path) = &record.part_path {
            remove_file_if_exists(part_path).await?;
        }
        store.reset_for_fresh_restart(id).await?;

        let fallback_dir = match &record.output_dir {
            Some(dir) => dir.clone(),
            None => store.get_settings().await?.default_output_dir,
        };
        let req = DownloadRequest {
            url: record.url,
            output_path: record.output_path,
            output_dir: fallback_dir,
            max_workers: record.max_workers,
            sync_on_complete: record.sync_on_complete,
            headers: record.headers,
            known_content_length: record.content_length,
            skip_probe: false,
            probe_via_get: false,
            preflight_probe: None,
        };

        Ok(self.spawn_fresh_download(id.clone(), req).await)
    }

    /// Lists all persisted `DownloadRecord`s ordered newest-first.
    pub async fn list_downloads(&self) -> Result<Vec<DownloadRecord>, EngineError> {
        match &self.store {
            Some(store) => store.list_downloads().await,
            None => Ok(Vec::new()),
        }
    }

    /// Fetches a single persisted `DownloadRecord` by [`DownloadId`].
    pub async fn get_download(&self, id: &DownloadId) -> Result<Option<DownloadRecord>, EngineError> {
        match &self.store {
            Some(store) => store.get_download(id).await,
            None => Ok(None),
        }
    }

    /// Finds the most recent non-cancelled persisted `DownloadRecord` matching `url`.
    pub async fn find_download_by_url(
        &self,
        url: &str,
    ) -> Result<Option<DownloadRecord>, EngineError> {
        match &self.store {
            Some(store) => store.find_download_by_url(url).await,
            None => Ok(None),
        }
    }

    /// Finds a duplicate `DownloadRecord` matching `url` or fallback `(filename, content_length)`
    /// (ignoring ` (N)` disambiguation counters for rotating-token URLs and auto-renamed files).
    pub async fn find_duplicate_download(
        &self,
        url: &str,
        filename: Option<&str>,
        content_length: Option<u64>,
        output_dir: Option<&Path>,
    ) -> Result<Option<DownloadRecord>, EngineError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        if let Some(rec) = store.find_download_by_url(url).await? {
            return Ok(Some(rec));
        }
        if let Some(name) = filename {
            return store
                .find_duplicate_by_file(name, content_length, output_dir)
                .await;
        }
        Ok(None)
    }

    /// Lists up to `limit` most recently used existing destination folders.
    pub async fn list_recent_folders(&self, limit: usize) -> Result<Vec<PathBuf>, EngineError> {
        match &self.store {
            Some(store) => store.list_recent_folders(limit).await,
            None => Ok(Vec::new()),
        }
    }

    /// Clears all persisted destination folder entries from history.
    pub async fn clear_recent_folders(&self) -> Result<(), EngineError> {
        match &self.store {
            Some(store) => store.clear_folder_history().await,
            None => Ok(()),
        }
    }

    /// Lists output paths currently claimed by active, queued, paused, or resumable-failed `Download`s.
    pub async fn list_reserved_output_paths(&self) -> Result<Vec<PathBuf>, EngineError> {
        match &self.store {
            Some(store) => store.list_reserved_output_paths(None).await,
            None => Ok(Vec::new()),
        }
    }

    /// Loads persisted `Segment` `Checkpoint`s for a `Download` as [`SegmentProgress`] entries.
    pub async fn list_segments(&self, id: &DownloadId) -> Result<Vec<SegmentProgress>, EngineError> {
        let (store, _) = self.require_download_record(id).await?;
        let checkpoints = store.get_segments(id).await?;
        Ok(checkpoints
            .into_iter()
            .map(|cp| SegmentProgress {
                id: cp.segment_index,
                start: cp.start,
                cursor: cp.cursor,
                checkpointed_cursor: cp.cursor,
                end: cp.end,
                completed: cp.completed,
            })
            .collect())
    }

    /// Returns persisted `Segment` counts keyed by [`DownloadId`] in a single query.
    pub async fn list_segment_counts(&self) -> Result<HashMap<DownloadId, usize>, EngineError> {
        match &self.store {
            Some(store) => store.list_segment_counts().await,
            None => Ok(HashMap::new()),
        }
    }

    /// Reads the persisted [`Settings`] for this engine.
    pub async fn get_settings(&self) -> Result<Settings, EngineError> {
        match &self.store {
            Some(store) => store.get_settings().await,
            None => Ok(self.ephemeral_settings.lock().await.clone()),
        }
    }

    /// Validates and persists updated [`Settings`] for this engine.
    pub async fn update_settings(&self, settings: Settings) -> Result<(), EngineError> {
        settings.validate()?;
        let normalized_proxy_url = settings.proxy_url.trim().to_string();
        let proxy_changed = {
            let state = self.client.read().expect("http client rwlock poisoned");
            state.proxy_mode != settings.proxy_mode || state.proxy_url != normalized_proxy_url
        };
        let rebuilt_client = if proxy_changed {
            Some(build_http_client(settings.proxy_mode, &normalized_proxy_url)?)
        } else {
            None
        };
        match &self.store {
            Some(store) => store.update_settings(&settings).await?,
            None => {
                *self.ephemeral_settings.lock().await = settings.clone();
            }
        }
        if let Some(new_client) = rebuilt_client {
            let mut state = self.client.write().expect("http client rwlock poisoned");
            state.client = new_client;
            state.proxy_mode = settings.proxy_mode;
            state.proxy_url = normalized_proxy_url;
            state.rebuilds = state.rebuilds.saturating_add(1);
        }
        self.limiter.set_limit_kib(settings.download_speed_limit_kib);
        self.queue.set_max_concurrent(settings.max_concurrent_downloads);
        let _ = self.event_tx.send(EngineEvent::SettingsUpdated(settings));
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(())
    }

    /// Computes the `BLAKE3` hex digest of a `Completed` `Download`'s output file.
    pub async fn compute_file_blake3(&self, id: &DownloadId) -> Result<String, EngineError> {
        Ok(self.compute_file_checksums(id).await?.blake3)
    }

    /// Computes `BLAKE3`, `SHA-256`, `SHA-512`, `SHA-1`, `MD5`, and `CRC32` hex digests of a
    /// `Completed` `Download`'s output file in a single sequential read pass.
    pub async fn compute_file_checksums(
        &self,
        id: &DownloadId,
    ) -> Result<FileChecksums, EngineError> {
        let (_, record) = self.require_download_record(id).await?;
        if record.state != DownloadState::Completed {
            return Err(EngineError::InvalidState(
                "file integrity is only available for completed downloads".to_string(),
            ));
        }
        let path = record.output_path.ok_or_else(|| {
            EngineError::NotFound("completed download has no output path".to_string())
        })?;
        tokio::task::spawn_blocking(move || -> Result<FileChecksums, EngineError> {
            use std::io::Read as _;
            use sha2::Digest as _;

            let mut opts = std::fs::OpenOptions::new();
            opts.read(true);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::fs::OpenOptionsExt as _;
                const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
                opts.custom_flags(FILE_FLAG_SEQUENTIAL_SCAN);
            }
            let mut file = opts.open(&path)?;
            let mut b3 = blake3::Hasher::new();
            let mut s256 = sha2::Sha256::new();
            let mut s512 = sha2::Sha512::new();
            let mut s1 = sha1::Sha1::new();
            let mut m5 = md5::Md5::new();
            let mut crc = crc32fast::Hasher::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let slice = &buf[..n];
                b3.update(slice);
                s256.update(slice);
                s512.update(slice);
                s1.update(slice);
                m5.update(slice);
                crc.update(slice);
            }
            Ok(FileChecksums {
                blake3: b3.finalize().to_hex().to_string(),
                sha256: format!("{:x}", s256.finalize()),
                sha512: format!("{:x}", s512.finalize()),
                sha1: format!("{:x}", s1.finalize()),
                md5: format!("{:x}", m5.finalize()),
                crc32: format!("{:08x}", crc.finalize()),
            })
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    async fn prepare_completed_file_op(
        &self,
        id: &DownloadId,
    ) -> Result<(DownloadStore, PathBuf), EngineError> {
        let (store, record) = self.require_download_record(id).await?;
        if record.state != DownloadState::Completed {
            return Err(EngineError::InvalidState(
                "file operation is only available for completed downloads".to_string(),
            ));
        }
        let path = record.output_path.ok_or_else(|| {
            EngineError::NotFound("completed download has no output path".to_string())
        })?;
        Ok((store.clone(), path))
    }

    /// Renames a `Completed` `Download`'s output file on disk and updates SQLite.
    pub async fn rename_completed_file(
        &self,
        id: &DownloadId,
        new_filename: &str,
    ) -> Result<PathBuf, EngineError> {
        let (store, old_path) = self.prepare_completed_file_op(id).await?;
        let parent = old_path
            .parent()
            .ok_or_else(|| EngineError::InvalidState("missing parent directory".to_string()))?;
        let sanitized = probe::sanitize_filename(new_filename).ok_or_else(|| {
            EngineError::InvalidState("filename cannot be empty or invalid".to_string())
        })?;
        let new_path = parent.join(&sanitized);
        if new_path == old_path {
            return Ok(new_path);
        }
        if new_path.exists() {
            return Err(EngineError::InvalidState(format!(
                "target file already exists: {}",
                new_path.display()
            )));
        }
        tokio::fs::rename(&old_path, &new_path).await?;
        store
            .update_completed_file_paths(id, &new_path, None)
            .await?;
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(new_path)
    }

    /// Moves a `Completed` `Download`'s output file to `target_dir` and updates SQLite.
    pub async fn move_completed_file(
        &self,
        id: &DownloadId,
        target_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, EngineError> {
        let (store, old_path) = self.prepare_completed_file_op(id).await?;
        let file_name = old_path
            .file_name()
            .ok_or_else(|| EngineError::InvalidState("missing filename".to_string()))?
            .to_os_string();
        let target_dir = target_dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&target_dir).await?;
        let new_path = target_dir.join(file_name);
        if new_path == old_path {
            return Ok(new_path);
        }
        if new_path.exists() {
            return Err(EngineError::InvalidState(format!(
                "target file already exists: {}",
                new_path.display()
            )));
        }
        if let Err(rename_err) = tokio::fs::rename(&old_path, &new_path).await {
            // Cross-volume move fallback: copy -> fsync -> remove original.
            if tokio::fs::copy(&old_path, &new_path).await.is_err() {
                return Err(EngineError::Io(rename_err));
            }
            tokio::fs::remove_file(&old_path).await?;
        }
        store
            .update_completed_file_paths(id, &new_path, Some(&target_dir))
            .await
            .map_err(|e| EngineError::TaskJoin(e.to_string()))?;
        let _ = store.record_folder_history(&target_dir).await;
        let _ = self.event_tx.send(EngineEvent::QueueUpdated);
        Ok(new_path)
    }

    /// Executes a single `Download` to completion, streaming progress updates via `progress_tx`.
    pub async fn download(
        &self,
        request: DownloadRequest,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
    ) -> Result<DownloadOutcome, EngineError> {
        let id = DownloadId::new();
        if let Some(store) = &self.store {
            store.insert_queued(&id, &request).await?;
        }
        run_direct_download(
            self.http_client(),
            Arc::clone(&self.limiter),
            self.store.clone(),
            self.event_tx.clone(),
            id,
            request,
            progress_tx,
        )
        .await
    }
}

fn build_http_client(proxy_mode: ProxyMode, proxy_url: &str) -> Result<reqwest::Client, EngineError> {
    let mut builder = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .http1_only()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(6)
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_nodelay(true);

    match proxy_mode {
        ProxyMode::System => {}
        ProxyMode::None => {
            builder = builder.no_proxy();
        }
        ProxyMode::Manual => {
            let trimmed = proxy_url.trim();
            if trimmed.is_empty() {
                return Err(EngineError::InvalidState(
                    "manual proxy mode requires a proxy URL".to_string(),
                ));
            }
            let proxy = reqwest::Proxy::all(trimmed).map_err(|err| {
                EngineError::InvalidState(format!("invalid proxy URL '{trimmed}': {err}"))
            })?;
            builder = builder.proxy(proxy);
        }
    }

    builder.build().map_err(EngineError::Http)
}

async fn reserve_non_conflicting_output_path(
    store: &DownloadStore,
    request: &mut DownloadRequest,
) {
    let Some(explicit) = request.output_path.as_ref() else {
        return;
    };
    let Ok(reserved) = store.list_reserved_output_paths(None).await else {
        return;
    };
    if crate::model::path_conflicts_with_reserved(explicit, &reserved) {
        let parent = explicit.parent().unwrap_or(&request.output_dir);
        let name = explicit
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("download.bin");
        request.output_path = Some(disambiguate_output_path_with_reserved(
            parent,
            name,
            &reserved,
        ));
    }
}

pub(crate) async fn remove_file_if_exists(path: &Path) -> Result<(), EngineError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(EngineError::Io(err)),
    }
}
