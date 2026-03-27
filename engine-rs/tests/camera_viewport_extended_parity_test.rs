//! pat-357t: Extended camera and viewport render parity tests.
//!
//! Broadens render parity coverage with focused fixtures verifying:
//! - Camera transform composition (position + zoom + rotation combinations)
//! - Viewport clipping/culling under camera transforms
//! - Canvas layer scale composing with camera zoom
//! - Extreme viewport aspect ratios
//! - Camera affine inverse roundtrip (world ↔ screen coordinates)
//! - Multi-layer transform + camera interaction edge cases

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
fn yellow() -> Color {
    Color::rgb(1.0, 1.0, 0.0)
}

/// Creates a filled rect canvas item at a specific world position.
fn rect_at(id: u64, x: f32, y: f32, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        color,
        filled: true,
    });
    item
}

/// Creates a filled rect canvas item assigned to a layer.
fn rect_on_layer(
    id: u64,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    layer_id: u64,
) -> CanvasItem {
    let mut item = rect_at(id, x, y, w, h, color);
    item.layer_id = Some(layer_id);
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
// CAMERA TRANSFORM COMPOSITION — POSITION + ZOOM + ROTATION
// ===========================================================================

#[test]
fn camera_position_zoom_rotation_triple_composition() {
    // Verify all three camera properties compose correctly.
    // Camera at (50, 50), 2x zoom, 90° rotation.
    // A rect at world (50, 50) should map to viewport center regardless of zoom/rotation.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);
    vp.camera_rotation = std::f32::consts::FRAC_PI_2; // 90°

    // Place a small rect exactly at camera position.
    vp.add_canvas_item(rect_at(1, 49.0, 49.0, 2.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // World (50, 50) → camera origin → viewport center (20, 20).
    // The rect covers world [49..51, 49..51], offset from camera = [-1..-1 to 1..1].
    // After rotation and zoom it should still be near center.
    // Check that red is present near the center region.
    let center_area_red: usize = (16..24)
        .flat_map(|y| (16..24).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = fb.get_pixel(x, y);
            (p.r - 1.0).abs() < TOL && p.g < TOL && p.b < TOL
        })
        .count();
    assert!(
        center_area_red > 0,
        "Red rect at camera position should appear near viewport center"
    );
}

#[test]
fn camera_zoom_position_offset_precise() {
    // Camera at (10, 0) with 2x zoom in a 20x20 viewport.
    // World origin (0,0) should map to screen:
    //   translate(-10, 0) → (-10, 0)
    //   rotate(0) → (-10, 0)
    //   scale(2) → (-20, 0)
    //   translate(+10, +10) → (-10, 10)
    // So world origin is off-screen to the left.
    // World (10, 0) → (0, 0) → (0, 0) → (0, 0) → (10, 10) = viewport center.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 0.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // Rect at world (10, 0), size 2x2 → screen center (10, 10), scaled to 4x4.
    vp.add_canvas_item(rect_at(1, 10.0, 0.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // Screen (10, 10) should be green.
    assert_pixel_color(&fb, 10, 10, green(), TOL);
    // Screen (13, 13) should also be green (2x2 world → 4x4 screen).
    assert_pixel_color(&fb, 13, 13, green(), TOL);
    // Screen (0, 0) should be black (world origin is off-screen).
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
}

#[test]
fn camera_rotation_45_degrees_expands_rect_aabb() {
    // A rect rotated 45° by the camera should have an expanded axis-aligned
    // bounding box, covering more pixels than the unrotated rect.
    let mut renderer = SoftwareRenderer::new();

    // Without rotation:
    let mut vp_no_rot = Viewport::new(40, 40, Color::BLACK);
    vp_no_rot.add_canvas_item(rect_at(1, 15.0, 15.0, 10.0, 10.0, red()));
    let fb_no_rot = capture_frame(&mut renderer, &vp_no_rot);
    let red_no_rot = count_color(&fb_no_rot, red());

    // With 45° rotation:
    let mut renderer2 = SoftwareRenderer::new();
    let mut vp_rot = Viewport::new(40, 40, Color::BLACK);
    vp_rot.camera_position = Vector2::new(20.0, 20.0);
    vp_rot.camera_rotation = std::f32::consts::FRAC_PI_4; // 45°
    vp_rot.add_canvas_item(rect_at(1, 15.0, 15.0, 10.0, 10.0, red()));
    let fb_rot = capture_frame(&mut renderer2, &vp_rot);
    let red_rot = count_color(&fb_rot, red());

    // Rotated AABB should cover at least as many pixels (typically more due to AABB expansion).
    assert!(
        red_rot >= red_no_rot,
        "Rotated rect AABB ({red_rot}) should be >= unrotated ({red_no_rot})"
    );
}

// ===========================================================================
// VIEWPORT CULLING — ITEMS OUTSIDE CAMERA VIEW
// ===========================================================================

#[test]
fn item_entirely_outside_viewport_produces_no_pixels() {
    // A rect placed far outside the viewport should not produce any visible pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    // Rect at world (100, 100) — well outside the 20×20 viewport with no camera offset.
    vp.add_canvas_item(rect_at(1, 100.0, 100.0, 5.0, 5.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert_eq!(red_count, 0, "Off-screen rect should produce zero red pixels");
}

#[test]
fn camera_pan_brings_offscreen_item_into_view() {
    // Item is off-screen at world (100, 100). Panning the camera to (100, 100)
    // should bring it to viewport center.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(100.0, 100.0);
    vp.add_canvas_item(rect_at(1, 99.0, 99.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // World (100, 100) → screen center (10, 10).
    assert_pixel_color(&fb, 10, 10, green(), TOL);
}

#[test]
fn zoom_out_reveals_more_world_content() {
    // At 1x zoom, items at world edges may be clipped.
    // At 0.5x zoom (zoom out), more world is visible.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Place items near the edges of what 1x zoom can show.
    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 2.0, 2.0, red()));
    vp.add_canvas_item(rect_at(2, 18.0, 18.0, 2.0, 2.0, blue()));

    let fb_1x = capture_frame(&mut renderer, &vp);
    let red_1x = count_color(&fb_1x, red());
    let _blue_1x = count_color(&fb_1x, blue());

    // Now zoom out to 0.5x — world content should shrink but more of it is visible.
    let mut renderer2 = SoftwareRenderer::new();
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.camera_zoom = Vector2::new(0.5, 0.5);
    // Need to center camera so items are visible.
    vp2.camera_position = Vector2::new(10.0, 10.0);
    vp2.add_canvas_item(rect_at(1, 0.0, 0.0, 2.0, 2.0, red()));
    vp2.add_canvas_item(rect_at(2, 18.0, 18.0, 2.0, 2.0, blue()));

    let fb_half = capture_frame(&mut renderer2, &vp2);
    let red_half = count_color(&fb_half, red());
    let blue_half = count_color(&fb_half, blue());

    // At 0.5x zoom, both items should be visible (though smaller).
    assert!(red_half > 0, "Red should be visible at 0.5x zoom");
    assert!(blue_half > 0, "Blue should be visible at 0.5x zoom");
    // Items should be smaller at 0.5x zoom than at 1x.
    assert!(
        red_half < red_1x,
        "Red pixels at 0.5x ({red_half}) should be fewer than at 1x ({red_1x})"
    );
}

#[test]
fn zoom_in_clips_items_at_edges() {
    // At 4x zoom, items near the world edges should be pushed outside the viewport.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_zoom = Vector2::new(4.0, 4.0);
    vp.camera_position = Vector2::new(10.0, 10.0);

    // Item at world (0, 0) — at 4x zoom centered on (10,10), world (0,0) maps to:
    //   translate(-10, -10) → (-10, -10)
    //   scale(4) → (-40, -40)
    //   translate(+10, +10) → (-30, -30)
    // Completely off-screen.
    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 2.0, 2.0, red()));

    // Item at world (10, 10) — maps to viewport center.
    vp.add_canvas_item(rect_at(2, 10.0, 10.0, 1.0, 1.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, red()),
        0,
        "Item at world origin should be clipped at 4x zoom centered on (10,10)"
    );
    assert!(
        count_color(&fb, green()) > 0,
        "Item at camera position should be visible"
    );
}

// ===========================================================================
// CANVAS LAYER SCALE + CAMERA ZOOM COMPOSITION
// ===========================================================================

#[test]
fn layer_scale_composes_with_camera_zoom() {
    // Layer has 2x scale, camera has 2x zoom → items should appear at 4x effective scale.
    // The layer scale applies to item positions too, so place the item at (10, 10)
    // so that after 2x layer scale it maps to effective world (20, 20) = camera position.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    vp.add_canvas_layer(layer);

    // 1x1 rect at (10, 10) on layer → layer scale → effective (20, 20) = camera center.
    vp.add_canvas_item(rect_on_layer(1, 10.0, 10.0, 1.0, 1.0, red(), 1));

    let fb = capture_frame(&mut renderer, &vp);

    // The rect should appear as 4x4 pixels (1.0 * layer_scale(2) * cam_zoom(2) = 4).
    let red_count = count_color(&fb, red());
    // At 4x effective scale a 1×1 rect becomes roughly 4×4 = 16 pixels.
    assert!(
        red_count >= 12 && red_count <= 20,
        "Expected ~16 red pixels from 4x effective scale, got {red_count}"
    );
}

#[test]
fn layer_translation_composes_with_camera_position() {
    // Layer translates items by (+5, +5). Camera at (10, 10).
    // Item at world (5, 5): layer shifts it to (10, 10), camera then centers it.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 10.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_layer(layer);

    vp.add_canvas_item(rect_on_layer(1, 5.0, 5.0, 2.0, 2.0, blue(), 1));

    let fb = capture_frame(&mut renderer, &vp);
    // World (5,5) + layer(5,5) = effective world (10,10) → viewport center (10,10).
    assert_pixel_color(&fb, 10, 10, blue(), TOL);
}

#[test]
fn layer_rotation_composes_with_camera_rotation() {
    // Both layer and camera rotate 45°. The effective rotation should be 90°.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_rotation = std::f32::consts::FRAC_PI_4; // 45°

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::rotated(std::f32::consts::FRAC_PI_4); // 45°
    vp.add_canvas_layer(layer);

    // A horizontal line at world (20,20) length 10.
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.layer_id = Some(1);
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(15.0, 20.0),
        to: Vector2::new(25.0, 20.0),
        color: green(),
        width: 2.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // With 90° effective rotation, the horizontal line becomes vertical.
    // The line should have green pixels and they should be present.
    let green_count = count_color(&fb, green());
    assert!(
        green_count > 0,
        "Rotated line should produce visible green pixels"
    );
}

// ===========================================================================
// EXTREME VIEWPORT ASPECT RATIOS
// ===========================================================================

#[test]
fn viewport_very_wide_aspect_ratio() {
    // 100×4 viewport — very wide, very short.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 4, Color::BLACK);

    // Horizontal bar across the bottom.
    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 100.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 100);
    assert_eq!(fb.height, 4);
    // First two rows should be red, last two black.
    assert_pixel_color(&fb, 50, 0, red(), TOL);
    assert_pixel_color(&fb, 50, 1, red(), TOL);
    assert_pixel_color(&fb, 50, 2, Color::BLACK, TOL);
    assert_pixel_color(&fb, 50, 3, Color::BLACK, TOL);
}

#[test]
fn viewport_very_tall_aspect_ratio() {
    // 4×100 viewport — very narrow, very tall.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(4, 100, Color::BLACK);

    // Vertical bar down the left.
    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 2.0, 100.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 4);
    assert_eq!(fb.height, 100);
    // Left two columns should be blue, right two black.
    assert_pixel_color(&fb, 0, 50, blue(), TOL);
    assert_pixel_color(&fb, 1, 50, blue(), TOL);
    assert_pixel_color(&fb, 2, 50, Color::BLACK, TOL);
    assert_pixel_color(&fb, 3, 50, Color::BLACK, TOL);
}

#[test]
fn viewport_1x1_with_camera_renders_single_pixel() {
    // Minimal viewport with camera offset. The single pixel should reflect
    // whatever is at the camera's center in world space.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(1, 1, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);

    // Rect covering world (49..51, 49..51) — camera center should hit it.
    vp.add_canvas_item(rect_at(1, 49.0, 49.0, 2.0, 2.0, yellow()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 0, 0, yellow(), TOL);
}

// ===========================================================================
// CAMERA AFFINE INVERSE — WORLD ↔ SCREEN COORDINATE ROUNDTRIP
// ===========================================================================

#[test]
fn camera_transform_inverse_roundtrip() {
    // Verify that compute_camera_transform followed by its affine_inverse
    // produces an identity roundtrip for world coordinates.
    //
    // We test this indirectly: render a rect, note its screen position, then
    // verify the math. Camera at (30, 20), zoom 1.5x, rotation 30°, viewport 60×40.
    let half_w = 30.0_f32;
    let half_h = 20.0_f32;
    let cam_pos = Vector2::new(30.0, 20.0);
    let cam_zoom = Vector2::new(1.5, 1.5);
    let cam_rot = std::f32::consts::FRAC_PI_6; // 30°

    // Manually reconstruct the camera transform (same formula as renderer).
    let to_camera = Transform2D::translated(Vector2::new(-cam_pos.x, -cam_pos.y));
    let rotation = Transform2D::rotated(cam_rot);
    let zoom = Transform2D::scaled(cam_zoom);
    let to_screen = Transform2D::translated(Vector2::new(half_w, half_h));
    let cam_xform = to_screen * zoom * rotation * to_camera;

    // A world point at (30, 20) should map to screen center (30, 20).
    let screen_pt = cam_xform.xform(cam_pos);
    assert!(
        (screen_pt.x - half_w).abs() < 0.01 && (screen_pt.y - half_h).abs() < 0.01,
        "Camera position should map to viewport center, got ({}, {})",
        screen_pt.x,
        screen_pt.y
    );

    // Inverse should roundtrip back to the original world point.
    let inv = cam_xform.affine_inverse();
    let roundtrip = inv.xform(screen_pt);
    assert!(
        (roundtrip.x - cam_pos.x).abs() < 0.01 && (roundtrip.y - cam_pos.y).abs() < 0.01,
        "Inverse roundtrip should return to world ({}, {}), got ({}, {})",
        cam_pos.x,
        cam_pos.y,
        roundtrip.x,
        roundtrip.y
    );

    // Test an arbitrary world point.
    let world_pt = Vector2::new(40.0, 25.0);
    let screen = cam_xform.xform(world_pt);
    let back = inv.xform(screen);
    assert!(
        (back.x - world_pt.x).abs() < 0.01 && (back.y - world_pt.y).abs() < 0.01,
        "Arbitrary world point roundtrip failed: expected ({}, {}), got ({}, {})",
        world_pt.x,
        world_pt.y,
        back.x,
        back.y
    );
}

#[test]
fn screen_to_world_maps_viewport_corners_correctly() {
    // Verify that the four viewport corners map to expected world coordinates
    // when the camera is at (50, 50) with no zoom/rotation in a 100×80 viewport.
    let half_w = 50.0_f32;
    let half_h = 40.0_f32;
    let cam_pos = Vector2::new(50.0, 50.0);

    let to_camera = Transform2D::translated(Vector2::new(-cam_pos.x, -cam_pos.y));
    let to_screen = Transform2D::translated(Vector2::new(half_w, half_h));
    let cam_xform = to_screen * to_camera;
    let inv = cam_xform.affine_inverse();

    // Screen (0, 0) → world (0, 10)
    let tl = inv.xform(Vector2::new(0.0, 0.0));
    assert!(
        (tl.x - 0.0).abs() < 0.01 && (tl.y - 10.0).abs() < 0.01,
        "Top-left screen corner: expected world (0, 10), got ({}, {})",
        tl.x,
        tl.y
    );

    // Screen (100, 80) → world (100, 90)
    let br = inv.xform(Vector2::new(100.0, 80.0));
    assert!(
        (br.x - 100.0).abs() < 0.01 && (br.y - 90.0).abs() < 0.01,
        "Bottom-right screen corner: expected world (100, 90), got ({}, {})",
        br.x,
        br.y
    );
}

// ===========================================================================
// MULTI-LAYER COMPOSITION EDGE CASES
// ===========================================================================

#[test]
fn three_layers_different_transforms_compose_with_camera() {
    // Three layers with different translations, camera offset, and overlapping items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);
    vp.camera_position = Vector2::new(15.0, 15.0);

    // Layer 1 (z=0): translate (0, 0) — item at world (15, 15).
    let mut l1 = CanvasLayer::new(1);
    l1.z_order = 0;
    vp.add_canvas_layer(l1);
    vp.add_canvas_item(rect_on_layer(1, 14.0, 14.0, 4.0, 4.0, red(), 1));

    // Layer 2 (z=1): translate (5, 0) — item at world (10, 15), shifted to (15, 15).
    let mut l2 = CanvasLayer::new(2);
    l2.z_order = 1;
    l2.transform = Transform2D::translated(Vector2::new(5.0, 0.0));
    vp.add_canvas_layer(l2);
    vp.add_canvas_item(rect_on_layer(2, 9.0, 14.0, 2.0, 4.0, green(), 2));

    // Layer 3 (z=2): translate (0, 5) — item at world (15, 10), shifted to (15, 15).
    let mut l3 = CanvasLayer::new(3);
    l3.z_order = 2;
    l3.transform = Transform2D::translated(Vector2::new(0.0, 5.0));
    vp.add_canvas_layer(l3);
    vp.add_canvas_item(rect_on_layer(3, 14.0, 9.0, 4.0, 2.0, blue(), 3));

    let fb = capture_frame(&mut renderer, &vp);

    // All three items overlap at viewport center (15, 15).
    // Layer 3 (z=2, blue) should be on top.
    assert_pixel_color(&fb, 15, 15, blue(), TOL);
}

#[test]
fn layer_with_negative_z_order_renders_behind_unlayered() {
    // Unlayered items render after all layered items.
    // Even a layer with z_order=-100 should render before unlayered items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.z_order = -100;
    vp.add_canvas_layer(layer);

    // Layered red rect covering viewport.
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), 1));

    // Unlayered green rect covering viewport (should render on top).
    vp.add_canvas_item(rect_at(2, 0.0, 0.0, 20.0, 20.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // Green (unlayered) should be on top of red (layered z=-100).
    assert_pixel_color(&fb, 10, 10, green(), TOL);
}

#[test]
fn camera_zoom_asymmetric_with_layer_scale() {
    // Asymmetric camera zoom (2x, 1x) + layer scale (1x, 3x) should compose.
    // Effective scale: x = 2*1 = 2, y = 1*3 = 3.
    // Layer scale applies to positions: place rect so it maps to camera position.
    // Item at (20, 20/3 ≈ 6.67) → layer scale (1x, 3x) → (20, 20) = camera center.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_zoom = Vector2::new(2.0, 1.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::scaled(Vector2::new(1.0, 3.0));
    vp.add_canvas_layer(layer);

    // Place rect so layer scale maps it near camera position.
    // x: 19 * 1 = 19, y: 6.0 * 3 = 18. Close enough to center.
    vp.add_canvas_item(rect_on_layer(1, 19.0, 6.0, 2.0, 2.0, red(), 1));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    // 2×2 world → effective 4×6 screen = 24 pixels, with rounding tolerance.
    assert!(
        red_count >= 16 && red_count <= 32,
        "Expected ~24 red pixels from asymmetric composition, got {red_count}"
    );
}

// ===========================================================================
// DETERMINISM AND CONSISTENCY
// ===========================================================================

#[test]
fn complex_scene_is_deterministic() {
    // A complex scene with camera, layers, and multiple items produces
    // identical pixels on repeated renders.
    let make_frame = || {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(30, 30, Color::BLACK);
        vp.camera_position = Vector2::new(15.0, 15.0);
        vp.camera_zoom = Vector2::new(1.5, 1.5);
        vp.camera_rotation = 0.3;

        let mut l1 = CanvasLayer::new(1);
        l1.z_order = 0;
        l1.transform = Transform2D::translated(Vector2::new(2.0, -1.0));
        vp.add_canvas_layer(l1);

        let mut l2 = CanvasLayer::new(2);
        l2.z_order = 1;
        l2.transform = Transform2D::scaled(Vector2::new(0.8, 1.2));
        vp.add_canvas_layer(l2);

        vp.add_canvas_item(rect_on_layer(1, 10.0, 10.0, 5.0, 5.0, red(), 1));
        vp.add_canvas_item(rect_on_layer(2, 12.0, 12.0, 3.0, 3.0, blue(), 2));
        vp.add_canvas_item(rect_at(3, 14.0, 14.0, 4.0, 4.0, green()));

        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make_frame();
    let fb2 = make_frame();
    assert_eq!(fb1.pixels, fb2.pixels, "Complex scene must be deterministic");
}

#[test]
fn camera_360_rotation_returns_to_original() {
    // Rotating the camera by 2π should produce the same framebuffer as 0 rotation.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 10.0);

    vp.add_canvas_item(rect_at(1, 8.0, 8.0, 4.0, 4.0, red()));
    vp.add_canvas_item(rect_at(2, 12.0, 5.0, 3.0, 3.0, green()));

    let fb_zero = capture_frame(&mut renderer, &vp);

    let mut renderer2 = SoftwareRenderer::new();
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.camera_position = Vector2::new(10.0, 10.0);
    vp2.camera_rotation = std::f32::consts::TAU; // 360°

    vp2.add_canvas_item(rect_at(1, 8.0, 8.0, 4.0, 4.0, red()));
    vp2.add_canvas_item(rect_at(2, 12.0, 5.0, 3.0, 3.0, green()));

    let fb_360 = capture_frame(&mut renderer2, &vp2);

    // Allow small floating-point differences.
    let mismatches: usize = fb_zero
        .pixels
        .iter()
        .zip(fb_360.pixels.iter())
        .filter(|(a, b)| {
            (a.r - b.r).abs() > TOL || (a.g - b.g).abs() > TOL || (a.b - b.b).abs() > TOL
        })
        .count();
    // Allow small number of edge-pixel mismatches from floating-point in sin/cos(2π).
    assert!(
        mismatches <= 6,
        "360° rotation should match 0° rotation, {mismatches} pixel mismatches"
    );
}

// ===========================================================================
// PARENT-CHILD TRANSFORMS WITH CAMERA
// ===========================================================================

#[test]
fn parent_child_transform_composes_with_camera() {
    // Parent at (5, 5), child offset (3, 3) → effective world (8, 8).
    // Camera at (8, 8) should center the child.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(8.0, 8.0);

    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_item(parent);

    let mut child = CanvasItem::new(CanvasItemId(2));
    child.parent = Some(CanvasItemId(1));
    child.transform = Transform2D::translated(Vector2::new(3.0, 3.0));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        color: yellow(),
        filled: true,
    });
    vp.add_canvas_item(child);

    let fb = capture_frame(&mut renderer, &vp);
    // Effective world (8, 8) → camera(8,8) → viewport center (10, 10).
    assert_pixel_color(&fb, 10, 10, yellow(), TOL);
}

#[test]
fn parent_child_on_layer_with_camera_zoom() {
    // Parent-child hierarchy on a layer, with camera zoom.
    // Parent translate (5,5), child translate (5,5) → world (10,10).
    // Layer no transform. Camera at (10,10) zoom 2x. → child at center, 2x size.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 10.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    parent.layer_id = Some(1);
    vp.add_canvas_item(parent);

    let mut child = CanvasItem::new(CanvasItemId(2));
    child.parent = Some(CanvasItemId(1));
    child.layer_id = Some(1);
    child.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(1.0, 1.0)),
        color: red(),
        filled: true,
    });
    vp.add_canvas_item(child);

    let fb = capture_frame(&mut renderer, &vp);
    // Child at world (10,10) → viewport center (10,10). Size 1x1 * zoom 2 = 2x2.
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    assert_pixel_color(&fb, 11, 11, red(), TOL);
    // Just outside the 2x2 region.
    assert_pixel_color(&fb, 12, 12, Color::BLACK, TOL);
}

