//! pat-2vw: Texture atlas sampling matches upstream pixel output within tolerance.
//!
//! Validates that draw_texture_region correctly samples sub-regions of a
//! texture atlas, producing pixel-accurate output within a small tolerance.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::draw::draw_texture_region;
use gdrender2d::{FrameBuffer, Texture2D};

fn color_distance(a: Color, b: Color) -> f32 {
    let dr = a.r - b.r;
    let dg = a.g - b.g;
    let db = a.b - b.b;
    let da = a.a - b.a;
    (dr * dr + dg * dg + db * db + da * da).sqrt()
}

const TOLERANCE: f32 = 0.02;

/// Create a 4x4 atlas with distinct colored quadrants:
/// Top-left: Red, Top-right: Green, Bottom-left: Blue, Bottom-right: White
fn make_test_atlas() -> Texture2D {
    let r = Color::new(1.0, 0.0, 0.0, 1.0);
    let g = Color::new(0.0, 1.0, 0.0, 1.0);
    let b = Color::new(0.0, 0.0, 1.0, 1.0);
    let w = Color::new(1.0, 1.0, 1.0, 1.0);
    Texture2D {
        width: 4,
        height: 4,
        pixels: vec![
            r, r, g, g, // row 0
            r, r, g, g, // row 1
            b, b, w, w, // row 2
            b, b, w, w, // row 3
        ],
    }
}

#[test]
fn atlas_sample_top_left_quadrant_is_red() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(2, 2, Color::BLACK);

    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)), // top-left 2x2
        Color::WHITE,
    );

    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    for y in 0..2 {
        for x in 0..2 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, red) < TOLERANCE,
                "pixel ({x},{y}) should be red, got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_sample_top_right_quadrant_is_green() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(2, 2, Color::BLACK);

    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        Rect2::new(Vector2::new(2.0, 0.0), Vector2::new(2.0, 2.0)), // top-right 2x2
        Color::WHITE,
    );

    let green = Color::new(0.0, 1.0, 0.0, 1.0);
    for y in 0..2 {
        for x in 0..2 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, green) < TOLERANCE,
                "pixel ({x},{y}) should be green, got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_sample_bottom_left_quadrant_is_blue() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(2, 2, Color::BLACK);

    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        Rect2::new(Vector2::new(0.0, 2.0), Vector2::new(2.0, 2.0)), // bottom-left
        Color::WHITE,
    );

    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    for y in 0..2 {
        for x in 0..2 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, blue) < TOLERANCE,
                "pixel ({x},{y}) should be blue, got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_sample_bottom_right_quadrant_is_white() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(2, 2, Color::BLACK);

    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0)), // bottom-right
        Color::WHITE,
    );

    let white = Color::new(1.0, 1.0, 1.0, 1.0);
    for y in 0..2 {
        for x in 0..2 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, white) < TOLERANCE,
                "pixel ({x},{y}) should be white, got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_modulate_tints_output() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(2, 2, Color::BLACK);

    // Sample the white quadrant but tint it to half-red
    let tint = Color::new(1.0, 0.0, 0.0, 0.5);
    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0)),
        tint,
    );

    let expected = Color::new(1.0, 0.0, 0.0, 0.5);
    for y in 0..2 {
        for x in 0..2 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, expected) < TOLERANCE,
                "pixel ({x},{y}) should be tinted red at 50% alpha, got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_scaled_up_samples_correctly() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(4, 4, Color::BLACK);

    // Sample 2x2 region but render to 4x4 (2x scale up)
    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)), // red quadrant
        Color::WHITE,
    );

    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    // All 4x4 pixels should be red (nearest-neighbor sampling)
    for y in 0..4 {
        for x in 0..4 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, red) < TOLERANCE,
                "scaled pixel ({x},{y}) should be red, got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_zero_size_source_is_noop() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(4, 4, Color::BLACK);

    // Zero-width source rect should be a no-op
    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        Rect2::new(Vector2::ZERO, Vector2::new(0.0, 2.0)),
        Color::WHITE,
    );

    // All pixels should remain black
    for y in 0..4 {
        for x in 0..4 {
            let px = fb.get_pixel(x, y);
            assert!(
                color_distance(px, Color::BLACK) < TOLERANCE,
                "pixel ({x},{y}) should be black (no-op), got {:?}",
                px
            );
        }
    }
}

#[test]
fn atlas_full_texture_as_region() {
    let atlas = make_test_atlas();
    let mut fb = FrameBuffer::new(4, 4, Color::BLACK);

    // Sample the entire 4x4 atlas into 4x4 framebuffer
    draw_texture_region(
        &mut fb,
        &atlas,
        Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        Color::WHITE,
    );

    // Check quadrant corners
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let green = Color::new(0.0, 1.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    let white = Color::new(1.0, 1.0, 1.0, 1.0);

    assert!(color_distance(fb.get_pixel(0, 0), red) < TOLERANCE, "top-left red");
    assert!(color_distance(fb.get_pixel(3, 0), green) < TOLERANCE, "top-right green");
    assert!(color_distance(fb.get_pixel(0, 3), blue) < TOLERANCE, "bottom-left blue");
    assert!(color_distance(fb.get_pixel(3, 3), white) < TOLERANCE, "bottom-right white");
}
