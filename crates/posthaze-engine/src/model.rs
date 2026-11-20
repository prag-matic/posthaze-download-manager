//! Pure domain types and configuration models for [`DownloadEngine`](crate::DownloadEngine).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use thiserror::Error;

/// Persistent identifier for a `Download` (UUID v4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DownloadId(pub(crate) String);

impl DownloadId {
    /// Generates a new random UUID v4 [`DownloadId`].
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Returns the string representation of this [`DownloadId`].
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DownloadId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DownloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for DownloadId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for DownloadId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Lifecycle state of a `Download`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    /// Waiting to start.
    Queued,
    /// Inspecting server capabilities and metadata (`HEAD` / `Range` probe).
    Probing,
    /// Actively transferring `Segment`s and writing `Chunk`s to `.part`.
    Active,
    /// Paused by user or `Queue`.
    Paused,
    /// All `Segment`s flushed, synced, and renamed to the final path.
    Completed,
    /// `Download` aborted due to an unrecoverable error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

impl DownloadState {
    /// Returns the canonical database/display string for this state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Probing => "Probing",
            Self::Active => "Active",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

impl FromStr for DownloadState {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Queued" => Ok(Self::Queued),
            "Probing" => Ok(Self::Probing),
            "Active" => Ok(Self::Active),
            "Paused" => Ok(Self::Paused),
            "Completed" => Ok(Self::Completed),
            "Failed" => Ok(Self::Failed),
            "Cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// Errors produced by [`DownloadEngine`](crate::DownloadEngine).
#[derive(Debug, Error)]
pub enum EngineError {
    /// Underlying `reqwest` transport or protocol error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Server returned a non-success HTTP status code.
    #[error("unexpected HTTP status {status} for {url}")]
    HttpStatus {
        /// HTTP status code.
        status: u16,
        /// Effective URL that returned the status.
        url: String,
    },

    /// Local filesystem or OS I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Dedicated `.part` writer task failed or panicked.
    #[error("file writer task failed: {0}")]
    Writer(String),

    /// SQLite persistence error.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// SQLite schema migration error.
    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// Requested operation is invalid for the current `Download` state.
    #[error("invalid download state: {0}")]
    InvalidState(String),

    /// No `Download` record exists for the requested [`DownloadId`].
    #[error("download not found: {0}")]
    NotFound(String),

    /// Server `Validator` or `Content-Length` changed since the `Download` was paused.
    #[error("remote resource changed: {0}")]
    ResourceChanged(String),

    /// `Download` was paused before completion.
    #[error("download paused")]
    Paused,

    /// `Download` was cancelled before completion.
    #[error("download cancelled")]
    Cancelled,

    /// Background `Download` execution failed to join.
    #[error("download task join error: {0}")]
    TaskJoin(String),
}

/// Classification of a `Failed` `Download` used by callers without matching error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Server `Validator` or `Content-Length` changed since pause; requires starting fresh.
    ResourceChanged,
    /// Failure is potentially resumable or retryable.
    Resumable,
    /// Failure is not resumable (e.g. invalid state or non-range `Download`).
    NonResumable,
}

impl EngineError {
    /// Classifies this error into a structured [`FailureKind`].
    pub fn failure_kind(&self) -> FailureKind {
        match self {
            Self::ResourceChanged(_) => FailureKind::ResourceChanged,
            Self::InvalidState(_) => FailureKind::NonResumable,
            Self::HttpStatus { status, .. }
                if *status != 408 && *status != 429 && !(500..=599).contains(status) =>
            {
                FailureKind::NonResumable
            }
            _ => FailureKind::Resumable,
        }
    }
}

/// Request parameters for starting a `Download`.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// Source HTTP/HTTPS URL.
    pub url: String,
    /// Explicit destination file path (overrides `output_dir` + resolved filename).
    pub output_path: Option<PathBuf>,
    /// Target directory used when `output_path` is `None`.
    pub output_dir: PathBuf,
    /// Maximum concurrent `Worker`s allowed for multi-`Segment` `Download`s.
    pub max_workers: usize,
    /// Whether to call `File::sync_all()` (`FlushFileBuffers`) before renaming `.part`.
    pub sync_on_complete: bool,
    /// Custom HTTP request headers (`(name, value)`) sent on probe, transfer, and resume.
    pub headers: Vec<(String, String)>,
}

impl DownloadRequest {
    /// Creates a new request targeting `output_dir` with default settings.
    pub fn new(url: impl Into<String>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            output_path: None,
            output_dir: output_dir.into(),
            max_workers: 8,
            sync_on_complete: false,
            headers: Vec::new(),
        }
    }
}

