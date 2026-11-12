//! pat-s3rrv.22 — `CanvasItem.top_level` detaches a 2D node from its parent.
//!
//! Godot implements this in `CanvasItem::get_parent_item`
//! (upstream/godot/scene/main/canvas_item.cpp): a top-level item reports **no**
//! parent item, so `get_global_transform` falls through to `get_transform()` —
//! the node's own local transform becomes its global one. It is how a HUD
//! element parented into a moving world node stays put, and how a detached
//! effect keeps world coordinates while living under a scrolling parent.
//!
//! Patina implemented the flag for 3D only (`node3d::get_global_transform_top_level`)
//! and `grep top_level crates/gdscene/src/node2d.rs` returned **nothing** — so
//! every 2D node composed the full ancestor chain unconditionally.
//!
//! ⚠️ The fix went into `get_global_transform` ITSELF, not into a parallel
//! `_top_level` variant as the 3D side did. A second function leaves every
//! existing caller silently on the wrong one, and in 2D those callers are the
//! entire shipped render path (`viewport_2d_render::collect_canvas_items`
//! reads `node2d::get_global_transform` directly), plus physics and input.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;

/// A parent at (100, 50) with a child at (10, 0).
fn parent_and_child() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree
        .add_child(root, Node::new("Parent", "Node2D"))
        .expect("parent adds");
    let child = tree
        .add_child(parent, Node::new("Child", "Node2D"))
        .expect("child adds");
    node2d::set_position(&mut tree, parent, Vector2::new(100.0, 50.0));
    node2d::set_position(&mut tree, child, Vector2::new(10.0, 0.0));
    (tree, parent, child)
}

fn approx(a: Vector2, b: Vector2) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

/// **The acceptance test**: the flag changes the composition, and its absence
/// leaves it alone.
#[test]
fn canvas_item_top_level_ignores_parent_transform() {
    // -- baseline: without the flag the chain composes ----------------------
    let (tree, _parent, child) = parent_and_child();
    let composed = node2d::get_global_position(&tree, child);
    assert!(
        approx(composed, Vector2::new(110.0, 50.0)),
        "a normal child composes with its parent: expected (110, 50), got {composed:?}. \
         If this fails the baseline is wrong and nothing below means anything",
    );

    // -- with the flag the parent contributes nothing ------------------------
    let (mut tree, _parent, child) = parent_and_child();
    node2d::set_top_level(&mut tree, child, true);
    let detached = node2d::get_global_position(&tree, child);
    assert!(
        approx(detached, Vector2::new(10.0, 0.0)),
        "a top_level child's global position IS its local one: expected \
         (10, 0), got {detached:?} — (110, 50) means the flag was ignored and \
         the parent chain was composed anyway",
    );
    assert!(
        node2d::is_top_level(&tree, child),
        "the flag must read back through the same property Godot uses",
    );
}

/// The walk stops **at** the top-level node, not before it (pat-s3rrv.22).
///
/// A grandchild under a top-level parent still composes with that parent — it
/// is only the ancestors *above* the flagged node that stop contributing.
/// Breaking before the push instead of after it would drop the flagged node's
/// own transform too, which reads as "top_level moves the node to the origin".
#[test]
fn a_grandchild_still_composes_with_its_top_level_parent() {
    let (mut tree, _parent, child) = parent_and_child();
    let grandchild = tree
        .add_child(child, Node::new("Grandchild", "Node2D"))
        .expect("grandchild adds");
    node2d::set_position(&mut tree, grandchild, Vector2::new(5.0, 5.0));
    node2d::set_top_level(&mut tree, child, true);

    let got = node2d::get_global_position(&tree, grandchild);
    assert!(
        approx(got, Vector2::new(15.0, 5.0)),
        "the grandchild composes with the top_level child (10,0) but NOT with \
         the grandparent (100,50): expected (15, 5), got {got:?}. (5, 5) would \
         mean the flagged node's own transform was dropped as well",
    );
}

/// Rotation stops at the same place the transform does (pat-s3rrv.22).
///
/// `get_global_rotation` sums local rotations up the chain rather than
/// decomposing the matrix, so it needs its own stop — otherwise a node's
/// reported rotation contradicts the transform used to draw it.
#[test]
fn global_rotation_stops_at_a_top_level_node() {
    let (mut tree, parent, child) = parent_and_child();
    node2d::set_rotation(&mut tree, parent, 1.0);
    node2d::set_rotation(&mut tree, child, 0.25);

    assert!(
        (node2d::get_global_rotation(&tree, child) - 1.25).abs() < 1e-5,
        "baseline: rotations sum along the chain",
    );

    node2d::set_top_level(&mut tree, child, true);
    let rot = node2d::get_global_rotation(&tree, child);
    assert!(
        (rot - 0.25).abs() < 1e-5,
        "a top_level node keeps only its OWN rotation: expected 0.25, got \
         {rot} — 1.25 means the sum walked past the flag while the transform \
         did not, so the two disagree about the same node",
    );
}

/// `set_global_position` round-trips through the flag (pat-s3rrv.22).
///
/// It inverts the *parent's* global transform to derive a local position. For a
/// top-level node there is no parent transform to invert, and using one puts
/// the node at the wrong place — the write and the read must agree.
#[test]
fn set_global_position_round_trips_for_a_top_level_node() {
    let (mut tree, _parent, child) = parent_and_child();
    node2d::set_top_level(&mut tree, child, true);

    node2d::set_global_position(&mut tree, child, Vector2::new(7.0, 3.0));
    let got = node2d::get_global_position(&tree, child);
    assert!(
        approx(got, Vector2::new(7.0, 3.0)),
        "setting a top_level node's global position must round-trip: asked for \
         (7, 3), read back {got:?}. (-93, -47) is the signature of the parent \
         transform being inverted when it should not be",
    );

    // …and the non-top-level case still round-trips too.
    let (mut tree, _parent, child) = parent_and_child();
    node2d::set_global_position(&mut tree, child, Vector2::new(7.0, 3.0));
    let got = node2d::get_global_position(&tree, child);
    assert!(approx(got, Vector2::new(7.0, 3.0)), "{got:?}");
}
