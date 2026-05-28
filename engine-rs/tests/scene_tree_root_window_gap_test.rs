//! pat-z20uw: Isolate the persistent 1-node scene-tree gap.
//!
//! The consistent 1-node gap observed in the 3D parity report
//! (`docs/3D_DEMO_PARITY_REPORT.md`) comes from Godot's root node being a
//! `Window` (class `Window`, path `/root`), while Patina's `SceneTree::new()`
//! creates a bare `Node` as root.
//!
//! Every Godot oracle tree includes `/root` as class `Window` with groups
//! `["_picking_viewports", "_viewports"]`. Patina's `extract_patina_tree`
//! skips `/root`, and even if it didn't, the class would be `Node` not
//! `Window`.
//!
//! This test suite:
//! 1. Confirms the root node class discrepancy (`Node` vs `Window`)
//! 2. Demonstrates the 1-node gap across all 5 core 3D fixtures
//! 3. Proves the gap is exactly 1 node per fixture (not a variable count)
//! 4. Verifies that all non-root nodes match, isolating the cause

mod oracle_fixture;

use std::collections::HashMap;
use std::sync::Mutex;

use gdcore::compare3d::{compare_scene_trees, SceneTreeEntry};
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

/// The 5 core 3D fixtures from the parity report.
const CORE_FIXTURES: &[(&str, &str)] = &[
    ("minimal_3d", "minimal_3d.tscn"),
    ("hierarchy_3d", "hierarchy_3d.tscn"),
    ("indoor_3d", "indoor_3d.tscn"),
    ("multi_light_3d", "multi_light_3d.tscn"),
    ("physics_3d_playground", "physics_3d_playground.tscn"),
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

/// Flatten the oracle tree JSON into SceneTreeEntry list (includes /root).
fn flatten_oracle_all(value: &Value) -> Vec<SceneTreeEntry> {
    let mut entries = Vec::new();
    flatten_node(value, &mut entries);
    entries
}

fn flatten_node(node: &Value, entries: &mut Vec<SceneTreeEntry>) {
    if let (Some(path), Some(class)) = (
        node.get("path").and_then(Value::as_str),
        node.get("class").and_then(Value::as_str),
    ) {
        entries.push(SceneTreeEntry::new(path, class));
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            flatten_node(child, entries);
        }
    }
}

/// Flatten the oracle tree JSON, excluding /root.
fn flatten_oracle_without_root(value: &Value) -> Vec<SceneTreeEntry> {
    let all = flatten_oracle_all(value);
    all.into_iter().filter(|e| e.path != "/root").collect()
}

/// Extract Patina tree entries, excluding /root.
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
            if path != "/root" {
                entries.push(SceneTreeEntry::new(&path, node.class_name()));
            }
        }
        for &child_id in node.children() {
            collect_subtree(tree, child_id, entries);
        }
    }
}

// ===========================================================================
// 1. Root node class discrepancy
// ===========================================================================

#[test]
fn patina_root_is_node_not_window() {
    let _g = setup();
    let tree = SceneTree::new();
    let root = tree.get_node(tree.root_id()).expect("root must exist");
    assert_eq!(
        root.class_name(),
        "Node",
        "Patina root is class 'Node', not 'Window'"
    );
    assert_eq!(root.name(), "root");
}

#[test]
fn godot_oracle_root_is_window() {
    let oracle_path = fixtures_dir()
        .join("oracle_outputs")
        .join("minimal_3d_tree.json");
    let oracle_json = load_json_fixture(&oracle_path);

    let root_class = oracle_json["class"].as_str().unwrap();
    let root_path = oracle_json["path"].as_str().unwrap();
    let root_name = oracle_json["name"].as_str().unwrap();

    assert_eq!(root_class, "Window", "Godot root is class 'Window'");
    assert_eq!(root_path, "/root");
    assert_eq!(root_name, "root");
}

// ===========================================================================
// 2. The gap is exactly 1 node per fixture (the /root Window)
// ===========================================================================

