//! Shadow map generation for directional and spot lights.
//!
//! Directional lights use an orthographic light-space projection; spot lights
//! use a perspective projection matching the cone FOV. Depth is stored in a
//! [`DepthBuffer`] and sampled during the main pass.

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

        // Map NDC [-1,1] to texel coords [0, size]. `floor` (not `as i32`
        // truncation) so texel-space u ∈ (−1, 0) rejects as out-of-frustum
        // instead of aliasing onto column 0 — matching the GPU sampler.
        let u = (ndc_x + 1.0) * 0.5 * self.size as f32;
        let v = (1.0 - ndc_y) * 0.5 * self.size as f32;

        let tx = u.floor() as i32;
        let ty = v.floor() as i32;

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

/// Generates shadow maps for all directional **and spot** lights with
/// `shadow_enabled = true` (point lights use cubemaps separately).
pub fn generate_shadow_maps(lights: &[Light3D], instances: &[Instance3D]) -> Vec<ShadowMap> {
    lights
        .iter()
        .filter(|l| {
            l.shadow_enabled
                && matches!(l.light_type, LightType::Directional | LightType::Spot)
        })
        .map(|l| match l.light_type {
            LightType::Spot => generate_spot_shadow_map(l, instances),
            _ => generate_directional_shadow_map(l, instances),
        })
        .collect()
}

/// Builds a view matrix looking from the spot light position along its direction.
pub fn spot_light_view_matrix(light: &Light3D) -> [[f32; 4]; 4] {
    let dir = light.direction.normalized();
    let up_candidate = if dir.y.abs() > 0.99 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let right = up_candidate.cross(dir).normalized();
    let up = dir.cross(right).normalized();
    let light_transform = Transform3D {
        basis: Basis {
            x: right,
            y: up,
            z: dir * -1.0,
        },
        origin: light.position,
    };
    transform_to_matrix(&light_transform.inverse())
}

/// Perspective projection for a spot light (FOV = 2 * spot_angle).
pub fn spot_light_proj_matrix(light: &Light3D) -> [[f32; 4]; 4] {
    // Clamp below π: at Godot's max spot_angle (90°) the full fov reaches π,
    // tan(fov/2) → ∞ and f → 0, collapsing every fragment to the center texel.
    let fov = (light.spot_angle * 2.0).clamp(0.05, std::f32::consts::PI - 0.01);
    let near = SHADOW_NEAR;
    let far = if light.range > 0.0 {
        light.range.max(near + 0.1)
    } else {
        SHADOW_FAR
    };
    // Column-major perspective (same convention as perspective_90_projection).
    let f = 1.0 / (fov * 0.5).tan();
    [
        [f, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, -(far + near) / (far - near), -1.0],
        [0.0, 0.0, -2.0 * far * near / (far - near), 0.0],
    ]
}

/// Generates a perspective shadow map for a spot light.
fn generate_spot_shadow_map(light: &Light3D, instances: &[Instance3D]) -> ShadowMap {
    let size = SHADOW_MAP_SIZE;
    let mut depth_buf = DepthBuffer::new(size, size);
    let view_matrix = spot_light_view_matrix(light);
    let proj_matrix = spot_light_proj_matrix(light);
    rasterize_instances_into_shadow(instances, &view_matrix, &proj_matrix, &mut depth_buf, size);
    ShadowMap {
        light_id: light.id,
        depth: depth_buf,
        view_matrix,
        proj_matrix,
        size,
    }
}

/// Rasterizes all visible mesh instances into a shadow depth buffer.
fn rasterize_instances_into_shadow(
    instances: &[Instance3D],
    view_matrix: &[[f32; 4]; 4],
    proj_matrix: &[[f32; 4]; 4],
    depth_buf: &mut DepthBuffer,
    size: u32,
) {
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
            let mut screen_pts: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
            let mut valid = true;
            for (j, &vi) in tri.iter().enumerate() {
                if vi >= verts.len() {
                    valid = false;
                    break;
                }
                let world = mat4_mul_point(&model_matrix, verts[vi]);
                let view = mat4_mul_point(view_matrix, world);
                let clip = mat4_mul_point4(proj_matrix, view);
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
            rasterize_shadow_triangle(&screen_pts, depth_buf, size, size);
        }
    }
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
                    // Project with the SAME face table `ShadowCubemap`'s
                    // sampler uses (`direction_to_face_uv`), so generator and
                    // sampler can never disagree on a texel — the previous
                    // view/projection matrices used ad-hoc face bases that
                    // mirrored some faces and silently voided off-center
                    // shadows. `ma` is the direction's dominant-axis extent
                    // toward this face; behind or beyond range rejects.
                    let rel = world - light.position;
                    let (sc, tc) = face_sc_tc(face, rel);
                    let ma = rel.dot(face.forward());
                    let depth = rel.length();
                    if ma <= near || depth > far {
                        valid = false;
                        break;
                    }
                    let u = (sc / ma) * 0.5 + 0.5;
                    let v = (tc / ma) * 0.5 + 0.5;
                    let sx = u * size as f32;
                    let sy = v * size as f32;

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

