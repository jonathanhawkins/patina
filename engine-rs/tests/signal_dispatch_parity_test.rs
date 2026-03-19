//! Signal dispatch parity tests (pat-x8u).
//!
//! Verifies that Patina's signal dispatch matches Godot's documented behavior:
//! 1. Signals connect between nodes via connect(signal_name, target, method)
//! 2. emit_signal dispatches to connected targets in connection order
//! 3. Signal connections survive reparenting
//! 4. Disconnecting works (single, bulk, and by target)
//! 5. One-shot connections (CONNECT_ONE_SHOT) auto-disconnect after first emit
//!
//! All tests use EventTrace to capture signal emissions and verify ordering.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::SignalConnection as Connection;
use gdvariant::Variant;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

fn signal_events(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

/// Build a tree with Emitter and two Receiver nodes.
fn build_signal_tree() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    tree.event_trace_mut().enable();

    (tree, emitter_id, recv_a_id, recv_b_id)
}

// ===========================================================================
// 1. Basic connect and emit between nodes
// ===========================================================================

/// Signals can be connected between nodes and emit_signal dispatches correctly.
#[test]
fn connect_and_emit_between_nodes() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_test_signal", move |_args| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "test_signal", conn);
    tree.emit_signal(emitter_id, "test_signal", &[]);

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "callback should fire once"
    );

    let sig_events = signal_events(&tree);
    assert_eq!(sig_events.len(), 1);
    assert_eq!(sig_events[0].0, "/root/Emitter");
    assert_eq!(sig_events[0].1, "test_signal");
}

/// Multiple emissions are all traced.
#[test]
fn multiple_emissions_all_traced() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let conn = Connection::with_callback(recv_a_id.object_id(), "handler", |_| Variant::Nil);
    tree.connect_signal(emitter_id, "ping", conn);

    tree.emit_signal(emitter_id, "ping", &[]);
    tree.emit_signal(emitter_id, "ping", &[]);
    tree.emit_signal(emitter_id, "ping", &[]);

    let sig_events = signal_events(&tree);
    assert_eq!(sig_events.len(), 3, "all 3 emissions should be traced");
    for (path, detail) in &sig_events {
        assert_eq!(path, "/root/Emitter");
        assert_eq!(detail, "ping");
    }
}

// ===========================================================================
// 2. Connection-order dispatch
// ===========================================================================

/// emit_signal dispatches to targets in the order they were connected,
/// matching Godot's insertion-order guarantee.
#[test]
fn dispatch_order_matches_connection_order() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_signal_tree();

    let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));

    let order_a = call_order.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_signal_a", move |_| {
        order_a.lock().unwrap().push("A");
        Variant::Nil
    });

    let order_b = call_order.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_signal_b", move |_| {
        order_b.lock().unwrap().push("B");
        Variant::Nil
    });

    // Connect A first, then B.
    tree.connect_signal(emitter_id, "ordered_signal", conn_a);
    tree.connect_signal(emitter_id, "ordered_signal", conn_b);

    tree.emit_signal(emitter_id, "ordered_signal", &[]);

    let order = call_order.lock().unwrap();
    assert_eq!(
        *order,
        vec!["A", "B"],
        "dispatch must be in connection order"
    );
}

/// Reversed connection order produces reversed dispatch.
#[test]
fn reversed_connection_order_reverses_dispatch() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_signal_tree();

    let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));

    let order_b = call_order.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "handler_b", move |_| {
        order_b.lock().unwrap().push("B");
        Variant::Nil
    });

    let order_a = call_order.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "handler_a", move |_| {
        order_a.lock().unwrap().push("A");
        Variant::Nil
    });

    // Connect B first, then A — reversed from previous test.
    tree.connect_signal(emitter_id, "rev_signal", conn_b);
    tree.connect_signal(emitter_id, "rev_signal", conn_a);

    tree.emit_signal(emitter_id, "rev_signal", &[]);

    let order = call_order.lock().unwrap();
    assert_eq!(
        *order,
        vec!["B", "A"],
        "dispatch must follow connection order"
    );
}

// ===========================================================================
// 3. Signal connections survive reparenting
// ===========================================================================

/// Moving a connected node to a different parent preserves signal connections.
#[test]
fn connections_survive_reparenting_emitter() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "handler", move |_| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "test_sig", conn);

    // Reparent emitter under RecvA.
    tree.reparent(emitter_id, recv_a_id).unwrap();

    // Signal should still work after reparent.
    tree.emit_signal(emitter_id, "test_sig", &[]);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "connection should survive reparent"
    );

    // Trace should show the new path.
    let sig_events = signal_events(&tree);
    assert_eq!(sig_events.len(), 1);
    assert_eq!(
        sig_events[0].0, "/root/RecvA/Emitter",
        "trace should reflect new path"
    );
}

