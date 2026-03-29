//! Oracle golden regression tests — compare Godot oracle JSON against live Patina runner output.
//!
//! Loads golden JSON files from `../fixtures/oracle_outputs/`, runs the Patina
//! headless runner on the corresponding `.tscn` scene, and compares:
//! - node count, node names and paths, node class names
//! - property values (with float tolerance per TEST_ORACLE.md)
//!
//! Each test states the observable behavior it checks (Oracle Rule 2).

mod oracle_fixture;

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use oracle_fixture::{fixtures_dir, load_json_fixture};

// ---------------------------------------------------------------------------
// Patina runner execution
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

fn run_patina_on_scene(scene_path: &Path) -> Value {
    let binary = runner_binary();
    let output = Command::new(&binary)
        .arg(scene_path.to_str().expect("valid UTF-8"))
        .arg("--frames")
        .arg("0")
        .output()
        .unwrap_or_else(|e| panic!("failed to execute patina-runner: {e}"));
    assert!(
        output.status.success(),
        "patina-runner failed on {}:\n{}",
        scene_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON from patina-runner for {}:\n{e}",
            scene_path.display()
        )
    })
}

// ---------------------------------------------------------------------------
// Oracle golden loading
// ---------------------------------------------------------------------------

fn oracle_outputs_dir() -> PathBuf {
    fixtures_dir().join("oracle_outputs")
}
fn scenes_dir() -> PathBuf {
    fixtures_dir().join("scenes")
}
fn load_oracle_tree(name: &str) -> Value {
    load_json_fixture(&oracle_outputs_dir().join(format!("{name}_tree.json")))
}
fn load_oracle_properties(name: &str) -> Value {
    load_json_fixture(&oracle_outputs_dir().join(format!("{name}_properties.json")))
}
fn scene_path(name: &str) -> PathBuf {
    scenes_dir().join(format!("{name}.tscn"))
}

// ---------------------------------------------------------------------------
// Format normalization
// ---------------------------------------------------------------------------

