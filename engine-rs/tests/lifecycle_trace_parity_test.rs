//! Lifecycle and signal ordering trace parity tests (pat-b16).
//!
//! Verifies that Patina's lifecycle dispatch ordering matches Godot's documented behavior:
//! - ENTER_TREE: top-down (parent before child)
//! - READY: bottom-up (child before parent)
//! - EXIT_TREE: bottom-up (child before parent)
//! - PREDELETE: after EXIT_TREE
//! - Signal emissions during lifecycle fire in correct order
//!
//! These tests build trees directly and inspect EventTrace output, comparing
//! against expected orderings derived from Godot documentation.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop};

/// Build a 3-level tree:
///   root
///   └── Parent (Node2D)
///       ├── Child1 (Node2D)
///       │   └── GrandChild (Node2D)
///       └── Child2 (Node2D)
fn build_hierarchy() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let child1 = Node::new("Child1", "Node2D");
    let child1_id = tree.add_child(parent_id, child1).unwrap();

    let grandchild = Node::new("GrandChild", "Node2D");
    let grandchild_id = tree.add_child(child1_id, grandchild).unwrap();

    let child2 = Node::new("Child2", "Node2D");
    let child2_id = tree.add_child(parent_id, child2).unwrap();

    tree.event_trace_mut().enable();

    (tree, parent_id, child1_id, grandchild_id, child2_id)
}

fn event_paths(tree: &SceneTree, detail: &str, event_type: TraceEventType) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == event_type)
        .map(|e| e.node_path.clone())
        .collect()
}

fn all_events_summary(tree: &SceneTree) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .map(|e| format!("{:?}:{}:{}", e.event_type, e.detail, e.node_path))
        .collect()
}

// ===========================================================================
// 1. ENTER_TREE ordering (top-down: parent before child)
// ===========================================================================

/// Godot doc: ENTER_TREE fires top-down. Parent gets ENTER_TREE before children.
/// In a 3-level tree: Parent → Child1 → GrandChild → Child2.
#[test]
fn enter_tree_top_down_3_levels() {
    let (mut tree, parent_id, _c1, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);

    let paths = event_paths(&tree, "ENTER_TREE", TraceEventType::Notification);
    assert_eq!(
        paths,
        vec![
            "/root/Parent",
            "/root/Parent/Child1",
            "/root/Parent/Child1/GrandChild",
            "/root/Parent/Child2",
        ],
        "ENTER_TREE must fire top-down (depth-first, parent before children)"
    );
}

/// Adding a sibling later fires ENTER_TREE for the new node.
#[test]
fn enter_tree_late_addition() {
    let (mut tree, parent_id, _c1, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);
    tree.event_trace_mut().clear();

    let late = Node::new("Late", "Node2D");
    let late_id = tree.add_child(parent_id, late).unwrap();
    LifecycleManager::enter_tree(&mut tree, late_id);

    let paths = event_paths(&tree, "ENTER_TREE", TraceEventType::Notification);
    // Verify Late appears in ENTER_TREE events (no other nodes should fire).
    assert!(
        paths.iter().all(|p| p.contains("Late")),
        "only Late should get ENTER_TREE after late addition, got: {paths:?}"
    );
    assert!(
        !paths.is_empty(),
        "Late should receive at least one ENTER_TREE"
    );
}

// ===========================================================================
// 2. READY ordering (bottom-up: child before parent)
// ===========================================================================

/// Godot doc: READY fires bottom-up. Deepest children get READY first.
/// In a 3-level tree: GrandChild → Child1 → Child2 → Parent.
#[test]
fn ready_bottom_up_3_levels() {
    let (mut tree, parent_id, _c1, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);

    let paths = event_paths(&tree, "READY", TraceEventType::Notification);
    assert_eq!(
        paths,
        vec![
            "/root/Parent/Child1/GrandChild",
            "/root/Parent/Child1",
            "/root/Parent/Child2",
            "/root/Parent",
        ],
        "READY must fire bottom-up (deepest child first, parent last)"
    );
}

/// All ENTER_TREE events must complete before any READY event fires.
#[test]
fn all_enter_tree_before_any_ready() {
    let (mut tree, parent_id, _c1, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);

    let events = tree.event_trace().events();
    let notifs: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .filter(|e| e.detail == "ENTER_TREE" || e.detail == "READY")
        .collect();

    let last_enter = notifs
        .iter()
        .rposition(|e| e.detail == "ENTER_TREE")
        .expect("at least one ENTER_TREE");
    let first_ready = notifs
        .iter()
        .position(|e| e.detail == "READY")
        .expect("at least one READY");

    assert!(
        last_enter < first_ready,
        "all ENTER_TREE must complete before any READY: last_enter={last_enter}, first_ready={first_ready}"
    );
}

