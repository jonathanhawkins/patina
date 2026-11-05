//! 3D collision detection.
//!
//! The narrow phase provides a **complete, rotation-aware pair table** for the
//! bounded convex primitives (sphere, box, capsule, cylinder, convex hull) plus
//! the infinite `WorldBoundary` plane (pat-u3l8c.54).
//!
//! Common and analytically-exact pairs (sphere↔anything, capsule↔capsule,
//! plane↔anything) are handled with closed-form routines. The remaining pairs
//! that need a general convex solver honouring body rotation — rotated box↔box,
//! box↔capsule, box↔cylinder, capsule↔cylinder, cylinder↔cylinder, and any pair
//! involving a `ConvexPolygonShape` — are resolved with GJK (intersection) and
//! EPA (penetration depth + normal) built on per-shape support functions.
//!
//! All results follow the convention that `normal` points **from A toward B**
//! and `depth` is the positive penetration distance along that normal.

use std::borrow::Cow;

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::shape::Shape3D;

/// Result of a collision test between two 3D shapes.
#[derive(Debug, Clone, Copy)]
pub struct CollisionResult3D {
    /// Whether the shapes are overlapping.
    pub colliding: bool,
    /// The collision normal (from A to B).
    pub normal: Vector3,
    /// The penetration depth.
    pub depth: f32,
}

impl CollisionResult3D {
    /// No collision.
    pub const NONE: Self = Self {
        colliding: false,
        normal: Vector3::ZERO,
        depth: 0.0,
    };

    /// Returns a copy with the normal flipped (A↔B convention swap).
    #[inline]
    fn flipped(self) -> Self {
        Self {
            normal: self.normal * -1.0,
            ..self
        }
    }
}

/// Tests for collision between two spheres at the given positions.
pub fn test_sphere_sphere(
    pos_a: Vector3,
    radius_a: f32,
    pos_b: Vector3,
    radius_b: f32,
) -> CollisionResult3D {
    let diff = pos_b - pos_a;
    let dist_sq = diff.length_squared();
    let min_dist = radius_a + radius_b;

    if dist_sq >= min_dist * min_dist {
        return CollisionResult3D::NONE;
    }
    // Coincident centres are the DEEPEST overlap, not none: Godot's SAT test
    // (`_collision_sphere_sphere`) cannot separate on a zero axis. Pick an
    // arbitrary axis so the normal stays finite. Treating it as no contact
    // made a point query at a sphere's exact centre miss the sphere.
    if dist_sq < f32::EPSILON {
        return CollisionResult3D {
            colliding: true,
            normal: Vector3::new(0.0, 1.0, 0.0),
            depth: min_dist,
        };
    }

    let dist = dist_sq.sqrt();
    CollisionResult3D {
        colliding: true,
        normal: diff * (1.0 / dist),
        depth: min_dist - dist,
    }
}

/// Tests for collision between a sphere and an axis-aligned box.
pub fn test_sphere_box(
    sphere_pos: Vector3,
    radius: f32,
    box_pos: Vector3,
    half_extents: Vector3,
) -> CollisionResult3D {
    // Find the closest point on the box to the sphere center.
    let local = sphere_pos - box_pos;
    let clamped = Vector3::new(
        local.x.clamp(-half_extents.x, half_extents.x),
        local.y.clamp(-half_extents.y, half_extents.y),
        local.z.clamp(-half_extents.z, half_extents.z),
    );
    let diff = local - clamped;
    let dist_sq = diff.length_squared();

    if dist_sq < f32::EPSILON {
        // Sphere center is inside the box — find the axis of minimum penetration.
        // Normal must point from A (sphere) toward B (box center), i.e. opposite
        // to the push-out direction.
        let pen_x = half_extents.x - local.x.abs();
        let pen_y = half_extents.y - local.y.abs();
        let pen_z = half_extents.z - local.z.abs();

        if pen_x <= pen_y && pen_x <= pen_z {
            let sign = if local.x >= 0.0 { -1.0 } else { 1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(sign, 0.0, 0.0),
                depth: pen_x + radius,
            };
        } else if pen_y <= pen_z {
            let sign = if local.y >= 0.0 { -1.0 } else { 1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(0.0, sign, 0.0),
                depth: pen_y + radius,
            };
        } else {
            let sign = if local.z >= 0.0 { -1.0 } else { 1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(0.0, 0.0, sign),
                depth: pen_z + radius,
            };
        }
    }

    if dist_sq >= radius * radius {
        return CollisionResult3D::NONE;
    }

    let dist = dist_sq.sqrt();
    CollisionResult3D {
        colliding: true,
        normal: diff * (1.0 / dist),
        depth: radius - dist,
    }
}

/// Tests for collision between two axis-aligned boxes.
pub fn test_box_box(
    pos_a: Vector3,
    half_a: Vector3,
    pos_b: Vector3,
    half_b: Vector3,
) -> CollisionResult3D {
    let diff = pos_b - pos_a;
    let overlap_x = (half_a.x + half_b.x) - diff.x.abs();
    let overlap_y = (half_a.y + half_b.y) - diff.y.abs();
    let overlap_z = (half_a.z + half_b.z) - diff.z.abs();

    if overlap_x <= 0.0 || overlap_y <= 0.0 || overlap_z <= 0.0 {
        return CollisionResult3D::NONE;
    }

    // Resolve along the axis with minimum overlap (least penetration).
    let (depth, mut normal) = if overlap_x <= overlap_y && overlap_x <= overlap_z {
        (overlap_x, Vector3::new(1.0, 0.0, 0.0))
    } else if overlap_y <= overlap_z {
        (overlap_y, Vector3::new(0.0, 1.0, 0.0))
    } else {
        (overlap_z, Vector3::new(0.0, 0.0, 1.0))
    };

    // Normal should point from A to B.
    if diff.x < 0.0 && normal.x > 0.0 {
        normal.x = -1.0;
    }
    if diff.y < 0.0 && normal.y > 0.0 {
        normal.y = -1.0;
    }
    if diff.z < 0.0 && normal.z > 0.0 {
        normal.z = -1.0;
    }

    CollisionResult3D {
        colliding: true,
        normal,
        depth,
    }
}

// ===========================================================================
// Rotation-aware narrow-phase dispatch
// ===========================================================================

/// Tests for collision between two shapes at the given positions, assuming no
/// rotation. Preserved for backwards compatibility; delegates to the
/// rotation-aware [`test_collision_rot`] with identity orientations.
pub fn test_collision(
    pos_a: Vector3,
    shape_a: &Shape3D,
    pos_b: Vector3,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    test_collision_rot(
        pos_a,
        Quaternion::IDENTITY,
        shape_a,
        pos_b,
        Quaternion::IDENTITY,
        shape_b,
    )
}

