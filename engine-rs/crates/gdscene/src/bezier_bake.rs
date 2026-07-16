//! Cubic-bezier arc-length baking for Curve2D / PathFollow2D (pat-nzgdu.118).
//!
//! Godot's `Curve2D` bakes its cubic-bezier control points into an arc-length
//! polyline so `sample_baked(offset)` (and the tangent for `PathFollow2D`) can
//! position a follower a given distance along the path. This module provides
//! that core as self-contained geometry over `gdcore::math::Vector2`: bake a
//! sequence of bezier points into an arc-length table, then sample a position or
//! tangent by distance. The Curve2D upgrade + the per-frame PathFollow2D step
//! consume these.

use gdcore::math::Vector2;

/// A Curve2D control point: a `position` with `in`/`out` bezier handles stored
/// relative to the position (Godot convention).
#[derive(Debug, Clone, Copy)]
pub struct CurvePoint {
    /// The point's position.
    pub position: Vector2,
    /// Incoming handle, relative to `position`.
    pub in_handle: Vector2,
    /// Outgoing handle, relative to `position`.
    pub out_handle: Vector2,
}

/// A baked polyline vertex: cumulative arc-length `distance` from the start and
/// the interpolated `position`.
#[derive(Debug, Clone, Copy)]
pub struct BakedPoint {
    /// Cumulative arc length from the curve start.
    pub distance: f32,
    /// Position on the curve.
    pub position: Vector2,
}

/// Evaluates the cubic bezier `p0 -> (c1, c2) -> p3` at parameter `t` in `[0,1]`.
fn cubic_bezier(p0: Vector2, c1: Vector2, c2: Vector2, p3: Vector2, t: f32) -> Vector2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// Bakes `points` into an arc-length table by sampling each bezier segment
/// `steps` times and accumulating straight-line distance between samples (Godot
/// `Curve2D` bake). An empty curve yields an empty table; a single point yields
/// one entry at distance 0.
pub fn bake_curve(points: &[CurvePoint], steps: usize) -> Vec<BakedPoint> {
    let mut baked = Vec::new();
    if points.is_empty() {
        return baked;
    }
    let steps = steps.max(1);
    baked.push(BakedPoint {
        distance: 0.0,
        position: points[0].position,
    });
    let mut dist = 0.0f32;
    for seg in points.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let p0 = a.position;
        let c1 = a.position + a.out_handle;
        let c2 = b.position + b.in_handle;
        let p3 = b.position;
        let mut prev = p0;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let pt = cubic_bezier(p0, c1, c2, p3, t);
            dist += prev.distance_to(pt);
            baked.push(BakedPoint {
                distance: dist,
                position: pt,
            });
            prev = pt;
        }
    }
    baked
}

/// Total baked arc length (0 for an empty/one-point curve).
pub fn baked_length(baked: &[BakedPoint]) -> f32 {
    baked.last().map(|p| p.distance).unwrap_or(0.0)
}

/// Samples the position at arc-length `distance`, linearly interpolating between
/// the surrounding baked points. Distances outside `[0, length]` clamp to the
/// endpoints (Godot `sample_baked`).
pub fn sample_baked(baked: &[BakedPoint], distance: f32) -> Vector2 {
    if baked.is_empty() {
        return Vector2::ZERO;
    }
    if distance <= 0.0 || baked.len() == 1 {
        return baked[0].position;
    }
    if distance >= baked_length(baked) {
        return baked[baked.len() - 1].position;
    }
    for i in 1..baked.len() {
        if baked[i].distance >= distance {
            let (a, b) = (baked[i - 1], baked[i]);
            let span = b.distance - a.distance;
            let t = if span > 1e-6 {
                (distance - a.distance) / span
            } else {
                0.0
            };
            return a.position.lerp(b.position, t);
        }
    }
    baked[baked.len() - 1].position
}

/// The tangent angle (radians) at arc-length `distance`, from the direction
/// between the surrounding baked points (drives `PathFollow2D.rotates`).
pub fn tangent_angle_at(baked: &[BakedPoint], distance: f32) -> f32 {
    if baked.len() < 2 {
        return 0.0;
    }
    let d = distance.clamp(0.0, baked_length(baked));
    for i in 1..baked.len() {
        if baked[i].distance >= d {
            return (baked[i].position - baked[i - 1].position).angle();
        }
    }
    let n = baked.len();
    (baked[n - 1].position - baked[n - 2].position).angle()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn pt(x: f32, y: f32, inx: f32, iny: f32, outx: f32, outy: f32) -> CurvePoint {
        CurvePoint {
            position: Vector2::new(x, y),
            in_handle: Vector2::new(inx, iny),
            out_handle: Vector2::new(outx, outy),
        }
    }

    #[test]
    fn linear_curve_bakes_uniform_arc_length() {
        // Two points on the x-axis with zero handles: geometrically the segment
        // (0,0)-(10,0). Arc length 10; the midpoint at distance 5 is (5,0).
        let pts = [
            pt(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            pt(10.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        ];
        let baked = bake_curve(&pts, 8);
        assert!(approx(baked_length(&baked), 10.0));
        let mid = sample_baked(&baked, 5.0);
        assert!(approx(mid.x, 5.0) && approx(mid.y, 0.0), "mid {mid:?}");
        assert!(approx(tangent_angle_at(&baked, 5.0), 0.0));
        // Out-of-range distances clamp to the endpoints.
        assert!(approx(sample_baked(&baked, -3.0).x, 0.0));
        assert!(approx(sample_baked(&baked, 999.0).x, 10.0));
    }

    #[test]
    fn curved_path_bulges_and_arc_length_is_monotonic() {
        // A hump from (0,0) to (10,0) with handles pulling upward.
        let pts = [
            pt(0.0, 0.0, 0.0, 0.0, 0.0, 10.0),
            pt(10.0, 0.0, 0.0, 10.0, 0.0, 0.0),
        ];
        let baked = bake_curve(&pts, 16);
        // Endpoints are exact.
        assert!(approx(baked[0].position.x, 0.0) && approx(baked[0].position.y, 0.0));
        let last = baked[baked.len() - 1].position;
        assert!(approx(last.x, 10.0) && approx(last.y, 0.0));
        // Arc length is strictly increasing and exceeds the chord (10) because
        // the path curves.
        for w in baked.windows(2) {
            assert!(w[1].distance > w[0].distance);
        }
        assert!(baked_length(&baked) > 10.0);
        // A mid-length sample sits above the x-axis (the hump).
        let mid = sample_baked(&baked, baked_length(&baked) * 0.5);
        assert!(mid.y > 1.0, "curve should bulge upward, got {mid:?}");
    }

    #[test]
    fn empty_and_single_point_curves_are_safe() {
        assert!(bake_curve(&[], 8).is_empty());
        let one = bake_curve(&[pt(3.0, 4.0, 0.0, 0.0, 0.0, 0.0)], 8);
        assert_eq!(one.len(), 1);
        assert!(approx(baked_length(&one), 0.0));
        let p = sample_baked(&one, 5.0);
        assert!(approx(p.x, 3.0) && approx(p.y, 4.0));
    }
}
