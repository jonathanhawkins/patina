//! Integration tests for space shooter demo scenes and scripts.
//!
//! Verifies that all .tscn scene files parse correctly, have the expected
//! node structure, and that all GDScript files parse without errors.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdscript_interop::{tokenize, Parser};

// ---------------------------------------------------------------------------
// Scene fixtures
// ---------------------------------------------------------------------------

const SPACE_SHOOTER_TSCN: &str = include_str!("../fixtures/scenes/space_shooter.tscn");
const TEST_SCRIPTS_TSCN: &str = include_str!("../fixtures/scenes/test_scripts.tscn");

// ---------------------------------------------------------------------------
// Script fixtures
// ---------------------------------------------------------------------------

const PLAYER_GD: &str = include_str!("../fixtures/scripts/player.gd");
const ENEMY_SPAWNER_GD: &str = include_str!("../fixtures/scripts/enemy_spawner.gd");
const TEST_VARIABLES_GD: &str = include_str!("../fixtures/scripts/test_variables.gd");
const TEST_MOVEMENT_GD: &str = include_str!("../fixtures/scripts/test_movement.gd");

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn parse_gdscript(source: &str) -> Vec<gdscript_interop::Stmt> {
    let tokens = tokenize(source).expect("tokenization should succeed");
    let mut parser = Parser::new(tokens, source);
    parser.parse_script().expect("parsing should succeed")
}

// ===========================================================================
// Space Shooter scene tests
// ===========================================================================

#[test]
fn space_shooter_tscn_parses() {
    let scene = PackedScene::from_tscn(SPACE_SHOOTER_TSCN).unwrap();
    assert_eq!(scene.node_count(), 5);
    assert_eq!(scene.uid.as_deref(), Some("uid://space_shooter"));
}

#[test]
fn space_shooter_instances_correctly() {
    let scene = PackedScene::from_tscn(SPACE_SHOOTER_TSCN).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(nodes.len(), 5);

    // Root
    assert_eq!(nodes[0].name(), "SpaceShooter");
    assert_eq!(nodes[0].class_name(), "Node2D");
    assert!(nodes[0].parent().is_none());
    assert_eq!(nodes[0].children().len(), 4);

    // Background
    assert_eq!(nodes[1].name(), "Background");
    assert_eq!(nodes[1].class_name(), "Node2D");
    assert_eq!(nodes[1].parent(), Some(nodes[0].id()));

    // Player
    assert_eq!(nodes[2].name(), "Player");
    assert_eq!(nodes[2].class_name(), "Node2D");
    assert_eq!(
        nodes[2].get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(320.0, 400.0))
    );
    assert_eq!(nodes[2].get_property("speed"), gdvariant::Variant::Int(200));

    // EnemySpawner
    assert_eq!(nodes[3].name(), "EnemySpawner");
    assert_eq!(nodes[3].class_name(), "Node2D");
    assert_eq!(
        nodes[3].get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(320.0, 0.0))
    );

    // ScoreLabel
    assert_eq!(nodes[4].name(), "ScoreLabel");
    assert_eq!(nodes[4].class_name(), "Node2D");
    assert_eq!(
        nodes[4].get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(10.0, 10.0))
    );
}

#[test]
fn space_shooter_script_paths_resolved() {
    let scene = PackedScene::from_tscn(SPACE_SHOOTER_TSCN).unwrap();
    let nodes = scene.instance().unwrap();

    // Player should have script path resolved.
    assert_eq!(
        nodes[2].get_property("_script_path"),
        gdvariant::Variant::String("res://fixtures/scripts/player.gd".into())
    );

    // EnemySpawner should have script path resolved.
    assert_eq!(
        nodes[3].get_property("_script_path"),
        gdvariant::Variant::String("res://fixtures/scripts/enemy_spawner.gd".into())
    );
}

#[test]
fn space_shooter_adds_to_tree() {
    let scene = PackedScene::from_tscn(SPACE_SHOOTER_TSCN).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // 1 tree root + 5 scene nodes = 6 total.
    assert_eq!(tree.node_count(), 6);

    assert!(tree.get_node_by_path("/root/SpaceShooter/Player").is_some());
    assert!(tree
        .get_node_by_path("/root/SpaceShooter/EnemySpawner")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/SpaceShooter/ScoreLabel")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/SpaceShooter/Background")
        .is_some());
}

// ===========================================================================
// Test Scripts scene tests
// ===========================================================================

#[test]
fn test_scripts_tscn_parses() {
    let scene = PackedScene::from_tscn(TEST_SCRIPTS_TSCN).unwrap();
    assert_eq!(scene.node_count(), 3);
    assert_eq!(scene.uid.as_deref(), Some("uid://test_scripts"));
}

#[test]
fn test_scripts_instances_correctly() {
    let scene = PackedScene::from_tscn(TEST_SCRIPTS_TSCN).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(nodes.len(), 3);

    assert_eq!(nodes[0].name(), "TestScene");
    assert_eq!(nodes[0].class_name(), "Node2D");

    assert_eq!(nodes[1].name(), "Mover");
    assert_eq!(
        nodes[1].get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(100.0, 200.0))
    );
    assert_eq!(
        nodes[1].get_property("_script_path"),
        gdvariant::Variant::String("res://fixtures/scripts/test_movement.gd".into())
    );

    assert_eq!(nodes[2].name(), "VarTest");
    assert_eq!(
        nodes[2].get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(300.0, 200.0))
    );
    assert_eq!(
        nodes[2].get_property("_script_path"),
        gdvariant::Variant::String("res://fixtures/scripts/test_variables.gd".into())
    );
}

