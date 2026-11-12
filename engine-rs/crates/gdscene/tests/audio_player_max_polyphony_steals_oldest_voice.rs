//! pat-mgixk.85 (first half) — `AudioStreamPlayer.max_polyphony` starts a new
//! voice per `play()` and steals the OLDEST past the cap.
//!
//! # What was wrong
//!
//! The bridge held exactly one `AudioStreamPlayer` per node
//! (`audio_server_bridge.rs`, `players: HashMap<NodeId, AudioStreamPlayer>`),
//! and `AudioStreamPlayer::play` opens by stopping its own `playback_id`. So
//! every `play()` RESTARTED the single voice: a node could never be audible
//! twice over, and `max_polyphony` — registered in ClassDB since forever — was
//! read by nothing at all (`grep max_polyphony gdscene/src gdscript-interop/src`
//! was 0 hits). In Godot each `play()` adds a voice and the oldest is stolen
//! once the cap is reached (`audio_stream_player_internal.cpp`).
//!
//! # The path driven
//!
//! ```text
//! GDScript $Sfx.play() -> interpreter writes `__play_request__` on the node
//!   -> MainLoop::step -> AudioServerBridge::process drains it
//!     -> AudioServer voices (active_stream_count)
//! ```
//!
//! That is this bead's Wiring requirement verbatim, and it is why the test
//! drives a real attached script rather than calling the bridge directly: the
//! one-shot property channel is the part that was never exercised above one
//! voice.
//!
//! # 🔑 Why this is a DIFFERENTIAL, not an absolute count
//!
//! Asserting "3 voices after 4 plays" alone would also pass if the engine
//! ignored `max_polyphony` and happened to cap somewhere else, or if the fourth
//! play simply failed. So the identical scene is run twice, changing ONE
//! authored value — `max_polyphony` 1 vs 3 — and the claim is that the cap is
//! what the voice count follows. The monophonic arm is the control: it must stay
//! at exactly one voice however many times `play()` is called, which is also the
//! pre-bead behaviour, so a regression that reverts polyphony fails the 3-arm
//! while leaving the control green.
//!
//! # ⚠️ What this test does NOT cover
//!
//! The bead's second half — `AudioStreamPolyphonic` /
//! `AudioStreamPlaybackPolyphonic::play_stream(...) -> id`,
//! `is_stream_playing(id)`, `stop_stream(id)` — is NOT implemented and NOT
//! asserted here. That half needs id-keyed voices, which `PolyphonicPlayer`
//! (`gdaudio/src/decode.rs`) does not have: its pool is anonymous
//! (`try_play`/`stop_one`/`play_buffer`/`voice_count`). Reported as remaining
//! scope rather than half-wired.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};

const SAMPLE_RATE: u32 = 44_100;

/// A constant-amplitude buffer, long enough that every voice started in the
/// first few frames is still sounding when the assertions run.
///
/// 🔑 DC rather than a ramp or a tone: overlapping voices then SUM to a
/// predictable level, so "three voices are louder than one" is a statement about
/// mixing rather than about where three independent playheads happen to sit.
/// 0.25 keeps three summed voices at 0.75, clear of any clamp at 1.0.
fn dc_buffer(seconds: f32, value: f32) -> gdaudio::AudioBuffer {
    let frames = (SAMPLE_RATE as f32 * seconds) as usize;
    gdaudio::AudioBuffer {
        samples: (0..frames).flat_map(|_| [value, value]).collect(),
        sample_rate: SAMPLE_RATE,
        channels: 2,
    }
}

/// Root `Node` with a scripted `_process` that calls `play()` on four
/// consecutive frames, over a child `AudioStreamPlayer` carrying `max_polyphony`.
///
/// ⚠️ `max_polyphony` is set as a node PROPERTY rather than authored in a
/// `.tscn`, because the bridge reads it off the node either way and this keeps
/// the two arms textually identical apart from the one number under test.
fn fixture(max_polyphony: i64) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let host = tree
        .add_child(root_id, Node::new("Host", "Node"))
        .expect("Host is addable");
    let sfx = tree
        .add_child(host, Node::new("Sfx", "AudioStreamPlayer"))
        .expect("Sfx is addable");
    tree.get_node_mut(sfx)
        .expect("Sfx exists")
        .set_property("max_polyphony", gdvariant::Variant::Int(max_polyphony));

    // `+=`/`elif` avoided for parser-independence, matching the sibling audio
    // tests in this crate.
    let src = "\
extends Node
var frame = 0
func _process(delta):
\tself.frame = self.frame + 1
\tvar p = get_node(\"Sfx\")
\tif self.frame < 5:
\t\tp.play()
";
    let script = GDScriptNodeInstance::from_source(src, host).expect("script parses");
    tree.attach_script(host, Box::new(script));

    let mut ml = MainLoop::new(tree);
    // Injected straight onto the player: the bridge treats an injected buffer as
    // winning over a path the node names, so no `res://` file or loader is
    // involved and the test cannot fail on IO.
    ml.audio_bridge_mut()
        .set_stream_buffer(sfx, dc_buffer(2.0, 0.25));
    ml.enter_tree_with_scripts(host);
    (ml, sfx)
}

/// Mean absolute amplitude of the last mixed frame. 0.0 is silence.
fn mix_level(ml: &MainLoop) -> f32 {
    let mix = ml.audio_bridge().last_mix();
    if mix.is_empty() {
        return 0.0;
    }
    mix.iter().map(|s| s.abs()).sum::<f32>() / mix.len() as f32
}

/// Steps far enough that all four `play()` calls have been written by the
/// script AND drained by the bridge, while a 2 s buffer leaves every voice alive.
fn run(ml: &mut MainLoop) {
    for _ in 0..8 {
        ml.step(1.0 / 60.0);
    }
}

/// **The acceptance (first half).**
#[test]
fn audio_player_max_polyphony_steals_oldest_voice() {
    // -- the control arm: monophonic stays at one voice ----------------------
    let (mut mono, _) = fixture(1);
    run(&mut mono);
    let mono_voices = mono.audio_bridge().active_stream_count();
    assert_eq!(
        mono_voices, 1,
        "max_polyphony = 1 is Godot's monophonic default: four play() calls must \
         leave exactly ONE voice, each restarting the last. {mono_voices} voices \
         means play() is adding voices regardless of the cap",
    );

    // -- the polyphonic arm: four plays, capped at three ---------------------
    let (mut poly, _) = fixture(3);
    run(&mut poly);
    let poly_voices = poly.audio_bridge().active_stream_count();
    assert_eq!(
        poly_voices, 3,
        "max_polyphony = 3 with four play() calls must leave exactly THREE \
         voices — a new voice per call, the oldest stolen once the cap is \
         reached. 1 means max_polyphony is still unread and play() restarts a \
         single voice; 4 means nothing is being stolen",
    );

    // -- and the overlap is audible, not merely counted ----------------------
    //
    // 🔑 Counting voices alone would pass if the extra voices were registered
    // but silent (stopped, or detached from the bus). Three summed DC voices
    // must read louder than one.
    let mono_level = mix_level(&mono);
    let poly_level = mix_level(&poly);
    assert!(
        mono_level > 0.0,
        "precondition: the control arm must be audible at all, else the \
         comparison below is vacuous (got {mono_level})",
    );
    assert!(
        poly_level > mono_level * 1.5,
        "three overlapping voices must mix LOUDER than one: poly {poly_level} vs \
         mono {mono_level}. A ratio near 1.0 means the extra voices exist in the \
         server's count but contribute no samples",
    );
}
