//! Pure domain types and configuration models for [`DownloadEngine`](crate::DownloadEngine).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

/// Persistent identifier for a `Download` (UUID v4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DownloadId(pub(crate) Arc<str>);

impl DownloadId {
    /// Generates a new random UUID v4 [`DownloadId`].
    pub fn new() -> Self {
        Self(Arc::from(uuid::Uuid::new_v4().to_string()))
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
        Self(Arc::from(value))
    }
}

impl From<&str> for DownloadId {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
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

    /// Server rate-limited or temporarily rejected the request with a `Retry-After` delay.
    #[error("HTTP {status} throttled for {url}; retry after {}s", retry_after.as_secs())]
    Throttled {
        /// HTTP status code (`429` or `503`).
        status: u16,
        /// Parsed server-requested retry delay.
        retry_after: Duration,
        /// Effective URL that returned the status.
        url: String,
    },

    /// Target volume does not have enough free space for the `Download`.
    #[error("insufficient disk space in {}: need {required_bytes} bytes, only {available_bytes} bytes available", path.display())]
    InsufficientDiskSpace {
        /// Directory or target file path checked before transfer.
        path: PathBuf,
        /// Required space in bytes (including safety margin).
        required_bytes: u64,
        /// Available bytes reported by the operating system.
        available_bytes: u64,
    },

    /// Target volume filesystem cannot store a file of this size (e.g. > 4 GiB - 1 on FAT32).
    #[error("file size ({size_bytes} bytes) exceeds filesystem limit ({max_bytes} bytes) at {}", path.display())]
    FileTooLargeForFilesystem {
        /// Directory or target file path checked before transfer.
        path: PathBuf,
        /// Requested file size in bytes.
        size_bytes: u64,
        /// Maximum supported file size on the target filesystem.
        max_bytes: u64,
    },

