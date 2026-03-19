//! End-to-end project loading integration tests.
//!
//! Validates the full pipeline: project.godot parsing -> scene loading ->
//! node instancing -> script attachment -> lifecycle execution.

use std::path::PathBuf;

use gdresource::project::ProjectLoader;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{MainLoop, SceneTree};
use gdvariant::Variant;

fn sample_project_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("fixtures")
        .join("sample_project")
}

fn load_project() -> ProjectLoader {
    ProjectLoader::load(&sample_project_path()).expect("failed to load sample project")
}

fn load_main_scene(loader: &ProjectLoader) -> PackedScene {
    let path = loader
        .resolve_path(&loader.config().main_scene)
        .expect("failed to resolve main scene path");
    let content = std::fs::read_to_string(&path).expect("failed to read main scene file");
    PackedScene::from_tscn(&content).expect("failed to parse main scene")
}

/// Builds a full scene tree with scripts attached and ready called.
fn build_full_tree() -> (MainLoop, Vec<(gdscene::NodeId, String)>) {
    let loader = load_project();
    let packed = load_main_scene(&loader);

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("failed to instance scene");

    // Attach scripts
    let mut scripted = Vec::new();
    let node_ids = tree.all_nodes_in_tree_order();
    for &nid in &node_ids {
        let script_path = {
            let node = tree.get_node(nid).unwrap();
            match node.get_property("_script_path") {
                Variant::String(p) => Some(p.clone()),
                _ => None,
            }
        };

        if let Some(ref path) = script_path {
            let abs = loader.resolve_path(path).unwrap();
            let source = std::fs::read_to_string(&abs).unwrap();
            let name = tree.get_node(nid).unwrap().name().to_string();
            let instance = GDScriptNodeInstance::from_source(&source, nid).unwrap();
            tree.attach_script(nid, Box::new(instance));
            scripted.push((nid, name));
        }
    }

    // Run enter_tree
    let all = tree.all_nodes_in_tree_order();
    for &nid in &all {
        tree.process_script_enter_tree(nid);
    }

    // Run _ready
    for &(nid, _) in &scripted {
        tree.process_script_ready(nid);
    }

    let config = loader.config();
    let mut main_loop = MainLoop::new(tree);
    main_loop.set_physics_ticks_per_second(config.physics_ticks_per_second);

    (main_loop, scripted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn load_project_godot_successfully() {
    let loader = load_project();
    assert_eq!(loader.config().project_name, "Sample Game");
}

#[test]
fn parse_main_scene_path() {
    let loader = load_project();
    assert_eq!(loader.config().main_scene, "res://scenes/main.tscn");
}

#[test]
fn load_main_tscn_successfully() {
    let loader = load_project();
    let scene = load_main_scene(&loader);
    assert_eq!(scene.node_count(), 4); // World, Player, Enemy, Ground
}

#[test]
fn instance_creates_correct_hierarchy() {
    let loader = load_project();
    let packed = load_main_scene(&loader);

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root_id, &packed).unwrap();

    // 5 total: SceneTree root + World + Player + Enemy + Ground
    assert_eq!(tree.node_count(), 5);

    let world = tree.get_node(scene_root).unwrap();
    assert_eq!(world.name(), "World");
    assert_eq!(world.class_name(), "Node2D");
    assert_eq!(world.children().len(), 3);
}

#[test]
fn gdscript_files_parse_without_error() {
    let loader = load_project();

    for script_name in &["scripts/player.gd", "scripts/enemy.gd"] {
        let path = loader
            .resolve_path(&format!("res://{script_name}"))
            .unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        let node_id = gdscene::NodeId::next();
        GDScriptNodeInstance::from_source(&source, node_id)
            .unwrap_or_else(|e| panic!("{script_name} failed to parse: {e}"));
    }
}

#[test]
fn scripts_attach_to_nodes() {
    let loader = load_project();
    let packed = load_main_scene(&loader);

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).unwrap();

    let mut script_count = 0;
    for &nid in &tree.all_nodes_in_tree_order() {
        let script_path = {
            let node = tree.get_node(nid).unwrap();
            match node.get_property("_script_path") {
                Variant::String(p) => Some(p.clone()),
                _ => None,
            }
        };

        if let Some(ref path) = script_path {
            let abs = loader.resolve_path(path).unwrap();
            let source = std::fs::read_to_string(&abs).unwrap();
            let instance = GDScriptNodeInstance::from_source(&source, nid).unwrap();
            tree.attach_script(nid, Box::new(instance));
            script_count += 1;
        }
    }

    assert_eq!(script_count, 2, "expected Player and Enemy scripts");

    // Verify scripts are actually attached
    for &nid in &tree.all_nodes_in_tree_order() {
        if tree.has_script(nid) {
            assert!(tree.get_script(nid).is_some());
        }
    }
}

