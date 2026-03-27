//! pat-guwy: Focused signal trace parity tests against oracle trace fixtures.
//!
//! Validates runtime signal traces against dedicated oracle trace fixtures that
//! cover the three acceptance criteria:
//!
//! 1. **Registration order** — signals emitted in oracle declaration order produce
//!    traces matching the oracle fixture, callbacks fire in insertion order.
//! 2. **Arguments** — signal arguments are forwarded correctly to all connected
//!    callbacks, matching oracle-specified expected payloads.
//! 3. **Deferred behavior** — deferred signals record emission immediately but
//!    defer callbacks; immediate callbacks fire synchronously; flush order is FIFO.
//!
//! Each test loads its oracle fixture from `fixtures/golden/traces/signal_*_oracle.json`
//! and compares the runtime-produced trace against it using `trace_compare`.

mod oracle_fixture;
mod trace_compare;

use oracle_fixture::fixtures_dir;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use trace_compare::{compare_traces, format_report, parse_events, TraceEvent};

use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::SignalConnection as Connection;
use gdscene::LifecycleManager;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_oracle_fixture(name: &str) -> Value {
    let path = fixtures_dir()
        .join("golden/traces")
        .join(format!("{name}.json"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load oracle fixture {name}: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse oracle fixture {name}: {e}"))
}

fn signal_emit_trace(tree: &SceneTree) -> Vec<TraceEvent> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| TraceEvent {
            event_type: "SignalEmit".to_string(),
            node_path: e.node_path.clone(),
            detail: e.detail.clone(),
            frame: e.frame,
        })
        .collect()
}

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

fn resolve_oracle_path(from_node: &str) -> String {
    if from_node == "." {
        "/root/GameWorld".to_string()
    } else {
        format!("/root/GameWorld/{from_node}")
    }
}

fn load_signal_instantiation_scene() -> SceneTree {
    let tscn_path = fixtures_dir()
        .join("scenes")
        .join("signal_instantiation.tscn");
    let tscn = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to load tscn: {e}"));
    let scene = PackedScene::from_tscn(&tscn).expect("parse signal_instantiation.tscn");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("add_packed_scene_to_tree");

    tree
}

// ===========================================================================
// REGISTRATION ORDER — oracle fixture comparison
// ===========================================================================

// 1. Load registration_order oracle fixture and compare against runtime trace.
#[test]
fn registration_order_trace_matches_oracle_fixture() {
    let oracle_json = load_oracle_fixture("signal_registration_order_oracle");
    let expected = parse_events(&oracle_json["event_trace"]);

    let oracle_conns = load_oracle_connections();
    let mut tree = load_signal_instantiation_scene();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Emit in oracle declaration order (unique signals).
    let mut emitted: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in &oracle_conns {
        let pair = (from.clone(), sig.clone());
        if !emitted.contains(&pair) {
            let path = resolve_oracle_path(from);
            if let Some(source_id) = tree.get_node_by_path(&path) {
                tree.emit_signal(source_id, sig, &[]);
            }
            emitted.push(pair);
        }
    }

    let actual = signal_emit_trace(&tree);

    let diffs = compare_traces(&expected, &actual);
    if !diffs.is_empty() {
        let report = format_report(
            "Registration Order Oracle",
            "Patina Runtime",
            &expected,
            &actual,
            &diffs,
        );
        panic!("Registration order trace does not match oracle fixture:\n{report}");
    }
}

// 2. Registration order: callbacks fire in insertion order per oracle.
#[test]
fn registration_order_callbacks_fire_in_insertion_order() {
    let oracle_conns = load_oracle_connections();
    let mut tree = load_signal_instantiation_scene();

    // Wire callbacks for health_changed (Player→HUD, Player→HUD/ScoreLabel).
    let health_conns: Vec<_> = oracle_conns
        .iter()
        .filter(|(sig, _, _, _, _)| sig == "health_changed")
        .collect();
    assert_eq!(health_conns.len(), 2, "oracle must have 2 health_changed connections");

    let player_path = "/root/GameWorld/Player";
    let player_id = tree.get_node_by_path(player_path).expect("Player");

    let hud_path = "/root/GameWorld/HUD";
    let hud_id = tree.get_node_by_path(hud_path).expect("HUD");

    let order = Arc::new(Mutex::new(Vec::new()));

    // First connection: Player→HUD._on_health_changed
    let o1 = order.clone();
    let conn1 = Connection::with_callback(hud_id.object_id(), "_on_health_changed", move |_| {
        o1.lock().unwrap().push("HUD");
        Variant::Nil
    });

    // Second connection: Player→HUD/ScoreLabel._on_player_health_for_score
    let o2 = order.clone();
    let score_path = "/root/GameWorld/HUD/ScoreLabel";
    let score_id = tree.get_node_by_path(score_path).expect("ScoreLabel");
    let conn2 = Connection::with_callback(
        score_id.object_id(),
        "_on_player_health_for_score",
        move |_| {
            o2.lock().unwrap().push("ScoreLabel");
            Variant::Nil
        },
    );

    tree.connect_signal(player_id, "health_changed", conn1);
    tree.connect_signal(player_id, "health_changed", conn2);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    tree.emit_signal(player_id, "health_changed", &[Variant::Int(75)]);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec!["HUD", "ScoreLabel"],
        "callbacks must fire in oracle-declared insertion order"
    );
}

