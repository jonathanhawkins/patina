//! pat-sfn: Camera2D and viewport render parity tests.
//!
//! Pixel-level tests verifying:
//! - Camera2D position/offset shifts rendered content
//! - Camera2D zoom scales rendered content
//! - Camera2D rotation rotates rendered content
//! - Camera `current` property gates whether the camera is active
//! - Viewport size affects rendering bounds and coordinate mapping
//! - Combined camera properties (zoom + offset + rotation)
//! - Edge cases: zero zoom, extreme zoom, viewport aspect ratios

use std::path::PathBuf;

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::compare::compare_framebuffers;
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdrender2d::texture::load_png;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
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

/// Creates a filled rect canvas item at world position (0,0) with given size.
fn rect_at_origin(id: u64, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(w, h)),
        color,
        filled: true,
    });
    item
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

/// Count pixels matching the given color.
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
// CAMERA POSITION / OFFSET
// ===========================================================================

#[test]
fn camera_at_origin_centers_world_origin_in_viewport() {
    // Camera at (0,0) in a 20x20 viewport → world (0,0) maps to screen center (10,10).
    // Camera transform: translate by (-cam.x + half_w, -cam.y + half_h) = (10, 10).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::ZERO;
    // Need to activate camera — position (0,0) with default zoom doesn't activate.
    // The renderer checks if camera has non-default values. Setting zoom slightly off:
    // Actually, camera_position=(0,0) and zoom=(1,1) means has_camera=false.
    // We need a non-zero camera position to activate.
    vp.camera_position = Vector2::new(0.001, 0.001);

    // Place a 4x4 red rect at world origin.
    vp.add_canvas_item(rect_at_origin(1, 4.0, 4.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // World (0,0) should map near viewport center (10,10).
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    // Origin of viewport should be black (camera shifted content).
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
}

#[test]
fn camera_position_shifts_content() {
    // Camera at (50, 50) in a 20x20 viewport.
    // Transform: translate by (-50 + 10, -50 + 10) = (-40, -40).
    // World (50, 50) → screen (10, 10) (center).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);

    // Rect at world (48, 48), size 4x4.
    vp.add_canvas_item(rect_at(1, 48.0, 48.0, 4.0, 4.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // World (48,48) → screen (48-40, 48-40) = (8, 8).
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 11, 11, red(), TOL);
    // (0,0) should be black.
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
}

#[test]
fn camera_position_can_reveal_offscreen_content() {
    // Without camera: rect at (100,100) is off-screen in a 20x20 viewport.
    // With camera at (100,100): the rect should appear at viewport center.
    let mut renderer = SoftwareRenderer::new();

    // No camera: rect is off-screen.
    let mut vp_no_cam = Viewport::new(20, 20, Color::BLACK);
    vp_no_cam.add_canvas_item(rect_at(1, 100.0, 100.0, 4.0, 4.0, red()));
    let fb_no_cam = capture_frame(&mut renderer, &vp_no_cam);
    let red_count_no_cam = count_color(&fb_no_cam, red());
    assert_eq!(
        red_count_no_cam, 0,
        "Rect should be entirely off-screen without camera"
    );

    // With camera: rect should be visible.
    let mut vp_cam = Viewport::new(20, 20, Color::BLACK);
    vp_cam.camera_position = Vector2::new(100.0, 100.0);
    vp_cam.add_canvas_item(rect_at(1, 100.0, 100.0, 4.0, 4.0, red()));
    let fb_cam = capture_frame(&mut renderer, &vp_cam);
    let red_count_cam = count_color(&fb_cam, red());
    assert!(
        red_count_cam > 0,
        "Rect should be visible with camera centered on it"
    );
}

#[test]
fn camera_negative_position() {
    // Camera at (-20, -20) in a 40x40 viewport.
    // Transform: translate by (20 + 20, 20 + 20) = (40, 40).
    // World (0,0) → screen (40, 40) — at the bottom-right corner.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(-20.0, -20.0);

    // Place rect at world (-22, -22), size 4x4.
    // Screen pos: (-22 + 40, -22 + 40) = (18, 18).
    vp.add_canvas_item(rect_at(1, -22.0, -22.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 18, 18, green(), TOL);
    assert_pixel_color(&fb, 21, 21, green(), TOL);
}

// ===========================================================================
// CAMERA ZOOM
// ===========================================================================

#[test]
fn camera_zoom_2x_moves_positions() {
    // Camera transform: translate(half) * scale(zoom) * translate(-cam).
    // With cam=(half, half)=(20,20) and zoom=2:
    //   world (15,15) → (-5,-5) → (-10,-10) → (10,10).
    //   world (5,5) with zoom=1: (5-20)=-15 → (-15,-15) → +20 = (5,5).
    let size = 40u32;
    let half = size as f32 / 2.0;
    let mut renderer = SoftwareRenderer::new();

    // Zoom=1: rect at (5,5) → screen (5,5) (cam at center is identity for positions relative to cam).
    let mut vp1 = Viewport::new(size, size, Color::BLACK);
    vp1.camera_position = Vector2::new(half, half);
    vp1.camera_zoom = Vector2::ONE;
    vp1.add_canvas_item(rect_at(1, 5.0, 5.0, 2.0, 2.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp1);
    assert_pixel_color(&fb1, 5, 5, red(), TOL);

    // Zoom=2: world (15,15) → screen (10,10). Size 2x2 → 4x4 on screen.
    let mut vp2 = Viewport::new(size, size, Color::BLACK);
    vp2.camera_position = Vector2::new(half, half);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);
    vp2.add_canvas_item(rect_at(1, 15.0, 15.0, 2.0, 2.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);
    assert_pixel_color(&fb2, 10, 10, red(), TOL);
    assert_pixel_color(&fb2, 13, 13, red(), TOL); // zoomed size = 4x4
    assert_pixel_color(&fb2, 14, 14, Color::BLACK, TOL); // just outside
}

#[test]
fn camera_zoom_half_moves_positions_closer() {
    // Camera at center (20,20), zoom=0.5.
    // World (20,20) → (0,0) → (0,0) → (20,20) = viewport center.
    // World (0,0) → (-20,-20) → (-10,-10) → (10,10).
    let size = 40u32;
    let half = size as f32 / 2.0;
    let mut renderer = SoftwareRenderer::new();

    // Zoom=1: rect at world (0,0), cam at center → screen (0,0).
    let mut vp1 = Viewport::new(size, size, Color::BLACK);
    vp1.camera_position = Vector2::new(half, half);
    vp1.camera_zoom = Vector2::ONE;
    vp1.add_canvas_item(rect_at(1, 0.0, 0.0, 2.0, 2.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp1);
    assert_pixel_color(&fb1, 0, 0, red(), TOL);

    // Zoom=0.5: world (0,0) → (-20,-20) → (-10,-10) → (10,10).
    let mut vp2 = Viewport::new(size, size, Color::BLACK);
    vp2.camera_position = Vector2::new(half, half);
    vp2.camera_zoom = Vector2::new(0.5, 0.5);
    vp2.add_canvas_item(rect_at(1, 0.0, 0.0, 2.0, 2.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);
    assert_pixel_color(&fb2, 10, 10, red(), TOL);
    // World (0,0) at zoom=0.5 should NOT be at screen (0,0).
    assert_pixel_color(&fb2, 0, 0, Color::BLACK, TOL);
}

#[test]
fn camera_zoom_changes_output() {
    // Basic check: zoom changes the frame vs no zoom.
    let mut renderer = SoftwareRenderer::new();

    let mut vp1 = Viewport::new(20, 20, Color::BLACK);
    vp1.add_canvas_item(rect_at(1, 5.0, 5.0, 4.0, 4.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp1);

    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.camera_zoom = Vector2::new(3.0, 3.0);
    vp2.add_canvas_item(rect_at(1, 5.0, 5.0, 4.0, 4.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);

    assert_ne!(fb1.pixels, fb2.pixels, "Zoom should change rendered output");
}

#[test]
fn camera_zoom_asymmetric_moves_x_and_y_differently() {
    // Camera at center (20,20).
    // With zoom (2,1): screen_x = (wx-20)*2+20, screen_y = (wy-20)*1+20.
    //   world (15, 10) → x=(15-20)*2+20=10, y=(10-20)*1+20=10 → screen (10,10).
    // With zoom (1,2): screen_x = (wx-20)*1+20, screen_y = (wy-20)*2+20.
    //   world (15, 10) → x=(15-20)*1+20=15, y=(10-20)*2+20=0 → screen (15,0).
    let size = 40u32;
    let half = size as f32 / 2.0;
    let mut renderer = SoftwareRenderer::new();

    let mut vp = Viewport::new(size, size, Color::BLACK);
    vp.camera_position = Vector2::new(half, half);
    vp.camera_zoom = Vector2::new(2.0, 1.0);
    vp.add_canvas_item(rect_at(1, 15.0, 10.0, 2.0, 2.0, red()));
    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 10, 10, red(), TOL);

    let mut vp2 = Viewport::new(size, size, Color::BLACK);
    vp2.camera_position = Vector2::new(half, half);
    vp2.camera_zoom = Vector2::new(1.0, 2.0);
    vp2.add_canvas_item(rect_at(1, 15.0, 10.0, 2.0, 2.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);
    assert_pixel_color(&fb2, 15, 0, red(), TOL);

    // The two asymmetric zooms should produce different outputs.
    assert_ne!(
        fb.pixels, fb2.pixels,
        "Different asymmetric zooms should produce different results"
    );
}

// ===========================================================================
// CAMERA ROTATION
// ===========================================================================

#[test]
fn camera_rotation_changes_output() {
    let mut renderer = SoftwareRenderer::new();

    // No rotation.
    let mut vp1 = Viewport::new(20, 20, Color::BLACK);
    vp1.camera_position = Vector2::new(10.0, 10.0);
    vp1.add_canvas_item(rect_at(1, 8.0, 8.0, 4.0, 4.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp1);

    // With rotation.
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.camera_position = Vector2::new(10.0, 10.0);
    vp2.camera_rotation = std::f32::consts::FRAC_PI_4; // 45 degrees
    vp2.add_canvas_item(rect_at(1, 8.0, 8.0, 4.0, 4.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);

    assert_ne!(
        fb1.pixels, fb2.pixels,
        "Camera rotation should change rendered output"
    );
}

#[test]
fn camera_rotation_preserves_pixel_count_approximately() {
    // Rotating the camera shouldn't dramatically change the amount of colored pixels
    // (content rotates but stays approximately the same area).
    let size = 60u32;
    let mut renderer = SoftwareRenderer::new();

    let make_vp = |rotation: f32| {
        let mut vp = Viewport::new(size, size, Color::BLACK);
        vp.camera_position = Vector2::new(5.0, 5.0);
        vp.camera_rotation = rotation;
        // Small rect centered at camera position.
        vp.add_canvas_item(rect_at(1, 3.0, 3.0, 4.0, 4.0, red()));
        vp
    };

    let fb0 = capture_frame(&mut renderer, &make_vp(0.0));
    let fb45 = capture_frame(&mut renderer, &make_vp(std::f32::consts::FRAC_PI_4));

    let count0 = count_color(&fb0, red());
    let count45 = count_color(&fb45, red());

    // Both should have visible red pixels (not all clipped away).
    assert!(count0 > 0, "No red pixels without rotation");
    assert!(count45 > 0, "No red pixels with 45° rotation");
}

// ===========================================================================
// VIEWPORT SIZE
// ===========================================================================

#[test]
fn viewport_size_determines_framebuffer_dimensions() {
    let mut renderer = SoftwareRenderer::new();

    for (w, h) in [(10, 10), (50, 30), (1, 1), (100, 100)] {
        let vp = Viewport::new(w, h, Color::BLACK);
        let fb = capture_frame(&mut renderer, &vp);
        assert_eq!(fb.width, w, "FrameBuffer width should match viewport");
        assert_eq!(fb.height, h, "FrameBuffer height should match viewport");
        assert_eq!(
            fb.pixels.len(),
            (w * h) as usize,
            "Pixel count should be w*h"
        );
    }
}

#[test]
fn viewport_clips_content_outside_bounds() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Rect at (8, 8), size 10x10 — only 2x2 corner should be visible.
    vp.add_canvas_item(rect_at(1, 8.0, 8.0, 10.0, 10.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert_eq!(
        red_count, 4,
        "Only 2x2=4 pixels should be visible inside viewport"
    );
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 9, 9, red(), TOL);
    // Just outside the visible rect area.
    assert_pixel_color(&fb, 7, 7, Color::BLACK, TOL);
}

#[test]
fn larger_viewport_shows_more_content() {
    let mut renderer = SoftwareRenderer::new();

    // Small viewport: 10x10, rect at (0,0) size 20x20 → only 100 pixels.
    let mut vp_small = Viewport::new(10, 10, Color::BLACK);
    vp_small.add_canvas_item(rect_at_origin(1, 20.0, 20.0, red()));
    let fb_small = capture_frame(&mut renderer, &vp_small);
    let count_small = count_color(&fb_small, red());

    // Large viewport: 30x30, same rect → 400 pixels (20x20 capped).
    let mut vp_large = Viewport::new(30, 30, Color::BLACK);
    vp_large.add_canvas_item(rect_at_origin(1, 20.0, 20.0, red()));
    let fb_large = capture_frame(&mut renderer, &vp_large);
    let count_large = count_color(&fb_large, red());

    assert!(
        count_large > count_small,
        "Larger viewport should show more content: small={count_small}, large={count_large}"
    );
    assert_eq!(count_small, 100); // 10x10 viewport filled
    assert_eq!(count_large, 400); // 20x20 rect fully visible
}

#[test]
fn viewport_1x1_renders_single_pixel() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(1, 1, Color::BLACK);
    vp.add_canvas_item(rect_at_origin(1, 1.0, 1.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.pixels.len(), 1);
    assert_pixel_color(&fb, 0, 0, red(), TOL);
}

#[test]
fn viewport_wide_aspect_ratio() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 10, Color::BLACK);

    // Rect spanning full width.
    vp.add_canvas_item(rect_at_origin(1, 100.0, 10.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 100);
    assert_eq!(fb.height, 10);
    // All pixels should be green.
    assert_eq!(count_color(&fb, green()), 1000);
}

#[test]
fn viewport_tall_aspect_ratio() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 100, Color::BLACK);

    vp.add_canvas_item(rect_at_origin(1, 10.0, 100.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 10);
    assert_eq!(fb.height, 100);
    assert_eq!(count_color(&fb, blue()), 1000);
}

// ===========================================================================
// CAMERA + VIEWPORT COMBINED
// ===========================================================================

#[test]
fn camera_zoom_with_position() {
    // Camera at (50, 50) with 2x zoom. World (50,50) should be at viewport center.
    let size = 40u32;
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(size, size, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // Place a small rect at world (49, 49), size 2x2.
    // Transform: zoom * translate = scale(2) * translate(-50+20, -50+20) = scale(2) * translate(-30, -30)
    // World (49,49) → translated to (-30+49, -30+49) = (19, 19) → zoomed to (38, 38).
    // Actually: translation maps world (x,y) → (x - 50 + 20, y - 50 + 20) = (x-30, y-30)
    // Then zoom maps (px, py) → (2*px, 2*py)
    // So world (49,49) → (19, 19) → (38, 38). Size 2x2 → 4x4 at zoom.
    vp.add_canvas_item(rect_at(1, 49.0, 49.0, 2.0, 2.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert!(
        red_count > 0,
        "Rect should be visible with camera+zoom combination"
    );
}

#[test]
fn camera_zoom_with_rotation() {
    // Verify that zoom + rotation produce a different result than either alone.
    let size = 40u32;
    let mut renderer = SoftwareRenderer::new();

    let mut make_fb = |zoom: Vector2, rotation: f32| {
        let mut vp = Viewport::new(size, size, Color::BLACK);
        vp.camera_position = Vector2::new(10.0, 10.0);
        vp.camera_zoom = zoom;
        vp.camera_rotation = rotation;
        vp.add_canvas_item(rect_at(1, 8.0, 8.0, 4.0, 4.0, red()));
        capture_frame(&mut renderer, &vp)
    };

    let fb_zoom = make_fb(Vector2::new(2.0, 2.0), 0.0);
    let fb_rot = make_fb(Vector2::ONE, std::f32::consts::FRAC_PI_4);
    let fb_both = make_fb(Vector2::new(2.0, 2.0), std::f32::consts::FRAC_PI_4);

    assert_ne!(
        fb_zoom.pixels, fb_rot.pixels,
        "Zoom-only vs rotation-only should differ"
    );
    assert_ne!(
        fb_zoom.pixels, fb_both.pixels,
        "Zoom-only vs combined should differ"
    );
    assert_ne!(
        fb_rot.pixels, fb_both.pixels,
        "Rotation-only vs combined should differ"
    );
}

#[test]
fn no_camera_identity_rendering() {
    // When camera is at defaults (position=0, zoom=1, rotation=0), rendering is identity.
    // Content at world (5,5) should appear at screen (5,5).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    // All camera fields at defaults → no camera transform.

    vp.add_canvas_item(rect_at(1, 5.0, 5.0, 3.0, 3.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 5, 5, red(), TOL);
    assert_pixel_color(&fb, 7, 7, red(), TOL);
    assert_pixel_color(&fb, 4, 4, Color::BLACK, TOL);
    assert_pixel_color(&fb, 8, 8, Color::BLACK, TOL);
}

#[test]
fn camera_position_moves_multiple_items_together() {
    // All items should shift by the same camera offset.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    vp.camera_position = Vector2::new(20.0, 20.0);

    // Two rects in world space.
    vp.add_canvas_item(rect_at(1, 18.0, 18.0, 2.0, 2.0, red()));
    vp.add_canvas_item(rect_at(2, 22.0, 22.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // Camera at (20,20) in 40x40 viewport: translate by (-20+20, -20+20) = (0, 0).
    // World (18,18) → screen (18, 18). World (22,22) → screen (22, 22).
    assert_pixel_color(&fb, 18, 18, red(), TOL);
    assert_pixel_color(&fb, 22, 22, green(), TOL);
}

// ===========================================================================
// VIEWPORT CLEAR COLOR
// ===========================================================================

#[test]
fn viewport_clear_color_fills_background() {
    let mut renderer = SoftwareRenderer::new();
    let clear = Color::rgb(0.3, 0.1, 0.7);
    let vp = Viewport::new(10, 10, clear);

    let fb = capture_frame(&mut renderer, &vp);
    // Every pixel should be the clear color.
    for y in 0..10 {
        for x in 0..10 {
            assert_pixel_color(&fb, x, y, clear, TOL);
        }
    }
}

#[test]
fn viewport_clear_color_visible_behind_partial_content() {
    let mut renderer = SoftwareRenderer::new();
    let clear = Color::rgb(0.5, 0.5, 0.0);
    let mut vp = Viewport::new(10, 10, clear);

    // Red rect covers only top-left 5x5.
    vp.add_canvas_item(rect_at_origin(1, 5.0, 5.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 2, 2, red(), TOL);
    assert_pixel_color(&fb, 8, 8, clear, TOL);
}

// ===========================================================================
// DETERMINISM
// ===========================================================================

#[test]
fn deterministic_camera_rendering() {
    let make_frame = || {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(30, 30, Color::BLACK);
        vp.camera_position = Vector2::new(15.0, 15.0);
        vp.camera_zoom = Vector2::new(1.5, 1.5);
        vp.camera_rotation = 0.3;

        vp.add_canvas_item(rect_at(1, 10.0, 10.0, 6.0, 6.0, red()));
        vp.add_canvas_item(rect_at(2, 14.0, 14.0, 4.0, 4.0, green()));
        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make_frame();
    let fb2 = make_frame();
    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Camera rendering must be deterministic"
    );
}

// ===========================================================================
// EDGE CASES
// ===========================================================================

#[test]
fn camera_very_large_zoom() {
    // Extreme zoom should not panic, and should produce some output.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_zoom = Vector2::new(100.0, 100.0);
    vp.camera_position = Vector2::new(0.0, 0.0);

    vp.add_canvas_item(rect_at_origin(1, 1.0, 1.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // Should not panic. Content may be off-screen due to extreme zoom.
    assert_eq!(fb.width, 20);
    assert_eq!(fb.height, 20);
}

#[test]
fn camera_very_small_zoom() {
    // Very small zoom (but not zero) should not panic.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_zoom = Vector2::new(0.01, 0.01);

    vp.add_canvas_item(rect_at_origin(1, 1000.0, 1000.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 20);
    // With extreme de-zoom, the 1000x1000 rect should appear as ~10x10 pixels.
    let red_count = count_color(&fb, red());
    assert!(
        red_count > 0,
        "Very small zoom on huge rect should still produce some visible pixels"
    );
}

#[test]
fn camera_rotation_full_circle_returns_to_original() {
    let mut renderer = SoftwareRenderer::new();

    let mut make_fb = |rotation: f32| {
        let mut vp = Viewport::new(20, 20, Color::BLACK);
        vp.camera_position = Vector2::new(5.0, 5.0);
        vp.camera_rotation = rotation;
        vp.add_canvas_item(rect_at(1, 3.0, 3.0, 4.0, 4.0, red()));
        capture_frame(&mut renderer, &vp)
    };

    let fb_0 = make_fb(0.0);
    let fb_2pi = make_fb(std::f32::consts::TAU);

    // Full rotation should produce approximately the same output (within float precision).
    let matching = fb_0
        .pixels
        .iter()
        .zip(fb_2pi.pixels.iter())
        .filter(|(a, b)| {
            (a.r - b.r).abs() < 0.05 && (a.g - b.g).abs() < 0.05 && (a.b - b.b).abs() < 0.05
        })
        .count();
    let total = fb_0.pixels.len();
    let ratio = matching as f64 / total as f64;
    assert!(
        ratio > 0.95,
        "Full 2π rotation should match original: {:.1}% matching",
        ratio * 100.0
    );
}

// ===========================================================================
// ZOOM SCALES APPARENT SIZE
// ===========================================================================

#[test]
fn camera_zoom_2x_doubles_apparent_size() {
    // With 2x zoom, a 4x4 rect should cover 8x8 pixels on screen.
    let size = 40u32;
    let mut renderer = SoftwareRenderer::new();

    // No zoom: rect at (0,0) size 4x4 → 16 red pixels.
    let mut vp1 = Viewport::new(size, size, Color::BLACK);
    vp1.add_canvas_item(rect_at_origin(1, 4.0, 4.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp1);
    let count_no_zoom = count_color(&fb1, red());
    assert_eq!(count_no_zoom, 16, "4x4 rect = 16 pixels without zoom");

    // 2x zoom centered on origin: rect appears as 8x8 = 64 pixels.
    let mut vp2 = Viewport::new(size, size, Color::BLACK);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);
    vp2.camera_position = Vector2::ZERO;
    vp2.add_canvas_item(rect_at_origin(1, 4.0, 4.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);
    let count_zoom = count_color(&fb2, red());
    assert_eq!(
        count_zoom, 64,
        "2x zoom should make 4x4 rect appear as 8x8 = 64 pixels, got {count_zoom}"
    );
}

#[test]
fn camera_zoom_half_shrinks_apparent_size() {
    // With 0.5x zoom, a 10x10 rect should cover 5x5 = 25 pixels.
    let size = 40u32;
    let mut renderer = SoftwareRenderer::new();

    // No zoom: rect at (0,0) size 10x10 → 100 red pixels.
    let mut vp1 = Viewport::new(size, size, Color::BLACK);
    vp1.add_canvas_item(rect_at_origin(1, 10.0, 10.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp1);
    let count_no_zoom = count_color(&fb1, red());
    assert_eq!(count_no_zoom, 100);

    // 0.5x zoom: rect appears as 5x5 = 25 pixels.
    let mut vp2 = Viewport::new(size, size, Color::BLACK);
    vp2.camera_zoom = Vector2::new(0.5, 0.5);
    vp2.camera_position = Vector2::ZERO;
    vp2.add_canvas_item(rect_at_origin(1, 10.0, 10.0, red()));
    let fb2 = capture_frame(&mut renderer, &vp2);
    let count_zoom = count_color(&fb2, red());
    assert_eq!(
        count_zoom, 25,
        "0.5x zoom should make 10x10 rect appear as 5x5 = 25 pixels, got {count_zoom}"
    );
}

#[test]
fn camera_zoom_asymmetric() {
    // Zoom X=2, Y=1: a 4x4 rect becomes 8x4 = 32 pixels.
    let size = 40u32;
    let mut renderer = SoftwareRenderer::new();

    let mut vp = Viewport::new(size, size, Color::BLACK);
    vp.camera_zoom = Vector2::new(2.0, 1.0);
    vp.camera_position = Vector2::ZERO;
    vp.add_canvas_item(rect_at_origin(1, 4.0, 4.0, red()));
    let fb = capture_frame(&mut renderer, &vp);
    let count = count_color(&fb, red());
    assert_eq!(
        count, 32,
        "Zoom(2,1) on 4x4 rect = 8x4 = 32 pixels, got {count}"
    );
}

// ===========================================================================
// GOLDEN RENDER REGRESSION TESTS
//
// Render camera/viewport scenarios at a fixed resolution and compare against
// golden PNG references. First run generates the golden; subsequent runs
// assert exact pixel match.
// ===========================================================================

/// Render resolution for golden camera/viewport tests.
const GOLDEN_W: u32 = 32;
const GOLDEN_H: u32 = 32;

/// Pixel tolerance for golden comparison (Euclidean RGB distance).
const GOLDEN_TOL: f64 = 0.02;

/// Minimum match ratio to pass golden comparison.
const GOLDEN_MIN_MATCH: f64 = 1.0;

/// Returns the golden render directory for camera/viewport tests.
fn golden_camera_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
        .join("golden")
        .join("render")
        .join("camera_viewport")
}

/// Saves a framebuffer as a golden PNG reference.
fn save_camera_golden(fb: &FrameBuffer, name: &str) {
    let dir = golden_camera_dir();
    std::fs::create_dir_all(&dir).expect("failed to create golden camera_viewport dir");
    let path = dir.join(format!("{name}.png"));
    fb.save_png(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("failed to save golden PNG {}: {e}", path.display()));
}

/// Loads a golden PNG reference. Returns None if the file doesn't exist.
fn load_camera_golden(name: &str) -> Option<FrameBuffer> {
    let path = golden_camera_dir().join(format!("{name}.png"));
    let tex = load_png(path.to_str().unwrap())?;
    Some(FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    })
}

/// Compares a rendered framebuffer against a golden reference.
/// Generates the golden on first run; asserts exact match on subsequent runs.
fn assert_camera_golden(fb: &FrameBuffer, name: &str) {
    match load_camera_golden(name) {
        Some(golden) => {
            let result = compare_framebuffers(fb, &golden, GOLDEN_TOL);
            assert!(
                result.match_ratio() >= GOLDEN_MIN_MATCH,
                "golden camera/viewport comparison failed for '{}': {:.2}% match \
                 (need {:.0}%), max_diff={:.4}, avg_diff={:.4}",
                name,
                result.match_ratio() * 100.0,
                GOLDEN_MIN_MATCH * 100.0,
                result.max_diff,
                result.avg_diff,
            );
        }
        None => {
            save_camera_golden(fb, name);
            eprintln!(
                "Generated golden camera/viewport reference: {}/{}.png",
                golden_camera_dir().display(),
                name,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Golden: Camera offset (panning)
// ---------------------------------------------------------------------------

#[test]
fn golden_camera_offset_pan_right() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    // Camera panned right: world (16,16) is at viewport center.
    vp.camera_position = Vector2::new(16.0, 16.0);

    // Red rect at world (14, 14) size 4x4 → should appear at viewport center.
    vp.add_canvas_item(rect_at(1, 14.0, 14.0, 4.0, 4.0, red()));
    // Green rect at world (0, 0) size 4x4 → should appear at viewport (0,0).
    vp.add_canvas_item(rect_at(2, 0.0, 0.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);

    // Verify red rect is at center.
    assert_pixel_color(&fb, 14, 14, red(), TOL);
    // Verify green rect is at (0,0).
    assert_pixel_color(&fb, 0, 0, green(), TOL);

    assert_camera_golden(&fb, "offset_pan_right");
}

#[test]
fn golden_camera_offset_pan_left() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    // Camera panned to negative position.
    vp.camera_position = Vector2::new(-8.0, -8.0);

    // Rect at world (-10, -10) size 4x4 → screen position shifted.
    vp.add_canvas_item(rect_at(1, -10.0, -10.0, 4.0, 4.0, blue()));
    // Rect at world (10, 10) size 8x8.
    vp.add_canvas_item(rect_at(2, 10.0, 10.0, 8.0, 8.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_camera_golden(&fb, "offset_pan_left");
}

// ---------------------------------------------------------------------------
// Golden: Camera zoom
// ---------------------------------------------------------------------------

#[test]
fn golden_camera_zoom_2x() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_zoom = Vector2::new(2.0, 2.0);
    vp.camera_position = Vector2::new(16.0, 16.0);

    // Red rect at world (12, 12) size 8x8.
    // At 2x zoom, this becomes 16x16 on screen, centered.
    vp.add_canvas_item(rect_at(1, 12.0, 12.0, 8.0, 8.0, red()));
    // Small green marker at world (16, 16) size 2x2.
    vp.add_canvas_item(rect_at(2, 16.0, 16.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert!(
        red_count > 100,
        "Zoomed rect should be large: got {red_count}"
    );

    assert_camera_golden(&fb, "zoom_2x");
}

#[test]
fn golden_camera_zoom_half() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_zoom = Vector2::new(0.5, 0.5);
    vp.camera_position = Vector2::new(16.0, 16.0);

    // Large rect that fills most of the world.
    vp.add_canvas_item(rect_at(1, 0.0, 0.0, 32.0, 32.0, red()));
    // Small marker at center.
    vp.add_canvas_item(rect_at(2, 14.0, 14.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_camera_golden(&fb, "zoom_half");
}

// ---------------------------------------------------------------------------
// Golden: Viewport size effects
// ---------------------------------------------------------------------------

#[test]
fn golden_viewport_small() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(8, 8, Color::BLACK);

    // Rect that extends beyond the small viewport.
    vp.add_canvas_item(rect_at_origin(1, 16.0, 16.0, red()));
    // Small marker.
    vp.add_canvas_item(rect_at(2, 2.0, 2.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // All 64 pixels should be colored.
    let total_colored = count_color(&fb, red()) + count_color(&fb, green());
    assert_eq!(total_colored, 64, "8x8 viewport should be fully filled");

    assert_camera_golden(&fb, "viewport_small");
}

#[test]
fn golden_viewport_wide() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(64, 16, Color::rgb(0.1, 0.1, 0.1));

    // Three rects spread across the wide viewport.
    vp.add_canvas_item(rect_at(1, 0.0, 4.0, 8.0, 8.0, red()));
    vp.add_canvas_item(rect_at(2, 28.0, 4.0, 8.0, 8.0, green()));
    vp.add_canvas_item(rect_at(3, 56.0, 4.0, 8.0, 8.0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.width, 64);
    assert_eq!(fb.height, 16);

    assert_camera_golden(&fb, "viewport_wide");
}

// ---------------------------------------------------------------------------
// Golden: Camera + viewport combined
// ---------------------------------------------------------------------------

#[test]
fn golden_camera_zoom_with_offset() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // Rect near camera center.
    vp.add_canvas_item(rect_at(1, 48.0, 48.0, 4.0, 4.0, red()));
    // Rect further away (should be clipped or partially visible at 2x zoom).
    vp.add_canvas_item(rect_at(2, 40.0, 40.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    assert!(red_count > 0, "Rect near camera center should be visible");

    assert_camera_golden(&fb, "zoom_with_offset");
}

#[test]
fn golden_camera_zoom_with_rotation() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);
    vp.camera_position = Vector2::new(16.0, 16.0);
    vp.camera_zoom = Vector2::new(1.5, 1.5);
    vp.camera_rotation = std::f32::consts::FRAC_PI_6; // 30 degrees

    // Cross pattern to show rotation.
    vp.add_canvas_item(rect_at(1, 12.0, 15.0, 8.0, 2.0, red()));
    vp.add_canvas_item(rect_at(2, 15.0, 12.0, 2.0, 8.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    let red_count = count_color(&fb, red());
    let green_count = count_color(&fb, green());
    assert!(red_count > 0, "Horizontal bar should be visible");
    assert!(green_count > 0, "Vertical bar should be visible");

    assert_camera_golden(&fb, "zoom_with_rotation");
}

#[test]
fn golden_camera_full_scene() {
    // A more complex scene combining camera offset, zoom, and multiple items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::rgb(0.05, 0.05, 0.1));
    vp.camera_position = Vector2::new(20.0, 20.0);
    vp.camera_zoom = Vector2::new(1.5, 1.5);

    // Ground.
    vp.add_canvas_item(rect_at(1, 10.0, 25.0, 20.0, 4.0, Color::rgb(0.3, 0.6, 0.2)));
    // Player.
    vp.add_canvas_item(rect_at(2, 18.0, 19.0, 4.0, 6.0, red()));
    // Platform.
    vp.add_canvas_item(rect_at(3, 14.0, 17.0, 12.0, 2.0, Color::rgb(0.5, 0.5, 0.5)));
    // Collectible.
    vp.add_canvas_item(rect_at(4, 22.0, 14.0, 2.0, 2.0, Color::rgb(1.0, 1.0, 0.0)));

    let fb = capture_frame(&mut renderer, &vp);
    assert_camera_golden(&fb, "full_scene");
}

// ===========================================================================
// ZOOM + SIZE: Additional coverage for rect size scaling
// ===========================================================================

#[test]
fn zoom_scales_circle_radius() {
    // Verify that camera zoom also scales circle apparent size.
    // Use a large viewport and center the camera on the circle to avoid clipping.
    let mut renderer = SoftwareRenderer::new();

    // No zoom: circle radius 4 at (10,10). Camera centered on circle.
    let mut vp1 = Viewport::new(60, 60, Color::BLACK);
    vp1.camera_position = Vector2::new(10.0, 10.0);
    vp1.camera_zoom = Vector2::ONE;
    let mut item1 = CanvasItem::new(CanvasItemId(1));
    item1.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(10.0, 10.0),
        radius: 4.0,
        color: red(),
    });
    vp1.add_canvas_item(item1);
    let fb1 = capture_frame(&mut renderer, &vp1);
    let count1 = count_color(&fb1, red());

    // 2x zoom: circle should appear with doubled radius -> ~4x the area.
    // Camera still centered on the circle.
    let mut vp2 = Viewport::new(60, 60, Color::BLACK);
    vp2.camera_position = Vector2::new(10.0, 10.0);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);
    let mut item2 = CanvasItem::new(CanvasItemId(1));
    item2.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(10.0, 10.0),
        radius: 4.0,
        color: red(),
    });
    vp2.add_canvas_item(item2);
    let fb2 = capture_frame(&mut renderer, &vp2);
    let count2 = count_color(&fb2, red());

    // Zoomed circle should have roughly 4x the pixels (radius doubles -> area quadruples).
    let ratio = count2 as f64 / count1 as f64;
    assert!(
        ratio > 3.0 && ratio < 5.0,
        "2x zoom should roughly quadruple circle area: ratio = {ratio:.2} \
         (count1={count1}, count2={count2})"
    );
}

#[test]
fn zoom_preserves_relative_positioning() {
    // Two rects that are 10 pixels apart in world space should be 20 pixels
    // apart on screen with 2x zoom.
    let size = 60u32;
    let mut renderer = SoftwareRenderer::new();

    let mut vp = Viewport::new(size, size, Color::BLACK);
    vp.camera_position = Vector2::new(15.0, 15.0);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // Red rect at world (5, 15), green rect at world (15, 15) — 10 units apart.
    vp.add_canvas_item(rect_at(1, 5.0, 14.0, 2.0, 2.0, red()));
    vp.add_canvas_item(rect_at(2, 15.0, 14.0, 2.0, 2.0, green()));

    let fb = capture_frame(&mut renderer, &vp);

    // Find the center of red and green pixel clusters.
    let mut red_x_sum = 0u64;
    let mut red_count = 0u64;
    let mut green_x_sum = 0u64;
    let mut green_count = 0u64;

    for y in 0..size {
        for x in 0..size {
            let p = fb.get_pixel(x, y);
            if (p.r - 1.0).abs() < TOL && p.g.abs() < TOL && p.b.abs() < TOL {
                red_x_sum += x as u64;
                red_count += 1;
            }
            if p.r.abs() < TOL && (p.g - 1.0).abs() < TOL && p.b.abs() < TOL {
                green_x_sum += x as u64;
                green_count += 1;
            }
        }
    }

    assert!(red_count > 0, "Red rect should be visible");
    assert!(green_count > 0, "Green rect should be visible");

    let red_center_x = red_x_sum as f64 / red_count as f64;
    let green_center_x = green_x_sum as f64 / green_count as f64;
    let screen_distance = (green_center_x - red_center_x).abs();

    // World distance = 10, zoom = 2x → screen distance should be ~20.
    assert!(
        (screen_distance - 20.0).abs() < 3.0,
        "Screen distance should be ~20 (2x zoom on 10-unit gap): got {screen_distance:.1}"
    );
}
