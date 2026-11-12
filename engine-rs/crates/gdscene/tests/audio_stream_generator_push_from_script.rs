//! pat-s3rrv.159 acceptance: an `AudioStreamGenerator` stream carries pushed
//! frames all the way to the mixed output.
//!
//! **Why this shape and not `_mix`.** The bead asks to "pull frames through
//! `_mix`". That virtual cannot be dispatched to a script in Patina *or in
//! Godot* — its signature is
//! `AudioStreamPlayback._mix(buffer: AudioFrame*, rate_scale: float, frames: int)`,
//! and GDScript has no pointer type, so it is GDExtension-only. Godot's actual
//! script-reachable route for procedural audio is
//! `AudioStreamGenerator` + `AudioStreamGeneratorPlayback.push_buffer()`, whose
//! whole surface is plain-typed. That is what these tests pin.
//!
//! **What the push channel is.** `gdscene` depends on `gdscript-interop` and
//! not the reverse, so a script cannot call the bridge directly. The
//! interpreter therefore writes frames into a node property and the bridge
//! drains it — exactly the arrangement `seek_request` already uses for
//! `AudioStreamPlayer.seek()`. Writing that property is what
//! `AudioStreamGeneratorPlayback.push_buffer()` will do; these tests drive it
//! directly, so they pin the bridge half independently of the interpreter arm
//! that is still to come.
//!
//! Every test asserts on the **mixed samples**, not on node properties, so a
//! regression to "writes a property nobody reads" — the exact failure
//! pat-0q2kx.5 found in this same file — fails here.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The `__resref__<prop>` companion `packed_scene.rs` writes for an inline
/// sub-resource: `"<class>::<sub id>"`. This is the only place the class name
/// of a fileless stream survives.
const GENERATOR_RESREF: &str = "AudioStreamGenerator::1_gen";

/// Amplitude of every pushed frame. Constant rather than a ramp so that "did
/// any audio arrive" is a single comparison, and loud enough that no plausible
/// bus gain could hide it.
const AMPLITUDE: f32 = 0.5;

/// Comfortably more than one 60 Hz block at 44100 Hz (735 frames), so the mix
/// is never starved by an off-by-one in the frame accumulator, and comfortably
/// under the generator's 0.5 s capacity so nothing is dropped.
const PUSH_FRAMES: usize = 2048;

/// Builds `Main -> Player`, where `Player` is an `AudioStreamPlayer` whose
/// stream is the inline sub-resource named by `resref`.
///
/// `resref` is a parameter rather than a constant so one test can hand in a
/// *different* class and prove the bridge is matching on the class name rather
/// than on "is an unresolved sub-resource".
fn scene_with_generator(resref: &str, playing: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let player_id = tree
        .add_child(main_id, Node::new("Player", "AudioStreamPlayer"))
        .unwrap();

    if let Some(node) = tree.get_node_mut(player_id) {
        // What a `.tscn` produces for an inline sub-resource: the raw ref in
        // `stream`, the class only in the companion.
        node.set_property(
            "stream",
            Variant::String("SubResource(\"1_gen\")".to_owned()),
        );
        node.set_property("__resref__stream", Variant::String(resref.to_owned()));
        node.set_property("playing", Variant::Bool(playing));
    }

    (MainLoop::new(tree), player_id)
}

/// Writes one block of frames into the push channel, as
/// `AudioStreamGeneratorPlayback.push_buffer()` will.
fn push(ml: &mut MainLoop, player: NodeId, frames: usize, amplitude: f32) {
    let block: Vec<Vector2> = (0..frames)
        .map(|_| Vector2::new(amplitude, amplitude))
        .collect();
    ml.tree_mut()
        .get_node_mut(player)
        .expect("player node must still be in the tree")
        .set_property("__generator_push__", Variant::PackedVector2Array(block));
}

/// Largest absolute sample in the last mix — "did any audio come out".
fn peak(ml: &MainLoop) -> f32 {
    ml.audio_bridge()
        .last_mix()
        .iter()
        .fold(0.0f32, |acc, s| acc.max(s.abs()))
}

/// 🔑 THE ACCEPTANCE: frames pushed from the script side reach the output.
#[test]
fn pushed_generator_frames_reach_the_mixed_output() {
    let (mut ml, player) = scene_with_generator(GENERATOR_RESREF, true);
    push(&mut ml, player, PUSH_FRAMES, AMPLITUDE);

    ml.step(1.0 / 60.0);

    assert!(
        peak(&ml) > 0.1,
        "pushed frames must be audible in the mix, got peak {}. Silence here \
         means one of: the `__resref__stream` companion was not recognised as a \
         generator (see `a_non_generator_subresource_is_not_treated_as_one`), \
         the push channel was not drained, or the block was queued but never \
         handed to the server before the mix",
        peak(&ml)
    );
}

