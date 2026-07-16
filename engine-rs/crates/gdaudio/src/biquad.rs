//! Biquad (second-order IIR) filters for the AudioEffectFilter family
//! (pat-nzgdu.152).
//!
//! Godot's `AudioEffectFilter` subclasses (LowPass/HighPass/BandPass/Notch/
//! LowShelf/HighShelf) are RBJ-cookbook biquads applied as bus insert effects.
//! This module is the dependency-free DSP core: compute normalized coefficients
//! for a filter mode from cutoff/sample-rate/resonance(Q)/gain, then stream
//! samples through a Direct-Form-II-Transposed state. The `AudioEffect` impls +
//! ClassDB registration + bus wiring consume this.

use std::f32::consts::PI;

/// The filter response type (Godot `AudioEffectFilter` modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Passes frequencies below the cutoff.
    LowPass,
    /// Passes frequencies above the cutoff.
    HighPass,
    /// Passes a band around the cutoff (0 dB peak).
    BandPass,
    /// Rejects a band around the cutoff.
    Notch,
    /// Boosts/cuts frequencies below the cutoff by `gain_db`.
    LowShelf,
    /// Boosts/cuts frequencies above the cutoff by `gain_db`.
    HighShelf,
}

/// A second-order IIR filter: normalized feed-forward (`b*`) / feedback (`a*`)
/// coefficients plus the two-sample Direct-Form-II-Transposed delay state.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// Builds a biquad for `mode` at `cutoff_hz` given the `sample_rate`, a
    /// resonance/quality factor `q` (Butterworth ≈ 0.707), and `gain_db` (only
    /// used by the shelf modes). Degenerate parameters yield a pass-through.
    pub fn new(mode: FilterMode, cutoff_hz: f32, sample_rate: f32, q: f32, gain_db: f32) -> Self {
        if sample_rate <= 0.0 || cutoff_hz <= 0.0 || q <= 0.0 {
            return Self::passthrough();
        }
        let w0 = 2.0 * PI * (cutoff_hz / sample_rate);
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let a = 10.0f32.powf(gain_db / 40.0); // shelf gain amplitude

        let (b0, b1, b2, a0, a1, a2) = match mode {
            FilterMode::LowPass => {
                let b1 = 1.0 - cw;
                (b1 * 0.5, b1, b1 * 0.5, 1.0 + alpha, -2.0 * cw, 1.0 - alpha)
            }
            FilterMode::HighPass => {
                let b1 = -(1.0 + cw);
                (
                    (1.0 + cw) * 0.5,
                    b1,
                    (1.0 + cw) * 0.5,
                    1.0 + alpha,
                    -2.0 * cw,
                    1.0 - alpha,
                )
            }
            FilterMode::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            FilterMode::Notch => (1.0, -2.0 * cw, 1.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            FilterMode::LowShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cw + sa),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    a * ((a + 1.0) - (a - 1.0) * cw - sa),
                    (a + 1.0) + (a - 1.0) * cw + sa,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    (a + 1.0) + (a - 1.0) * cw - sa,
                )
            }
            FilterMode::HighShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cw + sa),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - sa),
                    (a + 1.0) - (a - 1.0) * cw + sa,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - sa,
                )
            }
        };
        if a0.abs() < 1e-12 {
            return Self::passthrough();
        }
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// An identity filter (output == input).
    pub fn passthrough() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Processes one sample, advancing the filter state (Direct Form II
    /// Transposed).
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    /// Clears the delay state (call when playback restarts).
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    /// Sum of squares (energy) of `count` samples of a sine at `freq`, run
    /// through `filter`.
    fn filtered_energy(filter: &mut Biquad, freq: f32, count: usize) -> f32 {
        let mut e = 0.0f32;
        for n in 0..count {
            let x = (2.0 * PI * freq * n as f32 / SR).sin();
            let y = filter.process(x);
            e += y * y;
        }
        e
    }

    fn raw_energy(freq: f32, count: usize) -> f32 {
        let mut e = 0.0f32;
        for n in 0..count {
            let x = (2.0 * PI * freq * n as f32 / SR).sin();
            e += x * x;
        }
        e
    }

    #[test]
    fn lowpass_attenuates_highs_and_passes_lows() {
        let n = 8820; // 0.2s
        let ein_high = raw_energy(10_000.0, n);
        let ein_low = raw_energy(100.0, n);

        let mut lp = Biquad::new(FilterMode::LowPass, 1000.0, SR, 0.707, 0.0);
        let eout_high = filtered_energy(&mut lp, 10_000.0, n);
        let mut lp2 = Biquad::new(FilterMode::LowPass, 1000.0, SR, 0.707, 0.0);
        let eout_low = filtered_energy(&mut lp2, 100.0, n);

        assert!(
            eout_high < 0.2 * ein_high,
            "10kHz should be strongly attenuated: {eout_high} vs in {ein_high}"
        );
        assert!(
            eout_low > 0.7 * ein_low,
            "100Hz should pass a 1kHz low-pass: {eout_low} vs in {ein_low}"
        );
    }

    #[test]
    fn highpass_is_the_mirror() {
        let n = 8820;
        let ein_high = raw_energy(10_000.0, n);
        let mut hp = Biquad::new(FilterMode::HighPass, 1000.0, SR, 0.707, 0.0);
        let eout_high = filtered_energy(&mut hp, 10_000.0, n);
        // A high-pass passes the 10kHz tone largely intact.
        assert!(
            eout_high > 0.7 * ein_high,
            "10kHz should pass a 1kHz high-pass: {eout_high} vs in {ein_high}"
        );
    }

    #[test]
    fn passthrough_and_degenerate_are_identity() {
        let mut pt = Biquad::passthrough();
        assert_eq!(pt.process(0.5), 0.5);
        // Degenerate params fall back to pass-through.
        let mut bad = Biquad::new(FilterMode::LowPass, -1.0, SR, 0.707, 0.0);
        assert_eq!(bad.process(0.25), 0.25);
    }
}
