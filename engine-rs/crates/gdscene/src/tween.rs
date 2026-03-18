//! Tween system for procedural property animation.
//!
//! Tweens provide a code-driven approach to animating properties, as an
//! alternative to keyframe-based [`Animation`](crate::animation::Animation).
//! They support easing functions, delays, parallel execution, and looping.

use crate::animation::interpolate_variant;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// EaseType
// ---------------------------------------------------------------------------

/// Controls the acceleration profile of the easing curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EaseType {
    /// Ease in (slow start).
    In,
    /// Ease out (slow end).
    Out,
    /// Ease in then out (slow start and end).
    InOut,
}

impl Default for EaseType {
    fn default() -> Self {
        Self::InOut
    }
}

// ---------------------------------------------------------------------------
// TransFunc
// ---------------------------------------------------------------------------

/// The mathematical function used for easing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransFunc {
    /// No easing — constant speed.
    Linear,
    /// Sinusoidal easing.
    Sine,
    /// Quadratic easing.
    Quad,
    /// Cubic easing.
    Cubic,
    /// Exponential easing.
    Expo,
    /// Elastic (spring-like) easing.
    Elastic,
    /// Bounce easing.
    Bounce,
    /// Back (overshoot) easing.
    Back,
}

impl Default for TransFunc {
    fn default() -> Self {
        Self::Linear
    }
}

// ---------------------------------------------------------------------------
// ease()
// ---------------------------------------------------------------------------

/// Computes the eased value for `t` in `[0, 1]`.
///
/// Combines a [`TransFunc`] (the curve shape) with an [`EaseType`] (the
/// acceleration direction) to produce the final eased value.
pub fn ease(t: f64, ease_type: EaseType, trans_func: TransFunc) -> f64 {
    let t = t.clamp(0.0, 1.0);

    match ease_type {
        EaseType::In => ease_in(t, trans_func),
        EaseType::Out => ease_out(t, trans_func),
        EaseType::InOut => ease_in_out(t, trans_func),
    }
}

fn ease_in(t: f64, func: TransFunc) -> f64 {
    match func {
        TransFunc::Linear => t,
        TransFunc::Sine => 1.0 - ((t * std::f64::consts::FRAC_PI_2).cos()),
        TransFunc::Quad => t * t,
        TransFunc::Cubic => t * t * t,
        TransFunc::Expo => {
            if t == 0.0 {
                0.0
            } else {
                (2.0_f64).powf(10.0 * (t - 1.0))
            }
        }
        TransFunc::Elastic => {
            if t == 0.0 || t == 1.0 {
                t
            } else {
                let p = 0.3;
                -(2.0_f64.powf(10.0 * (t - 1.0)))
                    * ((t - 1.0 - p / 4.0) * std::f64::consts::TAU / p).sin()
            }
        }
        TransFunc::Bounce => 1.0 - bounce_out(1.0 - t),
        TransFunc::Back => {
            let s = 1.70158;
            t * t * ((s + 1.0) * t - s)
        }
    }
}

fn ease_out(t: f64, func: TransFunc) -> f64 {
    match func {
        TransFunc::Linear => t,
        TransFunc::Sine => (t * std::f64::consts::FRAC_PI_2).sin(),
        TransFunc::Quad => t * (2.0 - t),
        TransFunc::Cubic => {
            let t1 = t - 1.0;
            t1 * t1 * t1 + 1.0
        }
        TransFunc::Expo => {
            if t == 1.0 {
                1.0
            } else {
                1.0 - (2.0_f64).powf(-10.0 * t)
            }
        }
        TransFunc::Elastic => {
            if t == 0.0 || t == 1.0 {
                t
            } else {
                let p = 0.3;
                2.0_f64.powf(-10.0 * t) * ((t - p / 4.0) * std::f64::consts::TAU / p).sin()
                    + 1.0
            }
        }
        TransFunc::Bounce => bounce_out(t),
        TransFunc::Back => {
            let s = 1.70158;
            let t1 = t - 1.0;
            t1 * t1 * ((s + 1.0) * t1 + s) + 1.0
        }
    }
}

