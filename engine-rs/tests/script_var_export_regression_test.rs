//! pat-ztah: Regression tests for script-variable property export.
//!
//! These tests verify that GDScript-exported variables appear in the Patina
//! runner's JSON output within the `properties` object (matching Godot 4.6.1+
//! oracle format). If someone removes the script-var merge from
//! `dump_node_json()`, these tests will fail.

mod oracle_fixture;

use oracle_fixture::{fixtures_dir, load_json_fixture};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
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

fn run_patina_on_scene(scene_name: &str) -> Value {
    let scene_path = fixtures_dir().join("scenes").join(format!("{scene_name}.tscn"));
    let binary = runner_binary();
    let output = Command::new(&binary)
        .arg(scene_path.to_str().expect("valid UTF-8"))
        .arg("--frames")
        .arg("0")
        .output()
        .unwrap_or_else(|e| panic!("failed to execute patina-runner: {e}"));
    assert!(
        output.status.success(),
        "patina-runner failed on {scene_name}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("invalid JSON from patina-runner for {scene_name}:\n{e}")
    })
}

/// Collect all (node_path, property_name, value) triples from a Patina output tree,
/// filtering out internal properties (_script_path, script) and only keeping
/// user-visible properties.
fn collect_patina_properties(root: &Value) -> HashMap<(String, String), Value> {
    let tree = root.get("tree").unwrap_or(root);
    let mut result = HashMap::new();
    collect_node_properties(tree, &mut result);
    result
}

fn collect_node_properties(node: &Value, out: &mut HashMap<(String, String), Value>) {
    let path = node
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(props) = node.get("properties").and_then(|v| v.as_object()) {
        for (k, v) in props {
            if !k.starts_with('_') && k != "script" {
                out.insert((path.clone(), k.clone()), v.clone());
            }
        }
    }
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            collect_node_properties(child, out);
        }
    }
}

/// Extract the scalar value from a typed property like {"type":"Int","value":100}.
fn extract_value(v: &Value) -> &Value {
    v.get("value").unwrap_or(v)
}

// ===========================================================================
// 1. test_scripts scene: script vars must appear in properties
// ===========================================================================

#[test]
fn test_scripts_mover_has_speed_in_properties() {
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/TestScene/Mover".to_string(),
        "speed".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "Mover node must export 'speed' script variable in properties. \
         Found properties: {:?}",
        props.keys().filter(|(p, _)| p.contains("Mover")).collect::<Vec<_>>()
    );
    let val = extract_value(&props[&key]);
    assert_eq!(val, &serde_json::json!(50.0), "Mover speed should be 50.0");
}

#[test]
fn test_scripts_mover_has_direction_in_properties() {
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/TestScene/Mover".to_string(),
        "direction".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "Mover node must export 'direction' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(val, &serde_json::json!(1.0), "Mover direction should be 1.0");
}

#[test]
fn test_scripts_vartest_has_health_in_properties() {
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/TestScene/VarTest".to_string(),
        "health".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "VarTest node must export 'health' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(val, &serde_json::json!(100), "VarTest health should be 100");
}

#[test]
fn test_scripts_vartest_has_is_alive_in_properties() {
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/TestScene/VarTest".to_string(),
        "is_alive".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "VarTest node must export 'is_alive' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(val, &serde_json::json!(true), "VarTest is_alive should be true");
}

#[test]
fn test_scripts_vartest_has_name_str_in_properties() {
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/TestScene/VarTest".to_string(),
        "name_str".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "VarTest node must export 'name_str' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(
        val,
        &serde_json::json!("Player"),
        "VarTest name_str should be 'Player'"
    );
}

#[test]
fn test_scripts_vartest_has_velocity_in_properties() {
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/TestScene/VarTest".to_string(),
        "velocity".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "VarTest node must export 'velocity' script variable in properties"
    );
}

// ===========================================================================
// 2. space_shooter scene: script vars must appear in properties
// ===========================================================================

#[test]
fn space_shooter_player_has_speed_in_properties() {
    let output = run_patina_on_scene("space_shooter");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/SpaceShooter/Player".to_string(),
        "speed".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "Player node must export 'speed' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(val, &serde_json::json!(200.0), "Player speed should be 200");
}

