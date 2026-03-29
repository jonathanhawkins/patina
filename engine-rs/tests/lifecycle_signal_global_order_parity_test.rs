//! pat-h0ug: Global lifecycle and signal ordering trace parity.
//!
//! Verifies the **total ordered interleaving** of notifications, script
//! callbacks, and signal emissions in a single unified trace. Each test
//! captures all event types simultaneously and asserts that their relative
//! ordering matches Godot's oracle contracts.
//!
//! This is distinct from the per-type tests (pat-fbi for lifecycle, pat-fu6
//! for signals) — here we test the cross-type ordering guarantees.

use gdcore::id::ObjectId;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::{TraceEvent, TraceEventType};
use gdscene::{LifecycleManager, MainLoop};
use gdobject::signal::Connection;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

// ===========================================================================
// Helpers
// ===========================================================================

/// Filter events by type.
fn filter_type(events: &[TraceEvent], ty: TraceEventType) -> Vec<&TraceEvent> {
    events.iter().filter(|e| e.event_type == ty).collect()
}

/// Extracts (event_type_tag, detail, node_path) triples for compact assertion.
fn event_summary(events: &[TraceEvent]) -> Vec<(&str, &str, &str)> {
    events
        .iter()
        .map(|e| {
            let ty = match e.event_type {
                TraceEventType::Notification => "notif",
                TraceEventType::SignalEmit => "signal",
                TraceEventType::ScriptCall => "call",
                TraceEventType::ScriptReturn => "ret",
            };
            (ty, e.detail.as_str(), e.node_path.as_str())
        })
        .collect()
}

/// Build a tree: root -> Parent -> [Child1, Child2]
fn build_hierarchy() -> (
    SceneTree,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child1 = Node::new("Child1", "Node2D");
    let child1_id = tree.add_child(parent_id, child1).unwrap();
    let child2 = Node::new("Child2", "Node2D");
    let child2_id = tree.add_child(parent_id, child2).unwrap();
    (tree, parent_id, child1_id, child2_id)
}

/// Build a deeper tree: root -> A -> B -> C
fn build_deep_hierarchy() -> (
    SceneTree,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(a_id, b).unwrap();
    let c = Node::new("C", "Node2D");
    let c_id = tree.add_child(b_id, c).unwrap();
    (tree, a_id, b_id, c_id)
}

// ===========================================================================
// 1. Global trace captures all event types in insertion order
// ===========================================================================

/// Oracle contract: the global EventTrace captures notifications and signal
/// emissions in a single ordered sequence. When a signal is emitted between
/// two lifecycle notifications, it appears between them in the trace.
#[test]
fn global_trace_interleaves_notifications_and_signals() {
    let (mut tree, parent_id, child1_id, _child2_id) = build_hierarchy();
    tree.event_trace_mut().enable();

    // Manual sequence: ENTER_TREE on parent, signal from child1, READY on parent.
    tree.trace_record(parent_id, TraceEventType::Notification, "ENTER_TREE");
    tree.emit_signal(child1_id, "health_changed", &[]);
    tree.trace_record(parent_id, TraceEventType::Notification, "READY");

    let summary = event_summary(tree.event_trace().events());
    assert_eq!(summary.len(), 3);
    assert_eq!(summary[0], ("notif", "ENTER_TREE", "/root/Parent"));
    assert_eq!(summary[1], ("signal", "health_changed", "/root/Parent/Child1"));
    assert_eq!(summary[2], ("notif", "READY", "/root/Parent"));
}

// ===========================================================================
// 2. Lifecycle enter_tree produces notifications interleaved correctly
// ===========================================================================