/// Moving a receiver node preserves its incoming connections.
#[test]
fn connections_survive_reparenting_receiver() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let parent_a = Node::new("ParentA", "Node2D");
    let parent_a_id = tree.add_child(root, parent_a).unwrap();

    let recv = Node::new("Recv", "Node2D");
    let recv_id = tree.add_child(parent_a_id, recv).unwrap();

    let parent_b = Node::new("ParentB", "Node2D");
    let parent_b_id = tree.add_child(root, parent_b).unwrap();

    tree.event_trace_mut().enable();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "handler", move |_| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "test_sig", conn);

    // Reparent receiver to a different parent.
    tree.reparent(recv_id, parent_b_id).unwrap();

    tree.emit_signal(emitter_id, "test_sig", &[]);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "connection should survive receiver reparent"
    );
}

// ===========================================================================
// 4. Disconnecting
// ===========================================================================

/// disconnect removes the connection and prevents further dispatch.
#[test]
fn disconnect_prevents_dispatch() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "handler", move |_| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "test_sig", conn);

    // Emit once — should fire.
    tree.emit_signal(emitter_id, "test_sig", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Disconnect.
    let store = tree.signal_store_mut(emitter_id);
    let removed = store.disconnect("test_sig", recv_a_id.object_id(), "handler");
    assert!(removed, "disconnect should return true");

    // Emit again — should NOT fire.
    tree.emit_signal(emitter_id, "test_sig", &[]);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "callback should not fire after disconnect"
    );

    // But the emission should still be traced (signal was emitted, just no connections).
    let sig_events = signal_events(&tree);
    assert_eq!(sig_events.len(), 2, "both emissions should be traced");
}

/// disconnect_all_for removes all connections for a specific target.
#[test]
fn disconnect_all_for_target() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_signal_tree();

    let counter_a = Arc::new(AtomicU64::new(0));
    let counter_b = Arc::new(AtomicU64::new(0));

    let ca = counter_a.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "handler_a", move |_| {
        ca.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    let cb = counter_b.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "handler_b", move |_| {
        cb.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "sig", conn_a);
    tree.connect_signal(emitter_id, "sig", conn_b);

    // Disconnect all connections targeting RecvA.
    tree.signal_store_mut(emitter_id)
        .disconnect_all_for(recv_a_id.object_id());

    tree.emit_signal(emitter_id, "sig", &[]);

    assert_eq!(
        counter_a.load(Ordering::SeqCst),
        0,
        "RecvA should be disconnected"
    );
    assert_eq!(
        counter_b.load(Ordering::SeqCst),
        1,
        "RecvB should still fire"
    );
}

/// Disconnecting a non-existent connection returns false.
#[test]
fn disconnect_nonexistent_returns_false() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    tree.signal_store_mut(emitter_id).add_signal("some_signal");
    let removed = tree.signal_store_mut(emitter_id).disconnect(
        "some_signal",
        recv_a_id.object_id(),
        "nonexistent",
    );
    assert!(!removed);
}

// ===========================================================================
// 5. One-shot connections (CONNECT_ONE_SHOT)
// ===========================================================================

/// One-shot connections fire once then auto-disconnect.
#[test]
fn one_shot_fires_once_then_disconnects() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "handler", move |_| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();

    tree.connect_signal(emitter_id, "one_shot_sig", conn);

    // First emit — should fire.
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "one-shot should fire on first emit"
    );

    // Second emit — should NOT fire (auto-disconnected).
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "one-shot should not fire on second emit"
    );

    // Connection count should be 0.
    let count = tree
        .signal_store_mut(emitter_id)
        .get_signal("one_shot_sig")
        .map(|s| s.connection_count())
        .unwrap_or(0);
    assert_eq!(count, 0, "one-shot connection should be removed after emit");
}

