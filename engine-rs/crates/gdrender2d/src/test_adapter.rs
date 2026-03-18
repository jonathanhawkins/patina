//! Test adapter for headless render verification.
//!
//! Utilities for capturing rendered frames, inspecting pixels, and
//! saving framebuffers to PPM files for visual debugging.

use gdcore::math::Color;
use gdserver2d::server::FrameData;

use crate::renderer::{FrameBuffer, SoftwareRenderer};
use gdserver2d::viewport::Viewport;
use gdserver2d::server::RenderingServer2D;

/// Captures a frame by rendering the viewport with the given renderer.
pub fn capture_frame(renderer: &mut SoftwareRenderer, viewport: &Viewport) -> FrameBuffer {
    let frame = renderer.render_frame(viewport);
    FrameBuffer {
        width: frame.width,
        height: frame.height,
        pixels: frame.pixels,
    }
}

/// Returns the color at `(x, y)` in the framebuffer.
///
/// # Panics
///
/// Panics if `(x, y)` is out of bounds.
pub fn pixel_at(fb: &FrameBuffer, x: u32, y: u32) -> Color {
    fb.get_pixel(x, y)
}

/// Asserts that the pixel at `(x, y)` matches `expected` within `tolerance`.
///
/// # Panics
///
/// Panics if the color difference exceeds tolerance on any channel.
pub fn assert_pixel_color(fb: &FrameBuffer, x: u32, y: u32, expected: Color, tolerance: f32) {
    let actual = fb.get_pixel(x, y);
    assert!(
        (actual.r - expected.r).abs() <= tolerance
            && (actual.g - expected.g).abs() <= tolerance
            && (actual.b - expected.b).abs() <= tolerance
            && (actual.a - expected.a).abs() <= tolerance,
        "Pixel ({}, {}): expected {:?}, got {:?} (tolerance {})",
        x,
        y,
        expected,
        actual,
        tolerance,
    );
}

/// Saves the framebuffer as a PPM (P3) image file for visual debugging.
pub fn save_ppm(fb: &FrameBuffer, path: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "P3")?;
    writeln!(file, "{} {}", fb.width, fb.height)?;
    writeln!(file, "255")?;
    for pixel in &fb.pixels {
        let r = (pixel.r.clamp(0.0, 1.0) * 255.0) as u8;
        let g = (pixel.g.clamp(0.0, 1.0) * 255.0) as u8;
        let b = (pixel.b.clamp(0.0, 1.0) * 255.0) as u8;
        writeln!(file, "{} {} {}", r, g, b)?;
    }
    Ok(())
}

/// Converts a [`FrameData`] into a [`FrameBuffer`].
pub fn frame_data_to_buffer(frame: &FrameData) -> FrameBuffer {
    FrameBuffer {
        width: frame.width,
        height: frame.height,
        pixels: frame.pixels.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Rect2, Vector2};
    use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};

    #[test]
    fn capture_and_inspect_pixel() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(10, 10, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);

        let fb = capture_frame(&mut renderer, &vp);
        let c = pixel_at(&fb, 2, 2);
        assert_eq!(c, Color::rgb(1.0, 0.0, 0.0));

        // Should not panic.
        assert_pixel_color(&fb, 2, 2, Color::rgb(1.0, 0.0, 0.0), 0.01);
    }

    #[test]
    fn assert_pixel_color_within_tolerance() {
        let fb = FrameBuffer::new(4, 4, Color::rgb(0.501, 0.499, 0.5));
        assert_pixel_color(&fb, 0, 0, Color::rgb(0.5, 0.5, 0.5), 0.01);
    }

    #[test]
    fn save_ppm_creates_file() {
        let fb = FrameBuffer::new(2, 2, Color::rgb(1.0, 0.0, 0.0));
        let path = "/tmp/patina_test_render.ppm";
        save_ppm(&fb, path).expect("failed to write PPM");
        let content = std::fs::read_to_string(path).expect("failed to read PPM");
        assert!(content.starts_with("P3"));
        assert!(content.contains("255 0 0"));
        let _ = std::fs::remove_file(path);
    }
}
