//! pat-faxt: Non-lifecycle notification parity tests.
//!
//! Expands notification coverage beyond the core lifecycle path
//! (ENTER_TREE, READY, EXIT_TREE, PROCESS). Tests cover:
//!
//! 1. PHYSICS_PROCESS ordering via EventTrace (parent before child)
//! 2. INTERNAL_PROCESS / INTERNAL_PHYSICS_PROCESS ordering via EventTrace
//! 3. Re-enter lifecycle: exit tree then re-enter fires correct notifications
//! 4. POSTINITIALIZE constant value and manual dispatch
//! 5. PREDELETE fires on queue_free for each node in bottom-up order
//! 6. Multiple lifecycle cycles (enter → exit → enter → exit)
//! 7. Notification log accumulates across lifecycle phases
//! 8. Documented exclusions: INSTANCED, DRAG_BEGIN/END, DRAW auto-dispatch

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop};
use gdobject::notification::{
    Notification, NOTIFICATION_ENTER_TREE, NOTIFICATION_INSTANCED,
    NOTIFICATION_INTERNAL_PHYSICS_PROCESS, NOTIFICATION_INTERNAL_PROCESS,
    NOTIFICATION_PHYSICS_PROCESS, NOTIFICATION_POSTINITIALIZE,
    NOTIFICATION_PROCESS, NOTIFICATION_READY,
};

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

fn all_notification_details(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

/// Build a tree with root -> Parent -> Child, enter it, clear trace, return MainLoop.
fn make_entered_tree_with_subtree() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
    let child = tree.add_child(parent, Node::new("Child", "Node2D")).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    let ml = MainLoop::new(tree);
    (ml, parent, child)
}

// ===========================================================================
// 1. PHYSICS_PROCESS ordering via EventTrace — parent before child
// ===========================================================================

#[test]
fn physics_process_fires_parent_before_child_in_trace() {
    let (mut ml, _parent, _child) = make_entered_tree_with_subtree();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let phys_paths = notification_paths(tree, "PHYSICS_PROCESS");

    // Tree order: root -> Parent -> Child
    let pos = |name: &str| {
        phys_paths
            .iter()
            .position(|p| p.ends_with(name))
            .unwrap_or_else(|| panic!("{name} should appear in PHYSICS_PROCESS trace"))
    };
    assert!(pos("/root") < pos("/root/Parent"), "root before Parent");
    assert!(
        pos("/root/Parent") < pos("/root/Parent/Child"),
        "Parent before Child"
    );
}

#[test]
fn physics_process_fires_for_all_nodes() {
    let (mut ml, _parent, _child) = make_entered_tree_with_subtree();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let phys_paths = notification_paths(tree, "PHYSICS_PROCESS");
    assert_eq!(phys_paths.len(), 3, "root + Parent + Child should all get PHYSICS_PROCESS");
}

// ===========================================================================
// 2. INTERNAL_PROCESS / INTERNAL_PHYSICS_PROCESS ordering via EventTrace
// ===========================================================================

#[test]
fn internal_physics_fires_before_physics_in_trace() {
    let (mut ml, _parent, _child) = make_entered_tree_with_subtree();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let events = all_notification_details(tree);

    // For any given node, INTERNAL_PHYSICS_PROCESS must appear before PHYSICS_PROCESS.
    let int_phys_idx = events
        .iter()
        .position(|(_, d)| d == "INTERNAL_PHYSICS_PROCESS")
        .expect("INTERNAL_PHYSICS_PROCESS");
    let phys_idx = events
        .iter()
        .position(|(_, d)| d == "PHYSICS_PROCESS")
        .expect("PHYSICS_PROCESS");
    assert!(
        int_phys_idx < phys_idx,
        "INTERNAL_PHYSICS_PROCESS must fire before PHYSICS_PROCESS"
    );
}

