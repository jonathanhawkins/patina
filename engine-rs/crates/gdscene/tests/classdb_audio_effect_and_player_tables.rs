//! pat-s3rrv.196 acceptance: the audio ClassDB tables, and `finished` reaching a
//! connected script when a stream ends.
//!
//! ⓘ **Measured before writing: all three acceptance clauses were ALREADY
//! implemented.** `AudioStreamPlayer`/2D/3D each declare `finished` and carry
//! 10/13/22 properties (not the "6 of 11" and "11 of 22" the bead reports), and
//! `AudioEffectReverb` registers all 8 parameters with values matching
//! `doc/classes/AudioEffectReverb.xml` exactly. What was missing is that nothing
//! pinned any of it — this file is that pin.
//!
//! 🔑 **The signal half is asserted end-to-end, not by introspection.** A
//! declared signal and a reachable `emit_signal` call are two different claims
//! from "a connected handler fires", and this subsystem has burned exactly that
//! distinction before: `audio_player_seek_pause_position_test.rs`'s own header
//! records that the interpreter wrote `seek_request`/`stream_paused` while
//! `sync_player_node` consumed neither, "so every one of those script APIs
//! silently no-opped behind green tests". The route here is
//! `MainLoop::step` -> `audio_bridge.process` (`main_loop.rs:4106`) ->
//! `sync_player_node` (`:353`) -> `emit_signal(id, "finished")` (`:963`), gated
//! on the voice running out. Driving a real buffer to exhaustion is what tests
//! that route rather than asserting the pieces exist.

use gdaudio::AudioBuffer;
use gdobject::class_db::{class_exists, class_has_signal, get_property_list, register_all_classes};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const SAMPLE_RATE: u32 = 44100;

/// A deliberately SHORT stream — 0.1 s — so a handful of frames exhausts it.
///
/// ⚠️ The emission is gated on the voice dying (`!voice_alive` in
/// `sync_player_node`), so the fixture has to actually run the buffer out. A
/// one-second buffer would need 60+ steps and, if the mixer ever changed its
/// block size, would silently stop reaching the end — a short buffer keeps the
/// test's premise robust.
fn short_stream() -> AudioBuffer {
    let frames = (SAMPLE_RATE / 10) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f32 / SAMPLE_RATE as f32;
        samples.push(t);
        samples.push(t);
    }
    AudioBuffer {
        samples,
        sample_rate: SAMPLE_RATE,
        channels: 2,
    }
}

/// A listener that counts `finished` and proves it kept running.
///
/// 🔑 The "did anything happen at all" guard is the PLAYER's `playing` flag, not
/// a `_process` tick counter. `sync_player_node` clears `playing` and emits
/// `finished` from the same branch, so `playing == false` proves that branch ran
/// — and `hits == 0` alongside it then means precisely "the emission did not
/// reach the script", not "the stream never ended". A `_process`-based counter
/// would have coupled this test to the frame's script phase, which
/// `MainLoop::step` drives through its own path rather than
/// `process_all_scripts_process`.
const LISTENER_GD: &str = "\
extends Node
var hits = 0
func _on_finished():
    self.hits = self.hits + 1
";

fn script_int(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    let v = ml
        .tree()
        .get_script(node)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == name)
                .map(|p| p.default_value)
        })
        .unwrap_or(Variant::Nil);
    match v {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("{name} should be an int, got {other:?}"),
    }
}

