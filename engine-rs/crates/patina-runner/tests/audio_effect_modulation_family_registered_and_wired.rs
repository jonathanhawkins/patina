//! **The acceptance test for pat-mgixk.89.** All five modulation effects reach
//! a booted project's mixer from its `default_bus_layout.tres`, with their
//! authored properties, and behave.
//!
//! # ⚠️ Why this lives in `patina-runner` and not `gdaudio`
//!
//! My first version of this test was in `gdaudio/tests/`, built a bare
//! `AudioMixer` and called `load_bus_layout_tres` directly. The orchestrator
//! rejected it **before building** — this bead carries a `Wiring requirement:`
//! line, so the acceptance test has to enter through a public entry point and
//! assert what a *project* observes, not what the factory returns. A test that
//! calls the implementation directly proves the code exists, not that anything
//! reaches it.
//!
//! So this boots the real flow: a project directory with `project.godot` and a
//! `default_bus_layout.tres`, through `EngineBootstrap::run_all()`, which is
//! where `bootstrap.rs` reads `audio/buses/default_bus_layout` and hands the
//! text to the mixer. `gdaudio` cannot depend on `patina_runner`, hence the
//! move rather than an edit in place. The narrower per-effect DSP tests stay in
//! `gdaudio/tests/` where they belong.
//!
//! # What was missing
//!
//! Chorus, Panner, Phaser, PitchShift and StereoEnhance had no DSP, no ClassDB
//! entry, and `effect_from_sub_resource` answered `None` for all five — so a
//! Godot layout naming any of them silently lost the slot.
//!
//! # Why each clause is needed
//!
//! * **Registration** alone passes for a class that exists with no DSP.
//! * **Reaching the bus** alone passes for an effect whose authored parameters
//!   are discarded — the bug the Reverb arm had before pat-mgixk.84.
//! * **DSP** alone passes for an effect a project can never actually reach.

use std::path::PathBuf;

use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

const SR: u32 = 48_000;
const TONE_HZ: f32 = 440.0;

/// The five classes, each on its own bus so a failure names one effect.
const FAMILY: [(&str, &str); 5] = [
    ("Chorus", "AudioEffectChorus"),
    ("Panner", "AudioEffectPanner"),
    ("Phaser", "AudioEffectPhaser"),
    ("Pitch", "AudioEffectPitchShift"),
    ("Stereo", "AudioEffectStereoEnhance"),
];

const PROJECT_GODOT: &str = r#"[application]

config/name="ModulationFamily"

[audio]

buses/default_bus_layout="res://default_bus_layout.tres"
"#;

/// ⚠️ Authored values no default carries, so a key the factory misreads cannot
/// pass by accident: `pan = -1` (default 0), `depth = 0` (default 1),
/// `pitch_scale = 2` (default 1), `pan_pullout = 2` (default 1), and Chorus's
/// **1-indexed path** `voice/1/delay_ms = 33` (default 15).
const BUS_LAYOUT: &str = r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="AudioEffectChorus" id="E_Chorus"]
wet = 0.0
voice/1/delay_ms = 33.0

[sub_resource type="AudioEffectPanner" id="E_Panner"]
pan = -1.0

[sub_resource type="AudioEffectPhaser" id="E_Phaser"]
depth = 0.0

[sub_resource type="AudioEffectPitchShift" id="E_Pitch"]
pitch_scale = 2.0

[sub_resource type="AudioEffectStereoEnhance" id="E_Stereo"]
pan_pullout = 2.0

[sub_resource type="AudioEffectChorus" id="E_ChorusOn"]

[sub_resource type="AudioEffectPhaser" id="E_PhaserOn"]

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Chorus"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("E_Chorus")
bus/1/effect/0/enabled = true
bus/2/name = "Panner"
bus/2/volume_db = 0.0
bus/2/send = "Master"
bus/2/effect/0/effect = SubResource("E_Panner")
bus/2/effect/0/enabled = true
bus/3/name = "Phaser"
bus/3/volume_db = 0.0
bus/3/send = "Master"
bus/3/effect/0/effect = SubResource("E_Phaser")
bus/3/effect/0/enabled = true
bus/4/name = "Pitch"
bus/4/volume_db = 0.0
bus/4/send = "Master"
bus/4/effect/0/effect = SubResource("E_Pitch")
bus/4/effect/0/enabled = true
bus/5/name = "Stereo"
bus/5/volume_db = 0.0
bus/5/send = "Master"
bus/5/effect/0/effect = SubResource("E_Stereo")
bus/5/effect/0/enabled = true
bus/6/name = "ChorusOn"
bus/6/volume_db = 0.0
bus/6/send = "Master"
bus/6/effect/0/effect = SubResource("E_ChorusOn")
bus/6/effect/0/enabled = true
bus/7/name = "PhaserOn"
bus/7/volume_db = 0.0
bus/7/send = "Master"
bus/7/effect/0/effect = SubResource("E_PhaserOn")
bus/7/effect/0/enabled = true
"#;