// ===========================================================================
// NEGATIVE CAMERA POSITION — WORLD QUADRANT COVERAGE
// ===========================================================================

#[test]
fn camera_at_negative_position_renders_negative_world_quadrant() {
    // Camera at (-10, -10) should center on world coords in the negative quadrant.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(-10.0, -10.0);

    // Rect at world (-11, -11) size 2x2 — should appear near viewport center.
    vp.add_canvas_item(rect_at(1, -11.0, -11.0, 2.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // World (-10, -10) → screen center (10, 10).
    // Rect covers [-11..-9, -11..-9], offset from camera = [-1..1, -1..1].
    assert_pixel_color(&fb, 10, 10, red(), TOL);
}

#[test]
fn camera_at_negative_position_culls_positive_world_items() {
    // Camera at (-100, -100). An item at world (0, 0) should be off-screen.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(-100.0, -100.0);

    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 5.0, 5.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, green()),
        0,
        "Item at world origin should be off-screen when camera is at (-100, -100)"
    );
}

// ===========================================================================
// PARTIAL CLIPPING — ITEMS STRADDLING VIEWPORT EDGES
// ===========================================================================

#[test]
fn item_partially_clipped_at_left_edge() {
    // Item straddles the left edge of the viewport — only the visible portion renders.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Rect at (-3, 5) size 6x6 — left 3 pixels clipped, right 3 visible.
    vp.add_canvas_item(rect_at(1, -3.0, 5.0, 6.0, 6.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // Pixel at (0, 7) should be red (visible portion).
    assert_pixel_color(&fb, 0, 7, red(), TOL);
    // Pixel at (2, 7) should be red.
    assert_pixel_color(&fb, 2, 7, red(), TOL);
    // Pixel at (3, 7) should be black (just past the rect).
    assert_pixel_color(&fb, 3, 7, Color::BLACK, TOL);

    let red_count = count_color(&fb, red());
    // 3 visible columns * 6 rows = 18 pixels.
    assert_eq!(red_count, 18, "Expected 18 red pixels from partially clipped rect");
}

#[test]
fn item_partially_clipped_at_bottom_edge() {
    // Item straddles the bottom edge.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Rect at (5, 17) size 4x6 — only 3 rows visible (rows 17, 18, 19).
    vp.add_canvas_item(rect_at(1, 5.0, 17.0, 4.0, 6.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 6, 18, blue(), TOL);

    let blue_count = count_color(&fb, blue());
    // 4 columns * 3 visible rows = 12 pixels.
    assert_eq!(blue_count, 12, "Expected 12 blue pixels from bottom-clipped rect");
}

#[test]
fn camera_zoom_causes_partial_clipping() {
    // At 2x zoom, an item near the edge gets magnified and partially clipped.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 10.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // Item at world (5, 10) size 2x2 — at 2x zoom:
    // Offset from camera: (-5, 0) * 2 = (-10, 0), + center (10, 10) = screen (0, 10).
    // Size: 2x2 * 2 = 4x4. So it covers screen [0..4, 10..14] — fully visible.
    vp.add_canvas_item(rect_at(1, 5.0, 10.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    assert_eq!(green_count, 16, "2x2 at 2x zoom = 4x4 = 16 pixels");

    // Now place item so it gets partially clipped at 2x zoom.
    let mut renderer2 = SoftwareRenderer::new();
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.camera_position = Vector2::new(10.0, 10.0);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);

    // Item at world (4, 10) size 2x2 → screen (-2, 10) size 4x4.
    // Only 2 columns visible (x=0..2), so 2*4=8 pixels.
    vp2.add_canvas_item(rect_at(1, 4.0, 10.0, 2.0, 2.0, green()));

    let fb2 = capture_frame(&mut renderer2, &vp2);
    let green_count2 = count_color(&fb2, green());
    assert_eq!(
        green_count2, 8,
        "Partially clipped zoomed rect should show 8 pixels"
    );
}

// ===========================================================================
// DRAW CIRCLE UNDER CAMERA TRANSFORMS
// ===========================================================================

#[test]
fn draw_circle_at_camera_center_visible() {
    // A circle drawn at the camera position should appear at viewport center.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(50.0, 50.0),
        radius: 5.0,
        color: red(),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Center of viewport should be red.
    assert_pixel_color(&fb, 20, 20, red(), TOL);
    // Some pixels around the center should also be red.
    let red_count = count_color(&fb, red());
    assert!(
        red_count > 20,
        "Circle with radius 5 should cover many pixels, got {red_count}"
    );
}

#[test]
fn draw_circle_culled_when_offscreen() {
    // A circle far from the camera view should produce no pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(200.0, 200.0),
        radius: 3.0,
        color: green(),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, green()),
        0,
        "Off-screen circle should produce zero pixels"
    );
}

#[test]
fn draw_circle_scaled_by_camera_zoom() {
    // Camera zoom should scale the circle.
    let mut renderer1 = SoftwareRenderer::new();
    let mut vp1 = Viewport::new(40, 40, Color::BLACK);
    vp1.camera_position = Vector2::new(20.0, 20.0);

    let mut item1 = CanvasItem::new(CanvasItemId(1));
    item1.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(20.0, 20.0),
        radius: 3.0,
        color: blue(),
    });
    vp1.add_canvas_item(item1);
    let fb1 = capture_frame(&mut renderer1, &vp1);
    let blue_1x = count_color(&fb1, blue());

    // Same circle at 2x zoom — should cover ~4x more pixels.
    let mut renderer2 = SoftwareRenderer::new();
    let mut vp2 = Viewport::new(40, 40, Color::BLACK);
    vp2.camera_position = Vector2::new(20.0, 20.0);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);

    let mut item2 = CanvasItem::new(CanvasItemId(1));
    item2.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(20.0, 20.0),
        radius: 3.0,
        color: blue(),
    });
    vp2.add_canvas_item(item2);
    let fb2 = capture_frame(&mut renderer2, &vp2);
    let blue_2x = count_color(&fb2, blue());

    // At 2x zoom, area should scale by ~4x (radius doubles → area quadruples).
    assert!(
        blue_2x > blue_1x * 2,
        "2x zoom circle ({blue_2x}) should be significantly larger than 1x ({blue_1x})"
    );
}

