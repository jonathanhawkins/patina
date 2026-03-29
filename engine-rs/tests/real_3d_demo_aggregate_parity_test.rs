//! pat-u0gqb: First real 3D demo parity report using unified comparison tooling.
//!
//! Loads all 10 representative 3D scene fixtures, compares Patina's scene tree
//! output against Godot oracle tree outputs using `compare3d::compare_scene_trees`,
//! builds per-fixture `FixtureParityReport3D` and an `AggregateParityReport3D`,
//! and writes the aggregate JSON report to `fixtures/patina_outputs/`.

mod oracle_fixture;

use std::sync::Mutex;

use gdcore::compare3d::{
    compare_scene_trees, AggregateParityReport3D, FixtureParityReport3D, SceneTreeEntry,
};
use gdobject::class_db;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use oracle_fixture::{fixtures_dir, load_json_fixture};
use serde_json::Value;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    class_db::clear_for_testing();
    class_db::register_class(class_db::ClassRegistration::new("Object"));
    class_db::register_class(
        class_db::ClassRegistration::new("Node")
            .parent("Object")
            .property(class_db::PropertyInfo::new(
                "name",
                gdvariant::Variant::String(String::new()),
            )),
    );
    class_db::register_3d_classes();
    guard
}

/// All 10 representative 3D fixtures.
const FIXTURES_3D: &[(&str, &str)] = &[
    ("minimal_3d", "minimal_3d.tscn"),
    ("hierarchy_3d", "hierarchy_3d.tscn"),
    ("indoor_3d", "indoor_3d.tscn"),
    ("multi_light_3d", "multi_light_3d.tscn"),
    ("physics_3d_playground", "physics_3d_playground.tscn"),
    ("animated_scene_3d", "animated_scene_3d.tscn"),
    ("foggy_terrain_3d", "foggy_terrain_3d.tscn"),
    ("outdoor_3d", "outdoor_3d.tscn"),
    ("spotlight_gallery_3d", "spotlight_gallery_3d.tscn"),
    ("vehicle_3d", "vehicle_3d.tscn"),
];

fn load_tscn_to_tree(filename: &str) -> SceneTree {
    let path = fixtures_dir().join("scenes").join(filename);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("should read {}: {}", filename, e));
    let scene =
        PackedScene::from_tscn(&source).unwrap_or_else(|e| panic!("parse {}: {:?}", filename, e));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene)
        .unwrap_or_else(|e| panic!("add {} to tree: {:?}", filename, e));
    tree
}

/// Recursively flatten a Godot oracle tree JSON (nested children) into SceneTreeEntry list.
fn flatten_oracle_tree(value: &Value) -> Vec<SceneTreeEntry> {
    let mut entries = Vec::new();
    flatten_oracle_node(value, &mut entries);
    entries
}

fn flatten_oracle_node(node: &Value, entries: &mut Vec<SceneTreeEntry>) {
    if let (Some(path), Some(class)) = (
        node.get("path").and_then(Value::as_str),
        node.get("class").and_then(Value::as_str),
    ) {
        entries.push(SceneTreeEntry::new(path, class));
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            flatten_oracle_node(child, entries);
        }
    }
}

/// Extract all nodes from a Patina SceneTree as SceneTreeEntry list via DFS.
fn extract_patina_tree(tree: &SceneTree) -> Vec<SceneTreeEntry> {
    let mut entries = Vec::new();
    collect_subtree(tree, tree.root_id(), &mut entries);
    entries
}