#[test]
fn test_scripts_adds_to_tree() {
    let scene = PackedScene::from_tscn(TEST_SCRIPTS_TSCN).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // 1 tree root + 3 scene nodes = 4 total.
    assert_eq!(tree.node_count(), 4);

    assert!(tree.get_node_by_path("/root/TestScene/Mover").is_some());
    assert!(tree.get_node_by_path("/root/TestScene/VarTest").is_some());
}

// ===========================================================================
// GDScript parsing tests
// ===========================================================================

#[test]
fn player_gd_parses() {
    let stmts = parse_gdscript(PLAYER_GD);
    // extends Node2D + 3 var decls + 1 func _process
    assert!(!stmts.is_empty());
    assert!(
        matches!(&stmts[0], gdscript_interop::Stmt::Extends { class_name } if class_name == "Node2D")
    );

    // Check var declarations exist.
    let var_count = stmts
        .iter()
        .filter(|s| matches!(s, gdscript_interop::Stmt::VarDecl { .. }))
        .count();
    assert_eq!(var_count, 3, "player.gd should have 3 var declarations");

    // Check _process function exists.
    let has_process = stmts
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::FuncDef { name, .. } if name == "_process"));
    assert!(has_process, "player.gd should have _process function");
}

#[test]
fn enemy_spawner_gd_parses() {
    let stmts = parse_gdscript(ENEMY_SPAWNER_GD);
    assert!(!stmts.is_empty());
    assert!(
        matches!(&stmts[0], gdscript_interop::Stmt::Extends { class_name } if class_name == "Node2D")
    );

    let var_count = stmts
        .iter()
        .filter(|s| matches!(s, gdscript_interop::Stmt::VarDecl { .. }))
        .count();
    assert_eq!(
        var_count, 2,
        "enemy_spawner.gd should have 2 var declarations"
    );

    let has_process = stmts
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::FuncDef { name, .. } if name == "_process"));
    assert!(
        has_process,
        "enemy_spawner.gd should have _process function"
    );
}

#[test]
fn test_variables_gd_parses() {
    let stmts = parse_gdscript(TEST_VARIABLES_GD);
    assert!(!stmts.is_empty());
    assert!(
        matches!(&stmts[0], gdscript_interop::Stmt::Extends { class_name } if class_name == "Node2D")
    );

    let var_count = stmts
        .iter()
        .filter(|s| matches!(s, gdscript_interop::Stmt::VarDecl { .. }))
        .count();
    assert_eq!(
        var_count, 4,
        "test_variables.gd should have 4 var declarations"
    );

    let has_ready = stmts
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::FuncDef { name, .. } if name == "_ready"));
    assert!(has_ready, "test_variables.gd should have _ready function");

    let has_process = stmts
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::FuncDef { name, .. } if name == "_process"));
    assert!(
        has_process,
        "test_variables.gd should have _process function"
    );
}

#[test]
fn test_movement_gd_parses() {
    let stmts = parse_gdscript(TEST_MOVEMENT_GD);
    assert!(!stmts.is_empty());
    assert!(
        matches!(&stmts[0], gdscript_interop::Stmt::Extends { class_name } if class_name == "Node2D")
    );

    let var_count = stmts
        .iter()
        .filter(|s| matches!(s, gdscript_interop::Stmt::VarDecl { .. }))
        .count();
    assert_eq!(
        var_count, 2,
        "test_movement.gd should have 2 var declarations"
    );

    let has_process = stmts
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::FuncDef { name, .. } if name == "_process"));
    assert!(
        has_process,
        "test_movement.gd should have _process function"
    );
}

// ===========================================================================
// Script content validation
// ===========================================================================

#[test]
fn player_gd_process_has_movement_and_shooting() {
    let stmts = parse_gdscript(PLAYER_GD);
    let process_func = stmts.iter().find_map(|s| match s {
        gdscript_interop::Stmt::FuncDef { name, body, .. } if name == "_process" => Some(body),
        _ => None,
    });
    let body = process_func.expect("_process function should exist");

    // The body should have multiple if statements for movement + shooting.
    let if_count = body
        .iter()
        .filter(|s| matches!(s, gdscript_interop::Stmt::If { .. }))
        .count();
    assert!(
        if_count >= 4,
        "_process should have at least 4 if statements for movement, got {if_count}"
    );
}

#[test]
fn enemy_spawner_gd_process_has_timer_logic() {
    let stmts = parse_gdscript(ENEMY_SPAWNER_GD);
    let process_func = stmts.iter().find_map(|s| match s {
        gdscript_interop::Stmt::FuncDef { name, body, .. } if name == "_process" => Some(body),
        _ => None,
    });
    let body = process_func.expect("_process function should exist");

    // Should have an assignment (spawn_timer += delta) and an if statement.
    let has_assignment = body
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::Assignment { .. }));
    assert!(has_assignment, "_process should have assignment statement");

    let has_if = body
        .iter()
        .any(|s| matches!(s, gdscript_interop::Stmt::If { .. }));
    assert!(has_if, "_process should have if statement");
}
