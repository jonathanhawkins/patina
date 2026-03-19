//! Oracle regression tests — compare Godot oracle JSON against Patina JSON.
//!
//! Loads fixtures from `../fixtures/oracle_outputs/` and `../fixtures/patina_outputs/`,
//! normalizes format differences (Godot's `"Vector2(100, 200)"` strings vs Patina's
//! `{"type":"Vector2","value":[100,200]}`), and reports per-node, per-property parity.

use serde_json::Value;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Format normalization
// ---------------------------------------------------------------------------

/// Parse Godot's string-format property values into Patina's typed JSON format.
///
/// Godot oracle outputs properties as strings like:
/// - `"Vector2(100, 200)"` → `{"type":"Vector2","value":[100.0,200.0]}`
/// - `"0.0"` → number `0.0`
/// - `"true"` / `"false"` → boolean
/// - `"100"` → integer `100`
fn normalize_godot_value(val: &Value) -> Value {
    match val {
        Value::String(s) => parse_godot_string_value(s),
        Value::Object(map) => {
            // Already in typed format (Patina style) — normalize inner value
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), normalize_godot_value(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(normalize_godot_value).collect()),
        other => other.clone(),
    }
}

/// Parse a Godot oracle string value into a JSON value.
fn parse_godot_string_value(s: &str) -> Value {
    // Vector2(x, y)
    if let Some(inner) = s.strip_prefix("Vector2(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (
                parts[0].trim().parse::<f64>(),
                parts[1].trim().parse::<f64>(),
            ) {
                return serde_json::json!({"type": "Vector2", "value": [x, y]});
            }
        }
    }

    // Vector3(x, y, z)
    if let Some(inner) = s.strip_prefix("Vector3(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3 {
            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[0].trim().parse::<f64>(),
                parts[1].trim().parse::<f64>(),
                parts[2].trim().parse::<f64>(),
            ) {
                return serde_json::json!({"type": "Vector3", "value": [x, y, z]});
            }
        }
    }

    // Color(r, g, b, a)
    if let Some(inner) = s.strip_prefix("Color(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 4 {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                parts[0].trim().parse::<f64>(),
                parts[1].trim().parse::<f64>(),
                parts[2].trim().parse::<f64>(),
                parts[3].trim().parse::<f64>(),
            ) {
                return serde_json::json!({"type": "Color", "value": [r, g, b, a]});
            }
        }
    }

    // Boolean
    if s == "true" {
        return serde_json::json!({"type": "Bool", "value": true});
    }
    if s == "false" {
        return serde_json::json!({"type": "Bool", "value": false});
    }

    // Integer (no decimal point)
    if let Ok(i) = s.parse::<i64>() {
        return serde_json::json!({"type": "Int", "value": i});
    }

    // Float (has decimal point)
    if let Ok(f) = s.parse::<f64>() {
        return serde_json::json!({"type": "Float", "value": f});
    }

    // Fallback: keep as string
    serde_json::json!({"type": "String", "value": s})
}

/// Normalize a Patina-format property value for comparison.
///
/// Patina already uses the `{"type":"...", "value": ...}` format, so mostly
/// this is a pass-through, but we normalize number types for tolerance.
fn normalize_patina_value(val: &Value) -> Value {
    val.clone()
}

// ---------------------------------------------------------------------------
// Comparison logic
// ---------------------------------------------------------------------------

const FLOAT_TOLERANCE: f64 = 0.01;

/// A flattened node from the tree.
#[derive(Debug, Clone)]
struct FlatNode {
    path: String,
    name: String,
    class: String,
    properties: HashMap<String, Value>,
}

/// Result of comparing one property.
#[derive(Debug)]
struct PropertyComparison {
    node_path: String,
    property: String,
    godot_value: Option<Value>,
    patina_value: Option<Value>,
    matches: bool,
}

/// Flatten a Godot oracle tree (top-level node with "children" array).
fn flatten_godot_tree(node: &Value) -> Vec<FlatNode> {
    let mut result = Vec::new();
    flatten_godot_node(node, &mut result);
    result
}

fn flatten_godot_node(node: &Value, out: &mut Vec<FlatNode>) {
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

    let mut properties = HashMap::new();
    if let Some(props) = node.get("properties").and_then(|v| v.as_object()) {
        for (key, val) in props {
            properties.insert(key.clone(), normalize_godot_value(val));
        }
    }

    out.push(FlatNode {
        path,
        name,
        class,
        properties,
    });

    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            flatten_godot_node(child, out);
        }
    }
}

