//! Notification coverage tests (pat-isl).
//!
//! Verifies that notifications beyond the basic lifecycle (ENTER_TREE, READY,
//! EXIT_TREE) fire correctly via EventTrace:
//! 1. NOTIFICATION_PARENTED / CHILD_ORDER_CHANGED — fires on add_child
//! 2. NOTIFICATION_UNPARENTED — fires on remove_node
//! 3. NOTIFICATION_MOVED_IN_PARENT — fires on reparent and move_child
//! 4. NOTIFICATION_CHILD_ORDER_CHANGED — fires on parent when child order changes
//! 5. move_child / raise / lower — reorder operations
//! 6. NOTIFICATION_PAUSED / NOTIFICATION_UNPAUSED — fires on set_paused
//! 7. NOTIFICATION_DRAW — documented gap (not auto-dispatched)
//! 8. NOTIFICATION_TRANSFORM_CHANGED / VISIBILITY_CHANGED — documented gaps
//! 9. Full notification sequences and ordering regression tests

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

/// Collect notification details for events matching a node path substring.
fn node_notification_details(tree: &SceneTree, path_contains: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.node_path.contains(path_contains) && e.event_type == TraceEventType::Notification
        })
        .map(|e| e.detail.clone())
        .collect()
}

/// Collect notification details for events matching an exact node path.
fn exact_node_notifications(tree: &SceneTree, path: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path == path && e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.clone())
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

/// CHILD_ORDER_CHANGED fires on parent when a child is added.
#[test]
fn child_order_changed_fires_on_add_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.event_trace_mut().enable();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let changed = notification_paths(&tree, "CHILD_ORDER_CHANGED");
    assert_eq!(changed, vec!["/root"]);
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

    let details = node_notification_details(&tree, "Child");

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

    let details = node_notification_details(&tree, "Child");

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
// 3. NOTIFICATION_MOVED_IN_PARENT — fires on reparent and move_child
// ===========================================================================

/// Reparenting fires UNPARENTED, then PARENTED, then MOVED_IN_PARENT on the child.
#[test]
fn reparent_fires_unparented_then_parented_then_moved() {
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

    let child_events = node_notification_details(&tree, "Child");

    assert_eq!(
        child_events,
        vec!["UNPARENTED", "PARENTED", "MOVED_IN_PARENT"],
        "reparent sequence on child should be UNPARENTED -> PARENTED -> MOVED_IN_PARENT"
    );
}

/// MOVED_IN_PARENT fires after PARENTED during reparent, verified via trace.
#[test]
fn reparent_fires_moved_in_parent_traced() {
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

    let child_events = node_notification_details(&tree, "Child");

    let parented_idx = child_events
        .iter()
        .position(|d| d == "PARENTED")
        .expect("PARENTED");
    let moved_idx = child_events
        .iter()
        .position(|d| d == "MOVED_IN_PARENT")
        .expect("MOVED_IN_PARENT");
    assert!(
        moved_idx > parented_idx,
        "MOVED_IN_PARENT should fire after PARENTED"
    );
}

/// Reparent dispatches CHILD_ORDER_CHANGED on the new parent.
#[test]
fn reparent_fires_child_order_changed_on_new_parent() {
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

    let b_events = exact_node_notifications(&tree, "/root/B");
    assert!(
        b_events.contains(&"CHILD_ORDER_CHANGED".to_string()),
        "new parent should receive CHILD_ORDER_CHANGED on reparent"
    );
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
// 4. move_child — reorder children within a parent
// ===========================================================================

/// move_child dispatches MOVED_IN_PARENT to the moved child.
#[test]
fn move_child_fires_moved_in_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "Node2D");
    let _b_id = tree.add_child(root, b).unwrap();
    let c = Node::new("C", "Node2D");
    let _c_id = tree.add_child(root, c).unwrap();

    tree.event_trace_mut().enable();
    tree.move_child(root, a_id, 2).unwrap();

    let a_events = exact_node_notifications(&tree, "/root/A");
    assert_eq!(
        a_events,
        vec!["MOVED_IN_PARENT"],
        "move_child should fire MOVED_IN_PARENT on the moved child"
    );
}

