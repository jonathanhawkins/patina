//! Tween system for procedural property animation.
//!
//! Tweens provide a code-driven approach to animating properties, as an
//! alternative to keyframe-based [`Animation`](crate::animation::Animation).
//! They support easing functions, delays, parallel execution, and looping.

use std::fmt;

use crate::animation::interpolate_variant;
use gdcore::ObjectId;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// TweenId
// ---------------------------------------------------------------------------

/// A lightweight identifier for a tween within the scene tree's tween store.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TweenId(ObjectId);

impl TweenId {
    /// Creates a new, globally unique `TweenId`.
    pub fn next() -> Self {
        Self(ObjectId::next())
    }

    /// Returns the raw `u64` value.
    pub fn raw(self) -> u64 {
        self.0.raw()
    }
}

impl fmt::Debug for TweenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TweenId({})", self.0.raw())
    }
}

impl fmt::Display for TweenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.raw())
    }
}

// ---------------------------------------------------------------------------
// EaseType
// ---------------------------------------------------------------------------

/// Controls the acceleration profile of the easing curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EaseType {
    /// Ease in (slow start).
    In,
    /// Ease out (slow end).
    Out,
    /// Ease in then out (slow start and end).
    #[default]
    InOut,
}

// ---------------------------------------------------------------------------
// TransFunc
// ---------------------------------------------------------------------------

/// The mathematical function used for easing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransFunc {
    /// No easing — constant speed.
    #[default]
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
                2.0_f64.powf(-10.0 * t) * ((t - p / 4.0) * std::f64::consts::TAU / p).sin() + 1.0
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

/// The kind of work a [`TweenStep`] performs. `Property` interpolates a node
/// property (the original behaviour); the others mirror Godot's `tween_method`,
/// `tween_callback`, and `tween_interval` tweeners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TweenerKind {
    /// Interpolates `property_path` from start to end (default).
    #[default]
    Property,
    /// Interpolates start..end and delivers the value to a script method each
    /// frame (Godot `tween_method`). The method name is in [`TweenStep::method`].
    Method,
    /// Fires a script method once when the step is reached (Godot `tween_callback`).
    Callback,
    /// A pure time delay with no output (Godot `tween_interval`).
    Interval,
    /// Runs a nested [`Tween`] to completion as one step (Godot
    /// `tween_subtween`, pat-nzgdu.186). The nested tween lives in
    /// [`TweenStep::subtween`].
    Subtween,
}

/// A single step within a tween — a property transition, a `tween_method`
/// interpolation, a `tween_callback`, or a `tween_interval` delay.
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
    /// What this step does (property/method/callback/interval).
    pub kind: TweenerKind,
    /// Script method name for `Method`/`Callback` steps (empty otherwise).
    pub method: String,
    /// Callback fire-once guard (re-armed on loop).
    fired: bool,
    /// The nested tween for a `Subtween` step (Godot `tween_subtween`,
    /// pat-nzgdu.186); `None` for every other kind. Boxed to break the
    /// otherwise-recursive `TweenStep` → `Tween` → `TweenStep` type.
    subtween: Option<Box<Tween>>,
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
            kind: TweenerKind::Property,
            method: String::new(),
            fired: false,
            subtween: None,
        }
    }

    /// Creates a `tween_method` step: interpolates `start`..`end` over
    /// `duration`, delivering the value to the script `method` each frame.
    pub fn method(method: impl Into<String>, start: Variant, end: Variant, duration: f64) -> Self {
        let mut s = Self::new(String::new(), start, end, duration);
        s.kind = TweenerKind::Method;
        s.method = method.into();
        s
    }

    /// Creates a `tween_callback` step: fires `method` once when the sequence
    /// reaches it.
    pub fn callback(method: impl Into<String>) -> Self {
        let mut s = Self::new(String::new(), Variant::Nil, Variant::Nil, 0.0);
        s.kind = TweenerKind::Callback;
        s.method = method.into();
        s
    }

    /// Creates a `tween_interval` step: a pure `duration`-second delay with no
    /// output.
    pub fn interval(duration: f64) -> Self {
        let mut s = Self::new(String::new(), Variant::Nil, Variant::Nil, duration);
        s.kind = TweenerKind::Interval;
        s
    }

    /// Creates a `tween_subtween` step: runs a nested [`Tween`] to completion as
    /// a single step (Godot `Tween.tween_subtween`, pat-nzgdu.186). The step's
    /// duration is the nested tween's total duration, so it blocks the next
    /// chained step until the nested tween finishes. The nested tween is
    /// started so it advances alongside the parent.
    pub fn subtween(mut nested: Tween) -> Self {
        let dur = nested.total_duration();
        nested.start();
        let mut s = Self::new(String::new(), Variant::Nil, Variant::Nil, dur);
        s.kind = TweenerKind::Subtween;
        s.subtween = Some(Box::new(nested));
        s
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
    /// Playback speed multiplier applied to every `advance` (Godot
    /// `set_speed_scale`). Defaults to 1.0.
    pub speed_scale: f64,
    /// When paused, `advance` is a no-op (Godot `pause`).
    paused: bool,
    /// When killed, the tween is permanently inert (Godot `kill`).
    killed: bool,
    /// Set for one drain when the tween completes (Godot `finished` signal).
    finished_pending: bool,
}

