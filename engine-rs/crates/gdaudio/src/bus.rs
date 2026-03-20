//! Audio bus routing and configuration.

/// An audio bus carries mixed audio and applies volume/mute/solo controls.
///
/// Buses are organized in an [`super::mixer::AudioMixer`] and referenced by
/// [`super::stream::AudioStreamPlayback`] instances.
#[derive(Debug, Clone)]
pub struct AudioBus {
    /// Human-readable name of this bus (e.g. "Master", "SFX", "Music").
    name: String,
    /// Volume in decibels. 0.0 dB is unity gain.
    volume_db: f32,
    /// When true, this bus produces no output.
    mute: bool,
    /// When true, only solo buses are audible.
    solo: bool,
}

impl AudioBus {
    /// Create a new audio bus with the given name at unity gain (0.0 dB).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume_db: 0.0,
            mute: false,
            solo: false,
        }
    }

    /// Return the bus name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the current volume in decibels.
    pub fn volume_db(&self) -> f32 {
        self.volume_db
    }

    /// Convert the dB volume to a linear multiplier.
    ///
    /// The formula is `10^(dB / 20)`, so 0 dB maps to 1.0,
    /// -20 dB maps to 0.1, +20 dB maps to 10.0, etc.
    pub fn volume_linear(&self) -> f32 {
        Self::db_to_linear(self.volume_db)
    }

    /// Convert a dB value to a linear multiplier (static utility).
    pub fn db_to_linear(db: f32) -> f32 {
        f32::powf(10.0, db / 20.0)
    }

    /// Set the volume in decibels.
    pub fn set_volume_db(&mut self, db: f32) {
        self.volume_db = db;
    }

    /// Return whether this bus is muted.
    pub fn is_mute(&self) -> bool {
        self.mute
    }

    /// Set the mute flag.
    pub fn set_mute(&mut self, mute: bool) {
        self.mute = mute;
    }

    /// Return whether this bus is soloed.
    pub fn is_solo(&self) -> bool {
        self.solo
    }

    /// Set the solo flag.
    pub fn set_solo(&mut self, solo: bool) {
        self.solo = solo;
    }
}
