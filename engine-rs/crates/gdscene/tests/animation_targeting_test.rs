//! pat-u3l8c.94 — Animation track `node_path` targeting.
//!
//! Before this bead `AnimationTrack.node_path` was never read: every sampled
//! value was written to the AnimationPlayer's own owner node, so an animation
//! could not animate any other node. These tests drive the real
//! `process_animations` pipeline and assert that one AnimationPlayer animates
//! properties on two *different* nodes, resolved relative to the player's
//! `root_node` — while an empty `node_path` still targets the owner (Godot's
//! resolution rules, and the pre-existing behavior).

use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn const_track(node_path: &str, property: &str, value: f64) -> AnimationTrack {
    let mut t = AnimationTrack::with_node(node_path, property);
    t.add_keyframe(KeyFrame::linear(0.0, Variant::Float(value)));
    t.add_keyframe(KeyFrame::linear(1.0, Variant::Float(value)));
    t
}

#[test]
fn animation_targets_two_sibling_nodes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // A player node with two child sprites (siblings of each other). With the
    // default root_node "." the track paths resolve relative to the player.
    let player_node = tree
        .add_child(root, Node::new("Player", "AnimationPlayer"))
        .unwrap();
    let sprite_a = tree
        .add_child(player_node, Node::new("SpriteA", "Node2D"))
        .unwrap();
    let sprite_b = tree
        .add_child(player_node, Node::new("SpriteB", "Node2D"))
        .unwrap();

    let mut anim = Animation::new("fade", 1.0);
    anim.tracks.push(const_track("SpriteA", "opacity", 0.2));
    anim.tracks.push(const_track("SpriteB", "opacity", 0.8));

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fade");
    tree.attach_animation_player(player_node, player).unwrap();

    tree.process_animations(0.5);

    // Each sibling receives its own track's value.
    match tree.get_node(sprite_a).unwrap().get_property("opacity") {
        Variant::Float(f) => assert!((f - 0.2).abs() < 1e-4, "SpriteA opacity {f}"),
        v => panic!("SpriteA opacity not set (still targets player?), got {v:?}"),
    }
    match tree.get_node(sprite_b).unwrap().get_property("opacity") {
        Variant::Float(f) => assert!((f - 0.8).abs() < 1e-4, "SpriteB opacity {f}"),
        v => panic!("SpriteB opacity not set, got {v:?}"),
    }

    // The player node itself must NOT receive the track values — the bug fixed.
    assert!(
        matches!(
            tree.get_node(player_node).unwrap().get_property("opacity"),
            Variant::Nil
        ),
        "player node should not be animated by node-path-targeted tracks"
    );
}

#[test]
fn root_node_parent_targets_player_siblings() {
    // With root_node = "..", track paths resolve against the player's parent,
    // so the targets are siblings of the AnimationPlayer (Godot's default).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("UI", "Node2D")).unwrap();
    let player_node = tree
        .add_child(parent, Node::new("Anim", "AnimationPlayer"))
        .unwrap();
    let label = tree
        .add_child(parent, Node::new("Label", "Node2D"))
        .unwrap();

    let mut anim = Animation::new("a", 1.0);
    anim.tracks.push(const_track("Label", "opacity", 0.5));

    let mut player = AnimationPlayer::new();
    player.root_node = "..".to_string();
    player.add_animation(anim);
    player.play("a");
    tree.attach_animation_player(player_node, player).unwrap();

    tree.process_animations(0.5);

    match tree.get_node(label).unwrap().get_property("opacity") {
        Variant::Float(f) => assert!((f - 0.5).abs() < 1e-4, "Label opacity {f}"),
        v => panic!("Label (player sibling) not animated, got {v:?}"),
    }
}

#[test]
fn empty_node_path_targets_owner_node() {
    // Regression guard: a track with an empty node_path still animates the
    // player's own owner node (pre-existing behavior).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("P", "AnimationPlayer"))
        .unwrap();

    let mut anim = Animation::new("a", 1.0);
    let mut track = AnimationTrack::new("visible_ratio"); // node_path == ""
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(0.0)));
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("a");
    tree.attach_animation_player(player_node, player).unwrap();

    tree.process_animations(0.5);

    match tree
        .get_node(player_node)
        .unwrap()
        .get_property("visible_ratio")
    {
        Variant::Float(f) => assert!((f - 0.5).abs() < 1e-4, "owner opacity {f}"),
        v => panic!("empty node_path should target owner, got {v:?}"),
    }
}
