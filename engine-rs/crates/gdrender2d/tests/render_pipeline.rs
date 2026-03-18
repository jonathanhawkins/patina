//! Comprehensive rendering pipeline tests.
//!
//! Covers golden snapshots, edge cases, stress tests, and transform handling
//! for the 2D software renderer.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame, save_ppm};
use gdrender2d::texture::Texture2D;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Converts a FrameBuffer to a PPM string (P3 format) for comparison.
fn fb_to_ppm_string(fb: &FrameBuffer) -> String {
    let mut s = format!("P3\n{} {}\n255\n", fb.width, fb.height);
    for pixel in &fb.pixels {
        let r = (pixel.r.clamp(0.0, 1.0) * 255.0) as u8;
        let g = (pixel.g.clamp(0.0, 1.0) * 255.0) as u8;
        let b = (pixel.b.clamp(0.0, 1.0) * 255.0) as u8;
        s.push_str(&format!("{} {} {}\n", r, g, b));
    }
    s
}

/// Loads a golden PPM file and returns its content as a string.
fn load_golden(name: &str) -> String {
    let path = format!(
        "{}/fixtures/golden/render/{}",
        env!("CARGO_MANIFEST_DIR").replace("/engine-rs/crates/gdrender2d", ""),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to load golden {}: {}", path, e))
}

/// Count how many pixels in the framebuffer match the given color.
fn count_pixels(fb: &FrameBuffer, color: Color) -> usize {
    fb.pixels.iter().filter(|p| **p == color).count()
}

// ---------------------------------------------------------------------------
// Golden snapshot tests
// ---------------------------------------------------------------------------

#[test]
fn golden_red_rect_4x4() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(4, 4, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(1.0, 1.0), Vector2::new(2.0, 2.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let actual = fb_to_ppm_string(&fb);
    let expected = load_golden("red_rect_4x4.ppm");
    assert_eq!(actual, expected, "Golden mismatch for red_rect_4x4");
}

#[test]
fn golden_circle_8x8() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(8, 8, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(4.0, 4.0),
        radius: 3.0,
        color: Color::rgb(0.0, 1.0, 0.0),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let actual = fb_to_ppm_string(&fb);
    let expected = load_golden("green_circle_8x8.ppm");
    assert_eq!(actual, expected, "Golden mismatch for green_circle_8x8");
}

#[test]
fn golden_line_cross_8x8() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(8, 8, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Horizontal line across the middle.
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(0.0, 4.0),
        to: Vector2::new(7.0, 4.0),
        color: Color::WHITE,
        width: 1.0,
    });
    // Vertical line down the middle.
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(4.0, 0.0),
        to: Vector2::new(4.0, 7.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let actual = fb_to_ppm_string(&fb);
    let expected = load_golden("white_cross_8x8.ppm");
    assert_eq!(actual, expected, "Golden mismatch for white_cross_8x8");
}

// ---------------------------------------------------------------------------
// Zero-size viewport, empty canvas, no draw commands
// ---------------------------------------------------------------------------

#[test]
fn zero_size_viewport() {
    let mut renderer = SoftwareRenderer::new();
    let vp = Viewport::new(0, 0, Color::BLACK);
    let frame = renderer.render_frame(&vp);
    assert_eq!(frame.width, 0);
    assert_eq!(frame.height, 0);
    assert!(frame.pixels.is_empty());
}

#[test]
fn empty_canvas_returns_clear_color() {
    let mut renderer = SoftwareRenderer::new();
    let vp = Viewport::new(4, 4, Color::rgb(0.2, 0.3, 0.4));
    let fb = capture_frame(&mut renderer, &vp);
    for pixel in &fb.pixels {
        assert_eq!(*pixel, Color::rgb(0.2, 0.3, 0.4));
    }
}

#[test]
fn item_with_no_draw_commands() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(4, 4, Color::BLACK);
    // Add an item with no commands.
    vp.add_canvas_item(CanvasItem::new(CanvasItemId(1)));
    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(count_pixels(&fb, Color::BLACK), 16);
}

// ---------------------------------------------------------------------------
// Overlapping items at same z-index (draw order)
// ---------------------------------------------------------------------------

#[test]
fn same_z_index_insertion_order_wins() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(4, 4, Color::BLACK);

    let red = Color::rgb(1.0, 0.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);

    // Both z_index = 0. Blue is added second, should paint last (on top).
    let mut item_a = CanvasItem::new(CanvasItemId(1));
    item_a.z_index = 0;
    item_a.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        color: red,
        filled: true,
    });

    let mut item_b = CanvasItem::new(CanvasItemId(2));
    item_b.z_index = 0;
    item_b.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        color: blue,
        filled: true,
    });

    vp.add_canvas_item(item_a);
    vp.add_canvas_item(item_b);

    let fb = capture_frame(&mut renderer, &vp);
    // Blue was added second so it should draw on top.
    assert_eq!(fb.get_pixel(0, 0), blue);
    assert_eq!(fb.get_pixel(3, 3), blue);
}

