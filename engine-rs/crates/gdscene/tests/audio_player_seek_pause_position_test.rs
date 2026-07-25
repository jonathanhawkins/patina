//! pat-0q2kx.5 acceptance: `seek` / `stream_paused` / `playback_position` /
//! `pitch_scale` actually reach the mixed output, and a tree pause silences a
//! player without losing its position.
//!
//! The interpreter already wrote `seek_request` / `stream_paused` and read
//! `playback_position` node properties, but `AudioServerBridge::sync_player_node`
//! consumed none of them — so every one of those script APIs silently no-opped
//! behind green tests. These tests assert against the **mixed samples**, not the
//! node properties, so a regression to "writes a property nobody reads" fails.
//!
//! Fixture: a 1-second ramp stream whose sample value equals its timestamp in
//! seconds (`sample[t] == t`). That makes the mix self-describing — a mixed
//! sample of ~0.5 means the playhead is at ~0.5 s — so position, seeking and
//! pitch are all observable in the output rather than inferred.

use gdaudio::AudioBuffer;
use gdscene::node::{Node, ProcessMode};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const SAMPLE_RATE: u32 = 44100;

/// A 1-second stereo ramp where every sample equals its own timestamp.
fn ramp_stream() -> AudioBuffer {
    let frames = SAMPLE_RATE as usize;
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

/// Builds `Main` (script) -> `Player` (AudioStreamPlayer) with the ramp loaded.
fn scene_with_player(script_src: &str) -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let player_id = tree
        .add_child(main_id, Node::new("Player", "AudioStreamPlayer"))
        .unwrap();

    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut()
        .set_stream_buffer(player_id, ramp_stream());
    (ml, main_id, player_id)
}

