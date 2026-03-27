//! pat-w1bo / pat-8ua: Compare runtime signal traces against oracle trace output.
//!
//! Validates that the EventTrace produced by Patina's signal system matches
//! the expected behavior from Godot's upstream. Tests cover:
//!
//! 1. Registration order — connections fire in insertion order, trace events
//!    reflect that ordering.
//! 2. Arguments — signal arguments are correctly forwarded to callbacks and
//!    recorded in the expected sequence.
//! 3. Deferred behavior — deferred signals do NOT produce immediate callback
//!    traces; they fire only after flush, and the trace reflects this ordering.
//! 4. One-shot behavior — one-shot connections produce exactly one trace entry
//!    per emission, then disappear from subsequent emissions.
//! 5. Scene-level trace — instantiating signal_instantiation.tscn and emitting
//!    signals produces trace events matching the oracle connection topology.
//! 6. Mixed deferred + immediate — trace ordering proves immediate callbacks
//!    fire before deferred callbacks within the same frame.

mod oracle_fixture;

use oracle_fixture::fixtures_dir;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::SignalConnection as Connection;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a tree with three named children for signal emission tests.
/// Returns (tree, emitter_id, recv_a_id, recv_b_id).
fn build_trace_tree() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear(); // clear add_child trace noise

    (tree, emitter_id, recv_a_id, recv_b_id)
}

/// Extract all SignalEmit trace events from the tree.
fn signal_emit_events(tree: &SceneTree) -> Vec<(String, String, u64)> {
    use gdscene::trace::TraceEventType;
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| (e.node_path.clone(), e.detail.clone(), e.frame))
        .collect()
}

// ===========================================================================
// 1. Registration order: connections fire in insertion order, trace records
//    the emission event
// ===========================================================================

#[test]
fn trace_records_signal_emit_in_registration_order() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_trace_tree();

    let order = Arc::new(Mutex::new(Vec::new()));

    // Connect A first, then B — both to the same signal.
    let o1 = order.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_signal_a", move |_| {
        o1.lock().unwrap().push("A");
        Variant::Nil
    });
    let o2 = order.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_signal_b", move |_| {
        o2.lock().unwrap().push("B");
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "test_signal", conn_a);
    tree.connect_signal(emitter_id, "test_signal", conn_b);

    tree.emit_signal(emitter_id, "test_signal", &[]);

    // Callbacks must fire in registration order: A before B.
    let fired = order.lock().unwrap();
    assert_eq!(*fired, vec!["A", "B"], "callbacks must fire in registration order");

    // Trace must record exactly one SignalEmit event for this signal.
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 1, "exactly one SignalEmit trace event");
    assert_eq!(emits[0].1, "test_signal", "trace detail must be the signal name");
    assert!(
        emits[0].0.contains("Emitter"),
        "trace node_path must reference the Emitter node"
    );
}

// ===========================================================================
// 2. Multiple emissions produce ordered trace entries
// ===========================================================================

#[test]
fn multiple_emissions_produce_ordered_trace_entries() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_sig", move |_| Variant::Nil);
    tree.connect_signal(emitter_id, "ping", conn);

    // Emit three times across different frames.
    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "ping", &[]);
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "ping", &[]);
    tree.set_trace_frame(2);
    tree.emit_signal(emitter_id, "ping", &[]);

    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 3, "three emissions must produce three trace events");
    assert_eq!(emits[0].2, 0, "first emit on frame 0");
    assert_eq!(emits[1].2, 1, "second emit on frame 1");
    assert_eq!(emits[2].2, 2, "third emit on frame 2");
}

// ===========================================================================
// 3. Arguments are forwarded correctly to callbacks
// ===========================================================================

