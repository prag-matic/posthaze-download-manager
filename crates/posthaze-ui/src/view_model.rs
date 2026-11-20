//! Pure view-model logic for filtering, sorting, FIFO rank calculation, and formatting.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::assets::IconName;
use posthaze_engine::{
    DownloadId, DownloadRecord, DownloadState, EngineEvent, FailureKind, ProgressSnapshot,
    SegmentProgress, Settings,
};

use crate::bridge::EngineSnapshot;

/// Default width (in logical pixels) of the left filter & telemetry rail.
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 220.0;
/// Minimum allowed expanded width (in logical pixels) of the left filter & telemetry rail.
pub const MIN_SIDEBAR_WIDTH: f32 = 180.0;
/// Maximum allowed expanded width (in logical pixels) of the left filter & telemetry rail.
pub const MAX_SIDEBAR_WIDTH: f32 = 380.0;
/// Width (in logical pixels) of the collapsed left icon rail.
pub const RAIL_SIDEBAR_WIDTH: f32 = 52.0;
/// Dragging narrower than this threshold snaps the sidebar into the icon rail.
pub const SIDEBAR_RAIL_SNAP_THRESHOLD: f32 = 130.0;
/// Magnetic snap radius (in logical pixels) around canonical sidebar landmarks.
pub const SIDEBAR_MAGNETIC_SNAP_RADIUS: f32 = 8.0;

/// Default height (in logical pixels) of the bottom selected-download inspector.
pub const DEFAULT_INSPECTOR_HEIGHT: f32 = 240.0;
/// Minimum allowed expanded height (in logical pixels) of the bottom selected-download inspector.
pub const MIN_INSPECTOR_HEIGHT: f32 = 150.0;
/// Maximum allowed height (in logical pixels) of the bottom selected-download inspector.
pub const MAX_INSPECTOR_HEIGHT: f32 = 460.0;
/// Height (in logical pixels) of the collapsed bottom inspector summary strip.
pub const COLLAPSED_INSPECTOR_HEIGHT: f32 = 28.0;
/// Dragging shorter than this threshold snaps the inspector to its collapsed strip.
pub const INSPECTOR_COLLAPSE_SNAP_THRESHOLD: f32 = 90.0;
/// Magnetic snap radius (in logical pixels) around canonical inspector landmarks.
pub const INSPECTOR_MAGNETIC_SNAP_RADIUS: f32 = 8.0;

/// Snaps `value` to `target` when within `radius`, returning `(snapped_value, did_snap)`.
pub fn apply_magnetic_detent(value: f32, target: f32, radius: f32) -> (f32, bool) {
    if (value - target).abs() <= radius {
        (target, true)
    } else {
        (value, false)
    }
}

/// Clamps the left sidebar width to `[MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH]`.
pub fn clamp_sidebar_width(width: f32) -> f32 {
    width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
}

/// Resolves `(collapsed, expanded_width)` while dragging the left sidebar divider.
/// Dragging below [`SIDEBAR_RAIL_SNAP_THRESHOLD`] snaps to the icon rail while
/// preserving `previous_expanded_width`.
pub fn resolve_sidebar_drag(dragged_width: f32, previous_expanded_width: f32) -> (bool, f32) {
    if dragged_width < SIDEBAR_RAIL_SNAP_THRESHOLD {
        (true, clamp_sidebar_width(previous_expanded_width))
    } else {
        (false, clamp_sidebar_width(dragged_width))
    }
}

/// Clamps the bottom inspector height to `[MIN_INSPECTOR_HEIGHT, MAX_INSPECTOR_HEIGHT]`.
pub fn clamp_inspector_height(height: f32) -> f32 {
    height.clamp(MIN_INSPECTOR_HEIGHT, MAX_INSPECTOR_HEIGHT)
}

/// Resolves `(collapsed, expanded_height)` while dragging the bottom inspector divider.
/// Dragging below [`INSPECTOR_COLLAPSE_SNAP_THRESHOLD`] snaps to the collapsed strip while
/// preserving `previous_expanded_height`.
pub fn resolve_inspector_drag(dragged_height: f32, previous_expanded_height: f32) -> (bool, f32) {
    if dragged_height < INSPECTOR_COLLAPSE_SNAP_THRESHOLD {
        (true, clamp_inspector_height(previous_expanded_height))
    } else {
        (false, clamp_inspector_height(dragged_height))
    }
}

/// Adaptive layout breakpoints and clamped inspector height for the current right-column size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutProfile {
    pub show_speed: bool,
    pub show_eta: bool,
    pub show_segs: bool,
    pub compact_toolbar: bool,
    pub progress_col_width: f32,
    pub effective_inspector_height: f32,
}

/// Computes adaptive table/toolbar visibility and viewport-safe inspector height.
pub fn compute_layout_profile(
    right_width: f32,
    viewport_height: f32,
    inspector_height: f32,
) -> LayoutProfile {
    let show_speed = right_width >= 660.0;
    let show_eta = right_width >= 820.0;
    let show_segs = right_width >= 760.0;
    let compact_toolbar = right_width < 740.0;
    let progress_col_width = if right_width < 700.0 { 192.0 } else { 210.0 };
    let max_for_viewport = (viewport_height - 250.0).max(MIN_INSPECTOR_HEIGHT);
    let effective_inspector_height = clamp_inspector_height(inspector_height).min(max_for_viewport);
    LayoutProfile {
        show_speed,
        show_eta,
        show_segs,
        compact_toolbar,
        progress_col_width,
        effective_inspector_height,
    }
}

