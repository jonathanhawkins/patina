//! pat-ncj3: Extended camera and viewport render parity coverage.
//!
//! Extends parity coverage with focused fixtures verifying:
//!   - Canvas item local transforms composing with camera transforms
//!   - Parent-child transform inheritance under camera
//!   - Viewport boundary culling edge cases (exact edges, single pixel)
//!   - Multiple viewports with different camera configurations
//!   - Viewport resize behavior (same scene, different dimensions)
//!   - Triple composition: item transform + layer transform + camera
//!   - Camera zoom edge cases (extreme zoom in/out with transforms)
//!   - Visibility flag interaction with transform and culling
//!   - Determinism across all transform combinations

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::capture_frame;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TOL: f32 = 0.02;

fn red() -> Color {
    Color::rgb(1.0, 0.0, 0.0)
}
fn green() -> Color {
    Color::rgb(0.0, 1.0, 0.0)
}
fn blue() -> Color {
    Color::rgb(0.0, 0.0, 1.0)
}
fn white() -> Color {
    Color::rgb(1.0, 1.0, 1.0)
}
fn cyan() -> Color {
    Color::rgb(0.0, 1.0, 1.0)
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

fn rect_on_layer(
    id: u64,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    layer_id: u64,
) -> CanvasItem {
    let mut item = rect_item(id, x, y, w, h, color);
    item.layer_id = Some(layer_id);
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

fn has_color_at(fb: &FrameBuffer, x: u32, y: u32, color: Color) -> bool {
    let p = fb.get_pixel(x, y);
    (p.r - color.r).abs() < TOL && (p.g - color.g).abs() < TOL && (p.b - color.b).abs() < TOL
}

fn nonblack_count(fb: &FrameBuffer) -> usize {
    fb.pixels
        .iter()
        .filter(|p| p.r > TOL || p.g > TOL || p.b > TOL)
        .count()
}

// ===========================================================================
// 1. Canvas item local transform composing with camera
// ===========================================================================

#[test]
fn item_transform_translates_draw_commands() {
    // Item has a local transform that shifts its rect. Camera at origin.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(60, 60, Color::BLACK);

    let mut item = rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    item.transform = Transform2D::translated(Vector2::new(25.0, 25.0));
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Rect at (0,0)+(10,10) with transform offset (25,25) → pixel region [25..35, 25..35]
    assert!(has_color_at(&fb, 30, 30, red()));
    // Origin should be black (rect was shifted away)
    assert!(has_color_at(&fb, 0, 0, Color::BLACK));
}

#[test]
fn item_transform_scales_draw_commands() {
    // Item has 2x scale. Camera at origin.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(80, 80, Color::BLACK);

    let mut item = rect_item(1, 5.0, 5.0, 10.0, 10.0, green());
    item.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    // Scaled rect: [10..30, 10..30] = 20×20 = 400 pixels
    assert!(
        green_count >= 350,
        "expected ~400 green pixels from 2x scaled rect, got {}",
        green_count
    );
}

#[test]
fn item_transform_composes_with_camera_position() {
    // Item transform shifts rect by (20,20). Camera at (20,20) should cancel out.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);

    let mut item = rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    item.transform = Transform2D::translated(Vector2::new(20.0, 20.0));
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Item at world (20,20), camera at (20,20): item maps to viewport center.
    let center_red = has_color_at(&fb, 20, 20, red());
    assert!(
        center_red,
        "item at camera position should appear at viewport center"
    );
}

#[test]
fn item_transform_composes_with_camera_zoom() {
    // Item transform translates by (10,10). Camera zooms 2x.
    // World (10,10) with 2x zoom → screen distance from center doubles.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(80, 80, Color::BLACK);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    let mut item = rect_item(1, 0.0, 0.0, 5.0, 5.0, blue());
    item.transform = Transform2D::translated(Vector2::new(10.0, 10.0));
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let blue_count = count_color(&fb, blue());
    // 5×5 world rect at 2x zoom → 10×10 = 100 pixels
    assert!(
        blue_count >= 80,
        "expected ~100 blue pixels from 2x zoomed item, got {}",
        blue_count
    );
}

// ===========================================================================
// 2. Viewport boundary culling edge cases
// ===========================================================================

#[test]
fn item_exactly_at_viewport_right_edge_partially_visible() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(50, 50, Color::BLACK);

    // Rect from (45,20) to (55,30) — half inside, half outside.
    vp.add_canvas_item(rect_item(1, 45.0, 20.0, 10.0, 10.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    // Only 5×10 = 50 pixels should be visible (clipped at x=50).
    assert!(
        red_count > 0 && red_count < 100,
        "expected partial visibility (~50 pixels), got {}",
        red_count
    );
}

#[test]
fn item_exactly_at_viewport_bottom_edge_partially_visible() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(50, 50, Color::BLACK);

    // Rect from (20,45) to (30,55).
    vp.add_canvas_item(rect_item(1, 20.0, 45.0, 10.0, 10.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    assert!(
        green_count > 0 && green_count < 100,
        "expected partial visibility (~50 pixels), got {}",
        green_count
    );
}

#[test]
fn item_one_pixel_inside_viewport_is_visible() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(50, 50, Color::BLACK);

    // Rect mostly outside, only 1px inside at bottom-right corner.
    vp.add_canvas_item(rect_item(1, 49.0, 49.0, 10.0, 10.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert!(
        red_count >= 1,
        "item with 1px inside viewport should be visible"
    );
}

#[test]
fn item_one_pixel_outside_viewport_is_invisible() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(50, 50, Color::BLACK);

    // Rect starts at (50, 50) — entirely outside.
    vp.add_canvas_item(rect_item(1, 50.0, 50.0, 10.0, 10.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert_eq!(
        red_count, 0,
        "item fully outside viewport should be invisible"
    );
}

#[test]
fn camera_shift_pushes_item_to_exact_edge() {
    // screen = (world - cam_pos) + half_viewport
    // Item at (0,0) 20×10, camera at (-10, 0), viewport 40×40 (half=20):
    //   screen_x = (0 - (-10)) + 20 = 30 → rect [30..50, 20..30]
    //   Viewport clips at 40, so visible part [30..40, 20..30] = 100 px.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(-10.0, 0.0);
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 20.0, 10.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    let blue_count = count_color(&fb, blue());
    // Only partial rect visible (clipped on right).
    assert!(
        blue_count > 0,
        "shifted item at edge should be partially visible"
    );
    assert!(
        blue_count < 200,
        "item should be clipped, not fully visible"
    );
}

// ===========================================================================
// 3. Multiple viewports with different cameras
// ===========================================================================

#[test]
fn two_viewports_same_scene_different_cameras() {
    let mut renderer = SoftwareRenderer::new();

    // Viewport 1: camera at origin, sees item at (10,10)
    let mut vp1 = Viewport::new(40, 40, Color::BLACK);
    vp1.add_canvas_item(rect_item(1, 10.0, 10.0, 10.0, 10.0, red()));

    // Viewport 2: camera at (100,100), same item is far offscreen
    let mut vp2 = Viewport::new(40, 40, Color::BLACK);
    vp2.camera_position = Vector2::new(100.0, 100.0);
    vp2.add_canvas_item(rect_item(1, 10.0, 10.0, 10.0, 10.0, red()));

    let fb1 = capture_frame(&mut renderer, &vp1);
    let fb2 = capture_frame(&mut renderer, &vp2);

    let red1 = count_color(&fb1, red());
    let red2 = count_color(&fb2, red());

    assert!(red1 > 0, "viewport 1 should see the item");
    assert_eq!(
        red2, 0,
        "viewport 2 should not see the item (camera far away)"
    );
}

#[test]
fn two_viewports_different_zoom_same_item() {
    let mut renderer = SoftwareRenderer::new();

    // Item at origin, centered in viewport. screen = world * zoom + half_viewport.
    // zoom=1: rect at [30..40, 30..40] = 100px.
    // zoom=2: rect at [30..50, 30..50] = 400px.
    let mut vp1 = Viewport::new(60, 60, Color::BLACK);
    vp1.camera_zoom = Vector2::new(1.0, 1.0);
    vp1.add_canvas_item(rect_item(1, 0.0, 0.0, 10.0, 10.0, green()));

    let mut vp2 = Viewport::new(60, 60, Color::BLACK);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);
    vp2.add_canvas_item(rect_item(1, 0.0, 0.0, 10.0, 10.0, green()));

    let fb1 = capture_frame(&mut renderer, &vp1);
    let fb2 = capture_frame(&mut renderer, &vp2);

    let green1 = count_color(&fb1, green());
    let green2 = count_color(&fb2, green());

    // 2x zoom should produce ~4x more pixels.
    assert!(
        green2 > green1,
        "2x zoom viewport should show more green pixels ({} vs {})",
        green2,
        green1
    );
}

#[test]
fn two_viewports_different_sizes_same_camera() {
    let mut renderer = SoftwareRenderer::new();

    // Large viewport
    let mut vp_large = Viewport::new(100, 100, Color::BLACK);
    vp_large.add_canvas_item(rect_item(1, 10.0, 10.0, 20.0, 20.0, red()));

    // Small viewport — same scene, fewer pixels visible
    let mut vp_small = Viewport::new(30, 30, Color::BLACK);
    vp_small.add_canvas_item(rect_item(1, 10.0, 10.0, 20.0, 20.0, red()));

    let fb_large = capture_frame(&mut renderer, &vp_large);
    let fb_small = capture_frame(&mut renderer, &vp_small);

    let red_large = count_color(&fb_large, red());
    let red_small = count_color(&fb_small, red());

    // Small viewport clips the rect, so fewer red pixels.
    assert!(red_large > 0);
    assert!(red_small > 0);
    assert!(
        red_large >= red_small,
        "large viewport should show >= pixels ({} vs {})",
        red_large,
        red_small
    );
}

// ===========================================================================
// 4. Viewport resize behavior
// ===========================================================================

#[test]
fn viewport_dimensions_determine_framebuffer_size() {
    let mut renderer = SoftwareRenderer::new();

    let vp = Viewport::new(123, 67, Color::BLACK);
    let fb = capture_frame(&mut renderer, &vp);

    assert_eq!(fb.width, 123);
    assert_eq!(fb.height, 67);
    assert_eq!(fb.pixels.len(), 123 * 67);
}

#[test]
fn wider_viewport_reveals_more_horizontal_content() {
    let mut renderer = SoftwareRenderer::new();

    // Narrow viewport
    let mut vp_narrow = Viewport::new(30, 40, Color::BLACK);
    vp_narrow.add_canvas_item(rect_item(1, 25.0, 10.0, 10.0, 10.0, red()));
    let fb_narrow = capture_frame(&mut renderer, &vp_narrow);

    // Wide viewport
    let mut vp_wide = Viewport::new(60, 40, Color::BLACK);
    vp_wide.add_canvas_item(rect_item(1, 25.0, 10.0, 10.0, 10.0, red()));
    let fb_wide = capture_frame(&mut renderer, &vp_wide);

    let red_narrow = count_color(&fb_narrow, red());
    let red_wide = count_color(&fb_wide, red());

    // Wide viewport should show equal or more of the item.
    assert!(
        red_wide >= red_narrow,
        "wider viewport should show >= red pixels ({} vs {})",
        red_wide,
        red_narrow
    );
}

#[test]
fn taller_viewport_reveals_more_vertical_content() {
    let mut renderer = SoftwareRenderer::new();

    let mut vp_short = Viewport::new(40, 30, Color::BLACK);
    vp_short.add_canvas_item(rect_item(1, 10.0, 25.0, 10.0, 10.0, blue()));
    let fb_short = capture_frame(&mut renderer, &vp_short);

    let mut vp_tall = Viewport::new(40, 60, Color::BLACK);
    vp_tall.add_canvas_item(rect_item(1, 10.0, 25.0, 10.0, 10.0, blue()));
    let fb_tall = capture_frame(&mut renderer, &vp_tall);

    let blue_short = count_color(&fb_short, blue());
    let blue_tall = count_color(&fb_tall, blue());

    assert!(
        blue_tall >= blue_short,
        "taller viewport should show >= blue pixels ({} vs {})",
        blue_tall,
        blue_short
    );
}

// ===========================================================================
// 5. Triple composition: item transform + layer transform + camera
// ===========================================================================

#[test]
fn item_layer_camera_triple_composition() {
    // Item transform: translate (5,5)
    // Layer transform: translate (10,10)
    // Camera: at origin
    // Effective: rect at (15,15)
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(60, 60, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(10.0, 10.0));
    vp.add_canvas_layer(layer);

    let mut item = rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    item.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Red should appear around (15,15) to (25,25).
    assert!(has_color_at(&fb, 20, 20, red()));
    // Origin should be clear.
    assert!(has_color_at(&fb, 0, 0, Color::BLACK));
}

#[test]
fn item_layer_camera_triple_with_zoom() {
    // Item transform: translate (5,5)
    // Layer transform: translate (5,5)
    // Camera: 2x zoom
    // Effective world pos: (10,10), at 2x zoom → screen (20,20), 10×10 → 20×20
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(80, 80, Color::BLACK);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_layer(layer);

    let mut item = rect_item(1, 0.0, 0.0, 10.0, 10.0, green());
    item.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    // At 2x zoom, 10×10 world rect → 20×20 screen = 400 pixels.
    assert!(
        green_count >= 300,
        "triple composition with 2x zoom should produce ~400 green pixels, got {}",
        green_count
    );
}

#[test]
fn item_scale_layer_translate_camera_rotate() {
    // Item: 2x scale
    // Layer: translate (10, 0)
    // Camera: 45° rotation
    // Result: scaled rect shifted and rotated.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(80, 80, Color::BLACK);
    vp.camera_rotation = std::f32::consts::FRAC_PI_4; // 45°

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(10.0, 0.0));
    vp.add_canvas_layer(layer);

    let mut item = rect_item(1, 0.0, 0.0, 5.0, 5.0, cyan());
    item.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let cyan_count = count_color(&fb, cyan());
    assert!(
        cyan_count > 0,
        "rotated + scaled + layer-shifted item should be visible"
    );
}

// ===========================================================================
// 6. Visibility flag interaction with transforms
// ===========================================================================

#[test]
fn invisible_item_with_transform_produces_no_pixels() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);

    let mut item = rect_item(1, 10.0, 10.0, 20.0, 20.0, red());
    item.transform = Transform2D::IDENTITY;
    item.visible = false;
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, red()),
        0,
        "invisible item should produce no pixels"
    );
}

