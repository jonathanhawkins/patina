//! Deferred signal FIFO delivery with mixed one-shot listeners (pat-lals).
//!
//! Proves that Patina's deferred signal queue maintains strict FIFO order when
//! connections include a mix of persistent (non-one-shot) and one-shot listeners.
//!
//! Godot contract:
//! - Deferred callbacks are queued in emission order.
//! - Within a single emission, deferred callbacks queue in connection-registration order.
//! - One-shot connections are removed after the emission that queues them.
//! - Flush dispatches every queued callback in FIFO order, regardless of whether the
//!   originating connection was one-shot or persistent.
//! - After one-shot removal, subsequent emissions only queue surviving connections,
//!   but the global queue order is still strictly FIFO.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::SignalConnection as Connection;
use gdvariant::Variant;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a tree with an emitter and three receiver nodes.
fn build_tree_three_receivers() -> (SceneTree, NodeId, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    let recv_c = Node::new("RecvC", "Node2D");
    let recv_c_id = tree.add_child(root, recv_c).unwrap();

    tree.event_trace_mut().enable();

    (tree, emitter_id, recv_a_id, recv_b_id, recv_c_id)
}

/// Build a tree with an emitter and two receivers.
fn build_tree_two_receivers() -> (SceneTree, NodeId, NodeId, NodeId) {
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
// 1. Single emission: FIFO across mixed persistent + one-shot deferred
// ===========================================================================

/// Three deferred connections on the same signal in order: persistent, one-shot,
/// persistent. A single emit + flush must deliver callbacks in registration order.
#[test]
fn single_emit_fifo_mixed_persistent_oneshot() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id, recv_c_id) =
        build_tree_three_receivers();

    let order = Arc::new(Mutex::new(Vec::<&str>::new()));

    // Connection 0: persistent + deferred → RecvA
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "my_signal",
        Connection::with_callback(recv_a_id.object_id(), "on_a", move |_| {
            o.lock().unwrap().push("persistent_a");
            Variant::Nil
        })
        .as_deferred(),
    );

    // Connection 1: one-shot + deferred → RecvB
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "my_signal",
        Connection::with_callback(recv_b_id.object_id(), "on_b", move |_| {
            o.lock().unwrap().push("oneshot_b");
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // Connection 2: persistent + deferred → RecvC
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "my_signal",
        Connection::with_callback(recv_c_id.object_id(), "on_c", move |_| {
            o.lock().unwrap().push("persistent_c");
            Variant::Nil
        })
        .as_deferred(),
    );

    // Emit once.
    tree.emit_signal(emitter_id, "my_signal", &[]);
    assert_eq!(tree.deferred_signal_count(), 3);

    // Nothing should have fired yet.
    assert!(order.lock().unwrap().is_empty());

    // Flush — must fire in registration (FIFO) order.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 3);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["persistent_a", "oneshot_b", "persistent_c"],
        "deferred flush must respect connection registration order"
    );
}

// ===========================================================================
// 2. Two emissions before flush: global FIFO with one-shot consumed on first
// ===========================================================================

