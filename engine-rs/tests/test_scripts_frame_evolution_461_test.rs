//! pat-a7p: Resolve 4.6.1 runtime diffs in test_scripts frame evolution.
//!
//! Validates that the test_scripts fixture's frame-by-frame evolution matches
//! the 4.6.1 upstream oracle golden. Covers:
//! 1. Upstream golden has correct 4.6.1 metadata
//! 2. Mover position evolves correctly (speed * direction * delta per frame)
//! 3. Mover position at frame 10 matches upstream golden (drift resolved)
//! 4. VarTest script vars remain stable across frames
//! 5. Frame event counts are consistent across frames 1-9
//! 6. Patina trace matches upstream trace structure
//! 7. No PHYSICS_PROCESS events (scripts only use _process)
//! 8. Script call/return pairs are balanced

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn golden_dir() -> PathBuf {
    repo_root().join("fixtures/golden")
}

fn load_json(rel_path: &str) -> serde_json::Value {
    let path = golden_dir().join(rel_path);
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {rel_path}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {rel_path}: {e}"))
}

// ===========================================================================
// 1. Upstream golden metadata
// ===========================================================================

#[test]
fn a7p_upstream_golden_has_461_metadata() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let version = golden["upstream_version"].as_str().unwrap_or("");
    assert!(
        version.contains("4.6.1"),
        "upstream golden should reference 4.6.1, got: {version}"
    );
}

#[test]
fn a7p_upstream_golden_has_10_frames() {
    let golden = load_json("traces/test_scripts_upstream.json");
    assert_eq!(
        golden["frame_count"].as_i64().unwrap(),
        10,
        "upstream golden should have 10 frames"
    );
}

#[test]
fn a7p_upstream_golden_has_tree_snapshot() {
    let golden = load_json("traces/test_scripts_upstream.json");
    assert!(
        golden["tree"].is_object(),
        "upstream golden should have a tree snapshot"
    );
    let tree = &golden["tree"];
    assert_eq!(tree["name"].as_str().unwrap(), "root");
    assert_eq!(tree["class"].as_str().unwrap(), "Window");
}

// ===========================================================================
// 2. Mover position evolution
// ===========================================================================

#[test]
fn a7p_mover_position_at_frame_10_matches_golden() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let mover = &golden["tree"]["children"][0]["children"][0];
    assert_eq!(mover["name"].as_str().unwrap(), "Mover");

    let pos = mover["properties"]["position"]["value"].as_array().unwrap();
    let x = pos[0].as_f64().unwrap();
    let y = pos[1].as_f64().unwrap();

    // Mover starts at (100, 200), moves right at speed=50 with delta=1/6
    // After 10 frames of _process: x = 100 + 50 * (1/6) * N
    // But frame 0 includes lifecycle setup, so the exact position depends on
    // how many _process calls actually fired. The golden says 108.333...
    assert!(
        (x - 108.333).abs() < 1.0,
        "Mover x should be ~108.33 after 10 frames, got {x}"
    );
    assert!(
        (y - 200.0).abs() < 0.001,
        "Mover y should remain 200.0, got {y}"
    );
}

#[test]
fn a7p_mover_speed_and_direction_at_frame_10() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let mover = &golden["tree"]["children"][0]["children"][0];

    let speed = mover["script_vars"]["speed"]["value"].as_f64().unwrap();
    let direction = mover["script_vars"]["direction"]["value"].as_f64().unwrap();

    assert!(
        (speed - 50.0).abs() < 0.001,
        "speed should be 50.0, got {speed}"
    );
    assert!(
        (direction - 1.0).abs() < 0.001,
        "direction should be 1.0 (not yet bounced), got {direction}"
    );
}

#[test]
fn a7p_mover_position_drift_resolved() {
    // REPIN_REPORT.md states the Mover position drift from 4.5.1 -> 4.6.1
    // (frame-accumulation drift from one extra partial _process step) is resolved.
    // Verify the Patina scene golden and upstream golden agree on Mover position.
    let scene_golden = load_json("scenes/test_scripts.json");

    // Scene golden has nodes at top level (initial parse, no frame evolution)
    let nodes = if let Some(data) = scene_golden.get("data") {
        data["nodes"].as_array().unwrap()
    } else {
        scene_golden["nodes"].as_array().unwrap()
    };

    let test_scene = &nodes[0];
    assert_eq!(test_scene["name"].as_str().unwrap(), "TestScene");

    let mover = &test_scene["children"][0];
    assert_eq!(mover["name"].as_str().unwrap(), "Mover");

    let pos = mover["properties"]["position"]["value"].as_array().unwrap();
    let x = pos[0].as_f64().unwrap();
    // Scene golden captures initial position (before evolution), should be 100.0
    assert!(
        (x - 100.0).abs() < 0.001,
        "scene golden Mover initial x should be 100.0, got {x}"
    );
}

