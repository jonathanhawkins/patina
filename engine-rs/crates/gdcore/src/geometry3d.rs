//! 3D geometry utilities matching Godot's `Geometry3D` singleton
//! (pat-nzgdu.154, Geometry3D algorithm core).
//!
//! Pure, self-contained functions over [`crate::math::Vector3`] providing the
//! closest-feature and segment/ray intersection queries of Godot's
//! `Geometry3D`: closest points between two segments, closest point on a
//! segment, and segment/ray-versus-triangle and segment-versus-sphere
//! intersection.
//!
//! # Scope / remaining work (pat-nzgdu.154)
//!
//! The bead's full acceptance is that **GDScript** can call
//! `Geometry2D.*` and `Geometry3D.*` — i.e. routing these through the
//! interpreter's singleton dispatch — plus the polygon-clipping side of
//! `Geometry2D` (`convex_hull`, `intersect/merge/clip/exclude_polygons`,
//! `triangulate_delaunay`, …). This module delivers only the **Geometry3D
//! computational core** (the two Geometry3D functions the acceptance test names,
//! `get_closest_points_between_segments` and `segment_intersects_triangle`, are
//! both here). The script-singleton binding and the Geometry2D polygon ops are
//! NOT implemented here.
//!
//! Also note: `crate::` currently has a `geometry2d.rs` file that is **not
//! declared** in `lib.rs` (orphaned / not compiled). Whoever wires the singleton
//! dispatch will need to add `pub mod geometry2d;` (and verify it still builds)
//! before it can be reused as the bead suggests.

use crate::math::Vector3;
use crate::math3d::Plane;

/// Tolerance for treating a squared length / determinant as zero.
const EPS: f32 = 1e-8;

/// Returns the pair of closest points `(on_ab, on_cd)` between segment `a`–`b`
/// and segment `c`–`d`. Mirrors Godot's
/// `Geometry3D.get_closest_points_between_segments`.
pub fn get_closest_points_between_segments(
    a: Vector3,
    b: Vector3,
    c: Vector3,
    d: Vector3,
) -> (Vector3, Vector3) {
    let d1 = b - a; // direction and length of segment 1
    let d2 = d - c; // direction and length of segment 2
    let r = a - c;
    let len1 = d1.dot(d1);
    let len2 = d2.dot(d2);
    let f = d2.dot(r);

    let (s, t) = {
        if len1 <= EPS && len2 <= EPS {
            // Both segments are points.
            return (a, c);
        } else if len1 <= EPS {
            // Segment 1 is a point.
            (0.0, (f / len2).clamp(0.0, 1.0))
        } else {
            let cc = d1.dot(r);
            if len2 <= EPS {
                // Segment 2 is a point.
                ((-cc / len1).clamp(0.0, 1.0), 0.0)
            } else {
                let b12 = d1.dot(d2);
                let denom = len1 * len2 - b12 * b12;
                let s0 = if denom.abs() > EPS {
                    ((b12 * f - cc * len2) / denom).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let t0 = (b12 * s0 + f) / len2;
                if t0 < 0.0 {
                    ((-cc / len1).clamp(0.0, 1.0), 0.0)
                } else if t0 > 1.0 {
                    (((b12 - cc) / len1).clamp(0.0, 1.0), 1.0)
                } else {
                    (s0, t0)
                }
            }
        }
    };

    (a + d1 * s, c + d2 * t)
}

/// Returns the point on segment `a`–`b` closest to `point` (clamped to the
/// segment endpoints). Mirrors `Geometry3D.get_closest_point_to_segment`.
pub fn get_closest_point_to_segment(point: Vector3, a: Vector3, b: Vector3) -> Vector3 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 <= EPS {
        return a;
    }
    let t = ((point - a).dot(ab) / len2).clamp(0.0, 1.0);
    a + ab * t
}

/// Returns the point on the infinite line through `a` and `b` closest to
/// `point` (not clamped). Mirrors `Geometry3D.get_closest_point_to_segment_uncapped`.
pub fn get_closest_point_to_segment_uncapped(point: Vector3, a: Vector3, b: Vector3) -> Vector3 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 <= EPS {
        return a;
    }
    let t = (point - a).dot(ab) / len2;
    a + ab * t
}

