//! pat-s3rrv.53 — the dynamics family is nameable and actually processes.
//!
//! `AudioEffectCompressor`, `AudioEffectLimiter` and `AudioEffectHardLimiter`
//! had working, unit-tested DSP in `gdaudio::effects` and **nothing could name
//! them**: ClassDB stopped at Distortion, and `effect_from_sub_resource` had no
//! arm for any of them. A project whose Master bus ended in a compressor loaded
//! with an empty effect chain — the bus layout parsed, the slot was skipped, and
//! the audio came out untouched with no error anywhere.
//!
//! So the gap was never the maths. This test drives the two halves that were
//! missing:
//!
//! 1. **ClassDB** exposes all three with Godot's property names and defaults, so
//!    a `.tres` can set them by name and an effect picker can offer them.
//! 2. **A real `.tres` bus layout** builds live chain slots whose processing
//!    measurably changes the signal.
//!
//! A unit call to `AudioEffectCompressor::process` would prove neither.

use gdaudio::effects::AudioEffect;
use gdaudio::mixer::AudioMixer;
// ClassDB is NOT populated implicitly in tests — the domain registrar has to run
// first, and the AudioEffect family lives in `register_2d_classes` (an existing
// quirk the sibling `audio_effect_family_parity.rs` documents).
use gdobject::class_db::{class_exists, get_class_info};
use gdobject::register_2d_classes;

const SR: u32 = 48_000;
const CH: u16 = 1;

/// A bus layout whose Master chain is Compressor → HardLimiter, plus a second
/// bus for the compressor to name as its sidechain source.
///
/// Values are deliberately aggressive (threshold -24 dB, ratio 8:1, ceiling
/// -0.5 dB) so the effect on a full-scale tone is far larger than any rounding.
const LAYOUT: &str = r#"[sub_resource type="AudioEffectCompressor" id="Comp_1"]
threshold = -24.0
ratio = 8.0
attack_us = 20.0
release_ms = 250.0
sidechain = "Duck"

[sub_resource type="AudioEffectHardLimiter" id="Hard_1"]
ceiling_db = -0.5

[resource]
bus/0/name = "Master"
bus/0/effect/0/effect = SubResource("Comp_1")
bus/0/effect/0/enabled = true
bus/0/effect/1/effect = SubResource("Hard_1")
bus/0/effect/1/enabled = true
bus/1/name = "Duck"
"#;

/// A full-scale sine — loud enough to sit well above the -24 dB threshold.
fn tone(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin())
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// **The acceptance test.**
#[test]
fn audio_effect_dynamics_registered_and_compresses() {
    register_2d_classes();

    // -- 1. ClassDB exposes the three classes with Godot's defaults ----------
    for name in [
        "AudioEffectCompressor",
        "AudioEffectLimiter",
        "AudioEffectHardLimiter",
    ] {
        assert!(class_exists(name), "{name} must be registered");
        let info = get_class_info(name).unwrap_or_else(|| panic!("{name} resolves"));
        assert_eq!(
            info.parent_class, "AudioEffect",
            "{name} must inherit AudioEffect so the effect picker offers it",
        );
    }

    // The property NAMES are Godot's, not Patina's field names — a `.tres`
    // written by the Godot editor sets `threshold`, not `threshold_db`.
    let comp = get_class_info("AudioEffectCompressor").expect("registered");
    for prop in [
        "threshold",
        "ratio",
        "gain",
        "attack_us",
        "release_ms",
        "mix",
        "sidechain",
    ] {
        assert!(
            comp.properties.iter().any(|p| p.name == prop),
            "AudioEffectCompressor must expose `{prop}`; got {:?}",
            comp.properties.iter().map(|p| &p.name).collect::<Vec<_>>(),
        );
    }
    let limiter = get_class_info("AudioEffectLimiter").expect("registered");
    for prop in [
        "ceiling_db",
        "threshold_db",
        "soft_clip_db",
        "soft_clip_ratio",
    ] {
        assert!(
            limiter.properties.iter().any(|p| p.name == prop),
            "AudioEffectLimiter must expose `{prop}`",
        );
    }
    let hard = get_class_info("AudioEffectHardLimiter").expect("registered");
    for prop in ["ceiling_db", "pre_gain_db", "release"] {
        assert!(
            hard.properties.iter().any(|p| p.name == prop),
            "AudioEffectHardLimiter must expose `{prop}`",
        );
    }

    // -- 2. A bus layout builds LIVE slots, not an empty chain ---------------
    //
    // This is the half that was broken: the layout parsed fine before, and the
    // effect slots were silently dropped.
    let mut mixer = AudioMixer::new();
    let buses = mixer.load_bus_layout_tres(LAYOUT);
    assert!(buses >= 2, "Master and Duck must both load; got {buses}");

    let idx = mixer.get_bus_by_name("Master").expect("Master index");
    let chain = mixer.get_bus(idx).expect("Master bus").effects();
    assert_eq!(
        chain.len(),
        2,
        "Master must carry two live slots; 0 means the effect types are still \
         unknown to `effect_from_sub_resource` and the layout dropped them",
    );
    assert_eq!(
        chain.effect_at(0).map(|e| e.name()),
        Some("Compressor"),
        "slot 0 is the compressor",
    );
    assert_eq!(
        chain.effect_at(1).map(|e| e.name()),
        Some("HardLimiter"),
        "slot 1 is the hard limiter",
    );

    // -- 3. The chain measurably changes the signal --------------------------
    let frames = SR as usize / 10; // 100 ms, long enough to pass the attack
    let dry = tone(frames);
    let mut wet = dry.clone();

    mixer
        .get_bus_mut(idx)
        .expect("Master bus")
        .process_effects(&mut wet, SR, CH);

    // The hard limiter's ceiling is -0.5 dB. A small tolerance covers the
    // envelope's first samples and f32 rounding; the dry signal is full scale,
    // so this is a real reduction and not a no-op passing by luck.
    let ceiling = db_to_linear(-0.5);
    assert!(
        peak(&wet) <= ceiling + 0.02,
        "the hard limiter must cap the output at its ceiling ({ceiling:.4}); \
         peak was {:.4}. A peak at ~1.0 means the chain ran empty.",
        peak(&wet),
    );
    assert!(
        peak(&wet) < peak(&dry),
        "the processed signal must be quieter than the full-scale input: \
         dry {:.4} vs wet {:.4}",
        peak(&dry),
        peak(&wet),
    );
    // Not silence, and not a magic number: an effect that ZEROED the buffer
    // would satisfy both bounds above while being completely wrong, so the
    // floor has to be derived rather than guessed.
    //
    // A 0 dB input against threshold -24 dB at 8:1 is reduced by
    // `-(0 - (-24)) * (1 - 1/8)` = **-21 dB**, i.e. a linear peak of ~0.089.
    // The hard limiter's -0.5 dB ceiling (0.944) is far above that and does not
    // reduce it further, and the compressor's makeup `gain` defaults to 0.
    // Asserting a BAND around the derived level is strictly stronger than
    // "not zero": it also fails a compressor that is not actually compressing
    // (which would sit near 1.0).
    let expected = db_to_linear(-24.0 * (1.0 - 1.0 / 8.0));
    assert!(
        (peak(&wet) - expected).abs() < expected * 0.6,
        "the chain must pass audio at roughly the compressed level: expected \
         ~{expected:.4} (0 dB in, -24 dB threshold, 8:1 ratio => -21 dB), got \
         {:.4}. Near 0.0 means the chain muted the signal; near 1.0 means the \
         compressor slot is inert.",
        peak(&wet),
    );
}

