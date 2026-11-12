//! pat-s3rrv.190 — `Area2D.audio_bus_override` reroutes a 2D voice to the
//! area's bus, and gives it back when the player leaves.
//!
//! Godot routes an `AudioStreamPlayer2D` standing inside an `Area2D` with
//! `audio_bus_override` enabled to that area's `audio_bus_name`
//! (`scene/2d/physics/area_2d.cpp`) — the standard room/cave reverb technique.
//!
//! ⚠️ THE "TESTED LIBRARY CODE NOTHING CALLS" PATTERN. `redirect_stream_bus`
//! shipped in the audio server documented as being "for Godot's Area3D/Area2D
//! audio-bus override"; the Area3D caller was added by pat-s3rrv.102 and the 2D
//! half was still unreachable from a scene. These tests drive real
//! `MainLoop::step` frames, so a pass means the frame loop connects them — not
//! that a helper exists.
//!
//! 🔑 Resolution is a POINT test, not a body overlap. An `AudioStreamPlayer2D`
//! is not a physics body and never appears in an area's `overlapping_bodies`,
//! so the overlap machinery cannot answer this — hence a separate
//! `PhysicsServer::audio_bus_at` query.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A second of 440 Hz, enough for a live voice to exist across a few frames.
fn tone() -> gdaudio::AudioBuffer {
    let samples: Vec<f32> = (0..44100usize)
        .flat_map(|i| {
            let s = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.5;
            [s, s]
        })
        .collect();
    gdaudio::AudioBuffer {
        samples,
        sample_rate: 44100,
        channels: 2,
    }
}

/// Far enough from the origin to be clear of every zone below.
const OUTSIDE: Vector2 = Vector2::new(500.0, 0.0);

/// Adds an `Area2D` named `name` at `at`, an 80×80 box, carrying `props`.
///
/// ⚠️ The collider is a child `CollisionShape2D` with a `size`, which is what
/// `shape_from_node` actually reads. An Area2D with no shape child registers no
/// shape at all, so the point query would never hit and every assertion here
/// would pass or fail for a reason having nothing to do with audio.
fn add_zone(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    at: Vector2,
    props: &[(&str, Variant)],
) -> NodeId {
    let zone = tree.add_child(parent, Node::new(name, "Area2D")).unwrap();
    node2d::set_position(tree, zone, at);
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(80.0, 80.0)));
    tree.add_child(zone, shape).unwrap();
    for (key, value) in props {
        if let Some(node) = tree.get_node_mut(zone) {
            node.set_property(key, value.clone());
        }
    }
    zone
}

/// `Root -> {Zone (Area2D 80×80 at origin), Sfx (AudioStreamPlayer2D)}`.
///
/// The player starts OUTSIDE and on its own bus, so every test below begins
/// from the un-overridden state and has to be moved in.
fn scene(zone_props: &[(&str, Variant)]) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    add_zone(&mut tree, root, "Zone", Vector2::ZERO, zone_props);

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer2D");
    sfx.set_property("autoplay", Variant::Bool(true));
    let sfx_id = tree.add_child(root, sfx).unwrap();
    node2d::set_position(&mut tree, sfx_id, OUTSIDE);

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, tone());
    (ml, sfx_id)
}

fn settle(ml: &mut MainLoop) {
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
}

fn move_to(ml: &mut MainLoop, sfx: NodeId, to: Vector2) {
    node2d::set_position(ml.tree_mut(), sfx, to);
    settle(ml);
}

fn bus_of(ml: &mut MainLoop, sfx: NodeId) -> Option<String> {
    ml.audio_bridge_mut().node_bus(sfx)
}