/// Returns the intersection point of segment `from`–`to` with triangle
/// `v0,v1,v2`, or `None` if they do not intersect. Möller–Trumbore with the
/// hit parameter constrained to the segment. Mirrors
/// `Geometry3D.segment_intersects_triangle`.
pub fn segment_intersects_triangle(
    from: Vector3,
    to: Vector3,
    v0: Vector3,
    v1: Vector3,
    v2: Vector3,
) -> Option<Vector3> {
    let dir = to - from;
    moller_trumbore(from, dir, v0, v1, v2).and_then(|t| {
        if (0.0..=1.0).contains(&t) {
            Some(from + dir * t)
        } else {
            None
        }
    })
}

/// Returns the intersection point of the ray `from` + `t`·`dir` (`t >= 0`) with
/// triangle `v0,v1,v2`, or `None`. Mirrors `Geometry3D.ray_intersects_triangle`.
pub fn ray_intersects_triangle(
    from: Vector3,
    dir: Vector3,
    v0: Vector3,
    v1: Vector3,
    v2: Vector3,
) -> Option<Vector3> {
    moller_trumbore(from, dir, v0, v1, v2).and_then(|t| {
        if t >= 0.0 {
            Some(from + dir * t)
        } else {
            None
        }
    })
}

/// Core Möller–Trumbore test: returns the ray parameter `t` (in units of `dir`)
/// where `from` + `t`·`dir` meets the triangle, or `None` if the line misses
/// the triangle or is parallel to its plane.
fn moller_trumbore(
    from: Vector3,
    dir: Vector3,
    v0: Vector3,
    v1: Vector3,
    v2: Vector3,
) -> Option<f32> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < EPS {
        return None; // parallel to the triangle plane
    }
    let inv = 1.0 / det;
    let tv = from - v0;
    let u = tv.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tv.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

/// Returns the first intersection point of segment `from`–`to` with the sphere
/// at `center` with `radius`, or `None`. Mirrors
/// `Geometry3D.segment_intersects_sphere`.
pub fn segment_intersects_sphere(
    from: Vector3,
    to: Vector3,
    center: Vector3,
    radius: f32,
) -> Option<Vector3> {
    let dir = to - from;
    let seg_len2 = dir.dot(dir);
    if seg_len2 <= EPS {
        // Degenerate segment: hit iff the point is within the sphere.
        return if (from - center).length() <= radius {
            Some(from)
        } else {
            None
        };
    }
    let seg_len = seg_len2.sqrt();
    let u = dir * (1.0 / seg_len);
    let oc = from - center;
    let b = oc.dot(u);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    // Nearest root first (t0 <= t1).
    for t in [-b - sq, -b + sq] {
        if (0.0..=seg_len).contains(&t) {
            return Some(from + u * t);
        }
    }
    None
}

/// Barycentric weights of `point` inside triangle `a`, `b`, `c`. Mirrors
/// `Geometry3D.get_triangle_barycentric_coords`.
///
/// Ported from `upstream/godot/core/math/geometry_3d.h:814`
/// (`triangle_get_barycentric_coords`).
///
/// ⚠️ The argument order here is the SCRIPT-facing one, `(point, a, b, c)`, not
/// the internal Godot helper's `(a, b, c, pos)`. Godot's bind layer reorders
/// them (`core_bind.cpp:1205`), so porting the internal signature verbatim
/// would silently swap the queried point with the first vertex — the kind of
/// swap a relational assertion cannot see, because the weights still sum to 1.
///
/// A degenerate triangle returns `Vector3::ZERO`, matching Godot's
/// `return Vector3()` rather than propagating a division by zero as NaNs.
pub fn get_triangle_barycentric_coords(
    point: Vector3,
    a: Vector3,
    b: Vector3,
    c: Vector3,
) -> Vector3 {
    let v0 = b - a;
    let v1 = c - a;
    let v2 = point - a;

    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);

    let denom = d00 * d11 - d01 * d01;
    if denom == 0.0 {
        return Vector3::ZERO;
    }

    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    Vector3::new(1.0 - v - w, v, w)
}

