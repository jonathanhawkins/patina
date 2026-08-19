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

    /// Peak magnitude in `[from_hz, to_hz]`, for effects that analyse spectrum
    /// (pat-0q2kx.160).
    ///
    /// This is Godot's
    /// `AudioEffectSpectrumAnalyzerInstance.get_magnitude_for_frequency_range`.
    /// Godot models it on a separate per-playback *instance* object; Patina
    /// keeps the analysis state on the effect itself, so the query rides on the
    /// effect trait instead. Defaulted to `None` so the analyzer is the only
    /// impl that answers and no other effect needs to change — the alternative,
    /// an `as_any` downcast hook, would have touched every effect in this file.
    fn magnitude_for_frequency_range(&self, _from_hz: f32, _to_hz: f32) -> Option<f32> {
        None
    }

    /// The bus this effect keys its sidechain from, if any (pat-s3rrv.53).
    ///
    /// Godot's `AudioEffectCompressor.sidechain` names another bus whose signal
    /// drives this compressor's envelope — how ducking is built. The name was
    /// parsed out of a bus layout and then **dropped**: nothing carried it, and
    /// `mixer.rs` had no reference to sidechain at all, so a layout that
    /// configured ducking produced a compressor keyed off its own input.
    ///
    /// Defaulted for the same reason `magnitude_for_frequency_range` is: the
    /// alternative is an `as_any` downcast hook that would touch every effect
    /// in this file.
    fn sidechain_bus(&self) -> Option<&str> {
        None
    }

    /// Hands this effect the sidechain key for the frame, one sample per frame
    /// (pat-s3rrv.53). No-op for effects that do not key.
    fn set_sidechain_key(&mut self, _key: Option<Vec<f32>>) {}

    /// The Godot class this effect was built from, e.g.
    /// `"AudioEffectLowPassFilter"` (pat-s3rrv.54).
    ///
    /// Distinct from [`name`](Self::name), which stays a short human label and
    /// is asserted against by existing tests and the editor. SEVEN Godot filter
    /// classes share one Rust `AudioEffectFilter`, so `name()` alone cannot say
    /// which slot of a bus holds which effect; this can.
    ///
    /// Defaults to `name()` so every existing effect keeps answering something
    /// sensible without being touched.
    fn godot_class(&self) -> &str {
        self.name()
    }
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

/// Godot's `AudioEffectDelay` — two independent taps plus a feedback line.
///
/// Mirrors `servers/audio/effects/audio_effect_delay.cpp`: each tap is an
/// independent echo at its own delay/level/pan, and the feedback line re-injects
/// its own output through a one-pole lowpass so repeats darken instead of
/// ringing forever. `dry` keeps the unprocessed signal.
#[derive(Debug, Clone)]
pub struct AudioEffectDelay {
    /// Dry (unprocessed) mix in \[0, 1\].
    pub dry: f32,
    /// Enable the first tap.
    pub tap1_active: bool,
    /// First tap delay in milliseconds.
    pub tap1_delay_ms: f32,
    /// First tap level in dB.
    pub tap1_level_db: f32,
    /// Enable the second tap.
    pub tap2_active: bool,
    /// Second tap delay in milliseconds.
    pub tap2_delay_ms: f32,
    /// Second tap level in dB.
    pub tap2_level_db: f32,
    /// Enable the feedback line.
    pub feedback_active: bool,
    /// Feedback delay in milliseconds.
    pub feedback_delay_ms: f32,
    /// Feedback level in dB.
    pub feedback_level_db: f32,
    /// One-pole lowpass cutoff applied to the feedback path, in Hz.
    pub feedback_lowpass: f32,
    buffer: Vec<f32>,
    write_pos: usize,
    lp_state: f32,
}

impl AudioEffectDelay {
    /// Godot's defaults.
    pub fn new() -> Self {
        Self {
            dry: 1.0,
            tap1_active: true,
            tap1_delay_ms: 250.0,
            tap1_level_db: -6.0,
            tap2_active: true,
            tap2_delay_ms: 500.0,
            tap2_level_db: -12.0,
            feedback_active: false,
            feedback_delay_ms: 340.0,
            feedback_level_db: -6.0,
            feedback_lowpass: 16000.0,
            buffer: Vec::new(),
            write_pos: 0,
            lp_state: 0.0,
        }
    }

    fn db_to_linear(db: f32) -> f32 {
        10.0f32.powf(db / 20.0)
    }

    /// Ring-buffer frames needed to cover the longest configured delay.
    fn required_frames(&self, sample_rate: u32) -> usize {
        let longest = self
            .tap1_delay_ms
            .max(self.tap2_delay_ms)
            .max(self.feedback_delay_ms)
            .max(1.0);
        ((longest / 1000.0) * sample_rate as f32).ceil() as usize + 1
    }

    /// Read the sample `delay_ms` in the past for `channel`, or 0 when that
    /// reaches past the start of the ring.
    fn tap(
        &self,
        delay_ms: f32,
        sample_rate: u32,
        frames: usize,
        channels: usize,
        channel: usize,
    ) -> f32 {
        let back = (((delay_ms / 1000.0) * sample_rate as f32).round() as usize).max(1);
        if back >= frames {
            return 0.0;
        }
        let idx = (self.write_pos + frames - back) % frames;
        self.buffer[idx * channels + channel]
    }
}

