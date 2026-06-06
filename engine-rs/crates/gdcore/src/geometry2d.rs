//! 2D geometry utilities matching Godot's `Geometry2D` singleton.
//!
//! As of Godot 4.6.1, arc generation uses **fixed-count subdivision**
//! rather than the previous tolerance-based adaptive approach.  The
//! `point_count` parameter directly controls how many vertices the arc
//! polyline contains, producing evenly spaced points along the arc.

use crate::math::Vector2;

/// Generates an arc polyline of `point_count` evenly spaced points.
///
/// This mirrors Godot 4.6.1's `Geometry2D` arc generation after the
/// tolerance-scaling removal.  Points are placed at equal angular
/// intervals between `start_angle` and `end_angle` (both in radians).
///
/// # Arguments
///
/// * `center`      – Centre of the arc circle.
/// * `radius`      – Radius of the arc.
/// * `start_angle` – Starting angle in radians.
/// * `end_angle`   – Ending angle in radians.
/// * `point_count` – Number of points in the output polyline (≥ 2).
///
/// # Returns
///
/// A `Vec<Vector2>` with exactly `point_count` points, or an empty vec
/// if `point_count < 2`.
pub fn build_arc(
    center: Vector2,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    point_count: u32,
) -> Vec<Vector2> {
    if point_count < 2 {
        return Vec::new();
    }

    let count = point_count as usize;
    let mut points = Vec::with_capacity(count);
    let delta = (end_angle - start_angle) / (count as f32 - 1.0);

    for i in 0..count {
        let angle = start_angle + delta * i as f32;
        points.push(Vector2::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }

    points
}

/// Checks whether a point lies inside a convex polygon (given as an
/// ordered slice of vertices).
///
/// Uses the cross-product winding test: the point must be on the same
/// side of every edge.
pub fn is_point_in_polygon(point: Vector2, polygon: &[Vector2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }

    let mut sign: Option<bool> = None;
    for i in 0..n {
        let a = polygon[i];
        let b = polygon[(i + 1) % n];
        let cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
        let positive = cross > 0.0;

        match sign {
            None => sign = Some(positive),
            Some(s) if s != positive => return false,
            _ => {}
        }
    }

    true
}

/// Tests whether two line segments intersect and returns the intersection
/// point if they do.
///
/// Segments are defined by their endpoints: `p`–`q` and `r`–`s`.
///
/// Returns `None` when the segments are parallel (including collinear),
/// or when the intersection of the infinite lines falls outside either
/// segment's [0, 1] parameter range.
///
/// The cross-product denominator is tested against an epsilon of `1e-7`
/// to avoid ghost collisions from nearly-parallel segments — this matches
/// Godot 4.6.1's fix for the `Geometry2D` segment intersection bug.
pub fn segment_intersects_segment(
    p: Vector2,
    q: Vector2,
    r: Vector2,
    s: Vector2,
) -> Option<Vector2> {
    let d1 = Vector2::new(q.x - p.x, q.y - p.y);
    let d2 = Vector2::new(s.x - r.x, s.y - r.y);

    let denom = d1.cross(d2);

    // Parallel or nearly-parallel — no intersection.  The 1e-7 threshold
    // prevents ghost collisions from floating-point noise when segments
    // are almost (but not quite) parallel.
    if denom.abs() < 1e-7 {
        return None;
    }

    let pr = Vector2::new(r.x - p.x, r.y - p.y);
    let t = pr.cross(d2) / denom;
    let u = pr.cross(d1) / denom;

    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some(Vector2::new(p.x + d1.x * t, p.y + d1.y * t))
    } else {
        None
    }
}

/// Returns the closest point on the **infinite line** through `a`–`b` to
/// `point`, without clamping to the segment bounds.
///
/// Mirrors Godot 4.x `Geometry2D.get_closest_point_to_segment_unclamped()`.
pub fn get_closest_point_to_segment_unclamped(point: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let ab = Vector2::new(b.x - a.x, b.y - a.y);
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return a;
    }
    let t = ((point.x - a.x) * ab.x + (point.y - a.y) * ab.y) / len_sq;
    Vector2::new(a.x + ab.x * t, a.y + ab.y * t)
}

