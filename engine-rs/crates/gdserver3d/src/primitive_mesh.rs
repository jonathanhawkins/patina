//! Godot-compatible PrimitiveMesh and ArrayMesh resource types.
//!
//! Each `PrimitiveMesh` subtype stores its generation parameters and lazily
//! produces a [`Mesh3D`] via [`PrimitiveMeshType::generate`].  `ArrayMesh`
//! wraps caller-supplied vertex arrays directly.

use gdcore::math::Vector3;

use crate::mesh::{Mesh3D, PrimitiveType};

/// A Godot-compatible `BoxMesh` with configurable size.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxMesh {
    /// Width (X), height (Y), depth (Z).
    pub size: Vector3,
}

impl Default for BoxMesh {
    fn default() -> Self {
        Self {
            size: Vector3::new(1.0, 1.0, 1.0),
        }
    }
}

impl BoxMesh {
    /// Generates the [`Mesh3D`] geometry for this box.
    pub fn generate(&self) -> Mesh3D {
        let hx = self.size.x * 0.5;
        let hy = self.size.y * 0.5;
        let hz = self.size.z * 0.5;

        let mut vertices = Vec::with_capacity(24);
        let mut normals = Vec::with_capacity(24);
        let mut uvs = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);

        // Face definitions: (normal, u_axis, v_axis, half-extents along u/v, offset along normal)
        let faces: [(Vector3, Vector3, Vector3, f32, f32, f32); 6] = [
            // +X
            (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, -1.0), Vector3::new(0.0, 1.0, 0.0), hz, hy, hx),
            // -X
            (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0), hz, hy, hx),
            // +Y
            (Vector3::new(0.0, 1.0, 0.0), Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), hx, hz, hy),
            // -Y
            (Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, -1.0), hx, hz, hy),
            // +Z
            (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0), hx, hy, hz),
            // -Z
            (Vector3::new(0.0, 0.0, -1.0), Vector3::new(-1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0), hx, hy, hz),
        ];

        for (normal, u_dir, v_dir, hu, hv, offset) in &faces {
            let base = vertices.len() as u32;
            let center = *normal * *offset;
            let u = *u_dir * *hu;
            let v = *v_dir * *hv;

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

        Mesh3D {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }
}

/// A Godot-compatible `SphereMesh` with configurable radius and detail.
#[derive(Debug, Clone, PartialEq)]
pub struct SphereMesh {
    /// Sphere radius.
    pub radius: f32,
    /// Height (diameter by default). Godot uses this for squashing.
    pub height: f32,
    /// Number of radial segments.
    pub radial_segments: u32,
    /// Number of ring segments.
    pub rings: u32,
}

impl Default for SphereMesh {
    fn default() -> Self {
        Self {
            radius: 0.5,
            height: 1.0,
            radial_segments: 64,
            rings: 32,
        }
    }
}

impl SphereMesh {
    /// Generates the [`Mesh3D`] geometry for this sphere.
    pub fn generate(&self) -> Mesh3D {
        let y_scale = self.height / (2.0 * self.radius);
        let mut base = Mesh3D::sphere(self.radius, self.rings.max(4));
        if (y_scale - 1.0).abs() > 1e-6 {
            for v in &mut base.vertices {
                v.y *= y_scale;
            }
            // Re-normalize after squash
            for n in &mut base.normals {
                n.y /= y_scale;
                let len = n.length();
                if len > 1e-8 {
                    n.x /= len;
                    n.y /= len;
                    n.z /= len;
                }
            }
        }
        base
    }
}

/// A Godot-compatible `CapsuleMesh`.
#[derive(Debug, Clone, PartialEq)]
pub struct CapsuleMesh {
    /// Capsule radius.
    pub radius: f32,
    /// Total height including caps.
    pub height: f32,
    /// Number of radial segments.
    pub radial_segments: u32,
    /// Number of ring segments along the shaft.
    pub rings: u32,
}

impl Default for CapsuleMesh {
    fn default() -> Self {
        Self {
            radius: 0.5,
            height: 2.0,
            radial_segments: 64,
            rings: 8,
        }
    }
}

impl CapsuleMesh {
    /// Generates the [`Mesh3D`] geometry for this capsule.
    pub fn generate(&self) -> Mesh3D {
        Mesh3D::capsule(self.radius, self.height, self.radial_segments.max(3), self.rings.max(1))
    }
}

