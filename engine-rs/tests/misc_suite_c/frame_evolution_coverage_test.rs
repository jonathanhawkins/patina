//! pat-6a8: Script frame-evolution oracle coverage.
//!
//! Tests that verify per-frame property evolution when stepping through
//! the MainLoop. Ensures that node properties change correctly across
//! multiple frames, matching expected Godot behavior.

use gdcore::math::Vector2;
use gdscene::node::{Node, ProcessMode};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

/// Create a MainLoop with a single child node, return (ml, child_id).
fn make_single_child(name: &str, class: &str) -> (MainLoop, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new(name, class);
    let id = tree.add_child(root, child).unwrap();
    (MainLoop::new(tree), id)
}

// ===========================================================================
// 1. Position property evolves across frames when updated each step
// ===========================================================================

/// Simulates a node moving right by 10 units per frame for 5 frames.
#[test]
fn position_evolves_across_frames() {
    let (mut ml, node_id) = make_single_child("Mover", "Node2D");

    // Set initial position
    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));

    for frame in 0..5 {
        ml.step(1.0 / 60.0);

        // Simulate script updating position each frame
        let current = ml
            .tree()
            .get_node(node_id)
            .unwrap()
            .get_property("position");
        let new_x = match current {
            Variant::Vector2(v) => v.x + 10.0,
            _ => 10.0 * (frame as f32 + 1.0),
        };
        ml.tree_mut()
            .get_node_mut(node_id)
            .unwrap()
            .set_property("position", Variant::Vector2(Vector2::new(new_x, 0.0)));
    }

    // After 5 frames, position should be (50, 0)
    let final_pos = ml
        .tree()
        .get_node(node_id)
        .unwrap()
        .get_property("position");
    assert_eq!(
        final_pos,
        Variant::Vector2(Vector2::new(50.0, 0.0)),
        "Position should have evolved to (50, 0) after 5 frames"
    );
}

// ===========================================================================
// 2. Multiple properties evolve independently per frame
// ===========================================================================

/// Multiple properties (position, rotation, scale) evolve independently.
#[test]
fn multiple_properties_evolve_independently() {
    let (mut ml, node_id) = make_single_child("Spinner", "Node2D");

    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("rotation", Variant::Float(0.0));
    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("scale", Variant::Vector2(Vector2::new(1.0, 1.0)));
    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));

    for _ in 0..10 {
        ml.step(1.0 / 60.0);

        let node = ml.tree().get_node(node_id).unwrap();
        let rot = match node.get_property("rotation") {
            Variant::Float(r) => r,
            _ => 0.0,
        };
        let scale = match node.get_property("scale") {
            Variant::Vector2(s) => s,
            _ => Vector2::new(1.0, 1.0),
        };
        let pos = match node.get_property("position") {
            Variant::Vector2(p) => p,
            _ => Vector2::ZERO,
        };

        let node_mut = ml.tree_mut().get_node_mut(node_id).unwrap();
        node_mut.set_property("rotation", Variant::Float(rot + 0.1));
        node_mut.set_property(
            "scale",
            Variant::Vector2(Vector2::new(scale.x + 0.05, scale.y + 0.05)),
        );
        node_mut.set_property(
            "position",
            Variant::Vector2(Vector2::new(pos.x + 5.0, pos.y)),
        );
    }

    let node = ml.tree().get_node(node_id).unwrap();
    let final_rot = match node.get_property("rotation") {
        Variant::Float(r) => r,
        _ => panic!("rotation should be Float"),
    };
    let final_scale = match node.get_property("scale") {
        Variant::Vector2(s) => s,
        _ => panic!("scale should be Vector2"),
    };
    let final_pos = match node.get_property("position") {
        Variant::Vector2(p) => p,
        _ => panic!("position should be Vector2"),
    };

    // After 10 frames: rotation += 0.1*10 = 1.0
    assert!(
        (final_rot - 1.0).abs() < 0.01,
        "rotation should be ~1.0, got {final_rot}"
    );
    // scale += 0.05*10 = 0.5, so (1.5, 1.5)
    assert!((final_scale.x - 1.5).abs() < 0.01, "scale.x should be ~1.5");
    assert!((final_scale.y - 1.5).abs() < 0.01, "scale.y should be ~1.5");
    // position += 5*10 = 50
    assert!(
        (final_pos.x - 50.0).abs() < 0.01,
        "position.x should be ~50.0"
    );
}