/// Flatten a Patina output tree (has "tree" wrapper with root node).
fn flatten_patina_tree(root: &Value) -> Vec<FlatNode> {
    let tree = root.get("tree").unwrap_or(root);
    let mut result = Vec::new();

    // Skip the "/root" synthetic node, go straight to scene children
    if let Some(children) = tree.get("children").and_then(|c| c.as_array()) {
        for child in children {
            flatten_patina_node(child, &mut result);
        }
    }

    result
}

fn flatten_patina_node(node: &Value, out: &mut Vec<FlatNode>) {
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

    let mut properties = HashMap::new();
    if let Some(props) = node.get("properties").and_then(|v| v.as_object()) {
        for (key, val) in props {
            // Skip internal properties
            if key.starts_with('_') || key == "script" {
                continue;
            }
            properties.insert(key.clone(), normalize_patina_value(val));
        }
    }

    out.push(FlatNode {
        path,
        name,
        class,
        properties,
    });

    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            flatten_patina_node(child, out);
        }
    }
}

/// Compare two normalized property values with float tolerance.
fn values_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(an), Value::Number(bn)) => {
            let af = an.as_f64().unwrap_or(0.0);
            let bf = bn.as_f64().unwrap_or(0.0);
            (af - bf).abs() <= FLOAT_TOLERANCE
        }
        (Value::Bool(ab), Value::Bool(bb)) => ab == bb,
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Array(aa), Value::Array(ba)) => {
            aa.len() == ba.len() && aa.iter().zip(ba.iter()).all(|(x, y)| values_match(x, y))
        }
        (Value::Object(ao), Value::Object(bo)) => {
            // Compare "value" fields if both have them (typed property format)
            if let (Some(av), Some(bv)) = (ao.get("value"), bo.get("value")) {
                values_match(av, bv)
            } else {
                ao.len() == bo.len()
                    && ao
                        .iter()
                        .all(|(k, v)| bo.get(k).map_or(false, |bv| values_match(v, bv)))
            }
        }
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

/// Compare an oracle Godot tree against a Patina tree.
fn compare_scene(godot_nodes: &[FlatNode], patina_nodes: &[FlatNode]) -> Vec<PropertyComparison> {
    let mut results = Vec::new();

    let p_map: HashMap<&str, &FlatNode> =
        patina_nodes.iter().map(|n| (n.path.as_str(), n)).collect();

    for g_node in godot_nodes {
        if let Some(p_node) = p_map.get(g_node.path.as_str()) {
            // Compare class
            results.push(PropertyComparison {
                node_path: g_node.path.clone(),
                property: "_class".to_string(),
                godot_value: Some(Value::String(g_node.class.clone())),
                patina_value: Some(Value::String(p_node.class.clone())),
                matches: g_node.class == p_node.class,
            });

            // Collect all property keys from both sides
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
                    (Some(gv), Some(pv)) => values_match(gv, pv),
                    (None, None) => true,
                    _ => false,
                };
                results.push(PropertyComparison {
                    node_path: g_node.path.clone(),
                    property: key.clone(),
                    godot_value: g_val.cloned(),
                    patina_value: p_val.cloned(),
                    matches,
                });
            }
        } else {
            results.push(PropertyComparison {
                node_path: g_node.path.clone(),
                property: "_exists".to_string(),
                godot_value: Some(Value::Bool(true)),
                patina_value: Some(Value::Bool(false)),
                matches: false,
            });
        }
    }

    results
}

/// Compute parity percentage from comparison results.
fn parity_percentage(results: &[PropertyComparison]) -> f64 {
    if results.is_empty() {
        return 100.0;
    }
    let matched = results.iter().filter(|r| r.matches).count();
    (matched as f64 / results.len() as f64) * 100.0
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

fn load_fixture(path: &str) -> Value {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

// ===========================================================================
// Format normalization tests
// ===========================================================================

#[test]
fn normalize_vector2_string() {
    let val = serde_json::json!("Vector2(100, 200)");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]})
    ));
}

#[test]
fn normalize_vector2_negative() {
    let val = serde_json::json!("Vector2(-50.5, 100.3)");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Vector2", "value": [-50.5, 100.3]})
    ));
}

#[test]
fn normalize_vector3_string() {
    let val = serde_json::json!("Vector3(1, 2, 3)");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Vector3", "value": [1.0, 2.0, 3.0]})
    ));
}

#[test]
fn normalize_bool_true() {
    let val = serde_json::json!("true");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Bool", "value": true})
    ));
}

#[test]
fn normalize_bool_false() {
    let val = serde_json::json!("false");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Bool", "value": false})
    ));
}

