//! Animation tracks, keyframes, and AnimationPlayer.
//!
//! Provides a Godot-compatible animation system with keyframe-based tracks,
//! loop modes, and an [`AnimationPlayer`] that advances playback and samples
//! track values at the current position.

use std::collections::{HashMap, VecDeque};

use gdcore::math::{Color, Vector2, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// TransitionType
// ---------------------------------------------------------------------------

/// How values are interpolated between keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TransitionType {
    /// Linear interpolation between keyframe values.
    #[default]
    Linear,
    /// Snap to the nearest keyframe value (no interpolation).
    Nearest,
    /// Cubic Bézier interpolation with two control points.
    ///
    /// The four f32 values represent `(x1, y1, x2, y2)` of the two control
    /// points in the unit square `[0,1]×[0,1]` (CSS cubic-bezier convention).
    /// `x1`/`x2` are clamped to `[0,1]`; `y1`/`y2` may exceed `[0,1]` for
    /// overshoot effects.
    CubicBezier(f32, f32, f32, f32),
}

impl TransitionType {
    /// Returns `true` if this is a cubic bézier transition.
    pub fn is_bezier(&self) -> bool {
        matches!(self, Self::CubicBezier(..))
    }

    /// Standard ease-in preset: slow start, fast end.
    pub const EASE_IN: Self = Self::CubicBezier(0.42, 0.0, 1.0, 1.0);
    /// Standard ease-out preset: fast start, slow end.
    pub const EASE_OUT: Self = Self::CubicBezier(0.0, 0.0, 0.58, 1.0);
    /// Standard ease-in-out preset: slow start and end.
    pub const EASE_IN_OUT: Self = Self::CubicBezier(0.42, 0.0, 0.58, 1.0);
}

// ---------------------------------------------------------------------------
// KeyFrame
// ---------------------------------------------------------------------------

/// A single keyframe in an animation track.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyFrame {
    /// Time in seconds within the animation.
    pub time: f64,
    /// The value at this keyframe.
    pub value: Variant,
    /// How to transition *to* this keyframe from the previous one.
    pub transition: TransitionType,
    /// Godot's per-key `transition` **exponent** — the ease applied to the
    /// interpolation factor (pat-mgixk.80).
    ///
    /// ⚠️ Distinct from [`KeyFrame::transition`] above, which is Patina's
    /// per-key *interpolation mode*. Godot has both: a track-wide
    /// `interpolation_type` and a per-key `transition: real_t`
    /// (`animation.cpp:2663`). Naming them alike is unfortunate but the
    /// spellings are Godot's.
    ///
    /// 🔑 Read off the **floor** key when sampling, not the next one, because
    /// upstream takes `p_keys[idx].transition` where `idx` is the last key at
    /// or before the time. 1.0 is linear (no ease); **0.0 holds the floor key's
    /// value** rather than easing it.
    pub ease: f32,
    /// Bézier-track incoming control handle.
    ///
    /// Only read for [`TrackType::Bezier`]; zero on every other track type.
    /// This is a `(time, value)` offset **relative to this key's own point**,
    /// not an absolute position — Godot forms the control point as
    /// `Vector2(duration, value) + in_handle`
    /// (`upstream/godot/scene/resources/animation.cpp`,
    /// `bezier_track_interpolate`). Its `x` is normally negative, since the
    /// incoming handle reaches backwards in time.
    pub in_handle: Vector2,
    /// Bézier-track outgoing control handle, relative to this key's point (see
    /// [`KeyFrame::in_handle`]). Its `x` is normally positive.
    pub out_handle: Vector2,
}

impl KeyFrame {
    /// Creates a new keyframe.
    pub fn new(time: f64, value: Variant, transition: TransitionType) -> Self {
        Self {
            time,
            value,
            transition,
            // Godot's default per-key transition is 1.0 — linear, no ease.
            ease: 1.0,
            in_handle: Vector2::ZERO,
            out_handle: Vector2::ZERO,
        }
    }

    /// Creates a Bézier keyframe: a scalar value plus its two control handles.
    ///
    /// Both handles are offsets relative to `(time, value)` — see
    /// [`KeyFrame::in_handle`]. Zero handles make the segment a straight line,
    /// which is why a bézier track with unset handles samples linearly rather
    /// than degenerately.
    pub fn bezier(time: f64, value: f64, in_handle: Vector2, out_handle: Vector2) -> Self {
        Self {
            time,
            value: Variant::Float(value),
            transition: TransitionType::Linear,
            ease: 1.0,
            in_handle,
            out_handle,
        }
    }

    /// Creates a linear keyframe (convenience).
    pub fn linear(time: f64, value: Variant) -> Self {
        Self::new(time, value, TransitionType::Linear)
    }
}

// ---------------------------------------------------------------------------
// TrackType
// ---------------------------------------------------------------------------

/// The type of an animation track, matching Godot's track categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TrackType {
    /// Animates a node property over time (position, rotation, color, etc.).
    #[default]
    Property,
    /// Calls a method on the target node at each keyframe time.
    /// Keyframe values should be `Variant::Array` of `[method_name, arg1, arg2, ...]`.
    Method,
    /// Triggers audio playback at keyframe times.
    /// Keyframe values should be `Variant::Dictionary` with keys:
    /// `"stream"` (resource path), `"start_offset"` (float), `"end_offset"` (float).
    Audio,
    /// Animates a `Node3D`'s position (`Variant::Vector3`, linear) — pat-u3l8c.95.
    Position3D,
    /// Animates a `Node3D`'s rotation (`Variant::Quaternion`, shortest-path slerp).
    Rotation3D,
    /// Animates a `Node3D`'s scale (`Variant::Vector3`, linear).
    Scale3D,
    /// Animates a mesh blend-shape weight (`Variant::Float`).
    BlendShape,
    /// A Bézier curve over a single scalar property (pat-s3rrv.44).
    ///
    /// Keyframe values are `Variant::Float`, and the curve between two keys is
    /// set by the left key's `out_handle` and the right key's `in_handle`
    /// rather than by a [`TransitionType`]. Distinct from
    /// [`TransitionType::CubicBezier`], which is a per-key *easing preset*
    /// applied to an ordinary property track — a bézier TRACK carries its own
    /// control points per key and always drives a scalar.
    Bezier,
    /// Starts a named clip on another `AnimationPlayer` at each keyframe time
    /// (pat-s3rrv.44). Keyframe values are `Variant::String` clip names, and
    /// `node_path` addresses the nested player.
    Animation,
}

impl TrackType {
    /// Parses a track type from a string name.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "property" => Some(Self::Property),
            "method" => Some(Self::Method),
            "audio" => Some(Self::Audio),
            "position_3d" => Some(Self::Position3D),
            "rotation_3d" => Some(Self::Rotation3D),
            "scale_3d" => Some(Self::Scale3D),
            "blend_shape" => Some(Self::BlendShape),
            "bezier" => Some(Self::Bezier),
            "animation" => Some(Self::Animation),
            _ => None,
        }
    }

    /// Returns the string name for this track type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Property => "property",
            Self::Method => "method",
            Self::Audio => "audio",
            Self::Position3D => "position_3d",
            Self::Rotation3D => "rotation_3d",
            Self::Scale3D => "scale_3d",
            Self::BlendShape => "blend_shape",
            Self::Bezier => "bezier",
            Self::Animation => "animation",
        }
    }
}

// ---------------------------------------------------------------------------
// TrackUpdateMode / TrackInterpolation (pat-u3l8c.102)
// ---------------------------------------------------------------------------

/// How a value track's sampled value is updated over time, mirroring Godot's
/// `Animation.UpdateMode`.
///
/// Named `TrackUpdateMode` (not `UpdateMode`) to avoid colliding with the
/// unrelated [`crate::sub_viewport::UpdateMode`] re-exported at the crate root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackUpdateMode {
    /// Interpolate continuously between keys (default — legacy behavior).
    #[default]
    Continuous,
    /// Step function: hold the value of the most-recent key at-or-before the
    /// sample time, with no interpolation (Godot `UPDATE_DISCRETE`).
    Discrete,
    /// Godot `UPDATE_CAPTURE`. Treated the same as
    /// [`Continuous`](Self::Continuous) for sampling here (capture blending is
    /// not yet modeled).
    Capture,
}

/// The numeric interpolation curve applied per track, mirroring Godot's
/// `Animation.InterpolationType`.
///
/// The default [`PerKey`](Self::PerKey) preserves the legacy per-keyframe
/// [`TransitionType`] behavior exactly; the other variants override it with
/// Godot-accurate curves. A per-track selector is used (rather than new
/// [`TransitionType`] variants) so the interpolation type stays close to
/// Godot's model and so existing exhaustive matches on [`TransitionType`]
/// elsewhere remain valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackInterpolation {
    /// Use each keyframe's per-key [`TransitionType`] (Linear / Nearest /
    /// CubicBezier). Default — unchanged legacy behavior.
    #[default]
    PerKey,
    /// Godot `INTERPOLATION_CUBIC`: Catmull-Rom cubic through neighbor keys,
    /// passing through the surrounding key values.
    Cubic,
    /// Godot `INTERPOLATION_LINEAR_ANGLE`: shortest-path angular lerp across the
    /// 2π wrap. Only affects `Float` values (radians); other types fall back to
    /// linear.
    LinearAngle,
    /// Godot `INTERPOLATION_CUBIC_ANGLE`: cubic on the unwrapped angle sequence.
    /// Only affects `Float` values (radians); other types fall back to cubic.
    CubicAngle,
}

// ---------------------------------------------------------------------------
// AnimationTrack
// ---------------------------------------------------------------------------

/// A track that animates a single property over time via keyframes.
///
/// The `track_type` field determines how keyframe values are interpreted:
/// - `Property`: values are property values to interpolate.
/// - `Method`: values are `[method_name, ...args]` arrays to call.
/// - `Audio`: values are `{"stream": path, "start_offset": f, "end_offset": f}` dicts.
#[derive(Debug, Clone)]
pub struct AnimationTrack {
    /// The node path this track targets (e.g. `"Player"`).
    pub node_path: String,
    /// The property path this track targets (e.g. `"position:x"`).
    ///
    /// For method tracks, this is the method name prefix.
    /// For audio tracks, this is a descriptive label.
    pub property_path: String,
    /// The type of this track.
    pub track_type: TrackType,
    /// How the sampled value updates over time (Godot `UpdateMode`). Defaults
    /// to [`TrackUpdateMode::Continuous`] (pat-u3l8c.102).
    pub update_mode: TrackUpdateMode,
    /// The per-track interpolation curve (Godot `InterpolationType`). Defaults
    /// to [`TrackInterpolation::PerKey`], preserving legacy behavior
    /// (pat-u3l8c.102).
    pub interpolation: TrackInterpolation,
    /// Keyframes sorted by time.
    keyframes: Vec<KeyFrame>,
}

impl AnimationTrack {
    /// Creates a new empty property track for the given property path.
    pub fn new(property_path: impl Into<String>) -> Self {
        Self {
            node_path: String::new(),
            property_path: property_path.into(),
            track_type: TrackType::Property,
            update_mode: TrackUpdateMode::Continuous,
            interpolation: TrackInterpolation::PerKey,
            keyframes: Vec::new(),
        }
    }

    /// Creates a new empty property track with both node path and property path.
    pub fn with_node(node_path: impl Into<String>, property_path: impl Into<String>) -> Self {
        Self {
            node_path: node_path.into(),
            property_path: property_path.into(),
            track_type: TrackType::Property,
            update_mode: TrackUpdateMode::Continuous,
            interpolation: TrackInterpolation::PerKey,
            keyframes: Vec::new(),
        }
    }

    /// Creates a new track with an explicit type.
    pub fn with_type(
        node_path: impl Into<String>,
        property_path: impl Into<String>,
        track_type: TrackType,
    ) -> Self {
        Self {
            node_path: node_path.into(),
            property_path: property_path.into(),
            track_type,
            update_mode: TrackUpdateMode::Continuous,
            interpolation: TrackInterpolation::PerKey,
            keyframes: Vec::new(),
        }
    }

    /// Returns the track type.
    pub fn track_type(&self) -> TrackType {
        self.track_type
    }

    /// Sets the track's [`TrackUpdateMode`] (Godot `track_set_update_mode`).
    /// Returns `&mut self` for chaining.
    pub fn set_update_mode(&mut self, mode: TrackUpdateMode) -> &mut Self {
        self.update_mode = mode;
        self
    }

    /// Sets the track's [`TrackInterpolation`] curve (Godot
    /// `track_set_interpolation_type`). Returns `&mut self` for chaining.
    pub fn set_interpolation(&mut self, interp: TrackInterpolation) -> &mut Self {
        self.interpolation = interp;
        self
    }

