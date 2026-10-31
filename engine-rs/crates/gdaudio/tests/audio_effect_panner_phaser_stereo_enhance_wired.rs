//! Panner, Phaser and StereoEnhance: registered, loadable from a bus layout,
//! and numerically correct (pat-mgixk.89, 3 of the 5-effect family).
//!
//! # Scope
//!
//! ⚠️ This is **not** the bead's acceptance test. `pat-mgixk.89` covers five
//! classes, and its acceptance lives in **patina-runner**:
//! `crates/patina-runner/tests/audio_effect_modulation_family_registered_and_wired.rs`.
//! It has to, because the bead carries a `Wiring requirement:` — the acceptance
//! must boot a project through `EngineBootstrap` rather than call `AudioMixer`
//! directly, and `gdaudio` cannot depend on `patina_runner`. This file is a
//! narrow unit companion covering three of the five at close range.
//!
//! # What was missing
//!
//! `effects.rs` had no DSP for any of the five, so `load_bus_layout_tres`
//! dropped the slot: a Godot `default_bus_layout.tres` naming an
//! `AudioEffectPanner` silently lost it. None was in ClassDB either, so
//! `ClassDB.class_exists` answered false and a script could not `.new()` one.
//!
//! # Why these assertions
//!
//! Each effect gets a **neutral** case and an **active** case, because the two
//! fail differently: an effect that is wired but whose parameters are discarded
//! passes the active case only by accident, and one that is never invoked
//! passes the neutral case trivially. Panner additionally has an exact
//! closed-form answer, which is worth more than a difference check:
//! `pan = -1` gives `rvol = 0`, so `right = r*0 + l*(1-lvol)` and with
//! `lvol = 1` that is exactly zero.

use gdaudio::effects::{
    AudioEffect, AudioEffectPanner, AudioEffectPhaser, AudioEffectStereoEnhance,
};
use gdaudio::mixer::AudioMixer;
use gdobject::class_db::{class_exists, get_class_info};
use gdobject::register_2d_classes;

const SR: u32 = 48_000;

/// Interleaved stereo with both channels equal — the input the Panner
/// assertions are stated against.
fn mono_equal_stereo(frames: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let s = (std::f32::consts::TAU * 440.0 * i as f32 / SR as f32).sin();
        v.push(s);
        v.push(s);
    }
    v
}

/// Largest absolute difference between two equal-length buffers.
fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max)
}

/// Loads a one-bus layout holding `effect_type` with `body` properties and
/// returns the bus's effect count and the Godot class it reports.
fn load_one(effect_type: &str, body: &str) -> (usize, Option<String>) {
    let layout = format!(
        r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="{effect_type}" id="E_1"]
{body}

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Mod"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("E_1")
bus/1/effect/0/enabled = true
"#
    );
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&layout);
    let idx = mixer.get_bus_by_name("Mod").expect("Mod bus");
    let bus = mixer.get_bus(idx).expect("Mod bus");
    let count = bus.effects().len();
    let class = bus
        .effects()
        .effect_at(0)
        .map(|e| AudioEffect::godot_class(e).to_string());
    (count, class)
}

/// -- (1) ClassDB ----------------------------------------------------------
///
/// ⚠️ ClassDB is not populated implicitly. The AudioEffect family is registered
/// by `register_2d_classes` (an existing quirk this crate's other audio tests
/// also call out), so without this call every `class_exists` answers false and
/// the test fails for the wrong reason.
#[test]
fn modulation_effects_are_registered_under_audio_effect() {
    register_2d_classes();

    for class in [
        "AudioEffectPanner",
        "AudioEffectPhaser",
        "AudioEffectStereoEnhance",
    ] {
        assert!(
            class_exists(class),
            "{class} must be registered, or a script cannot `.new()` one"
        );
        let info = get_class_info(class).unwrap_or_else(|| panic!("{class} info"));
        assert_eq!(
            info.parent_class, "AudioEffect",
            "{class} : AudioEffect, as in Godot"
        );
    }

    // A representative default from each, taken from the upstream
    // `ADD_PROPERTY` calls rather than the doc XML — the two disagree on
    // accessor names, so the cpp is the authority.
    let phaser = get_class_info("AudioEffectPhaser").expect("phaser info");
    let range_min = phaser
        .properties
        .iter()
        .find(|p| p.name == "range_min_hz")
        .expect("Phaser exposes range_min_hz");
    // ⚠️ Read through `to_float()` rather than comparing against a
    // `Variant::Float(..)` literal: `gdvariant` is NOT a dependency of this
    // crate (only `gdcore` and, for tests, `gdobject`), so the type cannot be
    // named here — but an inherent method on it needs no import.
    assert_eq!(
        range_min.default_value.to_float(),
        440.0,
        "Godot's AudioEffectPhaser.range_min_hz default is 440 Hz"
    );
}

/// -- (2) a bus layout loads each one --------------------------------------
#[test]
fn modulation_effects_load_from_a_bus_layout() {
    for (ty, body) in [
        ("AudioEffectPanner", "pan = -1.0"),
        ("AudioEffectPhaser", "rate_hz = 2.0"),
        ("AudioEffectStereoEnhance", "pan_pullout = 2.0"),
    ] {
        let (count, class) = load_one(ty, body);
        assert_eq!(
            count, 1,
            "a layout naming {ty} must put ONE effect on the bus; 0 means \
             effect_from_sub_resource still returns None and the slot is dropped"
        );
        assert_eq!(
            class.as_deref(),
            Some(ty),
            "the slot must report its GODOT class, not the short DSP label — a \
             caller matching on the authored class name finds nothing otherwise"
        );
    }
}

