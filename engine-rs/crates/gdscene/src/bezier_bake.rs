//! Cubic-bezier arc-length baking for Curve2D / PathFollow2D (pat-nzgdu.118).
//!
//! Godot's `Curve2D` bakes its cubic-bezier control points into an arc-length
//! polyline so `sample_baked(offset)` (and the tangent for `PathFollow2D`) can
//! position a follower a given distance along the path. This module provides
//! that core as self-contained geometry over `gdcore::math::Vector2`: bake a
//! sequence of bezier points into an arc-length table, then sample a position or
//! tangent by distance. The Curve2D upgrade + the per-frame PathFollow2D step
//! consume these.

use gdcore::math::{Vector2, Vector3};

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
pub fn bake_curve(points: &[CurvePoint], schedule: &[Vec<f32>]) -> Vec<BakedPoint> {
    let mut baked = Vec::new();
    if points.is_empty() {
        return baked;
    }
    baked.push(BakedPoint {
        distance: 0.0,
        position: points[0].position,
    });
    let mut dist = 0.0f32;
    for (i, seg) in points.windows(2).enumerate() {
        let (a, b) = (seg[0], seg[1]);
        let p0 = a.position;
        let c1 = a.position + a.out_handle;
        let c2 = b.position + b.in_handle;
        let p3 = b.position;
        let mut prev = p0;
        for t in schedule.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            let pt = cubic_bezier(p0, c1, c2, p3, *t);
            dist += prev.distance_to(pt);
            baked.push(BakedPoint {
                distance: dist,
                position: pt,
            });
            prev = pt;
        }
        // The segment's own end point, exactly rather than as `bezier(1.0)`.
        dist += prev.distance_to(p3);
        baked.push(BakedPoint {
            distance: dist,
            position: p3,
        });
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
        let baked = bake_curve(&pts, &uniform_schedule(pts.len() - 1, 8));
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
        let baked = bake_curve(&pts, &uniform_schedule(pts.len() - 1, 16));
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
        assert!(bake_curve(&[], &[]).is_empty());
        let one = bake_curve(&[pt(3.0, 4.0, 0.0, 0.0, 0.0, 0.0)], &[]);
        assert_eq!(one.len(), 1);
        assert!(approx(baked_length(&one), 0.0));
        let p = sample_baked(&one, 5.0);
        assert!(approx(p.x, 3.0) && approx(p.y, 4.0));
    }

    // -- pat-s3rrv.91: parallel-transport up vectors --------------------------

    fn p3(pos: [f32; 3], inh: [f32; 3], outh: [f32; 3]) -> CurvePoint3 {
        CurvePoint3 {
            position: Vector3::new(pos[0], pos[1], pos[2]),
            in_handle: Vector3::new(inh[0], inh[1], inh[2]),
            out_handle: Vector3::new(outh[0], outh[1], outh[2]),
        }
    }

    fn near(a: Vector3, b: Vector3) -> bool {
        (a - b).length() < 1e-3
    }

    /// A straight segment has nothing to transport: every up stays world up.
    ///
    /// ⚠️ Also the guard on the DEGENERATE TANGENT. Zero handles make the bezier
    /// derivative exactly zero at `t = 0`, so a plain `normalized()` there yields
    /// NaN and poisons the entire chain — every later up vector included.
    /// 🔑 A segment already shorter than the interval is not subdivided.
    ///
    /// This is the chord test itself: Godot subdivides only while the CHORD of
    /// the current sub-interval exceeds `bake_interval`.
    #[test]
    fn even_length_schedule_leaves_a_short_segment_alone() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
            p3([1.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
        ];
        let sched = even_length_schedule_3d(&pts, 10, 5.0);
        assert_eq!(sched.len(), 1, "one segment, one schedule entry");
        assert!(
            sched[0].is_empty(),
            "a chord of 1.0 under an interval of 5.0 needs no midpoint, got {:?}",
            sched[0],
        );
    }

    /// 🔑 A finer interval refines further, and the depth cap bounds it.
    ///
    /// ⚠️ The cap is what makes this ADAPTIVE rather than unbounded: Godot passes
    /// `max_stages = 10`, so a segment can contribute at most `2^10 - 1`
    /// interior points however tight it is. A `segment_length / bake_interval`
    /// step count has no such ceiling, which is one way the two differ on a
    /// pathological curve.
    #[test]
    fn even_length_schedule_refines_with_the_interval_and_respects_the_cap() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
            p3([10.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
        ];
        let coarse = even_length_schedule_3d(&pts, 10, 4.0)[0].len();
        let fine = even_length_schedule_3d(&pts, 10, 0.5)[0].len();
        assert!(
            fine > coarse,
            "a finer interval must produce more samples: {fine} vs {coarse}",
        );

        // Depth 3 can insert at most 2^3 - 1 = 7 interior points.
        let capped = even_length_schedule_3d(&pts, 3, 1e-4)[0].len();
        assert!(
            capped <= 7,
            "max_stages = 3 bounds the segment at 7 interior points, got {capped}",
        );
    }

    /// 🔑 The schedule is sorted and strictly interior.
    ///
    /// ⚠️ The bakers will walk `[0, ..schedule.., 1]` and index the result
    /// against the tilt and up-vector arrays, so an out-of-order or duplicated
    /// `t` would silently misalign all three. Godot gets this for free from an
    /// RBMap keyed by `t`; a Vec has to be sorted deliberately.
    #[test]
    fn even_length_schedule_is_sorted_and_interior() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [4.0, 4.0, 0.0]),
            p3([8.0, 0.0, 0.0], [-4.0, 4.0, 0.0], [0.0; 3]),
        ];
        let ts = &even_length_schedule_3d(&pts, 10, 0.25)[0];
        assert!(
            ts.len() > 4,
            "a tight curve should subdivide, got {}",
            ts.len()
        );
        for w in ts.windows(2) {
            assert!(
                w[1] > w[0],
                "t values must be strictly increasing, got {:?} then {:?}",
                w[0],
                w[1],
            );
        }
        assert!(
            ts.iter().all(|t| *t > 0.0 && *t < 1.0),
            "the schedule excludes the endpoints; the bakers add them",
        );
    }

    #[test]
    fn up_vectors_along_a_straight_segment_are_world_up() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
            p3([10.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
        ];
        let ups = bake_curve_3d_up_vectors(&pts, &uniform_schedule(pts.len() - 1, 16));
        assert_eq!(
            ups.len(),
            bake_curve_3d(&pts, &uniform_schedule(pts.len() - 1, 16)).len(),
            "index-for-index"
        );
        for (i, up) in ups.iter().enumerate() {
            assert!(
                near(*up, Vector3::new(0.0, 1.0, 0.0)),
                "sample {i} should stay world up, got {up:?}"
            );
        }
    }
    /// 🔑 THE ASSERTION THAT REJECTS A FRENET FRAME.
    ///
    /// On a curve that turns in the HORIZONTAL plane the tangent rotates about
    /// world Y, so a parallel-transported up is carried by a rotation about Y —
    /// which leaves `+Y` exactly where it was. A Frenet frame derives up from the
    /// curvature normal instead, which here points HORIZONTALLY at the centre of
    /// the turn: it would bank the follower through a flat corner.
    ///
    /// Godot cites the parallel-transport paper by name in `Curve3D::_bake` for
    /// precisely this reason, so "up is still exactly +Y" is the parity check.
    #[test]
    fn a_horizontal_turn_does_not_bank_the_up_vector() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [5.0, 0.0, 0.0]),
            p3([10.0, 0.0, 10.0], [0.0, 0.0, -5.0], [0.0; 3]),
        ];
        let ups = bake_curve_3d_up_vectors(&pts, &uniform_schedule(pts.len() - 1, 16));
        for (i, up) in ups.iter().enumerate() {
            assert!(
                near(*up, Vector3::new(0.0, 1.0, 0.0)),
                "a flat corner must not roll the frame; sample {i} is {up:?}"
            );
        }
    }

    /// A curve that climbs DOES carry its up vector round with the tangent.
    ///
    /// The arc turns from heading `+X` to heading `+Y`, a quarter turn about Z,
    /// so up ends a quarter turn away from where it started: `+Y` becomes `-X`.
    /// Without this the test above would be satisfied by an implementation that
    /// simply returned world up for everything.
    #[test]
    fn a_climbing_turn_carries_the_up_vector_round() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [5.0, 0.0, 0.0]),
            p3([10.0, 10.0, 0.0], [0.0, -5.0, 0.0], [0.0; 3]),
        ];
        let ups = bake_curve_3d_up_vectors(&pts, &uniform_schedule(pts.len() - 1, 16));
        assert!(
            near(ups[0], Vector3::new(0.0, 1.0, 0.0)),
            "the seed is the world-up rule, got {:?}",
            ups[0]
        );
        assert!(
            near(*ups.last().unwrap(), Vector3::new(-1.0, 0.0, 0.0)),
            "a quarter turn upward must take up from +Y to -X, got {:?}",
            ups.last().unwrap()
        );
    }

    /// The frame stays orthonormal the whole way.
    ///
    /// ⚠️ This is what the per-step re-orthogonalization buys, and it is the
    /// failure no endpoint assertion can see: without it the incremental
    /// rotations accumulate float error and the up vector drifts out of the
    /// plane perpendicular to the tangent by a slowly growing amount, tilting
    /// the whole frame on a long curve.
    #[test]
    fn transported_up_vectors_stay_unit_and_perpendicular() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [5.0, 0.0, 0.0]),
            p3([10.0, 10.0, 0.0], [0.0, -5.0, 0.0], [3.0, 0.0, 3.0]),
            p3([20.0, 6.0, 8.0], [0.0, 4.0, -4.0], [0.0; 3]),
        ];
        let points = bake_curve_3d(&pts, &uniform_schedule(pts.len() - 1, 16));
        let ups = bake_curve_3d_up_vectors(&pts, &uniform_schedule(pts.len() - 1, 16));
        assert_eq!(ups.len(), points.len());
        for (i, up) in ups.iter().enumerate() {
            assert!(
                (up.length() - 1.0).abs() < 1e-3,
                "sample {i} up must stay unit length, got {}",
                up.length()
            );
        }
        // ⚠️ Perpendicularity is NOT asserted against the polyline chord here,
        // and that is deliberate: a chord is a SECANT, so on a 16-step bake of a
        // curving segment it lags the true tangent by ~3 degrees (measured:
        // |up . chord| reaches 0.05 even where |up . tangent| is exactly 0). A
        // tolerance loose enough to accommodate that could not fail for any real
        // defect, so unit length — which IS exact — carries the assertion.
    }

    /// ⚠️ A VERTICAL curve degenerates the Y-up seed rule, and Godot has a
    /// branch for it (`curve.cpp:1799-1803`).
    ///
    /// When the initial tangent is nearly parallel to world Y there is no usable
    /// `up x forward`: the cross product collapses and the frame would come out
    /// NaN. Godot switches the reference to world X, so the seed becomes `+X`.
    /// Without the guard a purely vertical Path3D produces NaN up vectors and
    /// takes the follower's whole transform with it.
    #[test]
    fn a_vertical_curve_seeds_its_frame_from_world_x() {
        let pts = [
            p3([0.0, 0.0, 0.0], [0.0; 3], [0.0; 3]),
            p3([0.0, 10.0, 0.0], [0.0; 3], [0.0; 3]),
        ];
        let ups = bake_curve_3d_up_vectors(&pts, &uniform_schedule(pts.len() - 1, 16));
        for (i, up) in ups.iter().enumerate() {
            assert!(
                up.x.is_finite() && up.y.is_finite() && up.z.is_finite(),
                "sample {i} must not be NaN, got {up:?}"
            );
            assert!(
                near(*up, Vector3::new(1.0, 0.0, 0.0)),
                "a vertical curve seeds from world X, got {up:?} at {i}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3D (pat-s3rrv.83)
// ---------------------------------------------------------------------------

/// One control point of a 3D Bezier path.
///
/// Handles are RELATIVE to the position, the same convention Godot serialises
/// (`curve.cpp` `Curve3D::_set_data`: `in`, `out`, `position` per triple).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvePoint3 {
    /// The control point itself.
    pub position: Vector3,
    /// Incoming handle, relative to [`position`](Self::position).
    pub in_handle: Vector3,
    /// Outgoing handle, relative to [`position`](Self::position).
    pub out_handle: Vector3,
}

fn cubic_bezier_3d(p0: Vector3, c1: Vector3, c2: Vector3, p3: Vector3, t: f32) -> Vector3 {
    let u = 1.0 - t;
    p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// Samples `points` into a polyline by evaluating each Bezier segment `steps`
/// times — the 3D twin of [`bake_curve`] (pat-s3rrv.83).
///
/// Returns positions only. The 2D baker also carries an arc-length table
/// because its consumer needs distances; the 3D path driver samples the
/// `curve_points` polyline directly, so a distance column here would be an
/// unused second source of truth.
///
/// An empty curve yields an empty polyline; a single point yields that point,
/// which is what makes a one-point Path3D render as a dot rather than nothing.
/// The unit tangent of one Bezier segment at `t`, mirroring Godot's
/// `Curve3D::_calculate_tangent` (`curve.cpp:882-906`).
///
/// ⚠️ **THE DEGENERATE CASES ARE THE POINT, NOT DEFENSIVE PADDING.** A segment
/// whose out-handle sits on its start point has a derivative of EXACTLY ZERO at
/// `t = 0` — and zero handles are the most common way to author a straight
/// segment — so the plain derivative normalizes to NaN there and poisons every
/// frame built from it. Godot substitutes the chord (or the far handle) in that
/// case, and so does this.
fn bezier_tangent_3d(p0: Vector3, c1: Vector3, c2: Vector3, p3: Vector3, t: f32) -> Vector3 {
    let close = |a: Vector3, b: Vector3| (a - b).length() < 1e-5;
    if t.abs() < 1e-5 && close(c1, p0) {
        return if close(c1, c2) {
            (p3 - p0).normalized()
        } else {
            (c2 - p0).normalized()
        };
    }
    if (t - 1.0).abs() < 1e-5 && close(c2, p3) {
        return if close(c2, c1) {
            (p3 - p0).normalized()
        } else {
            (p3 - c1).normalized()
        };
    }
    if close(c1, p3) && close(c2, p0) {
        return (p3 - p0).normalized();
    }
    let u = 1.0 - t;
    let d = (c1 - p0) * (3.0 * u * u) + (c2 - c1) * (6.0 * u * t) + (p3 - c2) * (3.0 * t * t);
    if d.length() < 1e-8 {
        return (p3 - p0).normalized();
    }
    d.normalized()
}

/// Per-sample UP VECTORS along the curve, index-for-index with
/// [`bake_curve_3d`] — Godot's `baked_up_vector_cache`.
///
/// This is a **parallel transport frame** (Dougan, "The Parallel Transport
/// Frame", *Game Programming Gems 2*), which Godot cites by name in
/// `Curve3D::_bake`. The up vector starts from the world Y rule and is then
/// carried along the curve by the MINIMAL rotation that takes each sample's
/// tangent to the next one's.
///
/// ⚠️ **NOT the Frenet frame**, which is the obvious alternative and is wrong
/// here: Frenet derives up from the curvature normal, so it flips discontinuously
/// wherever the curve is momentarily straight (curvature 0) and spins through
/// inflection points. Parallel transport has no such singularity — that is
/// exactly why Godot's comment points at the paper.
///
/// The seed follows Godot's Y-up rule with its own guard: when the initial
/// tangent is nearly parallel to world Y there is no usable `up × forward`, so
/// the reference switches to world X (`curve.cpp:1799-1803`).
pub fn bake_curve_3d_up_vectors(points: &[CurvePoint3], schedule: &[Vec<f32>]) -> Vec<Vector3> {
    let mut ups: Vec<Vector3> = Vec::new();
    if points.len() < 2 {
        // One point (or none) has no direction to transport along. Godot stores
        // a single world-up entry rather than an empty cache.
        if points.len() == 1 {
            ups.push(Vector3::new(0.0, 1.0, 0.0));
        }
        return ups;
    }
    // Sample tangents on the same cadence the polyline uses, so index i of both
    // arrays describes one place on the curve.
    let mut forwards: Vec<Vector3> = Vec::new();
    {
        let (a, b) = (points[0], points[1]);
        forwards.push(bezier_tangent_3d(
            a.position,
            a.position + a.out_handle,
            b.position + b.in_handle,
            b.position,
            0.0,
        ));
    }
    for (i, seg) in points.windows(2).enumerate() {
        let (a, b) = (seg[0], seg[1]);
        let p0 = a.position;
        let c1 = a.position + a.out_handle;
        let c2 = b.position + b.in_handle;
        let p3 = b.position;
        for t in schedule.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            forwards.push(bezier_tangent_3d(p0, c1, c2, p3, *t));
        }
        forwards.push(bezier_tangent_3d(p0, c1, c2, p3, 1.0));
    }

    // Seed: the Y-up rule, with Godot's near-parallel guard.
    let world_y = Vector3::new(0.0, 1.0, 0.0);
    let world_x = Vector3::new(1.0, 0.0, 0.0);
    let reference = if forwards[0].dot(world_y).abs() > 0.999_9 {
        world_x
    } else {
        world_y
    };
    let mut up = orthonormal_up(forwards[0], reference);
    ups.push(up);

    for i in 1..forwards.len() {
        // The minimal rotation carrying the previous tangent onto this one,
        // applied to the up vector. Godot builds the same rotation with
        // `Basis::rotate_to_align` and multiplies the whole frame; carrying just
        // the up vector is equivalent and needs no Basis orthonormalization.
        let (prev, next) = (forwards[i - 1], forwards[i]);
        let axis = prev.cross(next);
        if axis.length() > 1e-6 {
            let angle = prev.dot(next).clamp(-1.0, 1.0).acos();
            if angle.abs() > 1e-6 {
                up = rotate_about_axis(up, axis.normalized(), angle);
            }
        }
        // Re-orthogonalize every step. ⚠️ Godot calls `frame.orthonormalize()`
        // here for the same reason: the incremental rotations accumulate float
        // error, and an up vector that drifts out of the plane perpendicular to
        // the tangent tilts the whole frame by a slowly growing amount that no
        // short curve reveals.
        up = orthonormal_up(next, up);
        ups.push(up);
    }
    ups
}

/// The component of `reference` perpendicular to `forward`, normalized —
/// the frame's Y column for a `looking_at(forward, reference)` basis.
fn orthonormal_up(forward: Vector3, reference: Vector3) -> Vector3 {
    let f = forward.normalized();
    let projected = f * reference.dot(f);
    let up = reference - projected;
    if up.length() < 1e-6 {
        // Colinear: any perpendicular will do, which is Godot's
        // `get_any_perpendicular` fallback in `Basis::looking_at`.
        let alt = if f.x.abs() < 0.9 {
            Vector3::new(1.0, 0.0, 0.0)
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
        let up = alt - f * alt.dot(f);
        return up.normalized();
    }
    up.normalized()
}

/// Rodrigues' rotation of `v` about a UNIT `axis` by `angle` radians.
fn rotate_about_axis(v: Vector3, axis: Vector3, angle: f32) -> Vector3 {
    let (sin, cos) = angle.sin_cos();
    v * cos + axis.cross(v) * sin + axis * (axis.dot(v) * (1.0 - cos))
}

/// Bakes per-sample tilts to match [`bake_curve_3d`] index for index.
///
/// `control_tilts` is one radian angle per CONTROL point — Godot's
/// `_data["tilts"]` — and the result has the same length and the same cadence as
/// the polyline: `1 + (n - 1) * steps` entries, so `baked_points[i]` and
/// `baked_tilts[i]` always describe the same place on the curve.
///
/// Godot bakes tilt by linear interpolation in the BEZIER PARAMETER
/// (`curve.cpp:1749`: `btw[pidx] = Math::lerp(points[i].tilt, points[i+1].tilt,
/// E.key)`), not in arc length — so a tilt turns at an even rate along the
/// parameter and therefore slightly unevenly along a curved segment, exactly as
/// the positions do.
///
/// ⚠️ Returns an EMPTY vec when `control_tilts` is shorter than the control
/// points rather than padding with zeros. A short array means the `.tscn` was
/// not written by Godot — which always emits one tilt per point — and silently
/// substituting zeros would tilt part of the path and look like a modelling
/// mistake rather than a load failure.
pub fn bake_curve_3d_tilts(control_tilts: &[f32], schedule: &[Vec<f32>]) -> Vec<f32> {
    let mut baked: Vec<f32> = Vec::new();
    if control_tilts.is_empty() {
        return baked;
    }
    baked.push(control_tilts[0]);
    for (i, pair) in control_tilts.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        for t in schedule.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            baked.push(a + (b - a) * *t);
        }
        baked.push(b);
    }
    baked
}

/// Recursive half of [`even_length_schedule_2d`], mirroring
/// `Curve2D::_bake_segment2d_even_length` (`curve.cpp:1300-1310`).
///
/// ⚠️ Structurally identical to [`subdivide_even_length_3d`] — split while the
/// CHORD of the sub-interval is longer than the interval and the depth cap is
/// not reached. Kept as a separate function rather than made generic because the
/// two operate on different vector types; the shapes must stay in step.
#[allow(clippy::too_many_arguments)]
fn subdivide_even_length_2d(
    out: &mut Vec<f32>,
    begin: f32,
    end: f32,
    p0: Vector2,
    c1: Vector2,
    c2: Vector2,
    p3: Vector2,
    depth: u32,
    max_depth: u32,
    max_chord: f32,
) {
    let beg_p = cubic_bezier(p0, c1, c2, p3, begin);
    let end_p = cubic_bezier(p0, c1, c2, p3, end);
    if beg_p.distance_to(end_p) <= max_chord || depth >= max_depth {
        return;
    }
    let mid = (begin + end) * 0.5;
    out.push(mid);
    subdivide_even_length_2d(
        out,
        begin,
        mid,
        p0,
        c1,
        c2,
        p3,
        depth + 1,
        max_depth,
        max_chord,
    );
    subdivide_even_length_2d(
        out,
        mid,
        end,
        p0,
        c1,
        c2,
        p3,
        depth + 1,
        max_depth,
        max_chord,
    );
}

/// The 2D twin of [`even_length_schedule_3d`]: per segment, the interior `t`
/// values Godot samples at for a given `bake_interval`.
///
/// ⚠️ Its interval is in PIXELS and defaults to [`CURVE2D_BAKE_INTERVAL`] (5.0),
/// twenty-five times the 3D default. Feeding a Curve2D the 3D constant would
/// subdivide a typical sprite path into thousands of points.
pub fn even_length_schedule_2d(
    points: &[CurvePoint],
    max_stages: u32,
    bake_interval: f32,
) -> Vec<Vec<f32>> {
    if points.len() < 2 {
        return Vec::new();
    }
    // Same reading as the 3D schedule: a non-positive interval means "do not
    // subdivide" rather than "recurse to the cap".
    let max_chord = if bake_interval > 0.0 {
        bake_interval
    } else {
        f32::INFINITY
    };
    let mut schedule = Vec::with_capacity(points.len() - 1);
    for seg in points.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let mut ts = Vec::new();
        subdivide_even_length_2d(
            &mut ts,
            0.0,
            1.0,
            a.position,
            a.position + a.out_handle,
            b.position + b.in_handle,
            b.position,
            0,
            max_stages,
            max_chord,
        );
        ts.sort_by(|l, r| l.partial_cmp(r).unwrap_or(std::cmp::Ordering::Equal));
        schedule.push(ts);
    }
    schedule
}