    /// Server returned an HTML page (e.g. expired token, login gate, or error page) instead of file bytes.
    #[error("server returned HTML instead of file for {url}: {detail}")]
    UnexpectedHtmlResponse {
        /// Effective URL that returned HTML.
        url: String,
        /// Diagnostic summary (e.g. `Content-Type` and page title/snippet).
        detail: String,
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
            Self::InvalidState(_)
            | Self::UnexpectedHtmlResponse { .. }
            | Self::InsufficientDiskSpace { .. }
            | Self::FileTooLargeForFilesystem { .. } => FailureKind::NonResumable,
            Self::Throttled { .. } => FailureKind::Resumable,
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
    /// Optional known `Content-Length` (e.g., `totalBytes` from `Browser Capture`).
    pub known_content_length: Option<u64>,
    /// Skip `Probing` (`HEAD` / `Range: bytes=0-0` `GET`) and start a single-stream `Download` directly.
    pub skip_probe: bool,
    /// Use a single initial `Range: bytes=0-` `GET` request as the probe and reuse its live stream
    /// for Worker `0` (saving 1 RTT when `HEAD` metadata/HTML-mismatch pre-checks are unnecessary).
    pub probe_via_get: bool,
    /// Optional pre-resolved [`PreflightInfo`] from [`DownloadEngine::probe_with_headers`](crate::DownloadEngine::probe_with_headers) to avoid re-probing.
    pub preflight_probe: Option<PreflightInfo>,
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
            known_content_length: None,
            skip_probe: false,
            probe_via_get: false,
            preflight_probe: None,
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
    /// Show a Windows notification when a new `Download` starts transferring.
    pub notify_on_started: bool,
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
    /// Whether closing the main window minimizes to the Windows System Tray instead of exiting.
    pub ui_close_to_tray: bool,
    /// Whether Posthaze launches minimized to the System Tray on Windows user login.
    pub launch_on_startup: bool,
    /// Whether Posthaze checks GitHub Releases for a newer version at startup and every 24 hours.
    pub auto_check_updates: bool,
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
            notify_on_started: false,
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
            ui_close_to_tray: true,
            launch_on_startup: false,
            auto_check_updates: true,
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

/// Marker filename beside the executable that enables self-contained portable data storage.
pub const PORTABLE_MARKER_FILE: &str = "posthaze.portable";

/// Resolves Posthaze's persistent data directory given optional executable directory and
/// `LOCALAPPDATA` / `HOME` environment values.
///
/// When [`PORTABLE_MARKER_FILE`] exists in `exe_dir`, returns `exe_dir/data`. Otherwise falls back
/// to `LOCALAPPDATA/posthaze`, `HOME/.local/share/posthaze`, or `.` when neither is set.
pub fn resolve_data_dir(
    exe_dir: Option<&Path>,
    local_app_data: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> PathBuf {
    if let Some(dir) = exe_dir
        && dir.join(PORTABLE_MARKER_FILE).is_file()
    {
        return dir.join("data");
    }
    if let Some(local) = local_app_data
        && !local.is_empty()
    {
        return PathBuf::from(local).join("posthaze");
    }
    if let Some(home_dir) = home
        && !home_dir.is_empty()
    {
        return PathBuf::from(home_dir)
            .join(".local")
            .join("share")
            .join("posthaze");
    }
    PathBuf::from(".")
}

/// Returns the active Posthaze data directory (portable `./data` or `%LOCALAPPDATA%\posthaze`).
pub fn default_data_dir() -> PathBuf {
    let exe_path = std::env::current_exe().ok();
    let exe_dir = exe_path.as_deref().and_then(Path::parent);
    let local_app_data = std::env::var_os("LOCALAPPDATA");
    let home = std::env::var_os("HOME");
    resolve_data_dir(exe_dir, local_app_data.as_deref(), home.as_deref())
}

/// Returns the default diagnostic logs directory (`<data_dir>\logs`).
pub fn default_logs_dir() -> PathBuf {
    default_data_dir().join("logs")
}

/// Returns the default SQLite state database path (`<data_dir>\state.db`).
pub fn default_db_path() -> PathBuf {
    let data_dir = default_data_dir();
    if data_dir == Path::new(".") {
        return PathBuf::from("posthaze-state.db");
    }
    data_dir.join("state.db")
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

/// Checks whether `candidate` matches any path in `reserved` (ignoring trailing separators and
/// ASCII/Unicode case on Windows).
pub(crate) fn path_conflicts_with_reserved(candidate: &Path, reserved: &[PathBuf]) -> bool {
    let norm_cand = normalize_cmp_path(candidate);
    reserved
        .iter()
        .any(|r| normalize_cmp_path(r) == norm_cand)
}

fn normalize_cmp_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_end_matches(['/', '\\']);
    let base = if trimmed.is_empty() { s.as_ref() } else { trimmed };
    if cfg!(windows) {
        base.to_ascii_lowercase()
    } else {
        base.to_string()
    }
}

/// Returns a collision-free file path in `output_dir`, checking both target and `.part` paths.
pub fn disambiguate_output_path(output_dir: &Path, filename: &str) -> PathBuf {
    disambiguate_output_path_with_reserved(output_dir, filename, &[])
}

/// Strips a trailing ` (N)` collision-counter suffix (1+ ASCII digits) from a filename stem.
pub(crate) fn strip_disambiguation_suffix(stem: &str) -> &str {
    let trimmed = stem.trim_end();
    if let Some(open) = trimmed.rfind(" (")
        && trimmed.ends_with(')')
    {
        let inner = &trimmed[open + 2..trimmed.len() - 1];
        if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
            return trimmed[..open].trim_end();
        }
    }
    trimmed
}

/// Returns a lowercased canonical filename with any trailing ` (N)` disambiguation counter removed
/// (for duplicate detection across auto-renamed `(1)`/`(2)` copies).
pub fn canonical_filename_key(filename: &str) -> String {
    let trimmed = filename.trim();
    let file_path = Path::new(trimmed);
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed);
    let base_stem = strip_disambiguation_suffix(stem);
    match file_path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => {
            format!("{}.{ext}", base_stem).to_ascii_lowercase()
        }
        _ => base_stem.to_ascii_lowercase(),
    }
}

