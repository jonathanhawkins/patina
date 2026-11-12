//! A bus layout's Delay and SpectrumAnalyzer reach the mixer with their
//! authored properties (pat-mgixk.84).
//!
//! # What was broken
//!
//! `effect_from_sub_resource` (`gdaudio/src/mixer.rs`) matched Amplify,
//! Capture, Record, Reverb, the filter family, Compressor and the limiters, and
//! returned `None` for everything else. The DSP for Delay, Distortion, EQ and
//! SpectrumAnalyzer already existed and was unit-tested in `effects.rs` —
//! nothing could *name* those classes from a layout. So a
//! `default_bus_layout.tres` carrying an `AudioEffectSpectrumAnalyzer` (the
//! standard visualizer setup) silently dropped the slot, and
//! `AudioServer.get_bus_effect_instance` had no analyzer to hand back. The
//! Reverb arm was `AudioEffectReverb::new()`, discarding every authored field.
//!
//! # Why this boots through the runner
//!
//! The wiring this bead names is `bootstrap.rs` loading
//! `audio/buses/default_bus_layout` **before the first frame** → mixer →
//! `AudioServer::mix`. That loader already existed
//! (`bootstrap.rs:600-623`, `load_bus_layout_tres`); only the factory behind it
//! was incomplete. So the fixture is a real project directory read by the real
//! boot flow, not a hand-built mixer — a test that called the factory directly
//! could not tell whether a *project* gets its effects.
//!
//! # The three assertions, and why each is needed
//!
//! 1. **Instantiation.** Both classes are present in their slots. This is the
//!    one that fails before the change: the factory answered `None`, the slot
//!    was dropped, and `effect_at(0)` was `None` — so the assertion sees
//!    `None`, not a wrong class.
//! 2. 🔑 **The authored delay is honoured, not just the class.** The fixture
//!    authors `tap1_delay_ms = 100.0` while Godot's default is `250.0`. An
//!    impulse must come back at 100 ms. Asserting only "a Delay exists" would
//!    pass against an arm that built `AudioEffectDelay::new()` and threw the
//!    fields away — exactly the bug the Reverb arm had.
//! 3. **The analyzer actually analyses.** A magnitude query over the band
//!    containing the test tone must be non-zero, which it cannot be unless the
//!    analyzer both exists and received the mix.
//!
//! ⚠️ Deliberately NOT asserted: `tap1_pan`/`tap2_pan`, `keep_hf_hz`,
//! `fft_size`/`tap_back_pos` and Reverb's `damping`/`hipass`/`spread` have no
//! counterpart in this DSP, so there is nothing to observe. Asserting a default
//! for them would pin behaviour the engine does not have. They belong to the
//! audio-effect-dsp-parity bead.

use std::path::PathBuf;

use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// Sample rate the DSP is driven at. 48 kHz makes the 100 ms tap land on an
/// exact sample index (4800), so the echo assertion needs no rounding slack.
const SR: u32 = 48_000;

/// Authored first-tap delay. ⚠️ Deliberately NOT Godot's default of 250 ms —
/// that difference is what proves the field was read rather than defaulted.
const TAP1_MS: f32 = 100.0;

/// Godot's documented default, for the negative half of assertion 2.
const GODOT_DEFAULT_TAP1_MS: f32 = 250.0;

/// Tone fed through the analyzer, and the band the magnitude is queried over.
const TONE_HZ: f32 = 440.0;

const PROJECT_GODOT: &str = r#"[application]

config/name="BusLayoutEffects"

[audio]

buses/default_bus_layout="res://default_bus_layout.tres"
"#;

/// Master carries the analyzer, `Fx` carries the delay — the arrangement the
/// bead's acceptance names.
///
/// `dry = 0.0` leaves the tap alone in the output, so the impulse response is
/// the echo and nothing else. `tap2_active` and `feedback_active` are off for
/// the same reason: with them on, the output has three spikes and "the peak" is
/// no longer a single well-defined index.
const BUS_LAYOUT: &str = r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="AudioEffectSpectrumAnalyzer" id="SA_1"]
buffer_length = 0.5

[sub_resource type="AudioEffectDelay" id="DL_1"]
dry = 0.0
tap1_active = true
tap1_delay_ms = 100.0
tap1_level_db = 0.0
tap2_active = false
feedback_active = false

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/0/effect/0/effect = SubResource("SA_1")
bus/0/effect/0/enabled = true
bus/1/name = "Fx"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("DL_1")
bus/1/effect/0/enabled = true
"#;

/// A fresh project directory holding the two fixture files.
fn write_project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "patina-bus-layout-effects-{}-{}",
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

