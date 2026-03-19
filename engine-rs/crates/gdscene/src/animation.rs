//! Animation tracks, keyframes, and AnimationPlayer.
//!
//! Provides a Godot-compatible animation system with keyframe-based tracks,
//! loop modes, and an [`AnimationPlayer`] that advances playback and samples
//! track values at the current position.

use std::collections::HashMap;

use gdvariant::Variant;

// ---------------------------------------------------------------------------
// TransitionType
// ---------------------------------------------------------------------------

/// How values are interpolated between keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransitionType {
    /// Linear interpolation between keyframe values.
    #[default]
    Linear,
    /// Snap to the nearest keyframe value (no interpolation).
    Nearest,
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
// AnimationTrack
// ---------------------------------------------------------------------------

/// A track that animates a single property over time via keyframes.
#[derive(Debug, Clone)]
pub struct AnimationTrack {
    /// The node path this track targets (e.g. `"Player"`).
    pub node_path: String,
    /// The property path this track targets (e.g. `"position:x"`).
    pub property_path: String,
    /// Keyframes sorted by time.
    keyframes: Vec<KeyFrame>,
}

impl AnimationTrack {
    /// Creates a new empty track for the given property path.
    pub fn new(property_path: impl Into<String>) -> Self {
        Self {
            node_path: String::new(),
            property_path: property_path.into(),
            keyframes: Vec::new(),
        }
    }

    /// Creates a new empty track with both node path and property path.
    pub fn with_node(node_path: impl Into<String>, property_path: impl Into<String>) -> Self {
        Self {
            node_path: node_path.into(),
            property_path: property_path.into(),
            keyframes: Vec::new(),
        }
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
                }
            }
        }

        Some(last.value.clone())
    }
}

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

    /// Returns the current values of all tracks in the current animation.
    pub fn get_current_values(&self) -> Vec<(String, Variant)> {
        match self.current.as_ref().and_then(|n| self.animations.get(n)) {
            Some(anim) => anim.sample_all(self.position),
            None => Vec::new(),
        }
    }
}

impl Default for AnimationPlayer {
    fn default() -> Self {
        Self::new()
    }
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
        _ => None,
    }
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
}
