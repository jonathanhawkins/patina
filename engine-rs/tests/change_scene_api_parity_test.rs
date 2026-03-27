//! pat-tlna: API parity tests for `change_scene_to_file` and
//! `reload_current_scene`.
//!
//! Godot 4.x contract references:
//! - `SceneTree.change_scene_to_file(path)` loads a `.tscn` from disk and
//!   sets it as the current scene (equivalent to load + change_scene_to_packed)
//! - `SceneTree.reload_current_scene()` re-instances the current scene from
//!   its original packed source
//! - `reload_current_scene()` returns ERR_UNCONFIGURED when no scene is loaded
//! - `reload_current_scene()` after `change_scene_to_file` works (file was
//!   parsed into a PackedScene internally)

use gdscene::node::Node;
use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;

// ===========================================================================
// Helpers
// ===========================================================================

fn notification_paths(tree: &SceneTree, detail: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect()
}

fn make_tree() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    tree
}

fn simple_packed(name: &str) -> PackedScene {
    let tscn = format!(
        "[gd_scene format=3]\n\n[node name=\"{name}\" type=\"Node2D\"]\n"
    );
    PackedScene::from_tscn(&tscn).expect("valid tscn")
}

fn packed_with_children() -> PackedScene {
    PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="Level" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Enemy" type="CharacterBody2D" parent="."]
"#,
    )
    .expect("valid tscn")
}

// ===========================================================================
// reload_current_scene tests
// ===========================================================================

// 1. reload_current_scene after change_scene_to_packed succeeds
#[test]
fn reload_after_packed_succeeds() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&simple_packed("SceneA")).unwrap();

    let old_id = tree.current_scene().unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let new_id = tree.reload_current_scene().unwrap();

    // The reloaded scene should have a different NodeId (fresh instance).
    assert_ne!(old_id, new_id, "reload creates a fresh instance");
    assert_eq!(tree.current_scene(), Some(new_id));

    // Lifecycle: EXIT_TREE for old, ENTER_TREE + READY for new.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(!exits.is_empty(), "old scene should exit: {exits:?}");

    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(!enters.is_empty(), "new scene should enter: {enters:?}");
}

// 2. reload_current_scene on empty tree (no scene loaded) returns error
#[test]
fn reload_on_empty_tree_errors() {
    let mut tree = make_tree();
    let result = tree.reload_current_scene();
    assert!(result.is_err(), "reload with no scene should error");
}

// 3. reload_current_scene after change_scene_to_node returns error
#[test]
fn reload_after_node_scene_errors() {
    let mut tree = make_tree();
    tree.change_scene_to_node(Node::new("ManualScene", "Node2D")).unwrap();

    let result = tree.reload_current_scene();
    assert!(
        result.is_err(),
        "reload after change_scene_to_node should error (no packed source)"
    );
}

// 4. reload_current_scene preserves tree structure from packed source
#[test]
fn reload_preserves_structure() {
    let mut tree = make_tree();
    let packed = packed_with_children();
    tree.change_scene_to_packed(&packed).unwrap();

    // Before reload: root + Level + Player + Enemy = 4 nodes.
    assert_eq!(tree.node_count(), 4);

    tree.reload_current_scene().unwrap();

    // After reload: same structure.
    assert_eq!(tree.node_count(), 4, "reload should re-create same structure");

    let level = tree.get_node_by_path("/root/Level");
    assert!(level.is_some(), "Level should exist after reload");
    let player = tree.get_node_by_path("/root/Level/Player");
    assert!(player.is_some(), "Player should exist after reload");
    let enemy = tree.get_node_by_path("/root/Level/Enemy");
    assert!(enemy.is_some(), "Enemy should exist after reload");
}

// 5. reload_current_scene after unload_current_scene returns error
#[test]
fn reload_after_unload_errors() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&simple_packed("Scene")).unwrap();
    tree.unload_current_scene().unwrap();

    let result = tree.reload_current_scene();
    assert!(
        result.is_err(),
        "reload after unload should error (packed source cleared)"
    );
}

// 6. reload_current_scene fires full lifecycle (EXIT old, ENTER+READY new)
#[test]
fn reload_fires_full_lifecycle() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed_with_children()).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.reload_current_scene().unwrap();

    let events: Vec<(String, String)> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(e.detail.as_str(), "ENTER_TREE" | "READY" | "EXIT_TREE")
        })
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect();

    // All EXIT_TREE events should come before any ENTER_TREE.
    let last_exit = events.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter = events.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit, first_enter) {
        assert!(
            le < fe,
            "all EXIT_TREE before any ENTER_TREE during reload: {events:?}"
        );
    }
}

// 7. Double reload works correctly
#[test]
fn double_reload_succeeds() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&simple_packed("Scene")).unwrap();

    let id1 = tree.reload_current_scene().unwrap();
    let id2 = tree.reload_current_scene().unwrap();

    assert_ne!(id1, id2, "each reload creates a fresh instance");
    assert_eq!(tree.current_scene(), Some(id2));
    assert_eq!(tree.node_count(), 2, "root + scene");
}

// 8. reload_current_scene preserves properties from tscn
#[test]
fn reload_preserves_properties() {
    let tscn = r#"[gd_scene format=3]

[node name="Player" type="CharacterBody2D"]
speed = 300.0
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();

    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    tree.reload_current_scene().unwrap();

    let player_id = tree.get_node_by_path("/root/Player").unwrap();
    let player = tree.get_node(player_id).unwrap();
    assert_eq!(
        player.get_property("speed"),
        gdvariant::Variant::Float(300.0),
        "properties should be preserved after reload"
    );
}