#[test]
fn visible_item_behind_invisible_item_renders() {
    // Two items at same position: invisible red on top, visible green below.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);

    let mut bg = rect_item(1, 10.0, 10.0, 10.0, 10.0, green());
    bg.z_index = 0;
    vp.add_canvas_item(bg);

    let mut fg = rect_item(2, 10.0, 10.0, 10.0, 10.0, red());
    fg.z_index = 1;
    fg.visible = false;
    vp.add_canvas_item(fg);

    let fb = capture_frame(&mut renderer, &vp);
    assert!(count_color(&fb, green()) > 0, "visible item should render");
    assert_eq!(
        count_color(&fb, red()),
        0,
        "invisible item should not render"
    );
}

#[test]
fn invisible_layer_hides_transformed_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_layer(layer);

    let mut item = rect_item(1, 10.0, 10.0, 10.0, 10.0, red());
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, red()),
        0,
        "invisible layer should hide all items"
    );
}

// ===========================================================================
// 7. Camera zoom edge cases with transforms
// ===========================================================================

#[test]
fn extreme_zoom_in_with_item_transform() {
    // 10x zoom on a small item with a transform offset.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    vp.camera_zoom = Vector2::new(10.0, 10.0);

    let mut item = rect_item(1, 0.0, 0.0, 2.0, 2.0, red());
    item.transform = Transform2D::translated(Vector2::new(1.0, 1.0));
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    // 2×2 world rect at 10x zoom → 20×20 = 400 pixels.
    assert!(
        red_count >= 300,
        "10x zoom should magnify small item to ~400 pixels, got {}",
        red_count
    );
}