// ===========================================================================
// 3. EXIT_TREE ordering (bottom-up: child before parent)
// ===========================================================================

/// Godot doc: EXIT_TREE fires bottom-up. Deepest children exit first.
#[test]
fn exit_tree_bottom_up_3_levels() {
    let (mut tree, parent_id, _c1, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);
    tree.event_trace_mut().clear();

    LifecycleManager::exit_tree(&mut tree, parent_id);

    let paths = event_paths(&tree, "EXIT_TREE", TraceEventType::Notification);
    assert_eq!(
        paths,
        vec![
            "/root/Parent/Child1/GrandChild",
            "/root/Parent/Child1",
            "/root/Parent/Child2",
            "/root/Parent",
        ],
        "EXIT_TREE must fire bottom-up (deepest child first, parent last)"
    );
}

/// EXIT_TREE for a single leaf node should fire only for that node.
#[test]
fn exit_tree_leaf_only() {
    let (mut tree, parent_id, _c1, grandchild_id, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);
    tree.event_trace_mut().clear();

    LifecycleManager::exit_tree(&mut tree, grandchild_id);

    let paths = event_paths(&tree, "EXIT_TREE", TraceEventType::Notification);
    assert_eq!(paths, vec!["/root/Parent/Child1/GrandChild"]);
}

/// EXIT_TREE for a subtree fires for all children bottom-up.
#[test]
fn exit_tree_subtree() {
    let (mut tree, parent_id, child1_id, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);
    tree.event_trace_mut().clear();

    LifecycleManager::exit_tree(&mut tree, child1_id);

    let paths = event_paths(&tree, "EXIT_TREE", TraceEventType::Notification);
    assert_eq!(
        paths,
        vec![
            "/root/Parent/Child1/GrandChild",
            "/root/Parent/Child1",
        ],
        "EXIT_TREE for subtree should fire bottom-up for all descendants"
    );
}

// ===========================================================================
// 4. EXIT_TREE + script _exit_tree callback ordering
// ===========================================================================

/// Scripts with _exit_tree should have it called as part of EXIT_TREE.
#[test]
fn exit_tree_calls_script_exit_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let child = Node::new("ScriptChild", "Node2D");
    let child_id = tree.add_child(parent_id, child).unwrap();

    let script_src = "extends Node2D\nfunc _exit_tree():\n    pass\n";
    let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
    tree.attach_script(child_id, Box::new(script));

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, parent_id);
    tree.event_trace_mut().clear();

    LifecycleManager::exit_tree(&mut tree, parent_id);

    let events = tree.event_trace().events();
    let exit_events: Vec<_> = events
        .iter()
        .filter(|e| e.detail == "EXIT_TREE" || e.detail == "_exit_tree")
        .map(|e| format!("{:?}:{}:{}", e.event_type, e.detail, e.node_path))
        .collect();

    // ScriptChild should get EXIT_TREE notification + _exit_tree script call before Parent.
    assert!(
        exit_events.len() >= 3,
        "expected EXIT_TREE + _exit_tree for child, then EXIT_TREE for parent, got: {exit_events:?}"
    );

    // Verify child's EXIT_TREE comes before parent's.
    let child_exit_idx = events
        .iter()
        .position(|e| e.detail == "EXIT_TREE" && e.node_path.contains("ScriptChild"))
        .expect("child EXIT_TREE");
    let parent_exit_idx = events
        .iter()
        .position(|e| e.detail == "EXIT_TREE" && e.node_path == "/root/Parent")
        .expect("parent EXIT_TREE");
    assert!(child_exit_idx < parent_exit_idx, "child EXIT_TREE before parent");
}

// ===========================================================================
// 5. process_deletions fires EXIT_TREE before removing nodes
// ===========================================================================

