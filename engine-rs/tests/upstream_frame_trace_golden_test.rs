//! pat-2xu9: Validate the upstream frame-trace golden for `test_scripts`.
//!
//! These tests verify that the upstream golden (generated from the oracle
//! output + Godot's behavioral contract) has the correct structure and
//! encodes Godot's expected notification ordering:
//!
//! 1. Golden file exists and parses as valid JSON
//! 2. Event trace has expected event count and frame distribution
//! 3. ENTER_TREE fires top-down (parent before children)
//! 4. READY fires bottom-up (children before parent)
//! 5. _ready script calls are paired and fire for the correct nodes
//! 6. Per-frame PROCESS fires only for nodes with _process scripts
//! 7. INTERNAL_PHYSICS_PROCESS/INTERNAL_PROCESS fire only for root Window
//! 8. No PHYSICS_PROCESS events (no scripts have _physics_process)
//! 9. Script calls are properly bracketed (call/return pairs)
//! 10. Frame numbers are monotonically ordered
//! 11. Upstream version metadata is present

mod oracle_fixture;

use oracle_fixture::fixtures_dir;
use serde_json::Value;

fn load_upstream_golden() -> Value {
    let path = fixtures_dir()
        .join("golden/traces/test_scripts_upstream.json");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read upstream golden: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse upstream golden: {e}"))
}

fn extract_events(golden: &Value) -> Vec<&Value> {
    golden["event_trace"]
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

// ===========================================================================
// 1. Golden file exists and has correct structure
// ===========================================================================

#[test]
fn upstream_golden_exists_and_has_correct_structure() {
    let golden = load_upstream_golden();

    assert!(golden["event_trace"].is_array(), "event_trace should be an array");
    assert!(golden["frame_count"].is_number(), "frame_count should be a number");
    assert!(golden["scene_file"].is_string(), "scene_file should be a string");
    assert!(golden["tree"].is_object(), "tree should be an object");
    assert!(golden["upstream_version"].is_string(), "upstream_version should be present");

    assert_eq!(golden["frame_count"].as_i64().unwrap(), 10, "should have 10 frames");
}

// ===========================================================================
// 2. Event count and frame distribution
// ===========================================================================

#[test]
fn upstream_golden_event_count_and_distribution() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);

    // Frame 0 has lifecycle + processing; frames 1-9 have only processing
    let frame_0_events: Vec<_> = events.iter().filter(|e| e["frame"] == 0).collect();
    let frame_1_events: Vec<_> = events.iter().filter(|e| e["frame"] == 1).collect();

    assert!(
        frame_0_events.len() > frame_1_events.len(),
        "frame 0 should have more events (lifecycle + processing) than frame 1"
    );

    // All frames 1-9 should have the same count
    for frame in 1..10 {
        let count = events.iter().filter(|e| e["frame"] == frame).count();
        assert_eq!(
            count,
            frame_1_events.len(),
            "frame {frame} should have same event count as frame 1"
        );
    }
}

// ===========================================================================
// 3. ENTER_TREE fires top-down
// ===========================================================================

#[test]
fn upstream_golden_enter_tree_fires_top_down() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
    let enter_tree = events_with_detail(&events_of_type(&events, "notification"), "ENTER_TREE");

    assert!(
        enter_tree.len() >= 3,
        "expected at least 3 ENTER_TREE events"
    );

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

    assert!(scene_idx < mover_idx, "TestScene before Mover");
    assert!(scene_idx < var_idx, "TestScene before VarTest");
}

// ===========================================================================
// 4. READY fires bottom-up
// ===========================================================================

#[test]
fn upstream_golden_ready_fires_bottom_up() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
    let ready = events_with_detail(&events_of_type(&events, "notification"), "READY");

    assert!(ready.len() >= 3, "expected at least 3 READY events");

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

    assert!(mover_idx < scene_idx, "Mover READY before TestScene");
    assert!(var_idx < scene_idx, "VarTest READY before TestScene");
}

// ===========================================================================
// 5. _ready script calls fire for correct nodes
// ===========================================================================