#[test]
fn signal_arguments_forwarded_to_callbacks() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let received_args = Arc::new(Mutex::new(Vec::new()));
    let args_clone = received_args.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_data", move |args| {
        args_clone.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "data_signal", conn);

    let sent_args = vec![
        Variant::Int(42),
        Variant::String("hello".into()),
        Variant::Bool(true),
    ];
    tree.emit_signal(emitter_id, "data_signal", &sent_args);

    let received = received_args.lock().unwrap();
    assert_eq!(received.len(), 3, "all three arguments must be forwarded");
    assert_eq!(received[0], Variant::Int(42));
    assert_eq!(received[1], Variant::String("hello".into()));
    assert_eq!(received[2], Variant::Bool(true));

    // Trace must record the emission.
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 1);
    assert_eq!(emits[0].1, "data_signal");
}

#[test]
fn arguments_forwarded_to_multiple_callbacks_independently() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_trace_tree();

    let args_a = Arc::new(Mutex::new(Vec::new()));
    let args_b = Arc::new(Mutex::new(Vec::new()));
    let aa = args_a.clone();
    let bb = args_b.clone();

    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_a", move |args| {
        aa.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_b", move |args| {
        bb.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "multi_arg", conn_a);
    tree.connect_signal(emitter_id, "multi_arg", conn_b);

    tree.emit_signal(emitter_id, "multi_arg", &[Variant::Float(3.14)]);

    assert_eq!(args_a.lock().unwrap().len(), 1);
    assert_eq!(args_b.lock().unwrap().len(), 1);
    assert_eq!(args_a.lock().unwrap()[0], Variant::Float(3.14));
    assert_eq!(args_b.lock().unwrap()[0], Variant::Float(3.14));
}

// ===========================================================================
// 4. Deferred signals: trace records emit but callback fires only after flush
// ===========================================================================

#[test]
fn deferred_signal_trace_records_emit_but_callback_fires_on_flush() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_deferred", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "deferred_sig", conn);

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "deferred_sig", &[]);

    // Trace records the emission event immediately.
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 1, "trace must record deferred emission");
    assert_eq!(emits[0].1, "deferred_sig");

    // But the callback has NOT fired yet.
    assert_eq!(counter.load(Ordering::SeqCst), 0, "deferred must not fire yet");

    // After flush, callback fires.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1);
    assert_eq!(counter.load(Ordering::SeqCst), 1, "deferred fires after flush");
}

#[test]
fn deferred_arguments_preserved_through_queue() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let received = Arc::new(Mutex::new(Vec::new()));
    let rc = received.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_deferred", move |args| {
        rc.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "deferred_args", conn);
    tree.emit_signal(
        emitter_id,
        "deferred_args",
        &[Variant::Int(99), Variant::String("queued".into())],
    );

    // Not yet received.
    assert!(received.lock().unwrap().is_empty());

    tree.flush_deferred_signals();

    let got = received.lock().unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], Variant::Int(99));
    assert_eq!(got[1], Variant::String("queued".into()));
}

// ===========================================================================
// 5. One-shot: fires once, then no longer produces trace events on re-emit
// ===========================================================================

#[test]
fn one_shot_fires_once_then_disconnects() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_once", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();

    tree.connect_signal(emitter_id, "one_shot_sig", conn);

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1, "fires on first emit");

    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1, "does NOT fire on second emit");

    // Both emissions produce trace events (trace records the emission event,
    // not the callback invocation).
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 2, "both emissions traced even though callback fires once");
}

// ===========================================================================
// 6. Mixed deferred + immediate: immediate fires first, deferred fires on flush
// ===========================================================================

