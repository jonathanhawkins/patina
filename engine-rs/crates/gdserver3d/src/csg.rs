//! Constructive Solid Geometry (CSG) primitives for 3D scenes.
//!
//! Implements Godot's CSG node types:
//! - `CSGBox3D` — axis-aligned box
//! - `CSGSphere3D` — UV sphere
//! - `CSGCylinder3D` — cylinder / cone
//! - `CSGMesh3D` — arbitrary mesh wrapper
//!
//! Each primitive generates a triangle mesh and supports boolean operations
//! (union, intersection, subtraction) via the [`CSGOperation`] enum.

use gdcore::math::Vector3;

use crate::mesh::Mesh3D;

// ---------------------------------------------------------------------------
// CSGOperation
// ---------------------------------------------------------------------------

/// Boolean operation mode for CSG nodes.
///
/// Maps to Godot's `CSGShape3D.Operation` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CSGOperation {
    /// Combine volumes (A ∪ B).
    #[default]
    Union,
    /// Keep only the overlap (A ∩ B).
    Intersection,
    /// Subtract child from parent (A − B).
    Subtraction,
}

impl CSGOperation {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::Intersection,
            2 => Self::Subtraction,
            _ => Self::Union,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Union => 0,
            Self::Intersection => 1,
            Self::Subtraction => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// CSGBox3D
// ---------------------------------------------------------------------------

/// A CSG box primitive.
///
/// Maps to Godot's `CSGBox3D`. Generates a 6-faced axis-aligned box mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct CSGBox3D {
    /// Half-extents of the box along each axis.
    pub size: Vector3,
    /// Boolean operation when combined with siblings.
    pub operation: CSGOperation,
    /// Whether collision is generated.
    pub use_collision: bool,
    /// Material resource path (if any).
    pub material_path: Option<String>,
}

impl Default for CSGBox3D {
    fn default() -> Self {
        Self {
            size: Vector3::new(1.0, 1.0, 1.0),
            operation: CSGOperation::Union,
            use_collision: false,
            material_path: None,
        }
    }
}

impl CSGBox3D {
    /// Creates a new CSG box with the given size.
    pub fn new(size: Vector3) -> Self {
        Self {
            size,
            ..Default::default()
        }
    }

