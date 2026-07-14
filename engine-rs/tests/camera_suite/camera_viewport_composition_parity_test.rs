//! pat-xblb: Camera and viewport composition parity tests.
//!
//! Extends render parity coverage beyond the basic camera tests in
//! `render_camera_viewport_test.rs` by exercising canvas layer composition:
//! - Layer z-order determines draw order (higher z_order overwrites lower)
//! - Invisible canvas layers gate rendering of their items
//! - Canvas layer transforms compose with the camera transform
//! - Multi-layer rendering with camera zoom and rotation
//! - Unlayered items render after all layered items
//! - Layer transform + camera position interaction
//! - Viewport clear color visible through transparent layers

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
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
/// Creates a filled rect canvas item at a specific world position, optionally on a layer.
fn rect_on_layer(
    id: u64,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    layer_id: Option<u64>,
) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        color,
        filled: true,
    });
    item.layer_id = layer_id;
    item
}

/// Count pixels matching the given color within tolerance.
fn count_color(fb: &gdrender2d::renderer::FrameBuffer, color: Color) -> usize {
    fb.pixels
        .iter()
        .filter(|p| {
            (p.r - color.r).abs() < TOL
                && (p.g - color.g).abs() < TOL
                && (p.b - color.b).abs() < TOL
        })
        .count()
}

// ===========================================================================
// CANVAS LAYER Z-ORDER
// ===========================================================================

#[test]
fn layer_z_order_determines_draw_order() {
    // Two layers with overlapping items. Higher z_order should draw on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut bg_layer = CanvasLayer::new(1);
    bg_layer.z_order = 0;
    let mut fg_layer = CanvasLayer::new(2);
    fg_layer.z_order = 1;

    vp.add_canvas_layer(bg_layer);
    vp.add_canvas_layer(fg_layer);

    // Red rect on background layer covers full viewport.
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), Some(1)));
    // Green rect on foreground layer covers center.
    vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), Some(2)));

    let fb = capture_frame(&mut renderer, &vp);

    // Center should be green (foreground layer on top).
    assert_pixel_color(&fb, 10, 10, green(), TOL);
    // Corner should be red (only background layer).
    assert_pixel_color(&fb, 0, 0, red(), TOL);
}

#[test]
fn layer_z_order_reversed_swaps_result() {
    // Same setup as above but with reversed z_order — red should be on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer_a = CanvasLayer::new(1);
    layer_a.z_order = 1; // red layer on TOP now
    let mut layer_b = CanvasLayer::new(2);
    layer_b.z_order = 0; // green layer on bottom

    vp.add_canvas_layer(layer_a);
    vp.add_canvas_layer(layer_b);

    // Red on layer 1 (z=1, top), green on layer 2 (z=0, bottom).
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), Some(1)));
    vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), Some(2)));

    let fb = capture_frame(&mut renderer, &vp);

    // Center should be red now (red layer has higher z_order).
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    // Corner also red.
    assert_pixel_color(&fb, 0, 0, red(), TOL);
}

#[test]
fn three_layers_render_in_z_order() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    // Three layers: z=0, z=1, z=2.
    for (id, z) in [(1u64, 0i32), (2, 1), (3, 2)] {
        let mut layer = CanvasLayer::new(id);
        layer.z_order = z;
        vp.add_canvas_layer(layer);
    }

    // Each layer has a rect at the same position; topmost (z=2) wins.
    vp.add_canvas_item(rect_on_layer(1, 5.0, 5.0, 20.0, 20.0, red(), Some(1)));
    vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 20.0, 20.0, green(), Some(2)));
    vp.add_canvas_item(rect_on_layer(3, 5.0, 5.0, 20.0, 20.0, blue(), Some(3)));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 15, 15, blue(), TOL);
}

// ===========================================================================
// CANVAS LAYER VISIBILITY
// ===========================================================================

#[test]
fn invisible_layer_hides_its_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    // Red rect on the invisible layer.
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), Some(1)));

    let fb = capture_frame(&mut renderer, &vp);
    // No red pixels should be visible.
    assert_eq!(
        count_color(&fb, red()),
        0,
        "Invisible layer items must not render"
    );
    // Entire viewport should be clear color.
    assert_pixel_color(&fb, 10, 10, Color::BLACK, TOL);
}

