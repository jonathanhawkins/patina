//! pat-guwy: Compare runtime signal traces against oracle trace fixtures.
//!
//! These tests load golden signal trace fixtures from
//! `fixtures/golden/signals/` and verify that Patina's runtime signal
//! system produces trace events matching the oracle-derived expectations.
//!
//! Coverage:
//! 1. Registration order — callbacks fire in insertion order, trace matches fixture
//! 2. Arguments forwarding — all Variant args reach each callback, trace matches
//! 3. Deferred behavior — deferred queuing, FIFO flush, mixed immediate+deferred
//! 4. One-shot behavior — fires once then auto-disconnects, trace matches fixture
//! 5. Scene-level oracle comparison — signal_instantiation.tscn connections
//!    produce traces matching oracle topology

mod oracle_fixture;
mod trace_compare;

use oracle_fixture::fixtures_dir;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::SignalConnection as Connection;
use gdscene::trace::TraceEventType;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn load_signal_fixture(name: &str) -> Value {
    let path = fixtures_dir().join("golden/signals").join(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load signal fixture {name}: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse signal fixture {name}: {e}"))
}

fn build_trace_tree_3() -> (SceneTree, NodeId, NodeId, NodeId, NodeId) {
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
    tree.event_trace_mut().clear();

    (tree, emitter_id, recv_a_id, recv_b_id, recv_c_id)
}

fn signal_emit_events(tree: &SceneTree) -> Vec<trace_compare::TraceEvent> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| trace_compare::TraceEvent {
            event_type: "signal_emit".to_string(),
            node_path: e.node_path.clone(),
            detail: e.detail.clone(),
            frame: e.frame,
        })
        .collect()
}

fn parse_fixture_events(fixture: &Value, path: &str) -> Vec<trace_compare::TraceEvent> {
    let trace = if path.is_empty() {
        &fixture["event_trace"]
    } else {
        // Navigate dotted path like "scenarios.deferred_only.event_trace"
        let mut v = fixture;
        for part in path.split('.') {
            v = &v[part];
        }
        v
    };
    trace_compare::parse_events(trace)
}

// ===========================================================================
// 1. Registration order: fixture says one SignalEmit, callbacks fire A,B,C
// ===========================================================================

#[test]
fn fixture_registration_order_trace_matches() {
    let fixture = load_signal_fixture("registration_order_trace.json");
    let expected = parse_fixture_events(&fixture, "");

    let (mut tree, emitter_id, recv_a_id, recv_b_id, recv_c_id) = build_trace_tree_3();
    let order = Arc::new(Mutex::new(Vec::new()));

    for (id, label) in [(recv_a_id, "RecvA"), (recv_b_id, "RecvB"), (recv_c_id, "RecvC")] {
        let o = order.clone();
        let lbl = label.to_string();
        let conn = Connection::with_callback(id.object_id(), label, move |_| {
            o.lock().unwrap().push(lbl.clone());
            Variant::Nil
        });
        tree.connect_signal(emitter_id, "ordered_signal", conn);
    }

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "ordered_signal", &[]);

    // Compare trace events against fixture.
    let actual = signal_emit_events(&tree);
    let diffs = trace_compare::compare_traces(&expected, &actual);
    let report = trace_compare::format_report("fixture", "runtime", &expected, &actual, &diffs);
    assert!(diffs.is_empty(), "registration order trace mismatch:\n{report}");

    // Verify callback ordering matches fixture expectation.
    let expected_order: Vec<String> = fixture["expected_callback_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let fired = order.lock().unwrap();
    assert_eq!(*fired, expected_order, "callbacks must fire in registration order");
}

#[test]
fn fixture_registration_order_event_count() {
    let fixture = load_signal_fixture("registration_order_trace.json");
    let expected = parse_fixture_events(&fixture, "");
    assert_eq!(expected.len(), 1, "fixture encodes exactly one SignalEmit event");
    assert_eq!(expected[0].detail, "ordered_signal");
}

// ===========================================================================
// 2. Arguments forwarding: fixture says one SignalEmit, args reach all receivers
// ===========================================================================

