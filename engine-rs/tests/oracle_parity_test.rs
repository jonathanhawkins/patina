//! Oracle parity tests — compare Godot oracle output against Patina output.
//!
//! Uses the same JSON format as golden fixtures in `fixtures/golden/scenes/`.
//! The comparison logic can diff two scene trees and report per-node,
//! per-property matches with float tolerance.

use serde_json::Value;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Result of comparing a Godot oracle tree against a Patina tree.
#[derive(Debug)]
struct OracleResult {
    godot_json: Value,
    patina_json: Value,
    comparisons: Vec<ComparisonResult>,
}

/// A single property comparison between Godot and Patina.
#[derive(Debug, Clone, PartialEq)]
struct ComparisonResult {
    node_path: String,
    property: String,
    godot_value: Option<Value>,
    patina_value: Option<Value>,
    matches: bool,
}

/// A flattened node extracted from the tree JSON.
#[derive(Debug, Clone)]
struct FlatNode {
    path: String,
    name: String,
    class: String,
    properties: serde_json::Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Comparison logic
// ---------------------------------------------------------------------------

const DEFAULT_TOLERANCE: f64 = 0.001;

/// Flatten a nested scene tree JSON into a vec of `FlatNode`.
fn flatten_tree(tree: &Value) -> Vec<FlatNode> {
    let mut result = Vec::new();
    if let Some(nodes) = tree.get("nodes").and_then(|n| n.as_array()) {
        for node in nodes {
            flatten_node(node, &mut result);
        }
    } else if tree.get("name").is_some() {
        // Single root node
        flatten_node(tree, &mut result);
    }
    result
}

fn flatten_node(node: &Value, out: &mut Vec<FlatNode>) {
    let path = node
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = node
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let class = node
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let properties = node
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    out.push(FlatNode {
        path,
        name,
        class,
        properties,
    });

    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            flatten_node(child, out);
        }
    }
}

/// Count total nodes in a tree.
fn count_nodes(tree: &Value) -> usize {
    flatten_tree(tree).len()
}

/// Compare two JSON values with float tolerance.
fn values_match(a: &Value, b: &Value, tolerance: f64) -> bool {
    match (a, b) {
        (Value::Number(an), Value::Number(bn)) => {
            let af = an.as_f64().unwrap_or(0.0);
            let bf = bn.as_f64().unwrap_or(0.0);
            (af - bf).abs() <= tolerance
        }
        (Value::Array(aa), Value::Array(ba)) => {
            aa.len() == ba.len()
                && aa
                    .iter()
                    .zip(ba.iter())
                    .all(|(x, y)| values_match(x, y, tolerance))
        }
        (Value::Object(ao), Value::Object(bo)) => {
            // If both have a "value" key, compare just the values (typed property format)
            if let (Some(av), Some(bv)) = (ao.get("value"), bo.get("value")) {
                values_match(av, bv, tolerance)
            } else {
                ao.len() == bo.len()
                    && ao
                        .iter()
                        .all(|(k, v)| bo.get(k).map_or(false, |bv| values_match(v, bv, tolerance)))
            }
        }
        _ => a == b,
    }
}

/// Compare two scene trees, returning per-node, per-property comparison results.
fn compare_trees(godot: &Value, patina: &Value) -> Vec<ComparisonResult> {
    let g_nodes = flatten_tree(godot);
    let p_nodes = flatten_tree(patina);
    let mut results = Vec::new();

    // Build lookup by path
    let g_map: std::collections::HashMap<&str, &FlatNode> =
        g_nodes.iter().map(|n| (n.path.as_str(), n)).collect();
    let p_map: std::collections::HashMap<&str, &FlatNode> =
        p_nodes.iter().map(|n| (n.path.as_str(), n)).collect();

    // Check all godot nodes
    for g_node in &g_nodes {
        if let Some(p_node) = p_map.get(g_node.path.as_str()) {
            // Compare class
            results.push(ComparisonResult {
                node_path: g_node.path.clone(),
                property: "_class".to_string(),
                godot_value: Some(Value::String(g_node.class.clone())),
                patina_value: Some(Value::String(p_node.class.clone())),
                matches: g_node.class == p_node.class,
            });

            // Compare all properties from both sides
            let mut all_keys: Vec<&String> = g_node.properties.keys().collect();
            for k in p_node.properties.keys() {
                if !g_node.properties.contains_key(k) {
                    all_keys.push(k);
                }
            }
            all_keys.sort();
            all_keys.dedup();

            for key in all_keys {
                let g_val = g_node.properties.get(key);
                let p_val = p_node.properties.get(key);
                let matches = match (g_val, p_val) {
                    (Some(gv), Some(pv)) => values_match(gv, pv, DEFAULT_TOLERANCE),
                    (None, None) => true,
                    _ => false,
                };
                results.push(ComparisonResult {
                    node_path: g_node.path.clone(),
                    property: key.clone(),
                    godot_value: g_val.cloned(),
                    patina_value: p_val.cloned(),
                    matches,
                });
            }
        } else {
            // Node missing in patina
            results.push(ComparisonResult {
                node_path: g_node.path.clone(),
                property: "_exists".to_string(),
                godot_value: Some(Value::Bool(true)),
                patina_value: Some(Value::Bool(false)),
                matches: false,
            });
        }
    }

    // Check patina-only nodes (extra nodes not in godot)
    for p_node in &p_nodes {
        if !g_map.contains_key(p_node.path.as_str()) {
            results.push(ComparisonResult {
                node_path: p_node.path.clone(),
                property: "_exists".to_string(),
                godot_value: Some(Value::Bool(false)),
                patina_value: Some(Value::Bool(true)),
                matches: false,
            });
        }
    }

    results
}

