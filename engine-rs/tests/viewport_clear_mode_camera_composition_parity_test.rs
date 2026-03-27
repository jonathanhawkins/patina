//! pat-kc47: Match viewport clear mode and camera composition against oracle captures.
//!
//! Godot's SubViewport.ClearMode contract:
//!   - CLEAR_MODE_ALWAYS (default): framebuffer cleared to clear_color every frame.
//!   - CLEAR_MODE_NEVER: framebuffer retains pixels from previous frame (trails).
//!   - CLEAR_MODE_ONLY_NEXT_FRAME: clear once, then behave as Never.
//!
//! Camera composition contract:
//!   screen = viewport_center + zoom * rotation * (world - camera_position)
//!
//! Acceptance: tests compare clear mode behavior and camera+viewport layering
//! against expected oracle behavior.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::{ClearMode, Viewport};

// ===========================================================================
// Helpers
// ===========================================================================

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

fn make_rect_item(id: u64, x: f32, y: f32, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        color,
        filled: true,
    });
    item
}

fn count_color(pixels: &[Color], color: Color) -> usize {
    pixels
        .iter()
        .filter(|p| {
            (p.r - color.r).abs() < TOL
                && (p.g - color.g).abs() < TOL
                && (p.b - color.b).abs() < TOL
        })
        .count()
}

// ===========================================================================
// 1. ClearMode::Always — framebuffer cleared every frame (Godot default)
// ===========================================================================

#[test]
fn clear_mode_always_is_default() {
    let vp = Viewport::new(10, 10, Color::BLACK);
    assert_eq!(vp.clear_mode, ClearMode::Always);
}

#[test]
fn clear_mode_always_clears_between_frames() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    vp.clear_mode = ClearMode::Always;

    // Frame 1: red rect.
    vp.add_canvas_item(make_rect_item(1, 0.0, 0.0, 5.0, 5.0, red()));
    let fb1 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb1, 2, 2, red(), TOL);

    // Frame 2: remove all items, add green rect at different position.
    // (Viewport retains items, so we create a new viewport.)
    let mut vp2 = Viewport::new(10, 10, Color::BLACK);
    vp2.clear_mode = ClearMode::Always;
    vp2.add_canvas_item(make_rect_item(2, 5.0, 5.0, 5.0, 5.0, green()));
    let fb2 = capture_frame(&mut renderer, &vp2);

    // Old red position should be black (cleared).
    assert_pixel_color(&fb2, 2, 2, Color::BLACK, TOL);
    // New green position should be green.
    assert_pixel_color(&fb2, 7, 7, green(), TOL);
}

// ===========================================================================
// 2. ClearMode::Never — pixels persist across frames (trail effect)
// ===========================================================================

#[test]
fn clear_mode_never_retains_previous_frame_pixels() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1: red rect at top-left.
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    vp.clear_mode = ClearMode::Never;
    vp.add_canvas_item(make_rect_item(1, 0.0, 0.0, 4.0, 4.0, red()));
    let _fb1 = renderer.render_frame(&vp);

    // Frame 2: green rect at bottom-right (no red item).
    let mut vp2 = Viewport::new(10, 10, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.add_canvas_item(make_rect_item(2, 6.0, 6.0, 4.0, 4.0, green()));
    let frame2 = renderer.render_frame(&vp2);

    // Red pixels from frame 1 should persist.
    let red_count = count_color(&frame2.pixels, red());
    assert!(
        red_count > 0,
        "ClearMode::Never must retain red pixels from previous frame, got 0"
    );

    // Green pixels from frame 2 should also be present.
    let green_count = count_color(&frame2.pixels, green());
    assert!(
        green_count > 0,
        "ClearMode::Never must also render new green pixels"
    );
}

#[test]
fn clear_mode_never_accumulates_across_multiple_frames() {
    let mut renderer = SoftwareRenderer::new();
    let size = 12u32;

    // Frame 1: red at (0,0).
    let mut vp1 = Viewport::new(size, size, Color::BLACK);
    vp1.clear_mode = ClearMode::Never;
    vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 3.0, 3.0, red()));
    let _ = renderer.render_frame(&vp1);

    // Frame 2: green at (4,4).
    let mut vp2 = Viewport::new(size, size, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.add_canvas_item(make_rect_item(2, 4.0, 4.0, 3.0, 3.0, green()));
    let _ = renderer.render_frame(&vp2);

    // Frame 3: blue at (8,8).
    let mut vp3 = Viewport::new(size, size, Color::BLACK);
    vp3.clear_mode = ClearMode::Never;
    vp3.add_canvas_item(make_rect_item(3, 8.0, 8.0, 3.0, 3.0, blue()));
    let frame3 = renderer.render_frame(&vp3);

    // All three colors should be present.
    assert!(count_color(&frame3.pixels, red()) > 0, "Red from frame 1 must persist");
    assert!(count_color(&frame3.pixels, green()) > 0, "Green from frame 2 must persist");
    assert!(count_color(&frame3.pixels, blue()) > 0, "Blue from frame 3 must be present");
}

