//! pat-cdh: Refresh Patina-vs-Godot 4.6.1 oracle parity metrics.
//!
//! Enumerates all oracle outputs from fixtures/oracle_outputs/ and compares
//! Patina's PackedScene parser output against the Godot 4.6.1 oracle for each
//! scene that has a matching .tscn fixture file. Reports aggregate parity
//! metrics across all available scenes.
//!
//! Oracle: Godot 4.6.1-stable fixture outputs.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

fn oracle_outputs_dir() -> PathBuf {
    fixtures_dir().join("oracle_outputs")
}

fn scenes_dir() -> PathBuf {
    fixtures_dir().join("scenes")
}

/// Flatten an oracle tree JSON into (path -> class) pairs.
fn flatten_oracle_tree(node: &Value, out: &mut Vec<(String, String)>) {
    let path = node
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let class = node
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !path.is_empty() {
        out.push((path, class));
    }
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            flatten_oracle_tree(child, out);
        }
    }
}

/// Parse a .tscn file through Patina and extract (path -> class) pairs.
fn patina_parse_scene(tscn_path: &Path) -> Vec<(String, String)> {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
    use gdscene::scene_tree::SceneTree;

    let tscn = match std::fs::read_to_string(tscn_path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let scene = match PackedScene::from_tscn(&tscn) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    if add_packed_scene_to_tree(&mut tree, root, &scene).is_err() {
        return Vec::new();
    }

    // Collect all nodes
    let all_nodes = tree.all_nodes_in_tree_order();
    let mut result = Vec::new();
    for id in &all_nodes {
        if let (Some(path), Some(node)) = (tree.node_path(*id), tree.get_node(*id)) {
            result.push((path, node.class_name().to_string()));
        }
    }
    result
}

/// Compare oracle tree nodes against Patina-parsed nodes.
/// Returns (matched_count, total_oracle_count).
fn compare_node_structure(
    oracle_nodes: &[(String, String)],
    patina_nodes: &[(String, String)],
) -> (usize, usize) {
    let _patina_map: HashMap<&str, &str> = patina_nodes
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();

    let mut matched = 0;
    let mut total = 0;

    for (oracle_path, oracle_class) in oracle_nodes {
        // Skip the Window root node — Patina doesn't create it
        if oracle_class == "Window" {
            continue;
        }
        total += 1;

        // Try matching by path suffix (Patina's root name may differ)
        let oracle_suffix = oracle_path
            .strip_prefix("/root/")
            .unwrap_or(oracle_path.as_str());

        let found = patina_nodes.iter().any(|(p_path, p_class)| {
            let p_suffix = p_path.strip_prefix("/root/").unwrap_or(p_path.as_str());
            p_suffix == oracle_suffix && p_class == oracle_class
        });

        if found {
            matched += 1;
        }
    }

    (matched, total)
}

// ===========================================================================
// 1. Enumerate all oracle tree outputs
// ===========================================================================

#[test]
fn oracle_tree_outputs_enumerated() {
    let dir = oracle_outputs_dir();
    let tree_files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read oracle_outputs dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with("_tree.json"))
        .collect();

    eprintln!("Oracle tree outputs: {}", tree_files.len());
    assert!(
        tree_files.len() >= 20,
        "Expected at least 20 oracle tree outputs, found {}",
        tree_files.len()
    );
}

// ===========================================================================
// 2. Scenes with .tscn files produce parseable output
// ===========================================================================

#[test]
fn scenes_with_tscn_are_parseable() {
    let scenes = scenes_dir();
    let mut parseable = 0;
    let mut total = 0;

    for entry in std::fs::read_dir(&scenes).expect("read scenes dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".tscn") {
            continue;
        }
        total += 1;
        let nodes = patina_parse_scene(&entry.path());
        if !nodes.is_empty() {
            parseable += 1;
        }
    }

    eprintln!("Parseable scenes: {}/{}", parseable, total);
    assert!(
        parseable >= 10,
        "At least 10 scenes must be parseable, got {}/{}",
        parseable,
        total
    );
}

// ===========================================================================
// 3. Per-scene node structure parity
// ===========================================================================