/// -- (3) 🔑 Panner has an exact answer ------------------------------------
#[test]
fn panner_hard_left_silences_the_right_channel() {
    let dry = mono_equal_stereo(256);

    let mut hard_left = dry.clone();
    AudioEffectPanner::new(-1.0).process(&mut hard_left, SR, 2);
    let right_peak = hard_left
        .chunks(2)
        .map(|f| f[1].abs())
        .fold(0.0f32, f32::max);
    assert!(
        right_peak < 1e-6,
        "pan = -1 must leave the right channel silent (peak {right_peak:.2e}). \
         Godot's law is right = r*rvol + l*(1-lvol) with rvol = 0 and lvol = 1, \
         so this is exact, not approximate"
    );
    // ...and the signal is not merely gone: hard left SUMS both channels in.
    let left_peak = hard_left
        .chunks(2)
        .map(|f| f[0].abs())
        .fold(0.0f32, f32::max);
    assert!(
        left_peak > 1.5,
        "hard left must carry l + r (both channels are equal here, so ~2x), not \
         just attenuate the right. Got {left_peak:.3} — a value near 1.0 would \
         mean the opposite-channel bleed is missing"
    );

    // Neutral: pan = 0 leaves a stereo buffer untouched.
    let mut centred = dry.clone();
    AudioEffectPanner::new(0.0).process(&mut centred, SR, 2);
    assert!(
        max_abs_diff(&dry, &centred) < 1e-6,
        "pan = 0 must be a no-op"
    );

    // ⚠️ Mono is a no-op, deliberately: there is no stereo field to pan, and
    // scaling a mono buffer would quietly change the level of every mono bus
    // carrying a Panner.
    let mono: Vec<f32> = dry.iter().step_by(2).copied().collect();
    let mut mono_out = mono.clone();
    AudioEffectPanner::new(-1.0).process(&mut mono_out, SR, 1);
    assert!(
        max_abs_diff(&mono, &mono_out) < 1e-6,
        "a 1-channel buffer must pass through a Panner unchanged"
    );
}

/// -- (3b) Phaser: neutral at depth 0, active otherwise --------------------
#[test]
fn phaser_depth_zero_is_neutral_and_depth_one_colours() {
    let dry = mono_equal_stereo(2048);

    let mut neutral = dry.clone();
    let mut fx = AudioEffectPhaser::new();
    fx.depth = 0.0;
    fx.process(&mut neutral, SR, 2);
    assert!(
        max_abs_diff(&dry, &neutral) < 1e-6,
        "depth = 0 sums none of the allpass output, so the dry signal must come \
         back bit-for-bit. A difference means the allpass chain is writing to \
         the output path regardless of depth"
    );

    let mut coloured = dry.clone();
    AudioEffectPhaser::new().process(&mut coloured, SR, 2);
    assert!(
        max_abs_diff(&dry, &coloured) > 1e-3,
        "at Godot's defaults (depth 1.0, feedback 0.7) the six swept allpass \
         stages must audibly change the signal; no difference means process() \
         never ran the chain"
    );
}

/// -- (3c) StereoEnhance: neutral at Godot defaults ------------------------
///
/// 🔑 This is the assertion that catches a half-wired widener. Godot's defaults
/// are `pan_pullout = 1.0`, `time_pullout_ms = 0.0`, `surround = 0.0`, and with
/// those the maths reduces to identity: the width multiply is `center + (x -
/// center) * 1`, and the right channel is read back from the ring at a zero
/// delay — the sample just written. An implementation that, say, sized the ring
/// wrongly or applied a default delay would fail here while still "working".
#[test]
fn stereo_enhance_defaults_are_identity_and_width_changes_the_signal() {
    let dry = mono_equal_stereo(1024);

    let mut neutral = dry.clone();
    AudioEffectStereoEnhance::new().process(&mut neutral, SR, 2);
    assert!(
        max_abs_diff(&dry, &neutral) < 1e-6,
        "Godot's StereoEnhance defaults are an identity transform; a difference \
         means a default is wrong or the delay ring is misindexed"
    );

    // Width > 1 pushes the channels apart. With both channels equal the mid IS
    // the signal, so (x - center) is 0 and widening changes nothing — use a
    // genuinely stereo input instead.
    let mut widened: Vec<f32> = Vec::with_capacity(2048);
    for i in 0..1024 {
        let s = (std::f32::consts::TAU * 440.0 * i as f32 / SR as f32).sin();
        widened.push(s);
        widened.push(-s * 0.5);
    }
    let before = widened.clone();
    let mut fx = AudioEffectStereoEnhance::new();
    fx.pan_pullout = 2.0;
    fx.process(&mut widened, SR, 2);
    assert!(
        max_abs_diff(&before, &widened) > 1e-3,
        "pan_pullout = 2 must widen a genuinely stereo signal. ⚠️ Note this \
         fixture is NOT mono-equal: with both channels identical the mid equals \
         the signal, (x - center) is zero, and widening is a no-op for any \
         pullout — a mono-equal fixture here would pass while measuring nothing"
    );
}
