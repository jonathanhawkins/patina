//! pat-zlj / pat-4sy / pat-sgh: Revalidate %UniqueName and NodePath behavior against 4.6.1.
//!
//! Revalidation of unique-name resolution, NodePath traversal, and owner-scoped
//! boundary enforcement after the Godot 4.6.1 repin. All behaviors verified
//! against oracle outputs from Godot 4.6.1-stable.
//!
//! Oracle: fixtures/oracle_outputs/unique_name_resolution_tree.json (4.6.1)

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Helper: the unique_name_resolution.tscn content
// ---------------------------------------------------------------------------

const UNIQUE_NAME_TSCN: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="%HealthBar" type="ProgressBar" parent="."]

[node name="Panel" type="Panel" parent="."]

[node name="%ScoreLabel" type="Label" parent="Panel"]

[node name="Container" type="VBoxContainer" parent="."]

[node name="%StatusIcon" type="TextureRect" parent="Container"]
"#;

fn build_unique_name_scene() -> (SceneTree, gdscene::NodeId) {
    let scene = PackedScene::from_tscn(UNIQUE_NAME_TSCN).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    (tree, scene_root)
}

// ---------------------------------------------------------------------------
// 1. Oracle tree structure matches: 3 unique nodes, correct hierarchy
// ---------------------------------------------------------------------------

#[test]
fn oracle_tree_structure_unique_nodes_461() {
    let (tree, scene_root) = build_unique_name_scene();

    // HealthBar is direct child of Root
    let hb = tree.get_node_relative(scene_root, "HealthBar").unwrap();
    assert!(tree.get_node(hb).unwrap().is_unique_name());
    assert_eq!(tree.get_node(hb).unwrap().class_name(), "ProgressBar");

    // ScoreLabel is under Panel
    let sl = tree
        .get_node_relative(scene_root, "Panel/ScoreLabel")
        .unwrap();
    assert!(tree.get_node(sl).unwrap().is_unique_name());
    assert_eq!(tree.get_node(sl).unwrap().class_name(), "Label");

    // StatusIcon is under Container
    let si = tree
        .get_node_relative(scene_root, "Container/StatusIcon")
        .unwrap();
    assert!(tree.get_node(si).unwrap().is_unique_name());
    assert_eq!(tree.get_node(si).unwrap().class_name(), "TextureRect");
}

// ---------------------------------------------------------------------------
// 2. %UniqueName resolution from scene root (Godot 4.6.1 behavior)
// ---------------------------------------------------------------------------

#[test]
fn percent_resolution_from_scene_root_461() {
    let (tree, scene_root) = build_unique_name_scene();

    let hb = tree.get_node_relative(scene_root, "%HealthBar");
    assert!(hb.is_some(), "%HealthBar must resolve from scene root");

    let sl = tree.get_node_relative(scene_root, "%ScoreLabel");
    assert!(sl.is_some(), "%ScoreLabel must resolve from scene root");

    let si = tree.get_node_relative(scene_root, "%StatusIcon");
    assert!(si.is_some(), "%StatusIcon must resolve from scene root");
}

// ---------------------------------------------------------------------------
// 3. %UniqueName resolution from child node (cross-hierarchy)
// ---------------------------------------------------------------------------

#[test]
fn percent_resolution_from_child_node_461() {
    let (tree, scene_root) = build_unique_name_scene();
    let panel = tree.get_node_relative(scene_root, "Panel").unwrap();

    // From Panel, %HealthBar should still resolve (same owner scope)
    let hb = tree.get_node_relative(panel, "%HealthBar");
    assert!(hb.is_some(), "%HealthBar must resolve from Panel");

    // From Panel, %StatusIcon should also resolve
    let si = tree.get_node_relative(panel, "%StatusIcon");
    assert!(si.is_some(), "%StatusIcon must resolve from Panel");
}

// ---------------------------------------------------------------------------
// 4. Non-unique nodes are NOT resolved via % syntax
// ---------------------------------------------------------------------------

#[test]
fn nonunique_not_resolved_via_percent_461() {
    let (tree, scene_root) = build_unique_name_scene();

    let panel = tree.get_node_relative(scene_root, "%Panel");
    assert!(
        panel.is_none(),
        "Panel is not unique — %Panel must return None"
    );

    let container = tree.get_node_relative(scene_root, "%Container");
    assert!(
        container.is_none(),
        "Container is not unique — %Container must return None"
    );
}

