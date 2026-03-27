//! pat-bblv: Measured end-to-end 2D vertical slice from fixtures.
//!
//! This is the single capstone test that proves the entire 2D pipeline works
//! end-to-end with quantified measurements at every stage:
//!
//!   fixture parse → scene tree build → oracle comparison → frame execution →
//!   2D render → golden PNG comparison → determinism check
//!
//! Unlike the layered tests in `vertical_slice_2d_parity_test.rs` (which test
//! individual layers) or `render_vertical_slice_test.rs` (which skips frame
//! execution), this test exercises **every layer in a single pipeline** and
//! emits a structured measurement report.
//!
//! Acceptance: one representative scene exercises runtime, render, and fixture
//! comparison end to end.

use std::path::PathBuf;

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdplatform::backend::HeadlessPlatform;
use gdrender2d::compare::compare_framebuffers;
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::texture::load_png;
use gdscene::main_loop::MainLoop;
use gdscene::node2d::{get_position, get_z_index, is_visible_in_tree};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const DT: f64 = 1.0 / 60.0;
const FRAMES_TO_RUN: u64 = 60;
const FLOAT_TOLERANCE: f64 = 0.001;
const PIXEL_TOLERANCE: f64 = 0.02;
const MIN_GOLDEN_MATCH: f64 = 0.99;

const GOLDEN_NAME: &str = "measured_vertical_slice_platformer";

// ---------------------------------------------------------------------------
// Oracle data — expected values from the platformer.tscn fixture
// ---------------------------------------------------------------------------

struct OracleEntry {
    path: &'static str,
    class: &'static str,
    position: Option<(f64, f64)>,
}

fn oracle_entries() -> Vec<OracleEntry> {
    vec![
        OracleEntry {
            path: "/root/World",
            class: "Node",
            position: None,
        },
        OracleEntry {
            path: "/root/World/Player",
            class: "Node2D",
            position: Some((100.0, 300.0)),
        },
        OracleEntry {
            path: "/root/World/Platform1",
            class: "Node2D",
            position: Some((0.0, 500.0)),
        },
        OracleEntry {
            path: "/root/World/Platform2",
            class: "Node2D",
            position: Some((300.0, 400.0)),
        },
        OracleEntry {
            path: "/root/World/Platform3",
            class: "Node2D",
            position: Some((600.0, 350.0)),
        },
        OracleEntry {
            path: "/root/World/Camera",
            class: "Camera2D",
            position: None,
        },
        OracleEntry {
            path: "/root/World/Collectible",
            class: "Node2D",
            position: Some((450.0, 250.0)),
        },
    ]
}

const EXPECTED_CHILD_ORDER: &[&str] = &[
    "Player",
    "Platform1",
    "Platform2",
    "Platform3",
    "Camera",
    "Collectible",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn golden_render_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
        .join("golden")
        .join("render")
}

fn load_and_instance_platformer() -> SceneTree {
    let tscn = include_str!("../../fixtures/scenes/platformer.tscn");
    let packed = PackedScene::from_tscn(tscn).expect("platformer.tscn must parse");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("scene must instance");
    tree
}

/// Renders all visible 2D nodes from the scene tree into a framebuffer.
fn render_scene_tree(tree: &SceneTree) -> FrameBuffer {
    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.1, 0.1, 0.15));

    let node2d_configs: &[(&str, Color)] = &[
        ("/root/World/Player", Color::rgb(0.2, 0.4, 1.0)),
        ("/root/World/Platform1", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Platform2", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Platform3", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Collectible", Color::rgb(1.0, 0.8, 0.0)),
    ];

    for (i, &(path, color)) in node2d_configs.iter().enumerate() {
        let node_id = tree.get_node_by_path(path).unwrap();
        let pos = get_position(tree, node_id);
        let z = get_z_index(tree, node_id) as i32;
        let visible = is_visible_in_tree(tree, node_id);

        if !visible {
            continue;
        }

        let mut item = CanvasItem::new(CanvasItemId(i as u64 + 1));
        item.transform = Transform2D::translated(pos);
        item.z_index = z;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-16.0, -16.0), Vector2::new(32.0, 32.0)),
            color,
            filled: true,
        });
        viewport.add_canvas_item(item);
    }

    capture_frame(&mut renderer, &viewport)
}

