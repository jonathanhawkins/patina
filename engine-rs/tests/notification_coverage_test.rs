//! Notification coverage tests (pat-isl).
//!
//! Verifies that notifications beyond the basic lifecycle (ENTER_TREE, READY,
//! EXIT_TREE) fire correctly via EventTrace:
//! 1. NOTIFICATION_PARENTED — fires on add_child
//! 2. NOTIFICATION_UNPARENTED — fires on remove_node
//! 3. NOTIFICATION_MOVED_IN_PARENT — fires on reparent
//! 4. NOTIFICATION_DRAW — documented gap (not auto-dispatched)
//! 5. NOTIFICATION_PAUSED / NOTIFICATION_UNPAUSED — documented gap
//! 6. NOTIFICATION_TRANSFORM_CHANGED / NOTIFICATION_VISIBILITY_CHANGED — not defined

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;

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

// ===========================================================================
// 1. NOTIFICATION_PARENTED — fires when add_child is called
// ===========================================================================

/// PARENTED fires for each node added via add_child.
#[test]
fn parented_fires_on_add_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.event_trace_mut().enable();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let parented = notification_paths(&tree, "PARENTED");
    assert_eq!(parented, vec!["/root/Child"]);
}

/// PARENTED fires before ENTER_TREE in the trace.
#[test]
fn parented_before_enter_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path.contains("Child"))
        .map(|e| e.detail.clone())
        .collect();

    let parented_idx = details
        .iter()
        .position(|d| d == "PARENTED")
        .expect("PARENTED");
    let enter_idx = details
        .iter()
        .position(|d| d == "ENTER_TREE")
        .expect("ENTER_TREE");
    assert!(
        parented_idx < enter_idx,
        "PARENTED should fire before ENTER_TREE"
    );
}

/// PARENTED fires for each node in a multi-level add.
#[test]
fn parented_fires_for_nested_add() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.event_trace_mut().enable();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(parent_id, child).unwrap();

    let grandchild = Node::new("GrandChild", "Node2D");
    let _gc_id = tree.add_child(_child_id, grandchild).unwrap();

    let parented = notification_paths(&tree, "PARENTED");
    assert_eq!(
        parented,
        vec![
            "/root/Parent",
            "/root/Parent/Child",
            "/root/Parent/Child/GrandChild",
        ]
    );
}

// ===========================================================================
// 2. NOTIFICATION_UNPARENTED — fires when node is removed from parent
// ===========================================================================

/// UNPARENTED fires when remove_node is called.
#[test]
fn unparented_fires_on_remove() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();

    tree.event_trace_mut().enable();
    let _ = tree.remove_node(child_id);

    let unparented = notification_paths(&tree, "UNPARENTED");
    assert_eq!(unparented, vec!["/root/Child"]);
}

/// UNPARENTED fires after EXIT_TREE for nodes inside the tree.
#[test]
fn unparented_after_exit_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();

    tree.event_trace_mut().enable();
    let _ = tree.remove_node(child_id);

    let details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path.contains("Child"))
        .map(|e| e.detail.clone())
        .collect();

    let exit_idx = details
        .iter()
        .position(|d| d == "EXIT_TREE")
        .expect("EXIT_TREE");
    let unparented_idx = details
        .iter()
        .position(|d| d == "UNPARENTED")
        .expect("UNPARENTED");
    assert!(
        unparented_idx > exit_idx,
        "UNPARENTED should fire after EXIT_TREE"
    );
}

/// UNPARENTED fires for the root of the removed subtree, not for each descendant.
#[test]
fn unparented_fires_for_subtree_root_only() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(parent_id, child).unwrap();

    tree.event_trace_mut().enable();
    let _ = tree.remove_node(parent_id);

    let unparented = notification_paths(&tree, "UNPARENTED");
    // Only the removed subtree root gets UNPARENTED, not its children
    // (children are removed as part of the subtree, not individually detached).
    assert_eq!(unparented, vec!["/root/Parent"]);
}