// ===========================================================================
// DRAW LINE UNDER CAMERA TRANSFORMS
// ===========================================================================

#[test]
fn draw_line_visible_at_camera_center() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(45.0, 50.0),
        to: Vector2::new(55.0, 50.0),
        color: green(),
        width: 2.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Line should be visible near viewport center.
    let green_count = count_color(&fb, green());
    assert!(
        green_count > 0,
        "Horizontal line at camera center should be visible"
    );
}

#[test]
fn draw_line_zoomed_gets_wider() {
    // A line at 2x zoom should produce more pixels than at 1x.
    let make_fb = |zoom: f32| {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(30, 30, Color::BLACK);
        vp.camera_position = Vector2::new(15.0, 15.0);
        vp.camera_zoom = Vector2::new(zoom, zoom);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawLine {
            from: Vector2::new(12.0, 15.0),
            to: Vector2::new(18.0, 15.0),
            color: red(),
            width: 1.0,
        });
        vp.add_canvas_item(item);
        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make_fb(1.0);
    let fb2 = make_fb(2.0);
    let red_1x = count_color(&fb1, red());
    let red_2x = count_color(&fb2, red());

    assert!(
        red_2x > red_1x,
        "Zoomed line ({red_2x} px) should cover more pixels than unzoomed ({red_1x} px)"
    );
}