#[test]
fn invisible_layer_does_not_block_visible_layers() {
    // Invisible layer between two visible layers should not affect rendering.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer0 = CanvasLayer::new(1);
    layer0.z_order = 0;
    let mut layer1 = CanvasLayer::new(2);
    layer1.z_order = 1;
    layer1.visible = false; // middle layer invisible
    let mut layer2 = CanvasLayer::new(3);
    layer2.z_order = 2;

    vp.add_canvas_layer(layer0);
    vp.add_canvas_layer(layer1);
    vp.add_canvas_layer(layer2);

    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), Some(1)));
    vp.add_canvas_item(rect_on_layer(2, 0.0, 0.0, 20.0, 20.0, green(), Some(2))); // hidden
    vp.add_canvas_item(rect_on_layer(3, 5.0, 5.0, 10.0, 10.0, blue(), Some(3)));

    let fb = capture_frame(&mut renderer, &vp);
    // Center: blue (top layer)
    assert_pixel_color(&fb, 10, 10, blue(), TOL);
    // Corner: red (bottom layer, green hidden)
    assert_pixel_color(&fb, 0, 0, red(), TOL);
    // No green anywhere.
    assert_eq!(count_color(&fb, green()), 0);
}

// ===========================================================================
// CANVAS LAYER TRANSFORM
// ===========================================================================

#[test]
fn layer_transform_shifts_items() {
    // A canvas layer with a translation transform should shift its items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(10.0, 10.0));
    vp.add_canvas_layer(layer);

    // Rect at world (0,0), but layer shifts it to screen (10,10).
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 5.0, 5.0, red(), Some(1)));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    assert_pixel_color(&fb, 14, 14, red(), TOL);
    // Origin should be clear.
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
}

#[test]
fn different_layer_transforms_separate_items() {
    // Two layers with different translations place items at different screen positions.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    let mut layer_a = CanvasLayer::new(1);
    layer_a.transform = Transform2D::translated(Vector2::new(0.0, 0.0));
    let mut layer_b = CanvasLayer::new(2);
    layer_b.transform = Transform2D::translated(Vector2::new(15.0, 15.0));

    vp.add_canvas_layer(layer_a);
    vp.add_canvas_layer(layer_b);

    // Both items at (0,0) in their layer space.
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 5.0, 5.0, red(), Some(1)));
    vp.add_canvas_item(rect_on_layer(2, 0.0, 0.0, 5.0, 5.0, green(), Some(2)));

    let fb = capture_frame(&mut renderer, &vp);
    // Red at top-left.
    assert_pixel_color(&fb, 2, 2, red(), TOL);
    // Green shifted to (15,15).
    assert_pixel_color(&fb, 17, 17, green(), TOL);
    // They shouldn't overlap.
    assert_pixel_color(&fb, 10, 10, Color::BLACK, TOL);
}

// ===========================================================================
// CAMERA + CANVAS LAYER COMPOSITION
// ===========================================================================

#[test]
fn camera_position_composes_with_layer_transform() {
    // Camera at (10, 10) + layer translate (5, 5) = effective offset.
    // Camera transform: translate(half_vp) * translate(-cam) = translate(15-10, 15-10) = translate(5,5)
    // Total: camera_xform * layer_xform = translate(5,5) * translate(5,5) = translate(10,10)
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 10.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_layer(layer);

    // Item at (0,0) in layer space → layer transform → (5,5) → camera → (10,10)
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 4.0, 4.0, red(), Some(1)));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    assert_pixel_color(&fb, 13, 13, red(), TOL);
    assert_pixel_color(&fb, 9, 9, Color::BLACK, TOL);
}

#[test]
fn camera_zoom_scales_layered_items() {
    // Camera zoom should scale items on canvas layers too.
    let size = 40u32;
    let half = size as f32 / 2.0;
    let mut renderer = SoftwareRenderer::new();

    let mut vp = Viewport::new(size, size, Color::BLACK);
    vp.camera_position = Vector2::new(half, half);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // 2x2 rect at camera center → should become 4x4 on screen at center.
    vp.add_canvas_item(rect_on_layer(
        1,
        half - 1.0,
        half - 1.0,
        2.0,
        2.0,
        red(),
        Some(1),
    ));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    // At 2x zoom, 2x2 world → 4x4 screen = 16 pixels.
    assert_eq!(red_count, 16, "2x zoom should quadruple area: 2x2 → 4x4");
}

#[test]
fn camera_rotation_applies_to_layered_items() {
    // Rotation should affect layered items the same way as unlayered.
    let make_fb = |use_layer: bool| {
        let mut vp = Viewport::new(40, 40, Color::BLACK);
        vp.camera_position = Vector2::new(10.0, 10.0);
        vp.camera_rotation = std::f32::consts::FRAC_PI_4;

        if use_layer {
            let layer = CanvasLayer::new(1);
            vp.add_canvas_layer(layer);
            vp.add_canvas_item(rect_on_layer(1, 8.0, 8.0, 4.0, 4.0, red(), Some(1)));
        } else {
            vp.add_canvas_item(rect_on_layer(1, 8.0, 8.0, 4.0, 4.0, red(), None));
        }

        let mut r = SoftwareRenderer::new();
        capture_frame(&mut r, &vp)
    };

    let fb_layered = make_fb(true);
    let fb_unlayered = make_fb(false);

    // Both should produce the same result (identity layer transform).
    assert_eq!(
        fb_layered.pixels, fb_unlayered.pixels,
        "Identity-transform layer should not change rendering vs unlayered"
    );
}