impl Tween {
    /// Creates a new empty tween.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            running: false,
            loops: 1,
            loops_total: 1,
            speed_scale: 1.0,
            paused: false,
            killed: false,
            finished_pending: false,
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

    /// Total duration of one loop in seconds: the sum, over each sequential
    /// step-group, of the group's longest `delay + duration` (parallel steps
    /// overlap). Used to size a `tween_subtween` step (pat-nzgdu.186).
    pub fn total_duration(&self) -> f64 {
        let mut total = 0.0;
        let mut i = 0;
        while i < self.steps.len() {
            let mut group_max = self.steps[i].delay + self.steps[i].duration;
            let mut j = i + 1;
            while j < self.steps.len() && self.steps[j].parallel {
                group_max = group_max.max(self.steps[j].delay + self.steps[j].duration);
                j += 1;
            }
            total += group_max;
            i = j;
        }
        total
    }

    /// Advances the tween by `delta` seconds. Returns `true` when fully complete.
    /// A killed tween is inert; a paused tween does not progress.
    pub fn advance(&mut self, delta: f64) -> bool {
        if self.killed {
            return true;
        }
        if self.paused {
            return false;
        }
        if !self.running {
            return true;
        }
        if self.steps.is_empty() {
            self.running = false;
            return true;
        }

        // Process steps in groups: sequential steps form barriers,
        // parallel steps run together with the previous sequential step.
        let mut remaining = delta * self.speed_scale;
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
                    self.finished_pending = true;
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
                // A subtween step drives its nested tween forward in lockstep so
                // the nested properties animate too (pat-nzgdu.186).
                if self.steps[idx].kind == TweenerKind::Subtween {
                    if let Some(sub) = self.steps[idx].subtween.as_mut() {
                        sub.advance(advance_by);
                    }
                }
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
                self.finished_pending = true;
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

    /// Resets all steps for looping (re-arming callbacks and restarting any
    /// nested subtweens).
    fn reset_steps(&mut self) {
        for step in &mut self.steps {
            step.elapsed = 0.0;
            step.fired = false;
            if let Some(sub) = step.subtween.as_mut() {
                sub.reset_steps();
                sub.start();
            }
        }
    }

    /// Returns the current values of all active **property** steps. Method,
    /// callback, and interval steps are excluded — they surface through
    /// [`method_values`](Tween::method_values) / [`poll_callbacks`](Tween::poll_callbacks).
    pub fn get_current_values(&self) -> Vec<(String, Variant)> {
        let mut out: Vec<(String, Variant)> = self
            .steps
            .iter()
            .filter(|s| s.kind == TweenerKind::Property)
            .map(|s| (s.property_path.clone(), s.current_value()))
            .collect();
        // Bubble nested subtween property values up so a parent tween's consumer
        // observes the nested animation as well (pat-nzgdu.186).
        for step in &self.steps {
            if step.kind == TweenerKind::Subtween {
                if let Some(sub) = step.subtween.as_ref() {
                    out.extend(sub.get_current_values());
                }
            }
        }
        out
    }

    /// Returns `(method_name, interpolated_value)` for every active
    /// `tween_method` step — the values Godot delivers to the method each frame.
    pub fn method_values(&self) -> Vec<(String, Variant)> {
        self.current_step_group()
            .iter()
            .filter_map(|&i| {
                let s = &self.steps[i];
                (s.kind == TweenerKind::Method).then(|| (s.method.clone(), s.current_value()))
            })
            .collect()
    }

    /// Fires any `tween_callback` steps the sequence has now reached (all
    /// earlier steps complete), returning their method names in sequence order.
    /// Each callback fires once per loop.
    pub fn poll_callbacks(&mut self) -> Vec<String> {
        let mut fired_now = Vec::new();
        for i in 0..self.steps.len() {
            if self.steps[i].kind != TweenerKind::Callback || self.steps[i].fired {
                continue;
            }
            let reached = self.steps[..i].iter().all(TweenStep::is_complete);
            if reached {
                self.steps[i].fired = true;
                fired_now.push(self.steps[i].method.clone());
            }
        }
        fired_now
    }

    /// Pauses playback — subsequent `advance` calls are no-ops until [`play`](Tween::play).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resumes playback after a [`pause`](Tween::pause).
    pub fn play(&mut self) {
        self.paused = false;
    }

    /// `true` while paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Permanently stops the tween (Godot `kill`); it becomes inert and invalid.
    pub fn kill(&mut self) {
        self.killed = true;
        self.running = false;
    }

    /// `true` until the tween has been [`kill`](Tween::kill)ed (Godot `is_valid`).
    pub fn is_valid(&self) -> bool {
        !self.killed
    }

    /// Sets the playback speed multiplier (clamped to ≥ 0).
    pub fn set_speed_scale(&mut self, scale: f64) {
        self.speed_scale = scale.max(0.0);
    }

    /// Returns `true` exactly once after the tween completes, draining the
    /// `finished` signal.
    pub fn take_just_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished_pending)
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