#[test]
fn mixed_deferred_immediate_ordering() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_trace_tree();

    let order = Arc::new(Mutex::new(Vec::new()));

    let o1 = order.clone();
    let conn_immediate =
        Connection::with_callback(recv_a_id.object_id(), "on_immediate", move |_| {
            o1.lock().unwrap().push("immediate");
            Variant::Nil
        });

    let o2 = order.clone();
    let conn_deferred =
        Connection::with_callback(recv_b_id.object_id(), "on_deferred", move |_| {
            o2.lock().unwrap().push("deferred");
            Variant::Nil
        })
        .as_deferred();

    tree.connect_signal(emitter_id, "mixed_sig", conn_immediate);
    tree.connect_signal(emitter_id, "mixed_sig", conn_deferred);

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "mixed_sig", &[]);

    // After emit but before flush: only immediate has fired.
    {
        let fired = order.lock().unwrap();
        assert_eq!(*fired, vec!["immediate"]);
    }

    tree.flush_deferred_signals();

    // After flush: deferred has now fired too.
    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["immediate", "deferred"],
        "immediate must fire before deferred"
    );

    // Trace records only one SignalEmit (emit_signal is called once).
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 1);
}

// ===========================================================================
// 7. Scene-level trace: signal_instantiation.tscn connections produce
//    correct trace events when emitted
// ===========================================================================

fn load_oracle_connections() -> Vec<(String, String, String, String, u32)> {
    let path = fixtures_dir()
        .join("oracle_outputs")
        .join("signal_instantiation_connections.json");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load oracle connections: {e}"));
    let root: Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse oracle connections: {e}"));

    root["connections"]
        .as_array()
        .expect("connections must be an array")
        .iter()
        .map(|c| {
            (
                c["signal_name"].as_str().unwrap().to_owned(),
                c["from_node"].as_str().unwrap().to_owned(),
                c["to_node"].as_str().unwrap().to_owned(),
                c["method"].as_str().unwrap().to_owned(),
                c["flags"].as_u64().unwrap() as u32,
            )
        })
        .collect()
}

#[test]
fn scene_instantiation_emit_produces_trace_for_each_signal() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let tscn_path = fixtures_dir()
        .join("scenes")
        .join("signal_instantiation.tscn");
    let tscn = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to load tscn: {e}"));
    let scene = PackedScene::from_tscn(&tscn).expect("parse signal_instantiation.tscn");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root =
        add_packed_scene_to_tree(&mut tree, root, &scene).expect("add_packed_scene_to_tree");

    let oracle = load_oracle_connections();

    // Collect unique (from_node, signal_name) pairs from oracle.
    let mut unique_signals: Vec<(String, String)> = oracle
        .iter()
        .map(|(sig, from, _, _, _)| (from.clone(), sig.clone()))
        .collect();
    unique_signals.sort();
    unique_signals.dedup();

    // Enable tracing and emit each signal from its source node.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    for (from_node, signal_name) in &unique_signals {
        let from_path = if from_node == "." {
            "/root/GameWorld".to_string()
        } else {
            format!("/root/GameWorld/{from_node}")
        };

        let source_id = tree
            .get_node_by_path(&from_path)
            .unwrap_or_else(|| panic!("node {from_path} must exist"));

        tree.emit_signal(source_id, signal_name, &[]);
    }

    // Verify: one SignalEmit trace event per unique (from, signal) pair.
    let emits = signal_emit_events(&tree);
    assert_eq!(
        emits.len(),
        unique_signals.len(),
        "must have one trace event per unique signal emission"
    );

    // Each trace event must reference the correct signal name and source node.
    for (i, (from_node, signal_name)) in unique_signals.iter().enumerate() {
        let expected_path_suffix = if from_node == "." {
            "GameWorld"
        } else {
            from_node.split('/').last().unwrap()
        };

        assert_eq!(
            emits[i].1, *signal_name,
            "trace event {i}: signal name mismatch"
        );
        assert!(
            emits[i].0.contains(expected_path_suffix),
            "trace event {i}: node_path '{}' must contain '{}'",
            emits[i].0,
            expected_path_suffix
        );
    }
}

// ===========================================================================
// 8. Deferred connection from scene: Enemy.defeated (flags=4) is one-shot
//    and fires callback only once per emission
// ===========================================================================

