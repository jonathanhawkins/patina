//! Animation tracks, keyframes, and AnimationPlayer.
//!
//! Provides a Godot-compatible animation system with keyframe-based tracks,
//! loop modes, and an [`AnimationPlayer`] that advances playback and samples
//! track values at the current position.

use std::collections::HashMap;

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
}

impl KeyFrame {
    /// Creates a new keyframe.
    pub fn new(time: f64, value: Variant, transition: TransitionType) -> Self {
        Self {
            time,
            value,
            transition,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
        }
    }
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
            keyframes: Vec::new(),
        }
    }

    /// Creates a new empty property track with both node path and property path.
    pub fn with_node(node_path: impl Into<String>, property_path: impl Into<String>) -> Self {
        Self {
            node_path: node_path.into(),
            property_path: property_path.into(),
            track_type: TrackType::Property,
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
            keyframes: Vec::new(),
        }
    }

    /// Returns the track type.
    pub fn track_type(&self) -> TrackType {
        self.track_type
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
                match b.transition {
                    TransitionType::Nearest => {
                        let mid = (a.time + b.time) / 2.0;
                        if time < mid {
                            return Some(a.value.clone());
                        } else {
                            return Some(b.value.clone());
                        }
                    }
                    TransitionType::Linear => {
                        let duration = b.time - a.time;
                        if duration < 1e-10 {
                            return Some(b.value.clone());
                        }
                        let t = ((time - a.time) / duration) as f32;
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
        let mut results = Vec::new();
        for track in &self.tracks {
            if let Some(value) = track.sample(time) {
                results.push((
                    track.node_path.clone(),
                    track.property_path.clone(),
                    value,
                ));
            }
        }
        results
    }
}

// ---------------------------------------------------------------------------
// AnimationPlayer
// ---------------------------------------------------------------------------

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
}

/// Tracks an in-progress crossfade between two animations.
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
        }
    }

    /// Adds an animation to the player's library.
    pub fn add_animation(&mut self, anim: Animation) {
        self.animations.insert(anim.name.clone(), anim);
    }

    /// Starts playing the named animation from the beginning.
    pub fn play(&mut self, name: &str) {
        if self.animations.contains_key(name) {
            self.current = Some(name.to_owned());
            self.position = 0.0;
            self.playing = true;
            self.ping_pong_forward = true;
        }
    }

    /// Stops playback.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Returns the currently playing animation name.
    pub fn current_animation(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Returns the current playback position.
    pub fn position(&self) -> f64 {
        self.position
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
        let scaled_delta = delta * self.speed_scale;

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
                if self.position >= length {
                    self.position = length;
                    self.playing = false;
                } else if self.position < 0.0 {
                    self.position = 0.0;
                    self.playing = false;
                }
            }
            LoopMode::Linear => {
                self.position += scaled_delta;
                if self.position >= length {
                    self.position %= length;
                } else if self.position < 0.0 {
                    self.position = length + (self.position % length);
                    if self.position >= length {
                        self.position = 0.0;
                    }
                }
            }
            LoopMode::PingPong => {
                if self.ping_pong_forward {
                    self.position += scaled_delta;
                    if self.position >= length {
                        self.position = 2.0 * length - self.position;
                        self.ping_pong_forward = false;
                        if self.position < 0.0 {
                            self.position = 0.0;
                            self.ping_pong_forward = true;
                        }
                    }
                } else {
                    self.position -= scaled_delta;
                    if self.position <= 0.0 {
                        self.position = -self.position;
                        self.ping_pong_forward = true;
                        if self.position > length {
                            self.position = length;
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
        let to_anim = match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(a) => a,
            None => return Vec::new(),
        };
        let to_values = to_anim.sample_all_targeted(self.position);

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
        let from_values = from_anim.sample_all_targeted(bs.from_position);
        let from_map: HashMap<(&str, &str), &Variant> = from_values
            .iter()
            .map(|(n, p, v)| ((n.as_str(), p.as_str()), v))
            .collect();

        let mut result = Vec::with_capacity(to_values.len());
        for (node, prop, to_val) in &to_values {
            if let Some(from_val) = from_map.get(&(node.as_str(), prop.as_str())) {
                if let Some(blended) = interpolate_variant(from_val, to_val, factor) {
                    result.push((node.clone(), prop.clone(), blended));
                } else {
                    result.push((node.clone(), prop.clone(), to_val.clone()));
                }
            } else {
                result.push((node.clone(), prop.clone(), to_val.clone()));
            }
        }
        result
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

    /// Returns the property paths of the current animation that must NOT be
    /// applied to nodes as ordinary properties: method-track paths (dispatched
    /// as calls) and the root-motion track (applied as a transform delta).
    pub fn non_property_track_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(anim) = self.current.as_ref().and_then(|n| self.animations.get(n)) {
            for track in &anim.tracks {
                if track.track_type == TrackType::Method {
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
fn method_call_from_value(node_path: &str, value: &Variant) -> Option<MethodCall> {
    if let Variant::Array(items) = value {
        let method = match items.first() {
            Some(Variant::String(s)) => s.clone(),
            _ => return None,
        };
        let args = items.iter().skip(1).cloned().collect();
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

// ---------------------------------------------------------------------------
// cubic_bezier_y
// ---------------------------------------------------------------------------

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

        // Before midpoint → first value
        assert_eq!(track.sample(0.3), Some(Variant::Float(0.0)));
        // After midpoint → second value
        assert_eq!(track.sample(0.7), Some(Variant::Float(10.0)));
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
            Variant::Array(vec![Variant::String("on_hit".into()), Variant::Int(7)]),
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
        anim.tracks
            .push(AnimationTrack::with_type("N", "position", TrackType::Property));
        anim.tracks
            .push(AnimationTrack::with_type("N", "fire", TrackType::Method));
        let mut player = AnimationPlayer::new();
        player.add_animation(anim);
        player.play("a");
        player.root_motion_track = Some("position".to_string());

        let skip = player.non_property_track_paths();
        assert!(skip.contains(&"fire".to_string()), "method track excluded");
        assert!(skip.contains(&"position".to_string()), "root track excluded");
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
        assert!((mid.origin.x - 2.0).abs() < 1e-5, "origin.x lerps to 2, got {}", mid.origin.x);
        assert!((mid.origin.y - 4.0).abs() < 1e-5, "origin.y lerps to 4, got {}", mid.origin.y);
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
            Variant::Quaternion(Quaternion::from_axis_angle(axis, std::f32::consts::FRAC_PI_2)),
        ));
        match track.sample(0.5) {
            Some(Variant::Quaternion(q)) => {
                assert!((q.length() - 1.0).abs() < 1e-4, "sampled rotation normalized");
            }
            other => panic!("expected sampled Quaternion, got {other:?}"),
        }
    }
}