/// The six planes bounding a box centred on the origin, where `extents` is one
/// positive corner (half the box's size). Mirrors
/// `Geometry3D.build_box_planes`.
///
/// Ported from `upstream/godot/core/math/geometry_3d.cpp:727`.
///
/// ⚠️ Every plane's distance is the POSITIVE extent, including the three whose
/// normals point down the negative axes. That is Godot's convention and it is
/// the opposite of what "signed distance along the normal" suggests: negating
/// them to agree with the normal would move three faces to the wrong side of
/// the origin, collapsing the box. Order is +X, -X, +Y, -Y, +Z, -Z, which the
/// caller may rely on.
pub fn build_box_planes(extents: Vector3) -> [Plane; 6] {
    [
        Plane::new(Vector3::new(1.0, 0.0, 0.0), extents.x),
        Plane::new(Vector3::new(-1.0, 0.0, 0.0), extents.x),
        Plane::new(Vector3::new(0.0, 1.0, 0.0), extents.y),
        Plane::new(Vector3::new(0.0, -1.0, 0.0), extents.y),
        Plane::new(Vector3::new(0.0, 0.0, 1.0), extents.z),
        Plane::new(Vector3::new(0.0, 0.0, -1.0), extents.z),
    ]
}

/// Writes `value` into component `i` (0 = x, 1 = y, 2 = z) of `v`.
///
/// Godot's geometry code indexes a `Vector3` numerically (`normal[(axis+1)%3]`)
/// to stay axis-agnostic; `Vector3` here has named fields, so the indexing is
/// spelled out rather than the loops being unrolled per axis.
fn with_component(v: Vector3, i: usize, value: f32) -> Vector3 {
    match i {
        0 => Vector3::new(value, v.y, v.z),
        1 => Vector3::new(v.x, value, v.z),
        _ => Vector3::new(v.x, v.y, value),
    }
}

/// Component-wise product, Godot's `Vector3 * Vector3`.
///
/// Used only as a mirror multiplier; `Vector3` here implements `Mul<f32>` but
/// not `Mul<Vector3>`, so this is explicit rather than an operator.
fn mul_components(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}

/// The side and cap planes of a cylinder centred on the origin along `axis`
/// (0 = x, 1 = y, 2 = z). Mirrors `Geometry3D.build_cylinder_planes`.
///
/// Ported from `upstream/godot/core/math/geometry_3d.cpp:740`. Returns
/// `sides + 2` planes: `sides` radial ones at distance `radius`, then the two
/// caps.
///
/// ⚠️ Both caps carry `+height * 0.5`, the same positive-distance convention as
/// [`build_box_planes`] — the cap normals differ in sign, the distances do not.
///
/// An out-of-range `axis`, or a non-positive `sides`, yields an empty vector,
/// matching Godot's `ERR_FAIL_INDEX_V(p_axis, 3, Vector<Plane>())`.
pub fn build_cylinder_planes(radius: f32, height: f32, sides: i64, axis: i64) -> Vec<Plane> {
    if !(0..3).contains(&axis) || sides <= 0 {
        return Vec::new();
    }
    let axis = axis as usize;
    let sides_step = std::f32::consts::TAU / sides as f32;

    let mut planes = Vec::with_capacity(sides as usize + 2);
    for i in 0..sides {
        let angle = i as f32 * sides_step;
        let mut normal = Vector3::ZERO;
        normal = with_component(normal, (axis + 1) % 3, angle.cos());
        normal = with_component(normal, (axis + 2) % 3, angle.sin());
        planes.push(Plane::new(normal, radius));
    }

    let axis_v = with_component(Vector3::ZERO, axis, 1.0);
    planes.push(Plane::new(axis_v, height * 0.5));
    planes.push(Plane::new(-axis_v, height * 0.5));
    planes
}

