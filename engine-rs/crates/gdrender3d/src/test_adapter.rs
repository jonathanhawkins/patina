//! Test adapter for headless 3D render verification.
//!
//! Utilities for capturing rendered 3D frames, inspecting pixels and depth
//! values, and converting frame data for comparison testing.

use gdcore::math::Color;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;

use crate::renderer::{FrameBuffer3D, SoftwareRenderer3D};

/// Captures a 3D frame by rendering the viewport with the given renderer.
pub fn capture_frame_3d(renderer: &mut SoftwareRenderer3D, viewport: &Viewport3D) -> FrameBuffer3D {
    let frame = renderer.render_frame(viewport);
    frame_data_to_buffer_3d(&frame)
}

/// Returns the color at `(x, y)` in the framebuffer.
///
/// # Panics
///
/// Panics if `(x, y)` is out of bounds.
pub fn pixel_at_3d(fb: &FrameBuffer3D, x: u32, y: u32) -> Color {
    fb.get_pixel(x, y)
}

/// Returns the depth at `(x, y)` in the framebuffer.
///
/// # Panics
///
/// Panics if `(x, y)` is out of bounds.
pub fn depth_at(fb: &FrameBuffer3D, x: u32, y: u32) -> f32 {
    fb.get_depth(x, y)
}

/// Asserts that the pixel at `(x, y)` matches `expected` within `tolerance`.
///
/// # Panics
///
/// Panics if the color difference exceeds tolerance on any channel.
pub fn assert_pixel_color_3d(fb: &FrameBuffer3D, x: u32, y: u32, expected: Color, tolerance: f32) {
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

/// Asserts that the depth at `(x, y)` matches `expected` within `tolerance`.
///
/// # Panics
///
/// Panics if the depth difference exceeds tolerance.
pub fn assert_depth_3d(fb: &FrameBuffer3D, x: u32, y: u32, expected: f32, tolerance: f32) {
    let actual = fb.get_depth(x, y);
    assert!(
        (actual - expected).abs() <= tolerance,
        "Depth ({}, {}): expected {}, got {} (tolerance {})",
        x,
        y,
        expected,
        actual,
        tolerance,
    );
}

/// Saves the framebuffer color channel as a PPM (P3) image file for debugging.
pub fn save_ppm_3d(fb: &FrameBuffer3D, path: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "P3")?;
    writeln!(file, "{} {}", fb.width, fb.height)?;
    writeln!(file, "255")?;
    for pixel in &fb.pixels {
        let r = (pixel.r.clamp(0.0, 1.0) * 255.0) as u8;
        let g = (pixel.g.clamp(0.0, 1.0) * 255.0) as u8;
        let b = (pixel.b.clamp(0.0, 1.0) * 255.0) as u8;
        writeln!(file, "{r} {g} {b}")?;
    }
    Ok(())
}

/// Converts a [`FrameData3D`] into a [`FrameBuffer3D`].
pub fn frame_data_to_buffer_3d(frame: &FrameData3D) -> FrameBuffer3D {
    FrameBuffer3D {
        width: frame.width,
        height: frame.height,
        pixels: frame.pixels.clone(),
        depth: frame.depth.clone(),
    }
}

/// Counts non-background (non-black) pixels in the framebuffer.
pub fn count_visible_pixels(fb: &FrameBuffer3D) -> usize {
    fb.pixels.iter().filter(|c| **c != Color::BLACK).count()
}

/// Counts pixels that have been written to in the depth buffer (depth < max).
pub fn count_depth_written(fb: &FrameBuffer3D) -> usize {
    fb.depth.iter().filter(|d| **d < 1.0).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;
    use gdcore::math::Vector3;
    use gdcore::math3d::{Basis, Transform3D};
    use gdserver3d::material::Material3D;
    use gdserver3d::mesh::Mesh3D;

    #[test]
    fn capture_and_inspect_pixel() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));

        let mut mat = Material3D::default();
        mat.albedo = Color::rgb(1.0, 0.0, 0.0);
        renderer.set_material(id, mat);

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        let vp = Viewport3D::new(64, 64);
        let fb = capture_frame_3d(&mut renderer, &vp);
        assert_eq!(fb.width, 64);
        assert_eq!(fb.height, 64);

        let visible = count_visible_pixels(&fb);
        assert!(visible > 0, "cube should produce visible pixels");
    }

    #[test]
    fn frame_data_roundtrip() {
        let frame = FrameData3D {
            width: 4,
            height: 4,
            pixels: vec![Color::rgb(0.5, 0.5, 0.5); 16],
            depth: vec![0.5; 16],
        };
        let fb = frame_data_to_buffer_3d(&frame);
        assert_eq!(fb.width, 4);
        assert_eq!(fb.pixels.len(), 16);
        assert_eq!(fb.depth.len(), 16);
        assert_eq!(fb.get_depth(0, 0), 0.5);
    }

    #[test]
    fn count_visible_and_depth() {
        let mut fb = FrameBuffer3D::new(4, 4, Color::BLACK);
        assert_eq!(count_visible_pixels(&fb), 0);
        assert_eq!(count_depth_written(&fb), 0);

        fb.set_pixel(0, 0, Color::WHITE);
        fb.depth[0] = 0.5;
        assert_eq!(count_visible_pixels(&fb), 1);
        assert_eq!(count_depth_written(&fb), 1);
    }
}