#[test]
fn extreme_zoom_out_with_item_transform() {
    // 0.1x zoom: large item shrinks dramatically.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    vp.camera_zoom = Vector2::new(0.1, 0.1);

    let mut item = rect_item(1, 0.0, 0.0, 100.0, 100.0, green());
    item.transform = Transform2D::IDENTITY;
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    // 100×100 at 0.1x zoom → 10×10 = 100 pixels.
    assert!(
        green_count < 200,
        "0.1x zoom should shrink item drastically, got {} green pixels",
        green_count
    );
    assert!(green_count > 0, "item should still be partially visible");
}

#[test]
fn asymmetric_zoom_with_item_scale_transform() {
    // Camera zoom X=2, Y=1. Item scale 1x.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(80, 80, Color::BLACK);
    vp.camera_zoom = Vector2::new(2.0, 1.0);

    vp.add_canvas_item(rect_item(1, 10.0, 10.0, 10.0, 10.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    let blue_count = count_color(&fb, blue());
    // 10×10 with X-zoom 2 → 20×10 = 200 pixels.
    assert!(
        blue_count >= 150,
        "asymmetric X-zoom should stretch horizontally, got {}",
        blue_count
    );
}

// ===========================================================================
// 8. Determinism across all transform combinations
// ===========================================================================

#[test]
fn deterministic_item_transform_rendering() {
    let mut renderer = SoftwareRenderer::new();

    let build_vp = || {
        let mut vp = Viewport::new(50, 50, Color::BLACK);
        let mut item = rect_item(1, 5.0, 5.0, 10.0, 10.0, red());
        item.transform = Transform2D {
            x: Vector2::new(1.5, 0.0),
            y: Vector2::new(0.0, 1.5),
            origin: Vector2::new(3.0, 3.0),
        };
        vp.add_canvas_item(item);
        vp
    };

    let fb1 = capture_frame(&mut renderer, &build_vp());
    let fb2 = capture_frame(&mut renderer, &build_vp());

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "item transform rendering must be deterministic"
    );
}

#[test]
fn deterministic_triple_composition_rendering() {
    let mut renderer = SoftwareRenderer::new();

    let build_vp = || {
        let mut vp = Viewport::new(60, 60, Color::BLACK);
        vp.camera_position = Vector2::new(10.0, 10.0);
        vp.camera_zoom = Vector2::new(1.5, 1.5);
        vp.camera_rotation = 0.3;

        let mut layer = CanvasLayer::new(1);
        layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
        vp.add_canvas_layer(layer);

        let mut item = rect_item(1, 0.0, 0.0, 8.0, 8.0, green());
        item.transform = Transform2D::translated(Vector2::new(2.0, 2.0));
        item.layer_id = Some(1);
        vp.add_canvas_item(item);

        vp
    };

    let fb1 = capture_frame(&mut renderer, &build_vp());
    let fb2 = capture_frame(&mut renderer, &build_vp());

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "triple composition must be deterministic"
    );
}

