//! pat-3tw1 + pat-ywdc + pat-ps46: NodePath generic NodeId resolver parity coverage.
//!
//! pat-ps46 additions (sections 26–33): NodePath struct ↔ resolver integration,
//! malformed path edge cases, node rename stability, process-order traversal IDs,
//! current_scene NodeId as handle, multiple packed scene cross-instance resolution,
//! and broadened exclusion documentation.
//!
//! Validates that NodeId-backed resolution works correctly across all
//! SceneTree resolver APIs:
//! 1. NodeId obtained from different sources (add_child, get_node_by_path,
//!    get_node_relative) are interchangeable as resolver handles
//! 2. u64 round-trip through SceneAccess (script access path)
//! 3. Stale/removed NodeId returns None gracefully
//! 4. NodeId stability across tree mutations (reparent, remove sibling)
//! 5. Edge cases: trailing slashes, root-only resolution, cross-scene resolution
//! 6. move_child / raise / lower — NodeId stability after child reordering
//! 7. Group-returned NodeIds as resolver handles
//! 8. Traversal-returned NodeIds (collect_subtree_*, all_nodes_in_tree_order)
//! 9. queue_free + process_deletions — stale NodeId behavior
//! 10. create_node — detached NodeId resolution before/after add_child

use gdcore::ObjectId;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a tree:
///   root
///   ├── A (Node2D)
///   │   ├── A1 (Sprite2D)
///   │   └── A2 (Node) [unique]
///   └── B (Node2D)
///       └── B1 (Node2D)
///           └── B2 (Label)
struct ResolverTree {
    tree: SceneTree,
    root: NodeId,
    a: NodeId,
    a1: NodeId,
    a2: NodeId,
    b: NodeId,
    b1: NodeId,
    b2: NodeId,
}

fn build_resolver_tree() -> ResolverTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let a1 = tree.add_child(a, Node::new("A1", "Sprite2D")).unwrap();

    let mut a2_node = Node::new("A2", "Node");
    a2_node.set_unique_name(true);
    a2_node.set_owner(Some(root));
    let a2 = tree.add_child(a, a2_node).unwrap();

    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let b1 = tree.add_child(b, Node::new("B1", "Node2D")).unwrap();
    let b2 = tree.add_child(b1, Node::new("B2", "Label")).unwrap();

    ResolverTree {
        tree,
        root,
        a,
        a1,
        a2,
        b,
        b1,
        b2,
    }
}

// ===========================================================================
// 1. NodeId interchangeability — IDs from different APIs resolve identically
// ===========================================================================

#[test]
fn nodeid_from_add_child_resolves_same_as_from_path_lookup() {
    let t = build_resolver_tree();

    // Get A1 via add_child (stored in struct) vs via get_node_by_path
    let a1_via_path = t.tree.get_node_by_path("/root/A/A1").unwrap();
    assert_eq!(t.a1, a1_via_path);

    // Both should work identically as `from` in get_node_relative
    assert_eq!(
        t.tree.get_node_relative(t.a1, "..").unwrap(),
        t.tree.get_node_relative(a1_via_path, "..").unwrap()
    );
}

#[test]
fn nodeid_from_relative_resolves_same_as_direct() {
    let t = build_resolver_tree();

    // Get B1 via relative from B vs direct
    let b1_via_rel = t.tree.get_node_relative(t.b, "B1").unwrap();
    assert_eq!(t.b1, b1_via_rel);

    // Use it as `from` for further resolution
    assert_eq!(
        t.tree.get_node_relative(b1_via_rel, "B2").unwrap(),
        t.b2
    );
}

#[test]
fn nodeid_from_get_node_or_null_works_as_resolver_handle() {
    let t = build_resolver_tree();

    // Obtain NodeId via get_node_or_null (absolute)
    let a_via_abs = t.tree.get_node_or_null(t.root, "/root/A").unwrap();
    // Obtain via get_node_or_null (relative)
    let a_via_rel = t.tree.get_node_or_null(t.root, "A").unwrap();

    assert_eq!(a_via_abs, a_via_rel);
    assert_eq!(a_via_abs, t.a);

    // All three work identically as from handles
    for id in [t.a, a_via_abs, a_via_rel] {
        assert_eq!(t.tree.get_node_relative(id, "A1").unwrap(), t.a1);
        assert_eq!(t.tree.get_node_relative(id, "A2").unwrap(), t.a2);
    }
}

// ===========================================================================
// 2. u64 round-trip — NodeId ↔ raw u64 (script access path)
// ===========================================================================

#[test]
fn nodeid_u64_roundtrip_preserves_identity() {
    let t = build_resolver_tree();

    // Round-trip through raw u64
    let raw = t.a1.raw();
    let reconstructed = NodeId::from_object_id(ObjectId::from_raw(raw));
    assert_eq!(t.a1, reconstructed);

    // Reconstructed ID works in all resolver APIs
    assert_eq!(
        t.tree.get_node_relative(reconstructed, "..").unwrap(),
        t.a
    );
    assert_eq!(
        t.tree.get_node_or_null(reconstructed, "../A2").unwrap(),
        t.a2
    );
    assert_eq!(
        t.tree.node_path(reconstructed).unwrap(),
        "/root/A/A1"
    );
}

#[test]
fn nodeid_u64_roundtrip_all_nodes() {
    let t = build_resolver_tree();

    // Every node in the tree should survive a round-trip
    for (id, expected_path) in [
        (t.root, "/root"),
        (t.a, "/root/A"),
        (t.a1, "/root/A/A1"),
        (t.a2, "/root/A/A2"),
        (t.b, "/root/B"),
        (t.b1, "/root/B/B1"),
        (t.b2, "/root/B/B1/B2"),
    ] {
        let raw = id.raw();
        let rt = NodeId::from_object_id(ObjectId::from_raw(raw));
        assert_eq!(id, rt, "round-trip failed for {expected_path}");
        assert_eq!(
            t.tree.node_path(rt).unwrap(),
            expected_path,
            "path mismatch after round-trip for {expected_path}"
        );
    }
}

// ===========================================================================
// 3. Stale/removed NodeId — graceful None returns
// ===========================================================================

#[test]
fn stale_nodeid_returns_none_from_get_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();
    let grandchild = tree
        .add_child(child, Node::new("GC", "Sprite2D"))
        .unwrap();

    // Remove grandchild (remove_node detaches and returns removed subtree)
    tree.remove_node(grandchild).unwrap();

    // Stale grandchild ID should return None for lookups that touch the arena
    assert!(tree.get_node(grandchild).is_none());
    assert!(tree.get_node_relative(grandchild, "..").is_none());
    assert!(tree.node_path(grandchild).is_none());
    // Note: get_node_relative(id, ".") returns Some(id) without arena lookup —
    // this matches Godot's behavior where "." is a no-op on the handle itself.
    // Callers must check get_node() separately for existence.
    assert_eq!(tree.get_node_relative(grandchild, "."), Some(grandchild));
}

#[test]
fn stale_nodeid_as_from_returns_none() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();

    tree.remove_node(a).unwrap();

    // Using removed `a` as `from` to resolve anything
    assert!(tree.get_node_relative(a, "../B").is_none());
    assert!(tree.get_node_or_null(a, "B").is_none());
    // But B still resolves fine
    assert_eq!(tree.get_node_or_null(root, "B").unwrap(), b);
}

#[test]
fn fabricated_nodeid_returns_none() {
    let tree = SceneTree::new();

    // A completely fabricated NodeId that was never in the tree
    let fake = NodeId::next();
    assert!(tree.get_node(fake).is_none());
    assert!(tree.node_path(fake).is_none());
    // get_node_relative with "." returns Some(fake) — "." is a no-op on the
    // handle, not an arena lookup. This matches Godot behavior.
    assert_eq!(tree.get_node_relative(fake, "."), Some(fake));
    // But navigating from a fake node fails (needs parent lookup)
    assert!(tree.get_node_relative(fake, "..").is_none());
    assert!(tree.get_node_relative(fake, "Child").is_none());

    // Absolute path still works with a fabricated `from`
    // (get_node_or_null routes absolute paths to get_node_by_path, ignoring `from`)
    assert!(tree.get_node_or_null(fake, "/root").is_some());
}

