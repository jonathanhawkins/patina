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
    /// Pitch multiplier applied during mixing (Godot default 1.0). Values > 1.0
    /// speed the stream up (higher pitch), < 1.0 slow it down (lower pitch).
    pitch_scale: f32,
    /// Per-channel gain for stereo panning: left channel gain. Default 1.0.
    /// Positional players (2D/3D) push equal-power pan gains here so the mixer
    /// spatializes the stream.
    pan_left: f32,
    /// Per-channel gain for stereo panning: right channel gain. Default 1.0.
    pan_right: f32,
    /// Extra linear gain multiplier applied on top of `volume_db`. Default 1.0.
    /// Positional players push their computed distance-attenuated linear volume
    /// here so spatial attenuation multiplies the mix without disturbing the
    /// user-facing `volume_db`.
    linear_gain: f32,
    /// Loop start in seconds (Godot `loop_begin`); a forward loop wraps here
    /// when playback reaches `loop_end`. Default 0.0.
    loop_begin_secs: f32,
    /// Loop end in seconds (Godot `loop_end`); 0.0 means "use `length_secs`".
    /// A forward loop wraps back to `loop_begin_secs` upon reaching this point.
    loop_end_secs: f32,
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
            pitch_scale: 1.0,
            pan_left: 1.0,
            pan_right: 1.0,
            linear_gain: 1.0,
            loop_begin_secs: 0.0,
            loop_end_secs: 0.0,
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
    /// - [`LoopMode::None`]: playback stops when it reaches `length_secs`.
    /// - [`LoopMode::Forward`]: playback wraps at `loop_end` (which defaults to
    ///   `length_secs` when `loop_end_secs` is 0.0) back to `loop_begin`,
    ///   preserving overshoot so it keeps producing audio past its nominal
    ///   length.
    pub fn advance(&mut self, delta: f32) {
        if self.state != PlaybackState::Playing {
            return;
        }

        self.position_secs += delta;

        match self.loop_mode {
            LoopMode::None => {
                if self.position_secs >= self.length_secs {
                    self.stop();
                }
            }
            LoopMode::Forward => {
                let loop_end = if self.loop_end_secs > 0.0
                    && self.loop_end_secs <= self.length_secs
                {
                    self.loop_end_secs
                } else {
                    self.length_secs
                };
                let loop_begin = self.loop_begin_secs.clamp(0.0, loop_end);
                if self.position_secs >= loop_end {
                    let span = (loop_end - loop_begin).max(f32::EPSILON);
                    self.position_secs =
                        loop_begin + (self.position_secs - loop_begin).rem_euclid(span);
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

    /// Return the pitch scale multiplier.
    pub fn pitch_scale(&self) -> f32 {
        self.pitch_scale
    }

    /// Set the pitch scale multiplier. Clamped to a small positive minimum so a
    /// zero/negative scale can never stall or reverse playback.
    pub fn set_pitch_scale(&mut self, scale: f32) {
        self.pitch_scale = scale.max(0.01);
    }

    /// Left/right stereo pan gains applied per output channel during mixing.
    pub fn pan(&self) -> (f32, f32) {
        (self.pan_left, self.pan_right)
    }

    /// Set the left/right stereo pan gains (clamped to `[0, 1]`). Positional
    /// players push equal-power pan here from a spatial calculation.
    pub fn set_pan(&mut self, left: f32, right: f32) {
        self.pan_left = left.clamp(0.0, 1.0);
        self.pan_right = right.clamp(0.0, 1.0);
    }

    /// Extra linear gain applied on top of `volume_db` during mixing.
    pub fn linear_gain(&self) -> f32 {
        self.linear_gain
    }

    /// Set the extra linear gain (clamped non-negative). Positional players push
    /// their distance-attenuated linear volume here.
    pub fn set_linear_gain(&mut self, gain: f32) {
        self.linear_gain = gain.max(0.0);
    }

    /// Return the loop mode.
    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// Set the loop mode.
    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    /// Loop start in seconds (Godot `loop_begin`).
    pub fn loop_begin(&self) -> f32 {
        self.loop_begin_secs
    }

    /// Set the loop start in seconds, clamped to `[0, length]`.
    pub fn set_loop_begin(&mut self, secs: f32) {
        self.loop_begin_secs = secs.clamp(0.0, self.length_secs);
    }

    /// Loop end in seconds (Godot `loop_end`); 0.0 means "use `length_secs`".
    pub fn loop_end(&self) -> f32 {
        self.loop_end_secs
    }

    /// Set the loop end in seconds, clamped to `[0, length]`; 0.0 disables it
    /// (falls back to the full stream length).
    pub fn set_loop_end(&mut self, secs: f32) {
        self.loop_end_secs = secs.clamp(0.0, self.length_secs);
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
