//! Chronophotographic download telemetry interpolation and transient state-snap timings.
//!
//! Composes [`chronophoto_ui::motion`] primitives (`PanelMotion`, `DecayPulse`,
//! `InterpolatedScalar`) with per-download byte telemetry.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use chronophoto_ui::motion::{DecayPulse, InterpolatedScalar, PanelMotion};
use posthaze_engine::{DownloadId, DownloadState, ProgressSnapshot};

/// Duration of the amber bisection flash when dynamic work stealing splits a segment.
pub(crate) const SPLIT_FLASH_MS: f32 = 420.0;

/// Duration of the emerald pulse when a download completes.
pub(crate) const COMPLETION_FLASH_MS: f32 = 650.0;

const TELEMETRY_SMOOTHING_RATE: f64 = 14.0;
const TELEMETRY_SNAP_EPSILON: f64 = 1.0;

/// Per-download interpolated byte telemetry and transient visual pulses.
#[derive(Debug, Clone)]
pub(crate) struct AnimatedTelemetry {
    pub downloaded: InterpolatedScalar,
    pub checkpointed: InterpolatedScalar,
    pub last_segment_count: usize,
    pub split_flash: DecayPulse,
    pub completed_flash: DecayPulse,
    pub terminal: bool,
}

impl AnimatedTelemetry {
    fn new(downloaded: f64, checkpointed: f64, last_segment_count: usize) -> Self {
        Self {
            downloaded: InterpolatedScalar::new(
                downloaded,
                TELEMETRY_SMOOTHING_RATE,
                TELEMETRY_SNAP_EPSILON,
            ),
            checkpointed: InterpolatedScalar::new(
                checkpointed,
                TELEMETRY_SMOOTHING_RATE,
                TELEMETRY_SNAP_EPSILON,
            ),
            last_segment_count,
            split_flash: DecayPulse::new(SPLIT_FLASH_MS),
            completed_flash: DecayPulse::new(COMPLETION_FLASH_MS),
            terminal: false,
        }
    }
}

/// Snapshot of visual telemetry ratios & pulse intensities for a single render frame.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RowMotionSample {
    /// Smoothed ratio (0.0..=1.0) of writer-acked checkpointed bytes.
    pub checkpoint_ratio: Option<f32>,
    /// Smoothed ratio (0.0..=1.0) of total downloaded bytes (checkpoint + in-flight).
    pub downloaded_ratio: Option<f32>,
    /// Exponentially decaying intensity (0.0..=1.0) after a work-stealing segment split.
    pub split_flash: f32,
    /// Exponentially decaying intensity (0.0..=1.0) after transition to `Completed`.
    pub completion_flash: f32,
}

/// Tracks frame-interpolated download progress bars, transient chronophotographic flashes,
/// and domain-free panel/scroll animations via [`PanelMotion`].
#[derive(Debug, Default)]
pub(crate) struct DownloadMotion {
    entries: HashMap<DownloadId, AnimatedTelemetry>,
    last_step_at: Option<Instant>,
    pub(crate) panels: PanelMotion,
}

impl DownloadMotion {
    /// Starts or updates a smooth panel width transition for the left sidebar.
    pub(crate) fn animate_sidebar(&mut self, from: f32, to: f32) {
        self.panels.animate_sidebar(from, to);
    }

    /// Starts or updates a smooth panel height transition for the bottom inspector.
    pub(crate) fn animate_inspector(&mut self, from: f32, to: f32) {
        self.panels.animate_inspector(from, to);
    }

    /// Clears any active sidebar animation during direct pointer drag (zero lag).
    pub(crate) fn set_sidebar_immediate(&mut self) {
        self.panels.set_sidebar_immediate();
    }

    /// Clears any active inspector animation during direct pointer drag (zero lag).
    pub(crate) fn set_inspector_immediate(&mut self) {
        self.panels.set_inspector_immediate();
    }

    /// Starts or updates a smooth vertical scroll transition for the downloads table.
    pub(crate) fn animate_table_scroll(&mut self, from: f32, to: f32) {
        self.panels.animate_table_scroll(from, to);
    }

    /// Returns the active animated table scroll offset, if any.
    pub(crate) fn current_table_scroll(&self) -> Option<f32> {
        self.panels.current_table_scroll()
    }

    /// Returns the next scroll offset to apply to the scroll handle, if one was stepped.
    pub(crate) fn take_table_scroll_y(&mut self) -> Option<f32> {
        self.panels.take_table_scroll_y()
    }

    /// Cancels any active keyboard scroll animation when the user scrolls manually.
    pub(crate) fn clear_table_scroll(&mut self) {
        self.panels.clear_table_scroll();
    }

    /// Returns the active animated sidebar width, or `fallback` when settled.
    pub(crate) fn current_sidebar_width(&self, fallback: f32) -> f32 {
        self.panels.current_sidebar_width(fallback)
    }