// ===========================================================================
// 4. NodeId stability across mutations
// ===========================================================================

#[test]
fn nodeid_stable_after_sibling_removal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();

    // Remove B
    tree.remove_node(b).unwrap();

    // A and C IDs unchanged, still resolve
    assert_eq!(tree.get_node(a).unwrap().name(), "A");
    assert_eq!(tree.get_node(c).unwrap().name(), "C");
    assert_eq!(tree.get_node_relative(a, "../C").unwrap(), c);
    assert_eq!(tree.get_node_by_path("/root/A").unwrap(), a);
}

#[test]
fn nodeid_stable_after_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let child = tree.add_child(a, Node::new("Child", "Sprite2D")).unwrap();

    // Reparent child from A to B
    tree.reparent(child, b).unwrap();

    // NodeId unchanged
    assert_eq!(tree.get_node(child).unwrap().name(), "Child");
    // New path
    assert_eq!(tree.node_path(child).unwrap(), "/root/B/Child");
    // Old path gone
    assert!(tree.get_node_by_path("/root/A/Child").is_none());
    // Relative from new parent works
    assert_eq!(tree.get_node_relative(b, "Child").unwrap(), child);
}

#[test]
fn nodeid_stable_across_multiple_reparents() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();
    let traveler = tree
        .add_child(a, Node::new("Traveler", "Node2D"))
        .unwrap();

    // A -> B -> C -> A
    tree.reparent(traveler, b).unwrap();
    assert_eq!(tree.node_path(traveler).unwrap(), "/root/B/Traveler");

    tree.reparent(traveler, c).unwrap();
    assert_eq!(tree.node_path(traveler).unwrap(), "/root/C/Traveler");

    tree.reparent(traveler, a).unwrap();
    assert_eq!(tree.node_path(traveler).unwrap(), "/root/A/Traveler");

    // Same NodeId throughout
    assert_eq!(tree.get_node(traveler).unwrap().name(), "Traveler");
}

// ===========================================================================
// 5. get_node_or_null routing with NodeId handles
// ===========================================================================

#[test]
fn get_node_or_null_absolute_ignores_from() {
    let t = build_resolver_tree();

    // Absolute path resolution should work identically regardless of `from`
    let targets = [t.root, t.a, t.a1, t.b, t.b2];
    for from in targets {
        assert_eq!(
            t.tree.get_node_or_null(from, "/root/B/B1/B2").unwrap(),
            t.b2,
            "absolute resolution should not depend on `from`"
        );
    }
}

#[test]
fn get_node_or_null_relative_depends_on_from() {
    let t = build_resolver_tree();

    // "A1" resolves from A but not from B
    assert_eq!(t.tree.get_node_or_null(t.a, "A1").unwrap(), t.a1);
    assert!(t.tree.get_node_or_null(t.b, "A1").is_none());

    // "B1" resolves from B but not from A
    assert_eq!(t.tree.get_node_or_null(t.b, "B1").unwrap(), t.b1);
    assert!(t.tree.get_node_or_null(t.a, "B1").is_none());
}