// ===========================================================================
// 3. ClearMode::OnlyNextFrame — clear once, then retain
// ===========================================================================

#[test]
fn clear_mode_only_next_frame_clears_then_retains() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1 (OnlyNextFrame): red rect → should clear first.
    let mut vp1 = Viewport::new(10, 10, Color::BLACK);
    vp1.clear_mode = ClearMode::OnlyNextFrame;
    vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 4.0, 4.0, red()));
    let _ = renderer.render_frame(&vp1);

    // Frame 2 (Never): green rect only, but red should persist from frame 1.
    let mut vp2 = Viewport::new(10, 10, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.add_canvas_item(make_rect_item(2, 6.0, 6.0, 4.0, 4.0, green()));
    let frame2 = renderer.render_frame(&vp2);

    // Red from frame 1 should persist (OnlyNextFrame stored the buffer).
    assert!(
        count_color(&frame2.pixels, red()) > 0,
        "Red from OnlyNextFrame frame must persist into Never frame"
    );
    assert!(
        count_color(&frame2.pixels, green()) > 0,
        "Green from Never frame must also be present"
    );
}

// ===========================================================================
// 4. ClearMode::Always vs Never produce different results
// ===========================================================================

#[test]
fn clear_mode_always_vs_never_differ_on_second_frame() {
    let run = |mode: ClearMode| -> Vec<Color> {
        let mut renderer = SoftwareRenderer::new();

        // Frame 1: red rect.
        let mut vp1 = Viewport::new(10, 10, Color::BLACK);
        vp1.clear_mode = mode;
        vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 5.0, 5.0, red()));
        let _ = renderer.render_frame(&vp1);

        // Frame 2: only green rect (no red).
        let mut vp2 = Viewport::new(10, 10, Color::BLACK);
        vp2.clear_mode = mode;
        vp2.add_canvas_item(make_rect_item(2, 5.0, 5.0, 5.0, 5.0, green()));
        renderer.render_frame(&vp2).pixels
    };

    let always_pixels = run(ClearMode::Always);
    let never_pixels = run(ClearMode::Never);

    // With Always, top-left should be black (cleared). With Never, it should be red (retained).
    assert_ne!(
        always_pixels, never_pixels,
        "Always and Never modes must produce different second-frame results"
    );

    // Specifically: check pixel (2,2) — Always=black, Never=red.
    let always_px = always_pixels[2 * 10 + 2];
    let never_px = never_pixels[2 * 10 + 2];
    assert!(
        (always_px.r - 0.0).abs() < TOL,
        "Always mode: (2,2) should be black, got {:?}",
        always_px
    );
    assert!(
        (never_px.r - 1.0).abs() < TOL,
        "Never mode: (2,2) should be red (retained), got {:?}",
        never_px
    );
}

// ===========================================================================
// 5. Camera composition: position centers world on viewport
// ===========================================================================