fn normalize_godot_value(val: &Value) -> Value {
    match val {
        Value::String(s) => parse_godot_string_value(s),
        Value::Object(map) => {
            if let (Some(Value::String(ty)), Some(inner)) = (map.get("type"), map.get("value")) {
                let normalized = normalize_typed_value(ty, inner);
                let mut out = serde_json::Map::new();
                out.insert("type".into(), Value::String(ty.clone()));
                out.insert("value".into(), normalized);
                return Value::Object(out);
            }
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

fn normalize_typed_value(ty: &str, val: &Value) -> Value {
    match ty {
        "Vector2" | "Vector2i" => {
            if let Value::Object(o) = val {
                if let (Some(x), Some(y)) = (o.get("x"), o.get("y")) {
                    return serde_json::json!([x, y]);
                }
            } else {
            }
        }
        "Vector3" | "Vector3i" => {
            if let Value::Object(o) = val {
                if let (Some(x), Some(y), Some(z)) = (o.get("x"), o.get("y"), o.get("z")) {
                    return serde_json::json!([x, y, z]);
                }
            } else {
            }
        }
        "Color" => {
            if let Value::Object(o) = val {
                if let (Some(r), Some(g), Some(b), Some(a)) =
                    (o.get("r"), o.get("g"), o.get("b"), o.get("a"))
                {
                    return serde_json::json!([r, g, b, a]);
                }
            } else {
            }
        }
        _ => {}
    }
    val.clone()
}

fn parse_godot_string_value(s: &str) -> Value {
    if let Some(inner) = s.strip_prefix("Vector2(").and_then(|s| s.strip_suffix(')')) {
        let p: Vec<&str> = inner.split(',').collect();
        if p.len() == 2 {
            if let (Ok(x), Ok(y)) = (p[0].trim().parse::<f64>(), p[1].trim().parse::<f64>()) {
                return serde_json::json!({"type": "Vector2", "value": [x, y]});
            }
        }
    }
    if let Some(inner) = s.strip_prefix("Vector3(").and_then(|s| s.strip_suffix(')')) {
        let p: Vec<&str> = inner.split(',').collect();
        if p.len() == 3 {
            if let (Ok(x), Ok(y), Ok(z)) = (
                p[0].trim().parse::<f64>(),
                p[1].trim().parse::<f64>(),
                p[2].trim().parse::<f64>(),
            ) {
                return serde_json::json!({"type": "Vector3", "value": [x, y, z]});
            }
        }
    }
    if let Some(inner) = s.strip_prefix("Color(").and_then(|s| s.strip_suffix(')')) {
        let p: Vec<&str> = inner.split(',').collect();
        if p.len() == 4 {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                p[0].trim().parse::<f64>(),
                p[1].trim().parse::<f64>(),
                p[2].trim().parse::<f64>(),
                p[3].trim().parse::<f64>(),
            ) {
                return serde_json::json!({"type": "Color", "value": [r, g, b, a]});
            }
        }
    }
    match s {
        "true" => return serde_json::json!({"type": "Bool", "value": true}),
        "false" => return serde_json::json!({"type": "Bool", "value": false}),
        _ => {}
    }
    if let Ok(i) = s.parse::<i64>() {
        return serde_json::json!({"type": "Int", "value": i});
    }
    if let Ok(f) = s.parse::<f64>() {
        return serde_json::json!({"type": "Float", "value": f});
    }
    serde_json::json!({"type": "String", "value": s})
}

// ---------------------------------------------------------------------------
// Comparison logic
// ---------------------------------------------------------------------------

const FLOAT_TOLERANCE: f64 = 0.01;

#[derive(Debug, Clone)]
struct FlatNode {
    path: String,
    name: String,
    class: String,
    properties: HashMap<String, Value>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct PropertyComparison {
    node_path: String,
    property: String,
    godot_value: Option<Value>,
    patina_value: Option<Value>,
    matches: bool,
}

fn flatten_godot_tree(root: &Value) -> Vec<FlatNode> {
    let mut r = Vec::new();
    if let Some(ch) = root.get("children").and_then(|c| c.as_array()) {
        for c in ch {
            flatten_godot_node(c, &mut r);
        }
    }
    r
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
        for (k, v) in props {
            properties.insert(k.clone(), normalize_godot_value(v));
        }
    }
    out.push(FlatNode {
        path,
        name,
        class,
        properties,
    });
    if let Some(ch) = node.get("children").and_then(|c| c.as_array()) {
        for c in ch {
            flatten_godot_node(c, out);
        }
    }
}

fn flatten_patina_tree(root: &Value) -> Vec<FlatNode> {
    let tree = root.get("tree").unwrap_or(root);
    let mut r = Vec::new();
    if let Some(ch) = tree.get("children").and_then(|c| c.as_array()) {
        for c in ch {
            flatten_patina_node(c, &mut r);
        }
    }
    r
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
        for (k, v) in props {
            if !k.starts_with('_') && k != "script" {
                properties.insert(k.clone(), v.clone());
            }
        }
    }
    out.push(FlatNode {
        path,
        name,
        class,
        properties,
    });
    if let Some(ch) = node.get("children").and_then(|c| c.as_array()) {
        for c in ch {
            flatten_patina_node(c, out);
        }
    }
}

fn values_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(an), Value::Number(bn)) => {
            (an.as_f64().unwrap_or(0.0) - bn.as_f64().unwrap_or(0.0)).abs() <= FLOAT_TOLERANCE
        }
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_match(x, y))
        }
        (Value::Object(ao), Value::Object(bo)) => {
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

fn compare_scene(godot: &[FlatNode], patina: &[FlatNode]) -> Vec<PropertyComparison> {
    let mut results = Vec::new();
    let p_map: HashMap<&str, &FlatNode> = patina.iter().map(|n| (n.path.as_str(), n)).collect();
    for g in godot {
        if let Some(p) = p_map.get(g.path.as_str()) {
            results.push(PropertyComparison {
                node_path: g.path.clone(),
                property: "_class".into(),
                godot_value: Some(Value::String(g.class.clone())),
                patina_value: Some(Value::String(p.class.clone())),
                matches: g.class == p.class,
            });
            let mut keys: Vec<&String> = g.properties.keys().collect();
            for k in p.properties.keys() {
                if !g.properties.contains_key(k) {
                    keys.push(k);
                }
            }
            keys.sort();
            keys.dedup();
            for key in keys {
                let gv = g.properties.get(key);
                let pv = p.properties.get(key);
                let m = match (gv, pv) {
                    (Some(a), Some(b)) => values_match(a, b),
                    (None, None) => true,
                    _ => false,
                };
                results.push(PropertyComparison {
                    node_path: g.path.clone(),
                    property: key.clone(),
                    godot_value: gv.cloned(),
                    patina_value: pv.cloned(),
                    matches: m,
                });
            }
        } else {
            results.push(PropertyComparison {
                node_path: g.path.clone(),
                property: "_exists".into(),
                godot_value: Some(Value::Bool(true)),
                patina_value: Some(Value::Bool(false)),
                matches: false,
            });
        }
    }
    results
}

fn parity_percentage(results: &[PropertyComparison]) -> f64 {
    if results.is_empty() {
        return 100.0;
    }
    (results.iter().filter(|r| r.matches).count() as f64 / results.len() as f64) * 100.0
}

fn collect_godot_names(root: &Value) -> Vec<String> {
    flatten_godot_tree(root)
        .iter()
        .map(|n| n.name.clone())
        .collect()
}
fn collect_patina_names(root: &Value) -> Vec<String> {
    flatten_patina_tree(root)
        .iter()
        .map(|n| n.name.clone())
        .collect()
}
fn collect_godot_classes(root: &Value) -> Vec<(String, String)> {
    flatten_godot_tree(root)
        .iter()
        .map(|n| (n.name.clone(), n.class.clone()))
        .collect()
}
fn collect_patina_classes(root: &Value) -> Vec<(String, String)> {
    flatten_patina_tree(root)
        .iter()
        .map(|n| (n.name.clone(), n.class.clone()))
        .collect()
}

const GOLDEN_SCENES: &[&str] = &[
    "minimal",
    "hierarchy",
    "with_properties",
    "space_shooter",
    "platformer",
    "physics_playground",
    "signals_complex",
    "test_scripts",
    "ui_menu",
];

// ===========================================================================
// 1. Format normalization tests
// ===========================================================================

#[test]
fn normalize_vector2_string() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("Vector2(100, 200)")),
        &serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]})
    ));
}
#[test]
fn normalize_vector2_negative() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("Vector2(-50.5, 100.3)")),
        &serde_json::json!({"type": "Vector2", "value": [-50.5, 100.3]})
    ));
}
#[test]
fn normalize_vector3_string() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("Vector3(1, 2, 3)")),
        &serde_json::json!({"type": "Vector3", "value": [1.0, 2.0, 3.0]})
    ));
}
#[test]
fn normalize_bool_true() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("true")),
        &serde_json::json!({"type": "Bool", "value": true})
    ));
}
#[test]
fn normalize_bool_false() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("false")),
        &serde_json::json!({"type": "Bool", "value": false})
    ));
}
#[test]
fn normalize_integer_string() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("100")),
        &serde_json::json!({"type": "Int", "value": 100})
    ));
}
#[test]
fn normalize_float_string() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("0.0")),
        &serde_json::json!({"type": "Float", "value": 0.0})
    ));
}
#[test]
fn normalize_float_nonzero() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("200.42582666666667")),
        &serde_json::json!({"type": "Float", "value": 200.42582666666667})
    ));
}
#[test]
fn normalize_color_string() {
    assert!(values_match(
        &normalize_godot_value(&serde_json::json!("Color(1, 0.5, 0, 1)")),
        &serde_json::json!({"type": "Color", "value": [1.0, 0.5, 0.0, 1.0]})
    ));
}
#[test]
fn normalize_already_typed_value() {
    let v = serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]});
    assert!(values_match(&normalize_godot_value(&v), &v));
}