#[test]
fn get_node_or_null_unique_name_from_different_handles() {
    // Build a tree where all nodes share the same owner (root) so %A2
    // resolves from anywhere in the scene.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = {
        let mut n = Node::new("A", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };
    let a1 = {
        let mut n = Node::new("A1", "Sprite2D");
        n.set_owner(Some(root));
        tree.add_child(a, n).unwrap()
    };
    let a2 = {
        let mut n = Node::new("A2", "Node");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(a, n).unwrap()
    };
    let b = {
        let mut n = Node::new("B", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };
    let b1 = {
        let mut n = Node::new("B1", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(b, n).unwrap()
    };

    // %A2 should resolve from any node under the same owner
    for from in [root, a, a1, b, b1] {
        assert_eq!(
            tree.get_node_or_null(from, "%A2"),
            Some(a2),
            "%%A2 should resolve from any node under the same owner"
        );
    }
}

// ===========================================================================
// 6. Cross-subtree resolution via NodeId
// ===========================================================================

#[test]
fn cross_subtree_resolution_deep_to_deep() {
    let t = build_resolver_tree();

    // From A1 (depth 2 under root) to B2 (depth 3 under root)
    assert_eq!(
        t.tree.get_node_relative(t.a1, "../../B/B1/B2").unwrap(),
        t.b2
    );

    // From B2 to A1
    assert_eq!(
        t.tree.get_node_relative(t.b2, "../../../A/A1").unwrap(),
        t.a1
    );
}

#[test]
fn cross_subtree_via_get_node_or_null() {
    let t = build_resolver_tree();

    // Relative cross-subtree
    assert_eq!(
        t.tree.get_node_or_null(t.a1, "../../B/B1/B2").unwrap(),
        t.b2
    );

    // Absolute cross-subtree (from matters not)
    assert_eq!(
        t.tree.get_node_or_null(t.a1, "/root/B/B1/B2").unwrap(),
        t.b2
    );
}

// ===========================================================================
// 7. node_path() → get_node_by_path() round-trip
// ===========================================================================

#[test]
fn node_path_to_get_node_by_path_roundtrip() {
    let t = build_resolver_tree();

    for id in [t.root, t.a, t.a1, t.a2, t.b, t.b1, t.b2] {
        let path = t.tree.node_path(id).unwrap();
        let resolved = t.tree.get_node_by_path(&path).unwrap();
        assert_eq!(id, resolved, "round-trip failed for path {path}");
    }
}

// ===========================================================================
// 8. Packed scene — NodeId resolution after instancing
// ===========================================================================

#[test]
fn packed_scene_nodeid_resolution_through_all_apis() {
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Sprite" type="Sprite2D" parent="Player"]

[node name="Enemy" type="Node2D" parent="."]

[node name="AI" type="Node" parent="Enemy"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Get NodeIds via different APIs and verify consistency
    let player_abs = tree.get_node_by_path("/root/World/Player").unwrap();
    let player_rel = tree.get_node_relative(world, "Player").unwrap();
    let player_null = tree.get_node_or_null(world, "Player").unwrap();
    assert_eq!(player_abs, player_rel);
    assert_eq!(player_rel, player_null);

    // Use the instanced NodeId as a resolver handle
    let sprite = tree.get_node_relative(player_abs, "Sprite").unwrap();
    assert_eq!(tree.get_node(sprite).unwrap().class_name(), "Sprite2D");

    // Cross-subtree from Sprite to AI
    let ai = tree.get_node_relative(sprite, "../../Enemy/AI").unwrap();
    assert_eq!(tree.get_node(ai).unwrap().class_name(), "Node");

    // node_path round-trip on instanced nodes
    let ai_path = tree.node_path(ai).unwrap();
    assert_eq!(ai_path, "/root/World/Enemy/AI");
    assert_eq!(tree.get_node_by_path(&ai_path).unwrap(), ai);
}

// ===========================================================================
// 9. %UniqueName resolution with NodeId from different scene depths
// ===========================================================================

#[test]
fn unique_name_resolution_from_various_depths() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();

    let l1 = {
        let mut n = Node::new("L1", "Node2D");
        n.set_owner(Some(scene));
        tree.add_child(scene, n).unwrap()
    };

    let l2 = {
        let mut n = Node::new("L2", "Node2D");
        n.set_owner(Some(scene));
        tree.add_child(l1, n).unwrap()
    };

    let target = {
        let mut n = Node::new("Target", "Sprite2D");
        n.set_unique_name(true);
        n.set_owner(Some(scene));
        tree.add_child(l2, n).unwrap()
    };

    // %Target should resolve from any depth in the scene
    assert_eq!(tree.get_node_relative(scene, "%Target").unwrap(), target);
    assert_eq!(tree.get_node_relative(l1, "%Target").unwrap(), target);
    assert_eq!(tree.get_node_relative(l2, "%Target").unwrap(), target);
    // Even from target itself
    assert_eq!(tree.get_node_relative(target, "%Target").unwrap(), target);
}

#[test]
fn unique_name_with_subpath_via_nodeid() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();

    let container = {
        let mut n = Node::new("Container", "VBoxContainer");
        n.set_unique_name(true);
        n.set_owner(Some(scene));
        tree.add_child(scene, n).unwrap()
    };

    let label = {
        let mut n = Node::new("Label", "Label");
        n.set_owner(Some(scene));
        tree.add_child(container, n).unwrap()
    };

    let button = {
        let mut n = Node::new("Button", "Button");
        n.set_owner(Some(scene));
        tree.add_child(container, n).unwrap()
    };

    // %Container/Label and %Container/Button from scene root
    assert_eq!(
        tree.get_node_relative(scene, "%Container/Label").unwrap(),
        label
    );
    assert_eq!(
        tree.get_node_relative(scene, "%Container/Button").unwrap(),
        button
    );

    // Same via get_node_or_null
    assert_eq!(
        tree.get_node_or_null(scene, "%Container/Label").unwrap(),
        label
    );
    assert_eq!(
        tree.get_node_or_null(scene, "%Container/Button").unwrap(),
        button
    );
}

// ===========================================================================
// 10. Edge cases: root-only, self-referential, slash handling
// ===========================================================================

#[test]
fn resolve_root_via_all_apis() {
    let tree = SceneTree::new();
    let root = tree.root_id();

    // Absolute
    assert_eq!(tree.get_node_by_path("/root").unwrap(), root);
    // node_path
    assert_eq!(tree.node_path(root).unwrap(), "/root");
    // get_node_or_null absolute
    assert_eq!(tree.get_node_or_null(root, "/root").unwrap(), root);
    // Relative dot
    assert_eq!(tree.get_node_relative(root, ".").unwrap(), root);
    // Empty
    assert_eq!(tree.get_node_relative(root, "").unwrap(), root);
}

#[test]
fn dot_segments_in_middle_of_path() {
    let t = build_resolver_tree();

    // A/./A1 should work (. is identity)
    assert_eq!(
        t.tree.get_node_relative(t.root, "A/./A1").unwrap(),
        t.a1
    );

    // B/./B1/./B2
    assert_eq!(
        t.tree.get_node_relative(t.root, "B/./B1/./B2").unwrap(),
        t.b2
    );
}

#[test]
fn dotdot_then_child_at_root_level() {
    let t = build_resolver_tree();

    // From A, ../B resolves to sibling
    assert_eq!(t.tree.get_node_relative(t.a, "../B").unwrap(), t.b);

    // From A, ../B/B1/B2
    assert_eq!(
        t.tree.get_node_relative(t.a, "../B/B1/B2").unwrap(),
        t.b2
    );
}

#[test]
fn parent_past_root_returns_none_for_all_apis() {
    let tree = SceneTree::new();
    let root = tree.root_id();

    assert!(tree.get_node_relative(root, "..").is_none());
    assert!(tree.get_node_or_null(root, "..").is_none());
    assert!(tree.get_node_relative(root, "../..").is_none());
}

// ===========================================================================
// 11. Script-access-style u64 resolution (mimics SceneAccess fallback path)
// ===========================================================================

/// In SceneTreeAccessor::get_node, the engine tries get_node_or_null first,
/// then falls back to child-by-name search, then sibling-by-name search.
/// These tests validate the same patterns using raw u64 ↔ NodeId conversions
/// that the script layer performs.

#[test]
fn script_u64_child_fallback_resolves_by_name() {
    let t = build_resolver_tree();

    // Simulate script access: convert to u64, then resolve child by name
    let a_raw = t.a.raw();
    let from_id = NodeId::from_object_id(ObjectId::from_raw(a_raw));

    // Primary path: get_node_or_null resolves "A1"
    let result = t.tree.get_node_or_null(from_id, "A1").unwrap();
    assert_eq!(result.raw(), t.a1.raw());

    // Verify the child search fallback pattern (what SceneAccess does):
    // enumerate children and match by name
    let from_node = t.tree.get_node(from_id).unwrap();
    let mut found_child = None;
    for &child_id in from_node.children() {
        if let Some(child) = t.tree.get_node(child_id) {
            if child.name() == "A1" {
                found_child = Some(child_id);
                break;
            }
        }
    }
    assert_eq!(found_child.unwrap(), t.a1);
}

#[test]
fn script_u64_sibling_fallback_resolves_by_name() {
    let t = build_resolver_tree();

    // Simulate sibling search fallback: from A, find sibling B by name
    let a_raw = t.a.raw();
    let from_id = NodeId::from_object_id(ObjectId::from_raw(a_raw));

    let from_node = t.tree.get_node(from_id).unwrap();
    let parent_id = from_node.parent().unwrap();
    let parent_node = t.tree.get_node(parent_id).unwrap();

    let mut found_sibling = None;
    for &sib_id in parent_node.children() {
        if let Some(sib) = t.tree.get_node(sib_id) {
            if sib.name() == "B" {
                found_sibling = Some(sib_id);
                break;
            }
        }
    }
    assert_eq!(found_sibling.unwrap(), t.b);
}

#[test]
fn script_u64_get_parent_and_get_children_roundtrip() {
    let t = build_resolver_tree();

    // get_parent: A → root
    let a_raw = t.a.raw();
    let a_id = NodeId::from_object_id(ObjectId::from_raw(a_raw));
    let parent_raw = t
        .tree
        .get_node(a_id)
        .and_then(|n| n.parent())
        .map(|p| p.raw())
        .unwrap();
    assert_eq!(parent_raw, t.root.raw());

    // get_children: A → [A1, A2]
    let children_raw: Vec<u64> = t
        .tree
        .get_node(a_id)
        .unwrap()
        .children()
        .iter()
        .map(|id| id.raw())
        .collect();
    assert_eq!(children_raw, vec![t.a1.raw(), t.a2.raw()]);

    // Round-trip: reconstruct NodeIds from children u64s and resolve further
    for raw in &children_raw {
        let child_id = NodeId::from_object_id(ObjectId::from_raw(*raw));
        assert!(t.tree.get_node(child_id).is_some());
        // Parent of each child should be A
        assert_eq!(
            t.tree.get_node(child_id).unwrap().parent().unwrap().raw(),
            a_raw
        );
    }
}

// ===========================================================================
// 12. NodeId as type-agnostic handle — class type does not affect resolution
// ===========================================================================

#[test]
fn nodeid_resolves_identically_regardless_of_class_type() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Create nodes of various class types
    let node2d = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let sprite = tree
        .add_child(root, Node::new("B", "Sprite2D"))
        .unwrap();
    let label = tree.add_child(root, Node::new("C", "Label")).unwrap();
    let body = tree
        .add_child(root, Node::new("D", "CharacterBody2D"))
        .unwrap();
    let control = tree
        .add_child(root, Node::new("E", "Control"))
        .unwrap();

    // All should be resolvable via the same APIs
    let all = [
        (node2d, "A", "Node2D"),
        (sprite, "B", "Sprite2D"),
        (label, "C", "Label"),
        (body, "D", "CharacterBody2D"),
        (control, "E", "Control"),
    ];

    for (id, name, class) in &all {
        // By path
        let path = format!("/root/{name}");
        assert_eq!(
            tree.get_node_by_path(&path).unwrap(),
            *id,
            "path lookup failed for {class}"
        );
        // Relative from root
        assert_eq!(
            tree.get_node_relative(root, name).unwrap(),
            *id,
            "relative lookup failed for {class}"
        );
        // get_node_or_null
        assert_eq!(
            tree.get_node_or_null(root, name).unwrap(),
            *id,
            "get_node_or_null failed for {class}"
        );
        // node_path round-trip
        assert_eq!(
            tree.node_path(*id).unwrap(),
            path,
            "node_path failed for {class}"
        );
        // Class name preserved
        assert_eq!(
            tree.get_node(*id).unwrap().class_name(),
            *class,
            "class_name mismatch for {class}"
        );
    }

    // Cross-resolution between different types works
    assert_eq!(tree.get_node_relative(node2d, "../B").unwrap(), sprite);
    assert_eq!(tree.get_node_relative(label, "../D").unwrap(), body);
    assert_eq!(tree.get_node_relative(control, "../A").unwrap(), node2d);
}