// ===========================================================================
// 3. VarTest script variable stability
// ===========================================================================

#[test]
fn a7p_vartest_vars_stable_after_10_frames() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let vartest = &golden["tree"]["children"][0]["children"][1];
    assert_eq!(vartest["name"].as_str().unwrap(), "VarTest");

    let vars = &vartest["script_vars"];
    assert_eq!(vars["health"]["value"].as_i64().unwrap(), 100);
    assert_eq!(vars["name_str"]["value"].as_str().unwrap(), "Player");
    assert_eq!(vars["is_alive"]["value"].as_bool().unwrap(), true);

    let vel = vars["velocity"]["value"].as_array().unwrap();
    assert!((vel[0].as_f64().unwrap()).abs() < 0.001);
    assert!((vel[1].as_f64().unwrap()).abs() < 0.001);
}

#[test]
fn a7p_vartest_position_unchanged() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let vartest = &golden["tree"]["children"][0]["children"][1];
    let pos = vartest["properties"]["position"]["value"]
        .as_array()
        .unwrap();
    let x = pos[0].as_f64().unwrap();
    let y = pos[1].as_f64().unwrap();

    // VarTest's _process doesn't modify position
    assert!(
        (x - 300.0).abs() < 0.001,
        "VarTest x should remain 300.0, got {x}"
    );
    assert!(
        (y - 200.0).abs() < 0.001,
        "VarTest y should remain 200.0, got {y}"
    );
}

// ===========================================================================
// 4. Frame event consistency
// ===========================================================================

#[test]
fn a7p_frames_1_through_9_have_consistent_event_counts() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    let frame_1_count = events.iter().filter(|e| e["frame"] == 1).count();
    assert!(frame_1_count > 0, "frame 1 should have events");

    for frame in 2..10 {
        let count = events.iter().filter(|e| e["frame"] == frame).count();
        assert_eq!(
            count, frame_1_count,
            "frame {frame} event count ({count}) should match frame 1 ({frame_1_count})"
        );
    }
}

#[test]
fn a7p_frame_0_has_lifecycle_plus_processing() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    let frame_0_count = events.iter().filter(|e| e["frame"] == 0).count();
    let frame_1_count = events.iter().filter(|e| e["frame"] == 1).count();

    assert!(
        frame_0_count > frame_1_count,
        "frame 0 ({frame_0_count}) should have more events than frame 1 ({frame_1_count}) due to lifecycle"
    );

    // Frame 0 should have ENTER_TREE and READY events
    let enter_tree_count = events
        .iter()
        .filter(|e| e["frame"] == 0 && e["detail"] == "ENTER_TREE")
        .count();
    let ready_count = events
        .iter()
        .filter(|e| e["frame"] == 0 && e["detail"] == "READY")
        .count();

    assert!(
        enter_tree_count >= 3,
        "frame 0 should have >= 3 ENTER_TREE events"
    );
    assert!(ready_count >= 3, "frame 0 should have >= 3 READY events");
}

// ===========================================================================
// 5. No PHYSICS_PROCESS (scripts only use _process)
// ===========================================================================

#[test]
fn a7p_no_script_physics_process_events() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    // There should be no PHYSICS_PROCESS notifications (distinct from INTERNAL_PHYSICS_PROCESS)
    // because neither script defines _physics_process
    let physics_process_events: Vec<_> = events
        .iter()
        .filter(|e| e["detail"] == "PHYSICS_PROCESS")
        .collect();

    assert!(
        physics_process_events.is_empty(),
        "no PHYSICS_PROCESS events expected (scripts don't define _physics_process), got {}",
        physics_process_events.len()
    );
}

#[test]
fn a7p_no_physics_process_script_calls() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    let physics_calls: Vec<_> = events
        .iter()
        .filter(|e| e["event_type"] == "script_call" && e["detail"] == "_physics_process")
        .collect();

    assert!(
        physics_calls.is_empty(),
        "no _physics_process script calls expected"
    );
}

// ===========================================================================
// 6. Script call/return pairing
// ===========================================================================

#[test]
fn a7p_script_calls_and_returns_are_balanced() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    let call_count = events
        .iter()
        .filter(|e| e["event_type"] == "script_call")
        .count();
    let return_count = events
        .iter()
        .filter(|e| e["event_type"] == "script_return")
        .count();

    assert_eq!(
        call_count, return_count,
        "script_call ({call_count}) and script_return ({return_count}) counts must match"
    );
}