#[test]
fn overlapping_partial_coverage_at_same_z() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);

    // Red fills left half.
    let mut item_a = CanvasItem::new(CanvasItemId(1));
    item_a.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 10.0)),
        color: red,
        filled: true,
    });

    // Green fills right half, overlapping column 4.
    let mut item_b = CanvasItem::new(CanvasItemId(2));
    item_b.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(4.0, 0.0), Vector2::new(6.0, 10.0)),
        color: green,
        filled: true,
    });

    vp.add_canvas_item(item_a);
    vp.add_canvas_item(item_b);

    let fb = capture_frame(&mut renderer, &vp);
    // Column 0 should be red.
    assert_eq!(fb.get_pixel(0, 5), red);
    // Column 4 overlaps — green is drawn second, so green wins.
    assert_eq!(fb.get_pixel(4, 5), green);
    // Column 9 is green only.
    assert_eq!(fb.get_pixel(9, 5), green);
}

// ---------------------------------------------------------------------------
// Canvas items partially/fully outside viewport (clipping)
// ---------------------------------------------------------------------------

#[test]
fn rect_fully_outside_viewport() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(20.0, 20.0), Vector2::new(5.0, 5.0)),
        color: Color::WHITE,
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(count_pixels(&fb, Color::BLACK), 100);
}

#[test]
fn rect_partially_outside_viewport() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Rect at (8,8) size 5x5 — only a 2x2 corner should be visible.
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(8.0, 8.0), Vector2::new(5.0, 5.0)),
        color: Color::WHITE,
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.get_pixel(8, 8), Color::WHITE);
    assert_eq!(fb.get_pixel(9, 9), Color::WHITE);
    // 4 white pixels (2x2 corner).
    assert_eq!(count_pixels(&fb, Color::WHITE), 4);
}

#[test]
fn rect_negative_origin_clips() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Rect at (-3,-3) size 6x6 — visible region is (0,0)..(3,3) = 9 pixels.
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-3.0, -3.0), Vector2::new(6.0, 6.0)),
        color: Color::WHITE,
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.get_pixel(0, 0), Color::WHITE);
    assert_eq!(fb.get_pixel(2, 2), Color::WHITE);
    assert_eq!(fb.get_pixel(3, 3), Color::BLACK);
    assert_eq!(count_pixels(&fb, Color::WHITE), 9);
}

#[test]
fn circle_fully_outside_viewport() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(50.0, 50.0),
        radius: 3.0,
        color: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(count_pixels(&fb, Color::BLACK), 100);
}

// ---------------------------------------------------------------------------
// Rotated/scaled transforms applied to draw commands
// ---------------------------------------------------------------------------

#[test]
fn scale_transform_doubles_rect_position() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    // Rect at (2,2) with scale 2x → position becomes (4,4).
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(3.0, 3.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let red = Color::rgb(1.0, 0.0, 0.0);
    // (4,4) should be red (scaled position).
    assert_eq!(fb.get_pixel(4, 4), red);
    // (2,2) should be black (not at original position).
    assert_eq!(fb.get_pixel(2, 2), Color::BLACK);
}

#[test]
fn rotation_90_moves_rect_position() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Rotate 90 degrees: (x,y) → (-y,x). Then translate to keep in viewport.
    let rot = Transform2D::rotated(std::f32::consts::FRAC_PI_2);
    let translate = Transform2D::translated(Vector2::new(10.0, 0.0));
    item.transform = translate * rot;
    // Point at (5,0) after 90° rotation → (0,5), then translate → (10,5).
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(5.0, 0.0), Vector2::new(3.0, 3.0)),
        color: Color::rgb(0.0, 1.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // After rotation+translation, the rect position (5,0) maps to (10,5).
    assert_eq!(fb.get_pixel(10, 5), Color::rgb(0.0, 1.0, 0.0));
}

#[test]
fn scale_transform_on_circle() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Scale 2x moves center from (5,5) to (10,10).
    item.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(5.0, 5.0),
        radius: 2.0,
        color: Color::rgb(0.0, 0.0, 1.0),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Center at scaled position (10,10) should be blue.
    assert_eq!(fb.get_pixel(10, 10), Color::rgb(0.0, 0.0, 1.0));
    // Original position (5,5) should be black.
    assert_eq!(fb.get_pixel(5, 5), Color::BLACK);
}