impl Default for AudioEffectDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for AudioEffectDelay {
    fn name(&self) -> &str {
        "Delay"
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        let ch = channels.max(1) as usize;
        let frames = self.required_frames(sample_rate);
        if self.buffer.len() != frames * ch {
            self.buffer = vec![0.0; frames * ch];
            self.write_pos = 0;
            self.lp_state = 0.0;
        }

        let tap1_gain = Self::db_to_linear(self.tap1_level_db);
        let tap2_gain = Self::db_to_linear(self.tap2_level_db);
        let fb_gain = Self::db_to_linear(self.feedback_level_db);
        // One-pole lowpass coefficient for the feedback path.
        let cutoff = self.feedback_lowpass.clamp(1.0, sample_rate as f32 * 0.5);
        let alpha = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / sample_rate as f32).exp();

        for frame in samples.chunks_mut(ch) {
            for (c, sample) in frame.iter_mut().enumerate() {
                let dry_in = *sample;

                let mut wet = 0.0;
                if self.tap1_active {
                    wet += self.tap(self.tap1_delay_ms, sample_rate, frames, ch, c) * tap1_gain;
                }
                if self.tap2_active {
                    wet += self.tap(self.tap2_delay_ms, sample_rate, frames, ch, c) * tap2_gain;
                }

                // Feedback is written back into the line, darkened each pass.
                let mut written = dry_in;
                if self.feedback_active {
                    let fb = self.tap(self.feedback_delay_ms, sample_rate, frames, ch, c) * fb_gain;
                    self.lp_state += alpha * (fb - self.lp_state);
                    written += self.lp_state;
                    wet += self.lp_state;
                }

                self.buffer[self.write_pos * ch + c] = written;
                *sample = dry_in * self.dry + wet;
            }
            self.write_pos = (self.write_pos + 1) % frames;
        }
    }

    fn reset(&mut self) {
        for s in self.buffer.iter_mut() {
            *s = 0.0;
        }
        self.write_pos = 0;
        self.lp_state = 0.0;
    }
}

/// Waveshaping mode for [`AudioEffectDistortion`], matching Godot's enum order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistortionMode {
    /// Hard clip at ±1 after drive.
    Clip,
    /// Soft saturation via `atan`.
    ATan,
    /// Bit/rate crush.
    LoFi,
    /// Asymmetric overdrive.
    Overdrive,
    /// `sin`-based wave shaping.
    WaveShape,
}

/// Godot's `AudioEffectDistortion` (`audio_effect_distortion.cpp`).
#[derive(Debug, Clone)]
pub struct AudioEffectDistortion {
    /// Shaping curve.
    pub mode: DistortionMode,
    /// Input gain in dB, applied before shaping.
    pub pre_gain_db: f32,
    /// Output gain in dB, applied after shaping.
    pub post_gain_db: f32,
    /// Drive amount in \[0, 1\] — how hard the signal is pushed into the curve.
    pub drive: f32,
}

impl AudioEffectDistortion {
    /// Godot's defaults (Clip, 0 dB pre, 0 dB post, drive 0).
    pub fn new() -> Self {
        Self {
            mode: DistortionMode::Clip,
            pre_gain_db: 0.0,
            post_gain_db: 0.0,
            drive: 0.0,
        }
    }

    /// Apply the shaping curve to one sample already scaled by drive.
    fn shape(&self, x: f32) -> f32 {
        match self.mode {
            DistortionMode::Clip => x.clamp(-1.0, 1.0),
            DistortionMode::ATan => x.atan() * std::f32::consts::FRAC_2_PI,
            // Quantise to a coarse step — the audible part of Godot's lofi.
            DistortionMode::LoFi => {
                let steps = 32.0;
                (x * steps).round() / steps
            }
            DistortionMode::Overdrive => {
                // Asymmetric: compress the positive half harder.
                if x >= 0.0 {
                    1.0 - (-x).exp()
                } else {
                    -(1.0 - x.exp())
                }
            }
            DistortionMode::WaveShape => (x * std::f32::consts::FRAC_PI_2).sin(),
        }
    }
}

impl Default for AudioEffectDistortion {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for AudioEffectDistortion {
    fn name(&self) -> &str {
        "Distortion"
    }

    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, _channels: u16) {
        let pre = 10.0f32.powf(self.pre_gain_db / 20.0);
        let post = 10.0f32.powf(self.post_gain_db / 20.0);
        // drive 0 still shapes (Godot clips at unity); higher drive pushes further in.
        let drive = 1.0 + self.drive.clamp(0.0, 1.0) * 30.0;
        for s in samples.iter_mut() {
            *s = self.shape(*s * pre * drive) * post;
        }
    }

    fn reset(&mut self) {}
}

/// One insert slot: the effect plus Godot's per-slot bypass flag.
///
/// Godot models a bus insert as an effect *and* an enabled bit
/// (`AudioServer.set_bus_effect_enabled`), so a designer can audition a chain
/// without destroying it. Patina stored a bare `Vec<Box<dyn AudioEffect>>`,
/// which left the bus-layout loader no choice but to DROP disabled effects on
/// load — losing them from the chain entirely (pat-0q2kx.158).
struct EffectSlot {
    effect: Box<dyn AudioEffect>,
    enabled: bool,
}