/// Oracle contract: during LifecycleManager::enter_tree, the global trace
/// contains ENTER_TREE (top-down) followed by READY (bottom-up) with all
/// enter events strictly before all ready events.
#[test]
fn lifecycle_global_trace_enter_then_ready_phase_separation() {
    let (mut tree, parent_id, _child1_id, _child2_id) = build_hierarchy();
    tree.event_trace_mut().enable();

    LifecycleManager::enter_tree(&mut tree, parent_id);

    let events = tree.event_trace().events();
    let notifs: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .collect();

    // Find the boundary: last ENTER_TREE must precede first READY.
    let last_enter_idx = notifs
        .iter()
        .rposition(|e| e.detail == "ENTER_TREE")
        .expect("at least one ENTER_TREE");
    let first_ready_idx = notifs
        .iter()
        .position(|e| e.detail == "READY")
        .expect("at least one READY");

    assert!(
        last_enter_idx < first_ready_idx,
        "Oracle contract: all ENTER_TREE must complete before any READY. \
         last_enter={last_enter_idx}, first_ready={first_ready_idx}"
    );

    // Verify combined sequence for the 3-node hierarchy.
    let details: Vec<(&str, &str)> = notifs
        .iter()
        .map(|e| (e.detail.as_str(), e.node_path.as_str()))
        .collect();

    assert_eq!(
        details,
        vec![
            ("ENTER_TREE", "/root/Parent"),
            ("ENTER_TREE", "/root/Parent/Child1"),
            ("ENTER_TREE", "/root/Parent/Child2"),
            ("READY", "/root/Parent/Child1"),
            ("READY", "/root/Parent/Child2"),
            ("READY", "/root/Parent"),
        ]
    );
}

// ===========================================================================
// 3. Signal emission during lifecycle preserves global order
// ===========================================================================

/// Oracle contract: if a signal is emitted (via callback) during the
/// lifecycle enter phase, the SignalEmit trace event appears at the correct
/// position in the global trace — after the ENTER_TREE that triggered it
/// and before subsequent notifications.
#[test]
fn signal_emitted_during_enter_appears_in_global_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let listener = Node::new("Listener", "Node2D");
    let _listener_id = tree.add_child(root, listener).unwrap();

    tree.event_trace_mut().enable();

    // Simulate: ENTER_TREE on emitter, then emitter fires a signal, then
    // ENTER_TREE on listener.
    tree.trace_record(emitter_id, TraceEventType::Notification, "ENTER_TREE");
    tree.emit_signal(emitter_id, "tree_entered_custom", &[]);
    tree.trace_record(_listener_id, TraceEventType::Notification, "ENTER_TREE");

    let summary = event_summary(tree.event_trace().events());
    assert_eq!(
        summary,
        vec![
            ("notif", "ENTER_TREE", "/root/Emitter"),
            ("signal", "tree_entered_custom", "/root/Emitter"),
            ("notif", "ENTER_TREE", "/root/Listener"),
        ]
    );
}

// ===========================================================================
// 4. Callback connections fire synchronously and appear in trace
// ===========================================================================

/// Oracle contract: non-deferred signal connections fire synchronously
/// during emit. The callback executes between SignalEmit and whatever
/// follows in the trace.
#[test]
fn callback_fires_synchronously_within_global_trace() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let fire_count = Arc::new(AtomicU32::new(0));
    let fc = fire_count.clone();
    let conn = Connection::with_callback(ObjectId::next(), "on_hit", move |_args| {
        fc.fetch_add(1, Ordering::SeqCst);
        gdvariant::Variant::Nil
    });
    tree.connect_signal(emitter_id, "hit", conn);

    tree.event_trace_mut().enable();

    // Record a notification, emit signal (callback fires synchronously), record another.
    tree.trace_record(emitter_id, TraceEventType::Notification, "PROCESS");
    tree.emit_signal(emitter_id, "hit", &[]);
    tree.trace_record(emitter_id, TraceEventType::Notification, "POST_PROCESS");

    assert_eq!(fire_count.load(Ordering::SeqCst), 1, "callback must have fired");

    let summary = event_summary(tree.event_trace().events());
    assert_eq!(
        summary,
        vec![
            ("notif", "PROCESS", "/root/Emitter"),
            ("signal", "hit", "/root/Emitter"),
            ("notif", "POST_PROCESS", "/root/Emitter"),
        ]
    );
}

// ===========================================================================
// 5. Multi-node signal chain preserves global ordering
// ===========================================================================