/// Sidebar queue filter categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterTab {
    All,
    Active,
    Queued,
    Paused,
    Completed,
    Failed,
}

impl FilterTab {
    /// Human-readable label for the left filter rail.
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All Transfers",
            Self::Active => "Active",
            Self::Queued => "Queued",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    /// Returns whether `state` matches this filter tab.
    pub fn matches(self, state: DownloadState) -> bool {
        match self {
            Self::All => true,
            Self::Active => matches!(state, DownloadState::Active | DownloadState::Probing),
            Self::Queued => state == DownloadState::Queued,
            Self::Paused => state == DownloadState::Paused,
            Self::Completed => state == DownloadState::Completed,
            Self::Failed => state == DownloadState::Failed,
        }
    }
}

/// Sortable columns in the `Downloads` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    State,
    Filename,
    Progress,
    Speed,
    CreatedAt,
}

/// Sort direction for the `Downloads` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    /// Flips ascending <-> descending.
    pub fn toggle(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// Merged persisted + live telemetry state for a single `Download` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRowView {
    pub id: DownloadId,
    pub url: String,
    pub final_url: Option<String>,
    pub filename: String,
    pub output_path: Option<PathBuf>,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub checkpointed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_sec: u64,
    pub eta_secs: Option<u64>,
    pub active_workers: usize,
    pub segment_count: usize,
    pub supports_ranges: bool,
    pub sync_on_complete: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub error_message: Option<String>,
    pub is_resource_changed: bool,
    pub fifo_rank: Option<usize>,
    pub created_at: i64,
}

impl DownloadRowView {
    /// Builds a row view from a persisted [`DownloadRecord`], optional live [`ProgressSnapshot`],
    /// and optional persisted [`SegmentProgress`] slice.
    pub fn from_record(
        record: &DownloadRecord,
        live: Option<&ProgressSnapshot>,
        persisted_segments: Option<&[SegmentProgress]>,
        fifo_rank: Option<usize>,
    ) -> Self {
        let state = live.map_or(record.state, |s| s.state);
        let bytes_downloaded = live.map_or(record.bytes_downloaded, |s| s.bytes_downloaded);
        let total_bytes = live.and_then(|s| s.total_bytes).or(record.content_length);
        let bytes_per_sec = live.map_or(0, |s| s.bytes_per_sec);
        let eta_secs = live.and_then(|s| s.eta_secs);
        let active_workers = live.map_or(0, |s| s.active_workers);
        let segments: &[SegmentProgress] = live
            .filter(|s| !s.segments.is_empty())
            .map_or_else(|| persisted_segments.unwrap_or(&[]), |s| s.segments.as_slice());

        let checkpointed_bytes = if state == DownloadState::Completed {
            bytes_downloaded
        } else if !segments.is_empty() {
            segments
                .iter()
                .map(|s| s.checkpointed_cursor.saturating_sub(s.start))
                .sum()
        } else {
            record.bytes_downloaded.min(bytes_downloaded)
        };

        let filename = derive_display_filename(record);
        let is_resource_changed = state == DownloadState::Failed
            && record.failure_kind() == Some(FailureKind::ResourceChanged);

        Self {
            id: record.id.clone(),
            url: record.url.clone(),
            final_url: record.final_url.clone(),
            filename,
            output_path: record.output_path.clone(),
            state,
            bytes_downloaded,
            checkpointed_bytes,
            total_bytes,
            bytes_per_sec,
            eta_secs,
            active_workers,
            segment_count: segments.len(),
            supports_ranges: record.supports_ranges,
            sync_on_complete: record.sync_on_complete,
            etag: record.etag.clone(),
            last_modified: record.last_modified.clone(),
            error_message: record.error_message.clone(),
            is_resource_changed,
            fifo_rank,
            created_at: record.created_at,
        }
    }

    /// Fraction `[0.0, 1.0]` of writer-acknowledged `Checkpoint` bytes.
    pub fn checkpoint_ratio(&self) -> f32 {
        match self.total_bytes {
            Some(total) if total > 0 => {
                (self.checkpointed_bytes as f64 / total as f64).clamp(0.0, 1.0) as f32
            }
            _ if self.state == DownloadState::Completed => 1.0,
            _ => 0.0,
        }
    }

    /// Fraction `[0.0, 1.0]` of total received bytes (checkpointed + in-flight).
    pub fn total_ratio(&self) -> f32 {
        match self.total_bytes {
            Some(total) if total > 0 => {
                (self.bytes_downloaded as f64 / total as f64).clamp(0.0, 1.0) as f32
            }
            _ if self.state == DownloadState::Completed => 1.0,
            _ => 0.0,
        }
    }
}

/// Derives a display filename from `output_path` or the URL path.
pub fn derive_display_filename(record: &DownloadRecord) -> String {
    if let Some(name) = record
        .output_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
    {
        return name.to_string();
    }
    let clean = record
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or(&record.url);
    clean
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download.bin")
        .to_string()
}

