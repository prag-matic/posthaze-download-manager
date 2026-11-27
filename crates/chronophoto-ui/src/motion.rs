//! Domain-free motion primitives for transient pulses, scalar interpolation,
//! and panel/scroll animations.

use std::time::Instant;

use crate::tokens::DETENT_FLASH_MS;

/// Computes an exponential ease-out decay from `1.0` at `0ms` to `0.0` at `duration_ms`.
pub fn decay_ease_out(elapsed_ms: f32, duration_ms: f32) -> f32 {
    if elapsed_ms <= 0.0 {
        return 1.0;
    }
    if elapsed_ms >= duration_ms || duration_ms <= 0.0 {
        return 0.0;
    }
    let t = (elapsed_ms / duration_ms).clamp(0.0, 1.0);
    (1.0 - t).powi(3)
}

/// A transient ease-out pulse over a fixed duration in milliseconds.
#[derive(Debug, Clone, Copy)]
pub struct DecayPulse {
    duration_ms: f32,
    started_at: Option<Instant>,
}

impl DecayPulse {
    /// Creates a new [`DecayPulse`] with the given `duration_ms`.
    pub const fn new(duration_ms: f32) -> Self {
        Self {
            duration_ms,
            started_at: None,
        }
    }

    /// Starts (or restarts) the pulse at `now`.
    pub fn trigger(&mut self, now: Instant) {
        self.started_at = Some(now);
    }

    /// Clears any active pulse immediately.
    pub fn clear(&mut self) {
        self.started_at = None;
    }

    /// Returns the current pulse intensity in `0.0..=1.0` at `now`.
    pub fn intensity(&self, now: Instant) -> f32 {
        self.started_at
            .map(|started| {
                let elapsed_ms = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
                decay_ease_out(elapsed_ms, self.duration_ms)
            })
            .unwrap_or(0.0)
    }

    /// Advances the pulse at `now`, clearing it once expired.
    ///
    /// Returns `true` while the pulse remains active.
    pub fn step(&mut self, now: Instant) -> bool {
        let Some(started) = self.started_at else {
            return false;
        };
        let elapsed_ms = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
        if elapsed_ms < self.duration_ms {
            true
        } else {
            self.started_at = None;
            false
        }
    }
}

/// Smoothly interpolates a scalar value toward a target using frame-rate-independent
/// exponential smoothing (`1.0 - (-dt * rate).exp()`).
#[derive(Debug, Clone, Copy)]
pub struct InterpolatedScalar {
    current: f64,
    target: f64,
    rate: f64,
    epsilon: f64,
}

impl InterpolatedScalar {
    /// Creates a settled [`InterpolatedScalar`] at `initial` with smoothing `rate`
    /// and snap threshold `epsilon`.
    pub const fn new(initial: f64, rate: f64, epsilon: f64) -> Self {
        Self {
            current: initial,
            target: initial,
            rate,
            epsilon,
        }
    }

    /// Sets a new target value to interpolate toward.
    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    /// Snaps both current and target values immediately to `value`.
    pub fn snap(&mut self, value: f64) {
        self.current = value;
        self.target = value;
    }

    /// Returns the current interpolated value.
    pub const fn current(&self) -> f64 {
        self.current
    }

    /// Returns the target value.
    pub const fn target(&self) -> f64 {
        self.target
    }

    /// Returns `true` when the scalar is within `epsilon` of its target.
    pub fn is_settled(&self) -> bool {
        (self.target - self.current).abs() <= self.epsilon
    }

