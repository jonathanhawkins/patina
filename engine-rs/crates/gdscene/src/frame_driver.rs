//! Fixed-timestep frame driver for the browser main loop (pat-nzgdu.94).
//!
//! Godot's web platform installs a browser-driven main-loop callback
//! (`platform/web/js/engine/engine.js` → `emscripten_set_main_loop`); the
//! browser hands the engine a timestamp per animation frame and the engine turns
//! that into a delta. Native Patina drives [`MainLoop`] from a CLI for-loop over
//! a fixed frame count, which is not a real-time loop: it advances the same
//! amount regardless of how much wall-clock time actually passed.
//!
//! This module is that missing layer, factored deliberately as **pure logic over
//! an injected timestamp** so it is unit-testable on native (`cargo test`) rather
//! than only in a browser. It holds no browser types: the wasm entry point owns
//! `requestAnimationFrame` and feeds each `DOMHighResTimeStamp` straight into
//! [`FrameDriver::tick`].
//!
//! # Division of labour
//!
//! [`MainLoop::step`] already owns the *physics* accumulator — it banks `delta`
//! and runs as many fixed `1/physics_ticks_per_second` ticks as fit (capped by
//! `max_physics_steps_per_frame`). This driver deliberately does **not** repeat
//! that. It owns only the concerns that are the *browser's*:
//!
//! - turning successive rAF timestamps into a delta;
//! - clamping a pathological delta (a backgrounded tab resumes with a multi-second
//!   jump) so the engine doesn't try to catch up in one frame;
//! - honouring `max_fps` by coalescing rAF callbacks that arrive too early;
//! - throttling while the document is hidden.
//!
//! Time is never invented or discarded silently: a frame skipped for `max_fps` or
//! visibility carries its delta forward into the next step, so N seconds of
//! browser time still produce N seconds of engine time (and therefore Godot's
//! step count), independent of how the rAF callbacks were chunked.

use crate::main_loop::{FrameOutput, MainLoop};

/// Default ceiling on a single frame's delta, in seconds.
///
/// A hidden or blocked tab can resume with a delta of many seconds. Feeding that
/// in whole would make the engine simulate it all at once (a "spiral of death":
/// the catch-up frame is itself slow, producing an even larger next delta). Godot
/// bounds the same hazard with `max_physics_steps_per_frame`; browsers need the
/// wall-clock side bounded too. 0.25s ≈ 15 ticks at 60Hz.
pub const DEFAULT_MAX_FRAME_DELTA: f64 = 0.25;

/// Frames per second targeted while the document is hidden.
///
/// Browsers already throttle `requestAnimationFrame` in background tabs (often to
/// ~1Hz or not at all). This bounds engine work in the same situation without
/// stopping the loop, matching Godot's low-processor/hidden behaviour.
pub const DEFAULT_HIDDEN_FPS: u32 = 10;

/// Turns browser animation-frame timestamps into [`MainLoop`] steps.
///
/// Created once by the wasm entry point and ticked from `requestAnimationFrame`:
///
/// ```ignore
/// // in the rAF callback, `now_ms` is the DOMHighResTimeStamp argument
/// driver.tick(now_ms, &mut main_loop);
/// ```
#[derive(Debug, Clone)]
pub struct FrameDriver {
    /// Timestamp (ms) of the last rAF callback, `None` before the first.
    last_timestamp_ms: Option<f64>,
    /// Delta banked from callbacks that were skipped (max_fps / hidden).
    carry: f64,
    /// Upper bound applied to a single inter-frame delta, in seconds.
    max_frame_delta: f64,
    /// Target FPS while visible; 0 means "every animation frame" (uncapped).
    max_fps: u32,
    /// Target FPS while hidden.
    hidden_fps: u32,
    /// Whether the document is currently hidden (`visibilitychange`).
    hidden: bool,
    /// Total seconds of browser time this driver has accepted.
    elapsed: f64,
}