    #[test]
    fn tween_method_interpolates_and_reports_value() {
        let mut t = Tween::new();
        t.steps.push(TweenStep::method(
            "_set_alpha",
            Variant::Float(0.0),
            Variant::Float(10.0),
            1.0,
        ));
        t.start();
        // Method steps do not emit property values.
        assert!(t.get_current_values().is_empty());

        t.advance(0.5);
        let mv = t.method_values();
        assert_eq!(mv.len(), 1);
        assert_eq!(mv[0].0, "_set_alpha");
        match mv[0].1 {
            Variant::Float(f) => assert!((f - 5.0).abs() < 1e-6, "midpoint value {f}"),
            ref other => panic!("expected float, got {other:?}"),
        }
    }

    #[test]
    fn tween_callbacks_fire_in_sequence_order() {
        let mut t = Tween::new();
        t.steps.push(TweenStep::new(
            "x",
            Variant::Float(0.0),
            Variant::Float(1.0),
            0.5,
        ));
        t.steps.push(TweenStep::callback("_first"));
        t.steps.push(TweenStep::callback("_second"));
        t.start();

        // Before the property step completes, no callback fires.
        t.advance(0.25);
        assert!(t.poll_callbacks().is_empty());

        // Once it completes, both callbacks fire in sequence order.
        t.advance(0.5);
        assert_eq!(
            t.poll_callbacks(),
            vec!["_first".to_string(), "_second".to_string()]
        );
        // They do not fire again.
        assert!(t.poll_callbacks().is_empty());
    }

    #[test]
    fn tween_interval_delays_without_output() {
        let mut t = Tween::new();
        t.steps.push(TweenStep::interval(1.0));
        t.steps.push(TweenStep::new(
            "y",
            Variant::Float(0.0),
            Variant::Float(4.0),
            1.0,
        ));
        t.start();

        // Mid-interval: only the sequential property "y" surfaces, still at start.
        t.advance(0.5);
        let vals = t.get_current_values();
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].0, "y");
        match vals[0].1 {
            Variant::Float(f) => assert!(f.abs() < 1e-6, "y should be 0 during interval, got {f}"),
            ref other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tween_finished_signal_and_lifecycle() {
        let mut t = Tween::new();
        t.steps.push(TweenStep::new(
            "x",
            Variant::Float(0.0),
            Variant::Float(1.0),
            1.0,
        ));
        t.start();

        // speed_scale doubles effective progress.
        t.set_speed_scale(2.0);
        t.advance(0.25); // effective 0.5
        assert!(!t.take_just_finished());

        // pause halts progress entirely.
        t.pause();
        assert!(t.is_paused());
        t.advance(1.0); // no-op while paused
        assert!(!t.take_just_finished());
        t.play();

        // Finish it (effective 1.0 → total elapsed 1.5 ≥ 1.0).
        t.advance(0.5);
        assert!(t.take_just_finished(), "finished signal set on completion");
        assert!(!t.take_just_finished(), "signal drained once");

        // kill makes it invalid + inert.
        assert!(t.is_valid());
        t.kill();
        assert!(!t.is_valid());
        assert!(
            t.advance(1.0),
            "killed tween reports complete and does nothing"
        );
    }
}
