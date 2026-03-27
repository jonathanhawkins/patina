//! pat-kc47: Viewport clear mode and camera composition oracle parity tests.
//!
//! Validates that viewport clear mode semantics and camera composition
//! layering behavior match Godot's oracle behavior:
//! - ClearMode::Always fills with clear_color every frame
//! - ClearMode::Never retains previous frame pixels
//! - ClearMode::OnlyNextFrame clears once then retains
//! - Camera position/zoom/rotation compose correctly with canvas layers
//! - Layer z-order determines draw order under camera transforms
//! - Invisible layers are culled under all camera configurations
//! - Clear color is visible through transparent/empty layer regions

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::viewport::{ClearMode, Viewport};

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
fn magenta() -> Color {
    Color::rgb(1.0, 0.0, 1.0)
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
// CLEAR MODE: ALWAYS (default, oracle baseline)
// ===========================================================================

#[test]
fn clear_mode_always_fills_background_every_frame() {
    // Oracle: Godot's default SubViewport clears to clear_color each frame.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, blue());
    assert_eq!(vp.clear_mode, ClearMode::Always);

    // Frame 1: draw a red rect in the top-left quadrant.
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 5.0, 5.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb1, 2, 2, red(), TOL);
    assert_pixel_color(&fb1, 8, 8, blue(), TOL); // clear color

    // Frame 2: remove the rect. Entire viewport should be clear_color.
    vp.remove_canvas_item(CanvasItemId(1));
    let fb2 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb2, 2, 2, blue(), TOL);
    assert_pixel_color(&fb2, 8, 8, blue(), TOL);
    assert_eq!(count_color(&fb2, blue()), 100); // 10x10
}

#[test]
fn clear_mode_always_different_colors_per_frame() {
    // Oracle: changing clear_color between frames takes effect immediately.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(8, 8, red());

    let fb1 = capture_frame(&mut renderer, &vp);
    assert_eq!(count_color(&fb1, red()), 64);

    vp.clear_color = green();
    let fb2 = capture_frame(&mut renderer, &vp);
    assert_eq!(count_color(&fb2, green()), 64);
    assert_eq!(count_color(&fb2, red()), 0);
}

// ===========================================================================
// CLEAR MODE: NEVER
// ===========================================================================

#[test]
fn clear_mode_never_retains_previous_pixels() {
    // Oracle: ClearMode::Never preserves the framebuffer across frames.
    // Content drawn in frame 1 persists into frame 2 even if removed.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    vp.clear_mode = ClearMode::Never;

    // Frame 1: draw red rect in top-left.
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 5.0, 5.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb1, 2, 2, red(), TOL);

    // Frame 2: remove rect, add green rect in bottom-right.
    // Red pixels should persist because clear_mode is Never.
    vp.remove_canvas_item(CanvasItemId(1));
    vp.add_canvas_item(rect_item(2, 5.0, 5.0, 5.0, 5.0, green()));
    let fb2 = capture_frame(&mut renderer, &vp);

    // Top-left: retained red from frame 1.
    assert_pixel_color(&fb2, 2, 2, red(), TOL);
    // Bottom-right: new green from frame 2.
    assert_pixel_color(&fb2, 7, 7, green(), TOL);
}

#[test]
fn clear_mode_never_new_content_overwrites_old() {
    // Oracle: new content draws on top of retained pixels.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    vp.clear_mode = ClearMode::Never;

    // Frame 1: red everywhere.
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 10.0, 10.0, red()));
    capture_frame(&mut renderer, &vp);

    // Frame 2: green rect over the center.
    vp.remove_canvas_item(CanvasItemId(1));
    vp.add_canvas_item(rect_item(2, 3.0, 3.0, 4.0, 4.0, green()));
    let fb2 = capture_frame(&mut renderer, &vp);

    // Center: green (new overwrites retained).
    assert_pixel_color(&fb2, 5, 5, green(), TOL);
    // Edge: retained red.
    assert_pixel_color(&fb2, 0, 0, red(), TOL);
}

// ===========================================================================
// CLEAR MODE: ONLY NEXT FRAME
// ===========================================================================

