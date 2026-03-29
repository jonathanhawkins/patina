//! 3D mesh data structures and primitive constructors.
//!
//! Provides `Mesh3D` for storing vertex data and `PrimitiveType` for
//! specifying how vertices are interpreted by the rendering pipeline.

use gdcore::math::Vector3;

/// How vertices are interpreted during rasterization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    /// Every three vertices form a triangle.
    Triangles,
    /// Every two vertices form a line segment.
    Lines,
    /// Each vertex is rendered as a point.
    Points,
}

/// A single surface within a 3D mesh.
///
/// In Godot, a mesh can contain multiple surfaces, each with its own
/// vertex data and default material. Surface indices are used by
/// `MeshInstance3D::set_surface_override_material()` for per-surface
/// material assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface3D {
    /// Vertex positions.
    pub vertices: Vec<Vector3>,
    /// Per-vertex normals.
    pub normals: Vec<Vector3>,
    /// Per-vertex UV coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle/line/point indices into the vertex arrays.
    pub indices: Vec<u32>,
    /// How to interpret the vertex data.
    pub primitive_type: PrimitiveType,
}

/// A 3D mesh containing vertex data, optionally split into multiple surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh3D {
    /// Vertex positions.
    pub vertices: Vec<Vector3>,
    /// Per-vertex normals.
    pub normals: Vec<Vector3>,
    /// Per-vertex UV coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle/line/point indices into the vertex arrays.
    pub indices: Vec<u32>,
    /// How to interpret the vertex data.
    pub primitive_type: PrimitiveType,
    /// Additional surfaces beyond the primary one.
    ///
    /// When empty, the mesh is treated as a single-surface mesh using the
    /// top-level vertex/normal/uv/index data. When populated, each entry
    /// represents a distinct surface that can receive its own material
    /// override in MeshInstance3D.
    pub surfaces: Vec<Surface3D>,
}

impl Mesh3D {
    /// Creates an empty mesh with the given primitive type.
    pub fn new(primitive_type: PrimitiveType) -> Self {
        Self {
            vertices: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            primitive_type,
            surfaces: Vec::new(),
        }
    }

    /// Returns the number of vertices in this mesh.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns the number of triangles (only valid for `Triangles` primitive type).
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Generates a unit cube centered at the origin, scaled by `size`.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;

