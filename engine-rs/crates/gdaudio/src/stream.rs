//! Audio stream types and playback control.

/// Current state of an [`AudioStreamPlayback`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Not playing; position is at 0.
    Stopped,
    /// Actively producing audio.
    Playing,
    /// Suspended; resumes from current position.
    Paused,
}

/// How the stream behaves when it reaches the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// Stop when the end is reached.
    None,
    /// Wrap back to the beginning and continue playing.
    Forward,
}

/// Controls playback of a single audio stream.
///
/// Playback is driven externally by calling [`advance`](Self::advance) each
/// frame. The stream references an [`super::bus::AudioBus`] by name for
/// routing through the mixer.
#[derive(Debug, Clone)]
pub struct AudioStreamPlayback {
    state: PlaybackState,
    volume_db: f32,
    loop_mode: LoopMode,
    position_secs: f32,
    length_secs: f32,
    bus_name: String,
}

impl AudioStreamPlayback {
    /// Create a new playback instance with the given stream length.
    pub fn new(length_secs: f32) -> Self {
        Self {
            state: PlaybackState::Stopped,
            volume_db: 0.0,
            loop_mode: LoopMode::None,
            position_secs: 0.0,
            length_secs,
            bus_name: "Master".to_string(),
        }
    }

    /// Start or resume playback.
    pub fn play(&mut self) {
        self.state = PlaybackState::Playing;
    }

    /// Pause playback at the current position.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
    }

    /// Stop playback and reset position to 0.
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position_secs = 0.0;
    }

    /// Seek to an absolute position in seconds, clamped to `[0, length]`.
    pub fn seek(&mut self, position: f32) {
        self.position_secs = position.clamp(0.0, self.length_secs);
    }

    /// Return `true` if the stream is currently playing.
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    /// Return the current playback position in seconds.
    pub fn get_playback_position(&self) -> f32 {
        self.position_secs
    }

    /// Return the current playback state.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Advance playback by `delta` seconds.
    ///
    /// If the stream reaches the end:
    /// - [`LoopMode::None`]: playback stops and position resets to 0.
    /// - [`LoopMode::Forward`]: position wraps around modulo `length_secs`.
    pub fn advance(&mut self, delta: f32) {
        if self.state != PlaybackState::Playing {
            return;
        }

        self.position_secs += delta;

        if self.position_secs >= self.length_secs {
            match self.loop_mode {
                LoopMode::None => {
                    self.stop();
                }
                LoopMode::Forward => {
                    self.position_secs %= self.length_secs;
                }
            }
        }
    }

    /// Return the volume in decibels.
    pub fn volume_db(&self) -> f32 {
        self.volume_db
    }

    /// Set the volume in decibels.
    pub fn set_volume_db(&mut self, db: f32) {
        self.volume_db = db;
    }

    /// Return the loop mode.
    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// Set the loop mode.
    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    /// Set the target bus name for routing.
    pub fn set_bus(&mut self, name: impl Into<String>) {
        self.bus_name = name.into();
    }

    /// Return the target bus name.
    pub fn get_bus(&self) -> &str {
        &self.bus_name
    }

    /// Return the stream length in seconds.
    pub fn length_secs(&self) -> f32 {
        self.length_secs
    }
}
