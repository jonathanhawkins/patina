//! Bus audio effects chain (pat-p0s82.2).
//!
//! Implements a bounded Godot `AudioEffect` slice that can be inserted on an
//! [`AudioBus`](crate::bus::AudioBus):
//!
//! - [`AudioEffectAmplify`] — gain in dB
//! - [`AudioEffectReverb`] — simple feedback delay network
//! - [`AudioEffectEQ`] — 3-band peaking EQ (low / mid / high)
//!
//! Effects process interleaved f32 PCM in-place via [`AudioEffect::process`].

/// Trait implemented by every bus insert effect.
pub trait AudioEffect: Send {
    /// Human-readable effect type name (e.g. `"Amplify"`).
    fn name(&self) -> &str;
    /// Process interleaved samples at `sample_rate` / `channels` in-place.
    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16);
    /// Reset internal state (delay lines, filters).
    fn reset(&mut self);
}

/// Amplify effect — multiplies the signal by a linear gain derived from dB.
#[derive(Debug, Clone)]
pub struct AudioEffectAmplify {
    /// Volume offset in decibels (0 = unity).
    pub volume_db: f32,
}

impl AudioEffectAmplify {
    /// Create an amplify effect at the given dB.
    pub fn new(volume_db: f32) -> Self {
        Self { volume_db }
    }

    /// Linear gain for the current volume_db.
    pub fn linear_gain(&self) -> f32 {
        10.0f32.powf(self.volume_db / 20.0)
    }
}

impl AudioEffect for AudioEffectAmplify {
    fn name(&self) -> &str {
        "Amplify"
    }

    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, _channels: u16) {
        let g = self.linear_gain();
        for s in samples.iter_mut() {
            *s *= g;
        }
    }

    fn reset(&mut self) {}
}

/// Simple reverb via a multi-tap feedback delay.
#[derive(Debug, Clone)]
pub struct AudioEffectReverb {
    /// Wet mix amount in \[0, 1\].
    pub wet: f32,
    /// Dry mix amount in \[0, 1\].
    pub dry: f32,
    /// Feedback (room size proxy) in \[0, 0.95\].
    pub feedback: f32,
    /// Delay time in seconds.
    pub delay_secs: f32,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    write_pos: usize,
}

impl AudioEffectReverb {
    /// Create a reverb with default roomy settings.
    pub fn new() -> Self {
        Self {
            wet: 0.35,
            dry: 0.7,
            feedback: 0.5,
            delay_secs: 0.04,
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            write_pos: 0,
        }
    }

    fn ensure_buffers(&mut self, sample_rate: u32) {
        let len = ((self.delay_secs * sample_rate as f32) as usize).max(1);
        if self.delay_l.len() != len {
            self.delay_l = vec![0.0; len];
            self.delay_r = vec![0.0; len];
            self.write_pos = 0;
        }
    }
}

impl Default for AudioEffectReverb {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for AudioEffectReverb {
    fn name(&self) -> &str {
        "Reverb"
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        self.ensure_buffers(sample_rate);
        let len = self.delay_l.len();
        let fb = self.feedback.clamp(0.0, 0.95);
        let wet = self.wet.clamp(0.0, 1.0);
        let dry = self.dry.clamp(0.0, 1.0);
        let ch = channels.max(1) as usize;

        if ch == 1 {
            for s in samples.iter_mut() {
                let delayed = self.delay_l[self.write_pos];
                let input = *s;
                self.delay_l[self.write_pos] = input + delayed * fb;
                *s = input * dry + delayed * wet;
                self.write_pos = (self.write_pos + 1) % len;
            }
        } else {
            // Stereo interleaved
            let frames = samples.len() / ch;
            for f in 0..frames {
                let base = f * ch;
                let delayed_l = self.delay_l[self.write_pos];
                let delayed_r = self.delay_r[self.write_pos];
                let in_l = samples[base];
                let in_r = samples[base + 1];
                self.delay_l[self.write_pos] = in_l + delayed_l * fb;
                self.delay_r[self.write_pos] = in_r + delayed_r * fb;
                samples[base] = in_l * dry + delayed_l * wet;
                samples[base + 1] = in_r * dry + delayed_r * wet;
                self.write_pos = (self.write_pos + 1) % len;
            }
        }
    }

