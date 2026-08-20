//! Additional Godot `PrimitiveMesh` subtypes — `PrismMesh`, `PointMesh` and
//! `RibbonTrailMesh` (pat-nzgdu.131, pat-s3rrv.198).
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
//! # Scope / remaining work (pat-nzgdu.131, pat-s3rrv.198)
//!
//! `RibbonTrailMesh` landed here in pat-s3rrv.198, ported from
//! `RibbonTrailMesh::_create_mesh_array` including its per-row bone weights.
//! Still outstanding from the original bead: `TorusMesh` (its geometry already
//! exists as [`Mesh3D::torus`](crate::mesh::Mesh3D::torus)), `TextMesh`,
//! `TubeTrailMesh`, and registration of these as `PrimitiveMeshType` enum
//! variants. The `QuadMesh` orientation fix is pat-s3rrv.198's other half and
//! lives in [`primitive_mesh`], not here.
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

/// The cross-section shape of a [`RibbonTrailMesh`], matching Godot's
/// `RibbonTrailMesh.Shape` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RibbonTrailShape {
    /// A single flat ribbon lying in the XY plane (`SHAPE_FLAT`, value 0).
    Flat,
    /// Two ribbons crossed at right angles (`SHAPE_CROSS`, value 1).
    ///
    /// This is Godot's default, not `Flat` — a flat ribbon disappears when
    /// viewed edge-on, which is why the crossed pair is what you get out of the
    /// box.
    #[default]
    Cross,
}

impl RibbonTrailShape {
    /// The integer Godot serialises this shape as.
    pub fn as_int(self) -> i64 {
        match self {
            Self::Flat => 0,
            Self::Cross => 1,
        }
    }

    /// Reads a shape back from its serialised integer, defaulting to
    /// [`Cross`](Self::Cross) for anything out of range.
    pub fn from_int(value: i64) -> Self {
        match value {
            0 => Self::Flat,
            _ => Self::Cross,
        }
    }
}

/// A Godot-compatible `RibbonTrailMesh`: a vertical ribbon built from stacked
/// sections, used as the trail geometry behind a moving object.
///
/// Ported from `RibbonTrailMesh::_create_mesh_array`
/// (`scene/resources/3d/primitive_meshes.cpp`). The ribbon runs down the **Y
/// axis**, spanning `section_length * sections` in total and centred on the
/// origin, so the first row is at `+depth/2` and the last at `-depth/2`.
///
/// 🔑 The generated mesh is **rigged**: each row is weighted between bone
/// `j / section_segments` and the next one, which is what lets a Skeleton3D
/// whip the trail around. Those weights are ported rather than dropped, because
/// an unrigged trail mesh is a stick — the bones are the entire point of the
/// resource.
#[derive(Debug, Clone, PartialEq)]
pub struct RibbonTrailMesh {
    /// Width of the ribbon, before any `curve` scaling.
    pub size: f32,
    /// Number of sections down the trail; also the bone count.
    pub sections: u32,
    /// Length of each section along Y.
    pub section_length: f32,
    /// Subdivisions within each section.
    pub section_segments: u32,
    /// Cross-section shape.
    pub shape: RibbonTrailShape,
    /// Godot's `curve` resource, pre-baked into evenly spaced width multipliers
    /// over `v` in `0..=1`.
    ///
    /// ⚠️ Baked samples rather than a `Curve` handle on purpose: `Curve` is a
    /// scene-layer resource and `gdserver3d` sits below that layer, so taking
    /// one here would invert the dependency. The caller that owns the resource
    /// bakes it; `None` means an unscaled ribbon, which is what Godot draws for
    /// an unset or empty curve.
    pub curve_samples: Option<Vec<f32>>,
}

impl Default for RibbonTrailMesh {
    fn default() -> Self {
        Self {
            size: 1.0,
            sections: 5,
            section_length: 0.2,
            section_segments: 3,
            shape: RibbonTrailShape::Cross,
            curve_samples: None,
        }
    }
}

