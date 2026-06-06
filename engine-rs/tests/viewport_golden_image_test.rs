//! pat-1mwnj: Viewport golden image regression tests — Layer 3.
//!
//! Pixel-level golden-image tests for the viewport rendering pipeline,
//! focusing on canvas layers, z-index ordering, layer transforms,
//! visibility toggling, and combined camera + layer scenarios.
//!
//! Golden PNGs are auto-generated on first run and compared on subsequent runs.

use std::path::PathBuf;

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdeditor::scene_renderer;
use gdrender2d::compare::compare_framebuffers;
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdrender2d::texture::load_png;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Render resolution for golden viewport tests.
const GOLDEN_W: u32 = 32;
const GOLDEN_H: u32 = 32;

/// Pixel tolerance for golden comparison (Euclidean RGB distance).
const GOLDEN_TOL: f64 = 0.02;

/// Minimum match ratio to pass golden comparison.
const GOLDEN_MIN_MATCH: f64 = 1.0;

const TOL: f32 = 0.02;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn red() -> Color {
    Color::rgb(1.0, 0.0, 0.0)
}
fn green() -> Color {
    Color::rgb(0.0, 1.0, 0.0)
}
fn blue() -> Color {
    Color::rgb(0.0, 0.0, 1.0)
}
fn yellow() -> Color {
    Color::rgb(1.0, 1.0, 0.0)
}
fn gray() -> Color {
    Color::rgb(0.5, 0.5, 0.5)
}

fn rect_item(id: u64, x: f32, y: f32, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        color,
        filled: true,
    });
    item
}

fn layered_rect(id: u64, layer: u64, x: f32, y: f32, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = rect_item(id, x, y, w, h, color);
    item.layer_id = Some(layer);
    item
}

fn count_color(fb: &FrameBuffer, color: Color) -> usize {
    fb.pixels
        .iter()
        .filter(|p| {
            (p.r - color.r).abs() < TOL
                && (p.g - color.g).abs() < TOL
                && (p.b - color.b).abs() < TOL
        })
        .count()
}

/// Returns the golden directory for viewport layer tests.
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
        .join("golden")
        .join("render")
        .join("viewport_layers")
}

fn save_golden(fb: &FrameBuffer, name: &str) {
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).expect("failed to create golden viewport_layers dir");
    let path = dir.join(format!("{name}.png"));
    fb.save_png(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("failed to save golden PNG {}: {e}", path.display()));
}

fn load_golden(name: &str) -> Option<FrameBuffer> {
    let path = golden_dir().join(format!("{name}.png"));
    let tex = load_png(path.to_str().unwrap())?;
    Some(FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    })
}

/// Compares a rendered framebuffer against a golden reference.
/// Generates the golden on first run; asserts exact match on subsequent runs.
fn assert_golden(fb: &FrameBuffer, name: &str) {
    match load_golden(name) {
        Some(golden) => {
            let result = compare_framebuffers(fb, &golden, GOLDEN_TOL);
            assert!(
                result.match_ratio() >= GOLDEN_MIN_MATCH,
                "golden viewport layer comparison failed for '{}': {:.2}% match \
                 (need {:.0}%), max_diff={:.4}, avg_diff={:.4}",
                name,
                result.match_ratio() * 100.0,
                GOLDEN_MIN_MATCH * 100.0,
                result.max_diff,
                result.avg_diff,
            );
        }
        None => {
            save_golden(fb, name);
            eprintln!(
                "Generated golden viewport layer reference: {}/{}.png",
                golden_dir().display(),
                name,
            );
        }
    }
}

// ===========================================================================
// CANVAS LAYER COMPOSITING
// ===========================================================================