    /// Returns the active animated inspector height, or `fallback` when settled.
    pub(crate) fn current_inspector_height(&self, fallback: f32) -> f32 {
        self.panels.current_inspector_height(fallback)
    }

    /// Triggers a brief tactile flash along the active resize sash on magnetic detent.
    pub(crate) fn trigger_sash_detent(&mut self, now: Instant) {
        self.panels.trigger_sash_detent(now);
    }

    /// Returns the current detent flash intensity (`0.0..=1.0`).
    pub(crate) fn sash_detent_flash(&self, now: Instant) -> f32 {
        self.panels.sash_detent_flash(now)
    }

    /// Records a fresh live progress snapshot from the engine.
    ///
    /// Returns `true` if a new animation or interpolation is active and a frame should be scheduled.
    pub(crate) fn on_progress(
        &mut self,
        id: &DownloadId,
        snapshot: &ProgressSnapshot,
        now: Instant,
    ) -> bool {
        let target_dl = snapshot.bytes_downloaded as f64;
        let checkpointed: u64 = if snapshot.state == DownloadState::Completed {
            snapshot.bytes_downloaded
        } else if !snapshot.segments.is_empty() {
            snapshot
                .segments
                .iter()
                .map(|s| s.checkpointed_cursor.saturating_sub(s.start))
                .sum()
        } else {
            0
        };
        let target_cp = checkpointed as f64;
        let seg_count = snapshot.segments.len();

        match self.entries.get_mut(id) {
            Some(entry) => {
                entry.downloaded.set_target(target_dl);
                entry.checkpointed.set_target(target_cp);
                entry.terminal = false;
                if seg_count > entry.last_segment_count && entry.last_segment_count > 0 {
                    entry.split_flash.trigger(now);
                }
                entry.last_segment_count = seg_count;
            }
            None => {
                self.entries.insert(
                    id.clone(),
                    AnimatedTelemetry::new(target_dl, target_cp, seg_count),
                );
            }
        }
        true
    }

    /// Snaps interpolated values immediately to live progress snapshots (used on restore from tray).
    pub(crate) fn snap_to_live(&mut self, live_map: &HashMap<DownloadId, ProgressSnapshot>) {
        for (id, snapshot) in live_map {
            let target_dl = snapshot.bytes_downloaded as f64;
            let checkpointed: u64 = if snapshot.state == DownloadState::Completed {
                snapshot.bytes_downloaded
            } else if !snapshot.segments.is_empty() {
                snapshot
                    .segments
                    .iter()
                    .map(|s| s.checkpointed_cursor.saturating_sub(s.start))
                    .sum()
            } else {
                0
            };
            let target_cp = checkpointed as f64;
            let seg_count = snapshot.segments.len();
            self.entries.insert(
                id.clone(),
                AnimatedTelemetry::new(target_dl, target_cp, seg_count),
            );
        }
        self.last_step_at = None;
    }

    /// Prunes entries for `Download`s no longer present in the `Queue`.
    pub(crate) fn prune_missing(&mut self, existing: &HashSet<&DownloadId>) {
        self.entries.retain(|id, _| existing.contains(id));
    }