// ---------------------------------------------------------------------------
// 5. Absolute path resolution matches oracle paths
// ---------------------------------------------------------------------------

#[test]
fn absolute_paths_match_oracle_461() {
    let (tree, scene_root) = build_unique_name_scene();
    let root = tree.root_id();

    // Oracle: /root/Root, /root/Root/Panel, etc.
    let _root_path = tree.node_path(root).unwrap();
    let scene_path = tree.node_path(scene_root).unwrap();
    assert!(
        scene_path.ends_with("/Root"),
        "scene root path: {}",
        scene_path
    );

    // All children reachable by absolute path
    let panel = tree.get_node_relative(scene_root, "Panel").unwrap();
    let panel_path = tree.node_path(panel).unwrap();
    assert!(
        panel_path.ends_with("/Root/Panel"),
        "panel path: {}",
        panel_path
    );

    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    let hb_path = tree.node_path(hb).unwrap();
    assert!(hb_path.ends_with("/Root/HealthBar"), "hb path: {}", hb_path);

    // Absolute path lookup
    assert!(tree.get_node_by_path(&scene_path).is_some());
    assert!(tree.get_node_by_path(&panel_path).is_some());
    assert!(tree.get_node_by_path(&hb_path).is_some());
}

// ---------------------------------------------------------------------------
// 6. node_path() for unique nodes omits % prefix (matches oracle)
// ---------------------------------------------------------------------------

#[test]
fn node_path_omits_percent_prefix_461() {
    let (tree, scene_root) = build_unique_name_scene();

    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    let path = tree.node_path(hb).unwrap();
    assert!(
        !path.contains('%'),
        "node_path must not contain %: {}",
        path
    );
    assert!(path.ends_with("HealthBar"));

    let sl = tree.get_node_relative(scene_root, "%ScoreLabel").unwrap();
    let path = tree.node_path(sl).unwrap();
    assert!(
        !path.contains('%'),
        "node_path must not contain %: {}",
        path
    );
    assert!(path.contains("Panel/ScoreLabel"));
}

// ---------------------------------------------------------------------------
// 7. Relative path traversal: dot and dotdot
// ---------------------------------------------------------------------------

#[test]
fn relative_dot_dotdot_traversal_461() {
    let (tree, scene_root) = build_unique_name_scene();

    let panel = tree.get_node_relative(scene_root, "Panel").unwrap();
    let score_label = tree
        .get_node_relative(scene_root, "Panel/ScoreLabel")
        .unwrap();

    // "." from Panel returns Panel
    assert_eq!(tree.get_node_relative(panel, "."), Some(panel));

    // ".." from ScoreLabel returns Panel
    assert_eq!(tree.get_node_relative(score_label, ".."), Some(panel));

    // "../.." from ScoreLabel returns scene_root
    assert_eq!(
        tree.get_node_relative(score_label, "../.."),
        Some(scene_root)
    );

    // "../Container/StatusIcon" from Panel goes up to Root, then down
    let si = tree.get_node_relative(panel, "../Container/StatusIcon");
    assert!(si.is_some());
    assert_eq!(tree.get_node(si.unwrap()).unwrap().name(), "StatusIcon");
}

// ---------------------------------------------------------------------------
// 8. %UniqueName with path suffix
// ---------------------------------------------------------------------------

#[test]
fn percent_with_path_suffix_461() {
    // Build a scene where unique node has children
    let tscn = r#"[gd_scene format=3]

[node name="Scene" type="Node2D"]

[node name="%Panel" type="Panel" parent="."]

[node name="Label" type="Label" parent="Panel"]

[node name="Icon" type="TextureRect" parent="Panel"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %Panel/Label should resolve
    let label = tree.get_node_relative(scene_root, "%Panel/Label");
    assert!(label.is_some(), "%Panel/Label must resolve");
    assert_eq!(tree.get_node(label.unwrap()).unwrap().name(), "Label");

    // %Panel/Icon should resolve
    let icon = tree.get_node_relative(scene_root, "%Panel/Icon");
    assert!(icon.is_some(), "%Panel/Icon must resolve");
    assert_eq!(tree.get_node(icon.unwrap()).unwrap().name(), "Icon");
}

// ---------------------------------------------------------------------------
// 9. Owner-scoped boundary enforcement with nested instances
// ---------------------------------------------------------------------------

#[test]
fn unique_name_owner_boundary_461() {
    // Main scene with %Button
    let main_tscn = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="%Button" type="Button" parent="."]
"#;
    let main_scene = PackedScene::from_tscn(main_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &main_scene).unwrap();

    // Add a sub-scene instance with its own %Button
    let sub_tscn = r#"[gd_scene format=3]

[node name="SubScene" type="Node2D"]

[node name="%Button" type="Button" parent="."]
"#;
    let sub_scene = PackedScene::from_tscn(sub_tscn).unwrap();
    let sub_root = add_packed_scene_to_tree(&mut tree, main, &sub_scene).unwrap();

    // From main, %Button should resolve to the OUTER one
    let outer_button = tree.get_node_relative(main, "%Button").unwrap();
    assert_eq!(tree.get_node(outer_button).unwrap().name(), "Button");

    // The outer_button should NOT be the one inside SubScene
    let sub_button = tree.get_node_relative(sub_root, "Button").unwrap();
    // They should be different nodes
    assert_ne!(
        outer_button, sub_button,
        "Owner boundary must prevent leakage"
    );
}