fn write_project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "patina-modulation-family-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture project dir");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).expect("write project.godot");
    std::fs::write(dir.join("default_bus_layout.tres"), BUS_LAYOUT)
        .expect("write default_bus_layout.tres");
    dir
}

fn stereo_tone(frames: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let s = (std::f32::consts::TAU * TONE_HZ * i as f32 / SR as f32).sin();
        v.push(s);
        v.push(s);
    }
    v
}

fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max)
}

/// Single-bin DFT magnitude — enough to compare candidate bins against each
/// other, and less code to get wrong than a hand-rolled FFT.
fn magnitude_at(samples: &[f32], freq: f32) -> f32 {
    let w = std::f32::consts::TAU * freq / SR as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, s) in samples.iter().enumerate() {
        let ang = w * i as f32;
        re += s * ang.cos();
        im -= s * ang.sin();
    }
    (re * re + im * im).sqrt() / samples.len() as f32
}

fn dominant_freq(samples: &[f32], lo: f32, hi: f32, step: f32) -> f32 {
    let mut best = (lo, -1.0f32);
    let mut f = lo;
    while f <= hi {
        let m = magnitude_at(samples, f);
        if m > best.1 {
            best = (f, m);
        }
        f += step;
    }
    best.0
}

/// Bus index by name, panicking with the reason a miss actually means.
fn named_bus(server: &gdaudio::server::AudioServer, name: &str) -> usize {
    server.mixer().get_bus_by_name(name).unwrap_or_else(|| {
        panic!(
            "the layout declares a `{name}` bus; missing means bootstrap never \
             loaded default_bus_layout.tres"
        )
    })
}

/// Summed squared magnitude over `lo..=hi`.
///
/// 🔑 Band energy, not a single bin: see the PitchShift clause below — the
/// shifter's output is a **sideband pair**, so no one bin holds the answer and a
/// single-bin assertion measures the wrong thing.
fn band_energy(samples: &[f32], lo: f32, hi: f32) -> f32 {
    let mut total = 0.0f32;
    let mut f = lo;
    while f <= hi {
        let m = magnitude_at(samples, f);
        total += m * m;
        f += 2.0;
    }
    total
}