// ===========================================================================
// 2. Comparison logic tests
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
    assert!(values_match(
        &serde_json::json!({"type":"Vector2","value":[100.0,200.0]}),
        &serde_json::json!({"type":"Vector2","value":[100.005,199.998]})
    ));
}
#[test]
fn values_match_typed_properties_different() {
    assert!(!values_match(
        &serde_json::json!({"type":"Int","value":100}),
        &serde_json::json!({"type":"Int","value":200})
    ));
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
    assert!(values_match(
        &serde_json::json!([100.0, 200.0]),
        &serde_json::json!([100.005, 199.998])
    ));
}
#[test]
fn values_match_arrays_different_lengths() {
    assert!(!values_match(
        &serde_json::json!([1, 2]),
        &serde_json::json!([1])
    ));
}

// ===========================================================================
// 3. Flatten / compare unit tests
// ===========================================================================

#[test]
fn flatten_godot_tree_basic() {
    let tree = serde_json::json!({"name":"root","class":"Window","path":"/root","children":[
        {"name":"Root","class":"Node2D","path":"/root/Root","properties":{"position":"Vector2(0, 0)"},"children":[
            {"name":"Child","class":"Node2D","path":"/root/Root/Child","properties":{"visible":"true"},"children":[]}
        ]}
    ]});
    let flat = flatten_godot_tree(&tree);
    assert_eq!(flat.len(), 2);
    assert_eq!(flat[0].name, "Root");
    assert_eq!(flat[1].name, "Child");
}