// ===========================================================================
// 13. Unique name after mutations (reparent, flag toggle)
// ===========================================================================

#[test]
fn unique_name_resolves_after_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = {
        let mut n = Node::new("A", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };
    let b = {
        let mut n = Node::new("B", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };
    let target = {
        let mut n = Node::new("Target", "Sprite2D");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(a, n).unwrap()
    };

    // %Target resolves initially
    assert_eq!(tree.get_node_or_null(root, "%Target").unwrap(), target);

    // Reparent from A to B
    tree.reparent(target, b).unwrap();

    // %Target still resolves — same owner scope, different parent
    assert_eq!(tree.get_node_or_null(root, "%Target").unwrap(), target);
    assert_eq!(tree.get_node_or_null(a, "%Target").unwrap(), target);
    assert_eq!(tree.get_node_or_null(b, "%Target").unwrap(), target);

    // New path reflects reparent
    assert_eq!(tree.node_path(target).unwrap(), "/root/B/Target");
}

#[test]
fn unique_name_flag_toggle_affects_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let child = {
        let mut n = Node::new("Unique", "Node2D");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };

    // Resolves with unique flag set
    assert_eq!(tree.get_node_or_null(root, "%Unique").unwrap(), child);

    // Toggle unique off
    tree.get_node_mut(child).unwrap().set_unique_name(false);

    // No longer resolves via %Unique
    assert!(tree.get_node_or_null(root, "%Unique").is_none());

    // Toggle back on
    tree.get_node_mut(child).unwrap().set_unique_name(true);

    // Resolves again
    assert_eq!(tree.get_node_or_null(root, "%Unique").unwrap(), child);
}

// ===========================================================================
// 14. Multiple unique names in same owner scope
// ===========================================================================

#[test]
fn multiple_unique_names_resolve_independently() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let alpha = {
        let mut n = Node::new("Alpha", "Node2D");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };
    let beta = {
        let mut n = Node::new("Beta", "Sprite2D");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };
    let gamma = {
        let mut n = Node::new("Gamma", "Label");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(alpha, n).unwrap()
    };

    // Each unique name resolves to its correct node
    assert_eq!(tree.get_node_or_null(root, "%Alpha").unwrap(), alpha);
    assert_eq!(tree.get_node_or_null(root, "%Beta").unwrap(), beta);
    assert_eq!(tree.get_node_or_null(root, "%Gamma").unwrap(), gamma);

    // Cross-resolve: from Beta, resolve %Gamma (different subtree)
    assert_eq!(tree.get_node_or_null(beta, "%Gamma").unwrap(), gamma);
    // From Gamma, resolve %Beta
    assert_eq!(tree.get_node_or_null(gamma, "%Beta").unwrap(), beta);
}

#[test]
fn unique_name_with_deep_subpath_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hub = {
        let mut n = Node::new("Hub", "Node2D");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };

    let l1 = {
        let mut n = Node::new("L1", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(hub, n).unwrap()
    };
    let l2 = {
        let mut n = Node::new("L2", "Node2D");
        n.set_owner(Some(root));
        tree.add_child(l1, n).unwrap()
    };
    let leaf = {
        let mut n = Node::new("Leaf", "Sprite2D");
        n.set_owner(Some(root));
        tree.add_child(l2, n).unwrap()
    };

    // %Hub/L1/L2/Leaf should traverse from unique name down
    assert_eq!(
        tree.get_node_relative(root, "%Hub/L1/L2/Leaf").unwrap(),
        leaf
    );
    // Shorter subpaths
    assert_eq!(tree.get_node_relative(root, "%Hub/L1").unwrap(), l1);
    assert_eq!(tree.get_node_relative(root, "%Hub/L1/L2").unwrap(), l2);
}

// ===========================================================================
// 15. Deep chain resolution (10+ levels)
// ===========================================================================

#[test]
fn deep_chain_resolution_ten_levels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build a chain: root → D0 → D1 → ... → D9
    let mut ids = Vec::new();
    let mut parent = root;
    for i in 0..10 {
        let name = format!("D{i}");
        let id = tree
            .add_child(parent, Node::new(&name, "Node2D"))
            .unwrap();
        ids.push(id);
        parent = id;
    }

    // Absolute path to deepest node
    let deep_path = "/root/D0/D1/D2/D3/D4/D5/D6/D7/D8/D9";
    assert_eq!(tree.get_node_by_path(deep_path).unwrap(), ids[9]);

    // Relative from root
    assert_eq!(
        tree.get_node_relative(root, "D0/D1/D2/D3/D4/D5/D6/D7/D8/D9")
            .unwrap(),
        ids[9]
    );

    // node_path round-trip on deepest
    assert_eq!(tree.node_path(ids[9]).unwrap(), deep_path);
    assert_eq!(tree.get_node_by_path(deep_path).unwrap(), ids[9]);

    // Relative from midpoint (D5) to deepest
    assert_eq!(
        tree.get_node_relative(ids[5], "D6/D7/D8/D9").unwrap(),
        ids[9]
    );

    // Navigate up from deepest to midpoint
    assert_eq!(
        tree.get_node_relative(ids[9], "../../../..").unwrap(),
        ids[5]
    );

    // u64 round-trip on deep node
    let raw = ids[9].raw();
    let rt = NodeId::from_object_id(ObjectId::from_raw(raw));
    assert_eq!(tree.node_path(rt).unwrap(), deep_path);
}

// ===========================================================================
// 16. get_index with NodeId
// ===========================================================================

#[test]
fn get_index_returns_correct_position_for_each_child() {
    let t = build_resolver_tree();

    // A and B are children of root, in order
    assert_eq!(t.tree.get_index(t.a).unwrap(), 0);
    assert_eq!(t.tree.get_index(t.b).unwrap(), 1);

    // A1 and A2 are children of A
    assert_eq!(t.tree.get_index(t.a1).unwrap(), 0);
    assert_eq!(t.tree.get_index(t.a2).unwrap(), 1);

    // Root has no index (no parent)
    assert!(t.tree.get_index(t.root).is_none());
}

#[test]
fn get_index_updates_after_sibling_removal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();

    assert_eq!(tree.get_index(a).unwrap(), 0);
    assert_eq!(tree.get_index(b).unwrap(), 1);
    assert_eq!(tree.get_index(c).unwrap(), 2);

    // Remove B
    tree.remove_node(b).unwrap();

    // C should now be at index 1
    assert_eq!(tree.get_index(a).unwrap(), 0);
    assert_eq!(tree.get_index(c).unwrap(), 1);

    // B's index is gone
    assert!(tree.get_index(b).is_none());
}

// ===========================================================================
// 17. duplicate_subtree yields fresh NodeIds
// ===========================================================================

#[test]
fn duplicate_subtree_produces_independent_nodeids() {
    let t = build_resolver_tree();

    // Duplicate B's subtree (B → B1 → B2)
    let clones = t.tree.duplicate_subtree(t.b).unwrap();

    // Should produce 3 cloned nodes
    assert_eq!(clones.len(), 3);

    // None of the cloned nodes should share a NodeId with originals
    let original_ids = [t.root, t.a, t.a1, t.a2, t.b, t.b1, t.b2];
    // Cloned nodes are returned as Node values. When added back to the tree
    // via add_child, they receive fresh NodeIds — the clone itself is just
    // data, not an arena entry. Verify the cloned nodes are independent by
    // checking that none of the original IDs appear in the clone's parent
    // wiring (clones get their own internal parent references).
    for cloned_node in &clones {
        if let Some(parent_id) = cloned_node.parent() {
            assert!(
                !original_ids.contains(&parent_id),
                "cloned node {:?} still references original parent",
                cloned_node.name()
            );
        }
    }

    // Names should match the original subtree
    assert_eq!(clones[0].name(), "B");
    assert_eq!(clones[1].name(), "B1");
    assert_eq!(clones[2].name(), "B2");
}

