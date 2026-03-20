//! Headless runtime benchmark baselines (pat-hvv).
//!
//! Measures wall-clock time for core engine operations using `std::time::Instant`.
//! These tests always pass — they print timing results for baseline tracking.
//!
//! Named with `bench_` prefix so they land in Tier 3 (full suite).

use std::path::PathBuf;
use std::time::Instant;

use gdcore::math::{Color, Vector2, Vector3};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscript_interop::{tokenize, Parser};
use gdvariant::serialize::{from_json, to_json};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn monorepo_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

fn read_tscn(rel_path: &str) -> String {
    // Try engine-rs/fixtures first, then monorepo fixtures.
    let engine = engine_fixtures().join(rel_path);
    if engine.exists() {
        return std::fs::read_to_string(&engine).unwrap();
    }
    let mono = monorepo_fixtures().join(rel_path);
    std::fs::read_to_string(&mono).unwrap_or_else(|e| panic!("fixture not found: {rel_path}: {e}"))
}

fn read_script(rel_path: &str) -> String {
    let engine = engine_fixtures().join(rel_path);
    if engine.exists() {
        return std::fs::read_to_string(&engine).unwrap();
    }
    let mono = monorepo_fixtures().join(rel_path);
    std::fs::read_to_string(&mono).unwrap_or_else(|e| panic!("script not found: {rel_path}: {e}"))
}

