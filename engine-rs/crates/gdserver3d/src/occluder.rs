//! OccluderInstance3D: occlusion culling shapes for the 3D rendering server.
//!
//! Mirrors Godot's `OccluderInstance3D` node and `Occluder3D` resource.
//! Supports box, sphere, quad, and polygon occluder shapes. The rendering
//! server uses these to cull instances that are fully hidden behind occluders.

use gdcore::math::{Vector2, Vector3};
use gdcore::math3d::{Aabb, Transform3D};

// ---------------------------------------------------------------------------
// Occluder3D shapes
// ---------------------------------------------------------------------------

/// An occluder shape resource, matching Godot's `Occluder3D` subtypes.
#[derive(Debug, Clone, PartialEq)]
pub enum Occluder3D {
    /// A box occluder defined by half-extents.
    /// Matches Godot's `BoxOccluder3D`.
    Box { size: Vector3 },
    /// A sphere occluder defined by radius.
    /// Matches Godot's `SphereOccluder3D`.
    Sphere { radius: f32 },
    /// A single quad (flat rectangle) occluder defined by size.
    /// Matches Godot's `QuadOccluder3D`.
    Quad { size: Vector2 },
    /// A polygon occluder defined by a set of 2D vertices (in local XY plane).
    /// Matches Godot's `PolygonOccluder3D`.
    Polygon { vertices: Vec<Vector2> },
    /// An occluder defined by an arbitrary triangle mesh — a 3D vertex array
    /// plus a triangle index array (three indices per triangle). Matches
    /// Godot's `ArrayOccluder3D`.
    Array {
        /// Local-space mesh vertices.
        vertices: Vec<Vector3>,
        /// Triangle indices into `vertices` (three per triangle).
        indices: Vec<u32>,
    },
}

impl Default for Occluder3D {
    fn default() -> Self {
        Self::Box {
            size: Vector3::new(1.0, 1.0, 1.0),
        }
    }
}

impl Occluder3D {
    /// Creates a box occluder with the given full size (not half-extents).
    pub fn new_box(size: Vector3) -> Self {
        Self::Box { size }
    }

    /// Creates a sphere occluder with the given radius.
    pub fn new_sphere(radius: f32) -> Self {
        Self::Sphere { radius }
    }

    /// Creates a quad occluder with the given size (width, height).
    pub fn new_quad(size: Vector2) -> Self {
        Self::Quad { size }
    }

    /// Creates a polygon occluder from 2D vertices in the local XY plane.
    pub fn new_polygon(vertices: Vec<Vector2>) -> Self {
        Self::Polygon { vertices }
    }

    /// Creates an array occluder from a triangle mesh: `vertices` in local space
    /// and `indices` (three per triangle) into that array (Godot's
    /// `ArrayOccluder3D::set_arrays`).
    pub fn new_array(vertices: Vec<Vector3>, indices: Vec<u32>) -> Self {
        Self::Array { vertices, indices }
    }

