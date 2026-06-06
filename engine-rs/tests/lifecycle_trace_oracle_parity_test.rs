//! pat-mms8, pat-wifn: Compare lifecycle notification traces against oracle output.
//!
//! Parses upstream Godot oracle `_tree.json` files to derive the expected
//! ENTER_TREE (depth-first top-down) and READY (depth-first bottom-up)
//! notification ordering, then loads the corresponding `.tscn` fixture in
//! Patina's engine and asserts the runtime traces match exactly.
//!
//! Acceptance: deterministic trace comparisons that fail clearly on ordering
//! drift between Patina and the Godot oracle.

use std::path::PathBuf;

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;
use serde_json::Value;

/// All fixture IDs that have both a `_tree.json` oracle and a `.tscn` file.
const ALL_FIXTURE_IDS: &[&str] = &[
    "minimal",
    "hierarchy",
    "platformer",
    "ui_menu",
    "unique_name_resolution",
    "character_body_test",
    "signals_complex",
    "space_shooter",
    "test_scripts",
    "with_properties",
    "physics_playground",
    "signal_instantiation",
];

// ===========================================================================
// Oracle tree parser
// ===========================================================================

/// Normalize an oracle path by stripping `%` unique-name prefixes from
/// each segment. Godot's runtime paths do not include the `%` marker.
fn normalize_oracle_path(path: &str) -> String {
    path.split('/')
        .map(|seg| seg.strip_prefix('%').unwrap_or(seg))
        .collect::<Vec<_>>()
        .join("/")
}

/// Recursively collect node paths in depth-first pre-order (top-down).
/// Skips the Oracle autoload node injected by the instrumentation harness.
fn collect_paths_preorder(node: &Value, out: &mut Vec<String>) {
    let name = node["name"].as_str().unwrap_or("");
    // Skip the Oracle autoload node (instrumentation artifact).
    if name == "Oracle" {
        return;
    }
    if let Some(path) = node["path"].as_str() {
        out.push(normalize_oracle_path(path));
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_paths_preorder(child, out);
        }
    }
}

/// Recursively collect node paths in depth-first post-order (bottom-up).
/// Skips the Oracle autoload node.
fn collect_paths_postorder(node: &Value, out: &mut Vec<String>) {
    let name = node["name"].as_str().unwrap_or("");
    if name == "Oracle" {
        return;
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_paths_postorder(child, out);
        }
    }
    if let Some(path) = node["path"].as_str() {
        out.push(normalize_oracle_path(path));
    }
}

/// Extract the scene subtree from an oracle tree JSON.
/// The oracle tree root is the Window node ("/root"); the scene is
/// its first non-Oracle child.
fn scene_subtree(oracle_tree: &Value) -> Option<&Value> {
    oracle_tree["children"]
        .as_array()?
        .iter()
        .find(|c| c["name"].as_str() != Some("Oracle"))
}

/// Derive expected ENTER_TREE ordering from oracle tree (scene subtree only).
fn expected_enter_tree(oracle_tree: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(scene_root) = scene_subtree(oracle_tree) {
        collect_paths_preorder(scene_root, &mut paths);
    }
    paths
}

/// Derive expected READY ordering from oracle tree (scene subtree only).
fn expected_ready(oracle_tree: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(scene_root) = scene_subtree(oracle_tree) {
        collect_paths_postorder(scene_root, &mut paths);
    }
    paths
}

// ===========================================================================
// Patina trace extraction
// ===========================================================================

/// Extract notification paths for a given detail from Patina's event trace.
fn patina_notification_paths(tree: &SceneTree, detail: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect()
}

// ===========================================================================
// Fixture loading helpers
// ===========================================================================

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