/// Returns a collision-free file path in `output_dir`, avoiding existing files on disk, `.part`
/// files, and active/paused DB `reserved_paths`.
pub fn disambiguate_output_path_with_reserved(
    output_dir: &Path,
    filename: &str,
    reserved_paths: &[PathBuf],
) -> PathBuf {
    let is_free = |candidate: &Path| {
        !candidate.exists()
            && !make_part_path(candidate).exists()
            && !path_conflicts_with_reserved(candidate, reserved_paths)
    };
    let initial = output_dir.join(filename);
    if is_free(&initial) {
        return initial;
    }

    let file_path = Path::new(filename);
    let raw_stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(filename);
    let stem = strip_disambiguation_suffix(raw_stem);
    let stem = if stem.is_empty() { raw_stem } else { stem };
    let ext = file_path.extension().and_then(|e| e.to_str());

    let mut counter: usize = 1;
    loop {
        let numbered = match ext {
            Some(ext) if !ext.is_empty() => format!("{stem} ({counter}).{ext}"),
            _ => format!("{stem} ({counter})"),
        };
        let candidate = output_dir.join(numbered);
        if is_free(&candidate) {
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
    resolve_target_path_with_reserved(output_dir, filename, policy, &[])
}

/// Resolves a target path in `output_dir` for `filename` according to `policy`, while never
/// overwriting a path claimed by another ongoing/paused transfer in `reserved_paths`.
pub fn resolve_target_path_with_reserved(
    output_dir: &Path,
    filename: &str,
    policy: CollisionPolicy,
    reserved_paths: &[PathBuf],
) -> TargetPathResolution {
    let target = output_dir.join(filename);
    if path_conflicts_with_reserved(&target, reserved_paths) {
        return TargetPathResolution {
            path: disambiguate_output_path_with_reserved(output_dir, filename, reserved_paths),
            needs_overwrite_prompt: false,
        };
    }
    let collides = target.exists() || make_part_path(&target).exists();
    match policy {
        CollisionPolicy::Overwrite => TargetPathResolution {
            path: target,
            needs_overwrite_prompt: false,
        },
        CollisionPolicy::Rename => TargetPathResolution {
            path: if collides {
                disambiguate_output_path_with_reserved(output_dir, filename, reserved_paths)
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

/// Resolves the user's default Downloads folder.
///
/// On Windows, queries `SHGetKnownFolderPath(FOLDERID_Downloads)` so custom or relocated
/// Downloads folders (e.g. another drive or OneDrive) are honored, falling back to
/// `%USERPROFILE%\Downloads` (or `$HOME/Downloads`).
pub fn default_downloads_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(known) = windows_known_downloads_dir() {
        return known;
    }

    if let Some(profile) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let downloads = PathBuf::from(&profile).join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
        return PathBuf::from(profile);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(target_os = "windows")]
fn windows_known_downloads_dir() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};

    let mut raw_ptr: *mut u16 = std::ptr::null_mut();
    // SAFETY: `FOLDERID_Downloads` is a static GUID and `raw_ptr` is freed via `CoTaskMemFree`
    // regardless of whether `SHGetKnownFolderPath` succeeded or allocated a buffer.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut raw_ptr) };
    if raw_ptr.is_null() {
        return None;
    }
    let result = if hr >= 0 {
        let mut len = 0usize;
        // SAFETY: `SHGetKnownFolderPath` returns a null-terminated UTF-16 string on success.
        unsafe {
            while *raw_ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(raw_ptr, len);
            let path = PathBuf::from(OsString::from_wide(slice));
            (!path.as_os_str().is_empty()).then_some(path)
        }
    } else {
        None
    };
    // SAFETY: `raw_ptr` was allocated by `SHGetKnownFolderPath`'s task allocator.
    unsafe { CoTaskMemFree(raw_ptr.cast()) };
    result
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
    /// Server-advertised representation digest (`Repr-Digest` or `Content-MD5`), if present.
    pub server_digest: Option<(ChecksumAlgorithm, String)>,
}

/// Supported file integrity checksum algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    /// BLAKE3 (64 hex chars).
    Blake3,
    /// SHA-256 (64 hex chars).
    Sha256,
    /// SHA-512 (128 hex chars).
    Sha512,
    /// SHA-1 (40 hex chars).
    Sha1,
    /// MD5 (32 hex chars).
    Md5,
    /// CRC-32 IEEE (8 hex chars).
    Crc32,
}

impl ChecksumAlgorithm {
    /// Returns the human-readable uppercase label for this algorithm.
    pub fn label(self) -> &'static str {
        match self {
            Self::Blake3 => "BLAKE3",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
            Self::Sha1 => "SHA-1",
            Self::Md5 => "MD5",
            Self::Crc32 => "CRC32",
        }
    }
}

/// Multi-algorithm checksums computed in a single sequential read over a completed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChecksums {
    /// Lowercase hex `BLAKE3` digest.
    pub blake3: String,
    /// Lowercase hex `SHA-256` digest.
    pub sha256: String,
    /// Lowercase hex `SHA-512` digest.
    pub sha512: String,
    /// Lowercase hex `SHA-1` digest.
    pub sha1: String,
    /// Lowercase hex `MD5` digest.
    pub md5: String,
    /// Lowercase hex `CRC32` digest (8 hex chars).
    pub crc32: String,
}