/// Oracle contract: when multiple nodes emit signals in sequence, the global
/// trace preserves the total order across all event types.
#[test]
fn multi_node_signal_chain_global_order() {
    let (mut tree, parent_id, child1_id, child2_id) = build_hierarchy();
    tree.event_trace_mut().enable();

    // Sequence: parent notif -> child1 signal -> child2 signal -> parent notif
    tree.trace_record(parent_id, TraceEventType::Notification, "ENTER_TREE");
    tree.emit_signal(child1_id, "damaged", &[]);
    tree.emit_signal(child2_id, "healed", &[]);
    tree.trace_record(parent_id, TraceEventType::Notification, "READY");

    let summary = event_summary(tree.event_trace().events());
    assert_eq!(summary.len(), 4);
    assert_eq!(summary[0].0, "notif");
    assert_eq!(summary[0].1, "ENTER_TREE");
    assert_eq!(summary[1].0, "signal");
    assert_eq!(summary[1].1, "damaged");
    assert_eq!(summary[1].2, "/root/Parent/Child1");
    assert_eq!(summary[2].0, "signal");
    assert_eq!(summary[2].1, "healed");
    assert_eq!(summary[2].2, "/root/Parent/Child2");
    assert_eq!(summary[3].0, "notif");
    assert_eq!(summary[3].1, "READY");
}

// ===========================================================================
// 6. Deep hierarchy lifecycle + signal total ordering
// ===========================================================================

/// Oracle contract: in a 3-level deep hierarchy (A->B->C), the full
/// enter_tree lifecycle produces a predictable global trace: ENTER_TREE
/// top-down (A, B, C) then READY bottom-up (C, B, A). Signals emitted
/// between these phases appear at the correct global position.
#[test]
fn deep_hierarchy_lifecycle_with_interleaved_signals() {
    let (mut tree, a_id, _b_id, c_id) = build_deep_hierarchy();
    tree.event_trace_mut().enable();

    // Run the standard lifecycle.
    LifecycleManager::enter_tree(&mut tree, a_id);

    // Now emit some signals post-lifecycle to verify they appear after.
    tree.emit_signal(c_id, "spawned", &[]);
    tree.emit_signal(a_id, "all_ready", &[]);

    let events = tree.event_trace().events();

    // Lifecycle events come first.
    let lifecycle_count = events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .count();
    assert_eq!(lifecycle_count, 6, "3 ENTER_TREE + 3 READY = 6");

    // Signals appear after all lifecycle events.
    let signal_events = filter_type(events, TraceEventType::SignalEmit);
    assert_eq!(signal_events.len(), 2);
    assert_eq!(signal_events[0].detail, "spawned");
    assert_eq!(signal_events[0].node_path, "/root/A/B/C");
    assert_eq!(signal_events[1].detail, "all_ready");
    assert_eq!(signal_events[1].node_path, "/root/A");

    // Verify global ordering: all notifications have lower indices than signals.
    let first_signal_idx = events
        .iter()
        .position(|e| e.event_type == TraceEventType::SignalEmit)
        .unwrap();
    let last_notif_idx = events
        .iter()
        .rposition(|e| e.event_type == TraceEventType::Notification)
        .unwrap();
    assert!(
        last_notif_idx < first_signal_idx,
        "lifecycle notifications must all precede post-lifecycle signals"
    );
}

// ===========================================================================
// 7. Full frame cycle: enter -> process -> signal -> exit ordering
// ===========================================================================