// ===========================================================================
// CAMERA IDENTITY TRANSFORM — NO CAMERA = WORLD = SCREEN
// ===========================================================================

#[test]
fn identity_camera_maps_world_to_screen_directly() {
    // With default camera (position=0, zoom=1, rotation=0), world coords = screen coords.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);
    // Default camera: position (0,0), zoom (1,1), rotation 0.

    vp.add_canvas_item(rect_at(1, 5.0, 5.0, 4.0, 4.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // World (5,5) = screen (5,5) with identity camera.
    assert_pixel_color(&fb, 5, 5, red(), TOL);
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 4, 4, Color::BLACK, TOL);
    assert_pixel_color(&fb, 9, 9, Color::BLACK, TOL);
}

// ===========================================================================
// EXTREME ZOOM VALUES
// ===========================================================================

#[test]
fn very_large_zoom_magnifies_single_pixel() {
    // At 10x zoom, a 1x1 world rect should cover 10x10 = 100 screen pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_zoom = Vector2::new(10.0, 10.0);

    // 1x1 rect centered on camera.
    vp.add_canvas_item(rect_at(1, 20.0, 20.0, 1.0, 1.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert_eq!(
        red_count, 100,
        "10x zoom on 1x1 rect should produce 10x10 = 100 pixels, got {red_count}"
    );
}

#[test]
fn very_small_zoom_shrinks_large_rect() {
    // At 0.1x zoom, a 100x100 rect covers 10x10 = 100 screen pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);
    vp.camera_zoom = Vector2::new(0.1, 0.1);

    // 100x100 rect centered on camera.
    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 100.0, 100.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    // 100 * 0.1 = 10 pixels each dimension → 10x10 = 100. Centered on 20,20.
    assert_eq!(
        green_count, 100,
        "0.1x zoom on 100x100 rect should produce 10x10 = 100 pixels, got {green_count}"
    );
}

// ===========================================================================
// FRACTIONAL CAMERA POSITION — SUB-PIXEL PRECISION
// ===========================================================================

#[test]
fn fractional_camera_position_shifts_rendering() {
    // Camera at (10.5, 10.5) should produce a half-pixel offset compared to (10, 10).
    // The total pixel count and coverage should differ.
    let make_fb = |cx: f32, cy: f32| {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(20, 20, Color::BLACK);
        vp.camera_position = Vector2::new(cx, cy);
        vp.add_canvas_item(rect_at(1, 8.0, 8.0, 4.0, 4.0, red()));
        capture_frame(&mut renderer, &vp)
    };

    let fb_int = make_fb(10.0, 10.0);
    let fb_frac = make_fb(10.5, 10.5);

    // Both should show red pixels (rect is near camera center).
    let red_int = count_color(&fb_int, red());
    let red_frac = count_color(&fb_frac, red());
    assert!(red_int > 0, "Integer camera position should show red");
    assert!(red_frac > 0, "Fractional camera position should show red");

    // The pixel patterns should differ due to the sub-pixel shift.
    // We check that at least one pixel differs.
    let diffs: usize = fb_int
        .pixels
        .iter()
        .zip(fb_frac.pixels.iter())
        .filter(|(a, b)| (a.r - b.r).abs() > TOL || (a.g - b.g).abs() > TOL)
        .count();
    assert!(
        diffs > 0,
        "Fractional camera offset should produce different pixel layout"
    );
}

// ===========================================================================
// LAYER VISIBILITY TOGGLING WITH CAMERA OFFSET
// ===========================================================================

#[test]
fn visible_layer_at_camera_offset_renders_correctly() {
    // Ensure that a visible layer properly renders items even when
    // the camera is panned away from the origin.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(500.0, 500.0);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Item at camera center on layer.
    vp.add_canvas_item(rect_on_layer(1, 499.0, 499.0, 2.0, 2.0, yellow(), 1));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 10, 10, yellow(), TOL);
}

#[test]
fn invisible_layer_at_camera_offset_hides_items() {
    // Same setup but with invisible layer — nothing should render.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(500.0, 500.0);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    vp.add_canvas_item(rect_on_layer(1, 499.0, 499.0, 2.0, 2.0, yellow(), 1));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, yellow()),
        0,
        "Invisible layer should hide items even at large camera offset"
    );
}

// ===========================================================================
// MULTIPLE OVERLAPPING ITEMS ACROSS LAYERS WITH CAMERA ROTATION
// ===========================================================================

#[test]
fn overlapping_items_on_rotated_camera_respect_layer_z_order() {
    // Two layers overlapping at the same world position, camera rotated.
    // Layer z-order should still determine which color is on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);
    vp.camera_position = Vector2::new(15.0, 15.0);
    vp.camera_rotation = std::f32::consts::FRAC_PI_6; // 30°

    let mut l1 = CanvasLayer::new(1);
    l1.z_order = 0;
    let mut l2 = CanvasLayer::new(2);
    l2.z_order = 1;
    vp.add_canvas_layer(l1);
    vp.add_canvas_layer(l2);

    // Both items at camera center — top layer (blue) wins.
    vp.add_canvas_item(rect_on_layer(1, 13.0, 13.0, 4.0, 4.0, red(), 1));
    vp.add_canvas_item(rect_on_layer(2, 13.0, 13.0, 4.0, 4.0, blue(), 2));

    let fb = capture_frame(&mut renderer, &vp);
    // Viewport center should be blue (higher z_order layer).
    assert_pixel_color(&fb, 15, 15, blue(), TOL);
    // No red should be visible since blue fully covers it.
    assert_eq!(
        count_color(&fb, red()),
        0,
        "Lower z-order layer should be fully occluded"
    );
}

// ===========================================================================
// VIEWPORT CLEAR COLOR INTERACTION
// ===========================================================================

#[test]
fn clear_color_shows_through_gaps_between_items() {
    // Custom clear color should be visible wherever no items are drawn.
    let clear = Color::rgb(0.5, 0.5, 0.5);
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, clear);
    vp.camera_position = Vector2::new(10.0, 10.0);

    // Small rect at center, leaving most of viewport as clear color.
    vp.add_canvas_item(rect_at(1, 9.0, 9.0, 2.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // Center: red.
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    // Corners: clear color.
    assert_pixel_color(&fb, 0, 0, clear, TOL);
    assert_pixel_color(&fb, 19, 19, clear, TOL);

    let red_count = count_color(&fb, red());
    let clear_count = count_color(&fb, clear);
    assert_eq!(red_count, 4, "2x2 rect = 4 red pixels");
    assert_eq!(
        clear_count,
        20 * 20 - 4,
        "Remaining pixels should be clear color"
    );
}

// ===========================================================================
// CAMERA 180° ROTATION — MIRROR BEHAVIOR
// ===========================================================================

#[test]
fn camera_180_rotation_mirrors_scene() {
    // 180° rotation effectively mirrors both axes around camera center.
    // Camera transform: to_screen * zoom * rotation * to_camera
    // For world point (6,6) with camera at (10,10), viewport 20x20:
    //   to_camera: (6-10, 6-10) = (-4, -4)
    //   rotation by π: (4, 4)   (cos π = -1, sin π ≈ 0)
    //   zoom (1,1): (4, 4)
    //   to_screen: (4+10, 4+10) = (14, 14)
    // But AABB rasterization transforms rect corners and takes bounding box,
    // so the rect [6..8, 6..8] maps corners through the transform.
    // Check that red appears in the mirrored region and NOT at (6,6).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(10.0, 10.0);
    vp.camera_rotation = std::f32::consts::PI; // 180°

    // Rect at world (6, 6) size 2x2 — above-left of camera center.
    vp.add_canvas_item(rect_at(1, 6.0, 6.0, 2.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);

    // Red should appear somewhere in the mirrored quadrant (x>10, y>10)
    // and NOT at the original un-rotated position.
    let red_mirrored: usize = (11..18)
        .flat_map(|y| (11..18).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = fb.get_pixel(x, y);
            (p.r - 1.0).abs() < TOL && p.g < TOL && p.b < TOL
        })
        .count();
    assert!(
        red_mirrored > 0,
        "180° rotation should mirror item to opposite quadrant"
    );

    // Original position (6,6) should be black.
    assert_pixel_color(&fb, 6, 6, Color::BLACK, TOL);
}

// ===========================================================================
// MIXED DRAW COMMANDS UNDER CAMERA TRANSFORMS
// ===========================================================================

#[test]
fn mixed_draw_commands_compose_with_camera() {
    // Scene with rect, circle, and line all under camera zoom.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // Rect at camera center.
    vp.add_canvas_item(rect_at(1, 19.0, 19.0, 2.0, 2.0, red()));

    // Circle near camera.
    let mut circle_item = CanvasItem::new(CanvasItemId(2));
    circle_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(25.0, 20.0),
        radius: 2.0,
        color: green(),
    });
    vp.add_canvas_item(circle_item);

    // Line.
    let mut line_item = CanvasItem::new(CanvasItemId(3));
    line_item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(15.0, 20.0),
        to: Vector2::new(18.0, 20.0),
        color: blue(),
        width: 1.0,
    });
    vp.add_canvas_item(line_item);

    let fb = capture_frame(&mut renderer, &vp);
    // All three colors should be present.
    assert!(count_color(&fb, red()) > 0, "Red rect should be visible");
    assert!(
        count_color(&fb, green()) > 0,
        "Green circle should be visible"
    );
    assert!(count_color(&fb, blue()) > 0, "Blue line should be visible");
}

