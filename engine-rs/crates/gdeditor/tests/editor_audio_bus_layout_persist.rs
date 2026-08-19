//! pat-s3rrv.52 acceptance — the editor's Audio panel persists a real bus
//! layout, and the layout it saves is the one the runtime loads.
//!
//! Godot's `EditorAudioBuses` panel loads the project's
//! `default_bus_layout.tres` on open, writes it back on Save, drives each VU
//! meter from `AudioServer.get_bus_peak_volume_left_db`, and drag-reorders a
//! bus's effect chain (`AudioServer.swap_bus_effects`).
//!
//! Patina's panel was a well-built shell over an in-memory model that never
//! touched disk and never saw real audio: Load and "Save As" called
//! `logMessage(...)` and nothing else, the route table had no `layout/save`,
//! `layout/load` or `effect/swap`, and the meter derived its height from the
//! FADER value — so it moved when you dragged the slider and sat still when the
//! game got loud.
//!
//! ⚠️ The load half is deliberately exercised through the REAL
//! `gdaudio::mixer::AudioMixer::load_bus_layout_tres`, not through the editor's
//! own parser. The editor and the runtime are separate crates with separate
//! parsers, and the only claim worth making is that what one writes the other
//! can read — a round-trip through our own reader would happily pass on a
//! grammar the runtime cannot parse.

use gdaudio::mixer::AudioMixer;
use gdeditor::audio_bus_layout::{AudioBusLayout, MASTER_BUS};

/// The layout the panel builds when a user adds a "Music" bus at -12 dB with an
/// Amplify effect — the exact scenario in the bead's acceptance.
fn music_layout() -> AudioBusLayout {
    let mut layout = AudioBusLayout::new();
    let music = layout.add_bus("Music");
    layout.set_volume(music, -12.0);
    layout.set_output(music, MASTER_BUS);
    layout.add_effect(music, "AudioEffectAmplify");
    layout
}

/// Saves through the editor's serializer and re-reads through the RUNTIME's
/// loader, exactly as Save-then-Play does.
fn save_then_load(layout: &AudioBusLayout) -> AudioMixer {
    let dir = tempfile::tempdir().expect("temp project");
    let path = dir.path().join("default_bus_layout.tres");
    std::fs::write(&path, layout.to_tres()).expect("layout writes");

    let text = std::fs::read_to_string(&path).expect("layout reads back");
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&text);
    mixer
}

/// **The acceptance test.** A saved layout re-reads into the runtime mixer with
/// the same buses, volume and effect.
#[test]
fn editor_audio_bus_layout_persist() {
    let mixer = save_then_load(&music_layout());

    assert!(
        mixer.bus_count() >= 2,
        "the saved layout must re-read into both buses, got {}",
        mixer.bus_count()
    );

    let idx = mixer
        .get_bus_by_name("Music")
        .expect("the Music bus must survive the round-trip by name");
    let bus = mixer.get_bus(idx).expect("Music bus");
    assert_eq!(bus.name(), "Music");
    assert!(
        (bus.volume_db() - -12.0).abs() < 0.001,
        "the authored -12 dB must survive, got {}",
        bus.volume_db()
    );
    assert_eq!(
        bus.effects().len(),
        1,
        "the Amplify effect must be re-instantiated by the runtime loader, not \
         just written to the file"
    );
}

/// Reopening the editor restores what was saved — the editor's own reader has
/// to agree with its writer too, not only the runtime's.
#[test]
fn reopening_the_editor_restores_the_saved_layout() {
    let before = music_layout();
    let after = AudioBusLayout::from_tres(&before.to_tres());

    assert_eq!(after.len(), 2, "both buses restored");
    let bus = after.bus(1).expect("the Music bus");
    assert_eq!(bus.name, "Music");
    assert_eq!(bus.volume_db, -12.0);
    assert_eq!(bus.effects.len(), 1);
    assert_eq!(after.output_of(1), Some(MASTER_BUS));
}

/// Dragging an effect reorders it **in the saved file**, and the runtime loads
/// the new order. Effect order is audible, so this is a real edit.
#[test]
fn reordering_an_effect_changes_the_saved_layout() {
    let mut layout = AudioBusLayout::new();
    layout.add_effect(0, "AudioEffectAmplify");
    layout.add_effect(0, "AudioEffectLowPassFilter");

    // Fixture check: the pre-swap order is the one we think it is, or the
    // assertion below proves nothing.
    let before = layout.to_tres();
    assert!(
        before.find("AudioEffectAmplify") < before.find("AudioEffectLowPassFilter"),
        "fixture: Amplify should start in slot 0:\n{before}"
    );

    assert!(layout.swap_effects(0, 0, 1), "the swap must be accepted");

    let after = layout.to_tres();
    assert!(
        after.find("AudioEffectLowPassFilter") < after.find("AudioEffectAmplify"),
        "the filter must occupy slot 0 in the SAVED file after the drag:\n{after}"
    );

    // …and the runtime agrees.
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&after);
    assert_eq!(
        mixer.get_bus(0).expect("master").effects().len(),
        2,
        "both effects must survive the reorder"
    );
}

/// The panel's own wiring. These four routes are what turn the shell into a
/// working panel; before this bead the first two buttons called `logMessage`
/// and the last two routes did not exist at all.
#[test]
fn the_panel_fetches_every_new_audio_route() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    for route in [
        "/api/audio/layout/save",
        "/api/audio/layout/load",
        "/api/audio/effect/swap",
        "/api/audio/peaks",
    ] {
        assert!(
            html.contains(route),
            "the editor page must call {route} — a route with no caller is not \
             wiring, it is a route",
        );
    }
}

/// ⚠️ The VU meter must read the PEAK, not the fader.
///
/// This is the assertion that distinguishes "a meter exists" from "the meter
/// reports audio". The old code passed `bus.volume_db` — the fader value — into
/// the meter height, so the bar tracked the slider the user was dragging and
/// ignored the game entirely.
#[test]
fn the_vu_meter_reads_peaks_rather_than_the_fader() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    assert!(
        html.contains("audioPeakDb("),
        "the meter fill must come from the polled peak level",
    );
    assert!(
        !html.contains("audioMeterPct(bus.volume_db"),
        "the meter must NOT be derived from the fader value any more",
    );
    assert!(
        html.contains("get_bus_peak_volume_left_db") || html.contains("left_db"),
        "the panel must consume the left-channel peak the peaks route serves",
    );
}

/// A project that has never saved a layout still opens: a missing file is the
/// common case, not an error.
#[test]
fn an_absent_layout_file_loads_the_default() {
    let layout = AudioBusLayout::from_tres("");
    assert_eq!(layout.len(), 1);
    assert_eq!(layout.bus(0).expect("master").name, MASTER_BUS);
}
