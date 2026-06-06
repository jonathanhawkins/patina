//! Multi-scene trace parity tests (pat-fbi).
//!
//! Extends trace coverage beyond test_scripts to all fixture scenes, verifying
//! that Patina's lifecycle dispatch matches Godot's documented behavior across
//! diverse scene types:
//! - minimal: single node, baseline
//! - hierarchy: nested nodes (parent/child/grandchild)
//! - platformer: physics bodies, collision shapes, cameras
//! - space_shooter: scripts with _process, multiple node types
//! - physics_playground: RigidBody2D, StaticBody2D, CollisionShapes
//! - ui_menu: Control nodes (Button, Label)
//! - signals_complex: signal connections between nodes

mod oracle_fixture;
mod trace_compare;

use oracle_fixture::fixtures_dir;
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

fn run_scene_trace(scene_name: &str, frames: u64) -> serde_json::Value {
    let scene_path = fixtures_dir().join("scenes").join(scene_name);
    let binary = runner_binary();
    let output = std::process::Command::new(&binary)
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

// ===========================================================================
// Helpers for lifecycle assertions
// ===========================================================================

fn assert_enter_tree_top_down(events: &[trace_compare::TraceEvent], scene_name: &str) {
    let enter_paths: Vec<&str> = events
        .iter()
        .filter(|e| e.detail == "ENTER_TREE")
        .map(|e| e.node_path.as_str())
        .collect();

    // Each node's ENTER_TREE should appear before any of its children.
    for (i, path) in enter_paths.iter().enumerate() {
        for later_path in &enter_paths[i + 1..] {
            if later_path.starts_with(path) && later_path.len() > path.len() {
                // later_path is a descendant of path — correct (parent first).
            }
            // If path starts with later_path, that's wrong (child before parent).
            assert!(
                !path.starts_with(later_path) || path.len() <= later_path.len(),
                "{scene_name}: ENTER_TREE child {path} appeared before parent {later_path}"
            );
        }
    }
}

fn assert_ready_bottom_up(events: &[trace_compare::TraceEvent], scene_name: &str) {
    let ready_paths: Vec<&str> = events
        .iter()
        .filter(|e| e.detail == "READY" && e.event_type == "notification")
        .map(|e| e.node_path.as_str())
        .collect();

    // Each node's READY should appear after all its children's READY.
    for (i, path) in ready_paths.iter().enumerate() {
        for earlier_path in &ready_paths[..i] {
            // If an earlier READY is a parent of this path, that's wrong.
            if path.starts_with(earlier_path)
                && path.len() > earlier_path.len()
                && path.as_bytes().get(earlier_path.len()) == Some(&b'/')
            {
                panic!("{scene_name}: READY parent {earlier_path} appeared before child {path}");
            }
        }
    }
}

fn assert_all_enter_tree_before_ready(events: &[trace_compare::TraceEvent], scene_name: &str) {
    let last_enter = events
        .iter()
        .rposition(|e| e.detail == "ENTER_TREE")
        .expect(&format!("{scene_name}: no ENTER_TREE events"));
    let first_ready = events
        .iter()
        .position(|e| e.detail == "READY")
        .expect(&format!("{scene_name}: no READY events"));

    assert!(
        last_enter < first_ready,
        "{scene_name}: last ENTER_TREE (idx={last_enter}) should precede first READY (idx={first_ready})"
    );
}

fn assert_frame_monotonic(events: &[trace_compare::TraceEvent], scene_name: &str) {
    let mut last_frame = 0u64;
    for (i, e) in events.iter().enumerate() {
        assert!(
            e.frame >= last_frame,
            "{scene_name}: frame number decreased at event {i}: {} after {}",
            e.frame,
            last_frame
        );
        last_frame = e.frame;
    }
}

fn assert_script_calls_paired(events: &[trace_compare::TraceEvent], scene_name: &str) {
    let calls: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "script_call")
        .collect();
    let returns: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "script_return")
        .collect();
    assert_eq!(
        calls.len(),
        returns.len(),
        "{scene_name}: script call/return count mismatch: {} calls, {} returns",
        calls.len(),
        returns.len()
    );
    for (call, ret) in calls.iter().zip(returns.iter()) {
        assert_eq!(
            call.node_path, ret.node_path,
            "{scene_name}: call/return node_path mismatch"
        );
        assert_eq!(
            call.detail, ret.detail,
            "{scene_name}: call/return detail mismatch"
        );
    }
}