#[test]
fn translate_transform_on_line() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(5.0, 3.0));
    // Line from (0,0) to (9,0) → translated to (5,3) to (14,3).
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::ZERO,
        to: Vector2::new(9.0, 0.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Pixels along y=3 from x=5..14 should be white.
    for x in 5..=14 {
        assert_eq!(fb.get_pixel(x, 3), Color::WHITE, "Expected white at ({}, 3)", x);
    }
    // y=0 should be all black.
    assert_eq!(fb.get_pixel(5, 0), Color::BLACK);
}

// ---------------------------------------------------------------------------
// 100+ items stress test for z-index sorting
// ---------------------------------------------------------------------------

#[test]
fn stress_100_items_z_index_sorting() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Create 100 items with different z-indexes all drawing to (0,0).
    // The item with the highest z-index should win.
    for i in 0..100 {
        let mut item = CanvasItem::new(CanvasItemId(i + 1));
        item.z_index = i as i32;
        let intensity = i as f32 / 99.0;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(1.0, 1.0)),
            color: Color::rgb(intensity, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);
    }

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(0, 0);
    // z=99 item has intensity 1.0.
    assert!((pixel.r - 1.0).abs() < 0.02, "Expected r≈1.0, got {}", pixel.r);
}

#[test]
fn stress_100_items_reverse_z_order() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Add in reverse z-order: z=99 first, z=0 last. Sorting should still work.
    for i in (0..100).rev() {
        let mut item = CanvasItem::new(CanvasItemId(100 - i));
        item.z_index = i as i32;
        let intensity = i as f32 / 99.0;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(1.0, 1.0)),
            color: Color::rgb(intensity, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);
    }

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(0, 0);
    // z=99 should still be on top with intensity 1.0.
    assert!((pixel.r - 1.0).abs() < 0.02, "Expected r≈1.0, got {}", pixel.r);
}

#[test]
fn stress_150_items_mixed_z_and_negative() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(4, 4, Color::BLACK);

    // 150 items with z-indexes from -75 to +74.
    for i in 0..150 {
        let mut item = CanvasItem::new(CanvasItemId(i + 1));
        item.z_index = i as i32 - 75;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            color: Color::rgb(i as f32 / 149.0, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);
    }

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(0, 0);
    // Highest z-index is +74, which is i=149 → intensity 1.0.
    assert!((pixel.r - 1.0).abs() < 0.02);
}

// ---------------------------------------------------------------------------
// Negative coordinates in draw commands
// ---------------------------------------------------------------------------

#[test]
fn negative_rect_fully_offscreen() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-10.0, -10.0), Vector2::new(5.0, 5.0)),
        color: Color::WHITE,
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(count_pixels(&fb, Color::BLACK), 100);
}

#[test]
fn negative_circle_center() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Circle at (-1, -1) radius 3: should partially overlap top-left corner.
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(-1.0, -1.0),
        radius: 3.0,
        color: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Some pixels near (0,0) should be white.
    let white_count = count_pixels(&fb, Color::WHITE);
    assert!(white_count > 0, "Expected some white pixels from clipped circle");
    assert!(white_count < 100, "Not all pixels should be white");
}

#[test]
fn negative_line_endpoints() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Line from (-5, 5) to (5, 5) — only the portion x>=0 should appear.
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(-5.0, 5.0),
        to: Vector2::new(5.0, 5.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Pixels at (0,5) through (5,5) should be white.
    for x in 0..=5 {
        assert_eq!(fb.get_pixel(x, 5), Color::WHITE, "Expected white at ({}, 5)", x);
    }
}

// ---------------------------------------------------------------------------
// draw_line edge cases
// ---------------------------------------------------------------------------

#[test]
fn draw_line_horizontal() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(1.0, 5.0),
        to: Vector2::new(8.0, 5.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    for x in 1..=8 {
        assert_eq!(fb.get_pixel(x, 5), Color::WHITE);
    }
    // Adjacent rows should be black.
    assert_eq!(fb.get_pixel(5, 4), Color::BLACK);
    assert_eq!(fb.get_pixel(5, 6), Color::BLACK);
}

#[test]
fn draw_line_vertical() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(5.0, 1.0),
        to: Vector2::new(5.0, 8.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    for y in 1..=8 {
        assert_eq!(fb.get_pixel(5, y), Color::WHITE);
    }
    // Adjacent columns should be black.
    assert_eq!(fb.get_pixel(4, 5), Color::BLACK);
    assert_eq!(fb.get_pixel(6, 5), Color::BLACK);
}

#[test]
fn draw_line_diagonal() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(0.0, 0.0),
        to: Vector2::new(9.0, 9.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Diagonal should have white pixels along (i,i).
    for i in 0..10 {
        assert_eq!(fb.get_pixel(i, i), Color::WHITE, "Expected white at ({}, {})", i, i);
    }
}