/// A Godot-compatible `CylinderMesh`.
#[derive(Debug, Clone, PartialEq)]
pub struct CylinderMesh {
    /// Top radius.
    pub top_radius: f32,
    /// Bottom radius.
    pub bottom_radius: f32,
    /// Full height.
    pub height: f32,
    /// Number of radial segments.
    pub radial_segments: u32,
    /// Number of ring subdivisions along the height.
    pub rings: u32,
}

impl Default for CylinderMesh {
    fn default() -> Self {
        Self {
            top_radius: 0.5,
            bottom_radius: 0.5,
            height: 1.0,
            radial_segments: 64,
            rings: 4,
        }
    }
}

impl CylinderMesh {
    /// Generates the [`Mesh3D`] geometry for this cylinder.
    pub fn generate(&self) -> Mesh3D {
        Mesh3D::cylinder(
            self.top_radius,
            self.bottom_radius,
            self.height,
            self.radial_segments.max(3),
            self.rings.max(1),
        )
    }
}

/// A Godot-compatible `PlaneMesh` on the XZ plane.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneMesh {
    /// Size along X and Z axes.
    pub size: [f32; 2],
}

impl Default for PlaneMesh {
    fn default() -> Self {
        Self { size: [2.0, 2.0] }
    }
}

impl PlaneMesh {
    /// Generates the [`Mesh3D`] geometry for this plane.
    pub fn generate(&self) -> Mesh3D {
        let hx = self.size[0] * 0.5;
        let hz = self.size[1] * 0.5;
        let normal = Vector3::UP;

        Mesh3D {
            vertices: vec![
                Vector3::new(-hx, 0.0, -hz),
                Vector3::new(hx, 0.0, -hz),
                Vector3::new(hx, 0.0, hz),
                Vector3::new(-hx, 0.0, hz),
            ],
            normals: vec![normal; 4],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }
}

/// Enumeration of all built-in primitive mesh types.
///
/// Mirrors Godot's `PrimitiveMesh` hierarchy for dispatch in the render
/// adapter and resource loading.
#[derive(Debug, Clone, PartialEq)]
pub enum PrimitiveMeshType {
    /// `BoxMesh`
    Box(BoxMesh),
    /// `SphereMesh`
    Sphere(SphereMesh),
    /// `CapsuleMesh`
    Capsule(CapsuleMesh),
    /// `CylinderMesh`
    Cylinder(CylinderMesh),
    /// `PlaneMesh`
    Plane(PlaneMesh),
}

impl PrimitiveMeshType {
    /// Returns the Godot class name string for this primitive.
    pub fn class_name(&self) -> &'static str {
        match self {
            Self::Box(_) => "BoxMesh",
            Self::Sphere(_) => "SphereMesh",
            Self::Capsule(_) => "CapsuleMesh",
            Self::Cylinder(_) => "CylinderMesh",
            Self::Plane(_) => "PlaneMesh",
        }
    }

    /// Generates the concrete [`Mesh3D`] geometry.
    pub fn generate(&self) -> Mesh3D {
        match self {
            Self::Box(m) => m.generate(),
            Self::Sphere(m) => m.generate(),
            Self::Capsule(m) => m.generate(),
            Self::Cylinder(m) => m.generate(),
            Self::Plane(m) => m.generate(),
        }
    }
}

/// A Godot-compatible `ArrayMesh` constructed from raw vertex arrays.
///
/// Unlike `PrimitiveMesh` subtypes that generate geometry procedurally,
/// an `ArrayMesh` stores caller-provided vertex data directly.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayMesh {
    /// The underlying mesh data. Each call to `add_surface_from_arrays` appends
    /// to the surfaces list (or populates the primary surface for the first).
    mesh: Mesh3D,
}

impl Default for ArrayMesh {
    fn default() -> Self {
        Self::new()
    }
}

impl ArrayMesh {
    /// Creates an empty `ArrayMesh`.
    pub fn new() -> Self {
        Self {
            mesh: Mesh3D::new(PrimitiveType::Triangles),
        }
    }

    /// Adds a surface from raw vertex arrays.
    ///
    /// The first call populates the primary surface; subsequent calls add to
    /// the `surfaces` list.
    pub fn add_surface_from_arrays(
        &mut self,
        primitive_type: PrimitiveType,
        vertices: Vec<Vector3>,
        normals: Vec<Vector3>,
        uvs: Vec<[f32; 2]>,
        indices: Vec<u32>,
    ) {
        if self.mesh.vertices.is_empty() && self.mesh.surfaces.is_empty() {
            self.mesh.vertices = vertices;
            self.mesh.normals = normals;
            self.mesh.uvs = uvs;
            self.mesh.indices = indices;
            self.mesh.primitive_type = primitive_type;
        } else {
            self.mesh.surfaces.push(crate::mesh::Surface3D {
                vertices,
                normals,
                uvs,
                indices,
                primitive_type,
            });
        }
    }

