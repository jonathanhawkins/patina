//! pat-6t3: Render benchmark fixtures and reporting.
//!
//! Times `SoftwareRenderer::render_frame` for representative 2D scenes at
//! three resolutions: 640×480, 1280×720, 1920×1080.
//!
//! Named with `bench_` prefix so they land in Tier 3 (full suite).
//! Tests always pass — run with `--nocapture` to see timing output.

use std::path::PathBuf;
use std::time::Instant;

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::capture_frame;
use gdscene::node2d::{get_local_transform, get_z_index, is_visible_in_tree};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Resolutions
// ---------------------------------------------------------------------------

const RESOLUTIONS: [(u32, u32, &str); 3] = [
    (640, 480, "640x480"),
    (1280, 720, "1280x720"),
    (1920, 1080, "1920x1080"),
];

const ITERATIONS: u32 = 10;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn read_scene_fixture(filename: &str) -> String {
    let path = engine_fixtures_dir().join("scenes").join(filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read scene fixture {}: {e}", path.display()))
}

fn load_scene(tscn_source: &str) -> SceneTree {
    let packed = PackedScene::from_tscn(tscn_source).expect("failed to parse .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("failed to instance scene");
    tree
}

/// Builds a Viewport from SceneTree nodes (the runtime render path).
fn build_viewport(tree: &SceneTree, width: u32, height: u32) -> Viewport {
    let mut viewport = Viewport::new(width, height, Color::rgb(0.08, 0.08, 0.1));
    let all_nodes = tree.all_nodes_in_tree_order();
    let mut canvas_id_counter: u64 = 1;

    for &node_id in &all_nodes {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let class = node.class_name();
        let is_2d = matches!(
            class,
            "Node2D"
                | "Sprite2D"
                | "Camera2D"
                | "CharacterBody2D"
                | "RigidBody2D"
                | "StaticBody2D"
                | "Area2D"
                | "CollisionShape2D"
        );
        if !is_2d {
            continue;
        }
        if !is_visible_in_tree(tree, node_id) {
            continue;
        }

        let cid = CanvasItemId(canvas_id_counter);
        canvas_id_counter += 1;

        let local_xform = get_local_transform(tree, node_id);
        let z = get_z_index(tree, node_id) as i32;

        let mut item = CanvasItem::new(cid);
        item.transform = local_xform;
        item.z_index = z;

        let color = match class {
            "Node2D" => Color::rgb(1.0, 0.75, 0.0),
            "Sprite2D" => Color::rgb(0.3, 0.5, 1.0),
            "Camera2D" => Color::rgb(0.2, 0.9, 0.3),
            "CharacterBody2D" => Color::rgb(0.3, 0.5, 1.0),
            "StaticBody2D" => Color::rgb(0.5, 0.5, 0.5),
            "CollisionShape2D" => Color::rgb(0.0, 0.85, 0.3),
            "Area2D" => Color::rgb(0.3, 0.5, 1.0),
            _ => Color::rgb(0.8, 0.8, 0.8),
        };

        let half = 4.0_f32;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-half, -half), Vector2::new(8.0, 8.0)),
            color,
            filled: true,
        });

        viewport.add_canvas_item(item);
    }

    viewport
}