impl FileChecksums {
    /// Checks whether `expected` matches any computed digest (or the explicit `algo` if given),
    /// returning the matched [`ChecksumAlgorithm`] on match.
    pub fn match_expected(
        &self,
        algo: Option<ChecksumAlgorithm>,
        expected_hex: &str,
    ) -> Option<ChecksumAlgorithm> {
        let needle = expected_hex.trim().to_ascii_lowercase();
        match algo {
            Some(a) => (self.digest_for(a) == needle).then_some(a),
            None => [
                ChecksumAlgorithm::Sha256,
                ChecksumAlgorithm::Blake3,
                ChecksumAlgorithm::Sha512,
                ChecksumAlgorithm::Md5,
                ChecksumAlgorithm::Sha1,
                ChecksumAlgorithm::Crc32,
            ]
            .into_iter()
            .find(|&a| self.digest_for(a) == needle),
        }
    }

    /// Returns the lowercase hex digest string for `algo`.
    pub fn digest_for(&self, algo: ChecksumAlgorithm) -> &str {
        match algo {
            ChecksumAlgorithm::Blake3 => &self.blake3,
            ChecksumAlgorithm::Sha256 => &self.sha256,
            ChecksumAlgorithm::Sha512 => &self.sha512,
            ChecksumAlgorithm::Sha1 => &self.sha1,
            ChecksumAlgorithm::Md5 => &self.md5,
            ChecksumAlgorithm::Crc32 => &self.crc32,
        }
    }
}

/// Parses a user-pasted checksum string (`sha256:...`, `SHA256 (file) = ...`, `sha256sum` line,
/// or bare hex) into an optional explicit [`ChecksumAlgorithm`] and normalized lowercase hex string.
pub fn parse_expected_checksum(raw: &str) -> Option<(Option<ChecksumAlgorithm>, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Check BSD-style: `SHA256 (filename) = <hex>`
    if let Some((prefix, rhs)) = trimmed.split_once('=')
        && let Some(algo_tok) = prefix.split_whitespace().next()
        && let Some(algo) = parse_algo_token(algo_tok)
        && let Some(hex) = clean_hex_token(rhs.split_whitespace().next()?)
    {
        return Some((Some(algo), hex));
    }

    // Check `algo:hex` or `algo=hex`
    for sep in [':', '='] {
        if let Some((algo_tok, rest)) = trimmed.split_once(sep)
            && let Some(algo) = parse_algo_token(algo_tok.trim())
            && let Some(hex) = clean_hex_token(rest.split_whitespace().next()?)
        {
            return Some((Some(algo), hex));
        }
    }

    // Check bare hex or `sha256sum` `<hex>  <filename>` (take first token)
    let first = trimmed.split_whitespace().next()?;
    let hex = clean_hex_token(first)?;
    let inferred = match hex.len() {
        8 => Some(ChecksumAlgorithm::Crc32),
        32 => Some(ChecksumAlgorithm::Md5),
        40 => Some(ChecksumAlgorithm::Sha1),
        128 => Some(ChecksumAlgorithm::Sha512),
        // 64 hex chars can be either SHA-256 or BLAKE3; leave `None` so both are matched.
        64 => None,
        _ => return None,
    };
    Some((inferred, hex))
}

fn parse_algo_token(tok: &str) -> Option<ChecksumAlgorithm> {
    let norm: String = tok
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    match norm.as_str() {
        "blake3" | "b3" => Some(ChecksumAlgorithm::Blake3),
        "sha256" => Some(ChecksumAlgorithm::Sha256),
        "sha512" => Some(ChecksumAlgorithm::Sha512),
        "sha1" => Some(ChecksumAlgorithm::Sha1),
        "md5" => Some(ChecksumAlgorithm::Md5),
        "crc32" | "crc" => Some(ChecksumAlgorithm::Crc32),
        _ => None,
    }
}