#[test]
fn flatten_patina_tree_skips_root() {
    let tree = serde_json::json!({"tree":{"name":"root","class":"Node","path":"/root","properties":{},"script_vars":{},"children":[
        {"name":"World","class":"Node2D","path":"/root/World","properties":{},"script_vars":{},"children":[]}
    ]}});
    let flat = flatten_patina_tree(&tree);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "World");
}

#[test]
fn compare_identical_scenes() {
    let nodes = vec![FlatNode {
        path: "/root/R".into(),
        name: "R".into(),
        class: "Node2D".into(),
        properties: {
            let mut m = HashMap::new();
            m.insert(
                "position".into(),
                serde_json::json!({"type":"Vector2","value":[0.0,0.0]}),
            );
            m
        },
    }];
    assert!(compare_scene(&nodes, &nodes).iter().all(|r| r.matches));
}

#[test]
fn compare_missing_node() {
    let g = vec![FlatNode {
        path: "/root/R".into(),
        name: "R".into(),
        class: "Node2D".into(),
        properties: HashMap::new(),
    }];
    let r = compare_scene(&g, &[]);
    assert_eq!(r.len(), 1);
    assert!(!r[0].matches);
    assert_eq!(r[0].property, "_exists");
}

#[test]
fn parity_percentage_calculations() {
    let mk = |m| PropertyComparison {
        node_path: "".into(),
        property: "x".into(),
        godot_value: None,
        patina_value: None,
        matches: m,
    };
    assert!((parity_percentage(&[mk(true), mk(true)]) - 100.0).abs() < 0.01);
    assert!((parity_percentage(&[mk(true), mk(false)]) - 50.0).abs() < 0.01);
    assert!((parity_percentage(&[]) - 100.0).abs() < 0.01);
}

// ===========================================================================
// 4. Stale golden detection
// ===========================================================================

#[test]
fn every_fixture_scene_has_oracle_tree_golden() {
    let (scenes, oracle) = (scenes_dir(), oracle_outputs_dir());
    let mut missing = Vec::new();
    for e in std::fs::read_dir(&scenes).unwrap() {
        let f = e.unwrap().file_name().to_string_lossy().to_string();
        if !f.ends_with(".tscn") {
            continue;
        }
        let b = f.trim_end_matches(".tscn");
        if !oracle.join(format!("{b}_tree.json")).exists() {
            missing.push(f);
        }
    }
    assert!(
        missing.is_empty(),
        "Scenes missing oracle tree goldens: {missing:?}"
    );
}

#[test]
fn every_oracle_tree_golden_has_matching_properties_golden() {
    let oracle = oracle_outputs_dir();
    let mut missing = Vec::new();
    for e in std::fs::read_dir(&oracle).unwrap() {
        let f = e.unwrap().file_name().to_string_lossy().to_string();
        if !f.ends_with("_tree.json") {
            continue;
        }
        let b = f.trim_end_matches("_tree.json");
        if !oracle.join(format!("{b}_properties.json")).exists() {
            missing.push(f);
        }
    }
    assert!(
        missing.is_empty(),
        "Tree goldens missing properties: {missing:?}"
    );
}

// ===========================================================================
// 5. Scene tree structure tests (per-scene)
// ===========================================================================

