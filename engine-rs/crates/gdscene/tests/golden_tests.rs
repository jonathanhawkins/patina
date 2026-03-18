//! Golden-file integration tests for the Patina Engine.
//!
//! These tests load `.tscn` and `.tres` fixture files, process them through
//! the engine's parsers, dump the results to a JSON structure, and compare
//! against golden output files stored in `fixtures/golden/`.
//!
//! **Oracle rule**: Every test states what observable behavior it checks
//! (TEST_ORACLE.md Rule 2).

use std::collections::BTreeMap;
use std::path::PathBuf;

use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::serialize::to_json;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the path to the monorepo `fixtures/` directory.
fn fixtures_dir() -> PathBuf {
    // engine-rs/crates/gdscene -> engine-rs -> monorepo root
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap() // -> engine-rs/crates
        .parent()
        .unwrap() // -> engine-rs
        .parent()
        .unwrap() // -> monorepo root
        .join("fixtures")
}

/// Reads a fixture file and returns its contents as a string.
fn read_fixture(rel_path: &str) -> String {
    let path = fixtures_dir().join(rel_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

/// Reads a golden JSON file and returns it as a `serde_json::Value`.
fn read_golden(rel_path: &str) -> Value {
    let contents = read_fixture(rel_path);
    serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("failed to parse golden JSON {rel_path}: {e}"))
}

// ---------------------------------------------------------------------------
// Scene tree dumper
// ---------------------------------------------------------------------------

/// Recursively dumps a node and its children from the scene tree into a
/// JSON value matching the golden output format.
fn dump_node(tree: &SceneTree, node_id: NodeId) -> Value {
    let node = tree.get_node(node_id).unwrap();
    let path = tree.node_path(node_id).unwrap();

    // Collect properties in sorted order for deterministic output.
    let mut props = BTreeMap::new();
    for (key, value) in node.properties() {
        props.insert(key.clone(), to_json(value));
    }

    // Recurse into children (in order).
    let children: Vec<Value> = node
        .children()
        .iter()
        .map(|&child_id| dump_node(tree, child_id))
        .collect();

    json!({
        "name": node.name(),
        "class": node.class_name(),
        "path": path,
        "children": children,
        "properties": props,
    })
}

/// Loads a `.tscn` fixture, instances it into a SceneTree, and dumps the
/// scene root as JSON.
fn load_and_dump_scene(tscn_path: &str) -> Value {
    let source = read_fixture(tscn_path);
    let packed = PackedScene::from_tscn(&source)
        .unwrap_or_else(|e| panic!("failed to parse {tscn_path}: {e}"));

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let scene_root_id = add_packed_scene_to_tree(&mut tree, root_id, &packed)
        .unwrap_or_else(|e| panic!("failed to instance {tscn_path}: {e}"));

    dump_node(&tree, scene_root_id)
}

// ---------------------------------------------------------------------------
// Resource dumper
// ---------------------------------------------------------------------------

/// Dumps a `Resource` into JSON matching the golden output format.
fn dump_resource(res: &Resource) -> Value {
    let mut props = BTreeMap::new();
    for key in res.sorted_property_keys() {
        if let Some(value) = res.get_property(key) {
            props.insert(key.clone(), to_json(value));
        }
    }

    let mut subs = BTreeMap::new();
    let mut sub_keys: Vec<_> = res.subresources.keys().collect();
    sub_keys.sort();
    for key in sub_keys {
        let sub = &res.subresources[key];
        let mut sub_props = BTreeMap::new();
        for sub_key in sub.sorted_property_keys() {
            if let Some(value) = sub.get_property(sub_key) {
                sub_props.insert(sub_key.clone(), to_json(value));
            }
        }
        subs.insert(
            key.clone(),
            json!({
                "class_name": sub.class_name,
                "properties": sub_props,
            }),
        );
    }

    json!({
        "class_name": res.class_name,
        "properties": props,
        "subresources": subs,
    })
}

/// Loads a `.tres` fixture and dumps it as JSON.
fn load_and_dump_resource(tres_path: &str) -> Value {
    let source = read_fixture(tres_path);
    let loader = TresLoader::new();
    let res = loader
        .parse_str(&source, tres_path)
        .unwrap_or_else(|e| panic!("failed to parse {tres_path}: {e}"));
    dump_resource(&res)
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// Compares actual output against a golden file.
///
/// Only compares the structural fields (nodes, properties, etc.), ignoring
/// envelope metadata like `fixture_id`, `capture_type`, and `upstream_version`.
fn assert_scene_matches_golden(actual_tree_root: &Value, golden: &Value) {
    let golden_nodes = golden
        .get("nodes")
        .expect("golden file must have 'nodes' array");
    let golden_root = golden_nodes
        .as_array()
        .expect("'nodes' must be an array")
        .first()
        .expect("'nodes' array must have at least one element");

    assert_json_node_eq(actual_tree_root, golden_root, "");
}

/// Recursively compares two node JSON values.
fn assert_json_node_eq(actual: &Value, expected: &Value, context: &str) {
    let a_name = actual.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let e_name = expected.get("name").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        a_name, e_name,
        "node name mismatch at {context}: actual={a_name}, expected={e_name}"
    );

    let a_class = actual.get("class").and_then(|v| v.as_str()).unwrap_or("");
    let e_class = expected.get("class").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        a_class, e_class,
        "class mismatch at {context}/{a_name}: actual={a_class}, expected={e_class}"
    );

    let a_path = actual.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let e_path = expected.get("path").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        a_path, e_path,
        "path mismatch at {context}/{a_name}: actual={a_path}, expected={e_path}"
    );

    // Compare properties.
    let a_props = actual.get("properties").cloned().unwrap_or(json!({}));
    let e_props = expected.get("properties").cloned().unwrap_or(json!({}));
    assert_properties_eq(&a_props, &e_props, &format!("{context}/{a_name}"));

    // Compare children recursively.
    let a_children = actual
        .get("children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let e_children = expected
        .get("children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        a_children.len(),
        e_children.len(),
        "children count mismatch at {context}/{a_name}: actual={}, expected={}",
        a_children.len(),
        e_children.len()
    );

    for (ac, ec) in a_children.iter().zip(e_children.iter()) {
        assert_json_node_eq(ac, ec, &format!("{context}/{a_name}"));
    }
}

