//! pat-nzgdu.2 — AudioStreamPlayer play/stop/is_playing dispatched from GDScript.
//!
//! Before this bead, `$Player.play()` on a node ObjectId hit the interpreter's
//! ObjectId method catch-all and threw `UndefinedFunction("ObjectId.play")`;
//! only the autoplay flag or a direct `playing = true` property assignment
//! produced sound. The fix adds AudioStreamPlayer method dispatch in the
//! interpreter's ObjectId match, routing `play`/`stop`/`is_playing` to the node
//! `playing` property that `AudioServerBridge` polls each frame.
//!
//! This app-level test drives a scripted scene through `MainLoop::step` and
//! asserts the full path with no autoplay flag set: script `play()` -> interpreter
//! dispatch -> node `playing` -> bridge starts the voice -> non-silent mix; a
//! later `stop()` returns the mix to silence; and `is_playing()` reflects the
//! transition.

use gdaudio::AudioBuffer;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

#[test]
fn audio_script_play_dispatch() {
    // Scene: Main (script) -> Player (AudioStreamPlayer). No autoplay flag — the
    // script drives playback purely through method calls.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node");
    let main_id = tree.add_child(root, main).unwrap();
    let player = Node::new("Player", "AudioStreamPlayer");
    let player_id = tree.add_child(main_id, player).unwrap();

    // Frame 1: play() then record is_playing(). Frame 3: stop() then record
    // is_playing(). (`+=`/`elif` avoided for parser-independence.)
    let script_src = "\
extends Node
var frame = 0
var playing_after_play = false
var playing_after_stop = true
func _process(delta):
    self.frame = self.frame + 1
    var p = get_node(\"Player\")
    if self.frame == 1:
        p.play()
        self.playing_after_play = p.is_playing()
    if self.frame == 3:
        p.stop()
        self.playing_after_stop = p.is_playing()
";
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);

    // A 1-second 440 Hz stereo tone so the mix is clearly non-silent while playing.
    let samples: Vec<f32> = (0..44100usize)
        .flat_map(|i| {
            let s = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.5;
            [s, s]
        })
        .collect();
    ml.audio_bridge_mut().set_stream_buffer(
        player_id,
        AudioBuffer {
            samples,
            sample_rate: 44100,
            channels: 2,
        },
    );

    // Frame 1: script play() -> bridge starts the voice -> non-silent mix.
    ml.step(1.0 / 60.0);
    assert!(
        ml.audio_bridge().last_mix().iter().any(|s| s.abs() > 0.01),
        "script $Player.play() must drive a non-silent mix (got silence)",
    );

    // Frame 2: voice keeps playing between play() and stop().
    ml.step(1.0 / 60.0);
    assert!(
        ml.audio_bridge().last_mix().iter().any(|s| s.abs() > 0.01),
        "voice should keep playing between play() and stop()",
    );

    // Frame 3: script stop() -> bridge stops the voice -> silence.
    ml.step(1.0 / 60.0);
    assert!(
        ml.audio_bridge().last_mix().iter().all(|s| s.abs() < 1e-6),
        "script $Player.stop() must return the mix to silence",
    );

    // is_playing() reflected the transition (dispatch read the live `playing`).
    let script = ml.tree().get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("playing_after_play"),
        Some(Variant::Bool(true)),
        "is_playing() must be true immediately after play()",
    );
    assert_eq!(
        script.get_property("playing_after_stop"),
        Some(Variant::Bool(false)),
        "is_playing() must be false immediately after stop()",
    );
}
