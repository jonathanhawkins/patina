//! Acceptance test for pat-s3rrv.6: the `AudioServer` singleton is reachable
//! from GDScript and addresses the *live* mixer.
//!
//! Godot's `AudioServer` (`servers/audio/audio_server.cpp`, one of the 39
//! entries in `extension_api.json .singletons[]`) is how a game reaches the
//! mixer without nodes — `AudioServer.set_bus_volume_db(AudioServer.get_bus_index("Music"), db)`
//! is the standard options-menu idiom.
//!
//! Patina's Rust half was already implemented and unit-tested
//! (`gdaudio::AudioServer` peak meters, `playback_speed_scale`, `AudioMixer`
//! bus table) and the bridge mixes it every frame — but nothing script-facing
//! reached it: `AudioServer` was absent from `BUILTIN_SINGLETONS` and no
//! dispatch arm existed, so the call above raised `UndefinedFunction`.
//!
//! The path under test is the whole chain, with no Rust-side shortcut:
//!
//! ```text
//! GDScript AudioServer.* -> interpreter dispatch arm -> SceneAccess
//!   -> SceneTreeAccessor::audio_mut (address bound by MainLoop)
//!   -> the AudioServerBridge's live AudioServer -> next frame's mix
//! ```
//!
//! Every assertion reads the *mixed PCM* or a value the engine computed, never
//! a property the script itself wrote.

use gdaudio::AudioBuffer;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// One second of stereo 440 Hz at half amplitude — clearly non-silent.
fn sine_buffer() -> AudioBuffer {
    let sample_rate = 44_100u32;
    let mut samples = Vec::with_capacity(sample_rate as usize * 2);
    for i in 0..sample_rate as usize {
        let t = i as f32 / sample_rate as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        samples.push(v);
        samples.push(v);
    }
    AudioBuffer {
        samples,
        sample_rate,
        channels: 2,
    }
}

/// Builds `World` (carrying `script`) with one autoplaying `AudioStreamPlayer`
/// per entry in `buses`, each routed to the named bus and handed a non-silent
/// buffer directly — so nothing in these tests depends on file loading.
///
/// A "Music" bus is added to the live mixer *before* `_ready` runs, because
/// `get_bus_index("Music")` has to find a real bus for the ducking to mean
/// anything. `enter_tree_with_scripts` is what makes `_ready` see a bound
/// `AudioServer`; a bare `LifecycleManager::enter_tree` would leave the
/// singleton unbound (see `MainLoop::enter_tree_with_scripts`).
fn scene(script: &str, buses: &[&str]) -> (MainLoop, NodeId, Vec<NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();

    let mut players = Vec::new();
    for (i, bus) in buses.iter().enumerate() {
        let mut p = Node::new(format!("Player{i}"), "AudioStreamPlayer");
        p.set_property("bus", Variant::String((*bus).to_string()));
        p.set_property("autoplay", Variant::Bool(true));
        players.push(tree.add_child(world, p).unwrap());
    }

    // Attached after `add_child` so the premature enter/ready that `add_child`
    // performs cannot run `_ready` before the script exists.
    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(script, world).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    let music = ml
        .audio_bridge_mut()
        .server_mut()
        .mixer_mut()
        .add_bus("Music");
    assert_eq!(music, 1, "Music should be the first bus after Master");
    for id in &players {
        ml.audio_bridge_mut().set_stream_buffer(*id, sine_buffer());
    }

    ml.enter_tree_with_scripts(world);
    (ml, world, players)
}

fn script_var(ml: &MainLoop, node: NodeId, prop: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// Peak `|sample|` over `frames` mixed frames.
fn peak_over(ml: &mut MainLoop, frames: usize) -> f32 {
    let mut peak = 0.0f32;
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
        peak = ml
            .audio_bridge()
            .last_mix()
            .iter()
            .fold(peak, |a, s| a.max(s.abs()));
    }
    peak
}