/// Tests whether two **infinite lines** intersect and returns the
/// intersection point.
///
/// Lines are defined by a point and direction: line A passes through
/// `from_a` in direction `dir_a`, line B through `from_b` in `dir_b`.
///
/// Returns `None` when the lines are parallel (cross product near zero).
///
/// Mirrors Godot 4.x `Geometry2D.line_intersects_line()`.
pub fn line_intersects_line(
    from_a: Vector2,
    dir_a: Vector2,
    from_b: Vector2,
    dir_b: Vector2,
) -> Option<Vector2> {
    let denom = dir_a.cross(dir_b);
    if denom.abs() < 1e-7 {
        return None;
    }
    let diff = Vector2::new(from_b.x - from_a.x, from_b.y - from_a.y);
    let t = diff.cross(dir_b) / denom;
    Some(Vector2::new(from_a.x + dir_a.x * t, from_a.y + dir_a.y * t))
}

/// Tests whether a point lies inside the triangle defined by `a`, `b`, `c`.
///
/// Uses the barycentric sign method (cross-product winding).
///
/// Mirrors Godot 4.x `Geometry2D.point_is_inside_triangle()`.
pub fn point_is_inside_triangle(point: Vector2, a: Vector2, b: Vector2, c: Vector2) -> bool {
    let d1 = sign(point, a, b);
    let d2 = sign(point, b, c);
    let d3 = sign(point, c, a);
    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(has_neg && has_pos)
}

fn sign(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y)
}

/// Returns whether the polygon vertices are in clockwise order.
///
/// Uses the signed area (shoelace formula): negative area = clockwise.
///
/// Mirrors Godot 4.x `Geometry2D.is_polygon_clockwise()`.
pub fn is_polygon_clockwise(polygon: &[Vector2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut area: f32 = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area += polygon[i].x * polygon[j].y;
        area -= polygon[j].x * polygon[i].y;
    }
    area < 0.0
}