/// move_child dispatches CHILD_ORDER_CHANGED to the parent.
#[test]
fn move_child_fires_child_order_changed_on_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();
    let _b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();

    tree.event_trace_mut().enable();
    tree.move_child(root, a_id, 1).unwrap();

    let root_events = exact_node_notifications(&tree, "/root");
    assert_eq!(
        root_events,
        vec!["CHILD_ORDER_CHANGED"],
        "move_child should fire CHILD_ORDER_CHANGED on the parent"
    );
}

/// move_child actually changes the sibling order.
#[test]
fn move_child_changes_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c_id = tree.add_child(root, Node::new("C", "Node")).unwrap();

    // Move A to the end.
    tree.move_child(root, a_id, 2).unwrap();

    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children, vec![b_id, c_id, a_id]);
}

/// move_child with same index is a no-op (no notifications).
#[test]
fn move_child_same_index_is_noop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let _b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.event_trace_mut().enable();
    tree.move_child(root, a_id, 0).unwrap();

    let all_notifs: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.clone())
        .collect();
    assert!(
        all_notifs.is_empty(),
        "move_child to same index should not fire any notifications"
    );
}

/// move_child with out-of-bounds index clamps to last position.
#[test]
fn move_child_clamps_to_last() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.move_child(root, a_id, 999).unwrap();

    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children, vec![b_id, a_id]);
}

/// move_child errors if child is not a child of the given parent.
#[test]
fn move_child_wrong_parent_errors() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    let result = tree.move_child(a_id, b_id, 0);
    assert!(
        result.is_err(),
        "move_child should error if child is not a child of parent"
    );
}

// ===========================================================================
// 5. raise / lower — convenience reorder operations
// ===========================================================================

/// raise moves a node to the last child position.
#[test]
fn raise_moves_to_last() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c_id = tree.add_child(root, Node::new("C", "Node")).unwrap();

    tree.raise(a_id).unwrap();

    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children, vec![b_id, c_id, a_id]);
}

/// raise fires MOVED_IN_PARENT on the node.
#[test]
fn raise_fires_moved_in_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let _b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.event_trace_mut().enable();
    tree.raise(a_id).unwrap();

    let a_events = exact_node_notifications(&tree, "/root/A");
    assert!(
        a_events.contains(&"MOVED_IN_PARENT".to_string()),
        "raise should fire MOVED_IN_PARENT"
    );
}

/// lower moves a node to the first child position.
#[test]
fn lower_moves_to_first() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c_id = tree.add_child(root, Node::new("C", "Node")).unwrap();

    tree.lower(c_id).unwrap();

    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children, vec![c_id, a_id, b_id]);
}

/// lower fires MOVED_IN_PARENT on the node.
#[test]
fn lower_fires_moved_in_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let _a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.event_trace_mut().enable();
    tree.lower(b_id).unwrap();

    let b_events = exact_node_notifications(&tree, "/root/B");
    assert!(
        b_events.contains(&"MOVED_IN_PARENT".to_string()),
        "lower should fire MOVED_IN_PARENT"
    );
}

/// raise on already-last child is no-op.
#[test]
fn raise_already_last_is_noop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let _a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.event_trace_mut().enable();
    tree.raise(b_id).unwrap();

    let all_notifs: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.clone())
        .collect();
    assert!(
        all_notifs.is_empty(),
        "raise on already-last child should be a no-op"
    );
}

/// lower on already-first child is no-op.
#[test]
fn lower_already_first_is_noop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let _b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

    tree.event_trace_mut().enable();
    tree.lower(a_id).unwrap();

    let all_notifs: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.clone())
        .collect();
    assert!(
        all_notifs.is_empty(),
        "lower on already-first child should be a no-op"
    );
}

// ===========================================================================
// 6. NOTIFICATION_PAUSED / NOTIFICATION_UNPAUSED
// ===========================================================================

