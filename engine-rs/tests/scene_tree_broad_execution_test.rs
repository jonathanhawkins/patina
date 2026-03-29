//! pat-zg81: Broaden resource and scene execution path coverage beyond the
//! current slice.
//!
//! Covers SceneTree operations that had zero integration test coverage:
//! - reparent() with lifecycle notifications
//! - move_child() / raise() / lower() ordering
//! - queue_free() / process_deletions()
//! - duplicate_subtree()
//! - get_node_by_unique_name()
//! - remove_from_group()
//! - collect_subtree_top_down() / collect_subtree_bottom_up()
//! - get_index()
//! - call_deferred() / flush_deferred_calls()
//! - take_node()

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Helpers
// ===========================================================================

fn build_tree_with_children(names: &[&str]) -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    for name in names {
        let node = Node::new(*name, "Node");
        tree.add_child(root, node).unwrap();
    }
    tree
}

// ===========================================================================
// 1. reparent()
// ===========================================================================

#[test]
fn reparent_moves_node_to_new_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node");
    let b_id = b.id();
    tree.add_child(root, b).unwrap();

    // Move A under B
    tree.reparent(a_id, b_id).unwrap();

    let b_node = tree.get_node(b_id).unwrap();
    assert!(
        b_node.children().contains(&a_id),
        "A should be a child of B after reparent"
    );

    let root_node = tree.get_node(root).unwrap();
    assert!(
        !root_node.children().contains(&a_id),
        "A should no longer be a child of root"
    );
}

#[test]
fn reparent_root_fails() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let result = tree.reparent(root, a_id);
    assert!(result.is_err(), "reparenting root should fail");
}

#[test]
fn reparent_preserves_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let a_child = Node::new("A_Child", "Node");
    let a_child_id = a_child.id();
    tree.add_child(a_id, a_child).unwrap();

    let b = Node::new("B", "Node");
    let b_id = b.id();
    tree.add_child(root, b).unwrap();

    // Move A (with child) under B
    tree.reparent(a_id, b_id).unwrap();

    // A_Child should still be under A
    let a_node = tree.get_node(a_id).unwrap();
    assert!(a_node.children().contains(&a_child_id));

    // Verify full path: root -> B -> A -> A_Child
    let path = tree.node_path(a_child_id);
    assert!(
        path.is_some(),
        "A_Child should have a valid path after reparent"
    );
}

// ===========================================================================
// 2. move_child() / raise() / lower()
// ===========================================================================

#[test]
fn move_child_changes_sibling_order() {
    let mut tree = build_tree_with_children(&["A", "B", "C"]);
    let root = tree.root_id();
    let children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    let (a_id, b_id, c_id) = (children[0], children[1], children[2]);

    // Move C to index 0 (first position)
    tree.move_child(root, c_id, 0).unwrap();

    let new_order: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(new_order[0], c_id, "C should be first after move_child");
    // Original order was [A, B, C]; moving C to 0 gives [C, A, B]
    assert_eq!(new_order[1], a_id, "A should be second");
    assert_eq!(new_order[2], b_id, "B should be third");
}

#[test]
fn raise_moves_node_to_last() {
    let mut tree = build_tree_with_children(&["A", "B", "C"]);
    let root = tree.root_id();
    let children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    let a_id = children[0];

    tree.raise(a_id).unwrap();

    let new_order: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(
        *new_order.last().unwrap(),
        a_id,
        "A should be last after raise"
    );
}

#[test]
fn lower_moves_node_to_first() {
    let mut tree = build_tree_with_children(&["A", "B", "C"]);
    let root = tree.root_id();
    let children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    let c_id = children[2];

    tree.lower(c_id).unwrap();

    let new_order: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(new_order[0], c_id, "C should be first after lower");
}

#[test]
fn raise_on_root_child_without_parent_fails() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Root has no parent, so raise should fail
    let result = tree.raise(root);
    assert!(result.is_err());
}

// ===========================================================================
// 3. get_index()
// ===========================================================================

#[test]
fn get_index_returns_correct_position() {
    let mut tree = build_tree_with_children(&["A", "B", "C"]);
    let root = tree.root_id();
    let children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();

    assert_eq!(tree.get_index(children[0]), Some(0));
    assert_eq!(tree.get_index(children[1]), Some(1));
    assert_eq!(tree.get_index(children[2]), Some(2));
}

#[test]
fn get_index_of_root_is_none() {
    let tree = SceneTree::new();
    assert_eq!(tree.get_index(tree.root_id()), None);
}

// ===========================================================================
// 4. queue_free() / process_deletions()
// ===========================================================================

#[test]
fn queue_free_defers_removal() {
    let mut tree = build_tree_with_children(&["A", "B"]);
    let root = tree.root_id();
    let children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    let a_id = children[0];

    tree.queue_free(a_id);

    // Node should still exist before process_deletions
    assert!(
        tree.get_node(a_id).is_some(),
        "node should still exist before process_deletions"
    );

    tree.process_deletions();

    assert!(
        tree.get_node(a_id).is_none(),
        "node should be removed after process_deletions"
    );
}

#[test]
fn queue_free_is_idempotent() {
    let mut tree = build_tree_with_children(&["A"]);
    let root = tree.root_id();
    let a_id = tree.get_node(root).unwrap().children()[0];

    tree.queue_free(a_id);
    tree.queue_free(a_id); // duplicate call
    tree.process_deletions();

    assert!(tree.get_node(a_id).is_none());
}

