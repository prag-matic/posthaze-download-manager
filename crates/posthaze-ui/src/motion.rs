//! Chronophotographic telemetry interpolation and transient state-snap timings.

use std::collections::HashMap;
use std::time::Instant;

use posthaze_engine::{DownloadId, DownloadState, ProgressSnapshot};

/// Duration of the amber bisection flash when dynamic work stealing splits a segment.
pub(crate) const SPLIT_FLASH_MS: f32 = 420.0;

/// Duration of the emerald pulse when a download completes.
pub(crate) const COMPLETION_FLASH_MS: f32 = 650.0;

/// Duration of the cyanotype/amber sash detent flash.
pub(crate) const DETENT_FLASH_MS: f32 = 240.0;

/// Animated panel dimension state for smooth collapse/expand/reset transitions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PanelAnim {
    pub current: f32,
    pub target: f32,
}

/// Per-download interpolated byte telemetry and transient visual pulses.
#[derive(Debug, Clone)]
pub(crate) struct AnimatedTelemetry {
    pub display_downloaded: f64,
    pub display_checkpointed: f64,
    pub target_downloaded: f64,
    pub target_checkpointed: f64,
    pub last_segment_count: usize,
    pub split_flash_at: Option<Instant>,
    pub completed_flash_at: Option<Instant>,
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

/// Tracks frame-interpolated progress bars and transient chronophotographic flashes.
#[derive(Debug, Default)]
pub(crate) struct MotionState {
    entries: HashMap<DownloadId, AnimatedTelemetry>,
    last_step_at: Option<Instant>,
    sidebar_anim: Option<PanelAnim>,
    inspector_anim: Option<PanelAnim>,
    sash_detent_flash_at: Option<Instant>,
}

impl MotionState {
    /// Starts or updates a smooth panel width transition for the left sidebar.
    pub(crate) fn animate_sidebar(&mut self, from: f32, to: f32) {
        let current = self.sidebar_anim.map(|a| a.current).unwrap_or(from);
        if (to - current).abs() <= 0.5 {
            self.sidebar_anim = None;
        } else {
            self.sidebar_anim = Some(PanelAnim {
                current,
                target: to,
            });
        }
    }

    /// Starts or updates a smooth panel height transition for the bottom inspector.
    pub(crate) fn animate_inspector(&mut self, from: f32, to: f32) {
        let current = self.inspector_anim.map(|a| a.current).unwrap_or(from);
        if (to - current).abs() <= 0.5 {
            self.inspector_anim = None;
        } else {
            self.inspector_anim = Some(PanelAnim {
                current,
                target: to,
            });
        }
    }

    /// Clears any active sidebar animation during direct pointer drag (zero lag).
    pub(crate) fn set_sidebar_immediate(&mut self) {
        self.sidebar_anim = None;
    }

    /// Clears any active inspector animation during direct pointer drag (zero lag).
    pub(crate) fn set_inspector_immediate(&mut self) {
        self.inspector_anim = None;
    }

    /// Returns the active animated sidebar width, or `fallback` when settled.
    pub(crate) fn current_sidebar_width(&self, fallback: f32) -> f32 {
        self.sidebar_anim.map(|a| a.current).unwrap_or(fallback)
    }

    /// Returns the active animated inspector height, or `fallback` when settled.
    pub(crate) fn current_inspector_height(&self, fallback: f32) -> f32 {
        self.inspector_anim.map(|a| a.current).unwrap_or(fallback)
    }

    /// Triggers a brief tactile flash along the active resize sash on magnetic detent.
    pub(crate) fn trigger_sash_detent(&mut self, now: Instant) {
        self.sash_detent_flash_at = Some(now);
    }

