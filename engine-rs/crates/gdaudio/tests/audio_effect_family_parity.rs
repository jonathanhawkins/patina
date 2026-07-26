//! pat-0q2kx.158 — AudioEffect family: Delay/Distortion DSP and chain slot control.
//!
//! Godot models a bus insert as an effect PLUS an enabled bit, and exposes
//! `AudioServer.add_bus_effect(at_position)` / `swap_bus_effects` /
//! `set_bus_effect_enabled`. Patina's `EffectChain` was a bare
//! `Vec<Box<dyn AudioEffect>>`: no bypass, no insert-at, no swap — so the
//! bus-layout loader had to DROP disabled effects rather than carry them, and a
//! chain's order could never be edited after the fact.
//!
//! `Delay` and `Distortion` had no DSP at all — they existed only as strings in
//! the editor's effect-type dropdown.

// `AudioEffect` is imported for its METHODS, not as a type: `process`/`reset`
// are trait methods, so calling `delay.process(…)` on the concrete struct only
// resolves with the trait in scope.
use gdaudio::effects::{
    AudioEffect, AudioEffectAmplify, AudioEffectDelay, AudioEffectDistortion, DistortionMode,
    EffectChain,
};
// ClassDB is NOT populated implicitly in tests — the domain registrar has to be
// called first. The AudioEffect family is registered by `register_2d_classes`
// (an existing quirk: AudioEffectAmplify/Reverb/EQ already live there rather
// than in `register_audio_classes`), so that is the one to call.
use gdobject::class_db::{class_exists, get_class_info};
use gdobject::register_2d_classes;

const SR: u32 = 48_000;

/// A mono impulse: one full-scale sample then silence.
fn impulse(frames: usize) -> Vec<f32> {
    let mut v = vec![0.0; frames];
    v[0] = 1.0;
    v
}

/// Index of the largest absolute sample in `from..`.
fn peak_index_from(samples: &[f32], from: usize) -> usize {
    let mut best = from;
    for i in from..samples.len() {
        if samples[i].abs() > samples[best].abs() {
            best = i;
        }
    }
    best
}