    /// Adds a keyframe, maintaining sorted order by time.
    pub fn add_keyframe(&mut self, kf: KeyFrame) {
        let pos = self
            .keyframes
            .binary_search_by(|k| {
                k.time
                    .partial_cmp(&kf.time)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|e| e);
        self.keyframes.insert(pos, kf);
    }

    /// Returns the keyframes (sorted by time).
    pub fn keyframes(&self) -> &[KeyFrame] {
        &self.keyframes
    }

    /// Removes the keyframe at the given index. Returns `true` if removed.
    pub fn remove_keyframe(&mut self, index: usize) -> bool {
        if index < self.keyframes.len() {
            self.keyframes.remove(index);
            true
        } else {
            false
        }
    }

    /// Returns the number of keyframes.
    pub fn keyframe_count(&self) -> usize {
        self.keyframes.len()
    }

    /// Samples this track at the given time, interpolating between keyframes.
    pub fn sample(&self, time: f64) -> Option<Variant> {
        if self.keyframes.is_empty() {
            return None;
        }

        // A bézier track carries its curve in the keys' own handles, so it does
        // not reach the transition/interpolation logic below at all
        // (pat-s3rrv.44).
        if self.track_type == TrackType::Bezier {
            return self.sample_bezier(time);
        }

        // Discrete update mode (pat-u3l8c.102): step function — return the value
        // of the most-recent key at-or-before `time` (or the first key when
        // `time` precedes all keys), with no interpolation. `Continuous` and
        // `Capture` fall through to the interpolating path below unchanged.
        //
        // An animation track is inherently discrete: its value names a clip to
        // start, and interpolating between two clip NAMES is meaningless
        // (pat-s3rrv.44).
        if self.update_mode == TrackUpdateMode::Discrete || self.track_type == TrackType::Animation
        {
            let mut chosen = &self.keyframes[0];
            for kf in &self.keyframes {
                if kf.time <= time {
                    chosen = kf;
                } else {
                    break;
                }
            }
            return Some(chosen.value.clone());
        }

        if self.keyframes.len() == 1 {
            return Some(self.keyframes[0].value.clone());
        }

        // Before first keyframe
        if time <= self.keyframes[0].time {
            return Some(self.keyframes[0].value.clone());
        }

        // After last keyframe
        let last = &self.keyframes[self.keyframes.len() - 1];
        if time >= last.time {
            return Some(last.value.clone());
        }

        // Find the two surrounding keyframes
        for i in 0..self.keyframes.len() - 1 {
            let a = &self.keyframes[i];
            let b = &self.keyframes[i + 1];
            if time >= a.time && time <= b.time {
                // Per-track interpolation (pat-u3l8c.102) overrides the per-key
                // transition curve. The default `PerKey` falls through to the
                // unchanged legacy transition logic below.
                // Godot applies the FLOOR key's `transition` exponent to the
                // normalised factor before the interpolation switch, and a
                // transition of 0 holds that key's value outright
                // (`animation.cpp:2663-2671`) — pat-mgixk.80.
                if a.ease == 0.0 {
                    return Some(a.value.clone());
                }
                if self.interpolation != TrackInterpolation::PerKey {
                    let duration = b.time - a.time;
                    if duration < 1e-10 {
                        return Some(b.value.clone());
                    }
                    let t = eased_factor(((time - a.time) / duration) as f32, a.ease);
                    return self.sample_interpolated(i, t);
                }
                match b.transition {
                    TransitionType::Nearest => {
                        // 🔑 The FLOOR key, held until the next one is reached —
                        // upstream is literally `return p_keys[idx].value`
                        // (`animation.cpp:2673-2675`). This used to switch at the
                        // MIDPOINT between the two keys, so every NEAREST track
                        // changed value half a frame-interval early and a
                        // discrete clip stepped at the wrong times.
                        return Some(a.value.clone());
                    }
                    TransitionType::Linear => {
                        let duration = b.time - a.time;
                        if duration < 1e-10 {
                            return Some(b.value.clone());
                        }
                        let t = eased_factor(((time - a.time) / duration) as f32, a.ease);
                        return interpolate_variant(&a.value, &b.value, t)
                            .or_else(|| Some(a.value.clone()));
                    }
                    TransitionType::CubicBezier(x1, y1, x2, y2) => {
                        let duration = b.time - a.time;
                        if duration < 1e-10 {
                            return Some(b.value.clone());
                        }
                        let linear_t = ((time - a.time) / duration) as f32;
                        let eased_t = cubic_bezier_y(x1, y1, x2, y2, linear_t);
                        return interpolate_variant(&a.value, &b.value, eased_t)
                            .or_else(|| Some(a.value.clone()));
                    }
                }
            }
        }

        Some(last.value.clone())
    }

    /// Samples a [`TrackType::Bezier`] track, mirroring Godot's
    /// `Animation::bezier_track_interpolate`
    /// (`upstream/godot/scene/resources/animation.cpp`).
    ///
    /// The control polygon is expressed in **segment-local time**: the left key
    /// sits at `x = 0` and the right key at `x = duration`, with each handle
    /// added as an offset to its own endpoint. Because the curve is parametric,
    /// the parameter that reaches a given time is *not* `elapsed / duration` —
    /// Godot binary-searches it, and this reproduces that search (including its
    /// fixed 10 iterations) so sampled values agree with Godot's.
    ///
    /// Godot does not loop-wrap bézier tracks; times outside the key range
    /// clamp to the end values.
    fn sample_bezier(&self, time: f64) -> Option<Variant> {
        let keys = &self.keyframes;
        let first = keys.first()?;
        let last = keys.last()?;
        if keys.len() == 1 || time <= first.time {
            return Some(Variant::Float(bezier_key_value(first)));
        }
        if time >= last.time {
            return Some(Variant::Float(bezier_key_value(last)));
        }
        // `time` is strictly inside the range, so a preceding key exists and is
        // not the final one.
        let i = keys.iter().rposition(|k| k.time <= time)?;
        let (a, b) = (&keys[i], keys.get(i + 1)?);

        let duration = (b.time - a.time) as f32;
        if duration < 1e-10 {
            return Some(Variant::Float(bezier_key_value(b)));
        }
        let elapsed = (time - a.time) as f32;

        let start = Vector2::new(0.0, bezier_key_value(a) as f32);
        let start_out = Vector2::new(start.x + a.out_handle.x, start.y + a.out_handle.y);
        let end = Vector2::new(duration, bezier_key_value(b) as f32);
        let end_in = Vector2::new(end.x + b.in_handle.x, end.y + b.in_handle.y);

        let (mut low, mut high) = (0.0f32, 1.0f32);
        for _ in 0..10 {
            let middle = (low + high) / 2.0;
            if bezier_point(start, start_out, end_in, end, middle).x < elapsed {
                low = middle;
            } else {
                high = middle;
            }
        }

        let low_pos = bezier_point(start, start_out, end_in, end, low);
        let high_pos = bezier_point(start, start_out, end_in, end, high);
        let span = high_pos.x - low_pos.x;
        // Godot divides unguarded here; a degenerate bracket would yield NaN and
        // silently poison the driven property, so collapse to the low endpoint.
        let value = if span.abs() < 1e-10 {
            low_pos.y
        } else {
            let c = (elapsed - low_pos.x) / span;
            low_pos.y + (high_pos.y - low_pos.y) * c
        };
        Some(Variant::Float(value as f64))
    }

    /// Applies the non-default per-track [`TrackInterpolation`] curve to the
    /// segment between keyframes `i` (`from`, local `t=0`) and `i+1` (`to`,
    /// local `t=1`), using neighbor keys `i-1` (`pre`) and `i+2` (`post`) for
    /// cubic tangents. Missing neighbors clamp to `from`/`to` respectively.
    ///
    /// Unsupported [`Variant`] types (and the angle modes on non-`Float`
    /// values) fall back to linear interpolation, then to `from` (pat-u3l8c.102).
    fn sample_interpolated(&self, i: usize, t: f32) -> Option<Variant> {
        let from = &self.keyframes[i].value;
        let to = &self.keyframes[i + 1].value;
        let pre = if i > 0 {
            &self.keyframes[i - 1].value
        } else {
            from
        };
        let post = if i + 2 < self.keyframes.len() {
            &self.keyframes[i + 2].value
        } else {
            to
        };
        let out = match self.interpolation {
            TrackInterpolation::Cubic => cubic_interpolate_variant(pre, from, to, post, t),
            TrackInterpolation::LinearAngle => linear_angle_variant(from, to, t),
            TrackInterpolation::CubicAngle => cubic_angle_variant(pre, from, to, post, t),
            // `PerKey` is guarded out by the caller; keep the match exhaustive.
            TrackInterpolation::PerKey => None,
        };
        out.or_else(|| interpolate_variant(from, to, t))
            .or_else(|| Some(from.clone()))
    }
}

// cubic_bezier_y defined below as public function

// ---------------------------------------------------------------------------
// LoopMode
// ---------------------------------------------------------------------------

/// How an animation wraps when it reaches its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    /// Play once and stop at the end.
    #[default]
    None,
    /// Loop back to the start when the end is reached.
    Linear,
    /// Play forward then backward, repeating.
    PingPong,
}

// ---------------------------------------------------------------------------
// Animation
// ---------------------------------------------------------------------------

/// A named collection of animation tracks with a duration and loop mode.
#[derive(Debug, Clone)]
pub struct Animation {
    /// The animation's name.
    pub name: String,
    /// Total length of the animation in seconds.
    pub length: f64,
    /// How the animation loops.
    pub loop_mode: LoopMode,
    /// The tracks in this animation.
    pub tracks: Vec<AnimationTrack>,
}

impl Animation {
    /// Creates a new empty animation.
    pub fn new(name: impl Into<String>, length: f64) -> Self {
        Self {
            name: name.into(),
            length,
            loop_mode: LoopMode::None,
            tracks: Vec::new(),
        }
    }

    /// Samples a specific track at the given time.
    pub fn sample_track(&self, track_idx: usize, time: f64) -> Option<Variant> {
        self.tracks.get(track_idx)?.sample(time)
    }

    /// Samples all tracks at the given time, returning `(property_path, value)` pairs.
    pub fn sample_all(&self, time: f64) -> Vec<(String, Variant)> {
        let mut results = Vec::new();
        for track in &self.tracks {
            if let Some(value) = track.sample(time) {
                results.push((track.property_path.clone(), value));
            }
        }
        results
    }

    /// Like [`sample_all`](Self::sample_all) but also returns each track's
    /// `node_path`, so callers can resolve the target node (pat-u3l8c.94).
    /// Yields `(node_path, property_path, value)` triples.
    pub fn sample_all_targeted(&self, time: f64) -> Vec<(String, String, Variant)> {
        self.sample_all_typed(time)
            .into_iter()
            .map(|(node, prop, _, value)| (node, prop, value))
            .collect()
    }

    /// Like [`sample_all_targeted`](Self::sample_all_targeted) but also reports
    /// each track's [`TrackType`] (pat-ghw2s). Yields
    /// `(node_path, property_path, track_type, value)`.
    ///
    /// ⚠️ A bone is driven by up to THREE tracks with the same
    /// `(node_path, property_path)` — `Skeleton3D:hip` as `position_3d`,
    /// `rotation_3d` and `scale_3d`. Only the type tells them apart, so any
    /// consumer that pairs values with types by `(node_path, property_path)`
    /// silently routes a position into the scale.
    pub fn sample_all_typed(&self, time: f64) -> Vec<(String, String, TrackType, Variant)> {
        let mut results = Vec::new();
        for track in &self.tracks {
            if let Some(value) = track.sample(time) {
                results.push((
                    track.node_path.clone(),
                    track.property_path.clone(),
                    track.track_type,
                    value,
                ));
            }
        }
        results
    }
}

/// The blend CHANNEL of a track type: the part of a track's identity, beyond
/// `(node_path, property_path)`, that decides which other tracks it blends with
/// (pat-ghw2s).
///
/// The three 3D transform types are distinct channels, because a bone's
/// position, rotation and scale tracks share one path (`Skeleton3D:hip`) —
/// Godot keeps them as separate `loc`/`rot`/`scale` fields of one transform
/// cache. Every other type collapses to [`TrackType::Property`], so keying on
/// the channel changes nothing for tracks that never shared a path.
pub fn value_channel(track_type: TrackType) -> TrackType {
    match track_type {
        TrackType::Position3D | TrackType::Rotation3D | TrackType::Scale3D => track_type,
        _ => TrackType::Property,
    }
}

// ---------------------------------------------------------------------------
// AnimationPlayer
// ---------------------------------------------------------------------------

/// A playback signal emitted by an [`AnimationPlayer`], mirroring Godot's
/// `AnimationMixer`/`AnimationPlayer` signals. Queued as they occur and drained
/// via [`AnimationPlayer::drain_signals`] so the scripting layer can re-emit
/// them to connected GDScript callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimationPlayerSignal {
    /// `animation_started(name)` — playback of `name` began.
    AnimationStarted(String),
    /// `animation_finished(name)` — a non-looping `name` reached its end.
    AnimationFinished(String),
    /// `animation_changed(old, new)` — the playback QUEUE advanced.
    ///
    /// ⚠️ Not "the clip switched". Godot emits this only from the finish path
    /// draining `playback_queue` (`animation_player.cpp:348-350`); an ordinary
    /// `play("other")` emits [`CurrentAnimationChanged`](Self::CurrentAnimationChanged)
    /// instead. Patina used to push it on every switch, so a script connected to
    /// `animation_changed` fired on manual `play` calls Godot stays silent for
    /// (pat-mgixk.2).
    AnimationChanged {
        /// Previously-current animation name.
        old: String,
        /// Newly-current animation name.
        new: String,
    },
    /// `current_animation_changed(name)` — the assigned clip changed, with the
    /// empty string when playback finished with nothing queued
    /// (`animation_player.cpp:356`).
    CurrentAnimationChanged(String),
}

/// Which loop phase advances a mixer's playback — Godot's
/// `AnimationMixer.callback_mode_process` (`AnimationMixer::AnimationCallbackModeProcess`).
///
/// The node property has always existed in the ClassDB and the editor
/// inspector, but nothing read it: every player advanced in the idle phase, so
/// a `PHYSICS` mixer sampled at render rate (jittering against the bodies it
/// was meant to stay in step with) and a `MANUAL` one animated despite the
/// project asking to drive it by hand (pat-0q2kx.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationCallbackMode {
    /// Advance during the physics tick (`ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS`,
    /// `0`), in lockstep with the bodies the animation drives.
    Physics,
    /// Advance during the idle/process phase (`..._IDLE`, `1`) — Godot's default.
    #[default]
    Idle,
    /// Never advanced by the loop (`..._MANUAL`, `2`); only an explicit
    /// `advance()` moves the playhead.
    Manual,
}

impl AnimationCallbackMode {
    /// Maps Godot's `callback_mode_process` integer. Unknown values fall back to
    /// [`Idle`](Self::Idle), which is also the value used when the property is
    /// absent from the node.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => AnimationCallbackMode::Physics,
            2 => AnimationCallbackMode::Manual,
            _ => AnimationCallbackMode::Idle,
        }
    }

    /// The Godot `callback_mode_process` integer for this mode.
    pub fn to_godot_int(self) -> i64 {
        match self {
            AnimationCallbackMode::Physics => 0,
            AnimationCallbackMode::Idle => 1,
            AnimationCallbackMode::Manual => 2,
        }
    }
}

/// Manages playback of named animations, analogous to Godot's `AnimationPlayer`.
#[derive(Debug, Clone)]
pub struct AnimationPlayer {
    /// Library of available animations.
    pub animations: HashMap<String, Animation>,
    /// The currently playing animation name.
    current: Option<String>,
    /// Current playback position in seconds.
    position: f64,
    /// Playback speed multiplier (default 1.0).
    pub speed_scale: f64,
    /// Whether the player is actively playing.
    pub playing: bool,
    /// Name of the animation to play on start (if any).
    pub autoplay: Option<String>,
    /// PingPong direction: true = forward, false = backward.
    ping_pong_forward: bool,
    /// Active crossfade blend state (if any).
    blend_state: Option<BlendState>,
    /// Property path of the track consumed as root motion (Godot's
    /// `AnimationMixer.root_motion_track`). When set, this track is not applied
    /// as an absolute node property; its per-frame position delta is exposed
    /// via [`consume_root_motion`](AnimationPlayer::consume_root_motion) and
    /// applied to the owner node's transform instead.
    pub root_motion_track: Option<String>,
    /// Node path that track `node_path`s resolve against, relative to the
    /// player's owner node (Godot's `AnimationMixer.root_node`, pat-u3l8c.94).
    /// Defaults to `"."` (the owner node itself); set to `".."` for Godot's
    /// parent-relative default. An empty track `node_path` targets this node.
    pub root_node: String,
    /// Playback signals emitted since the last [`drain_signals`](AnimationPlayer::drain_signals).
    pending_signals: Vec<AnimationPlayerSignal>,
    /// Per-pair crossfade times: `(from, to) -> seconds` (Godot `set_blend_time`).
    blend_times: HashMap<(String, String), f64>,
    /// Queued follow-up animations: `anim -> next` (Godot `animation_set_next`).
    next_animations: HashMap<String, String>,
    /// Per-play speed multiplier from `play(..., custom_speed)`. Multiplies
    /// [`speed_scale`](Self::speed_scale); reset to 1.0 by `stop`. Negative for
    /// `play_backwards` (pat-0q2kx.52).
    custom_speed: f64,
    /// Clips chained by `queue()`, played in order as each finishes. Distinct
    /// from `next_animations`, which is a per-clip successor rather than a
    /// one-shot list, and consumed BEFORE it.
    queue: VecDeque<String>,
    /// Active playback section `(start, end)` in seconds (Godot 4.4's
    /// `play_section`/`set_section`), clamping advance to a sub-range.
    section: Option<(f64, f64)>,
    /// Crossfade used when no `(from, to)` pair is registered and `play` was
    /// given no `custom_blend` (Godot `playback_default_blend_time`).
    pub default_blend_time: f64,
    /// Godot's `end_reached`: set for the duration of the finish handling, so a
    /// `play` issued from that path (draining the queue, or from an
    /// `animation_finished` handler) does not clear the very queue being
    /// drained (`animation_player.cpp:509`).
    end_reached: bool,
    /// Root motion published for this frame, for the script getters to read in
    /// the same frame (pat-mgixk.78). Written by `record_root_motion` from the
    /// animation step.
    root_motion: RootMotionState,
}

/// What a running game needs to read back from root motion, held per mixer
/// (pat-mgixk.78).
///
/// Godot's `AnimationMixer` never moves the node: it cancels the root track from
/// the visual pose and publishes the motion through
/// `get_root_motion_position/rotation/scale` and the matching `*_accumulator`
/// getters, leaving the user to apply it — typically
/// `velocity = tree.get_root_motion_position() / delta`
/// (`AnimationMixer.xml:323-326`).
///
/// The `*_delta` fields are THIS FRAME's motion and are overwritten every frame;
/// the accumulators are the running total since the last reset.
///
/// ⚠️ `scale_delta` and `scale_accumulator` are RATIOS, so their identity is
/// `Vector3::ONE` and they compose by MULTIPLICATION. Defaulting them to zero
/// would publish "scaled to nothing", and summing them would walk away from 1.0
/// every frame. `rotation_*` is a quaternion for the same reason: identity,
/// composed by multiplication, which is why a single `rotation_y` angle could
/// not express it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RootMotionState {
    /// This frame's root translation.
    pub position_delta: gdcore::math::Vector3,
    /// This frame's root rotation.
    pub rotation_delta: gdcore::math3d::Quaternion,
    /// This frame's root scale ratio.
    pub scale_delta: gdcore::math::Vector3,
    /// Running total of `position_delta` since the last reset.
    pub position_accumulator: gdcore::math::Vector3,
    /// Running product of `rotation_delta` since the last reset.
    pub rotation_accumulator: gdcore::math3d::Quaternion,
    /// Running product of `scale_delta` since the last reset.
    pub scale_accumulator: gdcore::math::Vector3,
    /// Godot's `root_motion_local`: whether the motion is reported in the root
    /// node's local space rather than the mixer's.
    pub local: bool,
}