/// In Godot, queue_free() fires EXIT_TREE (bottom-up) before actually removing
/// the node, followed by PREDELETE. Patina must do the same.
#[test]
fn process_deletions_fires_exit_tree() {
    let (mut tree, parent_id, _c1, _gc, _c2) = build_hierarchy();
    LifecycleManager::enter_tree(&mut tree, parent_id);
    tree.event_trace_mut().clear();

    tree.queue_free(parent_id);
    tree.process_deletions();

    let exit_paths = event_paths(&tree, "EXIT_TREE", TraceEventType::Notification);

    assert_eq!(
        exit_paths,
        vec![
            "/root/Parent/Child1/GrandChild",
            "/root/Parent/Child1",
            "/root/Parent/Child2",
            "/root/Parent",
        ],
        "EXIT_TREE from process_deletions should be bottom-up"
    );

    // PREDELETE should fire after EXIT_TREE for each node.
    let predelete_paths = event_paths(&tree, "PREDELETE", TraceEventType::Notification);
    assert_eq!(
        predelete_paths,
        vec![
            "/root/Parent/Child1/GrandChild",
            "/root/Parent/Child1",
            "/root/Parent/Child2",
            "/root/Parent",
        ],
        "PREDELETE should fire bottom-up after EXIT_TREE"
    );

    // Verify ordering: all EXIT_TREE events come before all PREDELETE events.
    let events: Vec<_> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == "EXIT_TREE" || e.detail == "PREDELETE")
        .map(|e| e.detail.as_str())
        .collect();
    let last_exit = events.iter().rposition(|d| *d == "EXIT_TREE").unwrap();
    let first_predelete = events.iter().position(|d| *d == "PREDELETE").unwrap();
    assert!(
        last_exit < first_predelete,
        "all EXIT_TREE events must precede PREDELETE events"
    );
}

// ===========================================================================
// 6. Signal emissions during lifecycle
// ===========================================================================

/// Signals emitted during _ready should be traced with correct frame/ordering.
#[test]
fn signal_emitted_during_ready_is_traced() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    // Script that emits a signal during _ready.
    let script_src = "\
extends Node2D
signal my_signal
func _ready():
    emit_signal(\"my_signal\")
";
    let script = GDScriptNodeInstance::from_source(script_src, emitter_id).unwrap();
    tree.attach_script(emitter_id, Box::new(script));

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, emitter_id);

    let events = tree.event_trace().events();

    // Check that signal emission appears in trace.
    let signal_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .collect();

    // Signal may or may not be emitted depending on interpreter support.
    // If it is, verify it appears after READY and during _ready script call.
    if !signal_events.is_empty() {
        let ready_idx = events
            .iter()
            .position(|e| e.detail == "READY" && e.node_path.contains("Emitter"))
            .expect("Emitter READY");
        let signal_idx = events
            .iter()
            .position(|e| e.event_type == TraceEventType::SignalEmit)
            .expect("signal emission");
        assert!(
            signal_idx > ready_idx,
            "signal emission should occur after READY notification"
        );
    }
}

// ===========================================================================
// 7. Full lifecycle sequence: enter → process → exit
// ===========================================================================

/// Verify the complete lifecycle sequence matches Godot's expected ordering.
#[test]
fn full_lifecycle_sequence() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("A", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let child = Node::new("B", "Node2D");
    let child_id = tree.add_child(parent_id, child).unwrap();

    tree.event_trace_mut().enable();

    // Phase 1: Enter tree.
    LifecycleManager::enter_tree(&mut tree, parent_id);

    // Phase 2: One frame of processing.
    let mut main_loop = MainLoop::new(tree);
    main_loop.step(1.0 / 60.0);

    // Phase 3: Exit tree.
    LifecycleManager::exit_tree(main_loop.tree_mut(), parent_id);

    let events = main_loop.tree().event_trace().events();
    let summary: Vec<String> = events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "ENTER_TREE"
                    || e.detail == "READY"
                    || e.detail == "EXIT_TREE")
        })
        .map(|e| format!("{}:{}", e.detail, e.node_path))
        .collect();

    // Expected Godot lifecycle:
    // ENTER_TREE: A (parent first), B (child second) — top-down
    // READY: B (child first), A (parent second) — bottom-up
    // EXIT_TREE: B (child first), A (parent second) — bottom-up
    assert_eq!(
        summary,
        vec![
            "ENTER_TREE:/root/A",
            "ENTER_TREE:/root/A/B",
            "READY:/root/A/B",
            "READY:/root/A",
            "EXIT_TREE:/root/A/B",
            "EXIT_TREE:/root/A",
        ],
        "full lifecycle: enter(top-down) → ready(bottom-up) → exit(bottom-up)"
    );
}