    /// The triangles of an [`Occluder3D::Array`] as `[v0, v1, v2]` vertex
    /// triples resolved from its index array. Returns an empty vec for
    /// non-array occluders, and skips any triangle whose indices fall outside
    /// the vertex array.
    pub fn triangles(&self) -> Vec<[Vector3; 3]> {
        match self {
            Self::Array { vertices, indices } => indices
                .chunks_exact(3)
                .filter_map(|tri| {
                    Some([
                        *vertices.get(tri[0] as usize)?,
                        *vertices.get(tri[1] as usize)?,
                        *vertices.get(tri[2] as usize)?,
                    ])
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Returns the local-space AABB of this occluder shape.
    pub fn get_aabb(&self) -> Aabb {
        match self {
            Self::Box { size } => {
                let half = Vector3::new(size.x * 0.5, size.y * 0.5, size.z * 0.5);
                Aabb::new(Vector3::new(-half.x, -half.y, -half.z), *size)
            }
            Self::Sphere { radius } => {
                let r = *radius;
                Aabb::new(
                    Vector3::new(-r, -r, -r),
                    Vector3::new(r * 2.0, r * 2.0, r * 2.0),
                )
            }
            Self::Quad { size } => {
                let hw = size.x * 0.5;
                let hh = size.y * 0.5;
                Aabb::new(
                    Vector3::new(-hw, -hh, 0.0),
                    Vector3::new(size.x, size.y, 0.001),
                )
            }
            Self::Polygon { vertices } => {
                if vertices.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;
                for v in vertices {
                    min_x = min_x.min(v.x);
                    min_y = min_y.min(v.y);
                    max_x = max_x.max(v.x);
                    max_y = max_y.max(v.y);
                }
                Aabb::new(
                    Vector3::new(min_x, min_y, 0.0),
                    Vector3::new(max_x - min_x, max_y - min_y, 0.001),
                )
            }
            Self::Array { vertices, .. } => {
                if vertices.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let mut min = vertices[0];
                let mut max = vertices[0];
                for v in vertices {
                    min = Vector3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z));
                    max = Vector3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z));
                }
                Aabb::new(min, max - min)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// OccluderInstance3D
// ---------------------------------------------------------------------------

/// A 3D occluder instance placed in the scene for occlusion culling.
///
/// Maps to Godot's `OccluderInstance3D` node. Holds an `Occluder3D` shape
/// resource and a world-space transform. The rendering server tests whether
/// render instances are fully behind occluders to skip drawing them.
#[derive(Debug, Clone)]
pub struct OccluderInstance3D {
    /// The occluder shape resource.
    pub occluder: Option<Occluder3D>,
    /// World-space transform of this occluder.
    pub transform: Transform3D,
    /// Which layers this occluder is baked from.
    pub bake_mask: u32,
    /// Bake simplification distance (Godot default 0.1).
    pub bake_simplification_distance: f32,
    /// Whether this occluder takes part in culling (pat-nzgdu.212).
    ///
    /// ⚠️ A hidden occluder occludes NOTHING, and that is Godot's behaviour, not
    /// a convenience: `OccluderInstance3D` is a `VisualInstance3D`, and hiding
    /// one reaches `instance_set_visible`, which for `INSTANCE_OCCLUDER` calls
    /// `scenario_set_instance(..., p_visible)` and drops it from the occlusion
    /// scenario (`renderer_scene_cull.cpp:1075-1079`).
    ///
    /// `visible` itself is `Node3D`'s property, not one of this class's three
    /// own members — ClassDB inherits it rather than re-declaring it.
    pub visible: bool,
}

impl Default for OccluderInstance3D {
    fn default() -> Self {
        Self {
            occluder: None,
            transform: Transform3D::IDENTITY,
            bake_mask: 0xFFFF_FFFF,
            bake_simplification_distance: 0.1,
            // Nodes are visible by default; a default-constructed occluder that
            // silently did nothing would be a trap.
            visible: true,
        }
    }
}

impl OccluderInstance3D {
    /// Creates a new occluder instance with no shape.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new occluder instance with the given shape.
    pub fn with_occluder(occluder: Occluder3D) -> Self {
        Self {
            occluder: Some(occluder),
            ..Self::default()
        }
    }

    /// Returns the world-space AABB of the occluder, or `None` if no shape.
    pub fn get_world_aabb(&self) -> Option<Aabb> {
        let shape = self.occluder.as_ref()?;
        let local_aabb = shape.get_aabb();
        Some(transform_aabb(&local_aabb, &self.transform))
    }

    /// Tests whether the given world-space AABB is fully occluded by this
    /// occluder from the given camera position.
    ///
    /// Uses a conservative AABB-behind-AABB test: the target is considered
    /// occluded if the occluder's AABB fully covers the target's projection
    /// from the camera's point of view, and the target is farther from the
    /// camera than the occluder.
    pub fn occludes(&self, camera_pos: Vector3, target_aabb: Aabb) -> bool {
        // ⚠️ A hidden occluder occludes nothing (pat-nzgdu.212). Godot reaches
        // the same result by dropping the instance from the occlusion scenario
        // (`renderer_scene_cull.cpp:1075`); gating here means no caller can
        // consult a hidden occluder by forgetting to filter first.
        if !self.visible {
            return false;
        }
        let occluder_aabb = match self.get_world_aabb() {
            Some(aabb) => aabb,
            None => return false,
        };

        // The target is hidden only if EVERY corner of its box is hidden: the
        // segment from the camera to that corner must enter the occluder before
        // it arrives. Any corner the camera can see means the target is visible.
        //
        // ⚠️ This must stay a line-of-sight test (pat-nzgdu.211). The previous
        // version compared the two boxes' distances and angular extents, which
        // discarded the *directions* — so a big, near occluder culled a small,
        // far target no matter where each one sat. An occluder off to one side
        // erased geometry dead ahead of the camera. Culling is an optimisation:
        // a wrong cull deletes what the user should see, so every uncertain case
        // answers `false` here.
        for corner in aabb_corners(&target_aabb) {
            let to_corner = corner - camera_pos;
            let distance = to_corner.length();
            // The camera is inside (or on) the target — nothing to cull.
            if distance < 1e-6 {
                return false;
            }
            match ray_aabb_entry(camera_pos, to_corner / distance, &occluder_aabb) {
                // Enters the occluder before reaching the corner: blocked.
                Some(entry) if entry < distance => {}
                _ => return false,
            }
        }
        true
    }

    /// Returns whether a given layer bit (1-based) is set in the bake mask.
    pub fn get_bake_mask_value(&self, layer: u32) -> bool {
        if layer == 0 || layer > 32 {
            return false;
        }
        (self.bake_mask >> (layer - 1)) & 1 != 0
    }

    /// Sets or clears a specific layer bit (1-based) in the bake mask.
    pub fn set_bake_mask_value(&mut self, layer: u32, value: bool) {
        if layer == 0 || layer > 32 {
            return;
        }
        if value {
            self.bake_mask |= 1 << (layer - 1);
        } else {
            self.bake_mask &= !(1 << (layer - 1));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The eight corners of an AABB (pat-nzgdu.211).
fn aabb_corners(aabb: &Aabb) -> [Vector3; 8] {
    let p = aabb.position;
    let s = aabb.size;
    [
        Vector3::new(p.x, p.y, p.z),
        Vector3::new(p.x + s.x, p.y, p.z),
        Vector3::new(p.x, p.y + s.y, p.z),
        Vector3::new(p.x, p.y, p.z + s.z),
        Vector3::new(p.x + s.x, p.y + s.y, p.z),
        Vector3::new(p.x + s.x, p.y, p.z + s.z),
        Vector3::new(p.x, p.y + s.y, p.z + s.z),
        Vector3::new(p.x + s.x, p.y + s.y, p.z + s.z),
    ]
}

/// Distance along unit `dir` at which a ray from `origin` enters `aabb`, or
/// `None` if it misses (pat-nzgdu.211).
///
/// The standard slab test. ⚠️ An axis where the ray is parallel to the slab
/// (`dir` component ~0) contributes no bound — it can only reject, when the
/// origin already sits outside that slab. Treating it as a division would give
/// an infinity that poisons the interval.
///
/// A ray starting inside the box returns 0.0 (`t_enter` clamped), not a negative
/// distance; a box entirely behind the origin returns `None`.
fn ray_aabb_entry(origin: Vector3, dir: Vector3, aabb: &Aabb) -> Option<f32> {
    let min = aabb.position;
    let max = aabb.position + aabb.size;
    let mut t_enter = f32::NEG_INFINITY;
    let mut t_exit = f32::INFINITY;

    for (o, d, lo, hi) in [
        (origin.x, dir.x, min.x, max.x),
        (origin.y, dir.y, min.y, max.y),
        (origin.z, dir.z, min.z, max.z),
    ] {
        if d.abs() < 1e-9 {
            if o < lo || o > hi {
                return None;
            }
        } else {
            let (mut t0, mut t1) = ((lo - o) / d, (hi - o) / d);
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_enter = t_enter.max(t0);
            t_exit = t_exit.min(t1);
            if t_enter > t_exit {
                return None;
            }
        }
    }

    if t_exit < 0.0 {
        return None;
    }
    Some(t_enter.max(0.0))
}

/// Returns the center of an AABB.
#[allow(dead_code)]
fn aabb_center(aabb: &Aabb) -> Vector3 {
    Vector3::new(
        aabb.position.x + aabb.size.x * 0.5,
        aabb.position.y + aabb.size.y * 0.5,
        aabb.position.z + aabb.size.z * 0.5,
    )
}

/// Returns the half-extent of an AABB.
#[allow(dead_code)]
fn aabb_half_extent(aabb: &Aabb) -> Vector3 {
    Vector3::new(aabb.size.x * 0.5, aabb.size.y * 0.5, aabb.size.z * 0.5)
}

/// Transforms a local-space AABB to world space (conservative, axis-aligned).
///
/// `pub(crate)` for `Instance3D::world_aabb` (pat-nzgdu.211), which must produce
/// its target box the *same* way the occluder produces its own — a cull that
/// compared boxes built by two different transforms would be comparing two
/// different notions of "world space".
pub(crate) fn transform_aabb(aabb: &Aabb, t: &Transform3D) -> Aabb {
    // Transform all 8 corners and find the enclosing AABB.
    let corners = [
        Vector3::new(aabb.position.x, aabb.position.y, aabb.position.z),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y,
            aabb.position.z,
        ),
        Vector3::new(
            aabb.position.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z,
        ),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z,
        ),
        Vector3::new(
            aabb.position.x,
            aabb.position.y,
            aabb.position.z + aabb.size.z,
        ),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y,
            aabb.position.z + aabb.size.z,
        ),
        Vector3::new(
            aabb.position.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z + aabb.size.z,
        ),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z + aabb.size.z,
        ),
    ];

    let first = t.xform(corners[0]);
    let mut min = first;
    let mut max = first;

    for &c in &corners[1..] {
        let w = t.xform(c);
        min.x = min.x.min(w.x);
        min.y = min.y.min(w.y);
        min.z = min.z.min(w.z);
        max.x = max.x.max(w.x);
        max.y = max.y.max(w.y);
        max.z = max.z.max(w.z);
    }

    Aabb::new(
        min,
        Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // -- Occluder3D shapes --

    #[test]
    fn box_occluder_aabb() {
        let occ = Occluder3D::new_box(Vector3::new(2.0, 4.0, 6.0));
        let aabb = occ.get_aabb();
        assert!(approx_eq(aabb.position.x, -1.0));
        assert!(approx_eq(aabb.position.y, -2.0));
        assert!(approx_eq(aabb.position.z, -3.0));
        assert!(approx_eq(aabb.size.x, 2.0));
        assert!(approx_eq(aabb.size.y, 4.0));
        assert!(approx_eq(aabb.size.z, 6.0));
    }

    #[test]
    fn sphere_occluder_aabb() {
        let occ = Occluder3D::new_sphere(5.0);
        let aabb = occ.get_aabb();
        assert!(approx_eq(aabb.position.x, -5.0));
        assert!(approx_eq(aabb.size.x, 10.0));
        assert!(approx_eq(aabb.size.y, 10.0));
    }

    #[test]
    fn quad_occluder_aabb() {
        let occ = Occluder3D::new_quad(Vector2::new(4.0, 3.0));
        let aabb = occ.get_aabb();
        assert!(approx_eq(aabb.position.x, -2.0));
        assert!(approx_eq(aabb.position.y, -1.5));
        assert!(approx_eq(aabb.size.x, 4.0));
        assert!(approx_eq(aabb.size.y, 3.0));
    }

    #[test]
    fn polygon_occluder_aabb() {
        let occ = Occluder3D::new_polygon(vec![
            Vector2::new(-1.0, -2.0),
            Vector2::new(3.0, -2.0),
            Vector2::new(3.0, 2.0),
            Vector2::new(-1.0, 2.0),
        ]);
        let aabb = occ.get_aabb();
        assert!(approx_eq(aabb.position.x, -1.0));
        assert!(approx_eq(aabb.position.y, -2.0));
        assert!(approx_eq(aabb.size.x, 4.0));
        assert!(approx_eq(aabb.size.y, 4.0));
    }

    #[test]
    fn empty_polygon_aabb() {
        let occ = Occluder3D::new_polygon(vec![]);
        let aabb = occ.get_aabb();
        assert!(approx_eq(aabb.size.x, 0.0));
    }

    #[test]
    fn default_occluder_is_box() {
        let occ = Occluder3D::default();
        match occ {
            Occluder3D::Box { size } => {
                assert!(approx_eq(size.x, 1.0));
                assert!(approx_eq(size.y, 1.0));
                assert!(approx_eq(size.z, 1.0));
            }
            _ => panic!("Default occluder should be Box"),
        }
    }

    #[test]
    fn array_occluder_aabb_and_triangles() {
        let verts = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(0.0, 3.0, 1.0),
        ];
        let occ = Occluder3D::new_array(verts.clone(), vec![0, 1, 2]);

        // AABB encloses every vertex (including the +Z depth).
        let aabb = occ.get_aabb();
        assert!(approx_eq(aabb.position.x, 0.0));
        assert!(approx_eq(aabb.position.y, 0.0));
        assert!(approx_eq(aabb.position.z, 0.0));
        assert!(approx_eq(aabb.size.x, 2.0));
        assert!(approx_eq(aabb.size.y, 3.0));
        assert!(approx_eq(aabb.size.z, 1.0));

        // triangles() resolves the index triple in order.
        let tris = occ.triangles();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0], [verts[0], verts[1], verts[2]]);

        // An out-of-range index skips that triangle; non-array occluders have none.
        let bad = Occluder3D::new_array(verts, vec![0, 1, 9]);
        assert!(bad.triangles().is_empty());
        assert!(Occluder3D::new_box(Vector3::new(1.0, 1.0, 1.0))
            .triangles()
            .is_empty());
    }

    // -- OccluderInstance3D --

    #[test]
    fn instance_default() {
        let inst = OccluderInstance3D::new();
        assert!(inst.occluder.is_none());
        assert_eq!(inst.bake_mask, 0xFFFF_FFFF);
        assert!(approx_eq(inst.bake_simplification_distance, 0.1));
    }

    #[test]
    fn instance_with_occluder() {
        let inst = OccluderInstance3D::with_occluder(Occluder3D::new_sphere(3.0));
        assert!(inst.occluder.is_some());
    }

    #[test]
    fn world_aabb_none_without_shape() {
        let inst = OccluderInstance3D::new();
        assert!(inst.get_world_aabb().is_none());
    }

    #[test]
    fn world_aabb_identity_transform() {
        let inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(2.0, 2.0, 2.0)));
        let aabb = inst.get_world_aabb().unwrap();
        assert!(approx_eq(aabb.position.x, -1.0));
        assert!(approx_eq(aabb.size.x, 2.0));
    }

    #[test]
    fn world_aabb_translated() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(2.0, 2.0, 2.0)));
        inst.transform.origin = Vector3::new(10.0, 0.0, 0.0);
        let aabb = inst.get_world_aabb().unwrap();
        assert!(approx_eq(aabb.position.x, 9.0));
        assert!(approx_eq(aabb.size.x, 2.0));
    }

    // -- Occlusion test --

    #[test]
    fn large_occluder_occludes_small_target_behind() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(20.0, 20.0, 1.0)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -5.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -15.0), Vector3::new(1.0, 1.0, 1.0));

        assert!(inst.occludes(camera_pos, target));
    }

    #[test]
    fn small_occluder_does_not_occlude_large_target() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(0.5, 0.5, 0.5)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -5.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(
            Vector3::new(-10.0, -10.0, -15.0),
            Vector3::new(20.0, 20.0, 1.0),
        );

        assert!(!inst.occludes(camera_pos, target));
    }

    #[test]
    fn target_in_front_of_occluder_not_occluded() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(20.0, 20.0, 1.0)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -10.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -3.0), Vector3::new(1.0, 1.0, 1.0));

        assert!(!inst.occludes(camera_pos, target));
    }

    #[test]
    fn no_shape_does_not_occlude() {
        let inst = OccluderInstance3D::new();
        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -10.0), Vector3::new(1.0, 1.0, 1.0));
        assert!(!inst.occludes(camera_pos, target));
    }

    /// ⚠️ A HIDDEN occluder occludes nothing (pat-nzgdu.212) — Godot drops it
    /// from the occlusion scenario on `instance_set_visible`
    /// (`renderer_scene_cull.cpp:1075-1079`).
    #[test]
    fn a_hidden_occluder_does_not_occlude() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(20.0, 20.0, 1.0)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -5.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -15.0), Vector3::new(1.0, 1.0, 1.0));

        // Visible by default, and squarely in the way: it occludes.
        assert!(
            inst.occludes(camera_pos, target),
            "baseline: this occluder does occlude while visible"
        );

        inst.visible = false;
        assert!(
            !inst.occludes(camera_pos, target),
            "a hidden occluder must not occlude"
        );

        // ...and showing it again restores culling: visibility is a gate, not a
        // one-way teardown.
        inst.visible = true;
        assert!(inst.occludes(camera_pos, target));
    }

    /// Occluders are visible by default — a fresh one must work without anyone
    /// setting a flag.
    #[test]
    fn occluders_are_visible_by_default() {
        assert!(OccluderInstance3D::new().visible);
        assert!(OccluderInstance3D::with_occluder(Occluder3D::default()).visible);
    }

    /// ⚠️ An occluder OFF TO THE SIDE must not occlude — occlusion is about the
    /// line of sight, not about being nearer and bigger (pat-nzgdu.211).
    ///
    /// This is exactly what the original test got wrong: it built the
    /// camera→occluder and camera→target vectors and then used only their
    /// *lengths*, so direction never entered the comparison. Every earlier test
    /// placed the occluder on the camera's axis, which is the one arrangement
    /// where "nearer and bigger" happens to coincide with "in the way".
    #[test]
    fn an_occluder_beside_the_line_of_sight_does_not_occlude() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(3.0, 3.0, 3.0)));
        inst.transform.origin = Vector3::new(5.0, 0.0, -1.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        // Dead ahead, farther away and smaller — nearer-and-bigger alone would
        // have culled it.
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -8.5), Vector3::new(1.0, 1.0, 1.0));

        assert!(
            !inst.occludes(camera_pos, target),
            "an occluder 78 degrees off-axis must not occlude geometry in front of the camera"
        );
    }

    /// ⚠️ A FLAT occluder must still occlude. A quad's AABB is 0.001 thick, and
    /// the original per-axis depth comparison made `QuadOccluder3D` and
    /// `PolygonOccluder3D` incapable of ever culling (pat-nzgdu.211).
    #[test]
    fn a_flat_quad_occludes_what_is_behind_it() {
        let mut inst = OccluderInstance3D::with_occluder(Occluder3D::Quad {
            size: Vector2::new(10.0, 10.0),
        });
        inst.transform.origin = Vector3::new(0.0, 0.0, -2.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -5.5), Vector3::new(1.0, 1.0, 1.0));

        assert!(
            inst.occludes(camera_pos, target),
            "a flat quad squarely between camera and target must occlude it"
        );
    }

    /// Partial cover is not cover: if any corner of the target is visible, the
    /// target is visible. Fails open, by design.
    #[test]
    fn an_occluder_covering_only_part_of_the_target_does_not_occlude() {
        let mut inst =
            OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(1.0, 20.0, 1.0)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -2.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        // A wide target: the narrow occluder hides its middle, not its ends.
        let target = Aabb::new(
            Vector3::new(-10.0, -0.5, -5.5),
            Vector3::new(20.0, 1.0, 1.0),
        );

        assert!(
            !inst.occludes(camera_pos, target),
            "a narrow occluder must not cull a target whose edges are visible past it"
        );
    }

    // -- Bake mask --

    #[test]
    fn bake_mask_default_all_set() {
        let inst = OccluderInstance3D::new();
        assert!(inst.get_bake_mask_value(1));
        assert!(inst.get_bake_mask_value(32));
    }

    #[test]
    fn bake_mask_layer_access() {
        let mut inst = OccluderInstance3D::new();
        inst.set_bake_mask_value(5, false);
        assert!(!inst.get_bake_mask_value(5));
        assert!(inst.get_bake_mask_value(4));
        assert!(inst.get_bake_mask_value(6));

        inst.set_bake_mask_value(5, true);
        assert!(inst.get_bake_mask_value(5));
    }

    #[test]
    fn bake_mask_out_of_range() {
        let inst = OccluderInstance3D::new();
        assert!(!inst.get_bake_mask_value(0));
        assert!(!inst.get_bake_mask_value(33));
    }

    // -- transform_aabb --

    #[test]
    fn transform_aabb_identity() {
        let aabb = Aabb::new(Vector3::new(-1.0, -1.0, -1.0), Vector3::new(2.0, 2.0, 2.0));
        let result = transform_aabb(&aabb, &Transform3D::IDENTITY);
        assert!(approx_eq(result.position.x, -1.0));
        assert!(approx_eq(result.size.x, 2.0));
    }

    #[test]
    fn transform_aabb_translation() {
        let aabb = Aabb::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        let mut t = Transform3D::IDENTITY;
        t.origin = Vector3::new(5.0, 0.0, 0.0);
        let result = transform_aabb(&aabb, &t);
        assert!(approx_eq(result.position.x, 5.0));
        assert!(approx_eq(result.size.x, 1.0));
    }
}