/// Godot's `attack_us` is in MICROSECONDS while its sibling is `release_ms`.
///
/// The bus-layout arm divides by 1000 to reach the millisecond constructor. A
/// missing conversion is a 1000x attack time that still runs and still produces
/// plausible audio — it just never catches a transient — so it survives every
/// "does the chain change the signal" assertion. The only way to pin it without
/// downcasting the boxed effect is behaviourally: build the SAME compressor at
/// two attack times and compare how fast each clamps a full-scale onset.
#[test]
fn compressor_attack_is_read_as_microseconds() {
    fn early_peak(attack_us: f32) -> f32 {
        let layout = format!(
            r#"[sub_resource type="AudioEffectCompressor" id="C"]
threshold = -24.0
ratio = 8.0
attack_us = {attack_us}
release_ms = 250.0

[resource]
bus/0/name = "Master"
bus/0/effect/0/effect = SubResource("C")
bus/0/effect/0/enabled = true
"#
        );
        let mut mixer = AudioMixer::new();
        mixer.load_bus_layout_tres(&layout);
        let idx = mixer.get_bus_by_name("Master").expect("Master index");
        // A full-scale DC onset: the peak detector sees 0 dB from sample 0, so
        // the only thing separating the two runs is how fast the envelope rises.
        let mut buf = vec![1.0f32; SR as usize / 1000]; // 1 ms
        mixer
            .get_bus_mut(idx)
            .expect("Master bus")
            .process_effects(&mut buf, SR, CH);
        peak(&buf)
    }

    // 20 us = 0.02 ms: the envelope is essentially instant at 48 kHz, so the
    // 8:1 ratio over a -24 dB threshold pulls a 0 dB input down hard.
    let fast = early_peak(20.0);
    // 200000 us = 200 ms: within the first millisecond the envelope has barely
    // moved, so the signal passes near full scale.
    let slow = early_peak(200_000.0);

    assert!(
        fast < slow - 0.1,
        "a 20 us attack must clamp the first millisecond far harder than a \
         200 ms one: fast {fast:.4} vs slow {slow:.4}. Near-equal values mean \
         `attack_us` reached the constructor WITHOUT the microsecond-to-\
         millisecond conversion, leaving both attacks slow.",
    );
    assert!(
        slow > 0.5,
        "sanity: with a 200 ms attack the first millisecond should still be \
         near full scale, got {slow:.4}",
    );
}
