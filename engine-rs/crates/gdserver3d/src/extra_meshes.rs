//! Additional Godot `PrimitiveMesh` subtypes — `PrismMesh` and `PointMesh`
//! (pat-nzgdu.131).
//!
//! These follow the same shape as the generators in [`primitive_mesh`], each a
//! small resource struct with a `generate(&self) -> Mesh3D`. `PrismMesh` builds
//! a triangular-prism solid (a box whose top face collapses to a ridge line);
//! `PointMesh` is Godot's single-point mesh used to render one billboarded
//! point sprite.
//!
//! Face normals are derived geometrically and oriented outward from the mesh
//! centroid, so the generated solids are consistently wound regardless of the
//! ridge skew.
//!
//! # Scope / remaining work (pat-nzgdu.131)
//!
//! The bead also asks for `TorusMesh` (its geometry already exists as
//! [`Mesh3D::torus`](crate::mesh::Mesh3D::torus)), `TextMesh`, `RibbonTrailMesh`,
//! `TubeTrailMesh`, a `QuadMesh` orientation fix, and registration of these as
//! `PrimitiveMeshType` enum variants / ClassDB resources. This module delivers
//! only the Prism and Point geometry generators; the enum + ClassDB wiring and
//! the remaining subtypes are not implemented here.
//!
//! [`primitive_mesh`]: crate::primitive_mesh

use gdcore::math::Vector3;

use crate::mesh::{Mesh3D, PrimitiveType};

/// A Godot-compatible `PrismMesh`: a triangular prism whose top edge is a ridge
/// running along Z, positioned across X by `left_to_right`.
#[derive(Debug, Clone, PartialEq)]
pub struct PrismMesh {
    /// Bounding size (width X, height Y, depth Z).
    pub size: Vector3,
    /// Ridge apex position across the width, `0.0` (left) .. `1.0` (right).
    pub left_to_right: f32,
}

impl Default for PrismMesh {
    fn default() -> Self {
        Self {
            size: Vector3::new(1.0, 1.0, 1.0),
            left_to_right: 0.5,
        }
    }
}

impl PrismMesh {
    /// Generates the prism geometry: a bottom quad, two slanted side quads
    /// rising to the ridge, and two triangular end caps.
    pub fn generate(&self) -> Mesh3D {
        let hw = self.size.x * 0.5;
        let hh = self.size.y * 0.5;
        let hd = self.size.z * 0.5;
        let apex_x = -hw + self.left_to_right.clamp(0.0, 1.0) * self.size.x;

        // Bottom rectangle corners (y = -hh); f/b denote +Z (front) / -Z (back).
        let bfl = Vector3::new(-hw, -hh, hd);
        let bfr = Vector3::new(hw, -hh, hd);
        let bbr = Vector3::new(hw, -hh, -hd);
        let bbl = Vector3::new(-hw, -hh, -hd);
        // Top ridge endpoints (y = +hh).
        let tf = Vector3::new(apex_x, hh, hd);
        let tb = Vector3::new(apex_x, hh, -hd);

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        // Bottom, then the two slanted faces, then the end caps.
        add_quad(
            &mut vertices,
            &mut normals,
            &mut uvs,
            &mut indices,
            bbl,
            bbr,
            bfr,
            bfl,
        );
        add_quad(
            &mut vertices,
            &mut normals,
            &mut uvs,
            &mut indices,
            bbl,
            bfl,
            tf,
            tb,
        );
        add_quad(
            &mut vertices,
            &mut normals,
            &mut uvs,
            &mut indices,
            bfr,
            bbr,
            tb,
            tf,
        );
        add_tri(
            &mut vertices,
            &mut normals,
            &mut uvs,
            &mut indices,
            bfl,
            bfr,
            tf,
        );
        add_tri(
            &mut vertices,
            &mut normals,
            &mut uvs,
            &mut indices,
            bbr,
            bbl,
            tb,
        );

        Mesh3D {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            blend_shapes: Vec::new(),
        }
    }
}

/// A Godot-compatible `PointMesh`: a single vertex rendered as a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PointMesh;

impl PointMesh {
    /// Generates a one-vertex `Points`-primitive mesh at the origin.
    pub fn generate(&self) -> Mesh3D {
        Mesh3D {
            vertices: vec![Vector3::new(0.0, 0.0, 0.0)],
            normals: vec![Vector3::new(0.0, 0.0, 1.0)],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
            primitive_type: PrimitiveType::Points,
            surfaces: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            blend_shapes: Vec::new(),
        }
    }
}