// ===========================================================================
// LAYERED vs UNLAYERED RENDERING ORDER
// ===========================================================================

#[test]
fn unlayered_items_render_after_layered_items() {
    // Per renderer.rs: layered items render first, then unlayered items.
    // So unlayered items should appear on top of layered items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.z_order = 0;
    vp.add_canvas_layer(layer);

    // Red on layer (renders first).
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), Some(1)));
    // Green unlayered (renders after).
    vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), None));

    let fb = capture_frame(&mut renderer, &vp);
    // Center should be green (unlayered on top).
    assert_pixel_color(&fb, 10, 10, green(), TOL);
    // Corner should be red (only layered item).
    assert_pixel_color(&fb, 0, 0, red(), TOL);
}

// ===========================================================================
// VIEWPORT COMPOSITION EDGE CASES
// ===========================================================================

#[test]
fn empty_layer_does_not_affect_rendering() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Add an empty layer.
    vp.add_canvas_layer(CanvasLayer::new(1));

    // Only unlayered item.
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 10.0, 10.0, red(), None));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 5, 5, red(), TOL);
    assert_pixel_color(&fb, 15, 15, Color::BLACK, TOL);
}

#[test]
fn layer_transform_can_push_items_offscreen() {
    // Layer transform shifts items beyond viewport bounds → clipped.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(100.0, 100.0));
    vp.add_canvas_layer(layer);

    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 5.0, 5.0, red(), Some(1)));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, red()),
        0,
        "Item shifted off-screen by layer transform should be clipped"
    );
}

#[test]
fn camera_can_reveal_layer_transformed_items() {
    // Layer pushes item to (100, 100), camera centered on (100, 100) reveals it.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(100.0, 100.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(98.0, 98.0));
    vp.add_canvas_layer(layer);

    // Item at layer-local (0,0) → layer transform → world (98,98).
    // Camera at (100,100), viewport 20x20 → world (98,98) maps to screen (8,8).
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 4.0, 4.0, red(), Some(1)));

    let fb = capture_frame(&mut renderer, &vp);
    assert!(
        count_color(&fb, red()) > 0,
        "Camera should reveal items shifted by layer transform"
    );
}

#[test]
fn clear_color_visible_when_all_layers_are_invisible() {
    let mut renderer = SoftwareRenderer::new();
    let clear = Color::rgb(0.2, 0.4, 0.8);
    let mut vp = Viewport::new(10, 10, clear);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 10.0, 10.0, red(), Some(1)));

    let fb = capture_frame(&mut renderer, &vp);
    // Entire viewport should show clear color.
    for y in 0..10 {
        for x in 0..10 {
            assert_pixel_color(&fb, x, y, clear, TOL);
        }
    }
}

#[test]
fn z_index_within_same_layer_determines_item_order() {
    // Two items on the same layer; higher z_index draws on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    let mut bottom = rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), Some(1));
    bottom.z_index = 0;
    let mut top = rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), Some(1));
    top.z_index = 1;

    vp.add_canvas_item(bottom);
    vp.add_canvas_item(top);

    let fb = capture_frame(&mut renderer, &vp);
    // Center: green (higher z_index).
    assert_pixel_color(&fb, 10, 10, green(), TOL);
    // Corner: red (lower z_index, only item there).
    assert_pixel_color(&fb, 0, 0, red(), TOL);
}

#[test]
fn composition_is_deterministic() {
    let make_frame = || {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(30, 30, Color::BLACK);
        vp.camera_position = Vector2::new(15.0, 15.0);
        vp.camera_zoom = Vector2::new(1.5, 1.5);
        vp.camera_rotation = 0.3;

        let mut layer = CanvasLayer::new(1);
        layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
        vp.add_canvas_layer(layer);

        vp.add_canvas_item(rect_on_layer(1, 10.0, 10.0, 6.0, 6.0, red(), Some(1)));
        vp.add_canvas_item(rect_on_layer(2, 14.0, 14.0, 4.0, 4.0, green(), None));

        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make_frame();
    let fb2 = make_frame();
    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Composed layer+camera rendering must be deterministic"
    );
}
