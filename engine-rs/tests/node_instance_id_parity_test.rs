//! pat-4nvk: NodeId / get_instance_id structural parity tests.
//!
//! Verifies that Patina's instance-ID system exhibits the same structural
//! guarantees as Godot's `Object.get_instance_id()`:
//!
//! - IDs are positive non-zero integers (Godot uses unsigned 64-bit)
//! - Every object gets a unique ID at creation
//! - IDs are stable for the lifetime of the object (never change)
//! - IDs survive tree operations (reparenting, reordering)
//! - NodeId ↔ ObjectId round-trips correctly
//! - get_instance_id on GodotObject trait returns the same ID as the base
//!
//! These tests do NOT require exact numeric equality with Godot — only
//! structural shape and invariant parity.

use std::collections::HashSet;

use gdcore::id::ObjectId;
use gdobject::object::{GenericObject, GodotObject, ObjectBase};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;

// ── Godot contract: IDs are positive non-zero ───────────────────────

#[test]
fn object_id_is_positive_nonzero() {
    // Godot: get_instance_id() always returns a positive value > 0.
    for _ in 0..100 {
        let id = ObjectId::next();
        assert!(id.raw() > 0, "ObjectId must be positive, got {}", id.raw());
    }
}

#[test]
fn node_id_is_positive_nonzero() {
    for _ in 0..100 {
        let id = NodeId::next();
        assert!(id.raw() > 0, "NodeId must be positive, got {}", id.raw());
    }
}

#[test]
fn generic_object_instance_id_is_positive() {
    let obj = GenericObject::new("Node");
    assert!(
        obj.get_instance_id().raw() > 0,
        "get_instance_id() must return positive value"
    );
}

// ── Godot contract: every object gets a unique ID ───────────────────

#[test]
fn all_object_ids_unique_in_batch() {
    // Godot: no two live objects share an instance ID.
    let mut seen = HashSet::new();
    for _ in 0..500 {
        let id = ObjectId::next();
        assert!(seen.insert(id.raw()), "Duplicate ObjectId: {}", id.raw());
    }
}

#[test]
fn all_node_ids_unique_in_batch() {
    let mut seen = HashSet::new();
    for _ in 0..500 {
        let id = NodeId::next();
        assert!(seen.insert(id.raw()), "Duplicate NodeId: {}", id.raw());
    }
}

#[test]
fn scene_tree_nodes_all_have_distinct_ids() {
    // Build a tree with many nodes and verify no ID collisions.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ids = HashSet::new();
    ids.insert(root.raw());

    for i in 0..50 {
        let n = Node::new(&format!("N{i}"), "Node2D");
        let nid = tree.add_child(root, n).unwrap();
        assert!(
            ids.insert(nid.raw()),
            "Node N{i} got duplicate ID {}",
            nid.raw()
        );
    }
}

// ── Godot contract: IDs are stable for object lifetime ──────────────

#[test]
fn object_id_stable_across_property_mutations() {
    // Godot: setting properties never changes the instance ID.
    let mut obj = GenericObject::new("Sprite2D");
    let id = obj.get_instance_id();

    obj.set_property("position", gdvariant::Variant::Int(100));
    assert_eq!(obj.get_instance_id(), id, "ID must survive property set");

    obj.set_property("visible", gdvariant::Variant::Bool(false));
    assert_eq!(obj.get_instance_id(), id, "ID must survive second mutation");
}

#[test]
fn node_id_stable_across_tree_add() {
    // Godot: adding a node to the tree does not change its instance ID.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Stable", "Node2D");
    let expected_id = n.id();

    let actual_id = tree.add_child(root, n).unwrap();
    assert_eq!(
        actual_id, expected_id,
        "add_child must preserve the node's pre-existing ID"
    );

    let in_tree = tree.get_node(actual_id).unwrap();
    assert_eq!(
        in_tree.id(),
        expected_id,
        "ID must match after tree insertion"
    );
}