/// Computes 1-based FIFO ordinal rank (`#1`, `#2`, ...) for `Queued` `Download`s.
pub fn compute_fifo_ranks(
    records: &[DownloadRecord],
    live_map: &HashMap<DownloadId, ProgressSnapshot>,
) -> HashMap<DownloadId, usize> {
    let mut queued: Vec<&DownloadRecord> = records
        .iter()
        .filter(|rec| {
            let st = live_map.get(&rec.id).map_or(rec.state, |s| s.state);
            st == DownloadState::Queued
        })
        .collect();
    // `list_downloads()` is newest-first; oldest-first is FIFO admission order.
    queued.sort_by_key(|rec| rec.created_at);
    queued
        .into_iter()
        .enumerate()
        .map(|(idx, rec)| (rec.id.clone(), idx + 1))
        .collect()
}

/// Lucide state icon and short label for a [`DownloadState`].
pub fn state_icon(state: DownloadState) -> (IconName, &'static str) {
    match state {
        DownloadState::Queued => (IconName::Clock, "QUEUED"),
        DownloadState::Probing => (IconName::LoaderCircle, "PROBING"),
        DownloadState::Active => (IconName::ArrowDown, "ACTIVE"),
        DownloadState::Paused => (IconName::Pause, "PAUSED"),
        DownloadState::Completed => (IconName::CircleCheck, "DONE"),
        DownloadState::Failed => (IconName::TriangleAlert, "FAILED"),
        DownloadState::Cancelled => (IconName::CircleX, "CANCELLED"),
    }
}

/// Formats a byte count using binary IEC units (`B`, `KiB`, `MiB`, `GiB`).
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Formats throughput in `MiB/s` or `KiB/s`.
pub fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "—".to_string();
    }
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bps = bytes_per_sec as f64;
    if bps >= MIB {
        format!("{:.1} MiB/s", bps / MIB)
    } else if bps >= KIB {
        format!("{:.0} KiB/s", bps / KIB)
    } else {
        format!("{bytes_per_sec} B/s")
    }
}

/// Formats remaining seconds into `HH:MM:SS` or `MM:SS`.
pub fn format_eta(eta_secs: Option<u64>) -> String {
    let Some(secs) = eta_secs else {
        return "—".to_string();
    };
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem = secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{rem:02}")
    } else {
        format!("{mins:02}:{rem:02}")
    }
}

/// Filters and sorts [`DownloadRowView`] items for the center table.
pub fn filter_and_sort_rows(
    mut rows: Vec<DownloadRowView>,
    filter: FilterTab,
    sort_col: SortColumn,
    sort_dir: SortDirection,
) -> Vec<DownloadRowView> {
    rows.retain(|r| filter.matches(r.state));
    rows.sort_by(|a, b| {
        let ord = match sort_col {
            SortColumn::State => a.state.as_str().cmp(b.state.as_str()),
            SortColumn::Filename => a
                .filename
                .to_ascii_lowercase()
                .cmp(&b.filename.to_ascii_lowercase()),
            SortColumn::Progress => a
                .total_ratio()
                .partial_cmp(&b.total_ratio())
                .unwrap_or(std::cmp::Ordering::Equal),
            SortColumn::Speed => a.bytes_per_sec.cmp(&b.bytes_per_sec),
            SortColumn::CreatedAt => a.created_at.cmp(&b.created_at),
        };
        match sort_dir {
            SortDirection::Ascending => ord,
            SortDirection::Descending => ord.reverse(),
        }
    });
    rows
}

/// Normalized geometry for rendering a `Segment` inside the **Graduated Interval Byte-Map Ruler**.
#[derive(Debug, Clone, PartialEq)]
pub struct ByteMapSegment {
    pub id: usize,
    pub start: u64,
    pub end: Option<u64>,
    pub width_ratio: f32,
    pub checkpoint_fill_ratio: f32,
    pub inflight_fill_ratio: f32,
    pub completed: bool,
}

/// Normalizes `SegmentProgress` entries into proportional `[0.0, 1.0]` bar widths and dual-tone fills.
pub fn normalize_byte_map_segments(
    segments: &[SegmentProgress],
    total_bytes: Option<u64>,
    state: DownloadState,
) -> Vec<ByteMapSegment> {
    if total_bytes == Some(0) || (segments.is_empty() && state == DownloadState::Completed) {
        let done = state == DownloadState::Completed;
        return vec![ByteMapSegment {
            id: 0,
            start: 0,
            end: total_bytes,
            width_ratio: 1.0,
            checkpoint_fill_ratio: if done { 1.0 } else { 0.0 },
            inflight_fill_ratio: 0.0,
            completed: done,
        }];
    }

    if segments.is_empty() {
        return Vec::new();
    }

    let mut ordered = segments.to_vec();
    ordered.sort_by_key(|s| s.start);

    if let Some(total) = total_bytes
        && total > 0
        && ordered.iter().all(|s| s.end.is_some())
    {
        return ordered
            .into_iter()
            .map(|seg| {
                let end = seg.end.unwrap_or(total);
                let raw_span = end.saturating_sub(seg.start);
                let span = raw_span.max(1);
                let cp_bytes = if seg.completed || state == DownloadState::Completed {
                    raw_span
                } else {
                    seg.checkpointed_cursor.saturating_sub(seg.start).min(raw_span)
                };
                let cur_bytes = if seg.completed || state == DownloadState::Completed {
                    raw_span
                } else {
                    seg.cursor
                        .saturating_sub(seg.start)
                        .min(raw_span)
                        .max(cp_bytes)
                };
                ByteMapSegment {
                    id: seg.id,
                    start: seg.start,
                    end: Some(end),
                    width_ratio: (raw_span as f64 / total as f64).clamp(0.0, 1.0) as f32,
                    checkpoint_fill_ratio: (cp_bytes as f64 / span as f64).clamp(0.0, 1.0) as f32,
                    inflight_fill_ratio: ((cur_bytes - cp_bytes) as f64 / span as f64)
                        .clamp(0.0, 1.0) as f32,
                    completed: seg.completed || state == DownloadState::Completed,
                }
            })
            .collect();
    }

    // Unknown total size / single-stream mode.
    let seg = &ordered[0];
    let denom = seg.cursor.max(1);
    let cp = seg.checkpointed_cursor.min(seg.cursor);
    let inflight = seg.cursor.saturating_sub(cp);
    vec![ByteMapSegment {
        id: seg.id,
        start: seg.start,
        end: seg.end,
        width_ratio: 1.0,
        checkpoint_fill_ratio: (cp as f64 / denom as f64).clamp(0.0, 1.0) as f32,
        inflight_fill_ratio: (inflight as f64 / denom as f64).clamp(0.0, 1.0) as f32,
        completed: seg.completed || state == DownloadState::Completed,
    }]
}