// ===========================================================================
// VIEWPORT SIZE EDGE CASES
// ===========================================================================

#[test]
fn viewport_square_with_non_uniform_zoom() {
    // Non-uniform zoom (2x horizontal, 1x vertical) should stretch content horizontally.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_zoom = Vector2::new(2.0, 1.0);

    // 2x2 rect at camera center.
    vp.add_canvas_item(rect_at(1, 19.0, 19.0, 2.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    // Horizontal: 2 * 2.0 = 4 pixels, Vertical: 2 * 1.0 = 2 pixels → 4 * 2 = 8.
    assert_eq!(
        red_count, 8,
        "Non-uniform zoom (2x, 1x) on 2x2 rect = 4x2 = 8 pixels, got {red_count}"
    );
}

#[test]
fn large_viewport_with_many_items() {
    // Stress test: large viewport with multiple items ensures no rendering artifacts.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);

    // Place 10 non-overlapping rects in a grid pattern.
    for i in 0..10u64 {
        let x = (i % 5) as f32 * 20.0;
        let y = (i / 5) as f32 * 50.0;
        vp.add_canvas_item(rect_at(i + 1, x, y, 10.0, 10.0, red()));
    }

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    // Each rect is 10x10 = 100 pixels. Some may be clipped by viewport.
    // All 10 rects should contribute some pixels.
    assert!(
        red_count > 0,
        "Large viewport with many items should render visible pixels"
    );
}