/// **Negative control for the test above.** The same scene with nothing pushed
/// must be silent.
///
/// ⚠️ Without this, `pushed_generator_frames_reach_the_mixed_output` would also
/// pass if the bridge produced tone from some unrelated source — a "must be
/// non-zero" assertion passes for many reasons.
#[test]
fn a_generator_with_nothing_pushed_is_silent() {
    let (mut ml, _player) = scene_with_generator(GENERATOR_RESREF, true);

    ml.step(1.0 / 60.0);

    assert_eq!(
        peak(&ml),
        0.0,
        "a generator nobody pushed to must mix to silence — an underrun is \
         silence, not noise"
    );
}

/// The push channel is one-shot: a block pushed once is heard once.
///
/// ⚠️ This is the regression that would otherwise be inaudible in a test but
/// obvious to a user — a script that pushes and then idles would hear its last
/// block repeat forever, as a stuck loop rather than as silence.
#[test]
fn a_pushed_block_is_consumed_exactly_once() {
    let (mut ml, player) = scene_with_generator(GENERATOR_RESREF, true);
    push(&mut ml, player, PUSH_FRAMES, AMPLITUDE);

    ml.step(1.0 / 60.0);
    assert!(peak(&ml) > 0.1, "first frame must hear the block");

    // The channel must have been emptied by the drain above...
    let channel = ml
        .tree()
        .get_node(player)
        .map(|n| n.get_property("__generator_push__"));
    assert!(
        matches!(channel, Some(Variant::Nil)),
        "the push channel must be drained after it is consumed, else the same \
         block is re-pushed every frame; found {channel:?}"
    );

    // ...and the queue drains within a few frames rather than replaying.
    for _ in 0..8 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        peak(&ml),
        0.0,
        "after the pushed frames are consumed the generator must fall silent, \
         not loop its last block"
    );
}

/// A stopped player is silent even with frames queued.
///
/// In Godot `get_stream_playback()` returns nothing on a stopped player, so a
/// script cannot push at all and nothing already queued is heard.
#[test]
fn a_stopped_generator_player_does_not_sound() {
    let (mut ml, player) = scene_with_generator(GENERATOR_RESREF, false);
    push(&mut ml, player, PUSH_FRAMES, AMPLITUDE);

    ml.step(1.0 / 60.0);

    assert_eq!(
        peak(&ml),
        0.0,
        "a player with `playing = false` must not emit generated audio"
    );
    assert_eq!(
        ml.audio_bridge().generator_frames_queued(player),
        Some(0),
        "a stopped player's queue must be dropped, so pressing play does not \
         replay audio pushed while it was stopped"
    );
}

/// 🔑 Pins the *detection*, which is the part most likely to be written too
/// loosely.
///
/// ⚠️ Every inline sub-resource looks identical in the `stream` property —
/// `SubResource("id")`. A bridge that keyed off that instead of off the
/// `__resref__stream` class name would treat an inline `AudioStreamWAV` as a
/// generator and silently swallow its audio. `None` here means "not a
/// generator at all", which is a different answer from `Some(0)`.
#[test]
fn a_non_generator_subresource_is_not_treated_as_one() {
    let (mut ml, player) = scene_with_generator("AudioStreamWAV::1_gen", true);
    push(&mut ml, player, PUSH_FRAMES, AMPLITUDE);

    ml.step(1.0 / 60.0);

    assert_eq!(
        ml.audio_bridge().generator_frames_queued(player),
        None,
        "an inline AudioStreamWAV must not be backed by a generator"
    );
    assert_eq!(
        peak(&ml),
        0.0,
        "frames pushed at a non-generator stream must not be played"
    );
}

/// The prefix match must not accept `AudioStreamGeneratorPlayback`, a different
/// class whose name starts with the one we want.
#[test]
fn the_playback_class_is_not_mistaken_for_the_stream_class() {
    let (mut ml, player) = scene_with_generator("AudioStreamGeneratorPlayback::1_gen", true);

    ml.step(1.0 / 60.0);

    assert_eq!(
        ml.audio_bridge().generator_frames_queued(player),
        None,
        "`AudioStreamGeneratorPlayback` is not `AudioStreamGenerator` — the \
         `::` separator is what keeps the prefix test honest"
    );
}