/// Validates and parses user-edited `Settings` form fields.
#[cfg(test)]
pub fn parse_settings_inputs(
    default_output_dir: &str,
    max_workers_str: &str,
    max_concurrent_str: &str,
    sync_on_complete: bool,
) -> Result<Settings, String> {
    let dir = default_output_dir.trim();
    if dir.is_empty() {
        return Err("Default output directory cannot be empty.".to_string());
    }
    let max_workers: usize = max_workers_str
        .trim()
        .parse()
        .map_err(|_| "Max workers must be a number between 1 and 32.".to_string())?;
    if !(1..=32).contains(&max_workers) {
        return Err("Max workers must be between 1 and 32.".to_string());
    }
    let max_concurrent_downloads: usize = max_concurrent_str
        .trim()
        .parse()
        .map_err(|_| "Max concurrent downloads must be at least 1.".to_string())?;
    if max_concurrent_downloads == 0 {
        return Err("Max concurrent downloads must be at least 1.".to_string());
    }
    Ok(Settings {
        default_output_dir: PathBuf::from(dir),
        max_workers,
        max_concurrent_downloads,
        sync_on_complete,
        ..Settings::default()
    })
}

/// Modal/focus context used to resolve global keyboard shortcut precedence.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardContext {
    pub has_confirm_modal: bool,
    pub has_preflight_resolved: bool,
    pub has_any_preflight: bool,
    pub has_settings_modal: bool,
    pub has_file_op_modal: bool,
    pub url_input_focused: bool,
}

/// High-level action resolved from a key press and UI overlay/focus state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardAction {
    DismissOverlays,
    ConfirmActiveModal,
    StepSelection(isize),
    TriggerPrimaryAction,
    RequestDeleteSelected,
    None,
}

/// Resolves global keyboard shortcut precedence (`Escape` > `Enter` > focus guard > list navigation).
pub fn resolve_keyboard_action(key: &str, ctx: KeyboardContext) -> KeyboardAction {
    if key == "escape" {
        return KeyboardAction::DismissOverlays;
    }
    if key == "enter"
        && (ctx.has_confirm_modal
            || ctx.has_preflight_resolved
            || ctx.has_settings_modal
            || ctx.has_file_op_modal)
    {
        return KeyboardAction::ConfirmActiveModal;
    }
    if ctx.has_confirm_modal
        || ctx.has_any_preflight
        || ctx.has_settings_modal
        || ctx.has_file_op_modal
        || ctx.url_input_focused
    {
        return KeyboardAction::None;
    }
    match key {
        "up" => KeyboardAction::StepSelection(-1),
        "down" => KeyboardAction::StepSelection(1),
        "space" => KeyboardAction::TriggerPrimaryAction,
        "delete" => KeyboardAction::RequestDeleteSelected,
        _ => KeyboardAction::None,
    }
}

/// Primary action triggered by Space or a row's primary button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryDownloadAction {
    Pause(DownloadId),
    Resume(DownloadId),
    RestartFresh(DownloadId),
}

/// Side effects emitted by [`QueueController`] for the GPUI shell to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEffect {
    RefreshFromEngine,
    ResumeDownload(DownloadId),
    ShowToast { title: String, body: String },
    RunCompletionHook { path: PathBuf },
    LoadSegments(DownloadId),
}

/// Headless state and decision controller for the `Download` `Queue` UI.
#[derive(Debug, Default)]
pub struct QueueController {
    pub records: Vec<DownloadRecord>,
    pub live_map: HashMap<DownloadId, ProgressSnapshot>,
    pub persisted_segments: HashMap<DownloadId, Vec<SegmentProgress>>,
    pub settings: Settings,
    pub selected_id: Option<DownloadId>,
    pub auto_retries_used: HashMap<DownloadId, u32>,
}

impl QueueController {
    /// Applies a fresh [`EngineSnapshot`] and returns an optional segment-load effect.
    pub fn apply_snapshot(&mut self, snapshot: EngineSnapshot) -> Option<UiEffect> {
        self.records = snapshot.records;
        self.settings = snapshot.settings;
        self.persisted_segments = snapshot.segments;
        if self.selected_id.is_none() {
            self.selected_id = self.records.first().map(|r| r.id.clone());
            return self.selected_id.clone().map(UiEffect::LoadSegments);
        }
        None
    }