/// **The acceptance test.**
#[test]
fn audio_effect_modulation_family_registered_and_wired() {
    // ⚠️ The AudioEffect family is registered by `register_2d_classes` (an
    // existing quirk this crate's audio tests also call out). It is idempotent,
    // and calling it here means clause 1 cannot fail merely because boot
    // ordering changed.
    gdobject::register_2d_classes();

    let dir = write_project();
    let mut boot = EngineBootstrap::new(BootConfig::headless().project_dir(&dir));
    boot.run_all().expect("boot flow should complete");
    let main_loop = boot
        .main_loop_mut()
        .expect("the Running phase creates a MainLoop");
    let server = main_loop.audio_bridge_mut().server_mut();

    // -- (1) ClassDB -------------------------------------------------------
    for (_, class) in FAMILY {
        assert!(
            gdobject::class_db::class_exists(class),
            "{class} must be registered, or `ClassDB.class_exists` is false and \
             a script cannot `.new()` one"
        );
        assert_eq!(
            gdobject::class_db::get_class_info(class)
                .expect("class info")
                .parent_class,
            "AudioEffect",
            "{class} : AudioEffect, as in Godot"
        );
    }
    // Defaults from each `audio_effect_*.cpp` ADD_PROPERTY — the authority
    // where it disagrees with the doc XML.
    let default_of = |class: &str, prop: &str| -> f64 {
        gdobject::class_db::get_class_info(class)
            .unwrap_or_else(|| panic!("{class} must be registered"))
            .properties
            .iter()
            .find(|p| p.name == prop)
            .unwrap_or_else(|| panic!("{class} must expose `{prop}`"))
            .default_value
            .to_float()
    };
    assert_eq!(default_of("AudioEffectPhaser", "range_min_hz"), 440.0);
    // ⚠️ `fft_size` is an enum INDEX (3 == 2048 samples), not a window size.
    assert_eq!(default_of("AudioEffectPitchShift", "fft_size"), 3.0);
    // 🔑 Chorus voice keys are 1-INDEXED paths over a 0-based array, and voices
    // 3 and 4 keep the bare `Voice()` cutoff of 16000 rather than voice 1's
    // 8000 — an implementation that copied voice 1 everywhere fails here.
    assert_eq!(default_of("AudioEffectChorus", "voice/1/delay_ms"), 15.0);
    assert_eq!(
        default_of("AudioEffectChorus", "voice/3/cutoff_hz"),
        16000.0
    );

    // -- (2) every class reached the booted project's mixer -----------------
    let mut bus_of = Vec::new();
    for (bus_name, class) in FAMILY {
        let idx = server.mixer().get_bus_by_name(bus_name).unwrap_or_else(|| {
            panic!(
                "the layout declares a `{bus_name}` bus; missing means bootstrap \
                 never loaded default_bus_layout.tres, not that the effect was \
                 dropped"
            )
        });
        assert_eq!(
            server.bus_effect_class(idx, 0),
            Some(class),
            "bus `{bus_name}` slot 0 must hold {class}. `None` means \
             effect_from_sub_resource still returns None and a Godot layout \
             naming it silently loses the slot"
        );
        bus_of.push((class, idx));
    }

    // -- (3) DSP, through the buses the project actually built --------------
    let dry = stereo_tone(4096);
    let idx_of = |want: &str| -> usize {
        bus_of
            .iter()
            .find(|(c, _)| *c == want)
            .map(|(_, i)| *i)
            .expect("bus index")
    };
    // The two extra buses are the ACTIVE half of the Chorus/Phaser pairs; they
    // carry the same classes at Godot's defaults, so they are not in `FAMILY`.
    let on_chorus = named_bus(server, "ChorusOn");
    let on_phaser = named_bus(server, "PhaserOn");

    // Panner has an exact closed form: `pan = -1` gives rvol 0 and lvol 1, so
    // right = r*0 + l*(1-1) is exactly zero. Worth more than a difference check,
    // and it also proves the authored -1 was read (the default 0 is a no-op).
    let mut panned = dry.clone();
    server
        .mixer_mut()
        .get_bus_mut(idx_of("AudioEffectPanner"))
        .expect("Panner bus")
        .effects_mut()
        .process(&mut panned, SR, 2);
    let right_peak = panned.chunks(2).map(|f| f[1].abs()).fold(0.0f32, f32::max);
    assert!(
        right_peak < 1e-6,
        "authored pan = -1 must silence the right channel exactly, got \
         {right_peak:.2e}. A non-zero peak with the dry signal intact means the \
         authored value was discarded and the default 0 used"
    );

    // Phaser authored `depth = 0`, which is its neutral: the allpass output is
    // summed at zero weight, so the dry signal must return untouched. The
    // DEFAULT depth is 1.0, so this also proves the 0 was read.
    let mut phased = dry.clone();
    server
        .mixer_mut()
        .get_bus_mut(idx_of("AudioEffectPhaser"))
        .expect("Phaser bus")
        .effects_mut()
        .process(&mut phased, SR, 2);
    assert!(
        max_abs_diff(&dry, &phased) < 1e-6,
        "authored depth = 0 must leave the signal unchanged; a difference means \
         the default 1.0 was used instead of the authored value"
    );

    // The ACTIVE half, for the same reason as Chorus below: a neutral-only
    // assertion cannot tell an identity transform from an effect that is never
    // invoked.
    let mut phaser_on = dry.clone();
    server
        .mixer_mut()
        .get_bus_mut(on_phaser)
        .expect("PhaserOn bus")
        .effects_mut()
        .process(&mut phaser_on, SR, 2);
    assert!(
        max_abs_diff(&dry, &phaser_on) > 1e-3,
        "at Godot's defaults (depth 1.0, feedback 0.7) the Phaser must colour \
         the signal; an unchanged buffer means it never processed"
    );

    // Chorus authored `wet = 0` with dry defaulting to 1: identity. Again the
    // default wet is 0.5, so an unchanged signal proves the 0 arrived.
    let mut chorused = dry.clone();
    server
        .mixer_mut()
        .get_bus_mut(idx_of("AudioEffectChorus"))
        .expect("Chorus bus")
        .effects_mut()
        .process(&mut chorused, SR, 2);
    assert!(
        max_abs_diff(&dry, &chorused) < 1e-6,
        "authored wet = 0 must return the dry input; a difference means a voice \
         is mixed in without consulting wet, or the default 0.5 was used"
    );

    // 🔑 The ACTIVE half, on a second bus carrying a default Chorus. Without
    // this, the neutral assertion above is satisfied by an effect that is in the
    // slot but never processed at all — identity and absent look identical.
    let mut chorus_on = dry.clone();
    server
        .mixer_mut()
        .get_bus_mut(on_chorus)
        .expect("ChorusOn bus")
        .effects_mut()
        .process(&mut chorus_on, SR, 2);
    assert!(
        max_abs_diff(&dry, &chorus_on) > 1e-3,
        "at Godot's defaults (two detuned voices at wet 0.5) the Chorus must \
         colour the signal; an unchanged buffer means the effect is in the slot \
         but its voice loop never ran"
    );

    // StereoEnhance authored `pan_pullout = 2` widens about the mid.
    // ⚠️ A genuinely stereo fixture: with both channels equal the mid IS the
    // signal, (x - center) is zero, and widening is a no-op for ANY pullout — a
    // mono-equal input here would pass while measuring nothing.
    let mut wide: Vec<f32> = Vec::with_capacity(2048);
    for i in 0..1024 {
        let s = (std::f32::consts::TAU * TONE_HZ * i as f32 / SR as f32).sin();
        wide.push(s);
        wide.push(-s * 0.5);
    }
    let wide_before = wide.clone();
    server
        .mixer_mut()
        .get_bus_mut(idx_of("AudioEffectStereoEnhance"))
        .expect("Stereo bus")
        .effects_mut()
        .process(&mut wide, SR, 2);
    assert!(
        max_abs_diff(&wide_before, &wide) > 1e-3,
        "authored pan_pullout = 2 must widen a genuinely stereo signal; no \
         change means the default 1.0 (an identity) was used"
    );

    // -- (3b) 🔑 PitchShift moves the tone up an octave ---------------------
    //
    // ⚠️ MEASURED, not assumed. I first wrote `dominant == 880 Hz within one
    // bin` and simulated the implementation: it is false, and the test would
    // have been red forever. Two reasons, both inherent to the declared
    // deviation (a two-tap crossfaded variable delay, not Godot's phase
    // vocoder):
    //
    //  1. Most energy sits in a **sideband pair** at 862 / 909 Hz — the two
    //     taps read a fixed 1024 samples apart, so crossfading them amplitude-
    //     modulates at the grain rate (48000/2048 = 23.44 Hz) and suppresses
    //     the carrier. The 880 Hz bin itself holds very little.
    //  2. The pair centres on ~885 Hz, not 880: each grain boundary jumps the
    //     read position by a whole window, which at 440 Hz is 18.77 cycles —
    //     the 0.23-cycle remainder is a phase step every grain, worth ~+5 Hz.
    //
    // Simulated dominant: 863.5 Hz = 1.9625x the input (-33 cents). So the
    // assertions below are a **ratio within 5%** and a **band-energy** compare,
    // both of which hold with margin while still failing for every wrong value.
    // Do not tighten either to an exact 880: that is not what this resampler
    // produces, and the deviation is documented on the implementation.
    let frames = 16_384usize;
    let mut mono: Vec<f32> = (0..frames)
        .map(|i| (std::f32::consts::TAU * TONE_HZ * i as f32 / SR as f32).sin())
        .collect();
    server
        .mixer_mut()
        .get_bus_mut(idx_of("AudioEffectPitchShift"))
        .expect("Pitch bus")
        .effects_mut()
        .process(&mut mono, SR, 1);

    // ⚠️ Discard the warm-up: for the first window the taps read a ring that is
    // still mostly zeros, so the opening is quiet and analysing it would measure
    // silence rather than pitch. The default fft_size of 3 is a 2048 window and
    // a tap reads up to one whole window back, so two windows is the safe skip.
    let tail = &mono[4096..];
    let bin = SR as f32 / tail.len() as f32;

    // Clause 1: the dominant component sits near 2x, nowhere near 1x.
    let peak = dominant_freq(tail, TONE_HZ * 0.4, TONE_HZ * 3.0, bin);
    let ratio = peak / TONE_HZ;
    assert!(
        (ratio - 2.0).abs() <= 0.1,
        "authored pitch_scale = 2.0 must put the dominant component within 5% \
         of {} Hz; got {peak:.1} Hz ({ratio:.4}x). Simulation says 1.9625x, so \
         this has 2.7x of margin. A ratio near 1.0 means the authored scale was \
         discarded and the default 1.0 used, which is a documented exact \
         pass-through",
        TONE_HZ * 2.0
    );

    // Clause 2 — the differential half. Clause 1 alone would also pass for an
    // effect that merely added noise near 880; this one says the energy MOVED:
    // the octave band must dominate the fundamental band. Simulated ratio is
    // 8306x at pitch_scale 2.0 and 0.0002x at the default 1.0, so 100x is a
    // threshold neither case can straddle.
    let fundamental = band_energy(tail, TONE_HZ * 0.9, TONE_HZ * 1.1);
    let octave = band_energy(tail, TONE_HZ * 1.8, TONE_HZ * 2.2);
    assert!(
        octave > fundamental * 100.0,
        "the octave band must carry at least 100x the fundamental band's energy \
         (got octave {octave:.3e} vs fundamental {fundamental:.3e}). Comparable \
         energy in both means the tone was passed through and something was \
         added on top, rather than resampled"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
