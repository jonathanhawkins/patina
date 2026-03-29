//! Shadow map generation for directional lights.
//!
//! Renders the scene from a directional light's perspective using an
//! orthographic projection, storing depth values in a [`DepthBuffer`].
//! The resulting shadow map is sampled during the main render pass to
//! determine whether each fragment is in shadow.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdserver3d::instance::Instance3D;
use gdserver3d::light::{CubeFace, Light3D, Light3DId, LightType, OmniShadowMode, ShadowCubemap};
use gdserver3d::projection::orthographic_projection_matrix;

use crate::depth_buffer::DepthBuffer;

/// Resolution of the shadow map (width and height in texels).
pub const SHADOW_MAP_SIZE: u32 = 256;

/// Half-extent of the orthographic frustum for directional light shadow maps.
/// Objects within [-SHADOW_EXTENT, SHADOW_EXTENT] along each axis from the
/// scene center will be captured.
pub const SHADOW_EXTENT: f32 = 20.0;

/// Near and far planes for the shadow camera.
pub const SHADOW_NEAR: f32 = 0.1;
pub const SHADOW_FAR: f32 = 100.0;

/// Shadow bias to prevent self-shadowing artifacts (acne).
pub const SHADOW_BIAS: f32 = 0.005;

/// A generated shadow map for a single light source.
#[derive(Debug, Clone)]
pub struct ShadowMap {
    /// The light this shadow map belongs to.
    pub light_id: Light3DId,
    /// Depth buffer rendered from the light's perspective.
    pub depth: DepthBuffer,
    /// Light-space view matrix (inverse of light transform).
    pub view_matrix: [[f32; 4]; 4],
    /// Light-space orthographic projection matrix.
    pub proj_matrix: [[f32; 4]; 4],
    /// Shadow map resolution.
    pub size: u32,
}

impl ShadowMap {
    /// Tests whether a world-space point is in shadow.
    ///
    /// Returns a shadow factor: 0.0 = fully lit, 1.0 = fully in shadow.
    pub fn sample(&self, world_pos: Vector3) -> f32 {
        // Transform to light space.
        let view_pos = mat4_mul_point(&self.view_matrix, world_pos);
        let clip = mat4_mul_point4(&self.proj_matrix, view_pos);

        // Perspective divide (orthographic, so w=1 but be safe).
        let w = clip[3];
        if w <= 0.0 {
            return 0.0;
        }
        let ndc_x = clip[0] / w;
        let ndc_y = clip[1] / w;
        let depth = clip[2] / w;

        // Map NDC [-1,1] to texel coords [0, size].
        let u = (ndc_x + 1.0) * 0.5 * self.size as f32;
        let v = (1.0 - ndc_y) * 0.5 * self.size as f32;

        let tx = u as i32;
        let ty = v as i32;

        if tx < 0 || tx >= self.size as i32 || ty < 0 || ty >= self.size as i32 {
            return 0.0; // Outside shadow map = lit.
        }

        let stored_depth = self.depth.get(tx as u32, ty as u32);
        if stored_depth >= f32::MAX {
            return 0.0; // No occluder at this texel.
        }

        // Compare: if fragment is farther than stored depth + bias, it's in shadow.
        if depth > stored_depth + SHADOW_BIAS {
            1.0
        } else {
            0.0
        }
    }
}

/// Builds a view matrix (world → light space) for a directional light.
///
/// The light "looks" along its direction from a position high above the scene
/// center.
pub fn directional_light_view_matrix(light: &Light3D) -> [[f32; 4]; 4] {
    let dir = light.direction.normalized();

    // Build a look-at transform: light looking along `dir`.
    // Pick an up vector that isn't parallel to dir.
    let up_candidate = if dir.y.abs() > 0.99 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };

    let right = up_candidate.cross(dir).normalized();
    let up = dir.cross(right).normalized();

    // Light position: offset opposite to direction so scene is in front.
    let light_pos = dir * -50.0;

    let light_transform = Transform3D {
        basis: Basis {
            x: right,
            y: up,
            z: dir * -1.0, // -Z is forward in our convention.
        },
        origin: light_pos,
    };

    transform_to_matrix(&light_transform.inverse())
}

/// Builds an orthographic projection for the shadow map.
pub fn directional_light_proj_matrix() -> [[f32; 4]; 4] {
    orthographic_projection_matrix(
        -SHADOW_EXTENT,
        SHADOW_EXTENT,
        -SHADOW_EXTENT,
        SHADOW_EXTENT,
        SHADOW_NEAR,
        SHADOW_FAR,
    )
}