#[test]
fn fixture_arguments_trace_matches() {
    let fixture = load_signal_fixture("arguments_forwarding_trace.json");
    let expected = parse_fixture_events(&fixture, "");

    let (mut tree, emitter_id, recv_a_id, recv_b_id, _recv_c_id) = build_trace_tree_3();

    let args_a = Arc::new(Mutex::new(Vec::new()));
    let args_b = Arc::new(Mutex::new(Vec::new()));

    let aa = args_a.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_data_a", move |args| {
        aa.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });
    let bb = args_b.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_data_b", move |args| {
        bb.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    });

    tree.connect_signal(emitter_id, "data_signal", conn_a);
    tree.connect_signal(emitter_id, "data_signal", conn_b);

    // Parse expected arguments from fixture.
    let fixture_args: Vec<Variant> = fixture["expected_arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            match a["type"].as_str().unwrap() {
                "Int" => Variant::Int(a["value"].as_i64().unwrap()),
                "String" => Variant::String(a["value"].as_str().unwrap().into()),
                "Bool" => Variant::Bool(a["value"].as_bool().unwrap()),
                other => panic!("unsupported fixture arg type: {other}"),
            }
        })
        .collect();

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "data_signal", &fixture_args);

    // Compare trace against fixture.
    let actual = signal_emit_events(&tree);
    let diffs = trace_compare::compare_traces(&expected, &actual);
    let report = trace_compare::format_report("fixture", "runtime", &expected, &actual, &diffs);
    assert!(diffs.is_empty(), "arguments trace mismatch:\n{report}");

    // Both receivers got all arguments.
    let recv_count = fixture["expected_receiver_count"].as_u64().unwrap() as usize;
    assert_eq!(recv_count, 2);

    let got_a = args_a.lock().unwrap();
    let got_b = args_b.lock().unwrap();
    assert_eq!(got_a.len(), fixture_args.len(), "RecvA must get all args");
    assert_eq!(got_b.len(), fixture_args.len(), "RecvB must get all args");
    for (i, expected_arg) in fixture_args.iter().enumerate() {
        assert_eq!(got_a[i], *expected_arg, "RecvA arg {i} mismatch");
        assert_eq!(got_b[i], *expected_arg, "RecvB arg {i} mismatch");
    }
}

// ===========================================================================
// 3. Deferred FIFO: fixture says three SignalEmit events, callbacks fire after flush
// ===========================================================================

#[test]
fn fixture_deferred_fifo_trace_matches() {
    let fixture = load_signal_fixture("deferred_behavior_trace.json");
    let scenario = &fixture["scenarios"]["deferred_only"];
    let expected = trace_compare::parse_events(&scenario["event_trace"]);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("RecvA", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let order = Arc::new(Mutex::new(Vec::new()));

    for label in ["first", "second", "third"] {
        let o = order.clone();
        let lbl = label.to_string();
        let sig_name = format!("sig_{label}");
        let conn = Connection::with_callback(recv_id.object_id(), label, move |_| {
            o.lock().unwrap().push(lbl.clone());
            Variant::Nil
        })
        .as_deferred();
        tree.connect_signal(emitter_id, &sig_name, conn);
    }

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "sig_first", &[]);
    tree.emit_signal(emitter_id, "sig_second", &[]);
    tree.emit_signal(emitter_id, "sig_third", &[]);

    // Before flush: trace has emissions but no callbacks fired.
    let before_flush = scenario["callbacks_before_flush"].as_u64().unwrap();
    assert_eq!(
        order.lock().unwrap().len(),
        before_flush as usize,
        "no callbacks before flush"
    );

    // Compare trace against fixture.
    let actual = signal_emit_events(&tree);
    let diffs = trace_compare::compare_traces(&expected, &actual);
    let report = trace_compare::format_report("fixture", "runtime", &expected, &actual, &diffs);
    assert!(diffs.is_empty(), "deferred FIFO trace mismatch:\n{report}");

    // Flush and verify FIFO order.
    tree.flush_deferred_signals();
    let expected_order: Vec<String> = scenario["expected_callback_order_after_flush"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let fired = order.lock().unwrap();
    assert_eq!(*fired, expected_order, "deferred must flush in FIFO order");
}

// ===========================================================================
// 4. Mixed immediate + deferred: fixture says one SignalEmit, ordering verified
// ===========================================================================

#[test]
fn fixture_mixed_immediate_deferred_trace_matches() {
    let fixture = load_signal_fixture("deferred_behavior_trace.json");
    let scenario = &fixture["scenarios"]["mixed_immediate_deferred"];
    let expected = trace_compare::parse_events(&scenario["event_trace"]);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();
    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let order = Arc::new(Mutex::new(Vec::new()));

    let o1 = order.clone();
    let conn_imm = Connection::with_callback(recv_a_id.object_id(), "on_immediate", move |_| {
        o1.lock().unwrap().push("immediate".to_string());
        Variant::Nil
    });
    let o2 = order.clone();
    let conn_def = Connection::with_callback(recv_b_id.object_id(), "on_deferred", move |_| {
        o2.lock().unwrap().push("deferred".to_string());
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "mixed_sig", conn_imm);
    tree.connect_signal(emitter_id, "mixed_sig", conn_def);

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "mixed_sig", &[]);

    // Before flush: only immediate has fired.
    let before_count = scenario["callbacks_before_flush"].as_u64().unwrap() as usize;
    assert_eq!(order.lock().unwrap().len(), before_count);

    // Trace comparison.
    let actual = signal_emit_events(&tree);
    let diffs = trace_compare::compare_traces(&expected, &actual);
    let report = trace_compare::format_report("fixture", "runtime", &expected, &actual, &diffs);
    assert!(diffs.is_empty(), "mixed trace mismatch:\n{report}");

    // After flush.
    tree.flush_deferred_signals();
    let after_count = scenario["callbacks_after_flush"].as_u64().unwrap() as usize;
    let fired = order.lock().unwrap();
    assert_eq!(fired.len(), after_count);

    let expected_order: Vec<String> = scenario["expected_callback_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(*fired, expected_order, "immediate must fire before deferred");
}

// ===========================================================================
// 5. One-shot: fixture says two SignalEmit events, callback fires only once
// ===========================================================================

#[test]
fn fixture_one_shot_trace_matches() {
    let fixture = load_signal_fixture("deferred_behavior_trace.json");
    let scenario = &fixture["scenarios"]["one_shot_deferred"];
    let expected = trace_compare::parse_events(&scenario["event_trace"]);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("RecvA", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();
    let conn = Connection::with_callback(recv_id.object_id(), "on_once", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();

    tree.connect_signal(emitter_id, "one_shot_sig", conn);

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    let after_first = scenario["callback_count_after_first_emit"].as_u64().unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), after_first);

    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "one_shot_sig", &[]);
    let after_second = scenario["callback_count_after_second_emit"].as_u64().unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), after_second);

    // Trace comparison.
    let actual = signal_emit_events(&tree);
    let diffs = trace_compare::compare_traces(&expected, &actual);
    let report = trace_compare::format_report("fixture", "runtime", &expected, &actual, &diffs);
    assert!(diffs.is_empty(), "one-shot trace mismatch:\n{report}");

    // Connection count verification.
    let expected_conns = scenario["connection_count_after_second_emit"].as_u64().unwrap();
    let store = tree.signal_store(emitter_id).expect("signal store");
    let sig = store.get_signal("one_shot_sig").expect("one_shot_sig");
    assert_eq!(
        sig.connection_count() as u64,
        expected_conns,
        "one-shot must auto-disconnect"
    );
}