impl Default for RootMotionState {
    fn default() -> Self {
        Self {
            position_delta: gdcore::math::Vector3::ZERO,
            rotation_delta: gdcore::math3d::Quaternion::IDENTITY,
            scale_delta: gdcore::math::Vector3::ONE,
            position_accumulator: gdcore::math::Vector3::ZERO,
            rotation_accumulator: gdcore::math3d::Quaternion::IDENTITY,
            scale_accumulator: gdcore::math::Vector3::ONE,
            local: false,
        }
    }
}

impl RootMotionState {
    /// Records one frame's motion: the deltas are REPLACED, the accumulators
    /// advanced.
    ///
    /// ⚠️ The three compose DIFFERENTLY and that is Godot's model, not a
    /// shortcut: position SUMS, rotation MULTIPLIES (quaternion composition),
    /// and scale MULTIPLIES componentwise because it is a ratio. Summing the
    /// scale would walk away from 1.0 every frame even for an unscaled rig.
    ///
    /// Every delta is assigned unconditionally, so a frame with no rotation
    /// publishes identity rather than last frame's value.
    pub fn record(
        &mut self,
        position: gdcore::math::Vector3,
        rotation: gdcore::math3d::Quaternion,
        scale: gdcore::math::Vector3,
    ) {
        self.position_delta = position;
        self.rotation_delta = rotation;
        self.scale_delta = scale;
        self.position_accumulator = gdcore::math::Vector3::new(
            self.position_accumulator.x + position.x,
            self.position_accumulator.y + position.y,
            self.position_accumulator.z + position.z,
        );
        self.rotation_accumulator = self.rotation_accumulator * rotation;
        self.scale_accumulator = gdcore::math::Vector3::new(
            self.scale_accumulator.x * scale.x,
            self.scale_accumulator.y * scale.y,
            self.scale_accumulator.z * scale.z,
        );
    }

    /// Clears the accumulators, as Godot does when the mixer is reset.
    pub fn reset_accumulators(&mut self) {
        self.position_accumulator = gdcore::math::Vector3::ZERO;
        self.rotation_accumulator = gdcore::math3d::Quaternion::IDENTITY;
        self.scale_accumulator = gdcore::math::Vector3::ONE;
    }
}

/// Tracks an in-progress crossfade between two animations.
///
/// ⚠️ `Debug, Clone` are REQUIRED, not decoration: `AnimationPlayer` holds a
/// `blend_state: Option<BlendState>` field and derives both, so dropping them
/// here fails that derive with E0277 rather than anything pointing at this type.
/// They went missing when `RootMotionState` was spliced in between this doc
/// comment and the struct, which stranded the attributes 90 lines up where they
/// bound to `RootMotionState` instead — deriving `Debug`/`Clone` for it twice
/// (E0119) and leaving this struct bare (E0277). Four errors, one cause.
#[derive(Debug, Clone)]
pub struct BlendState {
    /// The animation being blended *from*.
    pub from_animation: String,
    /// Playback position in the "from" animation at blend start.
    pub from_position: f64,
    /// Total crossfade duration in seconds.
    pub blend_duration: f64,
    /// Elapsed time since the crossfade started.
    pub blend_elapsed: f64,
}

impl BlendState {
    /// Returns the blend factor in `[0.0, 1.0]` — 0.0 = fully "from", 1.0 = fully "to".
    pub fn blend_factor(&self) -> f32 {
        if self.blend_duration <= 0.0 {
            return 1.0;
        }
        (self.blend_elapsed / self.blend_duration).clamp(0.0, 1.0) as f32
    }

    /// Returns `true` when the crossfade is complete.
    pub fn is_complete(&self) -> bool {
        self.blend_elapsed >= self.blend_duration
    }
}

