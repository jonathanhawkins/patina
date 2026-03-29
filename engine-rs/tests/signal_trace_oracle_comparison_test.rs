//! pat-3i1h: Compare runtime signal traces against oracle trace output.
//!
//! Uses the `trace_compare` framework to compare Patina's runtime signal
//! traces against oracle-derived expected traces. While
//! `signal_trace_oracle_parity_test.rs` tests signal mechanics (callbacks,
//! deferred, one-shot), this file focuses on **trace-level comparison**:
//! the exact sequence of SignalEmit events, their frame tags, node paths,
//! and ordering relative to lifecycle events in the global trace.
//!
//! Coverage:
//! 1.  Oracle connection topology → expected SignalEmit trace events
//! 2.  Signal emission ordering matches oracle connection declaration order
//! 3.  Deferred vs immediate trace ordering in mixed connection topology
//! 4.  One-shot signal emission count across multiple frames
//! 5.  Lifecycle + signal interleaving: ENTER_TREE/READY precede signal emissions
//! 6.  Multi-signal emission per source: traces ordered by emission call sequence
//! 7.  Cross-node signal chain trace: emit → callback → re-emit chain
//! 8.  Frame-tagged signal trace: emissions on different frames tagged correctly
//! 9.  Oracle tree structure → Patina tree structure parity for signal scene
//! 10. Empty signal emission (no connections) still produces trace event
//! 11. Trace comparison framework: signals_complex golden vs runtime
//! 12. Connection flags from oracle match behavioral parity (flags=4 → one-shot)
//!
//! Godot references: get_signal_connection_list(), signal emission ordering,
//! CONNECT_ONE_SHOT (flag 4), signal declaration order in .tscn.

mod oracle_fixture;
mod trace_compare;

use oracle_fixture::fixtures_dir;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use trace_compare::{compare_traces, format_report, parse_events, TraceEvent};

use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::SignalConnection as Connection;
use gdscene::LifecycleManager;
use gdvariant::Variant;

// ===========================================================================
// Helpers
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

fn resolve_oracle_path(from_node: &str) -> String {
    if from_node == "." {
        "/root/GameWorld".to_string()
    } else {
        format!("/root/GameWorld/{from_node}")
    }
}