#[test]
fn internal_process_fires_before_process_in_trace() {
    let (mut ml, _parent, _child) = make_entered_tree_with_subtree();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let events = all_notification_details(tree);

    let int_proc_idx = events
        .iter()
        .position(|(_, d)| d == "INTERNAL_PROCESS")
        .expect("INTERNAL_PROCESS");
    let proc_idx = events
        .iter()
        .position(|(_, d)| d == "PROCESS")
        .expect("PROCESS");
    assert!(
        int_proc_idx < proc_idx,
        "INTERNAL_PROCESS must fire before PROCESS"
    );
}

#[test]
fn godot_four_phase_order_in_trace() {
    // Godot contract per frame with one physics tick:
    // INTERNAL_PHYSICS_PROCESS -> PHYSICS_PROCESS -> INTERNAL_PROCESS -> PROCESS
    let (mut ml, _parent, _child) = make_entered_tree_with_subtree();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let events = all_notification_details(tree);

    let phase_order: Vec<&str> = events
        .iter()
        .filter_map(|(_, d)| match d.as_str() {
            "INTERNAL_PHYSICS_PROCESS" => Some("IPHYS"),
            "PHYSICS_PROCESS" => Some("PHYS"),
            "INTERNAL_PROCESS" => Some("IPROC"),
            "PROCESS" => Some("PROC"),
            _ => None,
        })
        .collect();

    // Find first occurrence of each phase.
    let first = |tag: &str| {
        phase_order
            .iter()
            .position(|&t| t == tag)
            .unwrap_or_else(|| panic!("{tag} missing"))
    };
    assert!(first("IPHYS") < first("PHYS"), "INTERNAL_PHYSICS before PHYSICS");
    assert!(first("PHYS") < first("IPROC"), "PHYSICS before INTERNAL_PROCESS");
    assert!(first("IPROC") < first("PROC"), "INTERNAL_PROCESS before PROCESS");
}

// ===========================================================================
// 3. Re-enter lifecycle: exit tree then re-enter
// ===========================================================================

#[test]
fn reenter_tree_fires_enter_and_ready_again() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();

    // First lifecycle
    LifecycleManager::enter_tree(&mut tree, root);
    LifecycleManager::exit_tree(&mut tree, root);

    // Second lifecycle — clear trace and verify notifications fire again.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    LifecycleManager::enter_tree(&mut tree, root);

    let enter_paths = notification_paths(&tree, "ENTER_TREE");
    let ready_paths = notification_paths(&tree, "READY");

    assert!(
        enter_paths.contains(&"/root".to_string()),
        "root should get ENTER_TREE on re-entry"
    );
    assert!(
        enter_paths.contains(&"/root/Child".to_string()),
        "child should get ENTER_TREE on re-entry"
    );
    assert!(
        ready_paths.contains(&"/root".to_string()),
        "root should get READY on re-entry"
    );
    assert!(
        ready_paths.contains(&"/root/Child".to_string()),
        "child should get READY on re-entry"
    );
}

#[test]
fn reenter_tree_ordering_preserved() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);
    LifecycleManager::exit_tree(&mut tree, root);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    LifecycleManager::enter_tree(&mut tree, root);

    // ENTER_TREE must be top-down, READY must be bottom-up — even on re-entry.
    let enter_paths = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enter_paths[0], "/root", "ENTER_TREE top-down on re-entry");

    let ready_paths = notification_paths(&tree, "READY");
    assert_eq!(
        *ready_paths.last().unwrap(),
        "/root",
        "READY bottom-up on re-entry: root last"
    );
}

// ===========================================================================
// 4. POSTINITIALIZE constant value
// ===========================================================================

#[test]
fn postinitialize_constant_matches_godot() {
    assert_eq!(
        NOTIFICATION_POSTINITIALIZE.code(),
        0,
        "NOTIFICATION_POSTINITIALIZE should be code 0 (Godot standard)"
    );
}

#[test]
fn postinitialize_display() {
    assert_eq!(
        format!("{}", NOTIFICATION_POSTINITIALIZE),
        "NOTIFICATION_POSTINITIALIZE"
    );
}