impl AnimationPlayer {
    /// Creates a new empty animation player.
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            current: None,
            position: 0.0,
            speed_scale: 1.0,
            playing: false,
            autoplay: None,
            ping_pong_forward: true,
            blend_state: None,
            root_motion_track: None,
            root_node: ".".to_string(),
            pending_signals: Vec::new(),
            blend_times: HashMap::new(),
            next_animations: HashMap::new(),
            custom_speed: 1.0,
            queue: VecDeque::new(),
            section: None,
            default_blend_time: 0.0,
            end_reached: false,
            root_motion: RootMotionState::default(),
        }
    }

    /// This frame's root motion as `(position, rotation, scale)`, read from ALL
    /// THREE track kinds of the root-motion path (pat-mgixk.78).
    ///
    /// Godot's root motion is a full transform delta: a translation, a rotation
    /// quaternion and a scale RATIO (`animation_mixer.cpp`). Returns the
    /// identities — zero, `IDENTITY`, `ONE` — for anything the animation does
    /// not author, so a caller can apply the result unconditionally.
    ///
    /// 🔑 Filters by path and dispatches on `track_type`, rather than taking the
    /// first track with that path the way [`consume_root_motion`] does. A root
    /// bone normally has THREE tracks sharing one property path, so a `find()`
    /// that ignores the kind can hand back the rotation track and then fail to
    /// read it as a vector — silently reporting no motion at all.
    pub fn root_motion_frame(
        &self,
        prev_time: f64,
        curr_time: f64,
    ) -> (
        gdcore::math::Vector3,
        gdcore::math3d::Quaternion,
        gdcore::math::Vector3,
    ) {
        let mut position = gdcore::math::Vector3::ZERO;
        let mut rotation = gdcore::math3d::Quaternion::IDENTITY;
        let mut scale = gdcore::math::Vector3::ONE;

        let track_path = match &self.root_motion_track {
            Some(p) => p,
            None => return (position, rotation, scale),
        };
        let anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return (position, rotation, scale),
        };

        // Whether a dedicated `Position3D` bone track supplied the
        // translation. The generic Property arm below must not override it.
        let mut position_from_bone_track = false;
        for track in anim
            .tracks
            .iter()
            .filter(|t| &t.property_path == track_path)
        {
            match track.track_type {
                TrackType::Position3D => {
                    if let (Some(p), Some(c)) = (
                        track.sample(prev_time).and_then(variant_to_vec3),
                        track.sample(curr_time).and_then(variant_to_vec3),
                    ) {
                        position = gdcore::math::Vector3::new(c.x - p.x, c.y - p.y, c.z - p.z);
                        position_from_bone_track = true;
                    }
                }
                // ⚠️ A root-motion track is NOT always a bone track. Godot's
                // `root_motion_track` is a path, and a scene may point it at a
                // plain Vector3 property track — which is exactly what
                // `animation_root_motion_method_test.rs:127` does
                // (`AnimationTrack::with_type("", "position",
                // TrackType::Property)`). `consume_root_motion` has always read
                // that track with `variant_to_vec3` regardless of kind, so
                // dispatching on the three bone kinds ALONE made this sampler
                // report zero for a case the engine already supports — and no
                // test would have caught it, because the auto-apply still reads
                // `consume_root_motion` and keeps that test green.
                //
                // Only `Property`: the kinds are Property, Method, Audio,
                // Position3D, Rotation3D, Scale3D, BlendShape and Bezier, and a
                // Bezier track animates ONE component (`position:x`), so its
                // sample is a float `variant_to_vec3` rejects anyway.
                //
                // A dedicated `Position3D` track wins, so this arm can only
                // fill a gap, never override the specific kind.
                TrackType::Property => {
                    if position_from_bone_track {
                        continue;
                    }
                    if let (Some(p), Some(c)) = (
                        track.sample(prev_time).and_then(variant_to_vec3),
                        track.sample(curr_time).and_then(variant_to_vec3),
                    ) {
                        position = gdcore::math::Vector3::new(c.x - p.x, c.y - p.y, c.z - p.z);
                    }
                }
                TrackType::Rotation3D => {
                    if let (Some(p), Some(c)) = (
                        track.sample(prev_time).and_then(variant_to_quaternion),
                        track.sample(curr_time).and_then(variant_to_quaternion),
                    ) {
                        // The LOCAL delta that carries `prev` onto `curr`.
                        rotation = p.inverse() * c;
                    }
                }
                TrackType::Scale3D => {
                    if let (Some(p), Some(c)) = (
                        track.sample(prev_time).and_then(variant_to_vec3),
                        track.sample(curr_time).and_then(variant_to_vec3),
                    ) {
                        // ⚠️ A ratio, and a zero previous component has no
                        // ratio — 1.0 (no change) is the only safe answer.
                        // Dividing would publish inf/NaN into the accumulator
                        // and poison every later frame, since scale composes by
                        // multiplication.
                        let ratio =
                            |prev: f32, curr: f32| if prev == 0.0 { 1.0 } else { curr / prev };
                        scale = gdcore::math::Vector3::new(
                            ratio(p.x, c.x),
                            ratio(p.y, c.y),
                            ratio(p.z, c.z),
                        );
                    }
                }
                _ => {}
            }
        }
        (position, rotation, scale)
    }

    /// Publishes this frame's root motion so a getter can read it in the SAME
    /// frame (pat-mgixk.78).
    ///
    /// Called from the animation step, which is the only place that knows the
    /// interval just advanced over.
    pub fn record_root_motion_frame(&mut self, prev_time: f64, curr_time: f64) {
        let (position, rotation, scale) = self.root_motion_frame(prev_time, curr_time);
        self.root_motion.record(position, rotation, scale);
    }

    /// This frame's root motion and the running totals.
    pub fn root_motion(&self) -> &RootMotionState {
        &self.root_motion
    }

    /// Clears the accumulators (Godot's reset).
    pub fn reset_root_motion_accumulators(&mut self) {
        self.root_motion.reset_accumulators();
    }

    /// Godot's `root_motion_local` setter: whether the motion is reported in the
    /// root node's local space.
    ///
    /// A narrow setter rather than a `root_motion_mut()`: the deltas and
    /// accumulators are written by the animation step alone, and handing script
    /// dispatch a mutable handle to the whole state would let a setter clobber
    /// them.
    pub fn set_root_motion_local(&mut self, local: bool) {
        self.root_motion.local = local;
    }

    /// Adds an animation to the player's library.
    pub fn add_animation(&mut self, anim: Animation) {
        self.animations.insert(anim.name.clone(), anim);
    }

    /// Starts playing the named animation, with Godot's default arguments
    /// (`custom_blend = -1`, `custom_speed = 1.0`, `from_end = false`).
    ///
    /// Emits `animation_changed` (when the current animation differs) and
    /// `animation_started` into the signal queue.
    pub fn play(&mut self, name: &str) {
        self.play_ex(name, -1.0, 1.0, false);
    }

    /// Godot's full `play(name, custom_blend, custom_speed, from_end)`
    /// (pat-0q2kx.52).
    ///
    /// - **Same-clip continuation**, in Godot's three ordered cases
    ///   (`animation_player.cpp:522-531`): a `from_end` call with the playhead
    ///   at the start seeks to the end; a forward call with the playhead at or
    ///   past the end REWINDS (so a finished clip replays); otherwise, if the
    ///   clip is already playing, the call returns silently with no
    ///   `animation_started`. Restarting unconditionally made a per-frame
    ///   `play("walk")` in `_process` freeze the animation on frame 0;
    ///   resuming unconditionally — what this did before pat-mgixk.2 — left a
    ///   finished clip parked at its end, so it could never play twice.
    /// - `custom_blend >= 0` crossfades over that many seconds; a negative value
    ///   falls back to the registered `(from, to)` blend time and then to
    ///   [`default_blend_time`](Self::default_blend_time).
    /// - `custom_speed` multiplies [`speed_scale`](Self::speed_scale) for this
    ///   playback only, and may be negative to run the clip backwards.
    /// - `from_end` starts the playhead at the clip's end (or the section end)
    ///   rather than its start, which is what makes backwards playback begin
    ///   where the clip stops.
    pub fn play_ex(&mut self, name: &str, custom_blend: f64, custom_speed: f64, from_end: bool) {
        if !self.animations.contains_key(name) {
            return;
        }
        let already_current = self.current.as_deref() == Some(name);
        self.custom_speed = custom_speed;
        // ⚠️ Read BEFORE `playing` is set. Godot's `else if (playing) return`
        // (`animation_player.cpp:524`) asks whether the clip was ALREADY
        // running; assigning the flag first — as this did — makes that branch
        // unconditional, which is why a per-frame `play("walk")` could never
        // emit its one `animation_started`.
        let was_playing = self.playing;

        // An explicit `play` clears the queue, but the finish path draining that
        // same queue must not (`animation_player.cpp:509`).
        if !self.end_reached {
            self.queue.clear();
        }

        if already_current {
            // Godot's same-clip branch (`animation_player.cpp:522-531`). The
            // three cases are ordered, and the order is the whole behaviour.
            let start = self.section_start();
            let end = self.section_end_or_length();
            if from_end && self.position <= start {
                // Reset but asked to play backwards: start from the end.
                self.position = end;
                self.ping_pong_forward = false;
            } else if !from_end && self.position >= end {
                // THE REPLAY CASE. The clip has already run to its end, so
                // `play` rewinds. Without this the playhead stayed at `end`,
                // the next `advance` tripped the `position >= upper` test
                // immediately, and the clip finished again without ever
                // playing — so `play("attack")` could fire an attack animation
                // exactly once per scene.
                self.position = start;
                self.ping_pong_forward = true;
            } else if was_playing {
                // Still mid-clip. Godot returns here with NO
                // `animation_started`, which is what keeps a `play("walk")`
                // called every frame in `_process` down to one signal.
                return;
            }
            // Falls through on a rewind, and on a resume from `pause` (where
            // `was_playing` is false and the playhead is mid-clip) — both of
            // which Godot reports as a fresh start.
            //
            // ⚠️ `ping_pong_forward` is set by the two seek branches above and
            // NOT here: Godot's resume does not touch the travel direction, so
            // assigning it unconditionally would flip a paused ping-pong clip
            // that happened to be on its backward leg.
            self.playing = true;
            self.pending_signals
                .push(AnimationPlayerSignal::AnimationStarted(name.to_owned()));
            return;
        }

        self.playing = true;
        let previous = self.current.take();
        // Crossfade from whatever was playing, if a blend time applies.
        if let Some(ref prev) = previous {
            let blend = if custom_blend >= 0.0 {
                custom_blend
            } else {
                let registered = self.get_blend_time(prev, name);
                if registered > 0.0 {
                    registered
                } else {
                    self.default_blend_time
                }
            };
            if blend > 0.0 {
                self.blend_state = Some(BlendState {
                    from_animation: prev.clone(),
                    from_position: self.position,
                    blend_duration: blend,
                    blend_elapsed: 0.0,
                });
            }
        }

        self.current = Some(name.to_owned());
        self.position = if from_end {
            self.section_end_or_length()
        } else {
            self.section_start()
        };
        self.ping_pong_forward = !from_end;

        // Godot emits `current_animation_changed(name)` here — NOT
        // `animation_changed`, which it reserves for the queue advancing
        // (`animation_player.cpp:516` vs `:348-350`). `previous` is no longer
        // needed for a signal, only for the crossfade above.
        let _ = previous;
        self.pending_signals
            .push(AnimationPlayerSignal::CurrentAnimationChanged(
                name.to_owned(),
            ));
        self.pending_signals
            .push(AnimationPlayerSignal::AnimationStarted(name.to_owned()));
    }

    /// Godot's `play_backwards(name, custom_blend)`: plays `name` from its end
    /// at negative speed, so it runs to the start and finishes there.
    pub fn play_backwards(&mut self, name: &str, custom_blend: f64) {
        self.play_ex(name, custom_blend, -1.0, true);
    }

    /// Godot's `stop()` — halts playback AND resets the playhead, clearing the
    /// queue and any per-play speed.
    ///
    /// This used to be pause semantics (it only cleared `playing`), so a script
    /// calling `stop()` then `play()` resumed mid-clip instead of restarting,
    /// and a queued clip survived the stop. Use [`pause`](Self::pause) for the
    /// old behaviour.
    pub fn stop(&mut self) {
        self.playing = false;
        self.position = self.section_start();
        self.custom_speed = 1.0;
        self.ping_pong_forward = true;
        self.queue.clear();
        self.blend_state = None;
    }

    /// Godot's `stop(keep_state = true)`: halts playback but leaves the playhead
    /// (and the sampled pose) where it is.
    pub fn stop_keep_state(&mut self) {
        self.playing = false;
    }

    /// Godot's `pause()` — halts playback preserving the position, so a
    /// following `play()` of the same clip resumes from here.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Queues `name` to play once the current animation finishes (Godot
    /// `AnimationPlayer::queue`). Queued clips are consumed in order and take
    /// precedence over an `animation_set_next` successor.
    pub fn queue(&mut self, name: impl Into<String>) {
        self.queue.push_back(name.into());
    }

    /// The clips currently queued behind the playing one, in play order.
    pub fn get_queue(&self) -> Vec<String> {
        self.queue.iter().cloned().collect()
    }

    /// Clears the queue without touching the playing animation (Godot
    /// `clear_queue`).
    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    /// The effective playback speed: `speed_scale * custom_speed`, or `0.0` when
    /// nothing is playing (Godot `get_playing_speed`).
    pub fn get_playing_speed(&self) -> f64 {
        if self.playing {
            self.speed_scale * self.custom_speed
        } else {
            0.0
        }
    }

    /// Restricts playback to `[start, end]` seconds (Godot 4.4 `set_section`).
    /// A non-positive-width range is ignored rather than freezing the playhead.
    pub fn set_section(&mut self, start: f64, end: f64) {
        if end > start {
            self.section = Some((start.max(0.0), end));
        }
    }

    /// Plays `name` restricted to `[start, end]` (Godot `play_section`).
    pub fn play_section(
        &mut self,
        name: &str,
        start: f64,
        end: f64,
        custom_blend: f64,
        custom_speed: f64,
        from_end: bool,
    ) {
        self.set_section(start, end);
        self.play_ex(name, custom_blend, custom_speed, from_end);
    }

    /// Clears any active section, restoring whole-clip playback.
    pub fn reset_section(&mut self) {
        self.section = None;
    }

    /// Whether a section is currently restricting playback.
    pub fn has_section(&self) -> bool {
        self.section.is_some()
    }

    /// The section's start time, or `0.0` when no section is set.
    pub fn get_section_start_time(&self) -> f64 {
        self.section.map(|(s, _)| s).unwrap_or(0.0)
    }

    /// The section's end time, or the current clip's length when unset.
    pub fn get_section_end_time(&self) -> f64 {
        self.section
            .map(|(_, e)| e)
            .unwrap_or_else(|| self.current_length())
    }

    /// Length of the currently selected clip (`0.0` when none).
    fn current_length(&self) -> f64 {
        self.current
            .as_ref()
            .and_then(|n| self.animations.get(n))
            .map(|a| a.length)
            .unwrap_or(0.0)
    }

    /// Lower playback bound: the section start, else the clip start.
    fn section_start(&self) -> f64 {
        self.section.map(|(s, _)| s).unwrap_or(0.0)
    }

    /// Upper playback bound: the section end clamped to the clip, else its length.
    fn section_end_or_length(&self) -> f64 {
        let length = self.current_length();
        match self.section {
            Some((_, e)) if length > 0.0 => e.min(length),
            Some((_, e)) => e,
            None => length,
        }
    }

    /// Emits `animation_finished` for the current animation and, if one is
    /// queued via [`animation_set_next`](AnimationPlayer::animation_set_next),
    /// starts it. Called when a non-looping animation reaches its end.
    fn on_animation_finished(&mut self) {
        let Some(name) = self.current.clone() else {
            return;
        };
        // Godot's `end_reached` window (`animation_player.cpp:334-365`): held
        // across the whole handler so the `play` below, and any `play` a
        // connected `animation_finished` handler makes, leaves the queue alone
        // instead of clearing the list it is walking.
        self.end_reached = true;
        self.pending_signals
            .push(AnimationPlayerSignal::AnimationFinished(name.clone()));

        // Godot drains `queue()` first: an explicitly queued clip wins over the
        // clip's registered `animation_set_next` successor (pat-0q2kx.52).
        let mut successor = None;
        while let Some(queued) = self.queue.pop_front() {
            if self.animations.contains_key(&queued) {
                successor = Some(queued);
                break;
            }
        }
        if successor.is_none() {
            if let Some(next) = self.next_animations.get(&name).cloned() {
                if self.animations.contains_key(&next) {
                    successor = Some(next);
                }
            }
        }

        match successor {
            Some(next) => {
                // `play` emits `current_animation_changed` + `animation_started`.
                self.play(&next);
                // `animation_changed(old, new)` fires HERE and nowhere else —
                // the queue advancing is the only thing Godot reports with it
                // (`animation_player.cpp:348-350`). Pushed after `play` so the
                // order matches Godot's: finished, started, then changed.
                self.pending_signals
                    .push(AnimationPlayerSignal::AnimationChanged {
                        old: name,
                        new: next,
                    });
            }
            None => {
                // ⚠️ `current` deliberately stays set. Godot keeps
                // `playback.assigned` and only reports the empty name
                // (`animation_player.cpp:356`) — and it has to, or the
                // `play(same-clip)` rewind above would have no clip to
                // recognise and every replay would read as a fresh start.
                self.pending_signals
                    .push(AnimationPlayerSignal::CurrentAnimationChanged(String::new()));
            }
        }
        self.end_reached = false;
    }

    /// Seeks to `time` seconds within the current animation (clamped to its
    /// length). Godot's `seek(seconds, update)` — because sampling here is
    /// pull-based ([`get_current_values`](AnimationPlayer::get_current_values)
    /// reads the position), setting the position *is* the immediate update, so
    /// `update` is accepted for API parity.
    pub fn seek(&mut self, time: f64, update: bool) {
        let _ = update;
        let length = self
            .current
            .as_ref()
            .and_then(|n| self.animations.get(n))
            .map(|a| a.length)
            .unwrap_or(0.0);
        self.position = if length > 0.0 {
            time.clamp(0.0, length)
        } else {
            time.max(0.0)
        };
    }

    /// Sets the crossfade time in seconds applied when transitioning from `from`
    /// to `to` (Godot `AnimationPlayer::set_blend_time`).
    pub fn set_blend_time(&mut self, from: impl Into<String>, to: impl Into<String>, secs: f64) {
        self.blend_times
            .insert((from.into(), to.into()), secs.max(0.0));
    }

    /// Returns the configured blend time from `from` to `to`, or `0.0` if none.
    pub fn get_blend_time(&self, from: &str, to: &str) -> f64 {
        self.blend_times
            .get(&(from.to_string(), to.to_string()))
            .copied()
            .unwrap_or(0.0)
    }

    /// Queues `next` to auto-play when `anim` finishes (Godot
    /// `AnimationPlayer::animation_set_next`).
    pub fn animation_set_next(&mut self, anim: impl Into<String>, next: impl Into<String>) {
        self.next_animations.insert(anim.into(), next.into());
    }

    /// Returns the animation queued to follow `anim`, if any.
    pub fn animation_get_next(&self, anim: &str) -> Option<&str> {
        self.next_animations.get(anim).map(String::as_str)
    }

    /// Plays the configured [`autoplay`](AnimationPlayer::autoplay) animation, if
    /// present in the library. Intended to be called when the player's owner
    /// node enters the tree (Godot's autoplay-on-ready). Returns `true` if an
    /// animation was started.
    pub fn play_autoplay(&mut self) -> bool {
        if let Some(name) = self.autoplay.clone() {
            if self.animations.contains_key(&name) {
                self.play(&name);
                return true;
            }
        }
        false
    }

    /// Drains and returns queued playback signals in emission order.
    pub fn drain_signals(&mut self) -> Vec<AnimationPlayerSignal> {
        std::mem::take(&mut self.pending_signals)
    }

    /// `true` if there are queued signals awaiting [`drain_signals`](AnimationPlayer::drain_signals).
    pub fn has_pending_signals(&self) -> bool {
        !self.pending_signals.is_empty()
    }

    /// Returns the currently playing animation name.
    pub fn current_animation(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Returns the current playback position.
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Godot's `AnimationPlayer.has_animation` (pat-s3rrv.43).
    pub fn has_animation(&self, name: &str) -> bool {
        self.animations.contains_key(name)
    }

    /// Godot's `AnimationPlayer.get_animation_list` (pat-s3rrv.43).
    ///
    /// Sorted, because the backing map iterates arbitrarily and Godot returns a
    /// stable list — an unsorted result would make any script iterating it, and
    /// any test asserting on it, nondeterministic.
    pub fn get_animation_list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.animations.keys().cloned().collect();
        names.sort();
        names
    }

    /// Advances the playback position by `delta` seconds (scaled by `speed_scale`).
    pub fn advance(&mut self, delta: f64) {
        if !self.playing {
            return;
        }

        let anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return,
        };

        let length = anim.length;
        if length <= 0.0 {
            return;
        }

        let loop_mode = anim.loop_mode;
        // `custom_speed` from `play(..., custom_speed)` multiplies the player's
        // own scale, and carries the sign that makes `play_backwards` run in
        // reverse (pat-0q2kx.52).
        let scaled_delta = delta * self.speed_scale * self.custom_speed;
        // A section restricts the travelled range; without one these are the
        // clip's own bounds, so the arithmetic below is unchanged.
        let lower = self.section_start().min(length);
        let upper = self.section_end_or_length().min(length);

        // Advance blend state if crossfading.
        if let Some(ref mut bs) = self.blend_state {
            bs.blend_elapsed += scaled_delta.abs();
            bs.from_position += scaled_delta;
            if bs.is_complete() {
                self.blend_state = None;
            }
        }

        match loop_mode {
            LoopMode::None => {
                self.position += scaled_delta;
                // Which bound ends the clip depends on the direction of travel.
                // Backwards playback lands EXACTLY on `lower` (1.0s of a 1.0s
                // clip at -1x), so the reverse test has to be inclusive — while
                // the forward test must not fire for a clip sitting at `lower`
                // with a zero delta.
                if scaled_delta >= 0.0 {
                    if self.position >= upper {
                        self.position = upper;
                        self.playing = false;
                        self.on_animation_finished();
                    }
                } else if self.position <= lower {
                    self.position = lower;
                    self.playing = false;
                    self.on_animation_finished();
                }
            }
            LoopMode::Linear => {
                self.position += scaled_delta;
                // Wrap inside the section when one is set; `span` is the clip
                // length in the common (sectionless) case.
                let span = upper - lower;
                if span > 0.0 {
                    if self.position >= upper {
                        self.position = lower + (self.position - lower) % span;
                    } else if self.position < lower {
                        self.position = upper - (lower - self.position) % span;
                        if self.position >= upper {
                            self.position = lower;
                        }
                    }
                }
            }
            LoopMode::PingPong => {
                // Bounces between the section bounds, which are the clip's own
                // 0..length when no section is set.
                if self.ping_pong_forward {
                    self.position += scaled_delta;
                    if self.position >= upper {
                        self.position = 2.0 * upper - self.position;
                        self.ping_pong_forward = false;
                        if self.position < lower {
                            self.position = lower;
                            self.ping_pong_forward = true;
                        }
                    }
                } else {
                    self.position -= scaled_delta;
                    if self.position <= lower {
                        self.position = 2.0 * lower - self.position;
                        self.ping_pong_forward = true;
                        if self.position > upper {
                            self.position = upper;
                            self.ping_pong_forward = false;
                        }
                    }
                }
            }
        }
    }

    /// Starts a crossfade from the current animation to `target` over `duration` seconds.
    ///
    /// The current animation continues playing in the background while the
    /// blend factor ramps from 0.0 (fully "from") to 1.0 (fully "to").
    pub fn crossfade_to(&mut self, target: &str, duration: f64) {
        if !self.animations.contains_key(target) {
            return;
        }
        if let Some(from_name) = self.current.clone() {
            self.blend_state = Some(BlendState {
                from_animation: from_name,
                from_position: self.position,
                blend_duration: duration.max(0.0),
                blend_elapsed: 0.0,
            });
        }
        self.current = Some(target.to_owned());
        self.position = 0.0;
        self.playing = true;
        self.ping_pong_forward = true;
    }

    /// Returns the active blend state, if a crossfade is in progress.
    pub fn blend_state(&self) -> Option<&BlendState> {
        self.blend_state.as_ref()
    }

    /// Clears any active blend state.
    pub fn clear_blend(&mut self) {
        self.blend_state = None;
    }

    /// Sets a manual blend preview between two animations at a given weight.
    ///
    /// This is used by the editor for interactive blend previewing.
    /// `weight` is clamped to `[0.0, 1.0]`: 0.0 = fully `from`, 1.0 = fully `to`.
    pub fn set_blend_preview(&mut self, from: &str, to: &str, weight: f32) {
        if !self.animations.contains_key(from) || !self.animations.contains_key(to) {
            return;
        }
        // Use blend_duration=1.0 and elapsed=weight so blend_factor() returns weight.
        self.blend_state = Some(BlendState {
            from_animation: from.to_owned(),
            from_position: self.position,
            blend_duration: 1.0,
            blend_elapsed: weight.clamp(0.0, 1.0) as f64,
        });
        self.current = Some(to.to_owned());
    }

    /// Returns the current values of all tracks in the current animation,
    /// blending with the "from" animation if a crossfade is active.
    pub fn get_current_values(&self) -> Vec<(String, Variant)> {
        let to_anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return Vec::new(),
        };

        let to_values = to_anim.sample_all(self.position);

        // If no active blend, return the plain values.
        let bs = match &self.blend_state {
            Some(bs) => bs,
            None => return to_values,
        };

        let from_anim = match self.animations.get(&bs.from_animation) {
            Some(a) => a,
            None => return to_values,
        };

        let factor = bs.blend_factor();
        if factor >= 1.0 {
            return to_values;
        }

        let from_values = from_anim.sample_all(bs.from_position);

        // Build a map of from-animation values by property path.
        let from_map: HashMap<&str, &Variant> =
            from_values.iter().map(|(k, v)| (k.as_str(), v)).collect();

        // Blend matching tracks; for tracks only in "to", use them as-is.
        let mut result = Vec::with_capacity(to_values.len());
        for (prop, to_val) in &to_values {
            if let Some(from_val) = from_map.get(prop.as_str()) {
                if let Some(blended) = interpolate_variant(from_val, to_val, factor) {
                    result.push((prop.clone(), blended));
                } else {
                    result.push((prop.clone(), to_val.clone()));
                }
            } else {
                result.push((prop.clone(), to_val.clone()));
            }
        }

        result
    }

    /// Like [`get_current_values`](Self::get_current_values) but preserves each
    /// track's `node_path` so callers can resolve the target node (pat-u3l8c.94).
    /// Returns `(node_path, property_path, value)` triples; crossfade blending
    /// matches tracks by `(node_path, property_path)`.
    pub fn get_current_values_targeted(&self) -> Vec<(String, String, Variant)> {
        self.get_current_values_typed()
            .into_iter()
            .map(|(node, prop, _, value)| (node, prop, value))
            .collect()
    }

    /// Like [`get_current_values_targeted`](Self::get_current_values_targeted)
    /// but keeps each value's [`TrackType`] (pat-ghw2s). Crossfade blending
    /// pairs tracks by `(node_path, property_path, value_channel(type))`, so a
    /// bone's position is never blended against its scale.
    pub fn get_current_values_typed(&self) -> Vec<(String, String, TrackType, Variant)> {
        let to_anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return Vec::new(),
        };
        let to_values = to_anim.sample_all_typed(self.position);

        let bs = match &self.blend_state {
            Some(bs) => bs,
            None => return to_values,
        };
        let from_anim = match self.animations.get(&bs.from_animation) {
            Some(a) => a,
            None => return to_values,
        };
        let factor = bs.blend_factor();
        if factor >= 1.0 {
            return to_values;
        }
        let from_values = from_anim.sample_all_typed(bs.from_position);
        let from_map: HashMap<(&str, &str, TrackType), &Variant> = from_values
            .iter()
            .map(|(n, p, t, v)| ((n.as_str(), p.as_str(), value_channel(*t)), v))
            .collect();

        let mut result = Vec::with_capacity(to_values.len());
        for (node, prop, ty, to_val) in &to_values {
            let key = (node.as_str(), prop.as_str(), value_channel(*ty));
            let value = from_map
                .get(&key)
                .and_then(|from_val| interpolate_variant(from_val, to_val, factor))
                .unwrap_or_else(|| to_val.clone());
            result.push((node.clone(), prop.clone(), *ty, value));
        }
        result
    }

    /// Animation-track keys crossed in `(prev_time, curr_time]`, as
    /// `(node_path, clip_name)` pairs (pat-s3rrv.44).
    ///
    /// Mirrors [`method_calls_in_range`](Self::method_calls_in_range): an
    /// animation track *fires* at its key rather than holding a value, so it is
    /// gathered by crossing rather than by sampling, and a key the playhead
    /// jumped over is not silently lost. Playing backwards crosses keys in the
    /// reverse direction, which the same range test handles.
    pub fn animation_clip_starts_in_range(
        &self,
        prev_time: f64,
        curr_time: f64,
    ) -> Vec<(String, String)> {
        let anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return Vec::new(),
        };
        let mut starts = Vec::new();
        for track in &anim.tracks {
            if track.track_type != TrackType::Animation {
                continue;
            }
            for kf in track.keyframes() {
                let crossed = if curr_time >= prev_time {
                    kf.time > prev_time && kf.time <= curr_time
                } else {
                    kf.time < prev_time && kf.time >= curr_time
                };
                if !crossed {
                    continue;
                }
                // Godot's own spelling for "stop the sub-animation" is the
                // reserved clip name `[stop]`; it is forwarded verbatim so the
                // tree layer can act on it rather than trying to play it.
                if let Variant::String(clip) = &kf.value {
                    starts.push((track.node_path.clone(), clip.clone()));
                }
            }
        }
        starts
    }

    /// Returns the method-track invocations whose keyframes were crossed while
    /// advancing playback from `prev_time` to `curr_time`.
    ///
    /// The entry side of the interval is exclusive and the exit side inclusive
    /// (`(prev, curr]` for forward play, `[curr, prev)` for reversed), so a
    /// keyframe fires exactly once as the playhead passes it. Only
    /// [`TrackType::Method`] tracks are considered; each keyframe value is
    /// expected to be `Variant::Array([method_name, arg0, arg1, ...])`.
    pub fn method_calls_in_range(&self, prev_time: f64, curr_time: f64) -> Vec<MethodCall> {
        let anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return Vec::new(),
        };
        let mut calls = Vec::new();
        for track in &anim.tracks {
            if track.track_type != TrackType::Method {
                continue;
            }
            for kf in track.keyframes() {
                let crossed = if curr_time >= prev_time {
                    kf.time > prev_time && kf.time <= curr_time
                } else {
                    kf.time < prev_time && kf.time >= curr_time
                };
                if crossed {
                    if let Some(call) = method_call_from_value(&track.node_path, &kf.value) {
                        calls.push(call);
                    }
                }
            }
        }
        calls
    }

    /// Returns the root-motion position delta of the configured
    /// [`root_motion_track`](Self::root_motion_track) across the interval from
    /// `prev_time` to `curr_time`.
    ///
    /// Returns a zero delta when no root-motion track is set, the track is not
    /// found in the current animation, or it is not a `Vector3` position track.
    pub fn consume_root_motion(&self, prev_time: f64, curr_time: f64) -> RootMotionDelta {
        let track_path = match &self.root_motion_track {
            Some(p) => p,
            None => return RootMotionDelta::default(),
        };
        let anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return RootMotionDelta::default(),
        };
        let track = match anim.tracks.iter().find(|t| &t.property_path == track_path) {
            Some(t) => t,
            None => return RootMotionDelta::default(),
        };
        match (
            track.sample(prev_time).and_then(variant_to_vec3),
            track.sample(curr_time).and_then(variant_to_vec3),
        ) {
            (Some(prev), Some(curr)) => compute_root_motion_delta(prev, curr),
            _ => RootMotionDelta::default(),
        }
    }

    /// The track *kind* of every `(node_path, property_path)` pair in the
    /// current animation (pat-0q2kx.3).
    ///
    /// [`get_current_values_targeted`](Self::get_current_values_targeted) erases
    /// the track type, but a value's destination depends on it: Godot's
    /// `position_3d` / `rotation_3d` / `scale_3d` tracks whose path resolves to a
    /// `Skeleton3D` address a **bone pose** (`Skeleton3D:BoneName`), not a node
    /// property. This exposes the type so a caller can pair the two and route
    /// bone tracks into `skeleton3d::set_bone_pose_*`.
    pub fn current_track_types(&self) -> Vec<(String, String, TrackType)> {
        match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(anim) => anim
                .tracks
                .iter()
                .map(|t| (t.node_path.clone(), t.property_path.clone(), t.track_type))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Returns the property paths of the current animation that must NOT be
    /// applied to nodes as ordinary properties: method-track paths (dispatched
    /// as calls) and the root-motion track (applied as a transform delta).
    pub fn non_property_track_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(anim) = self.current.as_ref().and_then(|n| self.animations.get(n)) {
            for track in &anim.tracks {
                // An animation track's sampled value is a CLIP NAME, not a
                // property value — writing it would put the string into a
                // property called `animation` on the nested player
                // (pat-s3rrv.44). It is dispatched by
                // `animation_clip_starts_in_range` instead.
                if track.track_type == TrackType::Method || track.track_type == TrackType::Animation
                {
                    paths.push(track.property_path.clone());
                }
            }
        }
        if let Some(rm) = &self.root_motion_track {
            paths.push(rm.clone());
        }
        paths
    }
}