#[test]
fn golden_minimal_node_count_matches() {
    let (g, p) = (
        load_oracle_tree("minimal"),
        run_patina_on_scene(&scene_path("minimal")),
    );
    assert_eq!(
        collect_godot_names(&g).len(),
        collect_patina_names(&p).len(),
        "minimal: node count"
    );
}
#[test]
fn golden_minimal_node_names_match() {
    let (g, p) = (
        load_oracle_tree("minimal"),
        run_patina_on_scene(&scene_path("minimal")),
    );
    assert_eq!(collect_godot_names(&g), collect_patina_names(&p));
}
#[test]
fn golden_minimal_node_classes_match() {
    let (g, p) = (
        load_oracle_tree("minimal"),
        run_patina_on_scene(&scene_path("minimal")),
    );
    assert_eq!(collect_godot_classes(&g), collect_patina_classes(&p));
}
#[test]
fn golden_hierarchy_tree_structure_matches() {
    let (g, p) = (
        flatten_godot_tree(&load_oracle_tree("hierarchy")),
        flatten_patina_tree(&run_patina_on_scene(&scene_path("hierarchy"))),
    );
    assert_eq!(g.len(), p.len(), "hierarchy: node count");
    for (gn, pn) in g.iter().zip(p.iter()) {
        assert_eq!(gn.name, pn.name);
        assert_eq!(gn.class, pn.class);
    }
}
#[test]
fn golden_with_properties_node_names_match() {
    assert_eq!(
        collect_godot_names(&load_oracle_tree("with_properties")),
        collect_patina_names(&run_patina_on_scene(&scene_path("with_properties")))
    );
}
#[test]
fn golden_space_shooter_tree_structure_matches() {
    let (g, p) = (
        flatten_godot_tree(&load_oracle_tree("space_shooter")),
        flatten_patina_tree(&run_patina_on_scene(&scene_path("space_shooter"))),
    );
    assert_eq!(g.len(), p.len(), "space_shooter: node count");
    for (gn, pn) in g.iter().zip(p.iter()) {
        assert_eq!(gn.name, pn.name);
        assert_eq!(gn.class, pn.class);
    }
}
#[test]
fn golden_platformer_tree_structure_matches() {
    let (g, p) = (
        collect_godot_names(&load_oracle_tree("platformer")),
        collect_patina_names(&run_patina_on_scene(&scene_path("platformer"))),
    );
    assert_eq!(g, p, "platformer: node names");
}
#[test]
fn golden_ui_menu_tree_structure_matches() {
    assert_eq!(
        collect_godot_classes(&load_oracle_tree("ui_menu")),
        collect_patina_classes(&run_patina_on_scene(&scene_path("ui_menu")))
    );
}

// ===========================================================================
// 6. Property value comparison
// ===========================================================================

#[test]
fn golden_with_properties_player_position_matches() {
    let (g, p) = (
        flatten_godot_tree(&load_oracle_properties("with_properties")),
        flatten_patina_tree(&run_patina_on_scene(&scene_path("with_properties"))),
    );
    let (gp, pp) = (
        g.iter().find(|n| n.name == "Player").unwrap(),
        p.iter().find(|n| n.name == "Player").unwrap(),
    );
    assert!(
        values_match(
            gp.properties.get("position").unwrap(),
            pp.properties.get("position").unwrap()
        ),
        "Player.position mismatch"
    );
}
#[test]
fn golden_space_shooter_player_position_matches() {
    let (g, p) = (
        flatten_godot_tree(&load_oracle_properties("space_shooter")),
        flatten_patina_tree(&run_patina_on_scene(&scene_path("space_shooter"))),
    );
    let (gp, pp) = (
        g.iter().find(|n| n.name == "Player").unwrap(),
        p.iter().find(|n| n.name == "Player").unwrap(),
    );
    if let (Some(gv), Some(pv)) = (gp.properties.get("position"), pp.properties.get("position")) {
        assert!(values_match(gv, pv), "Player.position mismatch");
    }
}
#[test]
fn golden_with_properties_background_modulate_matches() {
    let (g, p) = (
        flatten_godot_tree(&load_oracle_properties("with_properties")),
        flatten_patina_tree(&run_patina_on_scene(&scene_path("with_properties"))),
    );
    let (gb, pb) = (
        g.iter().find(|n| n.name == "Background").unwrap(),
        p.iter().find(|n| n.name == "Background").unwrap(),
    );
    if let (Some(gv), Some(pv)) = (gb.properties.get("modulate"), pb.properties.get("modulate")) {
        assert!(values_match(gv, pv), "Background.modulate mismatch");
    }
}

// ===========================================================================
// 7. Full parity across all golden scenes
// ===========================================================================

