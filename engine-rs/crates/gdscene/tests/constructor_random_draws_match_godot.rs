//! pat-y2ylj — building a node draws the global random numbers Godot's
//! constructor draws.
//!
//! `GPUParticles2D()` takes two numbers from the global RNG (the rendering
//! server's `Particles()` does `random_seed = Math::rand()`, then the node does
//! `set_seed(Math::rand())`); `CPUParticles2D()` takes one. Dodge the Creeps'
//! Player carries a `Trail` GPUParticles2D, so in a Godot run pinned to seed 42
//! the first mob's `randf()` is the THIRD number of the stream (0.829341…, the
//! first `progress_ratio` a pinned Godot capture records) — Patina drew the
//! first (0.114713…) and every mob spawned from the wrong numbers.
//!
//! The global RNG is process-wide; the tests here serialize on one lock.

use std::sync::Mutex;

use gdscene::packed_scene::{add_packed_scene_to_tree_from_res, PackedScene};
use gdscene::SceneTree;

static LOCK: Mutex<()> = Mutex::new(());

/// Godot 4.6.1, seed 42: the first `progress_ratio = randf()` of a pinned
/// capture of games/dodge_the_creeps (draw #3 of the stream).
const GODOT_FIRST_SPAWN_RATIO: f64 = 0.829341351985931;
/// Draw #1 of seed 42 — what Patina's first mob read without the draws.
const FIRST_DRAW: f64 = 0.114713475108;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/dodge_the_creeps")
        .canonicalize()
        .expect("games/dodge_the_creeps fixture")
}

#[test]
fn instancing_dodge_the_creeps_main_draws_what_godot_draws() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("main.tscn")).unwrap();
    let scene = PackedScene::from_tscn(&text).unwrap();

    // The runner's route: instance main.tscn with its sub-scenes.
    gdcore::rng::pin_seed(Some(42));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_from_res(&mut tree, root, &scene).expect("main.tscn instances");
    let first = gdcore::rng::randf();
    gdcore::rng::pin_seed(None);
    assert!(
        (first - GODOT_FIRST_SPAWN_RATIO).abs() < 1e-9,
        "after building main.tscn (one GPUParticles2D), the next randf() must be Godot's \
         {GODOT_FIRST_SPAWN_RATIO}, got {first} (draw #1 is {FIRST_DRAW})"
    );
}

#[test]
fn script_side_construction_draws_too_and_other_classes_draw_nothing() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdobject::class_db::register_all_classes();
    let mut stream = gdcore::rng::Rng::with_seed(7);
    let expected: Vec<u32> = (0..8).map(|_| stream.randi()).collect();

    gdcore::rng::seed(7);
    let mut tree = SceneTree::new();
    tree.create_node("Node2D", "Plain");
    tree.create_node("RigidBody2D", "Body");
    assert_eq!(gdcore::rng::randi(), expected[0], "a Node2D / RigidBody2D draws nothing");

    gdcore::rng::seed(7);
    tree.create_node("GPUParticles2D", "Gpu");
    assert_eq!(gdcore::rng::randi(), expected[2], "GPUParticles2D() draws two");

    gdcore::rng::seed(7);
    tree.create_node("CPUParticles2D", "Cpu");
    assert_eq!(gdcore::rng::randi(), expected[1], "CPUParticles2D() draws one");

    gdcore::rng::seed(7);
    tree.create_node("GPUParticles3D", "Gpu3");
    tree.create_node("CPUParticles3D", "Cpu3");
    assert_eq!(gdcore::rng::randi(), expected[3], "GPUParticles3D() two + CPUParticles3D() one");

    // A host building in pieces suppresses them and draws once, in order.
    gdcore::rng::seed(7);
    let gpu = {
        let _quiet = gdscene::construction_randomness::suppress();
        tree.create_node("GPUParticles2D", "Quiet")
    };
    assert_eq!(gdcore::rng::randi(), expected[0], "no draws while suppressed");
    gdcore::rng::seed(7);
    assert_eq!(gdscene::construction_randomness::draw_for_subtree(&tree, gpu), 2);
    assert_eq!(gdcore::rng::randi(), expected[2]);
}