/// Generates shadow maps for all lights that have `shadow_enabled = true`.
pub fn generate_shadow_maps(lights: &[Light3D], instances: &[Instance3D]) -> Vec<ShadowMap> {
    lights
        .iter()
        .filter(|l| l.shadow_enabled && l.light_type == LightType::Directional)
        .map(|l| generate_directional_shadow_map(l, instances))
        .collect()
}

/// Resolution for omni shadow cubemap faces.
pub const OMNI_SHADOW_SIZE: u32 = 128;

/// Generates cubemap shadow maps for all point lights with shadows enabled
/// and `omni_shadow_mode == Cube`.
///
/// Returns a vec parallel to `lights` — entries are `Some` for qualifying
/// point lights, `None` otherwise.
pub fn generate_omni_shadow_cubemaps(
    lights: &[Light3D],
    instances: &[Instance3D],
) -> Vec<Option<ShadowCubemap>> {
    lights
        .iter()
        .map(|l| {
            if l.shadow_enabled
                && l.light_type == LightType::Point
                && l.omni_shadow_mode == OmniShadowMode::Cube
            {
                Some(generate_omni_shadow_cubemap(l, instances))
            } else {
                None
            }
        })
        .collect()
}

/// Generates a single cubemap shadow map for a point light.
///
/// Renders the scene from the light's position into 6 cubemap faces using a
/// 90-degree perspective projection. Depth values are linear distance from light.
fn generate_omni_shadow_cubemap(light: &Light3D, instances: &[Instance3D]) -> ShadowCubemap {
    let size = OMNI_SHADOW_SIZE;
    let mut cubemap = ShadowCubemap::new(size);
    let near = 0.05_f32;
    let far = if light.range > 0.0 {
        light.range
    } else {
        100.0
    };

    for face in CubeFace::ALL {
        let view = omni_face_view_matrix(light.position, face);
        let proj = perspective_90_projection(near, far);

        for inst in instances {
            if !inst.visible {
                continue;
            }
            let mesh = match &inst.mesh {
                Some(m) => m,
                None => continue,
            };

            let model_matrix = transform_to_matrix(&inst.transform);
            let mut i = 0;
            while i + 2 < mesh.indices.len() {
                let tri = [
                    mesh.indices[i] as usize,
                    mesh.indices[i + 1] as usize,
                    mesh.indices[i + 2] as usize,
                ];
                i += 3;

                let mut screen_pts: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
                let mut valid = true;

                for (j, &vi) in tri.iter().enumerate() {
                    if vi >= mesh.vertices.len() {
                        valid = false;
                        break;
                    }
                    let world = mat4_mul_point(&model_matrix, mesh.vertices[vi]);
                    let view_pos = mat4_mul_point(&view, world);
                    let clip = mat4_mul_point4(&proj, view_pos);

                    let w = clip[3];
                    if w <= 0.0 {
                        valid = false;
                        break;
                    }

                    let ndc_x = clip[0] / w;
                    let ndc_y = clip[1] / w;
                    // Linear distance from light for cubemap comparison.
                    let depth = (world - light.position).length();

                    let sx = (ndc_x + 1.0) * 0.5 * size as f32;
                    let sy = (1.0 - ndc_y) * 0.5 * size as f32;

                    screen_pts[j] = (sx, sy, depth);
                }

                if !valid {
                    continue;
                }

                rasterize_cubemap_face_triangle(&screen_pts, &mut cubemap, face, size);
            }
        }
    }

    cubemap
}

/// Builds a view matrix for one cubemap face, looking from `pos` along the
/// face's forward direction.
fn omni_face_view_matrix(pos: Vector3, face: CubeFace) -> [[f32; 4]; 4] {
    let forward = face.forward();
    let up = face.up();
    let right = up.cross(forward).normalized();
    let adjusted_up = forward.cross(right).normalized();

    let t = Transform3D {
        basis: Basis {
            x: right,
            y: adjusted_up,
            z: forward * -1.0, // look along -Z
        },
        origin: pos,
    };
    transform_to_matrix(&t.inverse())
}

/// Builds a 90-degree FOV perspective projection for cubemap face rendering.
fn perspective_90_projection(near: f32, far: f32) -> [[f32; 4]; 4] {
    // 90-degree FOV → tan(45°) = 1.0, aspect = 1.0
    let f = 1.0_f32;
    [
        [f, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, -(far + near) / (far - near), -1.0],
        [0.0, 0.0, -2.0 * far * near / (far - near), 0.0],
    ]
}

