//! pat-s3rrv.55: the per-voice high-shelf actually reaches the mix.
//!
//! `compute_spatial` has always produced a `highshelf_db` — Godot muffles a
//! distant or off-axis 3D source as well as quietening it, deriving the
//! attenuation in `_update_panning` and feeding
//! `set_playback_highshelf_params` (`audio_stream_player_3d.cpp:444-465`). But
//! nothing consumed it: the mixer had no filter slot, so the value was computed
//! and dropped and **the emission cone had no audible effect whatsoever**.
//!
//! # Why the low-frequency case is the important one
//!
//! `emission_angle_filter_attenuation_db` is a FILTER attenuation — a
//! high-frequency rolloff, not a broadband volume cut. A test that only checked
//! "the signal got quieter" would pass just as well against a plain gain
//! reduction, which is the wrong behaviour and the easier thing to implement by
//! accident. So each case here pairs a high tone with a low one: the shelf must
//! take the highs down and leave the lows essentially alone.
//!
//! Expected magnitudes are derived from the biquad's own transfer function
//! rather than guessed. For a -12 dB shelf at 5 kHz with `q = sqrt(0.5)` on a
//! 44.1 kHz stream, |H| is 1.000 at 100 Hz, 0.501 at the 5 kHz cutoff (half the
//! shelf, in dB — the textbook shelf midpoint) and 0.252 at 15 kHz.

use gdaudio::decode::AudioBuffer;
use gdaudio::server::AudioServer;

const RATE: u32 = 44_100;
const FRAMES: usize = 4096;
/// Skipped when measuring: a biquad starts with zeroed delay state, so the
/// first samples carry a startup transient that is not the steady-state gain.
const SETTLE: usize = 512;

/// A mono sine at `freq`, long enough to mix `FRAMES` output frames.
fn tone(freq: f32) -> AudioBuffer {
    let n = FRAMES * 4;
    let samples = (0..n)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin())
        .collect();
    AudioBuffer {
        samples,
        sample_rate: RATE,
        channels: 1,
    }
}

/// RMS of the left channel over the settled portion.
fn rms_left(mix: &[f32], channels: usize) -> f32 {
    let frames: Vec<f32> = mix.chunks(channels).skip(SETTLE).map(|f| f[0]).collect();
    (frames.iter().map(|s| s * s).sum::<f32>() / frames.len() as f32).sqrt()
}

/// Mixes one tone, optionally with a shelf, and returns the settled RMS.
fn mixed_rms(freq: f32, shelf_db: f32) -> f32 {
    let mut server = AudioServer::new();
    let id = server.play(tone(freq));
    if let Some(pb) = server.stream_mut(id) {
        pb.set_highshelf(shelf_db, 5000.0);
    }
    let out = server.mix(FRAMES);
    let channels = out.len() / FRAMES;
    rms_left(&out, channels)
}

/// **The acceptance case**: a shelf cuts the highs and spares the lows.
#[test]
fn the_high_shelf_attenuates_high_frequencies_and_spares_low_ones() {
    let high_off = mixed_rms(15_000.0, 0.0);
    let high_on = mixed_rms(15_000.0, -12.0);
    let low_off = mixed_rms(200.0, 0.0);
    let low_on = mixed_rms(200.0, -12.0);

    // |H| at 15 kHz for a -12 dB shelf is 0.252, computed from the transfer
    // function. The window is wide enough to absorb the resampler and the
    // finite measurement, and far too narrow to admit "no filtering" (1.0).
    let high_ratio = high_on / high_off;
    assert!(
        (0.15..0.36).contains(&high_ratio),
        "a -12 dB shelf must take 15 kHz down to about 0.25 of its level; got \
         {high_ratio} ({high_on} vs {high_off})"
    );

    // 🔑 The half that makes it a SHELF. |H| at 200 Hz is 1.000, so a broadband
    // gain cut — the easy wrong implementation, and the one an
    // "is it quieter?" test cannot distinguish — fails here.
    let low_ratio = low_on / low_off;
    assert!(
        low_ratio > 0.95,
        "the same shelf must leave 200 Hz essentially untouched; got \
         {low_ratio} — that is a broadband gain cut, not a high shelf"
    );
}

/// 0 dB bypasses entirely, so every non-spatial voice mixes exactly as it did
/// before this filter existed.
///
/// ⚠️ Asserts BIT equality, not approximate. A unity-gain biquad is not quite
/// an identity — it would drift the samples in the last bits and silently
/// change the output of every existing audio test — so the mixer skips the
/// filter rather than running it at 0 dB.
#[test]
fn a_zero_db_shelf_is_an_exact_bypass() {
    let mut plain = AudioServer::new();
    let a = plain.play(tone(15_000.0));
    assert!(plain.stream_mut(a).is_some(), "stream is playing");
    let want = plain.mix(FRAMES);

    let mut shelved = AudioServer::new();
    let b = shelved.play(tone(15_000.0));
    if let Some(pb) = shelved.stream_mut(b) {
        pb.set_highshelf(0.0, 5000.0);
    }
    let got = shelved.mix(FRAMES);

    assert_eq!(got, want, "a 0 dB shelf must not touch the signal at all");
}

/// A deeper shelf cuts more — the parameter is honoured, not just its presence.
#[test]
fn a_deeper_shelf_cuts_more() {
    let off = mixed_rms(15_000.0, 0.0);
    let shallow = mixed_rms(15_000.0, -12.0) / off;
    let deep = mixed_rms(15_000.0, -24.0) / off;

    // |H| at 15 kHz is 0.252 for -12 dB and 0.064 for -24 dB.
    assert!(
        deep < shallow * 0.5,
        "-24 dB must cut markedly more than -12 dB; got {deep} vs {shallow}"
    );
    assert!(
        (0.03..0.11).contains(&deep),
        "-24 dB at 15 kHz should land near 0.064; got {deep}"
    );
}

/// A degenerate cutoff must disable the filter rather than produce NaN
/// coefficients — those would silence the voice and every bus it mixes into.
#[test]
fn a_non_positive_cutoff_disables_the_filter_rather_than_producing_nans() {
    for cutoff in [0.0, -1.0, f32::NAN] {
        let mut server = AudioServer::new();
        let id = server.play(tone(15_000.0));
        if let Some(pb) = server.stream_mut(id) {
            pb.set_highshelf(-12.0, cutoff);
        }
        let out = server.mix(FRAMES);
        assert!(
            out.iter().all(|s| s.is_finite()),
            "cutoff {cutoff} produced a non-finite sample"
        );
        let channels = out.len() / FRAMES;
        assert!(
            rms_left(&out, channels) > 0.1,
            "cutoff {cutoff} silenced the voice instead of disabling the filter"
        );
    }
}
