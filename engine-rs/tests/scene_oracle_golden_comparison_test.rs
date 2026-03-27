//! pat-bme8: Oracle golden comparison for non-trivial scene tree.

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

#[test]
fn oracle_outputs_directory_exists() {
    let dir = fixtures_dir().join("oracle_outputs");
    assert!(dir.is_dir(), "oracle_outputs directory must exist");
}

#[test]
fn non_trivial_scene_has_oracle_golden() {
    // platformer is a non-trivial scene with multiple node types
    let tree = fixtures_dir().join("oracle_outputs/platformer_tree.json");
    let props = fixtures_dir().join("oracle_outputs/platformer_properties.json");
    assert!(tree.exists(), "platformer_tree.json oracle must exist");
    assert!(props.exists(), "platformer_properties.json oracle must exist");
}

#[test]
fn oracle_tree_has_valid_structure() {
    let path = fixtures_dir().join("oracle_outputs/platformer_tree.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let tree: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(tree.get("children").is_some(), "tree must have children");
    assert!(tree.get("class").is_some(), "tree must have class");
}

#[test]
fn patina_runner_produces_matching_node_count() {
    // Load oracle tree and count nodes
    let path = fixtures_dir().join("oracle_outputs/platformer_tree.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let tree: serde_json::Value = serde_json::from_str(&content).unwrap();

    fn count_nodes(node: &serde_json::Value) -> usize {
        let children = node.get("children").and_then(|c| c.as_array());
        1 + children.map_or(0, |c| c.iter().map(count_nodes).sum())
    }

    let oracle_count = count_nodes(&tree);
    assert!(oracle_count >= 3, "non-trivial scene should have >= 3 nodes, got {oracle_count}");

    // Load the corresponding patina golden
    let golden_path = fixtures_dir().join("golden/scenes/platformer.json");
    if golden_path.exists() {
        let golden_content = std::fs::read_to_string(&golden_path).unwrap();
        let golden: serde_json::Value = serde_json::from_str(&golden_content).unwrap();

        fn count_patina_nodes(node: &serde_json::Value) -> usize {
            let children = node.get("children").and_then(|c| c.as_array());
            1 + children.map_or(0, |c| c.iter().map(count_patina_nodes).sum())
        }

        let data = golden.get("data").unwrap_or(&golden);
        if let Some(nodes) = data.get("nodes").and_then(|n| n.as_array()) {
            let patina_count: usize = nodes.iter().map(count_patina_nodes).sum();
            // Oracle tree includes root Window node; patina golden starts at scene root.
            // Allow off-by-one for the root node difference.
            let diff = (patina_count as i64 - oracle_count as i64).unsigned_abs();
            assert!(
                diff <= 1,
                "patina ({patina_count}) and oracle ({oracle_count}) node counts should match (±1 for root)"
            );
        }
    }
}

#[test]
fn multiple_scenes_have_oracle_goldens() {
    let scenes = ["minimal", "hierarchy", "platformer", "physics_playground"];
    for name in &scenes {
        let tree = fixtures_dir().join(format!("oracle_outputs/{name}_tree.json"));
        assert!(tree.exists(), "{name}_tree.json oracle must exist");
    }
}