/// Run a full oracle comparison and return an `OracleResult`.
fn run_oracle_comparison(godot_json: Value, patina_json: Value) -> OracleResult {
    let comparisons = compare_trees(&godot_json, &patina_json);
    OracleResult {
        godot_json,
        patina_json,
        comparisons,
    }
}

// ---------------------------------------------------------------------------
// Fixture data (hardcoded Godot oracle output)
// ---------------------------------------------------------------------------

fn fixture_simple_scene() -> Value {
    serde_json::json!({
        "nodes": [{
            "name": "Root",
            "class": "Node",
            "path": "/root/Root",
            "children": [{
                "name": "Player",
                "class": "Node2D",
                "path": "/root/Root/Player",
                "children": [],
                "properties": {
                    "position": { "type": "Vector2", "value": [100.0, 200.0] },
                    "speed": { "type": "Int", "value": 350 }
                }
            }],
            "properties": {}
        }]
    })
}

fn fixture_platformer_scene() -> Value {
    serde_json::json!({
        "nodes": [{
            "name": "World",
            "class": "Node",
            "path": "/root/World",
            "children": [
                {
                    "name": "Player",
                    "class": "Node2D",
                    "path": "/root/World/Player",
                    "children": [],
                    "properties": {
                        "position": { "type": "Vector2", "value": [100.0, 300.0] },
                        "speed": { "type": "Int", "value": 250 }
                    }
                },
                {
                    "name": "Platform1",
                    "class": "Node2D",
                    "path": "/root/World/Platform1",
                    "children": [],
                    "properties": {
                        "position": { "type": "Vector2", "value": [0.0, 500.0] }
                    }
                },
                {
                    "name": "Camera",
                    "class": "Camera2D",
                    "path": "/root/World/Camera",
                    "children": [],
                    "properties": {}
                }
            ],
            "properties": {}
        }]
    })
}

// ===========================================================================
// Tests
// ===========================================================================

// --- flatten_tree tests ---

#[test]
fn flatten_tree_single_root() {
    let tree = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node", "path": "/root/Root",
            "children": [], "properties": {}
        }]
    });
    let flat = flatten_tree(&tree);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "Root");
    assert_eq!(flat[0].class, "Node");
}

#[test]
fn flatten_tree_nested_children() {
    let tree = fixture_simple_scene();
    let flat = flatten_tree(&tree);
    assert_eq!(flat.len(), 2);
    assert_eq!(flat[0].path, "/root/Root");
    assert_eq!(flat[1].path, "/root/Root/Player");
}

#[test]
fn flatten_tree_preserves_properties() {
    let tree = fixture_simple_scene();
    let flat = flatten_tree(&tree);
    let player = &flat[1];
    assert!(player.properties.contains_key("position"));
    assert!(player.properties.contains_key("speed"));
}

#[test]
fn count_nodes_empty_tree() {
    let tree = serde_json::json!({ "nodes": [] });
    assert_eq!(count_nodes(&tree), 0);
}

#[test]
fn count_nodes_with_hierarchy() {
    let tree = fixture_platformer_scene();
    // World + Player + Platform1 + Camera = 4
    assert_eq!(count_nodes(&tree), 4);
}

// --- values_match tests ---

