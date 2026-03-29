//! Triangle rasterizer with perspective-correct attribute interpolation.
//!
//! Converts projected triangles into fragments, applying depth testing and
//! invoking the fragment shader per pixel.

use gdcore::math::Color;

use crate::depth_buffer::DepthBuffer;
use crate::shader::{FragmentInput, FragmentShader, ShaderUniforms, VertexOutput};

/// Screen-space vertex after projection and viewport mapping.
#[derive(Debug, Clone, Copy)]
pub struct ScreenVertex {
    /// Screen X coordinate.
    pub x: f32,
    /// Screen Y coordinate.
    pub y: f32,
    /// NDC depth for depth testing.
    pub depth: f32,
    /// Reciprocal of clip W for perspective-correct interpolation.
    pub w_recip: f32,
    /// The full vertex output (for attribute interpolation).
    pub vertex: VertexOutput,
}

/// Converts a clip-space [`VertexOutput`] to a [`ScreenVertex`].
///
/// Returns `None` if the vertex is behind the near plane (w <= 0).
pub fn clip_to_screen(v: &VertexOutput, width: u32, height: u32) -> Option<ScreenVertex> {
    let w = v.clip_position[3];
    if w <= 0.0 {
        return None;
    }

    let w_recip = 1.0 / w;
    let ndc_x = v.clip_position[0] * w_recip;
    let ndc_y = v.clip_position[1] * w_recip;
    let depth = v.clip_position[2] * w_recip;

    let x = (ndc_x + 1.0) * 0.5 * width as f32;
    let y = (1.0 - ndc_y) * 0.5 * height as f32;

    Some(ScreenVertex {
        x,
        y,
        depth,
        w_recip,
        vertex: *v,
    })
}

/// Rasterizes a single triangle into the pixel and depth buffers.
///
/// Uses edge-function (barycentric) rasterization with perspective-correct
/// attribute interpolation. The fragment shader is invoked for each visible
/// fragment that passes the depth test.
pub fn rasterize_triangle(
    v0: &ScreenVertex,
    v1: &ScreenVertex,
    v2: &ScreenVertex,
    pixels: &mut [Color],
    depth_buf: &mut DepthBuffer,
    width: u32,
    height: u32,
    fragment_shader: &dyn FragmentShader,
    uniforms: &ShaderUniforms,
) {
    // Compute bounding box.
    let min_x = v0.x.min(v1.x).min(v2.x).max(0.0) as i32;
    let max_x = v0.x.max(v1.x).max(v2.x).min(width as f32 - 1.0) as i32;
    let min_y = v0.y.min(v1.y).min(v2.y).max(0.0) as i32;
    let max_y = v0.y.max(v1.y).max(v2.y).min(height as f32 - 1.0) as i32;

    if min_x > max_x || min_y > max_y {
        return;
    }

    // Edge function: positive = inside (CCW winding).
    let area = edge_function(v0.x, v0.y, v1.x, v1.y, v2.x, v2.y);
    if area.abs() < 1e-6 {
        return; // Degenerate triangle.
    }
    let area_recip = 1.0 / area;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let px_f = px as f32 + 0.5;
            let py_f = py as f32 + 0.5;

            let w0 = edge_function(v1.x, v1.y, v2.x, v2.y, px_f, py_f);
            let w1 = edge_function(v2.x, v2.y, v0.x, v0.y, px_f, py_f);
            let w2 = edge_function(v0.x, v0.y, v1.x, v1.y, px_f, py_f);

            // Check if inside triangle (handle both CW and CCW winding).
            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };

            if !inside {
                continue;
            }

            // Barycentric coordinates.
            let b0 = w0 * area_recip;
            let b1 = w1 * area_recip;
            let b2 = w2 * area_recip;

            // Perspective-correct interpolation.
            let z_recip = b0 * v0.w_recip + b1 * v1.w_recip + b2 * v2.w_recip;
            if z_recip <= 0.0 {
                continue;
            }
            let z_correct = 1.0 / z_recip;

            let depth = b0 * v0.depth + b1 * v1.depth + b2 * v2.depth;

            let ux = px as u32;
            let uy = py as u32;

            if !depth_buf.test_and_set(ux, uy, depth) {
                continue;
            }

            // Perspective-correct barycentric weights.
            let pc0 = b0 * v0.w_recip * z_correct;
            let pc1 = b1 * v1.w_recip * z_correct;
            let pc2 = b2 * v2.w_recip * z_correct;

            let interpolated =
                VertexOutput::barycentric(&v0.vertex, &v1.vertex, &v2.vertex, pc0, pc1, pc2);

            let frag_input = FragmentInput {
                world_position: interpolated.world_position,
                world_normal: interpolated.world_normal,
                uv: interpolated.uv,
                depth,
            };

            let color = fragment_shader.process(&frag_input, uniforms);
            pixels[(uy * width + ux) as usize] = color;
        }
    }
}