#[test]
fn upstream_golden_ready_script_calls() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
    let ready_calls = events_with_detail(&events_of_type(&events, "script_call"), "_ready");
    let ready_returns = events_with_detail(&events_of_type(&events, "script_return"), "_ready");

    // Only VarTest has _ready in its script
    assert_eq!(ready_calls.len(), 1, "only VarTest should have _ready call");
    assert_eq!(
        ready_calls[0]["node_path"], "/root/TestScene/VarTest",
        "VarTest should have _ready call"
    );

    assert_eq!(ready_returns.len(), 1, "_ready call should be paired");
    assert_eq!(
        ready_returns[0]["node_path"], "/root/TestScene/VarTest",
        "VarTest should have _ready return"
    );
}

// ===========================================================================
// 6. PROCESS fires only for nodes with _process scripts
// ===========================================================================

#[test]
fn upstream_golden_process_only_for_scripted_nodes() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
    let process = events_with_detail(&events_of_type(&events, "notification"), "PROCESS");

    // PROCESS should fire only for Mover and VarTest (both have _process)
    let paths: Vec<&str> = process
        .iter()
        .map(|e| e["node_path"].as_str().unwrap())
        .collect();

    // TestScene has no script, so should NOT appear
    assert!(
        !paths.contains(&"/root/TestScene"),
        "TestScene (no script) should not get PROCESS"
    );
    assert!(
        !paths.contains(&"/root"),
        "root Window should not get user PROCESS"
    );

    // Mover and VarTest should appear (10 frames each)
    let mover_count = paths.iter().filter(|&&p| p == "/root/TestScene/Mover").count();
    let var_count = paths.iter().filter(|&&p| p == "/root/TestScene/VarTest").count();
    assert_eq!(mover_count, 10, "Mover should have PROCESS in all 10 frames");
    assert_eq!(var_count, 10, "VarTest should have PROCESS in all 10 frames");
}

// ===========================================================================
// 7. INTERNAL_PHYSICS_PROCESS / INTERNAL_PROCESS only for root
// ===========================================================================

#[test]
fn upstream_golden_internal_processing_only_for_root() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);

    let int_phys = events_with_detail(
        &events_of_type(&events, "notification"),
        "INTERNAL_PHYSICS_PROCESS",
    );
    let int_proc = events_with_detail(
        &events_of_type(&events, "notification"),
        "INTERNAL_PROCESS",
    );

    // All should be for /root only
    for event in &int_phys {
        assert_eq!(
            event["node_path"], "/root",
            "INTERNAL_PHYSICS_PROCESS should only fire for /root"
        );
    }
    for event in &int_proc {
        assert_eq!(
            event["node_path"], "/root",
            "INTERNAL_PROCESS should only fire for /root"
        );
    }

    assert_eq!(int_phys.len(), 10, "one INTERNAL_PHYSICS_PROCESS per frame");
    assert_eq!(int_proc.len(), 10, "one INTERNAL_PROCESS per frame");
}

// ===========================================================================
// 8. No PHYSICS_PROCESS events (no scripts have _physics_process)
// ===========================================================================

#[test]
fn upstream_golden_no_physics_process() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
    let phys_proc = events_with_detail(
        &events_of_type(&events, "notification"),
        "PHYSICS_PROCESS",
    );

    assert!(
        phys_proc.is_empty(),
        "no PHYSICS_PROCESS should fire (no scripts have _physics_process)"
    );
}

// ===========================================================================
// 9. Script calls are properly paired
// ===========================================================================

#[test]
fn upstream_golden_script_calls_are_paired() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
    let calls = events_of_type(&events, "script_call");
    let returns = events_of_type(&events, "script_return");

    assert_eq!(
        calls.len(),
        returns.len(),
        "script calls and returns should be paired"
    );

    // Each call should have a matching return with same detail and node_path
    for (call, ret) in calls.iter().zip(returns.iter()) {
        assert_eq!(
            call["detail"], ret["detail"],
            "call/return detail mismatch"
        );
        assert_eq!(
            call["node_path"], ret["node_path"],
            "call/return node_path mismatch"
        );
    }
}

// ===========================================================================
// 10. Frame numbers are monotonically ordered
// ===========================================================================