/// Rasterizes a triangle into a cubemap face's depth storage.
fn rasterize_cubemap_face_triangle(
    pts: &[(f32, f32, f32); 3],
    cubemap: &mut ShadowCubemap,
    face: CubeFace,
    size: u32,
) {
    let (x0, y0, z0) = pts[0];
    let (x1, y1, z1) = pts[1];
    let (x2, y2, z2) = pts[2];

    let min_x = x0.min(x1).min(x2).max(0.0) as i32;
    let max_x = x0.max(x1).max(x2).min(size as f32 - 1.0) as i32;
    let min_y = y0.min(y1).min(y2).max(0.0) as i32;
    let max_y = y0.max(y1).max(y2).min(size as f32 - 1.0) as i32;

    if min_x > max_x || min_y > max_y {
        return;
    }

    let area = edge_function(x0, y0, x1, y1, x2, y2);
    if area.abs() < 1e-6 {
        return;
    }
    let area_recip = 1.0 / area;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let px_f = px as f32 + 0.5;
            let py_f = py as f32 + 0.5;

            let w0 = edge_function(x1, y1, x2, y2, px_f, py_f);
            let w1 = edge_function(x2, y2, x0, y0, px_f, py_f);
            let w2 = edge_function(x0, y0, x1, y1, px_f, py_f);

            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };

            if !inside {
                continue;
            }

            let b0 = w0 * area_recip;
            let b1 = w1 * area_recip;
            let b2 = w2 * area_recip;

            let depth = b0 * z0 + b1 * z1 + b2 * z2;
            cubemap.test_and_set(face, px as u32, py as u32, depth);
        }
    }
}

/// Generates a single shadow map for a directional light.
fn generate_directional_shadow_map(light: &Light3D, instances: &[Instance3D]) -> ShadowMap {
    let size = SHADOW_MAP_SIZE;
    let mut depth_buf = DepthBuffer::new(size, size);
    let view_matrix = directional_light_view_matrix(light);
    let proj_matrix = directional_light_proj_matrix();

    for inst in instances {
        if !inst.visible {
            continue;
        }
        let mesh = match &inst.mesh {
            Some(m) => m,
            None => continue,
        };

        let model_matrix = transform_to_matrix(&inst.transform);

        let indices = &mesh.indices;
        let verts = &mesh.vertices;

        let mut i = 0;
        while i + 2 < indices.len() {
            let tri = [
                indices[i] as usize,
                indices[i + 1] as usize,
                indices[i + 2] as usize,
            ];
            i += 3;

            // Project all three vertices into light space.
            let mut screen_pts: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
            let mut valid = true;

            for (j, &vi) in tri.iter().enumerate() {
                if vi >= verts.len() {
                    valid = false;
                    break;
                }
                // Model → world.
                let world = mat4_mul_point(&model_matrix, verts[vi]);
                // World → light view.
                let view = mat4_mul_point(&view_matrix, world);
                // Light view → clip.
                let clip = mat4_mul_point4(&proj_matrix, view);

                let w = clip[3];
                if w <= 0.0 {
                    valid = false;
                    break;
                }

                let ndc_x = clip[0] / w;
                let ndc_y = clip[1] / w;
                let depth = clip[2] / w;

                let sx = (ndc_x + 1.0) * 0.5 * size as f32;
                let sy = (1.0 - ndc_y) * 0.5 * size as f32;

                screen_pts[j] = (sx, sy, depth);
            }

            if !valid {
                continue;
            }

            // Rasterize triangle into shadow map depth buffer.
            rasterize_shadow_triangle(&screen_pts, &mut depth_buf, size, size);
        }
    }

    ShadowMap {
        light_id: light.id,
        depth: depth_buf,
        view_matrix,
        proj_matrix,
        size,
    }
}

/// Rasterizes a triangle into a depth buffer only (no color output).
fn rasterize_shadow_triangle(
    pts: &[(f32, f32, f32); 3],
    depth_buf: &mut DepthBuffer,
    width: u32,
    height: u32,
) {
    let (x0, y0, z0) = pts[0];
    let (x1, y1, z1) = pts[1];
    let (x2, y2, z2) = pts[2];

    let min_x = x0.min(x1).min(x2).max(0.0) as i32;
    let max_x = x0.max(x1).max(x2).min(width as f32 - 1.0) as i32;
    let min_y = y0.min(y1).min(y2).max(0.0) as i32;
    let max_y = y0.max(y1).max(y2).min(height as f32 - 1.0) as i32;

    if min_x > max_x || min_y > max_y {
        return;
    }

    let area = edge_function(x0, y0, x1, y1, x2, y2);
    if area.abs() < 1e-6 {
        return;
    }
    let area_recip = 1.0 / area;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let px_f = px as f32 + 0.5;
            let py_f = py as f32 + 0.5;

            let w0 = edge_function(x1, y1, x2, y2, px_f, py_f);
            let w1 = edge_function(x2, y2, x0, y0, px_f, py_f);
            let w2 = edge_function(x0, y0, x1, y1, px_f, py_f);

            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };

            if !inside {
                continue;
            }

            let b0 = w0 * area_recip;
            let b1 = w1 * area_recip;
            let b2 = w2 * area_recip;

            let depth = b0 * z0 + b1 * z1 + b2 * z2;
            depth_buf.test_and_set(px as u32, py as u32, depth);
        }
    }
}

fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Converts a Transform3D to a column-major 4x4 matrix.
fn transform_to_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
    let b = &t.basis;
    [
        [b.x.x, b.x.y, b.x.z, 0.0],
        [b.y.x, b.y.y, b.y.z, 0.0],
        [b.z.x, b.z.y, b.z.z, 0.0],
        [t.origin.x, t.origin.y, t.origin.z, 1.0],
    ]
}

/// Multiplies a 4x4 column-major matrix by a 3D point (w=1).
fn mat4_mul_point(m: &[[f32; 4]; 4], p: Vector3) -> Vector3 {
    Vector3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

/// Multiplies a 4x4 column-major matrix by a 3D point, returning [x, y, z, w].
fn mat4_mul_point4(m: &[[f32; 4]; 4], p: Vector3) -> [f32; 4] {
    [
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
        m[0][3] * p.x + m[1][3] * p.y + m[2][3] * p.z + m[3][3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;
    use gdserver3d::mesh::Mesh3D;

    #[test]
    fn directional_light_view_matrix_is_valid() {
        let light = Light3D::directional(Light3DId(1));
        let view = directional_light_view_matrix(&light);
        // Should produce a non-degenerate matrix.
        let det = view[0][0] * (view[1][1] * view[2][2] - view[1][2] * view[2][1])
            - view[1][0] * (view[0][1] * view[2][2] - view[0][2] * view[2][1])
            + view[2][0] * (view[0][1] * view[1][2] - view[0][2] * view[1][1]);
        assert!(
            det.abs() > 0.01,
            "view matrix should be non-degenerate, det={det}"
        );
    }

    #[test]
    fn shadow_map_generation_empty_scene() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(maps.len(), 1);
        // All depths should be unwritten (MAX).
        let (w, h) = maps[0].depth.dimensions();
        assert_eq!(w, SHADOW_MAP_SIZE);
        assert_eq!(h, SHADOW_MAP_SIZE);
    }

    #[test]
    fn shadow_map_disabled_light_skipped() {
        let light = Light3D::directional(Light3DId(1));
        assert!(!light.shadow_enabled);
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(maps.len(), 0);
    }

    #[test]
    fn shadow_map_point_light_skipped() {
        let mut light = Light3D::point(Light3DId(1), Vector3::new(0.0, 5.0, 0.0));
        light.shadow_enabled = true;
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(
            maps.len(),
            0,
            "only directional lights get shadow maps for now"
        );
    }

    #[test]
    fn shadow_map_with_occluder_writes_depth() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        // Light pointing straight down.
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Place a cube at origin.
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let maps = generate_shadow_maps(&[light], &[inst]);
        assert_eq!(maps.len(), 1);

        // Some depth values should be written.
        let (w, h) = maps[0].depth.dimensions();
        let mut written = 0u32;
        for y in 0..h {
            for x in 0..w {
                if maps[0].depth.get(x, y) < f32::MAX {
                    written += 1;
                }
            }
        }
        assert!(
            written > 0,
            "occluder cube should write depth values, got 0"
        );
    }

    #[test]
    fn shadow_sample_behind_occluder_is_shadowed() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Occluder cube at y=5 (above the test point).
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(4.0));
        inst.visible = true;
        inst.transform = Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::new(0.0, 5.0, 0.0),
        };

        let maps = generate_shadow_maps(&[light], &[inst]);
        assert_eq!(maps.len(), 1);

        // A point below the cube should be in shadow.
        let shadow = maps[0].sample(Vector3::new(0.0, 0.0, 0.0));
        assert!(
            shadow > 0.5,
            "point below occluder should be shadowed, got factor={shadow}"
        );
    }

    #[test]
    fn shadow_sample_beside_occluder_is_lit() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Small occluder cube at origin.
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let maps = generate_shadow_maps(&[light], &[inst]);

        // A point far to the side should be lit.
        let shadow = maps[0].sample(Vector3::new(10.0, 0.0, 10.0));
        assert!(
            shadow < 0.5,
            "point beside occluder should be lit, got factor={shadow}"
        );
    }
}
