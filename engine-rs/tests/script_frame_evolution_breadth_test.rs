//! pat-6a8: Broadened script frame-evolution oracle coverage.
//!
//! Tests property evolution across multiple frames with conditional logic,
//! function calls, and multi-node interactions.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_tree_with_children(names: &[(&str, &str)]) -> (MainLoop, Vec<gdscene::NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for &(name, class) in names {
        let node = Node::new(name, class);
        let id = tree.add_child(root, node).unwrap();
        ids.push(id);
    }
    (MainLoop::new(tree), ids)
}

// ===========================================================================
// 1. Conditional property update — only moves when "enabled"
// ===========================================================================

#[test]
fn conditional_position_update() {
    let (mut ml, ids) = make_tree_with_children(&[("Mover", "Node2D")]);
    let nid = ids[0];

    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));
    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("enabled", Variant::Bool(false));

    for frame in 0..10 {
        ml.step(1.0 / 60.0);

        let enabled = ml.tree().get_node(nid).unwrap().get_property("enabled");
        let is_enabled = matches!(enabled, Variant::Bool(true));

        // Enable after frame 5
        if frame == 5 {
            ml.tree_mut()
                .get_node_mut(nid)
                .unwrap()
                .set_property("enabled", Variant::Bool(true));
        }

        if is_enabled {
            let pos = match ml.tree().get_node(nid).unwrap().get_property("position") {
                Variant::Vector2(v) => v,
                _ => Vector2::ZERO,
            };
            ml.tree_mut()
                .get_node_mut(nid)
                .unwrap()
                .set_property("position", Variant::Vector2(Vector2::new(pos.x + 5.0, 0.0)));
        }
    }

    // Enabled at frame 6..9 = 4 frames of movement = 20 units
    let final_pos = ml.tree().get_node(nid).unwrap().get_property("position");
    assert_eq!(final_pos, Variant::Vector2(Vector2::new(20.0, 0.0)));
}

// ===========================================================================
// 2. Multiple properties with conditional logic
// ===========================================================================

#[test]
fn rotation_accumulates_while_visible() {
    let (mut ml, ids) = make_tree_with_children(&[("Spinner", "Node2D")]);
    let nid = ids[0];

    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("rotation", Variant::Float(0.0));
    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("visible", Variant::Bool(true));

    for frame in 0..8 {
        ml.step(1.0 / 60.0);

        // Become invisible at frame 4
        if frame == 4 {
            ml.tree_mut()
                .get_node_mut(nid)
                .unwrap()
                .set_property("visible", Variant::Bool(false));
        }

        let visible = matches!(
            ml.tree().get_node(nid).unwrap().get_property("visible"),
            Variant::Bool(true)
        );

        if visible {
            let rot = match ml.tree().get_node(nid).unwrap().get_property("rotation") {
                Variant::Float(r) => r,
                _ => 0.0,
            };
            ml.tree_mut()
                .get_node_mut(nid)
                .unwrap()
                .set_property("rotation", Variant::Float(rot + 0.1));
        }
    }

    // Rotation should have accumulated for frames 0..3 = 4 increments = 0.4
    let final_rot = match ml.tree().get_node(nid).unwrap().get_property("rotation") {
        Variant::Float(r) => r,
        _ => panic!("expected Float"),
    };
    assert!((final_rot - 0.4).abs() < 1e-6);
}

// ===========================================================================
// 3. Two nodes interacting — one reads the other's state
// ===========================================================================