/// Emit the same signal twice before flushing. The one-shot connection is
/// removed after the first emission, so the second emission only queues the
/// two persistent connections. The flush order must be:
///   [A₁, B₁(one-shot), C₁, A₂, C₂]
#[test]
fn two_emissions_fifo_oneshot_consumed_after_first() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id, recv_c_id) =
        build_tree_three_receivers();

    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    // Connection 0: persistent + deferred
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig",
        Connection::with_callback(recv_a_id.object_id(), "on_a", move |_| {
            o.lock().unwrap().push("A".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // Connection 1: one-shot + deferred
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig",
        Connection::with_callback(recv_b_id.object_id(), "on_b", move |_| {
            o.lock().unwrap().push("B_oneshot".into());
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // Connection 2: persistent + deferred
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig",
        Connection::with_callback(recv_c_id.object_id(), "on_c", move |_| {
            o.lock().unwrap().push("C".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // First emission: queues A, B_oneshot, C. Removes B from connections.
    tree.emit_signal(emitter_id, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 3);

    // Second emission: only A and C are still connected.
    tree.emit_signal(emitter_id, "sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        5,
        "3 from first emit + 2 from second emit"
    );

    // Flush — strict FIFO across both emissions.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 5);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["A", "B_oneshot", "C", "A", "C"],
        "global FIFO: first emission [A, B, C] then second emission [A, C]"
    );
}

// ===========================================================================
// 3. Interleaved signals: FIFO across different signal names
// ===========================================================================

/// Two different signals, each with mixed one-shot/persistent deferred listeners.
/// Emissions are interleaved: sig_x, sig_y, sig_x. The flush must deliver in
/// strict emission order across both signals.
#[test]
fn interleaved_signals_fifo_mixed_oneshot() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    // sig_x: persistent deferred on RecvA
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig_x",
        Connection::with_callback(recv_a_id.object_id(), "on_x", move |_| {
            o.lock().unwrap().push("X_persist".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // sig_y: one-shot deferred on RecvA, then persistent deferred on RecvB
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig_y",
        Connection::with_callback(recv_a_id.object_id(), "on_y_oneshot", move |_| {
            o.lock().unwrap().push("Y_oneshot".into());
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig_y",
        Connection::with_callback(recv_b_id.object_id(), "on_y_persist", move |_| {
            o.lock().unwrap().push("Y_persist".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // Interleaved emissions.
    tree.emit_signal(emitter_id, "sig_x", &[]);   // queues: X_persist
    tree.emit_signal(emitter_id, "sig_y", &[]);   // queues: Y_oneshot, Y_persist
    tree.emit_signal(emitter_id, "sig_x", &[]);   // queues: X_persist (again)

    assert_eq!(tree.deferred_signal_count(), 4);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 4);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["X_persist", "Y_oneshot", "Y_persist", "X_persist"],
        "interleaved emission FIFO across different signals"
    );
}

// ===========================================================================
// 4. All one-shot deferred: FIFO preserved, all removed after first emit
// ===========================================================================

/// Three one-shot deferred connections on the same signal. After one emission
/// they all queue in FIFO order, then all are removed. A second emission
/// produces nothing.
#[test]
fn all_oneshot_deferred_fifo_then_empty() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id, recv_c_id) =
        build_tree_three_receivers();

    let order = Arc::new(Mutex::new(Vec::<&str>::new()));

    for (label, recv_id) in [
        ("first", recv_a_id),
        ("second", recv_b_id),
        ("third", recv_c_id),
    ] {
        let o = order.clone();
        let lbl = label;
        tree.connect_signal(
            emitter_id,
            "ephemeral",
            Connection::with_callback(recv_id.object_id(), label, move |_| {
                o.lock().unwrap().push(lbl);
                Variant::Nil
            })
            .as_deferred()
            .as_one_shot(),
        );
    }

    // First emit: all three queue.
    tree.emit_signal(emitter_id, "ephemeral", &[]);
    assert_eq!(tree.deferred_signal_count(), 3);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 3);

    let fired = order.lock().unwrap().clone();
    assert_eq!(fired, vec!["first", "second", "third"], "FIFO among one-shot deferred");

    // Second emit: no connections remain.
    tree.emit_signal(emitter_id, "ephemeral", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        0,
        "all one-shot connections were consumed"
    );
}

// ===========================================================================
// 5. Alternating one-shot and persistent: five connections, FIFO verified
// ===========================================================================

/// Pattern: persistent, one-shot, persistent, one-shot, persistent.
/// Verifies FIFO on first flush, then correct survivorship on second emission.
#[test]
fn alternating_oneshot_persistent_fifo() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_tree_two_receivers();

    let order = Arc::new(Mutex::new(Vec::<usize>::new()));

    for i in 0..5 {
        let o = order.clone();
        let is_oneshot = i % 2 == 1; // indices 1, 3 are one-shot
        let mut conn = Connection::with_callback(
            recv_a_id.object_id(),
            &format!("handler_{i}"),
            move |_| {
                o.lock().unwrap().push(i);
                Variant::Nil
            },
        )
        .as_deferred();

        if is_oneshot {
            conn = conn.as_one_shot();
        }

        tree.connect_signal(emitter_id, "alt_sig", conn);
    }

    // First emission: all 5 queue.
    tree.emit_signal(emitter_id, "alt_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 5);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 5);

    {
        let fired = order.lock().unwrap();
        assert_eq!(*fired, vec![0, 1, 2, 3, 4], "FIFO for all five connections");
    }
    order.lock().unwrap().clear();

    // Second emission: only persistent connections (0, 2, 4) remain.
    tree.emit_signal(emitter_id, "alt_sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        3,
        "one-shot connections 1 and 3 should be gone"
    );

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 3);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec![0, 2, 4],
        "surviving persistent connections maintain relative FIFO"
    );
}

// ===========================================================================
// 6. Mixed immediate + deferred + one-shot: deferred FIFO unaffected by immediate
// ===========================================================================

/// Immediate (non-deferred) connections fire synchronously during emit. Deferred
/// connections (persistent and one-shot) queue in FIFO. This test verifies that
/// immediate dispatch does not disturb the deferred queue order.
#[test]
fn immediate_does_not_disturb_deferred_fifo() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id, recv_c_id) =
        build_tree_three_receivers();

    let deferred_order = Arc::new(Mutex::new(Vec::<&str>::new()));
    let immediate_order = Arc::new(Mutex::new(Vec::<&str>::new()));

    // Connection 0: immediate (non-deferred) on RecvA
    let io = immediate_order.clone();
    tree.connect_signal(
        emitter_id,
        "mixed",
        Connection::with_callback(recv_a_id.object_id(), "imm_a", move |_| {
            io.lock().unwrap().push("imm_a");
            Variant::Nil
        }),
    );

    // Connection 1: deferred persistent on RecvB
    let do_ = deferred_order.clone();
    tree.connect_signal(
        emitter_id,
        "mixed",
        Connection::with_callback(recv_b_id.object_id(), "def_b", move |_| {
            do_.lock().unwrap().push("def_b_persist");
            Variant::Nil
        })
        .as_deferred(),
    );

    // Connection 2: immediate one-shot on RecvC
    let io = immediate_order.clone();
    tree.connect_signal(
        emitter_id,
        "mixed",
        Connection::with_callback(recv_c_id.object_id(), "imm_c", move |_| {
            io.lock().unwrap().push("imm_c_oneshot");
            Variant::Nil
        })
        .as_one_shot(),
    );

    // Connection 3: deferred one-shot on RecvA
    let do_ = deferred_order.clone();
    tree.connect_signal(
        emitter_id,
        "mixed",
        Connection::with_callback(recv_a_id.object_id(), "def_a_os", move |_| {
            do_.lock().unwrap().push("def_a_oneshot");
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // Connection 4: deferred persistent on RecvC
    let do_ = deferred_order.clone();
    tree.connect_signal(
        emitter_id,
        "mixed",
        Connection::with_callback(recv_c_id.object_id(), "def_c", move |_| {
            do_.lock().unwrap().push("def_c_persist");
            Variant::Nil
        })
        .as_deferred(),
    );

    // Emit — immediate connections fire now.
    tree.emit_signal(emitter_id, "mixed", &[]);

    // Immediate callbacks already fired.
    assert_eq!(
        *immediate_order.lock().unwrap(),
        vec!["imm_a", "imm_c_oneshot"]
    );

    // Deferred callbacks have NOT fired yet.
    assert!(deferred_order.lock().unwrap().is_empty());
    assert_eq!(tree.deferred_signal_count(), 3);

    // Flush deferred — must be in registration order (connections 1, 3, 4).
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 3);

    let fired = deferred_order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["def_b_persist", "def_a_oneshot", "def_c_persist"],
        "deferred FIFO unaffected by interleaved immediate connections"
    );
}

// ===========================================================================
// 7. Deterministic trace: emit events recorded in emission order
// ===========================================================================

/// Verifies that the event trace records SignalEmit events in the correct order
/// when using mixed one-shot/persistent deferred connections, providing a
/// deterministic trace for oracle comparison.
#[test]
fn trace_records_emission_order_with_mixed_oneshot() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let counter = Arc::new(AtomicU64::new(0));

    // Persistent deferred
    let c = counter.clone();
    tree.connect_signal(
        emitter_id,
        "traced_sig",
        Connection::with_callback(recv_a_id.object_id(), "persist_handler", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );

    // One-shot deferred
    let c = counter.clone();
    tree.connect_signal(
        emitter_id,
        "traced_sig",
        Connection::with_callback(recv_b_id.object_id(), "oneshot_handler", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // Emit three times — one-shot is consumed after first.
    tree.emit_signal(emitter_id, "traced_sig", &[]);
    tree.emit_signal(emitter_id, "traced_sig", &[]);
    tree.emit_signal(emitter_id, "traced_sig", &[]);

    // Queue: 2 + 1 + 1 = 4 deferred calls.
    assert_eq!(tree.deferred_signal_count(), 4);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 4);
    assert_eq!(counter.load(Ordering::SeqCst), 4);

    // Trace should have 3 SignalEmit events for "traced_sig".
    let trace = tree.event_trace();
    let emit_events: Vec<_> = trace
        .events()
        .iter()
        .filter(|e| {
            e.event_type == gdscene::TraceEventType::SignalEmit
                && e.detail == "traced_sig"
        })
        .collect();
    assert_eq!(
        emit_events.len(),
        3,
        "three emissions recorded in trace"
    );
}

// ===========================================================================
// 8. Flush-emit-flush: one-shot consumed in first flush, second flush clean
// ===========================================================================

/// Emit, flush (consuming one-shot), emit again, flush again. Second flush
/// should only contain persistent connections.
#[test]
fn flush_emit_flush_oneshot_consumed_correctly() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let persist_count = Arc::new(AtomicU64::new(0));
    let oneshot_count = Arc::new(AtomicU64::new(0));

    let pc = persist_count.clone();
    tree.connect_signal(
        emitter_id,
        "fef_sig",
        Connection::with_callback(recv_a_id.object_id(), "persist", move |_| {
            pc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );

    let oc = oneshot_count.clone();
    tree.connect_signal(
        emitter_id,
        "fef_sig",
        Connection::with_callback(recv_b_id.object_id(), "oneshot", move |_| {
            oc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // First cycle: emit + flush.
    tree.emit_signal(emitter_id, "fef_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 2);
    tree.flush_deferred_signals();
    assert_eq!(persist_count.load(Ordering::SeqCst), 1);
    assert_eq!(oneshot_count.load(Ordering::SeqCst), 1);

    // Second cycle: emit + flush — one-shot is gone.
    tree.emit_signal(emitter_id, "fef_sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "only persistent connection remains"
    );
    tree.flush_deferred_signals();
    assert_eq!(persist_count.load(Ordering::SeqCst), 2);
    assert_eq!(
        oneshot_count.load(Ordering::SeqCst),
        1,
        "one-shot must not fire again"
    );
}

// ===========================================================================
// 9. Multiple one-shots at different positions: FIFO across all
// ===========================================================================

/// Six connections: alternating one-shot/persistent, emitted once.
/// Verifies strict FIFO ordering regardless of one-shot flag.
#[test]
fn six_connections_alternating_oneshot_fifo() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_tree_two_receivers();

    let order = Arc::new(Mutex::new(Vec::<usize>::new()));

    for i in 0..6 {
        let o = order.clone();
        let mut conn = Connection::with_callback(
            recv_a_id.object_id(),
            &format!("h{i}"),
            move |_| {
                o.lock().unwrap().push(i);
                Variant::Nil
            },
        )
        .as_deferred();

        if i % 2 == 0 {
            conn = conn.as_one_shot();
        }

        tree.connect_signal(emitter_id, "six_sig", conn);
    }

    tree.emit_signal(emitter_id, "six_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 6);

    tree.flush_deferred_signals();
    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec![0, 1, 2, 3, 4, 5],
        "strict FIFO across six mixed connections"
    );
}

// ===========================================================================
// 10. Arguments preserved across deferred one-shot delivery
// ===========================================================================

/// Deferred one-shot connections must receive the arguments captured at
/// emit-time, not stale or default values.
#[test]
fn deferred_oneshot_preserves_args() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_tree_two_receivers();

    let captured = Arc::new(Mutex::new(Vec::<Variant>::new()));

    let c = captured.clone();
    tree.connect_signal(
        emitter_id,
        "arg_sig",
        Connection::with_callback(recv_a_id.object_id(), "on_arg", move |args| {
            c.lock().unwrap().extend(args.iter().cloned());
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    tree.emit_signal(
        emitter_id,
        "arg_sig",
        &[Variant::Int(42), Variant::String("hello".into())],
    );

    // Not delivered yet.
    assert!(captured.lock().unwrap().is_empty());

    tree.flush_deferred_signals();

    let args = captured.lock().unwrap();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], Variant::Int(42));
    assert_eq!(args[1], Variant::String("hello".into()));
}