/// Godot `Curve3D.bake_interval` default, in METRES
/// (`doc/classes/Curve3D.xml:204`).
pub const CURVE3D_BAKE_INTERVAL: f32 = 0.2;

/// Godot `Curve2D.bake_interval` default, in PIXELS (`Curve2D.xml:174`).
///
/// ⚠️ 25x the 3D default and deliberately a separate constant: the two count
/// different units, so sharing one value would silently resample every 2D curve
/// at a metre-scale density.
pub const CURVE2D_BAKE_INTERVAL: f32 = 5.0;

/// Subdivision depth `Curve::_bake()` passes (`curve.cpp:937`).
///
/// ⚠️ NOT the `max_stages = 5` default on the public `tessellate_even_length()`
/// in `curve.h:313`. Five caps a segment at 32 pieces, which a fine
/// `bake_interval` blows straight through.
pub const MAX_BAKE_STAGES: u32 = 10;

/// A schedule with `steps` evenly spaced samples per segment — the fixed cadence
/// the bakers used before `bake_interval` was honoured.
///
/// Kept for callers that want a specific density regardless of curvature (the
/// module's own tests do, so their expected values stay derivable by hand).
pub fn uniform_schedule(segments: usize, steps: usize) -> Vec<Vec<f32>> {
    let steps = steps.max(1);
    (0..segments)
        .map(|_| {
            (1..steps)
                .map(|s| s as f32 / steps as f32)
                .collect::<Vec<f32>>()
        })
        .collect()
}