#[test]
fn per_scene_node_structure_parity() {
    let oracle_dir = oracle_outputs_dir();
    let scenes = scenes_dir();

    // Find scenes that have both a .tscn and oracle _tree.json
    let mut scene_names = Vec::new();
    for entry in std::fs::read_dir(&scenes).expect("read scenes dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".tscn") {
            continue;
        }
        let base = name.trim_end_matches(".tscn");
        let oracle_tree = oracle_dir.join(format!("{}_tree.json", base));
        if oracle_tree.exists() {
            scene_names.push(base.to_string());
        }
    }

    scene_names.sort();

    let mut total_matched = 0usize;
    let mut total_nodes = 0usize;
    let mut per_scene: Vec<(String, usize, usize, f64)> = Vec::new();

    for scene_name in &scene_names {
        let oracle_tree_path = oracle_dir.join(format!("{}_tree.json", scene_name));
        let tscn_path = scenes.join(format!("{}.tscn", scene_name));

        let oracle_json: Value = serde_json::from_str(
            &std::fs::read_to_string(&oracle_tree_path).expect("read oracle tree"),
        )
        .expect("parse oracle tree");

        let mut oracle_nodes = Vec::new();
        flatten_oracle_tree(&oracle_json, &mut oracle_nodes);

        let patina_nodes = patina_parse_scene(&tscn_path);

        let (matched, total) = compare_node_structure(&oracle_nodes, &patina_nodes);
        let pct = if total > 0 {
            (matched as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        per_scene.push((scene_name.clone(), matched, total, pct));
        total_matched += matched;
        total_nodes += total;
    }

    let overall_pct = if total_nodes > 0 {
        (total_matched as f64 / total_nodes as f64) * 100.0
    } else {
        0.0
    };

    eprintln!("\n=== Patina-vs-Godot 4.6.1 Oracle Parity Metrics ===");
    eprintln!("  Scenes compared: {}", scene_names.len());
    for (name, matched, total, pct) in &per_scene {
        let status = if *pct >= 100.0 {
            "FULL"
        } else if *pct >= 80.0 {
            "HIGH"
        } else if *pct >= 50.0 {
            "PART"
        } else {
            "LOW "
        };
        eprintln!("  [{status}] {name}: {matched}/{total} nodes ({pct:.1}%)");
    }
    eprintln!(
        "  Overall: {}/{} ({:.1}%)",
        total_matched, total_nodes, overall_pct
    );
    eprintln!("  Oracle: Godot 4.6.1-stable");
    eprintln!("====================================================\n");

    // Aggregate parity must be at least 70%
    assert!(
        overall_pct >= 70.0,
        "Overall node structure parity must be >= 70%, got {:.1}%",
        overall_pct
    );
}

// ===========================================================================
// 4. Oracle property outputs exist for key scenes
// ===========================================================================

#[test]
fn oracle_property_outputs_exist_for_key_scenes() {
    let oracle_dir = oracle_outputs_dir();
    let key_scenes = [
        "minimal",
        "hierarchy",
        "platformer",
        "with_properties",
        "physics_playground",
        "space_shooter",
    ];

    let mut missing = Vec::new();
    for scene in &key_scenes {
        let props = oracle_dir.join(format!("{}_properties.json", scene));
        if !props.exists() {
            missing.push(*scene);
        }
    }

    assert!(
        missing.is_empty(),
        "Missing oracle property outputs for key scenes: {:?}",
        missing
    );
}

// ===========================================================================
// 5. Oracle JSON files are well-formed
// ===========================================================================

#[test]
fn oracle_json_files_well_formed() {
    let oracle_dir = oracle_outputs_dir();
    let mut invalid = Vec::new();
    let mut total = 0;

    for entry in std::fs::read_dir(&oracle_dir).expect("read oracle dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        total += 1;
        let content = std::fs::read_to_string(entry.path()).expect("read file");
        if serde_json::from_str::<Value>(&content).is_err() {
            invalid.push(name);
        }
    }

    eprintln!("Oracle JSON files validated: {}", total);
    assert!(
        invalid.is_empty(),
        "Invalid JSON oracle files: {:?}",
        invalid
    );
}

// ===========================================================================
// 6. 3D scene parity metrics
// ===========================================================================

#[test]
fn scene_3d_parity_metrics() {
    let oracle_dir = oracle_outputs_dir();
    let scenes = scenes_dir();

    let scenes_3d = ["minimal_3d", "hierarchy_3d", "indoor_3d", "multi_light_3d"];
    let mut total_matched = 0usize;
    let mut total_nodes = 0usize;

    for scene_name in &scenes_3d {
        let oracle_tree_path = oracle_dir.join(format!("{}_tree.json", scene_name));
        let tscn_path = scenes.join(format!("{}.tscn", scene_name));

        if !oracle_tree_path.exists() || !tscn_path.exists() {
            continue;
        }

        let oracle_json: Value = serde_json::from_str(
            &std::fs::read_to_string(&oracle_tree_path).expect("read oracle tree"),
        )
        .expect("parse oracle tree");

        let mut oracle_nodes = Vec::new();
        flatten_oracle_tree(&oracle_json, &mut oracle_nodes);

        let patina_nodes = patina_parse_scene(&tscn_path);
        let (matched, total) = compare_node_structure(&oracle_nodes, &patina_nodes);

        total_matched += matched;
        total_nodes += total;

        let pct = if total > 0 {
            (matched as f64 / total as f64) * 100.0
        } else {
            100.0
        };
        eprintln!(
            "  3D scene {}: {}/{} ({:.1}%)",
            scene_name, matched, total, pct
        );
    }

    let pct = if total_nodes > 0 {
        (total_matched as f64 / total_nodes as f64) * 100.0
    } else {
        0.0
    };
    eprintln!(
        "  3D overall: {}/{} ({:.1}%)",
        total_matched, total_nodes, pct
    );

    // 3D parity is expected to be lower since 3D is deferred
    assert!(
        total_nodes > 0,
        "Must have at least some 3D oracle nodes to compare"
    );
}

// ===========================================================================
// 7. Class name coverage across all oracle scenes
// ===========================================================================

#[test]
fn class_name_coverage_across_oracle_scenes() {
    let oracle_dir = oracle_outputs_dir();

    let mut all_classes: HashMap<String, usize> = HashMap::new();

    for entry in std::fs::read_dir(&oracle_dir).expect("read oracle dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with("_tree.json") {
            continue;
        }

        let content = std::fs::read_to_string(entry.path()).expect("read tree");
        let json: Value = serde_json::from_str(&content).expect("parse tree");

        let mut nodes = Vec::new();
        flatten_oracle_tree(&json, &mut nodes);

        for (_, class) in &nodes {
            *all_classes.entry(class.clone()).or_insert(0) += 1;
        }
    }

    let mut classes: Vec<_> = all_classes.into_iter().collect();
    classes.sort_by(|a, b| b.1.cmp(&a.1));

    eprintln!("\n=== Class Name Coverage in Oracle Fixtures ===");
    for (class, count) in classes.iter().take(20) {
        eprintln!("  {}: {} occurrences", class, count);
    }
    eprintln!("  Total unique classes: {}", classes.len());
    eprintln!("================================================\n");

    assert!(
        classes.len() >= 15,
        "Oracle fixtures must cover at least 15 unique class types, found {}",
        classes.len()
    );
}

// ===========================================================================
// 8. Scene coverage: fixture scenes vs oracle outputs
// ===========================================================================

#[test]
fn scene_fixture_vs_oracle_coverage() {
    let scenes = scenes_dir();
    let oracle_dir = oracle_outputs_dir();

    let mut tscn_count = 0;
    let mut with_oracle = 0;

    for entry in std::fs::read_dir(&scenes).expect("read scenes dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".tscn") {
            continue;
        }
        tscn_count += 1;
        let base = name.trim_end_matches(".tscn");
        if oracle_dir.join(format!("{}_tree.json", base)).exists() {
            with_oracle += 1;
        }
    }

    let coverage = if tscn_count > 0 {
        (with_oracle as f64 / tscn_count as f64) * 100.0
    } else {
        0.0
    };

    eprintln!(
        "Scene oracle coverage: {}/{} ({:.1}%)",
        with_oracle, tscn_count, coverage
    );

    assert!(
        coverage >= 80.0,
        "At least 80% of .tscn fixtures must have oracle outputs, got {:.1}%",
        coverage
    );
}