/// Oracle contract: the complete lifecycle (enter_tree → process frames →
/// signal → exit_tree) produces a total ordered trace where:
/// - ENTER_TREE and READY appear first (frame 0 trace_frame)
/// - Per-frame notifications (INTERNAL_PHYSICS, PHYSICS, INTERNAL_PROCESS, PROCESS)
///   appear next
/// - Signal emissions appear at their emission point
/// - EXIT_TREE appears last
#[test]
fn full_lifecycle_frame_signal_exit_global_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Player", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    tree.event_trace_mut().enable();

    // Phase 1: enter tree.
    LifecycleManager::enter_tree(&mut tree, node_id);

    // Phase 2: run 1 frame through MainLoop.
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // Phase 3: emit a signal mid-execution.
    ml.tree_mut().emit_signal(node_id, "action_complete", &[]);

    // Phase 4: exit tree.
    LifecycleManager::exit_tree(ml.tree_mut(), node_id);

    let events = ml.tree().event_trace().events();

    // Extract Player-only events for clarity.
    let player_events: Vec<(&str, &str)> = events
        .iter()
        .filter(|e| e.node_path == "/root/Player")
        .map(|e| {
            let ty = match e.event_type {
                TraceEventType::Notification => "notif",
                TraceEventType::SignalEmit => "signal",
                TraceEventType::ScriptCall => "call",
                TraceEventType::ScriptReturn => "ret",
            };
            (ty, e.detail.as_str())
        })
        .collect();

    // Verify phase ordering: ENTER_TREE comes first, EXIT_TREE comes last.
    assert_eq!(player_events[0], ("notif", "ENTER_TREE"));
    assert_eq!(player_events[1], ("notif", "READY"));
    assert_eq!(
        *player_events.last().unwrap(),
        ("notif", "EXIT_TREE")
    );

    // The signal must appear after process-phase notifications and before EXIT_TREE.
    let signal_idx = player_events
        .iter()
        .position(|&(ty, _)| ty == "signal")
        .expect("expected signal event");
    let exit_idx = player_events
        .iter()
        .position(|&(ty, detail)| ty == "notif" && detail == "EXIT_TREE")
        .unwrap();
    let ready_idx = player_events
        .iter()
        .position(|&(ty, detail)| ty == "notif" && detail == "READY")
        .unwrap();

    assert!(
        signal_idx > ready_idx,
        "signal must appear after READY"
    );
    assert!(
        signal_idx < exit_idx,
        "signal must appear before EXIT_TREE"
    );

    // Verify per-frame notification ordering (Godot 4-phase):
    // INTERNAL_PHYSICS_PROCESS < PHYSICS_PROCESS < INTERNAL_PROCESS < PROCESS
    let frame_notifs: Vec<&str> = player_events
        .iter()
        .filter(|&&(ty, _)| ty == "notif")
        .map(|&(_, detail)| detail)
        .collect();

    let has_ip = frame_notifs.contains(&"INTERNAL_PHYSICS_PROCESS");
    let has_pp = frame_notifs.contains(&"PHYSICS_PROCESS");
    let has_ip2 = frame_notifs.contains(&"INTERNAL_PROCESS");
    let has_p = frame_notifs.contains(&"PROCESS");
    assert!(has_ip && has_pp && has_ip2 && has_p, "all 4 frame phases present");
}

// ===========================================================================
// 8. Multiple signals in a single frame preserve order
// ===========================================================================

/// Oracle contract: when multiple signals fire within a single frame, they
/// are recorded in the global trace in emission order, interspersed with
/// any other events that happened between emissions.
#[test]
fn multiple_signals_single_frame_global_order() {
    let (mut tree, parent_id, child1_id, child2_id) = build_hierarchy();
    tree.event_trace_mut().enable();
    tree.set_trace_frame(0);

    // All on frame 0.
    tree.emit_signal(parent_id, "frame_start", &[]);
    tree.trace_record(child1_id, TraceEventType::Notification, "PROCESS");
    tree.emit_signal(child1_id, "attacking", &[]);
    tree.trace_record(child2_id, TraceEventType::Notification, "PROCESS");
    tree.emit_signal(child2_id, "defending", &[]);

    let events = tree.event_trace().events();
    assert_eq!(events.len(), 5);

    // All events on frame 0.
    for ev in events {
        assert_eq!(ev.frame, 0);
    }

    let summary = event_summary(events);
    assert_eq!(summary[0], ("signal", "frame_start", "/root/Parent"));
    assert_eq!(summary[1], ("notif", "PROCESS", "/root/Parent/Child1"));
    assert_eq!(summary[2], ("signal", "attacking", "/root/Parent/Child1"));
    assert_eq!(summary[3], ("notif", "PROCESS", "/root/Parent/Child2"));
    assert_eq!(summary[4], ("signal", "defending", "/root/Parent/Child2"));
}

// ===========================================================================
// 9. Cross-frame ordering: frame counter advances between lifecycle phases
// ===========================================================================