        let mut vertices = Vec::with_capacity(24);
        let mut normals = Vec::with_capacity(24);
        let mut uvs = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);

        let faces: [(Vector3, Vector3, Vector3); 6] = [
            (
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            (
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            (
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            (
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            (
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            (
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
        ];

        for (normal, u_dir, v_dir) in &faces {
            let base = vertices.len() as u32;
            let center = *normal * h;
            let u = *u_dir * h;
            let v = *v_dir * h;

            vertices.push(center - u - v);
            vertices.push(center + u - v);
            vertices.push(center + u + v);
            vertices.push(center - u + v);

            for _ in 0..4 {
                normals.push(*normal);
            }

            uvs.push([0.0, 0.0]);
            uvs.push([1.0, 0.0]);
            uvs.push([1.0, 1.0]);
            uvs.push([0.0, 1.0]);

            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
            indices.push(base);
            indices.push(base + 2);
            indices.push(base + 3);
        }

        Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }

    /// Generates a UV sphere centered at the origin.
    pub fn sphere(radius: f32, segments: u32) -> Self {
        let rings = segments;
        let sectors = segments;

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        for r in 0..=rings {
            let phi = std::f32::consts::PI * r as f32 / rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();

            for s in 0..=sectors {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / sectors as f32;
                let (sin_theta, cos_theta) = theta.sin_cos();

                let x = cos_theta * sin_phi;
                let y = cos_phi;
                let z = sin_theta * sin_phi;

                let normal = Vector3::new(x, y, z);
                vertices.push(normal * radius);
                normals.push(normal);
                uvs.push([s as f32 / sectors as f32, r as f32 / rings as f32]);
            }
        }

        for r in 0..rings {
            for s in 0..sectors {
                let cur = r * (sectors + 1) + s;
                let next = cur + sectors + 1;

                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }

    /// Generates a flat plane on the XZ axis centered at the origin.
    pub fn plane(size: f32) -> Self {
        let h = size * 0.5;
        let normal = Vector3::UP;

        Self {
            vertices: vec![
                Vector3::new(-h, 0.0, -h),
                Vector3::new(h, 0.0, -h),
                Vector3::new(h, 0.0, h),
                Vector3::new(-h, 0.0, h),
            ],
            normals: vec![normal; 4],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }

    /// Generates a capsule (cylinder with hemisphere caps) along the Y axis.
    ///
    /// `radius` controls the cap/cylinder radius, `height` is the total height
    /// including caps. The body section has `radial_segments` around the
    /// circumference and `rings` subdivisions along the shaft.
    pub fn capsule(radius: f32, height: f32, radial_segments: u32, rings: u32) -> Self {
        let mid_height = (height - 2.0 * radius).max(0.0);
        let half_mid = mid_height * 0.5;
        let cap_rings = (rings / 2).max(2);

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        let total_rings = cap_rings * 2 + rings + 1;

        // Top hemisphere
        for r in 0..=cap_rings {
            let phi = std::f32::consts::FRAC_PI_2 * r as f32 / cap_rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let y = cos_phi * radius + half_mid;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                let nx = cos_t * sin_phi;
                let nz = sin_t * sin_phi;
                let ny = cos_phi;

                vertices.push(Vector3::new(nx * radius, y, nz * radius));
                normals.push(Vector3::new(nx, ny, nz));
                uvs.push([
                    s as f32 / radial_segments as f32,
                    r as f32 / total_rings as f32,
                ]);
            }
        }

        // Cylinder body
        for r in 1..=rings {
            let t = r as f32 / (rings + 1) as f32;
            let y = half_mid - mid_height * t;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(cos_t * radius, y, sin_t * radius));
                normals.push(Vector3::new(cos_t, 0.0, sin_t));
                uvs.push([
                    s as f32 / radial_segments as f32,
                    (cap_rings + r) as f32 / total_rings as f32,
                ]);
            }
        }

        // Bottom hemisphere
        for r in 0..=cap_rings {
            let phi = std::f32::consts::FRAC_PI_2
                + std::f32::consts::FRAC_PI_2 * r as f32 / cap_rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let y = cos_phi * radius - half_mid;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                let nx = cos_t * sin_phi;
                let nz = sin_t * sin_phi;
                let ny = cos_phi;

                vertices.push(Vector3::new(nx * radius, y, nz * radius));
                normals.push(Vector3::new(nx, ny, nz));
                uvs.push([
                    s as f32 / radial_segments as f32,
                    (cap_rings + rings + 1 + r) as f32 / total_rings as f32,
                ]);
            }
        }

        // Build triangle indices
        let row_len = radial_segments + 1;
        let total_rows = cap_rings + 1 + rings + cap_rings + 1;
        for r in 0..(total_rows - 1) {
            for s in 0..radial_segments {
                let cur = r * row_len + s;
                let next = cur + row_len;

                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }

    /// Generates a cylinder along the Y axis centered at the origin.
    ///
    /// `top_radius` and `bottom_radius` control the radii at each end (set
    /// equal for a regular cylinder, set one to zero for a cone).
    /// `height` is the full height. Caps are generated when the corresponding
    /// radius is > 0.
    pub fn cylinder(
        top_radius: f32,
        bottom_radius: f32,
        height: f32,
        radial_segments: u32,
        rings: u32,
    ) -> Self {
        let half_h = height * 0.5;

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        // Side surface
        let slope = (bottom_radius - top_radius) / height;
        let slope_len = (1.0 + slope * slope).sqrt();
        let ny = 1.0 / slope_len;
        let nr = if slope.abs() < 1e-8 {
            1.0
        } else {
            -slope / slope_len
        };

        for r in 0..=rings {
            let t = r as f32 / rings as f32;
            let y = half_h - height * t;
            let radius = top_radius + (bottom_radius - top_radius) * t;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(cos_t * radius, y, sin_t * radius));
                let n = Vector3::new(
                    cos_t * nr,
                    ny * slope.signum().max(0.0) + (1.0 - slope.abs().min(1.0)),
                    sin_t * nr,
                );
                // For straight cylinders, normal is purely radial.
                let normal = if slope.abs() < 1e-6 {
                    Vector3::new(cos_t, 0.0, sin_t)
                } else {
                    let _ = n;
                    // Cone normal: radial component + vertical tilt
                    let radial_n = Vector3::new(cos_t, 0.0, sin_t);
                    let tilt = (-slope / slope_len, 1.0 / slope_len);
                    Vector3::new(radial_n.x * tilt.1, tilt.0, radial_n.z * tilt.1)
                };
                normals.push(normal);
                uvs.push([s as f32 / radial_segments as f32, t]);
            }
        }

        // Side indices
        let row_len = radial_segments + 1;
        for r in 0..rings {
            for s in 0..radial_segments {
                let cur = r * row_len + s;
                let next = cur + row_len;

                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        // Top cap
        if top_radius > 0.0 {
            let center_idx = vertices.len() as u32;
            vertices.push(Vector3::new(0.0, half_h, 0.0));
            normals.push(Vector3::UP);
            uvs.push([0.5, 0.5]);

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(cos_t * top_radius, half_h, sin_t * top_radius));
                normals.push(Vector3::UP);
                uvs.push([cos_t * 0.5 + 0.5, sin_t * 0.5 + 0.5]);
            }

            for s in 0..radial_segments {
                indices.push(center_idx);
                indices.push(center_idx + 1 + s);
                indices.push(center_idx + 2 + s);
            }
        }

        // Bottom cap
        if bottom_radius > 0.0 {
            let center_idx = vertices.len() as u32;
            vertices.push(Vector3::new(0.0, -half_h, 0.0));
            normals.push(Vector3::new(0.0, -1.0, 0.0));
            uvs.push([0.5, 0.5]);

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(
                    cos_t * bottom_radius,
                    -half_h,
                    sin_t * bottom_radius,
                ));
                normals.push(Vector3::new(0.0, -1.0, 0.0));
                uvs.push([cos_t * 0.5 + 0.5, sin_t * 0.5 + 0.5]);
            }

            for s in 0..radial_segments {
                indices.push(center_idx);
                indices.push(center_idx + 2 + s);
                indices.push(center_idx + 1 + s);
            }
        }

        Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }

    /// Returns the number of surfaces in this mesh.
    ///
    /// A mesh with no explicit surfaces counts as 1 (the primary surface).
    pub fn surface_count(&self) -> usize {
        if self.surfaces.is_empty() {
            1
        } else {
            // Primary surface (top-level vertex data) + extra surfaces.
            1 + self.surfaces.len()
        }
    }

    /// Parses a Wavefront OBJ string into a `Mesh3D`.
    ///
    /// Supports `v` (vertex positions), `vn` (normals), `vt` (texture coords),
    /// and `f` (faces) directives. Faces may be triangles or quads (quads are
    /// automatically triangulated). Face indices may use any of the forms:
    /// `v`, `v/vt`, `v/vt/vn`, or `v//vn`.
    ///
    /// Returns an error string if parsing fails.
    pub fn from_obj(source: &str) -> Result<Self, String> {
        let mut positions: Vec<Vector3> = Vec::new();
        let mut tex_coords: Vec<[f32; 2]> = Vec::new();
        let mut obj_normals: Vec<Vector3> = Vec::new();

        // Unique (position, uv, normal) combinations → output vertex index.
        let mut vertex_map: std::collections::HashMap<(u32, u32, u32), u32> =
            std::collections::HashMap::new();

        let mut vertices: Vec<Vector3> = Vec::new();
        let mut normals: Vec<Vector3> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for (line_num, line) in source.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let directive = match parts.next() {
                Some(d) => d,
                None => continue,
            };

            match directive {
                "v" => {
                    let coords: Vec<f32> = parts
                        .map(|s| s.parse::<f32>())
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("line {}: bad vertex coord: {e}", line_num + 1))?;
                    if coords.len() < 3 {
                        return Err(format!(
                            "line {}: vertex needs at least 3 components, got {}",
                            line_num + 1,
                            coords.len()
                        ));
                    }
                    positions.push(Vector3::new(coords[0], coords[1], coords[2]));
                }
                "vn" => {
                    let coords: Vec<f32> = parts
                        .map(|s| s.parse::<f32>())
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("line {}: bad normal coord: {e}", line_num + 1))?;
                    if coords.len() < 3 {
                        return Err(format!(
                            "line {}: normal needs 3 components, got {}",
                            line_num + 1,
                            coords.len()
                        ));
                    }
                    obj_normals.push(Vector3::new(coords[0], coords[1], coords[2]));
                }
                "vt" => {
                    let coords: Vec<f32> = parts
                        .map(|s| s.parse::<f32>())
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("line {}: bad texcoord: {e}", line_num + 1))?;
                    if coords.is_empty() {
                        return Err(format!(
                            "line {}: texcoord needs at least 1 component",
                            line_num + 1
                        ));
                    }
                    let u = coords[0];
                    let v = if coords.len() > 1 { coords[1] } else { 0.0 };
                    tex_coords.push([u, v]);
                }
                "f" => {
                    let face_verts: Vec<&str> = parts.collect();
                    if face_verts.len() < 3 {
                        return Err(format!(
                            "line {}: face needs at least 3 vertices, got {}",
                            line_num + 1,
                            face_verts.len()
                        ));
                    }

                    // Parse each face vertex into an output index.
                    let mut face_indices: Vec<u32> = Vec::with_capacity(face_verts.len());
                    for fv in &face_verts {
                        let idx = Self::parse_obj_face_vertex(
                            fv,
                            line_num + 1,
                            &positions,
                            &tex_coords,
                            &obj_normals,
                            &mut vertex_map,
                            &mut vertices,
                            &mut uvs,
                            &mut normals,
                        )?;
                        face_indices.push(idx);
                    }

                    // Triangulate: fan from first vertex.
                    for i in 1..face_indices.len() - 1 {
                        indices.push(face_indices[0]);
                        indices.push(face_indices[i]);
                        indices.push(face_indices[i + 1]);
                    }
                }
                // Ignore unknown directives (mtllib, usemtl, g, o, s, etc.)
                _ => {}
            }
        }

        if positions.is_empty() {
            return Err("OBJ file contains no vertices".into());
        }

        // If no normals were provided, compute flat normals per triangle.
        let needs_normals = normals
            .iter()
            .all(|n| n.x == 0.0 && n.y == 0.0 && n.z == 0.0);
        if needs_normals && !indices.is_empty() {
            Self::compute_flat_normals(&vertices, &indices, &mut normals);
        }

        Ok(Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        })
    }

    /// Parses a single OBJ face vertex (e.g., "1/2/3", "1//3", "1/2", "1").
    ///
    /// Returns the output vertex index, creating a new vertex if this
    /// position/uv/normal combination hasn't been seen before.
    #[allow(clippy::too_many_arguments)]
    fn parse_obj_face_vertex(
        token: &str,
        line_num: usize,
        positions: &[Vector3],
        tex_coords: &[[f32; 2]],
        obj_normals: &[Vector3],
        vertex_map: &mut std::collections::HashMap<(u32, u32, u32), u32>,
        out_verts: &mut Vec<Vector3>,
        out_uvs: &mut Vec<[f32; 2]>,
        out_normals: &mut Vec<Vector3>,
    ) -> Result<u32, String> {
        let parts: Vec<&str> = token.split('/').collect();

        let vi = Self::parse_obj_index(parts[0], positions.len(), line_num, "vertex")?;

        let ti = if parts.len() > 1 && !parts[1].is_empty() {
            Self::parse_obj_index(parts[1], tex_coords.len(), line_num, "texcoord")?
        } else {
            0
        };

        let ni = if parts.len() > 2 && !parts[2].is_empty() {
            Self::parse_obj_index(parts[2], obj_normals.len(), line_num, "normal")?
        } else {
            0
        };

        let key = (vi as u32, ti as u32, ni as u32);
        if let Some(&existing) = vertex_map.get(&key) {
            return Ok(existing);
        }

        let new_idx = out_verts.len() as u32;
        out_verts.push(positions[vi]);
        out_uvs.push(if ti < tex_coords.len() {
            tex_coords[ti]
        } else {
            [0.0, 0.0]
        });
        out_normals.push(if ni < obj_normals.len() {
            obj_normals[ni]
        } else {
            Vector3::ZERO
        });
        vertex_map.insert(key, new_idx);
        Ok(new_idx)
    }

    /// Parses a 1-based (possibly negative) OBJ index into a 0-based index.
    fn parse_obj_index(
        s: &str,
        count: usize,
        line_num: usize,
        kind: &str,
    ) -> Result<usize, String> {
        let idx: i64 = s
            .parse()
            .map_err(|e| format!("line {line_num}: bad {kind} index '{s}': {e}"))?;
        let resolved = if idx > 0 {
            (idx - 1) as usize
        } else if idx < 0 {
            // Negative indices count backward from the end.
            let abs = (-idx) as usize;
            if abs > count {
                return Err(format!(
                    "line {line_num}: negative {kind} index {idx} out of range (count={count})"
                ));
            }
            count - abs
        } else {
            return Err(format!("line {line_num}: {kind} index must not be zero"));
        };
        if resolved >= count {
            return Err(format!(
                "line {line_num}: {kind} index {idx} out of range (count={count})"
            ));
        }
        Ok(resolved)
    }

    /// Computes flat (per-face) normals for triangles and overwrites the normal array.
    fn compute_flat_normals(vertices: &[Vector3], indices: &[u32], normals: &mut Vec<Vector3>) {
        // Accumulate face normals per vertex, then normalize.
        for n in normals.iter_mut() {
            *n = Vector3::ZERO;
        }
        let mut i = 0;
        while i + 2 < indices.len() {
            let i0 = indices[i] as usize;
            let i1 = indices[i + 1] as usize;
            let i2 = indices[i + 2] as usize;
            i += 3;

            if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
                continue;
            }

            let edge1 = vertices[i1] - vertices[i0];
            let edge2 = vertices[i2] - vertices[i0];
            let face_normal = edge1.cross(edge2);

            normals[i0] = normals[i0] + face_normal;
            normals[i1] = normals[i1] + face_normal;
            normals[i2] = normals[i2] + face_normal;
        }
        for n in normals.iter_mut() {
            let len = n.length();
            if len > 1e-8 {
                *n = *n * (1.0 / len);
            } else {
                *n = Vector3::UP;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_vertex_and_index_counts() {
        let mesh = Mesh3D::cube(1.0);
        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn cube_vertices_within_bounds() {
        let mesh = Mesh3D::cube(2.0);
        for v in &mesh.vertices {
            assert!(v.x.abs() <= 1.0 + 1e-6);
            assert!(v.y.abs() <= 1.0 + 1e-6);
            assert!(v.z.abs() <= 1.0 + 1e-6);
        }
    }

    #[test]
    fn cube_normals_unit_length() {
        let mesh = Mesh3D::cube(1.0);
        for n in &mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn cube_indices_in_range() {
        let mesh = Mesh3D::cube(1.0);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx);
        }
    }

    #[test]
    fn sphere_structure() {
        let mesh = Mesh3D::sphere(1.0, 8);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
    }

    #[test]
    fn sphere_vertices_at_radius() {
        let mesh = Mesh3D::sphere(3.0, 8);
        for v in &mesh.vertices {
            assert!((v.length() - 3.0).abs() < 1e-4);
        }
    }

    #[test]
    fn plane_counts() {
        let mesh = Mesh3D::plane(5.0);
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn plane_on_xz() {
        let mesh = Mesh3D::plane(4.0);
        for v in &mesh.vertices {
            assert!(v.y.abs() < 1e-6);
        }
    }

    #[test]
    fn capsule_structure() {
        let mesh = Mesh3D::capsule(0.5, 2.0, 12, 4);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn capsule_vertices_within_bounds() {
        let mesh = Mesh3D::capsule(0.5, 2.0, 8, 4);
        for v in &mesh.vertices {
            let radial = (v.x * v.x + v.z * v.z).sqrt();
            assert!(radial <= 0.5 + 1e-3, "radial {radial} exceeds radius");
            assert!(v.y.abs() <= 1.0 + 1e-3, "y {} exceeds half-height", v.y);
        }
    }

    #[test]
    fn capsule_indices_in_range() {
        let mesh = Mesh3D::capsule(0.3, 1.5, 8, 4);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx, "index {idx} >= vertex count {max_idx}");
        }
    }

    #[test]
    fn cylinder_structure() {
        let mesh = Mesh3D::cylinder(0.5, 0.5, 1.0, 12, 4);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
    }

    #[test]
    fn cylinder_has_caps() {
        let mesh = Mesh3D::cylinder(0.5, 0.5, 1.0, 8, 2);
        let has_up = mesh.normals.iter().any(|n| (n.y - 1.0).abs() < 1e-4);
        let has_down = mesh.normals.iter().any(|n| (n.y + 1.0).abs() < 1e-4);
        assert!(has_up, "missing top cap");
        assert!(has_down, "missing bottom cap");
    }

    #[test]
    fn cylinder_indices_in_range() {
        let mesh = Mesh3D::cylinder(0.3, 0.5, 2.0, 12, 3);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx, "index {idx} >= vertex count {max_idx}");
        }
    }

    #[test]
    fn cylinder_cone_no_top_cap() {
        let mesh = Mesh3D::cylinder(0.0, 0.5, 1.0, 8, 2);
        // No vertices at top cap center with UP normal
        let top_centers = mesh
            .vertices
            .iter()
            .zip(mesh.normals.iter())
            .filter(|(v, n)| {
                (v.y - 0.5).abs() < 1e-4
                    && (n.y - 1.0).abs() < 1e-4
                    && v.x.abs() < 1e-4
                    && v.z.abs() < 1e-4
            })
            .count();
        assert_eq!(top_centers, 0);
    }

    #[test]
    fn empty_mesh() {
        let mesh = Mesh3D::new(PrimitiveType::Lines);
        assert!(mesh.vertices.is_empty());
        assert_eq!(mesh.primitive_type, PrimitiveType::Lines);
    }

    #[test]
    fn single_surface_mesh_count() {
        let mesh = Mesh3D::cube(1.0);
        assert_eq!(mesh.surface_count(), 1);
        assert!(mesh.surfaces.is_empty());
    }

    #[test]
    fn multi_surface_mesh_count() {
        let mut mesh = Mesh3D::cube(1.0);
        mesh.surfaces.push(Surface3D {
            vertices: vec![Vector3::ZERO],
            normals: vec![Vector3::UP],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
            primitive_type: PrimitiveType::Points,
        });
        mesh.surfaces.push(Surface3D {
            vertices: vec![Vector3::new(1.0, 0.0, 0.0)],
            normals: vec![Vector3::UP],
            uvs: vec![[1.0, 0.0]],
            indices: vec![0],
            primitive_type: PrimitiveType::Points,
        });
        assert_eq!(mesh.surface_count(), 3);
    }

    // ── OBJ loader tests ──

    #[test]
    fn obj_simple_triangle() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn obj_with_normals_and_uvs() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
