//! pat-9zgy: Verify unique node ID semantics against 4.6.1 oracle expectations.
//!
//! Godot 4.6.1 added "unique node IDs (persistent internal identifiers) added
//! to Node". This test suite validates that Patina's NodeId / ObjectId system
//! satisfies every behavioral contract from the 4.6.1 oracle:
//!
//! - IDs are positive u64 values (never zero)
//! - IDs are globally unique across all nodes in a session
//! - IDs are monotonically increasing (creation order)
//! - IDs are stable for the lifetime of a node
//! - IDs survive reparenting within the scene tree
//! - IDs are accessible via the node's object_id / raw methods
//! - Unique names (%Name) are scoped to the owner, not confused with node IDs
//! - Node removal does not recycle IDs

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// 1. Core ID contract (matches Godot 4.6.1 get_instance_id() semantics)
// ===========================================================================

#[test]
fn node_id_is_positive() {
    let node = Node::new("Test", "Node");
    assert!(
        node.id().raw() > 0,
        "NodeId must be positive (Godot 4.6.1 contract)"
    );
}

#[test]
fn node_id_is_globally_unique() {
    let mut ids = std::collections::HashSet::new();
    for i in 0..1000 {
        let node = Node::new(format!("Node{i}"), "Node");
        assert!(
            ids.insert(node.id().raw()),
            "Duplicate NodeId detected at iteration {i}"
        );
    }
}

#[test]
fn node_id_monotonically_increasing() {
    let a = Node::new("A", "Node");
    let b = Node::new("B", "Node");
    let c = Node::new("C", "Node");
    assert!(a.id().raw() < b.id().raw());
    assert!(b.id().raw() < c.id().raw());
}

#[test]
fn node_id_stable_for_lifetime() {
    let node = Node::new("Stable", "Node");
    let id1 = node.id();
    let id2 = node.id();
    let id3 = node.id();
    assert_eq!(id1, id2);
    assert_eq!(id2, id3);
}

#[test]
fn node_id_raw_roundtrip() {
    let node = Node::new("RT", "Node");
    let raw = node.id().raw();
    let reconstructed = NodeId::from_object_id(gdcore::ObjectId::from_raw(raw));
    assert_eq!(node.id(), reconstructed);
}

// ===========================================================================
// 2. ID persistence across tree operations (4.6.1 persistent identifiers)
// ===========================================================================

#[test]
fn id_survives_add_to_tree() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let child = Node::new("Child", "Node");
    let expected_id = child.id();
    let child_id = tree.add_child(root_id, child).unwrap();

    assert_eq!(
        child_id, expected_id,
        "ID must not change when added to tree"
    );
}

#[test]
fn id_survives_reparenting() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let parent_a = Node::new("ParentA", "Node");
    let parent_b = Node::new("ParentB", "Node");
    let child = Node::new("Child", "Node");
    let child_id = child.id();

    let pa_id = tree.add_child(root_id, parent_a).unwrap();
    let pb_id = tree.add_child(root_id, parent_b).unwrap();
    tree.add_child(pa_id, child).unwrap();

    // Reparent: move child from ParentA to ParentB
    tree.reparent(child_id, pb_id).unwrap();

    // ID must be identical after reparenting
    let node = tree.get_node(child_id).unwrap();
    assert_eq!(node.id(), child_id, "ID must survive reparenting");
}

#[test]
fn id_stable_after_sibling_removal() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let a = Node::new("A", "Node");
    let b = Node::new("B", "Node");
    let b_id = b.id();

    let a_id = tree.add_child(root_id, a).unwrap();
    tree.add_child(root_id, b).unwrap();

    // Remove sibling A
    let _ = tree.remove_node(a_id);

    // B's ID must be unchanged
    let node_b = tree.get_node(b_id).unwrap();
    assert_eq!(
        node_b.id(),
        b_id,
        "Sibling removal must not affect other IDs"
    );
}

// ===========================================================================
// 3. ID uniqueness guarantees (no recycling)
// ===========================================================================

#[test]
fn removed_node_id_is_never_recycled() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    // Create and remove nodes, collecting their IDs
    let mut removed_ids = Vec::new();
    for i in 0..50 {
        let node = Node::new(format!("Temp{i}"), "Node");
        let nid = tree.add_child(root_id, node).unwrap();
        removed_ids.push(nid.raw());
        let _ = tree.remove_node(nid);
    }

    // Create new nodes — none should reuse a removed ID
    for i in 0..50 {
        let node = Node::new(format!("New{i}"), "Node");
        let nid = node.id();
        assert!(
            !removed_ids.contains(&nid.raw()),
            "Node ID {} was recycled from a removed node",
            nid.raw()
        );
    }
}

#[test]
fn concurrent_id_generation_is_unique() {
    // Simulate rapid sequential ID creation (AtomicU64 ensures uniqueness)
    let ids: Vec<u64> = (0..10_000).map(|_| NodeId::next().raw()).collect();
    let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "All 10,000 IDs must be unique");
}

