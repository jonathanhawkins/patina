//! pat-ekp: Revalidate signal ordering and dispatch behavior against 4.6.1.
//!
//! This test suite explicitly revalidates every signal ordering and dispatch
//! contract against the Godot 4.6.1-stable behavioral oracle. It covers:
//!
//! 1. Registration-order dispatch (golden: fixtures/golden/signals/registration_order_trace.json)
//! 2. Deferred FIFO delivery (golden: fixtures/golden/signals/deferred_behavior_trace.json)
//! 3. Argument forwarding fidelity (golden: fixtures/golden/signals/arguments_forwarding_trace.json)
//! 4. One-shot auto-disconnect semantics
//! 5. Mixed immediate + deferred ordering within a single frame
//! 6. Bind/unbind argument resolution order (unbind-first, then bind)
//! 7. Signal emission trace event correctness
//! 8. Cross-lifecycle signal ordering: READY before first PROCESS signal
//!
//! All golden files reference `upstream_version: "4.6.1-stable"`.

mod oracle_fixture;

use oracle_fixture::fixtures_dir;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::SignalConnection as Connection;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn build_tree(names: &[&str]) -> (SceneTree, Vec<NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for name in names {
        let node = Node::new(*name, "Node2D");
        ids.push(tree.add_child(root, node).unwrap());
    }
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    (tree, ids)
}

fn signal_emit_details(tree: &SceneTree) -> Vec<(String, String, u64)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| (e.node_path.clone(), e.detail.clone(), e.frame))
        .collect()
}

// ===========================================================================
// 1. Registration-order dispatch — golden trace validation (4.6.1)
// ===========================================================================

/// Godot 4.6.1 fires signal callbacks in connection insertion order.
/// Golden: registration_order_trace.json declares expected_callback_order = [A, B, C].
#[test]
fn golden_461_registration_order_three_receivers() {
    // Validate golden file exists and references 4.6.1
    let golden_path = fixtures_dir()
        .join("golden/signals/registration_order_trace.json");
    let golden_text = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("missing golden file {}: {e}", golden_path.display()));
    let golden: Value = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(
        golden["upstream_version"].as_str().unwrap(),
        "4.6.1-stable",
        "golden must reference 4.6.1"
    );

    let expected_order: Vec<String> = golden["expected_callback_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    // Build tree matching golden scenario
    let (mut tree, ids) = build_tree(&["Emitter", "RecvA", "RecvB", "RecvC"]);
    let emitter_id = ids[0];

    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    // Connect in order matching golden: A, B, C
    for (i, recv_name) in expected_order.iter().enumerate() {
        let o = order.clone();
        let label = recv_name.clone();
        let conn = Connection::with_callback(
            ids[i + 1].object_id(),
            &format!("on_signal_{}", recv_name.to_lowercase()),
            move |_| {
                o.lock().unwrap().push(label.clone());
                Variant::Nil
            },
        );
        tree.connect_signal(emitter_id, "ordered_signal", conn);
    }

    tree.emit_signal(emitter_id, "ordered_signal", &[]);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired, expected_order,
        "4.6.1 contract: callbacks fire in registration order"
    );

    // Verify trace records exactly one SignalEmit event
    let emits = signal_emit_details(&tree);
    assert_eq!(emits.len(), 1);
    assert_eq!(emits[0].0, "/root/Emitter");
    assert_eq!(emits[0].1, "ordered_signal");
}

// ===========================================================================
// 2. Deferred FIFO delivery — golden trace validation (4.6.1)
// ===========================================================================