/// Index of the largest absolute sample, and its value.
fn peak(samples: &[f32]) -> (usize, f32) {
    let mut best = (0usize, 0.0f32);
    for (i, s) in samples.iter().enumerate() {
        if s.abs() > best.1 {
            best = (i, s.abs());
        }
    }
    best
}

/// **The acceptance test.**
#[test]
fn audio_bus_layout_instantiates_all_effects() {
    let dir = write_project();

    // The real boot flow: Core → Servers → … → Running, which is where
    // `bootstrap.rs` reads `audio/buses/default_bus_layout` and hands the text
    // to `AudioMixer::load_bus_layout_tres`.
    let mut boot = EngineBootstrap::new(BootConfig::headless().project_dir(&dir));
    boot.run_all().expect("boot flow should complete");
    let main_loop = boot
        .main_loop_mut()
        .expect("the Running phase creates a MainLoop");

    let server = main_loop.audio_bridge_mut().server_mut();

    // -- 0. the layout was read at all --------------------------------------
    //
    // Checked first: if the buses are missing, every assertion below fails for
    // a reason that has nothing to do with the effect factory — a bad fixture
    // path reads exactly like a dropped effect otherwise.
    let master = server
        .mixer()
        .get_bus_by_name("Master")
        .expect("the layout declares a Master bus");
    let fx = server.mixer().get_bus_by_name("Fx").expect(
        "the layout declares an 'Fx' bus; missing means bootstrap never loaded \
             default_bus_layout.tres, not that the effect was dropped",
    );

    // -- 1. both effects were INSTANTIATED ----------------------------------
    assert_eq!(
        server.bus_effect_class(master, 0),
        Some("AudioEffectSpectrumAnalyzer"),
        "Master's slot 0 must hold a SpectrumAnalyzer. `None` means \
         effect_from_sub_resource returned None and the slot was dropped — the \
         standard visualizer setup silently producing nothing"
    );
    assert_eq!(
        server.bus_effect_class(fx, 0),
        Some("AudioEffectDelay"),
        "Fx's slot 0 must hold a Delay. `None` means the class could not be \
         named from a bus layout even though its DSP exists"
    );

    // -- 2. 🔑 the AUTHORED tap delay is honoured ----------------------------
    //
    // An impulse through the Fx chain. With `dry = 0` and one tap, the output is
    // a single spike whose index is the tap delay in samples.
    let back = ((TAP1_MS / 1000.0) * SR as f32).round() as usize;
    let default_back = ((GODOT_DEFAULT_TAP1_MS / 1000.0) * SR as f32).round() as usize;
    let mut impulse = vec![0.0f32; default_back * 2];
    impulse[0] = 1.0;
    server
        .mixer_mut()
        .get_bus_mut(fx)
        .expect("Fx bus")
        .effects_mut()
        .process(&mut impulse, SR, 1);

    let (peak_idx, peak_val) = peak(&impulse);
    assert!(
        peak_val > 0.5,
        "the impulse must come back as an echo (peak {peak_val:.3} at {peak_idx}). \
         A flat buffer means the Delay is present but processed nothing"
    );
    assert_eq!(
        peak_idx, back,
        "the echo must land at the AUTHORED {TAP1_MS} ms ({back} samples at \
         {SR} Hz). Finding it at {default_back} means the arm built \
         AudioEffectDelay::new() and discarded the authored fields — a Delay \
         that exists but ignores the project"
    );

    // -- 3. the analyzer analyses -------------------------------------------
    //
    // Drive the Master chain with the tone, then query the band around it.
    // ⚠️ The query has to come AFTER processing: the analyzer reports on what it
    // has seen, so querying a fresh one would answer zero for a working
    // analyzer and the assertion would be inverted.
    let mut tone: Vec<f32> = (0..SR as usize / 4)
        .map(|i| (std::f32::consts::TAU * TONE_HZ * i as f32 / SR as f32).sin())
        .collect();
    server
        .mixer_mut()
        .get_bus_mut(master)
        .expect("Master bus")
        .effects_mut()
        .process(&mut tone, SR, 1);

    let magnitude = server
        .bus_effect_magnitude("Master", 0, TONE_HZ - 140.0, TONE_HZ + 160.0)
        .expect(
            "a SpectrumAnalyzer in the slot must answer a magnitude query; None \
             means the effect is not an analyzer at all",
        );
    assert!(
        magnitude > 0.0,
        "the analyzer must report non-zero magnitude in the band containing the \
         {TONE_HZ} Hz tone, got {magnitude}. Zero means it was instantiated but \
         never received the mix"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