/// The planes bounding a capsule centred on the origin along `axis`. Mirrors
/// `Geometry3D.build_capsule_planes`.
///
/// Ported from `upstream/godot/core/math/geometry_3d.cpp:794`. Returns
/// `sides * (1 + 2 * lats)` planes: one radial plane per side, then `lats`
/// latitude planes per side for each hemisphere cap.
///
/// ⚠️ The latitude planes use Godot's `Plane(normal, POINT)` constructor, not
/// `Plane(normal, distance)` — so `d` is `normal.dot(position)`, where
/// `position` is a point ON the cap. Reading that overload as a distance is the
/// easy mistake: it would put every latitude plane at a `d` of whatever the
/// point's x happened to be.
///
/// ⚠️ Godot's `axis_neg` is a per-component MULTIPLIER (+1 on the two radial
/// components, -1 on the axial one), not a direction vector. It mirrors both the
/// normal and the point through the plane perpendicular to `axis`, which is what
/// produces the second hemisphere.
pub fn build_capsule_planes(
    radius: f32,
    height: f32,
    sides: i64,
    lats: i64,
    axis: i64,
) -> Vec<Plane> {
    if !(0..3).contains(&axis) || sides <= 0 {
        return Vec::new();
    }
    let axis = axis as usize;

    let axis_v = with_component(Vector3::ZERO, axis, 1.0);
    let mut axis_neg = Vector3::ZERO;
    axis_neg = with_component(axis_neg, (axis + 1) % 3, 1.0);
    axis_neg = with_component(axis_neg, (axis + 2) % 3, 1.0);
    axis_neg = with_component(axis_neg, axis, -1.0);

    let sides_step = std::f32::consts::TAU / sides as f32;
    let mut planes = Vec::new();
    for i in 0..sides {
        let angle = i as f32 * sides_step;
        let mut normal = Vector3::ZERO;
        normal = with_component(normal, (axis + 1) % 3, angle.cos());
        normal = with_component(normal, (axis + 2) % 3, angle.sin());
        planes.push(Plane::new(normal, radius));

        for j in 1..=lats {
            let t = j as f32 / lats as f32;
            let plane_normal = normal.lerp(axis_v, t).normalized();
            let position = axis_v * (height * 0.5) + plane_normal * radius;
            planes.push(Plane::new(plane_normal, plane_normal.dot(position)));

            let mirror_normal = mul_components(plane_normal, axis_neg);
            let mirror_point = mul_components(position, axis_neg);
            planes.push(Plane::new(mirror_normal, mirror_normal.dot(mirror_point)));
        }
    }
    planes
}

// ---------------------------------------------------------------------------
// pat-mgixk.102 — plane clipping.
// ---------------------------------------------------------------------------

/// Distance below which a point counts as lying ON a plane rather than over it.
///
/// ⚠️ Matters for the result's point count: a vertex exactly on the plane must
/// be emitted once, not emitted twice (as both a kept vertex and a fresh
/// crossing), so the "is this vertex over the plane" test and the "do these two
/// vertices straddle it" test have to use the same threshold.
const PLANE_EPS: f32 = 1e-6;