/// 2D edge function (signed area of parallelogram).
fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::{FragmentInput, ShaderUniforms, UnlitFragmentShader, VertexOutput};
    use gdcore::math::{Color, Vector3};

    fn identity_matrix() -> [[f32; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    fn default_uniforms() -> ShaderUniforms {
        ShaderUniforms {
            model_matrix: identity_matrix(),
            view_matrix: identity_matrix(),
            projection_matrix: identity_matrix(),
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            roughness: 0.5,
            metallic: 0.0,
            camera_position: Vector3::ZERO,
            lights: vec![],
            directional_shadow_maps: vec![],
            omni_shadow_cubemaps: vec![],
        }
    }

    fn make_screen_vertex(x: f32, y: f32, depth: f32) -> ScreenVertex {
        ScreenVertex {
            x,
            y,
            depth,
            w_recip: 1.0,
            vertex: VertexOutput {
                clip_position: [x, y, depth, 1.0],
                world_position: Vector3::new(x, y, depth),
                world_normal: Vector3::UP,
                uv: [0.0, 0.0],
            },
        }
    }

    #[test]
    fn edge_function_positive_ccw() {
        let val = edge_function(0.0, 0.0, 10.0, 0.0, 5.0, 10.0);
        assert!(val > 0.0);
    }

    #[test]
    fn edge_function_negative_cw() {
        let val = edge_function(0.0, 0.0, 5.0, 10.0, 10.0, 0.0);
        assert!(val < 0.0);
    }

    #[test]
    fn clip_to_screen_behind_camera() {
        let v = VertexOutput {
            clip_position: [1.0, 1.0, 1.0, -1.0],
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
        };
        assert!(clip_to_screen(&v, 64, 64).is_none());
    }

    #[test]
    fn clip_to_screen_center_maps_correctly() {
        let v = VertexOutput {
            clip_position: [0.0, 0.0, 0.5, 1.0],
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
        };
        let sv = clip_to_screen(&v, 100, 100).unwrap();
        assert!((sv.x - 50.0).abs() < 1e-3);
        assert!((sv.y - 50.0).abs() < 1e-3);
        assert!((sv.depth - 0.5).abs() < 1e-3);
    }

    #[test]
    fn rasterize_fills_triangle_pixels() {
        let width = 16u32;
        let height = 16u32;
        let count = (width * height) as usize;
        let mut pixels = vec![Color::BLACK; count];
        let mut depth = DepthBuffer::new(width, height);

        let v0 = make_screen_vertex(2.0, 2.0, 0.5);
        let v1 = make_screen_vertex(14.0, 2.0, 0.5);
        let v2 = make_screen_vertex(8.0, 14.0, 0.5);

        let fs = UnlitFragmentShader;
        let uniforms = default_uniforms();

        rasterize_triangle(
            &v0,
            &v1,
            &v2,
            &mut pixels,
            &mut depth,
            width,
            height,
            &fs,
            &uniforms,
        );

        let filled = pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(
            filled > 10,
            "triangle should fill many pixels, got {filled}"
        );
    }

    #[test]
    fn rasterize_respects_depth_test() {
        let width = 8u32;
        let height = 8u32;
        let count = (width * height) as usize;
        let mut pixels = vec![Color::BLACK; count];
        let mut depth = DepthBuffer::new(width, height);

        let v0 = make_screen_vertex(0.0, 0.0, 0.3);
        let v1 = make_screen_vertex(7.0, 0.0, 0.3);
        let v2 = make_screen_vertex(3.5, 7.0, 0.3);

        let fs = UnlitFragmentShader;
        let mut u1 = default_uniforms();
        u1.albedo = Color::new(1.0, 0.0, 0.0, 1.0);

        rasterize_triangle(
            &v0,
            &v1,
            &v2,
            &mut pixels,
            &mut depth,
            width,
            height,
            &fs,
            &u1,
        );

        let red_count = pixels.iter().filter(|c| c.r > 0.9 && c.g < 0.1).count();

        // Now draw a farther triangle — should NOT overwrite.
        let v0b = make_screen_vertex(0.0, 0.0, 0.8);
        let v1b = make_screen_vertex(7.0, 0.0, 0.8);
        let v2b = make_screen_vertex(3.5, 7.0, 0.8);

        let mut u2 = default_uniforms();
        u2.albedo = Color::new(0.0, 0.0, 1.0, 1.0);

        rasterize_triangle(
            &v0b,
            &v1b,
            &v2b,
            &mut pixels,
            &mut depth,
            width,
            height,
            &fs,
            &u2,
        );

        let still_red = pixels.iter().filter(|c| c.r > 0.9 && c.g < 0.1).count();
        assert_eq!(
            red_count, still_red,
            "farther triangle should not overwrite nearer"
        );
    }

    #[test]
    fn degenerate_triangle_produces_no_pixels() {
        let width = 8u32;
        let height = 8u32;
        let count = (width * height) as usize;
        let mut pixels = vec![Color::BLACK; count];
        let mut depth = DepthBuffer::new(width, height);

        // Collinear points = degenerate.
        let v0 = make_screen_vertex(0.0, 0.0, 0.5);
        let v1 = make_screen_vertex(4.0, 4.0, 0.5);
        let v2 = make_screen_vertex(8.0, 8.0, 0.5);

        let fs = UnlitFragmentShader;
        let uniforms = default_uniforms();

        rasterize_triangle(
            &v0,
            &v1,
            &v2,
            &mut pixels,
            &mut depth,
            width,
            height,
            &fs,
            &uniforms,
        );

        let filled = pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert_eq!(filled, 0, "degenerate triangle should produce no pixels");
    }

    #[test]
    fn offscreen_triangle_no_crash() {
        let width = 8u32;
        let height = 8u32;
        let count = (width * height) as usize;
        let mut pixels = vec![Color::BLACK; count];
        let mut depth = DepthBuffer::new(width, height);

        let v0 = make_screen_vertex(-100.0, -100.0, 0.5);
        let v1 = make_screen_vertex(-90.0, -100.0, 0.5);
        let v2 = make_screen_vertex(-95.0, -90.0, 0.5);

        let fs = UnlitFragmentShader;
        let uniforms = default_uniforms();

        rasterize_triangle(
            &v0,
            &v1,
            &v2,
            &mut pixels,
            &mut depth,
            width,
            height,
            &fs,
            &uniforms,
        );

        let filled = pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert_eq!(filled, 0);
    }
}