#[test]
fn values_match_exact_integers() {
    let a = serde_json::json!(42);
    let b = serde_json::json!(42);
    assert!(values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_different_integers() {
    let a = serde_json::json!(42);
    let b = serde_json::json!(43);
    assert!(!values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_floats_within_tolerance() {
    let a = serde_json::json!(100.0);
    let b = serde_json::json!(100.0005);
    assert!(values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_floats_outside_tolerance() {
    let a = serde_json::json!(100.0);
    let b = serde_json::json!(100.5);
    assert!(!values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_arrays_with_tolerance() {
    let a = serde_json::json!([100.0, 200.0]);
    let b = serde_json::json!([100.0001, 199.9999]);
    assert!(values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_arrays_different_length() {
    let a = serde_json::json!([100.0, 200.0]);
    let b = serde_json::json!([100.0]);
    assert!(!values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_strings() {
    let a = serde_json::json!("hello");
    let b = serde_json::json!("hello");
    assert!(values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_strings_different() {
    let a = serde_json::json!("hello");
    let b = serde_json::json!("world");
    assert!(!values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_typed_property_format() {
    // Godot oracle format: { "type": "Vector2", "value": [100.0, 200.0] }
    let a = serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]});
    let b = serde_json::json!({"type": "Vector2", "value": [100.0001, 199.9999]});
    assert!(values_match(&a, &b, DEFAULT_TOLERANCE));
}

#[test]
fn values_match_booleans() {
    assert!(values_match(
        &serde_json::json!(true),
        &serde_json::json!(true),
        DEFAULT_TOLERANCE
    ));
    assert!(!values_match(
        &serde_json::json!(true),
        &serde_json::json!(false),
        DEFAULT_TOLERANCE
    ));
}

#[test]
fn values_match_null() {
    assert!(values_match(&Value::Null, &Value::Null, DEFAULT_TOLERANCE));
    assert!(!values_match(
        &Value::Null,
        &serde_json::json!(0),
        DEFAULT_TOLERANCE
    ));
}

// --- compare_trees tests ---

#[test]
fn compare_identical_trees() {
    let tree = fixture_simple_scene();
    let results = compare_trees(&tree, &tree);
    assert!(
        results.iter().all(|r| r.matches),
        "all comparisons should match for identical trees: {:?}",
        results.iter().filter(|r| !r.matches).collect::<Vec<_>>()
    );
}

#[test]
fn compare_trees_mismatching_property() {
    let godot = fixture_simple_scene();
    let mut patina = fixture_simple_scene();
    // Change Player speed from 350 to 400
    patina["nodes"][0]["children"][0]["properties"]["speed"]["value"] = serde_json::json!(400);

    let results = compare_trees(&godot, &patina);
    let speed_result = results
        .iter()
        .find(|r| r.property == "speed")
        .expect("should have speed comparison");
    assert!(!speed_result.matches);
    assert_eq!(
        speed_result.godot_value,
        Some(serde_json::json!({"type": "Int", "value": 350}))
    );
}

#[test]
fn compare_trees_missing_node_in_patina() {
    let godot = fixture_platformer_scene();
    // Patina is missing Platform1 and Camera
    let patina = serde_json::json!({
        "nodes": [{
            "name": "World",
            "class": "Node",
            "path": "/root/World",
            "children": [{
                "name": "Player",
                "class": "Node2D",
                "path": "/root/World/Player",
                "children": [],
                "properties": {
                    "position": { "type": "Vector2", "value": [100.0, 300.0] }
                }
            }],
            "properties": {}
        }]
    });

    let results = compare_trees(&godot, &patina);
    let missing = results
        .iter()
        .filter(|r| r.property == "_exists" && !r.matches)
        .collect::<Vec<_>>();
    assert_eq!(missing.len(), 2, "Platform1 and Camera should be missing");
}

#[test]
fn compare_trees_extra_node_in_patina() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node", "path": "/root/Root",
            "children": [], "properties": {}
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node", "path": "/root/Root",
            "children": [{
                "name": "Extra", "class": "Node2D", "path": "/root/Root/Extra",
                "children": [], "properties": {}
            }],
            "properties": {}
        }]
    });

    let results = compare_trees(&godot, &patina);
    let extra = results
        .iter()
        .find(|r| r.node_path == "/root/Root/Extra" && r.property == "_exists")
        .expect("should detect extra node");
    assert!(!extra.matches);
}

#[test]
fn compare_trees_class_mismatch() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node", "path": "/root/Root",
            "children": [], "properties": {}
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [], "properties": {}
        }]
    });

    let results = compare_trees(&godot, &patina);
    let class_cmp = results
        .iter()
        .find(|r| r.property == "_class")
        .expect("should have class comparison");
    assert!(!class_cmp.matches);
}

#[test]
fn compare_trees_position_within_tolerance() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "position": { "type": "Vector2", "value": [100.0, 200.0] }
            }
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "position": { "type": "Vector2", "value": [100.0005, 199.9998] }
            }
        }]
    });

    let results = compare_trees(&godot, &patina);
    let pos = results
        .iter()
        .find(|r| r.property == "position")
        .expect("should have position comparison");
    assert!(pos.matches, "positions within tolerance should match");
}