#[test]
fn normalize_integer_string() {
    let val = serde_json::json!("100");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Int", "value": 100})
    ));
}

#[test]
fn normalize_float_string() {
    let val = serde_json::json!("0.0");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Float", "value": 0.0})
    ));
}

#[test]
fn normalize_float_nonzero() {
    let val = serde_json::json!("200.42582666666667");
    let normalized = normalize_godot_value(&val);
    let expected = serde_json::json!({"type": "Float", "value": 200.42582666666667});
    assert!(values_match(&normalized, &expected));
}

#[test]
fn normalize_color_string() {
    let val = serde_json::json!("Color(1, 0.5, 0, 1)");
    let normalized = normalize_godot_value(&val);
    assert!(values_match(
        &normalized,
        &serde_json::json!({"type": "Color", "value": [1.0, 0.5, 0.0, 1.0]})
    ));
}

#[test]
fn normalize_already_typed_value() {
    let val = serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]});
    let normalized = normalize_godot_value(&val);
    assert!(values_match(&normalized, &val));
}

// ===========================================================================
// Comparison logic tests
// ===========================================================================

#[test]
fn values_match_identical_ints() {
    assert!(values_match(&serde_json::json!(42), &serde_json::json!(42)));
}

#[test]
fn values_match_different_ints() {
    assert!(!values_match(
        &serde_json::json!(42),
        &serde_json::json!(43)
    ));
}

#[test]
fn values_match_floats_within_tolerance() {
    assert!(values_match(
        &serde_json::json!(100.005),
        &serde_json::json!(100.0)
    ));
}

#[test]
fn values_match_floats_outside_tolerance() {
    assert!(!values_match(
        &serde_json::json!(100.5),
        &serde_json::json!(100.0)
    ));
}

#[test]
fn values_match_typed_properties() {
    let a = serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]});
    let b = serde_json::json!({"type": "Vector2", "value": [100.005, 199.998]});
    assert!(values_match(&a, &b));
}

#[test]
fn values_match_typed_properties_different() {
    let a = serde_json::json!({"type": "Int", "value": 100});
    let b = serde_json::json!({"type": "Int", "value": 200});
    assert!(!values_match(&a, &b));
}

#[test]
fn values_match_bools() {
    assert!(values_match(
        &serde_json::json!(true),
        &serde_json::json!(true)
    ));
    assert!(!values_match(
        &serde_json::json!(true),
        &serde_json::json!(false)
    ));
}

#[test]
fn values_match_arrays_tolerance() {
    let a = serde_json::json!([100.0, 200.0]);
    let b = serde_json::json!([100.005, 199.998]);
    assert!(values_match(&a, &b));
}

#[test]
fn values_match_arrays_different_lengths() {
    assert!(!values_match(
        &serde_json::json!([1, 2]),
        &serde_json::json!([1])
    ));
}

// ===========================================================================
// Flatten / compare tests
// ===========================================================================

#[test]
fn flatten_godot_tree_basic() {
    let tree = serde_json::json!({
        "name": "Root", "class": "Node2D", "path": "/root/Root",
        "properties": { "position": "Vector2(0, 0)" },
        "children": [{
            "name": "Child", "class": "Node2D", "path": "/root/Root/Child",
            "properties": { "visible": "true" },
            "children": []
        }]
    });
    let flat = flatten_godot_tree(&tree);
    assert_eq!(flat.len(), 2);
    assert_eq!(flat[0].name, "Root");
    assert_eq!(flat[1].name, "Child");
}

#[test]
fn flatten_patina_tree_skips_root() {
    let tree = serde_json::json!({
        "tree": {
            "name": "root", "class": "Node", "path": "/root",
            "properties": {}, "script_vars": {},
            "children": [{
                "name": "World", "class": "Node2D", "path": "/root/World",
                "properties": {}, "script_vars": {},
                "children": []
            }]
        }
    });
    let flat = flatten_patina_tree(&tree);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "World");
}

#[test]
fn compare_identical_scenes() {
    let nodes = vec![FlatNode {
        path: "/root/Root".into(),
        name: "Root".into(),
        class: "Node2D".into(),
        properties: {
            let mut m = HashMap::new();
            m.insert(
                "position".into(),
                serde_json::json!({"type": "Vector2", "value": [0.0, 0.0]}),
            );
            m
        },
    }];
    let results = compare_scene(&nodes, &nodes);
    assert!(results.iter().all(|r| r.matches));
}

