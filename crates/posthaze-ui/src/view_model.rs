//! Pure view-model logic for filtering, sorting, FIFO rank calculation, and formatting.

use std::collections::HashMap;
use std::path::PathBuf;

use posthaze_engine::{DownloadId, DownloadRecord, DownloadState, ProgressSnapshot, SegmentProgress};

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
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub error_message: Option<String>,
    pub is_resource_changed: bool,
    pub fifo_rank: Option<usize>,
    pub created_at: i64,
}

impl DownloadRowView {
    /// Builds a row view from a persisted [`DownloadRecord`] and optional live [`ProgressSnapshot`].
    pub fn from_record(
        record: &DownloadRecord,
        live: Option<&ProgressSnapshot>,
        fifo_rank: Option<usize>,
    ) -> Self {
        let state = live.map_or(record.state, |s| s.state);
        let bytes_downloaded = live.map_or(record.bytes_downloaded, |s| s.bytes_downloaded);
        let total_bytes = live.and_then(|s| s.total_bytes).or(record.content_length);
        let bytes_per_sec = live.map_or(0, |s| s.bytes_per_sec);
        let eta_secs = live.and_then(|s| s.eta_secs);
        let active_workers = live.map_or(0, |s| s.active_workers);
        let segments: &[SegmentProgress] = live.map_or(&[], |s| s.segments.as_slice());

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
            && record
                .error_message
                .as_deref()
                .is_some_and(|msg| msg.contains("remote resource changed"));

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

/// Fixed-cell geometric state mark and short label for a [`DownloadState`].
pub fn state_glyph(state: DownloadState) -> (&'static str, &'static str) {
    match state {
        DownloadState::Queued => ("◌", "QUEUED"),
        DownloadState::Probing => ("◐", "PROBING"),
        DownloadState::Active => ("●", "ACTIVE"),
        DownloadState::Paused => ("❚❚", "PAUSED"),
        DownloadState::Completed => ("✓", "DONE"),
        DownloadState::Failed => ("✕", "FAILED"),
        DownloadState::Cancelled => ("✕", "CANCELLED"),
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
        let row = DownloadRowView::from_record(&rec, Some(&snap), None);
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
        );
        let r2 = DownloadRowView::from_record(
            &make_record("2", "https://example.com/alpha.bin", DownloadState::Queued, 20),
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
}
