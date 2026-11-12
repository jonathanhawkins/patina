//! Chorus: registered, loadable from a bus layout with its **path-shaped**
//! voice properties, and numerically active (pat-mgixk.89, 4th of 5).
//!
//! ⚠️ Not the bead's acceptance test. `pat-mgixk.89` covers five classes, and
//! its acceptance lives in **patina-runner**:
//! `crates/patina-runner/tests/audio_effect_modulation_family_registered_and_wired.rs`.
//! It has to, because the bead carries a `Wiring requirement:` — the acceptance
//! must boot a project through `EngineBootstrap` rather than call `AudioMixer`
//! directly, and `gdaudio` cannot depend on `patina_runner`. This file is a
//! narrow unit companion, deliberately NOT part of the bead's acceptance; its
//! three siblings are in `audio_effect_panner_phaser_stereo_enhance_wired.rs`.
//!
//! # 🔑 Why Chorus gets its own file
//!
//! It is the one class in the family whose authored keys are **paths**, and
//! **1-indexed** against a 0-based array: `voice/1/delay_ms` is `voices[0]`
//! (`audio_effect_chorus.cpp:316` uses `ADD_PROPERTYI(..., 0)` for `voice/1`).
//! Reading `voice/0/...` finds nothing and leaves every voice at its default —
//! a silent failure that looks exactly like a working chorus, because the
//! defaults are themselves audible. The layout test below authors a value no
//! default has so the difference is observable.
//!
//! Its per-voice defaults are also **not uniform**: Godot's constructor
//! pre-configures voices 1 and 2 and leaves 3 and 4 on the bare `Voice()`
//! values. An implementation that applied voice 1's defaults to all four would
//! pass a naive check, so the ClassDB assertion below pins voice 3 specifically.

use gdaudio::effects::{AudioEffect, AudioEffectChorus};
use gdaudio::mixer::AudioMixer;
use gdobject::class_db::{class_exists, get_class_info};
use gdobject::register_2d_classes;

const SR: u32 = 48_000;

/// Interleaved stereo sine.
fn stereo_sine(frames: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let s = (std::f32::consts::TAU * 440.0 * i as f32 / SR as f32).sin();
        v.push(s);
        v.push(s * 0.75);
    }
    v
}

fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max)
}

fn default_of(class: &str, prop: &str) -> f64 {
    let info = get_class_info(class).unwrap_or_else(|| panic!("{class} must be registered"));
    info.properties
        .iter()
        .find(|p| p.name == prop)
        .unwrap_or_else(|| panic!("{class} must expose `{prop}`"))
        .default_value
        .to_float()
}

#[test]
fn chorus_is_registered_with_godots_non_uniform_voice_defaults() {
    register_2d_classes();

    assert!(
        class_exists("AudioEffectChorus"),
        "AudioEffectChorus must be registered or a script cannot `.new()` one"
    );
    assert_eq!(
        get_class_info("AudioEffectChorus")
            .expect("chorus info")
            .parent_class,
        "AudioEffect",
        "AudioEffectChorus : AudioEffect, as in Godot"
    );

    // Voice 1 is pre-configured by Godot's constructor.
    assert_eq!(
        default_of("AudioEffectChorus", "voice/1/delay_ms"),
        15.0,
        "voice 1's delay default is 15 ms, and the key is the 1-INDEXED path \
         `voice/1/delay_ms`. A missing property here means the registration \
         used `voice/0/...` or a flat name"
    );
    assert_eq!(
        default_of("AudioEffectChorus", "voice/2/rate_hz"),
        1.2,
        "voice 2 has its own rate (1.2), distinct from voice 1's 0.8"
    );
    // 🔑 Voice 3 is NOT pre-configured — it keeps the bare `Voice()` cutoff of
    // 16000 (Godot's MS_CUTOFF_MAX, where the per-voice lowpass is bypassed).
    // An implementation that copied voice 1's 8000 to every slot fails here.
    assert_eq!(
        default_of("AudioEffectChorus", "voice/3/cutoff_hz"),
        16000.0,
        "voices 3 and 4 keep the bare Voice() defaults; only 1 and 2 are \
         pre-configured. 8000 here would mean voice 1's defaults were applied \
         to every slot"
    );
}

#[test]
fn chorus_loads_from_a_bus_layout_with_indexed_voice_paths() {
    // ⚠️ `voice/1/delay_ms = 33.0` is a value no default carries, so if the
    // path were misread the assertion below could not pass by accident.
    let layout = r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="AudioEffectChorus" id="C_1"]
voice_count = 3
dry = 0.25
wet = 0.75
voice/1/delay_ms = 33.0
voice/3/pan = -1.0

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Mod"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("C_1")
bus/1/effect/0/enabled = true
"#;
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(layout);
    let idx = mixer.get_bus_by_name("Mod").expect("Mod bus");
    let bus = mixer.get_bus(idx).expect("Mod bus");
    assert_eq!(
        bus.effects().len(),
        1,
        "a layout naming AudioEffectChorus must put one effect on the bus; 0 \
         means the factory still returns None and the slot is dropped"
    );
    let effect = bus.effects().effect_at(0).expect("the chorus");
    assert_eq!(
        AudioEffect::godot_class(effect),
        "AudioEffectChorus",
        "the slot must report its Godot class, not the short DSP label"
    );
}

#[test]
fn chorus_wet_zero_is_dry_only_and_defaults_colour_the_signal() {
    let dry_in = stereo_sine(4096);

    // 🔑 Neutral: `wet = 0` zeroes every voice's contribution, and `dry = 1`
    // passes the input through, so the output must be bit-identical. This is
    // the assertion that catches a voice summed in regardless of wet.
    let mut neutral = dry_in.clone();
    let mut fx = AudioEffectChorus::new();
    fx.wet = 0.0;
    fx.process(&mut neutral, SR, 2);
    assert!(
        max_abs_diff(&dry_in, &neutral) < 1e-6,
        "wet = 0 with dry = 1 must return the input unchanged; a difference \
         means a voice is mixed in without consulting wet"
    );

    // Active at Godot's defaults: two detuned, panned, lowpassed voices at
    // wet 0.5 must change the signal.
    let mut coloured = dry_in.clone();
    AudioEffectChorus::new().process(&mut coloured, SR, 2);
    assert!(
        max_abs_diff(&dry_in, &coloured) > 1e-3,
        "at Godot's defaults the two voices must audibly colour the signal; no \
         difference means the voice loop never ran"
    );

    // ⚠️ A voice whose cutoff is 0 is SKIPPED outright (Godot `continue`s on
    // it), not merely silenced — so a chorus whose only active voice has a zero
    // cutoff is dry-only. Pinning this stops the skip being "simplified" into a
    // zero-gain multiply, which would still cost the delay read every frame.
    let mut muted = dry_in.clone();
    let mut fx0 = AudioEffectChorus::new();
    fx0.voice_count = 1;
    fx0.voices[0].cutoff_hz = 0.0;
    fx0.process(&mut muted, SR, 2);
    assert!(
        max_abs_diff(&dry_in, &muted) < 1e-6,
        "a single voice with cutoff 0 is skipped, leaving the dry signal alone"
    );
}