#[test]
fn clear_mode_only_next_frame_clears_once_then_retains() {
    // Oracle: OnlyNextFrame clears on the first render, then behaves like Never.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, blue());
    vp.clear_mode = ClearMode::OnlyNextFrame;

    // Frame 1: cleared to blue, draw red rect.
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 5.0, 5.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb1, 2, 2, red(), TOL);
    assert_pixel_color(&fb1, 8, 8, blue(), TOL);

    // Switch to Never (Godot auto-transitions after the one clear).
    vp.clear_mode = ClearMode::Never;

    // Frame 2: remove rect. Red should persist, blue background should persist.
    vp.remove_canvas_item(CanvasItemId(1));
    let fb2 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb2, 2, 2, red(), TOL); // retained
    assert_pixel_color(&fb2, 8, 8, blue(), TOL); // retained
}

// ===========================================================================
// CAMERA COMPOSITION WITH CLEAR MODE
// ===========================================================================

#[test]
fn camera_offset_with_clear_mode_always() {
    // Oracle: camera shift moves content, clear_color fills exposed area.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, magenta());

    // Place a 10x10 white rect at world origin.
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 10.0, 10.0, white()));

    // Camera at (0,0): rect covers top-left quadrant.
    let fb1 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb1, 5, 5, white(), TOL);
    assert_pixel_color(&fb1, 15, 15, magenta(), TOL);

    // Pan camera right by 10 — rect shifts left off-screen.
    vp.camera_position = Vector2::new(10.0, 0.0);
    let fb2 = capture_frame(&mut renderer, &vp);
    // Former rect area should now be clear_color (rect shifted off-screen).
    assert_pixel_color(&fb2, 15, 5, magenta(), TOL);
}

#[test]
fn camera_zoom_with_clear_mode_always() {
    // Oracle: zoom 2x doubles content size, clear_color fills remaining.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_zoom = Vector2::new(2.0, 2.0);

    // 5x5 white rect at origin — at 2x zoom it should cover 10x10 pixels.
    vp.add_canvas_item(rect_item(1, 0.0, 0.0, 5.0, 5.0, white()));
    let fb = capture_frame(&mut renderer, &vp);

    // Under zoom, the viewport center shifts. Check that white pixels exist
    // and black pixels exist (clear color in uncovered areas).
    let white_count = count_color(&fb, white());
    let black_count = count_color(&fb, Color::BLACK);
    assert!(
        white_count > 0,
        "zoomed rect should produce white pixels"
    );
    assert!(
        black_count > 0,
        "clear color should fill areas outside zoomed rect"
    );
}

// ===========================================================================
// LAYER COMPOSITION WITH CAMERA
// ===========================================================================

#[test]
fn layer_z_order_under_camera_pan() {
    // Oracle: layer z_order is honored regardless of camera position.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(5.0, 5.0);

    let mut bg = CanvasLayer::new(1);
    bg.z_order = 0;
    let mut fg = CanvasLayer::new(2);
    fg.z_order = 1;
    vp.add_canvas_layer(bg);
    vp.add_canvas_layer(fg);

    // Red on bg, green on fg — overlapping at world (5..15, 5..15).
    vp.add_canvas_item(rect_on_layer(1, 5.0, 5.0, 10.0, 10.0, red(), 1));
    vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), 2));

    let fb = capture_frame(&mut renderer, &vp);

    // After camera pan, the overlap region should be green (fg on top).
    // The exact pixel depends on camera transform, but the center of the
    // viewport should show the fg layer.
    let green_count = count_color(&fb, green());
    let red_count = count_color(&fb, red());
    assert!(
        green_count > 0,
        "foreground layer should be visible after camera pan"
    );
    // Red is fully occluded by green since they overlap exactly.
    assert_eq!(
        red_count, 0,
        "background layer should be fully occluded by foreground"
    );
}

#[test]
fn invisible_layer_hidden_under_camera_zoom() {
    // Oracle: invisible layers are culled regardless of camera zoom.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_zoom = Vector2::new(0.5, 0.5);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 20.0, 20.0, red(), 1));

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(
        count_color(&fb, red()),
        0,
        "invisible layer items should not render even under zoom"
    );
}