impl Default for FrameDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDriver {
    /// Creates a driver that steps on every animation frame.
    pub fn new() -> Self {
        Self {
            last_timestamp_ms: None,
            carry: 0.0,
            max_frame_delta: DEFAULT_MAX_FRAME_DELTA,
            max_fps: 0,
            hidden_fps: DEFAULT_HIDDEN_FPS,
            hidden: false,
            elapsed: 0.0,
        }
    }

    /// Sets the visible-state frame cap (`Engine.max_fps`); 0 uncaps.
    ///
    /// Callbacks arriving sooner than `1/max_fps` are coalesced — their time is
    /// carried, not dropped.
    pub fn set_max_fps(&mut self, fps: u32) {
        self.max_fps = fps;
    }

    /// Returns the visible-state frame cap (0 = uncapped).
    pub fn max_fps(&self) -> u32 {
        self.max_fps
    }

    /// Sets the frame cap applied while the document is hidden.
    pub fn set_hidden_fps(&mut self, fps: u32) {
        self.hidden_fps = fps;
    }

    /// Marks the document hidden/visible (from `visibilitychange`).
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    /// Whether the document is currently marked hidden.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Sets the per-frame delta ceiling in seconds. Values <= 0 are ignored.
    pub fn set_max_frame_delta(&mut self, secs: f64) {
        if secs > 0.0 {
            self.max_frame_delta = secs;
        }
    }

    /// Total browser time accepted so far, in seconds.
    ///
    /// Counts time that was clamped away as *not* accepted, so this is the engine's
    /// view of elapsed time — the figure a step count should be reconciled against.
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// Delta banked from skipped callbacks, awaiting the next step.
    pub fn pending(&self) -> f64 {
        self.carry
    }

    /// The minimum seconds between steps given the current visibility/cap.
    fn min_frame_interval(&self) -> f64 {
        let fps = if self.hidden {
            self.hidden_fps
        } else {
            self.max_fps
        };
        if fps == 0 {
            0.0
        } else {
            1.0 / fps as f64
        }
    }

    /// Feeds one animation-frame timestamp (`DOMHighResTimeStamp`, milliseconds).
    ///
    /// Returns the [`FrameOutput`] when this callback produced an engine step, or
    /// `None` when it did not — the first callback (which only establishes a time
    /// origin), or one coalesced by `max_fps`/visibility throttling. A `None`
    /// return never loses time; it is carried into the next step.
    ///
    /// Non-monotonic or non-finite timestamps are treated as a zero delta rather
    /// than rewinding the clock.
    pub fn tick(&mut self, now_ms: f64, main_loop: &mut MainLoop) -> Option<FrameOutput> {
        if !now_ms.is_finite() {
            return None;
        }
        let Some(last) = self.last_timestamp_ms else {
            // First callback: establish the origin. There is no previous frame to
            // measure against, so no time has passed yet and nothing steps.
            self.last_timestamp_ms = Some(now_ms);
            return None;
        };
        self.last_timestamp_ms = Some(now_ms);

        let raw = (now_ms - last) / 1000.0;
        // A clock that goes backwards (or stands still) contributes nothing.
        let delta = raw.clamp(0.0, self.max_frame_delta);

        self.carry += delta;
        self.elapsed += delta;

        // Coalesce callbacks that arrived inside the frame budget. The carried
        // delta is not dropped — the next step receives it.
        let interval = self.min_frame_interval();
        if self.carry < interval {
            return None;
        }

        let step_delta = std::mem::replace(&mut self.carry, 0.0);
        if step_delta <= 0.0 {
            return None;
        }
        Some(main_loop.step(step_delta))
    }