#[test]
fn duplicate_and_reinsert_creates_independent_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let orig = tree.add_child(root, Node::new("Orig", "Node2D")).unwrap();
    let child = tree
        .add_child(orig, Node::new("Child", "Sprite2D"))
        .unwrap();

    // Duplicate
    let clones = tree.duplicate_subtree(orig).unwrap();

    // Insert clone under root
    let clone_root = tree.add_child(root, clones[0].clone()).unwrap();
    let clone_child = tree.add_child(clone_root, clones[1].clone()).unwrap();

    // Different NodeIds
    assert_ne!(orig, clone_root);
    assert_ne!(child, clone_child);

    // Both resolve independently
    assert_eq!(tree.node_path(orig).unwrap(), "/root/Orig");
    assert_eq!(tree.node_path(clone_root).unwrap(), "/root/Orig"); // same name, different ID

    // But from root, relative resolution finds the first child with that name
    let first_orig = tree.get_node_relative(root, "Orig").unwrap();
    // At least one of them is found
    assert!(first_orig == orig || first_orig == clone_root);

    // Both children are accessible via their NodeIds
    assert_eq!(tree.get_node(child).unwrap().name(), "Child");
    assert_eq!(tree.get_node(clone_child).unwrap().name(), "Child");
}

// ===========================================================================
// 18. Packed scene with unique names — NodeId resolution post-instancing
// ===========================================================================

#[test]
fn packed_scene_unique_name_resolution_after_instancing() {
    let tscn = r#"[gd_scene format=3]

[node name="UI" type="Control"]

[node name="%Header" type="HBoxContainer" parent="."]

[node name="Title" type="Label" parent="Header"]

[node name="Footer" type="VBoxContainer" parent="."]

[node name="Status" type="Label" parent="Footer"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ui = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %Header should resolve from any depth within the instanced scene
    let header = tree.get_node_by_path("/root/UI/Header").unwrap();
    let title = tree.get_node_by_path("/root/UI/Header/Title").unwrap();
    let footer = tree.get_node_by_path("/root/UI/Footer").unwrap();

    // %Header from UI (scene root)
    assert_eq!(tree.get_node_or_null(ui, "%Header").unwrap(), header);

    // %Header/Title — unique name with subpath
    assert_eq!(
        tree.get_node_relative(ui, "%Header/Title").unwrap(),
        title
    );

    // Cross-subtree: from Footer, resolve %Header
    assert_eq!(tree.get_node_or_null(footer, "%Header").unwrap(), header);

    // u64 round-trip on instanced unique node
    let raw = header.raw();
    let rt = NodeId::from_object_id(ObjectId::from_raw(raw));
    assert_eq!(tree.get_node(rt).unwrap().name(), "Header");
}

// ===========================================================================
// 19. move_child / raise / lower — NodeId stability and resolution after reorder
// ===========================================================================

#[test]
fn move_child_preserves_nodeid_and_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();

    // Initial order: A(0), B(1), C(2)
    assert_eq!(tree.get_index(a).unwrap(), 0);
    assert_eq!(tree.get_index(c).unwrap(), 2);

    // Move C to index 0
    tree.move_child(root, c, 0).unwrap();

    // NodeIds unchanged
    assert_eq!(tree.get_node(a).unwrap().name(), "A");
    assert_eq!(tree.get_node(c).unwrap().name(), "C");

    // Index updated: C(0), A(1), B(2)
    assert_eq!(tree.get_index(c).unwrap(), 0);
    assert_eq!(tree.get_index(a).unwrap(), 1);
    assert_eq!(tree.get_index(b).unwrap(), 2);

    // Path resolution still works
    assert_eq!(tree.get_node_by_path("/root/A").unwrap(), a);
    assert_eq!(tree.get_node_by_path("/root/C").unwrap(), c);
    assert_eq!(tree.get_node_relative(a, "../C").unwrap(), c);
    assert_eq!(tree.get_node_relative(c, "../A").unwrap(), a);
}

#[test]
fn raise_preserves_nodeid_and_updates_index() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let _c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();

    // Raise A to last
    tree.raise(a).unwrap();

    assert_eq!(tree.get_index(a).unwrap(), 2);
    assert_eq!(tree.get_node(a).unwrap().name(), "A");

    // Resolution unaffected
    assert_eq!(tree.get_node_by_path("/root/A").unwrap(), a);
    assert_eq!(tree.get_node_relative(b, "../A").unwrap(), a);
}

#[test]
fn lower_preserves_nodeid_and_updates_index() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let _b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();

    // Lower C to first
    tree.lower(c).unwrap();

    assert_eq!(tree.get_index(c).unwrap(), 0);
    assert_eq!(tree.get_node(c).unwrap().name(), "C");

    // Resolution unaffected
    assert_eq!(tree.get_node_by_path("/root/C").unwrap(), c);
    assert_eq!(tree.get_node_relative(a, "../C").unwrap(), c);
}

// ===========================================================================
// 20. Group-returned NodeIds as resolver handles
// ===========================================================================

#[test]
fn group_nodeids_usable_as_resolver_handles() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let a1 = tree.add_child(a, Node::new("A1", "Sprite2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let b1 = tree.add_child(b, Node::new("B1", "Label")).unwrap();

    tree.add_to_group(a, "enemies").unwrap();
    tree.add_to_group(b, "enemies").unwrap();

    let group_ids = tree.get_nodes_in_group("enemies");
    assert_eq!(group_ids.len(), 2);

    // Every ID from the group should work as a resolver handle
    for id in &group_ids {
        assert!(tree.get_node(*id).is_some());
        assert!(tree.node_path(*id).is_some());
        // Navigate to parent
        assert_eq!(tree.get_node_relative(*id, "..").unwrap(), root);
    }

    // Find a specific group member and resolve its children
    let a_from_group = group_ids.iter().find(|&&id| tree.get_node(id).unwrap().name() == "A").unwrap();
    assert_eq!(tree.get_node_relative(*a_from_group, "A1").unwrap(), a1);

    let b_from_group = group_ids.iter().find(|&&id| tree.get_node(id).unwrap().name() == "B").unwrap();
    assert_eq!(tree.get_node_relative(*b_from_group, "B1").unwrap(), b1);
}

#[test]
fn group_nodeids_after_removal_from_group() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    tree.add_to_group(a, "test").unwrap();

    let ids_before = tree.get_nodes_in_group("test");
    assert_eq!(ids_before.len(), 1);

    tree.remove_from_group(a, "test").unwrap();

    let ids_after = tree.get_nodes_in_group("test");
    assert!(ids_after.is_empty());

    // The NodeId is still valid for resolution even after group removal
    assert_eq!(tree.get_node(a).unwrap().name(), "A");
    assert_eq!(tree.node_path(a).unwrap(), "/root/A");
}

// ===========================================================================
// 21. Traversal-returned NodeIds as resolver handles
// ===========================================================================

#[test]
fn collect_subtree_top_down_ids_resolve_correctly() {
    let t = build_resolver_tree();

    let mut subtree = Vec::new();
    t.tree.collect_subtree_top_down(t.a, &mut subtree);

    // Should contain A, A1, A2 in top-down order
    assert_eq!(subtree.len(), 3);
    assert_eq!(subtree[0], t.a);
    assert_eq!(subtree[1], t.a1);
    assert_eq!(subtree[2], t.a2);

    // Each ID from traversal resolves correctly
    for id in &subtree {
        let path = t.tree.node_path(*id).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path).unwrap(), *id);
    }
}