/// The override props used by most tests.
fn reverb() -> Vec<(&'static str, Variant)> {
    vec![
        ("audio_bus_override", Variant::Bool(true)),
        ("audio_bus_name", Variant::String("Reverb".to_string())),
    ]
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** A player on Master that moves inside an overriding
/// Area2D is reported on that area's bus after the next tick, and returns to
/// Master when it leaves.
#[test]
fn area2d_audio_bus_override_reroutes_player() {
    let (mut ml, sfx) = scene(&reverb());
    settle(&mut ml);

    // -- starts outside, on its own bus --------------------------------------
    assert_eq!(
        bus_of(&mut ml, sfx).as_deref(),
        Some("Master"),
        "fixture: the player must start with a LIVE voice on Master. `None` \
         means there is no voice at all, and every assertion below would then \
         be about nothing"
    );

    // -- moves in -> the area's bus ------------------------------------------
    move_to(&mut ml, sfx, Vector2::ZERO);
    assert_eq!(
        bus_of(&mut ml, sfx).as_deref(),
        Some("Reverb"),
        "a voice inside an Area2D with audio_bus_override must be rerouted to \
         the area's audio_bus_name"
    );

    // -- moves out -> back to its own bus ------------------------------------
    //
    // ⚠️ The half that does NOT come for free. The redirect is applied to the
    // live voice, and the per-frame player sync only re-stamps the *player's*
    // configured bus — nothing resets the voice. Without an explicit restore a
    // player would carry the reverb bus for the rest of its life.
    move_to(&mut ml, sfx, OUTSIDE);
    assert_eq!(
        bus_of(&mut ml, sfx).as_deref(),
        Some("Master"),
        "leaving the area must give the voice its own bus back"
    );
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

/// ⚠️ A player that never enters is never touched.
///
/// Without this the acceptance would pass against a sweep that rerouted every
/// voice unconditionally, ignoring position entirely.
#[test]
fn a_player_that_never_enters_keeps_its_bus() {
    let (mut ml, sfx) = scene(&reverb());
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        bus_of(&mut ml, sfx).as_deref(),
        Some("Master"),
        "the override is a point test, not a global switch"
    );
}

/// ⚠️ Inside the area, but with the flag OFF.
///
/// Separates "the sweep respects position" from "the sweep respects the flag";
/// a sweep ignoring `audio_bus_override` passes the position control above and
/// is still wrong.
#[test]
fn an_area_without_the_override_flag_does_not_reroute() {
    let (mut ml, sfx) = scene(&[
        ("audio_bus_override", Variant::Bool(false)),
        ("audio_bus_name", Variant::String("Reverb".to_string())),
    ]);
    move_to(&mut ml, sfx, Vector2::ZERO);

    assert_eq!(
        bus_of(&mut ml, sfx).as_deref(),
        Some("Master"),
        "with audio_bus_override false the area must not touch the voice's bus"
    );
}

/// Two overlapping overriding areas: the higher `priority` wins.
///
/// ⚠️ Both zones sit on the same point, so containment cannot break the tie and
/// `priority` is the only discriminator. The lower-priority zone is added
/// SECOND, so "last one wins" — the answer a sweep with no priority comparison
/// gives — is the wrong answer here.
#[test]
fn the_highest_priority_overlapping_area_wins() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    add_zone(
        &mut tree,
        root,
        "Loud",
        Vector2::ZERO,
        &[
            ("audio_bus_override", Variant::Bool(true)),
            ("audio_bus_name", Variant::String("Reverb".to_string())),
            ("priority", Variant::Int(5)),
        ],
    );
    add_zone(
        &mut tree,
        root,
        "Quiet",
        Vector2::ZERO,
        &[
            ("audio_bus_override", Variant::Bool(true)),
            ("audio_bus_name", Variant::String("Muffled".to_string())),
            ("priority", Variant::Int(1)),
        ],
    );

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer2D");
    sfx.set_property("autoplay", Variant::Bool(true));
    let sfx_id = tree.add_child(root, sfx).unwrap();
    node2d::set_position(&mut tree, sfx_id, Vector2::ZERO);

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, tone());
    settle(&mut ml);

    assert_eq!(
        bus_of(&mut ml, sfx_id).as_deref(),
        Some("Reverb"),
        "priority 5 must beat priority 1 regardless of registration order"
    );
}

/// ⚠️ A player naming its own bus gets THAT bus back on exit, not Master.
///
/// The restore reads the node's own `bus` property. Hard-coding "Master" would
/// pass the acceptance above — where the player's own bus happens to be Master
/// — while silently breaking every player that selected a bus for itself.
#[test]
fn exiting_restores_the_players_own_bus_not_master() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    add_zone(&mut tree, root, "Zone", Vector2::ZERO, &reverb());

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer2D");
    sfx.set_property("autoplay", Variant::Bool(true));
    sfx.set_property("bus", Variant::String("Music".to_string()));
    let sfx_id = tree.add_child(root, sfx).unwrap();
    node2d::set_position(&mut tree, sfx_id, OUTSIDE);

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, tone());
    settle(&mut ml);
    assert_eq!(
        bus_of(&mut ml, sfx_id).as_deref(),
        Some("Music"),
        "fixture: the player starts on its own authored bus"
    );

    move_to(&mut ml, sfx_id, Vector2::ZERO);
    assert_eq!(bus_of(&mut ml, sfx_id).as_deref(), Some("Reverb"));

    move_to(&mut ml, sfx_id, OUTSIDE);
    assert_eq!(
        bus_of(&mut ml, sfx_id).as_deref(),
        Some("Music"),
        "the player's OWN bus must come back, not a hard-coded Master"
    );
}