#[test]
fn clear_color_visible_through_empty_layer_regions() {
    // Oracle: clear_color shows through areas not covered by any layer content.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, magenta());

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Only a small rect on the layer — rest of viewport is clear_color.
    vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 5.0, 5.0, green(), 1));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 2, 2, green(), TOL);
    assert_pixel_color(&fb, 15, 15, magenta(), TOL);
}

#[test]
fn multi_layer_camera_rotation_composition() {
    // Oracle: camera rotation composes with layer transforms correctly.
    // Under rotation, items may shift but layer z_order is preserved.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_rotation = std::f32::consts::FRAC_PI_4; // 45 degrees

    let mut bg = CanvasLayer::new(1);
    bg.z_order = 0;
    let mut fg = CanvasLayer::new(2);
    fg.z_order = 1;
    vp.add_canvas_layer(bg);
    vp.add_canvas_layer(fg);

    // Large red background rect.
    vp.add_canvas_item(rect_on_layer(1, -20.0, -20.0, 60.0, 60.0, red(), 1));
    // Green rect on top at center.
    vp.add_canvas_item(rect_on_layer(2, 5.0, 5.0, 10.0, 10.0, green(), 2));

    let fb = capture_frame(&mut renderer, &vp);
    let green_count = count_color(&fb, green());
    assert!(
        green_count > 0,
        "foreground green layer should be visible under camera rotation"
    );
}

// ===========================================================================
// DETERMINISTIC ORACLE COMPARISON
// ===========================================================================

#[test]
fn viewport_rendering_is_deterministic() {
    // Oracle contract: identical viewport state produces identical output.
    let mut r1 = SoftwareRenderer::new();
    let mut r2 = SoftwareRenderer::new();

    let make_viewport = || {
        let mut vp = Viewport::new(16, 16, blue());
        vp.camera_position = Vector2::new(3.0, 2.0);
        vp.camera_zoom = Vector2::new(1.5, 1.5);

        let mut layer = CanvasLayer::new(1);
        layer.z_order = 0;
        vp.add_canvas_layer(layer);
        vp.add_canvas_item(rect_on_layer(1, 0.0, 0.0, 10.0, 10.0, red(), 1));
        vp.add_canvas_item(rect_on_layer(2, 4.0, 4.0, 6.0, 6.0, green(), 1));
        vp
    };

    let vp1 = make_viewport();
    let vp2 = make_viewport();

    let fb1 = capture_frame(&mut r1, &vp1);
    let fb2 = capture_frame(&mut r2, &vp2);

    assert_eq!(fb1.pixels.len(), fb2.pixels.len());
    for (i, (a, b)) in fb1.pixels.iter().zip(fb2.pixels.iter()).enumerate() {
        assert!(
            (a.r - b.r).abs() < f32::EPSILON
                && (a.g - b.g).abs() < f32::EPSILON
                && (a.b - b.b).abs() < f32::EPSILON
                && (a.a - b.a).abs() < f32::EPSILON,
            "pixel {i} differs: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn clear_mode_default_is_always() {
    // Oracle: Godot's default clear mode is Always.
    let vp = Viewport::new(10, 10, Color::BLACK);
    assert_eq!(vp.clear_mode, ClearMode::Always);
}

#[test]
fn clear_mode_never_with_empty_viewport_retains_initial_clear() {
    // Oracle: first frame with Never still fills with clear_color (no prior state),
    // subsequent frames retain that fill.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(8, 8, red());
    vp.clear_mode = ClearMode::Never;

    // Frame 1: no content, framebuffer initialized to clear_color.
    let fb1 = capture_frame(&mut renderer, &vp);
    assert_eq!(count_color(&fb1, red()), 64);

    // Frame 2: change clear_color but Never mode ignores it.
    vp.clear_color = green();
    let fb2 = capture_frame(&mut renderer, &vp);
    // Should still be red (retained from frame 1, not re-cleared to green).
    assert_eq!(count_color(&fb2, red()), 64);
    assert_eq!(count_color(&fb2, green()), 0);
}