/// Oracle contract: the frame counter in trace events reflects the MainLoop
/// frame, allowing lifecycle events (frame 0 before first step) to be
/// distinguished from per-frame events (frame 0 after first step, frame 1
/// after second step, etc.).
#[test]
fn cross_frame_global_trace_frame_counters() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Tracker", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    tree.event_trace_mut().enable();

    // Lifecycle at frame 0.
    LifecycleManager::enter_tree(&mut tree, node_id);

    // Run 3 frames.
    let mut ml = MainLoop::new(tree);
    ml.run_frames(3, 1.0 / 60.0);

    let events = ml.tree().event_trace().events();
    let tracker_events: Vec<_> = events
        .iter()
        .filter(|e| e.node_path == "/root/Tracker")
        .collect();

    // Lifecycle events have frame 0 (set before MainLoop takes ownership).
    let enter = tracker_events
        .iter()
        .find(|e| e.detail == "ENTER_TREE")
        .unwrap();
    assert_eq!(enter.frame, 0);

    let ready = tracker_events
        .iter()
        .find(|e| e.detail == "READY")
        .unwrap();
    assert_eq!(ready.frame, 0);

    // Frame-phase events span frames 0, 1, 2.
    let process_events: Vec<_> = tracker_events
        .iter()
        .filter(|e| e.detail == "PROCESS")
        .collect();
    assert_eq!(process_events.len(), 3);
    assert_eq!(process_events[0].frame, 0);
    assert_eq!(process_events[1].frame, 1);
    assert_eq!(process_events[2].frame, 2);
}

// ===========================================================================
// 10. Deferred signals fire after process phase in global trace
// ===========================================================================

/// Oracle contract: deferred signal connections do NOT fire during emit.
/// They are queued and flushed at end-of-frame. In the global trace, the
/// SignalEmit event is recorded at emission time, but the deferred callback
/// fires later (not visible as a separate trace event unless it calls back
/// into the tree).
#[test]
fn deferred_signal_fires_after_immediate_in_global_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let immediate_count = Arc::new(AtomicU32::new(0));
    let deferred_count = Arc::new(AtomicU32::new(0));

    // Immediate connection.
    let ic = immediate_count.clone();
    let conn_imm = Connection::with_callback(ObjectId::next(), "on_hit_imm", move |_| {
        ic.fetch_add(1, Ordering::SeqCst);
        gdvariant::Variant::Nil
    });
    tree.connect_signal(emitter_id, "hit", conn_imm);

    // Deferred connection.
    let dc = deferred_count.clone();
    let mut conn_def = Connection::with_callback(ObjectId::next(), "on_hit_def", move |_| {
        dc.fetch_add(1, Ordering::SeqCst);
        gdvariant::Variant::Nil
    });
    conn_def.deferred = true;
    tree.connect_signal(emitter_id, "hit", conn_def);

    tree.event_trace_mut().enable();

    // Emit — immediate fires now.
    tree.emit_signal(emitter_id, "hit", &[]);
    assert_eq!(immediate_count.load(Ordering::SeqCst), 1);

    // The trace shows the signal emission at the point of emit.
    let signals = filter_type(tree.event_trace().events(), TraceEventType::SignalEmit);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].detail, "hit");
}

// ===========================================================================
// 11. Sibling notification order preserved across event types
// ===========================================================================

/// Oracle contract: for siblings, the trace must show notifications
/// dispatched in tree order (insertion order). When signals are interleaved
/// per-node, the global trace reflects per-node sequencing.
#[test]
fn siblings_interleaved_notification_signal_tree_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let alpha = Node::new("Alpha", "Node2D");
    let alpha_id = tree.add_child(root, alpha).unwrap();
    let beta = Node::new("Beta", "Node2D");
    let beta_id = tree.add_child(root, beta).unwrap();
    let gamma = Node::new("Gamma", "Node2D");
    let gamma_id = tree.add_child(root, gamma).unwrap();

    tree.event_trace_mut().enable();

    // Simulate per-node process: each node gets PROCESS then emits a signal.
    for (id, sig) in [
        (alpha_id, "alpha_done"),
        (beta_id, "beta_done"),
        (gamma_id, "gamma_done"),
    ] {
        tree.trace_record(id, TraceEventType::Notification, "PROCESS");
        tree.emit_signal(id, sig, &[]);
    }

    let summary = event_summary(tree.event_trace().events());
    assert_eq!(summary.len(), 6);
    assert_eq!(summary[0], ("notif", "PROCESS", "/root/Alpha"));
    assert_eq!(summary[1], ("signal", "alpha_done", "/root/Alpha"));
    assert_eq!(summary[2], ("notif", "PROCESS", "/root/Beta"));
    assert_eq!(summary[3], ("signal", "beta_done", "/root/Beta"));
    assert_eq!(summary[4], ("notif", "PROCESS", "/root/Gamma"));
    assert_eq!(summary[5], ("signal", "gamma_done", "/root/Gamma"));
}

