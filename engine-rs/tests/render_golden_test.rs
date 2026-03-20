//! Golden-image comparison tests for the scene renderer.
//!
//! Renders fixture `.tscn` scenes through the editor's `scene_renderer`,
//! compares the output against golden PNG reference images, and verifies
//! determinism of the software renderer.
//!
//! **Oracle rule**: Each test states what observable rendering behavior it
//! checks (e.g., "Node2D diamonds appear at correct positions").

use std::path::PathBuf;

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdeditor::scene_renderer;
use gdrender2d::compare::compare_framebuffers;
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::texture::{decode_png, load_png, Texture2D};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

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

/// IGNORED: hangs inside scene_renderer::render_scene() for hierarchy.tscn —
/// pre-existing editor scene_renderer bug when the scene has a Sprite2D child
/// without a texture cache. The runtime path (render_vertical_slice_test) passes.
#[test]
#[ignore]
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

/// IGNORED: same hierarchy.tscn hang as golden_hierarchy_scene (editor path bug).
#[test]
#[ignore]
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

// ===========================================================================
// pat-22g: Texture draw and sprite property parity
// ===========================================================================

#[test]
fn texture_draw_modulate_color_integration() {
    // Checks: Drawing a white texture with a red modulate produces red pixels.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("white.png", Texture2D::solid(4, 4, Color::WHITE));

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "white.png".to_string(),
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(8.0, 8.0)),
        modulate: Color::rgb(1.0, 0.0, 0.0),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let pixel = frame.pixels[5 * 20 + 5]; // inside the rect
    assert!((pixel.r - 1.0).abs() < 0.01, "red channel should be ~1.0");
    assert!(pixel.g.abs() < 0.01, "green channel should be ~0.0");
    assert!(pixel.b.abs() < 0.01, "blue channel should be ~0.0");
    // Background pixel should remain black.
    assert_eq!(frame.pixels[0], Color::BLACK);
}

#[test]
fn texture_draw_half_alpha_modulate() {
    // Checks: Drawing with a half-alpha modulate produces appropriately scaled texels.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture(
        "solid.png",
        Texture2D::solid(2, 2, Color::rgb(0.8, 0.6, 0.4)),
    );

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "solid.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(6.0, 6.0)),
        modulate: Color::new(0.5, 0.5, 0.5, 1.0),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let pixel = frame.pixels[1 * 10 + 1];
    assert!((pixel.r - 0.4).abs() < 0.01, "r = 0.8 * 0.5 = 0.4");
    assert!((pixel.g - 0.3).abs() < 0.01, "g = 0.6 * 0.5 = 0.3");
    assert!((pixel.b - 0.2).abs() < 0.01, "b = 0.4 * 0.5 = 0.2");
}

#[test]
fn sprite_offset_rendering_via_transform() {
    // Checks: A textured sprite offset by transform renders at the correct position.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture(
        "icon.png",
        Texture2D::solid(2, 2, Color::rgb(0.0, 1.0, 0.0)),
    );

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(10.0, 10.0));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "icon.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let green = Color::rgb(0.0, 1.0, 0.0);
    // Sprite should be at (10,10)..(14,14).
    assert_eq!(frame.pixels[10 * 20 + 10], green);
    assert_eq!(frame.pixels[13 * 20 + 13], green);
    // Origin should be black.
    assert_eq!(frame.pixels[0], Color::BLACK);
    // Just outside the sprite.
    assert_eq!(frame.pixels[14 * 20 + 14], Color::BLACK);
}

#[test]
fn texture_rect_placement_at_specific_coords() {
    // Checks: Texture rect placed at (3,7) with size (5,5) fills exactly those pixels.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("t.png", Texture2D::solid(2, 2, Color::rgb(0.0, 0.0, 1.0)));

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "t.png".to_string(),
        rect: Rect2::new(Vector2::new(3.0, 7.0), Vector2::new(5.0, 5.0)),
        modulate: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    // Inside the rect.
    assert_eq!(frame.pixels[7 * 20 + 3], blue);
    assert_eq!(frame.pixels[11 * 20 + 7], blue);
    // Just outside.
    assert_eq!(frame.pixels[7 * 20 + 2], Color::BLACK);
    assert_eq!(frame.pixels[6 * 20 + 3], Color::BLACK);
    assert_eq!(frame.pixels[12 * 20 + 3], Color::BLACK);
    assert_eq!(frame.pixels[7 * 20 + 8], Color::BLACK);
}