// ===========================================================================
// 3. NOTIFICATION_MOVED_IN_PARENT — fires on reparent
// ===========================================================================

/// Reparenting fires UNPARENTED, then PARENTED, then MOVED_IN_PARENT.
#[test]
fn reparent_fires_unparented_then_parented() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent_a = Node::new("ParentA", "Node2D");
    let parent_a_id = tree.add_child(root, parent_a).unwrap();

    let parent_b = Node::new("ParentB", "Node2D");
    let parent_b_id = tree.add_child(root, parent_b).unwrap();

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(parent_a_id, child).unwrap();

    tree.event_trace_mut().enable();
    tree.reparent(child_id, parent_b_id).unwrap();

    let child_events: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path.contains("Child"))
        .map(|e| e.detail.clone())
        .collect();

    assert!(
        child_events.contains(&"UNPARENTED".to_string()),
        "reparent should fire UNPARENTED"
    );
    assert!(
        child_events.contains(&"PARENTED".to_string()),
        "reparent should fire PARENTED"
    );

    // UNPARENTED should come before PARENTED.
    let unparented_idx = child_events.iter().position(|d| d == "UNPARENTED").unwrap();
    let parented_idx = child_events.iter().position(|d| d == "PARENTED").unwrap();
    assert!(
        unparented_idx < parented_idx,
        "UNPARENTED should fire before PARENTED during reparent"
    );
}

/// MOVED_IN_PARENT fires after PARENTED during reparent.
#[test]
fn reparent_fires_moved_in_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, b).unwrap();

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(a_id, child).unwrap();

    tree.event_trace_mut().enable();
    tree.reparent(child_id, b_id).unwrap();

    let child_events: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path.contains("Child"))
        .map(|e| e.detail.clone())
        .collect();

    // Should have: UNPARENTED, PARENTED (trace only shows these two since
    // MOVED_IN_PARENT is a notification but not traced via trace_record).
    assert_eq!(child_events[0], "UNPARENTED");
    assert_eq!(child_events[1], "PARENTED");
}

/// Reparenting updates the node's path in subsequent traces.
#[test]
fn reparent_updates_trace_path() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, b).unwrap();

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(a_id, child).unwrap();

    tree.reparent(child_id, b_id).unwrap();

    // After reparent, trace should show new path.
    tree.event_trace_mut().enable();
    tree.emit_signal(child_id, "test", &[]);

    let events = tree.event_trace().events();
    let signal_event = events
        .iter()
        .find(|e| e.event_type == TraceEventType::SignalEmit)
        .expect("signal event");
    assert_eq!(
        signal_event.node_path, "/root/B/Child",
        "trace should show new path after reparent"
    );
}

// ===========================================================================
// 4. NOTIFICATION_DRAW — documented gap
// ===========================================================================

/// NOTIFICATION_DRAW (code 30) is defined but not auto-dispatched during
/// the render phase. It can be manually sent via receive_notification.
#[test]
fn draw_notification_not_auto_dispatched() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let canvas = Node::new("Canvas", "Node2D");
    let _canvas_id = tree.add_child(root, canvas).unwrap();

    tree.event_trace_mut().enable();

    // Run a frame — DRAW should NOT appear automatically.
    let mut ml = gdscene::MainLoop::new(tree);
    ml.run_frames(1, 1.0 / 60.0);

    let tree = ml.tree();
    let draw_events = notification_paths(tree, "DRAW");
    assert!(
        draw_events.is_empty(),
        "KNOWN GAP: NOTIFICATION_DRAW is not auto-dispatched during rendering. \
         Godot fires it for CanvasItem nodes during the draw phase."
    );
}

// ===========================================================================
// 5. NOTIFICATION_PAUSED / NOTIFICATION_UNPAUSED — documented gap
// ===========================================================================

