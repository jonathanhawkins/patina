//! Animation **playback controls** (pat-8xss8).
//!
//! Drives a running animation: play / pause / stop transitions, a speed scalar
//! that scales the playback rate, looping (wrap vs. stop at the end), and an
//! autoplay flag that marks an animation to start on scene load. `advance(dt)`
//! steps the playhead while playing.

/// The transport state of playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    /// Not playing; the playhead is at the start.
    Stopped,
    /// Actively playing.
    Playing,
    /// Paused at the current position.
    Paused,
}

/// Animation playback transport.
#[derive(Debug, Clone)]
pub struct Playback {
    length: f32,
    time: f32,
    speed: f32,
    looping: bool,
    state: PlayState,
    autoplay: Option<String>,
}

impl Playback {
    /// Creates stopped playback for an animation of `length` seconds.
    pub fn new(length: f32) -> Self {
        Self {
            length: length.max(0.0),
            time: 0.0,
            speed: 1.0,
            looping: false,
            state: PlayState::Stopped,
            autoplay: None,
        }
    }

    /// The current playhead time.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// The playback state.
    pub fn state(&self) -> PlayState {
        self.state
    }

    /// Whether playing.
    pub fn is_playing(&self) -> bool {
        self.state == PlayState::Playing
    }

    /// Whether paused.
    pub fn is_paused(&self) -> bool {
        self.state == PlayState::Paused
    }

    /// Whether stopped.
    pub fn is_stopped(&self) -> bool {
        self.state == PlayState::Stopped
    }

    /// Starts (or resumes) playback from the current position.
    pub fn play(&mut self) {
        self.state = PlayState::Playing;
    }

    /// Pauses playback, keeping the current position.
    pub fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Paused;
        }
    }

    /// Stops playback and rewinds to the start.
    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
        self.time = 0.0;
    }

    /// The speed scalar.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Sets the speed scalar (multiplies the playback rate).
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// Whether looping is enabled.
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// Enables or disables looping.
    pub fn set_loop(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// The animation marked for autoplay-on-load, if any.
    pub fn autoplay(&self) -> Option<&str> {
        self.autoplay.as_deref()
    }

    /// Marks `name` to autoplay when the scene loads.
    pub fn set_autoplay(&mut self, name: impl Into<String>) {
        self.autoplay = Some(name.into());
    }

    /// Clears the autoplay flag.
    pub fn clear_autoplay(&mut self) {
        self.autoplay = None;
    }

    /// Advances the playhead by `dt` seconds (scaled by speed) while playing.
    /// At the end it wraps when looping, otherwise clamps and stops.
    pub fn advance(&mut self, dt: f32) {
        if self.state != PlayState::Playing {
            return;
        }
        let mut t = self.time + dt * self.speed;
        if t < 0.0 || t >= self.length {
            if self.looping && self.length > 0.0 {
                t = t.rem_euclid(self.length);
            } else {
                t = t.clamp(0.0, self.length);
                self.state = PlayState::Stopped;
            }
        }
        self.time = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-8xss8): play/pause/stop control the running animation,
    /// the speed scalar adjusts playback rate, and the autoplay flag marks the
    /// animation to start on scene load.
    #[test]
    fn anim_playback_controls() {
        let mut pb = Playback::new(2.0);
        assert!(pb.is_stopped());

        // Play and advance.
        pb.play();
        assert!(pb.is_playing());
        pb.advance(0.5);
        assert_eq!(pb.time(), 0.5);

        // The speed scalar scales the rate.
        pb.set_speed(2.0);
        pb.advance(0.5); // +0.5 * 2.0 = 1.0
        assert_eq!(pb.time(), 1.5);

        // Pause freezes the playhead.
        pb.pause();
        assert!(pb.is_paused());
        pb.advance(1.0);
        assert_eq!(pb.time(), 1.5);

        // Resuming and running past the end (no loop) clamps and stops.
        pb.play();
        pb.advance(1.0); // 1.5 + 2.0 = 3.5 → clamp to 2.0, stop
        assert_eq!(pb.time(), 2.0);
        assert!(pb.is_stopped());

        // Stop rewinds to the start.
        pb.play();
        pb.advance(0.3);
        pb.stop();
        assert!(pb.is_stopped());
        assert_eq!(pb.time(), 0.0);

        // Looping wraps and keeps playing.
        pb.set_loop(true);
        pb.set_speed(1.0);
        pb.play();
        pb.advance(2.5); // 2.5 mod 2.0 = 0.5
        assert_eq!(pb.time(), 0.5);
        assert!(pb.is_playing());

        // Autoplay-on-load flag.
        assert!(pb.autoplay().is_none());
        pb.set_autoplay("idle");
        assert_eq!(pb.autoplay(), Some("idle"));
        pb.clear_autoplay();
        assert!(pb.autoplay().is_none());
    }
}