fn clean_hex_token(tok: &str) -> Option<String> {
    let clean: String = tok
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .to_ascii_lowercase();
    if !matches!(clean.len(), 8 | 32 | 40 | 64 | 128) {
        return None;
    }
    clean.bytes().all(|b| b.is_ascii_hexdigit()).then_some(clean)
}

/// Strategy for removing a `Completed` `Download`'s file on disk when deleting its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteFileMode {
    /// Send the completed output file to the OS Recycle Bin / Trash when supported.
    RecycleBin,
    /// Permanently unlink the output file from disk.
    Permanent,
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
        assert_eq!(Settings::default().default_output_dir, default_downloads_dir());
        assert!(!default_downloads_dir().as_os_str().is_empty());
    }

    #[test]
    fn resolve_data_dir_switches_between_portable_and_standard_paths() {
        let temp_dir =
            std::env::temp_dir().join(format!("posthaze-portable-test-{}", uuid::Uuid::new_v4()));
        let exe_dir = temp_dir.join("bin");
        let local_app_data = temp_dir.join("LocalAppData");
        let home = temp_dir.join("Home");
        std::fs::create_dir_all(&exe_dir).unwrap();

        // Without marker file -> uses LocalAppData/posthaze.
        let standard = resolve_data_dir(
            Some(&exe_dir),
            Some(local_app_data.as_os_str()),
            Some(home.as_os_str()),
        );
        assert_eq!(standard, local_app_data.join("posthaze"));
        assert_eq!(standard.join("state.db"), local_app_data.join("posthaze").join("state.db"));
        assert_eq!(standard.join("logs"), local_app_data.join("posthaze").join("logs"));

        // Without LocalAppData -> falls back to Home/.local/share/posthaze.
        let home_fallback = resolve_data_dir(Some(&exe_dir), None, Some(home.as_os_str()));
        assert_eq!(
            home_fallback,
            home.join(".local").join("share").join("posthaze")
        );

        // With posthaze.portable marker beside executable -> uses exe_dir/data.
        std::fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").unwrap();
        let portable = resolve_data_dir(
            Some(&exe_dir),
            Some(local_app_data.as_os_str()),
            Some(home.as_os_str()),
        );
        assert_eq!(portable, exe_dir.join("data"));
        assert_eq!(portable.join("state.db"), exe_dir.join("data").join("state.db"));
        assert_eq!(portable.join("logs"), exe_dir.join("data").join("logs"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parse_expected_checksum_handles_prefixed_bsd_and_bare_inputs() {
        let sha256 = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert_eq!(
            parse_expected_checksum(&format!("sha256:{sha256}")),
            Some((Some(ChecksumAlgorithm::Sha256), sha256.to_string()))
        );
        assert_eq!(
            parse_expected_checksum(&format!("BLAKE3 (archive.zip) = {}", sha256.to_uppercase())),
            Some((Some(ChecksumAlgorithm::Blake3), sha256.to_string()))
        );
        assert_eq!(
            parse_expected_checksum(&format!("{sha256}  archive.zip")),
            Some((None, sha256.to_string()))
        );
        assert_eq!(
            parse_expected_checksum("0D4A1185"),
            Some((Some(ChecksumAlgorithm::Crc32), "0d4a1185".to_string()))
        );
        assert_eq!(parse_expected_checksum("not-valid-hex"), None);
    }

    #[test]
    fn canonical_filename_and_disambiguate_handle_numeric_suffixes_cleanly() {
        assert_eq!(
            canonical_filename_key("Mushoku Tensei - S03E14 (1).mkv"),
            "mushoku tensei - s03e14.mkv"
        );
        assert_eq!(
            canonical_filename_key("episode (1080p).mkv"),
            "episode (1080p).mkv"
        );

        let dir = PathBuf::from(r"E:\__nonexistent_test_dir__");
        let reserved = vec![dir.join("pack.zip"), dir.join("pack (1).zip")];
        let next = disambiguate_output_path_with_reserved(&dir, "pack (1).zip", &reserved);
        assert_eq!(next, dir.join("pack (2).zip"));
    }
}
