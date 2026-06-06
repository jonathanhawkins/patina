//! pat-i6h: WeakRef auto-invalidation on object free.
//!
//! Validates that WeakRef.get_ref() returns None after the referenced object
//! is freed via queue_free + process_deletions, matching Godot's behavior.

use gdobject::weak_ref::{self, WeakRef};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

#[test]
fn weakref_valid_before_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Temp", "Node")).unwrap();

    let obj_id = child_id.object_id();
    let weak = WeakRef::new(obj_id);

    assert!(
        weak.get_ref().is_some(),
        "WeakRef should be valid before free"
    );
}

#[test]
fn weakref_auto_invalidates_after_queue_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Temp", "Node")).unwrap();

    let obj_id = child_id.object_id();
    let weak = WeakRef::new(obj_id);

    assert!(
        weak.get_ref().is_some(),
        "WeakRef should be valid before free"
    );

    tree.queue_free(child_id);
    tree.process_deletions();

    assert!(
        tree.get_node(child_id).is_none(),
        "node should be gone after free"
    );

    assert!(
        weak.get_ref().is_none(),
        "WeakRef.get_ref() must return None after object is freed"
    );
}

#[test]
fn weakref_auto_invalidates_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent_id = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
    let child_id = tree
        .add_child(parent_id, Node::new("Child", "Node"))
        .unwrap();

    let parent_weak = WeakRef::new(parent_id.object_id());
    let child_weak = WeakRef::new(child_id.object_id());

    assert!(parent_weak.get_ref().is_some());
    assert!(child_weak.get_ref().is_some());

    // Free the parent — child should also be invalidated.
    tree.queue_free(parent_id);
    tree.process_deletions();

    assert!(
        parent_weak.get_ref().is_none(),
        "parent WeakRef must be None after free"
    );
    assert!(
        child_weak.get_ref().is_none(),
        "child WeakRef must be None when parent is freed"
    );
}

#[test]
fn weakref_still_valid_after_unrelated_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a_id = tree.add_child(root, Node::new("NodeA", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("NodeB", "Node")).unwrap();

    let weak_a = WeakRef::new(a_id.object_id());
    let weak_b = WeakRef::new(b_id.object_id());

    // Free only A
    tree.queue_free(a_id);
    tree.process_deletions();

    assert!(weak_a.get_ref().is_none(), "A should be invalidated");
    assert!(weak_b.get_ref().is_some(), "B should still be valid");
}

#[test]
fn weakref_to_variant_returns_nil_after_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Temp", "Node")).unwrap();

    let weak = WeakRef::new(child_id.object_id());
    assert!(matches!(weak.to_variant(), gdvariant::Variant::Int(_)));

    tree.queue_free(child_id);
    tree.process_deletions();

    assert_eq!(weak.to_variant(), gdvariant::Variant::Nil);
}

#[test]
fn weakref_manual_invalidation_still_works() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Temp", "Node")).unwrap();

    let mut weak = WeakRef::new(child_id.object_id());
    assert!(weak.get_ref().is_some());

    weak.invalidate();
    assert!(weak.get_ref().is_none(), "manual invalidation still works");
    // Node is still alive in tree though
    assert!(tree.get_node(child_id).is_some());
}