#[test]
fn upstream_golden_frame_numbers_monotonic() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);

    let mut prev_frame = 0i64;
    for event in &events {
        let frame = event["frame"].as_i64().unwrap();
        assert!(
            frame >= prev_frame,
            "frame numbers should be monotonically increasing"
        );
        prev_frame = frame;
    }
}

// ===========================================================================
// 11. Upstream version metadata
// ===========================================================================

#[test]
fn upstream_golden_has_version_metadata() {
    let golden = load_upstream_golden();

    let version = golden["upstream_version"]
        .as_str()
        .expect("upstream_version should be a string");
    assert!(
        !version.is_empty(),
        "upstream_version should not be empty"
    );
    assert!(
        version.contains("4."),
        "upstream_version should reference Godot 4.x: {version}"
    );
}

// ===========================================================================
// 12. All ENTER_TREE events fire before any READY event
// ===========================================================================

#[test]
fn upstream_golden_all_enter_tree_before_ready() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);
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
        "all ENTER_TREE should complete before any READY"
    );
}

// ===========================================================================
// 13. Tree nodes have path fields
// ===========================================================================

#[test]
fn upstream_golden_tree_has_paths() {
    let golden = load_upstream_golden();
    let tree = &golden["tree"];

    assert_eq!(tree["path"], "/root", "root should have path /root");

    let test_scene = &tree["children"][0];
    assert_eq!(test_scene["path"], "/root/TestScene");

    let mover = &test_scene["children"][0];
    assert_eq!(mover["path"], "/root/TestScene/Mover");

    let var_test = &test_scene["children"][1];
    assert_eq!(var_test["path"], "/root/TestScene/VarTest");
}

// ===========================================================================
// 14. Tree nodes have properties and script_vars fields
// ===========================================================================

#[test]
fn upstream_golden_tree_has_properties_and_script_vars() {
    let golden = load_upstream_golden();
    let tree = &golden["tree"];

    // Root and TestScene have empty properties/script_vars
    assert!(tree["properties"].is_object(), "root should have properties object");
    assert!(tree["script_vars"].is_object(), "root should have script_vars object");

    let test_scene = &tree["children"][0];
    assert!(test_scene["properties"].is_object());
    assert!(test_scene["script_vars"].is_object());

    // Mover and VarTest have populated fields
    let mover = &test_scene["children"][0];
    assert!(mover["properties"].is_object(), "Mover should have properties");
    assert!(mover["script_vars"].is_object(), "Mover should have script_vars");

    let var_test = &test_scene["children"][1];
    assert!(var_test["properties"].is_object(), "VarTest should have properties");
    assert!(var_test["script_vars"].is_object(), "VarTest should have script_vars");
}

// ===========================================================================
// 15. Mover script variables after 10 frames
// ===========================================================================

#[test]
fn upstream_golden_mover_script_vars() {
    let golden = load_upstream_golden();
    let mover = &golden["tree"]["children"][0]["children"][0];

    assert_eq!(mover["name"], "Mover");

    let vars = &mover["script_vars"];
    assert_eq!(vars["speed"]["value"], 50.0, "Mover speed should be 50.0");
    assert_eq!(vars["direction"]["value"], 1.0, "Mover direction should be 1.0 (never exceeds 500)");
    assert_eq!(vars["speed"]["type"], "Float");
    assert_eq!(vars["direction"]["type"], "Float");
}

// ===========================================================================
// 16. VarTest script variables after 10 frames
// ===========================================================================

#[test]
fn upstream_golden_vartest_script_vars() {
    let golden = load_upstream_golden();
    let var_test = &golden["tree"]["children"][0]["children"][1];

    assert_eq!(var_test["name"], "VarTest");

    let vars = &var_test["script_vars"];
    assert_eq!(vars["health"]["value"], 100, "health should remain 100");
    assert_eq!(vars["health"]["type"], "Int");
    assert_eq!(vars["name_str"]["value"], "Player");
    assert_eq!(vars["name_str"]["type"], "String");
    assert_eq!(vars["is_alive"]["value"], true, "is_alive should remain true (health > 0)");
    assert_eq!(vars["is_alive"]["type"], "Bool");

    // velocity stays at (0,0) — no velocity changes in script
    let vel = vars["velocity"]["value"].as_array().unwrap();
    assert_eq!(vel[0], 0.0);
    assert_eq!(vel[1], 0.0);
    assert_eq!(vars["velocity"]["type"], "Vector2");
}