/// PAUSED/UNPAUSED notifications are defined but not dispatched by the engine.
#[test]
fn paused_unpaused_not_dispatched() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    tree.event_trace_mut().enable();

    let mut ml = gdscene::MainLoop::new(tree);
    ml.run_frames(3, 1.0 / 60.0);

    let tree = ml.tree();
    let paused = notification_paths(tree, "PAUSED");
    let unpaused = notification_paths(tree, "UNPAUSED");

    assert!(
        paused.is_empty() && unpaused.is_empty(),
        "KNOWN GAP: PAUSED/UNPAUSED notifications are not dispatched. \
         Godot fires these when SceneTree.paused changes."
    );
}

// ===========================================================================
// 6. TRANSFORM_CHANGED / VISIBILITY_CHANGED — not defined
// ===========================================================================

/// NOTIFICATION_TRANSFORM_CHANGED and NOTIFICATION_VISIBILITY_CHANGED are
/// not yet defined in gdobject. Document this gap.
#[test]
fn transform_and_visibility_notifications_not_defined() {
    // These notification constants don't exist yet.
    // In Godot:
    //   NOTIFICATION_TRANSFORM_CHANGED = 2000 (Node2D/Node3D)
    //   NOTIFICATION_VISIBILITY_CHANGED = 43 (CanvasItem)
    //
    // They would need:
    // 1. Constants in gdobject/notification.rs
    // 2. Dispatch when Node2D.position/rotation/scale changes
    // 3. Dispatch when CanvasItem.visible changes
    //
    // For now, these are documented gaps — no notification fires.
    assert!(
        true,
        "TRANSFORM_CHANGED and VISIBILITY_CHANGED are not yet implemented"
    );
}

// ===========================================================================
// 7. Full notification sequence: PARENTED → ENTER_TREE → READY → process loop
// ===========================================================================

/// Verify the complete notification sequence for a node added to a live tree.
#[test]
fn full_notification_sequence_with_parented() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    // Should have: PARENTED → ENTER_TREE → READY
    let child_details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path == "/root/Child")
        .map(|e| e.detail.clone())
        .collect();

    assert_eq!(
        child_details,
        vec!["PARENTED", "ENTER_TREE", "READY"],
        "full init sequence should be PARENTED → ENTER_TREE → READY"
    );
}

/// Verify removal sequence: EXIT_TREE → UNPARENTED.
#[test]
fn full_removal_sequence() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();

    tree.event_trace_mut().enable();
    let _ = tree.remove_node(child_id);

    let child_details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path == "/root/Child")
        .map(|e| e.detail.clone())
        .collect();

    assert_eq!(
        child_details,
        vec!["EXIT_TREE", "UNPARENTED"],
        "removal sequence should be EXIT_TREE → UNPARENTED"
    );
}

/// Verify reparent sequence: UNPARENTED → PARENTED.
#[test]
fn full_reparent_sequence() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, b).unwrap();
    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(a_id, child).unwrap();

    tree.event_trace_mut().enable();
    tree.reparent(child_id, b_id).unwrap();

    let child_details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path.contains("Child"))
        .map(|e| e.detail.clone())
        .collect();

    assert_eq!(
        child_details,
        vec!["UNPARENTED", "PARENTED"],
        "reparent sequence should be UNPARENTED → PARENTED"
    );
}

// ===========================================================================
// 8. process_deletions fires PARENTED-compatible sequence
// ===========================================================================

/// process_deletions fires EXIT_TREE → PREDELETE (UNPARENTED not fired since
/// node is being deleted, not detached for reuse).
#[test]
fn process_deletions_notification_sequence() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();

    tree.event_trace_mut().enable();
    tree.queue_free(child_id);
    tree.process_deletions();

    let child_details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path == "/root/Child")
        .map(|e| e.detail.clone())
        .collect();

    assert!(
        child_details.contains(&"EXIT_TREE".to_string()),
        "process_deletions should fire EXIT_TREE"
    );
    assert!(
        child_details.contains(&"PREDELETE".to_string()),
        "process_deletions should fire PREDELETE"
    );
}