fn ease_in_out(t: f64, func: TransFunc) -> f64 {
    if t < 0.5 {
        ease_in(t * 2.0, func) * 0.5
    } else {
        ease_out(t * 2.0 - 1.0, func) * 0.5 + 0.5
    }
}

fn bounce_out(t: f64) -> f64 {
    if t < 1.0 / 2.75 {
        7.5625 * t * t
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        7.5625 * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        7.5625 * t * t + 0.9375
    } else {
        let t = t - 2.625 / 2.75;
        7.5625 * t * t + 0.984375
    }
}

// ---------------------------------------------------------------------------
// TweenStep
// ---------------------------------------------------------------------------

/// A single property transition within a tween.
#[derive(Debug, Clone)]
pub struct TweenStep {
    /// The property path to animate.
    pub property_path: String,
    /// The starting value.
    pub start_value: Variant,
    /// The ending value.
    pub end_value: Variant,
    /// Total duration of this step in seconds.
    pub duration: f64,
    /// Elapsed time for this step.
    pub elapsed: f64,
    /// The easing acceleration type.
    pub ease_type: EaseType,
    /// The easing function.
    pub trans_func: TransFunc,
    /// Delay before this step starts (in seconds).
    pub delay: f64,
    /// Whether this step runs in parallel with the previous step.
    pub parallel: bool,
}

impl TweenStep {
    /// Creates a new tween step.
    pub fn new(
        property_path: impl Into<String>,
        start_value: Variant,
        end_value: Variant,
        duration: f64,
    ) -> Self {
        Self {
            property_path: property_path.into(),
            start_value,
            end_value,
            duration,
            elapsed: 0.0,
            ease_type: EaseType::InOut,
            trans_func: TransFunc::Linear,
            delay: 0.0,
            parallel: false,
        }
    }

    /// Returns `true` if this step has completed.
    pub fn is_complete(&self) -> bool {
        self.elapsed >= self.delay + self.duration
    }

    /// Returns the current interpolated value.
    pub fn current_value(&self) -> Variant {
        if self.elapsed < self.delay {
            return self.start_value.clone();
        }
        let active_time = self.elapsed - self.delay;
        if active_time >= self.duration {
            return self.end_value.clone();
        }
        if self.duration <= 0.0 {
            return self.end_value.clone();
        }

        let raw_t = active_time / self.duration;
        let eased_t = ease(raw_t, self.ease_type, self.trans_func);

        interpolate_variant(&self.start_value, &self.end_value, eased_t as f32)
            .unwrap_or_else(|| self.start_value.clone())
    }
}

// ---------------------------------------------------------------------------
// Tween
// ---------------------------------------------------------------------------

/// A procedural animation that tweens property values over time.
///
/// Supports sequential and parallel step execution, easing, delays, and looping.
#[derive(Debug, Clone)]
pub struct Tween {
    /// The steps in this tween.
    pub steps: Vec<TweenStep>,
    /// Whether the tween is currently running.
    pub running: bool,
    /// Number of loops remaining (-1 = infinite, 0 = done).
    pub loops: i32,
    /// Total number of loops configured.
    loops_total: i32,
}