/// Compares property objects, handling floating-point tolerance.
fn assert_properties_eq(actual: &Value, expected: &Value, context: &str) {
    let a_obj = actual.as_object();
    let e_obj = expected.as_object();

    let a_keys: Vec<_> = a_obj.map(|o| o.keys().collect()).unwrap_or_default();
    let e_keys: Vec<_> = e_obj.map(|o| o.keys().collect()).unwrap_or_default();

    // Check that the same keys exist.
    let mut a_sorted = a_keys.clone();
    a_sorted.sort();
    let mut e_sorted = e_keys.clone();
    e_sorted.sort();
    assert_eq!(
        a_sorted, e_sorted,
        "property keys mismatch at {context}: actual={a_sorted:?}, expected={e_sorted:?}"
    );

    // Compare each property value.
    if let (Some(a_map), Some(e_map)) = (a_obj, e_obj) {
        for key in a_map.keys() {
            let a_val = &a_map[key];
            let e_val = &e_map[key];
            assert_variant_json_eq(a_val, e_val, &format!("{context}.{key}"));
        }
    }
}

/// Compares two Variant JSON representations with float tolerance.
fn assert_variant_json_eq(actual: &Value, expected: &Value, context: &str) {
    let a_type = actual.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let e_type = expected.get("type").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        a_type, e_type,
        "variant type mismatch at {context}: actual={a_type}, expected={e_type}"
    );

    match a_type {
        "Vector2" | "Color" => {
            let a_arr = actual.get("value").and_then(|v| v.as_array());
            let e_arr = expected.get("value").and_then(|v| v.as_array());
            if let (Some(a), Some(e)) = (a_arr, e_arr) {
                assert_eq!(a.len(), e.len(), "array length mismatch at {context}");
                for (i, (av, ev)) in a.iter().zip(e.iter()).enumerate() {
                    let af = av.as_f64().unwrap_or(0.0);
                    let ef = ev.as_f64().unwrap_or(0.0);
                    assert!(
                        (af - ef).abs() < 1e-6,
                        "float mismatch at {context}[{i}]: actual={af}, expected={ef}"
                    );
                }
            }
        }
        "Float" => {
            let af = actual.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let ef = expected.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
            assert!(
                (af - ef).abs() < 1e-6,
                "float mismatch at {context}: actual={af}, expected={ef}"
            );
        }
        _ => {
            // Exact comparison for non-float types.
            assert_eq!(
                actual, expected,
                "variant value mismatch at {context}: actual={actual}, expected={expected}"
            );
        }
    }
}