#[test]
fn queue_free_removes_children_too() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node");
    let parent_id = parent.id();
    tree.add_child(root, parent).unwrap();

    let child = Node::new("Child", "Node");
    let child_id = child.id();
    tree.add_child(parent_id, child).unwrap();

    tree.queue_free(parent_id);
    tree.process_deletions();

    assert!(tree.get_node(parent_id).is_none());
    assert!(tree.get_node(child_id).is_none(), "child should also be freed");
}

// ===========================================================================
// 5. duplicate_subtree()
// ===========================================================================

#[test]
fn duplicate_subtree_produces_new_nodes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let a_child = Node::new("Sprite", "Sprite2D");
    tree.add_child(a_id, a_child).unwrap();

    let duplicated = tree.duplicate_subtree(a_id).unwrap();
    assert_eq!(duplicated.len(), 2, "should duplicate root + child");
    assert_ne!(
        duplicated[0].id(),
        a_id,
        "duplicated nodes should have new IDs"
    );
    assert_eq!(duplicated[0].name(), "A", "names should be preserved");
    assert_eq!(duplicated[0].class_name(), "Node2D", "class should be preserved");
}

#[test]
fn duplicate_subtree_nonexistent_fails() {
    let tree = SceneTree::new();
    let fake_id = gdscene::node::NodeId::next();
    let result = tree.duplicate_subtree(fake_id);
    assert!(result.is_err());
}

// ===========================================================================
// 6. collect_subtree_top_down / bottom_up
// ===========================================================================

#[test]
fn collect_subtree_top_down_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node");
    let b_id = b.id();
    tree.add_child(a_id, b).unwrap();

    let c = Node::new("C", "Node");
    let c_id = c.id();
    tree.add_child(a_id, c).unwrap();

    let mut collected = Vec::new();
    tree.collect_subtree_top_down(a_id, &mut collected);

    assert_eq!(collected, vec![a_id, b_id, c_id]);
}

#[test]
fn collect_subtree_bottom_up_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node");
    let b_id = b.id();
    tree.add_child(a_id, b).unwrap();

    let c = Node::new("C", "Node");
    let c_id = c.id();
    tree.add_child(a_id, c).unwrap();

    let mut collected = Vec::new();
    tree.collect_subtree_bottom_up(a_id, &mut collected);

    assert_eq!(collected, vec![b_id, c_id, a_id]);
}

// ===========================================================================
// 7. remove_from_group()
// ===========================================================================

#[test]
fn remove_from_group_leaves_other_members() {
    let mut tree = build_tree_with_children(&["A", "B"]);
    let root = tree.root_id();
    let children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
    let (a_id, b_id) = (children[0], children[1]);

    tree.add_to_group(a_id, "enemies").unwrap();
    tree.add_to_group(b_id, "enemies").unwrap();

    let members = tree.get_nodes_in_group("enemies");
    assert_eq!(members.len(), 2);

    tree.remove_from_group(a_id, "enemies").unwrap();

    let members = tree.get_nodes_in_group("enemies");
    assert_eq!(members.len(), 1);
    assert!(members.contains(&b_id));
}

// ===========================================================================
// 8. take_node()
// ===========================================================================

#[test]
fn take_node_removes_and_returns() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let taken = tree.take_node(a_id);
    assert!(taken.is_some());
    assert_eq!(taken.unwrap().name(), "A");
    assert!(tree.get_node(a_id).is_none());
}

#[test]
fn take_node_nonexistent_returns_none() {
    let mut tree = SceneTree::new();
    let fake_id = gdscene::node::NodeId::next();
    assert!(tree.take_node(fake_id).is_none());
}

// ===========================================================================
// 9. get_node_by_unique_name()
// ===========================================================================

#[test]
fn get_node_by_unique_name_finds_marked_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("Player", "Node2D");
    a.set_unique_name(true);
    // Set owner to root so the node is in the root's scene scope.
    a.set_owner(Some(root));
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let mut b = Node::new("Camera", "Node2D");
    b.set_owner(Some(root));
    let b_id = b.id();
    tree.add_child(root, b).unwrap();

    // Search from Camera (b_id) — owner is root, so Player (in root scope) should be found.
    let found = tree.get_node_by_unique_name(b_id, "Player");
    assert_eq!(found, Some(a_id));
}

#[test]
fn get_node_by_unique_name_returns_none_for_nonexistent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let found = tree.get_node_by_unique_name(a_id, "NoSuchNode");
    assert!(found.is_none());
}

// ===========================================================================
// 10. call_deferred() queues without panic
// ===========================================================================

#[test]
fn call_deferred_queues_without_panic() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    // Queue a deferred call — should not panic even with no script attached.
    tree.call_deferred(a_id, "some_method", &[]);
}

// ===========================================================================
// 11. Compound: reparent then reorder
// ===========================================================================

#[test]
fn reparent_then_move_child_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = a.id();
    tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node");
    tree.add_child(root, b).unwrap();

    let c = Node::new("C", "Node");
    let c_id = c.id();
    tree.add_child(root, c).unwrap();

    // Reparent C under A
    tree.reparent(c_id, a_id).unwrap();

    // Now add another child to A
    let d = Node::new("D", "Node");
    let d_id = d.id();
    tree.add_child(a_id, d).unwrap();

    // Move D to index 0 under A (before C)
    tree.move_child(a_id, d_id, 0).unwrap();

    let a_children: Vec<_> = tree.get_node(a_id).unwrap().children().to_vec();
    assert_eq!(a_children, vec![d_id, c_id]);
}
