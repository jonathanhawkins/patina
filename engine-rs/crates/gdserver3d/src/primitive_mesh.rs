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
            (
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(0.0, 1.0, 0.0),
                hz,
                hy,
                hx,
            ),
            // -X
            (
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 1.0, 0.0),
                hz,
                hy,
                hx,
            ),
            // +Y
            (
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                hx,
                hz,
                hy,
            ),
            // -Y
            (
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                hx,
                hz,
                hy,
            ),
            // +Z
            (
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                hx,
                hy,
                hz,
            ),
            // -Z
            (
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                hx,
                hy,
                hz,
            ),
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

            // 🔑 Wound CLOCKWISE as seen from outside, on EVERY face — Godot's
            // front-face convention (`doc/classes/ArrayMesh.xml:49`). The quad
            // `0,1,2 / 0,2,3` is counter-clockwise exactly when `u × v` points
            // along the outward normal, which held for the X and Z faces but not
            // the Y faces, so this mesh used to be half inside-out: under
            // back-face culling the top of a box was dropped and its BOTTOM face
            // (lit by a downward normal) drew in its place. Only the index order
            // changes; vertices and UVs are untouched.
            if u_dir.cross(*v_dir).dot(*normal) > 0.0 {
                indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            } else {
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }

        Mesh3D {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
            ..Default::default()
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
        Mesh3D::capsule(
            self.radius,
            self.height,
            self.radial_segments.max(3),
            self.rings.max(1),
        )
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

/// Which way a [`PlaneMesh`] faces, matching Godot's `PlaneMesh.Orientation`.
///
/// ⚠️ The default is [`FaceY`](Self::FaceY) = **1**, not the first variant.
/// Godot numbers these FACE_X 0 / FACE_Y 1 / FACE_Z 2 (`PlaneMesh.xml:30-36`),
/// so `#[derive(Default)]` — which would take declaration order — is wrong here
/// and would silently face the plane along the wrong axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaneOrientation {
    /// Faces +X; the plane lies in the ZY plane.
    FaceX,
    /// Faces +Y; the plane lies in the XZ plane. Godot's default.
    #[default]
    FaceY,
    /// Faces +Z; the plane lies in the XY plane. What `QuadMesh` uses.
    FaceZ,
}

impl PlaneOrientation {
    /// The integer Godot serialises this orientation as.
    pub fn as_int(self) -> i64 {
        match self {
            Self::FaceX => 0,
            Self::FaceY => 1,
            Self::FaceZ => 2,
        }
    }

    /// Reads an orientation from its serialised integer, falling back to
    /// Godot's default for anything out of range.
    pub fn from_int(value: i64) -> Self {
        match value {
            0 => Self::FaceX,
            2 => Self::FaceZ,
            _ => Self::FaceY,
        }
    }

    /// The plane's outward unit normal.
    pub fn normal(self) -> Vector3 {
        match self {
            Self::FaceX => Vector3::new(1.0, 0.0, 0.0),
            Self::FaceY => Vector3::new(0.0, 1.0, 0.0),
            Self::FaceZ => Vector3::new(0.0, 0.0, 1.0),
        }
    }
}

/// A Godot-compatible `PlaneMesh` — by default a quad on the XZ plane, but any
/// of the three axis-aligned planes via [`orientation`](Self::orientation).
///
/// 🔑 `QuadMesh` is this same resource with two different defaults, 1x1 and
/// `FaceZ` — see [`PlaneMesh::quad`]. The two shared one generator AND one set
/// of defaults until pat-s3rrv.198, which made every QuadMesh render at double
/// size in the wrong plane.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneMesh {
    /// Size along the plane's two axes.
    pub size: [f32; 2],
    /// Which axis the plane faces.
    pub orientation: PlaneOrientation,
    /// Translation applied to every vertex after orientation.
    pub center_offset: Vector3,
    /// Extra subdivisions across the width; `0` yields a single quad.
    pub subdivide_width: u32,
    /// Extra subdivisions across the depth; `0` yields a single quad.
    pub subdivide_depth: u32,
}

