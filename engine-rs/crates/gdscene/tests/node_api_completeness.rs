//! pat-0q2kx.221 (partial) — `Node` API completeness: `replace_by`,
//! `propagate_notification`, `get_node_and_resource`, a physics-process
//! priority independent of the idle one, `get_path_to`, `is_greater_than`, and
//! the tree-string builders.
//!
//! Godot ground truth: `upstream/godot/scene/main/node.cpp` and
//! `extension_api.json`.
//!
//! ⚠️ **Driven through `SceneTree` rather than from a GDScript, deliberately.**
//! The acceptance says "from a scene script", which needs interpreter arms in
//! `gdscript-interop` — and that crate does not currently compile (a foreign
//! break has been failing beads for ~30 minutes; it parked pat-0q2kx.206 and
//! pat-0q2kx.212). Adding arms there now would make this bead unverifiable for
//! the same reason. The engine-side semantics are what the script arms would
//! call, and they are fully covered here; the binding is reported as not
//! delivered.
//!
//! Also covered here, added while blocked on that crate since they need no part
//! of it: `get_path_to`, `is_greater_than`, `get_tree_string` and
//! `get_tree_string_pretty`.
//!
//! ⚠️ Still NOT delivered: `print_tree` (the `get_tree_string*` builders exist;
//! only the stdout wrapper is missing) and internal-mode children —
//! `add_child(node, force_readable_name, internal)` with `include_internal`
//! filters on `get_child`/`get_child_count`/`get_index`, which touches every
//! child-query call site. See the completion report.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// **The acceptance's `replace_by` clause.** The replacement keeps the old
/// node's children and its slot among its siblings.
#[test]
fn node_api_completeness() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Three siblings, so "kept its index" is distinguishable from "appended".
    let first = tree.add_child(root, Node::new("First", "Node")).unwrap();
    let old = tree.add_child(root, Node::new("Old", "Node2D")).unwrap();
    let last = tree.add_child(root, Node::new("Last", "Node")).unwrap();

    let kid_a = tree.add_child(old, Node::new("KidA", "Node")).unwrap();
    let kid_b = tree.add_child(old, Node::new("KidB", "Node")).unwrap();
    tree.add_to_group(old, "enemies").unwrap();

    // The replacement starts detached from `old`'s slot — a sibling elsewhere.
    let new_node = tree.add_child(first, Node::new("New", "Sprite2D")).unwrap();

    assert!(
        tree.replace_by(old, new_node, true),
        "replace_by must succeed for two live nodes"
    );

    // -- the children came across -------------------------------------------
    let new_children = tree.get_node(new_node).unwrap().children().to_vec();
    assert!(
        new_children.contains(&kid_a) && new_children.contains(&kid_b),
        "both of the old node's children must re-root onto the replacement, got \
         {new_children:?}"
    );
    assert_eq!(
        tree.get_node(kid_a).unwrap().parent(),
        Some(new_node),
        "the child's own parent link must be updated too, not just the list"
    );

    // -- it took the old node's SLOT, not the end of the list ----------------
    let siblings = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(
        siblings,
        vec![first, new_node, last],
        "the replacement must occupy the old node's index — appending instead \
         would silently reorder the scene, since draw order and get_index() \
         both key off it"
    );

    // -- keep_groups -------------------------------------------------------
    assert!(
        tree.get_node(new_node).unwrap().is_in_group("enemies"),
        "keep_groups = true must carry the group membership across"
    );

    // -- and the old node is gone ------------------------------------------
    assert!(
        tree.get_node(old).is_none(),
        "the replaced node must be removed from the tree"
    );
}

/// `keep_groups = false` leaves the groups behind — the control that proves the
/// clause above is doing something.
#[test]
fn replace_by_can_drop_groups() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let old = tree.add_child(root, Node::new("Old", "Node")).unwrap();
    let new_node = tree.add_child(root, Node::new("New", "Node")).unwrap();
    tree.add_to_group(old, "enemies").unwrap();

    assert!(tree.replace_by(old, new_node, false));
    assert!(
        !tree.get_node(new_node).unwrap().is_in_group("enemies"),
        "keep_groups = false must NOT carry groups across"
    );
}