#[test]
fn space_shooter_player_has_can_shoot_in_properties() {
    let output = run_patina_on_scene("space_shooter");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/SpaceShooter/Player".to_string(),
        "can_shoot".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "Player node must export 'can_shoot' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(val, &serde_json::json!(true), "Player can_shoot should be true");
}

#[test]
fn space_shooter_player_has_shoot_cooldown_in_properties() {
    let output = run_patina_on_scene("space_shooter");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/SpaceShooter/Player".to_string(),
        "shoot_cooldown".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "Player node must export 'shoot_cooldown' script variable in properties"
    );
}

#[test]
fn space_shooter_spawner_has_spawn_interval_in_properties() {
    let output = run_patina_on_scene("space_shooter");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/SpaceShooter/EnemySpawner".to_string(),
        "spawn_interval".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "EnemySpawner node must export 'spawn_interval' script variable in properties"
    );
    let val = extract_value(&props[&key]);
    assert_eq!(
        val,
        &serde_json::json!(2.0),
        "EnemySpawner spawn_interval should be 2.0"
    );
}

#[test]
fn space_shooter_spawner_has_spawn_timer_in_properties() {
    let output = run_patina_on_scene("space_shooter");
    let props = collect_patina_properties(&output);
    let key = (
        "/root/SpaceShooter/EnemySpawner".to_string(),
        "spawn_timer".to_string(),
    );
    assert!(
        props.contains_key(&key),
        "EnemySpawner node must export 'spawn_timer' script variable in properties"
    );
}

// ===========================================================================
// 3. Cross-scene: all script-bearing nodes must have non-empty properties
// ===========================================================================

#[test]
fn all_scripted_nodes_export_at_least_one_script_variable() {
    // Every node that has a _script_path should also have at least one
    // non-internal property from the script in the properties output.
    for scene_name in &["test_scripts", "space_shooter"] {
        let output = run_patina_on_scene(scene_name);
        let tree = output.get("tree").unwrap_or(&output);
        let mut scripted_nodes: Vec<String> = Vec::new();
        collect_scripted_nodes(tree, &mut scripted_nodes);
        assert!(
            !scripted_nodes.is_empty(),
            "{scene_name}: should have at least one scripted node"
        );

        let props = collect_patina_properties(&output);
        for node_path in &scripted_nodes {
            let node_props: Vec<_> = props
                .keys()
                .filter(|(p, _)| p == node_path)
                .collect();
            // Each scripted node should have at least one exported script var
            // (position doesn't count as it's a class property, not a script var)
            let script_var_count = node_props
                .iter()
                .filter(|(_, k)| k != "position")
                .count();
            assert!(
                script_var_count > 0,
                "{scene_name}: scripted node {node_path} has no script variables \
                 in properties output. This likely means the script-var merge in \
                 dump_node_json() is broken. Found props: {node_props:?}"
            );
        }
    }
}

fn collect_scripted_nodes(node: &Value, out: &mut Vec<String>) {
    // A node is "scripted" if its script_vars object is non-empty.
    // The _script_path is filtered from the properties output by the runner,
    // so we detect script presence via the script_vars field.
    if let Some(sv) = node.get("script_vars").and_then(|v| v.as_object()) {
        if !sv.is_empty() {
            if let Some(path) = node.get("path").and_then(|v| v.as_str()) {
                out.push(path.to_string());
            }
        }
    }
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            collect_scripted_nodes(child, out);
        }
    }
}

// ===========================================================================
// 4. Properties output must include script vars alongside class properties
// ===========================================================================

#[test]
fn test_scripts_mover_position_coexists_with_script_vars() {
    // Verify that both the class property (position) and script variables
    // (speed, direction) appear together in the same properties object.
    let output = run_patina_on_scene("test_scripts");
    let props = collect_patina_properties(&output);
    let mover_path = "/root/TestScene/Mover".to_string();
    let mover_props: Vec<String> = props
        .keys()
        .filter(|(p, _)| p == &mover_path)
        .map(|(_, k)| k.clone())
        .collect();

    assert!(
        mover_props.contains(&"position".to_string()),
        "Mover should have position (class property)"
    );
    assert!(
        mover_props.contains(&"speed".to_string()),
        "Mover should have speed (script variable)"
    );
    assert!(
        mover_props.contains(&"direction".to_string()),
        "Mover should have direction (script variable)"
    );
}