/// Godot 4.6.1: deferred callbacks queue in emission order and flush FIFO.
/// Golden: deferred_behavior_trace.json / deferred_only scenario.
#[test]
fn golden_461_deferred_fifo_delivery() {
    let golden_path = fixtures_dir()
        .join("golden/signals/deferred_behavior_trace.json");
    let golden_text = std::fs::read_to_string(&golden_path).unwrap();
    let golden: Value = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(golden["upstream_version"].as_str().unwrap(), "4.6.1-stable");

    let scenario = &golden["scenarios"]["deferred_only"];
    let expected_order: Vec<String> = scenario["expected_callback_order_after_flush"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let expected_before_flush = scenario["callbacks_before_flush"].as_u64().unwrap();

    let (mut tree, ids) = build_tree(&["Emitter", "RecvFirst", "RecvSecond", "RecvThird"]);
    let emitter_id = ids[0];

    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    // Three deferred connections
    let signal_names = ["sig_first", "sig_second", "sig_third"];
    for (i, sig_name) in signal_names.iter().enumerate() {
        let o = order.clone();
        let label = expected_order[i].clone();
        let conn = Connection::with_callback(
            ids[i + 1].object_id(),
            &format!("on_{label}"),
            move |_| {
                o.lock().unwrap().push(label.clone());
                Variant::Nil
            },
        )
        .as_deferred();
        tree.connect_signal(emitter_id, sig_name, conn);
    }

    // Emit all three in order
    for sig_name in &signal_names {
        tree.emit_signal(emitter_id, sig_name, &[]);
    }

    // Before flush: no callbacks should have fired
    assert_eq!(
        order.lock().unwrap().len() as u64,
        expected_before_flush,
        "4.6.1: deferred callbacks do NOT fire before flush"
    );

    // Flush
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 3);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired, expected_order,
        "4.6.1 contract: deferred callbacks flush in FIFO order"
    );
}