    /// Processes an [`EngineEvent`] and returns any side effects for the UI shell.
    pub fn on_engine_event(&mut self, event: EngineEvent, window_active: bool) -> Vec<UiEffect> {
        let mut effects = Vec::new();
        match event {
            EngineEvent::ProgressUpdated { id, snapshot } => {
                self.live_map.insert(id, snapshot);
            }
            EngineEvent::StateChanged {
                id,
                state,
                error: _,
                failure_kind,
            } => {
                if state != DownloadState::Active {
                    self.live_map.remove(&id);
                }
                let rec = self.records.iter().find(|r| r.id == id).cloned();
                if state == DownloadState::Completed {
                    self.auto_retries_used.remove(&id);
                    if let Some(ref r) = rec {
                        let name = derive_display_filename(r);
                        if self.settings.notify_on_completed
                            && (!self.settings.notify_only_inactive || !window_active)
                        {
                            effects.push(UiEffect::ShowToast {
                                title: "Posthaze — Download Completed".to_string(),
                                body: name,
                            });
                        }
                        if self.settings.completion_cmd_enabled
                            && let Some(ref out_path) = r.output_path
                        {
                            effects.push(UiEffect::RunCompletionHook {
                                path: out_path.clone(),
                            });
                        }
                    }
                } else if state == DownloadState::Failed {
                    let is_resource_changed =
                        failure_kind == Some(FailureKind::ResourceChanged);
                    let used = self.auto_retries_used.get(&id).copied().unwrap_or(0);
                    if !is_resource_changed
                        && failure_kind != Some(FailureKind::NonResumable)
                        && used < self.settings.auto_retry_count
                    {
                        self.auto_retries_used.insert(id.clone(), used + 1);
                        effects.push(UiEffect::ResumeDownload(id));
                    } else if self.settings.notify_on_failed
                        && (!self.settings.notify_only_inactive || !window_active)
                    {
                        let name = rec
                            .as_ref()
                            .map(derive_display_filename)
                            .unwrap_or_else(|| "Download".to_string());
                        effects.push(UiEffect::ShowToast {
                            title: "Posthaze — Download Failed".to_string(),
                            body: name,
                        });
                    }
                }
                effects.push(UiEffect::RefreshFromEngine);
            }
            EngineEvent::QueueUpdated => {
                effects.push(UiEffect::RefreshFromEngine);
            }
            EngineEvent::SettingsUpdated(settings) => {
                self.settings = settings;
            }
        }
        effects
    }

    /// Returns the primary action (`Pause`, `Resume`, or `RestartFresh`) for `id`.
    pub fn primary_action_for(&self, id: &DownloadId) -> Option<PrimaryDownloadAction> {
        let rec = self.records.iter().find(|r| &r.id == id)?;
        let state = self.live_map.get(id).map_or(rec.state, |s| s.state);
        match state {
            DownloadState::Active | DownloadState::Queued | DownloadState::Probing => {
                Some(PrimaryDownloadAction::Pause(id.clone()))
            }
            DownloadState::Paused => Some(PrimaryDownloadAction::Resume(id.clone())),
            DownloadState::Failed => {
                if rec.failure_kind() == Some(FailureKind::ResourceChanged) {
                    Some(PrimaryDownloadAction::RestartFresh(id.clone()))
                } else {
                    Some(PrimaryDownloadAction::Resume(id.clone()))
                }
            }
            _ => None,
        }
    }

    /// Moves selection up (`delta < 0`) or down (`delta > 0`) within `visible_ids`.
    pub fn step_selection(
        &mut self,
        visible_ids: &[DownloadId],
        delta: isize,
    ) -> Option<DownloadId> {
        if visible_ids.is_empty() {
            return None;
        }
        let current_idx = self
            .selected_id
            .as_ref()
            .and_then(|id| visible_ids.iter().position(|v| v == id));
        let next_idx = match (delta < 0, current_idx) {
            (true, Some(0)) | (true, None) => 0,
            (true, Some(i)) => i - 1,
            (false, None) => 0,
            (false, Some(i)) => (i + 1).min(visible_ids.len() - 1),
        };
        let next_id = visible_ids[next_idx].clone();
        self.selected_id = Some(next_id.clone());
        Some(next_id)
    }

    /// Returns IDs of all `Paused` or `Failed` `Download`s for `Resume All`.
    pub fn resume_all_ids(&self) -> Vec<DownloadId> {
        self.records
            .iter()
            .filter(|r| matches!(r.state, DownloadState::Paused | DownloadState::Failed))
            .map(|r| r.id.clone())
            .collect()
    }

    /// Returns IDs of all `Active`, `Queued`, or `Probing` `Download`s for `Pause All`.
    pub fn pause_all_ids(&self) -> Vec<DownloadId> {
        self.records
            .iter()
            .filter(|r| {
                matches!(
                    self.live_map.get(&r.id).map_or(r.state, |s| s.state),
                    DownloadState::Active | DownloadState::Queued | DownloadState::Probing
                )
            })
            .map(|r| r.id.clone())
            .collect()
    }