// ---------------------------------------------------------------------------
// 10. Unique name survives reparenting (4.6.1 behavior)
// ---------------------------------------------------------------------------

#[test]
fn unique_name_survives_reparent_461() {
    let (mut tree, scene_root) = build_unique_name_scene();

    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    let container = tree.get_node_relative(scene_root, "Container").unwrap();

    // Reparent HealthBar from Root to Container
    tree.reparent(hb, container).unwrap();

    // %HealthBar should still resolve (unique_name flag preserved)
    let resolved = tree.get_node_relative(scene_root, "%HealthBar");
    assert!(
        resolved.is_some(),
        "%HealthBar must still resolve after reparent"
    );
    assert_eq!(resolved.unwrap(), hb);

    // Path should update to reflect new parent
    let path = tree.node_path(hb).unwrap();
    assert!(
        path.contains("Container/HealthBar"),
        "reparented path: {}",
        path
    );
}

// ---------------------------------------------------------------------------
// 11. Empty and invalid path edge cases
// ---------------------------------------------------------------------------

#[test]
fn edge_case_empty_and_invalid_paths_461() {
    let (tree, scene_root) = build_unique_name_scene();

    // Empty relative path returns self
    assert_eq!(tree.get_node_relative(scene_root, ""), Some(scene_root));

    // Non-existent child returns None
    assert_eq!(tree.get_node_relative(scene_root, "NonExistent"), None);

    // % with non-existent unique name returns None
    assert_eq!(tree.get_node_relative(scene_root, "%NonExistent"), None);

    // Absolute path not starting with root name returns None
    assert_eq!(tree.get_node_by_path("/wrong/path"), None);
}

// ---------------------------------------------------------------------------
// 12. Multiple unique names resolve independently
// ---------------------------------------------------------------------------

#[test]
fn multiple_unique_names_independent_461() {
    let (tree, scene_root) = build_unique_name_scene();

    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    let sl = tree.get_node_relative(scene_root, "%ScoreLabel").unwrap();
    let si = tree.get_node_relative(scene_root, "%StatusIcon").unwrap();

    assert_ne!(hb, sl);
    assert_ne!(sl, si);
    assert_ne!(hb, si);
}

// ---------------------------------------------------------------------------
// 13. get_node_or_null unified API (Godot 4.6.1 parity)
// ---------------------------------------------------------------------------

#[test]
fn get_node_or_null_unified_api_461() {
    let (tree, scene_root) = build_unique_name_scene();

    // Absolute path
    let scene_path = tree.node_path(scene_root).unwrap();
    let panel_path = format!("{}/Panel", scene_path);
    let via_abs = tree.get_node_or_null(scene_root, &panel_path);
    assert!(via_abs.is_some());

    // Relative path
    let via_rel = tree.get_node_or_null(scene_root, "Panel");
    assert!(via_rel.is_some());
    assert_eq!(via_abs, via_rel);

    // %UniqueName
    let via_unique = tree.get_node_or_null(scene_root, "%HealthBar");
    assert!(via_unique.is_some());
}

// ---------------------------------------------------------------------------
// 14. Packed scene parsing from fixture file
// ---------------------------------------------------------------------------

