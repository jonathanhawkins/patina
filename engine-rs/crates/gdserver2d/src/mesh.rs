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

/// A 3D mesh containing vertex data.
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
        }
    }

    /// Generates a unit cube centered at the origin, scaled by `size`.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;

        // 6 faces × 4 vertices = 24 vertices (unshared for correct normals).
        let mut vertices = Vec::with_capacity(24);
        let mut normals = Vec::with_capacity(24);
        let mut uvs = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);

        // (normal, tangent_u, tangent_v) for each face.
        let faces: [(Vector3, Vector3, Vector3); 6] = [
            // +X
            (
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            // -X
            (
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            // +Y
            (
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            // -Y
            (
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            // +Z
            (
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            // -Z
            (
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
        ];

        for (normal, u_dir, v_dir) in &faces {
            let base = vertices.len() as u32;
            let center = *normal * h;

            // Four corners: (-u -v), (+u -v), (+u +v), (-u +v).
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
        }
    }

    /// Generates a flat plane on the XZ axis centered at the origin.
    pub fn plane(size: f32) -> Self {
        let h = size * 0.5;
        let normal = Vector3::UP;

        let vertices = vec![
            Vector3::new(-h, 0.0, -h),
            Vector3::new(h, 0.0, -h),
            Vector3::new(h, 0.0, h),
            Vector3::new(-h, 0.0, h),
        ];
        let normals = vec![normal; 4];
        let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let indices = vec![0, 1, 2, 0, 2, 3];

        Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_correct_vertex_count() {
        let mesh = Mesh3D::cube(1.0);
        assert_eq!(mesh.vertices.len(), 24); // 6 faces × 4 verts
        assert_eq!(mesh.normals.len(), 24);
        assert_eq!(mesh.uvs.len(), 24);
        assert_eq!(mesh.indices.len(), 36); // 6 faces × 2 tris × 3
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
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
    fn cube_normals_are_unit_length() {
        let mesh = Mesh3D::cube(1.0);
        for n in &mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn sphere_has_expected_structure() {
        let mesh = Mesh3D::sphere(1.0, 8);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn sphere_vertices_at_radius() {
        let mesh = Mesh3D::sphere(3.0, 8);
        for v in &mesh.vertices {
            assert!((v.length() - 3.0).abs() < 1e-4);
        }
    }

    #[test]
    fn sphere_normals_are_unit_length() {
        let mesh = Mesh3D::sphere(2.0, 8);
        for n in &mesh.normals {
            // Poles may produce degenerate normals at exactly 0/pi, still unit.
            assert!((n.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn plane_has_correct_counts() {
        let mesh = Mesh3D::plane(5.0);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.normals.len(), 4);
        assert_eq!(mesh.uvs.len(), 4);
        assert_eq!(mesh.indices.len(), 6); // 2 triangles
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn plane_vertices_on_xz() {
        let mesh = Mesh3D::plane(4.0);
        for v in &mesh.vertices {
            assert!((v.y).abs() < 1e-6);
            assert!(v.x.abs() <= 2.0 + 1e-6);
            assert!(v.z.abs() <= 2.0 + 1e-6);
        }
    }

    #[test]
    fn plane_normals_point_up() {
        let mesh = Mesh3D::plane(1.0);
        for n in &mesh.normals {
            assert!((n.x).abs() < 1e-6);
            assert!((n.y - 1.0).abs() < 1e-6);
            assert!((n.z).abs() < 1e-6);
        }
    }

    #[test]
    fn empty_mesh_creation() {
        let mesh = Mesh3D::new(PrimitiveType::Lines);
        assert!(mesh.vertices.is_empty());
        assert!(mesh.indices.is_empty());
        assert_eq!(mesh.primitive_type, PrimitiveType::Lines);
    }

    #[test]
    fn cube_indices_in_range() {
        let mesh = Mesh3D::cube(1.0);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx);
        }
    }
}