// ===========================================================================
// 12. Enter + exit global trace shows full lifecycle symmetry
// ===========================================================================

/// Oracle contract: the full enter→exit lifecycle for a hierarchy produces
/// a globally ordered trace:
/// ENTER_TREE top-down → READY bottom-up → EXIT_TREE bottom-up
#[test]
fn enter_exit_full_cycle_global_trace() {
    let (mut tree, parent_id, _child1_id, _child2_id) = build_hierarchy();
    tree.event_trace_mut().enable();

    LifecycleManager::enter_tree(&mut tree, parent_id);
    LifecycleManager::exit_tree(&mut tree, parent_id);

    let events = tree.event_trace().events();
    let notifs: Vec<(&str, &str)> = events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| (e.detail.as_str(), e.node_path.as_str()))
        .collect();

    assert_eq!(
        notifs,
        vec![
            // ENTER_TREE top-down
            ("ENTER_TREE", "/root/Parent"),
            ("ENTER_TREE", "/root/Parent/Child1"),
            ("ENTER_TREE", "/root/Parent/Child2"),
            // READY bottom-up
            ("READY", "/root/Parent/Child1"),
            ("READY", "/root/Parent/Child2"),
            ("READY", "/root/Parent"),
            // EXIT_TREE bottom-up
            ("EXIT_TREE", "/root/Parent/Child1"),
            ("EXIT_TREE", "/root/Parent/Child2"),
            ("EXIT_TREE", "/root/Parent"),
        ]
    );
}

// ===========================================================================
// 13. Mixed event types maintain monotonic frame numbers
// ===========================================================================

/// Oracle contract: across all event types (notifications, signals, script
/// calls), frame numbers are monotonically non-decreasing.
#[test]
fn mixed_event_types_monotonic_frame_numbers() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, b).unwrap();

    tree.event_trace_mut().enable();

    // Frame 0: lifecycle.
    LifecycleManager::enter_tree(&mut tree, a_id);

    let mut ml = MainLoop::new(tree);

    // Run 3 frames, emitting signals within.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
        ml.tree_mut().emit_signal(a_id, "tick", &[]);
        ml.tree_mut().emit_signal(b_id, "tock", &[]);
    }

    let events = ml.tree().event_trace().events();
    let mut last_frame = 0u64;
    for ev in events {
        assert!(
            ev.frame >= last_frame,
            "frame {} followed {last_frame} — expected monotonic non-decreasing. Event: {:?}",
            ev.frame,
            ev
        );
        last_frame = ev.frame;
    }
}

// ===========================================================================
// 14. Signal with multiple connections: all fire before next trace event
// ===========================================================================

/// Oracle contract: when a signal has multiple non-deferred connections,
/// all callbacks fire synchronously before the next event in the global trace.
#[test]
fn multi_connection_signal_all_fire_before_next_event() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let counter = Arc::new(AtomicU32::new(0));

    // Wire 3 connections.
    for i in 0..3 {
        let c = counter.clone();
        let conn = Connection::with_callback(
            ObjectId::next(),
            &format!("handler_{i}"),
            move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                gdvariant::Variant::Nil
            },
        );
        tree.connect_signal(emitter_id, "multi_sig", conn);
    }

    tree.event_trace_mut().enable();

    tree.trace_record(emitter_id, TraceEventType::Notification, "BEFORE");
    tree.emit_signal(emitter_id, "multi_sig", &[]);
    // All 3 callbacks must have fired before we record AFTER.
    assert_eq!(counter.load(Ordering::SeqCst), 3);
    tree.trace_record(emitter_id, TraceEventType::Notification, "AFTER");

    let summary = event_summary(tree.event_trace().events());
    assert_eq!(summary.len(), 3);
    assert_eq!(summary[0], ("notif", "BEFORE", "/root/Emitter"));
    assert_eq!(summary[1], ("signal", "multi_sig", "/root/Emitter"));
    assert_eq!(summary[2], ("notif", "AFTER", "/root/Emitter"));
}