/// Rotation-aware narrow-phase test between two oriented shapes.
///
/// Implements the full sphere/box/capsule/cylinder/plane pair table honouring
/// each body's `rotation`. Returns a result whose `normal` points from A to B.
pub fn test_collision_rot(
    pos_a: Vector3,
    rot_a: Quaternion,
    shape_a: &Shape3D,
    pos_b: Vector3,
    rot_b: Quaternion,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    use Shape3D::*;

    // --- Infinite plane (WorldBoundary) pairs: handled analytically because a
    //     half-space is unbounded and cannot be fed to GJK. -------------------
    match (shape_a, shape_b) {
        (WorldBoundaryShape { .. }, WorldBoundaryShape { .. }) => {
            return CollisionResult3D::NONE;
        }
        (
            WorldBoundaryShape {
                normal: n,
                distance: d,
            },
            other,
        ) if is_bounded_convex(other) => {
            let (n_w, d_w) = plane_world(pos_a, rot_a, *n, *d);
            // A = plane, B = convex. Push B out along +n_w ⇒ normal A→B = +n_w.
            return match plane_penetration(other, pos_b, rot_b, n_w, d_w) {
                Some(depth) => CollisionResult3D {
                    colliding: true,
                    normal: n_w,
                    depth,
                },
                None => CollisionResult3D::NONE,
            };
        }
        (
            other,
            WorldBoundaryShape {
                normal: n,
                distance: d,
            },
        ) if is_bounded_convex(other) => {
            let (n_w, d_w) = plane_world(pos_b, rot_b, *n, *d);
            // A = convex, B = plane. A→B points from the convex toward the plane
            // (opposite the push-out direction) ⇒ −n_w.
            return match plane_penetration(other, pos_a, rot_a, n_w, d_w) {
                Some(depth) => CollisionResult3D {
                    colliding: true,
                    normal: n_w * -1.0,
                    depth,
                },
                None => CollisionResult3D::NONE,
            };
        }
        _ => {}
    }

    // --- Analytically-exact primitive pairs. --------------------------------
    match (shape_a, shape_b) {
        // Sphere is rotation invariant.
        (Sphere { radius: ra }, Sphere { radius: rb }) => {
            return test_sphere_sphere(pos_a, *ra, pos_b, *rb);
        }
        (Sphere { radius }, BoxShape { half_extents }) => {
            return sphere_obb(pos_a, *radius, pos_b, rot_b, *half_extents).flipped();
        }
        (BoxShape { half_extents }, Sphere { radius }) => {
            return sphere_obb(pos_b, *radius, pos_a, rot_a, *half_extents);
        }
        (Sphere { radius }, CapsuleShape { radius: cr, height }) => {
            return sphere_capsule(pos_a, *radius, pos_b, rot_b, *cr, *height).flipped();
        }
        (CapsuleShape { radius: cr, height }, Sphere { radius }) => {
            return sphere_capsule(pos_b, *radius, pos_a, rot_a, *cr, *height);
        }
        (Sphere { radius }, CylinderShape { radius: cr, height }) => {
            return sphere_cylinder(pos_a, *radius, pos_b, rot_b, *cr, *height).flipped();
        }
        (CylinderShape { radius: cr, height }, Sphere { radius }) => {
            return sphere_cylinder(pos_b, *radius, pos_a, rot_a, *cr, *height);
        }
        // Exact closest-feature solver (pat-1unro); falls through to GJK/EPA
        // only for a hull whose face planes cannot be derived.
        (Sphere { radius }, ConvexPolygonShape { points }) => {
            if let Some(r) = sphere_convex(pos_a, *radius, pos_b, rot_b, points) {
                return r.flipped();
            }
        }
        (ConvexPolygonShape { points }, Sphere { radius }) => {
            if let Some(r) = sphere_convex(pos_b, *radius, pos_a, rot_a, points) {
                return r;
            }
        }
        (
            CapsuleShape {
                radius: ra,
                height: ha,
            },
            CapsuleShape {
                radius: rb,
                height: hb,
            },
        ) => {
            return capsule_capsule(pos_a, rot_a, *ra, *ha, pos_b, rot_b, *rb, *hb);
        }
        (BoxShape { half_extents: ha }, BoxShape { half_extents: hb })
            if is_identity_rot(rot_a) && is_identity_rot(rot_b) =>
        {
            return test_box_box(pos_a, *ha, pos_b, *hb);
        }
        _ => {}
    }

    // --- Separation ray vs convex (pat-0q2kx.119). The ray has a bounded
    //     convex support function, so GJK/EPA handles the intersection; the
    //     `slide_on_slope` flag then decides whether the recovery follows the
    //     contact normal or is forced back along the ray's own axis, exactly as
    //     `GodotSeparationRayShape3D` does in the body recovery pass.
    match (shape_a, shape_b) {
        (
            SeparationRayShape {
                length,
                slide_on_slope,
            },
            other,
        ) if is_bounded_convex(other) && !matches!(other, SeparationRayShape { .. }) => {
            return separation_ray_convex(
                pos_a,
                rot_a,
                *length,
                *slide_on_slope,
                pos_b,
                rot_b,
                shape_b,
            );
        }
        (
            other,
            SeparationRayShape {
                length,
                slide_on_slope,
            },
        ) if is_bounded_convex(other) => {
            return separation_ray_convex(
                pos_b,
                rot_b,
                *length,
                *slide_on_slope,
                pos_a,
                rot_a,
                shape_a,
            )
            .flipped();
        }
        _ => {}
    }

    // --- General convex solver for the remaining rotation-aware pairs. -------
    if is_bounded_convex(shape_a) && is_bounded_convex(shape_b) {
        return gjk_epa(pos_a, rot_a, shape_a, pos_b, rot_b, shape_b);
    }

    // --- Concave (trimesh / heightmap) vs convex. A concave surface is not a
    //     single convex body, so it is tested triangle by triangle. Sphere and
    //     capsule keep their closest-point solvers (stable across triangle
    //     edges); every other convex shape — box, cylinder, convex hull, and
    //     separation ray — goes through the general per-triangle path
    //     (pat-0q2kx.119). The reported normal points from the surface (A)
    //     toward the other shape (B).
    match (
        concave_faces_for(shape_a, shape_b, pos_a, rot_a, pos_b, rot_b),
        concave_faces_for(shape_b, shape_a, pos_b, rot_b, pos_a, rot_a),
    ) {
        // Concave vs concave stays unsupported, as in Godot.
        (Some(_), Some(_)) => return CollisionResult3D::NONE,
        (Some((faces, backface)), None) if is_bounded_convex(shape_b) => {
            return concave_convex(
                faces.as_ref(),
                backface,
                pos_a,
                rot_a,
                pos_b,
                rot_b,
                shape_b,
            );
        }
        (None, Some((faces, backface))) if is_bounded_convex(shape_a) => {
            return concave_convex(
                faces.as_ref(),
                backface,
                pos_b,
                rot_b,
                pos_a,
                rot_a,
                shape_a,
            )
            .flipped();
        }
        _ => {}
    }

    // Other unsupported combinations (e.g. trimesh vs world boundary).
    CollisionResult3D::NONE
}

/// Returns `true` for shapes with a bounded convex support function.
fn is_bounded_convex(shape: &Shape3D) -> bool {
    matches!(
        shape,
        Shape3D::Sphere { .. }
            | Shape3D::BoxShape { .. }
            | Shape3D::CapsuleShape { .. }
            | Shape3D::CylinderShape { .. }
            | Shape3D::ConvexPolygonShape { .. }
            | Shape3D::SeparationRayShape { .. }
    )
}

/// Triangle soup (in `shape`'s local frame) and backface flag for a concave
/// shape, or `None` when `shape` is not concave.
///
/// A [`Shape3D::HeightMapShape`] materialises only the cells overlapping
/// `other`'s world AABB — the "sample triangles on demand" path, so a large
/// terrain never expands its whole grid for a single narrow-phase query.
fn concave_faces_for<'a>(
    shape: &'a Shape3D,
    other: &Shape3D,
    pos: Vector3,
    rot: Quaternion,
    other_pos: Vector3,
    other_rot: Quaternion,
) -> Option<(Cow<'a, [Vector3]>, bool)> {
    match shape {
        Shape3D::ConcavePolygonShape {
            faces,
            backface_collision,
            // Borrowed: an authored soup is already in the right form, and
            // cloning it per narrow-phase pair would scale with level size.
        } => Some((Cow::Borrowed(faces.as_slice()), *backface_collision)),
        Shape3D::HeightMapShape {
            map_width,
            map_depth,
            map_data,
        } => {
            let bounds = local_xz_window(other, other_pos, other_rot, pos, rot);
            Some((
                Cow::Owned(crate::shape::heightmap_faces(
                    *map_width,
                    *map_depth,
                    map_data,
                    Some(bounds),
                )),
                // Godot's heightmap faces are front-facing only.
                false,
            ))
        }
        _ => None,
    }
}

/// The X/Z window (`min_x, max_x, min_z, max_z`) that `other` occupies when
/// expressed in the concave shape's local frame. Conservative: the other
/// shape's local AABB is inflated to a sphere so any rotation is covered.
fn local_xz_window(
    other: &Shape3D,
    other_pos: Vector3,
    _other_rot: Quaternion,
    mesh_pos: Vector3,
    mesh_rot: Quaternion,
) -> (f32, f32, f32, f32) {
    let aabb = other.bounding_aabb();
    let half = aabb.size * 0.5;
    let center_local = aabb.position + half;
    // Radius of the sphere enclosing the other shape about its own origin.
    let radius = center_local.length() + half.length();
    let center = mesh_rot.inverse().xform(other_pos - mesh_pos);
    (
        center.x - radius,
        center.x + radius,
        center.z - radius,
        center.z + radius,
    )
}

/// Dispatches a concave triangle soup against a bounded convex shape.
#[allow(clippy::too_many_arguments)]
fn concave_convex(
    faces: &[Vector3],
    backface: bool,
    mesh_pos: Vector3,
    mesh_rot: Quaternion,
    pos_b: Vector3,
    rot_b: Quaternion,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    match shape_b {
        Shape3D::Sphere { radius } => {
            trimesh_sphere(mesh_pos, mesh_rot, faces, backface, pos_b, *radius)
        }
        Shape3D::CapsuleShape { radius, height } => trimesh_capsule(
            mesh_pos, mesh_rot, faces, backface, pos_b, rot_b, *radius, *height,
        ),
        _ => trimesh_convex(mesh_pos, mesh_rot, faces, backface, pos_b, rot_b, shape_b),
    }
}

/// Returns `true` if the quaternion is (numerically) the identity rotation.
fn is_identity_rot(q: Quaternion) -> bool {
    q.x.abs() < 1e-6 && q.y.abs() < 1e-6 && q.z.abs() < 1e-6 && (q.w.abs() - 1.0).abs() < 1e-6
}

// ===========================================================================
// Geometric helpers
// ===========================================================================

/// Closest point on segment `[a, b]` to point `p`.
fn closest_point_on_segment(p: Vector3, a: Vector3, b: Vector3) -> Vector3 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