    /// Generates a triangle mesh for this box.
    pub fn to_mesh(&self) -> Mesh3D {
        let hx = self.size.x * 0.5;
        let hy = self.size.y * 0.5;
        let hz = self.size.z * 0.5;

        // 8 vertices of the box
        let vertices = vec![
            // Front face (z+)
            Vector3::new(-hx, -hy, hz), // 0
            Vector3::new(hx, -hy, hz),  // 1
            Vector3::new(hx, hy, hz),   // 2
            Vector3::new(-hx, hy, hz),  // 3
            // Back face (z-)
            Vector3::new(hx, -hy, -hz),  // 4
            Vector3::new(-hx, -hy, -hz), // 5
            Vector3::new(-hx, hy, -hz),  // 6
            Vector3::new(hx, hy, -hz),   // 7
        ];

        // 12 triangles (2 per face, CCW winding)
        let indices = vec![
            // Front
            0, 1, 2, 0, 2, 3, // Back
            4, 5, 6, 4, 6, 7, // Top
            3, 2, 7, 3, 7, 6, // Bottom
            5, 4, 1, 5, 1, 0, // Right
            1, 4, 7, 1, 7, 2, // Left
            5, 0, 3, 5, 3, 6,
        ];

        Mesh3D {
            vertices,
            normals: Vec::new(),
            uvs: Vec::new(),
            indices,
            primitive_type: crate::mesh::PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// CSGSphere3D
// ---------------------------------------------------------------------------

/// A CSG sphere primitive.
///
/// Maps to Godot's `CSGSphere3D`. Generates a UV sphere mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct CSGSphere3D {
    /// Radius of the sphere.
    pub radius: f32,
    /// Number of radial segments (longitude).
    pub radial_segments: u32,
    /// Number of rings (latitude divisions).
    pub rings: u32,
    /// Boolean operation when combined with siblings.
    pub operation: CSGOperation,
    /// Whether collision is generated.
    pub use_collision: bool,
    /// Material resource path (if any).
    pub material_path: Option<String>,
    /// Whether to generate a smooth (vs. flat-shaded) sphere.
    pub smooth_faces: bool,
}

impl Default for CSGSphere3D {
    fn default() -> Self {
        Self {
            radius: 0.5,
            radial_segments: 12,
            rings: 6,
            operation: CSGOperation::Union,
            use_collision: false,
            material_path: None,
            smooth_faces: true,
        }
    }
}

impl CSGSphere3D {
    /// Creates a new CSG sphere with the given radius.
    pub fn new(radius: f32) -> Self {
        Self {
            radius,
            ..Default::default()
        }
    }

    /// Generates a triangle mesh for this sphere.
    pub fn to_mesh(&self) -> Mesh3D {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let rings = self.rings.max(2);
        let segments = self.radial_segments.max(3);

        for i in 0..=rings {
            let phi = std::f32::consts::PI * i as f32 / rings as f32;
            let y = self.radius * phi.cos();
            let r = self.radius * phi.sin();

            for j in 0..=segments {
                let theta = std::f32::consts::TAU * j as f32 / segments as f32;
                let x = r * theta.cos();
                let z = r * theta.sin();
                vertices.push(Vector3::new(x, y, z));
            }
        }

        let stride = segments + 1;
        for i in 0..rings {
            for j in 0..segments {
                let a = i * stride + j;
                let b = a + 1;
                let c = a + stride;
                let d = c + 1;
                indices.push(a);
                indices.push(c);
                indices.push(b);
                indices.push(b);
                indices.push(c);
                indices.push(d);
            }
        }

        Mesh3D {
            vertices,
            normals: Vec::new(),
            uvs: Vec::new(),
            indices,
            primitive_type: crate::mesh::PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// CSGCylinder3D
// ---------------------------------------------------------------------------

/// A CSG cylinder/cone primitive.
///
/// Maps to Godot's `CSGCylinder3D`. When `cone` is true, the top radius is 0.
#[derive(Debug, Clone, PartialEq)]
pub struct CSGCylinder3D {
    /// Radius of the cylinder.
    pub radius: f32,
    /// Height of the cylinder.
    pub height: f32,
    /// Number of sides around the circumference.
    pub sides: u32,
    /// If true, generate a cone (top radius = 0) instead of a cylinder.
    pub cone: bool,
    /// Boolean operation when combined with siblings.
    pub operation: CSGOperation,
    /// Whether collision is generated.
    pub use_collision: bool,
    /// Material resource path (if any).
    pub material_path: Option<String>,
    /// Whether to generate smooth faces.
    pub smooth_faces: bool,
}

impl Default for CSGCylinder3D {
    fn default() -> Self {
        Self {
            radius: 0.5,
            height: 2.0,
            sides: 8,
            cone: false,
            operation: CSGOperation::Union,
            use_collision: false,
            material_path: None,
            smooth_faces: true,
        }
    }
}

impl CSGCylinder3D {
    /// Creates a new CSG cylinder with the given radius and height.
    pub fn new(radius: f32, height: f32) -> Self {
        Self {
            radius,
            height,
            ..Default::default()
        }
    }

    /// Creates a new CSG cone with the given radius and height.
    pub fn new_cone(radius: f32, height: f32) -> Self {
        Self {
            radius,
            height,
            cone: true,
            ..Default::default()
        }
    }

    /// Generates a triangle mesh for this cylinder/cone.
    pub fn to_mesh(&self) -> Mesh3D {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let sides = self.sides.max(3);
        let half_h = self.height * 0.5;
        let top_radius = if self.cone { 0.0 } else { self.radius };

        // Bottom center vertex
        let bottom_center = vertices.len() as u32;
        vertices.push(Vector3::new(0.0, -half_h, 0.0));

        // Bottom ring
        let bottom_ring_start = vertices.len() as u32;
        for i in 0..sides {
            let angle = std::f32::consts::TAU * i as f32 / sides as f32;
            vertices.push(Vector3::new(
                self.radius * angle.cos(),
                -half_h,
                self.radius * angle.sin(),
            ));
        }

        // Top center vertex
        let top_center = vertices.len() as u32;
        vertices.push(Vector3::new(0.0, half_h, 0.0));

        // Top ring
        let top_ring_start = vertices.len() as u32;
        for i in 0..sides {
            let angle = std::f32::consts::TAU * i as f32 / sides as f32;
            vertices.push(Vector3::new(
                top_radius * angle.cos(),
                half_h,
                top_radius * angle.sin(),
            ));
        }

        // Bottom cap triangles (CCW when viewed from below → CW from above)
        for i in 0..sides {
            let next = (i + 1) % sides;
            indices.push(bottom_center);
            indices.push(bottom_ring_start + next);
            indices.push(bottom_ring_start + i);
        }

        // Top cap triangles
        for i in 0..sides {
            let next = (i + 1) % sides;
            indices.push(top_center);
            indices.push(top_ring_start + i);
            indices.push(top_ring_start + next);
        }

        // Side quads (two triangles each)
        for i in 0..sides {
            let next = (i + 1) % sides;
            let bl = bottom_ring_start + i;
            let br = bottom_ring_start + next;
            let tl = top_ring_start + i;
            let tr = top_ring_start + next;
            indices.push(bl);
            indices.push(br);
            indices.push(tr);
            indices.push(bl);
            indices.push(tr);
            indices.push(tl);
        }

        Mesh3D {
            vertices,
            normals: Vec::new(),
            uvs: Vec::new(),
            indices,
            primitive_type: crate::mesh::PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// CSGMesh3D
// ---------------------------------------------------------------------------

/// A CSG node that wraps an arbitrary mesh.
///
/// Maps to Godot's `CSGMesh3D`. Uses an existing `Mesh3D` for boolean operations.
#[derive(Debug, Clone, PartialEq)]
pub struct CSGMesh3D {
    /// The mesh to use for CSG operations.
    pub mesh: Option<Mesh3D>,
    /// Resource path to the mesh (for serialization).
    pub mesh_path: Option<String>,
    /// Boolean operation when combined with siblings.
    pub operation: CSGOperation,
    /// Whether collision is generated.
    pub use_collision: bool,
    /// Material resource path (if any).
    pub material_path: Option<String>,
}

impl Default for CSGMesh3D {
    fn default() -> Self {
        Self {
            mesh: None,
            mesh_path: None,
            operation: CSGOperation::Union,
            use_collision: false,
            material_path: None,
        }
    }
}

impl CSGMesh3D {
    /// Creates a new CSG mesh node with the given mesh.
    pub fn with_mesh(mesh: Mesh3D) -> Self {
        Self {
            mesh: Some(mesh),
            ..Default::default()
        }
    }

    /// Returns the mesh, if set.
    pub fn to_mesh(&self) -> Option<&Mesh3D> {
        self.mesh.as_ref()
    }
}

// ---------------------------------------------------------------------------
// CSGCombiner3D
// ---------------------------------------------------------------------------

/// A CSG combiner that holds child CSG shapes and produces a combined mesh.
///
/// Maps to Godot's `CSGCombiner3D`. Supports union (mesh concatenation),
/// intersection (keep overlapping volume), and subtraction (remove child
/// from parent) using AABB-based triangle classification.
#[derive(Debug, Clone, Default)]
pub struct CSGCombiner3D {
    /// Boolean operation for this combiner relative to its parent.
    pub operation: CSGOperation,
    /// Whether collision is generated from the combined mesh.
    pub use_collision: bool,
}

/// Axis-aligned bounding box for mesh overlap tests.
#[derive(Debug, Clone, Copy)]
struct Aabb {
    min: Vector3,
    max: Vector3,
}

impl Aabb {
    fn from_vertices(verts: &[Vector3]) -> Option<Self> {
        if verts.is_empty() {
            return None;
        }
        let mut min = verts[0];
        let mut max = verts[0];
        for v in &verts[1..] {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            min.z = min.z.min(v.z);
            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
            max.z = max.z.max(v.z);
        }
        Some(Self { min, max })
    }

    fn overlaps(&self, other: &Aabb) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    fn contains_point(&self, p: Vector3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }
}

/// Returns the centroid of a triangle.
fn triangle_centroid(a: Vector3, b: Vector3, c: Vector3) -> Vector3 {
    Vector3::new(
        (a.x + b.x + c.x) / 3.0,
        (a.y + b.y + c.y) / 3.0,
        (a.z + b.z + c.z) / 3.0,
    )
}

/// Classifies each triangle of `mesh` as inside or outside the AABB of `other`.
/// Returns (inside_triangles, outside_triangles) as separate meshes.
fn classify_triangles(mesh: &Mesh3D, other_aabb: &Aabb) -> (Mesh3D, Mesh3D) {
    let mut in_verts = Vec::new();
    let mut in_indices = Vec::new();
    let mut out_verts = Vec::new();
    let mut out_indices = Vec::new();

    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let a = mesh.vertices[tri[0] as usize];
        let b = mesh.vertices[tri[1] as usize];
        let c = mesh.vertices[tri[2] as usize];
        let center = triangle_centroid(a, b, c);

        if other_aabb.contains_point(center) {
            let off = in_verts.len() as u32;
            in_verts.extend_from_slice(&[a, b, c]);
            in_indices.extend_from_slice(&[off, off + 1, off + 2]);
        } else {
            let off = out_verts.len() as u32;
            out_verts.extend_from_slice(&[a, b, c]);
            out_indices.extend_from_slice(&[off, off + 1, off + 2]);
        }
    }

    let inside = Mesh3D {
        vertices: in_verts,
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: in_indices,
        primitive_type: crate::mesh::PrimitiveType::Triangles,
        surfaces: Vec::new(),
    };
    let outside = Mesh3D {
        vertices: out_verts,
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: out_indices,
        primitive_type: crate::mesh::PrimitiveType::Triangles,
        surfaces: Vec::new(),
    };
    (inside, outside)
}

impl CSGCombiner3D {
    /// Combines multiple meshes using simple union (concatenation).
    pub fn combine_union(meshes: &[Mesh3D]) -> Mesh3D {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for mesh in meshes {
            let offset = vertices.len() as u32;
            vertices.extend_from_slice(&mesh.vertices);
            indices.extend(mesh.indices.iter().map(|i| i + offset));
        }

        Mesh3D {
            vertices,
            normals: Vec::new(),
            uvs: Vec::new(),
            indices,
            primitive_type: crate::mesh::PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }

    /// Combines two meshes using intersection: keeps only the overlapping volume.
    ///
    /// Triangles from each mesh whose centroids fall inside the other mesh's
    /// AABB are retained. This is an AABB-based approximation suitable for
    /// convex shapes.
    pub fn combine_intersection(a: &Mesh3D, b: &Mesh3D) -> Mesh3D {
        let aabb_a = match Aabb::from_vertices(&a.vertices) {
            Some(bb) => bb,
            None => return Mesh3D::new(crate::mesh::PrimitiveType::Triangles),
        };
        let aabb_b = match Aabb::from_vertices(&b.vertices) {
            Some(bb) => bb,
            None => return Mesh3D::new(crate::mesh::PrimitiveType::Triangles),
        };

        if !aabb_a.overlaps(&aabb_b) {
            return Mesh3D::new(crate::mesh::PrimitiveType::Triangles);
        }

        let (a_inside, _) = classify_triangles(a, &aabb_b);
        let (b_inside, _) = classify_triangles(b, &aabb_a);

        Self::combine_union(&[a_inside, b_inside])
    }

    /// Combines two meshes using subtraction: removes B's volume from A.
    ///
    /// Triangles from A whose centroids fall inside B's AABB are removed.
    /// Triangles from B whose centroids fall inside A's AABB are kept with
    /// inverted winding order (flipped normals) to form the interior surface.
    pub fn combine_subtraction(a: &Mesh3D, b: &Mesh3D) -> Mesh3D {
        let aabb_a = match Aabb::from_vertices(&a.vertices) {
            Some(bb) => bb,
            None => return Mesh3D::new(crate::mesh::PrimitiveType::Triangles),
        };
        let aabb_b = match Aabb::from_vertices(&b.vertices) {
            Some(bb) => bb,
            None => return a.clone(),
        };

        if !aabb_a.overlaps(&aabb_b) {
            return a.clone();
        }

        // Keep A triangles outside B
        let (_, a_outside) = classify_triangles(a, &aabb_b);

        // Keep B triangles inside A, but with inverted winding (interior surface)
        let (b_inside, _) = classify_triangles(b, &aabb_a);
        let b_inverted = invert_winding(&b_inside);

        Self::combine_union(&[a_outside, b_inverted])
    }

    /// Combines two meshes using the specified operation.
    pub fn combine(a: &Mesh3D, b: &Mesh3D, operation: CSGOperation) -> Mesh3D {
        match operation {
            CSGOperation::Union => Self::combine_union(&[a.clone(), b.clone()]),
            CSGOperation::Intersection => Self::combine_intersection(a, b),
            CSGOperation::Subtraction => Self::combine_subtraction(a, b),
        }
    }
}

/// Inverts the winding order of all triangles in a mesh (flips face normals).
fn invert_winding(mesh: &Mesh3D) -> Mesh3D {
    let mut indices = mesh.indices.clone();
    for tri in indices.chunks_mut(3) {
        if tri.len() >= 3 {
            tri.swap(1, 2);
        }
    }
    Mesh3D {
        vertices: mesh.vertices.clone(),
        normals: mesh
            .normals
            .iter()
            .map(|n| Vector3::new(-n.x, -n.y, -n.z))
            .collect(),
        uvs: mesh.uvs.clone(),
        indices,
        primitive_type: mesh.primitive_type,
        surfaces: mesh.surfaces.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- CSGOperation --

    #[test]
    fn operation_roundtrip() {
        for (int_val, expected) in [
            (0, CSGOperation::Union),
            (1, CSGOperation::Intersection),
            (2, CSGOperation::Subtraction),
        ] {
            let op = CSGOperation::from_godot_int(int_val);
            assert_eq!(op, expected);
            assert_eq!(op.to_godot_int(), int_val);
        }
    }

    #[test]
    fn operation_unknown_defaults_to_union() {
        assert_eq!(CSGOperation::from_godot_int(99), CSGOperation::Union);
    }

    // -- CSGBox3D --

    #[test]
    fn box_defaults() {
        let b = CSGBox3D::default();
        assert_eq!(b.size, Vector3::new(1.0, 1.0, 1.0));
        assert_eq!(b.operation, CSGOperation::Union);
        assert!(!b.use_collision);
    }

    #[test]
    fn box_mesh_has_correct_topology() {
        let b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0));
        let mesh = b.to_mesh();
        assert_eq!(mesh.vertices.len(), 8, "box should have 8 vertices");
        assert_eq!(
            mesh.indices.len(),
            36,
            "box should have 36 indices (12 triangles)"
        );
    }

    #[test]
    fn box_mesh_vertices_within_half_extents() {
        let b = CSGBox3D::new(Vector3::new(4.0, 6.0, 2.0));
        let mesh = b.to_mesh();
        for v in &mesh.vertices {
            assert!(v.x.abs() <= 2.0 + 1e-5);
            assert!(v.y.abs() <= 3.0 + 1e-5);
            assert!(v.z.abs() <= 1.0 + 1e-5);
        }
    }

    #[test]
    fn box_indices_in_range() {
        let b = CSGBox3D::default();
        let mesh = b.to_mesh();
        let n = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < n, "index {idx} out of range (n={n})");
        }
    }

    // -- CSGSphere3D --

    #[test]
    fn sphere_defaults() {
        let s = CSGSphere3D::default();
        assert!((s.radius - 0.5).abs() < 1e-5);
        assert_eq!(s.radial_segments, 12);
        assert_eq!(s.rings, 6);
        assert!(s.smooth_faces);
    }

    #[test]
    fn sphere_mesh_vertices_on_radius() {
        let s = CSGSphere3D::new(3.0);
        let mesh = s.to_mesh();
        for v in &mesh.vertices {
            let len = v.length();
            assert!(
                (len - 3.0).abs() < 1e-4,
                "vertex at distance {len}, expected 3.0"
            );
        }
    }

    #[test]
    fn sphere_mesh_has_triangles() {
        let s = CSGSphere3D::new(1.0);
        let mesh = s.to_mesh();
        assert!(!mesh.vertices.is_empty());
        assert!(
            mesh.indices.len() >= 6,
            "sphere should have at least 2 triangles"
        );
        assert_eq!(mesh.indices.len() % 3, 0, "indices must be multiple of 3");
    }

    #[test]
    fn sphere_indices_in_range() {
        let s = CSGSphere3D::default();
        let mesh = s.to_mesh();
        let n = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < n, "index {idx} out of range (n={n})");
        }
    }

    // -- CSGCylinder3D --

    #[test]
    fn cylinder_defaults() {
        let c = CSGCylinder3D::default();
        assert!((c.radius - 0.5).abs() < 1e-5);
        assert!((c.height - 2.0).abs() < 1e-5);
        assert_eq!(c.sides, 8);
        assert!(!c.cone);
    }

    #[test]
    fn cylinder_mesh_has_caps_and_sides() {
        let c = CSGCylinder3D::new(1.0, 2.0);
        let mesh = c.to_mesh();
        // sides=8: 2 center verts + 2*8 ring verts = 18
        assert_eq!(mesh.vertices.len(), 18);
        // 8 bottom cap tris (24) + 8 top cap tris (24) + 8*2 side tris (48) = 96 indices
        assert_eq!(mesh.indices.len(), 96);
    }

    #[test]
    fn cylinder_indices_in_range() {
        let c = CSGCylinder3D::default();
        let mesh = c.to_mesh();
        let n = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < n, "index {idx} out of range (n={n})");
        }
    }

    #[test]
    fn cone_top_vertices_at_origin() {
        let c = CSGCylinder3D::new_cone(1.0, 3.0);
        let mesh = c.to_mesh();
        assert!(c.cone);
        // Top ring vertices should all be at y=1.5, radius=0
        let half_h = 1.5;
        let top_verts: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| (v.y - half_h).abs() < 1e-5 && v.x.abs() < 1e-5 && v.z.abs() < 1e-5)
            .collect();
        // Should have top center + all top ring verts collapsed to apex
        assert!(
            top_verts.len() >= 2,
            "cone should have top vertices at apex, got {}",
            top_verts.len()
        );
    }