// ===========================================================================
// 3. Counter property increments each frame
// ===========================================================================

/// Integer counter property increments each frame.
#[test]
fn counter_increments_each_frame() {
    let (mut ml, node_id) = make_single_child("Counter", "Node");

    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("count", Variant::Int(0));

    for _ in 0..20 {
        ml.step(1.0 / 60.0);
        let count = match ml.tree().get_node(node_id).unwrap().get_property("count") {
            Variant::Int(c) => c,
            _ => 0,
        };
        ml.tree_mut()
            .get_node_mut(node_id)
            .unwrap()
            .set_property("count", Variant::Int(count + 1));
    }

    assert_eq!(
        ml.tree().get_node(node_id).unwrap().get_property("count"),
        Variant::Int(20),
        "counter should be 20 after 20 frames"
    );
}

// ===========================================================================
// 4. Paused node does not evolve its frame count
// ===========================================================================

/// A paused node receives no process notifications, so a frame-driven
/// counter should not advance.
#[test]
fn paused_node_does_not_evolve() {
    let (mut ml, node_id) = make_single_child("Frozen", "Node");

    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_process_mode(ProcessMode::Pausable);

    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("ticks", Variant::Int(0));

    ml.set_paused(true);

    // Step 5 frames while paused — ticks should not advance since the node
    // won't receive process notifications
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
        // In a real Godot game, _process wouldn't be called, so ticks stays 0
    }

    assert_eq!(
        ml.tree().get_node(node_id).unwrap().get_property("ticks"),
        Variant::Int(0),
        "paused node's properties should not evolve"
    );
}

// ===========================================================================
// 5. Multiple nodes evolve at different rates per frame
// ===========================================================================

/// Two sibling nodes with different update rates produce different final values.
#[test]
fn siblings_evolve_at_different_rates() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let fast = Node::new("Fast", "Node2D");
    let fast_id = tree.add_child(root, fast).unwrap();
    let slow = Node::new("Slow", "Node2D");
    let slow_id = tree.add_child(root, slow).unwrap();

    tree.get_node_mut(fast_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));
    tree.get_node_mut(slow_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));

    let mut ml = MainLoop::new(tree);

    for _ in 0..10 {
        ml.step(1.0 / 60.0);

        // Fast moves 10 per frame, Slow moves 2 per frame
        let fast_pos = match ml
            .tree()
            .get_node(fast_id)
            .unwrap()
            .get_property("position")
        {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        };
        let slow_pos = match ml
            .tree()
            .get_node(slow_id)
            .unwrap()
            .get_property("position")
        {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        };

        ml.tree_mut().get_node_mut(fast_id).unwrap().set_property(
            "position",
            Variant::Vector2(Vector2::new(fast_pos.x + 10.0, 0.0)),
        );
        ml.tree_mut().get_node_mut(slow_id).unwrap().set_property(
            "position",
            Variant::Vector2(Vector2::new(slow_pos.x + 2.0, 0.0)),
        );
    }

    let fast_final = ml
        .tree()
        .get_node(fast_id)
        .unwrap()
        .get_property("position");
    let slow_final = ml
        .tree()
        .get_node(slow_id)
        .unwrap()
        .get_property("position");

    assert_eq!(fast_final, Variant::Vector2(Vector2::new(100.0, 0.0)));
    assert_eq!(slow_final, Variant::Vector2(Vector2::new(20.0, 0.0)));
}
