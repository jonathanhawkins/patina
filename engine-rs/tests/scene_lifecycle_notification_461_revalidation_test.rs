//! pat-n5r: Revalidate scene lifecycle and notification ordering against 4.6.1.
//!
//! After the Godot 4.6.1 repin, this test revalidates all lifecycle contracts
//! (ENTER_TREE, READY, EXIT_TREE) and notification ordering against the
//! refreshed oracle `_tree.json` outputs. Covers ALL fixtures that have both
//! a `.tscn` file and an oracle `_tree.json` — including the 3D fixtures
//! added post-repin.
//!
//! Oracle source: fixtures/oracle_outputs/*_tree.json (Godot 4.6.1-stable)

use std::path::PathBuf;

use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop};
use serde_json::Value;

/// All fixture IDs that have both a `_tree.json` oracle and a `.tscn` file,
/// including 3D fixtures added during the 4.6.1 repin.
const ALL_461_FIXTURE_IDS: &[&str] = &[
    // 2D fixtures (pre-existing)
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
    // 3D fixtures (added post-repin)
    "hierarchy_3d",
    "indoor_3d",
    "minimal_3d",
    "multi_light_3d",
    "physics_3d_playground",
    "physics_playground_extended",
];

// ===========================================================================
// Oracle tree parser (same proven logic as lifecycle_trace_oracle_parity_test)
// ===========================================================================