impl Default for PlaneMesh {
    fn default() -> Self {
        Self {
            size: [2.0, 2.0],
            orientation: PlaneOrientation::FaceY,
            center_offset: Vector3::ZERO,
            subdivide_width: 0,
            subdivide_depth: 0,
        }
    }
}

impl PlaneMesh {
    /// Godot's `QuadMesh`: a PlaneMesh whose `size` is 1x1 and whose
    /// `orientation` is `FACE_Z` (`QuadMesh.xml:14-15`).
    pub fn quad() -> Self {
        Self {
            size: [1.0, 1.0],
            orientation: PlaneOrientation::FaceZ,
            ..Self::default()
        }
    }

    /// Generates the [`Mesh3D`] geometry for this plane.
    ///
    /// Ported from `PlaneMesh::_create_mesh_array`
    /// (`scene/resources/3d/primitive_meshes.cpp`), including its inverted UVs
    /// (`1 - u`, `1 - v`), which upstream comments as matching Quad's
    /// orientation.
    pub fn generate(&self) -> Mesh3D {
        let sw = self.subdivide_width;
        let sd = self.subdivide_depth;
        let start_x = self.size[0] * -0.5;
        let start_z = self.size[1] * -0.5;
        let normal = self.orientation.normal();

        let cols = sw + 2;
        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        for j in 0..=(sd + 1) {
            let v = j as f32 / (sd as f32 + 1.0);
            let z = start_z + self.size[1] * v;
            for i in 0..=(sw + 1) {
                let u = i as f32 / (sw as f32 + 1.0);
                let x = start_x + self.size[0] * u;

                // Axis mapping straight from upstream — note the sign flips on
                // FACE_Y and FACE_Z, which are what keep the winding consistent
                // across the three orientations.
                let p = match self.orientation {
                    PlaneOrientation::FaceX => Vector3::new(0.0, z, x),
                    PlaneOrientation::FaceY => Vector3::new(-x, 0.0, -z),
                    PlaneOrientation::FaceZ => Vector3::new(-x, z, 0.0),
                };
                vertices.push(p + self.center_offset);
                normals.push(normal);
                uvs.push([1.0 - u, 1.0 - v]);

                if i > 0 && j > 0 {
                    let thisrow = j * cols;
                    let prevrow = (j - 1) * cols;
                    indices.extend_from_slice(&[
                        prevrow + i - 1,
                        prevrow + i,
                        thisrow + i - 1,
                        prevrow + i,
                        thisrow + i,
                        thisrow + i - 1,
                    ]);
                }
            }
        }

        Mesh3D {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            ..Default::default()
        }
    }
}

/// Godot's `TorusMesh` — a ring whose tube spans `inner_radius`..`outer_radius`.
///
/// Defaults transcribed from `doc/classes/TorusMesh.xml`, not derived: Godot's
/// `rings` is the count of steps around the MAJOR circle and `ring_segments`
/// the steps around the tube cross-section, and the two are easy to swap
/// because both are "how many segments".
#[derive(Debug, Clone, PartialEq)]
pub struct TorusMesh {
    /// Radius of the hole in the middle.
    pub inner_radius: f32,
    /// Radius out to the far edge of the tube.
    pub outer_radius: f32,
    /// Segments around the major circle.
    pub rings: u32,
    /// Segments around the tube cross-section.
    pub ring_segments: u32,
}

impl Default for TorusMesh {
    fn default() -> Self {
        Self {
            inner_radius: 0.5,
            outer_radius: 1.0,
            rings: 64,
            ring_segments: 32,
        }
    }
}

impl TorusMesh {
    /// Generates the [`Mesh3D`] geometry for this torus.
    pub fn generate(&self) -> Mesh3D {
        Mesh3D::torus(
            self.inner_radius,
            self.outer_radius,
            self.rings,
            self.ring_segments,
        )
    }
}