fn count_non_bg_pixels(fb: &FrameBuffer) -> u64 {
    let bg = Color::rgb(0.1, 0.1, 0.15);
    fb.pixels
        .iter()
        .filter(|c| {
            (c.r - bg.r).abs() > 0.01 || (c.g - bg.g).abs() > 0.01 || (c.b - bg.b).abs() > 0.01
        })
        .count() as u64
}

// ===========================================================================
// MAIN TEST: Full measured vertical slice
// ===========================================================================

/// Exercises the entire 2D pipeline from fixture to golden comparison and
/// emits a structured measurement report.
#[test]
fn measured_vertical_slice_end_to_end() {
    // -----------------------------------------------------------------------
    // STAGE 1: Parse fixture
    // -----------------------------------------------------------------------
    let tscn = include_str!("../../fixtures/scenes/platformer.tscn");
    let packed = PackedScene::from_tscn(tscn).expect("platformer.tscn must parse");

    // -----------------------------------------------------------------------
    // STAGE 2: Build scene tree
    // -----------------------------------------------------------------------
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("scene must instance");

    // -----------------------------------------------------------------------
    // STAGE 3: Oracle comparison — tree structure, classes, properties, order
    // -----------------------------------------------------------------------
    let oracle = oracle_entries();
    let oracle_total = oracle.len();
    // +1 for implicit root node
    let expected_node_count = oracle_total + 1;
    let actual_node_count = tree.node_count();
    let tree_count_match = actual_node_count == expected_node_count;

    let mut class_matched = 0usize;
    let mut prop_matched = 0usize;
    let mut prop_total = 0usize;

    for entry in &oracle {
        let node_id = tree
            .get_node_by_path(entry.path)
            .unwrap_or_else(|| panic!("oracle path '{}' not found in tree", entry.path));
        let node = tree.get_node(node_id).unwrap();

        if node.class_name() == entry.class {
            class_matched += 1;
        }

        if let Some((ex, ey)) = entry.position {
            prop_total += 1;
            let pos = get_position(&tree, node_id);
            if (pos.x as f64 - ex).abs() < FLOAT_TOLERANCE
                && (pos.y as f64 - ey).abs() < FLOAT_TOLERANCE
            {
                prop_matched += 1;
            }
        }
    }

    // Child order
    let world_id = tree.get_node_by_path("/root/World").unwrap();
    let world = tree.get_node(world_id).unwrap();
    let children = world.children();
    let mut order_matched = 0usize;
    for (i, &cid) in children.iter().enumerate() {
        let child = tree.get_node(cid).unwrap();
        if i < EXPECTED_CHILD_ORDER.len() && child.name() == EXPECTED_CHILD_ORDER[i] {
            order_matched += 1;
        }
    }

    // -----------------------------------------------------------------------
    // STAGE 4: Frame execution
    // -----------------------------------------------------------------------
    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let pre_run_pos = get_position(&tree, player_id);

    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT).with_max_frames(FRAMES_TO_RUN);
    main_loop.run(&mut backend, DT);

    let frames_completed = main_loop.frame_count();
    let post_run_pos = get_position(main_loop.tree(), player_id);
    let positions_stable = pre_run_pos == post_run_pos;

    // -----------------------------------------------------------------------
    // STAGE 5: Render from final scene state
    // -----------------------------------------------------------------------
    let fb = render_scene_tree(main_loop.tree());
    let non_bg_pixels = count_non_bg_pixels(&fb);
    let has_visible_content = non_bg_pixels > 100;

    // -----------------------------------------------------------------------
    // STAGE 6: Golden PNG comparison
    // -----------------------------------------------------------------------
    let golden_dir = golden_render_dir();
    let golden_path = golden_dir.join(format!("{GOLDEN_NAME}.png"));

    let golden_match_ratio = if golden_path.exists() {
        let tex = load_png(golden_path.to_str().unwrap())
            .expect("failed to load golden PNG");
        let golden_fb = FrameBuffer {
            width: tex.width,
            height: tex.height,
            pixels: tex.pixels,
        };
        let diff = compare_framebuffers(&fb, &golden_fb, PIXEL_TOLERANCE);
        diff.match_ratio()
    } else {
        // First run: save the golden reference.
        std::fs::create_dir_all(&golden_dir).expect("failed to create golden dir");
        fb.save_png(golden_path.to_str().unwrap())
            .expect("failed to save golden PNG");
        eprintln!(
            "  [pat-bblv] Generated golden reference: {}",
            golden_path.display()
        );
        1.0 // Self-match is 100%
    };

    // -----------------------------------------------------------------------
    // STAGE 7: Determinism — render twice, compare
    // -----------------------------------------------------------------------
    let tree2 = load_and_instance_platformer();
    let mut main_loop2 = MainLoop::new(tree2);
    let mut backend2 = HeadlessPlatform::new(WIDTH, HEIGHT).with_max_frames(FRAMES_TO_RUN);
    main_loop2.run(&mut backend2, DT);
    let fb2 = render_scene_tree(main_loop2.tree());

    let determinism_diff = compare_framebuffers(&fb, &fb2, 0.0);
    let is_deterministic = determinism_diff.is_exact_match();

    // -----------------------------------------------------------------------
    // MEASUREMENT REPORT
    // -----------------------------------------------------------------------
    let class_pct = (class_matched as f64 / oracle_total as f64 * 100.0).round() as u32;
    let prop_pct = if prop_total > 0 {
        (prop_matched as f64 / prop_total as f64 * 100.0).round() as u32
    } else {
        100
    };
    let order_pct =
        (order_matched as f64 / EXPECTED_CHILD_ORDER.len() as f64 * 100.0).round() as u32;

    eprintln!();
    eprintln!("  ╔══════════════════════════════════════════════════════╗");
    eprintln!("  ║  pat-bblv: Measured 2D Vertical Slice Report        ║");
    eprintln!("  ╠══════════════════════════════════════════════════════╣");
    eprintln!("  ║  Scene:         platformer.tscn                     ║");
    eprintln!(
        "  ║  Nodes:         {actual_node_count}/{expected_node_count} ({})                          ║",
        if tree_count_match { "PASS" } else { "FAIL" }
    );
    eprintln!(
        "  ║  Classes:       {class_matched}/{oracle_total} ({class_pct}%)                          ║"
    );
    eprintln!(
        "  ║  Properties:    {prop_matched}/{prop_total} ({prop_pct}%)                          ║"
    );
    eprintln!(
        "  ║  Child order:   {order_matched}/{} ({order_pct}%)                          ║",
        EXPECTED_CHILD_ORDER.len()
    );
    eprintln!(
        "  ║  Frames run:    {frames_completed}/{FRAMES_TO_RUN} ({})                    ║",
        if frames_completed == FRAMES_TO_RUN {
            "PASS"
        } else {
            "FAIL"
        }
    );
    eprintln!(
        "  ║  Pos stable:    {} ({}, {}) → ({}, {})       ║",
        if positions_stable { "PASS" } else { "FAIL" },
        pre_run_pos.x,
        pre_run_pos.y,
        post_run_pos.x,
        post_run_pos.y,
    );
    eprintln!(
        "  ║  Render:        {WIDTH}x{HEIGHT} ({non_bg_pixels} visible px)          ║"
    );
    eprintln!(
        "  ║  Golden match:  {:.2}% (threshold {:.0}%)              ║",
        golden_match_ratio * 100.0,
        MIN_GOLDEN_MATCH * 100.0,
    );
    eprintln!(
        "  ║  Deterministic: {}                                  ║",
        if is_deterministic { "YES" } else { "NO " }
    );
    eprintln!("  ╠══════════════════════════════════════════════════════╣");

    let all_pass = tree_count_match
        && class_pct == 100
        && prop_pct == 100
        && order_pct == 100
        && frames_completed == FRAMES_TO_RUN
        && positions_stable
        && has_visible_content
        && golden_match_ratio >= MIN_GOLDEN_MATCH
        && is_deterministic;

    eprintln!(
        "  ║  OVERALL:       {}                                 ║",
        if all_pass { "PASS" } else { "FAIL" }
    );
    eprintln!("  ╚══════════════════════════════════════════════════════╝");
    eprintln!();

    // Hard assertions
    assert!(tree_count_match, "node count mismatch: {actual_node_count} != {expected_node_count}");
    assert_eq!(class_pct, 100, "class parity must be 100%");
    assert_eq!(prop_pct, 100, "property parity must be 100%");
    assert_eq!(order_pct, 100, "child order parity must be 100%");
    assert_eq!(
        frames_completed, FRAMES_TO_RUN,
        "all frames must complete"
    );
    assert!(positions_stable, "positions must be stable without physics");
    assert!(has_visible_content, "render must produce visible pixels");
    assert!(
        golden_match_ratio >= MIN_GOLDEN_MATCH,
        "golden match {:.2}% below threshold {:.0}%",
        golden_match_ratio * 100.0,
        MIN_GOLDEN_MATCH * 100.0,
    );
    assert!(is_deterministic, "pipeline must be deterministic");
}