fn load_scene(tscn_source: &str) -> SceneTree {
    let packed = PackedScene::from_tscn(tscn_source).expect("parse .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("instance scene");
    tree
}

/// Run `iterations` repetitions of `f`, return (total_ms, per_iter_ms).
fn measure<F: FnMut()>(iterations: u32, mut f: F) -> (f64, f64) {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let total = start.elapsed().as_secs_f64() * 1000.0;
    (total, total / iterations as f64)
}

// ---------------------------------------------------------------------------
// Benchmark: step 1000 frames on fixture scenes
// ---------------------------------------------------------------------------

const FRAME_DELTA: f64 = 1.0 / 60.0;
const FRAME_COUNT: u64 = 1000;

fn bench_step_frames(scene_name: &str, tscn_rel: &str) {
    let source = read_tscn(tscn_rel);

    // Load + instance
    let tree = load_scene(&source);
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let start = Instant::now();
    ml.run_frames(FRAME_COUNT, FRAME_DELTA);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    eprintln!(
        "[bench] {scene_name}: {FRAME_COUNT} frames in {elapsed_ms:.2}ms ({:.3}ms/frame)",
        elapsed_ms / FRAME_COUNT as f64,
    );
}

#[test]
fn bench_step_1000_frames_space_shooter() {
    bench_step_frames("space_shooter", "scenes/space_shooter.tscn");
}

#[test]
fn bench_step_1000_frames_platformer() {
    bench_step_frames("platformer", "scenes/platformer.tscn");
}

#[test]
fn bench_step_1000_frames_physics_playground() {
    bench_step_frames("physics_playground", "scenes/physics_playground.tscn");
}

#[test]
fn bench_step_1000_frames_hierarchy() {
    bench_step_frames("hierarchy", "scenes/hierarchy.tscn");
}

#[test]
fn bench_step_1000_frames_minimal() {
    bench_step_frames("minimal", "scenes/minimal.tscn");
}

// ---------------------------------------------------------------------------
// Benchmark: .tscn load + instance time
// ---------------------------------------------------------------------------

fn bench_load_instance(scene_name: &str, tscn_rel: &str) {
    let source = read_tscn(tscn_rel);
    let iterations = 100;

    let (total_ms, per_iter_ms) = measure(iterations, || {
        let _ = load_scene(&source);
    });

    eprintln!(
        "[bench] load+instance {scene_name}: {iterations}x in {total_ms:.2}ms ({per_iter_ms:.3}ms/iter)",
    );
}

#[test]
fn bench_load_instance_space_shooter() {
    bench_load_instance("space_shooter", "scenes/space_shooter.tscn");
}

#[test]
fn bench_load_instance_platformer() {
    bench_load_instance("platformer", "scenes/platformer.tscn");
}

#[test]
fn bench_load_instance_physics_playground() {
    bench_load_instance("physics_playground", "scenes/physics_playground.tscn");
}

#[test]
fn bench_load_instance_hierarchy() {
    bench_load_instance("hierarchy", "scenes/hierarchy.tscn");
}

#[test]
fn bench_load_instance_minimal() {
    bench_load_instance("minimal", "scenes/minimal.tscn");
}

// ---------------------------------------------------------------------------
// Benchmark: 100 physics frames
// ---------------------------------------------------------------------------

const PHYSICS_FRAMES: u64 = 100;

fn bench_physics(scene_name: &str, tscn_rel: &str) {
    let source = read_tscn(tscn_rel);
    let tree = load_scene(&source);
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let start = Instant::now();
    ml.run_frames(PHYSICS_FRAMES, FRAME_DELTA);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    eprintln!(
        "[bench] physics {scene_name}: {PHYSICS_FRAMES} frames in {elapsed_ms:.2}ms ({:.3}ms/frame)",
        elapsed_ms / PHYSICS_FRAMES as f64,
    );
}

#[test]
fn bench_physics_100_frames_physics_playground() {
    bench_physics("physics_playground", "scenes/physics_playground.tscn");
}

#[test]
fn bench_physics_100_frames_platformer() {
    bench_physics("platformer", "scenes/platformer.tscn");
}

#[test]
fn bench_physics_100_frames_space_shooter() {
    bench_physics("space_shooter", "scenes/space_shooter.tscn");
}

// ---------------------------------------------------------------------------
// Benchmark: .gd script parsing
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Benchmark: Variant JSON roundtrip
// ---------------------------------------------------------------------------

#[test]
fn bench_variant_roundtrip() {
    let variants: Vec<Variant> = vec![
        Variant::Nil,
        Variant::Bool(true),
        Variant::Int(42),
        Variant::Float(3.14),
        Variant::String("hello world".into()),
        Variant::Vector2(Vector2::new(1.0, 2.0)),
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
        Variant::Color(Color::new(1.0, 0.5, 0.25, 0.8)),
        Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)]),
    ];
    let iterations = 100;

    let (total_ms, per_iter_ms) = measure(iterations, || {
        for v in &variants {
            let json = to_json(v);
            let _ = from_json(&json);
        }
    });

    eprintln!(
        "[bench] variant_roundtrip: {iterations}x ({} variants each) in {total_ms:.2}ms ({per_iter_ms:.3}ms/iter)",
        variants.len()
    );
}

// ---------------------------------------------------------------------------
// Benchmark: .gd script parsing
// ---------------------------------------------------------------------------

fn bench_parse_script(script_name: &str, script_rel: &str) {
    let source = read_script(script_rel);
    let iterations = 100;

    let (total_ms, per_iter_ms) = measure(iterations, || {
        let tokens = tokenize(&source).expect("tokenize");
        let mut parser = Parser::new(tokens, &source);
        let _ = parser.parse_script().expect("parse");
    });

    eprintln!(
        "[bench] parse {script_name}: {iterations}x in {total_ms:.2}ms ({per_iter_ms:.3}ms/iter)",
    );
}

#[test]
fn bench_parse_script_player() {
    bench_parse_script("player.gd", "scripts/player.gd");
}

#[test]
fn bench_parse_script_enemy_spawner() {
    bench_parse_script("enemy_spawner.gd", "scripts/enemy_spawner.gd");
}

#[test]
fn bench_parse_script_test_move() {
    bench_parse_script("test_move.gd", "scripts/test_move.gd");
}

#[test]
fn bench_parse_script_test_variables() {
    bench_parse_script("test_variables.gd", "scripts/test_variables.gd");
}

#[test]
fn bench_parse_script_test_movement() {
    bench_parse_script("test_movement.gd", "scripts/test_movement.gd");
}