/// Proxy configuration mode for [`DownloadEngine`](crate::DownloadEngine) HTTP/HTTPS requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    /// Use the operating system's configured proxy settings.
    System,
    /// Bypass all proxies and connect directly.
    None,
    /// Route requests through an explicit proxy URL (`http://`, `https://`, or `socks5://`).
    Manual,
}

impl ProxyMode {
    /// Returns the persisted database token for this proxy mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::None => "none",
            Self::Manual => "manual",
        }
    }
}

impl FromStr for ProxyMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "system" => Ok(Self::System),
            "none" => Ok(Self::None),
            "manual" => Ok(Self::Manual),
            _ => Err(()),
        }
    }
}

/// Reaction when a `Download`'s target filename already exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionPolicy {
    /// Automatically disambiguate with ` (1)`, ` (2)`, etc.
    Rename,
    /// Overwrite the existing destination file on completion.
    Overwrite,
    /// Prompt the user during Pre-Flight before starting.
    Ask,
}

impl CollisionPolicy {
    /// Returns the persisted database token for this collision policy.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Overwrite => "overwrite",
            Self::Ask => "ask",
        }
    }
}

impl FromStr for CollisionPolicy {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rename" => Ok(Self::Rename),
            "overwrite" => Ok(Self::Overwrite),
            "ask" => Ok(Self::Ask),
            _ => Err(()),
        }
    }
}

/// Default behavior when clicking a row's delete button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteDefaultAction {
    /// Prompt with a confirmation modal before deleting.
    Ask,
    /// Remove only from the `Queue` list while keeping completed files on disk.
    Remove,
    /// Remove from the list and permanently delete the file from disk.
    DeleteFile,
}

impl DeleteDefaultAction {
    /// Returns the persisted database token for this delete action.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Remove => "remove",
            Self::DeleteFile => "delete_file",
        }
    }
}

impl FromStr for DeleteDefaultAction {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ask" => Ok(Self::Ask),
            "remove" => Ok(Self::Remove),
            "delete_file" => Ok(Self::DeleteFile),
            _ => Err(()),
        }
    }
}

/// Returns an optional category subfolder name (`Archives`, `Video`, `Audio`, `Documents`,
/// `Programs`) for `filename` based on its extension.
pub fn categorize_subfolder(filename: &str) -> Option<&'static str> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "zst" | "bz2" | "iso" => Some("Archives"),
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "flv" | "m4v" => Some("Video"),
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" => Some("Audio"),
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv" | "epub" => {
            Some("Documents")
        }
        "exe" | "msi" | "msix" | "appx" | "bat" | "ps1" => Some("Programs"),
        _ => None,
    }
}

/// Row height and padding density for the `Downloads` table UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableDensity {
    /// High-density 30px rows for large queues.
    Compact,
    /// Balanced 38px rows (default).
    #[default]
    Standard,
    /// Relaxed 46px rows with extra vertical breathing room.
    Comfortable,
}

impl TableDensity {
    /// Returns the persisted database string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Standard => "standard",
            Self::Comfortable => "comfortable",
        }
    }

    /// Parses a persisted density identifier, defaulting to [`TableDensity::Standard`].
    pub fn from_db(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "compact" => Self::Compact,
            "comfortable" => Self::Comfortable,
            _ => Self::Standard,
        }
    }

    /// Returns the logical pixel height of a single `Downloads` table row.
    pub fn row_height_px(self) -> f32 {
        match self {
            Self::Compact => 30.0,
            Self::Standard => 38.0,
            Self::Comfortable => 46.0,
        }
    }
}