#[test]
fn golden_all_scenes_node_count_parity() {
    let mut fails = Vec::new();
    for n in GOLDEN_SCENES {
        let t = scene_path(n);
        if !t.exists() {
            continue;
        }
        let (g, p) = (
            flatten_godot_tree(&load_oracle_tree(n)).len(),
            flatten_patina_tree(&run_patina_on_scene(&t)).len(),
        );
        if g != p {
            fails.push(format!("{n}: godot={g} patina={p}"));
        }
    }
    assert!(
        fails.is_empty(),
        "Node count mismatches:\n{}",
        fails.join("\n")
    );
}
#[test]
fn golden_all_scenes_node_names_parity() {
    let mut fails = Vec::new();
    for n in GOLDEN_SCENES {
        let t = scene_path(n);
        if !t.exists() {
            continue;
        }
        let (g, p) = (
            collect_godot_names(&load_oracle_tree(n)),
            collect_patina_names(&run_patina_on_scene(&t)),
        );
        if g != p {
            fails.push(format!("{n}: godot={g:?} patina={p:?}"));
        }
    }
    assert!(
        fails.is_empty(),
        "Node name mismatches:\n{}",
        fails.join("\n")
    );
}
#[test]
fn golden_all_scenes_node_classes_parity() {
    let mut fails = Vec::new();
    for n in GOLDEN_SCENES {
        let t = scene_path(n);
        if !t.exists() {
            continue;
        }
        let (g, p) = (
            collect_godot_classes(&load_oracle_tree(n)),
            collect_patina_classes(&run_patina_on_scene(&t)),
        );
        if g != p {
            fails.push(format!("{n}: godot={g:?} patina={p:?}"));
        }
    }
    assert!(
        fails.is_empty(),
        "Node class mismatches:\n{}",
        fails.join("\n")
    );
}

#[test]
fn golden_all_scenes_property_parity_report() {
    let mut total = 0usize;
    let mut matched = 0usize;
    let mut per: Vec<(String, usize, usize, f64)> = Vec::new();
    for n in GOLDEN_SCENES {
        let t = scene_path(n);
        if !t.exists() {
            continue;
        }
        let r = compare_scene(
            &flatten_godot_tree(&load_oracle_properties(n)),
            &flatten_patina_tree(&run_patina_on_scene(&t)),
        );
        let m = r.iter().filter(|x| x.matches).count();
        per.push((n.to_string(), r.len(), m, parity_percentage(&r)));
        total += r.len();
        matched += m;
    }
    let overall = if total > 0 {
        (matched as f64 / total as f64) * 100.0
    } else {
        100.0
    };
    eprintln!("\n=== Oracle Golden Parity Report ===");
    eprintln!(
        "{:<25} {:>8} {:>8} {:>8}",
        "Scene", "Total", "Match", "Parity"
    );
    eprintln!("{}", "-".repeat(55));
    for (n, t, m, p) in &per {
        eprintln!("{:<25} {:>8} {:>8} {:>7.1}%", n, t, m, p);
    }
    eprintln!("{}", "-".repeat(55));
    eprintln!(
        "{:<25} {:>8} {:>8} {:>7.1}%",
        "OVERALL", total, matched, overall
    );
    assert!(total >= 9);
}

// ===========================================================================
// 8. Oracle golden file integrity
// ===========================================================================

#[test]
fn oracle_outputs_has_expected_golden_count() {
    let c = std::fs::read_dir(oracle_outputs_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .count();
    assert!(c >= 27, "Expected >= 27 oracle goldens, found {c}");
}

#[test]
fn oracle_tree_goldens_have_valid_structure() {
    let mut fails = Vec::new();
    for e in std::fs::read_dir(oracle_outputs_dir()).unwrap() {
        let e = e.unwrap();
        let f = e.file_name().to_string_lossy().to_string();
        if !f.ends_with("_tree.json") {
            continue;
        }
        match serde_json::from_str::<Value>(&std::fs::read_to_string(e.path()).unwrap()) {
            Ok(v) => {
                if v.get("children").is_none() {
                    fails.push(format!("{f}: no children"));
                }
                if v.get("class").is_none() {
                    fails.push(format!("{f}: no class"));
                }
            }
            Err(e) => fails.push(format!("{f}: {e}")),
        }
    }
    assert!(fails.is_empty(), "Tree golden errors: {fails:?}");
}