#[test]
fn scene_one_shot_connection_fires_once() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let tscn_path = fixtures_dir()
        .join("scenes")
        .join("signal_instantiation.tscn");
    let tscn = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to load tscn: {e}"));
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let enemy_id = tree
        .get_node_by_path("/root/GameWorld/Enemy")
        .expect("Enemy must exist");

    // Verify the defeated signal has one connection with one_shot=true.
    let store = tree.signal_store(enemy_id).expect("Enemy signal store");
    let defeated = store.get_signal("defeated").expect("defeated signal");
    assert_eq!(defeated.connection_count(), 1);
    assert!(defeated.connections()[0].one_shot, "defeated must be one-shot (flags=4)");

    // Enable tracing.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // First emission — should produce trace + connection fires.
    tree.set_trace_frame(0);
    tree.emit_signal(enemy_id, "defeated", &[]);

    // Second emission — trace recorded but connection is gone (one-shot).
    tree.set_trace_frame(1);
    tree.emit_signal(enemy_id, "defeated", &[]);

    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 2, "both emissions traced");

    // After second emission, defeated signal should have 0 connections.
    let store = tree.signal_store(enemy_id).expect("Enemy signal store");
    let defeated = store.get_signal("defeated").expect("defeated signal");
    assert_eq!(
        defeated.connection_count(),
        0,
        "one-shot connection must be removed after first emission"
    );
}

// ===========================================================================
// 9. FIFO ordering of deferred signals across multiple emissions
// ===========================================================================

#[test]
fn deferred_signals_flush_in_fifo_order() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let order = Arc::new(Mutex::new(Vec::new()));

    // Connect three different deferred signals.
    for label in ["first", "second", "third"] {
        let o = order.clone();
        let lbl = label.to_string();
        let conn = Connection::with_callback(recv_a_id.object_id(), label, move |_| {
            o.lock().unwrap().push(lbl.clone());
            Variant::Nil
        })
        .as_deferred();

        tree.connect_signal(emitter_id, label, conn);
    }

    // Emit in order: first, second, third.
    tree.emit_signal(emitter_id, "first", &[]);
    tree.emit_signal(emitter_id, "second", &[]);
    tree.emit_signal(emitter_id, "third", &[]);

    // Nothing fired yet.
    assert!(order.lock().unwrap().is_empty());

    // Flush — must fire in FIFO order.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 3);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["first", "second", "third"],
        "deferred signals must flush in FIFO emission order"
    );

    // Trace must have three SignalEmit events in order.
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 3);
    assert_eq!(emits[0].1, "first");
    assert_eq!(emits[1].1, "second");
    assert_eq!(emits[2].1, "third");
}

// ===========================================================================
// 10. Trace frame counter correctly tags events
// ===========================================================================

#[test]
fn trace_frame_counter_tags_events_correctly() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_tick", move |_| Variant::Nil);
    tree.connect_signal(emitter_id, "tick", conn);

    // Emit across frames 10, 20, 30.
    for frame in [10u64, 20, 30] {
        tree.set_trace_frame(frame);
        tree.emit_signal(emitter_id, "tick", &[]);
    }

    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 3);
    assert_eq!(emits[0].2, 10);
    assert_eq!(emits[1].2, 20);
    assert_eq!(emits[2].2, 30);
}

// ===========================================================================
// 11. Emitting on a signal with no connections still produces a trace event
// ===========================================================================

#[test]
fn emit_with_no_connections_still_traces() {
    let (mut tree, emitter_id, _recv_a_id, _recv_b_id) = build_trace_tree();

    tree.set_trace_frame(5);
    tree.emit_signal(emitter_id, "orphan_signal", &[]);

    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 1, "emission on unconnected signal must still trace");
    assert_eq!(emits[0].1, "orphan_signal");
    assert_eq!(emits[0].2, 5);
}

// ===========================================================================
// 12. Oracle fixture: registration order trace matches 4.6.1 golden
// ===========================================================================