#[test]
fn two_nodes_position_chase() {
    let (mut ml, ids) = make_tree_with_children(&[("Leader", "Node2D"), ("Follower", "Node2D")]);
    let leader = ids[0];
    let follower = ids[1];

    ml.tree_mut()
        .get_node_mut(leader)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    ml.tree_mut()
        .get_node_mut(follower)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));

    for _ in 0..5 {
        ml.step(1.0 / 60.0);

        // Leader moves right
        let l_pos = match ml.tree().get_node(leader).unwrap().get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        };
        ml.tree_mut().get_node_mut(leader).unwrap().set_property(
            "position",
            Variant::Vector2(Vector2::new(l_pos.x + 10.0, 0.0)),
        );

        // Follower chases: moves 50% toward leader's position
        let f_pos = match ml
            .tree()
            .get_node(follower)
            .unwrap()
            .get_property("position")
        {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        };
        let new_f_x = f_pos.x + (l_pos.x - f_pos.x) * 0.5;
        ml.tree_mut()
            .get_node_mut(follower)
            .unwrap()
            .set_property("position", Variant::Vector2(Vector2::new(new_f_x, 0.0)));
    }

    // Leader at 100 + 50 = 150
    let leader_final = match ml.tree().get_node(leader).unwrap().get_property("position") {
        Variant::Vector2(v) => v.x,
        _ => panic!("expected Vector2"),
    };
    assert!((leader_final - 150.0).abs() < 0.01);

    // Follower should have been chasing — should be > 0 and < leader
    let follower_final = match ml
        .tree()
        .get_node(follower)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v.x,
        _ => panic!("expected Vector2"),
    };
    assert!(follower_final > 0.0, "follower should have moved");
    assert!(follower_final < leader_final, "follower should lag behind");
}

// ===========================================================================
// 4. Scale evolution with frame-dependent logic
// ===========================================================================

#[test]
fn scale_pulse_animation() {
    let (mut ml, ids) = make_tree_with_children(&[("Pulsing", "Node2D")]);
    let nid = ids[0];

    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("scale", Variant::Vector2(Vector2::new(1.0, 1.0)));

    let mut scales = Vec::new();
    for frame in 0..6 {
        ml.step(1.0 / 60.0);

        // Alternate between growing and shrinking
        let grow = frame % 2 == 0;
        let current = match ml.tree().get_node(nid).unwrap().get_property("scale") {
            Variant::Vector2(v) => v,
            _ => Vector2::ONE,
        };
        let factor = if grow { 1.1 } else { 0.9 };
        let new_scale = Vector2::new(current.x * factor, current.y * factor);
        ml.tree_mut()
            .get_node_mut(nid)
            .unwrap()
            .set_property("scale", Variant::Vector2(new_scale));
        scales.push(new_scale.x);
    }

    // Scale should have alternated
    assert!(scales[0] > 1.0); // grew
    assert!(scales[1] < scales[0]); // shrank
    assert!(scales[2] > scales[1]); // grew
}

// ===========================================================================
// 5. Counter property incremented each frame
// ===========================================================================

#[test]
fn frame_counter_property() {
    let (mut ml, ids) = make_tree_with_children(&[("Counter", "Node")]);
    let nid = ids[0];

    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("count", Variant::Int(0));

    for _ in 0..100 {
        ml.step(1.0 / 60.0);
        let count = match ml.tree().get_node(nid).unwrap().get_property("count") {
            Variant::Int(n) => n,
            _ => 0,
        };
        ml.tree_mut()
            .get_node_mut(nid)
            .unwrap()
            .set_property("count", Variant::Int(count + 1));
    }

    let final_count = ml.tree().get_node(nid).unwrap().get_property("count");
    assert_eq!(final_count, Variant::Int(100));
}

// ===========================================================================
// 6. String property evolves (simulating state machine names)
// ===========================================================================

#[test]
fn state_machine_property_transitions() {
    let (mut ml, ids) = make_tree_with_children(&[("Actor", "Node2D")]);
    let nid = ids[0];

    ml.tree_mut()
        .get_node_mut(nid)
        .unwrap()
        .set_property("state", Variant::String("idle".into()));

    let states = ["idle", "run", "jump", "fall", "land", "idle"];
    for (_frame, &next_state) in states.iter().enumerate().skip(1) {
        ml.step(1.0 / 60.0);
        ml.tree_mut()
            .get_node_mut(nid)
            .unwrap()
            .set_property("state", Variant::String(next_state.into()));
    }

    let final_state = ml.tree().get_node(nid).unwrap().get_property("state");
    assert_eq!(final_state, Variant::String("idle".into()));
}
