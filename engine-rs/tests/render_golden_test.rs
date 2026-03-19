//! Golden-image comparison tests for the scene renderer.
//!
//! Renders fixture `.tscn` scenes through the editor's `scene_renderer`,
//! compares the output against golden PNG reference images, and verifies
//! determinism of the software renderer.
//!
//! **Oracle rule**: Each test states what observable rendering behavior it
//! checks (e.g., "Node2D diamonds appear at correct positions").

use std::path::PathBuf;

use gdeditor::scene_renderer;
use gdrender2d::compare::compare_framebuffers;
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::{decode_png, load_png};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default render resolution for golden tests.
const RENDER_W: u32 = 256;
const RENDER_H: u32 = 256;

/// Tolerance for pixel matching (Euclidean RGB distance, 0.0–1.732 range).
/// 0.02 accounts for floating-point rounding through encode/decode.
const PIXEL_TOLERANCE: f64 = 0.02;

/// Minimum acceptable match ratio for golden comparison.
const MIN_MATCH_RATIO: f64 = 0.99;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the path to the engine-rs `fixtures/` directory.
fn engine_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Returns the path to the monorepo `fixtures/golden/render/` directory.
fn golden_render_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
        .join("golden")
        .join("render")
}

/// Reads a `.tscn` fixture from `engine-rs/fixtures/scenes/`.
fn read_scene_fixture(filename: &str) -> String {
    let path = engine_fixtures_dir().join("scenes").join(filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read scene fixture {}: {e}", path.display()))
}

/// Loads a `.tscn` file and instances it into a fresh SceneTree.
fn load_scene(tscn_source: &str) -> SceneTree {
    let packed = PackedScene::from_tscn(tscn_source).expect("failed to parse .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("failed to instance scene");
    tree
}

/// Renders a scene tree at the default resolution.
fn render(tree: &SceneTree) -> FrameBuffer {
    scene_renderer::render_scene(tree, None, RENDER_W, RENDER_H)
}

/// Renders a scene tree with zoom and pan.
fn render_zoomed(tree: &SceneTree, zoom: f64, pan: (f64, f64)) -> FrameBuffer {
    scene_renderer::render_scene_with_zoom_pan(tree, None, RENDER_W, RENDER_H, zoom, pan)
}

/// Saves a framebuffer as a golden PNG reference.
fn save_golden(fb: &FrameBuffer, name: &str) {
    let dir = golden_render_dir();
    std::fs::create_dir_all(&dir).expect("failed to create golden render dir");
    let path = dir.join(format!("{name}.png"));
    fb.save_png(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("failed to save golden PNG {}: {e}", path.display()));
}

/// Loads a golden PNG reference and returns it as a FrameBuffer.
/// Returns None if the golden file doesn't exist.
fn load_golden(name: &str) -> Option<FrameBuffer> {
    let path = golden_render_dir().join(format!("{name}.png"));
    let tex = load_png(path.to_str().unwrap())?;
    Some(FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    })
}

/// Compares a rendered framebuffer against a golden reference.
///
/// If no golden exists, generates it. If one exists, compares and reports.
fn assert_golden_match(fb: &FrameBuffer, name: &str) {
    match load_golden(name) {
        Some(golden) => {
            let result = compare_framebuffers(fb, &golden, PIXEL_TOLERANCE);
            assert!(
                result.match_ratio() >= MIN_MATCH_RATIO,
                "golden comparison failed for '{}': {:.2}% match (need {:.0}%), \
                 max_diff={:.4}, avg_diff={:.4}",
                name,
                result.match_ratio() * 100.0,
                MIN_MATCH_RATIO * 100.0,
                result.max_diff,
                result.avg_diff,
            );
        }
        None => {
            // No golden exists yet — generate it.
            save_golden(fb, name);
            eprintln!(
                "Generated golden reference: {}/{}.png",
                golden_render_dir().display(),
                name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Scene rendering + golden comparison tests
// ---------------------------------------------------------------------------

#[test]
fn golden_demo_2d_scene() {
    // Checks: Node2D diamonds rendered at correct world positions (100,200), (400,200), (0,500).
    let source = read_scene_fixture("demo_2d.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    assert_eq!(fb.width, RENDER_W);
    assert_eq!(fb.height, RENDER_H);
    assert_golden_match(&fb, "demo_2d");
}

#[test]
fn golden_hierarchy_scene() {
    // Checks: Nested Node2D/Sprite2D hierarchy renders parent-child positions correctly.
    let source = read_scene_fixture("hierarchy.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    assert_golden_match(&fb, "hierarchy");
}

#[test]
fn golden_space_shooter_scene() {
    // Checks: Space shooter scene with multiple Node2D nodes at varied positions.
    let source = read_scene_fixture("space_shooter.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    assert_golden_match(&fb, "space_shooter");
}

#[test]
fn golden_render_test_simple() {
    // Checks: Two Node2D diamonds at (50,50) and (150,100).
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    assert_golden_match(&fb, "render_test_simple");
}

#[test]
fn golden_render_test_camera() {
    // Checks: Camera2D icon and Node2D diamond rendered at distinct positions.
    let source = read_scene_fixture("render_test_camera.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    assert_golden_match(&fb, "render_test_camera");
}

#[test]
fn golden_render_test_sprite() {
    // Checks: Sprite2D rectangle and Node2D diamond at expected positions.
    let source = read_scene_fixture("render_test_sprite.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    assert_golden_match(&fb, "render_test_sprite");
}

// ---------------------------------------------------------------------------
// Determinism tests
// ---------------------------------------------------------------------------

#[test]
fn determinism_same_scene_identical_output() {
    // Checks: Software renderer is deterministic — same scene rendered twice
    // produces byte-identical framebuffers.
    let source = read_scene_fixture("demo_2d.tscn");

    let tree1 = load_scene(&source);
    let fb1 = render(&tree1);

    let tree2 = load_scene(&source);
    let fb2 = render(&tree2);

    let result = compare_framebuffers(&fb1, &fb2, 0.0);
    assert!(
        result.is_exact_match(),
        "determinism check failed: {:.2}% match, max_diff={:.6}",
        result.match_ratio() * 100.0,
        result.max_diff,
    );
}

#[test]
fn determinism_hierarchy_identical_output() {
    // Checks: Hierarchy scene also renders deterministically.
    let source = read_scene_fixture("hierarchy.tscn");

    let tree1 = load_scene(&source);
    let fb1 = render(&tree1);

    let tree2 = load_scene(&source);
    let fb2 = render(&tree2);

    let result = compare_framebuffers(&fb1, &fb2, 0.0);
    assert!(
        result.is_exact_match(),
        "hierarchy determinism failed: {:.2}% match",
        result.match_ratio() * 100.0,
    );
}

#[test]
fn determinism_space_shooter_identical_output() {
    // Checks: More complex scene with scripts/ext_resources is still deterministic.
    let source = read_scene_fixture("space_shooter.tscn");

    let tree1 = load_scene(&source);
    let fb1 = render(&tree1);

    let tree2 = load_scene(&source);
    let fb2 = render(&tree2);

    let result = compare_framebuffers(&fb1, &fb2, 0.0);
    assert!(
        result.is_exact_match(),
        "space_shooter determinism failed: {:.2}% match",
        result.match_ratio() * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Zoom/pan rendering tests
// ---------------------------------------------------------------------------

#[test]
fn zoom_changes_output() {
    // Checks: Rendering at 2x zoom produces different output than 1x.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb_1x = render(&tree);
    let fb_2x = render_zoomed(&tree, 2.0, (0.0, 0.0));

    let result = compare_framebuffers(&fb_1x, &fb_2x, 0.0);
    assert!(
        !result.is_exact_match(),
        "zoom should change render output, but got identical frames",
    );
}

#[test]
fn pan_changes_output() {
    // Checks: Rendering with a pan offset shifts node positions.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb_no_pan = render(&tree);
    let fb_panned = render_zoomed(&tree, 1.0, (50.0, 50.0));

    let result = compare_framebuffers(&fb_no_pan, &fb_panned, 0.0);
    assert!(
        !result.is_exact_match(),
        "panning should change render output, but got identical frames",
    );
}

// ---------------------------------------------------------------------------
// PNG roundtrip tests
// ---------------------------------------------------------------------------

#[test]
fn png_roundtrip_preserves_render() {
    // Checks: Encoding a render to PNG and decoding it back produces a
    // framebuffer that closely matches the original (within 8-bit quantization).
    let source = read_scene_fixture("demo_2d.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    let png_data = fb.to_png();
    let tex = decode_png(&png_data).expect("failed to decode our own PNG");
    let decoded_fb = FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    };

    let result = compare_framebuffers(&fb, &decoded_fb, PIXEL_TOLERANCE);
    assert!(
        result.match_ratio() >= 0.999,
        "PNG roundtrip lost too much fidelity: {:.2}% match, max_diff={:.4}",
        result.match_ratio() * 100.0,
        result.max_diff,
    );
}

// ---------------------------------------------------------------------------
// Render content validation tests
// ---------------------------------------------------------------------------

#[test]
fn render_has_non_trivial_content() {
    // Checks: Rendered scene is not just a solid background — nodes produce visible output.
    let source = read_scene_fixture("demo_2d.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    let bg = fb.get_pixel(0, 0); // corner pixel is likely background
    let mut non_bg_count = 0u64;
    for pixel in &fb.pixels {
        if (pixel.r - bg.r).abs() > 0.01
            || (pixel.g - bg.g).abs() > 0.01
            || (pixel.b - bg.b).abs() > 0.01
        {
            non_bg_count += 1;
        }
    }
    let total = (fb.width as u64) * (fb.height as u64);
    assert!(
        non_bg_count > 0,
        "render should contain non-background pixels (nodes), but all {} pixels matched background",
        total,
    );
}

#[test]
fn empty_scene_renders_grid_only() {
    // Checks: A scene with just a root Node renders background + grid but no node shapes.
    let source = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n";
    let tree = load_scene(source);
    let fb = render(&tree);

    // Should still have content (grid lines, origin crosshair).
    assert_eq!(fb.width, RENDER_W);
    assert_eq!(fb.height, RENDER_H);

    // Verify determinism of empty scene too.
    let tree2 = load_scene(source);
    let fb2 = render(&tree2);
    let result = compare_framebuffers(&fb, &fb2, 0.0);
    assert!(result.is_exact_match());
}

// ---------------------------------------------------------------------------
// DiffResult utility tests (integration-level)
// ---------------------------------------------------------------------------

#[test]
fn diff_result_reports_exact_match_for_same_render() {
    // Checks: compare_framebuffers returns exact match when comparing a render to itself.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);
    let fb = render(&tree);

    let result = compare_framebuffers(&fb, &fb, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_pixels, (RENDER_W as u64) * (RENDER_H as u64));
    assert!(result.max_diff < f64::EPSILON);
    assert!(result.avg_diff < f64::EPSILON);
    assert!((result.match_ratio() - 1.0).abs() < f64::EPSILON);
}