/// Owned effect chain applied in insertion order.
#[derive(Default)]
pub struct EffectChain {
    slots: Vec<EffectSlot>,
}

impl EffectChain {
    /// Empty chain.
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Number of inserts.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// The effect in slot `index`, or `None` when out of range
    /// (pat-0q2kx.160).
    ///
    /// Backs `AudioServer.get_bus_effect_instance`. Returns the effect whether
    /// or not the slot is bypassed: Godot's `get_bus_effect_instance` hands
    /// back the instance regardless of `set_bus_effect_enabled`, and a
    /// visualizer reading a bypassed analyzer should get its (silent) data, not
    /// a null.
    /// Hands the effect at `index` its sidechain key for the frame
    /// (pat-s3rrv.53).
    ///
    /// ⚠️ Deliberately does the work in here and returns NOTHING, rather than
    /// exposing an `effect_at_mut` twin of [`effect_at`](Self::effect_at). The
    /// box holds an **unsized** `dyn AudioEffect`, and handing a `&mut` to it
    /// back out through `Option::map` fights the borrow checker in a way the
    /// shared version does not — the immutable accessor's symmetry is
    /// misleading here. Nothing else needs mutable access to a single slot, so
    /// the narrow method is both simpler and the only one that is obviously
    /// sound.
    pub fn set_sidechain_key_at(&mut self, index: usize, key: Option<Vec<f32>>) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.effect.set_sidechain_key(key);
        }
    }

    pub fn effect_at(&self, index: usize) -> Option<&dyn AudioEffect> {
        self.slots.get(index).map(|s| s.effect.as_ref())
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Append an effect, enabled.
    pub fn add(&mut self, effect: Box<dyn AudioEffect>) {
        self.slots.push(EffectSlot {
            effect,
            enabled: true,
        });
    }

    /// Insert at `index`, mirroring `AudioServer.add_bus_effect(at_position)`.
    ///
    /// An out-of-range index appends, which is what Godot does for the default
    /// `at_position = -1`.
    pub fn insert(&mut self, index: usize, effect: Box<dyn AudioEffect>) {
        let at = index.min(self.slots.len());
        self.slots.insert(
            at,
            EffectSlot {
                effect,
                enabled: true,
            },
        );
    }

    /// Remove effect at index.
    pub fn remove(&mut self, index: usize) -> Option<Box<dyn AudioEffect>> {
        if index < self.slots.len() {
            Some(self.slots.remove(index).effect)
        } else {
            None
        }
    }

    /// Exchange two slots (`AudioServer.swap_bus_effects`). Returns false when
    /// either index is out of range, leaving the chain untouched.
    pub fn swap(&mut self, a: usize, b: usize) -> bool {
        if a < self.slots.len() && b < self.slots.len() {
            self.slots.swap(a, b);
            true
        } else {
            false
        }
    }

    /// Set a slot's bypass flag (`AudioServer.set_bus_effect_enabled`).
    /// Returns false for an out-of-range index.
    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> bool {
        match self.slots.get_mut(index) {
            Some(slot) => {
                slot.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Whether the slot at `index` currently processes. False when absent.
    pub fn is_enabled(&self, index: usize) -> bool {
        self.slots.get(index).map(|s| s.enabled).unwrap_or(false)
    }

    /// Process the buffer through every ENABLED effect in order.
    ///
    /// A disabled slot is skipped entirely rather than processed-and-mixed-out,
    /// so bypassing is bit-exact with the dry path.
    pub fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        for slot in &mut self.slots {
            if slot.enabled {
                slot.effect.process(samples, sample_rate, channels);
            }
        }
    }

    /// Reset all effects, including disabled ones — a bypassed delay line must
    /// not resume with stale audio when it is switched back on.
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.effect.reset();
        }
    }

    /// Effect type names in order.
    pub fn names(&self) -> Vec<&str> {
        self.slots.iter().map(|s| s.effect.name()).collect()
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

// ---------------------------------------------------------------------------
// Biquad filter family — Godot `AudioEffectFilter` (pat-u3l8c.107)
// ---------------------------------------------------------------------------

/// Biquad response type (RBJ audio-EQ cookbook).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Low-pass (attenuate above cutoff).
    LowPass,
    /// High-pass (attenuate below cutoff).
    HighPass,
    /// Band-pass (0 dB peak at centre).
    BandPass,
    /// Notch / band-reject (null at centre).
    Notch,
    /// Peaking EQ (boost/cut a band by `gain_db`).
    Peak,
    /// Low shelf (boost/cut below cutoff by `gain_db`).
    LowShelf,
    /// High shelf (boost/cut above cutoff by `gain_db`).
    HighShelf,
}

/// Normalised biquad coefficients (Direct Form I), with `a0` folded to 1.
#[derive(Debug, Clone, Copy)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    /// RBJ cookbook coefficients for `mode` at `f0` Hz, quality `q`, shelf/peak
    /// `gain_db`, sampled at `sample_rate`.
    fn design(mode: FilterMode, f0: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let f0 = f0.clamp(1.0, sr * 0.49); // keep below Nyquist
        let q = q.max(1e-3);
        let w0 = 2.0 * std::f32::consts::PI * f0 / sr;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let a = 10.0f32.powf(gain_db / 40.0); // shelf/peak amplitude

        let (b0, b1, b2, a0, a1, a2) = match mode {
            FilterMode::LowPass => (
                (1.0 - cw) * 0.5,
                1.0 - cw,
                (1.0 - cw) * 0.5,
                1.0 + alpha,
                -2.0 * cw,
                1.0 - alpha,
            ),
            FilterMode::HighPass => (
                (1.0 + cw) * 0.5,
                -(1.0 + cw),
                (1.0 + cw) * 0.5,
                1.0 + alpha,
                -2.0 * cw,
                1.0 - alpha,
            ),
            FilterMode::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            FilterMode::Notch => (1.0, -2.0 * cw, 1.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            FilterMode::Peak => (
                1.0 + alpha * a,
                -2.0 * cw,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cw,
                1.0 - alpha / a,
            ),
            FilterMode::LowShelf => {
                let ap1 = a + 1.0;
                let am1 = a - 1.0;
                let ta = 2.0 * a.sqrt() * alpha;
                (
                    a * (ap1 - am1 * cw + ta),
                    2.0 * a * (am1 - ap1 * cw),
                    a * (ap1 - am1 * cw - ta),
                    ap1 + am1 * cw + ta,
                    -2.0 * (am1 + ap1 * cw),
                    ap1 + am1 * cw - ta,
                )
            }
            FilterMode::HighShelf => {
                let ap1 = a + 1.0;
                let am1 = a - 1.0;
                let ta = 2.0 * a.sqrt() * alpha;
                (
                    a * (ap1 + am1 * cw + ta),
                    -2.0 * a * (am1 + ap1 * cw),
                    a * (ap1 + am1 * cw - ta),
                    ap1 - am1 * cw + ta,
                    2.0 * (am1 - ap1 * cw),
                    ap1 - am1 * cw - ta,
                )
            }
        };

        let inv = 1.0 / a0;
        Self {
            b0: b0 * inv,
            b1: b1 * inv,
            b2: b2 * inv,
            a1: a1 * inv,
            a2: a2 * inv,
        }
    }
}

/// Godot `AudioEffectFilter` family — one biquad in any [`FilterMode`].
#[derive(Debug, Clone)]
pub struct AudioEffectFilter {
    /// Response type.
    pub mode: FilterMode,
    /// Cutoff / centre frequency in Hz.
    pub cutoff_hz: f32,
    /// Quality factor (resonance).
    pub resonance: f32,
    /// Shelf / peak gain in dB (ignored for LP/HP/BP/Notch).
    pub gain_db: f32,
    /// Per-channel state: `[x1, x2, y1, y2]`.
    state: Vec<[f32; 4]>,
    /// Which Godot class produced this filter (pat-s3rrv.54).
    ///
    /// Godot has seven filter classes over one biquad; the mode alone does not
    /// round-trip to a class name (LowPass could be `AudioEffectLowPassFilter`
    /// or one stage of a band limit), so the loader records it.
    godot_class: &'static str,
}

impl AudioEffectFilter {
    /// Explicit constructor.
    pub fn new(mode: FilterMode, cutoff_hz: f32, resonance: f32, gain_db: f32) -> Self {
        Self {
            mode,
            cutoff_hz,
            resonance,
            gain_db,
            state: Vec::new(),
            godot_class: "AudioEffectFilter",
        }
    }

    /// Tags this filter with the Godot class it was loaded from
    /// (pat-s3rrv.54).
    pub fn with_godot_class(mut self, class_name: &'static str) -> Self {
        self.godot_class = class_name;
        self
    }
    /// Low-pass filter wrapper.
    pub fn low_pass(cutoff_hz: f32, resonance: f32) -> Self {
        Self::new(FilterMode::LowPass, cutoff_hz, resonance, 0.0)
    }
    /// High-pass filter wrapper.
    pub fn high_pass(cutoff_hz: f32, resonance: f32) -> Self {
        Self::new(FilterMode::HighPass, cutoff_hz, resonance, 0.0)
    }
    /// Band-pass filter wrapper.
    pub fn band_pass(center_hz: f32, resonance: f32) -> Self {
        Self::new(FilterMode::BandPass, center_hz, resonance, 0.0)
    }
    /// Notch (band-reject) filter wrapper.
    pub fn notch(center_hz: f32, resonance: f32) -> Self {
        Self::new(FilterMode::Notch, center_hz, resonance, 0.0)
    }
    /// Peaking EQ filter wrapper.
    pub fn peak(center_hz: f32, resonance: f32, gain_db: f32) -> Self {
        Self::new(FilterMode::Peak, center_hz, resonance, gain_db)
    }
    /// Low-shelf filter wrapper.
    pub fn low_shelf(cutoff_hz: f32, gain_db: f32) -> Self {
        Self::new(FilterMode::LowShelf, cutoff_hz, 0.707, gain_db)
    }
    /// High-shelf filter wrapper.
    pub fn high_shelf(cutoff_hz: f32, gain_db: f32) -> Self {
        Self::new(FilterMode::HighShelf, cutoff_hz, 0.707, gain_db)
    }
}

/// Several [`AudioEffectFilter`] stages in series (pat-s3rrv.54).
///
/// Two things in Godot's filter family need more than one biquad:
///
/// * **`db` slope.** `FilterDB` selects 6/12/18/24 dB per octave, and one
///   biquad gives 12 dB/oct. Godot cascades identical stages to steepen the
///   skirt, so the count is what the enum index selects.
/// * **`AudioEffectBandLimitFilter`**, which bounds a band from both sides —
///   a high-pass and a low-pass in series, not a single response.
///
/// Ignoring the slope would not fail loudly: the filter still filters, just
/// with the wrong skirt, which is why it is worth modelling rather than
/// rounding every class to one biquad.
pub struct AudioEffectFilterCascade {
    stages: Vec<AudioEffectFilter>,
    godot_class: &'static str,
}

impl AudioEffectFilterCascade {
    /// `stages` applied in order. Empty is allowed and is a pass-through.
    pub fn new(stages: Vec<AudioEffectFilter>, godot_class: &'static str) -> Self {
        Self {
            stages,
            godot_class,
        }
    }

    /// `count` identical stages of `proto`, for a `db`-slope cascade.
    pub fn repeated(proto: AudioEffectFilter, count: usize, godot_class: &'static str) -> Self {
        Self::new(vec![proto; count.max(1)], godot_class)
    }

    /// How many biquad stages this cascade runs.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
}

impl AudioEffect for AudioEffectFilterCascade {
    fn name(&self) -> &str {
        "Filter"
    }

    fn godot_class(&self) -> &str {
        self.godot_class
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        // In series: each stage sees the previous stage's output, which is what
        // makes the skirts add rather than the responses average.
        for stage in &mut self.stages {
            stage.process(samples, sample_rate, channels);
        }
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
    }
}

impl AudioEffect for AudioEffectFilter {
    fn name(&self) -> &str {
        "Filter"
    }

    fn godot_class(&self) -> &str {
        self.godot_class
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        let ch = channels.max(1) as usize;
        if self.state.len() != ch {
            self.state = vec![[0.0; 4]; ch];
        }
        let c = BiquadCoeffs::design(
            self.mode,
            self.cutoff_hz,
            self.resonance,
            self.gain_db,
            sample_rate as f32,
        );
        let frames = samples.len() / ch;
        for f in 0..frames {
            for ci in 0..ch {
                let x0 = samples[f * ch + ci];
                let s = &mut self.state[ci];
                let y0 = c.b0 * x0 + c.b1 * s[0] + c.b2 * s[1] - c.a1 * s[2] - c.a2 * s[3];
                s[1] = s[0];
                s[0] = x0;
                s[3] = s[2];
                s[2] = y0;
                samples[f * ch + ci] = y0;
            }
        }
    }

    fn reset(&mut self) {
        for s in &mut self.state {
            *s = [0.0; 4];
        }
    }
}

/// Dynamics compressor with optional sidechain key input.
#[derive(Debug, Clone)]
pub struct AudioEffectCompressor {
    /// Threshold in dB above which compression applies.
    pub threshold_db: f32,
    /// Compression ratio (e.g. 4.0 → 4:1).
    pub ratio: f32,
    /// Attack time in milliseconds.
    pub attack_ms: f32,
    /// Release time in milliseconds.
    pub release_ms: f32,
    /// Makeup gain in dB applied after compression.
    pub gain_db: f32,
    /// Optional external sidechain key (mono, one sample per frame).
    sidechain: Option<Vec<f32>>,
    /// The bus named by `AudioEffectCompressor.sidechain` in a bus layout
    /// (pat-s3rrv.53). Held so the mixer can resolve it each frame; the DSP
    /// above only ever sees the resulting key buffer.
    sidechain_bus: Option<String>,
    /// Envelope follower state (linear).
    env: f32,
}

impl AudioEffectCompressor {
    /// Create a compressor.
    pub fn new(threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32) -> Self {
        Self {
            threshold_db,
            ratio: ratio.max(1.0),
            attack_ms,
            release_ms,
            gain_db: 0.0,
            sidechain: None,
            sidechain_bus: None,
            env: 0.0,
        }
    }

    /// Names the bus this compressor keys from (`AudioEffectCompressor.sidechain`).
    ///
    /// An empty name clears it: Godot writes `sidechain = ""` for "no sidechain",
    /// and treating that as a bus called "" would send the mixer looking for a
    /// bus that cannot exist on every frame.
    pub fn set_sidechain_bus(&mut self, bus: Option<String>) {
        self.sidechain_bus = bus.filter(|b| !b.is_empty());
    }

    /// Provide (or clear) an external sidechain key. One sample per frame.
    pub fn set_sidechain(&mut self, key: Option<Vec<f32>>) {
        self.sidechain = key;
    }
}

impl AudioEffect for AudioEffectCompressor {
    fn name(&self) -> &str {
        "Compressor"
    }

    fn sidechain_bus(&self) -> Option<&str> {
        self.sidechain_bus.as_deref()
    }

    fn set_sidechain_key(&mut self, key: Option<Vec<f32>>) {
        self.sidechain = key;
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        let ch = channels.max(1) as usize;
        let sr = sample_rate.max(1) as f32;
        let att = (-1.0 / ((self.attack_ms.max(0.01) * 0.001) * sr)).exp();
        let rel = (-1.0 / ((self.release_ms.max(0.01) * 0.001) * sr)).exp();
        let ratio = self.ratio.max(1.0);
        let makeup = 10.0f32.powf(self.gain_db / 20.0);
        let frames = samples.len() / ch;
        for f in 0..frames {
            // Detector: external sidechain key if present, else max abs across channels.
            let detector = if let Some(key) = &self.sidechain {
                key.get(f).copied().unwrap_or(0.0).abs()
            } else {
                let mut m = 0.0f32;
                for c in 0..ch {
                    m = m.max(samples[f * ch + c].abs());
                }
                m
            };
            // Peak envelope follower with attack/release ballistics.
            let coeff = if detector > self.env { att } else { rel };
            self.env = coeff * self.env + (1.0 - coeff) * detector;
            let env_db = 20.0 * (self.env + 1e-9).log10();
            let gain_db = if env_db > self.threshold_db {
                -(env_db - self.threshold_db) * (1.0 - 1.0 / ratio)
            } else {
                0.0
            };
            let g = 10.0f32.powf(gain_db / 20.0) * makeup;
            for c in 0..ch {
                samples[f * ch + c] *= g;
            }
        }
    }

    fn reset(&mut self) {
        self.env = 0.0;
    }
}

/// Soft-knee limiter — smoothly saturates peaks toward a ceiling.
#[derive(Debug, Clone)]
pub struct AudioEffectLimiter {
    /// Output ceiling in dB (peaks never exceed this).
    pub ceiling_db: f32,
    /// Soft-clip knee width in dB below the ceiling.
    pub soft_clip_db: f32,
}

impl AudioEffectLimiter {
    /// Create a soft limiter.
    pub fn new(ceiling_db: f32, soft_clip_db: f32) -> Self {
        Self {
            ceiling_db,
            soft_clip_db: soft_clip_db.max(0.0),
        }
    }
}

impl AudioEffect for AudioEffectLimiter {
    fn name(&self) -> &str {
        "Limiter"
    }

    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, _channels: u16) {
        let ceiling = 10.0f32.powf(self.ceiling_db / 20.0);
        let knee = 10.0f32.powf((self.ceiling_db - self.soft_clip_db) / 20.0);
        let range = (ceiling - knee).max(1e-6);
        for s in samples.iter_mut() {
            let mag = s.abs();
            if mag > knee {
                let over = mag - knee;
                let comp = knee + range * (over / range).tanh();
                *s = s.signum() * comp.min(ceiling);
            }
        }
    }

    fn reset(&mut self) {}
}

