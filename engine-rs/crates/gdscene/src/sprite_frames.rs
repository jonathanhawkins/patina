//! Sprite sheet regions + `SpriteFrames` animation playback (pat-u3l8c.26).
//!
//! `region_enabled`/`region_rect`/`hframes`/`vframes` previously existed only as
//! inspector schema and were ignored by the renderer, and there was no
//! `SpriteFrames` resource or frame advance anywhere. This module provides:
//!
//! * [`sprite_source_rect`] — the texture sub-rectangle a `Sprite2D` should draw
//!   given its region / sheet-frame settings.
//! * [`SpriteFrames`] + [`AnimatedSpritePlayback`] — an animation set and a
//!   playback state machine that advances at the animation FPS and signals
//!   `animation_finished`.
//!
//! ⚠️ The playback half is **dimension-agnostic**: [`AnimatedSpritePlayback`]
//! holds an animation name, a frame index and a clock, and [`SpriteFrames`] is
//! just the frame table. Neither inspects a node or a class name, so both serve
//! `AnimatedSprite3D` exactly as they serve `AnimatedSprite2D`. What is 2D-only
//! is the DRIVER that walks the tree looking for nodes to advance
//! (`main_loop.rs`) — adding 3D there needs no new playback code (pat-s3rrv.92).

use std::collections::HashMap;

use gdcore::math::{Rect2, Vector2};

/// Returns the sub-rectangle of the source texture a sprite should sample.
///
/// * If `region_enabled`, the explicit `region_rect` is used.
/// * Otherwise, if the sheet has more than one cell (`hframes`/`vframes`), the
///   texture is divided into a grid and cell `frame` (row-major, wrapping) is
///   selected.
/// * Otherwise the whole texture is used.
pub fn sprite_source_rect(
    texture_size: Vector2,
    region_enabled: bool,
    region_rect: Rect2,
    hframes: i32,
    vframes: i32,
    frame: i32,
) -> Rect2 {
    if region_enabled {
        return region_rect;
    }
    let h = hframes.max(1);
    let v = vframes.max(1);
    if h == 1 && v == 1 {
        return Rect2::new(Vector2::new(0.0, 0.0), texture_size);
    }
    let cells = h * v;
    let f = frame.rem_euclid(cells);
    let col = f % h;
    let row = f / h;
    let fw = texture_size.x / h as f32;
    let fh = texture_size.y / v as f32;
    Rect2::new(
        Vector2::new(col as f32 * fw, row as f32 * fh),
        Vector2::new(fw, fh),
    )
}

/// One frame of a [`SpriteAnimation`] — a texture plus how long it is held
/// (pat-0q2kx.100).
///
/// Godot's `SpriteFrames` stores a `Texture2D` and a **relative** `duration`
/// per frame: the frame is shown for `duration / speed` seconds, so a duration
/// of `2.0` holds twice as long as its neighbours without changing the
/// animation's fps (`scene/resources/2d/sprite_frames.cpp`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteFrameEntry {
    /// Resource path of the frame's texture (`res://…`), empty when the frame
    /// carries no texture.
    pub texture: String,
    /// Relative hold time, `1.0` being one whole frame at the animation's speed.
    pub duration: f32,
}

impl SpriteFrameEntry {
    /// A frame showing `texture` for the standard single-frame duration.
    pub fn new(texture: impl Into<String>) -> Self {
        Self {
            texture: texture.into(),
            duration: 1.0,
        }
    }

    /// A frame showing `texture` held for `duration` frame-times.
    pub fn with_duration(texture: impl Into<String>, duration: f32) -> Self {
        Self {
            texture: texture.into(),
            duration: duration.max(0.0),
        }
    }
}

/// A single named animation within a [`SpriteFrames`] resource.
#[derive(Debug, Clone)]
pub struct SpriteAnimation {
    /// The animation's frames, in play order.
    ///
    /// Replaces the bare frame *count* this carried before pat-0q2kx.100: the
    /// count alone could drive playback state but never say which texture to
    /// draw, which is why an animated sprite rendered as an invisible marker.
    pub frames: Vec<SpriteFrameEntry>,
    /// Playback rate in frames per second.
    pub speed: f32,
    /// Whether the animation loops back to frame 0 at the end.
    pub loops: bool,
}