// ===========================================================================
// 17. Mover position after 10 frames at delta=1/60
// ===========================================================================

#[test]
fn upstream_golden_mover_position_after_10_frames() {
    let golden = load_upstream_golden();
    let mover = &golden["tree"]["children"][0]["children"][0];

    let pos = &mover["properties"]["position"];
    assert_eq!(pos["type"], "Vector2");

    let coords = pos["value"].as_array().unwrap();
    let x = coords[0].as_f64().unwrap();
    let y = coords[1].as_f64().unwrap();

    // Mover starts at x=100, each frame adds speed*direction*delta = 50*1*(1/60)
    // After 10 frames: 100 + 10 * 50/60 = 100 + 8.333... ≈ 108.333
    let expected_x = 100.0 + 10.0 * (50.0 / 60.0);
    assert!(
        (x - expected_x).abs() < 0.01,
        "Mover x should be ~{expected_x}, got {x}"
    );
    assert_eq!(y, 200.0, "Mover y should remain 200.0");
}

// ===========================================================================
// 18. VarTest position unchanged after 10 frames
// ===========================================================================

#[test]
fn upstream_golden_vartest_position_unchanged() {
    let golden = load_upstream_golden();
    let var_test = &golden["tree"]["children"][0]["children"][1];

    let pos = &var_test["properties"]["position"];
    assert_eq!(pos["type"], "Vector2");

    let coords = pos["value"].as_array().unwrap();
    assert_eq!(coords[0], 300.0, "VarTest x should remain 300.0");
    assert_eq!(coords[1], 200.0, "VarTest y should remain 200.0");
}

// ===========================================================================
// 19. Script paths in tree match scene file
// ===========================================================================

#[test]
fn upstream_golden_script_paths_match_scene() {
    let golden = load_upstream_golden();
    let test_scene = &golden["tree"]["children"][0];
    let mover = &test_scene["children"][0];
    let var_test = &test_scene["children"][1];

    assert_eq!(
        mover["script"].as_str().unwrap(),
        "res://fixtures/scripts/test_movement.gd"
    );
    assert_eq!(
        var_test["script"].as_str().unwrap(),
        "res://fixtures/scripts/test_variables.gd"
    );

    // TestScene has no script
    assert!(test_scene.get("script").is_none() || test_scene["script"].is_null());
}

// ===========================================================================
// 20. Total event count is 88 (16 frame-0 + 8*9 steady-state)
// ===========================================================================

#[test]
fn upstream_golden_total_event_count() {
    let golden = load_upstream_golden();
    let events = extract_events(&golden);

    // Frame 0: 3 ENTER_TREE + 3 READY + 2 script_call/return (_ready) +
    //          2 INTERNAL + 2 PROCESS + 4 script_call/return (_process x2) = 16
    // Frames 1-9: 2 INTERNAL + 2 PROCESS + 4 script_call/return = 8 per frame, 9 frames = 72
    // Total: 16 + 72 = 88
    assert_eq!(events.len(), 88, "total event count should be 88");
}

// ===========================================================================
// 21. Golden differs from mock (proves it's not just a Patina copy)
// ===========================================================================

#[test]
fn upstream_golden_differs_from_mock() {
    let golden = load_upstream_golden();
    let mock_path = fixtures_dir()
        .join("golden/traces/test_scripts_upstream_mock.json");
    let mock_content = std::fs::read_to_string(&mock_path)
        .unwrap_or_else(|e| panic!("failed to read mock: {e}"));
    let mock: Value = serde_json::from_str(&mock_content)
        .unwrap_or_else(|e| panic!("failed to parse mock: {e}"));

    let golden_event_count = golden["event_trace"].as_array().unwrap().len();
    let mock_event_count = mock["event_trace"].as_array().unwrap().len();

    assert_ne!(
        golden_event_count, mock_event_count,
        "upstream golden ({golden_event_count} events) should differ from mock ({mock_event_count} events) — \
         mock fires notifications for all nodes, upstream only for nodes with processing enabled"
    );
}