impl Default for AnimationPlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// A method-track invocation resolved while advancing an [`AnimationPlayer`].
#[derive(Debug, Clone, PartialEq)]
pub struct MethodCall {
    /// Node path the method targets, resolved relative to the player's
    /// `root_node` (like property tracks). Empty or "." targets the root node.
    pub node_path: String,
    /// Method name to invoke.
    pub method: String,
    /// Arguments to pass to the method.
    pub args: Vec<Variant>,
}

/// Parses a method-track keyframe value `[method_name, arg0, ...]` into a
/// [`MethodCall`]. Returns `None` when the value is not an array whose first
/// element is a string method name.
pub(crate) fn method_call_from_value(node_path: &str, value: &Variant) -> Option<MethodCall> {
    if let Variant::Array(items) = value {
        let method = match items.to_vec().first() {
            Some(Variant::String(s)) => s.clone(),
            _ => return None,
        };
        let args = items.to_vec().iter().skip(1).cloned().collect();
        Some(MethodCall {
            node_path: node_path.to_owned(),
            method,
            args,
        })
    } else {
        None
    }
}

/// Extracts a `Vector3` from a `Variant::Vector3`, else `None`.
fn variant_to_vec3(value: Variant) -> Option<gdcore::math::Vector3> {
    match value {
        Variant::Vector3(v) => Some(v),
        _ => None,
    }
}