/// Returns the closest point on a line segment `a`–`b` to `point`.
pub fn get_closest_point_to_segment(point: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let ab = Vector2::new(b.x - a.x, b.y - a.y);
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return a;
    }
    let t = ((point.x - a.x) * ab.x + (point.y - a.y) * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    Vector2::new(a.x + ab.x * t, a.y + ab.y * t)
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    // -- build_arc ---------------------------------------------------------

    #[test]
    fn arc_point_count_matches_request() {
        let pts = build_arc(Vector2::ZERO, 100.0, 0.0, PI, 32);
        assert_eq!(pts.len(), 32);
    }

    #[test]
    fn arc_degenerate_point_count_returns_empty() {
        assert!(build_arc(Vector2::ZERO, 50.0, 0.0, PI, 0).is_empty());
        assert!(build_arc(Vector2::ZERO, 50.0, 0.0, PI, 1).is_empty());
    }

    #[test]
    fn arc_first_and_last_point_match_angles() {
        let pts = build_arc(Vector2::ZERO, 1.0, 0.0, FRAC_PI_2, 10);
        // First point: angle=0 → (1, 0)
        assert!((pts[0].x - 1.0).abs() < 1e-5);
        assert!(pts[0].y.abs() < 1e-5);
        // Last point: angle=π/2 → (0, 1)
        let last = pts.last().unwrap();
        assert!(last.x.abs() < 1e-5);
        assert!((last.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn arc_all_points_at_radius() {
        let center = Vector2::new(10.0, 20.0);
        let radius = 50.0;
        let pts = build_arc(center, radius, 0.0, TAU, 64);
        for p in &pts {
            let dist = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
            assert!(
                (dist - radius).abs() < 1e-4,
                "point {:?} is {dist} from center, expected {radius}",
                p
            );
        }
    }

    #[test]
    fn arc_even_angular_spacing() {
        // Fixed-subdivision means equal angular gaps between consecutive
        // points — this is the core 4.6.1 behavioral change.
        // Use a quarter-circle to avoid the atan2 discontinuity at ±PI.
        let pts = build_arc(Vector2::ZERO, 1.0, 0.0, FRAC_PI_2, 5);
        let expected_delta = FRAC_PI_2 / 4.0;

        for i in 0..4 {
            let a0 = pts[i].y.atan2(pts[i].x);
            let a1 = pts[i + 1].y.atan2(pts[i + 1].x);
            let gap = a1 - a0;
            assert!(
                (gap - expected_delta).abs() < 1e-5,
                "gap between point {i} and {} is {gap}, expected {expected_delta}",
                i + 1
            );
        }
    }

    #[test]
    fn arc_with_offset_center() {
        let center = Vector2::new(100.0, 200.0);
        let pts = build_arc(center, 10.0, 0.0, FRAC_PI_2, 2);
        // First: center + (10, 0)
        assert!((pts[0].x - 110.0).abs() < 1e-4);
        assert!((pts[0].y - 200.0).abs() < 1e-4);
        // Last: center + (0, 10)
        assert!((pts[1].x - 100.0).abs() < 1e-4);
        assert!((pts[1].y - 210.0).abs() < 1e-4);
    }

    #[test]
    fn arc_negative_sweep() {
        // Sweeping from PI to 0 (clockwise) should still produce evenly
        // spaced points, just in reverse angular order.
        let pts = build_arc(Vector2::ZERO, 1.0, PI, 0.0, 3);
        assert_eq!(pts.len(), 3);
        // First point at angle PI → (-1, ~0)
        assert!((pts[0].x - (-1.0)).abs() < 1e-5);
        // Middle at PI/2 → (0, 1)
        assert!((pts[1].x - 0.0).abs() < 1e-4);
        assert!((pts[1].y - 1.0).abs() < 1e-4, "mid y={}", pts[1].y);
        // Last at 0 → (1, 0)
        assert!((pts[2].x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn arc_full_circle() {
        let pts = build_arc(Vector2::ZERO, 5.0, 0.0, TAU, 100);
        assert_eq!(pts.len(), 100);
        // First and last should coincide (full circle).
        let first = pts[0];
        let last = pts[99];
        assert!((first.x - last.x).abs() < 1e-4);
        assert!((first.y - last.y).abs() < 1e-4);
    }

    #[test]
    fn arc_two_points_gives_endpoints_only() {
        let pts = build_arc(Vector2::ZERO, 1.0, 0.0, PI, 2);
        assert_eq!(pts.len(), 2);
        // (1,0) and (-1, ~0)
        assert!((pts[0].x - 1.0).abs() < 1e-5);
        assert!((pts[1].x - (-1.0)).abs() < 1e-5);
    }

    // -- is_point_in_polygon -----------------------------------------------

    #[test]
    fn point_inside_square() {
        let sq = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ];
        assert!(is_point_in_polygon(Vector2::new(5.0, 5.0), &sq));
    }

    #[test]
    fn point_outside_square() {
        let sq = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ];
        assert!(!is_point_in_polygon(Vector2::new(15.0, 5.0), &sq));
    }

    #[test]
    fn point_in_polygon_degenerate_returns_false() {
        assert!(!is_point_in_polygon(Vector2::ZERO, &[]));
        assert!(!is_point_in_polygon(
            Vector2::ZERO,
            &[Vector2::ZERO, Vector2::ONE]
        ));
    }

    // -- get_closest_point_to_segment --------------------------------------

    #[test]
    fn closest_point_midpoint_projection() {
        let p = get_closest_point_to_segment(
            Vector2::new(5.0, 10.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
        );
        assert!((p.x - 5.0).abs() < 1e-5);
        assert!(p.y.abs() < 1e-5);
    }

    #[test]
    fn closest_point_clamped_to_start() {
        let p = get_closest_point_to_segment(
            Vector2::new(-5.0, 0.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
        );
        assert!((p.x).abs() < 1e-5);
    }

    #[test]
    fn closest_point_clamped_to_end() {
        let p = get_closest_point_to_segment(
            Vector2::new(20.0, 0.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
        );
        assert!((p.x - 10.0).abs() < 1e-5);
    }
}