// ===========================================================================
// 4. Unique name (%Name) is orthogonal to node ID
// ===========================================================================

#[test]
fn unique_name_flag_does_not_affect_id() {
    let mut node = Node::new("Player", "Node");
    let id_before = node.id();
    node.set_unique_name(true);
    assert_eq!(
        node.id(),
        id_before,
        "Setting unique_name must not change the node ID"
    );
}

#[test]
fn unique_name_scoped_to_owner_not_global() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    // Create two sub-scenes with their own owners
    let scene_a = Node::new("SceneA", "Node");
    let scene_b = Node::new("SceneB", "Node");
    let sa_id = tree.add_child(root_id, scene_a).unwrap();
    let sb_id = tree.add_child(root_id, scene_b).unwrap();

    // Each scene has a child named "Button" with unique_name
    let mut btn_a = Node::new("Button", "Node");
    btn_a.set_unique_name(true);
    let btn_a_id = tree.add_child(sa_id, btn_a).unwrap();
    // Set owner to SceneA
    tree.get_node_mut(btn_a_id).unwrap().set_owner(Some(sa_id));

    let mut btn_b = Node::new("Button", "Node");
    btn_b.set_unique_name(true);
    let btn_b_id = tree.add_child(sb_id, btn_b).unwrap();
    // Set owner to SceneB
    tree.get_node_mut(btn_b_id).unwrap().set_owner(Some(sb_id));

    // Both have different IDs despite same name + unique flag
    assert_ne!(
        btn_a_id, btn_b_id,
        "Same-named unique nodes in different scopes must have different IDs"
    );

    // Unique name lookup is scoped to owner
    let found_a = tree.get_node_by_unique_name(btn_a_id, "Button");
    let found_b = tree.get_node_by_unique_name(btn_b_id, "Button");
    assert_eq!(found_a, Some(btn_a_id));
    assert_eq!(found_b, Some(btn_b_id));
}

#[test]
fn unique_name_lookup_finds_correct_node() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let mut target = Node::new("HUD", "CanvasLayer");
    target.set_unique_name(true);
    target.set_owner(Some(root_id));
    let target_id = tree.add_child(root_id, target).unwrap();

    let mut decoy = Node::new("HUD_copy", "CanvasLayer");
    decoy.set_owner(Some(root_id));
    tree.add_child(root_id, decoy).unwrap();

    let found = tree.get_node_by_unique_name(root_id, "HUD");
    assert_eq!(found, Some(target_id));
}

#[test]
fn non_unique_name_not_found_by_unique_lookup() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    // Node without unique_name flag
    let normal = Node::new("Player", "Node");
    tree.add_child(root_id, normal).unwrap();

    let found = tree.get_node_by_unique_name(root_id, "Player");
    assert_eq!(
        found, None,
        "Non-unique-named nodes must not be found by unique lookup"
    );
}

// ===========================================================================
// 5. ObjectId ↔ NodeId interop (4.6.1 get_instance_id compatibility)
// ===========================================================================

#[test]
fn node_id_wraps_object_id() {
    let oid = gdcore::ObjectId::next();
    let nid = NodeId::from_object_id(oid);
    assert_eq!(nid.object_id(), oid);
    assert_eq!(nid.raw(), oid.raw());
}

#[test]
fn node_id_display_matches_raw() {
    let node = Node::new("Test", "Node");
    let display = format!("{}", node.id());
    let raw = format!("{}", node.id().raw());
    assert_eq!(display, raw, "Display must show the raw numeric ID");
}

#[test]
fn node_id_debug_format() {
    let nid = NodeId::from_object_id(gdcore::ObjectId::from_raw(42));
    assert_eq!(format!("{nid:?}"), "NodeId(42)");
}

// ===========================================================================
// 6. Multiple nodes in a hierarchy preserve distinct IDs
// ===========================================================================

#[test]
fn deep_hierarchy_all_unique_ids() {
    let mut tree = SceneTree::new();
    let mut parent_id = tree.root_id();
    let mut all_ids = vec![parent_id.raw()];

    // Build a 20-deep chain
    for i in 0..20 {
        let child = Node::new(format!("Level{i}"), "Node");
        let cid = tree.add_child(parent_id, child).unwrap();
        assert!(
            !all_ids.contains(&cid.raw()),
            "Duplicate ID in hierarchy at depth {i}"
        );
        all_ids.push(cid.raw());
        parent_id = cid;
    }
    // +1 for root
    assert_eq!(all_ids.len(), 21);
}

#[test]
fn wide_tree_all_unique_ids() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let mut all_ids = std::collections::HashSet::new();
    all_ids.insert(root_id.raw());

    for i in 0..100 {
        let child = Node::new(format!("Child{i}"), "Node");
        let cid = tree.add_child(root_id, child).unwrap();
        assert!(all_ids.insert(cid.raw()), "Duplicate at child {i}");
    }
    assert_eq!(all_ids.len(), 101);
}