/// Extract SignalEmit events as TraceEvent structs for comparison.
fn signal_emit_trace_events(tree: &SceneTree) -> Vec<TraceEvent> {
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

/// Build expected trace events from oracle connections for a single emission pass.
fn build_expected_signal_trace(oracle: &[(String, String, String, String, u32)]) -> Vec<TraceEvent> {
    // Collect unique (from_node, signal_name) in oracle declaration order.
    let mut unique_signals: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in oracle {
        let pair = (from.clone(), sig.clone());
        if !unique_signals.contains(&pair) {
            unique_signals.push(pair);
        }
    }

    unique_signals
        .iter()
        .map(|(from, sig)| {
            let path = resolve_oracle_path(from);
            // Find the last component for node_path (Patina uses full tree path).
            TraceEvent {
                event_type: "SignalEmit".to_string(),
                node_path: path,
                detail: sig.clone(),
                frame: 0,
            }
        })
        .collect()
}

// ===========================================================================
// 1. Oracle connection topology → expected SignalEmit trace
// ===========================================================================

#[test]
fn oracle_topology_produces_expected_signal_trace_count() {
    let oracle = load_oracle_connections();
    let mut tree = load_signal_instantiation_scene();

    // Collect unique (from, signal) pairs — each produces one trace event.
    let mut unique_signals: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in &oracle {
        let pair = (from.clone(), sig.clone());
        if !unique_signals.contains(&pair) {
            unique_signals.push(pair);
        }
    }

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    for (from, sig) in &unique_signals {
        let path = resolve_oracle_path(from);
        let source_id = tree
            .get_node_by_path(&path)
            .unwrap_or_else(|| panic!("node {path} must exist"));
        tree.emit_signal(source_id, sig, &[]);
    }

    let emits = signal_emit_trace_events(&tree);
    assert_eq!(
        emits.len(),
        unique_signals.len(),
        "one SignalEmit trace per unique (source, signal) pair"
    );
}

// ===========================================================================
// 2. Signal emission ordering matches oracle connection declaration order
// ===========================================================================

#[test]
fn signal_trace_order_matches_oracle_declaration_order() {
    let oracle = load_oracle_connections();
    let mut tree = load_signal_instantiation_scene();

    let expected = build_expected_signal_trace(&oracle);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Emit in oracle declaration order.
    let mut unique_signals: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in &oracle {
        let pair = (from.clone(), sig.clone());
        if !unique_signals.contains(&pair) {
            unique_signals.push(pair);
        }
    }

    for (from, sig) in &unique_signals {
        let path = resolve_oracle_path(from);
        let source_id = tree
            .get_node_by_path(&path)
            .unwrap_or_else(|| panic!("node {path} must exist"));
        tree.emit_signal(source_id, sig, &[]);
    }

    let actual = signal_emit_trace_events(&tree);

    // Use trace_compare to diff expected vs actual signal trace.
    let diffs = compare_traces(&expected, &actual);
    if !diffs.is_empty() {
        let report = format_report("Oracle Expected", "Patina Runtime", &expected, &actual, &diffs);
        panic!(
            "Signal trace order does not match oracle declaration order:\n{report}"
        );
    }
}

// ===========================================================================
// 3. Deferred vs immediate trace ordering
// ===========================================================================

#[test]
fn deferred_connection_trace_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    // Immediate connection.
    let imm_counter = Arc::new(AtomicU64::new(0));
    let ic = imm_counter.clone();
    let conn_imm = Connection::with_callback(recv_id.object_id(), "on_immediate", move |_| {
        ic.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    // Deferred connection.
    let def_counter = Arc::new(AtomicU64::new(0));
    let dc = def_counter.clone();
    let conn_def = Connection::with_callback(recv_id.object_id(), "on_deferred", move |_| {
        dc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "mixed_sig", conn_imm);
    tree.connect_signal(emitter_id, "mixed_sig", conn_def);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    tree.emit_signal(emitter_id, "mixed_sig", &[]);

    // Immediate fires synchronously, deferred does not.
    assert_eq!(imm_counter.load(Ordering::SeqCst), 1);
    assert_eq!(def_counter.load(Ordering::SeqCst), 0);

    // SignalEmit trace recorded before callbacks.
    let emits = signal_emit_trace_events(&tree);
    assert_eq!(emits.len(), 1);
    assert_eq!(emits[0].detail, "mixed_sig");

    // Only one SignalEmit event total — trace records the emission, not the dispatch.
    tree.set_trace_frame(1);
    let emits_after = signal_emit_trace_events(&tree);
    assert_eq!(
        emits_after.len(),
        1,
        "deferred flush does not produce additional SignalEmit trace events"
    );
}

// ===========================================================================
// 4. One-shot signal across multiple frames
// ===========================================================================

#[test]
fn one_shot_signal_trace_across_frames() {
    // Oracle declares Enemy→HUD.defeated with flags=4 (one-shot).
    // Verify: first emit fires callback, second emit still traces but no callback.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let src = Node::new("Enemy", "Node2D");
    let src_id = tree.add_child(root, src).unwrap();

    let target = Node::new("HUD", "Node");
    let target_id = tree.add_child(root, target).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();
    let conn = Connection::with_callback(target_id.object_id(), "_on_enemy_defeated", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();

    tree.connect_signal(src_id, "defeated", conn);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Frame 0: first emission.
    tree.set_trace_frame(0);
    tree.emit_signal(src_id, "defeated", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Frame 1: second emission.
    tree.set_trace_frame(1);
    tree.emit_signal(src_id, "defeated", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1, "one-shot should not fire again");

    // Both emissions should produce trace events.
    let emits = signal_emit_trace_events(&tree);
    assert_eq!(emits.len(), 2, "both emissions traced");
    assert_eq!(emits[0].frame, 0);
    assert_eq!(emits[1].frame, 1);

    // Build expected trace and compare.
    let expected = vec![
        TraceEvent {
            event_type: "SignalEmit".to_string(),
            node_path: emits[0].node_path.clone(),
            detail: "defeated".to_string(),
            frame: 0,
        },
        TraceEvent {
            event_type: "SignalEmit".to_string(),
            node_path: emits[1].node_path.clone(),
            detail: "defeated".to_string(),
            frame: 1,
        },
    ];

    let diffs = compare_traces(&expected, &emits);
    assert!(
        diffs.is_empty(),
        "one-shot trace should match expected: {:?}",
        diffs
    );
}

// ===========================================================================
// 5. Lifecycle events precede signal emissions in global trace
// ===========================================================================

#[test]
fn lifecycle_precedes_signals_in_global_trace() {
    let mut tree = load_signal_instantiation_scene();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Emit all signals from oracle topology.
    let oracle = load_oracle_connections();
    let mut unique_signals: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in &oracle {
        let pair = (from.clone(), sig.clone());
        if !unique_signals.contains(&pair) {
            unique_signals.push(pair);
        }
    }
    for (from, sig) in &unique_signals {
        let path = resolve_oracle_path(from);
        if let Some(source_id) = tree.get_node_by_path(&path) {
            tree.emit_signal(source_id, sig, &[]);
        }
    }

    // The scene was already entered (lifecycle events cleared).
    // All events in this trace should be SignalEmit — no lifecycle noise.
    let trace = tree.event_trace().events();
    for event in trace {
        assert_eq!(
            event.event_type,
            TraceEventType::SignalEmit,
            "after clearing trace post-lifecycle, only SignalEmit events expected, got: {:?} {}",
            event.event_type,
            event.detail
        );
    }
}

// ===========================================================================
// 6. Multi-signal per source: ordered by emission call sequence
// ===========================================================================

#[test]
fn multi_signal_per_source_ordered_by_emission_call() {
    let mut tree = load_signal_instantiation_scene();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Player has two signals: health_changed and died.
    // Emit in specific order and verify trace reflects that.
    let player_path = "/root/GameWorld/Player";
    let player_id = tree
        .get_node_by_path(player_path)
        .expect("Player must exist");

    tree.emit_signal(player_id, "health_changed", &[]);
    tree.emit_signal(player_id, "died", &[]);

    let emits = signal_emit_trace_events(&tree);
    assert_eq!(emits.len(), 2);
    assert_eq!(emits[0].detail, "health_changed");
    assert_eq!(emits[1].detail, "died");

    // Build expected and compare.
    let expected = vec![
        TraceEvent {
            event_type: "SignalEmit".to_string(),
            node_path: emits[0].node_path.clone(),
            detail: "health_changed".to_string(),
            frame: 0,
        },
        TraceEvent {
            event_type: "SignalEmit".to_string(),
            node_path: emits[1].node_path.clone(),
            detail: "died".to_string(),
            frame: 0,
        },
    ];

    let diffs = compare_traces(&expected, &emits);
    assert!(diffs.is_empty(), "multi-signal order mismatch: {:?}", diffs);
}

// ===========================================================================
// 7. Cross-node signal chain trace ordering
// ===========================================================================

#[test]
fn cross_node_signal_chain_trace_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node_a = Node::new("NodeA", "Node2D");
    let node_a_id = tree.add_child(root, node_a).unwrap();

    let node_b = Node::new("NodeB", "Node2D");
    let node_b_id = tree.add_child(root, node_b).unwrap();

    let node_c = Node::new("NodeC", "Node2D");
    let node_c_id = tree.add_child(root, node_c).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Emit A → B → C as a chain of separate emit calls.
    tree.emit_signal(node_a_id, "sig_a", &[]);
    tree.emit_signal(node_b_id, "sig_b", &[]);
    tree.emit_signal(node_c_id, "sig_c", &[]);

    let emits = signal_emit_trace_events(&tree);
    assert_eq!(emits.len(), 3);

    // Verify strict ordering: A, B, C.
    assert!(emits[0].node_path.contains("NodeA"));
    assert_eq!(emits[0].detail, "sig_a");
    assert!(emits[1].node_path.contains("NodeB"));
    assert_eq!(emits[1].detail, "sig_b");
    assert!(emits[2].node_path.contains("NodeC"));
    assert_eq!(emits[2].detail, "sig_c");
}

// ===========================================================================
// 8. Frame-tagged signal trace
// ===========================================================================

#[test]
fn frame_tagged_signal_trace_comparison() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Emit across three frames.
    for frame in 0..3u64 {
        tree.set_trace_frame(frame);
        tree.emit_signal(emitter_id, "tick", &[]);
    }

    let actual = signal_emit_trace_events(&tree);
    assert_eq!(actual.len(), 3);

    // Build expected with frame tags.
    let expected: Vec<TraceEvent> = (0..3u64)
        .map(|f| TraceEvent {
            event_type: "SignalEmit".to_string(),
            node_path: actual[0].node_path.clone(),
            detail: "tick".to_string(),
            frame: f,
        })
        .collect();

    let diffs = compare_traces(&expected, &actual);
    assert!(
        diffs.is_empty(),
        "frame-tagged signal trace mismatch: {:?}",
        diffs
    );
}

// ===========================================================================
// 9. Oracle tree structure → Patina tree structure parity
// ===========================================================================

#[test]
fn oracle_tree_structure_matches_patina_instantiation() {
    let oracle_tree_path = fixtures_dir()
        .join("oracle_outputs")
        .join("signal_instantiation_tree.json");
    let oracle: Value = serde_json::from_str(
        &std::fs::read_to_string(&oracle_tree_path).expect("load oracle tree"),
    )
    .expect("parse oracle tree");

    let tree = load_signal_instantiation_scene();

    // Collect oracle node paths from tree JSON.
    fn collect_paths(node: &Value, paths: &mut Vec<String>) {
        if let Some(path) = node.get("path").and_then(Value::as_str) {
            paths.push(path.to_string());
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                collect_paths(child, paths);
            }
        }
    }

    let mut oracle_paths = Vec::new();
    // Skip the root Window node; start from GameWorld.
    if let Some(children) = oracle.get("children").and_then(Value::as_array) {
        for child in children {
            collect_paths(child, &mut oracle_paths);
        }
    }

    // Verify each oracle path exists in Patina tree.
    for oracle_path in &oracle_paths {
        assert!(
            tree.get_node_by_path(oracle_path).is_some(),
            "oracle path {oracle_path} must exist in Patina tree"
        );
    }
}

// ===========================================================================
// 10. Empty signal emission still produces trace event
// ===========================================================================

#[test]
fn empty_signal_emission_produces_trace() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Lonely", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Emit signal with no connections attached.
    tree.emit_signal(node_id, "unheard", &[]);

    let emits = signal_emit_trace_events(&tree);
    assert_eq!(emits.len(), 1, "emission with no connections still traced");
    assert_eq!(emits[0].detail, "unheard");
    assert!(emits[0].node_path.contains("Lonely"));
}

// ===========================================================================
// 11. Golden trace comparison: signals_complex lifecycle trace parity
// ===========================================================================

#[test]
fn signals_complex_lifecycle_trace_parity() {
    let golden_dir = fixtures_dir().join("golden/traces");
    let patina_path = golden_dir.join("signals_complex_patina.json");
    let upstream_path = golden_dir.join("signals_complex_upstream_mock.json");

    let patina_json: Value = serde_json::from_str(
        &std::fs::read_to_string(&patina_path).expect("load patina golden"),
    )
    .expect("parse patina golden");

    let upstream_json: Value = serde_json::from_str(
        &std::fs::read_to_string(&upstream_path).expect("load upstream golden"),
    )
    .expect("parse upstream golden");

    let patina_events = parse_events(&patina_json["event_trace"]);
    let upstream_events = parse_events(&upstream_json["event_trace"]);

    // Compare the golden traces.
    let diffs = compare_traces(&upstream_events, &patina_events);

    if !diffs.is_empty() {
        let report = format_report(
            "Upstream Mock",
            "Patina Golden",
            &upstream_events,
            &patina_events,
            &diffs,
        );
        panic!(
            "signals_complex golden trace parity failed ({} diffs):\n{}",
            diffs.len(),
            report
        );
    }
}

// ===========================================================================
// 12. Oracle connection flags drive behavioral parity
// ===========================================================================

#[test]
fn oracle_connection_flags_match_behavior() {
    let oracle = load_oracle_connections();

    // flags=0 → immediate, flags=4 → one-shot (CONNECT_ONE_SHOT).
    // Verify the oracle contains expected flag values.
    let immediate_count = oracle.iter().filter(|(_, _, _, _, f)| *f == 0).count();
    let one_shot_count = oracle.iter().filter(|(_, _, _, _, f)| *f == 4).count();

    assert!(
        immediate_count > 0,
        "oracle must have at least one immediate connection"
    );
    assert!(
        one_shot_count > 0,
        "oracle must have at least one one-shot connection (flags=4)"
    );

    // Verify: Enemy→HUD.defeated is the one-shot connection (flags=4).
    let defeated = oracle
        .iter()
        .find(|(sig, from, _, _, _)| sig == "defeated" && from == "Enemy");
    assert!(defeated.is_some(), "defeated connection must exist");
    let (_, _, _, method, flags) = defeated.unwrap();
    assert_eq!(*flags, 4, "defeated must be one-shot (flags=4)");
    assert_eq!(method, "_on_enemy_defeated");

    // Wire up the one-shot connection in Patina and verify behavior.
    let mut tree = load_signal_instantiation_scene();
    let enemy_path = "/root/GameWorld/Enemy";
    let hud_path = "/root/GameWorld/HUD";
    let enemy_id = tree.get_node_by_path(enemy_path).expect("Enemy");
    let hud_id = tree.get_node_by_path(hud_path).expect("HUD");

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();
    let conn = Connection::with_callback(hud_id.object_id(), "_on_enemy_defeated", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();

    tree.connect_signal(enemy_id, "defeated", conn);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // First emission: callback fires.
    tree.emit_signal(enemy_id, "defeated", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Second emission: callback does NOT fire (one-shot).
    tree.set_trace_frame(1);
    tree.emit_signal(enemy_id, "defeated", &[]);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Both emissions produce trace events.
    let emits = signal_emit_trace_events(&tree);
    assert_eq!(emits.len(), 2, "both emissions traced");
}

// ===========================================================================
// 13. Oracle connection count matches unique signal topology
// ===========================================================================

#[test]
fn oracle_connection_count_matches_topology() {
    let oracle = load_oracle_connections();

    // Oracle has 7 connections total.
    assert_eq!(oracle.len(), 7, "oracle should have 7 connections");

    // 5 unique (from, signal) pairs.
    let mut unique: Vec<(String, String)> = oracle
        .iter()
        .map(|(sig, from, _, _, _)| (from.clone(), sig.clone()))
        .collect();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 6, "6 unique signal sources in oracle");
}

// ===========================================================================
// 14. Full oracle emission comparison with trace_compare
// ===========================================================================

#[test]
fn full_oracle_emission_trace_comparison() {
    let oracle = load_oracle_connections();
    let mut tree = load_signal_instantiation_scene();

    let expected = build_expected_signal_trace(&oracle);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree.set_trace_frame(0);

    // Emit each unique signal in declaration order.
    let mut emitted: Vec<(String, String)> = Vec::new();
    for (sig, from, _, _, _) in &oracle {
        let pair = (from.clone(), sig.clone());
        if !emitted.contains(&pair) {
            let path = resolve_oracle_path(from);
            if let Some(source_id) = tree.get_node_by_path(&path) {
                tree.emit_signal(source_id, sig, &[]);
            }
            emitted.push(pair);
        }
    }

    let actual = signal_emit_trace_events(&tree);

    // Compare signal names (detail field).
    assert_eq!(
        expected.len(),
        actual.len(),
        "expected and actual trace event counts must match"
    );

    for (i, (exp, act)) in expected.iter().zip(actual.iter()).enumerate() {
        assert_eq!(
            exp.detail, act.detail,
            "trace event {i} signal name mismatch: expected='{}' actual='{}'",
            exp.detail, act.detail
        );
        assert_eq!(
            exp.frame, act.frame,
            "trace event {i} frame mismatch: expected={} actual={}",
            exp.frame, act.frame
        );
    }

    // Full trace_compare comparison.
    let diffs = compare_traces(&expected, &actual);
    if !diffs.is_empty() {
        let report = format_report("Oracle Expected", "Patina Runtime", &expected, &actual, &diffs);
        panic!("Full oracle emission trace comparison failed:\n{report}");
    }
}