fn normalize_oracle_path(path: &str) -> String {
    path.split('/')
        .map(|seg| seg.strip_prefix('%').unwrap_or(seg))
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_paths_preorder(node: &Value, out: &mut Vec<String>) {
    let name = node["name"].as_str().unwrap_or("");
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

fn scene_subtree(oracle_tree: &Value) -> Option<&Value> {
    oracle_tree["children"]
        .as_array()?
        .iter()
        .find(|c| c["name"].as_str() != Some("Oracle"))
}

fn expected_enter_tree(oracle_tree: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(scene_root) = scene_subtree(oracle_tree) {
        collect_paths_preorder(scene_root, &mut paths);
    }
    paths
}

fn expected_ready(oracle_tree: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(scene_root) = scene_subtree(oracle_tree) {
        collect_paths_postorder(scene_root, &mut paths);
    }
    paths
}

// ===========================================================================
// Helpers
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

fn load_scene_into_tree(fixture_id: &str) -> SceneTree {
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

    tree
}

fn load_scene_with_id(fixture_id: &str) -> (SceneTree, gdscene::NodeId) {
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

fn notification_paths(tree: &SceneTree, detail: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect()
}

// ===========================================================================
// 1. ENTER_TREE top-down ordering matches 4.6.1 oracle (all fixtures)
// ===========================================================================

#[test]
fn enter_tree_topdown_matches_461_oracle_all_fixtures() {
    for fixture_id in ALL_461_FIXTURE_IDS {
        let oracle_tree = load_oracle_tree(fixture_id);
        let tree = load_scene_into_tree(fixture_id);

        let expected = expected_enter_tree(&oracle_tree);
        let actual = notification_paths(&tree, "ENTER_TREE");

        assert_eq!(
            actual, expected,
            "\n[461 revalidation] [{fixture_id}] ENTER_TREE ordering drift!\n\
             Expected (oracle): {expected:?}\n\
             Actual   (patina): {actual:?}"
        );
    }
}

// ===========================================================================
// 2. READY bottom-up ordering matches 4.6.1 oracle (all fixtures)
// ===========================================================================

#[test]
fn ready_bottomup_matches_461_oracle_all_fixtures() {
    for fixture_id in ALL_461_FIXTURE_IDS {
        let oracle_tree = load_oracle_tree(fixture_id);
        let tree = load_scene_into_tree(fixture_id);

        let expected = expected_ready(&oracle_tree);
        let actual = notification_paths(&tree, "READY");

        assert_eq!(
            actual, expected,
            "\n[461 revalidation] [{fixture_id}] READY ordering drift!\n\
             Expected (oracle): {expected:?}\n\
             Actual   (patina): {actual:?}"
        );
    }
}

// ===========================================================================
// 3. EXIT_TREE bottom-up ordering matches 4.6.1 oracle (all fixtures)
// ===========================================================================

#[test]
fn exit_tree_bottomup_matches_461_oracle_all_fixtures() {
    for fixture_id in ALL_461_FIXTURE_IDS {
        let oracle_tree = load_oracle_tree(fixture_id);
        let expected = expected_ready(&oracle_tree); // same post-order as READY

        let (mut tree, scene_id) = load_scene_with_id(fixture_id);

        tree.event_trace_mut().clear();
        LifecycleManager::exit_tree(&mut tree, scene_id);

        let actual = notification_paths(&tree, "EXIT_TREE");

        assert_eq!(
            actual, expected,
            "\n[461 revalidation] [{fixture_id}] EXIT_TREE ordering drift!\n\
             Expected (oracle post-order): {expected:?}\n\
             Actual   (patina):            {actual:?}"
        );
    }
}

// ===========================================================================
// 4. Node count matches 4.6.1 oracle (all fixtures)
// ===========================================================================

#[test]
fn node_count_matches_461_oracle_all_fixtures() {
    for fixture_id in ALL_461_FIXTURE_IDS {
        let oracle_tree = load_oracle_tree(fixture_id);
        let expected_count = expected_enter_tree(&oracle_tree).len();
        let tree = load_scene_into_tree(fixture_id);

        let scene_count = tree.node_count() - 1; // subtract root
        assert_eq!(
            scene_count, expected_count,
            "\n[461 revalidation] [{fixture_id}] Node count mismatch!\n\
             Oracle: {expected_count}, Patina: {scene_count}"
        );
    }
}

// ===========================================================================
// 5. ENTER_TREE precedes READY for every node (all fixtures)
// ===========================================================================

#[test]
fn enter_tree_precedes_ready_per_node_461() {
    for fixture_id in ALL_461_FIXTURE_IDS {
        let tree = load_scene_into_tree(fixture_id);
        let oracle_tree = load_oracle_tree(fixture_id);
        let node_paths = expected_enter_tree(&oracle_tree);

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

        for path in &node_paths {
            let enter_pos = seq
                .iter()
                .position(|(p, d)| p == path && d == "ENTER_TREE")
                .unwrap_or_else(|| panic!("[461] [{fixture_id}] {path} missing ENTER_TREE"));
            let ready_pos = seq
                .iter()
                .position(|(p, d)| p == path && d == "READY")
                .unwrap_or_else(|| panic!("[461] [{fixture_id}] {path} missing READY"));
            assert!(
                enter_pos < ready_pos,
                "[461] [{fixture_id}] {path}: ENTER_TREE @{enter_pos} must precede READY @{ready_pos}"
            );
        }
    }
}

// ===========================================================================
// 6. All ENTER_TREE events fire before any READY event (global invariant)
// ===========================================================================

#[test]
fn all_enters_before_any_ready_461() {
    for fixture_id in ALL_461_FIXTURE_IDS {
        let tree = load_scene_into_tree(fixture_id);

        let seq: Vec<&str> = tree
            .event_trace()
            .events()
            .iter()
            .filter(|e| {
                e.event_type == TraceEventType::Notification
                    && (e.detail == "ENTER_TREE" || e.detail == "READY")
            })
            .map(|e| e.detail.as_str())
            .collect();

        if let Some(last_enter) = seq.iter().rposition(|d| *d == "ENTER_TREE") {
            if let Some(first_ready) = seq.iter().position(|d| *d == "READY") {
                assert!(
                    last_enter < first_ready,
                    "[461] [{fixture_id}] Last ENTER_TREE @{last_enter} must precede first READY @{first_ready}"
                );
            }
        }
    }
}

// ===========================================================================
// 7. 3D fixtures specifically: lifecycle ordering matches 2D contract
// ===========================================================================

#[test]
fn lifecycle_3d_fixtures_match_oracle_461() {
    let fixtures_3d = &[
        "hierarchy_3d",
        "indoor_3d",
        "minimal_3d",
        "multi_light_3d",
        "physics_3d_playground",
        "physics_playground_extended",
    ];

    for fixture_id in fixtures_3d {
        let oracle_tree = load_oracle_tree(fixture_id);
        let tree = load_scene_into_tree(fixture_id);

        // ENTER_TREE
        let expected_enters = expected_enter_tree(&oracle_tree);
        let actual_enters = notification_paths(&tree, "ENTER_TREE");
        assert_eq!(
            actual_enters, expected_enters,
            "\n[461 3D] [{fixture_id}] ENTER_TREE drift!\n\
             Expected: {expected_enters:?}\n\
             Actual:   {actual_enters:?}"
        );

        // READY
        let expected_readys = expected_ready(&oracle_tree);
        let actual_readys = notification_paths(&tree, "READY");
        assert_eq!(
            actual_readys, expected_readys,
            "\n[461 3D] [{fixture_id}] READY drift!\n\
             Expected: {expected_readys:?}\n\
             Actual:   {actual_readys:?}"
        );
    }
}

// ===========================================================================
// 8. Scene transition preserves ordering across 2D and 3D (4.6.1)
// ===========================================================================

#[test]
fn scene_transition_2d_to_3d_preserves_lifecycle_461() {
    // Load a 2D scene, transition to 3D — verify lifecycle ordering.
    let (mut tree, scene_2d) = load_scene_with_id("hierarchy");
    let root = tree.root_id();

    // Exit 2D scene
    tree.event_trace_mut().clear();
    LifecycleManager::exit_tree(&mut tree, scene_2d);
    tree.remove_node(scene_2d).unwrap();

    // Enter 3D scene
    let tscn_3d = load_tscn("hierarchy_3d");
    let packed_3d = PackedScene::from_tscn(&tscn_3d).unwrap();
    let scene_3d = add_packed_scene_to_tree(&mut tree, root, &packed_3d).unwrap();
    LifecycleManager::enter_tree(&mut tree, scene_3d);

    // Verify EXIT_TREE of 2D scene is bottom-up
    let hierarchy_oracle = load_oracle_tree("hierarchy");
    let expected_exits = expected_ready(&hierarchy_oracle);
    let actual_exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(
        actual_exits, expected_exits,
        "[461] 2D→3D EXIT_TREE drift"
    );

    // Verify ENTER_TREE of 3D scene is top-down
    let h3d_oracle = load_oracle_tree("hierarchy_3d");
    let expected_enters = expected_enter_tree(&h3d_oracle);
    let actual_enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(
        actual_enters, expected_enters,
        "[461] 2D→3D ENTER_TREE drift"
    );

    // Verify all exits before any enters
    let seq: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(e.detail.as_str(), "ENTER_TREE" | "EXIT_TREE")
        })
        .map(|e| e.detail.clone())
        .collect();

    let last_exit = seq.iter().rposition(|d| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|d| d == "ENTER_TREE").unwrap();
    assert!(
        last_exit < first_enter,
        "[461] 2D→3D: all exits must precede enters"
    );
}

// ===========================================================================
// 9. Notification ordering with add_child (non-packed) matches Godot 4.6.1
// ===========================================================================

#[test]
fn add_child_notification_ordering_461() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build: Root -> A -> B, C
    let a = tree.add_child(root, Node::new("A", "Node3D")).unwrap();
    let _b = tree.add_child(a, Node::new("B", "Node3D")).unwrap();
    let _c = tree.add_child(a, Node::new("C", "Node3D")).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    LifecycleManager::enter_tree(&mut tree, a);

    // ENTER_TREE: top-down A → B → C
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enters, vec!["/root/A", "/root/A/B", "/root/A/C"]);

    // READY: bottom-up B → C → A
    let readys = notification_paths(&tree, "READY");
    assert_eq!(readys, vec!["/root/A/B", "/root/A/C", "/root/A"]);

    // EXIT_TREE: bottom-up B → C → A
    tree.event_trace_mut().clear();
    LifecycleManager::exit_tree(&mut tree, a);
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(exits, vec!["/root/A/B", "/root/A/C", "/root/A"]);
}