impl SpriteAnimation {
    /// Number of frames in the animation.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The texture path of frame `index`, if it has one.
    pub fn texture(&self, index: usize) -> Option<&str> {
        self.frames
            .get(index)
            .map(|f| f.texture.as_str())
            .filter(|t| !t.is_empty())
    }

    /// How long frame `index` is held, in seconds, at this animation's speed.
    ///
    /// Zero when the animation has no speed, which the caller treats as
    /// "never advances" rather than dividing by zero.
    pub fn frame_seconds(&self, index: usize) -> f32 {
        if self.speed <= 0.0 {
            return 0.0;
        }
        let duration = self.frames.get(index).map(|f| f.duration).unwrap_or(1.0);
        duration.max(0.0) / self.speed
    }
}

/// A set of named animations, mirroring Godot's `SpriteFrames` resource.
#[derive(Debug, Clone, Default)]
pub struct SpriteFrames {
    animations: HashMap<String, SpriteAnimation>,
}

impl SpriteFrames {
    /// Creates an empty resource.
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
        }
    }

    /// Adds (or replaces) an animation of `frame_count` textureless frames.
    ///
    /// Kept for playback-only callers that care about timing rather than
    /// pixels; [`add_animation_frames`](Self::add_animation_frames) is the form
    /// a real `SpriteFrames` resource loads into.
    pub fn add_animation(&mut self, name: &str, frame_count: usize, speed: f32, loops: bool) {
        let frames = (0..frame_count)
            .map(|_| SpriteFrameEntry::new(""))
            .collect();
        self.add_animation_frames(name, frames, speed, loops);
    }

    /// Adds (or replaces) an animation from its real frames.
    pub fn add_animation_frames(
        &mut self,
        name: &str,
        frames: Vec<SpriteFrameEntry>,
        speed: f32,
        loops: bool,
    ) {
        self.animations.insert(
            name.to_string(),
            SpriteAnimation {
                frames,
                speed,
                loops,
            },
        );
    }

    /// The texture path of frame `index` of `name`, if any.
    ///
    /// This is what the renderer asks for: the node's `frame` property indexes
    /// into the animation, and the answer is the texture to draw this tick.
    pub fn frame_texture(&self, name: &str, index: usize) -> Option<&str> {
        self.animations.get(name)?.texture(index)
    }

    /// Names of every animation, sorted — the backing map is a `HashMap`, so an
    /// unsorted iteration would differ run to run.
    pub fn animation_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.animations.keys().cloned().collect();
        names.sort();
        names
    }

    /// Returns the animation, if present.
    pub fn animation(&self, name: &str) -> Option<&SpriteAnimation> {
        self.animations.get(name)
    }

    /// Number of frames in `name` (0 if the animation is absent).
    pub fn frame_count(&self, name: &str) -> usize {
        self.animations
            .get(name)
            .map(|a| a.frame_count())
            .unwrap_or(0)
    }

    /// FPS of `name` (0 if absent).
    pub fn speed(&self, name: &str) -> f32 {
        self.animations.get(name).map(|a| a.speed).unwrap_or(0.0)
    }

    /// Whether `name` loops (false if absent).
    pub fn loops(&self, name: &str) -> bool {
        self.animations.get(name).map(|a| a.loops).unwrap_or(false)
    }
}

/// Playback state for an animated sprite, 2D or 3D.
///
/// ⚠️ Nothing here is dimension-specific — it is an animation name, a frame
/// index and a clock. The previous doc said `AnimatedSprite2D`, which read as a
/// limitation of this type and sent at least two passes looking for 3D playback
/// code to write; the only thing missing for 3D is a driver that walks the tree
/// and calls [`advance`](Self::advance) (pat-s3rrv.92).
#[derive(Debug, Clone)]
pub struct AnimatedSpritePlayback {
    /// The currently-selected animation name.
    pub animation: String,
    /// Current frame index.
    pub frame: usize,
    /// Time accumulated toward the next frame (seconds).
    pub time: f32,
    /// Whether playback is currently advancing.
    pub playing: bool,
    /// Whether a non-looping animation has reached its end.
    pub finished: bool,
    /// Multiplier on playback speed (Godot `speed_scale`).
    pub speed_scale: f32,
}

