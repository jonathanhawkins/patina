//! pat-0q2kx.53 acceptance: an `AnimationTree` with a `BlendTree` root —
//! a `TimeScale` over an `Add2` of two clips, with a filter excluding one
//! track — produces the correct per-property mix through the ordinary
//! advance/sample path.
//!
//! Godot ground truth: `upstream/godot/scene/animation/animation_blend_tree.cpp`
//! (`AnimationNodeAdd2`, `AnimationNodeTimeScale`, `AnimationNodeBlendTree`).
//!
//! Patina's `AnimationNode` enum had 7 variants and no arm for any of these —
//! the class NAMES were registered in ClassDB with no evaluator behind them, so
//! a blend tree in a scene was inert. This exercises the wired evaluators.

use gdscene::animation::{Animation, AnimationTrack, KeyFrame};
use gdscene::animation_tree::{AnimationLeafOptions, AnimationNode, AnimationTree, BlendTree};
use gdvariant::Variant;

/// A one-second clip holding `value` on a track named `track`.
fn clip(name: &str, track: &str, value: f64) -> Animation {
    let mut anim = Animation::new(name, 1.0);
    let mut t = AnimationTrack::new(track);
    t.add_keyframe(KeyFrame::linear(0.0, Variant::Float(value)));
    t.add_keyframe(KeyFrame::linear(1.0, Variant::Float(value)));
    anim.tracks.push(t);
    anim
}

fn leaf(name: &str) -> AnimationNode {
    AnimationNode::Animation {
        name: name.into(),
        position: 0.0,
        opts: AnimationLeafOptions::default(),
    }
}

fn float_of(values: &[(String, Variant)], prop: &str) -> f64 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Float(f) = v {
                return *f;
            }
        }
    }
    panic!("no float for {prop} in {values:?}");
}

/// **The acceptance test.**
#[test]
fn animation_tree_blend_tree_nodes_runtime() {
    let mut tree = AnimationTree::new();

    // The base clip drives two tracks; the filter below drops one of them.
    let mut base = clip("base", "x", 1.0);
    let mut extra = AnimationTrack::new("y");
    extra.add_keyframe(KeyFrame::linear(0.0, Variant::Float(5.0)));
    extra.add_keyframe(KeyFrame::linear(1.0, Variant::Float(5.0)));
    base.tracks.push(extra);
    tree.add_animation(base);
    tree.add_animation(clip("add", "x", 2.0));

    let graph = AnimationNode::TimeScale {
        scale: 2.0,
        child: Box::new(AnimationNode::Add2 {
            base: Box::new(leaf("base")),
            add: Box::new(leaf("add")),
            amount: 0.5,
        }),
    };
    tree.set_root(AnimationNode::BlendTree(
        BlendTree::new("out")
            .with_node("out", graph)
            .with_filter("out", vec!["y".to_string()]),
    ));

    tree.advance(0.1);
    let values = tree.get_current_values();

    // ⚠️ ADDITIVE, not interpolating: base(1.0) + add(2.0) * 0.5 = 2.0. A
    // Blend2 at the same weight would have produced 1.5.
    assert_eq!(
        float_of(&values, "x"),
        2.0,
        "Add2 must layer the second input on top of the base"
    );

    assert!(
        !values.iter().any(|(p, _)| p == "y"),
        "the filtered track must not reach the tree's output: {values:?}"
    );

    // The TimeScale really scaled the delta on the way DOWN the graph: 0.1s of
    // wall clock at 2x leaves the clip at 0.2s.
    match tree.root.as_ref().expect("root is set") {
        AnimationNode::BlendTree(bt) => match bt.nodes.get("out") {
            Some(AnimationNode::TimeScale { child, .. }) => match child.as_ref() {
                AnimationNode::Add2 { base, .. } => match base.as_ref() {
                    AnimationNode::Animation { position, .. } => {
                        assert!(
                            (position - 0.2).abs() < 1e-6,
                            "0.1s at 2x scale must advance the clip to 0.2, got {position}"
                        );
                    }
                    other => panic!("expected an Animation leaf, got {other:?}"),
                },
                other => panic!("expected Add2 under the TimeScale, got {other:?}"),
            },
            other => panic!("expected a TimeScale output node, got {other:?}"),
        },
        other => panic!("expected a BlendTree root, got {other:?}"),
    }
}