#[test]
fn deterministic_multi_item_multi_layer_rendering() {
    let mut renderer = SoftwareRenderer::new();

    let build_vp = || {
        let mut vp = Viewport::new(80, 80, Color::BLACK);
        vp.camera_position = Vector2::new(5.0, 5.0);
        vp.camera_zoom = Vector2::new(2.0, 2.0);

        let layer_a = CanvasLayer::new(1);
        let layer_b = CanvasLayer::new(2);
        vp.add_canvas_layer(layer_a);
        vp.add_canvas_layer(layer_b);

        vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 10.0, 10.0, red(), 1));
        vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), 1));
        vp.add_canvas_item(rect_on_layer(3, 10.0, 10.0, 10.0, 10.0, blue(), 2));

        vp
    };

    let fb1 = capture_frame(&mut renderer, &build_vp());
    let fb2 = capture_frame(&mut renderer, &build_vp());

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "multi-item multi-layer rendering must be deterministic"
    );
}

// ===========================================================================
// 9. Camera rotation with item transforms
// ===========================================================================

#[test]
fn camera_rotation_with_translated_item_shifts_output() {
    let mut renderer = SoftwareRenderer::new();

    // No rotation
    let mut vp_no_rot = Viewport::new(60, 60, Color::BLACK);
    let mut item1 = rect_item(1, 20.0, 20.0, 10.0, 10.0, red());
    item1.transform = Transform2D::translated(Vector2::new(5.0, 0.0));
    vp_no_rot.add_canvas_item(item1);
    let fb_no_rot = capture_frame(&mut renderer, &vp_no_rot);

    // With 90° rotation
    let mut vp_rot = Viewport::new(60, 60, Color::BLACK);
    vp_rot.camera_rotation = std::f32::consts::FRAC_PI_2;
    let mut item2 = rect_item(1, 20.0, 20.0, 10.0, 10.0, red());
    item2.transform = Transform2D::translated(Vector2::new(5.0, 0.0));
    vp_rot.add_canvas_item(item2);
    let fb_rot = capture_frame(&mut renderer, &vp_rot);

    // Both should have red pixels, but at different positions.
    let red_no_rot = count_color(&fb_no_rot, red());
    let red_rot = count_color(&fb_rot, red());
    assert!(red_no_rot > 0, "non-rotated should show red");
    assert!(red_rot > 0, "rotated should still show red");
    // Pixel positions should differ.
    assert_ne!(
        fb_no_rot.pixels, fb_rot.pixels,
        "rotation should change pixel output"
    );
}