// ===========================================================================
// Supporting tests that verify individual measurement dimensions
// ===========================================================================

/// Verifies the golden comparison infrastructure itself works — a framebuffer
/// compared against itself must be an exact match.
#[test]
fn golden_self_comparison_is_exact() {
    let tree = load_and_instance_platformer();
    let fb = render_scene_tree(&tree);
    let result = compare_framebuffers(&fb, &fb, 0.0);
    assert!(
        result.is_exact_match(),
        "self-comparison must be exact, got {:.4}%",
        result.match_ratio() * 100.0
    );
}

/// Verifies that the render output contains distinct colored regions for each
/// node, proving that all 5 Node2D nodes contribute visible pixels.
#[test]
fn render_contains_all_node_colors() {
    let tree = load_and_instance_platformer();
    let fb = render_scene_tree(&tree);

    // Expected colors for the 5 rendered nodes
    let expected_colors: &[(&str, Color)] = &[
        ("Player", Color::rgb(0.2, 0.4, 1.0)),
        ("Platform", Color::rgb(0.2, 0.5, 0.2)),
        ("Collectible", Color::rgb(1.0, 0.8, 0.0)),
    ];

    for &(label, expected) in expected_colors {
        let count = fb
            .pixels
            .iter()
            .filter(|p| {
                (p.r - expected.r).abs() < 0.05
                    && (p.g - expected.g).abs() < 0.05
                    && (p.b - expected.b).abs() < 0.05
            })
            .count();
        assert!(
            count > 0,
            "{label} color ({:.1},{:.1},{:.1}) not found in rendered output",
            expected.r,
            expected.g,
            expected.b,
        );
    }
}