/// Hard (brick-wall) limiter — clips at the ceiling after optional pre-gain.
#[derive(Debug, Clone)]
pub struct AudioEffectHardLimiter {
    /// Clip ceiling in dB.
    pub ceiling_db: f32,
    /// Pre-gain (drive) in dB applied before clipping.
    pub pre_gain_db: f32,
}

impl AudioEffectHardLimiter {
    /// Create a hard limiter.
    pub fn new(ceiling_db: f32, pre_gain_db: f32) -> Self {
        Self {
            ceiling_db,
            pre_gain_db,
        }
    }
}

impl AudioEffect for AudioEffectHardLimiter {
    fn name(&self) -> &str {
        "HardLimiter"
    }

    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, _channels: u16) {
        let ceiling = 10.0f32.powf(self.ceiling_db / 20.0);
        let drive = 10.0f32.powf(self.pre_gain_db / 20.0);
        for s in samples.iter_mut() {
            *s = (*s * drive).clamp(-ceiling, ceiling);
        }
    }

    fn reset(&mut self) {}
}

/// Spectrum analyzer tap. Buffers a mono mixdown and answers
/// [`get_magnitude_for_frequency_range`](Self::get_magnitude_for_frequency_range)
/// via a windowed DFT (direct, not a fast FFT — correct magnitudes; can be
/// swapped for an FFT later). Mirrors Godot
/// `AudioEffectSpectrumAnalyzerInstance` (MAGNITUDE_MAX mode).
#[derive(Debug, Clone)]
pub struct AudioEffectSpectrumAnalyzer {
    /// History window length in seconds.
    pub buffer_length_secs: f32,
    buf: Vec<f32>,
    sample_rate: u32,
}