    /// Returns the number of surfaces in this mesh.
    pub fn surface_count(&self) -> usize {
        self.mesh.surface_count()
    }

    /// Returns a reference to the underlying [`Mesh3D`].
    pub fn mesh(&self) -> &Mesh3D {
        &self.mesh
    }

    /// Consumes this `ArrayMesh` and returns the underlying [`Mesh3D`].
    pub fn into_mesh(self) -> Mesh3D {
        self.mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BoxMesh ──────────────────────────────────────────────────────

    #[test]
    fn box_mesh_default_generates_unit_cube() {
        let mesh = BoxMesh::default().generate();
        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
        for v in &mesh.vertices {
            assert!(v.x.abs() <= 0.5 + 1e-6);
            assert!(v.y.abs() <= 0.5 + 1e-6);
            assert!(v.z.abs() <= 0.5 + 1e-6);
        }
    }

    #[test]
    fn box_mesh_non_uniform_size() {
        let mesh = BoxMesh {
            size: Vector3::new(2.0, 4.0, 6.0),
        }
        .generate();
        assert_eq!(mesh.vertex_count(), 24);
        for v in &mesh.vertices {
            assert!(v.x.abs() <= 1.0 + 1e-6);
            assert!(v.y.abs() <= 2.0 + 1e-6);
            assert!(v.z.abs() <= 3.0 + 1e-6);
        }
    }

    // ── SphereMesh ───────────────────────────────────────────────────

    #[test]
    fn sphere_mesh_default_radius() {
        let mesh = SphereMesh::default().generate();
        assert!(!mesh.vertices.is_empty());
        for v in &mesh.vertices {
            assert!(v.length() <= 0.5 + 1e-3);
        }
    }

    #[test]
    fn sphere_mesh_squashed() {
        let mesh = SphereMesh {
            radius: 1.0,
            height: 1.0, // half the normal 2.0 height => squash Y by 0.5
            radial_segments: 16,
            rings: 8,
        }
        .generate();
        for v in &mesh.vertices {
            assert!(v.y.abs() <= 0.5 + 1e-3);
        }
    }

    // ── CapsuleMesh ──────────────────────────────────────────────────

    #[test]
    fn capsule_mesh_default() {
        let mesh = CapsuleMesh::default().generate();
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
        // All vertices should be within radius horizontally and height/2 vertically
        for v in &mesh.vertices {
            let radial = (v.x * v.x + v.z * v.z).sqrt();
            assert!(radial <= 0.5 + 1e-3, "radial {radial} exceeds radius");
            assert!(v.y.abs() <= 1.0 + 1e-3, "y {} exceeds half-height", v.y);
        }
    }

    #[test]
    fn capsule_mesh_indices_in_range() {
        let mesh = CapsuleMesh {
            radius: 0.3,
            height: 1.5,
            radial_segments: 8,
            rings: 4,
        }
        .generate();
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx, "index {idx} >= vertex count {max_idx}");
        }
    }

    #[test]
    fn capsule_normals_unit_length() {
        let mesh = CapsuleMesh::default().generate();
        for n in &mesh.normals {
            let len = n.length();
            assert!(
                (len - 1.0).abs() < 0.02,
                "normal length {len} not unit"
            );
        }
    }

    // ── CylinderMesh ─────────────────────────────────────────────────

    #[test]
    fn cylinder_mesh_default() {
        let mesh = CylinderMesh::default().generate();
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn cylinder_mesh_has_caps() {
        let mesh = CylinderMesh::default().generate();
        // Should have top and bottom cap centers with UP and DOWN normals
        let has_up = mesh.normals.iter().any(|n| (n.y - 1.0).abs() < 1e-4);
        let has_down = mesh.normals.iter().any(|n| (n.y + 1.0).abs() < 1e-4);
        assert!(has_up, "missing top cap");
        assert!(has_down, "missing bottom cap");
    }

    #[test]
    fn cylinder_cone_no_top_cap() {
        let mesh = CylinderMesh {
            top_radius: 0.0,
            bottom_radius: 0.5,
            height: 1.0,
            radial_segments: 8,
            rings: 2,
        }
        .generate();
        // Top cap vertices at y=0.5 with UP normal should not exist
        let top_cap_centers = mesh
            .vertices
            .iter()
            .zip(mesh.normals.iter())
            .filter(|(v, n)| (v.y - 0.5).abs() < 1e-4 && (n.y - 1.0).abs() < 1e-4 && v.x.abs() < 1e-4 && v.z.abs() < 1e-4)
            .count();
        assert_eq!(top_cap_centers, 0, "cone should have no top cap center");
    }

    #[test]
    fn cylinder_indices_in_range() {
        let mesh = CylinderMesh {
            top_radius: 0.3,
            bottom_radius: 0.5,
            height: 2.0,
            radial_segments: 12,
            rings: 3,
        }
        .generate();
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx, "index {idx} >= vertex count {max_idx}");
        }
    }

    // ── PlaneMesh ────────────────────────────────────────────────────

    #[test]
    fn plane_mesh_default() {
        let mesh = PlaneMesh::default().generate();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
        for v in &mesh.vertices {
            assert!(v.y.abs() < 1e-6);
            assert!(v.x.abs() <= 1.0 + 1e-6);
            assert!(v.z.abs() <= 1.0 + 1e-6);
        }
    }

    #[test]
    fn plane_mesh_custom_size() {
        let mesh = PlaneMesh {
            size: [4.0, 6.0],
        }
        .generate();
        for v in &mesh.vertices {
            assert!(v.x.abs() <= 2.0 + 1e-6);
            assert!(v.z.abs() <= 3.0 + 1e-6);
        }
    }

    // ── PrimitiveMeshType ────────────────────────────────────────────

    #[test]
    fn primitive_type_class_names() {
        assert_eq!(PrimitiveMeshType::Box(BoxMesh::default()).class_name(), "BoxMesh");
        assert_eq!(PrimitiveMeshType::Sphere(SphereMesh::default()).class_name(), "SphereMesh");
        assert_eq!(PrimitiveMeshType::Capsule(CapsuleMesh::default()).class_name(), "CapsuleMesh");
        assert_eq!(PrimitiveMeshType::Cylinder(CylinderMesh::default()).class_name(), "CylinderMesh");
        assert_eq!(PrimitiveMeshType::Plane(PlaneMesh::default()).class_name(), "PlaneMesh");
    }

    #[test]
    fn primitive_type_generate_dispatches() {
        let types = vec![
            PrimitiveMeshType::Box(BoxMesh::default()),
            PrimitiveMeshType::Sphere(SphereMesh::default()),
            PrimitiveMeshType::Capsule(CapsuleMesh::default()),
            PrimitiveMeshType::Cylinder(CylinderMesh::default()),
            PrimitiveMeshType::Plane(PlaneMesh::default()),
        ];
        for t in types {
            let mesh = t.generate();
            assert!(!mesh.vertices.is_empty(), "empty mesh for {}", t.class_name());
        }
    }

    // ── ArrayMesh ────────────────────────────────────────────────────

    #[test]
    fn array_mesh_empty() {
        let am = ArrayMesh::new();
        assert_eq!(am.surface_count(), 1); // primary surface (empty)
        assert!(am.mesh().vertices.is_empty());
    }

    #[test]
    fn array_mesh_single_surface() {
        let mut am = ArrayMesh::new();
        am.add_surface_from_arrays(
            PrimitiveType::Triangles,
            vec![Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)],
            vec![Vector3::UP; 3],
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            vec![0, 1, 2],
        );
        assert_eq!(am.surface_count(), 1);
        assert_eq!(am.mesh().vertex_count(), 3);
    }

    #[test]
    fn array_mesh_multi_surface() {
        let mut am = ArrayMesh::new();
        am.add_surface_from_arrays(
            PrimitiveType::Triangles,
            vec![Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)],
            vec![Vector3::UP; 3],
            vec![[0.0, 0.0]; 3],
            vec![0, 1, 2],
        );
        am.add_surface_from_arrays(
            PrimitiveType::Lines,
            vec![Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0)],
            vec![Vector3::UP; 2],
            vec![[0.0, 0.0]; 2],
            vec![0, 1],
        );
        assert_eq!(am.surface_count(), 2);
    }

    #[test]
    fn array_mesh_into_mesh() {
        let mut am = ArrayMesh::new();
        am.add_surface_from_arrays(
            PrimitiveType::Triangles,
            vec![Vector3::ZERO; 3],
            vec![Vector3::UP; 3],
            vec![[0.0, 0.0]; 3],
            vec![0, 1, 2],
        );
        let mesh = am.into_mesh();
        assert_eq!(mesh.vertex_count(), 3);
    }
}