#[test]
fn golden_layer_transform_offset() {
    // Layer with translation offset shifts its items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(8.0, 8.0));
    vp.add_canvas_layer(layer);

    // Rect at origin in layer 1 → appears at (8, 8) due to layer transform.
    vp.add_canvas_item(layered_rect(1, 1, 0.0, 0.0, 8.0, 8.0, red()));
    // Unlayered rect at (0, 0) for reference.
    vp.add_canvas_item(rect_item(2, 0.0, 0.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);

    // Green should be at origin.
    assert_pixel_color(&fb, 1, 1, green(), TOL);
    // Red should be offset to (8, 8).
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    // Origin should not be red.
    assert_pixel_color(&fb, 6, 6, Color::BLACK, TOL);

    assert_golden(&fb, "layer_transform_offset");
}

#[test]
fn golden_layer_z_order_compositing() {
    // Two layers with different z_order — higher z_order draws on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    // Layer 1: z_order=0 (back).
    let mut back_layer = CanvasLayer::new(1);
    back_layer.z_order = 0;
    vp.add_canvas_layer(back_layer);

    // Layer 2: z_order=10 (front).
    let mut front_layer = CanvasLayer::new(2);
    front_layer.z_order = 10;
    vp.add_canvas_layer(front_layer);

    // Back layer: large red rect.
    vp.add_canvas_item(layered_rect(1, 1, 4.0, 4.0, 24.0, 24.0, red()));
    // Front layer: smaller blue rect overlapping.
    vp.add_canvas_item(layered_rect(2, 2, 10.0, 10.0, 12.0, 12.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);

    // Center should be blue (front layer).
    assert_pixel_color(&fb, 16, 16, blue(), TOL);
    // Edge of red rect (outside blue) should be red.
    assert_pixel_color(&fb, 5, 5, red(), TOL);
    // Outside both should be black.
    assert_pixel_color(&fb, 1, 1, Color::BLACK, TOL);

    assert_golden(&fb, "layer_z_order_compositing");
}

#[test]
fn golden_layer_visibility_toggle() {
    // Invisible layer's items should not render.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut hidden_layer = CanvasLayer::new(1);
    hidden_layer.visible = false;
    vp.add_canvas_layer(hidden_layer);

    let mut visible_layer = CanvasLayer::new(2);
    visible_layer.z_order = 1;
    vp.add_canvas_layer(visible_layer);

    // Hidden layer: full-screen white rect.
    vp.add_canvas_item(layered_rect(1, 1, 0.0, 0.0, 32.0, 32.0, Color::WHITE));
    // Visible layer: small green rect.
    vp.add_canvas_item(layered_rect(2, 2, 12.0, 12.0, 8.0, 8.0, green()));

    let fb = capture_frame(&mut renderer, &vp);

    // No white pixels — hidden layer should not render.
    let white_count = count_color(&fb, Color::WHITE);
    assert_eq!(white_count, 0, "hidden layer should produce no pixels");
    // Green should appear.
    assert_pixel_color(&fb, 14, 14, green(), TOL);

    assert_golden(&fb, "layer_visibility_toggle");
}

// ===========================================================================
// Z-INDEX ORDERING WITHIN AND ACROSS LAYERS
// ===========================================================================

#[test]
fn golden_z_index_within_layer() {
    // Items within the same layer respect z_index ordering.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Red rect at z=0 (back).
    let mut back = layered_rect(1, 1, 8.0, 8.0, 16.0, 16.0, red());
    back.z_index = 0;
    vp.add_canvas_item(back);

    // Green rect at z=5 (front), overlapping.
    let mut front = layered_rect(2, 1, 12.0, 12.0, 16.0, 16.0, green());
    front.z_index = 5;
    vp.add_canvas_item(front);

    let fb = capture_frame(&mut renderer, &vp);

    // Overlap region should be green (higher z_index).
    assert_pixel_color(&fb, 14, 14, green(), TOL);
    // Red-only region.
    assert_pixel_color(&fb, 9, 9, red(), TOL);

    assert_golden(&fb, "z_index_within_layer");
}

#[test]
fn golden_z_index_unlayered_items() {
    // Unlayered items with different z_index values.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut bottom = rect_item(1, 4.0, 4.0, 24.0, 24.0, red());
    bottom.z_index = -10;
    vp.add_canvas_item(bottom);

    let mut middle = rect_item(2, 8.0, 8.0, 16.0, 16.0, green());
    middle.z_index = 0;
    vp.add_canvas_item(middle);

    let mut top = rect_item(3, 12.0, 12.0, 8.0, 8.0, blue());
    top.z_index = 10;
    vp.add_canvas_item(top);

    let fb = capture_frame(&mut renderer, &vp);

    // Center should be blue.
    assert_pixel_color(&fb, 16, 16, blue(), TOL);
    // Middle ring should be green.
    assert_pixel_color(&fb, 9, 9, green(), TOL);
    // Outer ring should be red.
    assert_pixel_color(&fb, 5, 5, red(), TOL);

    assert_golden(&fb, "z_index_unlayered_items");
}

// ===========================================================================
// CAMERA + LAYER TRANSFORMS COMBINED
// ===========================================================================

#[test]
fn golden_camera_offset_with_layer_transform() {
    // Camera panning combined with layer translation.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_position = Vector2::new(16.0, 16.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(4.0, 4.0));
    vp.add_canvas_layer(layer);

    // Layered rect at world (10, 10) + layer offset (4, 4) = effective (14, 14).
    // Camera at (16, 16) centers that at viewport: (14-16+16, 14-16+16) = (14, 14).
    vp.add_canvas_item(layered_rect(1, 1, 10.0, 10.0, 8.0, 8.0, red()));

    // Unlayered reference rect at world (16, 16) → viewport center (16, 16).
    vp.add_canvas_item(rect_item(2, 16.0, 16.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);

    let red_count = count_color(&fb, red());
    let green_count = count_color(&fb, green());
    assert!(red_count > 0, "layered rect should be visible");
    assert!(green_count > 0, "unlayered rect should be visible");

    assert_golden(&fb, "camera_offset_with_layer_transform");
}

#[test]
fn golden_camera_zoom_with_layer_transform() {
    // Camera zoom at 2x combined with layer translation.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_position = Vector2::new(16.0, 16.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(2.0, 2.0));
    vp.add_canvas_layer(layer);

    // Small rect that gets magnified by zoom.
    vp.add_canvas_item(layered_rect(1, 1, 14.0, 14.0, 4.0, 4.0, yellow()));

    let fb = capture_frame(&mut renderer, &vp);

    // At 2x zoom the 4x4 rect should appear as ~8x8 on screen.
    let yellow_count = count_color(&fb, yellow());
    assert!(
        yellow_count >= 48,
        "zoomed layered rect should be large: got {yellow_count}"
    );

    assert_golden(&fb, "camera_zoom_with_layer_transform");
}

#[test]
fn golden_camera_rotation_with_layers() {
    // Camera rotation combined with multi-layer scene.
    // Uses larger rects to ensure visible pixels after 45° rotation.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_position = Vector2::new(16.0, 16.0);
    vp.camera_rotation = std::f32::consts::FRAC_PI_4; // 45 degrees

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Large rects so rotation doesn't push them fully off-screen.
    vp.add_canvas_item(layered_rect(1, 1, 8.0, 8.0, 16.0, 16.0, red()));

    let fb = capture_frame(&mut renderer, &vp);

    // Rotated rect should still produce non-black pixels somewhere.
    let non_black = fb
        .pixels
        .iter()
        .filter(|p| p.r > 0.05 || p.g > 0.05 || p.b > 0.05)
        .count();
    assert!(
        non_black > 0,
        "rotated layer should produce visible pixels, got all black"
    );

    assert_golden(&fb, "camera_rotation_with_layers");
}

// ===========================================================================
// MULTI-LAYER EDITOR-LIKE SCENES
// ===========================================================================

#[test]
fn golden_editor_three_layer_scene() {
    // Simulates an editor-like scene with background, content, and overlay layers.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::rgb(0.1, 0.1, 0.15));

    // Background layer (z_order = -10).
    let mut bg_layer = CanvasLayer::new(1);
    bg_layer.z_order = -10;
    vp.add_canvas_layer(bg_layer);

    // Content layer (z_order = 0).
    let mut content_layer = CanvasLayer::new(2);
    content_layer.z_order = 0;
    vp.add_canvas_layer(content_layer);

    // Overlay layer (z_order = 100).
    let mut overlay_layer = CanvasLayer::new(3);
    overlay_layer.z_order = 100;
    vp.add_canvas_layer(overlay_layer);

    // Background: ground plane.
    vp.add_canvas_item(layered_rect(
        1,
        1,
        0.0,
        24.0,
        32.0,
        8.0,
        Color::rgb(0.2, 0.5, 0.2),
    ));

    // Content: player and platform.
    vp.add_canvas_item(layered_rect(2, 2, 12.0, 14.0, 4.0, 10.0, red()));
    vp.add_canvas_item(layered_rect(3, 2, 6.0, 12.0, 20.0, 2.0, gray()));

    // Overlay: selection highlight and grid line.
    vp.add_canvas_item(layered_rect(
        4,
        3,
        11.0,
        13.0,
        6.0,
        12.0,
        Color::rgb(0.3, 0.5, 1.0),
    ));

    let fb = capture_frame(&mut renderer, &vp);

    // Overlay should be on top at the selection area.
    assert_pixel_color(&fb, 14, 16, Color::rgb(0.3, 0.5, 1.0), TOL);
    // Ground should be visible at the bottom edge.
    assert_pixel_color(&fb, 2, 26, Color::rgb(0.2, 0.5, 0.2), TOL);

    assert_golden(&fb, "editor_three_layer_scene");
}

#[test]
fn golden_editor_grid_overlay() {
    // Simulates editor grid lines overlaid on content.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::rgb(0.15, 0.15, 0.15));

    // Content layer.
    let mut content_layer = CanvasLayer::new(1);
    content_layer.z_order = 0;
    vp.add_canvas_layer(content_layer);

    // Grid overlay layer.
    let mut grid_layer = CanvasLayer::new(2);
    grid_layer.z_order = 50;
    vp.add_canvas_layer(grid_layer);

    // Content: colored blocks.
    vp.add_canvas_item(layered_rect(1, 1, 2.0, 2.0, 12.0, 12.0, red()));
    vp.add_canvas_item(layered_rect(2, 1, 18.0, 18.0, 12.0, 12.0, blue()));

    // Grid lines (vertical and horizontal every 8 pixels).
    let grid_color = Color::rgb(0.4, 0.4, 0.4);
    for i in 0..4 {
        let pos = (i * 8) as f32;
        // Vertical grid line.
        let mut v_line = CanvasItem::new(CanvasItemId(100 + i * 2));
        v_line.layer_id = Some(2);
        v_line.commands.push(DrawCommand::DrawLine {
            from: Vector2::new(pos, 0.0),
            to: Vector2::new(pos, 32.0),
            color: grid_color,
            width: 1.0,
        });
        vp.add_canvas_item(v_line);

        // Horizontal grid line.
        let mut h_line = CanvasItem::new(CanvasItemId(101 + i * 2));
        h_line.layer_id = Some(2);
        h_line.commands.push(DrawCommand::DrawLine {
            from: Vector2::new(0.0, pos),
            to: Vector2::new(32.0, pos),
            color: grid_color,
            width: 1.0,
        });
        vp.add_canvas_item(h_line);
    }

    let fb = capture_frame(&mut renderer, &vp);

    // Content should be visible.
    let red_count = count_color(&fb, red());
    let blue_count = count_color(&fb, blue());
    assert!(red_count > 0, "content rect should be visible under grid");
    assert!(blue_count > 0, "content rect should be visible under grid");

    assert_golden(&fb, "editor_grid_overlay");
}

#[test]
fn golden_multiple_layer_transforms() {
    // Multiple layers each with different transforms.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    // Layer 1: translated right.
    let mut layer1 = CanvasLayer::new(1);
    layer1.z_order = 0;
    layer1.transform = Transform2D::translated(Vector2::new(16.0, 0.0));
    vp.add_canvas_layer(layer1);

    // Layer 2: translated down.
    let mut layer2 = CanvasLayer::new(2);
    layer2.z_order = 1;
    layer2.transform = Transform2D::translated(Vector2::new(0.0, 16.0));
    vp.add_canvas_layer(layer2);

    // Layer 3: translated diagonally.
    let mut layer3 = CanvasLayer::new(3);
    layer3.z_order = 2;
    layer3.transform = Transform2D::translated(Vector2::new(16.0, 16.0));
    vp.add_canvas_layer(layer3);

    // Same rect at origin in each layer → appears at different positions.
    vp.add_canvas_item(layered_rect(1, 1, 0.0, 0.0, 8.0, 8.0, red()));
    vp.add_canvas_item(layered_rect(2, 2, 0.0, 0.0, 8.0, 8.0, green()));
    vp.add_canvas_item(layered_rect(3, 3, 0.0, 0.0, 8.0, 8.0, blue()));

    // Also an unlayered rect at origin.
    vp.add_canvas_item(rect_item(4, 0.0, 0.0, 8.0, 8.0, yellow()));

    let fb = capture_frame(&mut renderer, &vp);

    // Top-left quadrant: yellow (unlayered at origin).
    assert_pixel_color(&fb, 2, 2, yellow(), TOL);
    // Top-right quadrant: red (layer 1 shifted right).
    assert_pixel_color(&fb, 18, 2, red(), TOL);
    // Bottom-left quadrant: green (layer 2 shifted down).
    assert_pixel_color(&fb, 2, 18, green(), TOL);
    // Bottom-right quadrant: blue (layer 3 shifted diagonally).
    assert_pixel_color(&fb, 18, 18, blue(), TOL);

    assert_golden(&fb, "multiple_layer_transforms");
}

// ===========================================================================
// VIEWPORT SIZE REGRESSION
// ===========================================================================

#[test]
fn golden_viewport_aspect_ratio_wide() {
    // Wide viewport with layers to verify aspect ratio handling.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(64, 16, Color::rgb(0.05, 0.05, 0.05));

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Three colored columns spread across the wide viewport.
    vp.add_canvas_item(layered_rect(1, 1, 0.0, 0.0, 16.0, 16.0, red()));
    vp.add_canvas_item(layered_rect(2, 1, 24.0, 0.0, 16.0, 16.0, green()));
    vp.add_canvas_item(layered_rect(3, 1, 48.0, 0.0, 16.0, 16.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 64);
    assert_eq!(fb.height, 16);

    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 32, 8, green(), TOL);
    assert_pixel_color(&fb, 56, 8, blue(), TOL);

    assert_golden(&fb, "viewport_aspect_ratio_wide");
}

#[test]
fn golden_viewport_aspect_ratio_tall() {
    // Tall viewport with layers.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(16, 64, Color::rgb(0.05, 0.05, 0.05));

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Three colored rows.
    vp.add_canvas_item(layered_rect(1, 1, 0.0, 0.0, 16.0, 16.0, red()));
    vp.add_canvas_item(layered_rect(2, 1, 0.0, 24.0, 16.0, 16.0, green()));
    vp.add_canvas_item(layered_rect(3, 1, 0.0, 48.0, 16.0, 16.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 16);
    assert_eq!(fb.height, 64);

    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 8, 32, green(), TOL);
    assert_pixel_color(&fb, 8, 56, blue(), TOL);

    assert_golden(&fb, "viewport_aspect_ratio_tall");
}

// ===========================================================================
// EDGE CASES
// ===========================================================================

#[test]
fn golden_empty_layer_no_crash() {
    // Empty layers should not affect rendering.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    // Add several empty layers.
    for i in 0..5 {
        let mut layer = CanvasLayer::new(i);
        layer.z_order = i as i32;
        vp.add_canvas_layer(layer);
    }

    // Only unlayered content.
    vp.add_canvas_item(rect_item(1, 8.0, 8.0, 16.0, 16.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 16, 16, red(), TOL);

    assert_golden(&fb, "empty_layers_no_crash");
}

#[test]
fn golden_layer_same_z_order_stable() {
    // Layers with the same z_order should render stably (insertion order).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut l1 = CanvasLayer::new(1);
    l1.z_order = 0;
    vp.add_canvas_layer(l1);

    let mut l2 = CanvasLayer::new(2);
    l2.z_order = 0;
    vp.add_canvas_layer(l2);

    // Overlapping rects in layers with same z_order.
    vp.add_canvas_item(layered_rect(1, 1, 8.0, 8.0, 16.0, 16.0, red()));
    vp.add_canvas_item(layered_rect(2, 2, 8.0, 8.0, 16.0, 16.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);

    // Run twice to verify stability.
    let fb2 = capture_frame(&mut renderer, &vp);
    let result = compare_framebuffers(&fb, &fb2, 0.0);
    assert!(
        result.is_exact_match(),
        "same z_order rendering should be deterministic"
    );

    assert_golden(&fb, "layer_same_z_order_stable");
}

// ===========================================================================
// SCENE RENDERER VIEWPORT TESTS
//
// These tests exercise the editor's scene_renderer with selection highlights,
// gizmo drawing, zoom/pan, and multi-node-type scenes.
// ===========================================================================

/// Resolution for scene renderer golden tests.
const SR_W: u32 = 256;
const SR_H: u32 = 256;

/// Tolerance for scene renderer golden comparison.
const SR_PIXEL_TOL: f64 = 0.03;

/// Minimum match ratio for scene renderer goldens.
const SR_MIN_MATCH: f64 = 0.98;

fn golden_scene_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
        .join("golden")
        .join("render")
        .join("viewport_scene")
}

fn read_scene_fixture(filename: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("scenes")
        .join(filename);
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

fn save_scene_golden(fb: &FrameBuffer, name: &str) {
    let dir = golden_scene_dir();
    std::fs::create_dir_all(&dir).expect("failed to create golden viewport_scene dir");
    let path = dir.join(format!("{name}.png"));
    fb.save_png(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("failed to save golden PNG {}: {e}", path.display()));
}

fn load_scene_golden(name: &str) -> Option<FrameBuffer> {
    let path = golden_scene_dir().join(format!("{name}.png"));
    let tex = load_png(path.to_str().unwrap())?;
    Some(FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    })
}

fn assert_scene_golden(fb: &FrameBuffer, name: &str) {
    match load_scene_golden(name) {
        Some(golden) => {
            let result = compare_framebuffers(fb, &golden, SR_PIXEL_TOL);
            assert!(
                result.match_ratio() >= SR_MIN_MATCH,
                "scene golden comparison failed for '{}': {:.2}% match (need {:.0}%), \
                 max_diff={:.4}, avg_diff={:.4}",
                name,
                result.match_ratio() * 100.0,
                SR_MIN_MATCH * 100.0,
                result.max_diff,
                result.avg_diff,
            );
        }
        None => {
            save_scene_golden(fb, name);
            eprintln!(
                "Generated scene golden: {}/{}.png",
                golden_scene_dir().display(),
                name,
            );
        }
    }
}

/// Returns the Nth child of the instanced scene root.
fn scene_child_id(tree: &SceneTree, n: usize) -> Option<gdscene::node::NodeId> {
    let root = tree.root_id();
    let root_node = tree.get_node(root)?;
    let scene_root = root_node.children().first().copied()?;
    let scene_node = tree.get_node(scene_root)?;
    scene_node.children().get(n).copied()
}

// ---------------------------------------------------------------------------
// Selection highlighting
// ---------------------------------------------------------------------------

#[test]
fn render_golden_scene_selection_node2d() {
    // Checks: Selecting a Node2D produces a visible selection highlight (amber
    // outline) and move gizmo (red/green arrows) at the node position.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);
    let selected = scene_child_id(&tree, 0);
    assert!(
        selected.is_some(),
        "scene must have at least one child node"
    );

    let fb = scene_renderer::render_scene(&tree, selected, SR_W, SR_H);
    assert_scene_golden(&fb, "scene_selection_node2d");
}

#[test]
fn render_golden_scene_selection_differs_from_unselected() {
    // Checks: The selected render is visually different from unselected.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb_unselected = scene_renderer::render_scene(&tree, None, SR_W, SR_H);
    let fb_selected = scene_renderer::render_scene(&tree, scene_child_id(&tree, 0), SR_W, SR_H);

    let result = compare_framebuffers(&fb_unselected, &fb_selected, 0.0);
    assert!(
        !result.is_exact_match(),
        "selection highlight should produce visible changes in the render",
    );
}

#[test]
fn render_golden_scene_selection_camera2d() {
    // Checks: Selecting a Camera2D node renders its selection highlight.
    let source = read_scene_fixture("render_test_camera.tscn");
    let tree = load_scene(&source);
    let selected = scene_child_id(&tree, 0);
    assert!(selected.is_some(), "scene must have Camera2D child");

    let fb = scene_renderer::render_scene(&tree, selected, SR_W, SR_H);
    assert_scene_golden(&fb, "scene_selection_camera2d");
}

#[test]
fn render_golden_scene_selection_second_node() {
    // Checks: Selecting the second child renders the gizmo at a different
    // position than selecting the first child.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb_first = scene_renderer::render_scene(&tree, scene_child_id(&tree, 0), SR_W, SR_H);
    let fb_second = scene_renderer::render_scene(&tree, scene_child_id(&tree, 1), SR_W, SR_H);

    let result = compare_framebuffers(&fb_first, &fb_second, 0.0);
    assert!(
        !result.is_exact_match(),
        "selecting different nodes should produce different gizmo positions",
    );
    assert_scene_golden(&fb_second, "scene_selection_second_node");
}

// ---------------------------------------------------------------------------
// Zoom level goldens
// ---------------------------------------------------------------------------

#[test]
fn render_golden_scene_zoom_half() {
    // Checks: At 0.5x zoom, nodes appear smaller and more world is visible.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene_with_zoom_pan(&tree, None, SR_W, SR_H, 0.5, (0.0, 0.0));
    assert_scene_golden(&fb, "scene_zoom_0_5x");
}

#[test]
fn render_golden_scene_zoom_2x() {
    // Checks: At 2x zoom, nodes appear larger and grid is spaced further.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene_with_zoom_pan(&tree, None, SR_W, SR_H, 2.0, (0.0, 0.0));
    assert_scene_golden(&fb, "scene_zoom_2x");
}

#[test]
fn render_golden_scene_zoom_4x() {
    // Checks: At 4x zoom, individual grid cells are clearly visible.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene_with_zoom_pan(&tree, None, SR_W, SR_H, 4.0, (0.0, 0.0));
    assert_scene_golden(&fb, "scene_zoom_4x");
}

#[test]
fn render_golden_scene_zoom_with_selection() {
    // Checks: Selection highlight and gizmo render correctly at 2x zoom.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);
    let selected = scene_child_id(&tree, 0);

    let fb =
        scene_renderer::render_scene_with_zoom_pan(&tree, selected, SR_W, SR_H, 2.0, (0.0, 0.0));
    assert_scene_golden(&fb, "scene_zoom_2x_selected");
}

// ---------------------------------------------------------------------------
// Pan goldens
// ---------------------------------------------------------------------------

#[test]
fn render_golden_scene_pan_right() {
    // Checks: Panning right by 100px shifts all world content left.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene_with_zoom_pan(&tree, None, SR_W, SR_H, 1.0, (100.0, 0.0));
    assert_scene_golden(&fb, "scene_pan_right_100");
}

#[test]
fn render_golden_scene_pan_down() {
    // Checks: Panning down by 100px shifts all world content up.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene_with_zoom_pan(&tree, None, SR_W, SR_H, 1.0, (0.0, 100.0));
    assert_scene_golden(&fb, "scene_pan_down_100");
}

#[test]
fn render_golden_scene_pan_and_zoom_combined() {
    // Checks: Zoom 2x + pan (50, 50) correctly composites both transforms.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene_with_zoom_pan(&tree, None, SR_W, SR_H, 2.0, (50.0, 50.0));
    assert_scene_golden(&fb, "scene_zoom_2x_pan_50_50");
}

// ---------------------------------------------------------------------------
// Multi-node-type scene goldens
// ---------------------------------------------------------------------------

#[test]
fn render_golden_scene_demo_2d_selected() {
    // Checks: demo_2d scene with the first node selected.
    let source = read_scene_fixture("demo_2d.tscn");
    let tree = load_scene(&source);
    let selected = scene_child_id(&tree, 0);

    let fb = scene_renderer::render_scene(&tree, selected, SR_W, SR_H);
    assert_scene_golden(&fb, "scene_demo_2d_selected");
}

#[test]
fn render_golden_scene_space_shooter_selected() {
    // Checks: space_shooter scene with a node selected.
    let source = read_scene_fixture("space_shooter.tscn");
    let tree = load_scene(&source);
    let selected = scene_child_id(&tree, 0);

    let fb = scene_renderer::render_scene(&tree, selected, SR_W, SR_H);
    assert_scene_golden(&fb, "scene_space_shooter_selected");
}

#[test]
fn render_golden_scene_ui_menu() {
    // Checks: UI menu scene renders Control nodes (purple representation).
    let source = read_scene_fixture("ui_menu.tscn");
    let tree = load_scene(&source);

    let fb = scene_renderer::render_scene(&tree, None, SR_W, SR_H);
    assert_scene_golden(&fb, "scene_ui_menu");
}

#[test]
fn render_golden_scene_ui_menu_selected() {
    // Checks: Selecting a Control node renders the selection highlight.
    let source = read_scene_fixture("ui_menu.tscn");
    let tree = load_scene(&source);
    let selected = scene_child_id(&tree, 0);

    let fb = scene_renderer::render_scene(&tree, selected, SR_W, SR_H);
    assert_scene_golden(&fb, "scene_ui_menu_selected");
}

// ---------------------------------------------------------------------------
// Determinism (scene renderer)
// ---------------------------------------------------------------------------

#[test]
fn render_golden_scene_selection_determinism() {
    // Checks: Selection/gizmo rendering is byte-identical across two runs.
    let source = read_scene_fixture("render_test_simple.tscn");

    let tree1 = load_scene(&source);
    let fb1 = scene_renderer::render_scene(&tree1, scene_child_id(&tree1, 0), SR_W, SR_H);

    let tree2 = load_scene(&source);
    let fb2 = scene_renderer::render_scene(&tree2, scene_child_id(&tree2, 0), SR_W, SR_H);

    let result = compare_framebuffers(&fb1, &fb2, 0.0);
    assert!(
        result.is_exact_match(),
        "selection rendering should be deterministic: {:.2}% match, max_diff={:.6}",
        result.match_ratio() * 100.0,
        result.max_diff,
    );
}

#[test]
fn render_golden_scene_zoom_determinism() {
    // Checks: Zoom rendering is deterministic across two runs.
    let source = read_scene_fixture("render_test_simple.tscn");

    let tree1 = load_scene(&source);
    let fb1 =
        scene_renderer::render_scene_with_zoom_pan(&tree1, None, SR_W, SR_H, 2.5, (30.0, 40.0));

    let tree2 = load_scene(&source);
    let fb2 =
        scene_renderer::render_scene_with_zoom_pan(&tree2, None, SR_W, SR_H, 2.5, (30.0, 40.0));

    let result = compare_framebuffers(&fb1, &fb2, 0.0);
    assert!(
        result.is_exact_match(),
        "zoom+pan rendering should be deterministic: {:.2}% match",
        result.match_ratio() * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Content validation (gizmo & highlight pixel checks)
// ---------------------------------------------------------------------------

#[test]
fn render_golden_scene_gizmo_has_colored_pixels() {
    // Checks: The move gizmo draws red (X axis) and green (Y axis) pixels.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb_unselected = scene_renderer::render_scene(&tree, None, SR_W, SR_H);
    let fb_selected = scene_renderer::render_scene(&tree, scene_child_id(&tree, 0), SR_W, SR_H);

    let mut red_diff_count = 0u64;
    let mut green_diff_count = 0u64;
    for (sel, unsel) in fb_selected.pixels.iter().zip(fb_unselected.pixels.iter()) {
        let dr = sel.r - unsel.r;
        let dg = sel.g - unsel.g;
        if dr > 0.3 && dr > dg.abs() {
            red_diff_count += 1;
        }
        if dg > 0.3 && dg > dr.abs() {
            green_diff_count += 1;
        }
    }

    assert!(
        red_diff_count > 5,
        "expected red gizmo pixels (X axis), found only {red_diff_count}",
    );
    assert!(
        green_diff_count > 5,
        "expected green gizmo pixels (Y axis), found only {green_diff_count}",
    );
}

#[test]
fn render_golden_scene_selection_has_amber_highlight() {
    // Checks: Selection highlight draws amber pixels around the selected node.
    let source = read_scene_fixture("render_test_simple.tscn");
    let tree = load_scene(&source);

    let fb_unselected = scene_renderer::render_scene(&tree, None, SR_W, SR_H);
    let fb_selected = scene_renderer::render_scene(&tree, scene_child_id(&tree, 0), SR_W, SR_H);

    let mut amber_count = 0u64;
    for (sel, unsel) in fb_selected.pixels.iter().zip(fb_unselected.pixels.iter()) {
        let dr = sel.r - unsel.r;
        let dg = sel.g - unsel.g;
        let db = sel.b - unsel.b;
        // Amber: high red, moderate green, low blue change.
        if dr > 0.2 && dg > 0.1 && db.abs() < 0.2 {
            amber_count += 1;
        }
    }

    assert!(
        amber_count > 5,
        "expected amber selection highlight pixels, found only {amber_count}",
    );
}