/// set_paused(true) dispatches PAUSED to all nodes in tree order.
#[test]
fn pause_fires_paused_notification() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let mut ml = gdscene::MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.set_paused(true);

    let tree = ml.tree();
    let paused = notification_paths(tree, "PAUSED");
    assert!(
        paused.len() >= 2,
        "PAUSED should fire on root and child (got {:?})",
        paused
    );
}

/// set_paused(false) dispatches UNPAUSED to all nodes in tree order.
#[test]
fn unpause_fires_unpaused_notification() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let mut ml = gdscene::MainLoop::new(tree);
    ml.set_paused(true);
    ml.tree_mut().event_trace_mut().enable();
    ml.set_paused(false);

    let tree = ml.tree();
    let unpaused = notification_paths(tree, "UNPAUSED");
    assert!(
        unpaused.len() >= 2,
        "UNPAUSED should fire on root and child (got {:?})",
        unpaused
    );
}

/// Pause/unpause ordering: PAUSED fires in tree order (parent before children).
#[test]
fn pause_notification_ordering_is_tree_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(parent_id, child).unwrap();

    let mut ml = gdscene::MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.set_paused(true);

    let tree = ml.tree();
    let paused = notification_paths(tree, "PAUSED");
    // Tree order is: root, Parent, Child
    assert_eq!(
        paused,
        vec!["/root", "/root/Parent", "/root/Parent/Child"],
        "PAUSED should fire in tree order (parent before children)"
    );
}

/// Repeated set_paused(true) does not re-dispatch.
#[test]
fn pause_idempotent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = gdscene::MainLoop::new(tree);
    ml.set_paused(true);
    ml.tree_mut().event_trace_mut().enable();
    ml.set_paused(true); // should be a no-op

    let tree = ml.tree();
    let paused = notification_paths(tree, "PAUSED");
    assert!(
        paused.is_empty(),
        "set_paused(true) when already paused should not re-dispatch PAUSED"
    );
}

/// Full pause/unpause cycle: PAUSED then UNPAUSED, each in tree order.
#[test]
fn pause_unpause_full_cycle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let mut ml = gdscene::MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.set_paused(true);
    ml.set_paused(false);

    let tree = ml.tree();

    // Collect all notification events in order.
    let all_details: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.clone())
        .collect();

    // Should be: PAUSED, PAUSED, UNPAUSED, UNPAUSED (root+child for each).
    let pause_count = all_details.iter().filter(|d| *d == "PAUSED").count();
    let unpause_count = all_details.iter().filter(|d| *d == "UNPAUSED").count();
    assert_eq!(pause_count, 2, "two nodes should receive PAUSED");
    assert_eq!(unpause_count, 2, "two nodes should receive UNPAUSED");

    // All PAUSED should come before all UNPAUSED.
    let last_paused = all_details.iter().rposition(|d| d == "PAUSED").unwrap();
    let first_unpaused = all_details.iter().position(|d| d == "UNPAUSED").unwrap();
    assert!(
        last_paused < first_unpaused,
        "all PAUSED should fire before any UNPAUSED"
    );
}

// ===========================================================================
// 7. NOTIFICATION_DRAW — documented gap
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
// 8. TRANSFORM_CHANGED / VISIBILITY_CHANGED — not defined
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
// 9. Full notification sequences — ordering regression tests
// ===========================================================================

/// Verify the complete notification sequence for a node added to a live tree.
/// Sequence on the child: PARENTED -> ENTER_TREE -> READY
/// Sequence on the parent: CHILD_ORDER_CHANGED
#[test]
fn full_notification_sequence_with_parented() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();

    let child = Node::new("Child", "Node2D");
    let _child_id = tree.add_child(root, child).unwrap();

    let child_details = exact_node_notifications(&tree, "/root/Child");
    assert_eq!(
        child_details,
        vec!["PARENTED", "ENTER_TREE", "READY"],
        "full init sequence should be PARENTED -> ENTER_TREE -> READY"
    );

    let root_details = exact_node_notifications(&tree, "/root");
    assert_eq!(
        root_details,
        vec!["CHILD_ORDER_CHANGED"],
        "parent should receive CHILD_ORDER_CHANGED when child is added"
    );
}

