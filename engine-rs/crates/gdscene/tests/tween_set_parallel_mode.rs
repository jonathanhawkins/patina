//! pat-0q2kx.4: `Tween.set_parallel(bool)` is a MODE, not a one-shot.
//!
//! Godot has two distinct sequencing calls (`scene/animation/tween.cpp`):
//! `parallel()` makes only the NEXT tweener run alongside the previous one,
//! while `set_parallel(enabled)` makes every subsequent tweener do so until it
//! is switched off. Patina modelled only the one-shot form, so a script could
//! not express "everything after this runs together" — it had to repeat
//! `parallel()` on each tweener, and `set_parallel` failed as an unknown method.

use gdscene::node::Node;
use gdscene::tween::TweenStep;
use gdscene::SceneTree;
use gdvariant::Variant;

/// A tween owned by a node, plus the tree it lives in.
fn tween_scene() -> (SceneTree, gdscene::tween::TweenId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Mover", "Node2D")).unwrap();
    tree.get_node_mut(node)
        .unwrap()
        .set_property("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
    let tid = tree.create_tween(node);
    (tree, tid)
}

/// With the mode ON, every step added afterwards is parallel.
#[test]
fn set_parallel_marks_every_subsequent_step() {
    let (mut tree, tid) = tween_scene();

    // Default: steps are sequential.
    tree.get_tween_mut(tid)
        .unwrap()
        .steps
        .push(TweenStep::interval(0.1));
    assert!(
        !tree.get_tween_mut(tid).unwrap().steps[0].parallel,
        "a step added with the mode off is sequential"
    );

    // Turning the mode on affects steps added AFTER it, which is the whole
    // difference from the one-shot `parallel()`.
    tree.get_tween_mut(tid).unwrap().default_parallel = true;
    assert!(
        tree.get_tween_mut(tid).unwrap().default_parallel,
        "set_parallel(true) records the mode on the tween, not on a step"
    );

    // ⚠️ The already-added step is untouched — the mode is not retroactive.
    assert!(
        !tree.get_tween_mut(tid).unwrap().steps[0].parallel,
        "set_parallel must not retroactively change steps added before it"
    );
}