/// Persisted application configuration for [`DownloadEngine`](crate::DownloadEngine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Default target directory for new `Download`s when no explicit path is provided.
    pub default_output_dir: PathBuf,
    /// Default maximum concurrent `Worker`s per multi-`Segment` `Download`.
    pub max_workers: usize,
    /// Maximum number of `Download`s admitted simultaneously by the `Queue`.
    pub max_concurrent_downloads: usize,
    /// Whether completed `Download`s call `File::sync_all()` before renaming `.part`.
    pub sync_on_complete: bool,
    /// Proxy routing mode (`System`, `None`, or `Manual`).
    pub proxy_mode: ProxyMode,
    /// Explicit proxy URL when `proxy_mode == ProxyMode::Manual`.
    pub proxy_url: String,
    /// Global `Download` speed limit in KiB/s (`0` = unlimited).
    pub download_speed_limit_kib: u64,
    /// Whether to route downloads into file-type subfolders under `default_output_dir`.
    pub auto_categorize_folders: bool,
    /// Policy when a destination file already exists on disk.
    pub collision_policy: CollisionPolicy,
    /// Default action for the row delete button.
    pub delete_action_default: DeleteDefaultAction,
    /// Number of automatic retries after a `Download` transitions to `Failed`.
    pub auto_retry_count: u32,
    /// Whether to run an external command (or antivirus scanner) upon `Download` completion.
    pub completion_cmd_enabled: bool,
    /// Executable path for the post-completion hook.
    pub completion_cmd_path: String,
    /// Argument template for the post-completion hook (`%path%` expands to the file path).
    pub completion_cmd_args: String,
    /// Show a Windows notification when a `Download` is added.
    pub notify_on_added: bool,
    /// Show a Windows notification when a `Download` completes.
    pub notify_on_completed: bool,
    /// Show a Windows notification when a `Download` fails.
    pub notify_on_failed: bool,
    /// Suppress notifications while the Posthaze window is focused.
    pub notify_only_inactive: bool,
    /// Expand the bottom byte-map inspector automatically when clicking a row.
    pub ui_click_row_opens_inspector: bool,
    /// Show the `Add Paused` button in the Pre-Flight modal.
    pub ui_show_add_paused: bool,
    /// UI zoom percentage (`90`, `100`, `110`, `125`).
    pub ui_zoom_percent: u16,
    /// Active UI color theme identifier (e.g. `"chronophotographic"`, `"cyanotype"`, `"archival"`).
    pub ui_theme: String,
    /// Row height density preset for the `Downloads` table.
    pub ui_table_density: TableDensity,
    /// Whether the `SPEED` column is shown in the `Downloads` table.
    pub ui_show_speed_col: bool,
    /// Whether the `ETA` column is shown in the `Downloads` table.
    pub ui_show_eta_col: bool,
    /// Whether the `SEGS` column is shown in the `Downloads` table.
    pub ui_show_segs_col: bool,
    /// Whether top-toolbar action cells show text labels alongside icons.
    pub ui_show_toolbar_labels: bool,
    /// Whether the vertical scrollbar rail renders `Active`/`Failed` row minimap ticks.
    pub ui_show_scrollbar_minimap: bool,
    /// Whether the custom titlebar displays live `Queue` telemetry.
    pub ui_show_titlebar_telemetry: bool,
    /// Whether the left filter rail starts collapsed on launch.
    pub ui_start_sidebar_collapsed: bool,
    /// Optional accent color preset override identifier (e.g. `"amber"`, `"blue"`, `"emerald"`).
    pub ui_accent_override: Option<String>,
    /// Whether the Add Download dialog locks the probed file extension from accidental edits.
    pub ui_lock_file_extension: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_output_dir: default_downloads_dir(),
            max_workers: 8,
            max_concurrent_downloads: 3,
            sync_on_complete: false,
            proxy_mode: ProxyMode::System,
            proxy_url: String::new(),
            download_speed_limit_kib: 0,
            auto_categorize_folders: false,
            collision_policy: CollisionPolicy::Rename,
            delete_action_default: DeleteDefaultAction::Ask,
            auto_retry_count: 0,
            completion_cmd_enabled: false,
            completion_cmd_path: String::new(),
            completion_cmd_args: "%path%".to_string(),
            notify_on_added: false,
            notify_on_completed: true,
            notify_on_failed: true,
            notify_only_inactive: true,
            ui_click_row_opens_inspector: true,
            ui_show_add_paused: true,
            ui_zoom_percent: 100,
            ui_theme: "chronophotographic".to_string(),
            ui_table_density: TableDensity::Standard,
            ui_show_speed_col: true,
            ui_show_eta_col: true,
            ui_show_segs_col: true,
            ui_show_toolbar_labels: true,
            ui_show_scrollbar_minimap: true,
            ui_show_titlebar_telemetry: true,
            ui_start_sidebar_collapsed: false,
            ui_accent_override: None,
            ui_lock_file_extension: true,
        }
    }
}