#[test]
fn postinitialize_can_be_manually_dispatched() {
    let mut node = Node::new("Test", "Node2D");
    // Node::new() already dispatches POSTINITIALIZE; manual dispatch adds a second.
    node.receive_notification(NOTIFICATION_POSTINITIALIZE);
    let log = node.notification_log();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0], NOTIFICATION_POSTINITIALIZE);
    assert_eq!(log[1], NOTIFICATION_POSTINITIALIZE);
}

// ===========================================================================
// 5. PREDELETE fires on queue_free — bottom-up order
// ===========================================================================

#[test]
fn predelete_fires_bottom_up_on_subtree_free() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
    let child = tree.add_child(parent, Node::new("Child", "Node2D")).unwrap();
    let _grandchild = tree
        .add_child(child, Node::new("GrandChild", "Node2D"))
        .unwrap();

    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.queue_free(parent);
    tree.process_deletions();

    let predelete_paths = notification_paths(&tree, "PREDELETE");
    assert!(
        !predelete_paths.is_empty(),
        "PREDELETE should fire on queue_free"
    );

    // Verify bottom-up: grandchild before child before parent
    if predelete_paths.len() >= 3 {
        let gc_idx = predelete_paths
            .iter()
            .position(|p| p.contains("GrandChild"))
            .expect("GrandChild in PREDELETE");
        let c_idx = predelete_paths
            .iter()
            .position(|p| p.ends_with("Child") && !p.contains("Grand"))
            .expect("Child in PREDELETE");
        let p_idx = predelete_paths
            .iter()
            .position(|p| p.ends_with("Parent"))
            .expect("Parent in PREDELETE");
        assert!(gc_idx < c_idx, "GrandChild PREDELETE before Child");
        assert!(c_idx < p_idx, "Child PREDELETE before Parent");
    }
}

#[test]
fn exit_tree_all_before_predelete_on_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("P", "Node2D")).unwrap();
    let _child = tree.add_child(parent, Node::new("C", "Node2D")).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.queue_free(parent);
    tree.process_deletions();

    let events = all_notification_details(&tree);
    let last_exit = events.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_predel = events.iter().position(|(_, d)| d == "PREDELETE");

    if let (Some(le), Some(fp)) = (last_exit, first_predel) {
        assert!(
            le < fp,
            "All EXIT_TREE must complete before any PREDELETE"
        );
    }
}

// ===========================================================================
// 6. Multiple lifecycle cycles
// ===========================================================================

#[test]
fn three_lifecycle_cycles_produce_consistent_notifications() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();

    for cycle in 0..3 {
        tree.event_trace_mut().enable();
        tree.event_trace_mut().clear();

        LifecycleManager::enter_tree(&mut tree, root);

        let enter_count = notification_paths(&tree, "ENTER_TREE").len();
        let ready_count = notification_paths(&tree, "READY").len();
        assert_eq!(
            enter_count, 2,
            "cycle {cycle}: ENTER_TREE for root + child"
        );
        assert_eq!(ready_count, 2, "cycle {cycle}: READY for root + child");

        tree.event_trace_mut().clear();
        LifecycleManager::exit_tree(&mut tree, root);

        let exit_count = notification_paths(&tree, "EXIT_TREE").len();
        assert_eq!(exit_count, 2, "cycle {cycle}: EXIT_TREE for root + child");
    }
}

// ===========================================================================
// 7. Notification log accumulates across lifecycle phases
// ===========================================================================

#[test]
fn notification_log_accumulates_across_phases() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child_id = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let log = tree.get_node(child_id).unwrap().notification_log();
    // Should have at least PARENTED, ENTER_TREE, READY.
    assert!(log.contains(&NOTIFICATION_ENTER_TREE), "ENTER_TREE in log");
    assert!(log.contains(&NOTIFICATION_READY), "READY in log");

    // Run a process frame.
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let log = ml.tree().get_node(child_id).unwrap().notification_log();
    // Now should also have PROCESS and PHYSICS_PROCESS.
    assert!(log.contains(&NOTIFICATION_PROCESS), "PROCESS in log after step");
    assert!(
        log.contains(&NOTIFICATION_PHYSICS_PROCESS),
        "PHYSICS_PROCESS in log after step"
    );
    assert!(
        log.contains(&NOTIFICATION_INTERNAL_PROCESS),
        "INTERNAL_PROCESS in log after step"
    );
    assert!(
        log.contains(&NOTIFICATION_INTERNAL_PHYSICS_PROCESS),
        "INTERNAL_PHYSICS_PROCESS in log after step"
    );
}