    /// Advances the interpolation by `dt_secs`.
    ///
    /// Returns `true` if the value is still moving toward its target.
    pub fn step(&mut self, dt_secs: f64) -> bool {
        let diff = self.target - self.current;
        if diff.abs() > self.epsilon {
            let alpha = 1.0 - (-dt_secs * self.rate).exp();
            self.current += diff * alpha;
            true
        } else {
            self.current = self.target;
            false
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PanelAnim {
    scalar: InterpolatedScalar,
}

impl PanelAnim {
    fn new(current: f32, target: f32) -> Self {
        let mut scalar = InterpolatedScalar::new(f64::from(current), 18.0, 0.5);
        scalar.set_target(f64::from(target));
        Self { scalar }
    }

    fn current(&self) -> f32 {
        self.scalar.current() as f32
    }

    fn target(&self) -> f32 {
        self.scalar.target() as f32
    }

    fn step(&mut self, dt_secs: f64) -> bool {
        self.scalar.step(dt_secs)
    }
}

/// Domain-free panel dimension, scroll smoothing, and magnetic sash detent flash state.
#[derive(Debug)]
pub struct PanelMotion {
    last_step_at: Option<Instant>,
    sidebar_anim: Option<PanelAnim>,
    inspector_anim: Option<PanelAnim>,
    table_scroll_anim: Option<PanelAnim>,
    table_scroll_pending_y: Option<f32>,
    sash_detent_flash: DecayPulse,
}

impl Default for PanelMotion {
    fn default() -> Self {
        Self::new()
    }
}

impl PanelMotion {
    /// Creates a new idle [`PanelMotion`] state.
    pub const fn new() -> Self {
        Self {
            last_step_at: None,
            sidebar_anim: None,
            inspector_anim: None,
            table_scroll_anim: None,
            table_scroll_pending_y: None,
            sash_detent_flash: DecayPulse::new(DETENT_FLASH_MS),
        }
    }

    /// Starts or updates a smooth panel width transition for the left sidebar.
    pub fn animate_sidebar(&mut self, from: f32, to: f32) {
        let current = self.sidebar_anim.map(|a| a.current()).unwrap_or(from);
        if (to - current).abs() <= 0.5 {
            self.sidebar_anim = None;
        } else {
            self.sidebar_anim = Some(PanelAnim::new(current, to));
        }
    }

    /// Starts or updates a smooth panel height transition for the bottom inspector.
    pub fn animate_inspector(&mut self, from: f32, to: f32) {
        let current = self.inspector_anim.map(|a| a.current()).unwrap_or(from);
        if (to - current).abs() <= 0.5 {
            self.inspector_anim = None;
        } else {
            self.inspector_anim = Some(PanelAnim::new(current, to));
        }
    }

    /// Clears any active sidebar animation during direct pointer drag (zero lag).
    pub fn set_sidebar_immediate(&mut self) {
        self.sidebar_anim = None;
    }

    /// Clears any active inspector animation during direct pointer drag (zero lag).
    pub fn set_inspector_immediate(&mut self) {
        self.inspector_anim = None;
    }

    /// Starts or updates a smooth vertical scroll transition.
    pub fn animate_table_scroll(&mut self, from: f32, to: f32) {
        let current = self.table_scroll_anim.map(|a| a.current()).unwrap_or(from);
        if (to - current).abs() <= 0.5 {
            self.table_scroll_anim = None;
            self.table_scroll_pending_y = Some(to);
        } else {
            self.table_scroll_anim = Some(PanelAnim::new(current, to));
        }
    }

    /// Returns the active animated scroll offset, if any.
    pub fn current_table_scroll(&self) -> Option<f32> {
        self.table_scroll_anim.map(|a| a.current())
    }

    /// Returns the next scroll offset to apply to the scroll handle, if one was stepped.
    pub fn take_table_scroll_y(&mut self) -> Option<f32> {
        self.table_scroll_pending_y.take()
    }

    /// Cancels any active keyboard scroll animation when the user scrolls manually.
    pub fn clear_table_scroll(&mut self) {
        self.table_scroll_anim = None;
        self.table_scroll_pending_y = None;
    }

    /// Returns the active animated sidebar width, or `fallback` when settled.
    pub fn current_sidebar_width(&self, fallback: f32) -> f32 {
        self.sidebar_anim.map(|a| a.current()).unwrap_or(fallback)
    }

    /// Returns the active animated inspector height, or `fallback` when settled.
    pub fn current_inspector_height(&self, fallback: f32) -> f32 {
        self.inspector_anim.map(|a| a.current()).unwrap_or(fallback)
    }

    /// Triggers a brief tactile flash along the active resize sash on magnetic detent.
    pub fn trigger_sash_detent(&mut self, now: Instant) {
        self.sash_detent_flash.trigger(now);
    }

    /// Returns the current detent flash intensity (`0.0..=1.0`).
    pub fn sash_detent_flash(&self, now: Instant) -> f32 {
        self.sash_detent_flash.intensity(now)
    }

    /// Advances all panel and scroll animations toward their targets for `now`.
    ///
    /// Returns `true` while any animation or detent flash remains active.
    pub fn step(&mut self, now: Instant) -> bool {
        let dt = self
            .last_step_at
            .map(|prev| now.saturating_duration_since(prev).as_secs_f64())
            .unwrap_or(1.0 / 60.0)
            .clamp(0.001, 0.1);
        self.last_step_at = Some(now);

        let mut any_active = false;

        if let Some(mut anim) = self.sidebar_anim {
            if anim.step(dt) {
                self.sidebar_anim = Some(anim);
                any_active = true;
            } else {
                self.sidebar_anim = None;
            }
        }
        if let Some(mut anim) = self.inspector_anim {
            if anim.step(dt) {
                self.inspector_anim = Some(anim);
                any_active = true;
            } else {
                self.inspector_anim = None;
            }
        }
        if let Some(mut anim) = self.table_scroll_anim {
            if anim.step(dt) {
                self.table_scroll_anim = Some(anim);
                self.table_scroll_pending_y = Some(anim.current());
                any_active = true;
            } else {
                self.table_scroll_anim = None;
                self.table_scroll_pending_y = Some(anim.target());
            }
        }
        if self.sash_detent_flash.step(now) {
            any_active = true;
        }

        if !any_active {
            self.last_step_at = None;
        }
        any_active
    }
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
    fn decay_pulse_triggers_decays_and_expires() {
        let mut pulse = DecayPulse::new(200.0);
        let t0 = Instant::now();
        assert_eq!(pulse.intensity(t0), 0.0);
        assert!(!pulse.step(t0));

        pulse.trigger(t0);
        assert_eq!(pulse.intensity(t0), 1.0);
        assert!(pulse.step(t0 + Duration::from_millis(50)));
        assert!(pulse.intensity(t0 + Duration::from_millis(50)) > 0.4);

        assert!(!pulse.step(t0 + Duration::from_millis(250)));
        assert_eq!(pulse.intensity(t0 + Duration::from_millis(250)), 0.0);
    }

    #[test]
    fn interpolated_scalar_approaches_target_and_snaps() {
        let mut scalar = InterpolatedScalar::new(0.0, 14.0, 0.5);
        assert!(scalar.is_settled());

        scalar.set_target(100.0);
        assert!(!scalar.is_settled());
        assert!(scalar.step(1.0 / 60.0));
        assert!(scalar.current() > 0.0 && scalar.current() < 100.0);

        scalar.snap(100.0);
        assert_eq!(scalar.current(), 100.0);
        assert!(scalar.is_settled());
        assert!(!scalar.step(1.0 / 60.0));
    }

    #[test]
    fn panel_motion_interpolates_and_yields_to_immediate_drag() {
        let mut motion = PanelMotion::new();
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

    #[test]
    fn panel_motion_table_scroll_steps_and_clears() {
        let mut motion = PanelMotion::new();
        let t0 = Instant::now();
        motion.animate_table_scroll(0.0, 300.0);
        assert!(motion.step(t0 + Duration::from_millis(16)));
        let y = motion.take_table_scroll_y().expect("pending scroll offset");
        assert!(y > 0.0 && y < 300.0);
        assert!(motion.take_table_scroll_y().is_none());

        motion.clear_table_scroll();
        assert!(motion.current_table_scroll().is_none());
    }
}