    /// Returns the current detent flash intensity (`0.0..=1.0`).
    pub(crate) fn sash_detent_flash(&self, now: Instant) -> f32 {
        self.sash_detent_flash_at
            .map(|started| {
                let elapsed_ms = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
                decay_ease_out(elapsed_ms, DETENT_FLASH_MS)
            })
            .unwrap_or(0.0)
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
                entry.target_downloaded = target_dl;
                entry.target_checkpointed = target_cp;
                if seg_count > entry.last_segment_count && entry.last_segment_count > 0 {
                    entry.split_flash_at = Some(now);
                }
                entry.last_segment_count = seg_count;
            }
            None => {
                self.entries.insert(
                    id.clone(),
                    AnimatedTelemetry {
                        display_downloaded: target_dl,
                        display_checkpointed: target_cp,
                        target_downloaded: target_dl,
                        target_checkpointed: target_cp,
                        last_segment_count: seg_count,
                        split_flash_at: None,
                        completed_flash_at: None,
                    },
                );
            }
        }
        true
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
                    AnimatedTelemetry {
                        display_downloaded: full,
                        display_checkpointed: full,
                        target_downloaded: full,
                        target_checkpointed: full,
                        last_segment_count: 0,
                        split_flash_at: None,
                        completed_flash_at: Some(now),
                    }
                });
                if let Some(total) = total_bytes {
                    let full = total as f64;
                    entry.target_downloaded = full;
                    entry.target_checkpointed = full;
                }
                entry.completed_flash_at = Some(now);
                true
            }
            DownloadState::Cancelled => {
                self.entries.remove(id);
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

        let alpha = 1.0 - (-dt * 14.0).exp();
        let mut any_active = false;

        for entry in self.entries.values_mut() {
            let dl_diff = entry.target_downloaded - entry.display_downloaded;
            if dl_diff.abs() > 1.0 {
                entry.display_downloaded += dl_diff * alpha;
                any_active = true;
            } else {
                entry.display_downloaded = entry.target_downloaded;
            }

            let cp_diff = entry.target_checkpointed - entry.display_checkpointed;
            if cp_diff.abs() > 1.0 {
                entry.display_checkpointed += cp_diff * alpha;
                any_active = true;
            } else {
                entry.display_checkpointed = entry.target_checkpointed;
            }

            if let Some(started) = entry.split_flash_at {
                let elapsed = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
                if elapsed < SPLIT_FLASH_MS {
                    any_active = true;
                } else {
                    entry.split_flash_at = None;
                }
            }

            if let Some(started) = entry.completed_flash_at {
                let elapsed = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
                if elapsed < COMPLETION_FLASH_MS {
                    any_active = true;
                } else {
                    entry.completed_flash_at = None;
                }
            }
        }

        let panel_alpha = (1.0 - (-dt * 18.0).exp()) as f32;
        if let Some(mut anim) = self.sidebar_anim {
            let diff = anim.target - anim.current;
            if diff.abs() > 0.5 {
                anim.current += diff * panel_alpha;
                self.sidebar_anim = Some(anim);
                any_active = true;
            } else {
                self.sidebar_anim = None;
            }
        }
        if let Some(mut anim) = self.inspector_anim {
            let diff = anim.target - anim.current;
            if diff.abs() > 0.5 {
                anim.current += diff * panel_alpha;
                self.inspector_anim = Some(anim);
                any_active = true;
            } else {
                self.inspector_anim = None;
            }
        }
        if let Some(started) = self.sash_detent_flash_at {
            let elapsed = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
            if elapsed < DETENT_FLASH_MS {
                any_active = true;
            } else {
                self.sash_detent_flash_at = None;
            }
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
                    Some((entry.display_checkpointed / total_f).clamp(0.0, 1.0) as f32),
                    Some((entry.display_downloaded / total_f).clamp(0.0, 1.0) as f32),
                )
            }
            _ => (None, None),
        };

        let split_flash = entry
            .split_flash_at
            .map(|started| {
                let elapsed_ms = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
                decay_ease_out(elapsed_ms, SPLIT_FLASH_MS)
            })
            .unwrap_or(0.0);

        let completion_flash = entry
            .completed_flash_at
            .map(|started| {
                let elapsed_ms = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
                decay_ease_out(elapsed_ms, COMPLETION_FLASH_MS)
            })
            .unwrap_or(0.0);

        RowMotionSample {
            checkpoint_ratio,
            downloaded_ratio,
            split_flash,
            completion_flash,
        }
    }
}

/// Computes an exponential ease-out decay from `1.0` at `0ms` to `0.0` at `duration_ms`.
pub(crate) fn decay_ease_out(elapsed_ms: f32, duration_ms: f32) -> f32 {
    if elapsed_ms <= 0.0 {
        return 1.0;
    }
    if elapsed_ms >= duration_ms || duration_ms <= 0.0 {
        return 0.0;
    }
    let t = (elapsed_ms / duration_ms).clamp(0.0, 1.0);
    (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn decay_ease_out_is_monotonic_and_bounded() {
        assert_eq!(decay_ease_out(0.0, 400.0), 1.0);
        let mid = decay_ease_out(200.0, 400.0);
        assert!(mid > 0.0 && mid < 0.2);
        assert_eq!(decay_ease_out(400.0, 400.0), 0.0);
        assert_eq!(decay_ease_out(500.0, 400.0), 0.0);
    }

    #[test]
    fn motion_state_interpolates_and_expires_flashes() {
        let mut motion = MotionState::default();
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

        motion.on_state_changed(&id, DownloadState::Completed, Some(1000), t0 + Duration::from_millis(100));
        let flash_sample = motion.sample(&id, Some(1000), t0 + Duration::from_millis(120));
        assert!(flash_sample.completion_flash > 0.8);

        let after_ms = t0 + Duration::from_millis(900);
        motion.step(after_ms);
        let settled = motion.sample(&id, Some(1000), after_ms);
        assert_eq!(settled.completion_flash, 0.0);
    }

    #[test]
    fn panel_animations_interpolate_and_yield_to_immediate_drag() {
        let mut motion = MotionState::default();
        let t0 = Instant::now();
        motion.animate_sidebar(52.0, 220.0);
        assert!(motion.step(t0 + Duration::from_millis(16)));
        let mid = motion.current_sidebar_width(220.0);
        assert!(mid > 52.0 && mid < 220.0);

        // Pointer drag clears animation immediately with zero lag.
        motion.set_sidebar_immediate();
        assert_eq!(motion.current_sidebar_width(245.0), 245.0);

        // Detent flash decays cleanly.
        motion.trigger_sash_detent(t0);
        assert!(motion.sash_detent_flash(t0 + Duration::from_millis(40)) > 0.5);
        assert_eq!(motion.sash_detent_flash(t0 + Duration::from_millis(300)), 0.0);
    }
}
