//! Object.free() plus use-after-free guard tests.
//!
//! Tests both SceneTree-level queue_free/process_deletions and the ObjectBase-level
//! free() API with use-after-free guards.

use gdscene::scene_tree::SceneTree;
use gdscene::node::Node;
use gdobject::object::{GenericObject, GodotObject, ObjectBase};
use gdobject::weak_ref::{self, WeakRef};
use gdvariant::Variant;

#[test]
fn get_node_returns_none_after_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("ToFree", "Node")).unwrap();

    tree.queue_free(child_id);
    tree.process_deletions();

    assert!(tree.get_node(child_id).is_none(), "get_node must return None after free");
}

#[test]
fn get_node_does_not_panic_after_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Temp", "Node")).unwrap();

    tree.queue_free(child_id);
    tree.process_deletions();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.get_node(child_id)
    }));

    match result {
        Ok(None) => {}
        Ok(Some(_)) => panic!("should not return Some for freed node"),
        Err(_) => panic!("should not panic on freed node access"),
    }
}

#[test]
fn node_path_returns_none_after_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Gone", "Node")).unwrap();

    tree.queue_free(child_id);
    tree.process_deletions();

    assert!(tree.node_path(child_id).is_none(), "node_path must return None after free");
}

#[test]
fn double_free_does_not_panic() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Double", "Node")).unwrap();

    tree.queue_free(child_id);
    tree.queue_free(child_id); // double queue
    tree.process_deletions();

    assert!(tree.get_node(child_id).is_none());
}

#[test]
fn free_subtree_cleans_all_descendants() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
    let child = tree.add_child(parent, Node::new("Child", "Node")).unwrap();
    let grandchild = tree.add_child(child, Node::new("Grandchild", "Node")).unwrap();

    tree.queue_free(parent);
    tree.process_deletions();

    assert!(tree.get_node(parent).is_none());
    assert!(tree.get_node(child).is_none());
    assert!(tree.get_node(grandchild).is_none());
}

#[test]
fn free_does_not_affect_siblings() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.queue_free(a);
    tree.process_deletions();

    assert!(tree.get_node(a).is_none());
    assert!(tree.get_node(b).is_some(), "sibling should survive");
}

// ---------------------------------------------------------------------------
// ObjectBase free() and use-after-free guard
// ---------------------------------------------------------------------------

#[test]
fn object_base_new_is_not_freed() {
    let base = ObjectBase::new("Node");
    assert!(!base.is_freed());
}

#[test]
fn object_base_free_marks_freed() {
    let mut base = ObjectBase::new("Node");
    base.free();
    assert!(base.is_freed());
}

#[test]
fn object_base_double_free_is_safe() {
    let mut base = ObjectBase::new("Node");
    base.free();
    base.free(); // should not panic
    assert!(base.is_freed());
}

#[test]
fn object_base_get_property_nil_after_free() {
    let mut base = ObjectBase::new("Sprite2D");
    base.set_property("texture", Variant::String("player.png".into()));
    assert_eq!(base.get_property("texture"), Variant::String("player.png".into()));

    base.free();
    assert_eq!(base.get_property("texture"), Variant::Nil);
}

#[test]
fn object_base_set_property_noop_after_free() {
    let mut base = ObjectBase::new("Node2D");
    base.free();
    let prev = base.set_property("x", Variant::Int(42));
    assert_eq!(prev, Variant::Nil);
    assert_eq!(base.get_property("x"), Variant::Nil);
}

#[test]
fn object_base_meta_cleared_on_free() {
    let mut base = ObjectBase::new("Node");
    base.set_meta("editor_hint", Variant::Bool(true));
    assert!(base.has_meta("editor_hint"));
    base.free();
    assert!(!base.has_meta("editor_hint"));
}

#[test]
fn object_base_notification_log_cleared_on_free() {
    use gdobject::notification::NOTIFICATION_READY;
    let mut base = ObjectBase::new("Node");
    base.record_notification(NOTIFICATION_READY);
    assert_eq!(base.notification_log().len(), 1);
    base.free();
    assert!(base.notification_log().is_empty());
}

#[test]
fn object_base_identity_preserved_after_free() {
    let mut base = ObjectBase::new("Camera2D");
    let id = base.id();
    let class = base.class_name().to_owned();
    base.free();
    assert_eq!(base.id(), id);
    assert_eq!(base.class_name(), class);
}

#[test]
fn object_base_has_property_false_after_free() {
    let mut base = ObjectBase::new("Node");
    base.set_property("x", Variant::Int(1));
    assert!(base.has_property("x"));
    base.free();
    assert!(!base.has_property("x"));
}

#[test]
fn object_base_property_names_empty_after_free() {
    let mut base = ObjectBase::new("Node");
    base.set_property("a", Variant::Int(1));
    base.set_property("b", Variant::Int(2));
    base.free();
    assert!(base.property_names().is_empty());
}

// ---------------------------------------------------------------------------
// WeakRef integration with Object.free()
// ---------------------------------------------------------------------------

#[test]
fn weakref_returns_none_after_object_free() {
    let mut obj = GenericObject::new("Node");
    let id = obj.get_instance_id();
    let wr = WeakRef::new(id);
    assert_eq!(wr.get_ref(), Some(id));

    obj.free();
    assert_eq!(wr.get_ref(), None);
}

#[test]
fn is_object_alive_false_after_free() {
    let mut obj = GenericObject::new("Node");
    let id = obj.get_instance_id();
    assert!(weak_ref::is_object_alive(id));

    obj.free();
    assert!(!weak_ref::is_object_alive(id));
}

// ---------------------------------------------------------------------------
// GodotObject trait dispatch
// ---------------------------------------------------------------------------

#[test]
fn generic_object_free_via_trait() {
    let mut obj = GenericObject::new("Control");
    obj.set_property("size", Variant::Int(100));

    GodotObject::free(&mut obj);
    assert!(GodotObject::is_freed(&obj));
    assert_eq!(GodotObject::get_property(&obj, "size"), Variant::Nil);
}

#[test]
fn generic_object_set_property_noop_after_trait_free() {
    let mut obj = GenericObject::new("Button");
    GodotObject::free(&mut obj);

    let prev = GodotObject::set_property(&mut obj, "text", Variant::String("Click".into()));
    assert_eq!(prev, Variant::Nil);
}