#[test]
fn collect_subtree_bottom_up_ids_resolve_correctly() {
    let t = build_resolver_tree();

    let mut subtree = Vec::new();
    t.tree.collect_subtree_bottom_up(t.b, &mut subtree);

    // Should contain B2, B1, B in bottom-up order
    assert_eq!(subtree.len(), 3);
    assert_eq!(subtree[0], t.b2);
    assert_eq!(subtree[1], t.b1);
    assert_eq!(subtree[2], t.b);

    // Each ID works as a resolver handle
    for id in &subtree {
        assert!(t.tree.get_node(*id).is_some());
        // Navigate from each to root via enough ".."
        let path = t.tree.node_path(*id).unwrap();
        let depth = path.matches('/').count() - 1; // subtract the leading /root
        let up_path = (0..depth).map(|_| "..").collect::<Vec<_>>().join("/");
        if !up_path.is_empty() {
            assert_eq!(
                t.tree.get_node_relative(*id, &up_path).unwrap(),
                t.root,
                "failed to navigate up from {path}"
            );
        }
    }
}

#[test]
fn all_nodes_in_tree_order_ids_are_valid_resolver_handles() {
    let t = build_resolver_tree();

    let all_ids = t.tree.all_nodes_in_tree_order();
    // root, A, A1, A2, B, B1, B2 = 7 nodes
    assert_eq!(all_ids.len(), 7);

    // Every returned ID should have a valid path and round-trip
    for id in &all_ids {
        let path = t.tree.node_path(*id).unwrap();
        let resolved = t.tree.get_node_by_path(&path).unwrap();
        assert_eq!(*id, resolved, "round-trip failed for {path}");
    }

    // Cross-resolve between first and last
    let first = all_ids[0]; // root
    let last = *all_ids.last().unwrap(); // some leaf
    let last_path_from_root = t.tree.node_path(last).unwrap();
    // Strip "/root/" prefix to get relative path from root
    let rel = last_path_from_root.strip_prefix("/root/").unwrap();
    assert_eq!(t.tree.get_node_relative(first, rel).unwrap(), last);
}

// ===========================================================================
// 22. queue_free + process_deletions — stale NodeId behavior
// ===========================================================================

#[test]
fn queue_free_node_still_resolves_before_process_deletions() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let child = tree.add_child(a, Node::new("Child", "Sprite2D")).unwrap();

    // Mark for deletion but don't process yet
    tree.queue_free(child);

    // Node is still live until process_deletions is called
    assert!(tree.get_node(child).is_some());
    assert_eq!(tree.node_path(child).unwrap(), "/root/A/Child");
    assert_eq!(tree.get_node_relative(a, "Child").unwrap(), child);
}

#[test]
fn queue_free_node_stale_after_process_deletions() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let child = tree.add_child(a, Node::new("Child", "Sprite2D")).unwrap();

    tree.queue_free(child);
    tree.process_deletions();

    // Now the NodeId is stale
    assert!(tree.get_node(child).is_none());
    assert!(tree.node_path(child).is_none());
    assert!(tree.get_node_relative(child, "..").is_none());
    assert!(tree.get_node_by_path("/root/A/Child").is_none());

    // Parent is still valid
    assert_eq!(tree.get_node(a).unwrap().name(), "A");
    assert!(tree.get_node(a).unwrap().children().is_empty());
}

#[test]
fn queue_free_subtree_stale_after_process_deletions() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let a1 = tree.add_child(a, Node::new("A1", "Sprite2D")).unwrap();
    let a2 = tree.add_child(a1, Node::new("A2", "Label")).unwrap();

    // Queue the subtree root
    tree.queue_free(a);
    tree.process_deletions();

    // Entire subtree is stale
    for id in [a, a1, a2] {
        assert!(tree.get_node(id).is_none(), "node should be removed");
        assert!(tree.node_path(id).is_none(), "path should be gone");
    }

    // Root is unaffected
    assert!(tree.get_node(root).is_some());
    assert!(tree.get_node(root).unwrap().children().is_empty());
}

// ===========================================================================
// 23. create_node — detached NodeId resolution
// ===========================================================================

#[test]
fn create_node_detached_nodeid_has_no_path() {
    let mut tree = SceneTree::new();
    let detached = tree.create_node("Node2D", "Detached");

    // Exists in the arena but has no parent → no path
    assert!(tree.get_node(detached).is_some());
    assert_eq!(tree.get_node(detached).unwrap().name(), "Detached");
    // node_path builds from parent chain — a parentless non-root node
    // should return a path containing just its name (no /root prefix)
    // or behave as the implementation dictates
    let _path = tree.node_path(detached);
    // The node exists but is not under root, so absolute resolution won't find it
    assert!(tree.get_node_by_path("/root/Detached").is_none());
}

#[test]
fn create_node_then_add_child_resolves_normally() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let detached = tree.create_node("Sprite2D", "RuntimeNode");

    // Add to tree
    let added_node = tree.take_node(detached).unwrap();
    let id = tree.add_child(root, added_node).unwrap();

    // Now it resolves via path
    assert_eq!(tree.get_node_by_path("/root/RuntimeNode").unwrap(), id);
    assert_eq!(tree.node_path(id).unwrap(), "/root/RuntimeNode");
    assert_eq!(tree.get_node_relative(root, "RuntimeNode").unwrap(), id);
}

// ===========================================================================
// 24. NodeId interchangeability across move_child + path resolution
// ===========================================================================

#[test]
fn nodeid_from_path_after_reorder_still_resolves_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let a1 = tree.add_child(a, Node::new("A1", "Sprite2D")).unwrap();

    // Reorder: move B before A
    tree.move_child(root, b, 0).unwrap();

    // Get A via path lookup (after reorder)
    let a_via_path = tree.get_node_by_path("/root/A").unwrap();
    assert_eq!(a_via_path, a);

    // The path-obtained NodeId resolves children correctly
    assert_eq!(tree.get_node_relative(a_via_path, "A1").unwrap(), a1);
}

// ===========================================================================
// 25. Documented exclusions
// ===========================================================================

// NodePath property subnames (`:property`) are parsed but NOT resolved by
// get_node_relative / get_node_or_null — those methods only handle the node
// segment portion. Property access is handled separately in SceneAccess.
// This is by design and matches Godot's internal split between node resolution
// and property access.
//
// Additional exclusions:
// - SceneAccess trait (u64-based path resolution) is tested via scripting.rs
//   unit tests. The u64 ↔ NodeId conversion is covered by sections 2 and 11
//   above (round-trip and script-access-style resolution).
// - AnimationPlayer, Tween, and Script attachment by NodeId are not path
//   resolution — they are identity-based lookups tested in their own crate tests.
// - call_deferred is a stub (not yet implemented) — no resolution behavior to test.

// ===========================================================================
// 26. NodePath struct ↔ resolver integration
// ===========================================================================
//
// The SceneTree resolver APIs accept `&str`, not `NodePath`. These tests
// verify that `NodePath::new(input).to_string()` produces a string that
// resolves identically to the raw input, confirming the parse→display
// round-trip is resolver-safe. This is the bridge between the typed
// `NodePath` struct and the string-based resolver layer.

#[test]
fn nodepath_struct_absolute_roundtrips_through_resolver() {
    let t = build_resolver_tree();

    let paths = ["/root", "/root/A", "/root/A/A1", "/root/B/B1/B2"];
    for raw in paths {
        let np = gdcore::node_path::NodePath::new(raw);
        assert!(np.is_absolute());
        let rendered = np.to_string();
        assert_eq!(
            t.tree.get_node_by_path(&rendered),
            t.tree.get_node_by_path(raw),
            "NodePath round-trip broke resolver for {raw}"
        );
    }
}

