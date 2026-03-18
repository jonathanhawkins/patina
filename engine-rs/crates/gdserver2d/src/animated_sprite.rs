//! Animated sprite system for sprite sheet animations.
//!
//! Provides [`SpriteFrames`] for defining named animations as sequences of
//! [`SpriteFrame`] entries, and [`AnimatedSprite`] for playing them back.

use std::collections::HashMap;

use gdcore::math::Rect2;

/// A single frame in a sprite animation.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteFrame {
    /// Path to the texture resource.
    pub texture_path: String,
    /// Sub-region of the texture to display (for atlas/sprite sheet).
    pub region: Rect2,
    /// Duration of this frame in seconds.
    pub duration: f32,
}

/// A collection of named sprite animations, matching Godot's `SpriteFrames`.
#[derive(Debug, Clone, Default)]
pub struct SpriteFrames {
    /// Named animations, each containing a sequence of frames.
    pub animations: HashMap<String, Vec<SpriteFrame>>,
}

impl SpriteFrames {
    /// Creates an empty sprite frames collection.
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
        }
    }

    /// Adds an animation with the given name and frames.
    pub fn add_animation(&mut self, name: &str, frames: Vec<SpriteFrame>) {
        self.animations.insert(name.to_string(), frames);
    }

    /// Returns the frames for the given animation name.
    pub fn get_animation(&self, name: &str) -> Option<&[SpriteFrame]> {
        self.animations.get(name).map(|v| v.as_slice())
    }

    /// Returns `true` if the given animation exists.
    pub fn has_animation(&self, name: &str) -> bool {
        self.animations.contains_key(name)
    }
}

/// Playback controller for sprite animations.
#[derive(Debug, Clone)]
pub struct AnimatedSprite {
    /// The sprite frames resource.
    frames: SpriteFrames,
    /// Currently playing animation name.
    current_animation: String,
    /// Current frame index within the animation.
    current_frame: usize,
    /// Elapsed time within the current frame.
    elapsed: f32,
    /// Whether the animation is currently playing.
    playing: bool,
}

impl AnimatedSprite {
    /// Creates a new animated sprite with the given frames resource.
    pub fn new(frames: SpriteFrames) -> Self {
        Self {
            frames,
            current_animation: String::new(),
            current_frame: 0,
            elapsed: 0.0,
            playing: false,
        }
    }

    /// Starts playing the named animation from the beginning.
    pub fn play(&mut self, anim_name: &str) {
        if self.frames.has_animation(anim_name) {
            self.current_animation = anim_name.to_string();
            self.current_frame = 0;
            self.elapsed = 0.0;
            self.playing = true;
        }
    }

    /// Stops playback.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Returns the current frame index.
    pub fn get_frame(&self) -> usize {
        self.current_frame
    }

    /// Returns the current animation name.
    pub fn get_animation(&self) -> &str {
        &self.current_animation
    }

    /// Returns `true` if the animation is currently playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Returns the current [`SpriteFrame`] data, if an animation is active.
    pub fn get_current_sprite_frame(&self) -> Option<&SpriteFrame> {
        self.frames
            .get_animation(&self.current_animation)
            .and_then(|frames| frames.get(self.current_frame))
    }

    /// Advances the animation by `delta` seconds, cycling through frames.
    pub fn advance(&mut self, delta: f32) {
        if !self.playing {
            return;
        }

        let frames = match self.frames.get_animation(&self.current_animation) {
            Some(f) if !f.is_empty() => f,
            _ => return,
        };

        self.elapsed += delta;

        while self.elapsed >= frames[self.current_frame].duration {
            self.elapsed -= frames[self.current_frame].duration;
            self.current_frame += 1;
            if self.current_frame >= frames.len() {
                self.current_frame = 0; // Loop
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;

    fn make_test_frames() -> SpriteFrames {
        let mut sf = SpriteFrames::new();
        sf.add_animation(
            "walk",
            vec![
                SpriteFrame {
                    texture_path: "spritesheet.png".to_string(),
                    region: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(32.0, 32.0)),
                    duration: 0.1,
                },
                SpriteFrame {
                    texture_path: "spritesheet.png".to_string(),
                    region: Rect2::new(Vector2::new(32.0, 0.0), Vector2::new(32.0, 32.0)),
                    duration: 0.1,
                },
                SpriteFrame {
                    texture_path: "spritesheet.png".to_string(),
                    region: Rect2::new(Vector2::new(64.0, 0.0), Vector2::new(32.0, 32.0)),
                    duration: 0.2,
                },
            ],
        );
        sf.add_animation(
            "idle",
            vec![SpriteFrame {
                texture_path: "spritesheet.png".to_string(),
                region: Rect2::new(Vector2::new(0.0, 32.0), Vector2::new(32.0, 32.0)),
                duration: 0.5,
            }],
        );
        sf
    }

    #[test]
    fn sprite_frames_add_and_get() {
        let sf = make_test_frames();
        assert!(sf.has_animation("walk"));
        assert!(sf.has_animation("idle"));
        assert!(!sf.has_animation("run"));
        assert_eq!(sf.get_animation("walk").unwrap().len(), 3);
    }

    #[test]
    fn animated_sprite_play_and_stop() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        assert!(!sprite.is_playing());

        sprite.play("walk");
        assert!(sprite.is_playing());
        assert_eq!(sprite.get_animation(), "walk");
        assert_eq!(sprite.get_frame(), 0);

        sprite.stop();
        assert!(!sprite.is_playing());
    }

    #[test]
    fn animated_sprite_advance_single_frame() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("walk");

        // Advance 0.05s — still in frame 0 (duration 0.1s).
        sprite.advance(0.05);
        assert_eq!(sprite.get_frame(), 0);
    }

    #[test]
    fn animated_sprite_advance_to_next_frame() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("walk");

        // Advance past frame 0 (0.1s).
        sprite.advance(0.15);
        assert_eq!(sprite.get_frame(), 1);
    }

    #[test]
    fn animated_sprite_loops() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("walk");

        // Total walk duration: 0.1 + 0.1 + 0.2 = 0.4s. Advance 0.45s → back to frame 0.
        sprite.advance(0.45);
        assert_eq!(sprite.get_frame(), 0);
        assert!(sprite.is_playing());
    }

    #[test]
    fn animated_sprite_play_nonexistent_does_nothing() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("nonexistent");
        assert!(!sprite.is_playing());
    }

    #[test]
    fn animated_sprite_get_current_sprite_frame() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("walk");
        let frame = sprite.get_current_sprite_frame().unwrap();
        assert_eq!(frame.texture_path, "spritesheet.png");
        assert_eq!(frame.region.position, Vector2::new(0.0, 0.0));
    }

    #[test]
    fn animated_sprite_advance_when_stopped_does_nothing() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("walk");
        sprite.stop();
        sprite.advance(1.0);
        assert_eq!(sprite.get_frame(), 0);
    }

    #[test]
    fn animated_sprite_switch_animation() {
        let mut sprite = AnimatedSprite::new(make_test_frames());
        sprite.play("walk");
        sprite.advance(0.15);
        assert_eq!(sprite.get_frame(), 1);

        sprite.play("idle");
        assert_eq!(sprite.get_frame(), 0);
        assert_eq!(sprite.get_animation(), "idle");
    }
}