/// Closest points between two segments `[p1, q1]` and `[p2, q2]`.
///
/// Port of Ericson, *Real-Time Collision Detection*, §5.1.9.
fn closest_points_segments(
    p1: Vector3,
    q1: Vector3,
    p2: Vector3,
    q2: Vector3,
) -> (Vector3, Vector3) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);
    let eps = 1e-12;

    let (s, t);
    if a <= eps && e <= eps {
        return (p1, p2);
    }
    if a <= eps {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= eps {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let s0 = if denom > eps {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            } else {
                t = t0;
                s = s0;
            }
        }
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// An arbitrary vector perpendicular to `v` (assumes `v` non-zero).
fn any_perpendicular(v: Vector3) -> Vector3 {
    let axis = if v.x.abs() < 0.9 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    v.cross(axis)
}

// ===========================================================================
// Analytic primitive pairs (all return the normal pointing *toward the sphere*
// for the sphere-vs-X helpers, matching `test_sphere_box`'s convention).
// ===========================================================================

/// Sphere vs oriented box. Returns the normal pointing from the box toward the
/// sphere (box → sphere), matching [`test_sphere_box`]. Reduces exactly to
/// [`test_sphere_box`] for an identity box rotation.
fn sphere_obb(
    sphere_pos: Vector3,
    radius: f32,
    box_pos: Vector3,
    box_rot: Quaternion,
    half: Vector3,
) -> CollisionResult3D {
    // Work in the box's local frame, then rotate the resulting normal back.
    let local = box_rot.inverse().xform(sphere_pos - box_pos);
    let res = test_sphere_box(local, radius, Vector3::ZERO, half);
    if !res.colliding {
        return CollisionResult3D::NONE;
    }
    CollisionResult3D {
        normal: box_rot.xform(res.normal),
        ..res
    }
}

/// Sphere vs (rotated) convex hull, exactly: the hull's closest feature (face,
/// edge or vertex) to the sphere centre, or — with the centre inside — the
/// shallowest face. Returns the normal from the hull toward the sphere, like
/// [`sphere_obb`]; `None` when the hull's face planes cannot be derived
/// (degenerate or oversized point cloud), so the caller can fall back to
/// GJK/EPA.
///
/// 🔑 Why not GJK/EPA (pat-1unro): EPA against a sphere's smooth support
/// converges slowly, and at its iteration budget it reported a sphere resting
/// 1.5 mm into the platformer's flat `floor_edge` tile as 109 mm deep along a
/// tilted normal. Every enemy crossing from one such tile to the next was
/// kicked 0.1 m into the air and fell back — its `linear_velocity.y` jumped to
/// -0.6 where Godot's stays at -0.18.
fn sphere_convex(
    sphere_pos: Vector3,
    radius: f32,
    hull_pos: Vector3,
    hull_rot: Quaternion,
    points: &[Vector3],
) -> Option<CollisionResult3D> {
    let planes = crate::shape::convex_face_planes(points);
    if planes.is_empty() {
        return None;
    }
    let c = hull_rot.inverse().xform(sphere_pos - hull_pos);
    const EPS: f32 = 1e-4;

    // Centre inside: push out through the face it is closest to.
    let (max_s, max_n) = planes
        .iter()
        .map(|(n, d)| (n.dot(c) - d, *n))
        .fold((f32::NEG_INFINITY, Vector3::ZERO), |acc, x| {
            if x.0 > acc.0 {
                x
            } else {
                acc
            }
        });
    if max_s <= 0.0 {
        return Some(CollisionResult3D {
            colliding: true,
            normal: hull_rot.xform(max_n),
            depth: radius - max_s,
        });
    }

    // Centre outside: the closest point of the hull. A face's projection
    // counts only when it lands inside the hull; edges are the point pairs
    // sharing two face planes; vertices always count.
    let inside = |q: Vector3| planes.iter().all(|(n, d)| n.dot(q) - d <= EPS);
    let mut best = Vector3::ZERO;
    let mut best_d2 = f32::INFINITY;
    let mut offer = |q: Vector3| {
        let d2 = (c - q).length_squared();
        if d2 < best_d2 {
            best_d2 = d2;
            best = q;
        }
    };
    for (n, d) in &planes {
        let s = n.dot(c) - d;
        if s > 0.0 {
            let q = c - *n * s;
            if inside(q) {
                offer(q);
            }
        }
    }
    let on_plane = |p: Vector3, (n, d): &(Vector3, f32)| (n.dot(p) - d).abs() <= EPS;
    for i in 0..points.len() {
        offer(points[i]);
        for j in (i + 1)..points.len() {
            let shared = planes
                .iter()
                .filter(|pl| on_plane(points[i], pl) && on_plane(points[j], pl))
                .count();
            if shared >= 2 {
                offer(closest_point_on_segment(c, points[i], points[j]));
            }
        }
    }
    let dist = best_d2.sqrt();
    if dist >= radius {
        return Some(CollisionResult3D::NONE);
    }
    let out = if dist > 1e-9 {
        (c - best) * (1.0 / dist)
    } else {
        max_n
    };
    Some(CollisionResult3D {
        colliding: true,
        normal: hull_rot.xform(out),
        depth: radius - dist,
    })
}

/// Sphere vs (rotated) capsule. Returns the normal from the capsule toward the
/// sphere.
fn sphere_capsule(
    sphere_pos: Vector3,
    radius: f32,
    cap_pos: Vector3,
    cap_rot: Quaternion,
    cap_radius: f32,
    cap_height: f32,
) -> CollisionResult3D {
    let hs = (cap_height * 0.5 - cap_radius).max(0.0);
    let p0 = cap_pos + cap_rot.xform(Vector3::new(0.0, -hs, 0.0));
    let p1 = cap_pos + cap_rot.xform(Vector3::new(0.0, hs, 0.0));
    let q = closest_point_on_segment(sphere_pos, p0, p1);
    let diff = sphere_pos - q;
    let dist_sq = diff.length_squared();
    let sum = radius + cap_radius;
    if dist_sq >= sum * sum {
        return CollisionResult3D::NONE;
    }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-9 {
        diff * (1.0 / dist)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    CollisionResult3D {
        colliding: true,
        normal,
        depth: sum - dist,
    }
}

/// Capsule vs capsule. Returns the normal pointing from A toward B.
fn capsule_capsule(
    pos_a: Vector3,
    rot_a: Quaternion,
    ra: f32,
    ha: f32,
    pos_b: Vector3,
    rot_b: Quaternion,
    rb: f32,
    hb: f32,
) -> CollisionResult3D {
    let hsa = (ha * 0.5 - ra).max(0.0);
    let hsb = (hb * 0.5 - rb).max(0.0);
    let a0 = pos_a + rot_a.xform(Vector3::new(0.0, -hsa, 0.0));
    let a1 = pos_a + rot_a.xform(Vector3::new(0.0, hsa, 0.0));
    let b0 = pos_b + rot_b.xform(Vector3::new(0.0, -hsb, 0.0));
    let b1 = pos_b + rot_b.xform(Vector3::new(0.0, hsb, 0.0));
    let (qa, qb) = closest_points_segments(a0, a1, b0, b1);
    let diff = qb - qa;
    let dist_sq = diff.length_squared();
    let sum = ra + rb;
    if dist_sq >= sum * sum {
        return CollisionResult3D::NONE;
    }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-9 {
        diff * (1.0 / dist)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    CollisionResult3D {
        colliding: true,
        normal,
        depth: sum - dist,
    }
}

/// Sphere vs (rotated) cylinder. Returns the normal from the cylinder toward
/// the sphere. The cylinder axis is local +Y.
fn sphere_cylinder(
    sphere_pos: Vector3,
    radius: f32,
    cyl_pos: Vector3,
    cyl_rot: Quaternion,
    cyl_radius: f32,
    cyl_height: f32,
) -> CollisionResult3D {
    let local = cyl_rot.inverse().xform(sphere_pos - cyl_pos);
    let hy = cyl_height * 0.5;
    let rr = (local.x * local.x + local.z * local.z).sqrt();

    let normal_local;
    let depth;
    if local.y.abs() <= hy && rr <= cyl_radius {
        // Sphere center inside the cylinder — resolve along the least
        // penetrating face (curved side vs flat cap).
        let side_pen = cyl_radius - rr;
        let cap_pen = hy - local.y.abs();
        if side_pen <= cap_pen {
            normal_local = if rr > 1e-9 {
                Vector3::new(local.x / rr, 0.0, local.z / rr)
            } else {
                Vector3::new(1.0, 0.0, 0.0)
            };
            depth = side_pen + radius;
        } else {
            let sy = if local.y >= 0.0 { 1.0 } else { -1.0 };
            normal_local = Vector3::new(0.0, sy, 0.0);
            depth = cap_pen + radius;
        }
    } else {
        // Closest point on the solid cylinder to the sphere center.
        let cy = local.y.clamp(-hy, hy);
        let (cx, cz) = if rr > cyl_radius {
            (local.x * cyl_radius / rr, local.z * cyl_radius / rr)
        } else {
            (local.x, local.z)
        };
        let q = Vector3::new(cx, cy, cz);
        let diff = local - q;
        let dist_sq = diff.length_squared();
        if dist_sq >= radius * radius {
            return CollisionResult3D::NONE;
        }
        let dist = dist_sq.sqrt();
        normal_local = if dist > 1e-9 {
            diff * (1.0 / dist)
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
        depth = radius - dist;
    }

    CollisionResult3D {
        colliding: true,
        normal: cyl_rot.xform(normal_local),
        depth,
    }
}

// ===========================================================================
// Plane (WorldBoundary) helpers
// ===========================================================================

/// Transforms a local-space plane (normal + distance) by a body transform into
/// a world-space plane `(normal, offset)` where the solid half-space is
/// `dot(normal, x) <= offset`.
fn plane_world(
    plane_pos: Vector3,
    plane_rot: Quaternion,
    local_normal: Vector3,
    local_distance: f32,
) -> (Vector3, f32) {
    let n = plane_rot.xform(local_normal);
    let len = n.length();
    let n_w = if len > 1e-9 {
        n * (1.0 / len)
    } else {
        local_normal
    };
    let d_w = local_distance + n_w.dot(plane_pos);
    (n_w, d_w)
}

/// Penetration depth of a convex shape into the solid side of a world plane,
/// or `None` if it does not reach the plane.
fn plane_penetration(
    shape: &Shape3D,
    center: Vector3,
    rot: Quaternion,
    n_w: Vector3,
    d_w: f32,
) -> Option<f32> {
    // Deepest point of the shape into the solid half-space is its support point
    // along -n_w.
    let deepest = world_support(shape, center, rot, n_w * -1.0);
    let depth = d_w - n_w.dot(deepest);
    if depth > 0.0 {
        Some(depth)
    } else {
        None
    }
}

// ===========================================================================
// Concave trimesh narrow phase (pat-u3l8c.56)
// ===========================================================================

/// Closest point on triangle `(a, b, c)` to point `p`.
///
/// Port of Ericson, *Real-Time Collision Detection*, §5.1.5 (Voronoi regions).
fn closest_point_on_triangle(p: Vector3, a: Vector3, b: Vector3, c: Vector3) -> Vector3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a; // vertex region A
    }

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b; // vertex region B
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v; // edge region AB
    }

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c; // vertex region C
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w; // edge region AC
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w; // edge region BC
    }

    // Interior: barycentric combination.
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

/// Closest pair of points between segment `[p0, p1]` and triangle `(a, b, c)`,
/// returned as `(point_on_segment, point_on_triangle)`.
///
/// Evaluates the candidate features (each segment endpoint vs the triangle, and
/// the segment vs each triangle edge) and keeps the nearest pair. This covers
/// all vertex/edge/face contact configurations for a segment against a triangle.
fn closest_segment_triangle(
    p0: Vector3,
    p1: Vector3,
    a: Vector3,
    b: Vector3,
    c: Vector3,
) -> (Vector3, Vector3) {
    let mut best_seg = p0;
    let mut best_tri = closest_point_on_triangle(p0, a, b, c);
    let mut best_d = (best_seg - best_tri).length_squared();

    let tp1 = closest_point_on_triangle(p1, a, b, c);
    let d1 = (p1 - tp1).length_squared();
    if d1 < best_d {
        best_d = d1;
        best_seg = p1;
        best_tri = tp1;
    }

    for (e0, e1) in [(a, b), (b, c), (c, a)] {
        let (sp, ep) = closest_points_segments(p0, p1, e0, e1);
        let d = (sp - ep).length_squared();
        if d < best_d {
            best_d = d;
            best_seg = sp;
            best_tri = ep;
        }
    }

    (best_seg, best_tri)
}

/// Transforms a triangle-soup index triple into world space.
#[inline]
fn world_triangle(
    mesh_pos: Vector3,
    mesh_rot: Quaternion,
    tri: &[Vector3],
) -> (Vector3, Vector3, Vector3) {
    (
        mesh_pos + mesh_rot.xform(tri[0]),
        mesh_pos + mesh_rot.xform(tri[1]),
        mesh_pos + mesh_rot.xform(tri[2]),
    )
}

/// Fallback normal (face normal) for a triangle when the witness distance is
/// too small to derive a stable direction.
#[inline]
fn triangle_normal(a: Vector3, b: Vector3, c: Vector3) -> Vector3 {
    let n = (b - a).cross(c - a);
    let len = n.length();
    if len > 1e-9 {
        n * (1.0 / len)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    }
}

/// Winding-aware face normal using Godot's convention, `Plane(a, b, c)` with
/// `CLOCKWISE` direction: `normal = (a - c) × (a - b)`. This is the *front*
/// face direction that `ConcavePolygonShape3D.backface_collision` is measured
/// against, and it is the opposite sign from [`triangle_normal`]. Returns
/// `None` for a degenerate (zero-area) triangle.
#[inline]
fn godot_face_normal(a: Vector3, b: Vector3, c: Vector3) -> Option<Vector3> {
    let n = (a - c).cross(a - b);
    let len = n.length();
    if len > 1e-9 {
        Some(n * (1.0 / len))
    } else {
        None
    }
}

/// Whether a triangle may collide with a shape whose centre is `center`, given
/// the soup's `backface_collision` flag. Returns the *outward* push direction
/// (the face normal oriented toward the shape) when the contact is allowed.
///
/// Mirrors `godot_collision_solver_3d_sat.cpp`: with `backface_collision` off,
/// a contact whose push-out direction opposes the face normal is discarded.
#[inline]
fn face_out_normal(
    a: Vector3,
    b: Vector3,
    c: Vector3,
    center: Vector3,
    backface: bool,
) -> Option<Vector3> {
    let n = godot_face_normal(a, b, c)?;
    let side = (center - a).dot(n);
    if !backface && side < 0.0 {
        return None;
    }
    Some(if side >= 0.0 { n } else { n * -1.0 })
}

/// Concave trimesh vs sphere. The trimesh `faces` are a flat triangle soup
/// (every three vertices form one triangle) in the mesh's local frame; the
/// sphere is at `sphere_pos` with `radius`. Returns the deepest-penetrating
/// triangle contact, normal pointing from the mesh toward the sphere.
fn trimesh_sphere(
    mesh_pos: Vector3,
    mesh_rot: Quaternion,
    faces: &[Vector3],
    backface: bool,
    sphere_pos: Vector3,
    radius: f32,
) -> CollisionResult3D {
    let mut best = CollisionResult3D::NONE;
    for tri in faces.chunks_exact(3) {
        let (a, b, c) = world_triangle(mesh_pos, mesh_rot, tri);
        if face_out_normal(a, b, c, sphere_pos, backface).is_none() {
            continue;
        }
        let cp = closest_point_on_triangle(sphere_pos, a, b, c);
        let diff = sphere_pos - cp;
        let dist_sq = diff.length_squared();
        if dist_sq > radius * radius {
            continue;
        }
        let dist = dist_sq.sqrt();
        let depth = radius - dist;
        if depth <= best.depth {
            continue;
        }
        let normal = if dist > 1e-6 {
            diff * (1.0 / dist)
        } else {
            // Sphere centre on the triangle: use the face normal, oriented
            // toward the sphere so resolution pushes it back out.
            let n = triangle_normal(a, b, c);
            if n.dot(sphere_pos - a) < 0.0 {
                n * -1.0
            } else {
                n
            }
        };
        best = CollisionResult3D {
            colliding: true,
            normal,
            depth,
        };
    }
    best
}

/// Concave trimesh vs capsule. The capsule's core segment is tested against
/// each triangle; a contact occurs when the segment passes within `cap_radius`
/// of a triangle. Returns the deepest contact, normal pointing from the mesh
/// toward the capsule.
#[allow(clippy::too_many_arguments)]
fn trimesh_capsule(
    mesh_pos: Vector3,
    mesh_rot: Quaternion,
    faces: &[Vector3],
    backface: bool,
    cap_pos: Vector3,
    cap_rot: Quaternion,
    cap_radius: f32,
    cap_height: f32,
) -> CollisionResult3D {
    // Capsule core segment endpoints (the cylinder axis, excluding the caps).
    let hs = (cap_height * 0.5 - cap_radius).max(0.0);
    let p0 = cap_pos + cap_rot.xform(Vector3::new(0.0, -hs, 0.0));
    let p1 = cap_pos + cap_rot.xform(Vector3::new(0.0, hs, 0.0));

    let mut best = CollisionResult3D::NONE;
    for tri in faces.chunks_exact(3) {
        let (a, b, c) = world_triangle(mesh_pos, mesh_rot, tri);
        if face_out_normal(a, b, c, cap_pos, backface).is_none() {
            continue;
        }
        let (seg_pt, tri_pt) = closest_segment_triangle(p0, p1, a, b, c);
        let diff = seg_pt - tri_pt;
        let dist_sq = diff.length_squared();
        if dist_sq > cap_radius * cap_radius {
            continue;
        }
        let dist = dist_sq.sqrt();
        let depth = cap_radius - dist;
        if depth <= best.depth {
            continue;
        }
        let normal = if dist > 1e-6 {
            diff * (1.0 / dist)
        } else {
            // Capsule axis crossing the triangle: use the face normal, oriented
            // toward the capsule so resolution pushes it back out.
            let n = triangle_normal(a, b, c);
            if n.dot(cap_pos - a) < 0.0 {
                n * -1.0
            } else {
                n
            }
        };
        best = CollisionResult3D {
            colliding: true,
            normal,
            depth,
        };
    }
    best
}

/// Concave surface vs any bounded convex shape — box, cylinder, convex hull, or
/// separation ray (pat-0q2kx.119).
///
/// Each candidate triangle is fed to GJK as a three-point convex hull to decide
/// whether the shape actually overlaps *that* triangle rather than merely the
/// infinite plane through it. The contact normal is then taken from the
/// triangle's own face normal (oriented toward the shape) and the depth from the
/// shape's support point along it.
///
/// Deriving the normal from the face rather than from EPA is deliberate: a
/// triangle has zero thickness, so "push the body out of the front" and "push
/// the body out through the back" are the same minimal penetration depth and EPA
/// picks between them arbitrarily — half the time sinking the body through the
/// floor. Godot sidesteps this the same way, by testing the face axis in its SAT
/// solver and rejecting contacts that oppose the face normal.
#[allow(clippy::too_many_arguments)]
fn trimesh_convex(
    mesh_pos: Vector3,
    mesh_rot: Quaternion,
    faces: &[Vector3],
    backface: bool,
    pos_b: Vector3,
    rot_b: Quaternion,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    let mut best = CollisionResult3D::NONE;
    for tri in faces.chunks_exact(3) {
        let (a, b, c) = world_triangle(mesh_pos, mesh_rot, tri);
        let Some(out) = face_out_normal(a, b, c, pos_b, backface) else {
            continue;
        };

        // Penetration of B through the triangle's plane, along the face normal.
        let deepest = world_support(shape_b, pos_b, rot_b, out * -1.0);
        let depth = (a - deepest).dot(out);
        if depth <= 0.0 || depth <= best.depth {
            continue;
        }

        // Confirm the overlap is over *this* triangle, not just its plane.
        // The hull is centred on the triangle's centroid so GJK gets a sane
        // initial search direction (`centroid - pos_b`) regardless of how far
        // the soup sits from the world origin.
        let centroid = (a + b + c) * (1.0 / 3.0);
        let tri_hull = Shape3D::ConvexPolygonShape {
            points: vec![a - centroid, b - centroid, c - centroid],
        };
        if !gjk_intersects(
            centroid,
            Quaternion::IDENTITY,
            &tri_hull,
            pos_b,
            rot_b,
            shape_b,
        ) {
            continue;
        }

        best = CollisionResult3D {
            colliding: true,
            normal: out,
            depth,
        };
    }
    best
}

// ===========================================================================
// Separation ray (pat-0q2kx.119)
// ===========================================================================

/// Separation ray vs a bounded convex shape. `ray_*` describes the ray body
/// (A); the returned normal points from the ray toward `shape_b` (B).
///
/// With `slide_on_slope` the contact normal comes straight from EPA, so the
/// recovery follows the surface and the body slides along it. Without it the
/// recovery is forced back along the ray's own axis — Godot's behaviour for a
/// `SeparationRayShape3D` that must not creep down a slope.
#[allow(clippy::too_many_arguments)]
fn separation_ray_convex(
    ray_pos: Vector3,
    ray_rot: Quaternion,
    length: f32,
    slide_on_slope: bool,
    pos_b: Vector3,
    rot_b: Quaternion,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    let ray = Shape3D::SeparationRayShape {
        length,
        slide_on_slope,
    };
    let hit = gjk_epa(ray_pos, ray_rot, &ray, pos_b, rot_b, shape_b);
    if !hit.colliding || slide_on_slope {
        return hit;
    }

    // Forced axis recovery: measure how far the ray tip has sunk past the
    // shape's near surface, along the ray's own direction.
    let dir = ray_rot.xform(Vector3::new(0.0, 0.0, 1.0));
    let len = dir.length();
    if len < 1e-9 {
        return hit;
    }
    let dir = dir * (1.0 / len);
    let tip = ray_pos + dir * length;
    let entry = world_support(shape_b, pos_b, rot_b, dir * -1.0);
    let depth = (tip - entry).dot(dir);
    if depth <= 0.0 {
        return hit;
    }
    CollisionResult3D {
        colliding: true,
        normal: dir,
        depth,
    }
}

// ===========================================================================
// GJK + EPA general convex solver
// ===========================================================================

/// Support point of `shape` (local frame) farthest along `dir` (local frame).
fn local_support(shape: &Shape3D, dir: Vector3) -> Vector3 {
    match shape {
        Shape3D::Sphere { radius } => {
            let len = dir.length();
            if len > 1e-9 {
                dir * (*radius / len)
            } else {
                Vector3::ZERO
            }
        }
        Shape3D::BoxShape { half_extents } => Vector3::new(
            if dir.x >= 0.0 {
                half_extents.x
            } else {
                -half_extents.x
            },
            if dir.y >= 0.0 {
                half_extents.y
            } else {
                -half_extents.y
            },
            if dir.z >= 0.0 {
                half_extents.z
            } else {
                -half_extents.z
            },
        ),
        Shape3D::CapsuleShape { radius, height } => {
            let hs = (*height * 0.5 - *radius).max(0.0);
            let seg_y = if dir.y >= 0.0 { hs } else { -hs };
            let len = dir.length();
            let cap = if len > 1e-9 {
                dir * (*radius / len)
            } else {
                Vector3::ZERO
            };
            Vector3::new(cap.x, seg_y + cap.y, cap.z)
        }
        Shape3D::CylinderShape { radius, height } => {
            let hy = *height * 0.5;
            let y = if dir.y >= 0.0 { hy } else { -hy };
            let radial_len = (dir.x * dir.x + dir.z * dir.z).sqrt();
            if radial_len > 1e-9 {
                Vector3::new(
                    dir.x * *radius / radial_len,
                    y,
                    dir.z * *radius / radial_len,
                )
            } else {
                Vector3::new(0.0, y, 0.0)
            }
        }
        Shape3D::ConvexPolygonShape { points } => {
            let mut best = Vector3::ZERO;
            let mut best_dot = f32::NEG_INFINITY;
            for p in points {
                let d = p.dot(dir);
                if d > best_dot {
                    best_dot = d;
                    best = *p;
                }
            }
            best
        }
        // Mirrors `GodotSeparationRayShape3D::get_support`: the ray runs from
        // the origin along local +Z.
        Shape3D::SeparationRayShape { length, .. } => {
            if dir.z > 0.0 {
                Vector3::new(0.0, 0.0, *length)
            } else {
                Vector3::ZERO
            }
        }
        // Not fed through GJK: concave surfaces are decomposed into triangles
        // by the callers above, and a half-space is unbounded.
        Shape3D::ConcavePolygonShape { .. }
        | Shape3D::HeightMapShape { .. }
        | Shape3D::WorldBoundaryShape { .. } => Vector3::ZERO,
    }
}

/// World-space support point of `shape` along world-space `dir`.
fn world_support(shape: &Shape3D, center: Vector3, rot: Quaternion, dir: Vector3) -> Vector3 {
    let local_dir = rot.inverse().xform(dir);
    center + rot.xform(local_support(shape, local_dir))
}

/// GJK phase only: evolves a simplex over the Minkowski difference and returns
/// the enclosing tetrahedron when the two shapes overlap.
fn gjk_simplex<F: Fn(Vector3) -> Vector3>(support: &F, seed: Vector3) -> Option<Vec<Vector3>> {
    let mut simplex: Vec<Vector3> = Vec::with_capacity(4);
    let mut dir = seed;
    if dir.length_squared() < 1e-12 {
        dir = Vector3::new(1.0, 0.0, 0.0);
    }
    let first = support(dir);
    simplex.push(first);
    dir = first * -1.0;

    for _ in 0..64 {
        if dir.length_squared() < 1e-24 {
            dir = Vector3::new(1.0, 0.0, 0.0);
        }
        let a = support(dir);
        if a.dot(dir) < 0.0 {
            return None; // origin unreachable ⇒ separated
        }
        simplex.insert(0, a);
        if do_simplex(&mut simplex, &mut dir) {
            return (simplex.len() >= 4).then_some(simplex);
        }
    }
    None
}

/// GJK intersection test + EPA penetration for two bounded convex shapes.
fn gjk_epa(
    pos_a: Vector3,
    rot_a: Quaternion,
    shape_a: &Shape3D,
    pos_b: Vector3,
    rot_b: Quaternion,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    let support = |dir: Vector3| -> Vector3 {
        world_support(shape_a, pos_a, rot_a, dir) - world_support(shape_b, pos_b, rot_b, dir * -1.0)
    };

    match gjk_simplex(&support, pos_a - pos_b) {
        Some(simplex) => epa(&support, &simplex),
        None => CollisionResult3D::NONE,
    }
}

/// Boolean overlap test for two bounded convex shapes — the GJK half of
/// [`gjk_epa`], without the EPA expansion.
fn gjk_intersects(
    pos_a: Vector3,
    rot_a: Quaternion,
    shape_a: &Shape3D,
    pos_b: Vector3,
    rot_b: Quaternion,
    shape_b: &Shape3D,
) -> bool {
    let support = |dir: Vector3| -> Vector3 {
        world_support(shape_a, pos_a, rot_a, dir) - world_support(shape_b, pos_b, rot_b, dir * -1.0)
    };
    gjk_simplex(&support, pos_a - pos_b).is_some()
}

#[inline]
fn same_direction(a: Vector3, b: Vector3) -> bool {
    a.dot(b) > 0.0
}

/// Evolves the GJK simplex toward the origin. `simplex[0]` is the newest point.
/// Returns `true` when the simplex (a tetrahedron) encloses the origin.
fn do_simplex(simplex: &mut Vec<Vector3>, dir: &mut Vector3) -> bool {
    match simplex.len() {
        2 => {
            simplex_line(simplex, dir);
            false
        }
        3 => {
            simplex_triangle(simplex, dir);
            false
        }
        4 => simplex_tetra(simplex, dir),
        _ => false,
    }
}

fn simplex_line(simplex: &mut Vec<Vector3>, dir: &mut Vector3) {
    let a = simplex[0];
    let b = simplex[1];
    let ab = b - a;
    let ao = a * -1.0;
    if same_direction(ab, ao) {
        let mut d = ab.cross(ao).cross(ab);
        if d.length_squared() < 1e-24 {
            d = any_perpendicular(ab);
        }
        *dir = d;
    } else {
        simplex.clear();
        simplex.push(a);
        *dir = ao;
    }
}

fn simplex_triangle(simplex: &mut Vec<Vector3>, dir: &mut Vector3) {
    let a = simplex[0];
    let b = simplex[1];
    let c = simplex[2];
    let ab = b - a;
    let ac = c - a;
    let ao = a * -1.0;
    let abc = ab.cross(ac);

    if same_direction(abc.cross(ac), ao) {
        if same_direction(ac, ao) {
            *simplex = vec![a, c];
            let mut d = ac.cross(ao).cross(ac);
            if d.length_squared() < 1e-24 {
                d = any_perpendicular(ac);
            }
            *dir = d;
        } else {
            *simplex = vec![a, b];
            simplex_line(simplex, dir);
        }
    } else if same_direction(ab.cross(abc), ao) {
        *simplex = vec![a, b];
        simplex_line(simplex, dir);
    } else if same_direction(abc, ao) {
        *dir = abc;
    } else {
        *simplex = vec![a, c, b];
        *dir = abc * -1.0;
    }
}

fn simplex_tetra(simplex: &mut Vec<Vector3>, dir: &mut Vector3) -> bool {
    let a = simplex[0];
    let b = simplex[1];
    let c = simplex[2];
    let d = simplex[3];

    // Faces incident to the newest vertex `a`, with the opposite vertex.
    let faces = [(a, b, c, d), (a, c, d, b), (a, d, b, c)];
    for (p0, p1, p2, opp) in faces {
        let mut n = (p1 - p0).cross(p2 - p0);
        if n.dot(opp - p0) > 0.0 {
            n = n * -1.0; // orient outward (away from the opposite vertex)
        }
        if n.dot(p0 * -1.0) > 1e-9 {
            // Origin is outside this face — reduce to it and continue.
            *simplex = vec![p0, p1, p2];
            *dir = n;
            simplex_triangle(simplex, dir);
            return false;
        }
    }
    true
}

/// A triangular face of the EPA polytope with an outward unit normal and its
/// distance from the origin.
struct EpaFace {
    a: usize,
    b: usize,
    c: usize,
    normal: Vector3,
    dist: f32,
}

/// Builds a face from three vertices, orienting its normal outward (away from
/// the origin, which lies inside the polytope).
fn make_epa_face(verts: &[Vector3], a: usize, b: usize, c: usize) -> EpaFace {
    let mut normal = (verts[b] - verts[a]).cross(verts[c] - verts[a]);
    let len = normal.length();
    if len > 1e-12 {
        normal = normal * (1.0 / len);
    } else {
        normal = Vector3::ZERO;
    }
    let mut dist = normal.dot(verts[a]);
    if dist < 0.0 {
        normal = normal * -1.0;
        dist = -dist;
    }
    EpaFace {
        a,
        b,
        c,
        normal,
        dist,
    }
}

/// Records an undirected edge with multiplicity (for horizon extraction).
fn add_epa_edge(edges: &mut Vec<((usize, usize), i32)>, i: usize, j: usize) {
    let key = if i < j { (i, j) } else { (j, i) };
    for e in edges.iter_mut() {
        if e.0 == key {
            e.1 += 1;
            return;
        }
    }
    edges.push((key, 1));
}

/// Expanding Polytope Algorithm: given a GJK tetrahedron enclosing the origin,
/// finds the minimum penetration normal (A→B) and depth.
fn epa<F: Fn(Vector3) -> Vector3>(support: &F, simplex: &[Vector3]) -> CollisionResult3D {
    let mut verts: Vec<Vector3> = simplex.to_vec();
    let mut faces: Vec<EpaFace> = vec![
        make_epa_face(&verts, 0, 1, 2),
        make_epa_face(&verts, 0, 2, 3),
        make_epa_face(&verts, 0, 3, 1),
        make_epa_face(&verts, 1, 3, 2),
    ];

    let mut best_normal = Vector3::new(0.0, 1.0, 0.0);
    let mut best_depth = 0.0f32;

    for _ in 0..64 {
        // Closest non-degenerate face to the origin.
        let mut fi = usize::MAX;
        let mut best = f32::INFINITY;
        for (i, f) in faces.iter().enumerate() {
            if f.normal.length_squared() < 0.5 {
                continue; // degenerate
            }
            if f.dist < best {
                best = f.dist;
                fi = i;
            }
        }
        if fi == usize::MAX {
            break;
        }

        let normal = faces[fi].normal;
        best_normal = normal;
        best_depth = best;

        let p = support(normal);
        let d = p.dot(normal);
        if d - best < 1e-4 {
            return CollisionResult3D {
                colliding: true,
                normal,
                depth: best,
            };
        }
        if verts.iter().any(|v| (*v - p).length_squared() < 1e-12) {
            return CollisionResult3D {
                colliding: true,
                normal,
                depth: best,
            };
        }

        let new_i = verts.len();
        verts.push(p);

        let mut edges: Vec<((usize, usize), i32)> = Vec::new();
        let mut kept: Vec<EpaFace> = Vec::new();
        for f in faces.drain(..) {
            if f.normal.dot(p - verts[f.a]) > 1e-9 {
                add_epa_edge(&mut edges, f.a, f.b);
                add_epa_edge(&mut edges, f.b, f.c);
                add_epa_edge(&mut edges, f.c, f.a);
            } else {
                kept.push(f);
            }
        }
        for ((i, j), count) in &edges {
            if *count == 1 {
                let face = make_epa_face(&verts, *i, *j, new_i);
                if face.normal.length_squared() > 0.5 {
                    kept.push(face);
                }
            }
        }
        faces = kept;
        if faces.is_empty() {
            break;
        }
    }

    CollisionResult3D {
        colliding: true,
        normal: best_normal,
        depth: best_depth.max(1e-4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincident_spheres_collide_at_full_depth() {
        let r = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::ZERO, 0.5);
        assert!(r.colliding);
        assert!((r.depth - 1.5).abs() < 1e-6);
        assert!(r.normal.length_squared() > 0.99, "normal must be finite and unit");
    }

    #[test]
    fn sphere_sphere_overlap() {
        let result = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::new(1.5, 0.0, 0.0), 1.0);
        assert!(result.colliding);
        assert!((result.depth - 0.5).abs() < 1e-5);
        assert!((result.normal.x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn sphere_sphere_no_overlap() {
        let result = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::new(3.0, 0.0, 0.0), 1.0);
        assert!(!result.colliding);
    }

    #[test]
    fn sphere_sphere_touching() {
        let result = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::new(2.0, 0.0, 0.0), 1.0);
        assert!(!result.colliding, "exactly touching should not collide");
    }

    #[test]
    fn test_collision_dispatch() {
        let a = Shape3D::Sphere { radius: 1.0 };
        let b = Shape3D::Sphere { radius: 1.0 };
        let result = test_collision(Vector3::ZERO, &a, Vector3::new(1.0, 0.0, 0.0), &b);
        assert!(result.colliding);
    }

    #[test]
    fn box_sphere_overlap_at_center() {
        // Sphere centered inside box should collide.
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::Sphere { radius: 1.0 };
        let result = test_collision(Vector3::ZERO, &a, Vector3::ZERO, &b);
        assert!(result.colliding, "sphere inside box should collide");
        assert!(result.depth > 0.0);
    }

    #[test]
    fn box_box_overlap() {
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let result = test_collision(Vector3::ZERO, &a, Vector3::new(1.5, 0.0, 0.0), &b);
        assert!(result.colliding);
        assert!((result.depth - 0.5).abs() < 1e-5);
    }

    #[test]
    fn box_box_no_overlap() {
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let result = test_collision(Vector3::ZERO, &a, Vector3::new(3.0, 0.0, 0.0), &b);
        assert!(!result.colliding);
    }

    #[test]
    fn sphere_inside_box_detects_collision() {
        let result = test_sphere_box(
            Vector3::new(0.5, 0.0, 0.0),
            0.5,
            Vector3::ZERO,
            Vector3::new(2.0, 2.0, 2.0),
        );
        assert!(result.colliding, "sphere inside box should collide");
        assert!(result.depth > 0.0);
    }

    // -- Rotation-aware matrix (pat-u3l8c.54) --------------------------------

    fn yaw(deg: f32) -> Quaternion {
        Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), deg.to_radians())
    }

    #[test]
    fn capsule_sphere_side_contact() {
        // Vertical capsule at origin (r=0.5, h=2) and a sphere touching its side.
        let cap = Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        };
        let sphere = Shape3D::Sphere { radius: 0.5 };
        let res = test_collision(Vector3::ZERO, &cap, Vector3::new(0.8, 0.0, 0.0), &sphere);
        assert!(
            res.colliding,
            "sphere overlapping capsule side should collide"
        );
        // Normal A(capsule)→B(sphere) points +x.
        assert!(
            res.normal.x > 0.9,
            "normal should point toward sphere, got {:?}",
            res.normal
        );
        assert!(
            (res.depth - 0.2).abs() < 1e-4,
            "depth ~0.2, got {}",
            res.depth
        );
    }

    #[test]
    fn capsule_sphere_no_contact() {
        let cap = Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        };
        let sphere = Shape3D::Sphere { radius: 0.5 };
        let res = test_collision(Vector3::ZERO, &cap, Vector3::new(3.0, 0.0, 0.0), &sphere);
        assert!(!res.colliding);
    }

    #[test]
    fn capsule_capsule_crossing() {
        // Vertical capsule vs horizontal capsule crossing at origin.
        let vcap = Shape3D::CapsuleShape {
            radius: 0.4,
            height: 2.0,
        };
        let hcap = Shape3D::CapsuleShape {
            radius: 0.4,
            height: 2.0,
        };
        // Rotate B 90° about Z so it lies along X.
        let rot_b = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 90f32.to_radians());
        let res = test_collision_rot(
            Vector3::ZERO,
            Quaternion::IDENTITY,
            &vcap,
            Vector3::new(0.5, 0.0, 0.0),
            rot_b,
            &hcap,
        );
        assert!(res.colliding, "crossing capsules should collide");
        assert!(res.depth > 0.0);
    }

    #[test]
    fn capsule_capsule_far_apart() {
        let cap = Shape3D::CapsuleShape {
            radius: 0.4,
            height: 2.0,
        };
        let res = test_collision(Vector3::ZERO, &cap, Vector3::new(5.0, 0.0, 0.0), &cap);
        assert!(!res.colliding);
    }

    #[test]
    fn sphere_cylinder_side_and_cap() {
        let cyl = Shape3D::CylinderShape {
            radius: 1.0,
            height: 2.0,
        };
        let sphere = Shape3D::Sphere { radius: 0.5 };
        // Side contact.
        let side = test_collision(Vector3::ZERO, &cyl, Vector3::new(1.3, 0.0, 0.0), &sphere);
        assert!(side.colliding, "sphere at cylinder side should collide");
        assert!(side.normal.x > 0.9);
        // Cap contact (above the top face at y=1).
        let cap = test_collision(Vector3::ZERO, &cyl, Vector3::new(0.0, 1.3, 0.0), &sphere);
        assert!(cap.colliding, "sphere above cylinder cap should collide");
        assert!(cap.normal.y > 0.9);
        // Clear of the cylinder.
        let miss = test_collision(Vector3::ZERO, &cyl, Vector3::new(0.0, 3.0, 0.0), &sphere);
        assert!(!miss.colliding);
    }

    #[test]
    fn box_box_rotated_collides() {
        // Two unit boxes; B rotated 45° about Y and pushed along X so the corner
        // pokes into A. Axis-aligned they would be clear at this distance.
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        // A rotated box's half-diagonal in XZ is sqrt(2) ≈ 1.414. Place B at
        // x=2.2: axis-aligned (half-widths 1+1=2) would NOT overlap, but the
        // rotated corner (reach 1.414) plus A's 1.0 = 2.414 > 2.2 overlaps.
        let res = test_collision_rot(
            Vector3::ZERO,
            Quaternion::IDENTITY,
            &a,
            Vector3::new(2.2, 0.0, 0.0),
            yaw(45.0),
            &b,
        );
        assert!(res.colliding, "rotated box corner should poke into box A");
        assert!(res.depth > 0.0);
        // Separating normal should be essentially along X.
        assert!(
            res.normal.x.abs() > 0.8,
            "normal mostly X, got {:?}",
            res.normal
        );
    }

    #[test]
    fn box_box_rotated_clear() {
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        // At x=2.6 the rotated corner reach (2.414) no longer overlaps A.
        let res = test_collision_rot(
            Vector3::ZERO,
            Quaternion::IDENTITY,
            &a,
            Vector3::new(2.6, 0.0, 0.0),
            yaw(45.0),
            &b,
        );
        assert!(!res.colliding, "rotated boxes should be separated");
    }

    #[test]
    fn capsule_on_box_floor() {
        // A wide, flat box acts as the floor; a vertical capsule rests on it,
        // overlapping slightly. The capsule must be pushed up (+Y).
        let floor = Shape3D::BoxShape {
            half_extents: Vector3::new(5.0, 0.5, 5.0),
        };
        let capsule = Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        };
        // Floor top at y=0.5. Capsule bottom cap center at y = cap_y - 0.5.
        // Place capsule so its bottom sphere overlaps the floor top by ~0.1.
        // cap_y - (0.5) - 0.5(radius) = 0.4  ⇒ cap_y = 1.4 gives bottom at 0.4,
        // overlapping the y=0.5 top by 0.1.
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 1.4, 0.0), &capsule);
        assert!(res.colliding, "capsule should rest on the box floor");
        // Normal A(floor)→B(capsule) should push the capsule up.
        assert!(
            res.normal.y > 0.8,
            "floor→capsule normal should be +Y, got {:?}",
            res.normal
        );
        assert!(
            res.depth > 0.0 && res.depth < 0.3,
            "shallow rest depth, got {}",
            res.depth
        );
    }

    #[test]
    fn worldboundary_plane_stops_sphere() {
        // Ground plane: solid half-space is y <= 0 (normal +Y, distance 0).
        let plane = Shape3D::WorldBoundaryShape {
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: 0.0,
        };
        let sphere = Shape3D::Sphere { radius: 1.0 };
        // Sphere center at y=0.5 ⇒ dips 0.5 below the plane.
        let hit = test_collision(Vector3::ZERO, &plane, Vector3::new(0.0, 0.5, 0.0), &sphere);
        assert!(
            hit.colliding,
            "sphere crossing the ground plane should collide"
        );
        assert!(
            hit.normal.y > 0.9,
            "plane→sphere normal is +Y, got {:?}",
            hit.normal
        );
        assert!(
            (hit.depth - 0.5).abs() < 1e-4,
            "depth ~0.5, got {}",
            hit.depth
        );

        // Sphere well above the plane does not collide.
        let miss = test_collision(Vector3::ZERO, &plane, Vector3::new(0.0, 5.0, 0.0), &sphere);
        assert!(!miss.colliding);
    }

    #[test]
    fn worldboundary_plane_order_independent() {
        let plane = Shape3D::WorldBoundaryShape {
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: 0.0,
        };
        let sphere = Shape3D::Sphere { radius: 1.0 };
        // Convex as A, plane as B: normal A→B points from sphere toward plane (−Y).
        let hit = test_collision(Vector3::new(0.0, 0.5, 0.0), &sphere, Vector3::ZERO, &plane);
        assert!(hit.colliding);
        assert!(
            hit.normal.y < -0.9,
            "sphere→plane normal is −Y, got {:?}",
            hit.normal
        );
    }

    #[test]
    fn box_capsule_gjk_matches_side() {
        // Box vs vertical capsule side contact — routed through GJK/EPA.
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let cap = Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        };
        // Capsule center at x=1.3: its radius 0.5 reaches x=0.8, box face at x=1.
        let res = test_collision(Vector3::ZERO, &b, Vector3::new(1.3, 0.0, 0.0), &cap);
        assert!(res.colliding, "box and capsule side should collide");
        assert!(
            res.normal.x > 0.8,
            "box→capsule normal +X, got {:?}",
            res.normal
        );
        assert!(
            (res.depth - 0.2).abs() < 0.05,
            "depth ~0.2, got {}",
            res.depth
        );
    }

    #[test]
    fn cylinder_cylinder_gjk() {
        let cyl = Shape3D::CylinderShape {
            radius: 1.0,
            height: 2.0,
        };
        // Two vertical cylinders side by side overlapping along X.
        let hit = test_collision(Vector3::ZERO, &cyl, Vector3::new(1.5, 0.0, 0.0), &cyl);
        assert!(hit.colliding, "overlapping cylinders should collide");
        assert!(hit.normal.x.abs() > 0.8);
        assert!(
            (hit.depth - 0.5).abs() < 0.1,
            "depth ~0.5, got {}",
            hit.depth
        );

        let miss = test_collision(Vector3::ZERO, &cyl, Vector3::new(2.5, 0.0, 0.0), &cyl);
        assert!(!miss.colliding);
    }

    #[test]
    fn gjk_matches_analytic_box_box() {
        // GJK/EPA on a rotated-by-0 box pair must agree with the analytic AABB
        // result (depth 0.5 along +X).
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let res = gjk_epa(
            Vector3::ZERO,
            Quaternion::IDENTITY,
            &a,
            Vector3::new(1.5, 0.0, 0.0),
            Quaternion::IDENTITY,
            &b,
        );
        assert!(res.colliding);
        assert!(
            (res.depth - 0.5).abs() < 0.05,
            "gjk depth ~0.5, got {}",
            res.depth
        );
        assert!(
            res.normal.x.abs() > 0.9,
            "gjk normal ~X, got {:?}",
            res.normal
        );
    }

    // -- Convex hull geometry + concave trimesh (pat-u3l8c.56) ---------------

    fn unit_cube_points() -> Vec<Vector3> {
        vec![
            Vector3::new(-1.0, -1.0, -1.0),
            Vector3::new(1.0, -1.0, -1.0),
            Vector3::new(1.0, 1.0, -1.0),
            Vector3::new(-1.0, 1.0, -1.0),
            Vector3::new(-1.0, -1.0, 1.0),
            Vector3::new(1.0, -1.0, 1.0),
            Vector3::new(1.0, 1.0, 1.0),
            Vector3::new(-1.0, 1.0, 1.0),
        ]
    }

    /// A flat 4×4 quad floor in the XZ plane at y=0, as a two-triangle soup.
    fn floor_faces() -> Vec<Vector3> {
        let v0 = Vector3::new(-2.0, 0.0, -2.0);
        let v1 = Vector3::new(2.0, 0.0, -2.0);
        let v2 = Vector3::new(2.0, 0.0, 2.0);
        let v3 = Vector3::new(-2.0, 0.0, 2.0);
        vec![v0, v1, v2, v0, v2, v3]
    }

    #[test]
    fn convex_hull_vs_sphere_collides() {
        let cube = Shape3D::ConvexPolygonShape {
            points: unit_cube_points(),
        };
        let sphere = Shape3D::Sphere { radius: 1.0 };
        // Sphere centre at x=1.5 overlaps the hull face at x=1 by 0.5.
        let res = test_collision(Vector3::ZERO, &cube, Vector3::new(1.5, 0.0, 0.0), &sphere);
        assert!(
            res.colliding,
            "populated convex hull should collide with sphere"
        );
        assert!(res.depth > 0.0);
    }

    #[test]
    fn convex_hull_vs_sphere_misses_when_separated() {
        let cube = Shape3D::ConvexPolygonShape {
            points: unit_cube_points(),
        };
        let sphere = Shape3D::Sphere { radius: 1.0 };
        // Sphere centre at x=3.0 leaves a 1.0 gap to the hull face at x=1.
        let res = test_collision(Vector3::ZERO, &cube, Vector3::new(3.0, 0.0, 0.0), &sphere);
        assert!(!res.colliding);
    }

    #[test]
    fn trimesh_vs_sphere_collides() {
        let floor = Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: false,
        };
        let sphere = Shape3D::Sphere { radius: 0.5 };
        // Sphere centre at y=0.4 dips 0.1 into the floor.
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 0.4, 0.0), &sphere);
        assert!(res.colliding, "sphere resting on trimesh should collide");
        assert!(
            (res.depth - 0.1).abs() < 1e-4,
            "depth ~0.1, got {}",
            res.depth
        );
        assert!(
            res.normal.y > 0.9,
            "normal up toward sphere, got {:?}",
            res.normal
        );
    }

    #[test]
    fn trimesh_vs_sphere_misses_when_above() {
        let floor = Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: false,
        };
        let sphere = Shape3D::Sphere { radius: 0.5 };
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 2.0, 0.0), &sphere);
        assert!(!res.colliding, "sphere well above floor should not collide");
    }

    #[test]
    fn trimesh_vs_sphere_normal_flips_with_order() {
        // Sphere as shape A, trimesh as B: normal must follow the A→B convention.
        let floor = Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: false,
        };
        let sphere = Shape3D::Sphere { radius: 0.5 };
        let res = test_collision(Vector3::new(0.0, 0.4, 0.0), &sphere, Vector3::ZERO, &floor);
        assert!(res.colliding);
        assert!(
            res.normal.y < -0.9,
            "A(sphere)->B(floor) points down, got {:?}",
            res.normal
        );
    }

    #[test]
    fn trimesh_vs_capsule_collides() {
        let floor = Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: false,
        };
        let capsule = Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        };
        // Vertical capsule whose lower half dips through the floor.
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 0.4, 0.0), &capsule);
        assert!(res.colliding, "capsule crossing trimesh should collide");
        assert!(res.depth > 0.0);
    }

    #[test]
    fn trimesh_vs_capsule_misses_when_above() {
        let floor = Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: false,
        };
        let capsule = Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        };
        // Capsule high above: its lowest core point is y=2.5, well clear.
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 3.0, 0.0), &capsule);
        assert!(!res.colliding);
    }

    // --- pat-0q2kx.119: concave vs every convex shape, heightmap, ray. ------

    fn trimesh_floor(backface: bool) -> Shape3D {
        Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: backface,
        }
    }

    #[test]
    fn godot_face_normal_matches_plane_winding() {
        // `floor_faces()` is wound so Godot's `Plane(a, b, c)` normal points up,
        // the opposite sign from the raw (b-a)×(c-a) cross product.
        let f = floor_faces();
        let n = godot_face_normal(f[0], f[1], f[2]).expect("non-degenerate");
        assert!(n.y > 0.99, "front face must point up, got {n:?}");
        let raw = triangle_normal(f[0], f[1], f[2]);
        assert!(raw.y < -0.99, "raw cross product points the other way");
        // Degenerate triangles are rejected rather than yielding a bogus axis.
        let p = Vector3::new(1.0, 2.0, 3.0);
        assert!(godot_face_normal(p, p, p).is_none());
    }

    #[test]
    fn trimesh_vs_box_collides() {
        let floor = trimesh_floor(false);
        let cube = Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        };
        // Box centre at y=0.4 ⇒ its underside dips 0.1 below the floor.
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 0.4, 0.0), &cube);
        assert!(res.colliding, "box resting on trimesh must collide");
        assert!(
            (res.depth - 0.1).abs() < 1e-4,
            "depth ~0.1, got {}",
            res.depth
        );
        assert!(
            res.normal.y > 0.99,
            "normal must push the box up, got {:?}",
            res.normal
        );
    }

    #[test]
    fn trimesh_vs_box_misses_beyond_the_soup() {
        let floor = trimesh_floor(false);
        let cube = Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        };
        // Below the plane's height but far outside the triangles' extent: the
        // plane test alone would report a hit, the per-triangle test must not.
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(20.0, 0.4, 0.0), &cube);
        assert!(!res.colliding, "a box off the end of the soup must not hit");
    }

    #[test]
    fn trimesh_vs_cylinder_and_convex_collide() {
        let floor = trimesh_floor(false);
        let cylinder = Shape3D::CylinderShape {
            radius: 0.5,
            height: 1.0,
        };
        let res = test_collision(
            Vector3::ZERO,
            &floor,
            Vector3::new(0.0, 0.4, 0.0),
            &cylinder,
        );
        assert!(res.colliding, "cylinder on trimesh must collide");
        assert!(res.normal.y > 0.99, "got {:?}", res.normal);

        let hull = Shape3D::ConvexPolygonShape {
            points: vec![
                Vector3::new(-0.5, -0.5, -0.5),
                Vector3::new(0.5, -0.5, -0.5),
                Vector3::new(0.5, -0.5, 0.5),
                Vector3::new(-0.5, -0.5, 0.5),
                Vector3::new(0.0, 0.5, 0.0),
            ],
        };
        let res = test_collision(Vector3::ZERO, &floor, Vector3::new(0.0, 0.4, 0.0), &hull);
        assert!(res.colliding, "convex hull on trimesh must collide");
        assert!(res.normal.y > 0.99, "got {:?}", res.normal);
    }

    #[test]
    fn trimesh_backface_flag_gates_contacts_from_behind() {
        let cube = Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        };
        // Box centre below the floor ⇒ approaching the face from behind.
        let below = Vector3::new(0.0, -0.4, 0.0);
        let front_only = test_collision(Vector3::ZERO, &trimesh_floor(false), below, &cube);
        assert!(
            !front_only.colliding,
            "backface_collision=false must ignore a contact from behind"
        );

        let two_sided = test_collision(Vector3::ZERO, &trimesh_floor(true), below, &cube);
        assert!(
            two_sided.colliding,
            "backface_collision=true must report the same contact"
        );
        assert!(
            two_sided.normal.y < -0.99,
            "the rear contact pushes the box down, got {:?}",
            two_sided.normal
        );
    }

    #[test]
    fn heightmap_vs_box_rests_at_the_sampled_height() {
        // 4x4 grid, flat at y=2, spanning x,z ∈ [-1.5, 1.5].
        let terrain = Shape3D::HeightMapShape {
            map_width: 4,
            map_depth: 4,
            map_data: vec![2.0; 16],
        };
        let cube = Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        };
        // Centre at 2.4 ⇒ underside 1.9, i.e. 0.1 into the surface.
        let res = test_collision(Vector3::ZERO, &terrain, Vector3::new(0.0, 2.4, 0.0), &cube);
        assert!(res.colliding, "box must rest on the heightmap surface");
        assert!(
            (res.depth - 0.1).abs() < 1e-4,
            "depth ~0.1, got {}",
            res.depth
        );
        assert!(res.normal.y > 0.99, "got {:?}", res.normal);

        // Well above the surface: no contact.
        let clear = test_collision(Vector3::ZERO, &terrain, Vector3::new(0.0, 5.0, 0.0), &cube);
        assert!(!clear.colliding);
        // Off the edge of the grid: no contact either.
        let outside = test_collision(Vector3::ZERO, &terrain, Vector3::new(9.0, 2.4, 0.0), &cube);
        assert!(!outside.colliding);
    }

    #[test]
    fn heightmap_contact_follows_its_height_data() {
        // Two identical grids that differ only in `map_data`: the contact must
        // move with the sampled heights, not sit at y=0 or at the AABB.
        let low = Shape3D::HeightMapShape {
            map_width: 4,
            map_depth: 4,
            map_data: vec![0.0; 16],
        };
        let high = Shape3D::HeightMapShape {
            map_width: 4,
            map_depth: 4,
            map_data: vec![3.0; 16],
        };
        let cube = Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        };
        let at = Vector3::new(0.0, 0.4, 0.0);

        let res = test_collision(Vector3::ZERO, &low, at, &cube);
        assert!(res.colliding, "box must rest on the y=0 grid");
        assert!((res.depth - 0.1).abs() < 1e-4, "got {}", res.depth);

        // The same box is entirely *under* the 3.0-high grid, i.e. behind its
        // (upward) front faces — which the heightmap ignores, as Godot does.
        let res = test_collision(Vector3::ZERO, &high, at, &cube);
        assert!(
            !res.colliding,
            "a box beneath the raised grid must not be caught by it"
        );

        // Raised to match, it rests on the high grid instead.
        let res = test_collision(Vector3::ZERO, &high, Vector3::new(0.0, 3.4, 0.0), &cube);
        assert!(res.colliding, "box must rest on the y=3 grid");
        assert!((res.depth - 0.1).abs() < 1e-4, "got {}", res.depth);
    }

    #[test]
    fn separation_ray_hits_and_respects_slide_on_slope() {
        // Ray runs along local +Z (Godot's axis) for 2 units, into a box.
        let ray_at = Vector3::new(0.0, 0.0, -1.0);
        let target = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };

        let forced = Shape3D::SeparationRayShape {
            length: 2.0,
            slide_on_slope: false,
        };
        let res = test_collision(ray_at, &forced, Vector3::ZERO, &target);
        assert!(res.colliding, "a ray reaching into the box must collide");
        assert!(
            res.normal.z > 0.99,
            "without slide_on_slope the recovery follows the ray axis, got {:?}",
            res.normal
        );
        // Tip at z=1.0, box near face at z=-1.0 ⇒ 2.0 units of penetration.
        assert!((res.depth - 2.0).abs() < 1e-4, "got {}", res.depth);

        let sliding = Shape3D::SeparationRayShape {
            length: 2.0,
            slide_on_slope: true,
        };
        let res = test_collision(ray_at, &sliding, Vector3::ZERO, &target);
        assert!(res.colliding);

        // A ray that stops short of the box does not collide either way.
        let short = Shape3D::SeparationRayShape {
            length: 0.1,
            slide_on_slope: false,
        };
        let res = test_collision(Vector3::new(0.0, 0.0, -3.0), &short, Vector3::ZERO, &target);
        assert!(!res.colliding, "a short ray must not reach the box");
    }

    #[test]
    fn separation_ray_normal_flips_with_argument_order() {
        let ray = Shape3D::SeparationRayShape {
            length: 2.0,
            slide_on_slope: false,
        };
        let target = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let ab = test_collision(Vector3::new(0.0, 0.0, -1.0), &ray, Vector3::ZERO, &target);
        let ba = test_collision(Vector3::ZERO, &target, Vector3::new(0.0, 0.0, -1.0), &ray);
        assert!(ab.colliding && ba.colliding);
        assert!(
            (ab.normal + ba.normal).length() < 1e-4,
            "A→B convention: {:?} vs {:?}",
            ab.normal,
            ba.normal
        );
    }

    #[test]
    fn closest_point_on_triangle_projects_and_clamps() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(2.0, 0.0, 0.0);
        let c = Vector3::new(0.0, 0.0, 2.0);
        // Point above the interior projects straight down onto the triangle.
        let cp = closest_point_on_triangle(Vector3::new(0.5, 5.0, 0.5), a, b, c);
        assert!((cp - Vector3::new(0.5, 0.0, 0.5)).length() < 1e-5);
        // Point beyond vertex A clamps to A.
        let cp2 = closest_point_on_triangle(Vector3::new(-3.0, 0.0, -3.0), a, b, c);
        assert!((cp2 - a).length() < 1e-5);
    }
}