impl AudioEffectSpectrumAnalyzer {
    /// Create an analyzer holding `buffer_length_secs` of history.
    pub fn new(buffer_length_secs: f32) -> Self {
        Self {
            buffer_length_secs: buffer_length_secs.max(0.01),
            buf: Vec::new(),
            sample_rate: 0,
        }
    }

    /// Peak magnitude in `[from_hz, to_hz]` over the buffered window.
    pub fn get_magnitude_for_frequency_range(&self, from_hz: f32, to_hz: f32) -> f32 {
        let n = self.buf.len();
        if n == 0 || self.sample_rate == 0 {
            return 0.0;
        }
        let sr = self.sample_rate as f32;
        let nyq = sr * 0.5;
        let f_lo = from_hz.min(to_hz).clamp(0.0, nyq);
        let f_hi = from_hz.max(to_hz).clamp(0.0, nyq);
        let df = sr / n as f32;
        let step = df.max((f_hi - f_lo) / 32.0).max(1.0);
        let denom = (n as f32 - 1.0).max(1.0);
        let mut maxmag = 0.0f32;
        let mut f = f_lo;
        while f <= f_hi + 1e-3 {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            for (i, &x) in self.buf.iter().enumerate() {
                // Hann window
                let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / denom).cos();
                let ang = -2.0 * std::f32::consts::PI * f * i as f32 / sr;
                re += w * x * ang.cos();
                im += w * x * ang.sin();
            }
            let mag = (re * re + im * im).sqrt() / n as f32 * 2.0;
            if mag > maxmag {
                maxmag = mag;
            }
            f += step;
        }
        maxmag
    }
}