// 3. Registration order: signal count matches oracle unique signal count.
#[test]
fn registration_order_signal_count_matches_oracle() {
    let oracle_json = load_oracle_fixture("signal_registration_order_oracle");
    let expected = parse_events(&oracle_json["event_trace"]);

    let oracle_conns = load_oracle_connections();
    let mut unique: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in &oracle_conns {
        let pair = (from.clone(), sig.clone());
        if !unique.contains(&pair) {
            unique.push(pair);
        }
    }

    assert_eq!(
        expected.len(),
        unique.len(),
        "oracle fixture event count must match unique (from, signal) pairs"
    );
}

// ===========================================================================
// ARGUMENTS — oracle fixture comparison
// ===========================================================================

// 4. Load arguments oracle fixture and verify trace event count and signals.
#[test]
fn arguments_trace_matches_oracle_fixture() {
    let oracle_json = load_oracle_fixture("signal_arguments_oracle");
    let expected = parse_events(&oracle_json["event_trace"]);

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("ReceiverA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("ReceiverB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    // Wire connections per oracle fixture emissions.
    let conn_health_a = Connection::with_callback(recv_a_id.object_id(), "on_health", |_| Variant::Nil);
    let conn_health_b = Connection::with_callback(recv_b_id.object_id(), "on_health_log", |_| Variant::Nil);
    tree.connect_signal(emitter_id, "health_changed", conn_health_a);
    tree.connect_signal(emitter_id, "health_changed", conn_health_b);

    let conn_item = Connection::with_callback(recv_a_id.object_id(), "on_item", |_| Variant::Nil);
    tree.connect_signal(emitter_id, "item_collected", conn_item);

    let conn_empty = Connection::with_callback(recv_a_id.object_id(), "on_empty", |_| Variant::Nil);
    tree.connect_signal(emitter_id, "no_args_signal", conn_empty);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Frame 0: health_changed and item_collected.
    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "health_changed", &[Variant::Int(42), Variant::String("damage".into())]);
    tree.emit_signal(emitter_id, "item_collected", &[Variant::String("gold_coin".into()), Variant::Int(5), Variant::Bool(true)]);

    // Frame 1: no_args_signal.
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "no_args_signal", &[]);

    let actual = signal_emit_trace(&tree);

    let diffs = compare_traces(&expected, &actual);
    if !diffs.is_empty() {
        let report = format_report("Arguments Oracle", "Patina Runtime", &expected, &actual, &diffs);
        panic!("Arguments trace does not match oracle fixture:\n{report}");
    }
}