/// A `Rotation3D` track's sampled value. Those tracks carry
/// `Variant::Quaternion` — the same arm `skeleton3d.rs:1994` reads when it turns
/// one into a bone pose.
fn variant_to_quaternion(value: Variant) -> Option<gdcore::math3d::Quaternion> {
    match value {
        Variant::Quaternion(q) => Some(q),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// cubic_bezier_y
// ---------------------------------------------------------------------------

/// Evaluates a cubic Bézier at parameter `t`, over four control points in the
/// plane (Godot's `Vector2::bezier_interpolate`).
///
/// Unlike [`cubic_bezier_y`], the endpoints here are explicit rather than fixed
/// at `(0,0)`/`(1,1)`: a bézier TRACK's segment spans arbitrary times and
/// values, so neither axis can be assumed normalized.
fn bezier_point(p0: Vector2, p1: Vector2, p2: Vector2, p3: Vector2, t: f32) -> Vector2 {
    let omt = 1.0 - t;
    let (omt2, t2) = (omt * omt, t * t);
    let (a, b, c, d) = (omt2 * omt, 3.0 * omt2 * t, 3.0 * omt * t2, t2 * t);
    Vector2::new(
        p0.x * a + p1.x * b + p2.x * c + p3.x * d,
        p0.y * a + p1.y * b + p2.y * c + p3.y * d,
    )
}

/// Reads a bézier key's scalar value, tolerating the `Int` spelling a
/// hand-written `.tscn` may use for a whole number.
fn bezier_key_value(k: &KeyFrame) -> f64 {
    match &k.value {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        _ => 0.0,
    }
}

/// Evaluates a cubic Bézier easing curve at linear parameter `t ∈ [0,1]`.
///
/// The curve is defined by control points `(x1,y1)` and `(x2,y2)` in the CSS
/// `cubic-bezier()` convention, where the endpoints are implicitly `(0,0)` and
/// `(1,1)`.  Returns the *y* coordinate (the eased value) for the given *t*
/// (the time fraction).
///
/// Uses Newton-Raphson iteration to solve `B_x(s) = t` for the curve parameter
/// `s`, then evaluates `B_y(s)`.
pub fn cubic_bezier_y(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }

    // Solve for s where B_x(s) = t using Newton-Raphson.
    let mut s = t; // initial guess
    for _ in 0..8 {
        let bx = bezier_component(x1, x2, s) - t;
        let dx = bezier_derivative(x1, x2, s);
        if dx.abs() < 1e-7 {
            break;
        }
        s -= bx / dx;
        s = s.clamp(0.0, 1.0);
    }

    bezier_component(y1, y2, s)
}

/// Evaluates one component of a cubic Bézier at parameter `s`.
/// Control points: `(0, p1, p2, 1)`.
#[inline]
fn bezier_component(p1: f32, p2: f32, s: f32) -> f32 {
    let inv = 1.0 - s;
    3.0 * inv * inv * s * p1 + 3.0 * inv * s * s * p2 + s * s * s
}

/// Derivative of a cubic Bézier component at parameter `s`.
#[inline]
fn bezier_derivative(p1: f32, p2: f32, s: f32) -> f32 {
    let inv = 1.0 - s;
    3.0 * inv * inv * p1 + 6.0 * inv * s * (p2 - p1) + 3.0 * s * s * (1.0 - p2)
}

// ---------------------------------------------------------------------------
// interpolate_variant
// ---------------------------------------------------------------------------

/// Linearly interpolates between two Variant values.
///
/// Returns `None` if the types are incompatible or interpolation is not
/// supported for the given type. Supports numeric types (Int, Float),
/// Vector2, Vector3, and Color.
/// A key's `transition` exponent applied to a normalised interpolation factor
/// (pat-mgixk.80).
///
/// `1.0` short-circuits, exactly as upstream does — `if (tr != 1.0) { c =
/// Math::ease(c, tr); }` (`animation.cpp:2669-2671`). That is not just a
/// micro-optimisation: it keeps every clip authored without transitions
/// sampling bit-identically to before this existed, so the change cannot move
/// an existing golden.
///
/// `0.0` never arrives here — the caller returns the floor key's value before
/// easing, which is upstream's `if (tr == 0) return p_keys[idx].value`.
fn eased_factor(t: f32, tr: f32) -> f32 {
    if tr == 1.0 {
        t
    } else {
        gdcore::math::ease(t, tr)
    }
}

pub fn interpolate_variant(from: &Variant, to: &Variant, t: f32) -> Option<Variant> {
    match (from, to) {
        (Variant::Int(a), Variant::Int(b)) => {
            let result = *a as f64 + (*b as f64 - *a as f64) * t as f64;
            Some(Variant::Int(result.round() as i64))
        }
        (Variant::Float(a), Variant::Float(b)) => {
            let result = a + (b - a) * t as f64;
            Some(Variant::Float(result))
        }
        (Variant::Int(a), Variant::Float(b)) => {
            let result = *a as f64 + (b - *a as f64) * t as f64;
            Some(Variant::Float(result))
        }
        (Variant::Float(a), Variant::Int(b)) => {
            let result = a + (*b as f64 - a) * t as f64;
            Some(Variant::Float(result))
        }
        (Variant::Vector2(a), Variant::Vector2(b)) => Some(Variant::Vector2(a.lerp(*b, t))),
        (Variant::Vector3(a), Variant::Vector3(b)) => Some(Variant::Vector3(a.lerp(*b, t))),
        (Variant::Color(a), Variant::Color(b)) => Some(Variant::Color(a.lerp(*b, t))),
        // Rotation tracks slerp along the shortest arc (pat-u3l8c.95):
        // `Quaternion::slerp` negates one endpoint when their dot is negative.
        (Variant::Quaternion(a), Variant::Quaternion(b)) => {
            Some(Variant::Quaternion(a.slerp(*b, t)))
        }
        // Transform tracks: translation lerps and the basis columns lerp.
        // Rotation-dominant animation should use a Rotation3D/Quaternion track
        // (which slerps correctly) rather than a raw Transform3D track.
        (Variant::Transform3D(a), Variant::Transform3D(b)) => {
            Some(Variant::Transform3D(Transform3D {
                basis: Basis {
                    x: a.basis.x.lerp(b.basis.x, t),
                    y: a.basis.y.lerp(b.basis.y, t),
                    z: a.basis.z.lerp(b.basis.z, t),
                },
                origin: a.origin.lerp(b.origin, t),
            }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Cubic (Catmull-Rom) and angle interpolation (pat-u3l8c.102)
// ---------------------------------------------------------------------------

/// Catmull-Rom cubic through four control values, evaluated at `t ∈ [0,1]` for
/// the segment `from → to`. `pre` is the value before `from`, `post` the value
/// after `to`. Passes through the keys: `t=0 → from`, `t=1 → to`.
///
/// This is Godot's uniform `_cubic_interpolate` form (uniform key spacing).
pub fn cubic_interp_f32(pre: f32, from: f32, to: f32, post: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * from)
        + (-pre + to) * t
        + (2.0 * pre - 5.0 * from + 4.0 * to - post) * t2
        + (-pre + 3.0 * from - 3.0 * to + post) * t3)
}

/// `f64` variant of [`cubic_interp_f32`], used for `Float` keys to preserve
/// precision.
pub fn cubic_interp_f64(pre: f64, from: f64, to: f64, post: f64, t: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * from)
        + (-pre + to) * t
        + (2.0 * pre - 5.0 * from + 4.0 * to - post) * t2
        + (-pre + 3.0 * from - 3.0 * to + post) * t3)
}

/// Catmull-Rom cubic interpolation applied component-wise to a [`Variant`].
///
/// Supports `Float`, `Vector2`, `Vector3`, and `Color`; returns `None` for
/// other/ mismatched types (callers fall back to linear). `pre`/`post` are the
/// neighbor key values (clamped to `from`/`to` by the caller when missing).
pub fn cubic_interpolate_variant(
    pre: &Variant,
    from: &Variant,
    to: &Variant,
    post: &Variant,
    t: f32,
) -> Option<Variant> {
    match (pre, from, to, post) {
        (Variant::Float(pr), Variant::Float(fr), Variant::Float(tv), Variant::Float(po)) => Some(
            Variant::Float(cubic_interp_f64(*pr, *fr, *tv, *po, t as f64)),
        ),
        (
            Variant::Vector2(pr),
            Variant::Vector2(fr),
            Variant::Vector2(tv),
            Variant::Vector2(po),
        ) => Some(Variant::Vector2(Vector2::new(
            cubic_interp_f32(pr.x, fr.x, tv.x, po.x, t),
            cubic_interp_f32(pr.y, fr.y, tv.y, po.y, t),
        ))),
        (
            Variant::Vector3(pr),
            Variant::Vector3(fr),
            Variant::Vector3(tv),
            Variant::Vector3(po),
        ) => Some(Variant::Vector3(Vector3::new(
            cubic_interp_f32(pr.x, fr.x, tv.x, po.x, t),
            cubic_interp_f32(pr.y, fr.y, tv.y, po.y, t),
            cubic_interp_f32(pr.z, fr.z, tv.z, po.z, t),
        ))),
        (Variant::Color(pr), Variant::Color(fr), Variant::Color(tv), Variant::Color(po)) => {
            Some(Variant::Color(Color::new(
                cubic_interp_f32(pr.r, fr.r, tv.r, po.r, t),
                cubic_interp_f32(pr.g, fr.g, tv.g, po.g, t),
                cubic_interp_f32(pr.b, fr.b, tv.b, po.b, t),
                cubic_interp_f32(pr.a, fr.a, tv.a, po.a, t),
            )))
        }
        _ => None,
    }
}

/// Wraps `x` into the half-open range `[min, max)` (Godot's `wrapf`). Used to
/// compute shortest signed angle deltas across the 2π boundary.
pub fn wrapf(x: f32, min: f32, max: f32) -> f32 {
    let range = max - min;
    if range.abs() < 1e-20 {
        return min;
    }
    min + (x - min).rem_euclid(range)
}

/// Shortest-path angular lerp (Godot `LINEAR_ANGLE`) for `Float` (radian)
/// values: `from + wrapf(to - from, -PI, PI) * t`. Returns `None` for
/// non-`Float` values so callers fall back to plain linear interpolation.
fn linear_angle_variant(from: &Variant, to: &Variant, t: f32) -> Option<Variant> {
    match (from, to) {
        (Variant::Float(a), Variant::Float(b)) => {
            let pi = std::f32::consts::PI;
            let delta = wrapf((*b - *a) as f32, -pi, pi);
            Some(Variant::Float(*a + (delta * t) as f64))
        }
        _ => None,
    }
}

/// Cubic interpolation on the *unwrapped* angle sequence (Godot `CUBIC_ANGLE`)
/// for `Float` (radian) values: `to`/`pre`/`post` are unwrapped to be
/// continuous relative to `from` before applying the Catmull-Rom cubic, so the
/// curve follows the shortest path and never overshoots by ~π. Returns `None`
/// for non-`Float` values so callers fall back to plain cubic.
fn cubic_angle_variant(
    pre: &Variant,
    from: &Variant,
    to: &Variant,
    post: &Variant,
    t: f32,
) -> Option<Variant> {
    match (pre, from, to, post) {
        (Variant::Float(pr), Variant::Float(fr), Variant::Float(tv), Variant::Float(po)) => {
            let pi = std::f32::consts::PI;
            let from_f = *fr as f32;
            let to_u = from_f + wrapf(*tv as f32 - from_f, -pi, pi);
            let pre_u = from_f + wrapf(*pr as f32 - from_f, -pi, pi);
            let post_u = to_u + wrapf(*po as f32 - to_u, -pi, pi);
            Some(Variant::Float(
                cubic_interp_f32(pre_u, from_f, to_u, post_u, t) as f64,
            ))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// blend_animations (standalone)
// ---------------------------------------------------------------------------

/// Blends the sampled output of two animations at a given time and blend factor.
///
/// `factor` is clamped to `[0.0, 1.0]`: 0.0 = fully `a`, 1.0 = fully `b`.
/// Tracks present in both animations are interpolated; tracks only in one
/// animation are included at full weight.
pub fn blend_animations(
    a: &Animation,
    b: &Animation,
    time_a: f64,
    time_b: f64,
    factor: f32,
) -> Vec<(String, Variant)> {
    let factor = factor.clamp(0.0, 1.0);

    if factor <= 0.0 {
        return a.sample_all(time_a);
    }
    if factor >= 1.0 {
        return b.sample_all(time_b);
    }

    let vals_a = a.sample_all(time_a);
    let vals_b = b.sample_all(time_b);

    let map_b: HashMap<&str, &Variant> = vals_b.iter().map(|(k, v)| (k.as_str(), v)).collect();

    // Collect all property paths (preserving order: a first, then b-only).
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for (prop, val_a) in &vals_a {
        seen.insert(prop.as_str());
        if let Some(val_b) = map_b.get(prop.as_str()) {
            if let Some(blended) = interpolate_variant(val_a, val_b, factor) {
                result.push((prop.clone(), blended));
            } else {
                // Types incompatible — snap to the dominant side.
                if factor < 0.5 {
                    result.push((prop.clone(), val_a.clone()));
                } else {
                    result.push((prop.clone(), (*val_b).clone()));
                }
            }
        } else {
            result.push((prop.clone(), val_a.clone()));
        }
    }

    for (prop, val_b) in &vals_b {
        if !seen.contains(prop.as_str()) {
            result.push((prop.clone(), val_b.clone()));
        }
    }

    result
}

// ---------------------------------------------------------------------------
// glTF animation import (pat-0q2kx.155)
// ---------------------------------------------------------------------------

/// Maps a glTF node index to the node name the importer gave it, from the
/// document's `nodes` array.
fn gltf_node_names(resource: &gdresource::Resource) -> HashMap<i64, String> {
    let mut out = HashMap::new();
    if let Some(Variant::Array(nodes)) = resource.get_property("nodes") {
        for n in nodes.to_vec().iter() {
            if let Variant::Dictionary(d) = n {
                if let (Some(Variant::Int(i)), Some(Variant::String(name))) =
                    (d.get("index"), d.get("name"))
                {
                    out.insert(i, name.clone());
                }
            }
        }
    }
    out
}

/// Reads a `Variant::Array` of `Variant::Float` into `f32`s.
fn gltf_float_array(v: Option<&Variant>) -> Vec<f32> {
    match v {
        Some(Variant::Array(arr)) => arr
            .to_vec()
            .iter()
            .filter_map(|x| match x {
                Variant::Float(f) => Some(*f as f32),
                Variant::Int(i) => Some(*i as f32),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Builds `Animation` clips from a glTF document produced by
/// [`gdresource::import_gltf`] (pat-0q2kx.155).
///
/// `parse_animations` produced this data and nothing consumed it — the imported
/// scene had no clips at all. The importer now publishes it on the document and
/// this is the consumer. The conversion lives here rather than in
/// `gltf_import.rs` because [`Animation`] is a `gdscene` type and `gdscene`
/// depends on `gdresource`, so the dependency edge only runs one way.
///
/// Track mapping follows Godot's `GLTFDocument::_import_animation`:
/// `translation`/`rotation`/`scale` become [`TrackType::Position3D`],
/// [`TrackType::Rotation3D`] and [`TrackType::Scale3D`] on the target node's
/// name; `weights` becomes one [`TrackType::BlendShape`] track per morph target.
pub fn animations_from_gltf_resource(resource: &gdresource::Resource) -> Vec<Animation> {
    let targets: HashMap<i64, GltfTrackTarget> = gltf_node_names(resource)
        .into_iter()
        .map(|(i, name)| (i, GltfTrackTarget::Node(name)))
        .collect();
    animations_from_gltf_resource_targeted(resource, &targets)
}

/// What a glTF animation channel's target node became in the instanced scene
/// (pat-m2jgd).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GltfTrackTarget {
    /// An ordinary node, by its path relative to the mixer root.
    Node(String),
    /// A JOINT, which Godot's importer turns into a bone of a generated
    /// `Skeleton3D`: the track keys `<skeleton path>:<bone>`
    /// (`gltf_document.cpp` `_import_animation`), so it lands on the bone pose
    /// rather than on a node that does not exist.
    Bone {
        /// Path of the `Skeleton3D` relative to the mixer root.
        skeleton: String,
        /// The bone's name on that skeleton.
        bone: String,
    },
}

/// Godot's glTF clip-name loop rule (`gltf_document.cpp:3752`): lower-cased,
/// the name begins or ends with `loop` or `cycle` (pat-3mbf5).
pub fn gltf_name_requests_loop(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["loop", "cycle"]
        .iter()
        .any(|w| lower.starts_with(w) || lower.ends_with(w))
}

/// [`animations_from_gltf_resource`] with each channel's target resolved by
/// the caller, which knows the instanced hierarchy: a nested node needs its
/// full path (not its bare name) and a joint needs its skeleton + bone.
/// A channel whose target is absent from `targets` is dropped.
pub fn animations_from_gltf_resource_targeted(
    resource: &gdresource::Resource,
    targets: &HashMap<i64, GltfTrackTarget>,
) -> Vec<Animation> {
    let Some(Variant::Array(clips)) = resource.get_property("animations") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for clip in clips.to_vec().iter() {
        let Variant::Dictionary(clip) = clip else {
            continue;
        };
        let clip_name = match clip.get("name") {
            Some(Variant::String(s)) => s.clone(),
            _ => format!("animation_{}", out.len()),
        };
        let length = match clip.get("length") {
            Some(Variant::Float(f)) => f,
            _ => 0.0,
        };
        let mut anim = Animation::new(clip_name, length);
        // Godot's glTF importer loops a clip whose name begins or ends with
        // "loop" or "cycle", case-insensitively (`gltf_document.cpp`
        // `_parse_animations`) - the only loop signal a .glb carries itself
        // (pat-3mbf5). A `.import` sidecar can still override it:
        // `gltf_scene::apply_import_animation_settings`.
        if gltf_name_requests_loop(&anim.name) {
            anim.loop_mode = LoopMode::Linear;
        }

        let Some(Variant::Array(channels)) = clip.get("channels") else {
            out.push(anim);
            continue;
        };
        for ch in channels.to_vec() {
            let Variant::Dictionary(ch) = ch else {
                continue;
            };
            let target = match ch.get("target_node") {
                Some(Variant::Int(i)) => i,
                _ => continue,
            };
            // A channel targeting a node the importer did not build cannot be
            // addressed by path — dropping it beats emitting a track that
            // silently animates nothing.
            let Some(target) = targets.get(&target) else {
                continue;
            };
            let (node_name, bone) = match target {
                GltfTrackTarget::Node(path) => (path.clone(), None),
                GltfTrackTarget::Bone { skeleton, bone } => (skeleton.clone(), Some(bone.clone())),
            };
            let path = match ch.get("path") {
                Some(Variant::String(s)) => s,
                _ => continue,
            };
            let path = path.as_str();
            let components = match ch.get("components") {
                Some(Variant::Int(i)) => i.max(1) as usize,
                _ => continue,
            };
            let times = gltf_float_array(ch.get("times").as_ref());
            let values = gltf_float_array(ch.get("values").as_ref());
            if times.is_empty() {
                continue;
            }

            // glTF CUBICSPLINE stores in-tangent, value, out-tangent per key, so
            // each key occupies 3x the components and the value is the middle
            // element. Godot's cubic track interpolates through key values, which
            // is the closest available behaviour.
            let cubic =
                matches!(ch.get("interpolation"), Some(Variant::String(s)) if s == "cubicspline");
            let stride = if cubic { components * 3 } else { components };
            let offset = if cubic { components } else { 0 };

            let transition = match ch.get("interpolation") {
                Some(Variant::String(s)) if s == "step" => TransitionType::Nearest,
                _ => TransitionType::Linear,
            };

            // `weights` drives one track per morph target; the transform paths
            // drive exactly one.
            let track_count = if path == "weights" { components } else { 1 };
            for slot in 0..track_count {
                // Property names must match `implicit_property_path` in
                // `animation_tscn.rs`: a rotation track drives `quaternion`, not
                // `rotation` — Godot's `rotation` is Euler `Vector3`, so writing a
                // quaternion there would set the wrong property with a wrong type.
                // A bone track's sub-path is the BONE name; the track type alone
                // says which pose component it drives
                // (`skeleton3d::apply_animation_bone_tracks`).
                let (track_type, property) = match (path, &bone) {
                    ("translation", Some(b)) => (TrackType::Position3D, b.clone()),
                    ("rotation", Some(b)) => (TrackType::Rotation3D, b.clone()),
                    ("scale", Some(b)) => (TrackType::Scale3D, b.clone()),
                    ("translation", None) => (TrackType::Position3D, "position".to_string()),
                    ("rotation", None) => (TrackType::Rotation3D, "quaternion".to_string()),
                    ("scale", None) => (TrackType::Scale3D, "scale".to_string()),
                    ("weights", _) => (
                        TrackType::BlendShape,
                        // Morph-target names are not carried on the document, so
                        // the target is addressed by index.
                        format!("blend_shapes/morph_{slot}"),
                    ),
                    _ => break,
                };
                let mut track = AnimationTrack::with_type(node_name.clone(), property, track_type);
                if cubic {
                    track.set_interpolation(TrackInterpolation::Cubic);
                }

                for (ki, t) in times.iter().enumerate() {
                    let base = ki * stride + offset;
                    let value = match track_type {
                        TrackType::Position3D | TrackType::Scale3D => {
                            if base + 2 >= values.len() {
                                break;
                            }
                            Variant::Vector3(Vector3::new(
                                values[base],
                                values[base + 1],
                                values[base + 2],
                            ))
                        }
                        TrackType::Rotation3D => {
                            if base + 3 >= values.len() {
                                break;
                            }
                            // glTF stores quaternions xyzw, matching
                            // `Quaternion::new`'s argument order.
                            Variant::Quaternion(gdcore::math3d::Quaternion::new(
                                values[base],
                                values[base + 1],
                                values[base + 2],
                                values[base + 3],
                            ))
                        }
                        _ => {
                            let idx = base + slot;
                            if idx >= values.len() {
                                break;
                            }
                            Variant::Float(values[idx] as f64)
                        }
                    };
                    track.add_keyframe(KeyFrame::new(*t as f64, value, transition));
                }

                if track.keyframe_count() > 0 {
                    anim.tracks.push(track);
                }
            }
        }
        out.push(anim);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Color, Vector2, Vector3};

    // -- KeyFrame / AnimationTrack ------------------------------------------

    #[test]
    fn track_add_keyframes_sorted() {
        let mut track = AnimationTrack::new("position:x");
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(0.5, Variant::Float(5.0)));
        assert_eq!(track.keyframes().len(), 3);
        assert_eq!(track.keyframes()[0].time, 0.0);
        assert_eq!(track.keyframes()[1].time, 0.5);
        assert_eq!(track.keyframes()[2].time, 1.0);
    }

    #[test]
    fn track_sample_empty() {
        let track = AnimationTrack::new("x");
        assert!(track.sample(0.5).is_none());
    }

    #[test]
    fn track_sample_single_keyframe() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(42.0)));
        assert_eq!(track.sample(0.0), Some(Variant::Float(42.0)));
        assert_eq!(track.sample(1.0), Some(Variant::Float(42.0)));
    }

    #[test]
    fn track_sample_linear_interpolation() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));

        let val = track.sample(0.5).unwrap();
        if let Variant::Float(f) = val {
            assert!((f - 5.0).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn track_sample_before_first_keyframe() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        track.add_keyframe(KeyFrame::linear(2.0, Variant::Float(20.0)));
        assert_eq!(track.sample(0.0), Some(Variant::Float(10.0)));
    }

    #[test]
    fn track_sample_after_last_keyframe() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        assert_eq!(track.sample(5.0), Some(Variant::Float(10.0)));
    }

    #[test]
    fn track_sample_nearest_transition() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::new(
            1.0,
            Variant::Float(10.0),
            TransitionType::Nearest,
        ));

        // Godot's NEAREST holds the FLOOR key until the next key is reached
        // (`animation.cpp:2674`, `return p_keys[idx].value`) — there is no
        // midpoint switch, either side of it.
        assert_eq!(track.sample(0.3), Some(Variant::Float(0.0)));
        assert_eq!(track.sample(0.7), Some(Variant::Float(0.0)));
        assert_eq!(track.sample(1.0), Some(Variant::Float(10.0)));
    }

    #[test]
    fn track_sample_at_exact_keyframe_time() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        assert_eq!(track.sample(0.0), Some(Variant::Float(0.0)));
        assert_eq!(track.sample(1.0), Some(Variant::Float(10.0)));
    }

    // -- Animation ----------------------------------------------------------

    #[test]
    fn animation_sample_track() {
        let mut anim = Animation::new("walk", 1.0);
        let mut track = AnimationTrack::new("position:x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(100.0)));
        anim.tracks.push(track);

        let val = anim.sample_track(0, 0.5).unwrap();
        if let Variant::Float(f) = val {
            assert!((f - 50.0).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn animation_sample_track_out_of_bounds() {
        let anim = Animation::new("empty", 1.0);
        assert!(anim.sample_track(0, 0.5).is_none());
    }

    #[test]
    fn animation_sample_all() {
        let mut anim = Animation::new("walk", 1.0);

        let mut t1 = AnimationTrack::new("x");
        t1.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        t1.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        anim.tracks.push(t1);

        let mut t2 = AnimationTrack::new("y");
        t2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
        t2.add_keyframe(KeyFrame::linear(1.0, Variant::Float(200.0)));
        anim.tracks.push(t2);

        let values = anim.sample_all(0.5);
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].0, "x");
        assert_eq!(values[1].0, "y");
    }

    // -- AnimationPlayer ----------------------------------------------------

    #[test]
    fn player_play_and_advance() {
        let mut player = AnimationPlayer::new();
        let mut anim = Animation::new("idle", 2.0);
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(2.0, Variant::Float(20.0)));
        anim.tracks.push(track);
        player.add_animation(anim);

        player.play("idle");
        assert!(player.playing);
        assert_eq!(player.current_animation(), Some("idle"));

        player.advance(1.0);
        assert!((player.position() - 1.0).abs() < 1e-10);

        let values = player.get_current_values();
        assert_eq!(values.len(), 1);
        if let Variant::Float(f) = &values[0].1 {
            assert!((f - 10.0).abs() < 1e-6);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn player_stops_at_end_no_loop() {
        let mut player = AnimationPlayer::new();
        let anim = Animation::new("once", 1.0);
        player.add_animation(anim);

        player.play("once");
        player.advance(2.0);
        assert!(!player.playing);
        assert!((player.position() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn player_loop_linear() {
        let mut player = AnimationPlayer::new();
        let mut anim = Animation::new("loop", 1.0);
        anim.loop_mode = LoopMode::Linear;
        player.add_animation(anim);

        player.play("loop");
        player.advance(1.5);
        assert!(player.playing);
        assert!((player.position() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn player_loop_pingpong() {
        let mut player = AnimationPlayer::new();
        let mut anim = Animation::new("pp", 1.0);
        anim.loop_mode = LoopMode::PingPong;
        player.add_animation(anim);

        player.play("pp");
        // Advance past the end — should bounce back
        player.advance(1.5);
        assert!(player.playing);
        // position should be 0.5 (bounced from 1.0 back by 0.5)
        assert!((player.position() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn player_speed_scale() {
        let mut player = AnimationPlayer::new();
        let anim = Animation::new("fast", 2.0);
        player.add_animation(anim);

        player.play("fast");
        player.speed_scale = 2.0;
        player.advance(0.5);
        assert!((player.position() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn player_stop() {
        let mut player = AnimationPlayer::new();
        let anim = Animation::new("anim", 1.0);
        player.add_animation(anim);

        player.play("anim");
        player.stop();
        assert!(!player.playing);
        player.advance(1.0);
        assert!((player.position() - 0.0).abs() < 1e-10);
    }

    #[test]
    fn player_play_nonexistent_animation() {
        let mut player = AnimationPlayer::new();
        player.play("nonexistent");
        assert!(!player.playing);
        assert!(player.current_animation().is_none());
    }

    #[test]
    fn player_get_current_values_no_animation() {
        let player = AnimationPlayer::new();
        assert!(player.get_current_values().is_empty());
    }

    #[test]
    fn player_default() {
        let player = AnimationPlayer::default();
        assert!(!player.playing);
        assert!(player.current_animation().is_none());
    }

    // -- interpolate_variant ------------------------------------------------

    #[test]
    fn interpolate_float() {
        let result = interpolate_variant(&Variant::Float(0.0), &Variant::Float(10.0), 0.5);
        assert_eq!(result, Some(Variant::Float(5.0)));
    }

    #[test]
    fn interpolate_int() {
        let result = interpolate_variant(&Variant::Int(0), &Variant::Int(10), 0.5);
        assert_eq!(result, Some(Variant::Int(5)));
    }

    #[test]
    fn interpolate_int_float_mixed() {
        let result = interpolate_variant(&Variant::Int(0), &Variant::Float(10.0), 0.5);
        assert_eq!(result, Some(Variant::Float(5.0)));

        let result = interpolate_variant(&Variant::Float(0.0), &Variant::Int(10), 0.5);
        assert_eq!(result, Some(Variant::Float(5.0)));
    }

    #[test]
    fn interpolate_vector2() {
        let a = Variant::Vector2(Vector2::new(0.0, 0.0));
        let b = Variant::Vector2(Vector2::new(10.0, 20.0));
        let result = interpolate_variant(&a, &b, 0.5).unwrap();
        if let Variant::Vector2(v) = result {
            assert!((v.x - 5.0).abs() < 1e-5);
            assert!((v.y - 10.0).abs() < 1e-5);
        } else {
            panic!("expected Vector2");
        }
    }

    #[test]
    fn interpolate_vector3() {
        let a = Variant::Vector3(Vector3::new(0.0, 0.0, 0.0));
        let b = Variant::Vector3(Vector3::new(10.0, 20.0, 30.0));
        let result = interpolate_variant(&a, &b, 0.5).unwrap();
        if let Variant::Vector3(v) = result {
            assert!((v.x - 5.0).abs() < 1e-5);
            assert!((v.y - 10.0).abs() < 1e-5);
            assert!((v.z - 15.0).abs() < 1e-5);
        } else {
            panic!("expected Vector3");
        }
    }

    #[test]
    fn interpolate_color() {
        let a = Variant::Color(Color::new(0.0, 0.0, 0.0, 1.0));
        let b = Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0));
        let result = interpolate_variant(&a, &b, 0.5).unwrap();
        if let Variant::Color(c) = result {
            assert!((c.r - 0.5).abs() < 1e-5);
            assert!((c.g - 0.5).abs() < 1e-5);
            assert!((c.b - 0.5).abs() < 1e-5);
            assert!((c.a - 1.0).abs() < 1e-5);
        } else {
            panic!("expected Color");
        }
    }

    #[test]
    fn interpolate_incompatible_returns_none() {
        let result = interpolate_variant(&Variant::Float(0.0), &Variant::String("hi".into()), 0.5);
        assert!(result.is_none());
    }

    #[test]
    fn interpolate_nil_returns_none() {
        assert!(interpolate_variant(&Variant::Nil, &Variant::Nil, 0.5).is_none());
    }

    // -- TransitionType / LoopMode defaults ---------------------------------

    #[test]
    fn transition_type_default_is_linear() {
        assert_eq!(TransitionType::default(), TransitionType::Linear);
    }

    #[test]
    fn loop_mode_default_is_none() {
        assert_eq!(LoopMode::default(), LoopMode::None);
    }

    // -- Multi-keyframe interpolation ---------------------------------------

    #[test]
    fn track_three_keyframes_interpolation() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        track.add_keyframe(KeyFrame::linear(2.0, Variant::Float(0.0)));

        if let Some(Variant::Float(f)) = track.sample(0.5) {
            assert!((f - 5.0).abs() < 1e-6);
        } else {
            panic!("expected Float at 0.5");
        }

        if let Some(Variant::Float(f)) = track.sample(1.5) {
            assert!((f - 5.0).abs() < 1e-6);
        } else {
            panic!("expected Float at 1.5");
        }
    }

    #[test]
    fn vector2_track_interpolation() {
        let mut track = AnimationTrack::new("position");
        track.add_keyframe(KeyFrame::linear(
            0.0,
            Variant::Vector2(Vector2::new(0.0, 0.0)),
        ));
        track.add_keyframe(KeyFrame::linear(
            1.0,
            Variant::Vector2(Vector2::new(10.0, 20.0)),
        ));

        if let Some(Variant::Vector2(v)) = track.sample(0.5) {
            assert!((v.x - 5.0).abs() < 1e-5);
            assert!((v.y - 10.0).abs() < 1e-5);
        } else {
            panic!("expected Vector2");
        }
    }

    // -- AnimationTrack::with_node -----------------------------------------

    #[test]
    fn track_with_node() {
        let track = AnimationTrack::with_node("Player", "position");
        assert_eq!(track.node_path, "Player");
        assert_eq!(track.property_path, "position");
        assert_eq!(track.keyframe_count(), 0);
    }

    #[test]
    fn track_new_has_empty_node_path() {
        let track = AnimationTrack::new("position:x");
        assert_eq!(track.node_path, "");
        assert_eq!(track.property_path, "position:x");
    }

    // -- remove_keyframe / keyframe_count ----------------------------------

    #[test]
    fn track_remove_keyframe() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
        assert_eq!(track.keyframe_count(), 2);
        assert!(track.remove_keyframe(0));
        assert_eq!(track.keyframe_count(), 1);
        assert_eq!(track.keyframes()[0].time, 1.0);
    }

    #[test]
    fn track_remove_keyframe_out_of_bounds() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        assert!(!track.remove_keyframe(5));
        assert_eq!(track.keyframe_count(), 1);
    }

    #[test]
    fn track_remove_all_keyframes() {
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        assert!(track.remove_keyframe(0));
        assert_eq!(track.keyframe_count(), 0);
        assert!(track.sample(0.0).is_none());
    }

    // -- BlendState ---------------------------------------------------------

    #[test]
    fn blend_state_factor_ramps() {
        let bs = BlendState {
            from_animation: "a".to_string(),
            from_position: 0.0,
            blend_duration: 1.0,
            blend_elapsed: 0.5,
        };
        assert!((bs.blend_factor() - 0.5).abs() < 1e-5);
        assert!(!bs.is_complete());
    }

    #[test]
    fn blend_state_zero_duration() {
        let bs = BlendState {
            from_animation: "a".to_string(),
            from_position: 0.0,
            blend_duration: 0.0,
            blend_elapsed: 0.0,
        };
        assert!((bs.blend_factor() - 1.0).abs() < 1e-5);
        assert!(bs.is_complete());
    }

    #[test]
    fn blend_state_complete() {
        let bs = BlendState {
            from_animation: "a".to_string(),
            from_position: 0.0,
            blend_duration: 0.5,
            blend_elapsed: 0.6,
        };
        assert!((bs.blend_factor() - 1.0).abs() < 1e-5);
        assert!(bs.is_complete());
    }

    // -- AnimationPlayer crossfade ------------------------------------------

    #[test]
    fn player_crossfade_to() {
        let mut player = AnimationPlayer::new();
        let mut anim_a = Animation::new("walk", 2.0);
        let mut t = AnimationTrack::new("x");
        t.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        t.add_keyframe(KeyFrame::linear(2.0, Variant::Float(100.0)));
        anim_a.tracks.push(t);
        player.add_animation(anim_a);

        let mut anim_b = Animation::new("run", 2.0);
        let mut t2 = AnimationTrack::new("x");
        t2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(200.0)));
        t2.add_keyframe(KeyFrame::linear(2.0, Variant::Float(400.0)));
        anim_b.tracks.push(t2);
        player.add_animation(anim_b);

        player.play("walk");
        player.advance(1.0);
        player.crossfade_to("run", 1.0);

        assert_eq!(player.current_animation(), Some("run"));
        assert!(player.blend_state().is_some());
        let bs = player.blend_state().unwrap();
        assert_eq!(bs.from_animation, "walk");
        assert!((bs.from_position - 1.0).abs() < 1e-10);
    }

    #[test]
    fn player_crossfade_blends_values() {
        let mut player = AnimationPlayer::new();

        let mut walk = Animation::new("walk", 2.0);
        let mut t1 = AnimationTrack::new("x");
        t1.add_keyframe(KeyFrame::linear(0.0, Variant::Float(10.0)));
        t1.add_keyframe(KeyFrame::linear(2.0, Variant::Float(10.0)));
        walk.tracks.push(t1);
        player.add_animation(walk);

        let mut run = Animation::new("run", 2.0);
        let mut t2 = AnimationTrack::new("x");
        t2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(20.0)));
        t2.add_keyframe(KeyFrame::linear(2.0, Variant::Float(20.0)));
        run.tracks.push(t2);
        player.add_animation(run);

        player.set_blend_preview("walk", "run", 0.5);
        player.playing = false;

        let values = player.get_current_values();
        assert_eq!(values.len(), 1);
        if let Variant::Float(f) = &values[0].1 {
            assert!((f - 15.0).abs() < 1e-5, "expected 15.0, got {}", f);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn player_crossfade_completes_and_clears() {
        let mut player = AnimationPlayer::new();
        let anim_a = Animation::new("a", 2.0);
        let anim_b = Animation::new("b", 2.0);
        player.add_animation(anim_a);
        player.add_animation(anim_b);

        player.play("a");
        player.crossfade_to("b", 0.5);
        assert!(player.blend_state().is_some());

        player.advance(0.6);
        assert!(
            player.blend_state().is_none(),
            "blend should auto-clear after completion"
        );
        assert_eq!(player.current_animation(), Some("b"));
    }

    #[test]
    fn player_crossfade_to_nonexistent_is_noop() {
        let mut player = AnimationPlayer::new();
        let anim = Animation::new("a", 1.0);
        player.add_animation(anim);
        player.play("a");
        player.crossfade_to("nonexistent", 1.0);
        assert_eq!(player.current_animation(), Some("a"));
        assert!(player.blend_state().is_none());
    }

    #[test]
    fn player_set_blend_preview_weight_edges() {
        let mut player = AnimationPlayer::new();

        let mut walk = Animation::new("walk", 1.0);
        let mut t = AnimationTrack::new("x");
        t.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
        walk.tracks.push(t);
        player.add_animation(walk);

        let mut run = Animation::new("run", 1.0);
        let mut t2 = AnimationTrack::new("x");
        t2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
        run.tracks.push(t2);
        player.add_animation(run);

        // Weight 0.0 = fully "from"
        player.set_blend_preview("walk", "run", 0.0);
        let values = player.get_current_values();
        if let Variant::Float(f) = &values[0].1 {
            assert!((f - 0.0).abs() < 1e-5);
        }

        // Weight 1.0 = fully "to"
        player.set_blend_preview("walk", "run", 1.0);
        let values = player.get_current_values();
        if let Variant::Float(f) = &values[0].1 {
            assert!((f - 100.0).abs() < 1e-5);
        }
    }

    #[test]
    fn player_clear_blend() {
        let mut player = AnimationPlayer::new();
        let anim_a = Animation::new("a", 1.0);
        let anim_b = Animation::new("b", 1.0);
        player.add_animation(anim_a);
        player.add_animation(anim_b);
        player.set_blend_preview("a", "b", 0.5);
        assert!(player.blend_state().is_some());
        player.clear_blend();
        assert!(player.blend_state().is_none());
    }

    /// ⚠️ This test used to assert `AnimationChanged` on a manual clip switch.
    /// That was the bug pat-mgixk.2 fixes: Godot emits `animation_changed` only
    /// when the playback QUEUE advances (`animation_player.cpp:348-350`) and
    /// reports a manual switch with `current_animation_changed`
    /// (`:516`). `advance_past_end_emits_finished_and_plays_queued_next` below
    /// is where `AnimationChanged` is now expected.
    #[test]
    fn play_emits_started_and_current_animation_changed_signals() {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("idle", 1.0));
        p.add_animation(Animation::new("run", 1.0));

        p.play("idle");
        assert_eq!(
            p.drain_signals(),
            vec![
                AnimationPlayerSignal::CurrentAnimationChanged("idle".into()),
                AnimationPlayerSignal::AnimationStarted("idle".into()),
            ]
        );

        p.play("run");
        assert_eq!(
            p.drain_signals(),
            vec![
                AnimationPlayerSignal::CurrentAnimationChanged("run".into()),
                AnimationPlayerSignal::AnimationStarted("run".into()),
            ]
        );
        assert!(!p.has_pending_signals(), "drain clears the queue");
    }

    /// The per-frame `play()` case: Godot returns silently from `_play` while
    /// the same clip is still running, so a looping `play("walk")` in `_process`
    /// emits ONE `animation_started`, not one per frame
    /// (`animation_player.cpp:528-530`).
    #[test]
    fn repeated_play_while_still_playing_emits_one_started() {
        let mut p = AnimationPlayer::new();
        let mut walk = Animation::new("walk", 1.0);
        walk.loop_mode = LoopMode::Linear;
        p.add_animation(walk);

        for _ in 0..10 {
            p.play("walk");
            p.advance(1.0 / 60.0);
        }

        let started = p
            .drain_signals()
            .into_iter()
            .filter(|s| matches!(s, AnimationPlayerSignal::AnimationStarted(n) if n == "walk"))
            .count();
        assert_eq!(started, 1, "a per-frame play() must start the clip once");
    }

    /// The replay case, at the unit level: a non-looping clip that has finished
    /// rewinds on the next `play` instead of sitting at its end
    /// (`animation_player.cpp:526-528`).
    #[test]
    fn play_after_finish_rewinds_and_restarts() {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("attack", 0.2));

        p.play("attack");
        p.advance(0.5);
        assert!(!p.playing, "a non-looping clip stops at its end");
        assert_eq!(p.current_animation(), Some("attack"));
        let _ = p.drain_signals();

        p.play("attack");
        assert!(p.playing, "play() must restart a finished clip");
        assert_eq!(
            p.position(),
            0.0,
            "play() must rewind to the start, not resume at the end",
        );
        let sigs = p.drain_signals();
        assert!(
            sigs.contains(&AnimationPlayerSignal::AnimationStarted("attack".into())),
            "the replay must report a fresh start: {sigs:?}",
        );

        // And it must actually run, rather than finish again at once.
        p.advance(0.1);
        assert!(p.playing, "the replayed clip must still be mid-flight");
    }

    /// Finishing with nothing queued reports the empty current animation
    /// (`animation_player.cpp:356`) while KEEPING `current` assigned, which is
    /// what lets the replay above recognise the clip.
    #[test]
    fn finish_reports_empty_current_animation() {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("attack", 0.2));
        p.play("attack");
        let _ = p.drain_signals();

        p.advance(0.5);
        let sigs = p.drain_signals();
        assert!(
            sigs.contains(&AnimationPlayerSignal::CurrentAnimationChanged(
                String::new()
            )),
            "finish must emit current_animation_changed(\"\"): {sigs:?}",
        );
        assert_eq!(
            p.current_animation(),
            Some("attack"),
            "current stays assigned after finish, as Godot's playback.assigned does",
        );
    }

    #[test]
    fn advance_past_end_emits_finished_and_plays_queued_next() {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("attack", 1.0)); // non-looping (default)
        p.add_animation(Animation::new("idle", 1.0));
        p.animation_set_next("attack", "idle");
        assert_eq!(p.animation_get_next("attack"), Some("idle"));

        p.play("attack");
        let _ = p.drain_signals(); // clear the started signal

        // Advance past the 1.0s length → attack finishes → idle auto-plays.
        p.advance(1.5);
        let sigs = p.drain_signals();
        assert!(
            sigs.contains(&AnimationPlayerSignal::AnimationFinished("attack".into())),
            "finished signal: {sigs:?}"
        );
        assert!(
            sigs.contains(&AnimationPlayerSignal::AnimationStarted("idle".into())),
            "queued-next started: {sigs:?}"
        );
        assert_eq!(p.current_animation(), Some("idle"));
    }

    #[test]
    fn seek_clamps_and_sets_position() {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("clip", 2.0));
        p.play("clip");

        p.seek(1.25, true);
        assert!((p.position() - 1.25).abs() < 1e-9);
        p.seek(99.0, false); // clamped to length
        assert!((p.position() - 2.0).abs() < 1e-9);
        p.seek(-5.0, true); // clamped to 0
        assert!(p.position().abs() < 1e-9);
    }

    #[test]
    fn blend_times_and_autoplay() {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("a", 1.0));
        p.add_animation(Animation::new("b", 1.0));

        p.set_blend_time("a", "b", 0.4);
        assert!((p.get_blend_time("a", "b") - 0.4).abs() < 1e-9);
        assert_eq!(p.get_blend_time("b", "a"), 0.0); // unset pair defaults to 0

        p.autoplay = Some("a".into());
        assert!(p.play_autoplay());
        assert_eq!(p.current_animation(), Some("a"));

        // Autoplay of a missing animation is a no-op.
        p.autoplay = Some("missing".into());
        assert!(!p.play_autoplay());
    }

    // -- pat-u3l8c.102: discrete / cubic / angle interpolation --------------

    #[test]
    fn anim_interp_discrete_holds_previous_key() {
        let mut track = AnimationTrack::new("x");
        track.update_mode = TrackUpdateMode::Discrete;
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0))); // A
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(2.0))); // B

        // Between keys: hold the previous key (A), no interpolation.
        assert_eq!(track.sample(0.5), Some(Variant::Float(1.0)));
        // At the second key: value B.
        assert_eq!(track.sample(1.0), Some(Variant::Float(2.0)));
        // Before all keys: first key.
        assert_eq!(track.sample(-1.0), Some(Variant::Float(1.0)));
    }

    #[test]
    fn anim_interp_cubic_passes_through_keys() {
        let mut track = AnimationTrack::new("x");
        track.interpolation = TrackInterpolation::Cubic;
        let vals = [0.0_f64, 2.0, -1.0, 3.0];
        for (i, v) in vals.iter().enumerate() {
            track.add_keyframe(KeyFrame::linear(i as f64, Variant::Float(*v)));
        }
        // Cubic must pass through the exact key value at each key time.
        for (i, v) in vals.iter().enumerate() {
            match track.sample(i as f64) {
                Some(Variant::Float(f)) => assert!(
                    (f - *v).abs() < 1e-4,
                    "cubic key {i}: expected {v}, got {f}"
                ),
                other => panic!("expected Float at key {i}, got {other:?}"),
            }
        }
    }

    #[test]
    fn anim_interp_cubic_differs_from_linear_at_midpoint() {
        // Non-collinear keys: 0, 0, 10, 0 at t = 0, 1, 2, 3.
        let vals = [0.0_f64, 0.0, 10.0, 0.0];
        let mut cubic = AnimationTrack::new("x");
        cubic.interpolation = TrackInterpolation::Cubic;
        let mut linear = AnimationTrack::new("x");
        for (i, v) in vals.iter().enumerate() {
            cubic.add_keyframe(KeyFrame::linear(i as f64, Variant::Float(*v)));
            linear.add_keyframe(KeyFrame::linear(i as f64, Variant::Float(*v)));
        }

        // Midpoint of the [t=1, t=2] segment (from=0, to=10).
        let c = match cubic.sample(1.5) {
            Some(Variant::Float(f)) => f,
            o => panic!("expected Float, got {o:?}"),
        };
        let l = match linear.sample(1.5) {
            Some(Variant::Float(f)) => f,
            o => panic!("expected Float, got {o:?}"),
        };
        assert!(
            (l - 5.0).abs() < 1e-4,
            "linear midpoint should be 5.0, got {l}"
        );
        assert!(
            (c - l).abs() > 1e-2,
            "cubic ({c}) must differ from linear ({l})"
        );
        // Catmull-Rom overshoots above the linear midpoint here (≈ 5.625).
        assert!((c - 5.625).abs() < 1e-3, "cubic midpoint ≈ 5.625, got {c}");
    }

    #[test]
    fn anim_interp_linear_angle_shortest_path() {
        use std::f32::consts::PI;
        let from = 350.0_f64.to_radians(); // ≈ 6.108
        let to = 10.0_f64.to_radians(); // ≈ 0.1745
        let mut track = AnimationTrack::new("rotation");
        track.interpolation = TrackInterpolation::LinearAngle;
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(from)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(to)));

        let mid = match track.sample(0.5) {
            Some(Variant::Float(f)) => f as f32,
            o => panic!("expected Float, got {o:?}"),
        };
        // Shortest path is +20° through the 360° wrap → midpoint ≈ 0 (mod 2π),
        // NOT −340° through ~180°.
        let wrapped = wrapf(mid, -PI, PI);
        assert!(
            wrapped.abs() < 0.05,
            "expected ≈0 (mod 2π), got {mid} (wrapped {wrapped})"
        );
    }

    #[test]
    fn anim_interp_cubic_angle_wraps() {
        use std::f32::consts::PI;
        // Angles (wrapped into [0, 2π)) marching across the 0/2π boundary.
        let vals = [6.0_f64, 6.2, 0.1, 0.3];
        let mut track = AnimationTrack::new("rotation");
        track.interpolation = TrackInterpolation::CubicAngle;
        for (i, v) in vals.iter().enumerate() {
            track.add_keyframe(KeyFrame::linear(i as f64, Variant::Float(*v)));
        }

        // Sample the [t=1 (6.2), t=2 (0.1)] segment, which straddles 2π.
        let mid = match track.sample(1.5) {
            Some(Variant::Float(f)) => f as f32,
            o => panic!("expected Float, got {o:?}"),
        };
        // The unwrapped cubic stays near the 0/2π boundary — it must NOT
        // overshoot toward π.
        let wrapped = wrapf(mid, -PI, PI);
        assert!(
            wrapped.abs() < 0.3,
            "cubic-angle should stay near the 0/2π boundary, got {mid} (wrapped {wrapped})"
        );
    }
}

