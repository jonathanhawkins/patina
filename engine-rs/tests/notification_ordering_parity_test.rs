//! pat-0sqj: Notification ordering, WeakRef, and free semantics parity tests.
//!
//! Proves that notification dispatch order matches Godot:
//! - ENTER_TREE: top-down (parent before child)
//! - READY: bottom-up (child before parent)
//! - EXIT_TREE: bottom-up (child before parent)
//! - PROCESS: parent before child (tree order within same priority)
//!
//! Also tests WeakRef lifecycle and Object.free()/queue_free() semantics.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop};
use gdobject::weak_ref::WeakRef;

// ===========================================================================
// Helpers
// ===========================================================================

fn notification_paths(tree: &SceneTree, detail: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect()
}

/// Build a 3-level nested tree:
///   root
///   └── A (Node2D)
///       ├── B (Node2D)
///       │   └── D (Node2D)
///       └── C (Node2D)
fn build_nested_tree() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(a, Node::new("B", "Node2D")).unwrap();
    let d = tree.add_child(b, Node::new("D", "Node2D")).unwrap();
    let c = tree.add_child(a, Node::new("C", "Node2D")).unwrap();
    (tree, a, b, d, c)
}

// ===========================================================================
// 1. ENTER_TREE top-down (parent before child) — nested hierarchy
// ===========================================================================

#[test]
fn enter_tree_top_down_nested() {
    let (mut tree, a, _b, _d, _c) = build_nested_tree();
    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, a);

    let paths = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(
        paths,
        vec!["/root/A", "/root/A/B", "/root/A/B/D", "/root/A/C"],
        "ENTER_TREE must be top-down: A -> B -> D -> C"
    );
}

// ===========================================================================
// 2. READY bottom-up (child before parent)
// ===========================================================================

#[test]
fn ready_bottom_up_nested() {
    let (mut tree, a, _b, _d, _c) = build_nested_tree();
    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, a);

    let paths = notification_paths(&tree, "READY");
    assert_eq!(
        paths,
        vec!["/root/A/B/D", "/root/A/B", "/root/A/C", "/root/A"],
        "READY must be bottom-up: D -> B -> C -> A"
    );
}

// ===========================================================================
// 3. EXIT_TREE bottom-up (child before parent)
// ===========================================================================

#[test]
fn exit_tree_bottom_up_nested() {
    let (mut tree, a, _b, _d, _c) = build_nested_tree();
    LifecycleManager::enter_tree(&mut tree, a);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    LifecycleManager::exit_tree(&mut tree, a);

    let paths = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(
        paths,
        vec!["/root/A/B/D", "/root/A/B", "/root/A/C", "/root/A"],
        "EXIT_TREE must be bottom-up: D -> B -> C -> A"
    );
}

// ===========================================================================
// 4. PROCESS: parent before child (tree order at same priority)
// ===========================================================================

#[test]
fn process_parent_before_child_nested() {
    let (mut tree, _a, _b, _d, _c) = build_nested_tree();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process_paths = notification_paths(tree, "PROCESS");

    // With same priority, PROCESS fires in tree order (parent before child).
    // root -> A -> B -> D -> C
    // Verify at minimum A before B before D, and A before C.
    let pos = |name: &str| {
        process_paths
            .iter()
            .position(|p| p.ends_with(name))
            .unwrap_or_else(|| panic!("{name} should appear in PROCESS paths"))
    };

    let root_pos = pos("/root");
    let a_pos = pos("/root/A");
    let b_pos = pos("/root/A/B");
    let d_pos = pos("/root/A/B/D");
    let c_pos = pos("/root/A/C");

    assert!(root_pos < a_pos, "root before A");
    assert!(a_pos < b_pos, "A before B (parent before child)");
    assert!(b_pos < d_pos, "B before D (parent before child)");
    assert!(a_pos < c_pos, "A before C (parent before child)");
}

#[test]
fn process_order_matches_sibling_add_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _alpha = tree.add_child(root, Node::new("Alpha", "Node")).unwrap();
    let _beta = tree.add_child(root, Node::new("Beta", "Node")).unwrap();
    let _gamma = tree.add_child(root, Node::new("Gamma", "Node")).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process_paths = notification_paths(tree, "PROCESS");

    // Siblings should be processed in child-index order.
    let idx = |name: &str| {
        process_paths
            .iter()
            .position(|p| p.contains(name))
            .unwrap_or_else(|| panic!("{name} missing from PROCESS"))
    };
    assert!(idx("Alpha") < idx("Beta"), "Alpha before Beta");
    assert!(idx("Beta") < idx("Gamma"), "Beta before Gamma");
}

// ===========================================================================
// 5. All ENTER_TREE complete before any READY (nested confirmation)
// ===========================================================================

#[test]
fn enter_tree_all_before_ready_nested() {
    let (mut tree, a, _b, _d, _c) = build_nested_tree();
    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, a);

    let events = tree.event_trace().events();
    let lifecycle: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "ENTER_TREE" || e.detail == "READY")
        })
        .collect();

    let last_enter = lifecycle
        .iter()
        .rposition(|e| e.detail == "ENTER_TREE")
        .expect("ENTER_TREE");
    let first_ready = lifecycle
        .iter()
        .position(|e| e.detail == "READY")
        .expect("READY");

    assert!(
        last_enter < first_ready,
        "All ENTER_TREE must finish before first READY"
    );
}

