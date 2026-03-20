//! Frame trace parity tests — compare Patina traces against upstream (Godot) goldens.
//!
//! This is the core comparison for bead pat-9j5. It loads both the Patina-generated
//! trace and the upstream golden (mock or real), runs structural comparison, and
//! reports all parity differences.

mod trace_compare;

use std::path::PathBuf;
use trace_compare::{compare_traces, format_report, parse_events, TraceDiff};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/golden/traces")
}

fn load_trace(file_name: &str) -> serde_json::Value {
    let path = golden_dir().join(file_name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&content).expect("valid JSON")
}

// ---------------------------------------------------------------------------
// Structural tests on the comparison infrastructure
// ---------------------------------------------------------------------------

#[test]
fn comparison_detects_identical_traces() {
    let patina = load_trace("test_scripts_patina.json");
    let events = parse_events(&patina["event_trace"]);
    let diffs = compare_traces(&events, &events);
    // Only ordering diffs should appear (same ordering = no diffs)
    let non_ordering: Vec<_> = diffs
        .iter()
        .filter(|d| !matches!(d, TraceDiff::OrderingDiff { .. }))
        .collect();
    assert!(
        non_ordering.is_empty(),
        "identical traces should produce no structural diffs, got: {non_ordering:?}"
    );
}

#[test]
fn comparison_detects_extra_events() {
    let patina = load_trace("test_scripts_patina.json");
    let all_events = parse_events(&patina["event_trace"]);
    let fewer = &all_events[..10]; // upstream has only 10 events
    let diffs = compare_traces(fewer, &all_events);
    // Should detect extras in patina (the events beyond the first 10)
    let extras: Vec<_> = diffs
        .iter()
        .filter(|d| matches!(d, TraceDiff::ExtraInPatina { .. }))
        .collect();
    assert!(!extras.is_empty(), "should detect extra events in Patina");
}

// ---------------------------------------------------------------------------
// Core parity comparison: Patina vs upstream mock golden
// ---------------------------------------------------------------------------

#[test]
fn patina_vs_upstream_mock_full_comparison() {
    let patina_json = load_trace("test_scripts_patina.json");
    let upstream_json = load_trace("test_scripts_upstream_mock.json");

    let patina_events = parse_events(&patina_json["event_trace"]);
    let upstream_events = parse_events(&upstream_json["event_trace"]);

    let diffs = compare_traces(&upstream_events, &patina_events);
    let report = format_report(
        "test_scripts_upstream_mock",
        "test_scripts_patina",
        &upstream_events,
        &patina_events,
        &diffs,
    );

    // Print the full report for visibility.
    eprintln!("\n{report}");

    // Full parity achieved — no structural differences remain.
    assert!(
        diffs.is_empty(),
        "expected full parity between Patina and upstream, but got {} diffs:\n{report}",
        diffs.len()
    );
}

/// Lifecycle ordering: ENTER_TREE should be top-down in both traces.
#[test]
fn parity_enter_tree_order_matches() {
    let patina_json = load_trace("test_scripts_patina.json");
    let upstream_json = load_trace("test_scripts_upstream_mock.json");

    let patina_events = parse_events(&patina_json["event_trace"]);
    let upstream_events = parse_events(&upstream_json["event_trace"]);

    let patina_et: Vec<_> = patina_events
        .iter()
        .filter(|e| e.detail == "ENTER_TREE")
        .map(|e| e.node_path.as_str())
        .collect();
    let upstream_et: Vec<_> = upstream_events
        .iter()
        .filter(|e| e.detail == "ENTER_TREE")
        .map(|e| e.node_path.as_str())
        .collect();

    assert_eq!(
        patina_et, upstream_et,
        "ENTER_TREE ordering should match between Patina and upstream"
    );
}

/// Lifecycle ordering: READY should be bottom-up in both traces.
#[test]
fn parity_ready_order_matches() {
    let patina_json = load_trace("test_scripts_patina.json");
    let upstream_json = load_trace("test_scripts_upstream_mock.json");

    let patina_events = parse_events(&patina_json["event_trace"]);
    let upstream_events = parse_events(&upstream_json["event_trace"]);

    let patina_ready: Vec<_> = patina_events
        .iter()
        .filter(|e| e.detail == "READY" && e.event_type == "notification")
        .map(|e| e.node_path.as_str())
        .collect();
    let upstream_ready: Vec<_> = upstream_events
        .iter()
        .filter(|e| e.detail == "READY" && e.event_type == "notification")
        .map(|e| e.node_path.as_str())
        .collect();

    assert_eq!(
        patina_ready, upstream_ready,
        "READY ordering should match between Patina and upstream"
    );
}

