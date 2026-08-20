//! Read-only [`Curve2D`] queries (pat-s3rrv.241).
//!
//! A companion module rather than more `impl Curve2D`, for the same reason
//! [`crate::curve3d_query`] is one: `path3d.rs` holds both curve types and is
//! contended. Everything here reads only `Curve2D`'s public point data, so it
//! folds into the inherent impl later without a line of its body changing.

use gdcore::math::Vector2;

use crate::path3d::Curve2D;

/// Godot `Curve2D.get_closest_offset` — the arc-length offset of the point on
/// the baked polyline nearest to `to_point`.
///
/// The 2D twin of [`crate::curve3d_query::closest_offset`], ported from the same
/// upstream source (`scene/resources/curve.cpp`, which labels it the "brute
/// force method"): walk each baked segment, project `to_point` onto it clamped
/// to the segment, and keep the offset of whichever projection lands nearest.
///
/// ⚠️ An exact per-segment projection, NOT a sampled search. A sampled search
/// evaluates a fixed number of positions along the whole curve and returns the
/// best of them, so its answer is quantised to the sample spacing and degrades
/// as the curve lengthens. Projection is both exact and cheaper, and it is what
/// Godot's API contract for this method describes.
///
/// ⚠️ Zero-length segments are SKIPPED. Godot divides by the segment interval
/// without guarding, so a duplicated point yields a division by zero and then a
/// NaN offset, which propagates into every position the caller derives from it.
/// Coincident control points are ordinary authoring, so this is reachable —
/// the same hazard `Curve2D::sample_baked` already guards against.
pub fn closest_offset(curve: &Curve2D, to_point: Vector2) -> f32 {
    if curve.points.len() < 2 {
        // Godot: no points answers 0, and a single point is offset 0 by
        // definition — there is no segment to project onto.
        return 0.0;
    }

    let mut nearest = 0.0f32;
    let mut nearest_dist = -1.0f32;
    let mut offset = 0.0f32;

    for i in 0..curve.points.len() - 1 {
        let origin = curve.points[i];
        let to_next = curve.points[i + 1] - origin;
        let interval = to_next.length();
        if interval <= 0.0 {
            // Contributes no arc length and has no direction; skipping keeps
            // `offset` correct and avoids the NaN.
            continue;
        }

        // Unit, because the baked polyline's arc length between consecutive
        // points IS the chord between them.
        let direction = to_next * (1.0 / interval);
        let d = (to_point - origin).dot(direction).clamp(0.0, interval);
        let proj = origin + direction * d;
        let dist = (proj - to_point).length_squared();

        if nearest_dist < 0.0 || dist < nearest_dist {
            nearest = offset + d;
            nearest_dist = dist;
        }
        offset += interval;
    }

    nearest
}
