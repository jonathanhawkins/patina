//! Frame trace golden tests — verify Patina frame trace output for test_scripts scene.
//!
//! These tests run the Patina runner with `--event-trace` and verify:
//! - Lifecycle ordering: ENTER_TREE (top-down), READY (bottom-up)
//! - Per-frame _process dispatch ordering (tree order)
//! - Script calls are properly bracketed (call/return pairs)
//! - Frame numbers are monotonically increasing
//! - The output matches the stored golden trace

mod oracle_fixture;

use oracle_fixture::fixtures_dir;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Runner helpers
// ---------------------------------------------------------------------------

fn runner_binary() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let release = manifest_dir.join("target/release/patina-runner");
    if release.exists() {
        return release;
    }
    let debug = manifest_dir.join("target/debug/patina-runner");
    if debug.exists() {
        return debug;
    }
    panic!("patina-runner binary not found. Run `cargo build -p patina-runner` first.");
}

fn run_with_event_trace(scene_name: &str, frames: u64) -> Value {
    let scene_path = fixtures_dir().join("scenes").join(scene_name);
    let binary = runner_binary();
    let output = Command::new(&binary)
        .arg(scene_path.to_str().expect("valid UTF-8"))
        .arg("--frames")
        .arg(frames.to_string())
        .arg("--event-trace")
        .output()
        .unwrap_or_else(|e| panic!("failed to execute patina-runner: {e}"));
    assert!(
        output.status.success(),
        "patina-runner failed on {scene_name}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("invalid JSON from patina-runner for {scene_name}:\n{e}"))
}

fn extract_events(output: &Value) -> Vec<&Value> {
    output["event_trace"]
        .as_array()
        .expect("event_trace should be an array")
        .iter()
        .collect()
}

fn events_of_type<'a>(events: &[&'a Value], event_type: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|e| e["event_type"].as_str() == Some(event_type))
        .copied()
        .collect()
}

fn events_with_detail<'a>(events: &[&'a Value], detail: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|e| e["detail"].as_str() == Some(detail))
        .copied()
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// ENTER_TREE notifications fire top-down: parent before children.
#[test]
fn enter_tree_fires_top_down() {
    let output = run_with_event_trace("test_scripts.tscn", 1);
    let events = extract_events(&output);
    let enter_tree = events_with_detail(&events_of_type(&events, "notification"), "ENTER_TREE");

    assert!(
        enter_tree.len() >= 3,
        "expected at least 3 ENTER_TREE events (TestScene, Mover, VarTest)"
    );

    // Find positions of TestScene, Mover, VarTest.
    let scene_idx = enter_tree
        .iter()
        .position(|e| e["node_path"] == "/root/TestScene")
        .expect("TestScene ENTER_TREE");
    let mover_idx = enter_tree
        .iter()
        .position(|e| e["node_path"] == "/root/TestScene/Mover")
        .expect("Mover ENTER_TREE");
    let var_idx = enter_tree
        .iter()
        .position(|e| e["node_path"] == "/root/TestScene/VarTest")
        .expect("VarTest ENTER_TREE");

    assert!(
        scene_idx < mover_idx,
        "TestScene ENTER_TREE should fire before Mover"
    );
    assert!(
        scene_idx < var_idx,
        "TestScene ENTER_TREE should fire before VarTest"
    );
}

/// READY notifications fire bottom-up: children before parent.
#[test]
fn ready_fires_bottom_up() {
    let output = run_with_event_trace("test_scripts.tscn", 1);
    let events = extract_events(&output);
    let ready = events_with_detail(&events_of_type(&events, "notification"), "READY");

    assert!(
        ready.len() >= 3,
        "expected at least 3 READY events (Mover, VarTest, TestScene)"
    );

    let mover_idx = ready
        .iter()
        .position(|e| e["node_path"] == "/root/TestScene/Mover")
        .expect("Mover READY");
    let var_idx = ready
        .iter()
        .position(|e| e["node_path"] == "/root/TestScene/VarTest")
        .expect("VarTest READY");
    let scene_idx = ready
        .iter()
        .position(|e| e["node_path"] == "/root/TestScene")
        .expect("TestScene READY");

    assert!(
        mover_idx < scene_idx,
        "Mover READY should fire before TestScene"
    );
    assert!(
        var_idx < scene_idx,
        "VarTest READY should fire before TestScene"
    );
}