    /// Drops any banked time and forgets the time origin.
    ///
    /// Call when the loop is re-entered after a long pause and the intervening
    /// wall-clock time should not be simulated.
    pub fn reset(&mut self) {
        self.last_timestamp_ms = None;
        self.carry = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_tree::SceneTree;

    /// A MainLoop over a bare tree — enough to count frames and physics steps.
    fn bare_loop() -> MainLoop {
        MainLoop::new(SceneTree::new())
    }

    #[test]
    fn first_tick_only_establishes_the_time_origin() {
        let mut driver = FrameDriver::new();
        let mut ml = bare_loop();
        // Nothing to measure against yet, so no step and no invented time.
        assert!(driver.tick(1_000.0, &mut ml).is_none());
        assert_eq!(driver.elapsed(), 0.0);
    }

    #[test]
    fn steps_once_per_animation_frame_when_uncapped() {
        let mut driver = FrameDriver::new();
        let mut ml = bare_loop();
        driver.tick(0.0, &mut ml);

        let out = driver.tick(16.0, &mut ml).expect("second callback steps");
        assert_eq!(out.frame_count, 1);
        assert!((out.delta - 0.016).abs() < 1e-9);
    }

    #[test]
    fn backwards_and_nonfinite_timestamps_do_not_rewind() {
        let mut driver = FrameDriver::new();
        let mut ml = bare_loop();
        driver.tick(1_000.0, &mut ml);

        // A timestamp before the previous one yields a clamped-to-zero delta,
        // which is not a step rather than a negative one.
        assert!(driver.tick(900.0, &mut ml).is_none());
        assert_eq!(driver.elapsed(), 0.0);
        assert!(driver.tick(f64::NAN, &mut ml).is_none());
    }

    #[test]
    fn pathological_delta_is_clamped_to_the_ceiling() {
        let mut driver = FrameDriver::new();
        let mut ml = bare_loop();
        driver.tick(0.0, &mut ml);

        // Tab was backgrounded for 10s: the engine must not simulate all of it.
        let out = driver.tick(10_000.0, &mut ml).expect("steps");
        assert!((out.delta - DEFAULT_MAX_FRAME_DELTA).abs() < 1e-9);
        assert!((driver.elapsed() - DEFAULT_MAX_FRAME_DELTA).abs() < 1e-9);
    }

    #[test]
    fn max_fps_coalesces_frames_without_losing_time() {
        let mut driver = FrameDriver::new();
        driver.set_max_fps(30); // one step per 33.3ms
        let mut ml = bare_loop();
        driver.tick(0.0, &mut ml);

        // 16ms < 33.3ms budget: coalesced, but banked.
        assert!(driver.tick(16.0, &mut ml).is_none());
        assert!((driver.pending() - 0.016).abs() < 1e-9);

        // 32ms total >= 33.3ms? not yet — still short.
        assert!(driver.tick(32.0, &mut ml).is_none());
        // 48ms total clears the budget; the step gets the FULL banked time.
        let out = driver.tick(48.0, &mut ml).expect("budget reached");
        assert!((out.delta - 0.048).abs() < 1e-9, "delta {} ", out.delta);
        assert_eq!(driver.pending(), 0.0);
    }

    #[test]
    fn hidden_documents_throttle_but_keep_time() {
        let mut driver = FrameDriver::new();
        let mut ml = bare_loop();
        driver.set_hidden(true);
        assert!(driver.is_hidden());
        driver.tick(0.0, &mut ml);

        // Hidden budget is 1/10s: a 16ms callback is coalesced.
        assert!(driver.tick(16.0, &mut ml).is_none());
        // ...and once 100ms accrues, one step carries all of it.
        let out = driver.tick(100.0, &mut ml).expect("hidden budget reached");
        assert!((out.delta - 0.100).abs() < 1e-9);
    }

    #[test]
    fn reset_forgets_origin_and_banked_time() {
        let mut driver = FrameDriver::new();
        driver.set_max_fps(30);
        let mut ml = bare_loop();
        driver.tick(0.0, &mut ml);
        driver.tick(16.0, &mut ml);
        assert!(driver.pending() > 0.0);

        driver.reset();
        assert_eq!(driver.pending(), 0.0);
        // Origin forgotten: the next callback establishes a new one, so the gap
        // spanning the pause is never simulated.
        assert!(driver.tick(60_000.0, &mut ml).is_none());
    }
}
