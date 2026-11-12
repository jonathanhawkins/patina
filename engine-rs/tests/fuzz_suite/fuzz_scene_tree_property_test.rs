//! Property-based tests for SceneTree mutation operations.
//!
//! Exercises `add_child`, `remove_node`, `reparent`, `move_child`,
//! `queue_free`, groups, and path resolution under random operation
//! sequences. Verifies structural invariants hold after every operation.

use proptest::prelude::*;

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Invariant checkers
// ---------------------------------------------------------------------------

/// Verifies that the scene tree's internal state is consistent:
/// 1. Every child's parent field points back to the actual parent.
/// 2. Every parent's child list only contains IDs that exist in the arena.
/// 3. The root node has no parent.
/// 4. Node count matches arena size.
fn assert_invariants(tree: &SceneTree) {
    let root = tree.root_id();

    // Root has no parent.
    assert!(
        tree.get_node(root).unwrap().parent().is_none(),
        "Root must have no parent",
    );

    // Walk every node.
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        assert!(visited.insert(id), "Cycle detected at node {:?}", id);
        let node = tree.get_node(id).expect("Node in child list not in arena");
        for &child_id in node.children() {
            let child = tree
                .get_node(child_id)
                .expect("Child ID in parent's list not found in arena");
            assert_eq!(
                child.parent(),
                Some(id),
                "Child {:?} parent mismatch: expected {:?}, got {:?}",
                child_id,
                id,
                child.parent(),
            );
            stack.push(child_id);
        }
    }

    // Every node reachable from root = all nodes in tree.
    assert_eq!(
        visited.len(),
        tree.node_count(),
        "Orphaned nodes: {} reachable from root vs {} in arena",
        visited.len(),
        tree.node_count(),
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_node(name: &str) -> Node {
    Node::new(name, "Node")
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Adding N children to root preserves all invariants and node count.
    #[test]
    fn add_children_preserves_invariants(n in 1usize..50) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        for i in 0..n {
            let node = make_node(&format!("child_{i}"));
            tree.add_child(root, node).unwrap();
        }
        assert_eq!(tree.node_count(), n + 1); // +1 for root
        assert_invariants(&tree);
    }

    /// Adding children to random valid parents builds a consistent tree.
    #[test]
    fn random_tree_shape_preserves_invariants(
        depths in prop::collection::vec(0usize..20, 1..40),
    ) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut all_ids = vec![root];

        for (i, &target_depth) in depths.iter().enumerate() {
            // Pick a parent from existing nodes (mod to stay in range).
            let parent_idx = target_depth % all_ids.len();
            let parent_id = all_ids[parent_idx];
            let node = make_node(&format!("n_{i}"));
            let child_id = tree.add_child(parent_id, node).unwrap();
            all_ids.push(child_id);
        }

        assert_eq!(tree.node_count(), all_ids.len());
        assert_invariants(&tree);
    }

    /// Removing a leaf node preserves invariants.
    #[test]
    fn remove_leaf_preserves_invariants(n in 2usize..30) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut ids = vec![];
        for i in 0..n {
            let node = make_node(&format!("child_{i}"));
            let id = tree.add_child(root, node).unwrap();
            ids.push(id);
        }

        // Remove the last child (leaf).
        let removed = tree.remove_node(*ids.last().unwrap()).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(tree.node_count(), n); // root + (n-1) children
        assert_invariants(&tree);
    }

    /// Removing a subtree removes exactly the right number of nodes.
    #[test]
    fn remove_subtree_removes_descendants(
        chain_len in 2usize..15,
    ) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Build a chain: root -> c0 -> c1 -> ... -> c_{chain_len-1}
        let mut ids = vec![];
        let mut parent = root;
        for i in 0..chain_len {
            let node = make_node(&format!("chain_{i}"));
            let id = tree.add_child(parent, node).unwrap();
            ids.push(id);
            parent = id;
        }

        // Remove the first in the chain — should remove the entire subtree.
        let removed = tree.remove_node(ids[0]).unwrap();
        assert_eq!(removed.len(), chain_len);
        assert_eq!(tree.node_count(), 1); // only root remains
        assert_invariants(&tree);
    }

    /// Reparenting a node preserves invariants and doesn't orphan nodes.
    #[test]
    fn reparent_preserves_invariants(n in 3usize..20) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut ids = vec![];
        for i in 0..n {
            let node = make_node(&format!("child_{i}"));
            let id = tree.add_child(root, node).unwrap();
            ids.push(id);
        }

        // Reparent the first child under the second child.
        tree.reparent(ids[0], ids[1]).unwrap();

        assert_eq!(tree.node_count(), n + 1);
        assert_invariants(&tree);

        // ids[0] is now a child of ids[1].
        let moved = tree.get_node(ids[0]).unwrap();
        assert_eq!(moved.parent(), Some(ids[1]));
    }

    /// move_child reorders children without changing tree structure.
    #[test]
    fn move_child_preserves_count_and_invariants(n in 3usize..15, to in 0usize..15) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut ids = vec![];
        for i in 0..n {
            let node = make_node(&format!("child_{i}"));
            let id = tree.add_child(root, node).unwrap();
            ids.push(id);
        }

        let to_clamped = to % n;
        tree.move_child(root, ids[0], to_clamped).unwrap();

        assert_eq!(tree.node_count(), n + 1);
        assert_invariants(&tree);
    }

    /// node_path returns consistent paths after tree mutations.
    #[test]
    fn paths_consistent_after_mutations(n in 2usize..10) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut ids = vec![];
        for i in 0..n {
            let node = make_node(&format!("child_{i}"));
            let id = tree.add_child(root, node).unwrap();
            ids.push(id);
        }

        // Verify each child has a path starting with /root/
        for &id in &ids {
            let path = tree.node_path(id).expect("node should have a path");
            prop_assert!(
                path.starts_with("/root/"),
                "Path should start with /root/, got: {}",
                path,
            );
        }

        // After removing a node, its path should be gone.
        let removed_id = ids[0];
        tree.remove_node(removed_id).unwrap();
        prop_assert!(
            tree.node_path(removed_id).is_none(),
            "Removed node should have no path",
        );
    }
}