impl RibbonTrailMesh {
    /// Samples the baked curve at `v` (`0..=1`), returning `1.0` when there is
    /// no curve. Linear between samples, matching `Curve::sample_baked`'s own
    /// interpolation between baked points.
    fn curve_at(&self, v: f32) -> f32 {
        let Some(samples) = self.curve_samples.as_ref() else {
            return 1.0;
        };
        if samples.is_empty() {
            return 1.0;
        }
        if samples.len() == 1 {
            return samples[0];
        }
        let t = v.clamp(0.0, 1.0) * (samples.len() - 1) as f32;
        let i = t.floor() as usize;
        // ⚠️ `v == 1.0` puts `i` exactly on the last index, where `i + 1` would
        // be out of bounds. Clamping the base index (rather than the fraction)
        // keeps the endpoint exact instead of nudging it off the last sample.
        let i = i.min(samples.len() - 2);
        let frac = t - i as f32;
        samples[i] + (samples[i + 1] - samples[i]) * frac
    }

    /// Generates the ribbon geometry.
    ///
    /// Returns an empty mesh when `sections` or `section_segments` is zero —
    /// the row loop would produce a single degenerate row with no triangles,
    /// and Godot's own `set_sections` refuses values below 1.
    pub fn generate(&self) -> Mesh3D {
        if self.sections == 0 || self.section_segments == 0 {
            return Mesh3D::default();
        }

        let total_segments = self.section_segments * self.sections;
        let depth = self.section_length * self.sections as f32;
        let crossed = self.shape == RibbonTrailShape::Cross;
        let per_row: u32 = if crossed { 4 } else { 2 };

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        let mut bone_indices = Vec::new();
        let mut bone_weights = Vec::new();

        for j in 0..=total_segments {
            let v = j as f32 / total_segments as f32;
            // Rows run from +depth/2 down to -depth/2, so the trail hangs below
            // its origin the way Godot's does.
            let y = depth * 0.5 - depth * v;

            let bone = j / self.section_segments;
            let blend = 1.0 - (j % self.section_segments) as f32 / self.section_segments as f32;
            let s = self.size * self.curve_at(v);

            vertices.push(Vector3::new(-s * 0.5, y, 0.0));
            vertices.push(Vector3::new(s * 0.5, y, 0.0));
            normals.push(Vector3::new(0.0, 0.0, 1.0));
            normals.push(Vector3::new(0.0, 0.0, 1.0));
            uvs.push([0.0, v]);
            uvs.push([1.0, v]);
            if crossed {
                vertices.push(Vector3::new(0.0, y, -s * 0.5));
                vertices.push(Vector3::new(0.0, y, s * 0.5));
                normals.push(Vector3::new(1.0, 0.0, 0.0));
                normals.push(Vector3::new(1.0, 0.0, 0.0));
                uvs.push([0.0, v]);
                uvs.push([1.0, v]);
            }

            // Every vertex in the row shares the row's two-bone blend.
            for _ in 0..per_row {
                bone_indices.push([bone, self.sections.min(bone + 1), 0, 0]);
                bone_weights.push([blend, 1.0 - blend, 0.0, 0.0]);
            }

            if j > 0 {
                if crossed {
                    let base = j * 4 - 4;
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 4,
                        base + 1,
                        base + 5,
                        base + 4,
                        base + 2,
                        base + 3,
                        base + 6,
                        base + 3,
                        base + 7,
                        base + 6,
                    ]);
                } else {
                    let base = j * 2 - 2;
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base + 1,
                        base + 3,
                        base + 2,
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
            bone_indices,
            bone_weights,
            ..Default::default()
        }
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

    // ── RibbonTrailMesh (pat-s3rrv.198) ──────────────────────────────────────

    #[test]
    fn ribbon_trail_default_is_a_crossed_ribbon_with_godot_counts() {
        let m = RibbonTrailMesh::default();
        // Godot's documented defaults, transcribed from RibbonTrailMesh.xml
        // rather than guessed: shape is CROSS (1), not FLAT.
        assert_eq!(m.sections, 5);
        assert_eq!(m.section_segments, 3);
        assert!(approx(m.section_length, 0.2));
        assert!(approx(m.size, 1.0));
        assert_eq!(m.shape, RibbonTrailShape::Cross);
        assert_eq!(m.shape.as_int(), 1);

        let mesh = m.generate();
        // 15 segments -> 16 rows, 4 vertices each when crossed.
        assert_eq!(mesh.vertices.len(), 16 * 4);
        assert_eq!(mesh.normals.len(), 16 * 4);
        assert_eq!(mesh.uvs.len(), 16 * 4);
        // 4 triangles per segment for the crossed pair.
        assert_eq!(mesh.indices.len(), 15 * 12);
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn ribbon_trail_flat_shape_halves_the_geometry() {
        let mesh = RibbonTrailMesh {
            shape: RibbonTrailShape::Flat,
            ..Default::default()
        }
        .generate();
        assert_eq!(mesh.vertices.len(), 16 * 2);
        assert_eq!(mesh.indices.len(), 15 * 6);
        // A flat ribbon lies in XY: nothing is displaced along Z.
        assert!(mesh.vertices.iter().all(|v| approx(v.z, 0.0)));
    }

    #[test]
    fn ribbon_trail_indices_are_in_range_for_both_shapes() {
        for shape in [RibbonTrailShape::Flat, RibbonTrailShape::Cross] {
            let mesh = RibbonTrailMesh {
                shape,
                ..Default::default()
            }
            .generate();
            let n = mesh.vertices.len() as u32;
            assert!(
                mesh.indices.iter().all(|&i| i < n),
                "{shape:?} indexes past its {n} vertices"
            );
        }
    }

    #[test]
    fn ribbon_trail_spans_its_full_depth_centred_on_the_origin() {
        let mesh = RibbonTrailMesh::default().generate();
        // depth = section_length * sections = 0.2 * 5 = 1.0, centred: +0.5..-0.5.
        let ys: Vec<f32> = mesh.vertices.iter().map(|v| v.y).collect();
        let max = ys.iter().cloned().fold(f32::MIN, f32::max);
        let min = ys.iter().cloned().fold(f32::MAX, f32::min);
        assert!(approx(max, 0.5), "top row at {max}");
        assert!(approx(min, -0.5), "bottom row at {min}");
    }

    #[test]
    fn ribbon_trail_is_rigged_with_a_two_bone_blend_per_row() {
        let mesh = RibbonTrailMesh::default().generate();
        // 🔑 The weights are the point of the resource — an unrigged trail cannot
        // whip. One entry per vertex, and each blend must sum to 1.
        assert_eq!(mesh.bone_weights.len(), mesh.vertices.len());
        assert_eq!(mesh.bone_indices.len(), mesh.vertices.len());
        for w in &mesh.bone_weights {
            assert!(approx(w[0] + w[1], 1.0), "weights {w:?} do not sum to 1");
        }
        // The first row sits entirely on bone 0; the last cannot exceed `sections`.
        assert_eq!(mesh.bone_indices[0][0], 0);
        assert!(mesh.bone_indices.iter().all(|b| b[0] <= 5 && b[1] <= 5));
    }

    #[test]
    fn ribbon_trail_curve_scales_width_and_absence_of_one_does_not() {
        let flat = RibbonTrailMesh {
            shape: RibbonTrailShape::Flat,
            ..Default::default()
        };
        let unscaled = flat.generate();
        // Every row is `size` wide without a curve.
        assert!(approx(unscaled.vertices[0].x, -0.5));
        assert!(approx(unscaled.vertices[1].x, 0.5));

        // A curve tapering 1 -> 0 leaves the first row full width and collapses
        // the last one.
        let tapered = RibbonTrailMesh {
            shape: RibbonTrailShape::Flat,
            curve_samples: Some(vec![1.0, 0.0]),
            ..Default::default()
        }
        .generate();
        assert!(approx(tapered.vertices[0].x, -0.5), "first row tapered");
        let last = tapered.vertices.len() - 1;
        assert!(
            approx(tapered.vertices[last].x, 0.0),
            "last row should collapse, got {}",
            tapered.vertices[last].x
        );
        // ⚠️ The v == 1.0 endpoint is the index that would run off the sample
        // table; it must land exactly on the last sample, not one short of it.
        assert!(approx(tapered.vertices[last - 1].x, 0.0));
    }

    #[test]
    fn ribbon_trail_shape_round_trips_through_its_int() {
        assert_eq!(RibbonTrailShape::from_int(0), RibbonTrailShape::Flat);
        assert_eq!(RibbonTrailShape::from_int(1), RibbonTrailShape::Cross);
        // Out-of-range falls back to Godot's default rather than to Flat.
        assert_eq!(RibbonTrailShape::from_int(7), RibbonTrailShape::Cross);
        assert_eq!(RibbonTrailShape::Flat.as_int(), 0);
    }

    #[test]
    fn ribbon_trail_with_zero_sections_is_empty_not_degenerate() {
        let mesh = RibbonTrailMesh {
            sections: 0,
            ..Default::default()
        }
        .generate();
        assert!(mesh.vertices.is_empty());
        assert!(mesh.indices.is_empty());
    }
}