// ===========================================================================
// 6. Scene-level: oracle connections produce expected trace topology
// ===========================================================================

#[test]
fn fixture_scene_oracle_trace_topology() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let oracle_path = fixtures_dir()
        .join("oracle_outputs/signal_instantiation_connections.json");
    let oracle_content = std::fs::read_to_string(&oracle_path)
        .unwrap_or_else(|e| panic!("failed to load oracle: {e}"));
    let oracle: Value = serde_json::from_str(&oracle_content)
        .unwrap_or_else(|e| panic!("failed to parse oracle: {e}"));

    let connections = oracle["connections"].as_array().unwrap();

    // Build unique (from_node, signal_name) pairs from oracle.
    let mut unique_signals: Vec<(String, String)> = connections
        .iter()
        .map(|c| {
            (
                c["from_node"].as_str().unwrap().to_owned(),
                c["signal_name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    unique_signals.sort();
    unique_signals.dedup();

    // Build expected trace events from oracle topology.
    let expected: Vec<trace_compare::TraceEvent> = unique_signals
        .iter()
        .map(|(from_node, signal_name)| {
            let path = if from_node == "." {
                "/root/GameWorld".to_string()
            } else {
                format!("/root/GameWorld/{from_node}")
            };
            trace_compare::TraceEvent {
                event_type: "signal_emit".to_string(),
                node_path: path,
                detail: signal_name.clone(),
                frame: 0,
            }
        })
        .collect();

    // Instantiate scene and emit signals.
    let tscn_path = fixtures_dir().join("scenes/signal_instantiation.tscn");
    let tscn = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to load tscn: {e}"));
    let scene = PackedScene::from_tscn(&tscn).expect("parse signal_instantiation.tscn");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

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

    let actual = signal_emit_events(&tree);
    let diffs = trace_compare::compare_traces(&expected, &actual);
    let report = trace_compare::format_report(
        "oracle-derived",
        "runtime",
        &expected,
        &actual,
        &diffs,
    );
    assert!(diffs.is_empty(), "scene oracle trace topology mismatch:\n{report}");
}

// ===========================================================================
// 7. Oracle connection flags: one-shot flag=4 matches as_one_shot behavior
// ===========================================================================

#[test]
fn fixture_oracle_one_shot_flag_matches_behavior() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let oracle_path = fixtures_dir()
        .join("oracle_outputs/signal_instantiation_connections.json");
    let oracle_content = std::fs::read_to_string(&oracle_path).unwrap();
    let oracle: Value = serde_json::from_str(&oracle_content).unwrap();

    // Find connections with flags=4 (CONNECT_ONE_SHOT).
    let one_shots: Vec<&Value> = oracle["connections"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["flags"].as_u64() == Some(4))
        .collect();

    assert!(
        !one_shots.is_empty(),
        "oracle must contain at least one one-shot connection"
    );

    // Instantiate scene.
    let tscn_path = fixtures_dir().join("scenes/signal_instantiation.tscn");
    let tscn = std::fs::read_to_string(&tscn_path).unwrap();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    for conn in &one_shots {
        let from_node = conn["from_node"].as_str().unwrap();
        let signal_name = conn["signal_name"].as_str().unwrap();

        let from_path = if from_node == "." {
            "/root/GameWorld".to_string()
        } else {
            format!("/root/GameWorld/{from_node}")
        };

        let source_id = tree
            .get_node_by_path(&from_path)
            .unwrap_or_else(|| panic!("node {from_path} must exist"));

        let store = tree.signal_store(source_id).expect("signal store");
        let sig = store.get_signal(signal_name).expect(signal_name);
        assert!(
            sig.connections().iter().any(|c| c.one_shot),
            "{signal_name} on {from_node} must have one_shot=true (oracle flags=4)"
        );

        // Enable tracing and emit twice.
        tree.event_trace_mut().enable();
        tree.event_trace_mut().clear();

        tree.set_trace_frame(0);
        tree.emit_signal(source_id, signal_name, &[]);
        tree.set_trace_frame(1);
        tree.emit_signal(source_id, signal_name, &[]);

        // Both emissions produce trace events.
        let emits = signal_emit_events(&tree);
        assert_eq!(emits.len(), 2, "both emissions traced for {signal_name}");

        // But connection is gone after first emission.
        let store = tree.signal_store(source_id).expect("signal store");
        let sig = store.get_signal(signal_name).expect(signal_name);
        assert_eq!(
            sig.connection_count(),
            0,
            "one-shot {signal_name} must auto-disconnect after first emission"
        );
    }
}

// ===========================================================================
// 8. Fixture file structure validation
// ===========================================================================

#[test]
fn fixture_files_have_required_metadata() {
    for name in [
        "registration_order_trace.json",
        "arguments_forwarding_trace.json",
        "deferred_behavior_trace.json",
    ] {
        let fixture = load_signal_fixture(name);
        assert!(
            fixture["description"].is_string(),
            "{name}: missing description"
        );
        assert!(
            fixture["upstream_version"].is_string(),
            "{name}: missing upstream_version"
        );
        let version = fixture["upstream_version"].as_str().unwrap();
        assert!(
            version.contains("4."),
            "{name}: upstream_version should reference Godot 4.x: {version}"
        );
        assert!(
            fixture["source"].is_string(),
            "{name}: missing source"
        );
    }
}

// ===========================================================================
// 9. Deferred arguments are preserved through the queue (fixture-driven)
// ===========================================================================

#[test]
fn fixture_deferred_preserves_arguments() {
    let fixture = load_signal_fixture("arguments_forwarding_trace.json");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("RecvA", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let received = Arc::new(Mutex::new(Vec::new()));
    let rc = received.clone();
    let conn = Connection::with_callback(recv_id.object_id(), "on_deferred", move |args| {
        rc.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "data_signal", conn);

    let fixture_args: Vec<Variant> = fixture["expected_arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| match a["type"].as_str().unwrap() {
            "Int" => Variant::Int(a["value"].as_i64().unwrap()),
            "String" => Variant::String(a["value"].as_str().unwrap().into()),
            "Bool" => Variant::Bool(a["value"].as_bool().unwrap()),
            other => panic!("unsupported: {other}"),
        })
        .collect();

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "data_signal", &fixture_args);

    // Not yet received.
    assert!(received.lock().unwrap().is_empty());

    tree.flush_deferred_signals();

    let got = received.lock().unwrap();
    assert_eq!(got.len(), fixture_args.len());
    for (i, expected_arg) in fixture_args.iter().enumerate() {
        assert_eq!(got[i], *expected_arg, "deferred arg {i} mismatch");
    }
}