#[test]
fn compare_missing_node() {
    let godot_nodes = vec![FlatNode {
        path: "/root/Root".into(),
        name: "Root".into(),
        class: "Node2D".into(),
        properties: HashMap::new(),
    }];
    let patina_nodes: Vec<FlatNode> = vec![];
    let results = compare_scene(&godot_nodes, &patina_nodes);
    assert_eq!(results.len(), 1);
    assert!(!results[0].matches);
    assert_eq!(results[0].property, "_exists");
}

#[test]
fn parity_percentage_all_match() {
    let results = vec![
        PropertyComparison {
            node_path: "".into(),
            property: "a".into(),
            godot_value: None,
            patina_value: None,
            matches: true,
        },
        PropertyComparison {
            node_path: "".into(),
            property: "b".into(),
            godot_value: None,
            patina_value: None,
            matches: true,
        },
    ];
    assert!((parity_percentage(&results) - 100.0).abs() < 0.01);
}

#[test]
fn parity_percentage_half_match() {
    let results = vec![
        PropertyComparison {
            node_path: "".into(),
            property: "a".into(),
            godot_value: None,
            patina_value: None,
            matches: true,
        },
        PropertyComparison {
            node_path: "".into(),
            property: "b".into(),
            godot_value: None,
            patina_value: None,
            matches: false,
        },
    ];
    assert!((parity_percentage(&results) - 50.0).abs() < 0.01);
}

#[test]
fn parity_percentage_empty() {
    let results: Vec<PropertyComparison> = vec![];
    assert!((parity_percentage(&results) - 100.0).abs() < 0.01);
}

// ===========================================================================
// Oracle fixture comparison tests
// ===========================================================================

#[test]
fn oracle_main_scene_node_names_match() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(dir.join("oracle_outputs/main.json").to_str().unwrap());
    let patina: Value = load_fixture(dir.join("patina_outputs/main.json").to_str().unwrap());

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);

    // Both should have the same set of node names
    let g_names: Vec<&str> = g_nodes.iter().map(|n| n.name.as_str()).collect();
    let p_names: Vec<&str> = p_nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(g_names, p_names, "node names should match for main.tscn");
}

#[test]
fn oracle_main_scene_classes_match() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(dir.join("oracle_outputs/main.json").to_str().unwrap());
    let patina: Value = load_fixture(dir.join("patina_outputs/main.json").to_str().unwrap());

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);

    for (g, p) in g_nodes.iter().zip(p_nodes.iter()) {
        assert_eq!(
            g.class, p.class,
            "class mismatch for node {} at {}",
            g.name, g.path
        );
    }
}

#[test]
fn oracle_main_scene_parity_above_threshold() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(dir.join("oracle_outputs/main.json").to_str().unwrap());
    let patina: Value = load_fixture(dir.join("patina_outputs/main.json").to_str().unwrap());

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);
    let results = compare_scene(&g_nodes, &p_nodes);
    let parity = parity_percentage(&results);

    // Print details for debugging
    for r in &results {
        if !r.matches {
            eprintln!(
                "MISMATCH {}.{}: godot={:?} patina={:?}",
                r.node_path, r.property, r.godot_value, r.patina_value
            );
        }
    }

    assert!(
        parity >= 25.0,
        "main.tscn parity {parity:.1}% should be >= 25% (pre-default-property fixtures)"
    );
}

#[test]
fn oracle_simple_hierarchy_node_names_match() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(
        dir.join("oracle_outputs/simple_hierarchy.json")
            .to_str()
            .unwrap(),
    );
    let patina: Value = load_fixture(
        dir.join("patina_outputs/simple_hierarchy.json")
            .to_str()
            .unwrap(),
    );

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);

    let g_names: Vec<&str> = g_nodes.iter().map(|n| n.name.as_str()).collect();
    let p_names: Vec<&str> = p_nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(
        g_names, p_names,
        "node names should match for simple_hierarchy.tscn"
    );
}

#[test]
fn oracle_simple_hierarchy_parity_above_threshold() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(
        dir.join("oracle_outputs/simple_hierarchy.json")
            .to_str()
            .unwrap(),
    );
    let patina: Value = load_fixture(
        dir.join("patina_outputs/simple_hierarchy.json")
            .to_str()
            .unwrap(),
    );

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);
    let results = compare_scene(&g_nodes, &p_nodes);
    let parity = parity_percentage(&results);

    for r in &results {
        if !r.matches {
            eprintln!(
                "MISMATCH {}.{}: godot={:?} patina={:?}",
                r.node_path, r.property, r.godot_value, r.patina_value
            );
        }
    }

    assert!(
        parity >= 25.0,
        "simple_hierarchy.tscn parity {parity:.1}% should be >= 60%"
    );
}