/// The outward unit normal of a face, given three of its corners and the face
/// centroid. Returns the normal and whether the assumed winding was flipped to
/// face outward (away from the mesh origin).
fn outward_normal(a: Vector3, b: Vector3, c: Vector3, centroid: Vector3) -> (Vector3, bool) {
    let raw = (b - a).cross(c - a);
    let n = if raw.length_squared() > 1e-12 {
        raw.normalized()
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    if n.dot(centroid) >= 0.0 {
        (n, false)
    } else {
        (n * -1.0, true)
    }
}

#[allow(clippy::too_many_arguments)]
fn add_quad(
    vertices: &mut Vec<Vector3>,
    normals: &mut Vec<Vector3>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    a: Vector3,
    b: Vector3,
    c: Vector3,
    d: Vector3,
) {
    let centroid = (a + b + c + d) * 0.25;
    let (normal, flip) = outward_normal(a, b, c, centroid);
    let base = vertices.len() as u32;

    vertices.push(a);
    vertices.push(b);
    vertices.push(c);
    vertices.push(d);
    for _ in 0..4 {
        normals.push(normal);
    }
    uvs.push([0.0, 0.0]);
    uvs.push([1.0, 0.0]);
    uvs.push([1.0, 1.0]);
    uvs.push([0.0, 1.0]);

    if flip {
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    } else {
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn add_tri(
    vertices: &mut Vec<Vector3>,
    normals: &mut Vec<Vector3>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    a: Vector3,
    b: Vector3,
    c: Vector3,
) {
    let centroid = (a + b + c) * (1.0 / 3.0);
    let (normal, flip) = outward_normal(a, b, c, centroid);
    let base = vertices.len() as u32;

    vertices.push(a);
    vertices.push(b);
    vertices.push(c);
    for _ in 0..3 {
        normals.push(normal);
    }
    uvs.push([0.0, 0.0]);
    uvs.push([1.0, 0.0]);
    uvs.push([0.5, 1.0]);

    if flip {
        indices.extend_from_slice(&[base, base + 2, base + 1]);
    } else {
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn prism_has_expected_geometry_counts() {
        let mesh = PrismMesh::default().generate();
        // 4 (bottom) + 4 + 4 (slants) + 3 + 3 (caps) = 18 vertices.
        assert_eq!(mesh.vertices.len(), 18);
        assert_eq!(mesh.normals.len(), 18);
        assert_eq!(mesh.uvs.len(), 18);
        // 2 + 2 + 2 + 1 + 1 = 8 triangles.
        assert_eq!(mesh.indices.len(), 8 * 3);
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn prism_indices_are_in_range() {
        let mesh = PrismMesh::default().generate();
        let n = mesh.vertices.len() as u32;
        assert!(mesh.indices.iter().all(|&i| i < n));
    }

    #[test]
    fn prism_normals_are_unit_length() {
        let mesh = PrismMesh {
            size: Vector3::new(2.0, 3.0, 1.5),
            left_to_right: 0.3,
        }
        .generate();
        for n in &mesh.normals {
            assert!(approx(n.length(), 1.0), "normal not unit: {n:?}");
        }
    }

    #[test]
    fn prism_face_normals_point_outward() {
        // For this convex solid centered at the origin, every triangle's normal
        // must point away from the centroid (outward).
        let mesh = PrismMesh {
            size: Vector3::new(2.0, 2.0, 2.0),
            left_to_right: 0.7,
        }
        .generate();
        for tri in mesh.indices.chunks(3) {
            let a = mesh.vertices[tri[0] as usize];
            let b = mesh.vertices[tri[1] as usize];
            let c = mesh.vertices[tri[2] as usize];
            let face_center = (a + b + c) * (1.0 / 3.0);
            let geo_normal = (b - a).cross(c - a).normalized();
            assert!(
                geo_normal.dot(face_center) > 0.0,
                "winding gives inward normal for {a:?},{b:?},{c:?}"
            );
        }
    }

    #[test]
    fn prism_vertices_fit_the_bounding_box() {
        let mesh = PrismMesh {
            size: Vector3::new(4.0, 6.0, 2.0),
            left_to_right: 0.25,
        }
        .generate();
        for v in &mesh.vertices {
            assert!(v.x >= -2.0 - 1e-5 && v.x <= 2.0 + 1e-5, "x {}", v.x);
            assert!(v.y >= -3.0 - 1e-5 && v.y <= 3.0 + 1e-5, "y {}", v.y);
            assert!(v.z >= -1.0 - 1e-5 && v.z <= 1.0 + 1e-5, "z {}", v.z);
        }
    }

    #[test]
    fn prism_ridge_apex_follows_left_to_right() {
        // left_to_right = 0 puts the ridge at the left edge (x = -hw).
        let mesh = PrismMesh {
            size: Vector3::new(2.0, 2.0, 2.0),
            left_to_right: 0.0,
        }
        .generate();
        let top: Vec<&Vector3> = mesh.vertices.iter().filter(|v| approx(v.y, 1.0)).collect();
        assert!(!top.is_empty(), "expected top-ridge vertices");
        assert!(
            top.iter().all(|v| approx(v.x, -1.0)),
            "ridge should sit at the left edge: {top:?}"
        );

        // left_to_right = 1 puts it at the right edge (x = +hw).
        let mesh_r = PrismMesh {
            size: Vector3::new(2.0, 2.0, 2.0),
            left_to_right: 1.0,
        }
        .generate();
        let top_r: Vec<&Vector3> = mesh_r
            .vertices
            .iter()
            .filter(|v| approx(v.y, 1.0))
            .collect();
        assert!(
            top_r.iter().all(|v| approx(v.x, 1.0)),
            "ridge at right: {top_r:?}"
        );
    }

    #[test]
    fn point_mesh_is_a_single_point() {
        let mesh = PointMesh.generate();
        assert_eq!(mesh.vertices.len(), 1);
        assert_eq!(mesh.indices, vec![0]);
        assert_eq!(mesh.primitive_type, PrimitiveType::Points);
        assert!(approx(mesh.vertices[0].length(), 0.0));
    }
}