impl Tween {
    /// Creates a new empty tween.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            running: false,
            loops: 1,
            loops_total: 1,
        }
    }

    /// Starts the tween.
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Stops the tween.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Sets the number of loops (-1 for infinite).
    pub fn set_loops(&mut self, loops: i32) {
        self.loops = loops;
        self.loops_total = loops;
    }

    /// Advances the tween by `delta` seconds. Returns `true` when fully complete.
    pub fn advance(&mut self, delta: f64) -> bool {
        if !self.running {
            return true;
        }
        if self.steps.is_empty() {
            self.running = false;
            return true;
        }

        // Process steps in groups: sequential steps form barriers,
        // parallel steps run together with the previous sequential step.
        let mut remaining = delta;
        while remaining > 0.0 {
            // Find the current group of steps to process.
            let group = self.current_step_group();
            if group.is_empty() {
                // All steps complete — check loops.
                if self.loops == -1 {
                    self.reset_steps();
                    continue;
                }
                self.loops -= 1;
                if self.loops <= 0 {
                    self.running = false;
                    return true;
                }
                self.reset_steps();
                continue;
            }

            // Find the minimum remaining time in this group.
            let mut min_remaining = f64::MAX;
            for &idx in &group {
                let step = &self.steps[idx];
                let step_remaining = (step.delay + step.duration) - step.elapsed;
                if step_remaining < min_remaining {
                    min_remaining = step_remaining;
                }
            }

            let advance_by = remaining.min(min_remaining.max(0.0));
            for &idx in &group {
                self.steps[idx].elapsed += advance_by;
            }
            remaining -= advance_by;

            // If we didn't advance at all, break to avoid infinite loop
            if advance_by <= 0.0 {
                break;
            }
        }

        // Check if all steps are complete
        let all_done = self.steps.iter().all(|s| s.is_complete());
        if all_done {
            if self.loops == -1 {
                self.reset_steps();
                return false;
            }
            self.loops -= 1;
            if self.loops <= 0 {
                self.running = false;
                return true;
            }
            self.reset_steps();
        }
        false
    }

    /// Returns the indices of the current group of active steps.
    fn current_step_group(&self) -> Vec<usize> {
        let mut group = Vec::new();
        for (i, step) in self.steps.iter().enumerate() {
            if step.is_complete() {
                continue;
            }
            group.push(i);
            // Look ahead for parallel steps
            for j in (i + 1)..self.steps.len() {
                if self.steps[j].parallel && !self.steps[j].is_complete() {
                    group.push(j);
                } else if !self.steps[j].parallel {
                    break;
                }
            }
            break;
        }
        group
    }

    /// Resets all steps for looping.
    fn reset_steps(&mut self) {
        for step in &mut self.steps {
            step.elapsed = 0.0;
        }
    }

    /// Returns the current values of all active steps.
    pub fn get_current_values(&self) -> Vec<(String, Variant)> {
        self.steps
            .iter()
            .map(|s| (s.property_path.clone(), s.current_value()))
            .collect()
    }
}

impl Default for Tween {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// TweenBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing tweens fluently.
pub struct TweenBuilder {
    tween: Tween,
}

impl TweenBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            tween: Tween::new(),
        }
    }

    /// Adds a property tween step.
    pub fn tween_property(
        mut self,
        path: impl Into<String>,
        start: Variant,
        end: Variant,
        duration: f64,
    ) -> Self {
        let step = TweenStep::new(path, start, end, duration);
        self.tween.steps.push(step);
        self
    }

    /// Sets the ease type on the last step.
    pub fn set_ease(mut self, ease_type: EaseType) -> Self {
        if let Some(step) = self.tween.steps.last_mut() {
            step.ease_type = ease_type;
        }
        self
    }

    /// Sets the transition function on the last step.
    pub fn set_trans(mut self, trans_func: TransFunc) -> Self {
        if let Some(step) = self.tween.steps.last_mut() {
            step.trans_func = trans_func;
        }
        self
    }

    /// Sets the delay on the last step.
    pub fn set_delay(mut self, delay: f64) -> Self {
        if let Some(step) = self.tween.steps.last_mut() {
            step.delay = delay;
        }
        self
    }

    /// Marks the last step as parallel with the previous one.
    pub fn parallel(mut self) -> Self {
        if let Some(step) = self.tween.steps.last_mut() {
            step.parallel = true;
        }
        self
    }

    /// Sets the number of loops.
    pub fn set_loops(mut self, loops: i32) -> Self {
        self.tween.set_loops(loops);
        self
    }

    /// Builds and returns the tween (already started).
    pub fn build(mut self) -> Tween {
        self.tween.start();
        self.tween
    }
}