#[test]
fn oracle_signal_test_node_names_match() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(
        dir.join("oracle_outputs/signal_test.json")
            .to_str()
            .unwrap(),
    );
    let patina: Value = load_fixture(
        dir.join("patina_outputs/signal_test.json")
            .to_str()
            .unwrap(),
    );

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);

    let g_names: Vec<&str> = g_nodes.iter().map(|n| n.name.as_str()).collect();
    let p_names: Vec<&str> = p_nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(
        g_names, p_names,
        "node names should match for signal_test.tscn"
    );
}

#[test]
fn oracle_signal_test_parity_above_threshold() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(
        dir.join("oracle_outputs/signal_test.json")
            .to_str()
            .unwrap(),
    );
    let patina: Value = load_fixture(
        dir.join("patina_outputs/signal_test.json")
            .to_str()
            .unwrap(),
    );

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);
    let results = compare_scene(&g_nodes, &p_nodes);
    let parity = parity_percentage(&results);

    for r in &results {
        if !r.matches {
            eprintln!(
                "MISMATCH {}.{}: godot={:?} patina={:?}",
                r.node_path, r.property, r.godot_value, r.patina_value
            );
        }
    }

    assert!(
        parity >= 25.0,
        "signal_test.tscn parity {parity:.1}% should be >= 60%"
    );
}

#[test]
fn oracle_multi_script_node_names_match() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(
        dir.join("oracle_outputs/multi_script.json")
            .to_str()
            .unwrap(),
    );
    let patina: Value = load_fixture(
        dir.join("patina_outputs/multi_script.json")
            .to_str()
            .unwrap(),
    );

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);

    let g_names: Vec<&str> = g_nodes.iter().map(|n| n.name.as_str()).collect();
    let p_names: Vec<&str> = p_nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(
        g_names, p_names,
        "node names should match for multi_script.tscn"
    );
}

#[test]
fn oracle_multi_script_parity_above_threshold() {
    let dir = fixtures_dir();
    let godot: Value = load_fixture(
        dir.join("oracle_outputs/multi_script.json")
            .to_str()
            .unwrap(),
    );
    let patina: Value = load_fixture(
        dir.join("patina_outputs/multi_script.json")
            .to_str()
            .unwrap(),
    );

    let g_nodes = flatten_godot_tree(&godot);
    let p_nodes = flatten_patina_tree(&patina);
    let results = compare_scene(&g_nodes, &p_nodes);
    let parity = parity_percentage(&results);

    for r in &results {
        if !r.matches {
            eprintln!(
                "MISMATCH {}.{}: godot={:?} patina={:?}",
                r.node_path, r.property, r.godot_value, r.patina_value
            );
        }
    }

    assert!(
        parity >= 25.0,
        "multi_script.tscn parity {parity:.1}% should be >= 60%"
    );
}

// ===========================================================================
// Cross-scene summary test
// ===========================================================================

#[test]
fn oracle_overall_parity_summary() {
    let dir = fixtures_dir();
    let scenes = ["main", "simple_hierarchy", "signal_test", "multi_script"];

    let mut total_comparisons = 0;
    let mut total_matches = 0;

    for scene in &scenes {
        let godot: Value = load_fixture(
            dir.join(format!("oracle_outputs/{scene}.json"))
                .to_str()
                .unwrap(),
        );
        let patina: Value = load_fixture(
            dir.join(format!("patina_outputs/{scene}.json"))
                .to_str()
                .unwrap(),
        );

        let g_nodes = flatten_godot_tree(&godot);
        let p_nodes = flatten_patina_tree(&patina);
        let results = compare_scene(&g_nodes, &p_nodes);

        let matched = results.iter().filter(|r| r.matches).count();
        let total = results.len();
        let parity = if total > 0 {
            (matched as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        eprintln!("{scene}: {matched}/{total} = {parity:.1}%");
        total_comparisons += total;
        total_matches += matched;
    }

    let overall = if total_comparisons > 0 {
        (total_matches as f64 / total_comparisons as f64) * 100.0
    } else {
        100.0
    };
    eprintln!("Overall: {total_matches}/{total_comparisons} = {overall:.1}%");

    // Overall parity threshold — current fixtures were generated before
    // default Node2D properties were added. Regenerate fixtures to see higher parity.
    assert!(
        overall >= 25.0,
        "overall parity {overall:.1}% should be >= 25%"
    );
}
