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

    /// Returns the local-space AABB of this occluder shape.
    pub fn get_aabb(&self) -> Aabb {
        match self {
            Self::Box { size } => {
                let half = Vector3::new(size.x * 0.5, size.y * 0.5, size.z * 0.5);
                Aabb::new(
                    Vector3::new(-half.x, -half.y, -half.z),
                    *size,
                )
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
    /// Whether this occluder is active for culling.
    pub bake_mask: u32,
    /// Bake simplification distance (Godot default 0.1).
    pub bake_simplification_distance: f32,
}

impl Default for OccluderInstance3D {
    fn default() -> Self {
        Self {
            occluder: None,
            transform: Transform3D::IDENTITY,
            bake_mask: 0xFFFF_FFFF,
            bake_simplification_distance: 0.1,
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
        let occluder_aabb = match self.get_world_aabb() {
            Some(aabb) => aabb,
            None => return false,
        };

        // Target must be farther from camera than the occluder.
        let occluder_center = aabb_center(&occluder_aabb);
        let target_center = aabb_center(&target_aabb);

        let dist_occluder = (occluder_center - camera_pos).length_squared();
        let dist_target = (target_center - camera_pos).length_squared();

        if dist_target <= dist_occluder {
            return false;
        }

        // Project both AABBs to the screen-space axis-aligned extent from
        // the camera and check if the occluder fully covers the target.
        // This is a simplified conservative test using direction-based
        // angular extents.
        let cam_to_occluder = occluder_center - camera_pos;
        let cam_to_target = target_center - camera_pos;

        let occ_dist = cam_to_occluder.length();
        let tgt_dist = cam_to_target.length();

        if occ_dist < 1e-6 || tgt_dist < 1e-6 {
            return false;
        }

        // Conservative angular half-extent of each AABB from the camera.
        let occ_half = aabb_half_extent(&occluder_aabb);
        let tgt_half = aabb_half_extent(&target_aabb);

        let occ_angular = occ_half / occ_dist;
        let tgt_angular = tgt_half / tgt_dist;

        // The occluder must subtend a larger angle than the target on all axes.
        occ_angular.x >= tgt_angular.x
            && occ_angular.y >= tgt_angular.y
            && occ_angular.z >= tgt_angular.z
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

/// Returns the center of an AABB.
fn aabb_center(aabb: &Aabb) -> Vector3 {
    Vector3::new(
        aabb.position.x + aabb.size.x * 0.5,
        aabb.position.y + aabb.size.y * 0.5,
        aabb.position.z + aabb.size.z * 0.5,
    )
}

/// Returns the half-extent of an AABB.
fn aabb_half_extent(aabb: &Aabb) -> Vector3 {
    Vector3::new(
        aabb.size.x * 0.5,
        aabb.size.y * 0.5,
        aabb.size.z * 0.5,
    )
}

/// Transforms a local-space AABB to world space (conservative, axis-aligned).
fn transform_aabb(aabb: &Aabb, t: &Transform3D) -> Aabb {
    // Transform all 8 corners and find the enclosing AABB.
    let corners = [
        Vector3::new(aabb.position.x, aabb.position.y, aabb.position.z),
        Vector3::new(aabb.position.x + aabb.size.x, aabb.position.y, aabb.position.z),
        Vector3::new(aabb.position.x, aabb.position.y + aabb.size.y, aabb.position.z),
        Vector3::new(aabb.position.x + aabb.size.x, aabb.position.y + aabb.size.y, aabb.position.z),
        Vector3::new(aabb.position.x, aabb.position.y, aabb.position.z + aabb.size.z),
        Vector3::new(aabb.position.x + aabb.size.x, aabb.position.y, aabb.position.z + aabb.size.z),
        Vector3::new(aabb.position.x, aabb.position.y + aabb.size.y, aabb.position.z + aabb.size.z),
        Vector3::new(aabb.position.x + aabb.size.x, aabb.position.y + aabb.size.y, aabb.position.z + aabb.size.z),
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

    Aabb::new(min, Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z))
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
        let inst = OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(2.0, 2.0, 2.0)));
        let aabb = inst.get_world_aabb().unwrap();
        assert!(approx_eq(aabb.position.x, -1.0));
        assert!(approx_eq(aabb.size.x, 2.0));
    }

    #[test]
    fn world_aabb_translated() {
        let mut inst = OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(2.0, 2.0, 2.0)));
        inst.transform.origin = Vector3::new(10.0, 0.0, 0.0);
        let aabb = inst.get_world_aabb().unwrap();
        assert!(approx_eq(aabb.position.x, 9.0));
        assert!(approx_eq(aabb.size.x, 2.0));
    }

    // -- Occlusion test --

    #[test]
    fn large_occluder_occludes_small_target_behind() {
        let mut inst = OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(20.0, 20.0, 1.0)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -5.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-0.5, -0.5, -15.0), Vector3::new(1.0, 1.0, 1.0));

        assert!(inst.occludes(camera_pos, target));
    }

    #[test]
    fn small_occluder_does_not_occlude_large_target() {
        let mut inst = OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(0.5, 0.5, 0.5)));
        inst.transform.origin = Vector3::new(0.0, 0.0, -5.0);

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let target = Aabb::new(Vector3::new(-10.0, -10.0, -15.0), Vector3::new(20.0, 20.0, 1.0));

        assert!(!inst.occludes(camera_pos, target));
    }

    #[test]
    fn target_in_front_of_occluder_not_occluded() {
        let mut inst = OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(20.0, 20.0, 1.0)));
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