#[test]
fn compare_trees_position_outside_tolerance() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "position": { "type": "Vector2", "value": [100.0, 200.0] }
            }
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "position": { "type": "Vector2", "value": [105.0, 200.0] }
            }
        }]
    });

    let results = compare_trees(&godot, &patina);
    let pos = results
        .iter()
        .find(|r| r.property == "position")
        .expect("should have position comparison");
    assert!(!pos.matches, "positions outside tolerance should not match");
}

#[test]
fn compare_trees_missing_property_in_patina() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "speed": { "type": "Int", "value": 350 },
                "label": { "type": "String", "value": "Hero" }
            }
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "speed": { "type": "Int", "value": 350 }
            }
        }]
    });

    let results = compare_trees(&godot, &patina);
    let label = results
        .iter()
        .find(|r| r.property == "label")
        .expect("should detect missing label");
    assert!(!label.matches);
    assert!(label.patina_value.is_none());
}

#[test]
fn compare_trees_extra_property_in_patina() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {}
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node2D", "path": "/root/Root",
            "children": [],
            "properties": {
                "extra_prop": { "type": "Bool", "value": true }
            }
        }]
    });

    let results = compare_trees(&godot, &patina);
    let extra = results
        .iter()
        .find(|r| r.property == "extra_prop")
        .expect("should detect extra property");
    assert!(!extra.matches);
    assert!(extra.godot_value.is_none());
}

// --- OracleResult integration tests ---

#[test]
fn oracle_result_matching_trees() {
    let tree = fixture_simple_scene();
    let result = run_oracle_comparison(tree.clone(), tree);
    assert!(result.comparisons.iter().all(|c| c.matches));
}

#[test]
fn oracle_result_counts_failures() {
    let godot = fixture_simple_scene();
    let mut patina = fixture_simple_scene();
    patina["nodes"][0]["children"][0]["properties"]["speed"]["value"] = serde_json::json!(999);

    let result = run_oracle_comparison(godot, patina);
    let failures: Vec<_> = result.comparisons.iter().filter(|c| !c.matches).collect();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].property, "speed");
}

#[test]
fn compare_empty_trees() {
    let empty = serde_json::json!({ "nodes": [] });
    let results = compare_trees(&empty, &empty);
    assert!(results.is_empty());
}

#[test]
fn compare_single_root_node_no_properties() {
    let tree = serde_json::json!({
        "nodes": [{
            "name": "Root", "class": "Node", "path": "/root/Root",
            "children": [], "properties": {}
        }]
    });
    let results = compare_trees(&tree, &tree);
    // Should have exactly 1 comparison: the _class check
    assert_eq!(results.len(), 1);
    assert!(results[0].matches);
    assert_eq!(results[0].property, "_class");
}

// --- Edge case: single root node format (no "nodes" wrapper) ---

#[test]
fn flatten_tree_single_node_format() {
    let tree = serde_json::json!({
        "name": "Root", "class": "Node", "path": "/root/Root",
        "children": [], "properties": {}
    });
    let flat = flatten_tree(&tree);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "Root");
}

#[test]
fn compare_trees_color_property() {
    let godot = serde_json::json!({
        "nodes": [{
            "name": "BG", "class": "Node2D", "path": "/root/BG",
            "children": [],
            "properties": {
                "modulate": { "type": "Color", "value": [0.2, 0.4, 0.6, 1.0] }
            }
        }]
    });
    let patina = serde_json::json!({
        "nodes": [{
            "name": "BG", "class": "Node2D", "path": "/root/BG",
            "children": [],
            "properties": {
                "modulate": { "type": "Color", "value": [0.2001, 0.3999, 0.6001, 1.0] }
            }
        }]
    });

    let results = compare_trees(&godot, &patina);
    let color = results
        .iter()
        .find(|r| r.property == "modulate")
        .expect("should have modulate comparison");
    assert!(color.matches, "color values within tolerance should match");
}