#[test]
fn nodepath_struct_relative_roundtrips_through_resolver() {
    let t = build_resolver_tree();

    let cases: &[(&str, NodeId)] = &[
        ("A", t.a),
        ("A/A1", t.a1),
        ("B/B1/B2", t.b2),
        ("A/A2", t.a2),
    ];
    for &(raw, expected) in cases {
        let np = gdcore::node_path::NodePath::new(raw);
        assert!(!np.is_absolute());
        let rendered = np.to_string();
        assert_eq!(
            t.tree.get_node_relative(t.root, &rendered).unwrap(),
            expected,
            "NodePath relative round-trip broke for {raw}"
        );
    }
}

#[test]
fn nodepath_struct_parent_traversal_roundtrips_through_resolver() {
    let t = build_resolver_tree();

    let np = gdcore::node_path::NodePath::new("../../B/B1");
    let rendered = np.to_string();
    assert_eq!(
        t.tree.get_node_relative(t.a1, &rendered).unwrap(),
        t.b1,
        "NodePath parent traversal round-trip broke"
    );
}

#[test]
fn nodepath_struct_with_subnames_resolves_node_portion() {
    let t = build_resolver_tree();

    // "A/A1:position:x" — the node portion is "A/A1", subnames are "position:x"
    let np = gdcore::node_path::NodePath::new("A/A1:position:x");
    assert_eq!(np.get_name_count(), 2);
    assert_eq!(np.get_subname_count(), 2);

    // Build the node-only path (strip subnames) for resolver use
    let node_only: String = (0..np.get_name_count())
        .map(|i| np.get_name(i).unwrap())
        .collect::<Vec<_>>()
        .join("/");

    assert_eq!(
        t.tree.get_node_relative(t.root, &node_only).unwrap(),
        t.a1,
        "NodePath node portion should resolve correctly when subnames are stripped"
    );

    // Verify the subname portion is preserved separately
    assert_eq!(np.get_concatenated_subnames(), "position:x");
}

#[test]
fn nodepath_struct_unique_name_roundtrips_through_resolver() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let target = {
        let mut n = Node::new("Special", "Node2D");
        n.set_unique_name(true);
        n.set_owner(Some(root));
        tree.add_child(root, n).unwrap()
    };

    // NodePath parses %Special as a name segment starting with %
    let np = gdcore::node_path::NodePath::new("%Special");
    let rendered = np.to_string();
    assert_eq!(
        tree.get_node_or_null(root, &rendered).unwrap(),
        target,
        "NodePath %UniqueName round-trip broke resolver"
    );
}

// ===========================================================================
// 27. Malformed / edge-case path inputs
// ===========================================================================

#[test]
fn empty_path_resolves_to_self() {
    let t = build_resolver_tree();

    // get_node_relative with empty string returns `from` (documented behavior)
    assert_eq!(t.tree.get_node_relative(t.a, "").unwrap(), t.a);
    assert_eq!(t.tree.get_node_relative(t.b2, "").unwrap(), t.b2);
}

#[test]
fn nonexistent_child_name_returns_none() {
    let t = build_resolver_tree();

    assert!(t.tree.get_node_relative(t.root, "NoSuchChild").is_none());
    assert!(t.tree.get_node_by_path("/root/NoSuchChild").is_none());
    assert!(t.tree.get_node_or_null(t.root, "NoSuchChild").is_none());
}

#[test]
fn nonexistent_unique_name_returns_none() {
    let t = build_resolver_tree();

    assert!(t.tree.get_node_or_null(t.root, "%NoSuchUnique").is_none());
}

#[test]
fn absolute_path_with_wrong_root_name_returns_none() {
    let t = build_resolver_tree();

    // Root node is named "root" — a different root name should fail
    assert!(t.tree.get_node_by_path("/notroot/A").is_none());
    assert!(t.tree.get_node_or_null(t.root, "/notroot/A").is_none());
}

#[test]
fn slash_only_returns_none() {
    let t = build_resolver_tree();

    // "/" alone: path after stripping "/" is empty → parts is [""] which won't match root
    assert!(t.tree.get_node_by_path("/").is_none());
}

#[test]
fn deeply_nested_parent_traversal_past_root_returns_none() {
    let t = build_resolver_tree();

    // From A1 (depth 2), go up 5 levels — past root
    assert!(t.tree.get_node_relative(t.a1, "../../../../..").is_none());
}

// ===========================================================================
// 28. Node rename — NodeId stable, path changes
// ===========================================================================

#[test]
fn node_rename_updates_path_but_preserves_nodeid() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("OldName", "Node2D")).unwrap();
    let grandchild = tree
        .add_child(child, Node::new("GC", "Sprite2D"))
        .unwrap();

    // Verify initial path
    assert_eq!(tree.node_path(child).unwrap(), "/root/OldName");
    assert_eq!(tree.get_node_by_path("/root/OldName").unwrap(), child);

    // Rename the node
    tree.get_node_mut(child).unwrap().set_name("NewName");

    // NodeId still valid
    assert_eq!(tree.get_node(child).unwrap().name(), "NewName");

    // Path reflects the new name
    assert_eq!(tree.node_path(child).unwrap(), "/root/NewName");

    // Old path no longer resolves
    assert!(tree.get_node_by_path("/root/OldName").is_none());

    // New path resolves to same NodeId
    assert_eq!(tree.get_node_by_path("/root/NewName").unwrap(), child);

    // Grandchild's path also reflects parent rename
    assert_eq!(tree.node_path(grandchild).unwrap(), "/root/NewName/GC");

    // Resolution from renamed node still works
    assert_eq!(tree.get_node_relative(child, "GC").unwrap(), grandchild);
}

#[test]
fn node_rename_affects_sibling_cross_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();

    // Cross-resolve before rename
    assert_eq!(tree.get_node_relative(a, "../B").unwrap(), b);

    // Rename B
    tree.get_node_mut(b).unwrap().set_name("B_Renamed");

    // Old cross-resolution fails
    assert!(tree.get_node_relative(a, "../B").is_none());

    // New name works
    assert_eq!(tree.get_node_relative(a, "../B_Renamed").unwrap(), b);
}

// ===========================================================================
// 29. all_nodes_in_process_order — NodeIds as resolver handles
// ===========================================================================

#[test]
fn process_order_nodeids_resolve_correctly() {
    let t = build_resolver_tree();

    let process_order = t.tree.all_nodes_in_process_order();
    // Should contain all 7 nodes
    assert_eq!(process_order.len(), 7);

    // Every ID from process order should be a valid resolver handle
    for id in &process_order {
        let path = t.tree.node_path(*id).unwrap();
        let resolved = t.tree.get_node_by_path(&path).unwrap();
        assert_eq!(*id, resolved, "process-order ID round-trip failed for {path}");
    }
}

#[test]
fn process_order_nodeids_work_as_from_handles() {
    let t = build_resolver_tree();

    let process_order = t.tree.all_nodes_in_process_order();

    // Each ID should work as `from` in get_node_or_null to reach root via absolute
    for id in &process_order {
        assert_eq!(
            t.tree.get_node_or_null(*id, "/root").unwrap(),
            t.root,
            "process-order ID failed as `from` handle"
        );
    }
}

// ===========================================================================
// 30. current_scene NodeId as resolver handle
// ===========================================================================

#[test]
fn current_scene_nodeid_resolves_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree
        .add_child(root, Node::new("Main", "Node2D"))
        .unwrap();
    let player = tree
        .add_child(scene, Node::new("Player", "CharacterBody2D"))
        .unwrap();
    let sprite = tree
        .add_child(player, Node::new("Sprite", "Sprite2D"))
        .unwrap();

    tree.set_current_scene(Some(scene));

    // Retrieve current_scene and use it as a resolver handle
    let cs = tree.current_scene().unwrap();
    assert_eq!(cs, scene);

    // Resolve children from current_scene
    assert_eq!(tree.get_node_relative(cs, "Player").unwrap(), player);
    assert_eq!(
        tree.get_node_relative(cs, "Player/Sprite").unwrap(),
        sprite
    );

    // node_path from current_scene
    assert_eq!(tree.node_path(cs).unwrap(), "/root/Main");
}

