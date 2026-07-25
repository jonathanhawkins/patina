//! pat-0q2kx.56 acceptance: a project's `default_bus_layout.tres` reaches the
//! mixer with its bus table AND its effect chains.
//!
//! Godot ground truth: `AudioServer::load_default_bus_layout()`
//! (`servers/audio/audio_server.cpp:1626`) reads the `.tres` named by the
//! project setting `audio/buses/default_bus_layout` at startup;
//! `doc/classes/AudioBusLayout.xml` documents the per-bus fields and the
//! sub-resource effect chain.
//!
//! `AudioMixer::load_bus_layout` existed and was tested, but had ZERO callers
//! anywhere outside `gdaudio` and dropped effect fields entirely — so every
//! project booted with a bare Master bus, and a player naming another bus fell
//! back to Master without a word. This covers the parsing half; the runner's
//! boot call is wired in `patina-runner/src/bootstrap.rs`.

use gdaudio::mixer::AudioMixer;

/// The layout from the bead's acceptance: Music at -10 dB sending to Master,
/// with an Amplify(+6 dB) insert.
const LAYOUT: &str = r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="AudioEffectAmplify" id="Amplify_1"]
volume_db = 6.0

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Music"
bus/1/volume_db = -10.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("Amplify_1")
bus/1/effect/0/enabled = true
bus/2/name = "SFX"
bus/2/volume_db = -3.0
bus/2/send = "Master"
"#;

#[test]
fn audio_bus_layout_boot_load() {
    let mut mixer = AudioMixer::new();
    let count = mixer.load_bus_layout_tres(LAYOUT);

    assert_eq!(count, 3, "the layout declares three buses");
    assert_eq!(mixer.bus_count(), 3);

    // Buses are addressable by their authored names, not just by index.
    let music = mixer
        .get_bus_by_name("Music")
        .expect("the Music bus must exist by name");
    let sfx = mixer
        .get_bus_by_name("SFX")
        .expect("the SFX bus must exist by name");
    assert_eq!(mixer.get_bus_by_name("Master"), Some(0));

    let music_bus = mixer.get_bus(music).expect("Music bus");
    assert_eq!(music_bus.volume_db(), -10.0);
    assert_eq!(music_bus.send(), "Master");

    assert_eq!(mixer.get_bus(sfx).expect("SFX bus").volume_db(), -3.0);

    // ⚠️ The effect chain is the part that used to be dropped silently: the
    // buses would load and the inserts would vanish.
    assert_eq!(
        mixer.get_bus(music).expect("Music bus").effects().len(),
        1,
        "the Amplify insert must reach the Music bus"
    );
    assert_eq!(
        mixer.get_bus(0).expect("Master bus").effects().len(),
        0,
        "no effect was declared on Master"
    );
}

/// ⚠️ A disabled slot must NOT be inserted. `EffectChain` has no per-slot
/// enable flag, so inserting it anyway would colour the bus exactly as if the
/// author had left it enabled.
#[test]
fn a_disabled_effect_slot_is_not_inserted() {
    let layout = LAYOUT.replace(
        "bus/1/effect/0/enabled = true",
        "bus/1/effect/0/enabled = false",
    );
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&layout);

    let music = mixer.get_bus_by_name("Music").expect("Music bus");
    assert_eq!(
        mixer.get_bus(music).expect("Music bus").effects().len(),
        0,
        "a slot with enabled = false must be skipped"
    );
}

/// An effect slot with no `enabled` key defaults to enabled, as Godot does.
#[test]
fn an_effect_slot_without_an_enabled_key_defaults_to_enabled() {
    let layout = LAYOUT.replace("bus/1/effect/0/enabled = true\n", "");
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&layout);

    let music = mixer.get_bus_by_name("Music").expect("Music bus");
    assert_eq!(mixer.get_bus(music).expect("Music bus").effects().len(), 1);
}

/// An effect type Patina has not implemented must not cost the project its bus
/// layout — the slot is skipped and everything else still loads.
#[test]
fn an_unknown_effect_type_is_skipped_without_losing_the_buses() {
    let layout = LAYOUT.replace(
        r#"[sub_resource type="AudioEffectAmplify" id="Amplify_1"]"#,
        r#"[sub_resource type="AudioEffectPitchShift" id="Amplify_1"]"#,
    );
    let mut mixer = AudioMixer::new();
    let count = mixer.load_bus_layout_tres(&layout);

    assert_eq!(count, 3, "the buses must still load");
    let music = mixer.get_bus_by_name("Music").expect("Music bus");
    assert_eq!(
        mixer.get_bus(music).expect("Music bus").effects().len(),
        0,
        "the unmodelled effect is skipped"
    );
}

/// A project with no layout file at all boots with Master only — the runner
/// treats a missing `.tres` as "nothing to load", not as a boot failure.
#[test]
fn an_empty_layout_leaves_the_default_master_bus() {
    let mut mixer = AudioMixer::new();
    let count = mixer.load_bus_layout_tres("[gd_resource type=\"AudioBusLayout\"]\n");
    assert_eq!(count, 1);
    assert_eq!(mixer.get_bus_by_name("Master"), Some(0));
}