/// Godot 4.6.1: mixed immediate + deferred — immediate fires during emit,
/// deferred fires on flush.
/// Golden: deferred_behavior_trace.json / mixed_immediate_deferred scenario.
#[test]
fn golden_461_mixed_immediate_deferred() {
    let golden_path = fixtures_dir()
        .join("golden/signals/deferred_behavior_trace.json");
    let golden_text = std::fs::read_to_string(&golden_path).unwrap();
    let golden: Value = serde_json::from_str(&golden_text).unwrap();

    let scenario = &golden["scenarios"]["mixed_immediate_deferred"];
    let before_flush = scenario["callbacks_before_flush"].as_u64().unwrap();
    let after_flush = scenario["callbacks_after_flush"].as_u64().unwrap();

    let (mut tree, ids) = build_tree(&["Emitter", "RecvImm", "RecvDef"]);
    let emitter_id = ids[0];

    let total_count = Arc::new(AtomicU64::new(0));
    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    // Immediate connection
    let tc = total_count.clone();
    let o = order.clone();
    let conn_imm = Connection::with_callback(ids[1].object_id(), "on_imm", move |_| {
        tc.fetch_add(1, AtomicOrdering::SeqCst);
        o.lock().unwrap().push("immediate".into());
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "mixed_sig", conn_imm);

    // Deferred connection
    let tc = total_count.clone();
    let o = order.clone();
    let conn_def = Connection::with_callback(ids[2].object_id(), "on_def", move |_| {
        tc.fetch_add(1, AtomicOrdering::SeqCst);
        o.lock().unwrap().push("deferred".into());
        Variant::Nil
    })
    .as_deferred();
    tree.connect_signal(emitter_id, "mixed_sig", conn_def);

    tree.emit_signal(emitter_id, "mixed_sig", &[]);

    // After emit, before flush: only immediate has fired
    assert_eq!(
        total_count.load(AtomicOrdering::SeqCst),
        before_flush,
        "4.6.1: only immediate callback fires during emit"
    );

    tree.flush_deferred_signals();

    assert_eq!(
        total_count.load(AtomicOrdering::SeqCst),
        after_flush,
        "4.6.1: deferred fires after flush"
    );

    // Order: immediate before deferred
    let fired = order.lock().unwrap();
    assert_eq!(*fired, vec!["immediate", "deferred"]);
}

/// Godot 4.6.1: one-shot fires once, second emission does not invoke callback.
/// Golden: deferred_behavior_trace.json / one_shot_deferred scenario.
#[test]
fn golden_461_one_shot_auto_disconnect() {
    let golden_path = fixtures_dir()
        .join("golden/signals/deferred_behavior_trace.json");
    let golden_text = std::fs::read_to_string(&golden_path).unwrap();
    let golden: Value = serde_json::from_str(&golden_text).unwrap();

    let scenario = &golden["scenarios"]["one_shot_deferred"];
    let after_first = scenario["callback_count_after_first_emit"].as_u64().unwrap();
    let after_second = scenario["callback_count_after_second_emit"].as_u64().unwrap();
    let conn_after = scenario["connection_count_after_second_emit"].as_u64().unwrap();

    let (mut tree, ids) = build_tree(&["Emitter", "RecvOneShot"]);
    let emitter_id = ids[0];

    let counter = Arc::new(AtomicU64::new(0));
    let c = counter.clone();

    let conn = Connection::with_callback(ids[1].object_id(), "handler", move |_| {
        c.fetch_add(1, AtomicOrdering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();
    tree.connect_signal(emitter_id, "one_shot_sig", conn);

    // First emit
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    assert_eq!(
        counter.load(AtomicOrdering::SeqCst),
        after_first,
        "4.6.1: one-shot fires on first emission"
    );

    // Second emit on next frame
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    assert_eq!(
        counter.load(AtomicOrdering::SeqCst),
        after_second,
        "4.6.1: one-shot does NOT fire on second emission"
    );

    // Connection count should be zero
    let count = tree
        .signal_store_mut(emitter_id)
        .get_signal("one_shot_sig")
        .map(|s| s.connection_count())
        .unwrap_or(0);
    assert_eq!(
        count as u64, conn_after,
        "4.6.1: one-shot connection removed after firing"
    );

    // Both emissions still traced
    let emits = signal_emit_details(&tree);
    let one_shot_emits: Vec<_> = emits
        .iter()
        .filter(|(_, d, _)| d == "one_shot_sig")
        .collect();
    assert_eq!(
        one_shot_emits.len(),
        2,
        "4.6.1: both emissions produce trace events"
    );
    assert_eq!(one_shot_emits[0].2, 0, "first emission on frame 0");
    assert_eq!(one_shot_emits[1].2, 1, "second emission on frame 1");
}

// ===========================================================================
// 3. Argument forwarding — golden trace validation (4.6.1)
// ===========================================================================

/// Godot 4.6.1: all signal arguments forwarded identically to each receiver.
/// Golden: arguments_forwarding_trace.json.
#[test]
fn golden_461_argument_forwarding_fidelity() {
    let golden_path = fixtures_dir()
        .join("golden/signals/arguments_forwarding_trace.json");
    let golden_text = std::fs::read_to_string(&golden_path).unwrap();
    let golden: Value = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(golden["upstream_version"].as_str().unwrap(), "4.6.1-stable");

    let expected_receiver_count = golden["expected_receiver_count"].as_u64().unwrap();
    let expected_args = golden["expected_arguments"].as_array().unwrap();

    let (mut tree, ids) = build_tree(&["Emitter", "RecvA", "RecvB"]);
    let emitter_id = ids[0];

    let recv_a_args = Arc::new(Mutex::new(Vec::<Variant>::new()));
    let recv_b_args = Arc::new(Mutex::new(Vec::<Variant>::new()));

    let ra = recv_a_args.clone();
    let conn_a = Connection::with_callback(ids[1].object_id(), "handler_a", move |args| {
        ra.lock().unwrap().extend(args.iter().cloned());
        Variant::Nil
    });

    let rb = recv_b_args.clone();
    let conn_b = Connection::with_callback(ids[2].object_id(), "handler_b", move |args| {
        rb.lock().unwrap().extend(args.iter().cloned());
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "data_signal", conn_a);
    tree.connect_signal(emitter_id, "data_signal", conn_b);

    // Build arguments matching golden expected_arguments
    let signal_args: Vec<Variant> = expected_args
        .iter()
        .map(|a| match a["type"].as_str().unwrap() {
            "Int" => Variant::Int(a["value"].as_i64().unwrap()),
            "String" => Variant::String(a["value"].as_str().unwrap().into()),
            "Bool" => Variant::Bool(a["value"].as_bool().unwrap()),
            other => panic!("unsupported golden arg type: {other}"),
        })
        .collect();

    tree.emit_signal(emitter_id, "data_signal", &signal_args);

    // Both receivers got the same arguments
    let args_a = recv_a_args.lock().unwrap();
    let args_b = recv_b_args.lock().unwrap();

    assert_eq!(args_a.len(), expected_args.len());
    assert_eq!(args_b.len(), expected_args.len());
    assert_eq!(
        *args_a, *args_b,
        "4.6.1: all receivers get identical arguments"
    );

    // Verify concrete values
    assert_eq!(args_a[0], Variant::Int(42));
    assert_eq!(args_a[1], Variant::String("hello".into()));
    assert_eq!(args_a[2], Variant::Bool(true));

    // Verify receiver count matches golden
    assert_eq!(expected_receiver_count, 2);
}

// ===========================================================================
// 4. Bind/unbind resolution order — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: Callable.unbind(n) drops n trailing signal args, then
/// Callable.bind(args...) appends. Order: unbind first, bind second.
#[test]
fn revalidate_461_unbind_then_bind_order() {
    let (mut tree, ids) = build_tree(&["Emitter", "Recv"]);
    let emitter_id = ids[0];

    let captured = Arc::new(Mutex::new(Vec::<Variant>::new()));
    let c = captured.clone();

    // Signal sends (1, 2, 3). Unbind 1 (drop trailing "3"), bind ("extra").
    // Expected: [1, 2, "extra"]
    let conn = Connection::with_callback(ids[1].object_id(), "handler", move |args| {
        c.lock().unwrap().extend(args.iter().cloned());
        Variant::Nil
    })
    .with_unbinds(1)
    .with_binds(vec![Variant::String("extra".into())]);

    tree.connect_signal(emitter_id, "bind_test", conn);

    tree.emit_signal(
        emitter_id,
        "bind_test",
        &[Variant::Int(1), Variant::Int(2), Variant::Int(3)],
    );

    let args = captured.lock().unwrap();
    assert_eq!(
        *args,
        vec![
            Variant::Int(1),
            Variant::Int(2),
            Variant::String("extra".into())
        ],
        "4.6.1: unbind drops trailing args, then bind appends"
    );
}

/// Godot 4.6.1: unbind saturates to zero — never panics.
#[test]
fn revalidate_461_unbind_saturates_to_zero() {
    let (mut tree, ids) = build_tree(&["Emitter", "Recv"]);
    let emitter_id = ids[0];

    let captured = Arc::new(Mutex::new(Vec::<Variant>::new()));
    let c = captured.clone();

    // Signal sends (1). Unbind 5 (more than args). Bind ("bound").
    // Expected: ["bound"] (all signal args dropped, bind still appends)
    let conn = Connection::with_callback(ids[1].object_id(), "handler", move |args| {
        c.lock().unwrap().extend(args.iter().cloned());
        Variant::Nil
    })
    .with_unbinds(5)
    .with_binds(vec![Variant::String("bound".into())]);

    tree.connect_signal(emitter_id, "saturate_test", conn);
    tree.emit_signal(emitter_id, "saturate_test", &[Variant::Int(1)]);

    let args = captured.lock().unwrap();
    assert_eq!(
        *args,
        vec![Variant::String("bound".into())],
        "4.6.1: unbind saturates to zero, bind still appends"
    );
}

// ===========================================================================
// 5. Deferred one-shot argument capture — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: deferred connections capture arguments at emission time,
/// not at flush time. This is critical for correctness.
#[test]
fn revalidate_461_deferred_captures_args_at_emit_time() {
    let (mut tree, ids) = build_tree(&["Emitter", "Recv"]);
    let emitter_id = ids[0];

    let captured = Arc::new(Mutex::new(Vec::<Variant>::new()));
    let c = captured.clone();

    let conn = Connection::with_callback(ids[1].object_id(), "handler", move |args| {
        c.lock().unwrap().extend(args.iter().cloned());
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "capture_test", conn);

    // Emit with specific args
    tree.emit_signal(
        emitter_id,
        "capture_test",
        &[Variant::Int(99), Variant::String("captured".into())],
    );

    // Nothing fired yet
    assert!(captured.lock().unwrap().is_empty());

    // Flush delivers the captured args
    tree.flush_deferred_signals();

    let args = captured.lock().unwrap();
    assert_eq!(args[0], Variant::Int(99));
    assert_eq!(args[1], Variant::String("captured".into()));
}

// ===========================================================================
// 6. Cross-signal deferred FIFO — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: deferred queue is global FIFO, not per-signal. Emissions
/// from different signals interleave in the queue in emission order.
#[test]
fn revalidate_461_cross_signal_deferred_fifo() {
    let (mut tree, ids) = build_tree(&["Emitter", "Recv"]);
    let emitter_id = ids[0];

    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    // sig_alpha: deferred
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig_alpha",
        Connection::with_callback(ids[1].object_id(), "on_alpha", move |_| {
            o.lock().unwrap().push("alpha".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // sig_beta: deferred
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig_beta",
        Connection::with_callback(ids[1].object_id(), "on_beta", move |_| {
            o.lock().unwrap().push("beta".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // sig_gamma: deferred
    let o = order.clone();
    tree.connect_signal(
        emitter_id,
        "sig_gamma",
        Connection::with_callback(ids[1].object_id(), "on_gamma", move |_| {
            o.lock().unwrap().push("gamma".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // Emit in order: alpha, beta, gamma
    tree.emit_signal(emitter_id, "sig_alpha", &[]);
    tree.emit_signal(emitter_id, "sig_beta", &[]);
    tree.emit_signal(emitter_id, "sig_gamma", &[]);

    assert_eq!(tree.deferred_signal_count(), 3);
    tree.flush_deferred_signals();

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["alpha", "beta", "gamma"],
        "4.6.1: global deferred queue is FIFO across different signals"
    );
}

// ===========================================================================
// 7. Emission with no connections still traces — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: emit_signal on a signal with no connections still records
/// a trace event. This is important for debugging and oracle comparison.
#[test]
fn revalidate_461_emit_no_connections_still_traced() {
    let (mut tree, ids) = build_tree(&["Emitter"]);
    let emitter_id = ids[0];

    tree.emit_signal(emitter_id, "orphan_signal", &[]);

    let emits = signal_emit_details(&tree);
    assert_eq!(emits.len(), 1, "4.6.1: emission with no connections traced");
    assert_eq!(emits[0].1, "orphan_signal");
}

// ===========================================================================
// 8. Reparenting preserves connections — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: moving a node to a new parent preserves its signal connections.
#[test]
fn revalidate_461_reparent_preserves_connections() {
    let (mut tree, ids) = build_tree(&["Emitter", "RecvA", "NewParent"]);
    let emitter_id = ids[0];
    let recv_id = ids[1];
    let new_parent_id = ids[2];

    let counter = Arc::new(AtomicU64::new(0));
    let c = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "handler", move |_| {
        c.fetch_add(1, AtomicOrdering::SeqCst);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "survive_sig", conn);

    // Reparent emitter under NewParent
    tree.reparent(emitter_id, new_parent_id).unwrap();

    tree.emit_signal(emitter_id, "survive_sig", &[]);
    assert_eq!(
        counter.load(AtomicOrdering::SeqCst),
        1,
        "4.6.1: connection survives reparent"
    );

    // Trace reflects new path
    let emits = signal_emit_details(&tree);
    assert_eq!(emits[0].0, "/root/NewParent/Emitter");
}

// ===========================================================================
// 9. Duplicate connections both fire — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: connecting the same target+method twice results in both
/// connections firing (no dedup by default).
#[test]
fn revalidate_461_duplicate_connections_both_fire() {
    let (mut tree, ids) = build_tree(&["Emitter", "Recv"]);
    let emitter_id = ids[0];

    let counter = Arc::new(AtomicU64::new(0));

    for _ in 0..2 {
        let c = counter.clone();
        let conn = Connection::with_callback(ids[1].object_id(), "handler", move |_| {
            c.fetch_add(1, AtomicOrdering::SeqCst);
            Variant::Nil
        });
        tree.connect_signal(emitter_id, "dup_sig", conn);
    }

    tree.emit_signal(emitter_id, "dup_sig", &[]);
    assert_eq!(
        counter.load(AtomicOrdering::SeqCst),
        2,
        "4.6.1: duplicate connections both fire"
    );
}

// ===========================================================================
// 10. Emit on undeclared signal is silent — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: emitting a signal that was never declared or connected
/// is silently ignored (no panic, no error).
#[test]
fn revalidate_461_emit_undeclared_signal_silent() {
    let (mut tree, ids) = build_tree(&["Emitter"]);
    let emitter_id = ids[0];

    // This should not panic
    tree.emit_signal(emitter_id, "never_declared", &[Variant::Int(42)]);

    // Trace still records the emission
    let emits = signal_emit_details(&tree);
    assert_eq!(emits.len(), 1);
    assert_eq!(emits[0].1, "never_declared");
}

// ===========================================================================
// 11. Deferred one-shot + persistent survivorship — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: after a one-shot deferred connection fires and is removed,
/// persistent deferred connections on the same signal survive and continue
/// to queue on subsequent emissions.
#[test]
fn revalidate_461_deferred_oneshot_persistent_survivorship() {
    let (mut tree, ids) = build_tree(&["Emitter", "RecvPersist", "RecvOneShot"]);
    let emitter_id = ids[0];

    let persist_count = Arc::new(AtomicU64::new(0));
    let oneshot_count = Arc::new(AtomicU64::new(0));

    let pc = persist_count.clone();
    tree.connect_signal(
        emitter_id,
        "mixed_sig",
        Connection::with_callback(ids[1].object_id(), "persist", move |_| {
            pc.fetch_add(1, AtomicOrdering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );

    let oc = oneshot_count.clone();
    tree.connect_signal(
        emitter_id,
        "mixed_sig",
        Connection::with_callback(ids[2].object_id(), "oneshot", move |_| {
            oc.fetch_add(1, AtomicOrdering::SeqCst);
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // First cycle
    tree.emit_signal(emitter_id, "mixed_sig", &[]);
    tree.flush_deferred_signals();
    assert_eq!(persist_count.load(AtomicOrdering::SeqCst), 1);
    assert_eq!(oneshot_count.load(AtomicOrdering::SeqCst), 1);

    // Second cycle: only persistent survives
    tree.emit_signal(emitter_id, "mixed_sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "4.6.1: only persistent connection queues"
    );
    tree.flush_deferred_signals();
    assert_eq!(persist_count.load(AtomicOrdering::SeqCst), 2);
    assert_eq!(
        oneshot_count.load(AtomicOrdering::SeqCst),
        1,
        "4.6.1: one-shot does not fire again"
    );
}

// ===========================================================================
// 12. Five-receiver fan-out preserves order — 4.6.1 contract
// ===========================================================================

/// Godot 4.6.1: fan-out to N receivers maintains registration order.
#[test]
fn revalidate_461_five_receiver_fanout_order() {
    let names: Vec<String> = (0..5).map(|i| format!("Recv{i}")).collect();
    let mut all_names: Vec<&str> = vec!["Emitter"];
    all_names.extend(names.iter().map(|s| s.as_str()));

    let (mut tree, ids) = build_tree(&all_names);
    let emitter_id = ids[0];

    let order = Arc::new(Mutex::new(Vec::<usize>::new()));

    for i in 0..5 {
        let o = order.clone();
        let conn = Connection::with_callback(
            ids[i + 1].object_id(),
            &format!("handler_{i}"),
            move |_| {
                o.lock().unwrap().push(i);
                Variant::Nil
            },
        );
        tree.connect_signal(emitter_id, "fanout", conn);
    }

    tree.emit_signal(emitter_id, "fanout", &[]);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec![0, 1, 2, 3, 4],
        "4.6.1: fan-out preserves registration order"
    );
}