vt 0.0 0.0
vt 1.0 0.0
vt 0.0 1.0
f 1/1/1 2/2/1 3/3/1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.normals.len(), 3);
        assert_eq!(mesh.uvs.len(), 3);
        // All normals should be (0, 0, 1).
        for n in &mesh.normals {
            assert!((n.z - 1.0).abs() < 1e-5);
        }
        assert!((mesh.uvs[1][0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn obj_quad_is_triangulated() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 1.0 1.0 0.0
v 0.0 1.0 0.0
f 1 2 3 4
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        // Quad → 2 triangles → 6 indices.
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn obj_vertex_position_slash_normal() {
        let obj = "\
v 1.0 2.0 3.0
v 4.0 5.0 6.0
v 7.0 8.0 9.0
vn 0.0 1.0 0.0
f 1//1 2//1 3//1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        for n in &mesh.normals {
            assert!((n.y - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn obj_comments_and_blank_lines_ignored() {
        let obj = "\
# This is a comment
v 0.0 0.0 0.0

# Another comment
v 1.0 0.0 0.0
v 0.0 1.0 0.0

f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
    }

    #[test]
    fn obj_negative_indices() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f -3 -2 -1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    #[test]
    fn obj_multiple_faces() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 1.0 1.0 0.0
v 0.0 1.0 0.0
f 1 2 3
f 1 3 4
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn obj_shared_vertices_deduped() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 1.0 1.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
f 1//1 2//1 3//1
f 1//1 3//1 4//1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // Vertices 1//1 and 3//1 appear in both faces — should be deduped.
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn obj_different_normals_create_new_vertices() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
vn 0.0 0.0 -1.0
f 1//1 2//1 3//1
f 1//2 2//2 3//2
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // Same positions but different normals → 6 unique vertices.
        assert_eq!(mesh.vertex_count(), 6);
    }

    #[test]
    fn obj_auto_normals_when_none_provided() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // Should have auto-generated normals pointing in +Z.
        for n in &mesh.normals {
            let len = n.length();
            assert!((len - 1.0).abs() < 1e-4, "normal not unit length: {len}");
        }
    }

    #[test]
    fn obj_empty_fails() {
        let result = Mesh3D::from_obj("");
        assert!(result.is_err());
    }

    #[test]
    fn obj_bad_vertex_fails() {
        let obj = "v 1.0 abc 3.0\nf 1 1 1\n";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    #[test]
    fn obj_index_out_of_range_fails() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 4
";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    #[test]
    fn obj_zero_index_fails() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 0 1 2
";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    #[test]
    fn obj_unknown_directives_ignored() {
        let obj = "\
mtllib cube.mtl
o Cube
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
usemtl Material
s off
f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
    }

    #[test]
    fn obj_cube_roundtrip() {
        // A simple cube in OBJ format.
        let obj = "\
v -0.5 -0.5  0.5
v  0.5 -0.5  0.5
v  0.5  0.5  0.5
v -0.5  0.5  0.5
v -0.5 -0.5 -0.5
v  0.5 -0.5 -0.5
v  0.5  0.5 -0.5
v -0.5  0.5 -0.5
f 1 2 3 4
f 5 8 7 6
f 1 4 8 5
f 2 6 7 3
f 4 3 7 8
f 1 5 6 2
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // 6 quads → 12 triangles.
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.indices.len(), 36);
        // All indices should be valid.
        let max = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max);
        }
    }

    #[test]
    fn obj_face_with_only_two_vertices_fails() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
f 1 2
";
        assert!(Mesh3D::from_obj(obj).is_err());
    }
}