#[test]
fn gap_is_exactly_one_node_per_fixture() {
    let _g = setup();

    for &(name, tscn) in CORE_FIXTURES {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_all = flatten_oracle_all(&oracle_json);
        let patina = extract_patina_tree(&tree);

        let oracle_without_root: Vec<_> =
            oracle_all.iter().filter(|e| e.path != "/root").collect();

        // Oracle has exactly 1 more entry than Patina (the /root Window)
        assert_eq!(
            oracle_all.len(),
            patina.len() + 1,
            "{name}: oracle should have exactly 1 more entry than Patina \
             (oracle={}, patina={})",
            oracle_all.len(),
            patina.len()
        );

        // The extra entry is always /root
        let root_entries: Vec<_> = oracle_all.iter().filter(|e| e.path == "/root").collect();
        assert_eq!(
            root_entries.len(),
            1,
            "{name}: exactly one /root entry in oracle"
        );
        assert_eq!(
            root_entries[0].class_name, "Window",
            "{name}: /root entry is class Window"
        );

        // Without /root, oracle and Patina have the same count
        assert_eq!(
            oracle_without_root.len(),
            patina.len(),
            "{name}: without /root, oracle and Patina have same node count"
        );
    }
}

// ===========================================================================
// 3. All non-root nodes match perfectly
// ===========================================================================

#[test]
fn all_non_root_nodes_match_perfectly() {
    let _g = setup();

    for &(name, tscn) in CORE_FIXTURES {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_without_root(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);

        let result = compare_scene_trees(&oracle_entries, &patina_entries);

        assert!(
            result.is_exact_match(),
            "{name}: excluding /root, scene trees should match exactly \
             (matched {}/{}, mismatches: {:?})",
            result.matching_nodes,
            oracle_entries.len(),
            result
                .mismatches
                .iter()
                .map(|m| format!("{:?}", m))
                .collect::<Vec<_>>()
        );
    }
}

// ===========================================================================
// 4. Including /root, match ratio is consistent with the report (~85-94%)
// ===========================================================================

#[test]
fn match_ratio_with_root_matches_reported_range() {
    let _g = setup();
    let mut ratios = HashMap::new();

    for &(name, tscn) in CORE_FIXTURES {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_all(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);
        let result = compare_scene_trees(&oracle_entries, &patina_entries);

        let ratio = result.match_ratio();
        ratios.insert(name, ratio);

        // The gap is exactly 1 node (/root Window), so ratio = (N-1)/N
        let expected_ratio = (oracle_entries.len() - 1) as f64 / oracle_entries.len() as f64;
        assert!(
            (ratio - expected_ratio).abs() < 0.01,
            "{name}: ratio {ratio:.3} should be ~{expected_ratio:.3} \
             (oracle_count={}, matched={})",
            oracle_entries.len(),
            result.matching_nodes
        );
    }

    // All ratios should be in the 80-95% range reported in the doc
    for (name, ratio) in &ratios {
        assert!(
            *ratio >= 0.80 && *ratio <= 0.96,
            "{name}: ratio {:.1}% outside expected 80-96% range",
            ratio * 100.0
        );
    }
}

// ===========================================================================
// 5. Summary: the gap is the /root Window node, nothing else
// ===========================================================================

#[test]
fn gap_summary_only_root_window_is_missing() {
    let _g = setup();

    for &(name, tscn) in CORE_FIXTURES {
        let tree = load_tscn_to_tree(tscn);
        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{name}_tree.json"));
        let oracle_json = load_json_fixture(&oracle_path);

        let oracle_entries = flatten_oracle_all(&oracle_json);
        let patina_entries = extract_patina_tree(&tree);
        let result = compare_scene_trees(&oracle_entries, &patina_entries);

        // There should be exactly 1 mismatch: /root Missing
        assert_eq!(
            result.mismatches.len(),
            1,
            "{name}: should have exactly 1 mismatch, got {}",
            result.mismatches.len()
        );

        match &result.mismatches[0] {
            gdcore::compare3d::SceneTreeMismatch::Missing { path, class_name } => {
                assert_eq!(path, "/root", "{name}: missing node should be /root");
                assert_eq!(
                    class_name, "Window",
                    "{name}: missing class should be Window"
                );
            }
            other => panic!("{name}: expected Missing mismatch, got {:?}", other),
        }
    }
}
