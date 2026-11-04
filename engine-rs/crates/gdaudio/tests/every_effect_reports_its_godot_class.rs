//! Every bus effect reports its real Godot class name.
//!
//! `AudioEffect::godot_class` defaulted to the SHORT display `name()`, so the
//! six effects that never overrode it reported "Amplify", "Reverb", "EQ",
//! "Compressor", "Limiter" and "HardLimiter" through
//! `AudioServer::bus_effect_class` — while the bus-layout loader and Godot both
//! call them `AudioEffectAmplify` and so on. A script or tool comparing the
//! class it added with the class it reads back saw a mismatch.

use gdaudio::effects::*;

#[test]
fn every_effect_reports_its_godot_class() {
    let effects: Vec<(Box<dyn AudioEffect>, &str)> = vec![
        (Box::new(AudioEffectAmplify::new(0.0)), "AudioEffectAmplify"),
        (Box::new(AudioEffectReverb::default()), "AudioEffectReverb"),
        (Box::new(AudioEffectEQ::new(0.0, 0.0, 0.0)), "AudioEffectEQ"),
        (
            Box::new(AudioEffectCompressor::new(0.0, 4.0, 20.0, 250.0)),
            "AudioEffectCompressor",
        ),
        (Box::new(AudioEffectLimiter::new(-0.1, 2.0)), "AudioEffectLimiter"),
        (
            Box::new(AudioEffectHardLimiter::new(-0.3, 0.0)),
            "AudioEffectHardLimiter",
        ),
        (Box::new(AudioEffectDelay::default()), "AudioEffectDelay"),
        (Box::new(AudioEffectPanner::default()), "AudioEffectPanner"),
        (Box::new(AudioEffectChorus::default()), "AudioEffectChorus"),
        (Box::new(AudioEffectPhaser::default()), "AudioEffectPhaser"),
        (Box::new(AudioEffectCapture::default()), "AudioEffectCapture"),
        (Box::new(AudioEffectRecord::default()), "AudioEffectRecord"),
    ];
    for (effect, want) in &effects {
        assert_eq!(effect.godot_class(), *want, "{} reports the wrong class", effect.name());
    }
}