#[test]
fn camera_rotation_preserves_total_colored_area_approximately() {
    let mut renderer = SoftwareRenderer::new();

    let build_vp = |rotation: f32| {
        let mut vp = Viewport::new(100, 100, Color::BLACK);
        vp.camera_position = Vector2::new(50.0, 50.0);
        vp.camera_rotation = rotation;
        // Rect centered at camera position so it stays in viewport center.
        vp.add_canvas_item(rect_item(1, 40.0, 40.0, 20.0, 20.0, white()));
        vp
    };

    let fb_0 = capture_frame(&mut renderer, &build_vp(0.0));
    let fb_45 = capture_frame(&mut renderer, &build_vp(std::f32::consts::FRAC_PI_4));
    let fb_90 = capture_frame(&mut renderer, &build_vp(std::f32::consts::FRAC_PI_2));

    let white_0 = count_color(&fb_0, white());
    let white_45 = count_color(&fb_45, white());
    let white_90 = count_color(&fb_90, white());

    // All rotations should show approximately the same number of pixels
    // (rect fully inside viewport in all cases).
    assert!(white_0 > 300);
    assert!(white_90 > 300);
    // 45° rotation of a rect produces a diamond — similar area but rasterization
    // may differ slightly.
    assert!(white_45 > 200);
}

// ===========================================================================
// 10. Clear color and background
// ===========================================================================

#[test]
fn viewport_clear_color_fills_empty_regions() {
    let mut renderer = SoftwareRenderer::new();
    let clear = Color::rgb(0.2, 0.3, 0.4);
    let mut vp = Viewport::new(40, 40, clear);
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 10.0, 10.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // Region outside the red rect should be clear color.
    assert!(has_color_at(&fb, 35, 35, clear));
}

#[test]
fn different_clear_colors_produce_different_backgrounds() {
    let mut renderer = SoftwareRenderer::new();

    let fb1 = capture_frame(
        &mut renderer,
        &Viewport::new(20, 20, Color::rgb(1.0, 0.0, 0.0)),
    );
    let fb2 = capture_frame(
        &mut renderer,
        &Viewport::new(20, 20, Color::rgb(0.0, 1.0, 0.0)),
    );

    assert_ne!(
        fb1.pixels, fb2.pixels,
        "different clear colors must produce different output"
    );
}