// ===========================================================================
// 10. Deep hierarchy: 5+ levels lifecycle ordering (4.6.1)
// ===========================================================================

#[test]
fn deep_hierarchy_lifecycle_ordering_461() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build 5-level chain: Root -> L1 -> L2 -> L3 -> L4 -> L5
    let l1 = tree.add_child(root, Node::new("L1", "Node2D")).unwrap();
    let l2 = tree.add_child(l1, Node::new("L2", "Node2D")).unwrap();
    let l3 = tree.add_child(l2, Node::new("L3", "Node2D")).unwrap();
    let l4 = tree.add_child(l3, Node::new("L4", "Node2D")).unwrap();
    let _l5 = tree.add_child(l4, Node::new("L5", "Node2D")).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    LifecycleManager::enter_tree(&mut tree, l1);

    // ENTER_TREE: top-down L1 → L2 → L3 → L4 → L5
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(
        enters,
        vec![
            "/root/L1",
            "/root/L1/L2",
            "/root/L1/L2/L3",
            "/root/L1/L2/L3/L4",
            "/root/L1/L2/L3/L4/L5",
        ]
    );

    // READY: bottom-up L5 → L4 → L3 → L2 → L1
    let readys = notification_paths(&tree, "READY");
    assert_eq!(
        readys,
        vec![
            "/root/L1/L2/L3/L4/L5",
            "/root/L1/L2/L3/L4",
            "/root/L1/L2/L3",
            "/root/L1/L2",
            "/root/L1",
        ]
    );

    // EXIT_TREE: same bottom-up as READY
    tree.event_trace_mut().clear();
    LifecycleManager::exit_tree(&mut tree, l1);
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(exits, readys);
}