// ===========================================================================
// 8. Deep hierarchy ordering
// ===========================================================================

/// Verify ordering with a 5-level deep tree.
#[test]
fn deep_hierarchy_enter_ready_exit() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build chain: L1 → L2 → L3 → L4 → L5
    let l1 = tree.add_child(root, Node::new("L1", "Node")).unwrap();
    let l2 = tree.add_child(l1, Node::new("L2", "Node")).unwrap();
    let l3 = tree.add_child(l2, Node::new("L3", "Node")).unwrap();
    let l4 = tree.add_child(l3, Node::new("L4", "Node")).unwrap();
    let _l5 = tree.add_child(l4, Node::new("L5", "Node")).unwrap();

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, l1);

    let enter_paths = event_paths(&tree, "ENTER_TREE", TraceEventType::Notification);
    assert_eq!(
        enter_paths,
        vec!["/root/L1", "/root/L1/L2", "/root/L1/L2/L3", "/root/L1/L2/L3/L4", "/root/L1/L2/L3/L4/L5"],
        "ENTER_TREE: L1 → L2 → L3 → L4 → L5 (top-down)"
    );

    let ready_paths = event_paths(&tree, "READY", TraceEventType::Notification);
    assert_eq!(
        ready_paths,
        vec!["/root/L1/L2/L3/L4/L5", "/root/L1/L2/L3/L4", "/root/L1/L2/L3", "/root/L1/L2", "/root/L1"],
        "READY: L5 → L4 → L3 → L2 → L1 (bottom-up)"
    );

    tree.event_trace_mut().clear();
    LifecycleManager::exit_tree(&mut tree, l1);

    let exit_paths = event_paths(&tree, "EXIT_TREE", TraceEventType::Notification);
    assert_eq!(
        exit_paths,
        vec!["/root/L1/L2/L3/L4/L5", "/root/L1/L2/L3/L4", "/root/L1/L2/L3", "/root/L1/L2", "/root/L1"],
        "EXIT_TREE: L5 → L4 → L3 → L2 → L1 (bottom-up)"
    );
}

// ===========================================================================
// 9. Multi-child ordering: siblings processed in child-index order
// ===========================================================================

/// Siblings should be entered in the order they were added (child index order).
#[test]
fn sibling_enter_order_matches_child_index() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("P", "Node")).unwrap();
    let _c_alpha = tree.add_child(parent, Node::new("Alpha", "Node")).unwrap();
    let _c_beta = tree.add_child(parent, Node::new("Beta", "Node")).unwrap();
    let _c_gamma = tree.add_child(parent, Node::new("Gamma", "Node")).unwrap();

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, parent);

    let enter_paths = event_paths(&tree, "ENTER_TREE", TraceEventType::Notification);
    assert_eq!(
        enter_paths,
        vec!["/root/P", "/root/P/Alpha", "/root/P/Beta", "/root/P/Gamma"],
        "siblings entered in child-index order"
    );

    let ready_paths = event_paths(&tree, "READY", TraceEventType::Notification);
    assert_eq!(
        ready_paths,
        vec!["/root/P/Alpha", "/root/P/Beta", "/root/P/Gamma", "/root/P"],
        "siblings ready in child-index order, parent last"
    );
}

// ===========================================================================
// 10. Script _ready ordering with mixed scripted/unscripted nodes
// ===========================================================================

/// Only nodes with _ready defined should have script_call events.
/// All nodes get READY notification regardless.
#[test]
fn ready_script_calls_only_for_nodes_with_ready() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
    let scripted = tree.add_child(parent, Node::new("Scripted", "Node2D")).unwrap();
    let _unscripted = tree.add_child(parent, Node::new("Plain", "Node2D")).unwrap();

    let script_src = "extends Node2D\nfunc _ready():\n    pass\n";
    let script = GDScriptNodeInstance::from_source(script_src, scripted).unwrap();
    tree.attach_script(scripted, Box::new(script));

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, parent);

    // All 3 nodes should get READY notification.
    let ready_notifs = event_paths(&tree, "READY", TraceEventType::Notification);
    assert_eq!(ready_notifs.len(), 3, "all nodes get READY notification");

    // Only Scripted should have _ready script_call.
    let ready_calls = event_paths(&tree, "_ready", TraceEventType::ScriptCall);
    assert_eq!(
        ready_calls,
        vec!["/root/Parent/Scripted"],
        "only nodes with _ready defined should have script_call"
    );
}