impl Default for TweenBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Color, Vector2, Vector3};

    // -- ease() function ----------------------------------------------------

    #[test]
    fn ease_linear_is_identity() {
        for &et in &[EaseType::In, EaseType::Out, EaseType::InOut] {
            let val = ease(0.5, et, TransFunc::Linear);
            assert!((val - 0.5).abs() < 1e-10, "ease_type={et:?}");
        }
    }

    #[test]
    fn ease_boundaries() {
        for &tf in &[
            TransFunc::Linear,
            TransFunc::Sine,
            TransFunc::Quad,
            TransFunc::Cubic,
            TransFunc::Expo,
            TransFunc::Back,
            TransFunc::Bounce,
        ] {
            for &et in &[EaseType::In, EaseType::Out, EaseType::InOut] {
                let v0 = ease(0.0, et, tf);
                let v1 = ease(1.0, et, tf);
                assert!(
                    (v0 - 0.0).abs() < 1e-6,
                    "ease({:?}, {:?}) at t=0 = {v0}",
                    et,
                    tf
                );
                assert!(
                    (v1 - 1.0).abs() < 1e-6,
                    "ease({:?}, {:?}) at t=1 = {v1}",
                    et,
                    tf
                );
            }
        }
    }

    #[test]
    fn ease_clamps_input() {
        let v = ease(-0.5, EaseType::In, TransFunc::Quad);
        assert!((v - 0.0).abs() < 1e-10);

        let v = ease(1.5, EaseType::In, TransFunc::Quad);
        assert!((v - 1.0).abs() < 1e-10);
    }

    #[test]
    fn ease_quad_in_is_t_squared() {
        let val = ease(0.5, EaseType::In, TransFunc::Quad);
        assert!((val - 0.25).abs() < 1e-10);
    }

    #[test]
    fn ease_cubic_in() {
        let val = ease(0.5, EaseType::In, TransFunc::Cubic);
        assert!((val - 0.125).abs() < 1e-10);
    }

    #[test]
    fn ease_elastic_boundaries() {
        let v0 = ease(0.0, EaseType::In, TransFunc::Elastic);
        let v1 = ease(1.0, EaseType::In, TransFunc::Elastic);
        assert!((v0 - 0.0).abs() < 1e-10);
        assert!((v1 - 1.0).abs() < 1e-10);

        let v0 = ease(0.0, EaseType::Out, TransFunc::Elastic);
        let v1 = ease(1.0, EaseType::Out, TransFunc::Elastic);
        assert!((v0 - 0.0).abs() < 1e-10);
        assert!((v1 - 1.0).abs() < 1e-10);
    }

    // -- TweenStep ----------------------------------------------------------

    #[test]
    fn tween_step_basic() {
        let mut step = TweenStep::new("x", Variant::Float(0.0), Variant::Float(10.0), 1.0);
        assert!(!step.is_complete());

        step.elapsed = 0.5;
        let val = step.current_value();
        if let Variant::Float(f) = val {
            assert!((f - 5.0).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }

        step.elapsed = 1.0;
        assert!(step.is_complete());
        assert_eq!(step.current_value(), Variant::Float(10.0));
    }

    #[test]
    fn tween_step_with_delay() {
        let mut step = TweenStep::new("x", Variant::Float(0.0), Variant::Float(10.0), 1.0);
        step.delay = 0.5;

        step.elapsed = 0.25; // Still in delay
        assert_eq!(step.current_value(), Variant::Float(0.0));

        step.elapsed = 1.0; // 0.5s into the actual tween
        if let Variant::Float(f) = step.current_value() {
            assert!((f - 5.0).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }

        step.elapsed = 1.5;
        assert!(step.is_complete());
    }

    #[test]
    fn tween_step_zero_duration() {
        let step = TweenStep::new("x", Variant::Float(0.0), Variant::Float(10.0), 0.0);
        assert_eq!(step.current_value(), Variant::Float(10.0));
    }

    // -- Tween --------------------------------------------------------------

    #[test]
    fn tween_sequential_steps() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .tween_property("y", Variant::Float(0.0), Variant::Float(20.0), 1.0)
            .build();

        // Advance through first step
        tween.advance(1.0);
        assert!(!tween.advance(0.0)); // not done yet

        // Advance through second step
        let done = tween.advance(1.0);
        assert!(done);
        assert!(!tween.running);
    }

    #[test]
    fn tween_parallel_steps() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .tween_property("y", Variant::Float(0.0), Variant::Float(20.0), 1.0)
            .parallel()
            .build();

        // Both should complete in 1 second since they're parallel
        let done = tween.advance(1.0);
        assert!(done);
    }

    #[test]
    fn tween_looping() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .set_loops(2)
            .build();

        tween.advance(1.0); // First loop
        assert!(tween.running);

        let done = tween.advance(1.0); // Second loop
        assert!(done);
        assert!(!tween.running);
    }

    #[test]
    fn tween_infinite_loop() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .set_loops(-1)
            .build();

        for _ in 0..10 {
            let done = tween.advance(1.0);
            assert!(!done);
            assert!(tween.running);
        }
    }

    #[test]
    fn tween_get_current_values() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .build();

        tween.advance(0.5);
        let values = tween.get_current_values();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].0, "x");
        if let Variant::Float(f) = &values[0].1 {
            assert!((f - 5.0).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn tween_builder_with_ease_and_trans() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .set_ease(EaseType::In)
            .set_trans(TransFunc::Quad)
            .build();

        tween.advance(0.5);
        let values = tween.get_current_values();
        if let Variant::Float(f) = &values[0].1 {
            // Ease-in quad at t=0.5 → 0.25, so value = 2.5
            assert!((f - 2.5).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn tween_builder_with_delay() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .set_delay(0.5)
            .build();

        tween.advance(0.5); // Still in delay
        let values = tween.get_current_values();
        assert_eq!(values[0].1, Variant::Float(0.0));
    }

    #[test]
    fn tween_stop() {
        let mut tween = TweenBuilder::new()
            .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
            .build();

        tween.stop();
        let done = tween.advance(1.0);
        assert!(done); // Not running, so returns true
    }

    #[test]
    fn tween_empty() {
        let mut tween = Tween::new();
        tween.start();
        let done = tween.advance(1.0);
        assert!(done);
    }

    #[test]
    fn tween_default() {
        let tween = Tween::default();
        assert!(!tween.running);
        assert!(tween.steps.is_empty());
    }

    #[test]
    fn tween_builder_default() {
        let builder = TweenBuilder::default();
        let tween = builder.build();
        assert!(tween.running);
        assert!(tween.steps.is_empty());
    }

    #[test]
    fn ease_type_default() {
        assert_eq!(EaseType::default(), EaseType::InOut);
    }

    #[test]
    fn trans_func_default() {
        assert_eq!(TransFunc::default(), TransFunc::Linear);
    }

    // -- Vector2 tween ------------------------------------------------------

    #[test]
    fn tween_vector2_interpolation() {
        let mut tween = TweenBuilder::new()
            .tween_property(
                "position",
                Variant::Vector2(Vector2::new(0.0, 0.0)),
                Variant::Vector2(Vector2::new(10.0, 20.0)),
                1.0,
            )
            .build();

        tween.advance(0.5);
        let values = tween.get_current_values();
        if let Variant::Vector2(v) = &values[0].1 {
            assert!((v.x - 5.0).abs() < 1e-4);
            assert!((v.y - 10.0).abs() < 1e-4);
        } else {
            panic!("expected Vector2");
        }
    }

    #[test]
    fn tween_color_interpolation() {
        let mut tween = TweenBuilder::new()
            .tween_property(
                "color",
                Variant::Color(Color::BLACK),
                Variant::Color(Color::WHITE),
                1.0,
            )
            .build();

        tween.advance(0.5);
        let values = tween.get_current_values();
        if let Variant::Color(c) = &values[0].1 {
            assert!((c.r - 0.5).abs() < 1e-4);
            assert!((c.g - 0.5).abs() < 1e-4);
            assert!((c.b - 0.5).abs() < 1e-4);
        } else {
            panic!("expected Color");
        }
    }
}
