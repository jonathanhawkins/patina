//! pat-0q2kx.160 — a spectrum analyzer inserted on a bus is queryable.
//!
//! `AudioEffectSpectrumAnalyzer` and its FFT existed with a passing unit test,
//! but nothing could reach an analyzer once it was living in a bus's effect
//! chain: `EffectChain` kept its slots private and `AudioServer` had no
//! `get_bus_effect_instance`. So the analyzer processed real mixed audio every
//! frame and no caller could ever read the result — the audit epic's recurring
//! "real code, zero reachable callers" shape.
//!
//! These drive the acceptance chain: insert the analyzer on a bus → play a
//! known tone → mix → query the magnitude around that tone and well away from
//! it.
//!
//! NOTE ON THE FILE NAME: `-p gdaudio --test spectrum_analyzer_bus_query` is 63
//! characters and the verifier truncates its command at 58, so it would run a
//! non-existent target. The acceptance's name is kept on the test function.

use gdaudio::effects::AudioEffectSpectrumAnalyzer;
// The trait must be in scope to call `magnitude_for_frequency_range` on the
// `&dyn AudioEffect` that `get_bus_effect_instance` returns.
use gdaudio::{AudioBuffer, AudioEffect, AudioEffectAmplify, AudioServer};

const SR: u32 = 44100;

/// A mono sine at `freq` Hz.
fn tone(freq: f32, seconds: f32) -> AudioBuffer {
    let frames = (SR as f32 * seconds) as usize;
    let samples = (0..frames)
        .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * freq / SR as f32).sin() * 0.8)
        .collect();
    AudioBuffer {
        samples,
        sample_rate: SR,
        channels: 1,
    }
}

/// A bus named `Viz` carrying a spectrum analyzer in slot 0, fed `freq` and
/// mixed long enough to fill the analyzer's window.
fn analyzed_bus(freq: f32) -> AudioServer {
    let mut server = AudioServer::new();
    let idx = server.mixer_mut().add_bus("Viz");
    server
        .mixer_mut()
        .get_bus_mut(idx)
        .unwrap()
        .effects_mut()
        .add(Box::new(AudioEffectSpectrumAnalyzer::new(0.05)));
    server.play_on_bus(tone(freq, 1.0), "Viz");
    // 12 * 256 = 3072 frames ≈ 70 ms, comfortably past the 50 ms window so the
    // analyzer's buffer is full of tone rather than partly silence.
    for _ in 0..12 {
        server.mix(256);
    }
    server
}

/// The acceptance: the magnitude around the played tone is far above an
/// off-tone band.
#[test]
fn spectrum_analyzer_instance_reports_tone_magnitude() {
    let server = analyzed_bus(1000.0);

    let inst = server
        .get_bus_effect_instance("Viz", 0)
        .expect("the analyzer inserted in slot 0 must be retrievable");

    let in_band = inst
        .magnitude_for_frequency_range(900.0, 1100.0)
        .expect("a spectrum analyzer must answer the magnitude query");
    let out_band = inst
        .magnitude_for_frequency_range(5000.0, 6000.0)
        .expect("a spectrum analyzer must answer the magnitude query");

    assert!(
        in_band > 0.01,
        "the 1 kHz tone must register in its own band, got {in_band}"
    );
    assert!(
        in_band > out_band * 4.0,
        "the tone's band must dominate an off-tone band; in={in_band} out={out_band}"
    );
}

/// The convenience wrapper agrees with going through the instance — it is what
/// a caller that cannot hold a borrow (the script bridge) will use.
#[test]
fn bus_effect_magnitude_matches_the_instance_query() {
    let server = analyzed_bus(1000.0);

    let via_instance = server
        .get_bus_effect_instance("Viz", 0)
        .unwrap()
        .magnitude_for_frequency_range(900.0, 1100.0)
        .unwrap();
    let via_server = server
        .bus_effect_magnitude("Viz", 0, 900.0, 1100.0)
        .expect("the server-level query must reach the analyzer");

    assert_eq!(via_instance, via_server);
}

/// Following the tone: moving the source moves which band dominates, so the
/// query is reading real spectrum rather than returning a constant.
#[test]
fn the_reported_band_follows_the_tone() {
    let low = analyzed_bus(500.0);
    let high = analyzed_bus(4000.0);

    let low_at_500 = low.bus_effect_magnitude("Viz", 0, 400.0, 600.0).unwrap();
    let low_at_4k = low.bus_effect_magnitude("Viz", 0, 3800.0, 4200.0).unwrap();
    let high_at_500 = high.bus_effect_magnitude("Viz", 0, 400.0, 600.0).unwrap();
    let high_at_4k = high.bus_effect_magnitude("Viz", 0, 3800.0, 4200.0).unwrap();

    assert!(
        low_at_500 > low_at_4k,
        "a 500 Hz tone must dominate at 500 Hz; 500={low_at_500} 4k={low_at_4k}"
    );
    assert!(
        high_at_4k > high_at_500,
        "a 4 kHz tone must dominate at 4 kHz; 500={high_at_500} 4k={high_at_4k}"
    );
}

/// Absent bus, out-of-range slot, and a non-analyzing effect all report
/// `None` rather than a misleading zero magnitude.
#[test]
fn non_analyzers_and_bad_indices_report_none() {
    let server = analyzed_bus(1000.0);
    assert!(
        server.get_bus_effect_instance("NoSuchBus", 0).is_none(),
        "an unknown bus has no effect instance"
    );
    assert!(
        server.get_bus_effect_instance("Viz", 7).is_none(),
        "an out-of-range slot has no effect instance"
    );

    // An Amplify in the slot answers `None` — the trait default — so callers can
    // tell "not an analyzer" apart from "analyzer reading silence" (0.0).
    let mut plain = AudioServer::new();
    let idx = plain.mixer_mut().add_bus("Plain");
    plain
        .mixer_mut()
        .get_bus_mut(idx)
        .unwrap()
        .effects_mut()
        .add(Box::new(AudioEffectAmplify::new(0.0)));
    let amp = plain.get_bus_effect_instance("Plain", 0).unwrap();
    assert!(
        amp.magnitude_for_frequency_range(900.0, 1100.0).is_none(),
        "a non-analyzing effect must report None, not a magnitude"
    );
}