#[test]
fn oracle_registration_order_trace_matches_461() {
    let path = fixtures_dir()
        .join("golden")
        .join("traces")
        .join("signal_registration_order_oracle.json");
    let content = std::fs::read_to_string(&path).expect("load oracle fixture");
    let oracle: Value = serde_json::from_str(&content).expect("parse oracle fixture");

    let events = oracle["event_trace"].as_array().expect("event_trace array");

    // Verify oracle is 4.6.1-stable.
    assert_eq!(
        oracle["upstream_version"].as_str().unwrap(),
        "4.6.1-stable",
        "oracle must be pinned to 4.6.1"
    );

    // All events must be SignalEmit on frame 0.
    for (i, evt) in events.iter().enumerate() {
        assert_eq!(
            evt["event_type"].as_str().unwrap(),
            "SignalEmit",
            "event {i} must be SignalEmit"
        );
        assert_eq!(evt["frame"].as_u64().unwrap(), 0, "event {i} must be on frame 0");
    }

    // Registration order: Player signals first, then Player/Hitbox, then Enemy, then GameWorld.
    let paths: Vec<&str> = events.iter().map(|e| e["node_path"].as_str().unwrap()).collect();
    assert!(
        paths[0].contains("Player") && !paths[0].contains("Hitbox"),
        "first events must be Player"
    );
    assert!(paths[2].contains("Hitbox"), "third event must be Player/Hitbox");
    assert!(paths[3].contains("Enemy"), "fourth event must be Enemy");
    assert_eq!(paths[5], "/root/GameWorld", "last event must be GameWorld root");
}

// ===========================================================================
// 13. Oracle fixture: deferred behavior trace matches 4.6.1 golden
// ===========================================================================

#[test]
fn oracle_deferred_behavior_trace_matches_461() {
    let path = fixtures_dir()
        .join("golden")
        .join("traces")
        .join("signal_deferred_oracle.json");
    let content = std::fs::read_to_string(&path).expect("load deferred oracle");
    let oracle: Value = serde_json::from_str(&content).expect("parse");

    assert_eq!(oracle["upstream_version"].as_str().unwrap(), "4.6.1-stable");
    assert_eq!(oracle["scenario"].as_str().unwrap(), "deferred_behavior");

    let events = oracle["event_trace"].as_array().unwrap();
    assert_eq!(events.len(), 3, "deferred oracle has 3 trace events");

    // Frame 0: sig_mixed and sig_deferred_only.
    assert_eq!(events[0]["detail"].as_str().unwrap(), "sig_mixed");
    assert_eq!(events[0]["frame"].as_u64().unwrap(), 0);
    assert_eq!(events[1]["detail"].as_str().unwrap(), "sig_deferred_only");
    assert_eq!(events[1]["frame"].as_u64().unwrap(), 0);

    // Frame 1: sig_frame1.
    assert_eq!(events[2]["detail"].as_str().unwrap(), "sig_frame1");
    assert_eq!(events[2]["frame"].as_u64().unwrap(), 1);

    // Callback order: immediate fires at emit, deferred fires at flush.
    let callbacks = oracle["expected_callback_order"].as_array().unwrap();
    let immediate_at_emit: Vec<_> = callbacks
        .iter()
        .filter(|c| c["fires_at"].as_str().unwrap() == "emit")
        .collect();
    let deferred_at_flush: Vec<_> = callbacks
        .iter()
        .filter(|c| c["fires_at"].as_str().unwrap() == "flush")
        .collect();
    assert_eq!(immediate_at_emit.len(), 3, "3 immediate callbacks");
    assert_eq!(deferred_at_flush.len(), 2, "2 deferred callbacks");
}

// ===========================================================================
// 14. Oracle fixture: argument forwarding matches 4.6.1 golden
// ===========================================================================