impl Settings {
    /// Validates engine-level `Settings` invariants before persisting or applying them.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.max_workers == 0 {
            return Err(EngineError::InvalidState(
                "max_workers must be at least 1".to_string(),
            ));
        }
        if self.max_concurrent_downloads == 0 {
            return Err(EngineError::InvalidState(
                "max_concurrent_downloads must be at least 1".to_string(),
            ));
        }
        if self.proxy_mode == ProxyMode::Manual && self.proxy_url.trim().is_empty() {
            return Err(EngineError::InvalidState(
                "manual proxy mode requires a proxy URL".to_string(),
            ));
        }
        Ok(())
    }
}

/// Returns the default SQLite state database path (`%LOCALAPPDATA%\posthaze\state.db`).
pub fn default_db_path() -> PathBuf {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA")
        && !local_app_data.is_empty()
    {
        return PathBuf::from(local_app_data)
            .join("posthaze")
            .join("state.db");
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("posthaze")
            .join("state.db");
    }
    PathBuf::from("posthaze-state.db")
}

/// Result of resolving a target file path under a [`CollisionPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPathResolution {
    /// Resolved destination file path.
    pub path: PathBuf,
    /// Whether an existing target or `.part` file requires user confirmation (`CollisionPolicy::Ask`).
    pub needs_overwrite_prompt: bool,
}

/// Returns the `.part` sidecar path corresponding to `final_path`.
pub(crate) fn make_part_path(final_path: &Path) -> PathBuf {
    let mut os: OsString = final_path.as_os_str().to_os_string();
    os.push(".part");
    PathBuf::from(os)
}