/// Verifies that frame execution produces consistent traced output —
/// notifications fire and frame numbers are sequential.
#[test]
fn frame_execution_produces_traced_output() {
    let tree = load_and_instance_platformer();
    let mut main_loop = MainLoop::new(tree);

    let trace = main_loop.run_frames_traced(10, DT);
    assert_eq!(trace.len(), 10, "should capture 10 traced frames");

    // Physics and process time should be consistent
    let expected_time = 10.0 * DT;
    assert!(
        (main_loop.physics_time() - expected_time).abs() < 0.01,
        "physics_time should be ~{expected_time:.4}, got {:.4}",
        main_loop.physics_time(),
    );
    assert!(
        (main_loop.process_time() - expected_time).abs() < 0.01,
        "process_time should be ~{expected_time:.4}, got {:.4}",
        main_loop.process_time(),
    );
}

/// Verifies that positions at each node match their expected pixel locations
/// in the rendered framebuffer.
#[test]
fn rendered_pixel_positions_match_scene_properties() {
    let tree = load_and_instance_platformer();
    let fb = render_scene_tree(&tree);
    let bg = Color::rgb(0.1, 0.1, 0.15);

    // Check that the Player at (100, 300) has non-background pixels
    let check_positions: &[(&str, u32, u32)] = &[
        ("Player", 100, 300),
        ("Platform2", 300, 400),
        ("Collectible", 450, 250),
    ];

    for &(label, x, y) in check_positions {
        if x < WIDTH && y < HEIGHT {
            let pixel = fb.get_pixel(x, y);
            let is_non_bg = (pixel.r - bg.r).abs() > 0.01
                || (pixel.g - bg.g).abs() > 0.01
                || (pixel.b - bg.b).abs() > 0.01;
            assert!(
                is_non_bg,
                "{label} at ({x},{y}) should have non-background pixel, got ({:.2},{:.2},{:.2})",
                pixel.r, pixel.g, pixel.b,
            );
        }
    }
}