    fn reset(&mut self) {
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.write_pos = 0;
    }
}

/// 3-band EQ (low shelf-ish, mid peak, high shelf-ish) via one-pole / biquad-lite.
#[derive(Debug, Clone)]
pub struct AudioEffectEQ {
    /// Low band gain in dB (centred ~200 Hz).
    pub low_db: f32,
    /// Mid band gain in dB (centred ~1 kHz).
    pub mid_db: f32,
    /// High band gain in dB (centred ~5 kHz).
    pub high_db: f32,
    /// Per-channel filter memory: [low_lp, mid_bp_z, high_hp_z] * channels
    state: Vec<[f32; 3]>,
}

impl AudioEffectEQ {
    /// Flat EQ (0 dB all bands).
    pub fn flat() -> Self {
        Self {
            low_db: 0.0,
            mid_db: 0.0,
            high_db: 0.0,
            state: Vec::new(),
        }
    }

    /// Create with explicit band gains.
    pub fn new(low_db: f32, mid_db: f32, high_db: f32) -> Self {
        Self {
            low_db,
            mid_db,
            high_db,
            state: Vec::new(),
        }
    }

    fn db_gain(db: f32) -> f32 {
        10.0f32.powf(db / 20.0)
    }
}

impl AudioEffect for AudioEffectEQ {
    fn name(&self) -> &str {
        "EQ"
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        let ch = channels.max(1) as usize;
        if self.state.len() != ch {
            self.state = vec![[0.0; 3]; ch];
        }
        let sr = sample_rate.max(1) as f32;
        // One-pole coefficients
        let low_cut = 200.0 / sr;
        let high_cut = 5000.0 / sr;
        let a_low = (-2.0 * std::f32::consts::PI * low_cut).exp();
        let a_high = (-2.0 * std::f32::consts::PI * high_cut).exp();

        let g_low = Self::db_gain(self.low_db);
        let g_mid = Self::db_gain(self.mid_db);
        let g_high = Self::db_gain(self.high_db);

        let frames = samples.len() / ch;
        for f in 0..frames {
            for c in 0..ch {
                let x = samples[f * ch + c];
                let st = &mut self.state[c];
                // low-pass for low band
                st[0] = st[0] + (1.0 - a_low) * (x - st[0]);
                let low = st[0];
                // high-pass for high band
                st[2] = a_high * (st[2] + x - st[1]);
                let high = st[2];
                st[1] = x;
                let mid = x - low - high;
                samples[f * ch + c] = low * g_low + mid * g_mid + high * g_high;
            }
        }
    }

    fn reset(&mut self) {
        for s in &mut self.state {
            *s = [0.0; 3];
        }
    }
}

/// Owned effect chain applied in insertion order.
#[derive(Default)]
pub struct EffectChain {
    effects: Vec<Box<dyn AudioEffect>>,
}

impl EffectChain {
    /// Empty chain.
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    /// Number of inserts.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Append an effect.
    pub fn add(&mut self, effect: Box<dyn AudioEffect>) {
        self.effects.push(effect);
    }

    /// Remove effect at index.
    pub fn remove(&mut self, index: usize) -> Option<Box<dyn AudioEffect>> {
        if index < self.effects.len() {
            Some(self.effects.remove(index))
        } else {
            None
        }
    }

    /// Process the buffer through every effect in order.
    pub fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        for e in &mut self.effects {
            e.process(samples, sample_rate, channels);
        }
    }

    /// Reset all effects.
    pub fn reset(&mut self) {
        for e in &mut self.effects {
            e.reset();
        }
    }

    /// Effect type names in order.
    pub fn names(&self) -> Vec<&str> {
        self.effects.iter().map(|e| e.name()).collect()
    }
}