/// Verify that PROCESS notifications and _process script calls are interleaved per-node,
/// matching Godot's dispatch ordering.
#[test]
fn process_interleaving_matches_godot() {
    let patina_json = load_trace("test_scripts_patina.json");
    let patina_events = parse_events(&patina_json["event_trace"]);

    // In frame 0: find PROCESS notifications and _process script calls.
    let frame0: Vec<_> = patina_events
        .iter()
        .filter(|e| e.frame == 0 && (e.detail == "PROCESS" || e.detail == "_process"))
        .collect();

    // Verify interleaved ordering: PROCESS(Mover) -> _process(Mover) -> PROCESS(VarTest) -> _process(VarTest)
    let mover_process_idx = frame0
        .iter()
        .position(|e| {
            e.detail == "PROCESS"
                && e.event_type == "notification"
                && e.node_path == "/root/TestScene/Mover"
        })
        .expect("Mover PROCESS notification");
    let mover_script_idx = frame0
        .iter()
        .position(|e| {
            e.detail == "_process"
                && e.event_type == "script_call"
                && e.node_path == "/root/TestScene/Mover"
        })
        .expect("Mover _process script call");
    let vartest_process_idx = frame0
        .iter()
        .position(|e| {
            e.detail == "PROCESS"
                && e.event_type == "notification"
                && e.node_path == "/root/TestScene/VarTest"
        })
        .expect("VarTest PROCESS notification");
    let vartest_script_idx = frame0
        .iter()
        .position(|e| {
            e.detail == "_process"
                && e.event_type == "script_call"
                && e.node_path == "/root/TestScene/VarTest"
        })
        .expect("VarTest _process script call");

    // Godot ordering: PROCESS(Mover) < _process(Mover) < PROCESS(VarTest) < _process(VarTest)
    assert!(
        mover_process_idx < mover_script_idx,
        "Mover PROCESS should come before Mover _process"
    );
    assert!(
        mover_script_idx < vartest_process_idx,
        "Mover _process should come before VarTest PROCESS (interleaved per-node)"
    );
    assert!(
        vartest_process_idx < vartest_script_idx,
        "VarTest PROCESS should come before VarTest _process"
    );
}

/// FIXED: Patina no longer generates _physics_process calls for scripts that don't define it.
#[test]
fn no_spurious_physics_process_calls() {
    let patina_json = load_trace("test_scripts_patina.json");

    let patina_events = parse_events(&patina_json["event_trace"]);

    let patina_phys_scripts: Vec<_> = patina_events
        .iter()
        .filter(|e| e.detail == "_physics_process" && e.event_type == "script_call")
        .collect();

    assert!(
        patina_phys_scripts.is_empty(),
        "Patina should NOT generate _physics_process script calls for scripts that don't define it, got {}",
        patina_phys_scripts.len()
    );
}

/// FIXED: Patina no longer generates _enter_tree calls for scripts that don't define it.
#[test]
fn no_spurious_enter_tree_calls() {
    let patina_json = load_trace("test_scripts_patina.json");

    let patina_events = parse_events(&patina_json["event_trace"]);

    let patina_enter_scripts: Vec<_> = patina_events
        .iter()
        .filter(|e| e.detail == "_enter_tree" && e.event_type == "script_call")
        .collect();

    assert!(
        patina_enter_scripts.is_empty(),
        "Patina should NOT generate _enter_tree script calls for scripts that don't define it, got {}",
        patina_enter_scripts.len()
    );
}

/// FIXED: Patina no longer calls _ready on Mover (test_movement.gd doesn't define _ready).
#[test]
fn no_spurious_ready_call_mover() {
    let patina_json = load_trace("test_scripts_patina.json");

    let patina_events = parse_events(&patina_json["event_trace"]);

    let patina_mover_ready: Vec<_> = patina_events
        .iter()
        .filter(|e| {
            e.detail == "_ready"
                && e.event_type == "script_call"
                && e.node_path == "/root/TestScene/Mover"
        })
        .collect();

    assert!(
        patina_mover_ready.is_empty(),
        "Patina should NOT call _ready on Mover (test_movement.gd doesn't define it)"
    );
}

/// Event counts now match between Patina and upstream mock golden.
#[test]
fn event_count_matches_upstream() {
    let patina_json = load_trace("test_scripts_patina.json");
    let upstream_json = load_trace("test_scripts_upstream_mock.json");

    let patina_events = parse_events(&patina_json["event_trace"]);
    let upstream_events = parse_events(&upstream_json["event_trace"]);

    assert_eq!(
        patina_events.len(),
        upstream_events.len(),
        "Patina event count ({}) should match upstream ({})",
        patina_events.len(),
        upstream_events.len()
    );
}