/// Recursive half of [`even_length_schedule_3d`] — Godot's
/// `_bake_segment3d_even_length` (`scene/resources/curve.cpp`).
///
/// Subdivides while the CHORD between the current sub-interval's endpoints is
/// longer than `max_chord`, capped at `max_depth` levels. Godot inserts the
/// midpoint before recursing into both halves and keeps them in a map keyed by
/// `t`; collecting into a Vec and sorting reproduces that order.
fn subdivide_even_length_3d(
    out: &mut Vec<f32>,
    begin: f32,
    end: f32,
    p0: Vector3,
    c1: Vector3,
    c2: Vector3,
    p3: Vector3,
    depth: u32,
    max_depth: u32,
    max_chord: f32,
) {
    let beg_p = cubic_bezier_3d(p0, c1, c2, p3, begin);
    let end_p = cubic_bezier_3d(p0, c1, c2, p3, end);
    if (end_p - beg_p).length() <= max_chord || depth >= max_depth {
        return;
    }
    let mid = (begin + end) * 0.5;
    out.push(mid);
    subdivide_even_length_3d(
        out,
        begin,
        mid,
        p0,
        c1,
        c2,
        p3,
        depth + 1,
        max_depth,
        max_chord,
    );
    subdivide_even_length_3d(
        out,
        mid,
        end,
        p0,
        c1,
        c2,
        p3,
        depth + 1,
        max_depth,
        max_chord,
    );
}