// ---------------------------------------------------------------------------
// Deterministic edge-case tests
// ---------------------------------------------------------------------------

#[test]
fn cannot_remove_root() {
    let mut tree = SceneTree::new();
    let result = tree.remove_node(tree.root_id());
    assert!(result.is_err());
}

#[test]
fn cannot_reparent_root() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = make_node("child");
    let child_id = tree.add_child(root, child).unwrap();
    let result = tree.reparent(root, child_id);
    assert!(result.is_err());
}

#[test]
fn add_child_to_nonexistent_parent_fails() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = make_node("child");
    let child_id = tree.add_child(root, child).unwrap();

    // Remove the child, then try to add to it.
    tree.remove_node(child_id).unwrap();
    let orphan = make_node("orphan");
    let result = tree.add_child(child_id, orphan);
    assert!(result.is_err());
}

#[test]
fn reparent_to_nonexistent_node_fails() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = make_node("child");
    let child_id = tree.add_child(root, child).unwrap();

    let fake_id = {
        let temp = make_node("temp");
        let id = tree.add_child(root, temp).unwrap();
        tree.remove_node(id).unwrap();
        id
    };

    let result = tree.reparent(child_id, fake_id);
    assert!(result.is_err());
}

#[test]
fn groups_cleaned_up_on_remove() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let child = make_node("grouped_child");
    let child_id = tree.add_child(root, child).unwrap();
    tree.add_to_group(child_id, "enemies").unwrap();

    // Verify the node is in the group.
    let members = tree.get_nodes_in_group("enemies");
    assert!(members.contains(&child_id));

    // Remove the node.
    tree.remove_node(child_id).unwrap();

    // Group should no longer contain the removed node.
    let members = tree.get_nodes_in_group("enemies");
    assert!(!members.contains(&child_id));
}

#[test]
fn queue_free_defers_deletion() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = make_node("deferred");
    let child_id = tree.add_child(root, child).unwrap();

    // queue_free marks for deferred deletion.
    tree.queue_free(child_id);

    // Node still exists until process_deletions.
    assert!(tree.get_node(child_id).is_some());

    // Process deletions.
    tree.process_deletions();

    // Now it's gone.
    assert!(tree.get_node(child_id).is_none());
    assert_invariants(&tree);
}

#[test]
fn stress_add_remove_cycle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    for cycle in 0..100 {
        let node = make_node(&format!("cycle_{cycle}"));
        let id = tree.add_child(root, node).unwrap();
        assert_invariants(&tree);
        tree.remove_node(id).unwrap();
        assert_invariants(&tree);
    }

    // After all cycles, only root remains.
    assert_eq!(tree.node_count(), 1);
}

#[test]
fn stress_wide_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add 500 children to root.
    let mut ids = vec![];
    for i in 0..500 {
        let node = make_node(&format!("wide_{i}"));
        let id = tree.add_child(root, node).unwrap();
        ids.push(id);
    }
    assert_eq!(tree.node_count(), 501);
    assert_invariants(&tree);

    // Remove half of them.
    for &id in ids.iter().take(250) {
        tree.remove_node(id).unwrap();
    }
    assert_eq!(tree.node_count(), 251);
    assert_invariants(&tree);
}

#[test]
fn stress_deep_chain() {
    let mut tree = SceneTree::new();
    let mut parent = tree.root_id();

    // Build a chain 200 nodes deep.
    for i in 0..200 {
        let node = make_node(&format!("deep_{i}"));
        let id = tree.add_child(parent, node).unwrap();
        parent = id;
    }
    assert_eq!(tree.node_count(), 201);
    assert_invariants(&tree);

    // The deepest node should have a path with 201 components.
    let path = tree.node_path(parent).unwrap();
    let depth = path.split('/').filter(|s| !s.is_empty()).count();
    assert_eq!(depth, 201); // root + 200 children
}