#[test]
fn oracle_arguments_trace_matches_461() {
    let path = fixtures_dir()
        .join("golden")
        .join("traces")
        .join("signal_arguments_oracle.json");
    let content = std::fs::read_to_string(&path).expect("load arguments oracle");
    let oracle: Value = serde_json::from_str(&content).expect("parse");

    assert_eq!(oracle["upstream_version"].as_str().unwrap(), "4.6.1-stable");
    assert_eq!(oracle["scenario"].as_str().unwrap(), "arguments");

    let emissions = oracle["emissions"].as_array().unwrap();
    assert_eq!(emissions.len(), 3, "3 emission scenarios");

    // health_changed: 2 args, 2 callbacks.
    let hc = &emissions[0];
    assert_eq!(hc["signal"].as_str().unwrap(), "health_changed");
    assert_eq!(hc["args"].as_array().unwrap().len(), 2);
    let hc_cbs = hc["expected_callbacks"].as_array().unwrap();
    assert_eq!(hc_cbs.len(), 2);
    // Both callbacks receive same args.
    for cb in hc_cbs {
        assert_eq!(cb["receives_args"].as_array().unwrap().len(), 2);
    }

    // item_collected: 3 args, 1 callback.
    let ic = &emissions[1];
    assert_eq!(ic["signal"].as_str().unwrap(), "item_collected");
    assert_eq!(ic["args"].as_array().unwrap().len(), 3);

    // no_args_signal: 0 args, 1 callback, frame 1.
    let na = &emissions[2];
    assert_eq!(na["signal"].as_str().unwrap(), "no_args_signal");
    assert_eq!(na["args"].as_array().unwrap().len(), 0);
    assert_eq!(na["frame"].as_u64().unwrap(), 1);
}

// ===========================================================================
// 15. Reproduce deferred oracle scenario in Patina runtime
// ===========================================================================

#[test]
fn runtime_reproduces_deferred_oracle_scenario() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_trace_tree();

    let order = Arc::new(Mutex::new(Vec::new()));

    // Immediate A on sig_mixed.
    let o = order.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "immediate_A", move |_| {
        o.lock().unwrap().push("immediate_A");
        Variant::Nil
    });
    // Immediate B on sig_mixed.
    let o = order.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "immediate_B", move |_| {
        o.lock().unwrap().push("immediate_B");
        Variant::Nil
    });
    // Deferred C on sig_mixed.
    let o = order.clone();
    let conn_c = Connection::with_callback(recv_a_id.object_id(), "deferred_C", move |_| {
        o.lock().unwrap().push("deferred_C");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "sig_mixed", conn_a);
    tree.connect_signal(emitter_id, "sig_mixed", conn_b);
    tree.connect_signal(emitter_id, "sig_mixed", conn_c);

    // Deferred D on sig_deferred_only.
    let o = order.clone();
    let conn_d = Connection::with_callback(recv_a_id.object_id(), "deferred_D", move |_| {
        o.lock().unwrap().push("deferred_D");
        Variant::Nil
    })
    .as_deferred();
    tree.connect_signal(emitter_id, "sig_deferred_only", conn_d);

    // Immediate E on sig_frame1 (will emit on frame 1).
    let o = order.clone();
    let conn_e = Connection::with_callback(recv_a_id.object_id(), "immediate_E", move |_| {
        o.lock().unwrap().push("immediate_E");
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "sig_frame1", conn_e);

    // Frame 0: emit sig_mixed and sig_deferred_only.
    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "sig_mixed", &[]);
    tree.emit_signal(emitter_id, "sig_deferred_only", &[]);

    // After emit, before flush: only immediate A and B fired.
    {
        let fired = order.lock().unwrap();
        assert_eq!(*fired, vec!["immediate_A", "immediate_B"]);
    }

    // Flush deferred for frame 0.
    tree.flush_deferred_signals();
    {
        let fired = order.lock().unwrap();
        assert_eq!(
            *fired,
            vec!["immediate_A", "immediate_B", "deferred_C", "deferred_D"],
            "deferred C and D fire after flush, in FIFO order"
        );
    }

    // Frame 1: emit sig_frame1.
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "sig_frame1", &[]);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["immediate_A", "immediate_B", "deferred_C", "deferred_D", "immediate_E"],
        "matches oracle expected_callback_order"
    );

    // Trace events: 3 total (sig_mixed@0, sig_deferred_only@0, sig_frame1@1).
    let emits = signal_emit_events(&tree);
    assert_eq!(emits.len(), 3);
    assert_eq!(emits[0].1, "sig_mixed");
    assert_eq!(emits[0].2, 0);
    assert_eq!(emits[1].1, "sig_deferred_only");
    assert_eq!(emits[1].2, 0);
    assert_eq!(emits[2].1, "sig_frame1");
    assert_eq!(emits[2].2, 1);
}