#[test]
fn camera_position_centers_on_viewport() {
    // Godot contract: camera at (X,Y) places world (X,Y) at viewport center.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(50.0, 50.0);

    // 4x4 rect centered at world (50,50).
    vp.add_canvas_item(make_rect_item(1, 48.0, 48.0, 4.0, 4.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // Viewport center (10,10) should be red.
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    // Far corner should be black.
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
}

// ===========================================================================
// 6. Camera zoom scales world around viewport center
// ===========================================================================

#[test]
fn camera_zoom_doubles_apparent_size() {
    let size = 40u32;
    let half = size as f32 / 2.0;

    let make_frame = |zoom: f32| {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(size, size, Color::BLACK);
        vp.camera_position = Vector2::new(half, half);
        vp.camera_zoom = Vector2::new(zoom, zoom);
        vp.add_canvas_item(make_rect_item(
            1,
            half - 2.0,
            half - 2.0,
            4.0,
            4.0,
            red(),
        ));
        capture_frame(&mut renderer, &vp)
    };

    let fb_1x = make_frame(1.0);
    let fb_2x = make_frame(2.0);

    let count_1x = count_color(&fb_1x.pixels, red());
    let count_2x = count_color(&fb_2x.pixels, red());

    // At 2x zoom, a 4x4 rect becomes 8x8 = 64 pixels vs 4x4 = 16 pixels.
    assert_eq!(count_1x, 16, "1x zoom: 4x4 rect = 16 red pixels");
    assert_eq!(count_2x, 64, "2x zoom: 4x4 rect scaled to 8x8 = 64 red pixels");
}

// ===========================================================================
// 7. Camera rotation rotates world content
// ===========================================================================

#[test]
fn camera_rotation_changes_pixel_layout() {
    let size = 30u32;

    let make_frame = |rotation: f32| {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(size, size, Color::BLACK);
        vp.camera_position = Vector2::new(15.0, 15.0);
        vp.camera_rotation = rotation;
        // Asymmetric rect to make rotation visible.
        vp.add_canvas_item(make_rect_item(1, 13.0, 14.0, 6.0, 2.0, red()));
        capture_frame(&mut renderer, &vp)
    };

    let fb_0 = make_frame(0.0);
    let fb_rot = make_frame(std::f32::consts::FRAC_PI_4);

    assert_ne!(
        fb_0.pixels, fb_rot.pixels,
        "45-degree rotation must change pixel layout"
    );
}

// ===========================================================================
// 8. Camera + layer transform composition
// ===========================================================================

#[test]
fn camera_and_layer_transforms_compose_correctly() {
    // Camera at (100, 100), layer translates by (95, 95).
    // Item at layer-local (0,0) → world (95,95) → screen offset from center.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = Vector2::new(100.0, 100.0);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(95.0, 95.0));
    vp.add_canvas_layer(layer);

    let mut item = make_rect_item(1, 0.0, 0.0, 4.0, 4.0, red());
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);

    // World (95,95) maps to screen (10 + (95-100), 10 + (95-100)) = (5,5).
    assert_pixel_color(&fb, 5, 5, red(), TOL);
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    // Origin should be clear.
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
}

// ===========================================================================
// 9. Clear mode + camera composition combined
// ===========================================================================

#[test]
fn clear_mode_never_with_camera_movement_creates_trail() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1: camera at (10,10), red rect at (10,10).
    let mut vp1 = Viewport::new(20, 20, Color::BLACK);
    vp1.clear_mode = ClearMode::Never;
    vp1.camera_position = Vector2::new(10.0, 10.0);
    vp1.add_canvas_item(make_rect_item(1, 8.0, 8.0, 4.0, 4.0, red()));
    let _ = renderer.render_frame(&vp1);

    // Frame 2: camera moves to (15,15), same rect → appears at different screen position.
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.camera_position = Vector2::new(15.0, 15.0);
    vp2.add_canvas_item(make_rect_item(1, 8.0, 8.0, 4.0, 4.0, green()));
    let frame2 = renderer.render_frame(&vp2);

    // Both the old red ghost and new green rect should be visible.
    let red_count = count_color(&frame2.pixels, red());
    let green_count = count_color(&frame2.pixels, green());
    assert!(red_count > 0, "Trail: red from frame 1 must persist");
    assert!(green_count > 0, "New green from frame 2 must be present");
}

// ===========================================================================
// 10. Viewport clear color is respected
// ===========================================================================

#[test]
fn viewport_clear_color_fills_background() {
    let bg = Color::rgb(0.2, 0.4, 0.8);
    let mut renderer = SoftwareRenderer::new();
    let vp = Viewport::new(10, 10, bg);

    let fb = capture_frame(&mut renderer, &vp);

    // Every pixel should be the clear color.
    for y in 0..10 {
        for x in 0..10 {
            assert_pixel_color(&fb, x, y, bg, TOL);
        }
    }
}

// ===========================================================================
// 11. Composition determinism
// ===========================================================================