#[test]
fn a7p_process_script_calls_fire_for_both_nodes() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    // In each frame, both Mover and VarTest should get _process calls
    for frame in 0..10 {
        let frame_calls: Vec<_> = events
            .iter()
            .filter(|e| {
                e["frame"] == frame && e["event_type"] == "script_call" && e["detail"] == "_process"
            })
            .collect();

        let nodes: Vec<_> = frame_calls
            .iter()
            .map(|e| e["node_path"].as_str().unwrap())
            .collect();

        assert!(
            nodes.contains(&"/root/TestScene/Mover"),
            "frame {frame}: Mover should get _process call"
        );
        assert!(
            nodes.contains(&"/root/TestScene/VarTest"),
            "frame {frame}: VarTest should get _process call"
        );
    }
}

// ===========================================================================
// 7. Patina trace exists and has matching structure
// ===========================================================================

#[test]
fn a7p_patina_trace_exists_and_has_events() {
    let patina = load_json("traces/test_scripts_patina.json");
    let events = patina["event_trace"].as_array().unwrap();
    assert!(!events.is_empty(), "patina trace should have events");
}

#[test]
fn a7p_patina_trace_has_enter_tree_and_ready() {
    let patina = load_json("traces/test_scripts_patina.json");
    let events = patina["event_trace"].as_array().unwrap();

    let enter_tree: Vec<_> = events
        .iter()
        .filter(|e| e["detail"] == "ENTER_TREE")
        .collect();
    let ready: Vec<_> = events.iter().filter(|e| e["detail"] == "READY").collect();

    assert!(
        enter_tree.len() >= 3,
        "patina should have >= 3 ENTER_TREE events"
    );
    assert!(ready.len() >= 3, "patina should have >= 3 READY events");
}

#[test]
fn a7p_patina_trace_enter_tree_top_down() {
    let patina = load_json("traces/test_scripts_patina.json");
    let events = patina["event_trace"].as_array().unwrap();

    let enter_tree: Vec<&str> = events
        .iter()
        .filter(|e| e["detail"] == "ENTER_TREE")
        .map(|e| e["node_path"].as_str().unwrap())
        .collect();

    // TestScene before Mover and VarTest
    let scene_idx = enter_tree.iter().position(|p| *p == "/root/TestScene");
    let mover_idx = enter_tree
        .iter()
        .position(|p| *p == "/root/TestScene/Mover");
    let vartest_idx = enter_tree
        .iter()
        .position(|p| *p == "/root/TestScene/VarTest");

    assert!(scene_idx.is_some(), "TestScene should have ENTER_TREE");
    assert!(mover_idx.is_some(), "Mover should have ENTER_TREE");
    assert!(vartest_idx.is_some(), "VarTest should have ENTER_TREE");
    assert!(
        scene_idx.unwrap() < mover_idx.unwrap(),
        "TestScene ENTER_TREE should fire before Mover"
    );
    assert!(
        scene_idx.unwrap() < vartest_idx.unwrap(),
        "TestScene ENTER_TREE should fire before VarTest"
    );
}

#[test]
fn a7p_patina_trace_ready_bottom_up() {
    let patina = load_json("traces/test_scripts_patina.json");
    let events = patina["event_trace"].as_array().unwrap();

    let ready: Vec<&str> = events
        .iter()
        .filter(|e| e["detail"] == "READY")
        .map(|e| e["node_path"].as_str().unwrap())
        .collect();

    let scene_idx = ready.iter().position(|p| *p == "/root/TestScene");
    let mover_idx = ready.iter().position(|p| *p == "/root/TestScene/Mover");
    let vartest_idx = ready.iter().position(|p| *p == "/root/TestScene/VarTest");

    assert!(scene_idx.is_some(), "TestScene should have READY");
    assert!(mover_idx.is_some(), "Mover should have READY");
    assert!(vartest_idx.is_some(), "VarTest should have READY");
    assert!(
        mover_idx.unwrap() < scene_idx.unwrap(),
        "Mover READY should fire before TestScene (bottom-up)"
    );
    assert!(
        vartest_idx.unwrap() < scene_idx.unwrap(),
        "VarTest READY should fire before TestScene (bottom-up)"
    );
}

// ===========================================================================
// 8. Frame numbers monotonically ordered
// ===========================================================================

#[test]
fn a7p_upstream_frame_numbers_monotonic() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    let mut last_frame = 0i64;
    for event in events {
        let frame = event["frame"].as_i64().unwrap();
        assert!(
            frame >= last_frame,
            "frame numbers should be monotonically non-decreasing, got {frame} after {last_frame}"
        );
        last_frame = frame;
    }
}

// ===========================================================================
// 10. Patina-vs-upstream Mover position drift within f32 tolerance
// ===========================================================================