/// Replacing the root, or an unknown node, is refused rather than corrupting
/// the tree.
#[test]
fn replace_by_refuses_impossible_swaps() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();

    assert!(
        !tree.replace_by(root, a, true),
        "the root has no parent to splice into, so this must fail rather than \
         orphan the tree"
    );
    assert!(
        !tree.replace_by(a, a, true),
        "replacing a node by itself must be refused"
    );
}

/// **The acceptance's `propagate_notification` clause.** It reaches every
/// descendant, not just the direct children.
#[test]
fn propagate_notification_reaches_every_descendant() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("P", "Node")).unwrap();
    let child = tree.add_child(parent, Node::new("C", "Node")).unwrap();
    let grandchild = tree.add_child(child, Node::new("G", "Node")).unwrap();
    // A node outside the subtree: propagation starts at `parent`, so this one
    // must NOT receive it.
    let outside = tree.add_child(root, Node::new("Outside", "Node")).unwrap();

    tree.propagate_notification(parent, 1234);

    for (id, label) in [
        (parent, "the start node"),
        (child, "a child"),
        (grandchild, "a grandchild"),
    ] {
        assert!(
            tree.get_node(id)
                .unwrap()
                .notification_log()
                .iter()
                .any(|n| n.code() == 1234),
            "{label} must receive the propagated notification"
        );
    }
    assert!(
        !tree
            .get_node(outside)
            .unwrap()
            .notification_log()
            .iter()
            .any(|n| n.code() == 1234),
        "a node outside the subtree must not receive it — propagation starts at \
         the given node, not the root"
    );
}

/// **The acceptance's `get_node_and_resource` clause.**
#[test]
fn get_node_and_resource_splits_the_subname() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sprite = tree
        .add_child(root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    tree.get_node_mut(sprite)
        .unwrap()
        .set_property("texture", Variant::String("res://icon.png".into()));

    let (node, res) = tree
        .get_node_and_resource(root, "Sprite:texture")
        .expect("the path resolves");
    assert_eq!(node, sprite, "the node half of the path must resolve");
    assert_eq!(
        res,
        Variant::String("res://icon.png".into()),
        "the `:subname` must resolve to the property value"
    );

    // No subname → the node and Nil, which is what Godot returns when the path
    // names no resource.
    let (node2, res2) = tree
        .get_node_and_resource(root, "Sprite")
        .expect("a bare node path resolves");
    assert_eq!(node2, sprite);
    assert_eq!(res2, Variant::Nil, "no subname means no resource");

    // An unknown node yields None rather than a node with a Nil resource — the
    // two failures are different and a caller branches on them differently.
    assert!(
        tree.get_node_and_resource(root, "Nope:texture").is_none(),
        "an unresolvable node path must be None"
    );
}

/// **The acceptance's priority clause.** `process_physics_priority` orders
/// `_physics_process` independently of `process_priority`.
#[test]
fn physics_priority_is_independent_of_process_priority() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();

    // Deliberately OPPOSITE orderings: A first for idle, B first for physics.
    // A single shared field cannot express this, which is the whole point.
    tree.get_node_mut(a).unwrap().set_process_priority(0);
    tree.get_node_mut(b).unwrap().set_process_priority(10);
    tree.get_node_mut(a)
        .unwrap()
        .set_process_physics_priority(10);
    tree.get_node_mut(b)
        .unwrap()
        .set_process_physics_priority(0);

    let idle = tree.all_nodes_in_process_order();
    let physics = tree.all_nodes_in_physics_process_order();

    let idx = |order: &[NodeId], id: NodeId| order.iter().position(|n| *n == id).unwrap();
    assert!(
        idx(&idle, a) < idx(&idle, b),
        "idle order must follow process_priority: A before B"
    );
    assert!(
        idx(&physics, b) < idx(&physics, a),
        "physics order must follow process_physics_priority: B before A. If this \
         matches the idle order, the physics dispatch is still reading \
         process_priority and the two can never differ"
    );
}