#[test]
fn camera_composition_is_deterministic() {
    let make = || {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(30, 30, Color::BLACK);
        vp.camera_position = Vector2::new(20.0, 20.0);
        vp.camera_zoom = Vector2::new(1.5, 1.5);
        vp.camera_rotation = 0.3;

        let mut layer = CanvasLayer::new(1);
        layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
        vp.add_canvas_layer(layer);

        let mut item = make_rect_item(1, 15.0, 15.0, 6.0, 6.0, red());
        item.layer_id = Some(1);
        vp.add_canvas_item(item);
        vp.add_canvas_item(make_rect_item(2, 18.0, 18.0, 4.0, 4.0, green()));

        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make();
    let fb2 = make();
    assert_eq!(fb1.pixels, fb2.pixels, "Camera composition must be deterministic");
}

// ===========================================================================
// 12. ClearMode transition: Always does not persist into subsequent Never
// ===========================================================================

#[test]
fn clear_mode_always_does_not_persist_into_never() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1: ClearMode::Always with red rect — renderer does NOT persist this buffer.
    let mut vp1 = Viewport::new(10, 10, Color::BLACK);
    vp1.clear_mode = ClearMode::Always;
    vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 5.0, 5.0, red()));
    let _ = renderer.render_frame(&vp1);

    // Frame 2: switch to Never, draw green at different spot.
    // Since Always didn't persist, Never starts with a fresh clear_color buffer.
    let mut vp2 = Viewport::new(10, 10, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.add_canvas_item(make_rect_item(2, 5.0, 5.0, 5.0, 5.0, green()));
    let fb2 = renderer.render_frame(&vp2);

    // Red should NOT persist — Always mode doesn't save persistent buffer.
    let red_count = count_color(&fb2.pixels, red());
    assert_eq!(
        red_count, 0,
        "Always mode frame should not persist into subsequent Never frame"
    );
    let green_count = count_color(&fb2.pixels, green());
    assert!(green_count > 0, "New green content must be present");
    // Uncovered area should be clear_color (black).
    let idx = 2 * 10 + 2; // pixel (2,2) — was red in frame 1
    let px = fb2.pixels[idx];
    assert!(
        (px.r).abs() < TOL && (px.g).abs() < TOL && (px.b).abs() < TOL,
        "Uncovered area should be clear_color black, got {:?}",
        px
    );
}

// ===========================================================================
// 13. ClearMode::Never with camera zoom change creates scaled trail
// ===========================================================================

#[test]
fn clear_mode_never_with_zoom_change_retains_old_scale() {
    let mut renderer = SoftwareRenderer::new();
    let half = 10.0f32;

    // Frame 1: camera centered, 1x zoom, red rect at top-left of viewport.
    // Camera at (10,10), rect at (0,0,4,4) → screen (0,0)-(4,4) = 16 red pixels.
    let mut vp1 = Viewport::new(20, 20, Color::BLACK);
    vp1.clear_mode = ClearMode::Never;
    vp1.camera_position = Vector2::new(half, half);
    vp1.camera_zoom = Vector2::new(1.0, 1.0);
    vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 4.0, 4.0, red()));
    let fb1 = renderer.render_frame(&vp1);
    let red_1x = count_color(&fb1.pixels, red());
    assert_eq!(red_1x, 16, "1x zoom: 4x4 = 16 red pixels");

    // Frame 2: camera centered, 2x zoom, green 3x3 rect at world (12,12).
    // At 2x zoom: screen = (10,10) + 2*((12,12)-(10,10)) = (14,14), size 3*2=6.
    // Green at screen (14,14)-(20,20) = 6x6 = 36 green pixels (no overlap with red).
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.camera_position = Vector2::new(half, half);
    vp2.camera_zoom = Vector2::new(2.0, 2.0);
    vp2.add_canvas_item(make_rect_item(2, 12.0, 12.0, 3.0, 3.0, green()));
    let fb2 = renderer.render_frame(&vp2);
    let green_2x = count_color(&fb2.pixels, green());

    // Green at 2x zoom should be larger (3x3 → 6x6 = 36 > 16).
    assert!(
        green_2x > red_1x,
        "2x zoom rect ({green_2x} px) should be larger than 1x rect ({red_1x} px)"
    );
    // Red from frame 1 should persist in the retained buffer.
    let red_retained = count_color(&fb2.pixels, red());
    assert!(
        red_retained > 0,
        "Red from 1x zoom frame must persist under Never mode"
    );
}

// ===========================================================================
// 14. OnlyNextFrame with camera movement
// ===========================================================================