// ===========================================================================
// Scene golden tests
// ===========================================================================

/// Checks: Scene with a single root Node produces the expected tree structure.
/// Fixture: scenes/minimal.tscn
/// Comparison: Full scene tree structure (node name, class, path, children).
#[test]
fn golden_scene_minimal() {
    let actual = load_and_dump_scene("scenes/minimal.tscn");
    let golden = read_golden("golden/scenes/minimal.json");
    assert_scene_matches_golden(&actual, &golden);
}

/// Checks: 3-level node hierarchy (Root > Player > Sprite) produces correct
/// parent-child relationships and paths.
/// Fixture: scenes/hierarchy.tscn
/// Comparison: Full scene tree structure including nested children.
#[test]
fn golden_scene_hierarchy() {
    let actual = load_and_dump_scene("scenes/hierarchy.tscn");
    let golden = read_golden("golden/scenes/hierarchy.json");
    assert_scene_matches_golden(&actual, &golden);
}

/// Checks: Nodes with various property types (Vector2, Color, int, string, bool)
/// are parsed and stored correctly.
/// Fixture: scenes/with_properties.tscn
/// Comparison: Full scene tree structure including typed property values.
#[test]
fn golden_scene_with_properties() {
    let actual = load_and_dump_scene("scenes/with_properties.tscn");
    let golden = read_golden("golden/scenes/with_properties.json");
    assert_scene_matches_golden(&actual, &golden);
}

// ===========================================================================
// Resource golden tests
// ===========================================================================

/// Checks: Simple resource with basic property types (String, Int, Vector2, Bool)
/// is parsed correctly from .tres format.
/// Fixture: resources/simple.tres
/// Comparison: class_name, all property keys, types, and values.
#[test]
fn golden_resource_simple() {
    let actual = load_and_dump_resource("resources/simple.tres");
    let golden = read_golden("golden/resources/simple.json");
    assert_resource_matches_golden(&actual, &golden);
}

/// Checks: Resource with a sub_resource section is parsed correctly, with the
/// sub-resource's class name and properties preserved.
/// Fixture: resources/with_subresource.tres
/// Comparison: class_name, properties, sub-resource class_name and properties.
#[test]
fn golden_resource_with_subresource() {
    let actual = load_and_dump_resource("resources/with_subresource.tres");
    let golden = read_golden("golden/resources/with_subresource.json");
    assert_resource_matches_golden(&actual, &golden);
}

// ===========================================================================
// Phase 4 comprehensive fixture tests
// ===========================================================================

/// Checks: 2D platformer scene with 6 child nodes (Player, 3 Platforms,
/// Camera, Collectible) at various positions produces correct hierarchy.
/// Fixture: scenes/platformer.tscn
/// Comparison: Full tree structure with Vector2 positions and int properties.
#[test]
fn golden_scene_platformer() {
    let actual = load_and_dump_scene("scenes/platformer.tscn");
    let golden = read_golden("golden/scenes/platformer.json");
    assert_scene_matches_golden(&actual, &golden);
}

/// Checks: UI menu scene with Title (string property), 3 button nodes, and
/// 3 signal connections parses correctly.
/// Fixture: scenes/ui_menu.tscn
/// Comparison: Tree structure, string property, and connection count.
#[test]
fn golden_scene_ui_menu() {
    let actual = load_and_dump_scene("scenes/ui_menu.tscn");
    let golden = read_golden("golden/scenes/ui_menu.json");
    assert_scene_matches_golden(&actual, &golden);

    // Also verify connections parsed correctly.
    let source = read_fixture("scenes/ui_menu.tscn");
    let packed = PackedScene::from_tscn(&source).unwrap();
    assert_eq!(packed.connection_count(), 3);
    assert_eq!(packed.connections()[0].signal_name, "pressed");
    assert_eq!(packed.connections()[0].from_path, "PlayButton");
    assert_eq!(packed.connections()[1].from_path, "SettingsButton");
    assert_eq!(packed.connections()[2].from_path, "QuitButton");
}

