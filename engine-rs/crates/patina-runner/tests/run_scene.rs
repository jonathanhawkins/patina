//! Integration test: runs the patina-runner binary on a fixture `.tscn` file
//! and verifies the output is valid JSON with expected structure.

use std::process::Command;

/// Returns the path to the fixture scene file, relative to the workspace root.
fn fixture_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/../../fixtures/scenes/hierarchy.tscn", manifest_dir)
}

#[test]
fn runner_outputs_valid_json() {
    let bin = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(bin)
        .arg(fixture_path())
        .arg("--frames")
        .arg("5")
        .output()
        .expect("failed to execute patina-runner");

    assert!(
        output.status.success(),
        "patina-runner exited with error:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("non-UTF-8 stdout");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is not valid JSON");

    // Verify top-level keys.
    assert!(parsed.get("scene_file").is_some(), "missing scene_file");
    assert!(parsed.get("frame_count").is_some(), "missing frame_count");
    assert!(parsed.get("physics_time").is_some(), "missing physics_time");
    assert!(parsed.get("process_time").is_some(), "missing process_time");
    assert!(parsed.get("tree").is_some(), "missing tree");

    // Verify frame count matches what we requested.
    assert_eq!(parsed["frame_count"].as_u64(), Some(5));

    // Verify tree root structure.
    let tree = &parsed["tree"];
    assert_eq!(tree["name"].as_str(), Some("root"));
    assert_eq!(tree["class"].as_str(), Some("Node"));
    assert_eq!(tree["path"].as_str(), Some("/root"));

    // The instanced scene root should be a child of the tree root.
    let children = tree["children"]
        .as_array()
        .expect("root should have children");
    assert!(!children.is_empty(), "root should have at least one child");

    let scene_root = &children[0];
    assert_eq!(scene_root["name"].as_str(), Some("Root"));
    assert_eq!(scene_root["class"].as_str(), Some("Node"));
}

#[test]
fn runner_default_frames() {
    let bin = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(bin)
        .arg(fixture_path())
        .output()
        .expect("failed to execute patina-runner");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("non-UTF-8 stdout");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is not valid JSON");

    // Default is 10 frames.
    assert_eq!(parsed["frame_count"].as_u64(), Some(10));
}

#[test]
fn runner_fails_on_missing_file() {
    let bin = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(bin)
        .arg("nonexistent.tscn")
        .output()
        .expect("failed to execute patina-runner");

    assert!(
        !output.status.success(),
        "expected failure for missing file"
    );
}
