//! pat-s3rrv.102: `Area3D.audio_bus_override` reroutes a voice to the area's bus.
//!
//! Godot routes an `AudioStreamPlayer3D` standing inside an `Area3D` with
//! `audio_bus_override` enabled to that area's `audio_bus_name` — the mechanism
//! behind a cave or room reverb (`scene/3d/physics/area_3d.cpp`).
//!
//! ⚠️ THIS IS THE "TESTED LIBRARY CODE NOTHING CALLS" PATTERN, exactly as the
//! audit's Second Law describes. `AudioServer::redirect_stream_bus` shipped with
//! the audio server and is documented in-tree as existing "for Godot's
//! Area3D/Area2D audio-bus override" — and a repo-wide search found NO caller.
//! The effect was implemented, tested at module level, and unreachable from a
//! scene. These tests drive real `MainLoop::step` frames, so a pass means the
//! frame loop connects them, not that a helper exists.
//!
//! 🔑 The resolution is a POINT test, not a body overlap. An
//! `AudioStreamPlayer3D` is not a physics body and never appears in an area's
//! `_overlapping_bodies`, so the overlap machinery cannot answer this question
//! — which is why a separate `audio_bus_at` query exists.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
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

/// `Root -> {Zone (Area3D 4×4×4 at origin), Sfx (AudioStreamPlayer3D)}` with the
/// player at `player_at` and the zone carrying `zone_props`.
fn scene(player_at: Vector3, zone_props: &[(&str, Variant)]) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let zone = tree.add_child(root, Node::new("Zone", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone, Vector3::ZERO);
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0)));
    tree.add_child(zone, shape).unwrap();
    for (key, value) in zone_props {
        if let Some(node) = tree.get_node_mut(zone) {
            node.set_property(key, value.clone());
        }
    }

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer3D");
    sfx.set_property("autoplay", Variant::Bool(true));
    let sfx_id = tree.add_child(root, sfx).unwrap();
    node3d::set_position(&mut tree, sfx_id, player_at);

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, tone());
    (ml, sfx_id)
}

/// **The acceptance.** A player inside an overriding area is routed to its bus.
#[test]
fn a_player_inside_an_overriding_area_is_routed_to_its_bus() {
    let (mut ml, sfx) = scene(
        Vector3::ZERO,
        &[
            ("audio_bus_override", Variant::Bool(true)),
            ("audio_bus_name", Variant::String("Reverb".to_string())),
        ],
    );

    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        ml.audio_bridge_mut().node_bus(sfx).as_deref(),
        Some("Reverb"),
        "a voice inside an Area3D with audio_bus_override must be rerouted to \
         the area's bus. `None` means the player has no live voice at all, so \
         the sweep had nothing to reroute and this says nothing about routing"
    );
}

/// CONTROL: the same scene with the player OUTSIDE the area.
///
/// Without this, the assertion above would pass just as happily against a sweep
/// that rerouted every voice unconditionally, ignoring position entirely.
#[test]
fn a_player_outside_the_area_is_left_alone() {
    let (mut ml, sfx) = scene(
        // Well clear of the 4×4×4 box centred on the origin.
        Vector3::new(50.0, 0.0, 0.0),
        &[
            ("audio_bus_override", Variant::Bool(true)),
            ("audio_bus_name", Variant::String("Reverb".to_string())),
        ],
    );

    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    let bus = ml.audio_bridge_mut().node_bus(sfx);
    assert!(
        bus.is_some(),
        "fixture: the player must have a live voice, else this control proves \
         nothing about position"
    );
    assert_ne!(
        bus.as_deref(),
        Some("Reverb"),
        "a voice OUTSIDE the area must not be rerouted — the override is a \
         point test, not a global switch"
    );
}

/// CONTROL: inside the area, but with the override switched OFF.
///
/// Separates "the sweep respects position" from "the sweep respects the flag";
/// a sweep that ignored `audio_bus_override` would pass the position control
/// above and still be wrong.
#[test]
fn an_area_without_the_override_flag_does_not_reroute() {
    let (mut ml, sfx) = scene(
        Vector3::ZERO,
        &[
            ("audio_bus_override", Variant::Bool(false)),
            ("audio_bus_name", Variant::String("Reverb".to_string())),
        ],
    );

    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    assert_ne!(
        ml.audio_bridge_mut().node_bus(sfx).as_deref(),
        Some("Reverb"),
        "with `audio_bus_override` false the area must not touch the voice's bus"
    );
}