/// `Geometry3D.clip_polygon` — the part of a planar polygon on the plane's
/// **back** side.
///
/// ⚠️ Back side, not front. Godot's `Geometry3D::clip_polygon` classifies a
/// vertex as `LOC_INSIDE` when its signed distance is negative and keeps the
/// inside part, so the half kept is the one `Plane::is_point_over` reports
/// `false` for. Keeping the front half instead would return the complement —
/// a plausible-looking polygon of the same size, which is exactly the kind of
/// inversion a single-polygon test cannot see.
///
/// Returns the polygon unchanged when it lies wholly behind the plane, and an
/// empty vector when it lies wholly in front — again matching Godot, whose
/// early-outs are `over == 0` and `under == 0`.
pub fn clip_polygon(points: &[Vector3], plane: Plane) -> Vec<Vector3> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    let dist: Vec<f32> = points.iter().map(|p| plane.distance_to(*p)).collect();
    let any_over = dist.iter().any(|d| *d > PLANE_EPS);
    if !any_over {
        return points.to_vec();
    }
    let any_under = dist.iter().any(|d| *d <= PLANE_EPS);
    if !any_under {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = (points[i], points[j]);
        let (da, db) = (dist[i], dist[j]);
        // A vertex on or behind the plane survives.
        if da <= PLANE_EPS {
            out.push(a);
        }
        // Only a true straddle contributes a crossing; a vertex sitting on the
        // plane was already emitted by the branch above.
        if (da > PLANE_EPS) != (db > PLANE_EPS) {
            let t = da / (da - db);
            out.push(Vector3::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vector3, b: Vector3) -> bool {
        a.distance_to(b) < 1e-4
    }

    #[test]
    fn closest_points_perpendicular_skew_segments() {
        // Segment along X through the origin, segment along Y one unit up in Z.
        let a = Vector3::new(-1.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        let c = Vector3::new(0.0, -1.0, 1.0);
        let d = Vector3::new(0.0, 1.0, 1.0);
        let (p, q) = get_closest_points_between_segments(a, b, c, d);
        assert!(approx(p, Vector3::new(0.0, 0.0, 0.0)), "p {p:?}");
        assert!(approx(q, Vector3::new(0.0, 0.0, 1.0)), "q {q:?}");
    }

    #[test]
    fn closest_points_parallel_segments_report_gap() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        let c = Vector3::new(0.0, 1.0, 0.0);
        let d = Vector3::new(1.0, 1.0, 0.0);
        let (p, q) = get_closest_points_between_segments(a, b, c, d);
        // The perpendicular gap between the parallel lines is 1.0.
        assert!((p.distance_to(q) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn closest_points_between_point_segments() {
        let p = Vector3::new(3.0, 4.0, 0.0);
        let q = Vector3::new(0.0, 0.0, 0.0);
        let (r, s) = get_closest_points_between_segments(p, p, q, q);
        assert!(approx(r, p) && approx(s, q));
    }

    #[test]
    fn closest_point_to_segment_projects_and_clamps() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        // Interior projection.
        assert!(approx(
            get_closest_point_to_segment(Vector3::new(0.5, 1.0, 0.0), a, b),
            Vector3::new(0.5, 0.0, 0.0)
        ));
        // Past the far end → clamped to b.
        assert!(approx(
            get_closest_point_to_segment(Vector3::new(5.0, 1.0, 0.0), a, b),
            Vector3::new(1.0, 0.0, 0.0)
        ));
        // Before the near end → clamped to a.
        assert!(approx(
            get_closest_point_to_segment(Vector3::new(-5.0, 1.0, 0.0), a, b),
            a
        ));
    }

    #[test]
    fn closest_point_to_segment_uncapped_extends() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        assert!(approx(
            get_closest_point_to_segment_uncapped(Vector3::new(5.0, 1.0, 0.0), a, b),
            Vector3::new(5.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn segment_hits_triangle_interior() {
        let v0 = Vector3::new(0.0, 0.0, 0.0);
        let v1 = Vector3::new(1.0, 0.0, 0.0);
        let v2 = Vector3::new(0.0, 1.0, 0.0);
        let hit = segment_intersects_triangle(
            Vector3::new(0.25, 0.25, 1.0),
            Vector3::new(0.25, 0.25, -1.0),
            v0,
            v1,
            v2,
        );
        assert!(hit.is_some());
        assert!(approx(hit.unwrap(), Vector3::new(0.25, 0.25, 0.0)));
    }

    #[test]
    fn segment_misses_triangle_outside_and_short() {
        let v0 = Vector3::new(0.0, 0.0, 0.0);
        let v1 = Vector3::new(1.0, 0.0, 0.0);
        let v2 = Vector3::new(0.0, 1.0, 0.0);
        // Passes through the triangle's plane but outside the triangle.
        assert!(segment_intersects_triangle(
            Vector3::new(2.0, 2.0, 1.0),
            Vector3::new(2.0, 2.0, -1.0),
            v0,
            v1,
            v2,
        )
        .is_none());
        // Aimed at the interior but too short to reach the plane.
        assert!(segment_intersects_triangle(
            Vector3::new(0.25, 0.25, 1.0),
            Vector3::new(0.25, 0.25, 0.5),
            v0,
            v1,
            v2,
        )
        .is_none());
    }

    #[test]
    fn ray_hits_triangle_only_forward() {
        let v0 = Vector3::new(0.0, 0.0, 0.0);
        let v1 = Vector3::new(1.0, 0.0, 0.0);
        let v2 = Vector3::new(0.0, 1.0, 0.0);
        let from = Vector3::new(0.25, 0.25, 1.0);
        assert!(approx(
            ray_intersects_triangle(from, Vector3::new(0.0, 0.0, -1.0), v0, v1, v2).unwrap(),
            Vector3::new(0.25, 0.25, 0.0)
        ));
        // Pointing away from the triangle → no hit.
        assert!(ray_intersects_triangle(from, Vector3::new(0.0, 0.0, 1.0), v0, v1, v2).is_none());
    }

    #[test]
    fn segment_intersects_sphere_first_hit() {
        let center = Vector3::new(0.0, 0.0, 0.0);
        // Passing straight through: first hit at the near surface.
        assert!(approx(
            segment_intersects_sphere(
                Vector3::new(-2.0, 0.0, 0.0),
                Vector3::new(2.0, 0.0, 0.0),
                center,
                1.0,
            )
            .unwrap(),
            Vector3::new(-1.0, 0.0, 0.0)
        ));
        // Starting at the center: exits at the far surface.
        assert!(approx(
            segment_intersects_sphere(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(2.0, 0.0, 0.0),
                center,
                1.0,
            )
            .unwrap(),
            Vector3::new(1.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn segment_misses_sphere() {
        let center = Vector3::new(0.0, 0.0, 0.0);
        assert!(segment_intersects_sphere(
            Vector3::new(0.0, 3.0, 0.0),
            Vector3::new(2.0, 3.0, 0.0),
            center,
            1.0,
        )
        .is_none());
    }

    // -- get_triangle_barycentric_coords -----------------------------------

    #[test]
    fn barycentric_at_a_vertex_is_a_unit_weight() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        let c = Vector3::new(0.0, 1.0, 0.0);
        assert!(approx(
            get_triangle_barycentric_coords(a, a, b, c),
            Vector3::new(1.0, 0.0, 0.0)
        ));
        assert!(approx(
            get_triangle_barycentric_coords(b, a, b, c),
            Vector3::new(0.0, 1.0, 0.0)
        ));
        assert!(approx(
            get_triangle_barycentric_coords(c, a, b, c),
            Vector3::new(0.0, 0.0, 1.0)
        ));
    }

    /// ⚠️ The control for ARGUMENT ORDER. Every weight here is distinct
    /// (0.3, 0.2, 0.5), so swapping the queried point with a vertex — the trap
    /// in porting Godot's internal `(a, b, c, pos)` signature instead of the
    /// script-facing `(point, a, b, c)` — changes the answer. A symmetric
    /// fixture such as the centroid could not tell the two apart, because the
    /// weights stay equal under the swap.
    #[test]
    fn barycentric_weights_pin_the_argument_order() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        let c = Vector3::new(0.0, 1.0, 0.0);
        let point = Vector3::new(0.2, 0.5, 0.0);
        let w = get_triangle_barycentric_coords(point, a, b, c);
        assert!(approx(w, Vector3::new(0.3, 0.2, 0.5)), "weights {w:?}");

        // And the weights reconstruct the point, which is what makes them
        // barycentric rather than merely summing to one.
        let rebuilt = a * w.x + b * w.y + c * w.z;
        assert!(approx(rebuilt, point), "rebuilt {rebuilt:?}");
    }

    #[test]
    fn barycentric_of_a_degenerate_triangle_is_zero_not_nan() {
        // Three collinear points: zero area, so the denominator vanishes.
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(1.0, 0.0, 0.0);
        let c = Vector3::new(2.0, 0.0, 0.0);
        let w = get_triangle_barycentric_coords(Vector3::new(0.5, 0.0, 0.0), a, b, c);
        assert_eq!(w, Vector3::ZERO, "degenerate triangle must not yield NaNs");
        assert!(!w.x.is_nan() && !w.y.is_nan() && !w.z.is_nan());
    }

    // -- build_box_planes --------------------------------------------------

    #[test]
    fn box_planes_face_outward_along_each_axis() {
        let planes = build_box_planes(Vector3::new(2.0, 3.0, 4.0));
        let normals: Vec<Vector3> = planes.iter().map(|p| p.normal).collect();
        assert_eq!(
            normals,
            vec![
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, -1.0),
            ],
            "order is +X, -X, +Y, -Y, +Z, -Z"
        );
    }

    /// ⚠️ THE SIGN CONVENTION, and the reason this needed a semantic test and
    /// not just a value check: Godot stores the POSITIVE extent as `d` even on
    /// the planes whose normals point down the negative axes
    /// (`geometry_3d.cpp:727`). Negating them to "agree" with the normal reads
    /// as the obvious fix and collapses the box — three faces land on the wrong
    /// side of the origin. Asserted here as containment, which is what the
    /// convention is FOR: every interior point is on the negative side of all
    /// six planes.
    #[test]
    fn every_interior_point_is_inside_all_six_box_planes() {
        let extents = Vector3::new(2.0, 3.0, 4.0);
        let planes = build_box_planes(extents);

        for p in &planes {
            assert!(p.d >= 0.0, "distances are never negative, got {}", p.d);
        }

        for inside in [
            Vector3::ZERO,
            Vector3::new(1.9, -2.9, 3.9),
            Vector3::new(-1.9, 2.9, -3.9),
        ] {
            for (i, plane) in planes.iter().enumerate() {
                assert!(
                    plane.distance_to(inside) <= 0.0,
                    "{inside:?} must be inside plane {i} ({plane:?})"
                );
            }
        }

        // And a point beyond +X is outside exactly that plane.
        let outside = Vector3::new(2.5, 0.0, 0.0);
        assert!(planes[0].distance_to(outside) > 0.0, "+X must reject it");
        assert!(
            planes[1].distance_to(outside) < 0.0,
            "-X must still accept it"
        );
    }

    #[test]
    fn box_planes_distances_are_the_extents() {
        let planes = build_box_planes(Vector3::new(2.0, 3.0, 4.0));
        let ds: Vec<f32> = planes.iter().map(|p| p.d).collect();
        assert_eq!(ds, vec![2.0, 2.0, 3.0, 3.0, 4.0, 4.0]);
    }

    // -- build_cylinder_planes ---------------------------------------------

    #[test]
    fn cylinder_planes_are_sides_plus_two_caps() {
        let planes = build_cylinder_planes(2.0, 6.0, 8, 1);
        assert_eq!(planes.len(), 10, "8 radial + 2 caps");
    }

    #[test]
    fn cylinder_radial_planes_are_unit_and_perpendicular_to_the_axis() {
        // axis = 1 (Y), so every radial normal must lie in the XZ plane.
        let planes = build_cylinder_planes(2.0, 6.0, 8, 1);
        for plane in &planes[..8] {
            assert!(
                plane.normal.y.abs() < 1e-5,
                "radial normal must have no axial component, got {plane:?}"
            );
            assert!(
                (plane.normal.length() - 1.0).abs() < 1e-5,
                "radial normal must be unit length, got {plane:?}"
            );
            assert!((plane.d - 2.0).abs() < 1e-5, "radial d is the radius");
        }
    }

    /// ⚠️ The cap convention, same trap as `build_box_planes`: the normals are
    /// opposite but BOTH distances are `+height * 0.5`. Negating the second to
    /// match its normal would put both caps on the same side.
    #[test]
    fn cylinder_caps_oppose_in_normal_but_share_a_positive_distance() {
        let planes = build_cylinder_planes(2.0, 6.0, 8, 1);
        let top = planes[8];
        let bottom = planes[9];
        assert!((top.normal.y - 1.0).abs() < 1e-5, "top faces +Y");
        assert!((bottom.normal.y + 1.0).abs() < 1e-5, "bottom faces -Y");
        assert!((top.d - 3.0).abs() < 1e-5, "top at +height/2");
        assert!((bottom.d - 3.0).abs() < 1e-5, "bottom ALSO at +height/2");
    }

    /// The semantic control: the planes actually bound the cylinder.
    #[test]
    fn every_interior_point_is_inside_all_cylinder_planes() {
        let planes = build_cylinder_planes(2.0, 6.0, 12, 1);
        for inside in [
            Vector3::ZERO,
            Vector3::new(1.0, 2.5, 1.0),
            Vector3::new(-1.0, -2.5, -1.0),
        ] {
            for (i, plane) in planes.iter().enumerate() {
                assert!(
                    plane.distance_to(inside) <= 1e-5,
                    "{inside:?} must be inside plane {i} ({plane:?})"
                );
            }
        }
        // Beyond the top cap, exactly that plane rejects the point.
        let above = Vector3::new(0.0, 4.0, 0.0);
        assert!(planes[12].distance_to(above) > 0.0, "top cap must reject");
        assert!(planes[13].distance_to(above) < 0.0, "bottom cap must not");
    }

    #[test]
    fn cylinder_respects_the_chosen_axis() {
        // axis = 0 (X): radial normals must now lie in the YZ plane.
        let planes = build_cylinder_planes(1.0, 2.0, 6, 0);
        for plane in &planes[..6] {
            assert!(plane.normal.x.abs() < 1e-5, "got {plane:?}");
        }
        assert!((planes[6].normal.x - 1.0).abs() < 1e-5, "cap along X");
    }

    #[test]
    fn an_out_of_range_axis_builds_no_planes() {
        assert!(build_cylinder_planes(1.0, 2.0, 6, 3).is_empty());
        assert!(build_cylinder_planes(1.0, 2.0, 6, -1).is_empty());
        assert!(build_capsule_planes(1.0, 2.0, 6, 2, 3).is_empty());
        // And a degenerate side count cannot divide TAU.
        assert!(build_cylinder_planes(1.0, 2.0, 0, 1).is_empty());
    }

    // -- build_capsule_planes ----------------------------------------------

    #[test]
    fn capsule_plane_count_is_sides_times_one_plus_two_lats() {
        assert_eq!(build_capsule_planes(1.0, 4.0, 8, 2, 1).len(), 8 * 5);
        // No latitudes: just the radial band.
        assert_eq!(build_capsule_planes(1.0, 4.0, 8, 0, 1).len(), 8);
    }

    /// 🔑 THE CONTROL FOR THE `Plane(normal, POINT)` OVERLOAD.
    ///
    /// Godot builds each latitude plane from a normal and a POINT on the cap,
    /// so `d` is `normal.dot(point)`. If that overload were read as
    /// `Plane(normal, distance)`, `d` would become some component of the point
    /// instead and the plane would sit somewhere arbitrary. The invariant that
    /// catches it: the generating point lies exactly ON its plane.
    #[test]
    fn capsule_latitude_planes_pass_through_their_generating_point() {
        let (radius, height, sides, lats, axis) = (1.5_f32, 4.0_f32, 6_i64, 3_i64, 1_i64);
        let planes = build_capsule_planes(radius, height, sides, lats, axis);

        let axis_v = Vector3::new(0.0, 1.0, 0.0);
        let stride = (1 + 2 * lats) as usize;
        for i in 0..sides {
            let angle = i as f32 * (std::f32::consts::TAU / sides as f32);
            // ⚠️ For axis = 1 (Y) the radial components are
            // `(axis + 1) % 3 = 2` (Z) for COS and `(axis + 2) % 3 = 0` (X) for
            // SIN. I first wrote this the intuitive way round — cos on X — and
            // the test failed while the implementation was right, because the
            // implementation follows Godot's index arithmetic and this line was
            // a hand-written restatement of it.
            let normal = Vector3::new(angle.sin(), 0.0, angle.cos());
            for j in 1..=lats {
                let t = j as f32 / lats as f32;
                let plane_normal = normal.lerp(axis_v, t).normalized();
                let position = axis_v * (height * 0.5) + plane_normal * radius;
                let plane = planes[i as usize * stride + (2 * (j as usize - 1) + 1)];
                assert!(
                    plane.distance_to(position).abs() < 1e-4,
                    "latitude plane must contain its point: {plane:?} vs {position:?}"
                );
            }
        }
    }

    /// 🔑 AN INDEPENDENT ORACLE, not a restatement of the implementation.
    ///
    /// The test above recomputes Godot's own formula and checks the plane
    /// contains the point it was built from — which proves the two agree, not
    /// that either is right. This one derives the answer from the SHAPE: a
    /// capsule along `axis` reaches exactly `height / 2 + radius` along it, and
    /// `radius` across it. At the last latitude (`t = 1`) the plane normal IS
    /// the axis, so its `d` must be that axial extent; the radial planes' `d`
    /// must be the radius. Neither number comes from the code under test.
    #[test]
    fn capsule_extents_match_the_shape_not_the_formula() {
        let (radius, height, sides, lats) = (1.5_f32, 4.0_f32, 6_i64, 3_i64);
        let planes = build_capsule_planes(radius, height, sides, lats, 1);
        let stride = (1 + 2 * lats) as usize;

        for i in 0..sides as usize {
            // The radial plane of each side sits at the radial extent.
            let radial = planes[i * stride];
            assert!(
                (radial.d - radius).abs() < 1e-5,
                "radial extent must be the radius, got {radial:?}"
            );

            // The LAST latitude of the +axis hemisphere is the axis-aligned cap.
            let cap = planes[i * stride + 2 * (lats as usize - 1) + 1];
            assert!(
                (cap.normal.y - 1.0).abs() < 1e-5,
                "at t = 1 the normal is the axis itself, got {cap:?}"
            );
            assert!(
                (cap.d - (height / 2.0 + radius)).abs() < 1e-4,
                "axial extent must be height/2 + radius = {}, got {cap:?}",
                height / 2.0 + radius
            );
        }
    }

    /// The mirrored hemisphere: for each latitude the pair's axial normal
    /// components must be equal and opposite, which is what `axis_neg` does.
    #[test]
    fn capsule_latitude_planes_come_in_mirrored_pairs() {
        let lats = 2_i64;
        let planes = build_capsule_planes(1.0, 4.0, 4, lats, 1);
        let stride = (1 + 2 * lats) as usize;
        for i in 0..4usize {
            for j in 0..lats as usize {
                let upper = planes[i * stride + 2 * j + 1];
                let lower = planes[i * stride + 2 * j + 2];
                assert!(
                    (upper.normal.y + lower.normal.y).abs() < 1e-5,
                    "axial components must cancel: {upper:?} / {lower:?}"
                );
                assert!(
                    upper.normal.y > 0.0,
                    "the first of each pair is the +axis hemisphere, got {upper:?}"
                );
            }
        }
    }
}