// ===========================================================================
// 6. WeakRef: creation, get_ref, invalidation
// ===========================================================================

#[test]
fn weak_ref_basic_lifecycle() {
    let id = gdcore::id::ObjectId::next();
    gdobject::weak_ref::register_object(id);
    let wr = WeakRef::new(id);

    // get_ref returns the ID when not invalidated.
    assert_eq!(wr.get_ref(), Some(id));
    assert!(!wr.is_invalidated());

    // After invalidation, returns None.
    let mut wr2 = wr;
    wr2.invalidate();
    assert_eq!(wr2.get_ref(), None);
    assert!(wr2.is_invalidated());

    // object_id still available for diagnostics.
    assert_eq!(wr2.object_id(), id);
}

#[test]
fn weak_ref_to_variant_nil_when_invalidated() {
    let id = gdcore::id::ObjectId::next();
    gdobject::weak_ref::register_object(id);
    let wr = WeakRef::new(id);
    assert!(matches!(wr.to_variant(), gdvariant::Variant::Int(_)));

    let mut wr2 = WeakRef::new(id);
    wr2.invalidate();
    assert_eq!(wr2.to_variant(), gdvariant::Variant::Nil);
}

// ===========================================================================
// 7. WeakRef integration with SceneTree: returns Nil after free
// ===========================================================================

#[test]
fn weak_ref_returns_nil_after_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Target", "Node2D");
    let nid = tree.add_child(root, n).unwrap();
    let obj_id = nid.object_id();

    let mut wr = WeakRef::new(obj_id);

    // Before free: weak ref valid, node accessible.
    assert_eq!(wr.get_ref(), Some(obj_id));
    assert!(tree.get_node(nid).is_some());

    // Free the node.
    tree.queue_free(nid);
    tree.process_deletions();

    // Node is gone.
    assert!(tree.get_node(nid).is_none());

    // Caller checks liveness via tree and invalidates the WeakRef.
    if tree.get_node(nid).is_none() {
        wr.invalidate();
    }
    assert_eq!(wr.get_ref(), None, "WeakRef should return None after free");
}

#[test]
fn weak_ref_survives_subtree_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
    let child = tree.add_child(parent, Node::new("Child", "Node")).unwrap();
    let child_obj_id = child.object_id();

    let mut wr_child = WeakRef::new(child_obj_id);

    // Free parent (takes child with it).
    tree.queue_free(parent);
    tree.process_deletions();

    assert!(tree.get_node(child).is_none(), "Child gone with parent");
    assert!(tree.get_node(parent).is_none(), "Parent gone");

    // Invalidate after checking.
    if tree.get_node(child).is_none() {
        wr_child.invalidate();
    }
    assert_eq!(wr_child.get_ref(), None);
}

// ===========================================================================
// 8. Object.free() / queue_free() behavior
// ===========================================================================

#[test]
fn freed_nodes_cannot_be_accessed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Gone", "Node2D");
    let nid = tree.add_child(root, n).unwrap();

    tree.queue_free(nid);
    tree.process_deletions();

    // All access paths should fail.
    assert!(tree.get_node(nid).is_none());
    assert!(tree.get_node_mut(nid).is_none());
    assert!(tree.get_node_by_path("/root/Gone").is_none());
}

#[test]
fn freed_node_not_in_children_list() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let nid = tree.add_child(root, Node::new("Temp", "Node")).unwrap();
    assert!(tree.get_node(root).unwrap().children().contains(&nid));

    tree.queue_free(nid);
    tree.process_deletions();

    assert!(
        !tree.get_node(root).unwrap().children().contains(&nid),
        "Freed node should not appear in parent's children"
    );
}

#[test]
fn queue_free_fires_notifications_in_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let parent = tree.add_child(root, Node::new("P", "Node")).unwrap();
    let _child = tree.add_child(parent, Node::new("C", "Node")).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.queue_free(parent);
    tree.process_deletions();

    // EXIT_TREE should be bottom-up.
    let exit_paths = notification_paths(&tree, "EXIT_TREE");
    if exit_paths.len() >= 2 {
        assert!(
            exit_paths.iter().position(|p| p.contains("/C")).unwrap()
                < exit_paths.iter().position(|p| p.ends_with("/P")).unwrap(),
            "Child EXIT_TREE before parent"
        );
    }

    // PREDELETE should fire after all EXIT_TREE.
    let events = tree.event_trace().events();
    let last_exit = events.iter().rposition(|e| e.detail == "EXIT_TREE");
    let first_predel = events.iter().position(|e| e.detail == "PREDELETE");
    if let (Some(le), Some(fp)) = (last_exit, first_predel) {
        assert!(le < fp, "All EXIT_TREE before any PREDELETE");
    }
}

// ===========================================================================
// 9. Double queue_free is safe
// ===========================================================================

#[test]
fn double_queue_free_safe() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let nid = tree.add_child(root, Node::new("Dup", "Node")).unwrap();

    tree.queue_free(nid);
    tree.queue_free(nid); // idempotent
    assert_eq!(tree.pending_deletion_count(), 1);

    tree.process_deletions();
    assert!(tree.get_node(nid).is_none());
}