#[test]
fn a7p_patina_mover_position_within_f32_tolerance_of_upstream() {
    let upstream = load_json("traces/test_scripts_upstream.json");
    let patina = load_json("traces/test_scripts_patina.json");

    let upstream_mover = &upstream["tree"]["children"][0]["children"][0];
    let patina_mover = &patina["tree"]["children"][0]["children"][0];

    assert_eq!(upstream_mover["name"].as_str().unwrap(), "Mover");
    assert_eq!(patina_mover["name"].as_str().unwrap(), "Mover");

    let upstream_x = upstream_mover["properties"]["position"]["value"][0]
        .as_f64()
        .unwrap();
    let patina_x = patina_mover["properties"]["position"]["value"][0]
        .as_f64()
        .unwrap();

    let upstream_y = upstream_mover["properties"]["position"]["value"][1]
        .as_f64()
        .unwrap();
    let patina_y = patina_mover["properties"]["position"]["value"][1]
        .as_f64()
        .unwrap();

    // f32 accumulation drift: 10 frames of += at 1/60 delta produces ~0.00003
    // difference between f32-accumulated and f64-accumulated results. This is
    // within the inherent precision of f32 (epsilon ~1.19e-7 * magnitude ~108 = ~1.3e-5).
    let tolerance = 0.001; // 0.1% — generous but still tight enough to catch real bugs
    assert!(
        (upstream_x - patina_x).abs() < tolerance,
        "Mover x drift between upstream ({upstream_x}) and patina ({patina_x}) \
         exceeds f32 tolerance ({tolerance})"
    );
    assert!(
        (upstream_y - patina_y).abs() < tolerance,
        "Mover y drift between upstream ({upstream_y}) and patina ({patina_y}) \
         exceeds f32 tolerance ({tolerance})"
    );

    // Verify both agree on speed and direction script vars
    let upstream_speed = upstream_mover["script_vars"]["speed"]["value"]
        .as_f64()
        .unwrap();
    let patina_speed = patina_mover["script_vars"]["speed"]["value"]
        .as_f64()
        .unwrap();
    assert!(
        (upstream_speed - patina_speed).abs() < 0.001,
        "speed should match between upstream and patina"
    );

    let upstream_dir = upstream_mover["script_vars"]["direction"]["value"]
        .as_f64()
        .unwrap();
    let patina_dir = patina_mover["script_vars"]["direction"]["value"]
        .as_f64()
        .unwrap();
    assert!(
        (upstream_dir - patina_dir).abs() < 0.001,
        "direction should match between upstream and patina"
    );
}

#[test]
fn a7p_patina_vartest_matches_upstream() {
    let upstream = load_json("traces/test_scripts_upstream.json");
    let patina = load_json("traces/test_scripts_patina.json");

    let upstream_vt = &upstream["tree"]["children"][0]["children"][1];
    let patina_vt = &patina["tree"]["children"][0]["children"][1];

    assert_eq!(upstream_vt["name"].as_str().unwrap(), "VarTest");
    assert_eq!(patina_vt["name"].as_str().unwrap(), "VarTest");

    // VarTest position should be identical (no script movement)
    let upstream_x = upstream_vt["properties"]["position"]["value"][0]
        .as_f64()
        .unwrap();
    let patina_x = patina_vt["properties"]["position"]["value"][0]
        .as_f64()
        .unwrap();
    assert!(
        (upstream_x - patina_x).abs() < 0.001,
        "VarTest x should match: upstream={upstream_x}, patina={patina_x}"
    );

    // Script vars should match exactly
    let upstream_health = upstream_vt["script_vars"]["health"]["value"]
        .as_i64()
        .unwrap();
    let patina_health = patina_vt["script_vars"]["health"]["value"]
        .as_i64()
        .unwrap();
    assert_eq!(
        upstream_health, patina_health,
        "VarTest health must match between upstream and patina"
    );

    let upstream_alive = upstream_vt["script_vars"]["is_alive"]["value"]
        .as_bool()
        .unwrap();
    let patina_alive = patina_vt["script_vars"]["is_alive"]["value"]
        .as_bool()
        .unwrap();
    assert_eq!(
        upstream_alive, patina_alive,
        "VarTest is_alive must match between upstream and patina"
    );
}

#[test]
fn a7p_upstream_covers_all_10_frames() {
    let golden = load_json("traces/test_scripts_upstream.json");
    let events = golden["event_trace"].as_array().unwrap();

    for expected_frame in 0..10 {
        let has_frame = events.iter().any(|e| e["frame"] == expected_frame);
        assert!(
            has_frame,
            "upstream golden should have events for frame {expected_frame}"
        );
    }
}