fn load_oracle_tree(fixture_id: &str) -> Value {
    let path = fixtures_dir()
        .join("oracle_outputs")
        .join(format!("{fixture_id}_tree.json"));
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

fn load_tscn(fixture_id: &str) -> String {
    let path = fixtures_dir()
        .join("scenes")
        .join(format!("{fixture_id}.tscn"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Load a tscn fixture into a fresh SceneTree and return it with tracing enabled.
///
/// Uses `add_packed_scene_to_tree` without the root marked as inside_tree,
/// then explicitly fires `LifecycleManager::enter_tree` on the scene root
/// to get the correct bottom-up READY ordering (children before parent).
///
/// This avoids the per-node auto-lifecycle that `add_child` triggers when
/// the parent is already inside_tree, which would produce top-down READY
/// ordering instead of the correct Godot bottom-up ordering.
fn load_scene_into_tree(fixture_id: &str) -> SceneTree {
    let tscn = load_tscn(fixture_id);
    let packed = PackedScene::from_tscn(&tscn)
        .unwrap_or_else(|e| panic!("failed to parse tscn for {fixture_id}: {e}"));

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Do NOT call LifecycleManager::enter_tree on root here —
    // root must not be inside_tree during add_packed_scene_to_tree
    // to prevent per-node auto-lifecycle from firing.

    let scene_id = add_packed_scene_to_tree(&mut tree, root, &packed)
        .unwrap_or_else(|e| panic!("failed to instance {fixture_id}: {e}"));

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Now fire lifecycle for the entire subtree at once —
    // ENTER_TREE top-down, READY bottom-up (matching Godot).
    LifecycleManager::enter_tree(&mut tree, scene_id);

    tree
}

/// Load a tscn fixture and return (tree, scene_root_id) with tracing enabled.
fn load_scene_into_tree_with_id(fixture_id: &str) -> (SceneTree, NodeId) {
    let tscn = load_tscn(fixture_id);
    let packed = PackedScene::from_tscn(&tscn)
        .unwrap_or_else(|e| panic!("failed to parse tscn for {fixture_id}: {e}"));

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_id = add_packed_scene_to_tree(&mut tree, root, &packed)
        .unwrap_or_else(|e| panic!("failed to instance {fixture_id}: {e}"));

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    LifecycleManager::enter_tree(&mut tree, scene_id);

    (tree, scene_id)
}

/// Core comparison: load fixture, compare ENTER_TREE and READY traces against oracle.
fn assert_lifecycle_matches_oracle(fixture_id: &str) {
    let oracle_tree = load_oracle_tree(fixture_id);
    let tree = load_scene_into_tree(fixture_id);

    let expected_enters = expected_enter_tree(&oracle_tree);
    let actual_enters = patina_notification_paths(&tree, "ENTER_TREE");

    assert_eq!(
        actual_enters, expected_enters,
        "\n[{fixture_id}] ENTER_TREE ordering drift!\n\
         Expected (oracle): {expected_enters:?}\n\
         Actual   (patina): {actual_enters:?}"
    );

    let expected_readys = expected_ready(&oracle_tree);
    let actual_readys = patina_notification_paths(&tree, "READY");

    assert_eq!(
        actual_readys, expected_readys,
        "\n[{fixture_id}] READY ordering drift!\n\
         Expected (oracle): {expected_readys:?}\n\
         Actual   (patina): {actual_readys:?}"
    );
}

// ===========================================================================
// 1. minimal: single scene node
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_minimal() {
    assert_lifecycle_matches_oracle("minimal");
}

// ===========================================================================
// 2. hierarchy: Root -> Player -> Sprite (3 nodes, 2 levels)
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_hierarchy() {
    assert_lifecycle_matches_oracle("hierarchy");
}

// ===========================================================================
// 3. platformer: World with 6 flat siblings
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_platformer() {
    assert_lifecycle_matches_oracle("platformer");
}

// ===========================================================================
// 4. ui_menu: MenuRoot with 4 siblings
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_ui_menu() {
    assert_lifecycle_matches_oracle("ui_menu");
}

// ===========================================================================
// 5. unique_name_resolution: 3 branches with unique-named nodes
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_unique_name_resolution() {
    assert_lifecycle_matches_oracle("unique_name_resolution");
}

// ===========================================================================
// 6. pat-wifn: Broadened oracle coverage — 7 additional scenes
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_character_body_test() {
    assert_lifecycle_matches_oracle("character_body_test");
}

#[test]
fn lifecycle_trace_oracle_parity_signals_complex() {
    assert_lifecycle_matches_oracle("signals_complex");
}

#[test]
fn lifecycle_trace_oracle_parity_space_shooter() {
    assert_lifecycle_matches_oracle("space_shooter");
}

#[test]
fn lifecycle_trace_oracle_parity_test_scripts() {
    assert_lifecycle_matches_oracle("test_scripts");
}

#[test]
fn lifecycle_trace_oracle_parity_with_properties() {
    assert_lifecycle_matches_oracle("with_properties");
}

#[test]
fn lifecycle_trace_oracle_parity_physics_playground() {
    assert_lifecycle_matches_oracle("physics_playground");
}

#[test]
fn lifecycle_trace_oracle_parity_signal_instantiation() {
    assert_lifecycle_matches_oracle("signal_instantiation");
}

// ===========================================================================
// 7. pat-wifn: All-fixtures parametric ENTER/READY ordering
// ===========================================================================

/// Parametric test across ALL fixture scenes: ENTER_TREE and READY
/// orderings match oracle for every scene with both a tscn and oracle tree.
#[test]
fn lifecycle_trace_all_fixtures_match_oracle() {
    for fixture_id in ALL_FIXTURE_IDS {
        assert_lifecycle_matches_oracle(fixture_id);
    }
}

// ===========================================================================
// 8. simple_hierarchy: Root -> Child1 -> GrandChild, Child2
// ===========================================================================

#[test]
fn lifecycle_trace_oracle_parity_simple_hierarchy() {
    // simple_hierarchy has a different oracle format (_tree.json uses different
    // schema without groups/owner fields). Parse it manually to derive expected
    // ordering from the tree structure.
    let oracle_tree = load_oracle_tree("simple_hierarchy");

    // The simple_hierarchy oracle tree may or may not have a Window root.
    // Check structure: if "path" is "/root/Root", it's the scene root directly.
    let scene_root = if oracle_tree["path"].as_str() == Some("/root") {
        scene_subtree(&oracle_tree).expect("simple_hierarchy oracle should have scene children")
    } else {
        // The oracle IS the scene root.
        &oracle_tree
    };

    let mut expected_enters = Vec::new();
    collect_paths_preorder(scene_root, &mut expected_enters);

    let mut expected_readys = Vec::new();
    collect_paths_postorder(scene_root, &mut expected_readys);

    // Load in Patina.
    // simple_hierarchy doesn't have a tscn file — build it manually.
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Child1" type="Node2D" parent="."]

[node name="GrandChild" type="Node2D" parent="Child1"]

[node name="Child2" type="Node2D" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    LifecycleManager::enter_tree(&mut tree, scene_id);

    let actual_enters = patina_notification_paths(&tree, "ENTER_TREE");
    assert_eq!(
        actual_enters, expected_enters,
        "\n[simple_hierarchy] ENTER_TREE ordering drift!\n\
         Expected (oracle): {expected_enters:?}\n\
         Actual   (patina): {actual_enters:?}"
    );

    let actual_readys = patina_notification_paths(&tree, "READY");
    assert_eq!(
        actual_readys, expected_readys,
        "\n[simple_hierarchy] READY ordering drift!\n\
         Expected (oracle): {expected_readys:?}\n\
         Actual   (patina): {actual_readys:?}"
    );
}

// ===========================================================================
// 7. Parametric: EXIT_TREE ordering is post-order / bottom-up (oracle-derived)
//    Godot fires EXIT_TREE bottom-up: children (in forward sibling order,
//    each subtree bottom-up) then parent. This is the same traversal as
//    READY (depth-first post-order).
// ===========================================================================

#[test]
fn exit_tree_is_postorder_for_oracle_scenes() {
    for fixture_id in ALL_FIXTURE_IDS {
        let oracle_tree = load_oracle_tree(fixture_id);
        // EXIT_TREE uses the same bottom-up (post-order) traversal as READY.
        let expected_exits = expected_ready(&oracle_tree);

        let (mut tree, scene_id) = load_scene_into_tree_with_id(fixture_id);

        // Trigger exit.
        tree.event_trace_mut().clear();
        LifecycleManager::exit_tree(&mut tree, scene_id);

        let actual_exits = patina_notification_paths(&tree, "EXIT_TREE");

        assert_eq!(
            actual_exits, expected_exits,
            "\n[{fixture_id}] EXIT_TREE ordering drift!\n\
             Expected (oracle post-order): {expected_exits:?}\n\
             Actual   (patina):            {actual_exits:?}"
        );
    }
}

// ===========================================================================
// 8. Transition between oracle-backed scenes preserves ordering
// ===========================================================================

#[test]
fn transition_between_oracle_scenes_preserves_ordering() {
    // Load hierarchy scene, then transition to platformer.
    let (mut tree, hierarchy_scene_id) = load_scene_into_tree_with_id("hierarchy");
    let root = tree.root_id();

    // Clear trace and transition: exit hierarchy, remove, add platformer.
    tree.event_trace_mut().clear();
    LifecycleManager::exit_tree(&mut tree, hierarchy_scene_id);
    tree.remove_node(hierarchy_scene_id).unwrap();

    let platformer_tscn = load_tscn("platformer");
    let packed = PackedScene::from_tscn(&platformer_tscn).unwrap();
    let platformer_scene_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, platformer_scene_id);

    // EXIT_TREE should match hierarchy oracle's post-order (bottom-up).
    let hierarchy_oracle = load_oracle_tree("hierarchy");
    let expected_exits = expected_ready(&hierarchy_oracle);
    let actual_exits = patina_notification_paths(&tree, "EXIT_TREE");
    assert_eq!(
        actual_exits, expected_exits,
        "\n[hierarchy->platformer] EXIT_TREE drift!\n\
         Expected: {expected_exits:?}\n\
         Actual:   {actual_exits:?}"
    );

    // ENTER_TREE should match platformer oracle.
    let platformer_oracle = load_oracle_tree("platformer");
    let expected_enters = expected_enter_tree(&platformer_oracle);
    let actual_enters = patina_notification_paths(&tree, "ENTER_TREE");
    assert_eq!(
        actual_enters, expected_enters,
        "\n[hierarchy->platformer] ENTER_TREE drift!\n\
         Expected: {expected_enters:?}\n\
         Actual:   {actual_enters:?}"
    );

    // READY should match platformer oracle.
    let expected_readys = expected_ready(&platformer_oracle);
    let actual_readys = patina_notification_paths(&tree, "READY");
    assert_eq!(
        actual_readys, expected_readys,
        "\n[hierarchy->platformer] READY drift!\n\
         Expected: {expected_readys:?}\n\
         Actual:   {actual_readys:?}"
    );

    // Global invariant: all exits before any enters.
    let seq: Vec<(String, String)> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(e.detail.as_str(), "ENTER_TREE" | "READY" | "EXIT_TREE")
        })
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect();

    let last_exit = seq
        .iter()
        .rposition(|(_, d)| d == "EXIT_TREE")
        .expect("should have exits");
    let first_enter = seq
        .iter()
        .position(|(_, d)| d == "ENTER_TREE")
        .expect("should have enters");
    assert!(
        last_exit < first_enter,
        "hierarchy->platformer: all EXIT before ENTER: {seq:?}"
    );
}

// ===========================================================================
// 9. Oracle node count matches Patina node count
// ===========================================================================

#[test]
fn oracle_node_count_matches_patina() {
    for fixture_id in ALL_FIXTURE_IDS {
        let oracle_tree = load_oracle_tree(fixture_id);
        let expected_enters = expected_enter_tree(&oracle_tree);
        let tree = load_scene_into_tree(fixture_id);

        // Patina node count = root + scene nodes.
        // Oracle expected_enters count = scene nodes only (excludes root Window).
        let scene_node_count = tree.node_count() - 1; // subtract root
        assert_eq!(
            scene_node_count,
            expected_enters.len(),
            "\n[{fixture_id}] Node count mismatch!\n\
             Oracle scene nodes: {}\n\
             Patina scene nodes: {scene_node_count}",
            expected_enters.len()
        );
    }
}

// ===========================================================================
// 10. ENTER_TREE precedes READY for every node (per oracle scenes)
// ===========================================================================

#[test]
fn enter_tree_precedes_ready_for_every_node_oracle_scenes() {
    for fixture_id in ALL_FIXTURE_IDS {
        let tree = load_scene_into_tree(fixture_id);

        let seq: Vec<(String, String)> = tree
            .event_trace()
            .events()
            .iter()
            .filter(|e| {
                e.event_type == TraceEventType::Notification
                    && (e.detail == "ENTER_TREE" || e.detail == "READY")
            })
            .map(|e| (e.node_path.clone(), e.detail.clone()))
            .collect();

        // For each node that appears, ENTER_TREE must come before READY.
        let oracle_tree = load_oracle_tree(fixture_id);
        let node_paths = expected_enter_tree(&oracle_tree);

        for path in &node_paths {
            let enter_pos = seq
                .iter()
                .position(|(p, d)| p == path && d == "ENTER_TREE")
                .unwrap_or_else(|| panic!("[{fixture_id}] {path} missing ENTER_TREE"));
            let ready_pos = seq
                .iter()
                .position(|(p, d)| p == path && d == "READY")
                .unwrap_or_else(|| panic!("[{fixture_id}] {path} missing READY"));
            assert!(
                enter_pos < ready_pos,
                "[{fixture_id}] {path}: ENTER_TREE (pos {enter_pos}) must precede READY (pos {ready_pos})"
            );
        }
    }
}
