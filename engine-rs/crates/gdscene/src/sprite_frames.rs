//! Sprite sheet regions + `SpriteFrames` animation playback (pat-u3l8c.26).
//!
//! `region_enabled`/`region_rect`/`hframes`/`vframes` previously existed only as
//! inspector schema and were ignored by the renderer, and there was no
//! `SpriteFrames` resource or frame advance anywhere. This module provides:
//!
//! * [`sprite_source_rect`] — the texture sub-rectangle a `Sprite2D` should draw
//!   given its region / sheet-frame settings.
//! * [`SpriteFrames`] + [`AnimatedSpritePlayback`] — an `AnimatedSprite2D`
//!   animation set and a playback state machine that advances at the animation
//!   FPS and signals `animation_finished`.

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

/// A single named animation within a [`SpriteFrames`] resource.
#[derive(Debug, Clone)]
pub struct SpriteAnimation {
    /// Number of frames in the animation.
    pub frame_count: usize,
    /// Playback rate in frames per second.
    pub speed: f32,
    /// Whether the animation loops back to frame 0 at the end.
    pub loops: bool,
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

    /// Adds (or replaces) an animation.
    pub fn add_animation(&mut self, name: &str, frame_count: usize, speed: f32, loops: bool) {
        self.animations.insert(
            name.to_string(),
            SpriteAnimation {
                frame_count,
                speed,
                loops,
            },
        );
    }

    /// Returns the animation, if present.
    pub fn animation(&self, name: &str) -> Option<&SpriteAnimation> {
        self.animations.get(name)
    }

    /// Number of frames in `name` (0 if the animation is absent).
    pub fn frame_count(&self, name: &str) -> usize {
        self.animations.get(name).map(|a| a.frame_count).unwrap_or(0)
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

/// Playback state for an `AnimatedSprite2D`.
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
    pub fn advance(&mut self, dt: f32, frames: &SpriteFrames) -> bool {
        if !self.playing {
            return false;
        }
        let count = frames.frame_count(&self.animation);
        let fps = frames.speed(&self.animation);
        if count == 0 || fps <= 0.0 {
            return false;
        }
        let loops = frames.loops(&self.animation);
        let interval = 1.0 / fps;
        self.time += dt * self.speed_scale;
        let mut just_finished = false;
        while self.time >= interval {
            self.time -= interval;
            if self.frame + 1 < count {
                self.frame += 1;
            } else if loops {
                self.frame = 0;
            } else {
                // Rest on the last frame and fire animation_finished once.
                self.playing = false;
                if !self.finished {
                    self.finished = true;
                    just_finished = true;
                }
                break;
            }
        }
        just_finished
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
        assert!(approx(src.position.x, 10.0) && approx(src.position.y, 10.0), "pos {:?}", src.position);
        assert!(approx(src.size.x, 10.0) && approx(src.size.y, 10.0), "size {:?}", src.size);
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