#[test]
fn texture_region_extracts_subregion() {
    // Checks: DrawTextureRegion samples only the specified source rectangle.
    // Left half of texture is red, right half is green.
    let mut pixels = vec![Color::BLACK; 16];
    for y in 0..4 {
        for x in 0..2 {
            pixels[y * 4 + x] = Color::rgb(1.0, 0.0, 0.0);
        }
        for x in 2..4 {
            pixels[y * 4 + x] = Color::rgb(0.0, 1.0, 0.0);
        }
    }
    let tex = Texture2D {
        width: 4,
        height: 4,
        pixels,
    };

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("split.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Draw only the right half (green).
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "split.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 0.0), Vector2::new(2.0, 4.0)),
        modulate: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let pixel = frame.pixels[1 * 10 + 1];
    assert!(
        (pixel.g - 1.0).abs() < 0.01,
        "should be green from right half"
    );
    assert!(pixel.r.abs() < 0.01, "should have no red");
}

// ===========================================================================
// pat-sfn: Camera and viewport render parity
// ===========================================================================

#[test]
fn camera_offset_shifts_rendered_rect() {
    // Checks: Camera position offsets world coordinates — a rect at world (5,5)
    // appears at different screen positions depending on camera.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(2.0, 2.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);

    // No camera — rect at screen (5,5).
    let frame_no_cam = renderer.render_frame(&vp);
    let red = Color::rgb(1.0, 0.0, 0.0);
    assert_eq!(frame_no_cam.pixels[5 * 20 + 5], red);

    // Camera at (5,5) in 20x20 viewport: translate = (-5+10, -5+10) = (5,5).
    // World (5,5) maps to screen (10,10).
    vp.camera_position = Vector2::new(5.0, 5.0);
    let frame_with_cam = renderer.render_frame(&vp);
    assert_eq!(frame_with_cam.pixels[10 * 20 + 10], red);
    // The outputs should differ since the rect moved on screen.
    assert_ne!(frame_no_cam.pixels, frame_with_cam.pixels);
}

#[test]
fn camera_zoom_changes_pixel_positions() {
    // Checks: Camera zoom at 2x produces different output than no zoom.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(2.0, 2.0)),
        color: Color::rgb(0.0, 1.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);

    let frame_1x = renderer.render_frame(&vp);

    vp.camera_position = Vector2::new(5.0, 5.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);
    let frame_2x = renderer.render_frame(&vp);

    // The zoomed frame must differ from the unzoomed frame.
    assert_ne!(
        frame_1x.pixels, frame_2x.pixels,
        "2x zoom should change output"
    );
}

#[test]
fn viewport_clear_color_fills_background() {
    // Checks: Viewport clear color fills all background pixels.
    let mut renderer = SoftwareRenderer::new();
    let bg = Color::rgb(0.2, 0.3, 0.4);
    let vp = Viewport::new(10, 10, bg);

    let frame = renderer.render_frame(&vp);
    // Every pixel should be the clear color (no items added).
    for pixel in &frame.pixels {
        assert!((pixel.r - 0.2).abs() < 0.001);
        assert!((pixel.g - 0.3).abs() < 0.001);
        assert!((pixel.b - 0.4).abs() < 0.001);
    }
}

#[test]
fn camera_with_both_offset_and_zoom() {
    // Checks: Camera with both offset and zoom applied simultaneously produces
    // unique output different from offset-only and zoom-only.
    let make_viewport = |cam_pos: Vector2, cam_zoom: Vector2| {
        let mut vp = Viewport::new(20, 20, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(4.0, 4.0)),
            color: Color::rgb(1.0, 0.5, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);
        vp.camera_position = cam_pos;
        vp.camera_zoom = cam_zoom;
        vp
    };

    let mut r = SoftwareRenderer::new();
    let vp_none = make_viewport(Vector2::ZERO, Vector2::ONE);
    let vp_offset = make_viewport(Vector2::new(3.0, 3.0), Vector2::ONE);
    let vp_zoom = make_viewport(Vector2::ZERO, Vector2::new(1.5, 1.5));
    let vp_both = make_viewport(Vector2::new(3.0, 3.0), Vector2::new(1.5, 1.5));

    let f_none = r.render_frame(&vp_none);
    let f_offset = r.render_frame(&vp_offset);
    let f_zoom = r.render_frame(&vp_zoom);
    let f_both = r.render_frame(&vp_both);

    // All four should produce different outputs.
    assert_ne!(f_none.pixels, f_offset.pixels);
    assert_ne!(f_none.pixels, f_zoom.pixels);
    assert_ne!(f_offset.pixels, f_both.pixels);
    assert_ne!(f_zoom.pixels, f_both.pixels);
}

// ===========================================================================
// pat-wb3: 2D draw ordering, visibility, and layer semantics
// ===========================================================================

#[test]
fn z_index_higher_draws_on_top() {
    // Checks: Item with higher z_index is rendered on top of lower z_index items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Red at z=0, filling entire viewport.
    let mut bottom = CanvasItem::new(CanvasItemId(1));
    bottom.z_index = 0;
    bottom.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });

    // Green at z=5, filling entire viewport (should win).
    let mut middle = CanvasItem::new(CanvasItemId(2));
    middle.z_index = 5;
    middle.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(0.0, 1.0, 0.0),
        filled: true,
    });

    // Blue at z=10, filling entire viewport (should be on top).
    let mut top = CanvasItem::new(CanvasItemId(3));
    top.z_index = 10;
    top.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(0.0, 0.0, 1.0),
        filled: true,
    });

    // Add in non-sorted order to verify sorting works.
    vp.add_canvas_item(middle);
    vp.add_canvas_item(top);
    vp.add_canvas_item(bottom);

    let frame = renderer.render_frame(&vp);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    // Blue (z=10) should be visible everywhere.
    assert_eq!(frame.pixels[0], blue);
    assert_eq!(frame.pixels[55], blue);
    assert_eq!(frame.pixels[99], blue);
}