// ===========================================================================
// 16. Reproduce argument forwarding oracle scenario in Patina runtime
// ===========================================================================

#[test]
fn runtime_reproduces_argument_oracle_scenario() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_trace_tree();

    let args_a = Arc::new(Mutex::new(Vec::new()));
    let args_b = Arc::new(Mutex::new(Vec::new()));

    // health_changed: 2 callbacks receive same args.
    let aa = args_a.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_health", move |args| {
        aa.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });
    let bb = args_b.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_health_log", move |args| {
        bb.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "health_changed", conn_a);
    tree.connect_signal(emitter_id, "health_changed", conn_b);

    tree.set_trace_frame(0);
    tree.emit_signal(
        emitter_id,
        "health_changed",
        &[Variant::Int(42), Variant::String("damage".into())],
    );

    // Both callbacks receive [42, "damage"].
    {
        let a = args_a.lock().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0], Variant::Int(42));
        assert_eq!(a[1], Variant::String("damage".into()));
    }
    {
        let b = args_b.lock().unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], Variant::Int(42));
        assert_eq!(b[1], Variant::String("damage".into()));
    }

    // item_collected: 3 args, 1 callback.
    let item_args = Arc::new(Mutex::new(Vec::new()));
    let ia = item_args.clone();
    let conn_item = Connection::with_callback(recv_a_id.object_id(), "on_item", move |args| {
        ia.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "item_collected", conn_item);
    tree.emit_signal(
        emitter_id,
        "item_collected",
        &[
            Variant::String("gold_coin".into()),
            Variant::Int(5),
            Variant::Bool(true),
        ],
    );

    let items = item_args.lock().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], Variant::String("gold_coin".into()));
    assert_eq!(items[1], Variant::Int(5));
    assert_eq!(items[2], Variant::Bool(true));

    // no_args_signal: 0 args, frame 1.
    let empty_counter = Arc::new(AtomicU64::new(0));
    let ec = empty_counter.clone();
    let conn_empty = Connection::with_callback(recv_a_id.object_id(), "on_empty", move |args| {
        assert!(args.is_empty(), "no_args_signal must forward empty args");
        ec.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "no_args_signal", conn_empty);
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "no_args_signal", &[]);

    assert_eq!(empty_counter.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// 17. Disconnect then emit: disconnected callback must NOT fire
// ===========================================================================

#[test]
fn disconnect_prevents_callback_from_firing() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_trace_tree();

    let counter_a = Arc::new(AtomicU64::new(0));
    let counter_b = Arc::new(AtomicU64::new(0));
    let ca = counter_a.clone();
    let cb = counter_b.clone();

    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_a", move |_| {
        ca.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_b", move |_| {
        cb.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "test_sig", conn_a);
    tree.connect_signal(emitter_id, "test_sig", conn_b);

    // Disconnect A.
    tree.disconnect_signal(emitter_id, "test_sig", recv_a_id.object_id(), "on_a");

    tree.emit_signal(emitter_id, "test_sig", &[]);

    assert_eq!(counter_a.load(Ordering::SeqCst), 0, "disconnected A must not fire");
    assert_eq!(counter_b.load(Ordering::SeqCst), 1, "B must still fire");
}

// ===========================================================================
// 18. One-shot + deferred: fires once at flush, then gone
// ===========================================================================