// 9. change_scene_to_packed then change_scene_to_node clears packed source
#[test]
fn packed_then_node_clears_reload_source() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&simple_packed("PackedScene")).unwrap();

    // This should work (packed source exists).
    assert!(tree.reload_current_scene().is_ok());

    // Switch to node-based scene.
    tree.change_scene_to_node(Node::new("NodeScene", "Node")).unwrap();

    // Now reload should fail (no packed source).
    assert!(
        tree.reload_current_scene().is_err(),
        "reload should fail after switching to node-based scene"
    );
}

// 10. reload_current_scene with groups preserves group membership
#[test]
fn reload_preserves_groups() {
    let tscn = r#"[gd_scene format=3]

[node name="Enemy" type="CharacterBody2D" groups=["enemies", "damageable"]]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();

    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();
    assert_eq!(tree.get_nodes_in_group("enemies").len(), 1);

    tree.reload_current_scene().unwrap();

    assert_eq!(
        tree.get_nodes_in_group("enemies").len(),
        1,
        "enemies group should have 1 member after reload"
    );
    assert_eq!(
        tree.get_nodes_in_group("damageable").len(),
        1,
        "damageable group should have 1 member after reload"
    );
}

// ===========================================================================
// change_scene_to_file tests
// ===========================================================================

// 11. change_scene_to_file with valid tscn file succeeds
#[test]
fn change_to_file_succeeds() {
    let mut tree = make_tree();

    // Use the minimal fixture from the repo.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/scenes/minimal.tscn");
    let new_id = tree.change_scene_to_file(path).unwrap();

    assert_eq!(tree.current_scene(), Some(new_id));
    let root_node = tree.get_node(tree.root_id()).unwrap();
    assert_eq!(root_node.children().len(), 1, "should have 1 scene child");
}

// 12. change_scene_to_file with non-existent path returns error
#[test]
fn change_to_file_missing_path_errors() {
    let mut tree = make_tree();
    let result = tree.change_scene_to_file("/non/existent/scene.tscn");
    assert!(result.is_err(), "missing file should error");
}

// 13. change_scene_to_file removes old scene
#[test]
fn change_to_file_removes_old_scene() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed_with_children()).unwrap();
    assert_eq!(tree.node_count(), 4); // root + Level + Player + Enemy

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/scenes/minimal.tscn");
    tree.change_scene_to_file(path).unwrap();

    // Old scene should have been removed.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(exits.len() >= 3, "old scene nodes should exit: {exits:?}");

    // Only root + new scene root.
    assert_eq!(tree.node_count(), 2, "root + minimal scene root");
}

// 14. change_scene_to_file enables reload_current_scene
#[test]
fn change_to_file_enables_reload() {
    let mut tree = make_tree();

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/scenes/minimal.tscn");
    tree.change_scene_to_file(path).unwrap();

    let old_id = tree.current_scene().unwrap();
    let new_id = tree.reload_current_scene().unwrap();

    assert_ne!(old_id, new_id, "reload after file change creates fresh instance");
    assert_eq!(tree.current_scene(), Some(new_id));
}

// 15. change_scene_to_file sets current_scene
#[test]
fn change_to_file_sets_current_scene() {
    let mut tree = make_tree();
    assert!(tree.current_scene().is_none());

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/scenes/minimal.tscn");
    let id = tree.change_scene_to_file(path).unwrap();
    assert_eq!(tree.current_scene(), Some(id));
}

// 16. change_scene_to_file with invalid tscn content errors
#[test]
fn change_to_file_invalid_tscn_errors() {
    // Create a temp file with invalid content.
    let dir = std::env::temp_dir().join("patina_test_invalid_tscn");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.tscn");
    std::fs::write(&path, "this is not valid tscn content").unwrap();

    let mut tree = make_tree();
    let result = tree.change_scene_to_file(path.to_str().unwrap());
    assert!(result.is_err(), "invalid tscn content should error");

    // Cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

// 17. Sequential: file -> node -> file, current_scene tracks correctly
#[test]
fn file_node_file_current_scene_tracking() {
    let mut tree = make_tree();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/scenes/minimal.tscn");

    let id_a = tree.change_scene_to_file(path).unwrap();
    assert_eq!(tree.current_scene(), Some(id_a));

    let id_b = tree.change_scene_to_node(Node::new("Manual", "Node")).unwrap();
    assert_eq!(tree.current_scene(), Some(id_b));
    assert!(tree.reload_current_scene().is_err(), "no packed source after node change");

    let id_c = tree.change_scene_to_file(path).unwrap();
    assert_eq!(tree.current_scene(), Some(id_c));
    assert!(tree.reload_current_scene().is_ok(), "reload should work after file change");
}

// 18. Sequential: packed -> file -> reload uses file's packed scene
#[test]
fn packed_then_file_reload_uses_file_source() {
    let mut tree = make_tree();

    // Load a packed scene with children.
    tree.change_scene_to_packed(&packed_with_children()).unwrap();
    assert_eq!(tree.node_count(), 4); // root + Level + Player + Enemy

    // Switch to file-based scene (minimal.tscn has just Root).
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/scenes/minimal.tscn");
    tree.change_scene_to_file(path).unwrap();
    assert_eq!(tree.node_count(), 2); // root + Root

    // Reload should re-instance from the file's packed scene, not the old one.
    tree.reload_current_scene().unwrap();
    assert_eq!(
        tree.node_count(),
        2,
        "reload should use the file's packed scene (minimal, not packed_with_children)"
    );
}