/// Godot's `TubeTrailMesh` — a capped cylinder subdivided into `sections`,
/// used as the skinned trail behind a moving object.
///
/// Ported from `upstream/godot/scene/resources/3d/primitive_meshes.cpp`
/// (`TubeTrailMesh::_create_mesh_array`) rather than reconstructed from the
/// class docs, so the seam duplication, cap winding and vertex ordering match.
///
/// ⚠️ Godot's `curve` property, which modulates the radius along the tube, is
/// not represented: it is a `Curve` resource and this crate has no Curve type.
/// Geometry here is therefore the `curve`-less case, which is also Godot's
/// behaviour when the property is unset.
#[derive(Debug, Clone, PartialEq)]
pub struct TubeTrailMesh {
    /// Radius of the tube.
    pub radius: f32,
    /// Vertices around the tube's circumference.
    pub radial_steps: u32,
    /// Number of sections along the tube.
    pub sections: u32,
    /// Length of each section.
    pub section_length: f32,
    /// Rings subdividing each section lengthwise.
    pub section_rings: u32,
    /// Whether the +Y end is closed.
    pub cap_top: bool,
    /// Whether the -Y end is closed.
    pub cap_bottom: bool,
}

impl Default for TubeTrailMesh {
    fn default() -> Self {
        // Transcribed from `doc/classes/TubeTrailMesh.xml`.
        Self {
            radius: 0.5,
            radial_steps: 8,
            sections: 5,
            section_length: 0.2,
            section_rings: 3,
            cap_top: true,
            cap_bottom: true,
        }
    }
}