/// Root motion sample (pat-z35r2.10).
#[derive(Debug, Clone, Copy)]
pub struct RootMotionDelta {
    pub position: gdcore::math::Vector3,
    pub rotation_y: f32,
}

impl Default for RootMotionDelta {
    fn default() -> Self {
        Self {
            position: gdcore::math::Vector3::ZERO,
            rotation_y: 0.0,
        }
    }
}

/// Accumulate root motion from a position track delta.
pub fn compute_root_motion_delta(
    prev: gdcore::math::Vector3,
    next: gdcore::math::Vector3,
) -> RootMotionDelta {
    RootMotionDelta {
        position: gdcore::math::Vector3::new(next.x - prev.x, next.y - prev.y, next.z - prev.z),
        rotation_y: 0.0,
    }
}

#[cfg(test)]
mod root_motion_tests {
    use super::*;
    use gdcore::math::Vector3;

    #[test]
    fn root_motion_delta() {
        let d = compute_root_motion_delta(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 2.0));
        assert!((d.position.x - 1.0).abs() < 1e-5);
        assert!((d.position.z - 2.0).abs() < 1e-5);
    }

    #[test]
    fn method_calls_fire_once_on_crossing() {
        let mut anim = Animation::new("a", 2.0);
        let mut track = AnimationTrack::with_type("Enemy", "attack", TrackType::Method);
        track.add_keyframe(KeyFrame::linear(
            1.0,
            Variant::Array(vec![Variant::String("on_hit".into()), Variant::Int(7)].into()),
        ));
        anim.tracks.push(track);
        let mut player = AnimationPlayer::new();
        player.add_animation(anim);
        player.play("a");

        // Interval before the keyframe → no calls.
        assert!(player.method_calls_in_range(0.0, 0.5).is_empty());

        // Interval crossing t=1.0 → one parsed call (name, node path, args).
        let calls = player.method_calls_in_range(0.5, 1.5);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "on_hit");
        assert_eq!(calls[0].node_path, "Enemy");
        assert_eq!(calls[0].args, vec![Variant::Int(7)]);

        // A keyframe already passed does not fire again.
        assert!(player.method_calls_in_range(1.5, 2.0).is_empty());
    }

    #[test]
    fn method_calls_ignore_non_method_tracks() {
        let mut anim = Animation::new("a", 2.0);
        // A property track is never treated as a method call.
        let mut track = AnimationTrack::with_type("N", "opacity", TrackType::Property);
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(1.0)));
        anim.tracks.push(track);
        let mut player = AnimationPlayer::new();
        player.add_animation(anim);
        player.play("a");
        assert!(player.method_calls_in_range(0.0, 2.0).is_empty());
    }

    #[test]
    fn root_motion_consumes_track_delta() {
        let mut anim = Animation::new("walk", 2.0);
        let mut track = AnimationTrack::with_type("Root", "position", TrackType::Property);
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
        track.add_keyframe(KeyFrame::linear(
            2.0,
            Variant::Vector3(Vector3::new(4.0, 0.0, 8.0)),
        ));
        anim.tracks.push(track);
        let mut player = AnimationPlayer::new();
        player.add_animation(anim);
        player.play("walk");
        player.root_motion_track = Some("position".to_string());

        // Linear track: delta from t=0.0 to t=1.0 is half the total → (2, 0, 4).
        let d = player.consume_root_motion(0.0, 1.0);
        assert!((d.position.x - 2.0).abs() < 1e-4, "x={}", d.position.x);
        assert!((d.position.z - 4.0).abs() < 1e-4, "z={}", d.position.z);

        // No root-motion track configured → zero delta.
        player.root_motion_track = None;
        assert_eq!(player.consume_root_motion(0.0, 1.0).position, Vector3::ZERO);
    }

    #[test]
    fn non_property_paths_lists_method_and_root_tracks() {
        let mut anim = Animation::new("a", 1.0);
        anim.tracks.push(AnimationTrack::with_type(
            "N",
            "position",
            TrackType::Property,
        ));
        anim.tracks
            .push(AnimationTrack::with_type("N", "fire", TrackType::Method));
        let mut player = AnimationPlayer::new();
        player.add_animation(anim);
        player.play("a");
        player.root_motion_track = Some("position".to_string());

        let skip = player.non_property_track_paths();
        assert!(skip.contains(&"fire".to_string()), "method track excluded");
        assert!(
            skip.contains(&"position".to_string()),
            "root track excluded"
        );
        // A plain property track that is not root motion stays applicable.
        assert_eq!(skip.iter().filter(|p| *p == "position").count(), 1);
    }
}

