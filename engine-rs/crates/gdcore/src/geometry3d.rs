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
}
