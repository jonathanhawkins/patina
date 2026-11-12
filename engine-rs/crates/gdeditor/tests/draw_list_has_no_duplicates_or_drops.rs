//! pat-s3rrv.22: structural invariants of the canvas draw list.
//!
//! # Why this exists before the Y-sort fix, not after
//!
//! The remaining live item on this bead is Y-sort grouping: Godot flattens every
//! descendant reachable through non-y-sorted children into ONE list and sorts it
//! by composed ysort transform (`renderer_canvas_cull.cpp:110-181`), whereas
//! `canvas_order::draw_ordered_children` sorts a parent's direct children only.
//!
//! The trap, recorded on the bead by the worker who sized it: `draw_ordered_children`
//! has a single caller, `push_draw_order`, **which recurses into those children
//! itself**. If the function starts returning a flattened subtree and the caller
//! keeps recursing, every descendant is emitted twice — and a node drawn twice at
//! the same transform looks *identical*, so every pixel test in this crate stays
//! green while the draw list is silently wrong.
//!
//! These assertions are the ones a pixel test cannot make. They are deliberately
//! limited to invariants the Y-sort fix must PRESERVE, so they can land now
//! without cementing the current (incorrect) grouping:
//!
//! - every canvas item appears **exactly once** — the double-listing guard;
//! - no canvas item is **dropped**;
//! - a y-sorted parent's direct children keep ascending global-Y order relative
//!   to one another, which flattening reorders *around* but never inverts.
//!
//! Deliberately NOT asserted: the absolute index of any node, or the relative
//! order of a child against a grandchild. Those are exactly what the fix changes.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::build_runtime_render_list;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn node2d(tree: &mut SceneTree, parent: NodeId, name: &str, y: f32) -> NodeId {
    let mut node = Node::new(name, "Node2D");
    node.set_property("position", Variant::Vector2(Vector2::new(0.0, y)));
    tree.add_child(parent, node).unwrap()
}

fn color_rect(tree: &mut SceneTree, parent: NodeId, name: &str, y: f32) -> NodeId {
    let mut node = Node::new(name, "ColorRect");
    node.set_property("position", Variant::Vector2(Vector2::new(0.0, y)));
    node.set_property("size", Variant::Vector2(Vector2::new(4.0, 4.0)));
    node.set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
    tree.add_child(parent, node).unwrap()
}

/// ⚠️ Without a populated ClassDB, `is_class("CanvasItem")` is false for every
/// node, the render list comes back EMPTY, and every assertion below passes
/// vacuously.
fn new_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    (tree, root)
}