/// **The acceptance test.**
#[test]
fn audio_effect_family_parity() {
    // -- a 50 ms delay puts a second peak 50 ms after the impulse -----------
    //
    // 50 ms at 48 kHz is exactly 2400 frames. Only tap1 is active so the
    // response has one unambiguous echo to find.
    let mut delay = AudioEffectDelay::new();
    delay.tap1_active = true;
    delay.tap1_delay_ms = 50.0;
    delay.tap1_level_db = 0.0;
    delay.tap2_active = false;
    delay.feedback_active = false;

    let mut buf = impulse(SR as usize / 4); // 250 ms of mono
    delay.process(&mut buf, SR, 1);

    let expected = (0.050 * SR as f32) as usize; // 2400
    let echo = peak_index_from(&buf, 1);
    assert!(
        (echo as i64 - expected as i64).abs() <= 1,
        "a 50 ms tap must echo at frame {expected}, found the post-impulse peak at {echo}",
    );
    assert!(
        buf[echo].abs() > 0.5,
        "the echo must be audible at 0 dB tap level, got {}",
        buf[echo],
    );

    // -- distortion in clip mode flattens a loud sine ------------------------
    let mut sine: Vec<f32> = (0..480)
        .map(|i| (i as f32 / SR as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.9)
        .collect();
    let mut dist = AudioEffectDistortion::new();
    dist.mode = DistortionMode::Clip;
    dist.drive = 1.0;
    dist.process(&mut sine, SR, 1);

    assert!(
        sine.iter().all(|s| s.abs() <= 1.0 + 1e-6),
        "clip mode must never exceed unity",
    );
    // A driven sine spends most of its time pinned at the rails.
    let pinned = sine.iter().filter(|s| s.abs() > 0.99).count();
    assert!(
        pinned > sine.len() / 2,
        "a driven sine must be flattened against the rails; only {pinned}/{} samples were",
        sine.len(),
    );

    // -- a disabled slot is bit-exact with the dry path ----------------------
    let dry: Vec<f32> = (0..256).map(|i| (i as f32 * 0.01).sin()).collect();

    let mut chain = EffectChain::new();
    chain.add(Box::new(AudioEffectAmplify::new(6.0)));
    assert!(chain.is_enabled(0), "a freshly added slot is enabled");

    let mut wet = dry.clone();
    chain.process(&mut wet, SR, 1);
    assert!(wet != dry, "the enabled Amplify must change the signal");

    assert!(chain.set_enabled(0, false), "slot 0 exists");
    assert!(!chain.is_enabled(0));
    let mut bypassed = dry.clone();
    chain.process(&mut bypassed, SR, 1);
    assert_eq!(
        bypassed, dry,
        "a disabled slot must leave the buffer BYTE-EQUAL to the dry path",
    );

    // Out-of-range indices report failure instead of panicking.
    assert!(!chain.set_enabled(99, true));
    assert!(!chain.is_enabled(99));

    // -- insert-at-0 then swap reorders processing observably ----------------
    //
    // Amplify-then-clip and clip-then-Amplify are genuinely different: clipping
    // first bounds the signal at 1.0, so a later 6 dB boost pushes it to ~2.0,
    // whereas boosting first then clipping can never exceed 1.0.
    let loud = vec![0.6f32; 64];

    let mut boost_then_clip = EffectChain::new();
    boost_then_clip.add(Box::new(clip_effect()));
    boost_then_clip.insert(0, Box::new(AudioEffectAmplify::new(6.0)));
    assert_eq!(
        boost_then_clip.names(),
        vec!["Amplify", "Distortion"],
        "insert(0, …) must place the effect FIRST",
    );
    let mut a = loud.clone();
    boost_then_clip.process(&mut a, SR, 1);

    // Swapping the same chain yields clip-then-boost.
    assert!(boost_then_clip.swap(0, 1));
    assert_eq!(boost_then_clip.names(), vec!["Distortion", "Amplify"]);
    let mut b = loud.clone();
    boost_then_clip.process(&mut b, SR, 1);

    assert!(
        a != b,
        "swapping two effects must change the output; both gave {:?}",
        &a[..4],
    );
    assert!(
        a[0].abs() <= 1.0 + 1e-6,
        "boost-then-clip is bounded by the clipper, got {}",
        a[0],
    );
    assert!(
        b[0].abs() > 1.0,
        "clip-then-boost amplifies AFTER the bound, so it must exceed unity, got {}",
        b[0],
    );

    // A swap with an out-of-range index is a no-op, not a panic.
    assert!(!boost_then_clip.swap(0, 99));
    assert_eq!(boost_then_clip.names(), vec!["Distortion", "Amplify"]);

    // -- every implemented class instantiates from its registered name -------
    register_2d_classes();
    for name in ["AudioEffectDelay", "AudioEffectDistortion"] {
        assert!(class_exists(name), "{name} must be registered in ClassDB");
        let info = get_class_info(name).unwrap_or_else(|| panic!("{name} resolves"));
        assert_eq!(
            info.parent_class, "AudioEffect",
            "{name} must inherit AudioEffect so the effect picker offers it",
        );
    }
    // The properties are what let a `.tres` chain set them by Godot name.
    let delay = get_class_info("AudioEffectDelay").expect("registered");
    for prop in ["dry", "tap1/delay_ms", "feedback/lowpass"] {
        assert!(
            delay.properties.iter().any(|p| p.name == prop),
            "AudioEffectDelay must expose `{prop}`; got {:?}",
            delay.properties.iter().map(|p| &p.name).collect::<Vec<_>>(),
        );
    }
}

/// A unity-drive clipper — the ordering probe above needs a bounded effect.
fn clip_effect() -> AudioEffectDistortion {
    let mut d = AudioEffectDistortion::new();
    d.mode = DistortionMode::Clip;
    d.drive = 0.0;
    d
}

/// `reset()` must clear a bypassed slot too, or re-enabling replays stale audio.
#[test]
fn reset_clears_disabled_slots() {
    let mut chain = EffectChain::new();
    let mut delay = AudioEffectDelay::new();
    delay.tap1_delay_ms = 10.0;
    delay.tap1_level_db = 0.0;
    delay.tap2_active = false;
    delay.feedback_active = false;
    chain.add(Box::new(delay));

    // Push an impulse through, then bypass and reset.
    let mut buf = impulse(1024);
    chain.process(&mut buf, SR, 1);
    chain.set_enabled(0, false);
    chain.reset();
    chain.set_enabled(0, true);

    // With the line cleared, silence in must give silence out.
    let mut silence = vec![0.0f32; 1024];
    chain.process(&mut silence, SR, 1);
    assert!(
        silence.iter().all(|s| s.abs() < 1e-6),
        "reset() must clear a disabled slot's delay line",
    );
}

/// Godot's `add_bus_effect(at_position)` appends when the position is past the end.
#[test]
fn insert_past_end_appends() {
    let mut chain = EffectChain::new();
    chain.add(Box::new(AudioEffectAmplify::new(1.0)));
    chain.insert(99, Box::new(clip_effect()));
    assert_eq!(chain.names(), vec!["Amplify", "Distortion"]);
    assert_eq!(chain.len(), 2);
}