// ===========================================================================
// 11. Process notification fires after lifecycle complete (4.6.1)
// ===========================================================================

#[test]
fn process_fires_after_lifecycle_complete_461() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let _b = tree.add_child(a, Node::new("B", "Node2D")).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    LifecycleManager::enter_tree(&mut tree, a);

    // Step one frame via MainLoop (which owns the tree)
    let mut main_loop = MainLoop::new(tree);
    main_loop.step(1.0 / 60.0);

    let all_events: Vec<(&str, &str)> = main_loop
        .tree()
        .event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(
                    e.detail.as_str(),
                    "ENTER_TREE" | "READY" | "PROCESS"
                )
        })
        .map(|e| (e.detail.as_str(), e.node_path.as_str()))
        .collect();

    // All ENTER_TREE and READY events must precede any PROCESS events
    let last_ready = all_events
        .iter()
        .rposition(|(d, _)| *d == "READY")
        .expect("should have READY events");
    let first_process = all_events
        .iter()
        .position(|(d, _)| *d == "PROCESS")
        .expect("should have PROCESS events");
    assert!(
        last_ready < first_process,
        "[461] PROCESS must fire after all lifecycle complete: {all_events:?}"
    );
}

// ===========================================================================
// 12. Fixture count guard — ensure we're testing all expected fixtures
// ===========================================================================

#[test]
fn fixture_count_guard_461() {
    // Ensure the fixture list includes both 2D and 3D scenes.
    assert_eq!(
        ALL_461_FIXTURE_IDS.len(),
        18,
        "Expected 18 fixtures (12 2D + 6 3D) for 4.6.1 revalidation"
    );

    // Verify all fixture files exist.
    for fixture_id in ALL_461_FIXTURE_IDS {
        let tscn_path = fixtures_dir()
            .join("scenes")
            .join(format!("{fixture_id}.tscn"));
        assert!(
            tscn_path.exists(),
            "Missing tscn for {fixture_id}: {}",
            tscn_path.display()
        );

        let oracle_path = fixtures_dir()
            .join("oracle_outputs")
            .join(format!("{fixture_id}_tree.json"));
        assert!(
            oracle_path.exists(),
            "Missing oracle tree for {fixture_id}: {}",
            oracle_path.display()
        );
    }
}