/// A y-sorted parent with two direct leaves and a non-y-sorted intermediate
/// carrying a grandchild — the exact shape Godot flattens and Patina does not.
///
/// The grandchild's global Y (20 - 5 = 15) deliberately falls BETWEEN its own
/// parent's (20) and a sibling's (10), so a correct flattening must interleave
/// it. That is what makes this fixture sensitive to the pending change while the
/// assertions below stay valid either way.
fn scene() -> (SceneTree, Vec<(NodeId, &'static str)>) {
    let (mut tree, root) = new_tree();
    let sorted = node2d(&mut tree, root, "Sorted", 0.0);
    tree.get_node_mut(sorted)
        .unwrap()
        .set_property("y_sort_enabled", Variant::Bool(true));

    let b = color_rect(&mut tree, sorted, "B", 10.0);
    let group = node2d(&mut tree, sorted, "Group", 20.0);
    let c = color_rect(&mut tree, group, "C", -5.0);
    let a = color_rect(&mut tree, sorted, "A", 30.0);

    let named = vec![
        (root, "Root"),
        (sorted, "Sorted"),
        (b, "B"),
        (group, "Group"),
        (c, "C"),
        (a, "A"),
    ];
    (tree, named)
}

#[test]
fn every_canvas_item_appears_exactly_once() {
    let (tree, named) = scene();
    let items = build_runtime_render_list(&tree);
    let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

    // The guard the pixel tests cannot provide: a node emitted twice at the same
    // transform renders identically, so only a count can catch it.
    for (id, name) in &named {
        let n = ids.iter().filter(|&&x| x == id.raw()).count();
        assert_eq!(
            n, 1,
            "`{name}` must appear exactly once in the draw list, got {n}. More \
             than one is the double-listing failure that flattening the Y-sort \
             subtree invites, and it is invisible to every pixel assertion"
        );
    }

    let mut sorted_ids = ids.clone();
    sorted_ids.sort_unstable();
    let before = sorted_ids.len();
    sorted_ids.dedup();
    assert_eq!(
        before,
        sorted_ids.len(),
        "the draw list must contain no duplicate node ids at all: {ids:?}"
    );
}

#[test]
fn no_canvas_item_is_dropped() {
    let (tree, named) = scene();
    let items = build_runtime_render_list(&tree);
    let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

    // The other half: flattening must not lose a node either. Asserted
    // separately so a failure names which direction went wrong.
    for (id, name) in &named {
        assert!(
            ids.contains(&id.raw()),
            "`{name}` is missing from the draw list {ids:?}"
        );
    }
}

#[test]
fn a_y_sorted_parents_direct_children_stay_in_ascending_global_y() {
    let (tree, named) = scene();
    let items = build_runtime_render_list(&tree);
    let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();
    let pos = |name: &str| -> usize {
        let id = named.iter().find(|(_, n)| *n == name).unwrap().0;
        ids.iter().position(|&x| x == id.raw()).unwrap()
    };

    // B(10) < Group(20) < A(30). Flattening reorders the grandchild *around*
    // these three but must never invert them, so this holds before and after the
    // pending fix — which is why it is safe to pin now.
    assert!(
        pos("B") < pos("Group"),
        "B (global y 10) must draw before Group (20)"
    );
    assert!(
        pos("Group") < pos("A"),
        "Group (global y 20) must draw before A (30)"
    );
}

/// **The Y-sort grouping claim** (pat-s3rrv.22).
///
/// `C` is a child of `Group`, but its global Y (20 − 5 = 15) sits between `B`'s
/// (10) and `Group`'s own (20). Godot flattens every descendant reachable
/// through non-y-sorted children into one list and sorts THAT
/// (`renderer_canvas_cull.cpp:110-181`), so `C` must draw *before* the parent it
/// is nested under.
///
/// ⚠️ This is the assertion that fails on the old behaviour, where
/// `draw_ordered_children` sorted `Sorted`'s direct children only and each
/// child's subtree moved as one indivisible block — putting `C` immediately
/// after `Group` no matter how small its Y. It is deliberately kept separate
/// from the structural invariants above, which hold under both behaviours.
#[test]
fn a_grandchild_interleaves_into_its_ancestors_y_sort_group() {
    let (tree, named) = scene();
    let items = build_runtime_render_list(&tree);
    let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();
    let pos = |name: &str| -> usize {
        let id = named.iter().find(|(_, n)| *n == name).unwrap().0;
        ids.iter()
            .position(|&x| x == id.raw())
            .unwrap_or_else(|| panic!("`{name}` missing from draw list {ids:?}"))
    };

    assert!(
        pos("C") < pos("Group"),
        "C (global y 15) must draw before Group (global y 20) even though Group \
         is its parent: a y-sorted ancestor sorts the flattened subtree, not \
         each child's block. Got C at {} and Group at {}",
        pos("C"),
        pos("Group")
    );
    assert!(
        pos("B") < pos("C"),
        "B (10) still precedes C (15) — the group is sorted by global Y"
    );
}

// ---------------------------------------------------------------------------
// pat-s3rrv.22 — `show_behind_parent`, the other half of the ordering contract
// ---------------------------------------------------------------------------

/// A plain parent (NO y-sort anywhere) with one child flagged
/// `show_behind_parent` and one not.
///
/// ⚠️ Deliberately y-sort-FREE. The remaining known deviation on this bead is
/// that `show_behind_parent` is partitioned over a whole *flattened* group, so a
/// grandchild carrying the flag lands before the y-sorted ancestor instead of
/// before its own parent. A fixture with no y-sort cannot express that case, so
/// this pins only the semantics that are correct today — which is exactly what a
/// safety net for that fix should do.
fn behind_scene() -> (SceneTree, Vec<(NodeId, &'static str)>) {
    let (mut tree, root) = new_tree();
    let parent = color_rect(&mut tree, root, "Parent", 0.0);
    let behind = color_rect(&mut tree, parent, "Behind", 10.0);
    tree.get_node_mut(behind)
        .unwrap()
        .set_property("show_behind_parent", Variant::Bool(true));
    let front = color_rect(&mut tree, parent, "Front", 20.0);

    let named = vec![
        (root, "Root"),
        (parent, "Parent"),
        (behind, "Behind"),
        (front, "Front"),
    ];
    (tree, named)
}

/// 🔑 `show_behind_parent` means what its name says: the child draws BEFORE the
/// parent it is nested under (`canvas_item.cpp`).
///
/// ⚠️ This flag had NO behavioural coverage anywhere in the workspace — the only
/// other mention in a test is a ClassDB property registration, which asserts the
/// property exists rather than that it does anything. The pending fix to the
/// flattened-group case rewrites this partitioning, so pinning the correct half
/// first is what makes that change safe to attempt — the same reason the
/// structural invariants above were landed before the Y-sort fix.
#[test]
fn a_child_with_show_behind_parent_draws_before_its_parent() {
    let (tree, named) = behind_scene();
    let items = build_runtime_render_list(&tree);
    let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();
    let pos = |name: &str| -> usize {
        let id = named.iter().find(|(_, n)| *n == name).unwrap().0;
        ids.iter()
            .position(|&x| x == id.raw())
            .unwrap_or_else(|| panic!("{name} is missing from the draw list"))
    };

    assert!(
        pos("Behind") < pos("Parent"),
        "a child with show_behind_parent must draw BEFORE its parent; got \
         Behind at {} and Parent at {}",
        pos("Behind"),
        pos("Parent"),
    );

    // 🔑 The control. Without this, a build that emitted EVERY child before its
    // parent would satisfy the assertion above and look correct — the flag has
    // to be what moves the node, not the walk order.
    assert!(
        pos("Parent") < pos("Front"),
        "a child WITHOUT the flag must still draw after its parent; got Parent \
         at {} and Front at {}",
        pos("Parent"),
        pos("Front"),
    );
}