/// What one [`AnimatedSpritePlayback::advance`] call did, beyond moving the
/// playhead.
///
/// ⚠️ `looped` and `finished` are mutually exclusive per call and mean opposite
/// things: a looping clip WRAPS (and keeps playing), a non-looping clip STOPS.
/// `advance` used to return a bare `bool` meaning "just finished", which had
/// nowhere to report a wrap — the reason `animation_looped` was declared in
/// ClassDB and never emitted (pat-s3rrv.182).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AdvanceOutcome {
    /// A non-looping clip reached its end this call, for the first time.
    pub finished: bool,
    /// A looping clip wrapped past an end this call.
    pub looped: bool,
}

impl AnimatedSpritePlayback {
    /// Starts playback of `animation` from frame 0.
    pub fn new(animation: &str) -> Self {
        Self {
            animation: animation.to_string(),
            frame: 0,
            time: 0.0,
            playing: true,
            finished: false,
            speed_scale: 1.0,
        }
    }

    /// Switches to `animation`, restarting from frame 0.
    pub fn play(&mut self, animation: &str) {
        self.animation = animation.to_string();
        self.frame = 0;
        self.time = 0.0;
        self.playing = true;
        self.finished = false;
    }

    /// Advances playback by `dt` seconds against `frames`.
    ///
    /// Returns `true` exactly on the step a non-looping animation reaches its
    /// last frame, so the caller can emit `animation_finished` once. Looping
    /// animations wrap and never report finished.
    pub fn advance(&mut self, dt: f32, frames: &SpriteFrames) -> AdvanceOutcome {
        let mut out = AdvanceOutcome::default();
        if !self.playing {
            return out;
        }
        let count = frames.frame_count(&self.animation);
        let fps = frames.speed(&self.animation);
        if count == 0 || fps <= 0.0 {
            return out;
        }
        let loops = frames.loops(&self.animation);
        let interval = 1.0 / fps;
        // ⚠️ `fps` is the CLIP's rate and must stay positive — the direction
        // lives in `speed_scale`, which Godot allows to be negative
        // (`play_backwards` is literally `play(name, -1, true)`,
        // `animated_sprite_2d.cpp:520-522`). Folding the sign into `interval`
        // instead would make the `<= 0.0` guard above reject every reversed
        // clip as if it had no frame rate.
        self.time += dt * self.speed_scale;
        let backwards = self.speed_scale < 0.0;

        if backwards {
            // Mirror image of the forward loop: time runs negative, and the
            // wrap goes 0 -> last rather than last -> 0
            // (`animated_sprite_2d.cpp:66-72`).
            while self.time <= -interval {
                self.time += interval;
                if self.frame > 0 {
                    self.frame -= 1;
                } else if loops {
                    self.frame = count - 1;
                    out.looped = true;
                } else {
                    // Rest on frame 0, the reverse of resting on the last one.
                    self.playing = false;
                    if !self.finished {
                        self.finished = true;
                        out.finished = true;
                    }
                    break;
                }
            }
        } else {
            while self.time >= interval {
                self.time -= interval;
                if self.frame + 1 < count {
                    self.frame += 1;
                } else if loops {
                    self.frame = 0;
                    out.looped = true;
                } else {
                    // Rest on the last frame and fire animation_finished once.
                    self.playing = false;
                    if !self.finished {
                        self.finished = true;
                        out.finished = true;
                    }
                    break;
                }
            }
        }
        out
    }

    /// Starts playing `animation` in reverse from its last frame — Godot's
    /// `play_backwards(name)`, which is exactly `play(name, -1, true)`
    /// (`animated_sprite_2d.cpp:520-522`).
    ///
    /// ⚠️ `from_end` is the half that is easy to drop. Reversing the speed alone
    /// leaves the playhead at frame 0, where a non-looping clip is already at
    /// its reverse end — it would finish immediately without showing a frame.
    pub fn play_backwards(&mut self, animation: &str, frames: &SpriteFrames) {
        self.animation = animation.to_string();
        self.frame = frames.frame_count(animation).saturating_sub(1);
        self.time = 0.0;
        self.speed_scale = -self.speed_scale.abs().max(f32::MIN_POSITIVE);
        self.playing = true;
        self.finished = false;
    }