/// All ENTER_TREE events fire before any READY events.
#[test]
fn all_enter_tree_before_ready() {
    let output = run_with_event_trace("test_scripts.tscn", 1);
    let events = extract_events(&output);
    let notifications = events_of_type(&events, "notification");

    let last_enter = notifications
        .iter()
        .rposition(|e| e["detail"] == "ENTER_TREE")
        .expect("at least one ENTER_TREE");
    let first_ready = notifications
        .iter()
        .position(|e| e["detail"] == "READY")
        .expect("at least one READY");

    assert!(
        last_enter < first_ready,
        "all ENTER_TREE events should complete before any READY event"
    );
}

/// Script calls come in matched call/return pairs.
#[test]
fn script_calls_are_paired() {
    let output = run_with_event_trace("test_scripts.tscn", 5);
    let events = extract_events(&output);
    let calls = events_of_type(&events, "script_call");
    let returns = events_of_type(&events, "script_return");

    assert_eq!(
        calls.len(),
        returns.len(),
        "every script_call should have a matching script_return"
    );

    // Verify each call has a corresponding return with same node_path and detail.
    for (call, ret) in calls.iter().zip(returns.iter()) {
        assert_eq!(
            call["node_path"], ret["node_path"],
            "call/return node_path mismatch"
        );
        assert_eq!(call["detail"], ret["detail"], "call/return detail mismatch");
    }
}

/// Frame numbers are monotonically non-decreasing across all events.
#[test]
fn frame_numbers_monotonic() {
    let output = run_with_event_trace("test_scripts.tscn", 10);
    let events = extract_events(&output);

    let mut last_frame = 0u64;
    for ev in &events {
        let frame = ev["frame"].as_u64().expect("frame should be a number");
        assert!(
            frame >= last_frame,
            "frame numbers must be non-decreasing, got {} after {}",
            frame,
            last_frame
        );
        last_frame = frame;
    }
}

/// Per-frame PROCESS notifications fire for all scene nodes in tree order.
#[test]
fn process_notifications_fire_each_frame() {
    let output = run_with_event_trace("test_scripts.tscn", 3);
    let events = extract_events(&output);

    // Check that PROCESS notifications exist for frames 0, 1, 2.
    for frame in 0..3 {
        let frame_process: Vec<_> = events
            .iter()
            .filter(|e| {
                e["event_type"] == "notification"
                    && e["detail"] == "PROCESS"
                    && e["frame"].as_u64() == Some(frame)
            })
            .collect();
        assert!(
            !frame_process.is_empty(),
            "expected PROCESS notifications for frame {frame}"
        );
    }
}

/// _process script callbacks fire for nodes with scripts each frame.
#[test]
fn script_process_fires_each_frame() {
    let output = run_with_event_trace("test_scripts.tscn", 3);
    let events = extract_events(&output);

    for frame in 0..3 {
        let mover_calls: Vec<_> = events
            .iter()
            .filter(|e| {
                e["event_type"] == "script_call"
                    && e["detail"] == "_process"
                    && e["node_path"] == "/root/TestScene/Mover"
                    && e["frame"].as_u64() == Some(frame)
            })
            .collect();
        assert_eq!(
            mover_calls.len(),
            1,
            "Mover should have exactly one _process script_call on frame {frame}"
        );
    }
}

/// The golden trace file matches the current runner output.
#[test]
fn patina_trace_matches_golden() {
    let output = run_with_event_trace("test_scripts.tscn", 10);
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/traces/test_scripts_patina.json");
    let golden_str = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        panic!(
            "failed to read golden trace at {}: {e}",
            golden_path.display()
        )
    });
    let golden: Value = serde_json::from_str(&golden_str).expect("golden should be valid JSON");

    // Compare event traces.
    let output_events = output["event_trace"]
        .as_array()
        .expect("output event_trace");
    let golden_events = golden["event_trace"]
        .as_array()
        .expect("golden event_trace");

    assert_eq!(
        output_events.len(),
        golden_events.len(),
        "event count mismatch: got {} events, golden has {}",
        output_events.len(),
        golden_events.len()
    );

    for (i, (got, want)) in output_events.iter().zip(golden_events.iter()).enumerate() {
        assert_eq!(
            got["event_type"], want["event_type"],
            "event {i}: event_type mismatch"
        );
        assert_eq!(
            got["node_path"], want["node_path"],
            "event {i}: node_path mismatch"
        );
        assert_eq!(got["detail"], want["detail"], "event {i}: detail mismatch");
        assert_eq!(got["frame"], want["frame"], "event {i}: frame mismatch");
    }
}