/// Returns a collision-free file path in `output_dir`, checking both target and `.part` paths.
pub fn disambiguate_output_path(output_dir: &Path, filename: &str) -> PathBuf {
    let initial = output_dir.join(filename);
    if !initial.exists() && !make_part_path(&initial).exists() {
        return initial;
    }

    let file_path = Path::new(filename);
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(filename);
    let ext = file_path.extension().and_then(|e| e.to_str());

    let mut counter: usize = 1;
    loop {
        let numbered = match ext {
            Some(ext) if !ext.is_empty() => format!("{stem} ({counter}).{ext}"),
            _ => format!("{stem} ({counter})"),
        };
        let candidate = output_dir.join(numbered);
        if !candidate.exists() && !make_part_path(&candidate).exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Resolves a target path in `output_dir` for `filename` according to `policy`.
pub fn resolve_target_path(
    output_dir: &Path,
    filename: &str,
    policy: CollisionPolicy,
) -> TargetPathResolution {
    let target = output_dir.join(filename);
    let collides = target.exists() || make_part_path(&target).exists();
    match policy {
        CollisionPolicy::Overwrite => TargetPathResolution {
            path: target,
            needs_overwrite_prompt: false,
        },
        CollisionPolicy::Rename => TargetPathResolution {
            path: if collides {
                disambiguate_output_path(output_dir, filename)
            } else {
                target
            },
            needs_overwrite_prompt: false,
        },
        CollisionPolicy::Ask => TargetPathResolution {
            path: target,
            needs_overwrite_prompt: collides,
        },
    }
}

fn default_downloads_dir() -> PathBuf {
    if let Some(profile) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let downloads = PathBuf::from(&profile).join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
        return PathBuf::from(profile);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Non-mutating metadata returned by [`DownloadEngine::probe`](crate::DownloadEngine::probe) before starting a `Download`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightInfo {
    /// Original URL supplied to the probe.
    pub original_url: String,
    /// Effective URL after following HTTP redirects.
    pub final_url: String,
    /// Sanitized filename derived from `Content-Disposition` or the URL path.
    pub suggested_filename: String,
    /// Collision-free target path inside the requested output directory.
    pub disambiguated_output_path: PathBuf,
    /// Server-reported `Content-Length`, if known.
    pub content_length: Option<u64>,
    /// Whether the server advertised byte-range (`206 Partial Content`) support.
    pub supports_ranges: bool,
    /// Server `ETag` `Validator`, if present.
    pub etag: Option<String>,
    /// Server `Last-Modified` `Validator`, if present.
    pub last_modified: Option<String>,
}

/// Live or persisted `Segment` progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentProgress {
    /// Zero-based `Segment` index within the `Download`.
    pub id: usize,
    /// Inclusive start byte offset of this `Segment`.
    pub start: u64,
    /// Current network progress offset (`>= start`).
    pub cursor: u64,
    /// Highest writer-acknowledged `Checkpoint` offset flushed to disk (`start <= checkpointed_cursor <= cursor`).
    pub checkpointed_cursor: u64,
    /// Exclusive end byte offset of this `Segment`, or `None` for an unbounded stream.
    pub end: Option<u64>,
    /// Whether this `Segment` has finished receiving all bytes.
    pub completed: bool,
}

/// Snapshot of a `Download`'s progress and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    /// Current lifecycle state.
    pub state: DownloadState,
    /// Total bytes received so far.
    pub bytes_downloaded: u64,
    /// Expected total size from `Content-Length`, if known.
    pub total_bytes: Option<u64>,
    /// Number of currently active `Worker`s.
    pub active_workers: usize,
    /// Smoothed instantaneous throughput in bytes per second.
    pub bytes_per_sec: u64,
    /// Estimated remaining seconds until completion, if `total_bytes` and speed are known.
    pub eta_secs: Option<u64>,
    /// Per-`Segment` progress and disk `Checkpoint` offsets.
    pub segments: Vec<SegmentProgress>,
}

impl Default for ProgressSnapshot {
    fn default() -> Self {
        Self {
            state: DownloadState::Queued,
            bytes_downloaded: 0,
            total_bytes: None,
            active_workers: 0,
            bytes_per_sec: 0,
            eta_secs: None,
            segments: Vec::new(),
        }
    }
}

impl ProgressSnapshot {
    pub(crate) fn status(
        state: DownloadState,
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
        active_workers: usize,
    ) -> Self {
        Self {
            state,
            bytes_downloaded,
            total_bytes,
            active_workers,
            bytes_per_sec: 0,
            eta_secs: None,
            segments: Vec::new(),
        }
    }
}

/// Engine-wide event stream consumed by desktop UI views and observers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    /// A `Download` transitioned to a new [`DownloadState`].
    StateChanged {
        /// Identifier of the `Download`.
        id: DownloadId,
        /// New lifecycle state.
        state: DownloadState,
        /// Optional failure message when `state` is [`DownloadState::Failed`].
        error: Option<String>,
        /// Structured failure classification when `state` is [`DownloadState::Failed`].
        failure_kind: Option<FailureKind>,
    },
    /// Periodic or terminal [`ProgressSnapshot`] update for a `Download`.
    ProgressUpdated {
        /// Identifier of the `Download`.
        id: DownloadId,
        /// Latest progress and per-`Segment` snapshot.
        snapshot: ProgressSnapshot,
    },
    /// `Queue` membership or concurrency slot assignment changed.
    QueueUpdated,
    /// Persisted [`Settings`] were updated.
    SettingsUpdated(Settings),
}

