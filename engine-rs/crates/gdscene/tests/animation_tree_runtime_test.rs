//! pat-u3l8c.96 — AnimationTree drives at runtime.
//!
//! App-level (integration) tests for the AnimationTree runtime. Before this
//! bead the blend math existed but was never executed against nodes, and the
//! blend-space clocks never advanced (BlendSpace1D/2D sampled at t=0 always).
//!
//! These tests assert the two acceptance criteria via the crate's public API:
//!   1. A state machine drives a property animation on a real SceneTree node.
//!   2. A 1D blend space blends two clips by its position parameter AND plays
//!      them forward over time (the elapsed clock advances).
//!
//! They live in `tests/` (an integration `--test` binary) rather than as
//! `#[cfg(test)]` unit tests so verification compiles `gdscene` as a plain
//! library — mirroring how the sibling `animation_targeting_test` is run.

use gdscene::animation::{Animation, AnimationTrack, KeyFrame};
use gdscene::animation_tree::{
    advance_and_apply, AnimationNode, AnimationTree, BlendSpace1D, StateMachine,
};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// An animation that ramps property `x` linearly from `v0` to `v1` over 1s.
fn ramp_anim(name: &str, v0: f64, v1: f64) -> Animation {
    let mut anim = Animation::new(name, 1.0);
    let mut track = AnimationTrack::new("x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(v0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(v1)));
    anim.tracks.push(track);
    anim
}

fn float_of(values: &[(String, Variant)], prop: &str) -> f64 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Float(f) = v {
                return *f;
            }
        }
    }
    panic!("no float for {prop}");
}

#[test]
fn blend_space_1d_blends_by_position_and_plays_over_time() {
    let mut tree = AnimationTree::new();
    // Two ramp clips: A goes 0->10, B goes 100->110 over 1s.
    tree.add_animation(ramp_anim("a", 0.0, 10.0));
    tree.add_animation(ramp_anim("b", 100.0, 110.0));

    let mut bs = BlendSpace1D::new();
    bs.add_point(0.0, "a");
    bs.add_point(1.0, "b");
    bs.blend_position = 0.5; // midway between the two clips
    tree.set_root(AnimationNode::BlendSpace1D(bs));

    // Blend by position: midway between clip A (start 0) and clip B (start 100).
    let v0 = float_of(&tree.get_current_values(), "x");
    assert!(
        (40.0..=60.0).contains(&v0),
        "blend-by-position at start: v0={v0}"
    );

    // Over time: advancing the tree plays both clips forward (they ramp up),
    // so the blended value increases — previously it was frozen at t=0.
    tree.advance(0.5);
    let v_mid = float_of(&tree.get_current_values(), "x");
    assert!(
        v_mid > v0 + 1.0,
        "blend-space clips must play over time: v0={v0} v_mid={v_mid}"
    );
}

#[test]
fn state_machine_drives_node_property_over_time() {
    let mut scene = SceneTree::new();
    let root = scene.root_id();
    let node = scene.add_child(root, Node::new("N", "Node")).unwrap();

    // A state machine whose current state plays a clip that ramps "x" 0->42.
    let mut tree = AnimationTree::new();
    tree.add_animation(ramp_anim("run", 0.0, 42.0));
    let mut sm = StateMachine::new();
    sm.states.insert(
        "run".into(),
        AnimationNode::Animation {
            name: "run".into(),
            position: 0.0,
        },
    );
    sm.current = "run".into();
    tree.set_root(AnimationNode::StateMachine(sm));

    // Drive the tree at runtime and apply its pose to the node.
    advance_and_apply(&mut tree, &mut scene, node, 0.5);

    // The state machine drove the node's "x" property over time
    // (0 -> 42 over 1s, so ~21 at 0.5s).
    match scene.get_node(node).unwrap().get_property("x") {
        Variant::Float(x) => assert!((x - 21.0).abs() < 1.0, "x={x}"),
        other => panic!("x not driven by AnimationTree: {other:?}"),
    }
}