/// One-shot mixed with persistent connections: only one-shot is removed.
#[test]
fn one_shot_mixed_with_persistent() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_signal_tree();

    let counter_one = Arc::new(AtomicU64::new(0));
    let counter_persist = Arc::new(AtomicU64::new(0));

    let co = counter_one.clone();
    let conn_one_shot =
        Connection::with_callback(recv_a_id.object_id(), "one_shot_handler", move |_| {
            co.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_one_shot();

    let cp = counter_persist.clone();
    let conn_persistent =
        Connection::with_callback(recv_b_id.object_id(), "persistent_handler", move |_| {
            cp.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        });

    tree.connect_signal(emitter_id, "mixed_sig", conn_one_shot);
    tree.connect_signal(emitter_id, "mixed_sig", conn_persistent);

    // First emit — both fire.
    tree.emit_signal(emitter_id, "mixed_sig", &[]);
    assert_eq!(counter_one.load(Ordering::SeqCst), 1);
    assert_eq!(counter_persist.load(Ordering::SeqCst), 1);

    // Second emit — only persistent fires.
    tree.emit_signal(emitter_id, "mixed_sig", &[]);
    assert_eq!(
        counter_one.load(Ordering::SeqCst),
        1,
        "one-shot should not fire again"
    );
    assert_eq!(
        counter_persist.load(Ordering::SeqCst),
        2,
        "persistent should fire again"
    );
}

/// One-shot preserves dispatch order on the single emission where it fires.
#[test]
fn one_shot_dispatch_order_preserved() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_signal_tree();

    let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));

    let oa = call_order.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "handler_a", move |_| {
        oa.lock().unwrap().push("A-oneshot");
        Variant::Nil
    })
    .as_one_shot();

    let ob = call_order.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "handler_b", move |_| {
        ob.lock().unwrap().push("B-persist");
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "sig", conn_a);
    tree.connect_signal(emitter_id, "sig", conn_b);

    tree.emit_signal(emitter_id, "sig", &[]);

    let order = call_order.lock().unwrap();
    assert_eq!(
        *order,
        vec!["A-oneshot", "B-persist"],
        "one-shot should fire in connection order before being removed"
    );
}

// ===========================================================================
// 6. Signal trace ordering relative to lifecycle
// ===========================================================================

/// Signal emissions are traced with correct frame numbers.
#[test]
fn signal_trace_has_correct_frame_numbers() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let conn = Connection::with_callback(recv_a_id.object_id(), "handler", |_| Variant::Nil);
    tree.connect_signal(emitter_id, "tick", conn);

    // Emit on frame 0 (default).
    tree.emit_signal(emitter_id, "tick", &[]);

    // Advance frame counter and emit again.
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "tick", &[]);

    let frames: Vec<u64> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| e.frame)
        .collect();

    assert_eq!(
        frames,
        vec![0, 1],
        "signal emissions should have correct frame numbers"
    );
}

/// Emitting a signal with no connections still records a trace event.
#[test]
fn emit_no_connections_still_traced() {
    let (mut tree, emitter_id, _recv_a_id, _recv_b_id) = build_signal_tree();

    tree.emit_signal(emitter_id, "unconnected_signal", &[]);

    let sig_events = signal_events(&tree);
    assert_eq!(
        sig_events.len(),
        1,
        "emission with no connections should still be traced"
    );
    assert_eq!(sig_events[0].1, "unconnected_signal");
}

/// Multiple signals on the same emitter are traced independently.
#[test]
fn multiple_signals_traced_independently() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let conn1 = Connection::with_callback(recv_a_id.object_id(), "h1", |_| Variant::Nil);
    let conn2 = Connection::with_callback(recv_a_id.object_id(), "h2", |_| Variant::Nil);

    tree.connect_signal(emitter_id, "signal_alpha", conn1);
    tree.connect_signal(emitter_id, "signal_beta", conn2);

    tree.emit_signal(emitter_id, "signal_alpha", &[]);
    tree.emit_signal(emitter_id, "signal_beta", &[]);

    let details: Vec<String> = signal_events(&tree)
        .iter()
        .map(|(_, d)| d.clone())
        .collect();

    assert_eq!(details, vec!["signal_alpha", "signal_beta"]);
}

// ===========================================================================
// 7. Duplicate connections (Godot allows them)
// ===========================================================================

/// Connecting the same callback twice causes it to fire twice per emission.
#[test]
fn duplicate_connections_fire_twice() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_signal_tree();

    let counter = Arc::new(AtomicU64::new(0));

    let c1 = counter.clone();
    let conn1 = Connection::with_callback(recv_a_id.object_id(), "handler", move |_| {
        c1.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    let c2 = counter.clone();
    let conn2 = Connection::with_callback(recv_a_id.object_id(), "handler", move |_| {
        c2.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "dup_sig", conn1);
    tree.connect_signal(emitter_id, "dup_sig", conn2);

    tree.emit_signal(emitter_id, "dup_sig", &[]);

    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "duplicate connections should both fire"
    );
}