#[test]
fn node_id_stable_across_reparent() {
    // Godot: reparenting preserves instance ID.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node");
    let b_id = tree.add_child(root, b).unwrap();

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(a_id, child).unwrap();
    let child_raw = child_id.raw();

    // Reparent Child from A to B.
    tree.reparent(child_id, b_id).unwrap();

    let node = tree.get_node(child_id).unwrap();
    assert_eq!(
        node.id().raw(),
        child_raw,
        "Reparent must preserve instance ID"
    );
    // Verify it's accessible under the new parent.
    assert!(tree.get_node(child_id).is_some());
}

#[test]
fn node_id_stable_across_rename() {
    // Godot: renaming a node does not change its instance ID.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Before", "Node");
    let nid = tree.add_child(root, n).unwrap();
    let raw = nid.raw();

    tree.get_node_mut(nid).unwrap().set_name("After");

    let node = tree.get_node(nid).unwrap();
    assert_eq!(node.id().raw(), raw, "Rename must preserve instance ID");
    assert_eq!(node.name(), "After");
}

// ── Godot contract: NodeId ↔ ObjectId round-trip ────────────────────

#[test]
fn node_id_object_id_round_trip() {
    // NodeId wraps ObjectId; round-tripping must be lossless.
    let oid = ObjectId::next();
    let nid = NodeId::from_object_id(oid);
    assert_eq!(nid.object_id(), oid);
    assert_eq!(nid.raw(), oid.raw());

    // And a fresh NodeId's object_id().raw() must match raw().
    let nid2 = NodeId::next();
    assert_eq!(nid2.object_id().raw(), nid2.raw());
}

#[test]
fn object_base_id_matches_godot_object_trait() {
    // GenericObject.get_instance_id() must return the same value as base.id().
    let obj = GenericObject::new("Camera2D");
    assert_eq!(
        obj.get_instance_id(),
        obj.base.id(),
        "GodotObject trait must delegate to ObjectBase"
    );
}

#[test]
fn object_base_with_id_preserves_exact_value() {
    // ObjectBase::with_id must store the exact ID provided (deserialization path).
    let id = ObjectId::from_raw(0xDEAD_BEEF);
    let base = ObjectBase::with_id("TestClass", id);
    assert_eq!(base.id().raw(), 0xDEAD_BEEF);
}

// ── Godot contract: monotonically increasing IDs ────────────────────

#[test]
fn object_ids_monotonically_increase() {
    // Godot allocates instance IDs with a monotonic counter.
    // Patina uses AtomicU64 — verify the same invariant.
    let mut prev = ObjectId::next().raw();
    for _ in 0..100 {
        let next = ObjectId::next().raw();
        assert!(
            next > prev,
            "IDs must be monotonically increasing: {} not > {}",
            next,
            prev
        );
        prev = next;
    }
}

// ── Structural shape: IDs fit in u64, Display is numeric ────────────

#[test]
fn instance_id_display_is_numeric_string() {
    // Godot prints instance IDs as plain decimal integers.
    let id = ObjectId::from_raw(12345);
    assert_eq!(format!("{id}"), "12345");

    let nid = NodeId::from_object_id(id);
    assert_eq!(format!("{nid}"), "12345");
}

#[test]
fn instance_id_debug_is_typed_wrapper() {
    // Debug format includes the type name for diagnostics.
    let id = ObjectId::from_raw(42);
    assert_eq!(format!("{id:?}"), "ObjectId(42)");

    let nid = NodeId::from_object_id(id);
    assert_eq!(format!("{nid:?}"), "NodeId(42)");
}

// ── Hierarchical: every node in a loaded scene has a unique ID ──────

#[test]
fn deep_hierarchy_all_ids_unique() {
    // Simulate a realistic hierarchy: root → 10 parents → 5 children each.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut all_ids = HashSet::new();
    all_ids.insert(root.raw());

    for p in 0..10 {
        let parent = Node::new(&format!("P{p}"), "Node");
        let pid = tree.add_child(root, parent).unwrap();
        assert!(all_ids.insert(pid.raw()), "Duplicate parent ID");

        for c in 0..5 {
            let child = Node::new(&format!("P{p}_C{c}"), "Node2D");
            let cid = tree.add_child(pid, child).unwrap();
            assert!(all_ids.insert(cid.raw()), "Duplicate child ID");
        }
    }

    // 1 root + 10 parents + 50 children = 61
    assert_eq!(all_ids.len(), 61);
    assert_eq!(tree.node_count(), 61);
}