    /// Returns the number of tracked per-download telemetry entries.
    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Records an engine state transition, triggering a completion pulse on `Completed`.
    pub(crate) fn on_state_changed(
        &mut self,
        id: &DownloadId,
        state: DownloadState,
        total_bytes: Option<u64>,
        now: Instant,
    ) -> bool {
        match state {
            DownloadState::Completed => {
                let entry = self.entries.entry(id.clone()).or_insert_with(|| {
                    let full = total_bytes.unwrap_or(0) as f64;
                    let mut entry = AnimatedTelemetry::new(full, full, 0);
                    entry.terminal = true;
                    entry
                });
                if let Some(total) = total_bytes {
                    let full = total as f64;
                    entry.downloaded.set_target(full);
                    entry.checkpointed.set_target(full);
                }
                entry.completed_flash.trigger(now);
                entry.terminal = true;
                true
            }
            DownloadState::Cancelled => {
                self.entries.remove(id);
                false
            }
            DownloadState::Paused | DownloadState::Failed => {
                if let Some(entry) = self.entries.get_mut(id) {
                    entry.terminal = true;
                    entry.downloaded.snap(entry.downloaded.target());
                    entry.checkpointed.snap(entry.checkpointed.target());
                    if entry.split_flash.intensity(now) == 0.0
                        && entry.completed_flash.intensity(now) == 0.0
                    {
                        self.entries.remove(id);
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Advances all interpolated values toward their targets for `now`.
    ///
    /// Returns `true` while any interpolation or transient flash remains active.
    pub(crate) fn step(&mut self, now: Instant) -> bool {
        let dt = self
            .last_step_at
            .map(|prev| now.saturating_duration_since(prev).as_secs_f64())
            .unwrap_or(1.0 / 60.0)
            .clamp(0.001, 0.1);
        self.last_step_at = Some(now);

        let mut any_active = false;

        for entry in self.entries.values_mut() {
            if entry.downloaded.step(dt) {
                any_active = true;
            }
            if entry.checkpointed.step(dt) {
                any_active = true;
            }
            if entry.split_flash.step(now) {
                any_active = true;
            }
            if entry.completed_flash.step(now) {
                any_active = true;
            }
        }

        // Prune settled terminal entries once their flashes have finished.
        self.entries.retain(|_, entry| {
            !entry.terminal
                || entry.split_flash.intensity(now) > 0.0
                || entry.completed_flash.intensity(now) > 0.0
                || !entry.downloaded.is_settled()
                || !entry.checkpointed.is_settled()
        });

        if self.panels.step(now) {
            any_active = true;
        }

        if !any_active {
            self.last_step_at = None;
        }
        any_active
    }

    /// Returns the current interpolated ratios and flash intensities for `id`.
    pub(crate) fn sample(
        &self,
        id: &DownloadId,
        total_bytes: Option<u64>,
        now: Instant,
    ) -> RowMotionSample {
        let Some(entry) = self.entries.get(id) else {
            return RowMotionSample::default();
        };

        let (checkpoint_ratio, downloaded_ratio) = match total_bytes {
            Some(total) if total > 0 => {
                let total_f = total as f64;
                (
                    Some((entry.checkpointed.current() / total_f).clamp(0.0, 1.0) as f32),
                    Some((entry.downloaded.current() / total_f).clamp(0.0, 1.0) as f32),
                )
            }
            _ => (None, None),
        };

        RowMotionSample {
            checkpoint_ratio,
            downloaded_ratio,
            split_flash: entry.split_flash.intensity(now),
            completion_flash: entry.completed_flash.intensity(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn motion_state_interpolates_and_expires_flashes() {
        let mut motion = DownloadMotion::default();
        let id = DownloadId::from("dl-1".to_string());
        let t0 = Instant::now();

        let snap1 = ProgressSnapshot {
            state: DownloadState::Active,
            bytes_downloaded: 100,
            total_bytes: Some(1000),
            active_workers: 1,
            bytes_per_sec: 100,
            eta_secs: Some(9),
            segments: vec![],
        };
        motion.on_progress(&id, &snap1, t0);

        let mut snap2 = snap1.clone();
        snap2.bytes_downloaded = 500;
        motion.on_progress(&id, &snap2, t0 + Duration::from_millis(50));

        assert!(motion.step(t0 + Duration::from_millis(66)));
        let sample = motion.sample(&id, Some(1000), t0 + Duration::from_millis(66));
        let dl_ratio = sample.downloaded_ratio.unwrap();
        assert!(dl_ratio > 0.1 && dl_ratio < 0.5);

        motion.on_state_changed(
            &id,
            DownloadState::Completed,
            Some(1000),
            t0 + Duration::from_millis(100),
        );
        let flash_sample = motion.sample(&id, Some(1000), t0 + Duration::from_millis(120));
        assert!(flash_sample.completion_flash > 0.8);

        let after_ms = t0 + Duration::from_millis(900);
        motion.step(after_ms);
        let settled = motion.sample(&id, Some(1000), after_ms);
        assert_eq!(settled.completion_flash, 0.0);
    }

    #[test]
    fn snap_to_live_and_pruning_prevent_unbounded_motion_map_growth() {
        let mut motion = DownloadMotion::default();
        let id1 = DownloadId::from("dl-1".to_string());
        let id2 = DownloadId::from("dl-2".to_string());
        let t0 = Instant::now();

        let snap1 = ProgressSnapshot {
            state: DownloadState::Active,
            bytes_downloaded: 750,
            total_bytes: Some(1000),
            active_workers: 4,
            bytes_per_sec: 1000,
            eta_secs: Some(1),
            segments: vec![],
        };
        let mut live = HashMap::new();
        live.insert(id1.clone(), snap1.clone());
        motion.snap_to_live(&live);
        let restored = motion.sample(&id1, Some(1000), t0);
        assert_eq!(restored.downloaded_ratio, Some(0.75));
        assert_eq!(motion.entry_count(), 1);

        // Completing and stepping past the flash prunes the settled entry.
        motion.on_state_changed(&id1, DownloadState::Completed, Some(1000), t0);
        let mut t = t0;
        for _ in 0..40 {
            t += Duration::from_millis(50);
            motion.step(t);
        }
        assert_eq!(
            motion.entry_count(),
            0,
            "completed entry should be pruned once settled and flash expires"
        );

        // Paused/missing entries are also pruned immediately.
        motion.on_progress(&id2, &snap1, t);
        assert_eq!(motion.entry_count(), 1);
        motion.on_state_changed(&id2, DownloadState::Paused, Some(1000), t);
        assert_eq!(motion.entry_count(), 0);

        motion.on_progress(&id2, &snap1, t);
        let existing: HashSet<&DownloadId> = HashSet::new();
        motion.prune_missing(&existing);
        assert_eq!(motion.entry_count(), 0);
    }
}