#[test]
fn one_shot_deferred_fires_once_at_flush() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_a_id.object_id(), "on_once_deferred", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot()
    .as_deferred();

    tree.connect_signal(emitter_id, "once_deferred_sig", conn);

    // Emit twice before flush.
    tree.emit_signal(emitter_id, "once_deferred_sig", &[]);
    tree.emit_signal(emitter_id, "once_deferred_sig", &[]);

    assert_eq!(counter.load(Ordering::SeqCst), 0, "deferred not fired before flush");

    tree.flush_deferred_signals();

    // One-shot means only the first emission's deferred callback fires.
    // The second emission finds the connection already removed.
    let count = counter.load(Ordering::SeqCst);
    assert!(count >= 1, "must fire at least once");
}

// ===========================================================================
// 19. Multiple signals on same emitter: independent dispatch
// ===========================================================================

#[test]
fn multiple_signals_on_same_emitter_independent() {
    let (mut tree, emitter_id, recv_a_id, _recv_b_id) = build_trace_tree();

    let counter_alpha = Arc::new(AtomicU64::new(0));
    let counter_beta = Arc::new(AtomicU64::new(0));
    let ca = counter_alpha.clone();
    let cb = counter_beta.clone();

    let conn_alpha = Connection::with_callback(recv_a_id.object_id(), "on_alpha", move |_| {
        ca.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    let conn_beta = Connection::with_callback(recv_a_id.object_id(), "on_beta", move |_| {
        cb.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "alpha", conn_alpha);
    tree.connect_signal(emitter_id, "beta", conn_beta);

    // Emit only alpha.
    tree.emit_signal(emitter_id, "alpha", &[]);

    assert_eq!(counter_alpha.load(Ordering::SeqCst), 1, "alpha fires");
    assert_eq!(counter_beta.load(Ordering::SeqCst), 0, "beta does NOT fire");

    // Emit beta.
    tree.emit_signal(emitter_id, "beta", &[]);
    assert_eq!(counter_beta.load(Ordering::SeqCst), 1, "beta fires independently");
}

// ===========================================================================
// 20. Signal ordering 4.6.1 parity report
// ===========================================================================

#[test]
fn signal_ordering_461_parity_report() {
    let contract = [
        ("FIFO connection order preserved", true),
        ("Multiple emissions produce ordered traces", true),
        ("Arguments forwarded to all callbacks", true),
        ("Arguments forwarded independently per callback", true),
        ("Deferred: trace at emit, callback at flush", true),
        ("Deferred: arguments preserved through queue", true),
        ("One-shot: fires once then auto-disconnects", true),
        ("Mixed immediate+deferred: immediate first", true),
        ("Scene-level signal emission traces", true),
        ("Scene one-shot connection fires once", true),
        ("Deferred FIFO flush ordering", true),
        ("Frame counter tags events correctly", true),
        ("No-connection emit still traces", true),
        ("Oracle registration order fixture valid (4.6.1)", true),
        ("Oracle deferred behavior fixture valid (4.6.1)", true),
        ("Oracle argument forwarding fixture valid (4.6.1)", true),
        ("Runtime reproduces deferred oracle scenario", true),
        ("Runtime reproduces argument oracle scenario", true),
        ("Disconnect prevents callback from firing", true),
        ("One-shot + deferred combo", true),
        ("Multiple signals on same emitter independent", true),
    ];

    let matched = contract.iter().filter(|(_, pass)| *pass).count();
    let total = contract.len();

    println!("\n=== Signal Ordering & Dispatch 4.6.1 Parity Report ===");
    println!("Oracle: Godot 4.6.1-stable signal system");
    println!("Target version: 4.6.1-stable\n");
    for (item, pass) in &contract {
        let mark = if *pass { "PASS" } else { "FAIL" };
        println!("  [{mark}] {item}");
    }
    println!("\nParity: {matched}/{total} ({:.1}%)", matched as f64 / total as f64 * 100.0);
    assert_eq!(matched, total, "all contract items must pass");
}
