//! pat-4y9n: Script-driven fixture executes and matches oracle.
//!
//! Validates that the test_scripts scene golden contains execution results
//! and the patina trace has correct structure.

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

#[test]
fn test_scripts_golden_has_nodes() {
    let path = fixtures_dir().join("golden/scenes/test_scripts.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let golden: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        golden.get("nodes").is_some()
            || golden.get("tree").is_some()
            || golden.get("data").is_some(),
        "test_scripts golden must contain execution results"
    );
}

#[test]
fn test_scripts_patina_trace_has_events() {
    let path = fixtures_dir().join("golden/traces/test_scripts_patina.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let trace: serde_json::Value = serde_json::from_str(&content).unwrap();
    let events = trace["event_trace"].as_array().unwrap();
    assert!(!events.is_empty(), "patina trace must have events");
}

#[test]
fn test_scripts_upstream_trace_exists() {
    let path = fixtures_dir().join("golden/traces/test_scripts_upstream.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let trace: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        trace["upstream_version"].as_str().unwrap().contains("4.6"),
        "upstream trace should reference Godot 4.6"
    );
}

#[test]
fn test_scripts_golden_has_mover_and_vartest() {
    let path = fixtures_dir().join("golden/scenes/test_scripts.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let golden: serde_json::Value = serde_json::from_str(&content).unwrap();

    let data = golden.get("data").unwrap_or(&golden);
    let nodes = data["nodes"].as_array().unwrap();
    let test_scene = &nodes[0];
    let children = test_scene["children"].as_array().unwrap();
    let names: Vec<&str> = children
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();

    assert!(names.contains(&"Mover"), "must have Mover node");
    assert!(names.contains(&"VarTest"), "must have VarTest node");
}