    #[test]
    fn cone_indices_in_range() {
        let c = CSGCylinder3D::new_cone(1.0, 2.0);
        let mesh = c.to_mesh();
        let n = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < n, "index {idx} out of range (n={n})");
        }
    }

    // -- CSGMesh3D --

    #[test]
    fn csg_mesh_default_is_empty() {
        let m = CSGMesh3D::default();
        assert!(m.mesh.is_none());
        assert!(m.to_mesh().is_none());
    }

    #[test]
    fn csg_mesh_with_mesh() {
        let cube = Mesh3D::cube(1.0);
        let m = CSGMesh3D::with_mesh(cube.clone());
        assert!(m.to_mesh().is_some());
        assert_eq!(m.to_mesh().unwrap().vertices.len(), cube.vertices.len());
    }

    // -- CSGCombiner3D --

    #[test]
    fn combiner_union_concatenates() {
        let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let combined = CSGCombiner3D::combine_union(&[a.clone(), b.clone()]);
        assert_eq!(combined.vertices.len(), a.vertices.len() + b.vertices.len());
        assert_eq!(combined.indices.len(), a.indices.len() + b.indices.len());
    }

    #[test]
    fn combiner_union_empty() {
        let combined = CSGCombiner3D::combine_union(&[]);
        assert!(combined.vertices.is_empty());
        assert!(combined.indices.is_empty());
    }

    #[test]
    fn combiner_union_indices_valid() {
        let a = CSGSphere3D::new(1.0).to_mesh();
        let b = CSGCylinder3D::new(0.5, 2.0).to_mesh();
        let combined = CSGCombiner3D::combine_union(&[a, b]);
        let n = combined.vertices.len() as u32;
        for &idx in &combined.indices {
            assert!(idx < n, "combined index {idx} out of range (n={n})");
        }
    }

    // -- Intersection --

    #[test]
    fn intersection_overlapping_boxes() {
        // Two overlapping unit boxes offset by 0.5 in X
        let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let mut b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        // Offset B by 0.5 in X — still overlapping
        for v in &mut b.vertices {
            v.x += 0.5;
        }
        let result = CSGCombiner3D::combine_intersection(&a, &b);
        // Should have some triangles (the overlap region)
        assert!(
            !result.vertices.is_empty(),
            "intersection of overlapping boxes should produce geometry"
        );
        assert!(!result.indices.is_empty());
        assert_eq!(result.indices.len() % 3, 0, "indices must be multiple of 3");
    }

    #[test]
    fn intersection_disjoint_boxes_is_empty() {
        let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let mut b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        // Move B far away — no overlap
        for v in &mut b.vertices {
            v.x += 10.0;
        }
        let result = CSGCombiner3D::combine_intersection(&a, &b);
        assert!(
            result.vertices.is_empty(),
            "disjoint boxes should produce empty intersection"
        );
        assert!(result.indices.is_empty());
    }

    #[test]
    fn intersection_empty_mesh_returns_empty() {
        let a = CSGBox3D::default().to_mesh();
        let empty = Mesh3D::new(crate::mesh::PrimitiveType::Triangles);
        let result = CSGCombiner3D::combine_intersection(&a, &empty);
        assert!(result.vertices.is_empty());
    }

    #[test]
    fn intersection_indices_in_range() {
        let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let b = CSGSphere3D::new(1.0).to_mesh();
        let result = CSGCombiner3D::combine_intersection(&a, &b);
        let n = result.vertices.len() as u32;
        for &idx in &result.indices {
            assert!(idx < n, "intersection index {idx} out of range (n={n})");
        }
    }

    #[test]
    fn intersection_contained_box_keeps_inner() {
        // Small box fully inside large box — all of small box's triangles should be kept
        let big = CSGBox3D::new(Vector3::new(4.0, 4.0, 4.0)).to_mesh();
        let small = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let result = CSGCombiner3D::combine_intersection(&big, &small);
        // All of small's triangles are inside big's AABB, so they should all be kept
        assert!(
            result.indices.len() >= small.indices.len(),
            "small box inside big box: expected at least {} indices, got {}",
            small.indices.len(),
            result.indices.len()
        );
    }

    // -- Subtraction --

    #[test]
    fn subtraction_disjoint_returns_original() {
        let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let mut b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        for v in &mut b.vertices {
            v.x += 10.0;
        }
        let result = CSGCombiner3D::combine_subtraction(&a, &b);
        // No overlap, so A is returned unchanged
        assert_eq!(result.vertices.len(), a.vertices.len());
        assert_eq!(result.indices.len(), a.indices.len());
    }

    #[test]
    fn subtraction_overlapping_produces_inverted_interior() {
        let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let mut b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        // Offset B slightly so only some triangles overlap
        for v in &mut b.vertices {
            v.x += 0.3;
        }
        let result = CSGCombiner3D::combine_subtraction(&a, &b);
        // Should have geometry from both A (exterior) and inverted B (interior)
        assert!(
            !result.indices.is_empty(),
            "subtraction should produce geometry"
        );
        assert_eq!(result.indices.len() % 3, 0);
        // Result should differ from A alone (B carves into it)
        let n = result.vertices.len() as u32;
        for &idx in &result.indices {
            assert!(idx < n, "index {idx} out of range (n={n})");
        }
    }

    #[test]
    fn subtraction_empty_b_returns_a() {
        let a = CSGBox3D::default().to_mesh();
        let empty = Mesh3D::new(crate::mesh::PrimitiveType::Triangles);
        let result = CSGCombiner3D::combine_subtraction(&a, &empty);
        assert_eq!(result.vertices.len(), a.vertices.len());
    }

    #[test]
    fn subtraction_indices_in_range() {
        let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let b = CSGSphere3D::new(0.5).to_mesh();
        let result = CSGCombiner3D::combine_subtraction(&a, &b);
        let n = result.vertices.len() as u32;
        for &idx in &result.indices {
            assert!(idx < n, "subtraction index {idx} out of range (n={n})");
        }
    }

    // -- combine() dispatch --

    #[test]
    fn combine_dispatches_union() {
        let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let result = CSGCombiner3D::combine(&a, &b, CSGOperation::Union);
        assert_eq!(result.vertices.len(), a.vertices.len() + b.vertices.len());
    }

    #[test]
    fn combine_dispatches_intersection() {
        let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let result = CSGCombiner3D::combine(&a, &b, CSGOperation::Intersection);
        // Same boxes fully overlapping — intersection should have geometry
        assert!(!result.vertices.is_empty());
    }

    #[test]
    fn combine_dispatches_subtraction() {
        let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
        let mut b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        for v in &mut b.vertices {
            v.x += 10.0;
        }
        let result = CSGCombiner3D::combine(&a, &b, CSGOperation::Subtraction);
        // Disjoint → returns A unchanged
        assert_eq!(result.vertices.len(), a.vertices.len());
    }

    // -- invert_winding --

    #[test]
    fn invert_winding_swaps_triangle_order() {
        let mesh = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
        let inverted = invert_winding(&mesh);
        assert_eq!(inverted.vertices.len(), mesh.vertices.len());
        assert_eq!(inverted.indices.len(), mesh.indices.len());
        // Check first triangle has swapped indices 1 and 2
        if mesh.indices.len() >= 3 {
            assert_eq!(inverted.indices[0], mesh.indices[0]);
            assert_eq!(inverted.indices[1], mesh.indices[2]);
            assert_eq!(inverted.indices[2], mesh.indices[1]);
        }
    }

    #[test]
    fn invert_winding_normals_flipped() {
        // Build a mesh with normals
        let mesh = Mesh3D::cube(1.0);
        let inverted = invert_winding(&mesh);
        for (orig, inv) in mesh.normals.iter().zip(inverted.normals.iter()) {
            assert!((orig.x + inv.x).abs() < 1e-6);
            assert!((orig.y + inv.y).abs() < 1e-6);
            assert!((orig.z + inv.z).abs() < 1e-6);
        }
    }

    // -- AABB --

    #[test]
    fn aabb_from_empty_vertices() {
        assert!(Aabb::from_vertices(&[]).is_none());
    }

    #[test]
    fn aabb_contains_point_inside() {
        let aabb =
            Aabb::from_vertices(&[Vector3::new(-1.0, -1.0, -1.0), Vector3::new(1.0, 1.0, 1.0)])
                .unwrap();
        assert!(aabb.contains_point(Vector3::new(0.0, 0.0, 0.0)));
        assert!(aabb.contains_point(Vector3::new(0.5, 0.5, 0.5)));
    }

    #[test]
    fn aabb_does_not_contain_point_outside() {
        let aabb =
            Aabb::from_vertices(&[Vector3::new(-1.0, -1.0, -1.0), Vector3::new(1.0, 1.0, 1.0)])
                .unwrap();
        assert!(!aabb.contains_point(Vector3::new(2.0, 0.0, 0.0)));
    }

    #[test]
    fn aabb_overlaps_adjacent() {
        let a = Aabb::from_vertices(&[Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0)])
            .unwrap();
        let b = Aabb::from_vertices(&[Vector3::new(0.5, 0.0, 0.0), Vector3::new(1.5, 1.0, 1.0)])
            .unwrap();
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
    }

    #[test]
    fn aabb_no_overlap_disjoint() {
        let a = Aabb::from_vertices(&[Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0)])
            .unwrap();
        let b = Aabb::from_vertices(&[Vector3::new(5.0, 5.0, 5.0), Vector3::new(6.0, 6.0, 6.0)])
            .unwrap();
        assert!(!a.overlaps(&b));
    }

    // -- triangle_centroid --

    #[test]
    fn triangle_centroid_is_average() {
        let c = triangle_centroid(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
            Vector3::new(0.0, 3.0, 0.0),
        );
        assert!((c.x - 1.0).abs() < 1e-6);
        assert!((c.y - 1.0).abs() < 1e-6);
        assert!((c.z - 0.0).abs() < 1e-6);
    }

    // -- classify_triangles --

    #[test]
    fn classify_triangles_splits_correctly() {
        // Build a mesh with two triangles: one at origin, one far away
        let mesh = Mesh3D {
            vertices: vec![
                // Triangle 1 (centered near origin)
                Vector3::new(-0.1, -0.1, 0.0),
                Vector3::new(0.1, -0.1, 0.0),
                Vector3::new(0.0, 0.1, 0.0),
                // Triangle 2 (centered at x=10)
                Vector3::new(9.9, -0.1, 0.0),
                Vector3::new(10.1, -0.1, 0.0),
                Vector3::new(10.0, 0.1, 0.0),
            ],
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: vec![0, 1, 2, 3, 4, 5],
            primitive_type: crate::mesh::PrimitiveType::Triangles,
            surfaces: Vec::new(),
        };
        let aabb = Aabb {
            min: Vector3::new(-1.0, -1.0, -1.0),
            max: Vector3::new(1.0, 1.0, 1.0),
        };
        let (inside, outside) = classify_triangles(&mesh, &aabb);
        assert_eq!(inside.indices.len(), 3, "one triangle inside");
        assert_eq!(outside.indices.len(), 3, "one triangle outside");
    }
}