fn assert_process_interleaved(events: &[trace_compare::TraceEvent], scene_name: &str) {
    // For each frame, verify PROCESS notification + _process script call are
    // interleaved per-node (not batched).
    let max_frame = events.iter().map(|e| e.frame).max().unwrap_or(0);

    for frame in 0..=max_frame {
        let frame_events: Vec<_> = events
            .iter()
            .filter(|e| e.frame == frame && (e.detail == "PROCESS" || e.detail == "_process"))
            .collect();

        // Collect nodes that have both PROCESS notification and _process script call.
        let mut last_process_node: Option<&str> = None;
        for e in &frame_events {
            if e.detail == "_process" && e.event_type == "script_call" {
                // _process script call should immediately follow that node's PROCESS notification.
                assert_eq!(
                    last_process_node,
                    Some(e.node_path.as_str()),
                    "{scene_name} frame {frame}: _process for {} should follow its PROCESS notification",
                    e.node_path
                );
            }
            if e.detail == "PROCESS" && e.event_type == "notification" {
                last_process_node = Some(&e.node_path);
            }
        }
    }
}

// ===========================================================================
// Per-scene lifecycle correctness tests
// ===========================================================================

macro_rules! scene_lifecycle_test {
    ($test_name:ident, $scene_file:expr, $scene_label:expr) => {
        #[test]
        fn $test_name() {
            let output = run_scene_trace($scene_file, 3);
            let events = parse_events(&output["event_trace"]);

            assert_enter_tree_top_down(&events, $scene_label);
            assert_ready_bottom_up(&events, $scene_label);
            assert_all_enter_tree_before_ready(&events, $scene_label);
            assert_frame_monotonic(&events, $scene_label);
            assert_script_calls_paired(&events, $scene_label);
            assert_process_interleaved(&events, $scene_label);
        }
    };
}

scene_lifecycle_test!(lifecycle_minimal, "minimal.tscn", "minimal");
scene_lifecycle_test!(lifecycle_hierarchy, "hierarchy.tscn", "hierarchy");
scene_lifecycle_test!(lifecycle_platformer, "platformer.tscn", "platformer");
scene_lifecycle_test!(
    lifecycle_space_shooter,
    "space_shooter.tscn",
    "space_shooter"
);
scene_lifecycle_test!(
    lifecycle_physics_playground,
    "physics_playground.tscn",
    "physics_playground"
);
scene_lifecycle_test!(lifecycle_ui_menu, "ui_menu.tscn", "ui_menu");
scene_lifecycle_test!(
    lifecycle_signals_complex,
    "signals_complex.tscn",
    "signals_complex"
);

// ===========================================================================
// Golden trace regression tests — live output must match stored golden
// ===========================================================================

macro_rules! golden_regression_test {
    ($test_name:ident, $scene_file:expr, $golden_file:expr, $scene_label:expr) => {
        #[test]
        fn $test_name() {
            let live = run_scene_trace($scene_file, 3);
            let golden = load_trace($golden_file);

            let live_events = parse_events(&live["event_trace"]);
            let golden_events = parse_events(&golden["event_trace"]);

            assert_eq!(
                live_events.len(),
                golden_events.len(),
                "{}: event count mismatch — live={}, golden={}",
                $scene_label,
                live_events.len(),
                golden_events.len()
            );

            let diffs = compare_traces(&golden_events, &live_events);
            if !diffs.is_empty() {
                let report = format_report(
                    &format!("{}_golden", $scene_label),
                    &format!("{}_live", $scene_label),
                    &golden_events,
                    &live_events,
                    &diffs,
                );
                panic!("{}: golden regression detected:\n{report}", $scene_label);
            }
        }
    };
}

golden_regression_test!(
    golden_minimal,
    "minimal.tscn",
    "minimal_patina.json",
    "minimal"
);
golden_regression_test!(
    golden_hierarchy,
    "hierarchy.tscn",
    "hierarchy_patina.json",
    "hierarchy"
);
golden_regression_test!(
    golden_platformer,
    "platformer.tscn",
    "platformer_patina.json",
    "platformer"
);
golden_regression_test!(
    golden_space_shooter,
    "space_shooter.tscn",
    "space_shooter_patina.json",
    "space_shooter"
);
golden_regression_test!(
    golden_physics_playground,
    "physics_playground.tscn",
    "physics_playground_patina.json",
    "physics_playground"
);
golden_regression_test!(
    golden_ui_menu,
    "ui_menu.tscn",
    "ui_menu_patina.json",
    "ui_menu"
);
golden_regression_test!(
    golden_signals_complex,
    "signals_complex.tscn",
    "signals_complex_patina.json",
    "signals_complex"
);
golden_regression_test!(
    golden_character_body_test,
    "character_body_test.tscn",
    "character_body_test_patina.json",
    "character_body_test"
);
golden_regression_test!(
    golden_unique_name_resolution,
    "unique_name_resolution.tscn",
    "unique_name_resolution_patina.json",
    "unique_name_resolution"
);
golden_regression_test!(
    golden_with_properties,
    "with_properties.tscn",
    "with_properties_patina.json",
    "with_properties"
);

// ===========================================================================
// Cross-scene parity: Patina traces match upstream mocks
// ===========================================================================