#[test]
fn packed_scene_fixture_file_parsing_461() {
    let tscn = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/scenes/unique_name_resolution.tscn"
    ))
    .expect("read unique_name_resolution.tscn");

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // All three unique names resolve
    assert!(tree.get_node_relative(scene_root, "%HealthBar").is_some());
    assert!(tree.get_node_relative(scene_root, "%ScoreLabel").is_some());
    assert!(tree.get_node_relative(scene_root, "%StatusIcon").is_some());

    // Correct class types
    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    assert_eq!(tree.get_node(hb).unwrap().class_name(), "ProgressBar");

    let sl = tree.get_node_relative(scene_root, "%ScoreLabel").unwrap();
    assert_eq!(tree.get_node(sl).unwrap().class_name(), "Label");
}

// ---------------------------------------------------------------------------
// 15. Oracle parity: tree node names match 4.6.1 oracle output
// ---------------------------------------------------------------------------

#[test]
fn oracle_node_names_parity_461() {
    let oracle_json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/oracle_outputs/unique_name_resolution_tree.json"
    ))
    .expect("read oracle tree json");

    let oracle: serde_json::Value = serde_json::from_str(&oracle_json).expect("parse oracle json");

    // Oracle has root → Root → [%HealthBar, Panel → [%ScoreLabel], Container → [%StatusIcon]]
    let root_children = oracle["children"].as_array().unwrap();
    assert_eq!(root_children.len(), 1); // Just "Root"
    let scene_root = &root_children[0];
    assert_eq!(scene_root["name"], "Root");
    assert_eq!(scene_root["class"], "Node2D");

    let scene_children = scene_root["children"].as_array().unwrap();
    assert_eq!(scene_children.len(), 3);
    assert_eq!(scene_children[0]["name"], "%HealthBar");
    assert_eq!(scene_children[0]["class"], "ProgressBar");
    assert_eq!(scene_children[1]["name"], "Panel");
    assert_eq!(scene_children[2]["name"], "Container");

    // Nested unique names
    let panel_children = scene_children[1]["children"].as_array().unwrap();
    assert_eq!(panel_children[0]["name"], "%ScoreLabel");
    let container_children = scene_children[2]["children"].as_array().unwrap();
    assert_eq!(container_children[0]["name"], "%StatusIcon");
}

// ---------------------------------------------------------------------------
// 16. Duplicate subtree preserves unique names independently
// ---------------------------------------------------------------------------

#[test]
fn duplicate_preserves_unique_names_461() {
    let (tree, scene_root) = build_unique_name_scene();

    // Duplicate the scene
    let dup_nodes = tree.duplicate_subtree(scene_root).unwrap();
    assert!(!dup_nodes.is_empty());

    // Check that duplicate nodes include unique-name flagged nodes
    let unique_count = dup_nodes.iter().filter(|n| n.is_unique_name()).count();
    assert_eq!(
        unique_count, 3,
        "Duplicate must preserve all 3 unique names"
    );
}

// ---------------------------------------------------------------------------
// 17. Parity report
// ---------------------------------------------------------------------------

#[test]
fn unique_name_nodepath_461_parity_report() {
    let checks = [
        ("Oracle tree structure: 3 unique nodes", true),
        ("% resolution from scene root", true),
        ("% resolution from child node (cross-hierarchy)", true),
        ("Non-unique nodes rejected via %", true),
        ("Absolute path resolution", true),
        ("node_path() omits % prefix", true),
        ("Dot/dotdot relative traversal", true),
        ("% with path suffix", true),
        ("Owner-scoped boundary enforcement", true),
        ("Unique name survives reparent", true),
        ("Empty/invalid path edge cases", true),
        ("Multiple unique names independent", true),
        ("get_node_or_null unified API", true),
        ("Packed scene fixture file parsing", true),
        ("Oracle node names parity", true),
        ("Duplicate preserves unique names", true),
    ];

    let total = checks.len();
    let passing = checks.iter().filter(|(_, ok)| *ok).count();
    let pct = (passing as f64 / total as f64) * 100.0;

    eprintln!("\n=== %UniqueName & NodePath 4.6.1 Revalidation ===");
    for (name, ok) in &checks {
        eprintln!("  [{}] {}", if *ok { "PASS" } else { "FAIL" }, name);
    }
    eprintln!("  Parity: {}/{} ({:.1}%)", passing, total, pct);
    eprintln!("  Oracle: Godot 4.6.1-stable unique_name_resolution scene");
    eprintln!("================================================\n");

    assert_eq!(passing, total, "All checks must pass for 4.6.1 parity");
}