/// Benchmark render_frame for a scene at all three resolutions.
fn bench_render_scene(scene_name: &str, filename: &str) {
    let source = read_scene_fixture(filename);
    let tree = load_scene(&source);
    let mut renderer = SoftwareRenderer::new();

    eprintln!("[bench-render] {scene_name}:");

    for &(w, h, label) in &RESOLUTIONS {
        let viewport = build_viewport(&tree, w, h);

        // Warmup.
        let _ = capture_frame(&mut renderer, &viewport);

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = capture_frame(&mut renderer, &viewport);
        }
        let total_ms = start.elapsed().as_secs_f64() * 1000.0;
        let per_frame_ms = total_ms / ITERATIONS as f64;
        let megapixels = (w as f64 * h as f64) / 1_000_000.0;
        let mp_per_sec = if per_frame_ms > 0.0 {
            megapixels / (per_frame_ms / 1000.0)
        } else {
            0.0
        };

        eprintln!(
            "  {label}: {ITERATIONS}x in {total_ms:.2}ms ({per_frame_ms:.3}ms/frame, {mp_per_sec:.1} MP/s)"
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark: render each fixture scene
// ---------------------------------------------------------------------------

#[test]
fn bench_render_space_shooter() {
    bench_render_scene("space_shooter", "space_shooter.tscn");
}

#[test]
fn bench_render_demo_2d() {
    bench_render_scene("demo_2d", "demo_2d.tscn");
}

#[test]
fn bench_render_hierarchy() {
    bench_render_scene("hierarchy", "hierarchy.tscn");
}

// ---------------------------------------------------------------------------
// Benchmark: synthetic stress — many canvas items at high resolution
// ---------------------------------------------------------------------------

#[test]
fn bench_render_stress_100_items() {
    let mut renderer = SoftwareRenderer::new();

    eprintln!("[bench-render] stress_100_items:");

    for &(w, h, label) in &RESOLUTIONS {
        let mut viewport = Viewport::new(w, h, Color::BLACK);

        // Create 100 canvas items spread across the viewport.
        for i in 0..100 {
            let cid = CanvasItemId(i + 1);
            let mut item = CanvasItem::new(cid);
            let x = ((i % 10) as f32) * (w as f32 / 10.0);
            let y = ((i / 10) as f32) * (h as f32 / 10.0);
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(
                    Vector2::new(x, y),
                    Vector2::new(w as f32 / 12.0, h as f32 / 12.0),
                ),
                color: Color::rgb(
                    (i % 3) as f32 / 2.0,
                    ((i + 1) % 3) as f32 / 2.0,
                    ((i + 2) % 3) as f32 / 2.0,
                ),
                filled: true,
            });
            viewport.add_canvas_item(item);
        }

        // Warmup.
        let _ = capture_frame(&mut renderer, &viewport);

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = capture_frame(&mut renderer, &viewport);
        }
        let total_ms = start.elapsed().as_secs_f64() * 1000.0;
        let per_frame_ms = total_ms / ITERATIONS as f64;
        let megapixels = (w as f64 * h as f64) / 1_000_000.0;
        let mp_per_sec = if per_frame_ms > 0.0 {
            megapixels / (per_frame_ms / 1000.0)
        } else {
            0.0
        };

        eprintln!(
            "  {label}: {ITERATIONS}x in {total_ms:.2}ms ({per_frame_ms:.3}ms/frame, {mp_per_sec:.1} MP/s)"
        );
    }
}

#[test]
fn bench_render_stress_500_items() {
    let mut renderer = SoftwareRenderer::new();

    eprintln!("[bench-render] stress_500_items:");

    for &(w, h, label) in &RESOLUTIONS {
        let mut viewport = Viewport::new(w, h, Color::BLACK);

        for i in 0..500u64 {
            let cid = CanvasItemId(i + 1);
            let mut item = CanvasItem::new(cid);
            let x = ((i % 25) as f32) * (w as f32 / 25.0);
            let y = ((i / 25) as f32) * (h as f32 / 20.0);
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(
                    Vector2::new(x, y),
                    Vector2::new(w as f32 / 30.0, h as f32 / 30.0),
                ),
                color: Color::rgb(
                    (i % 5) as f32 / 4.0,
                    ((i + 1) % 5) as f32 / 4.0,
                    ((i + 2) % 5) as f32 / 4.0,
                ),
                filled: true,
            });
            viewport.add_canvas_item(item);
        }

        let _ = capture_frame(&mut renderer, &viewport);

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = capture_frame(&mut renderer, &viewport);
        }
        let total_ms = start.elapsed().as_secs_f64() * 1000.0;
        let per_frame_ms = total_ms / ITERATIONS as f64;
        let megapixels = (w as f64 * h as f64) / 1_000_000.0;
        let mp_per_sec = if per_frame_ms > 0.0 {
            megapixels / (per_frame_ms / 1000.0)
        } else {
            0.0
        };

        eprintln!(
            "  {label}: {ITERATIONS}x in {total_ms:.2}ms ({per_frame_ms:.3}ms/frame, {mp_per_sec:.1} MP/s)"
        );
    }
}