/// The (sc, tc) face-plane coordinates for a direction on a given cube face —
/// the **same table** `ShadowCubemap::direction_to_face_uv` uses to sample, so
/// generation and sampling always agree (standard GL cubemap orientation).
fn face_sc_tc(face: CubeFace, dir: Vector3) -> (f32, f32) {
    match face {
        CubeFace::PositiveX => (-dir.z, -dir.y),
        CubeFace::NegativeX => (dir.z, -dir.y),
        CubeFace::PositiveY => (dir.x, dir.z),
        CubeFace::NegativeY => (dir.x, -dir.z),
        CubeFace::PositiveZ => (dir.x, -dir.y),
        CubeFace::NegativeZ => (-dir.x, -dir.y),
    }
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
    rasterize_instances_into_shadow(instances, &view_matrix, &proj_matrix, &mut depth_buf, size);
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

    /// The cubemap generator's per-face view/projection and the sampler's
    /// `direction_to_face_uv` must agree texel-for-texel, or stored depths
    /// land in the wrong texels and omni shadows silently vanish. Probe an
    /// occluder in several face directions and expect its distance back.
    #[test]
    fn omni_cubemap_generator_and_sampler_agree() {
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::OmniShadowMode;

        // An occluder in a given direction from the light; sampling that
        // direction must return roughly the occluder's distance.
        let cases = [
            Vector3::new(0.0, 0.0, -1.5),  // −Z face
            Vector3::new(0.0, 0.0, 1.5),   // +Z face
            Vector3::new(1.5, 0.0, 0.0),   // +X face
            Vector3::new(-1.5, 0.0, 0.0),  // −X face
            Vector3::new(0.0, 1.5, 0.0),   // +Y face
            Vector3::new(0.0, -1.5, 0.0),  // −Y face
            Vector3::new(-0.6, 0.2, -1.1), // off-axis (−Z dominant)
        ];
        for rel in cases {
            let light_pos = Vector3::new(1.2, 0.0, 2.2);
            let mut light = Light3D::point(Light3DId(1), light_pos);
            light.range = 20.0;
            light.shadow_enabled = true;
            light.omni_shadow_mode = OmniShadowMode::Cube;

            let mut inst = Instance3D::new(Instance3DId(1));
            inst.mesh = Some(Mesh3D::cube(0.6));
            inst.transform.origin = light_pos + rel;

            let maps = generate_omni_shadow_cubemaps(&[light], &[inst]);
            let cm = maps[0].as_ref().expect("cube-mode light must get a map");
            let stored = cm.sample(rel);
            let dist = rel.length();
            assert!(
                stored < dist + 0.35 && stored > dist - 0.65,
                "sampling toward the occluder at {rel:?} (dist {dist:.2}) must \
                 return its recorded depth, got {stored}"
            );
        }
    }

    /// At Godot's maximum spot half-angle (90°) the full fov reaches π, where
    /// an unclamped `tan(fov/2)` explodes and the focal factor collapses to 0
    /// (every fragment mapping to the center texel). The clamp keeps the
    /// projection non-degenerate.
    #[test]
    fn spot_proj_matrix_stays_finite_at_wide_cone_angles() {
        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.spot_angle = std::f32::consts::FRAC_PI_2; // 90° half-angle
        let proj = spot_light_proj_matrix(&light);
        let f = proj[0][0];
        // The fov clamp (π − 0.01) puts f at ~tan(0.005) ≈ 0.005 — assert it's
        // finite and strictly positive (unclamped it would be ~0 / NaN).
        assert!(
            f.is_finite() && f > 1e-4,
            "wide-cone spot projection must stay non-degenerate, f={f}"
        );
    }

    /// Texel-space coordinates just outside the frustum's left/top edge
    /// (u ∈ (−1, 0)) must reject as out-of-map (lit), not truncate onto
    /// column 0 — the GPU sampler rejects them, and the two must agree.
    #[test]
    fn sample_rejects_negative_texel_band_as_lit() {
        // An empty map (no occluders): every in-bounds texel returns 0.0
        // ("no occluder"), so any 0.0 here proves we didn't panic and the
        // boundary behavior is exercised via a map with a full-depth column.
        let mut map = ShadowMap {
            light_id: Light3DId(1),
            size: 4,
            depth: crate::depth_buffer::DepthBuffer::new(4, 4),
            view_matrix: identity(),
            proj_matrix: identity(),
        };
        // Occluder depth 0 across the whole map: any sampled texel shadows a
        // fragment at depth > bias.
        map.depth.clear(0.0);
        // NDC x = −1.001 → texel u ∈ (−0.01, 0): floor → −1 → out of map → lit.
        let just_outside = Vector3::new(-1.001, 0.0, 0.5);
        assert_eq!(
            map.sample(just_outside),
            0.0,
            "fragment just outside the frustum edge must be lit"
        );
        // NDC x = −0.999 → texel 0 → shadowed (occluder at depth 0).
        let just_inside = Vector3::new(-0.999, 0.0, 0.5);
        assert_eq!(
            map.sample(just_inside),
            1.0,
            "fragment just inside the frustum edge must sample the map"
        );
    }

    fn identity() -> [[f32; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
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
            "point lights use cubemaps, not 2D shadow maps"
        );
    }

    #[test]
    fn spot_shadow_map_generated_for_enabled_spot() {
        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.shadow_enabled = true;
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(maps.len(), 1, "spot lights with shadows get a depth map");
    }

    #[test]
    fn spot_shadow_sample_behind_occluder() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 8.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.shadow_enabled = true;
        light.spot_angle = std::f32::consts::FRAC_PI_3; // wide cone
        light.range = 20.0;

        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(3.0));
        inst.visible = true;
        inst.transform = Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::new(0.0, 4.0, 0.0),
        };

        let maps = generate_shadow_maps(&[light], &[inst]);
        assert_eq!(maps.len(), 1);
        // Point under the occluder along the spot axis.
        let shadow = maps[0].sample(Vector3::new(0.0, 0.0, 0.0));
        assert!(
            shadow > 0.5,
            "spot: point below occluder should be shadowed, got {shadow}"
        );
        // Far off-axis / outside frustum → lit (outside map).
        let lit = maps[0].sample(Vector3::new(50.0, 0.0, 50.0));
        assert!(
            lit < 0.5,
            "spot: point outside frustum should be lit, got {lit}"
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


/// Directional light cascade split (pat-p0s82.8).
#[derive(Debug, Clone)]
pub struct DirectionalCascade {
    /// Near plane distance of this cascade.
    pub near: f32,
    /// Far plane distance of this cascade.
    pub far: f32,
    /// Shadow map for this cascade.
    pub map: ShadowMap,
}

/// Compute cascade split distances (PSSM-style, log/linear blended at 0.5).
pub fn cascade_split_distances(near: f32, far: f32, count: usize) -> Vec<f32> {
    cascade_split_distances_lambda(near, far, count, 0.5)
}

/// Compute cascade split distances with an explicit log/linear blend weight.
///
/// Mirrors Godot's practical split scheme: `lambda = 0.0` gives uniform
/// (linear) splits, `lambda = 1.0` gives fully logarithmic splits, and values
/// in between blend the two (Godot sits near 0.5). `count` maps to Godot's
/// directional shadow mode (2 or 4 splits; 1 and 3 are also accepted).
///
/// Returns `count + 1` strictly-increasing boundaries `[near, .., far]`.
pub fn cascade_split_distances_lambda(near: f32, far: f32, count: usize, lambda: f32) -> Vec<f32> {
    let count = count.max(1);
    let lambda = lambda.clamp(0.0, 1.0);
    let mut splits = Vec::with_capacity(count + 1);
    splits.push(near);
    for i in 1..count {
        let id = i as f32 / count as f32;
        let log_s = near * (far / near).powf(id);
        let uni_s = near + (far - near) * id;
        splits.push(lambda * log_s + (1.0 - lambda) * uni_s);
    }
    splits.push(far);
    splits
}

/// Renders a directional light's shadow as `count` cascades (PSSM).
///
/// This is the directional shadow **cascade loop**: it slices the light-view
/// depth range `[SHADOW_NEAR, SHADOW_FAR]` into `count` successive cascades via
/// [`cascade_split_distances`] and renders one [`ShadowMap`] per cascade. Each
/// cascade `i` records the *selection* slice `[near, far] = [split[i],
/// split[i+1]]` (consumed by [`select_cascade`]), while its map is rendered
/// over the depth range `[SHADOW_NEAR, split[i+1]]` — so occluders *in front of*
/// the slice still cast into it — and over a tightened orthographic extent
/// proportional to the cascade's far distance, so near cascades pack more
/// texels onto near geometry (the point of PSSM).
///
/// `count` is clamped to `1..=4` (Godot supports 1/2/4 directional splits).
pub fn generate_directional_cascades(
    light: &Light3D,
    instances: &[Instance3D],
    count: usize,
) -> Vec<DirectionalCascade> {
    let count = count.clamp(1, 4);
    let splits = cascade_split_distances(SHADOW_NEAR, SHADOW_FAR, count);
    let view_matrix = directional_light_view_matrix(light);
    let size = SHADOW_MAP_SIZE;

    (0..count)
        .map(|i| {
            let near = splits[i];
            let far = splits[i + 1];
            // Tighten the ortho extent for nearer cascades (higher effective
            // resolution up close); floor it so it never collapses.
            let extent = (SHADOW_EXTENT * far / SHADOW_FAR).max(2.0);
            // Render depth from the light (SHADOW_NEAR) to this cascade's far so
            // occluders ahead of the slice are captured.
            let proj_matrix =
                orthographic_projection_matrix(-extent, extent, -extent, extent, SHADOW_NEAR, far);
            let mut depth = DepthBuffer::new(size, size);
            rasterize_instances_into_shadow(instances, &view_matrix, &proj_matrix, &mut depth, size);
            DirectionalCascade {
                near,
                far,
                map: ShadowMap {
                    light_id: light.id,
                    depth,
                    view_matrix,
                    proj_matrix,
                    size,
                },
            }
        })
        .collect()
}

/// Light-view depth of a world point along a directional light's forward axis
/// (its distance from the light), used to pick a cascade for the fragment.
fn light_view_depth(view_matrix: &[[f32; 4]; 4], world_pos: Vector3) -> f32 {
    // The light looks along its local -Z; a point in front has negative view z.
    -mat4_mul_point(view_matrix, world_pos).z
}

/// Selects the cascade whose depth slice contains `depth`: the first cascade
/// whose `far` is at or beyond `depth`. Clamps to the last cascade for depths
/// past the final split (and to the first for depths before the first).
pub fn select_cascade(cascades: &[DirectionalCascade], depth: f32) -> usize {
    for (i, c) in cascades.iter().enumerate() {
        if depth <= c.far {
            return i;
        }
    }
    cascades.len().saturating_sub(1)
}

/// Index of the cascade a world-space fragment falls into (its light-view depth
/// routed through [`select_cascade`]).
pub fn cascade_index_for(cascades: &[DirectionalCascade], world_pos: Vector3) -> usize {
    if cascades.is_empty() {
        return 0;
    }
    let depth = light_view_depth(&cascades[0].map.view_matrix, world_pos);
    select_cascade(cascades, depth)
}

/// Samples cascaded directional shadows for a world-space fragment: picks the
/// cascade covering the fragment's light-view depth (per-cascade split
/// selection), then samples that cascade's shadow map. Returns a shadow factor
/// (`0.0` lit .. `1.0` shadowed); `0.0` for an empty cascade set.
pub fn sample_cascaded_shadow(cascades: &[DirectionalCascade], world_pos: Vector3) -> f32 {
    if cascades.is_empty() {
        return 0.0;
    }
    cascades[cascade_index_for(cascades, world_pos)].map.sample(world_pos)
}

#[cfg(test)]
mod cascade_tests {
    use super::*;

    #[test]
    fn cascade_splits_monotonic() {
        let s = cascade_split_distances(0.1, 100.0, 4);
        assert_eq!(s.len(), 5);
        for w in s.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[test]
    fn cascade_split_lambda_extremes() {
        // lambda = 0 → uniform (linear) interior splits: 1, 25.75, 50.5, 75.25.
        let uni = cascade_split_distances_lambda(1.0, 100.0, 4, 0.0);
        assert!((uni[1] - 25.75).abs() < 0.01, "uniform split1 {}", uni[1]);
        assert!((uni[2] - 50.5).abs() < 0.01, "uniform split2 {}", uni[2]);
        // lambda = 1 → logarithmic interior splits: 1, 3.162, 10, 31.62.
        let log = cascade_split_distances_lambda(1.0, 100.0, 4, 1.0);
        assert!((log[1] - 3.1623).abs() < 0.01, "log split1 {}", log[1]);
        assert!((log[2] - 10.0).abs() < 0.02, "log split2 {}", log[2]);
        // Logarithmic packs more resolution near the camera than uniform.
        assert!(log[1] < uni[1], "log should be tighter near the camera");
    }

    #[test]
    fn cascade_count_maps_to_mode() {
        let light = Light3D::directional(Light3DId(1));
        for count in [2usize, 3, 4] {
            let cascades = generate_directional_cascades(&light, &[], count);
            assert_eq!(cascades.len(), count, "count {count}");
            // Slices span [SHADOW_NEAR, SHADOW_FAR], contiguous and increasing.
            assert!((cascades[0].near - SHADOW_NEAR).abs() < 1e-3);
            assert!((cascades[count - 1].far - SHADOW_FAR).abs() < 1e-3);
            for w in cascades.windows(2) {
                assert!(w[1].near > w[0].near, "cascade nears must increase");
                assert!(
                    (w[0].far - w[1].near).abs() < 1e-4,
                    "cascade slices must be contiguous"
                );
            }
        }
        // Godot caps directional splits at 4.
        assert_eq!(generate_directional_cascades(&light, &[], 9).len(), 4);
    }

    #[test]
    fn select_cascade_routes_by_depth() {
        let light = Light3D::directional(Light3DId(1));
        let cascades = generate_directional_cascades(&light, &[], 4);
        // Before the first near → 0; past the last far → the last cascade.
        assert_eq!(select_cascade(&cascades, -5.0), 0);
        assert_eq!(select_cascade(&cascades, 1000.0), 3);
        // A depth inside a cascade's own slice selects that cascade.
        let mid0 = 0.5 * (cascades[0].near + cascades[0].far);
        assert_eq!(select_cascade(&cascades, mid0), 0);
        let mid_last = 0.5 * (cascades[3].near + cascades[3].far);
        assert_eq!(select_cascade(&cascades, mid_last), 3);
    }

    /// The core parity goal: with a single shadow map both a near caster and a
    /// far caster cannot be resolved well; cascades give each its own slice.
    /// Here a high occluder (small light-view depth) and a low occluder (large
    /// depth) each shadow a receiver beneath them, routed through *different*
    /// cascades.
    #[test]
    fn near_and_far_casters_both_shadowed() {
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::mesh::Mesh3D;

        // Light straight down: it sits at y≈+50 looking along -Y, so a world
        // point's light-view depth ≈ 50 − y. A high occluder is "near" (small
        // depth); a low occluder is "far" (large depth).
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        let cube = |id: u64, y: f32| {
            let mut inst = Instance3D::new(Instance3DId(id));
            inst.mesh = Some(Mesh3D::cube(2.0));
            inst.visible = true;
            inst.transform = Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, y, 0.0),
            };
            inst
        };

        // Near occluder high up (depth ≈ 3), far occluder low down (depth ≈ 88).
        let occluders = [cube(1, 47.0), cube(2, -38.0)];
        let cascades = generate_directional_cascades(&light, &occluders, 4);
        assert_eq!(cascades.len(), 4);

        // Receivers just beneath each occluder along the same light ray.
        let near_recv = Vector3::new(0.0, 44.0, 0.0); // depth ≈ 6
        let far_recv = Vector3::new(0.0, -41.0, 0.0); // depth ≈ 91

        // They must route to *different* cascades — proving cascades are
        // actually exercised, not collapsed onto one map.
        let near_idx = cascade_index_for(&cascades, near_recv);
        let far_idx = cascade_index_for(&cascades, far_recv);
        assert!(
            near_idx < far_idx,
            "near receiver (cascade {near_idx}) should use an earlier cascade \
             than the far receiver (cascade {far_idx})"
        );

        // Both receivers are shadowed by their respective casters.
        let near_shadow = sample_cascaded_shadow(&cascades, near_recv);
        let far_shadow = sample_cascaded_shadow(&cascades, far_recv);
        assert!(
            near_shadow > 0.5,
            "near caster must shadow its receiver, got {near_shadow}"
        );
        assert!(
            far_shadow > 0.5,
            "far caster must shadow its receiver, got {far_shadow}"
        );
    }
}