/// **The acceptance test.** ClassDB tables, plus `finished` reaching a script.
#[test]
fn classdb_audio_effect_and_player_tables() {
    register_all_classes();

    // -- clause 2: every player declares `finished` -------------------------
    for class in [
        "AudioStreamPlayer",
        "AudioStreamPlayer2D",
        "AudioStreamPlayer3D",
    ] {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            class_has_signal(class, "finished"),
            "{class}.has_signal(\"finished\") must be true — a script cannot connect \
             to a signal the class does not declare"
        );
    }

    // -- clause 3: AudioEffectReverb's 8 parameters, with Godot's defaults ---
    //
    // Values transcribed from `doc/classes/AudioEffectReverb.xml`. Asserting the
    // NAMES alone would pass on a table of eight wrong numbers, and the defaults
    // are what the bus-effect inspector renders as slider positions.
    let reverb: Vec<_> = get_property_list("AudioEffectReverb");
    let expected: [(&str, f64); 8] = [
        ("predelay_msec", 150.0),
        ("predelay_feedback", 0.4),
        ("room_size", 0.8),
        ("damping", 0.5),
        ("spread", 1.0),
        ("hipass", 0.0),
        ("dry", 1.0),
        ("wet", 0.5),
    ];
    for (name, want) in expected {
        let p = reverb
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("AudioEffectReverb must declare `{name}`"));
        match p.default_value {
            Variant::Float(f) => assert!(
                (f - want).abs() < 1e-6,
                "AudioEffectReverb.{name} default should be {want}, got {f}"
            ),
            ref other => panic!("AudioEffectReverb.{name} should be a Float, got {other:?}"),
        }
    }
    // ⚠️ Godot declares exactly these eight OF ITS OWN — but `get_property_list`
    // walks the inheritance chain, so `AudioEffectReverb` also reports
    // `Resource`'s three (`resource_local_to_scene`, `resource_name`,
    // `resource_path`). Subtracting the base is what makes "exactly eight"
    // checkable at all; a bare `len() == 8` asserts the class has no ancestors.
    let base: std::collections::HashSet<String> = get_property_list("Resource")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let own: Vec<&String> = reverb
        .iter()
        .map(|p| &p.name)
        .filter(|n| !base.contains(*n))
        .collect();
    assert_eq!(
        own.len(),
        8,
        "AudioEffectReverb declares exactly 8 parameters of its own upstream; a \
         ninth would be one the bus-effect inspector renders and the mixer \
         ignores. Got {own:?}"
    );

    // -- clause 1: a connected script receives `finished` --------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let listener = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    let player = tree
        .add_child(root, Node::new("Player", "AudioStreamPlayer"))
        .unwrap();
    tree.attach_script(
        listener,
        Box::new(
            GDScriptNodeInstance::from_source(LISTENER_GD, listener).expect("listener parses"),
        ),
    );
    tree.connect_signal(
        player,
        "finished",
        gdobject::signal::Connection::new(listener.object_id(), "_on_finished"),
    );
    // Godot's AudioStreamPlayer starts on `playing = true`; the bridge reads the
    // node property, so the fixture sets it the way a `.tscn` would.
    if let Some(n) = tree.get_node_mut(player) {
        n.set_property("playing", Variant::Bool(true));
    }

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut()
        .set_stream_buffer(player, short_stream());

    // Well past the 0.1 s buffer.
    for _ in 0..40 {
        ml.step(1.0 / 60.0);
    }

    // The emitting branch ran: `sync_player_node` clears `playing` and emits
    // `finished` together, so this separates "the stream never ended" from "the
    // signal never arrived".
    assert!(
        matches!(
            ml.tree()
                .get_node(player)
                .map(|n| n.get_property("playing")),
            Some(Variant::Bool(false))
        ),
        "the stream must have ended — otherwise `hits` below is 0 because nothing \
         finished, not because the signal failed to reach the script"
    );
    assert_eq!(
        script_int(&ml, listener, "hits"),
        1,
        "a script connected to AudioStreamPlayer.finished must be called exactly \
         once when the stream ends — 0 means the signal never reached it, and >1 \
         means `sync_player_node` re-emitted for an already-stopped player"
    );
}

/// The player stops itself when the stream ends.
///
/// The signal and the `playing` flag are written by the same branch, so this
/// pins the half a script observes by polling rather than by connecting.
#[test]
fn a_finished_player_reports_itself_stopped() {
    register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = tree
        .add_child(root, Node::new("Player", "AudioStreamPlayer"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(player) {
        n.set_property("playing", Variant::Bool(true));
    }
    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut()
        .set_stream_buffer(player, short_stream());
    for _ in 0..40 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        matches!(
            ml.tree()
                .get_node(player)
                .map(|n| n.get_property("playing")),
            Some(Variant::Bool(false))
        ),
        "an exhausted stream must clear `playing`"
    );
}