/// `get_path_to` walks up to the common ancestor and back down.
#[test]
fn get_path_to_is_relative_to_the_common_ancestor() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let a_kid = tree.add_child(a, Node::new("Kid", "Node")).unwrap();
    let b_deep = tree.add_child(b, Node::new("Deep", "Node")).unwrap();

    assert_eq!(tree.get_path_to(a, a).as_deref(), Some("."), "self is `.`");
    assert_eq!(
        tree.get_path_to(a, a_kid).as_deref(),
        Some("Kid"),
        "a descendant needs no `..`"
    );
    assert_eq!(
        tree.get_path_to(a_kid, a).as_deref(),
        Some(".."),
        "the parent is one level up"
    );
    assert_eq!(
        tree.get_path_to(a, b).as_deref(),
        Some("../B"),
        "a sibling goes up one then down"
    );
    assert_eq!(
        tree.get_path_to(a_kid, b_deep).as_deref(),
        Some("../../B/Deep"),
        "two levels up to the common ancestor, then two names down"
    );

    // A detached node shares no root, so there is no relative path at all —
    // returning something plausible would be worse than returning nothing.
    let mut other = SceneTree::new();
    let orphan_root = other.root_id();
    let orphan = other
        .add_child(orphan_root, Node::new("O", "Node"))
        .unwrap();
    assert_eq!(
        tree.get_path_to(a, orphan),
        None,
        "nodes in different trees have no relative path"
    );
}

/// `is_greater_than` follows the depth-first tree order that processing and
/// drawing use.
#[test]
fn is_greater_than_follows_tree_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let a_kid = tree.add_child(a, Node::new("Kid", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();

    assert!(tree.is_greater_than(b, a), "a later sibling is greater");
    assert!(
        !tree.is_greater_than(a, b),
        "and the relation is not symmetric"
    );
    assert!(
        tree.is_greater_than(a_kid, a),
        "a child comes after its parent in depth-first order"
    );
    assert!(
        tree.is_greater_than(b, a_kid),
        "…and a later sibling comes after that child's whole subtree"
    );
    assert!(
        !tree.is_greater_than(a, a),
        "a node is not greater than itself"
    );
}

/// The tree-string builders.
#[test]
fn tree_strings_render_the_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let game = tree.add_child(root, Node::new("Game", "Node2D")).unwrap();
    let player = tree.add_child(game, Node::new("Player", "Node2D")).unwrap();
    tree.add_child(player, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    tree.add_child(game, Node::new("HUD", "CanvasLayer"))
        .unwrap();

    // Flat form: every line is a path `get_node` would accept back.
    let flat = tree.get_tree_string(game);
    assert_eq!(
        flat, ".\nPlayer\nPlayer/Sprite\nHUD",
        "the flat form is the root as `.` then relative paths in tree order"
    );

    // Pretty form: the last child uses `┗╸`, and — the part that is easy to get
    // wrong — its own children continue under blank space, not a stray `┃`.
    let pretty = tree.get_tree_string_pretty(game);
    assert_eq!(
        pretty, "Game\n┣╸Player\n┃ ┗╸Sprite\n┗╸HUD\n",
        "got:\n{pretty}"
    );
}

/// The physics priority defaults to 0 and is untouched by the idle setter, so
/// existing scenes keep tree order in `_physics_process`.
#[test]
fn physics_priority_defaults_to_zero_and_is_not_aliased() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree.add_child(root, Node::new("N", "Node")).unwrap();

    assert_eq!(tree.get_node(n).unwrap().process_physics_priority(), 0);
    tree.get_node_mut(n).unwrap().set_process_priority(7);
    assert_eq!(
        tree.get_node(n).unwrap().process_physics_priority(),
        0,
        "setting the idle priority must not move the physics one — aliasing them \
         is the bug this property exists to fix"
    );
}