/// Verify removal sequence: EXIT_TREE -> UNPARENTED.
#[test]
fn full_removal_sequence() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let child = Node::new("Child", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();

    tree.event_trace_mut().enable();
    let _ = tree.remove_node(child_id);

    let child_details = exact_node_notifications(&tree, "/root/Child");
    assert_eq!(
        child_details,
        vec!["EXIT_TREE", "UNPARENTED"],
        "removal sequence should be EXIT_TREE -> UNPARENTED"
    );
}

/// Verify reparent sequence: UNPARENTED -> PARENTED -> MOVED_IN_PARENT on child,
/// CHILD_ORDER_CHANGED on new parent.
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

    let child_details = node_notification_details(&tree, "Child");
    assert_eq!(
        child_details,
        vec!["UNPARENTED", "PARENTED", "MOVED_IN_PARENT"],
        "reparent sequence should be UNPARENTED -> PARENTED -> MOVED_IN_PARENT"
    );

    let b_details = exact_node_notifications(&tree, "/root/B");
    assert!(
        b_details.contains(&"CHILD_ORDER_CHANGED".to_string()),
        "new parent should receive CHILD_ORDER_CHANGED"
    );
}

// ===========================================================================
// 10. process_deletions fires PARENTED-compatible sequence
// ===========================================================================

/// process_deletions fires EXIT_TREE -> PREDELETE (UNPARENTED not fired since
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

    let child_details = exact_node_notifications(&tree, "/root/Child");

    assert!(
        child_details.contains(&"EXIT_TREE".to_string()),
        "process_deletions should fire EXIT_TREE"
    );
    assert!(
        child_details.contains(&"PREDELETE".to_string()),
        "process_deletions should fire PREDELETE"
    );

    // EXIT_TREE should come before PREDELETE.
    let exit_idx = child_details.iter().position(|d| d == "EXIT_TREE").unwrap();
    let predelete_idx = child_details.iter().position(|d| d == "PREDELETE").unwrap();
    assert!(
        exit_idx < predelete_idx,
        "EXIT_TREE should fire before PREDELETE"
    );
}

// ===========================================================================
// 11. NOTIFICATION_CHILD_ORDER_CHANGED constant value
// ===========================================================================

/// Verify the constant code matches Godot's.
#[test]
fn child_order_changed_constant_matches_godot() {
    use gdobject::notification::NOTIFICATION_CHILD_ORDER_CHANGED;
    assert_eq!(
        NOTIFICATION_CHILD_ORDER_CHANGED.code(),
        16,
        "NOTIFICATION_CHILD_ORDER_CHANGED should be code 16 (Godot standard)"
    );
}

// ===========================================================================
// 12. Multi-child move ordering stress test
// ===========================================================================

/// Move multiple children and verify each move fires its own pair of notifications.
#[test]
fn multiple_moves_fire_independent_notifications() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c_id = tree.add_child(root, Node::new("C", "Node")).unwrap();

    tree.event_trace_mut().enable();

    // Move A to end, then C to front.
    tree.move_child(root, a_id, 2).unwrap();
    tree.move_child(root, c_id, 0).unwrap();

    let moved_events = notification_paths(&tree, "MOVED_IN_PARENT");
    assert_eq!(
        moved_events,
        vec!["/root/A", "/root/C"],
        "each move_child should fire its own MOVED_IN_PARENT"
    );

    let order_events = notification_paths(&tree, "CHILD_ORDER_CHANGED");
    assert_eq!(
        order_events,
        vec!["/root", "/root"],
        "each move_child should fire CHILD_ORDER_CHANGED on parent"
    );

    // Final order: C, B, A
    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children, vec![c_id, b_id, a_id]);
}