fn collect_subtree(
    tree: &SceneTree,
    node_id: gdscene::node::NodeId,
    entries: &mut Vec<SceneTreeEntry>,
) {
    if let Some(node) = tree.get_node(node_id) {
        if let Some(path) = tree.node_path(node_id) {
            // Skip the bare "/root" node — oracle trees start from scene root children
            if path != "/root" {
                entries.push(SceneTreeEntry::new(&path, node.class_name()));
            }
        }
        for &child_id in node.children() {
            collect_subtree(tree, child_id, entries);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn all_10_fixtures_load_successfully() {
    let _g = setup();
    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        assert!(
            tree.node_count() > 1,
            "{name}: scene should have nodes beyond root"
        );
    }
}

#[test]
fn all_10_fixtures_have_oracle_tree_outputs() {
    for &(name, _) in FIXTURES_3D {
        let path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        assert!(
            path.exists(),
            "{name}: missing oracle tree output at {}",
            path.display()
        );
    }
}

#[test]
fn per_fixture_scene_tree_parity_above_threshold() {
    let _g = setup();
    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_tree(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);

        assert!(
            !oracle_entries.is_empty(),
            "{name}: oracle tree should have entries"
        );
        assert!(
            !patina_entries.is_empty(),
            "{name}: patina tree should have entries"
        );

        let result = compare_scene_trees(&oracle_entries, &patina_entries);
        let ratio = result.match_ratio();
        assert!(
            ratio >= 0.5,
            "{name}: scene tree parity {:.1}% below 50% threshold ({} matched / {} oracle nodes)",
            ratio * 100.0,
            result.matching_nodes,
            oracle_entries.len()
        );
    }
}

#[test]
fn aggregate_parity_report_covers_all_10_fixtures() {
    let _g = setup();
    let mut agg = AggregateParityReport3D::new();

    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_tree(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);
        let scene_result = compare_scene_trees(&oracle_entries, &patina_entries);

        let report = FixtureParityReport3D::new(name).with_scene_tree(scene_result);
        agg.add(report);
    }

    assert_eq!(agg.fixture_count(), 10);

    let text = agg.render_text();
    for &(name, _) in FIXTURES_3D {
        assert!(
            text.contains(name),
            "aggregate text report should mention {name}"
        );
    }
    assert!(text.contains("Total fixtures: 10"));

    let json = agg.render_json();
    assert!(json.contains("\"fixture_count\": 10"));
}

#[test]
fn aggregate_report_json_is_well_formed() {
    let _g = setup();
    let mut agg = AggregateParityReport3D::new();

    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_tree(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);
        let scene_result = compare_scene_trees(&oracle_entries, &patina_entries);

        agg.add(FixtureParityReport3D::new(name).with_scene_tree(scene_result));
    }

    let json = agg.render_json();
    let trimmed = json.trim();
    assert!(trimmed.starts_with('{'), "JSON should start with {{");
    assert!(trimmed.ends_with('}'), "JSON should end with }}");
    assert!(json.contains("\"fixtures\":"));
    assert!(json.contains("\"all_pass\":"));
}

#[test]
fn per_fixture_reports_have_correct_verdicts() {
    let _g = setup();

    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_tree(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);
        let scene_result = compare_scene_trees(&oracle_entries, &patina_entries);

        let report = FixtureParityReport3D::new(name).with_scene_tree(scene_result);

        // Verdict should not be Skipped since we provided scene_tree data
        assert_ne!(
            report.scene_tree_verdict(),
            gdcore::compare3d::DimensionVerdict::Skipped,
            "{name}: scene tree verdict should not be Skipped"
        );

        let text = report.render_text();
        assert!(text.contains(name));
        assert!(text.contains("Scene Tree Parity"));
    }
}

#[test]
fn write_aggregate_report_artifact() {
    let _g = setup();
    let mut agg = AggregateParityReport3D::new();

    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_tree(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);
        let scene_result = compare_scene_trees(&oracle_entries, &patina_entries);

        agg.add(FixtureParityReport3D::new(name).with_scene_tree(scene_result));
    }

    // Verify the aggregate covers all 10 fixtures
    assert_eq!(agg.fixture_count(), 10);
    let agg_json_str = agg.render_json();
    let parsed: serde_json::Value =
        serde_json::from_str(&agg_json_str).expect("aggregate report JSON should be valid");
    assert_eq!(parsed["fixture_count"], 10);

    // The enriched report artifact (with metadata, scene inventory, physics goldens,
    // and evidence) is maintained as a committed fixture — it is NOT overwritten here.
    // The artifact test (`real_3d_demo_parity_report_artifact_test`) validates the
    // committed report's structural integrity against the golden corpus.
    let output_path = fixtures_dir()
        .join("patina_outputs")
        .join("real_3d_demo_parity_report.json");
    assert!(
        output_path.exists(),
        "enriched parity report artifact should exist at {}",
        output_path.display()
    );

    // Verify the committed artifact is valid JSON with required metadata
    let committed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output_path).unwrap())
            .expect("committed report should be valid JSON");
    assert_eq!(committed["report_id"], "real_3d_demo_parity_report");
    assert_eq!(committed["phase"], "Phase 6: 3D Runtime Slice");
    assert!(
        committed["scene_fixtures"].as_array().is_some(),
        "committed report should have scene_fixtures array"
    );
}