macro_rules! parity_test {
    ($test_name:ident, $patina_file:expr, $upstream_file:expr, $scene_label:expr) => {
        #[test]
        fn $test_name() {
            let patina_json = load_trace($patina_file);
            let upstream_json = load_trace($upstream_file);

            let patina_events = parse_events(&patina_json["event_trace"]);
            let upstream_events = parse_events(&upstream_json["event_trace"]);

            let diffs = compare_traces(&upstream_events, &patina_events);
            let report = format_report(
                &format!("{}_upstream", $scene_label),
                &format!("{}_patina", $scene_label),
                &upstream_events,
                &patina_events,
                &diffs,
            );

            assert!(
                diffs.is_empty(),
                "{}: expected full parity, got {} diffs:\n{report}",
                $scene_label,
                diffs.len()
            );
        }
    };
}

parity_test!(
    parity_minimal,
    "minimal_patina.json",
    "minimal_upstream_mock.json",
    "minimal"
);
parity_test!(
    parity_hierarchy,
    "hierarchy_patina.json",
    "hierarchy_upstream_mock.json",
    "hierarchy"
);
parity_test!(
    parity_platformer,
    "platformer_patina.json",
    "platformer_upstream_mock.json",
    "platformer"
);
parity_test!(
    parity_space_shooter,
    "space_shooter_patina.json",
    "space_shooter_upstream_mock.json",
    "space_shooter"
);
parity_test!(
    parity_physics_playground,
    "physics_playground_patina.json",
    "physics_playground_upstream_mock.json",
    "physics_playground"
);
parity_test!(
    parity_ui_menu,
    "ui_menu_patina.json",
    "ui_menu_upstream_mock.json",
    "ui_menu"
);
parity_test!(
    parity_signals_complex,
    "signals_complex_patina.json",
    "signals_complex_upstream_mock.json",
    "signals_complex"
);

// ===========================================================================
// Scene-specific feature tests
// ===========================================================================

/// space_shooter has scripts with _process — verify interleaved dispatch.
#[test]
fn space_shooter_script_dispatch_order() {
    let output = run_scene_trace("space_shooter.tscn", 3);
    let events = parse_events(&output["event_trace"]);

    // Verify _process fires for scripted nodes each frame.
    for frame in 0..3 {
        let process_calls: Vec<_> = events
            .iter()
            .filter(|e| {
                e.frame == frame as u64 && e.event_type == "script_call" && e.detail == "_process"
            })
            .map(|e| e.node_path.as_str())
            .collect();

        assert!(
            !process_calls.is_empty(),
            "space_shooter frame {frame}: expected _process script calls"
        );
    }
}

/// physics_playground has CollisionShape children — verify full hierarchy traced.
#[test]
fn physics_playground_collision_shapes_in_tree() {
    let output = run_scene_trace("physics_playground.tscn", 1);
    let events = parse_events(&output["event_trace"]);

    let enter_paths: Vec<&str> = events
        .iter()
        .filter(|e| e.detail == "ENTER_TREE")
        .map(|e| e.node_path.as_str())
        .collect();

    // CollisionShape nodes should be in the enter tree list.
    let collision_shapes: Vec<_> = enter_paths
        .iter()
        .filter(|p| p.contains("CollisionShape"))
        .collect();

    assert!(
        collision_shapes.len() >= 3,
        "physics_playground should have at least 3 CollisionShape nodes in tree, got {}",
        collision_shapes.len()
    );
}

/// signals_complex has signal connections — verify node structure is traced.
#[test]
fn signals_complex_all_nodes_traced() {
    let output = run_scene_trace("signals_complex.tscn", 1);
    let events = parse_events(&output["event_trace"]);

    let enter_paths: Vec<&str> = events
        .iter()
        .filter(|e| e.detail == "ENTER_TREE")
        .map(|e| e.node_path.as_str())
        .collect();

    // Should include Player, Enemy, HUD, ItemDrop, and TriggerZone.
    for expected in &["Player", "Enemy", "HUD", "ItemDrop", "TriggerZone"] {
        assert!(
            enter_paths.iter().any(|p| p.contains(expected)),
            "signals_complex should trace {expected} node"
        );
    }
}

/// ui_menu has Control/Button nodes — verify they get lifecycle events.
#[test]
fn ui_menu_control_nodes_lifecycle() {
    let output = run_scene_trace("ui_menu.tscn", 1);
    let events = parse_events(&output["event_trace"]);

    let ready_paths: Vec<&str> = events
        .iter()
        .filter(|e| e.detail == "READY")
        .map(|e| e.node_path.as_str())
        .collect();

    for expected in &["PlayButton", "QuitButton", "SettingsButton", "Title"] {
        assert!(
            ready_paths.iter().any(|p| p.contains(expected)),
            "ui_menu should have READY for {expected}"
        );
    }
}
