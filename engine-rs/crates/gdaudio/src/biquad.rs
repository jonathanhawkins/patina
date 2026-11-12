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
    ///
    /// ⚠️ `gain_db` is the RBJ-cookbook convention: the shelf amplitude is
    /// `A = 10^(dB/40)` and a shelf's asymptotic gain is `A²`, so a `gain_db` of
    /// −24 moves the shelf by −24 dB. Godot's own filters do **not** all use
    /// this spelling — `AudioServer::_mix_step_for_channel` hands the 3D
    /// attenuation filter a *linear* gain that lands in the `A` slot directly.
    /// Callers matching that path want [`Biquad::with_shelf_amplitude`].
    pub fn new(mode: FilterMode, cutoff_hz: f32, sample_rate: f32, q: f32, gain_db: f32) -> Self {
        // The RBJ shelf amplitude; `with_shelf_amplitude` is the same filter
        // parameterised by `A` instead of by dB.
        Self::with_shelf_amplitude(
            mode,
            cutoff_hz,
            sample_rate,
            q,
            10.0f32.powf(gain_db / 40.0),
        )
    }

    /// Builds a biquad from the shelf **amplitude** `a` rather than a dB gain.
    ///
    /// `a` is RBJ's `A`: the shelf's asymptotic gain is `a²`, and the
    /// non-shelf modes ignore it entirely.
    ///
    /// This exists because Godot's 3D attenuation filter is parameterised that
    /// way. `AudioStreamPlayer3D::_notification` computes
    /// `linear_attenuation = db_to_linear(db_att)` and passes it as the filter's
    /// *gain* (`audio_stream_player_3d.cpp:460-462`), and
    /// `AudioFilterSW::prepare_coefficients` puts that value straight into the
    /// `A` slot (`audio_filter_sw.cpp:79`, `:163-176`) — with `set_stages(1)`
    /// there is no `pow(gain, 1/(stages+1))` root to undo it. So Godot's
    /// effective shelf depth is `linear_attenuation²`, i.e. **twice the
    /// authored `attenuation_filter_db` in dB**, and calling
    /// [`Biquad::new`] with that dB value produces a shelf half as deep.
    pub fn with_shelf_amplitude(
        mode: FilterMode,
        cutoff_hz: f32,
        sample_rate: f32,
        q: f32,
        a: f32,
    ) -> Self {
        if sample_rate <= 0.0 || cutoff_hz <= 0.0 || q <= 0.0 {
            return Self::passthrough();
        }
        let w0 = 2.0 * PI * (cutoff_hz / sample_rate);
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);

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

    /// Response magnitude at DC (`z = 1`) and Nyquist (`z = -1`) — the two
    /// points where a shelf's asymptotes live, and where its depth is readable
    /// without running a sweep.
    fn gain_at_dc(f: &Biquad) -> f32 {
        (f.b0 + f.b1 + f.b2) / (1.0 + f.a1 + f.a2)
    }

    fn gain_at_nyquist(f: &Biquad) -> f32 {
        (f.b0 - f.b1 + f.b2) / (1.0 - f.a1 + f.a2)
    }

    /// pat-s3rrv.55 — the 3D attenuation filter's coefficients, against Godot.
    ///
    /// The bead recorded this as unresolvable without "an A/B against a real
    /// Godot build". It is resolvable from the source: `set_resonance(1)` and
    /// `set_stages(1)` at `audio_server.cpp:674-675` fix both free parameters,
    /// and `linear_attenuation` goes into the `A` slot at
    /// `audio_filter_sw.cpp:79`.
    ///
    /// ⚠️ The expected values are NOT read off this implementation. They come
    /// from a separate transcription of `audio_filter_sw.cpp`'s HIGHSHELF branch
    /// into Python that was executed, so a shared mistake in the Rust cannot
    /// make this pass — the technique exists because a filter that is
    /// self-consistently wrong looks exactly like one that is right.
    #[test]
    fn the_spatial_high_shelf_matches_godots_coefficients() {
        // db_att = -24 dB → linear_attenuation = 10^(-24/20).
        let linear_attenuation = 10.0f32.powf(-24.0 / 20.0);
        // ⚠️ q = 1, not the `AudioFilterSW` class default of 0.5: this call path
        // sets it explicitly (`audio_server.cpp:674`). Godot's shelf term is
        // `sqrt(gain)/sqrt(Q) * sin`, this one's is `2*sqrt(a)*alpha` with
        // `alpha = sin/(2q)`, so the two agree when `q == sqrt(Q)` — and
        // `sqrt(1) == 1`.
        let f = Biquad::with_shelf_amplitude(
            FilterMode::HighShelf,
            5_000.0,
            SR,
            1.0,
            linear_attenuation,
        );

        for (got, want, name) in [
            (f.b0, 0.016_886_37, "b0"),
            (f.b1, 0.008_624_88, "b1"),
            (f.b2, 0.006_186_35, "b2"),
            (f.a1, -1.798_718_46, "a1"),
            (f.a2, 0.830_416_06, "a2"),
        ] {
            assert!(
                (got - want).abs() < 1e-5,
                "{name}: got {got}, Godot gives {want}",
            );
        }
    }

    /// The depth question the bead left open, stated as behaviour.
    ///
    /// ⚠️ Godot's shelf is **twice as deep in dB** as the authored
    /// `attenuation_filter_db`, because `linear_attenuation = 10^(dB/20)` lands
    /// in the `A` slot whose asymptotic gain is `A²`. Feeding the same dB
    /// through the RBJ spelling (`Biquad::new`, `A = 10^(dB/40)`) yields a shelf
    /// at exactly half the depth — which is what this engine did, and which
    /// sounds plausible in isolation. Only the comparison names it.
    #[test]
    fn the_spatial_shelf_depth_is_the_square_of_the_linear_attenuation() {
        let db = -24.0f32;
        let linear_attenuation = 10.0f32.powf(db / 20.0);
        let f = Biquad::with_shelf_amplitude(
            FilterMode::HighShelf,
            5_000.0,
            SR,
            1.0,
            linear_attenuation,
        );

        // Lows pass untouched — a high shelf that also moved DC would be a
        // gain stage, not a shelf.
        assert!(
            (gain_at_dc(&f) - 1.0).abs() < 1e-4,
            "DC gain must be unity, got {}",
            gain_at_dc(&f),
        );
        let want = linear_attenuation * linear_attenuation;
        assert!(
            (gain_at_nyquist(&f) - want).abs() < 1e-6,
            "high-frequency gain must be linear_attenuation² = {want} (that is \
             {db} dB doubled to {} dB), got {}",
            db * 2.0,
            gain_at_nyquist(&f),
        );

        // The half-depth spelling, kept as the contrast: this is what the same
        // dB produces through the RBJ parameterisation.
        let rbj = Biquad::new(FilterMode::HighShelf, 5_000.0, SR, 1.0, db);
        assert!(
            (gain_at_nyquist(&rbj) - linear_attenuation).abs() < 1e-6,
            "the RBJ dB spelling gives 10^(dB/20), half the depth",
        );
    }

    /// The refactor must not have moved `Biquad::new`, which the bus-effect
    /// filters use. `new(…, db)` is exactly `with_shelf_amplitude(…, 10^(db/40))`.
    #[test]
    fn the_db_constructor_still_means_the_rbj_amplitude() {
        for db in [-24.0f32, -6.0, 0.0, 6.0] {
            for mode in [FilterMode::HighShelf, FilterMode::LowShelf] {
                let by_db = Biquad::new(mode, 5_000.0, SR, 0.707, db);
                let by_a =
                    Biquad::with_shelf_amplitude(mode, 5_000.0, SR, 0.707, 10.0f32.powf(db / 40.0));
                assert!(
                    (by_db.b0 - by_a.b0).abs() < 1e-9 && (by_db.a2 - by_a.a2).abs() < 1e-9,
                    "{mode:?} at {db} dB changed under the refactor",
                );
            }
        }
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