impl TubeTrailMesh {
    /// Generates the [`Mesh3D`] geometry for this tube.
    pub fn generate(&self) -> Mesh3D {
        let radial_steps = self.radial_steps.max(3);
        let sections = self.sections.max(1);
        let section_rings = self.section_rings.max(1);

        let total_rings = section_rings * sections;
        let depth = self.section_length * sections as f32;

        let mut vertices: Vec<Vector3> = Vec::new();
        let mut normals: Vec<Vector3> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let tau = std::f32::consts::TAU;
        let mut point: u32 = 0;
        let mut thisrow: u32 = 0;
        let mut prevrow: u32 = 0;

        for j in 0..=total_rings {
            let v = j as f32 / total_rings as f32;
            let y = (depth * 0.5) - depth * v;

            for i in 0..=radial_steps {
                let u = i as f32 / radial_steps as f32;

                // ⚠️ The last step is NOT computed from the angle: Godot forces
                // (0, 1) so the seam vertex is bit-identical to the first one
                // while still carrying u = 1 for the UV wrap.
                let (x, z) = if i < radial_steps {
                    let a = u * tau;
                    (a.sin(), a.cos())
                } else {
                    (0.0, 1.0)
                };

                vertices.push(Vector3::new(x * self.radius, y, z * self.radius));
                normals.push(Vector3::new(x, 0.0, z));
                uvs.push([u, v * 0.5]);
                point += 1;

                if i > 0 && j > 0 {
                    indices.push(prevrow + i - 1);
                    indices.push(prevrow + i);
                    indices.push(thisrow + i - 1);
                    indices.push(prevrow + i);
                    indices.push(thisrow + i);
                    indices.push(thisrow + i - 1);
                }
            }

            prevrow = thisrow;
            thisrow = point;
        }

        // Both caps are a centre vertex fanned out to a ring. Their windings are
        // opposite so each faces outward along its own axis.
        let mut cap = |y: f32, normal: Vector3, centre_uv: [f32; 2], top: bool| {
            let centre = point;
            vertices.push(Vector3::new(0.0, y, 0.0));
            normals.push(normal);
            uvs.push(centre_uv);
            point += 1;

            for i in 0..=radial_steps {
                let r = i as f32 / radial_steps as f32;
                let (x, z) = if i < radial_steps {
                    let a = r * tau;
                    (a.sin(), a.cos())
                } else {
                    (0.0, 1.0)
                };

                vertices.push(Vector3::new(x * self.radius, y, z * self.radius));
                normals.push(normal);
                uvs.push([(x + 1.0) * 0.25, 0.5 + (z + 1.0) * 0.25]);
                point += 1;

                if i > 0 {
                    indices.push(centre);
                    if top {
                        indices.push(point - 1);
                        indices.push(point - 2);
                    } else {
                        indices.push(point - 2);
                        indices.push(point - 1);
                    }
                }
            }
        };

        if self.cap_top {
            cap(depth * 0.5, Vector3::new(0.0, 1.0, 0.0), [0.25, 0.75], true);
        }
        if self.cap_bottom {
            cap(
                depth * -0.5,
                Vector3::new(0.0, -1.0, 0.0),
                [0.75, 0.75],
                false,
            );
        }

        Mesh3D {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
            ..Default::default()
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
    /// `TorusMesh`
    Torus(TorusMesh),
    /// `TubeTrailMesh`
    TubeTrail(TubeTrailMesh),
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
            Self::Torus(_) => "TorusMesh",
            Self::TubeTrail(_) => "TubeTrailMesh",
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
            Self::Torus(m) => m.generate(),
            Self::TubeTrail(m) => m.generate(),
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
                bone_weights: Vec::new(),
                bone_indices: Vec::new(),
                lods: Vec::new(),
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
            assert!((len - 1.0).abs() < 0.02, "normal length {len} not unit");
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
            .filter(|(v, n)| {
                (v.y - 0.5).abs() < 1e-4
                    && (n.y - 1.0).abs() < 1e-4
                    && v.x.abs() < 1e-4
                    && v.z.abs() < 1e-4
            })
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
            ..Default::default()
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
        assert_eq!(
            PrimitiveMeshType::Box(BoxMesh::default()).class_name(),
            "BoxMesh"
        );
        assert_eq!(
            PrimitiveMeshType::Sphere(SphereMesh::default()).class_name(),
            "SphereMesh"
        );
        assert_eq!(
            PrimitiveMeshType::Capsule(CapsuleMesh::default()).class_name(),
            "CapsuleMesh"
        );
        assert_eq!(
            PrimitiveMeshType::Cylinder(CylinderMesh::default()).class_name(),
            "CylinderMesh"
        );
        assert_eq!(
            PrimitiveMeshType::Plane(PlaneMesh::default()).class_name(),
            "PlaneMesh"
        );
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
            assert!(
                !mesh.vertices.is_empty(),
                "empty mesh for {}",
                t.class_name()
            );
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
            vec![
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ],
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
            vec![
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ],
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

/// Builds any supported `PrimitiveMesh` subtype from its Godot class name.
///
/// `authored(key)` yields the value the scene set for a property, or `None`
/// when it did not. **Godot's documented defaults live here and nowhere else.**
/// That is the point of this function rather than a convenience: the editor
/// viewport and the runtime renderer both call it, and the divergence it
/// replaces existed precisely because each caller carried its own fallback
/// numbers — the editor drew a torus of `0.22/0.45/24/12` where Godot documents
/// `0.5/1.0/64/32`, and the runner drew no torus at all.
///
/// Vector properties are addressed component-wise (`"size.x"`), matching the
/// keys the scene marker already encodes.
///
/// Returns `None` for a class this cannot generate, so a caller can fall back to
/// its own placeholder rather than be handed the wrong shape silently.
pub fn build_primitive(class_name: &str, authored: &dyn Fn(&str) -> Option<f32>) -> Option<Mesh3D> {
    let num = |key: &str, default: f32| authored(key).unwrap_or(default);
    // A negative or fractional count is malformed scene data; clamp rather than
    // wrap a negative float into a huge u32 and try to allocate it.
    let count = |key: &str, default: u32| {
        authored(key)
            .map(|v| if v < 1.0 { 1 } else { v as u32 })
            .unwrap_or(default)
    };
    // ⚠️ Like `count`, but permits 0. `count` clamps anything below 1 up to 1,
    // which is right for a segment count (0 segments is degenerate) and WRONG
    // for `subdivide_*`, where 0 is both legal and the default — an authored
    // `subdivide_width = 0` would otherwise split a quad nobody asked to split.
    let sub = |key: &str, default: u32| {
        authored(key)
            .map(|v| if v < 0.0 { 0 } else { v.round() as u32 })
            .unwrap_or(default)
    };

    let mesh = match class_name {
        "BoxMesh" | "CubeMesh" => {
            let d = BoxMesh::default();
            BoxMesh {
                size: Vector3::new(
                    num("size.x", d.size.x),
                    num("size.y", d.size.y),
                    num("size.z", d.size.z),
                ),
            }
            .generate()
        }
        "SphereMesh" => {
            let d = SphereMesh::default();
            SphereMesh {
                radius: num("radius", d.radius),
                height: num("height", d.height),
                radial_segments: count("radial_segments", d.radial_segments),
                rings: count("rings", d.rings),
            }
            .generate()
        }
        "CapsuleMesh" => {
            let d = CapsuleMesh::default();
            CapsuleMesh {
                radius: num("radius", d.radius),
                height: num("height", d.height),
                radial_segments: count("radial_segments", d.radial_segments),
                rings: count("rings", d.rings),
            }
            .generate()
        }
        "CylinderMesh" => {
            let d = CylinderMesh::default();
            CylinderMesh {
                top_radius: num("top_radius", d.top_radius),
                bottom_radius: num("bottom_radius", d.bottom_radius),
                height: num("height", d.height),
                radial_segments: count("radial_segments", d.radial_segments),
                rings: count("rings", d.rings),
            }
            .generate()
        }
        // 🔑 QuadMesh is NOT an alias for PlaneMesh (pat-s3rrv.198). It inherits
        // PlaneMesh and overrides `size` to Vector2(1, 1) and `orientation` to
        // FACE_Z (`QuadMesh.xml:14-15`), so sharing one set of defaults drew
        // every QuadMesh at double size in the wrong plane.
        //
        // The two still share this arm because they share a GENERATOR — what
        // differs is only which defaults they start from. Whatever the scene
        // authored then overrides either.
        "PlaneMesh" | "QuadMesh" => {
            let d = if class_name == "QuadMesh" {
                PlaneMesh::quad()
            } else {
                PlaneMesh::default()
            };
            PlaneMesh {
                size: [num("size.x", d.size[0]), num("size.y", d.size[1])],
                // `orientation` rides the same numeric bag as everything else,
                // so it arrives as an f32 and has to be rounded back.
                orientation: PlaneOrientation::from_int(
                    num("orientation", d.orientation.as_int() as f32).round() as i64,
                ),
                center_offset: Vector3::new(
                    num("center_offset.x", d.center_offset.x),
                    num("center_offset.y", d.center_offset.y),
                    num("center_offset.z", d.center_offset.z),
                ),
                subdivide_width: sub("subdivide_width", d.subdivide_width),
                subdivide_depth: sub("subdivide_depth", d.subdivide_depth),
            }
            .generate()
        }
        // RibbonTrailMesh's geometry lives in `extra_meshes` beside PrismMesh
        // and PointMesh (pat-s3rrv.198). `curve` is a Curve resource reference
        // rather than a number, so it cannot arrive through this numeric bag —
        // an authored curve is not yet honoured here, which yields an unscaled
        // ribbon rather than a wrong one.
        "RibbonTrailMesh" => {
            let d = crate::extra_meshes::RibbonTrailMesh::default();
            crate::extra_meshes::RibbonTrailMesh {
                size: num("size", d.size),
                sections: count("sections", d.sections),
                section_length: num("section_length", d.section_length),
                section_segments: count("section_segments", d.section_segments),
                shape: crate::extra_meshes::RibbonTrailShape::from_int(
                    num("shape", d.shape.as_int() as f32).round() as i64,
                ),
                curve_samples: None,
            }
            .generate()
        }
        "PrismMesh" => {
            let d = crate::extra_meshes::PrismMesh::default();
            crate::extra_meshes::PrismMesh {
                size: Vector3::new(
                    num("size.x", d.size.x),
                    num("size.y", d.size.y),
                    num("size.z", d.size.z),
                ),
                left_to_right: num("left_to_right", d.left_to_right),
            }
            .generate()
        }
        "TorusMesh" => {
            let d = TorusMesh::default();
            TorusMesh {
                inner_radius: num("inner_radius", d.inner_radius),
                outer_radius: num("outer_radius", d.outer_radius),
                rings: count("rings", d.rings),
                ring_segments: count("ring_segments", d.ring_segments),
            }
            .generate()
        }
        "TubeTrailMesh" => {
            let d = TubeTrailMesh::default();
            // Bools arrive as 0/1 through the numeric lookup, which is how the
            // scene marker encodes them.
            let flag =
                |key: &str, default: bool| authored(key).map(|v| v != 0.0).unwrap_or(default);
            TubeTrailMesh {
                radius: num("radius", d.radius),
                radial_steps: count("radial_steps", d.radial_steps),
                sections: count("sections", d.sections),
                section_length: num("section_length", d.section_length),
                section_rings: count("section_rings", d.section_rings),
                cap_top: flag("cap_top", d.cap_top),
                cap_bottom: flag("cap_bottom", d.cap_bottom),
            }
            .generate()
        }
        "PointMesh" => crate::extra_meshes::PointMesh.generate(),
        _ => return None,
    };
    Some(mesh)
}

/// Builds a primitive that may need string-valued properties.
///
/// `TextMesh` is the only subtype whose geometry depends on strings — the text
/// itself and the path to a font — and the numeric lookup [`build_primitive`]
/// takes cannot carry them. Rather than resolve TextMesh separately inside each
/// caller, which is precisely how the editor and the runtime diverged in the
/// first place (pat-s3rrv.199), this stays the single entry point and delegates
/// every other class straight through.
///
/// Returns `None` when there is nothing to draw, so a caller can fall back to
/// its own placeholder rather than receive an empty mesh it cannot distinguish
/// from a font that failed to load.
pub fn build_primitive_with_text(
    class_name: &str,
    authored: &dyn Fn(&str) -> Option<f32>,
    authored_str: &dyn Fn(&str) -> Option<String>,
) -> Option<Mesh3D> {
    if class_name != "TextMesh" {
        return build_primitive(class_name, authored);
    }

    let num = |key: &str, default: f32| authored(key).unwrap_or(default);
    let d = crate::text_mesh::TextMesh::default();

    // Empty text is a legitimate state, not an error — but it has no geometry.
    let text = authored_str("text").filter(|t| !t.is_empty())?;

    // ⚠️ Godot falls back to the project's default font here. This engine has
    // no default-font plumbing, so a TextMesh with no authored `font` has
    // nothing to outline. Returning None says that, where drawing a placeholder
    // box would claim the text rendered.
    let font_path = authored_str("font")?;
    let bytes = std::fs::read(&font_path).ok()?;

    let font_size = num("font_size", d.font_size as f32);
    let pixel_size = num("pixel_size", d.pixel_size);
    let curve_step = num("curve_step", gdresource::glyph_outline::DEFAULT_CURVE_STEP);

    let outline =
        gdresource::glyph_outline::text_outline(&bytes, &text, font_size, pixel_size, curve_step)?;
    if outline.contours.is_empty() {
        return None;
    }

    let mesh = crate::text_mesh::TextMesh {
        text,
        depth: num("depth", d.depth),
        pixel_size,
        font_size: font_size as i64,
    };
    Some(mesh.generate_from_contours(&outline.contours))
}
