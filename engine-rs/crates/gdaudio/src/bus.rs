//! Audio bus routing and configuration.

use crate::effects::EffectChain;

/// An audio bus carries mixed audio and applies volume/mute/solo controls.
///
/// Buses are organized in an [`super::mixer::AudioMixer`] and referenced by
/// [`super::stream::AudioStreamPlayback`] instances.
///
/// Each bus owns an ordered [`EffectChain`] (pat-p0s82.2) applied after volume
/// and before final mix-down.
pub struct AudioBus {
    /// Human-readable name of this bus (e.g. "Master", "SFX", "Music").
    name: String,
    /// Volume in decibels. 0.0 dB is unity gain.
    volume_db: f32,
    /// When true, this bus produces no output.
    mute: bool,
    /// When true, only solo buses are audible.
    solo: bool,
    /// Ordered effect inserts (Amplify / Reverb / EQ / …).
    effects: EffectChain,
    /// Bus send target name (Godot `bus/send`); empty = no send (pat-p0s82.12).
    send: String,
    /// Send level in dB (0 = full send).
    send_db: f32,
}

impl Clone for AudioBus {
    fn clone(&self) -> Self {
        // Effect chain is stateful and not Clone; start a fresh empty chain.
        Self {
            name: self.name.clone(),
            volume_db: self.volume_db,
            mute: self.mute,
            solo: self.solo,
            effects: EffectChain::new(),
            send: self.send.clone(),
            send_db: self.send_db,
        }
    }
}

impl std::fmt::Debug for AudioBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioBus")
            .field("name", &self.name)
            .field("volume_db", &self.volume_db)
            .field("mute", &self.mute)
            .field("solo", &self.solo)
            .field("effects", &self.effects.len())
            .finish()
    }
}

impl AudioBus {
    /// Create a new audio bus with the given name at unity gain (0.0 dB).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume_db: 0.0,
            mute: false,
            solo: false,
            effects: EffectChain::new(),
            send: String::new(),
            send_db: 0.0,
        }
    }

    /// Bus send target (empty = none).
    pub fn send(&self) -> &str {
        &self.send
    }

    /// Set bus send target by name (e.g. `"Master"`).
    pub fn set_send(&mut self, bus: impl Into<String>) {
        self.send = bus.into();
    }

    /// Send level in dB.
    pub fn send_db(&self) -> f32 {
        self.send_db
    }

    /// Set send level in dB.
    pub fn set_send_db(&mut self, db: f32) {
        self.send_db = db;
    }

    /// Linear send gain.
    pub fn send_linear(&self) -> f32 {
        Self::db_to_linear(self.send_db)
    }

    /// Immutable access to the effect chain.
    pub fn effects(&self) -> &EffectChain {
        &self.effects
    }

    /// Mutable access to the effect chain (add Amplify / Reverb / EQ inserts).
    pub fn effects_mut(&mut self) -> &mut EffectChain {
        &mut self.effects
    }

    /// Run the bus effect chain on interleaved samples.
    pub fn process_effects(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        self.effects.process(samples, sample_rate, channels);
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

#[cfg(test)]
mod send_tests {
    use super::*;

    #[test]
    fn bus_send_defaults_empty() {
        let b = AudioBus::new("SFX");
        assert!(b.send().is_empty());
        assert!((b.send_linear() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn bus_send_to_master() {
        let mut b = AudioBus::new("Music");
        b.set_send("Master");
        b.set_send_db(-6.0);
        assert_eq!(b.send(), "Master");
        assert!(b.send_linear() < 0.6 && b.send_linear() > 0.4);
    }

    // pat-nzgdu.27: DSP effects (from pat-u3l8c.107) reach the mixer through a
    // bus's EffectChain — a low-pass then a compressor on one bus.
    #[test]
    fn audio_effects_dsp_chain() {
        use crate::effects::{AudioEffectCompressor, AudioEffectFilter};
        let sr = 48000u32;
        let sine = |f: f32, amp: f32, n: usize| {
            (0..n)
                .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / sr as f32).sin() * amp)
                .collect::<Vec<f32>>()
        };

        // A bus carrying a low-pass filter then a compressor.
        let mut bus = AudioBus::new("SFX");
        bus.effects_mut()
            .add(Box::new(AudioEffectFilter::low_pass(1000.0, 0.707)));
        bus.effects_mut()
            .add(Box::new(AudioEffectCompressor::new(-24.0, 8.0, 2.0, 50.0)));
        assert_eq!(bus.effects().names(), vec!["Filter", "Compressor"]);

        // (1) Frequency-dependent attenuation: quiet signals keep the compressor
        // inert (below threshold), so only the low-pass shapes them — a high sine
        // is attenuated far more than a low one.
        let mut low = sine(150.0, 0.03, 4096);
        bus.process_effects(&mut low, sr, 1);
        bus.effects_mut().reset();
        let mut high = sine(11000.0, 0.03, 4096);
        bus.process_effects(&mut high, sr, 1);
        let e_low: f32 = low.iter().map(|s| s * s).sum();
        let e_high: f32 = high.iter().map(|s| s * s).sum();
        assert!(
            e_low > e_high * 4.0,
            "low-pass bus must attenuate highs: low={e_low} high={e_high}"
        );

        // (2) Gain reduction: a loud in-band signal is compressed above threshold.
        bus.effects_mut().reset();
        let mut loud = sine(200.0, 0.9, 8000);
        let in_peak = loud.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        bus.process_effects(&mut loud, sr, 1);
        let out_peak = loud[4000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            out_peak < in_peak * 0.7,
            "compressor must reduce loud signal: in={in_peak} out={out_peak}"
        );
    }
}