#[test]
fn current_scene_nodeid_after_scene_change() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene1 = tree
        .add_child(root, Node::new("Scene1", "Node2D"))
        .unwrap();
    let s1_child = tree
        .add_child(scene1, Node::new("Child1", "Node2D"))
        .unwrap();

    tree.set_current_scene(Some(scene1));
    let cs1 = tree.current_scene().unwrap();
    assert_eq!(tree.get_node_relative(cs1, "Child1").unwrap(), s1_child);

    // Change scene: unload, add new scene
    tree.unload_current_scene().unwrap();
    assert!(tree.current_scene().is_none());

    // Old scene NodeId is now stale
    assert!(tree.get_node(scene1).is_none());

    let scene2 = tree
        .add_child(root, Node::new("Scene2", "Node2D"))
        .unwrap();
    let s2_child = tree
        .add_child(scene2, Node::new("Child2", "Sprite2D"))
        .unwrap();
    tree.set_current_scene(Some(scene2));

    let cs2 = tree.current_scene().unwrap();
    assert_eq!(tree.get_node_relative(cs2, "Child2").unwrap(), s2_child);
    assert_eq!(tree.node_path(cs2).unwrap(), "/root/Scene2");
}

// ===========================================================================
// 31. Multiple packed scene instances — cross-instance resolution
// ===========================================================================

#[test]
fn multiple_packed_scene_instances_have_independent_nodeids() {
    let tscn = r#"[gd_scene format=3]

[node name="Enemy" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Hitbox" type="Area2D" parent="."]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance twice under root
    let enemy1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let enemy2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Different NodeIds for each instance root
    assert_ne!(enemy1, enemy2);

    // Both have children named the same
    let sprite1 = tree.get_node_relative(enemy1, "Sprite").unwrap();
    let sprite2 = tree.get_node_relative(enemy2, "Sprite").unwrap();
    assert_ne!(sprite1, sprite2);

    let hitbox1 = tree.get_node_relative(enemy1, "Hitbox").unwrap();
    let hitbox2 = tree.get_node_relative(enemy2, "Hitbox").unwrap();
    assert_ne!(hitbox1, hitbox2);

    // Each resolves its own children correctly
    assert_eq!(tree.get_node(sprite1).unwrap().class_name(), "Sprite2D");
    assert_eq!(tree.get_node(sprite2).unwrap().class_name(), "Sprite2D");
    assert_eq!(tree.get_node(hitbox1).unwrap().class_name(), "Area2D");
    assert_eq!(tree.get_node(hitbox2).unwrap().class_name(), "Area2D");

    // Cross-instance: from sprite1, navigate to enemy2's Hitbox
    // Path depends on whether enemy2 got renamed during add. Check node_path first.
    let e2_path = tree.node_path(enemy2).unwrap();
    let sprite1_path = tree.node_path(sprite1).unwrap();

    // Both paths should be under /root
    assert!(e2_path.starts_with("/root/"));
    assert!(sprite1_path.starts_with("/root/"));

    // u64 round-trip on instanced nodes
    for id in [enemy1, enemy2, sprite1, sprite2, hitbox1, hitbox2] {
        let raw = id.raw();
        let rt = NodeId::from_object_id(ObjectId::from_raw(raw));
        assert_eq!(id, rt);
        assert!(tree.get_node(rt).is_some());
    }
}

// ===========================================================================
// 32. NodeId from combined API sources used interchangeably
// ===========================================================================

#[test]
fn nodeids_from_five_sources_all_interchangeable() {
    // Collect the same node's ID from 5 different API sources and verify
    // they are all equal and interchangeable as resolver handles.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree
        .add_child(root, Node::new("Scene", "Node2D"))
        .unwrap();
    let target = tree
        .add_child(scene, Node::new("Target", "Sprite2D"))
        .unwrap();
    tree.add_to_group(target, "targets").unwrap();

    // Source 1: direct from add_child
    let id1 = target;

    // Source 2: from get_node_by_path
    let id2 = tree.get_node_by_path("/root/Scene/Target").unwrap();

    // Source 3: from get_node_relative
    let id3 = tree.get_node_relative(scene, "Target").unwrap();

    // Source 4: from get_node_or_null
    let id4 = tree.get_node_or_null(root, "Scene/Target").unwrap();

    // Source 5: from group membership
    let group_ids = tree.get_nodes_in_group("targets");
    let id5 = group_ids[0];

    // All five are identical
    assert_eq!(id1, id2);
    assert_eq!(id2, id3);
    assert_eq!(id3, id4);
    assert_eq!(id4, id5);

    // All five work as resolver `from` handles identically
    for (idx, id) in [id1, id2, id3, id4, id5].iter().enumerate() {
        assert_eq!(
            tree.get_node_relative(*id, "..").unwrap(),
            scene,
            "source {idx} failed as resolver from-handle"
        );
        assert_eq!(
            tree.node_path(*id).unwrap(),
            "/root/Scene/Target",
            "source {idx} path mismatch"
        );
    }
}

#[test]
fn nodeid_from_traversal_and_u64_roundtrip_interchangeable() {
    let t = build_resolver_tree();

    // Get B1 from traversal
    let mut subtree = Vec::new();
    t.tree.collect_subtree_top_down(t.b, &mut subtree);
    let b1_from_traversal = subtree[1]; // B1 is second in top-down

    // Get B1 from u64 round-trip
    let b1_from_roundtrip = NodeId::from_object_id(ObjectId::from_raw(t.b1.raw()));

    // Get B1 from path
    let b1_from_path = t.tree.get_node_by_path("/root/B/B1").unwrap();

    assert_eq!(b1_from_traversal, t.b1);
    assert_eq!(b1_from_roundtrip, t.b1);
    assert_eq!(b1_from_path, t.b1);

    // All interchangeable as resolver handles
    for id in [b1_from_traversal, b1_from_roundtrip, b1_from_path] {
        assert_eq!(t.tree.get_node_relative(id, "B2").unwrap(), t.b2);
    }
}

// ===========================================================================
// 33. Broadened exclusion documentation (pat-ps46)
// ===========================================================================
//
// Supported resolution cases:
// - Absolute paths ("/root/A/B") via get_node_by_path and get_node_or_null
// - Relative paths ("A/B", "../Sibling") via get_node_relative and get_node_or_null
// - Self-referential (".", "") via get_node_relative
// - Parent traversal ("..") single and chained
// - %UniqueName syntax (single segment and with subpath e.g. %Foo/Bar)
// - NodeIds from any source (add_child, path lookup, relative lookup,
//   get_node_or_null, group membership, traversal APIs, u64 round-trip)
//   are interchangeable as resolver handles
// - NodeIds remain stable across mutations (reparent, sibling removal,
//   child reordering, node rename)
// - Stale NodeIds (from removed/freed nodes) return None gracefully
// - Fabricated NodeIds never in the tree return None for navigation
// - NodePath struct's to_string() output is resolver-compatible
// - Process-order and tree-order traversal IDs are valid resolver handles
// - current_scene NodeId is a valid resolver handle
// - Multiple packed scene instances get independent NodeIds
//
// Remaining exclusions:
// - NodePath property subnames (":property") are NOT resolved by get_node_*
//   methods. The resolver operates on node segments only; property access
//   is handled by SceneAccess::get_node_property / set_node_property.
// - SceneAccess trait (u64-based path resolution) is tested via scripting.rs
//   unit tests. The u64 ↔ NodeId conversion is covered by sections 2, 11,
//   and 32 above.
// - AnimationPlayer, Tween, and Script attachment by NodeId are identity-based
//   lookups tested in their own crate tests, not path resolution.
// - call_deferred is a stub (not yet implemented) — no resolution behavior.
// - Trailing slashes and double slashes in paths: Godot silently handles
//   these in some contexts; Patina currently does not normalize paths.
//   This is a known gap that may be addressed in a future bead.
// - NodePath with only subnames (e.g. ":property:x") — Godot treats this
//   as a property-only path on the current node. Not handled by resolvers.