#[test]
fn only_next_frame_clears_then_camera_movement_retains() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1: OnlyNextFrame clears, draws red at top-left (no camera offset).
    // Red at screen (0,0)-(4,4).
    let mut vp1 = Viewport::new(20, 20, Color::BLACK);
    vp1.clear_mode = ClearMode::OnlyNextFrame;
    vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 4.0, 4.0, red()));
    let _ = renderer.render_frame(&vp1);

    // Frame 2: Never mode (post-OnlyNextFrame), camera pans right.
    // Camera at (5,5), green rect at world (10,10,4,4).
    // Screen = (10,10) + ((10,10)-(5,5)) = (15,15). Green at screen (15,15)-(19,19).
    let mut vp2 = Viewport::new(20, 20, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.camera_position = Vector2::new(5.0, 5.0);
    vp2.add_canvas_item(make_rect_item(2, 10.0, 10.0, 4.0, 4.0, green()));
    let fb2 = renderer.render_frame(&vp2);

    // Red from frame 1 should persist (OnlyNextFrame stores persistent buffer).
    assert!(
        count_color(&fb2.pixels, red()) > 0,
        "Red from OnlyNextFrame initial render must persist after camera move"
    );
    assert!(
        count_color(&fb2.pixels, green()) > 0,
        "Green from moved camera frame must be present"
    );
}

// ===========================================================================
// 15. Multiple layers accumulate under ClearMode::Never
// ===========================================================================

#[test]
fn clear_mode_never_accumulates_multi_layer_content() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1: layer 1 with red rect.
    let mut vp1 = Viewport::new(12, 12, Color::BLACK);
    vp1.clear_mode = ClearMode::Never;
    let layer1 = CanvasLayer::new(1);
    vp1.add_canvas_layer(layer1);
    let mut red_item = make_rect_item(1, 0.0, 0.0, 4.0, 4.0, red());
    red_item.layer_id = Some(1);
    vp1.add_canvas_item(red_item);
    let _ = renderer.render_frame(&vp1);

    // Frame 2: layer 2 with green rect at different position.
    let mut vp2 = Viewport::new(12, 12, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    let layer2 = CanvasLayer::new(2);
    vp2.add_canvas_layer(layer2);
    let mut green_item = make_rect_item(2, 8.0, 8.0, 4.0, 4.0, green());
    green_item.layer_id = Some(2);
    vp2.add_canvas_item(green_item);
    let fb2 = renderer.render_frame(&vp2);

    // Both layers' content should be visible (Never retains across frames).
    assert!(
        count_color(&fb2.pixels, red()) > 0,
        "Layer 1 red from frame 1 must persist"
    );
    assert!(
        count_color(&fb2.pixels, green()) > 0,
        "Layer 2 green from frame 2 must be present"
    );
}

// ===========================================================================
// 16. Asymmetric camera zoom
// ===========================================================================

#[test]
fn asymmetric_camera_zoom_stretches_content() {
    let size = 40u32;
    let half = size as f32 / 2.0;

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(size, size, Color::BLACK);
    vp.camera_position = Vector2::new(half, half);
    // 2x horizontal, 1x vertical.
    vp.camera_zoom = Vector2::new(2.0, 1.0);
    // 4x4 rect centered at viewport center.
    vp.add_canvas_item(make_rect_item(
        1,
        half - 2.0,
        half - 2.0,
        4.0,
        4.0,
        red(),
    ));
    let fb = capture_frame(&mut renderer, &vp);

    let red_count = count_color(&fb.pixels, red());
    // At (2x, 1x) zoom, 4x4 rect becomes 8x4 = 32 pixels.
    assert_eq!(
        red_count, 32,
        "Asymmetric zoom (2x,1x) on 4x4 rect should produce 8x4=32 red pixels, got {red_count}"
    );
}

// ===========================================================================
// 17. ClearMode::Never with overlapping items overwrite order
// ===========================================================================

#[test]
fn clear_mode_never_later_frame_overwrites_same_position() {
    let mut renderer = SoftwareRenderer::new();

    // Frame 1: red fills entire viewport.
    let mut vp1 = Viewport::new(10, 10, Color::BLACK);
    vp1.clear_mode = ClearMode::Never;
    vp1.add_canvas_item(make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red()));
    let _ = renderer.render_frame(&vp1);

    // Frame 2: green fills entire viewport — should overwrite red.
    let mut vp2 = Viewport::new(10, 10, Color::BLACK);
    vp2.clear_mode = ClearMode::Never;
    vp2.add_canvas_item(make_rect_item(2, 0.0, 0.0, 10.0, 10.0, green()));
    let fb2 = renderer.render_frame(&vp2);

    // Green overwrites red at same pixels.
    assert_eq!(
        count_color(&fb2.pixels, green()),
        100,
        "Later frame content must overwrite retained pixels at same position"
    );
    assert_eq!(
        count_color(&fb2.pixels, red()),
        0,
        "Red should be fully overwritten by green"
    );
}