#[cfg(test)]
mod track_3d_tests {
    use super::*;
    use gdcore::math::Vector3;
    use gdcore::math3d::Quaternion;

    #[test]
    fn track_type_3d_variants_round_trip() {
        for (s, t) in [
            ("position_3d", TrackType::Position3D),
            ("rotation_3d", TrackType::Rotation3D),
            ("scale_3d", TrackType::Scale3D),
            ("blend_shape", TrackType::BlendShape),
        ] {
            assert_eq!(TrackType::from_str_name(s), Some(t));
            assert_eq!(t.as_str(), s);
        }
    }

    #[test]
    fn rotation_slerps_shortest_path() {
        let axis = Vector3::new(0.0, 1.0, 0.0);
        let a = Quaternion::IDENTITY;
        // Nearly a full turn: its dot with identity is ≈ -0.99 (far hemisphere).
        let b = Quaternion::from_axis_angle(axis, 1.9 * std::f32::consts::PI);
        let mid = match interpolate_variant(&Variant::Quaternion(a), &Variant::Quaternion(b), 0.5) {
            Some(Variant::Quaternion(q)) => q,
            other => panic!("expected Quaternion, got {other:?}"),
        };
        assert!(
            (mid.length() - 1.0).abs() < 1e-4,
            "slerp result is normalized, len={}",
            mid.length()
        );
        // Shortest arc barely moves from identity, so w stays near 1; a naive
        // lerp toward b's negative w would collapse w to ~0.
        assert!(
            mid.w > 0.9,
            "shortest-path slerp stays near identity, w={}",
            mid.w
        );
    }

    #[test]
    fn transform3d_interpolates_translation() {
        let mut a = Transform3D::IDENTITY;
        a.origin = Vector3::new(0.0, 0.0, 0.0);
        let mut b = Transform3D::IDENTITY;
        b.origin = Vector3::new(4.0, 8.0, 0.0);
        let mid = match interpolate_variant(&Variant::Transform3D(a), &Variant::Transform3D(b), 0.5)
        {
            Some(Variant::Transform3D(tf)) => tf,
            other => panic!("expected Transform3D, got {other:?}"),
        };
        assert!(
            (mid.origin.x - 2.0).abs() < 1e-5,
            "origin.x lerps to 2, got {}",
            mid.origin.x
        );
        assert!(
            (mid.origin.y - 4.0).abs() < 1e-5,
            "origin.y lerps to 4, got {}",
            mid.origin.y
        );
    }

    #[test]
    fn rotation_track_sample_slerps() {
        // A Rotation3D track with two quaternion keyframes slerps when sampled.
        let mut track = AnimationTrack::with_type("Player", "quaternion", TrackType::Rotation3D);
        let axis = Vector3::new(0.0, 1.0, 0.0);
        track.add_keyframe(KeyFrame::linear(
            0.0,
            Variant::Quaternion(Quaternion::IDENTITY),
        ));
        track.add_keyframe(KeyFrame::linear(
            1.0,
            Variant::Quaternion(Quaternion::from_axis_angle(
                axis,
                std::f32::consts::FRAC_PI_2,
            )),
        ));
        match track.sample(0.5) {
            Some(Variant::Quaternion(q)) => {
                assert!(
                    (q.length() - 1.0).abs() < 1e-4,
                    "sampled rotation normalized"
                );
            }
            other => panic!("expected sampled Quaternion, got {other:?}"),
        }
    }
}
