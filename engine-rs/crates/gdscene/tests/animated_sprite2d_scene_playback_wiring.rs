//! pat-0q2kx.100 — an `AnimatedSprite2D` from a `.tscn` actually animates.
//!
//! The pieces existed but did not meet: the `.tscn` parser flattened a
//! `SpriteFrames` sub-resource onto the node as `__sf_*` companion keys, and the
//! playback phase advanced `frame` from them — but nothing ever published the
//! **current frame's texture**, so every renderer saw a node with no texture and
//! drew nothing. `AnimatedSprite2D` was an invisible marker no matter how
//! correct the timing was.
//!
//! These drive a real scene through `MainLoop` and assert on the texture the
//! node exposes to the render path, plus the signals a game connects to.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::sprite_frames::{SpriteFrameEntry, SpriteFrames};
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A two-frame, 10fps, looping `SpriteFrames` on an autoplaying node — the
/// shape a `.tscn` produces once its sub-resource is flattened.
const SCENE: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Texture2D" path="res://frame_a.png" id="1_a"]
[ext_resource type="Texture2D" path="res://frame_b.png" id="1_b"]

[sub_resource type="SpriteFrames" id="SpriteFrames_1"]
animations = [{
"frames": [{
"duration": 1.0,
"texture": ExtResource("1_a")
}, {
"duration": 1.0,
"texture": ExtResource("1_b")
}],
"loop": true,
"name": &"default",
"speed": 10.0
}]

[node name="Root" type="Node2D"]

[node name="Anim" type="AnimatedSprite2D" parent="."]
sprite_frames = SubResource("SpriteFrames_1")
autoplay = "default"
"#;

fn node_prop(tree: &SceneTree, id: NodeId, key: &str) -> Variant {
    tree.get_node(id).unwrap().get_property(key)
}

fn texture_of(tree: &SceneTree, id: NodeId) -> String {
    match node_prop(tree, id, "texture") {
        Variant::String(s) => s,
        other => panic!("texture should be a String path, got {other:?}"),
    }
}

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core wiring test.** A `.tscn` autoplaying AnimatedSprite2D exposes
/// frame 0's texture at t=0 and frame 1's after 0.15s of stepping.
#[test]
fn animated_sprite2d_scene_playback_wiring() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // The ordinary scene-instancing entry point the runner and editor both use.
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let anim = find(&tree, "Anim");

    // The parser flattened the sub-resource onto the node; without that the
    // rest of this test would be checking nothing.
    assert!(
        matches!(node_prop(&tree, anim, "__sf_anims__"), Variant::PackedStringArray(ref v) if !v.is_empty()),
        "the .tscn SpriteFrames must reach the node as __sf_* companion keys"
    );

    let mut ml = MainLoop::new(tree);
    // One short step: autoplay starts and frame 0 is published.
    ml.step(0.001);
    assert_eq!(
        node_prop(ml.tree(), anim, "frame"),
        Variant::Int(0),
        "autoplay starts on frame 0"
    );
    let frame0 = texture_of(ml.tree(), anim);
    assert_eq!(
        frame0, "res://frame_a.png",
        "frame 0 must publish its own texture — this is the property every 2D \
         render path reads, and it was never written before"
    );

    // 10fps means a frame lasts 0.1s, so 0.15s of playback lands on frame 1.
    for _ in 0..15 {
        ml.step(0.01);
    }
    assert_eq!(
        node_prop(ml.tree(), anim, "frame"),
        Variant::Int(1),
        "0.15s at 10fps is frame 1"
    );
    let frame1 = texture_of(ml.tree(), anim);
    assert_eq!(
        frame1, "res://frame_b.png",
        "advancing a frame must change the published texture"
    );
    assert_ne!(
        frame0, frame1,
        "the two frames must draw different pixels, or 'it animates' is unfalsifiable"
    );
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/// A non-looping animation emits `animation_finished` exactly once, however
/// long playback keeps being stepped afterwards.
#[test]
fn non_looping_animation_finishes_exactly_once() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Anim", "AnimatedSprite2D");
    // Two frames at 10fps, non-looping, written as the companion keys the
    // parser produces.
    node.set_property(
        "__sf_anims__",
        Variant::PackedStringArray(vec!["default".to_string()]),
    );
    node.set_property("__sf_default_speed", Variant::Float(10.0));
    node.set_property("__sf_default_loop", Variant::Bool(false));
    node.set_property(
        "__sf_default_frames",
        Variant::PackedStringArray(vec![
            "res://frame_a.png".to_string(),
            "res://frame_b.png".to_string(),
        ]),
    );
    node.set_property("autoplay", Variant::String("default".to_string()));
    let anim = tree.add_child(root, node).unwrap();

    // A listener on the node itself, connected the way a game connects.
    let listener = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    let src = "\
extends Node
var finished = 0
var frames_seen = 0
func _on_finished():
    self.finished = self.finished + 1