/// Godot's adaptive bake schedule: for each segment, the interior `t` values it
/// samples at, sorted and excluding the endpoints (pat-s3rrv.91 item 4).
///
/// Godot's `Curve3D::_bake` calls `_tessellate_even_length(10, bake_interval)`
/// (`curve.cpp`), which subdivides a segment recursively while the CHORD of the
/// current sub-interval exceeds `bake_interval`, to a depth of `max_stages`.
///
/// ⚠️ **NOT `segment_length / bake_interval`.** A step count derived from arc
/// length produces a similar point count on a gentle curve and diverges badly on
/// a tight one, so it reads as parity without being it.
///
/// ⚠️ Returned as a SCHEDULE rather than as points, deliberately. The three 3D
/// bakers — geometry, tilts and up vectors — produce per-sample arrays the
/// driver reads BY INDEX, and today the only thing keeping them aligned is that
/// all three are handed the same uniform step count. Adaptive subdivision varies
/// the count per segment, and `bake_curve_3d_tilts` receives no geometry, so it
/// could never derive the same subdivision itself. Every baker consuming one
/// schedule makes that alignment structural instead of a convention three call
/// sites have to remember.
pub fn even_length_schedule_3d(
    points: &[CurvePoint3],
    max_stages: u32,
    bake_interval: f32,
) -> Vec<Vec<f32>> {
    if points.len() < 2 {
        return Vec::new();
    }
    // A non-positive interval would subdivide to the depth cap on every segment
    // rather than erroring; treat it as "no subdivision", which is what a caller
    // asking for zero spacing can actually be given.
    let max_chord = if bake_interval > 0.0 {
        bake_interval
    } else {
        f32::INFINITY
    };

    let mut schedule = Vec::with_capacity(points.len() - 1);
    for seg in points.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let mut ts = Vec::new();
        subdivide_even_length_3d(
            &mut ts,
            0.0,
            1.0,
            a.position,
            a.position + a.out_handle,
            b.position + b.in_handle,
            b.position,
            0,
            max_stages,
            max_chord,
        );
        ts.sort_by(|l, r| l.partial_cmp(r).unwrap_or(std::cmp::Ordering::Equal));
        schedule.push(ts);
    }
    schedule
}

pub fn bake_curve_3d(points: &[CurvePoint3], schedule: &[Vec<f32>]) -> Vec<Vector3> {
    let mut baked: Vec<Vector3> = Vec::new();
    if points.is_empty() {
        return baked;
    }
    baked.push(points[0].position);
    for (i, seg) in points.windows(2).enumerate() {
        let (a, b) = (seg[0], seg[1]);
        let p0 = a.position;
        let c1 = a.position + a.out_handle;
        let c2 = b.position + b.in_handle;
        let p3 = b.position;
        for t in schedule.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            baked.push(cubic_bezier_3d(p0, c1, c2, p3, *t));
        }
        // The segment's own end point, exactly rather than as `bezier(1.0)`.
        baked.push(p3);
    }
    baked
}