/// Checks: Physics playground scene with Ball (position + velocity), Wall,
/// and Floor nodes produces correct hierarchy and properties.
/// Fixture: scenes/physics_playground.tscn
/// Comparison: Full tree structure with Vector2 position/velocity properties.
#[test]
fn golden_scene_physics_playground() {
    let actual = load_and_dump_scene("scenes/physics_playground.tscn");
    let golden = read_golden("golden/scenes/physics_playground.json");
    assert_scene_matches_golden(&actual, &golden);
}

/// Checks: Complex signals scene with 5 child nodes (including nested
/// TriggerZone under Player) and 6 connections including cross-node
/// connections parses correctly.
/// Fixture: scenes/signals_complex.tscn
/// Comparison: Tree structure and all 6 signal connections.
#[test]
fn golden_scene_signals_complex() {
    let actual = load_and_dump_scene("scenes/signals_complex.tscn");
    let golden = read_golden("golden/scenes/signals_complex.json");
    assert_scene_matches_golden(&actual, &golden);

    // Verify all 6 connections parsed.
    let source = read_fixture("scenes/signals_complex.tscn");
    let packed = PackedScene::from_tscn(&source).unwrap();
    assert_eq!(packed.node_count(), 6);
    assert_eq!(packed.connection_count(), 6);

    // Verify cross-node connection from nested child.
    let trigger_conn = packed.connections().iter()
        .find(|c| c.signal_name == "body_entered")
        .expect("should have body_entered connection");
    assert_eq!(trigger_conn.from_path, "Player/TriggerZone");
    assert_eq!(trigger_conn.to_path, "Enemy");
    assert_eq!(trigger_conn.flags, 3);
}

/// Checks: Theme resource with two sub_resource blocks (panel_style,
/// button_style) each with Color and int properties parses correctly.
/// Fixture: resources/theme.tres
/// Comparison: class_name, properties, both sub-resources with their properties.
#[test]
fn golden_resource_theme() {
    let actual = load_and_dump_resource("resources/theme.tres");
    let golden = read_golden("golden/resources/theme.json");
    assert_resource_matches_golden(&actual, &golden);
}

/// Checks: Animation resource with Array properties (int array, float array,
/// Vector2 array) representing keyframe data parses correctly.
/// Fixture: resources/animation.tres
/// Comparison: class_name, all properties including nested Array contents.
#[test]
fn golden_resource_animation() {
    let actual = load_and_dump_resource("resources/animation.tres");
    let golden = read_golden("golden/resources/animation.json");
    assert_resource_matches_golden(&actual, &golden);
}

/// Compares actual resource dump against a golden file.
fn assert_resource_matches_golden(actual: &Value, golden: &Value) {
    // class_name
    let a_class = actual.get("class_name").and_then(|v| v.as_str()).unwrap_or("");
    let e_class = golden.get("class_name").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(a_class, e_class, "resource class_name mismatch");

    // properties
    let a_props = actual.get("properties").cloned().unwrap_or(json!({}));
    let e_props = golden.get("properties").cloned().unwrap_or(json!({}));
    assert_properties_eq(&a_props, &e_props, "resource");

    // subresources
    let a_subs = actual.get("subresources").and_then(|v| v.as_object());
    let e_subs = golden.get("subresources").and_then(|v| v.as_object());

    let a_sub_keys: Vec<_> = a_subs.map(|o| {
        let mut keys: Vec<_> = o.keys().collect();
        keys.sort();
        keys
    }).unwrap_or_default();
    let e_sub_keys: Vec<_> = e_subs.map(|o| {
        let mut keys: Vec<_> = o.keys().collect();
        keys.sort();
        keys
    }).unwrap_or_default();
    assert_eq!(a_sub_keys, e_sub_keys, "subresource keys mismatch");

    if let (Some(a_map), Some(e_map)) = (a_subs, e_subs) {
        for key in a_map.keys() {
            let a_sub = &a_map[key];
            let e_sub = &e_map[key];

            let a_cn = a_sub.get("class_name").and_then(|v| v.as_str()).unwrap_or("");
            let e_cn = e_sub.get("class_name").and_then(|v| v.as_str()).unwrap_or("");
            assert_eq!(a_cn, e_cn, "subresource '{key}' class_name mismatch");

            let a_sp = a_sub.get("properties").cloned().unwrap_or(json!({}));
            let e_sp = e_sub.get("properties").cloned().unwrap_or(json!({}));
            assert_properties_eq(&a_sp, &e_sp, &format!("subresource/{key}"));
        }
    }
}