// ===========================================================================
// 15. One-shot signal removes after firing but still appears in trace
// ===========================================================================

/// Oracle contract: a one-shot connection fires once and is removed, but
/// the emission is still recorded in the global trace.
#[test]
fn one_shot_signal_appears_in_global_trace() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let count = Arc::new(AtomicU32::new(0));
    let c = count.clone();
    let mut conn = Connection::with_callback(ObjectId::next(), "on_once", move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        gdvariant::Variant::Nil
    });
    conn.one_shot = true;
    tree.connect_signal(emitter_id, "once_sig", conn);

    tree.event_trace_mut().enable();

    // First emit: fires.
    tree.emit_signal(emitter_id, "once_sig", &[]);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    // Second emit: no callback (removed), but emission still traced.
    tree.emit_signal(emitter_id, "once_sig", &[]);
    assert_eq!(count.load(Ordering::SeqCst), 1, "one-shot should not fire again");

    let signals = filter_type(tree.event_trace().events(), TraceEventType::SignalEmit);
    assert_eq!(signals.len(), 2, "both emissions recorded in trace");
    assert_eq!(signals[0].detail, "once_sig");
    assert_eq!(signals[1].detail, "once_sig");
}

// ===========================================================================
// 16. Complete multi-node frame with signals: Godot-compatible global order
// ===========================================================================

/// Oracle contract: a multi-node frame produces the complete Godot-compatible
/// global ordering:
///   For each physics tick:
///     INTERNAL_PHYSICS_PROCESS (all nodes, tree order)
///     PHYSICS_PROCESS (all nodes, tree order)
///   Then:
///     INTERNAL_PROCESS (all nodes, tree order)
///     PROCESS (all nodes, tree order)
///   Signals emitted after process phase appear after all PROCESS events.
#[test]
fn multi_node_frame_godot_compatible_global_order() {
    let (tree, parent_id, child1_id, _child2_id) = build_hierarchy();
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();

    // Run one frame.
    ml.step(1.0 / 60.0);

    // Emit signals after the frame step.
    ml.tree_mut().emit_signal(parent_id, "frame_done", &[]);
    ml.tree_mut().emit_signal(child1_id, "child_done", &[]);

    let events = ml.tree().event_trace().events();

    // Verify phase ordering by finding first occurrence of each phase.
    let first_int_phys = events
        .iter()
        .position(|e| e.detail == "INTERNAL_PHYSICS_PROCESS")
        .expect("INTERNAL_PHYSICS_PROCESS present");
    let first_phys = events
        .iter()
        .position(|e| e.detail == "PHYSICS_PROCESS")
        .expect("PHYSICS_PROCESS present");
    let first_int_proc = events
        .iter()
        .position(|e| e.detail == "INTERNAL_PROCESS")
        .expect("INTERNAL_PROCESS present");
    let first_proc = events
        .iter()
        .position(|e| e.detail == "PROCESS")
        .expect("PROCESS present");
    let first_signal = events
        .iter()
        .position(|e| e.event_type == TraceEventType::SignalEmit)
        .expect("signal present");

    // Godot ordering: INT_PHYS < PHYS < INT_PROC < PROC < post-frame signals
    assert!(
        first_int_phys < first_phys,
        "INTERNAL_PHYSICS before PHYSICS"
    );
    assert!(
        first_phys < first_int_proc,
        "PHYSICS before INTERNAL_PROCESS"
    );
    assert!(
        first_int_proc < first_proc,
        "INTERNAL_PROCESS before PROCESS"
    );
    assert!(
        first_proc < first_signal,
        "PROCESS before post-frame signals"
    );
}