/// Result and timing summary returned when a `Download` completes.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadOutcome {
    /// Final destination file path after `.part` rename.
    pub final_path: PathBuf,
    /// Total bytes written to disk.
    pub bytes_downloaded: u64,
    /// Server-reported `Content-Length`, if known.
    pub content_length: Option<u64>,
    /// Whether the server advertised byte-range support.
    pub supports_ranges: bool,
    /// Number of `Worker`s used during the `Download`.
    pub workers_used: usize,
    /// Number of dynamic `Segment` splits (work steals) performed.
    pub splits_performed: usize,
    /// Number of transient `Worker` retries performed.
    pub retries_performed: usize,
    /// Time spent in `Probing`.
    pub probe_duration: Duration,
    /// Time spent transferring `Chunk`s and finalizing the `.part` file.
    pub transfer_duration: Duration,
    /// End-to-end wall-clock duration.
    pub total_duration: Duration,
    /// Server `ETag` header, if present.
    pub etag: Option<String>,
    /// Server `Last-Modified` header, if present.
    pub last_modified: Option<String>,
    /// Final effective URL after redirects.
    pub final_url: String,
}

impl DownloadOutcome {
    /// Average transfer throughput in bytes per second.
    pub fn average_bytes_per_sec(&self) -> f64 {
        let secs = self.transfer_duration.as_secs_f64();
        if secs <= 0.0 {
            0.0
        } else {
            self.bytes_downloaded as f64 / secs
        }
    }
}

/// Initial `[start, end)` bounds for a `Segment` persisted when a `Download` activates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlannedSegment {
    pub start: u64,
    pub end: Option<u64>,
}

/// Persisted `Checkpoint` record for a single `Segment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentCheckpoint {
    pub segment_index: usize,
    pub start: u64,
    pub cursor: u64,
    pub end: Option<u64>,
    pub completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_validate_rejects_invalid_bounds_and_empty_manual_proxy() {
        let mut s = Settings::default();
        assert!(s.validate().is_ok());

        s.max_workers = 0;
        assert!(matches!(s.validate(), Err(EngineError::InvalidState(_))));

        s.max_workers = 4;
        s.max_concurrent_downloads = 0;
        assert!(matches!(s.validate(), Err(EngineError::InvalidState(_))));

        s.max_concurrent_downloads = 2;
        s.proxy_mode = ProxyMode::Manual;
        s.proxy_url = "   ".to_string();
        assert!(matches!(s.validate(), Err(EngineError::InvalidState(_))));

        s.proxy_url = "http://127.0.0.1:8080".to_string();
        assert!(s.validate().is_ok());
    }

    #[test]
    fn resolve_target_path_checks_both_target_and_part_files() {
        let temp_dir = std::env::temp_dir().join(format!("posthaze_path_test_{}", DownloadId::new()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let part_only = temp_dir.join("archive.zip.part");
        std::fs::write(&part_only, b"partial").unwrap();

        let renamed = resolve_target_path(&temp_dir, "archive.zip", CollisionPolicy::Rename);
        assert_eq!(renamed.path, temp_dir.join("archive (1).zip"));
        assert!(!renamed.needs_overwrite_prompt);

        let asked = resolve_target_path(&temp_dir, "archive.zip", CollisionPolicy::Ask);
        assert_eq!(asked.path, temp_dir.join("archive.zip"));
        assert!(asked.needs_overwrite_prompt);

        let overwrite = resolve_target_path(&temp_dir, "archive.zip", CollisionPolicy::Overwrite);
        assert_eq!(overwrite.path, temp_dir.join("archive.zip"));
        assert!(!overwrite.needs_overwrite_prompt);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn failure_kind_and_default_db_path_classify_and_resolve_cleanly() {
        assert_eq!(
            EngineError::ResourceChanged("etag".to_string()).failure_kind(),
            FailureKind::ResourceChanged
        );
        assert_eq!(
            EngineError::InvalidState("bad".to_string()).failure_kind(),
            FailureKind::NonResumable
        );
        assert_eq!(
            EngineError::HttpStatus {
                status: 404,
                url: "https://example.com".to_string()
            }
            .failure_kind(),
            FailureKind::NonResumable
        );
        assert_eq!(
            EngineError::HttpStatus {
                status: 503,
                url: "https://example.com".to_string()
            }
            .failure_kind(),
            FailureKind::Resumable
        );
        assert_eq!(
            EngineError::Writer("disk".to_string()).failure_kind(),
            FailureKind::Resumable
        );
        assert!(default_db_path().ends_with("state.db") || default_db_path().ends_with("posthaze-state.db"));
    }
}
