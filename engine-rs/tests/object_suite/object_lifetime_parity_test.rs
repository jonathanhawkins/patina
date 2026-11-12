//! pat-0sqj: Object lifetime and notification semantics parity tests.
//!
//! Covers:
//! - Notification ordering: EXIT_TREE fires before PREDELETE on queue_free
//! - PREDELETE fires bottom-up (children before parents)
//! - WeakRef: basic lifecycle, invalidation after free
//! - Use-after-free guard: NodeId lookups return None after deletion
//! - Double queue_free is idempotent

use gdobject::weak_ref::WeakRef;
use gdscene::lifecycle::LifecycleManager;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;

// ── Notification ordering ───────────────────────────────────────────

#[test]
fn queue_free_fires_exit_tree_before_predelete() {
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();

    let n = Node::new("Victim", "Node2D");
    let nid = tree.add_child(root, n).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().clear();

    tree.queue_free(nid);
    tree.process_deletions();

    let events = tree.event_trace().events();
    let exit_idx = events.iter().position(|e| {
        e.node_path.contains("Victim")
            && e.event_type == TraceEventType::Notification
            && e.detail == "EXIT_TREE"
    });
    let predel_idx = events.iter().position(|e| {
        e.node_path.contains("Victim")
            && e.event_type == TraceEventType::Notification
            && e.detail == "PREDELETE"
    });

    assert!(exit_idx.is_some(), "EXIT_TREE must fire on queue_free");
    assert!(predel_idx.is_some(), "PREDELETE must fire on queue_free");
    assert!(
        exit_idx.unwrap() < predel_idx.unwrap(),
        "EXIT_TREE ({}) must fire before PREDELETE ({})",
        exit_idx.unwrap(),
        predel_idx.unwrap()
    );
}

#[test]
fn predelete_fires_bottom_up_on_subtree() {
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child = Node::new("Child", "Node");
    let _child_id = tree.add_child(parent_id, child).unwrap();
    let grandchild = Node::new("Grandchild", "Node");
    let _gc_id = tree.add_child(_child_id, grandchild).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().clear();

    tree.queue_free(parent_id);
    tree.process_deletions();

    let predel_events: Vec<&str> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification && e.detail == "PREDELETE")
        .map(|e| e.node_path.as_str())
        .collect();

    assert_eq!(predel_events.len(), 3, "PREDELETE fires for entire subtree");
    // Bottom-up: grandchild, child, parent
    assert!(
        predel_events[0].contains("Grandchild"),
        "Grandchild PREDELETE first, got: {}",
        predel_events[0]
    );
    assert!(
        predel_events[1].contains("Child"),
        "Child PREDELETE second, got: {}",
        predel_events[1]
    );
    assert!(
        predel_events[2].contains("Parent"),
        "Parent PREDELETE last, got: {}",
        predel_events[2]
    );
}

#[test]
fn exit_tree_fires_bottom_up_before_predelete_subtree() {
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();

    let a = Node::new("A", "Node");
    let a_id = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "Node");
    let _b_id = tree.add_child(a_id, b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().clear();

    tree.queue_free(a_id);
    tree.process_deletions();

    let events = tree.event_trace().events();
    // All EXIT_TREE must come before all PREDELETE
    let last_exit = events
        .iter()
        .rposition(|e| e.detail == "EXIT_TREE")
        .expect("EXIT_TREE must fire");
    let first_predel = events
        .iter()
        .position(|e| e.detail == "PREDELETE")
        .expect("PREDELETE must fire");

    assert!(
        last_exit < first_predel,
        "All EXIT_TREE ({}) must complete before first PREDELETE ({})",
        last_exit,
        first_predel
    );
}

// ── Use-after-free guard paths ──────────────────────────────────────

#[test]
fn node_lookup_returns_none_after_queue_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Ephemeral", "Node2D");
    let nid = tree.add_child(root, n).unwrap();
    assert!(tree.get_node(nid).is_some());

    tree.queue_free(nid);
    tree.process_deletions();

    assert!(
        tree.get_node(nid).is_none(),
        "Freed node must not be accessible"
    );
    assert!(
        tree.get_node_by_path("/root/Ephemeral").is_none(),
        "Path lookup must fail after free"
    );
}

#[test]
fn double_queue_free_is_idempotent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Double", "Node");
    let nid = tree.add_child(root, n).unwrap();

    tree.queue_free(nid);
    tree.queue_free(nid); // should not panic or double-add
    assert_eq!(
        tree.pending_deletion_count(),
        1,
        "No duplicate pending entries"
    );

    tree.process_deletions();
    assert!(tree.get_node(nid).is_none());
}

#[test]
fn queue_free_during_iteration_is_safe() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ids = Vec::new();
    for i in 0..5 {
        let n = Node::new(&format!("N{i}"), "Node");
        ids.push(tree.add_child(root, n).unwrap());
    }

    // Queue all for deletion (simulates _process marking nodes for free).
    for &id in &ids {
        tree.queue_free(id);
    }

    // Nodes still accessible before process_deletions.
    for &id in &ids {
        assert!(tree.get_node(id).is_some());
    }

    tree.process_deletions();

    // All gone.
    for &id in &ids {
        assert!(tree.get_node(id).is_none());
    }
    assert_eq!(tree.node_count(), 1); // Only root.
}

// ── WeakRef integration ─────────────────────────────────────────────

#[test]
fn weak_ref_tracks_object_identity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Target", "Node2D");
    let nid = tree.add_child(root, n).unwrap();
    let obj_id = nid.object_id();

    let wr = WeakRef::new(obj_id);
    assert_eq!(wr.get_ref(), Some(obj_id));
}

#[test]
fn weak_ref_detects_freed_object() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Target", "Node2D");
    let nid = tree.add_child(root, n).unwrap();
    let obj_id = nid.object_id();

    let mut wr = WeakRef::new(obj_id);
    assert_eq!(wr.get_ref(), Some(obj_id));

    // Free the node.
    tree.queue_free(nid);
    tree.process_deletions();

    // WeakRef auto-invalidates when the object is freed (checks alive-objects registry).
    assert_eq!(wr.get_ref(), None);
    // The node is gone from the tree too.
    assert!(tree.get_node(nid).is_none());
}

#[test]
fn weak_ref_to_variant_round_trip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("V", "Node");
    let nid = tree.add_child(root, n).unwrap();
    let obj_id = nid.object_id();

    let wr = WeakRef::new(obj_id);
    let v = wr.to_variant();
    assert!(matches!(v, gdvariant::Variant::Int(_)));

    let mut wr2 = WeakRef::new(obj_id);
    wr2.invalidate();
    assert_eq!(wr2.to_variant(), gdvariant::Variant::Nil);
}

// ── Notification log verification ───────────────────────────────────

#[test]
fn freed_node_received_predelete_in_notification_log() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n = Node::new("Logged", "Node");
    let nid = tree.add_child(root, n).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);

    // Check the node received lifecycle notifications.
    let log_before: Vec<_> = tree.get_node(nid).unwrap().notification_log().to_vec();
    assert!(
        log_before
            .iter()
            .any(|n| n.code() == gdobject::NOTIFICATION_ENTER_TREE.code()),
        "Node should have received ENTER_TREE"
    );

    // We can't read the log after deletion (node is gone), but we can
    // verify the trace captured PREDELETE.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.queue_free(nid);
    tree.process_deletions();

    let got_predelete = tree
        .event_trace()
        .events()
        .iter()
        .any(|e| e.node_path.contains("Logged") && e.detail == "PREDELETE");
    assert!(got_predelete, "PREDELETE must be traced for freed node");
}