    /// Godot's `pause()`: freeze where we are.
    ///
    /// ⚠️ Distinct from `stop()`, which resets. Godot spells both as
    /// `_stop_internal(reset)` (`animated_sprite_2d.cpp:524-540`) — the whole
    /// difference is whether `frame` and progress are cleared, and collapsing
    /// the two loses the ability to resume mid-clip.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Godot's `get_playing_speed()`: the SIGNED effective speed, or 0 while
    /// stopped. Negative means the clip is running backwards.
    pub fn playing_speed(&self) -> f32 {
        if self.playing {
            self.speed_scale
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn sheet_frame_selects_subrect() {
        // 40x20 texture, 4x2 grid -> 10x10 cells. Frame 5 = col 1, row 1.
        let src = sprite_source_rect(
            Vector2::new(40.0, 20.0),
            false,
            Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(0.0, 0.0)),
            4,
            2,
            5,
        );
        assert!(
            approx(src.position.x, 10.0) && approx(src.position.y, 10.0),
            "pos {:?}",
            src.position
        );
        assert!(
            approx(src.size.x, 10.0) && approx(src.size.y, 10.0),
            "size {:?}",
            src.size
        );
    }

    #[test]
    fn frame_index_wraps_across_grid() {
        // 9-cell grid, frame 9 wraps to cell 0.
        let src = sprite_source_rect(
            Vector2::new(30.0, 30.0),
            false,
            Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(0.0, 0.0)),
            3,
            3,
            9,
        );
        assert!(approx(src.position.x, 0.0) && approx(src.position.y, 0.0));
    }

    #[test]
    fn region_enabled_uses_region_rect() {
        let region = Rect2::new(Vector2::new(4.0, 6.0), Vector2::new(12.0, 8.0));
        let src = sprite_source_rect(Vector2::new(64.0, 64.0), true, region, 4, 4, 3);
        assert!(approx(src.position.x, 4.0) && approx(src.position.y, 6.0));
        assert!(approx(src.size.x, 12.0) && approx(src.size.y, 8.0));
    }

    #[test]
    fn no_frames_uses_full_texture() {
        let src = sprite_source_rect(
            Vector2::new(48.0, 24.0),
            false,
            Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(0.0, 0.0)),
            1,
            1,
            0,
        );
        assert!(approx(src.position.x, 0.0) && approx(src.position.y, 0.0));
        assert!(approx(src.size.x, 48.0) && approx(src.size.y, 24.0));
    }

    #[test]
    fn animation_advances_at_fps() {
        let mut frames = SpriteFrames::new();
        frames.add_animation("walk", 4, 10.0, false); // 10 fps -> 0.1s/frame
        let mut pb = AnimatedSpritePlayback::new("walk");
        // 0.25s at 10fps = 2.5 frames -> frame index 2.
        let finished = pb.advance(0.25, &frames);
        assert_eq!(pb.frame, 2);
        assert!(!finished);
    }

    #[test]
    fn non_looping_animation_finishes_once() {
        let mut frames = SpriteFrames::new();
        frames.add_animation("attack", 3, 10.0, false);
        let mut pb = AnimatedSpritePlayback::new("attack");
        // 1.0s is well past the 3 frames -> should finish, resting on last frame.
        let finished = pb.advance(1.0, &frames);
        assert!(finished, "animation_finished should fire");
        assert_eq!(pb.frame, 2, "rests on last frame");
        assert!(!pb.playing);
        assert!(pb.finished);
        // A further advance neither moves nor re-fires.
        let again = pb.advance(1.0, &frames);
        assert!(!again);
        assert_eq!(pb.frame, 2);
    }

    #[test]
    fn looping_animation_wraps_and_never_finishes() {
        let mut frames = SpriteFrames::new();
        frames.add_animation("idle", 4, 10.0, true);
        let mut pb = AnimatedSpritePlayback::new("idle");
        // 0.5s = 5 frames at 10fps; wraps 4->0->1.
        let finished = pb.advance(0.5, &frames);
        assert!(!finished);
        assert!(pb.playing, "looping animation keeps playing");
        assert_eq!(pb.frame, 1);
    }

    #[test]
    fn missing_or_zero_fps_animation_is_noop() {
        let frames = SpriteFrames::new();
        let mut pb = AnimatedSpritePlayback::new("absent");
        assert!(!pb.advance(1.0, &frames));
        assert_eq!(pb.frame, 0);
    }
}