/// Ducks the Music bus in `_ready`, recording the index it resolved so the test
/// can tell "the bus was found and silenced" from "nothing happened".
const DUCK: &str = "\
extends Node
var music_idx = -99
func _ready():
    var idx = AudioServer.get_bus_index(\"Music\")
    self.music_idx = idx
    AudioServer.set_bus_volume_db(idx, -80.0)
";

/// The control: identical scene and players, but no `AudioServer` call at all.
const INERT: &str = "\
extends Node
var music_idx = -99
func _ready():
    self.music_idx = AudioServer.get_bus_index(\"Music\")
";

/// **The acceptance clause.** A script ducking the Music bus in `_ready()`
/// silences a player routed to Music on the next frame, while a player on
/// Master stays audible.
#[test]
fn audio_server_singleton_from_script() {
    // (a) The script's `get_bus_index("Music")` found the bus that was added to
    //     the live mixer — not a stub, not -1.
    let (mut ducked, world, _) = scene(DUCK, &["Music"]);
    assert_eq!(
        script_var(&ducked, world, "music_idx"),
        Variant::Int(1),
        "AudioServer.get_bus_index(\"Music\") must resolve against the live \
         mixer's bus table (Master=0, Music=1)",
    );

    // (b) A Music-only scene mixes to silence after the duck.
    let ducked_peak = peak_over(&mut ducked, 4);
    assert!(
        ducked_peak < 1e-3,
        "a player on the ducked Music bus must mix to silence; peak was {ducked_peak}",
    );

    // (c) The control run proves (b) is the duck's doing and not a scene that
    //     never made sound: same scene, same buffer, no volume call.
    let (mut control, _, _) = scene(INERT, &["Music"]);
    let control_peak = peak_over(&mut control, 4);
    assert!(
        control_peak > 0.01,
        "without the duck the very same scene must be audible; peak was \
         {control_peak} — if this fails the silence above proves nothing",
    );

    // (d) The duck is bus-scoped, not a global mute: with a second player on
    //     Master, the mix stays audible even though Music is silenced.
    let (mut mixed, _, _) = scene(DUCK, &["Music", "Master"]);
    let mixed_peak = peak_over(&mut mixed, 4);
    assert!(
        mixed_peak > 0.01,
        "ducking Music must leave a Master player audible; peak was {mixed_peak}",
    );
}

/// The exact one-liner from the bead — the nested call form Godot code actually
/// uses — rather than the index-in-a-variable spelling above.
#[test]
fn audio_server_nested_get_bus_index_call_ducks_the_bus() {
    const NESTED: &str = "\
extends Node
func _ready():
    AudioServer.set_bus_volume_db(AudioServer.get_bus_index(\"Music\"), -80.0)
";
    let (mut ml, _, _) = scene(NESTED, &["Music"]);
    let peak = peak_over(&mut ml, 4);
    assert!(
        peak < 1e-3,
        "the nested `set_bus_volume_db(get_bus_index(\"Music\"), -80.0)` form \
         must duck the bus too; peak was {peak}",
    );
}

/// `get_bus_peak_volume_left_db` returns a finite dB value that rises once audio
/// has actually been mixed.
#[test]
fn audio_server_peak_meter_rises_when_audio_plays() {
    // Frame 1's `_process` runs BEFORE that frame's mix, so it samples the
    // meter while nothing has been mixed yet; frame 2 sees the first block.
    const METER: &str = "\
extends Node
var frames = 0
var first = 0.0
var later = 0.0
func _process(delta):
    self.frames = self.frames + 1
    if self.frames == 1:
        self.first = AudioServer.get_bus_peak_volume_left_db(0, 0)
    else:
        self.later = AudioServer.get_bus_peak_volume_left_db(0, 0)
";
    let (mut ml, world, _) = scene(METER, &["Master"]);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    let first = match script_var(&ml, world, "first") {
        Variant::Float(f) => f,
        other => panic!("meter should read a float, got {other:?}"),
    };
    let later = match script_var(&ml, world, "later") {
        Variant::Float(f) => f,
        other => panic!("meter should read a float, got {other:?}"),
    };
    assert!(
        first.is_finite() && later.is_finite(),
        "peak meters must report finite dB, got first={first} later={later}",
    );
    assert!(
        later > first,
        "the Master peak meter must rise once audio has been mixed \
         (before={first} dB, after={later} dB)",
    );
}

/// `set_playback_speed_scale(2.0)` doubles the rate a stream's
/// `playback_position` advances.
#[test]
fn audio_server_playback_speed_scale_doubles_position_advance() {
    const FAST: &str = "\
extends Node
func _ready():
    AudioServer.set_playback_speed_scale(2.0)
";
    fn position_after(script: &str, frames: usize) -> f64 {
        let (mut ml, _, players) = scene(script, &["Master"]);
        for _ in 0..frames {
            ml.step(1.0 / 60.0);
        }
        match ml
            .tree()
            .get_node(players[0])
            .expect("player node")
            .get_property("playback_position")
        {
            Variant::Float(f) => f,
            other => panic!("playback_position should be a float, got {other:?}"),
        }
    }

    let normal = position_after(INERT, 6);
    let fast = position_after(FAST, 6);

    assert!(
        normal > 0.0,
        "the control stream must actually be advancing; position was {normal}",
    );
    assert!(
        fast > normal * 1.8 && fast < normal * 2.2,
        "playback_speed_scale(2.0) must roughly double the advance: \
         {fast} vs {normal} (ratio {})",
        fast / normal,
    );
}

/// The wider bus surface round-trips through the singleton: count, name, mute
/// and volume all read back what the script wrote, from the live mixer.
#[test]
fn audio_server_bus_surface_roundtrips_from_script() {
    const SURFACE: &str = "\
extends Node
var count = 0
var name0 = \"\"
var muted = false
var vol = 0.0
func _ready():
    self.count = AudioServer.get_bus_count()
    self.name0 = AudioServer.get_bus_name(0)
    AudioServer.set_bus_mute(1, true)
    self.muted = AudioServer.is_bus_mute(1)
    AudioServer.set_bus_volume_db(1, -12.0)
    self.vol = AudioServer.get_bus_volume_db(1)
";
    let (ml, world, _) = scene(SURFACE, &["Master"]);

    assert_eq!(
        script_var(&ml, world, "count"),
        Variant::Int(2),
        "get_bus_count must see Master + Music on the live mixer",
    );
    assert_eq!(
        script_var(&ml, world, "name0"),
        Variant::String("Master".to_string()),
        "get_bus_name(0) must name the master bus",
    );
    assert_eq!(
        script_var(&ml, world, "muted"),
        Variant::Bool(true),
        "set_bus_mute must land on the live bus and read back through is_bus_mute",
    );
    match script_var(&ml, world, "vol") {
        Variant::Float(v) => assert!(
            (v - -12.0).abs() < 1e-4,
            "get_bus_volume_db must read back the volume just written, got {v}",
        ),
        other => panic!("get_bus_volume_db should return a float, got {other:?}"),
    }

    // The mute really reached the engine, not just the script's own view.
    assert!(
        ml.audio_bridge()
            .server()
            .mixer()
            .get_bus(1)
            .expect("Music bus")
            .is_mute(),
        "the engine-side bus must be muted after the script's call",
    );
}

/// With no server bound — a script phase the host drove itself, or a bare
/// interpreter — the singleton answers with the trait defaults instead of
/// panicking or reaching a stale address.
#[test]
fn audio_server_calls_are_inert_without_a_bound_server() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();
    const PROBE: &str = "\
extends Node
var idx = 0
var count = -1
func _ready():
    self.idx = AudioServer.get_bus_index(\"Music\")
    self.count = AudioServer.get_bus_count()
    AudioServer.set_bus_volume_db(0, -80.0)
    AudioServer.remove_bus(0)
";
    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(PROBE, world).expect("script parses")),
    );

    // Deliberately NOT `MainLoop::enter_tree_with_scripts` — nothing is bound.
    gdscene::LifecycleManager::enter_tree(&mut tree, world);

    let script = tree.get_script(world).expect("script");
    assert_eq!(
        script.get_property("idx"),
        Some(Variant::Int(-1)),
        "an unbound AudioServer reports -1 for every bus name",
    );
    assert_eq!(
        script.get_property("count"),
        Some(Variant::Int(0)),
        "an unbound AudioServer reports no buses",
    );
}