#[test]
fn draw_line_zero_length() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Zero-length line: from == to. Should draw exactly one pixel.
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(5.0, 5.0),
        to: Vector2::new(5.0, 5.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_eq!(fb.get_pixel(5, 5), Color::WHITE);
    // Only one pixel should be white.
    assert_eq!(count_pixels(&fb, Color::WHITE), 1);
}

#[test]
fn draw_line_reverse_direction() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Line drawn right-to-left should produce same pixels.
    item.commands.push(DrawCommand::DrawLine {
        from: Vector2::new(8.0, 5.0),
        to: Vector2::new(1.0, 5.0),
        color: Color::WHITE,
        width: 1.0,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    for x in 1..=8 {
        assert_eq!(fb.get_pixel(x, 5), Color::WHITE);
    }
}

// ---------------------------------------------------------------------------
// fill_circle at viewport edge
// ---------------------------------------------------------------------------

#[test]
fn fill_circle_at_viewport_edge_top_left() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(0.0, 0.0),
        radius: 3.0,
        color: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Some pixels near (0,0) should be white.
    let white_count = count_pixels(&fb, Color::WHITE);
    assert!(white_count > 0, "Circle at edge should draw some pixels");
    // Should not fill entire viewport.
    assert!(white_count < 100);
}

#[test]
fn fill_circle_at_viewport_edge_bottom_right() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(9.0, 9.0),
        radius: 3.0,
        color: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let white_count = count_pixels(&fb, Color::WHITE);
    assert!(white_count > 0);
    assert!(white_count < 100);
    // Center should be white.
    assert_eq!(fb.get_pixel(9, 9), Color::WHITE);
}

#[test]
fn fill_circle_larger_than_viewport() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(6, 6, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    // Circle centered in viewport with radius larger than viewport.
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(3.0, 3.0),
        radius: 100.0,
        color: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // All pixels should be white since circle encompasses entire viewport.
    assert_eq!(count_pixels(&fb, Color::WHITE), 36);
}

// ---------------------------------------------------------------------------
// Texture drawing with non-white modulate
// ---------------------------------------------------------------------------

#[test]
fn texture_with_red_modulate() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("solid.png", Texture2D::solid(2, 2, Color::WHITE));

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "solid.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::rgb(1.0, 0.0, 0.0),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(0, 0);
    assert!((pixel.r - 1.0).abs() < 0.01);
    assert!(pixel.g.abs() < 0.01);
    assert!(pixel.b.abs() < 0.01);
}

#[test]
fn texture_with_half_green_modulate() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("solid.png", Texture2D::solid(2, 2, Color::WHITE));

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "solid.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::rgb(0.0, 0.5, 0.0),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(2, 2);
    assert!(pixel.r.abs() < 0.01);
    assert!((pixel.g - 0.5).abs() < 0.01);
    assert!(pixel.b.abs() < 0.01);
}

#[test]
fn texture_colored_with_modulate() {
    let mut renderer = SoftwareRenderer::new();
    // Blue texture modulated by green → black (0*0, 0*1, 1*0).
    renderer.register_texture("blue.png", Texture2D::solid(2, 2, Color::rgb(0.0, 0.0, 1.0)));

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "blue.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::rgb(0.0, 1.0, 0.0),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Blue * green modulate = black.
    assert_pixel_color(&fb, 1, 1, Color::new(0.0, 0.0, 0.0, 1.0), 0.01);
}

#[test]
fn texture_missing_path_no_crash() {
    let mut renderer = SoftwareRenderer::new();
    // Don't register any textures.
    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "nonexistent.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Should not crash, framebuffer stays clear.
    assert_eq!(count_pixels(&fb, Color::BLACK), 16);
}

// ---------------------------------------------------------------------------
// Additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn unfilled_rect_is_no_op() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(5.0, 5.0)),
        color: Color::WHITE,
        filled: false,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Non-filled rects are not implemented, so nothing should be drawn.
    assert_eq!(count_pixels(&fb, Color::BLACK), 100);
}

#[test]
fn zero_radius_circle() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::new(5.0, 5.0),
        radius: 0.0,
        color: Color::WHITE,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // A zero-radius circle should draw nothing or at most the center pixel.
    let white_count = count_pixels(&fb, Color::WHITE);
    assert!(white_count <= 1, "Zero-radius circle drew {} pixels", white_count);
}

#[test]
fn ppm_save_and_roundtrip() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(3, 3, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(1.0, 1.0), Vector2::new(1.0, 1.0)),
        color: Color::WHITE,
        filled: true,
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    let path = "/tmp/patina_render_pipeline_test.ppm";
    save_ppm(&fb, path).expect("Failed to save PPM");

    let content = std::fs::read_to_string(path).expect("Failed to read PPM");
    assert!(content.starts_with("P3\n3 3\n255\n"));
    assert!(content.contains("255 255 255")); // White pixel.
    let _ = std::fs::remove_file(path);
}