    /// Returns IDs of all `Completed` or `Cancelled` `Download`s for `Clear Completed`.
    pub fn clear_completed_ids(&self) -> Vec<DownloadId> {
        self.records
            .iter()
            .filter(|r| matches!(r.state, DownloadState::Completed | DownloadState::Cancelled))
            .map(|r| r.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(id: &str, url: &str, state: DownloadState, created_at: i64) -> DownloadRecord {
        DownloadRecord {
            id: DownloadId::from(id.to_string()),
            url: url.to_string(),
            final_url: None,
            output_dir: None,
            output_path: None,
            part_path: None,
            state,
            content_length: Some(1000),
            supports_ranges: true,
            etag: None,
            last_modified: None,
            max_workers: 8,
            sync_on_complete: false,
            headers: Vec::new(),
            bytes_downloaded: 250,
            error_message: None,
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn formats_bytes_speed_and_eta() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GiB");

        assert_eq!(format_speed(0), "—");
        assert_eq!(format_speed(512 * 1024), "512 KiB/s");
        assert_eq!(format_speed(10 * 1024 * 1024), "10.0 MiB/s");

        assert_eq!(format_eta(None), "—");
        assert_eq!(format_eta(Some(65)), "01:05");
        assert_eq!(format_eta(Some(3661)), "01:01:01");
    }

    #[test]
    fn computes_fifo_ranks_oldest_first() {
        let records = vec![
            make_record("newest", "https://example.com/c.bin", DownloadState::Queued, 300),
            make_record("active", "https://example.com/a.bin", DownloadState::Active, 100),
            make_record("oldest", "https://example.com/b.bin", DownloadState::Queued, 200),
        ];
        let ranks = compute_fifo_ranks(&records, &HashMap::new());
        assert_eq!(ranks.get(&DownloadId::from("oldest".to_string())), Some(&1));
        assert_eq!(ranks.get(&DownloadId::from("newest".to_string())), Some(&2));
        assert!(!ranks.contains_key(&DownloadId::from("active".to_string())));
    }

    #[test]
    fn dual_tone_progress_ratios_distinguish_checkpoint_from_inflight() {
        let rec = make_record("id1", "https://example.com/model.gguf", DownloadState::Active, 1);
        let snap = ProgressSnapshot {
            state: DownloadState::Active,
            bytes_downloaded: 750,
            total_bytes: Some(1000),
            active_workers: 2,
            bytes_per_sec: 1024,
            eta_secs: Some(1),
            segments: vec![
                SegmentProgress {
                    id: 0,
                    start: 0,
                    cursor: 400,
                    checkpointed_cursor: 250,
                    end: Some(500),
                    completed: false,
                },
                SegmentProgress {
                    id: 1,
                    start: 500,
                    cursor: 850,
                    checkpointed_cursor: 750,
                    end: Some(1000),
                    completed: false,
                },
            ],
        };
        let row = DownloadRowView::from_record(&rec, Some(&snap), None, None);
        assert!((row.checkpoint_ratio() - 0.5).abs() < 1e-6);
        assert!((row.total_ratio() - 0.75).abs() < 1e-6);
        assert_eq!(row.segment_count, 2);
    }

    #[test]
    fn filters_and_sorts_rows() {
        let r1 = DownloadRowView::from_record(
            &make_record("1", "https://example.com/zeta.bin", DownloadState::Paused, 10),
            None,
            None,
            None,
        );
        let r2 = DownloadRowView::from_record(
            &make_record("2", "https://example.com/alpha.bin", DownloadState::Queued, 20),
            None,
            None,
            Some(1),
        );
        let filtered = filter_and_sort_rows(
            vec![r1.clone(), r2.clone()],
            FilterTab::Queued,
            SortColumn::CreatedAt,
            SortDirection::Descending,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].filename, "alpha.bin");

        let sorted = filter_and_sort_rows(
            vec![r1, r2],
            FilterTab::All,
            SortColumn::Filename,
            SortDirection::Ascending,
        );
        assert_eq!(sorted[0].filename, "alpha.bin");
        assert_eq!(sorted[1].filename, "zeta.bin");
    }

    #[test]
    fn normalizes_byte_map_segments_across_edge_cases() {
        let zero = normalize_byte_map_segments(&[], Some(0), DownloadState::Completed);
        assert_eq!(zero.len(), 1);
        assert!((zero[0].width_ratio - 1.0).abs() < 1e-6);
        assert!((zero[0].checkpoint_fill_ratio - 1.0).abs() < 1e-6);

        let stream = vec![SegmentProgress {
            id: 0,
            start: 0,
            cursor: 4096,
            checkpointed_cursor: 2048,
            end: None,
            completed: false,
        }];
        let unknown = normalize_byte_map_segments(&stream, None, DownloadState::Active);
        assert_eq!(unknown.len(), 1);
        assert!((unknown[0].checkpoint_fill_ratio - 0.5).abs() < 1e-6);
        assert!((unknown[0].inflight_fill_ratio - 0.5).abs() < 1e-6);

        let seg_size = 1024 * 1024u64;
        let segs_32: Vec<SegmentProgress> = (0..32)
            .map(|i| SegmentProgress {
                id: i,
                start: i as u64 * seg_size,
                cursor: i as u64 * seg_size + seg_size / 2,
                checkpointed_cursor: i as u64 * seg_size + seg_size / 4,
                end: Some((i as u64 + 1) * seg_size),
                completed: false,
            })
            .collect();
        let norm_32 = normalize_byte_map_segments(
            &segs_32,
            Some(32 * seg_size),
            DownloadState::Active,
        );
        assert_eq!(norm_32.len(), 32);
        assert!((norm_32[0].width_ratio - (1.0 / 32.0)).abs() < 1e-5);
        assert!((norm_32[0].checkpoint_fill_ratio - 0.25).abs() < 1e-5);
        assert!((norm_32[0].inflight_fill_ratio - 0.25).abs() < 1e-5);
    }

    #[test]
    fn validates_settings_form_bounds() {
        assert!(parse_settings_inputs("C:\\Downloads", "8", "3", true).is_ok());
        assert!(parse_settings_inputs("", "8", "3", false).is_err());
        assert!(parse_settings_inputs("C:\\Downloads", "0", "3", false).is_err());
        assert!(parse_settings_inputs("C:\\Downloads", "8", "0", false).is_err());
        assert!(parse_settings_inputs("C:\\Downloads", "64", "3", false).is_err());
    }

    #[test]
    fn detects_resource_changed_error_for_errata_recovery() {
        let mut rec = make_record("rc", "https://example.com/fw.bin", DownloadState::Failed, 1);
        rec.error_message = Some("remote resource changed: ETag mismatch".to_string());
        let row = DownloadRowView::from_record(&rec, None, None, None);
        assert!(row.is_resource_changed);

        rec.error_message = Some("HTTP error: timeout".to_string());
        let transient_row = DownloadRowView::from_record(&rec, None, None, None);
        assert!(!transient_row.is_resource_changed);
        assert_eq!(
            transient_row.error_message.as_deref(),
            Some("HTTP error: timeout")
        );
    }

    #[test]
    fn preserves_persisted_segment_count_when_live_snapshot_absent() {
        let persisted = vec![
            SegmentProgress {
                id: 0,
                start: 0,
                cursor: 500,
                checkpointed_cursor: 500,
                end: Some(500),
                completed: true,
            },
            SegmentProgress {
                id: 1,
                start: 500,
                cursor: 750,
                checkpointed_cursor: 750,
                end: Some(1000),
                completed: false,
            },
        ];
        for state in [
            DownloadState::Paused,
            DownloadState::Failed,
            DownloadState::Completed,
        ] {
            let rec = make_record("s", "https://example.com/archive.bin", state, 1);
            let row = DownloadRowView::from_record(&rec, None, Some(&persisted), None);
            assert_eq!(row.segment_count, 2);
        }
    }

    #[test]
    fn maps_every_download_state_to_lucide_icon() {
        assert_eq!(state_icon(DownloadState::Queued).1, "QUEUED");
        assert_eq!(state_icon(DownloadState::Probing).1, "PROBING");
        assert_eq!(state_icon(DownloadState::Active).1, "ACTIVE");
        assert_eq!(state_icon(DownloadState::Paused).1, "PAUSED");
        assert_eq!(state_icon(DownloadState::Completed).1, "DONE");
        assert_eq!(state_icon(DownloadState::Failed).1, "FAILED");
        assert_eq!(state_icon(DownloadState::Cancelled).1, "CANCELLED");
    }

    #[test]
    fn clamps_panel_dimensions_within_bounds() {
        assert_eq!(clamp_sidebar_width(50.0), MIN_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(260.0), 260.0);
        assert_eq!(clamp_sidebar_width(900.0), MAX_SIDEBAR_WIDTH);

        assert_eq!(clamp_inspector_height(40.0), MIN_INSPECTOR_HEIGHT);
        assert_eq!(clamp_inspector_height(310.0), 310.0);
        assert_eq!(clamp_inspector_height(800.0), MAX_INSPECTOR_HEIGHT);

        assert_eq!(resolve_sidebar_drag(110.0, 260.0), (true, 260.0));
        assert_eq!(resolve_sidebar_drag(130.0, 260.0), (false, MIN_SIDEBAR_WIDTH));
        assert_eq!(resolve_sidebar_drag(250.0, 220.0), (false, 250.0));

        assert_eq!(resolve_inspector_drag(60.0, 280.0), (true, 280.0));
        assert_eq!(resolve_inspector_drag(90.0, 280.0), (false, MIN_INSPECTOR_HEIGHT));
        assert_eq!(resolve_inspector_drag(320.0, 240.0), (false, 320.0));

        assert_eq!(apply_magnetic_detent(225.0, 220.0, 8.0), (220.0, true));
        assert_eq!(apply_magnetic_detent(212.0, 220.0, 8.0), (220.0, true));
        assert_eq!(apply_magnetic_detent(235.0, 220.0, 8.0), (235.0, false));
    }

    #[test]
    fn computes_adaptive_layout_profile_breakpoints() {
        let wide = compute_layout_profile(960.0, 640.0, 240.0);
        assert!(wide.show_speed);
        assert!(wide.show_eta);
        assert!(wide.show_segs);
        assert!(!wide.compact_toolbar);
        assert_eq!(wide.progress_col_width, 210.0);
        assert_eq!(wide.effective_inspector_height, 240.0);

        let mid = compute_layout_profile(790.0, 640.0, 460.0);
        assert!(mid.show_speed);
        assert!(!mid.show_eta);
        assert!(mid.show_segs);
        assert!(!mid.compact_toolbar);
        assert_eq!(mid.effective_inspector_height, 390.0);

        let narrow = compute_layout_profile(630.0, 640.0, 240.0);
        assert!(!narrow.show_speed);
        assert!(!narrow.show_eta);
        assert!(!narrow.show_segs);
        assert!(narrow.compact_toolbar);
        assert_eq!(narrow.progress_col_width, 192.0);
    }

    #[test]
    fn queue_controller_auto_retries_resumable_failures_and_skips_resource_changed() {
        let mut ctrl = QueueController::default();
        ctrl.settings.auto_retry_count = 1;
        ctrl.settings.notify_on_failed = true;
        ctrl.settings.notify_only_inactive = false;
        let id = DownloadId::from("dl-1".to_string());
        ctrl.records
            .push(make_record("dl-1", "https://example.com/a.bin", DownloadState::Active, 1));

        let effects = ctrl.on_engine_event(
            EngineEvent::StateChanged {
                id: id.clone(),
                state: DownloadState::Failed,
                error: Some("network drop".to_string()),
                failure_kind: Some(FailureKind::Resumable),
            },
            true,
        );
        assert_eq!(
            effects,
            vec![UiEffect::ResumeDownload(id.clone()), UiEffect::RefreshFromEngine]
        );
        assert_eq!(ctrl.auto_retries_used.get(&id), Some(&1));

        // Second failure exhausts retry budget and emits toast instead of resume.
        let effects_exhausted = ctrl.on_engine_event(
            EngineEvent::StateChanged {
                id: id.clone(),
                state: DownloadState::Failed,
                error: Some("network drop".to_string()),
                failure_kind: Some(FailureKind::Resumable),
            },
            true,
        );
        assert!(effects_exhausted.iter().any(|e| matches!(e, UiEffect::ShowToast { .. })));
        assert!(!effects_exhausted.iter().any(|e| matches!(e, UiEffect::ResumeDownload(_))));

        // ResourceChanged never auto-retries even with budget available.
        let id2 = DownloadId::from("dl-2".to_string());
        let effects_rc = ctrl.on_engine_event(
            EngineEvent::StateChanged {
                id: id2.clone(),
                state: DownloadState::Failed,
                error: Some("remote resource changed".to_string()),
                failure_kind: Some(FailureKind::ResourceChanged),
            },
            false,
        );
        assert!(!effects_rc.iter().any(|e| matches!(e, UiEffect::ResumeDownload(_))));
        assert!(effects_rc.iter().any(|e| matches!(e, UiEffect::ShowToast { .. })));

        // NonResumable never auto-retries either.
        let id3 = DownloadId::from("dl-3".to_string());
        let effects_nr = ctrl.on_engine_event(
            EngineEvent::StateChanged {
                id: id3,
                state: DownloadState::Failed,
                error: Some("404 Not Found".to_string()),
                failure_kind: Some(FailureKind::NonResumable),
            },
            false,
        );
        assert!(!effects_nr.iter().any(|e| matches!(e, UiEffect::ResumeDownload(_))));
    }

    #[test]
    fn queue_controller_primary_action_and_keyboard_precedence() {
        let mut ctrl = QueueController::default();
        let id_active = DownloadId::from("a".to_string());
        let id_paused = DownloadId::from("b".to_string());
        let mut rec_rc = make_record("c", "https://example.com/c.bin", DownloadState::Failed, 3);
        rec_rc.error_message = Some("remote resource changed: ETag".to_string());
        let mut rec_resumable_fail =
            make_record("d", "https://example.com/d.bin", DownloadState::Failed, 4);
        rec_resumable_fail.error_message = Some("HTTP error: timeout".to_string());

        ctrl.records = vec![
            make_record("a", "https://example.com/a.bin", DownloadState::Active, 1),
            make_record("b", "https://example.com/b.bin", DownloadState::Paused, 2),
            rec_rc,
            rec_resumable_fail,
        ];
        let id_rc = DownloadId::from("c".to_string());
        let id_resumable_fail = DownloadId::from("d".to_string());

        assert_eq!(
            ctrl.primary_action_for(&id_active),
            Some(PrimaryDownloadAction::Pause(id_active.clone()))
        );
        assert_eq!(
            ctrl.primary_action_for(&id_paused),
            Some(PrimaryDownloadAction::Resume(id_paused.clone()))
        );
        assert_eq!(
            ctrl.primary_action_for(&id_rc),
            Some(PrimaryDownloadAction::RestartFresh(id_rc.clone()))
        );
        assert_eq!(
            ctrl.primary_action_for(&id_resumable_fail),
            Some(PrimaryDownloadAction::Resume(id_resumable_fail))
        );

        let visible = vec![id_active.clone(), id_paused.clone(), id_rc.clone()];
        assert_eq!(ctrl.step_selection(&visible, 1), Some(id_active.clone()));
        assert_eq!(ctrl.step_selection(&visible, 1), Some(id_paused.clone()));
        assert_eq!(ctrl.step_selection(&visible, -1), Some(id_active));

        // Keyboard precedence: Escape > Enter on modal > URL/modal block > list shortcuts.
        let modal_ctx = KeyboardContext {
            has_confirm_modal: true,
            url_input_focused: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_keyboard_action("escape", modal_ctx),
            KeyboardAction::DismissOverlays
        );
        assert_eq!(
            resolve_keyboard_action("enter", modal_ctx),
            KeyboardAction::ConfirmActiveModal
        );
        assert_eq!(
            resolve_keyboard_action("space", modal_ctx),
            KeyboardAction::None
        );

        let list_ctx = KeyboardContext::default();
        assert_eq!(
            resolve_keyboard_action("up", list_ctx),
            KeyboardAction::StepSelection(-1)
        );
        assert_eq!(
            resolve_keyboard_action("space", list_ctx),
            KeyboardAction::TriggerPrimaryAction
        );
        assert_eq!(
            resolve_keyboard_action("delete", list_ctx),
            KeyboardAction::RequestDeleteSelected
        );
    }
}