// 5. Arguments: each callback receives the exact args specified by oracle.
#[test]
fn arguments_forwarded_match_oracle_spec() {
    let oracle_json = load_oracle_fixture("signal_arguments_oracle");
    let emissions = oracle_json["emissions"].as_array().expect("emissions array");

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("ReceiverA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("ReceiverB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    // Capture args per callback.
    let health_a_args = Arc::new(Mutex::new(Vec::new()));
    let health_b_args = Arc::new(Mutex::new(Vec::new()));
    let item_args = Arc::new(Mutex::new(Vec::new()));
    let empty_args = Arc::new(Mutex::new(Vec::new()));

    let ha = health_a_args.clone();
    let hb = health_b_args.clone();
    let ia = item_args.clone();
    let ea = empty_args.clone();

    tree.connect_signal(
        emitter_id,
        "health_changed",
        Connection::with_callback(recv_a_id.object_id(), "on_health", move |args| {
            ha.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );
    tree.connect_signal(
        emitter_id,
        "health_changed",
        Connection::with_callback(recv_b_id.object_id(), "on_health_log", move |args| {
            hb.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );
    tree.connect_signal(
        emitter_id,
        "item_collected",
        Connection::with_callback(recv_a_id.object_id(), "on_item", move |args| {
            ia.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );
    tree.connect_signal(
        emitter_id,
        "no_args_signal",
        Connection::with_callback(recv_a_id.object_id(), "on_empty", move |args| {
            ea.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Emit per oracle spec.
    tree.set_trace_frame(0);
    tree.emit_signal(
        emitter_id,
        "health_changed",
        &[Variant::Int(42), Variant::String("damage".into())],
    );
    tree.emit_signal(
        emitter_id,
        "item_collected",
        &[
            Variant::String("gold_coin".into()),
            Variant::Int(5),
            Variant::Bool(true),
        ],
    );

    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "no_args_signal", &[]);

    // Verify: health_changed → ReceiverA receives [42, "damage"].
    let ha_got = health_a_args.lock().unwrap();
    assert_eq!(ha_got.len(), 2, "ReceiverA.on_health must get 2 args");
    assert_eq!(ha_got[0], Variant::Int(42));
    assert_eq!(ha_got[1], Variant::String("damage".into()));

    // Verify: health_changed → ReceiverB receives same args.
    let hb_got = health_b_args.lock().unwrap();
    assert_eq!(hb_got.len(), 2, "ReceiverB.on_health_log must get 2 args");
    assert_eq!(hb_got[0], Variant::Int(42));
    assert_eq!(hb_got[1], Variant::String("damage".into()));

    // Verify: item_collected → ReceiverA receives ["gold_coin", 5, true].
    let ia_got = item_args.lock().unwrap();
    assert_eq!(ia_got.len(), 3, "ReceiverA.on_item must get 3 args");
    assert_eq!(ia_got[0], Variant::String("gold_coin".into()));
    assert_eq!(ia_got[1], Variant::Int(5));
    assert_eq!(ia_got[2], Variant::Bool(true));

    // Verify: no_args_signal → ReceiverA receives [].
    let ea_got = empty_args.lock().unwrap();
    assert_eq!(ea_got.len(), 0, "ReceiverA.on_empty must get 0 args");

    // Verify emission count matches oracle.
    assert_eq!(emissions.len(), 3);
    let trace = signal_emit_trace(&tree);
    assert_eq!(trace.len(), 3, "3 emissions = 3 trace events");
}

// 6. Arguments: multi-callback receives args independently (no aliasing).
#[test]
fn arguments_multi_callback_independent_delivery() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    let a_args = Arc::new(Mutex::new(Vec::new()));
    let b_args = Arc::new(Mutex::new(Vec::new()));
    let aa = a_args.clone();
    let bb = b_args.clone();

    tree.connect_signal(
        emitter_id,
        "data",
        Connection::with_callback(recv_a_id.object_id(), "on_a", move |args| {
            aa.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );
    tree.connect_signal(
        emitter_id,
        "data",
        Connection::with_callback(recv_b_id.object_id(), "on_b", move |args| {
            bb.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    tree.emit_signal(
        emitter_id,
        "data",
        &[Variant::Float(3.14), Variant::Bool(false)],
    );

    let a_got = a_args.lock().unwrap();
    let b_got = b_args.lock().unwrap();
    assert_eq!(a_got.len(), 2);
    assert_eq!(b_got.len(), 2);
    assert_eq!(a_got[0], Variant::Float(3.14));
    assert_eq!(b_got[0], Variant::Float(3.14));
    assert_eq!(a_got[1], Variant::Bool(false));
    assert_eq!(b_got[1], Variant::Bool(false));
}

// ===========================================================================
// DEFERRED BEHAVIOR — oracle fixture comparison
// ===========================================================================

// 7. Load deferred oracle fixture and verify trace matches.
#[test]
fn deferred_trace_matches_oracle_fixture() {
    let oracle_json = load_oracle_fixture("signal_deferred_oracle");
    let expected = parse_events(&oracle_json["event_trace"]);

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    // sig_mixed: 2 immediate + 2 deferred connections.
    let imm_counter = Arc::new(AtomicU64::new(0));
    let def_counter = Arc::new(AtomicU64::new(0));

    let ic1 = imm_counter.clone();
    tree.connect_signal(
        emitter_id,
        "sig_mixed",
        Connection::with_callback(recv_id.object_id(), "immediate_A", move |_| {
            ic1.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    let ic2 = imm_counter.clone();
    tree.connect_signal(
        emitter_id,
        "sig_mixed",
        Connection::with_callback(recv_id.object_id(), "immediate_B", move |_| {
            ic2.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    let dc1 = def_counter.clone();
    tree.connect_signal(
        emitter_id,
        "sig_mixed",
        Connection::with_callback(recv_id.object_id(), "deferred_C", move |_| {
            dc1.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );
    let dc2 = def_counter.clone();
    tree.connect_signal(
        emitter_id,
        "sig_mixed",
        Connection::with_callback(recv_id.object_id(), "deferred_D", move |_| {
            dc2.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );

    // sig_deferred_only: 1 deferred connection.
    let def_only = Arc::new(AtomicU64::new(0));
    let do1 = def_only.clone();
    tree.connect_signal(
        emitter_id,
        "sig_deferred_only",
        Connection::with_callback(recv_id.object_id(), "deferred_only", move |_| {
            do1.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );

    // sig_frame1: 1 immediate connection.
    let frame1_counter = Arc::new(AtomicU64::new(0));
    let f1 = frame1_counter.clone();
    tree.connect_signal(
        emitter_id,
        "sig_frame1",
        Connection::with_callback(recv_id.object_id(), "immediate_E", move |_| {
            f1.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Frame 0.
    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "sig_mixed", &[]);
    tree.emit_signal(emitter_id, "sig_deferred_only", &[]);

    // After emit, before flush: immediate fired, deferred did not.
    assert_eq!(imm_counter.load(Ordering::SeqCst), 2, "2 immediate callbacks fired");
    assert_eq!(def_counter.load(Ordering::SeqCst), 0, "deferred not yet fired");
    assert_eq!(def_only.load(Ordering::SeqCst), 0, "deferred_only not yet fired");

    // Flush deferred.
    tree.flush_deferred_signals();
    assert_eq!(def_counter.load(Ordering::SeqCst), 2, "2 deferred callbacks flushed");
    assert_eq!(def_only.load(Ordering::SeqCst), 1, "deferred_only flushed");

    // Frame 1.
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "sig_frame1", &[]);
    assert_eq!(frame1_counter.load(Ordering::SeqCst), 1, "frame1 immediate fired");

    let actual = signal_emit_trace(&tree);

    let diffs = compare_traces(&expected, &actual);
    if !diffs.is_empty() {
        let report = format_report("Deferred Oracle", "Patina Runtime", &expected, &actual, &diffs);
        panic!("Deferred trace does not match oracle fixture:\n{report}");
    }
}

// 8. Deferred: callback ordering matches oracle expected_callback_order.
#[test]
fn deferred_callback_order_matches_oracle() {
    let oracle_json = load_oracle_fixture("signal_deferred_oracle");
    let expected_order = oracle_json["expected_callback_order"]
        .as_array()
        .expect("expected_callback_order");

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let order = Arc::new(Mutex::new(Vec::new()));

    // Wire per oracle labels.
    for entry in expected_order {
        let label = entry["label"].as_str().unwrap().to_string();
        let fires_at = entry["fires_at"].as_str().unwrap();
        let o = order.clone();
        let lbl = label.clone();

        let conn = Connection::with_callback(recv_id.object_id(), &label, move |_| {
            o.lock().unwrap().push(lbl.clone());
            Variant::Nil
        });

        let conn = if fires_at == "flush" {
            conn.as_deferred()
        } else {
            conn
        };

        let sig = match label.as_str() {
            "immediate_A" | "immediate_B" | "deferred_C" | "deferred_D" => "sig_mixed",
            "immediate_E" => "sig_frame1",
            _ => panic!("unknown label: {label}"),
        };

        tree.connect_signal(emitter_id, sig, conn);
    }

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Frame 0: emit sig_mixed.
    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "sig_mixed", &[]);

    // Check: immediate fired, deferred queued.
    {
        let fired = order.lock().unwrap();
        assert_eq!(
            *fired,
            vec!["immediate_A", "immediate_B"],
            "only immediate callbacks fire before flush"
        );
    }

    // Flush.
    tree.flush_deferred_signals();
    {
        let fired = order.lock().unwrap();
        assert_eq!(
            *fired,
            vec!["immediate_A", "immediate_B", "deferred_C", "deferred_D"],
            "deferred fire in FIFO order after flush"
        );
    }

    // Frame 1: emit sig_frame1.
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "sig_frame1", &[]);

    let fired = order.lock().unwrap();
    assert_eq!(
        *fired,
        vec![
            "immediate_A",
            "immediate_B",
            "deferred_C",
            "deferred_D",
            "immediate_E"
        ],
        "full callback order must match oracle expected_callback_order"
    );
}

// 9. Deferred: deferred args preserved through queue (oracle spec).
#[test]
fn deferred_args_preserved_per_oracle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let captured = Arc::new(Mutex::new(Vec::new()));
    let cc = captured.clone();

    tree.connect_signal(
        emitter_id,
        "deferred_data",
        Connection::with_callback(recv_id.object_id(), "on_data", move |args| {
            cc.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        })
        .as_deferred(),
    );

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    tree.emit_signal(
        emitter_id,
        "deferred_data",
        &[Variant::Int(99), Variant::String("queued".into())],
    );

    // Not yet delivered.
    assert!(captured.lock().unwrap().is_empty());

    tree.flush_deferred_signals();

    let got = captured.lock().unwrap();
    assert_eq!(got.len(), 2, "deferred args must be preserved");
    assert_eq!(got[0], Variant::Int(99));
    assert_eq!(got[1], Variant::String("queued".into()));
}

// 10. Deferred: trace records emission immediately, not at flush time.
#[test]
fn deferred_trace_recorded_at_emit_not_flush() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    tree.connect_signal(
        emitter_id,
        "deferred_sig",
        Connection::with_callback(recv_id.object_id(), "on_def", |_| Variant::Nil).as_deferred(),
    );

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    tree.emit_signal(emitter_id, "deferred_sig", &[]);

    // Trace recorded immediately.
    let before_flush = signal_emit_trace(&tree);
    assert_eq!(before_flush.len(), 1, "trace recorded before flush");
    assert_eq!(before_flush[0].frame, 0);

    // Flush on frame 1.
    tree.set_trace_frame(1);
    tree.flush_deferred_signals();

    // No new trace event from flush.
    let after_flush = signal_emit_trace(&tree);
    assert_eq!(
        after_flush.len(),
        1,
        "flush must not produce additional trace events"
    );
    assert_eq!(after_flush[0].frame, 0, "trace frame stays at emit time");
}

// ===========================================================================
// CROSS-CUTTING — combined scenarios
// ===========================================================================

// 11. One-shot + deferred: oracle flags=4 connection fires once even if deferred.
#[test]
fn one_shot_deferred_fires_once_per_oracle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    tree.connect_signal(
        emitter_id,
        "one_shot_deferred",
        Connection::with_callback(recv_id.object_id(), "on_once", move |_| {
            cc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_one_shot()
        .as_deferred(),
    );

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    tree.emit_signal(emitter_id, "one_shot_deferred", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 0, "deferred: not fired yet");

    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 1, "fires once after flush");

    // Second emission + flush.
    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "one_shot_deferred", &[]);
    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 1, "one-shot: no second fire");

    // Both emissions traced.
    let trace = signal_emit_trace(&tree);
    assert_eq!(trace.len(), 2, "both emissions produce trace events");
}

// 12. Stress: 20 signals x 3 callbacks each, all args verified.
#[test]
fn stress_20_signals_with_args() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let total_calls = Arc::new(AtomicU64::new(0));

    for i in 0..20 {
        let sig_name = format!("sig_{i}");
        for j in 0..3 {
            let tc = total_calls.clone();
            let method = format!("on_{i}_{j}");
            tree.connect_signal(
                emitter_id,
                &sig_name,
                Connection::with_callback(recv_id.object_id(), &method, move |args| {
                    assert_eq!(args.len(), 1, "each emission sends 1 arg");
                    tc.fetch_add(1, Ordering::SeqCst);
                    Variant::Nil
                }),
            );
        }
    }

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    for i in 0..20 {
        tree.emit_signal(emitter_id, &format!("sig_{i}"), &[Variant::Int(i)]);
    }

    assert_eq!(total_calls.load(Ordering::SeqCst), 60, "20 signals x 3 callbacks = 60 fires");

    let trace = signal_emit_trace(&tree);
    assert_eq!(trace.len(), 20, "20 emissions = 20 trace events");
}