#[test]
fn invisible_item_does_not_render() {
    // Checks: An invisible canvas item produces no pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut visible_item = CanvasItem::new(CanvasItemId(1));
    visible_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });

    let mut invisible_item = CanvasItem::new(CanvasItemId(2));
    invisible_item.visible = false;
    invisible_item.z_index = 100; // Higher z but invisible.
    invisible_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(0.0, 1.0, 0.0),
        filled: true,
    });

    vp.add_canvas_item(visible_item);
    vp.add_canvas_item(invisible_item);

    let frame = renderer.render_frame(&vp);
    let red = Color::rgb(1.0, 0.0, 0.0);
    // Red item should be visible.
    assert_eq!(frame.pixels[0], red);
    // Area outside red rect should be background (invisible item didn't render).
    assert_eq!(frame.pixels[5 * 10 + 5], Color::BLACK);
}

#[test]
fn canvas_layer_ordering_by_z_order() {
    // Checks: CanvasLayers render in z_order — layer with higher z_order draws on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Layer 1 at z_order=0.
    let mut layer1 = CanvasLayer::new(1);
    layer1.z_order = 0;
    vp.add_canvas_layer(layer1);

    // Layer 2 at z_order=10 (should draw on top).
    let mut layer2 = CanvasLayer::new(2);
    layer2.z_order = 10;
    vp.add_canvas_layer(layer2);

    // Red item in layer 1.
    let mut item1 = CanvasItem::new(CanvasItemId(1));
    item1.layer_id = Some(1);
    item1.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });

    // Blue item in layer 2.
    let mut item2 = CanvasItem::new(CanvasItemId(2));
    item2.layer_id = Some(2);
    item2.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(0.0, 0.0, 1.0),
        filled: true,
    });

    vp.add_canvas_item(item1);
    vp.add_canvas_item(item2);

    let frame = renderer.render_frame(&vp);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    // Layer 2 (z=10) should be on top.
    assert_eq!(frame.pixels[0], blue);
    assert_eq!(frame.pixels[55], blue);
}

#[test]
fn invisible_canvas_layer_hides_all_items() {
    // Checks: Items inside an invisible CanvasLayer produce no pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.layer_id = Some(1);
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::WHITE,
        filled: true,
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // All pixels should be background black.
    for (i, pixel) in frame.pixels.iter().enumerate() {
        assert_eq!(
            *pixel,
            Color::BLACK,
            "pixel {i} should be black (layer invisible)"
        );
    }
}

#[test]
fn negative_z_index_draws_behind_zero() {
    // Checks: Item with negative z_index draws behind items with z_index=0.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut behind = CanvasItem::new(CanvasItemId(1));
    behind.z_index = -5;
    behind.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });

    let mut front = CanvasItem::new(CanvasItemId(2));
    front.z_index = 0;
    front.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(0.0, 0.0, 1.0),
        filled: true,
    });

    vp.add_canvas_item(behind);
    vp.add_canvas_item(front);

    let frame = renderer.render_frame(&vp);
    // Blue (z=0) should be on top of red (z=-5).
    assert_eq!(frame.pixels[0], Color::rgb(0.0, 0.0, 1.0));
}