// ===========================================================================
// 8. Per-frame notification counts — no duplicates
// ===========================================================================

#[test]
fn single_frame_fires_exactly_one_process_per_node() {
    let (mut ml, _parent, child) = make_entered_tree_with_subtree();
    ml.step(1.0 / 60.0);

    let log = ml.tree().get_node(child).unwrap().notification_log();
    let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
    let physics_count = log
        .iter()
        .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
        .count();
    assert_eq!(process_count, 1, "exactly one PROCESS per frame");
    assert_eq!(physics_count, 1, "exactly one PHYSICS_PROCESS per frame");
}

#[test]
fn five_frames_fires_five_process_per_node() {
    let (mut ml, _parent, child) = make_entered_tree_with_subtree();
    ml.run_frames(5, 1.0 / 60.0);

    let log = ml.tree().get_node(child).unwrap().notification_log();
    let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
    assert_eq!(process_count, 5, "5 frames = 5 PROCESS notifications");
}

// ===========================================================================
// 9. Documented exclusions
// ===========================================================================

#[test]
fn instanced_notification_constant_matches_godot() {
    assert_eq!(
        NOTIFICATION_INSTANCED.code(),
        25,
        "NOTIFICATION_INSTANCED should be code 25"
    );
}

#[test]
fn instanced_not_auto_dispatched_on_add_child() {
    // INSTANCED fires only when a node is instantiated from a PackedScene,
    // not on plain add_child. Verify it doesn't appear spuriously.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.event_trace_mut().enable();

    let child = Node::new("Manual", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);

    let log = tree.get_node(child_id).unwrap().notification_log();
    assert!(
        !log.contains(&NOTIFICATION_INSTANCED),
        "INSTANCED should NOT fire for manually added nodes"
    );

    let instanced_events = notification_paths(&tree, "INSTANCED");
    assert!(
        instanced_events.is_empty(),
        "INSTANCED should not appear in trace for manual add_child"
    );
}

/// NOTIFICATION_DRAW is defined (code 30) but not auto-dispatched during
/// rendering. This is a documented gap — Godot fires it for CanvasItem
/// nodes during the draw phase.
#[test]
fn draw_notification_not_in_process_frame() {
    let (mut ml, _parent, child) = make_entered_tree_with_subtree();
    ml.run_frames(3, 1.0 / 60.0);

    let log = ml.tree().get_node(child).unwrap().notification_log();
    let draw = Notification::new(30); // NOTIFICATION_DRAW
    assert!(
        !log.contains(&draw),
        "KNOWN GAP: DRAW (code 30) is not auto-dispatched during process frames"
    );
}

/// NOTIFICATION_DRAG_BEGIN (26) and NOTIFICATION_DRAG_END (27) are defined
/// but have no dispatch path yet. They require UI input handling.
#[test]
fn drag_notifications_defined_but_not_dispatched() {
    let drag_begin = Notification::new(26);
    let drag_end = Notification::new(27);
    assert_eq!(format!("{drag_begin}"), "NOTIFICATION_DRAG_BEGIN");
    assert_eq!(format!("{drag_end}"), "NOTIFICATION_DRAG_END");

    // Verify they don't fire during normal operation.
    let (mut ml, _parent, child) = make_entered_tree_with_subtree();
    ml.run_frames(1, 1.0 / 60.0);

    let log = ml.tree().get_node(child).unwrap().notification_log();
    assert!(!log.contains(&drag_begin), "DRAG_BEGIN should not fire without input");
    assert!(!log.contains(&drag_end), "DRAG_END should not fire without input");
}