func _on_frame_changed():
    self.frames_seen = self.frames_seen + 1
";
    tree.attach_script(
        listener,
        Box::new(GDScriptNodeInstance::from_source(src, listener).expect("listener parses")),
    );
    for (signal, method) in [
        ("animation_finished", "_on_finished"),
        ("frame_changed", "_on_frame_changed"),
    ] {
        tree.connect_signal(
            anim,
            signal,
            gdobject::signal::Connection::new(listener.object_id(), method),
        );
    }

    let mut ml = MainLoop::new(tree);
    // Well past the end of a 2-frame 10fps animation.
    for _ in 0..60 {
        ml.step(0.01);
    }

    let finished = match ml
        .tree()
        .get_script(listener)
        .unwrap()
        .get_property("finished")
    {
        Some(Variant::Int(i)) => i,
        other => panic!("finished should be an int, got {other:?}"),
    };
    assert_eq!(
        finished, 1,
        "animation_finished must fire exactly once for a non-looping animation, \
         not once per step after it ends"
    );

    // And the frame actually advanced on the way there.
    let seen = match ml
        .tree()
        .get_script(listener)
        .unwrap()
        .get_property("frames_seen")
    {
        Some(Variant::Int(i)) => i,
        other => panic!("frames_seen should be an int, got {other:?}"),
    };
    assert!(
        seen >= 1,
        "frame_changed must fire when the frame advances, got {seen}"
    );
}

// ---------------------------------------------------------------------------
// The consolidated model
// ---------------------------------------------------------------------------

/// Per-frame durations are **relative** to the animation's speed, as Godot
/// documents: a duration of 2.0 holds twice as long without changing the fps.
#[test]
fn frame_duration_is_relative_to_speed() {
    let mut frames = SpriteFrames::new();
    frames.add_animation_frames(
        "default",
        vec![
            SpriteFrameEntry::with_duration("res://a.png", 2.0),
            SpriteFrameEntry::new("res://b.png"),
        ],
        10.0,
        true,
    );
    let anim = frames.animation("default").expect("animation exists");

    assert_eq!(anim.frame_count(), 2);
    assert!(
        (anim.frame_seconds(0) - 0.2).abs() < 1e-6,
        "duration 2.0 at 10fps holds for 0.2s, got {}",
        anim.frame_seconds(0)
    );
    assert!(
        (anim.frame_seconds(1) - 0.1).abs() < 1e-6,
        "duration 1.0 at 10fps holds for 0.1s, got {}",
        anim.frame_seconds(1)
    );
    assert_eq!(frames.frame_texture("default", 1), Some("res://b.png"));
    // A speed of zero must not divide by zero; it simply never advances.
    frames.add_animation_frames(
        "still",
        vec![SpriteFrameEntry::new("res://a.png")],
        0.0,
        false,
    );
    assert_eq!(frames.animation("still").unwrap().frame_seconds(0), 0.0);
}