impl AudioEffect for AudioEffectSpectrumAnalyzer {
    fn name(&self) -> &str {
        "SpectrumAnalyzer"
    }

    /// The analyzer is the one effect that answers the spectrum query
    /// (pat-0q2kx.160) — this is what makes it reachable through a bus's
    /// effect chain rather than only on a directly-owned value.
    fn magnitude_for_frequency_range(&self, from_hz: f32, to_hz: f32) -> Option<f32> {
        Some(self.get_magnitude_for_frequency_range(from_hz, to_hz))
    }

    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        self.sample_rate = sample_rate.max(1);
        let ch = channels.max(1) as usize;
        let frames = samples.len() / ch;
        let max_len = ((self.buffer_length_secs * self.sample_rate as f32) as usize).max(1);
        for f in 0..frames {
            let mut m = 0.0f32;
            for c in 0..ch {
                m += samples[f * ch + c];
            }
            self.buf.push(m / ch as f32);
        }
        if self.buf.len() > max_len {
            let excess = self.buf.len() - max_len;
            self.buf.drain(0..excess);
        }
        // Analyzer is a tap: samples pass through unchanged.
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.sample_rate = 0;
    }
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

    // --- Biquad filter family (pat-u3l8c.107) ------------------------------

    fn sine(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin() * 0.5)
            .collect()
    }

    fn energy(buf: &[f32]) -> f32 {
        buf.iter().map(|s| s * s).sum()
    }

    #[test]
    fn low_pass_keeps_lows_cuts_highs() {
        let sr = 48000u32;
        let mut lp = AudioEffectFilter::low_pass(1000.0, 0.707);
        let mut low = sine(200.0, sr, 4096);
        let mut high = sine(12000.0, sr, 4096);
        lp.process(&mut low, sr, 1);
        lp.reset();
        lp.process(&mut high, sr, 1);
        assert!(
            energy(&low) > energy(&high) * 4.0,
            "LP: low={} high={}",
            energy(&low),
            energy(&high)
        );
    }

    #[test]
    fn high_pass_keeps_highs_cuts_lows() {
        let sr = 48000u32;
        let mut hp = AudioEffectFilter::high_pass(2000.0, 0.707);
        let mut low = sine(100.0, sr, 4096);
        let mut high = sine(10000.0, sr, 4096);
        hp.process(&mut low, sr, 1);
        hp.reset();
        hp.process(&mut high, sr, 1);
        assert!(
            energy(&high) > energy(&low) * 4.0,
            "HP: low={} high={}",
            energy(&low),
            energy(&high)
        );
    }

    #[test]
    fn band_pass_favours_center() {
        let sr = 48000u32;
        let run = |freq: f32| {
            let mut bp = AudioEffectFilter::band_pass(1000.0, 2.0);
            let mut buf = sine(freq, sr, 4096);
            bp.process(&mut buf, sr, 1);
            energy(&buf)
        };
        let center = run(1000.0);
        let far = run(8000.0);
        assert!(center > far * 4.0, "BP: center={center} far={far}");
    }

    #[test]
    fn notch_rejects_center() {
        let sr = 48000u32;
        let mut notch = AudioEffectFilter::notch(1000.0, 2.0);
        let mut at_center = sine(1000.0, sr, 4096);
        let raw = energy(&at_center);
        notch.process(&mut at_center, sr, 1);
        let cut = energy(&at_center);
        assert!(cut < raw * 0.5, "notch: raw={raw} cut={cut}");
    }

    #[test]
    fn peak_boost_raises_center_band() {
        let sr = 48000u32;
        let mut peak = AudioEffectFilter::peak(1000.0, 1.0, 12.0);
        let mut buf = sine(1000.0, sr, 4096);
        let raw = energy(&buf);
        peak.process(&mut buf, sr, 1);
        let boosted = energy(&buf);
        assert!(
            boosted > raw * 1.5,
            "peak +12dB: raw={raw} boosted={boosted}"
        );
    }

    // --- Dynamics (pat-u3l8c.107) ------------------------------------------

    fn peak_abs(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn compressor_reduces_loud_signal() {
        let sr = 48000u32;
        let n = 8000;
        let mut loud: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin() * 0.9)
            .collect();
        let in_peak = peak_abs(&loud);
        let mut comp = AudioEffectCompressor::new(-24.0, 8.0, 2.0, 50.0);
        comp.process(&mut loud, sr, 1);
        let out_peak = peak_abs(&loud[n / 2..]);
        assert!(
            out_peak < in_peak * 0.7,
            "compressor: in={in_peak} out={out_peak}"
        );
    }

    #[test]
    fn compressor_passes_signal_below_threshold() {
        let sr = 48000u32;
        let n = 4000;
        let mut quiet: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin() * 0.02)
            .collect();
        let in_peak = peak_abs(&quiet);
        let mut comp = AudioEffectCompressor::new(-20.0, 8.0, 2.0, 50.0); // signal ~ -34 dB
        comp.process(&mut quiet, sr, 1);
        let out_peak = peak_abs(&quiet[n / 2..]);
        assert!(
            (out_peak - in_peak).abs() < in_peak * 0.1,
            "below threshold passes: in={in_peak} out={out_peak}"
        );
    }

    #[test]
    fn compressor_sidechain_ducks_quiet_main() {
        let sr = 48000u32;
        let n = 4000;
        // Quiet main (~ -26 dB) that would NOT self-compress below the -24 dB threshold.
        let mut main: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin() * 0.05)
            .collect();
        let in_peak = peak_abs(&main);
        let key: Vec<f32> = vec![0.9; n]; // loud sidechain key
        let mut comp = AudioEffectCompressor::new(-24.0, 8.0, 2.0, 50.0);
        comp.set_sidechain(Some(key));
        comp.process(&mut main, sr, 1);
        let out_peak = peak_abs(&main[n / 2..]);
        assert!(
            out_peak < in_peak * 0.7,
            "sidechain ducks main: in={in_peak} out={out_peak}"
        );
    }

    #[test]
    fn soft_limiter_respects_ceiling() {
        let ceiling = 10.0f32.powf(-0.1 / 20.0);
        let mut lim = AudioEffectLimiter::new(-0.1, 6.0);
        let mut buf: Vec<f32> = vec![2.0, -2.0, 1.5, -1.5, 0.8, -0.8];
        lim.process(&mut buf, 48000, 1);
        for &s in &buf {
            assert!(
                s.abs() <= ceiling + 1e-4,
                "soft limiter ceiling {ceiling}: got {s}"
            );
        }
        // Sub-knee sample preserved.
        let mut small = vec![0.1_f32];
        lim.process(&mut small, 48000, 1);
        assert!(
            (small[0] - 0.1).abs() < 1e-4,
            "sub-knee unchanged: {}",
            small[0]
        );
    }

    #[test]
    fn hard_limiter_brickwalls() {
        let mut lim = AudioEffectHardLimiter::new(0.0, 0.0); // ceiling = 1.0
        let mut buf: Vec<f32> = vec![2.0, -3.0, 0.5, -0.5];
        lim.process(&mut buf, 48000, 1);
        assert_eq!(buf[0], 1.0);
        assert_eq!(buf[1], -1.0);
        assert!((buf[2] - 0.5).abs() < 1e-6);
        assert!((buf[3] + 0.5).abs() < 1e-6);
        assert!(buf.iter().all(|&s| s.abs() <= 1.0 + 1e-6));
    }

    // --- Spectrum analyzer (pat-u3l8c.107) ---------------------------------

    #[test]
    fn spectrum_peaks_in_tone_band() {
        let sr = 48000u32;
        let n = 4096;
        let mut tone: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * 0.8)
            .collect();
        let mut analyzer = AudioEffectSpectrumAnalyzer::new(1.0);
        analyzer.process(&mut tone, sr, 1);
        let in_band = analyzer.get_magnitude_for_frequency_range(900.0, 1100.0);
        let out_band = analyzer.get_magnitude_for_frequency_range(5000.0, 6000.0);
        assert!(
            in_band > out_band * 4.0,
            "1kHz tone peaks in band: in={in_band} out={out_band}"
        );
        assert!(in_band > 0.1, "in-band magnitude substantial: {in_band}");
        // Tap passes samples through unchanged.
        assert!(tone.iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn filter_and_dynamics_compose_in_chain() {
        let mut chain = EffectChain::new();
        chain.add(Box::new(AudioEffectFilter::low_pass(2000.0, 0.707)));
        chain.add(Box::new(AudioEffectCompressor::new(-18.0, 4.0, 5.0, 60.0)));
        chain.add(Box::new(AudioEffectHardLimiter::new(-0.5, 0.0)));
        assert_eq!(chain.names(), vec!["Filter", "Compressor", "HardLimiter"]);
        let mut buf = sine(500.0, 48000, 2048);
        chain.process(&mut buf, 48000, 1);
        let ceiling = 10.0f32.powf(-0.5 / 20.0);
        assert!(buf.iter().all(|s| s.is_finite()));
        assert!(buf.iter().all(|&s| s.abs() <= ceiling + 1e-4));
    }
}