/// Run a known impulse/sine through a chain — helper for tests.
pub fn process_buffer(
    chain: &mut EffectChain,
    samples: &mut [f32],
    sample_rate: u32,
    channels: u16,
) {
    chain.process(samples, sample_rate, channels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amplify_doubles_amplitude_at_plus_6db() {
        let mut amp = AudioEffectAmplify::new(6.0206); // ≈ 2x
        let mut buf = vec![0.25_f32, -0.25, 0.5, -0.5];
        amp.process(&mut buf, 44100, 1);
        assert!((buf[0] - 0.5).abs() < 0.01, "got {}", buf[0]);
        assert!((buf[2] - 1.0).abs() < 0.02, "got {}", buf[2]);
    }

    #[test]
    fn amplify_silence_stays_silent() {
        let mut amp = AudioEffectAmplify::new(12.0);
        let mut buf = vec![0.0_f32; 64];
        amp.process(&mut buf, 44100, 2);
        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn reverb_produces_tail_after_impulse() {
        let mut rev = AudioEffectReverb::new();
        rev.wet = 1.0;
        rev.dry = 0.0;
        rev.feedback = 0.6;
        rev.delay_secs = 0.01; // 441 samples at 44.1k
        // Impulse then silence
        let mut buf = vec![0.0_f32; 2000];
        buf[0] = 1.0;
        rev.process(&mut buf, 44100, 1);
        // After delay, wet path should show energy
        let energy: f32 = buf[100..].iter().map(|s| s * s).sum();
        assert!(
            energy > 0.01,
            "reverb must leave a tail after impulse, energy={energy}"
        );
        // Original sample position is dry-muted (wet only from delay)
        // First sample: dry=0 so output ≈ 0 from empty delay
        assert!(buf[0].abs() < 0.01);
    }

    #[test]
    fn eq_boost_low_increases_bass_sine() {
        // 100 Hz sine should gain with +12 dB low
        let sr = 44100u32;
        let frames = 2048;
        let mut flat = vec![0.0_f32; frames];
        let mut boosted = vec![0.0_f32; frames];
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * 100.0 * t).sin() * 0.5;
            flat[i] = s;
            boosted[i] = s;
        }
        let mut eq_flat = AudioEffectEQ::flat();
        let mut eq_boost = AudioEffectEQ::new(12.0, 0.0, 0.0);
        // Warm up filters
        eq_flat.process(&mut flat, sr, 1);
        eq_boost.process(&mut boosted, sr, 1);
        // Fresh buffers for measurement after warmup
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * 100.0 * t).sin() * 0.5;
            flat[i] = s;
            boosted[i] = s;
        }
        eq_flat.process(&mut flat, sr, 1);
        eq_boost.process(&mut boosted, sr, 1);
        let e_flat: f32 = flat.iter().map(|s| s * s).sum();
        let e_boost: f32 = boosted.iter().map(|s| s * s).sum();
        assert!(
            e_boost > e_flat * 1.5,
            "low boost should raise 100Hz energy: flat={e_flat} boost={e_boost}"
        );
    }

    #[test]
    fn effect_chain_order_amplify_then_reverb() {
        let mut chain = EffectChain::new();
        chain.add(Box::new(AudioEffectAmplify::new(6.0)));
        chain.add(Box::new(AudioEffectReverb::new()));
        assert_eq!(chain.names(), vec!["Amplify", "Reverb"]);
        let mut buf = vec![0.1_f32; 512];
        process_buffer(&mut chain, &mut buf, 44100, 1);
        // Amplify first → signal larger before reverb; output non-zero
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 0.0);
    }

    #[test]
    fn chain_processes_known_dc_impulse() {
        let mut chain = EffectChain::new();
        chain.add(Box::new(AudioEffectAmplify::new(0.0))); // unity
        chain.add(Box::new(AudioEffectEQ::flat()));
        let mut buf = vec![0.5_f32; 128];
        process_buffer(&mut chain, &mut buf, 44100, 1);
        // Flat EQ + unity amplify ≈ preserve DC (after filter settle)
        // Just assert not zeroed and not NaN
        assert!(buf.iter().all(|s| s.is_finite()));
        assert!(buf.iter().any(|s| s.abs() > 0.1));
    }
}