#[test]
fn ready_fires_on_scripted_nodes() {
    let (main_loop, scripted) = build_full_tree();
    let tree = main_loop.tree();

    // After _ready, player health should be 100
    let (player_id, _) = scripted.iter().find(|(_, n)| n == "Player").unwrap();
    let script = tree.get_script(*player_id).unwrap();
    assert_eq!(
        script.get_property("health"),
        Some(Variant::Int(100)),
        "player health should be 100 after _ready"
    );
}

#[test]
fn process_fires_each_frame() {
    let (mut main_loop, _scripted) = build_full_tree();

    // Run a single frame
    let delta = 1.0 / 60.0;
    main_loop.step(delta);

    assert_eq!(main_loop.frame_count(), 1);
}

#[test]
fn script_modifies_properties_over_frames() {
    // Use a script that assigns via `self.speed` so property changes persist
    // across method calls (bare `speed = ...` only updates local scope).
    let loader = load_project();
    let packed = load_main_scene(&loader);

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).unwrap();

    // Find the Player node and attach a script that uses self-assignment.
    let player_script = r#"
extends Node2D
var speed = 200.0
var health = 100
func _ready():
    health = 100
func _process(delta):
    self.speed = self.speed + delta
"#;

    let node_ids = tree.all_nodes_in_tree_order();
    let player_id = node_ids
        .iter()
        .find(|&&nid| tree.get_node(nid).unwrap().name() == "Player")
        .copied()
        .unwrap();

    let instance = GDScriptNodeInstance::from_source(player_script, player_id).unwrap();
    tree.attach_script(player_id, Box::new(instance));
    tree.process_script_ready(player_id);

    // Initial speed should be 200.0
    let initial_speed = tree
        .get_script(player_id)
        .unwrap()
        .get_property("speed")
        .unwrap();
    let initial_val = match initial_speed {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("expected numeric speed, got {other:?}"),
    };
    assert!(
        (initial_val - 200.0).abs() < 0.01,
        "initial speed should be ~200, got {initial_val}"
    );

    // Run 10 frames
    let mut main_loop = MainLoop::new(tree);
    let delta = 1.0 / 60.0;
    main_loop.run_frames(10, delta);

    // Speed should have increased: 200.0 + 10 * (1/60) ≈ 200.167
    let new_speed = main_loop
        .tree()
        .get_script(player_id)
        .unwrap()
        .get_property("speed")
        .unwrap();

    let new_val = match new_speed {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("expected numeric speed, got {other:?}"),
    };
    assert!(
        new_val > initial_val,
        "speed should have increased from {initial_val}, got {new_val}"
    );
}

#[test]
fn full_60_frame_run_completes() {
    let (mut main_loop, _scripted) = build_full_tree();

    let delta = 1.0 / 60.0;
    main_loop.run_frames(60, delta);

    assert_eq!(main_loop.frame_count(), 60);
    assert!(main_loop.process_time() > 0.99, "should be ~1 second");
}

#[test]
fn viewport_config_parsed() {
    let loader = load_project();
    assert_eq!(loader.config().viewport_width, 640);
    assert_eq!(loader.config().viewport_height, 480);
}

#[test]
fn player_tscn_loads_independently() {
    let loader = load_project();
    let path = loader.resolve_path("res://scenes/player.tscn").unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let scene = PackedScene::from_tscn(&content).unwrap();
    assert_eq!(scene.node_count(), 2); // PlayerBody + Sprite
}