fn script_var(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Mean absolute amplitude of the last mixed frame — 0 means silence.
fn mix_level(ml: &MainLoop) -> f32 {
    let mix = ml.audio_bridge().last_mix();
    if mix.is_empty() {
        return 0.0;
    }
    mix.iter().map(|s| s.abs()).sum::<f32>() / mix.len() as f32
}

/// First sample of the last mixed frame — with the ramp fixture this *is* the
/// playhead position in seconds at the start of the frame.
fn mix_first(ml: &MainLoop) -> f32 {
    ml.audio_bridge().last_mix().first().copied().unwrap_or(0.0)
}

fn node_f64(ml: &MainLoop, node: gdscene::node::NodeId, prop: &str) -> f64 {
    match ml.tree().get_node(node).unwrap().get_property(prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{prop} should be numeric, got {other:?}"),
    }
}

/// **The acceptance test** — all five clauses (a)–(e) of pat-0q2kx.5, driven
/// from GDScript through `MainLoop::step`.
#[test]
fn audio_player_seek_pause_position_wiring() {
    // The script drives the whole sequence off a frame counter so each clause is
    // exercised through the real interpreter dispatch, not by poking properties.
    // (`+=`/`elif` avoided for parser-independence, matching the sibling tests.)
    let src = "\
extends Node
var frame = 0
var pos_f2 = 0.0
var pos_f3 = 0.0
var playing_while_paused = false
func _process(delta):
    self.frame = self.frame + 1
    var p = get_node(\"Player\")
    if self.frame == 1:
        p.play()
    if self.frame == 2:
        self.pos_f2 = p.get_playback_position()
    if self.frame == 3:
        self.pos_f3 = p.get_playback_position()
        p.set_stream_paused(true)
    if self.frame == 5:
        self.playing_while_paused = p.is_playing()
    if self.frame == 6:
        p.set_stream_paused(false)
";
    let (mut ml, main_id, player_id) = scene_with_player(src);

    // Frame 1: play() starts the voice; audio is audible.
    ml.step(1.0 / 60.0);
    assert!(
        mix_level(&ml) > 0.0,
        "frame 1: play() must produce a non-silent mix"
    );

    // (c) position advances between frames, read from GDScript.
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    let p2 = match script_var(&ml, main_id, "pos_f2") {
        Some(Variant::Float(f)) => f,
        other => panic!("pos_f2 should be a float, got {other:?}"),
    };
    let p3 = match script_var(&ml, main_id, "pos_f3") {
        Some(Variant::Float(f)) => f,
        other => panic!("pos_f3 should be a float, got {other:?}"),
    };
    assert!(
        p3 > p2 && p2 > 0.0,
        "(c) get_playback_position() must advance between frames, got {p2} then {p3}"
    );

    // (a) frame 3 paused the stream. Frame 4 mixes silence, and the position is
    // frozen where it stopped — not reset.
    ml.step(1.0 / 60.0);
    assert_eq!(
        mix_level(&ml),
        0.0,
        "(a) a paused stream must mix to pure silence"
    );
    let frozen = node_f64(&ml, player_id, "playback_position");
    assert!(frozen > 0.0, "(a) paused position must be non-zero");

    ml.step(1.0 / 60.0);
    assert_eq!(
        node_f64(&ml, player_id, "playback_position"),
        frozen,
        "(a) position must not advance while paused"
    );
    assert_eq!(
        script_var(&ml, main_id, "playing_while_paused"),
        Some(Variant::Bool(true)),
        "(a) is_playing() stays true while stream_paused — the voice is suspended, not stopped"
    );

    // (a cont.) frame 6 unpauses: playback resumes from the frozen position,
    // not from zero.
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    let resumed = node_f64(&ml, player_id, "playback_position");
    assert!(
        resumed > frozen,
        "(a) unpausing resumes from the frozen position, got {resumed} after {frozen}"
    );
    assert!(
        mix_level(&ml) > 0.0,
        "(a) unpausing makes the stream audible again"
    );
}

/// (b) `seek(0.5)` on the 1-second ramp makes the next mixed samples read ~0.5.
#[test]
fn gdscript_seek_repositions_the_mixed_output() {
    let src = "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    var p = get_node(\"Player\")
    if self.frame == 1:
        p.play()
    if self.frame == 2:
        p.seek(0.5)
";
    let (mut ml, _main_id, _player_id) = scene_with_player(src);

    ml.step(1.0 / 60.0);
    let before = mix_first(&ml);
    assert!(
        before < 0.1,
        "playback starts near the head of the ramp, got {before}"
    );

    // Frame 2 seeks to 0.5s; the mix produced in that same frame reads from there.
    ml.step(1.0 / 60.0);
    let after = mix_first(&ml);
    assert!(
        (after - 0.5).abs() < 0.01,
        "(b) after seek(0.5) the mixed samples must read the 0.5 s ramp value, got {after}"
    );
}

/// (d) `pitch_scale = 2.0` doubles how fast the ramp climbs within one frame.
#[test]
fn pitch_scale_property_doubles_the_ramp_slope() {
    fn slope_at_pitch(pitch: Option<f64>) -> f32 {
        let (mut ml, _main, player_id) = scene_with_player(
            "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        get_node(\"Player\").play()
",
        );
        if let Some(p) = pitch {
            ml.tree_mut()
                .get_node_mut(player_id)
                .unwrap()
                .set_property("pitch_scale", Variant::Float(p));
        }
        ml.step(1.0 / 60.0);
        ml.step(1.0 / 60.0);
        let mix = ml.audio_bridge().last_mix();
        assert!(!mix.is_empty(), "expected a mixed frame");
        // The ramp encodes time, so (last - first) over the frame is how far the
        // playhead travelled — i.e. the effective playback rate.
        mix[mix.len() - 1] - mix[0]
    }

    let normal = slope_at_pitch(None);
    let doubled = slope_at_pitch(Some(2.0));
    assert!(normal > 0.0, "baseline ramp must climb, got {normal}");
    assert!(
        (doubled / normal - 2.0).abs() < 0.15,
        "(d) pitch_scale = 2.0 must double the ramp slope: {normal} -> {doubled}"
    );
}

/// (e) `MainLoop::set_paused(true)` silences a default-process-mode player and
/// preserves its position; a `ProcessMode::Always` player keeps playing.
#[test]
fn tree_pause_silences_pausable_player_but_not_always_player() {
    let src = "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        get_node(\"Player\").play()
";
    let (mut ml, _main_id, player_id) = scene_with_player(src);

    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    assert!(mix_level(&ml) > 0.0, "player is audible before the pause");

    ml.set_paused(true);
    ml.step(1.0 / 60.0);
    assert_eq!(
        mix_level(&ml),
        0.0,
        "(e) a default (Pausable) player is silenced by SceneTree pause"
    );
    let held = node_f64(&ml, player_id, "playback_position");
    assert!(held > 0.0, "(e) paused position is preserved, not reset");

    ml.step(1.0 / 60.0);
    assert_eq!(
        node_f64(&ml, player_id, "playback_position"),
        held,
        "(e) position does not drift while the tree is paused"
    );

    // Unpausing resumes from where it stopped.
    ml.set_paused(false);
    ml.step(1.0 / 60.0);
    assert!(
        mix_level(&ml) > 0.0,
        "(e) unpausing the tree makes the player audible again"
    );
    assert!(
        node_f64(&ml, player_id, "playback_position") > held,
        "(e) playback continues from the held position"
    );

    // A ProcessMode::Always player ignores the tree pause, like Godot.
    let (mut ml2, _m2, always_id) = scene_with_player(src);
    ml2.tree_mut()
        .get_node_mut(always_id)
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    ml2.step(1.0 / 60.0);
    ml2.step(1.0 / 60.0);
    ml2.set_paused(true);
    ml2.step(1.0 / 60.0);
    assert!(
        mix_level(&ml2) > 0.0,
        "(e) a ProcessMode::Always player keeps playing through a tree pause"
    );
}
