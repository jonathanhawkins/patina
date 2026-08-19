//! Read-only `Curve3D` queries (pat-s3rrv.91 item 5).
//!
//! ⚠️ A companion module rather than more `impl Curve3D`, because `path3d.rs` is
//! leased to another agent while this lands. Every function here reads only
//! `Curve3D`'s public baked data, so nothing is duplicated and these fold into
//! the inherent impl later without changing a line of their bodies.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Quaternion, Transform3D};

use crate::path3d::Curve3D;

/// Godot `Curve3D.get_closest_offset` — the arc-length offset of the point on
/// the baked polyline nearest to `to_point`.
///
/// Ported from `scene/resources/curve.cpp` (`Curve3D::get_closest_offset`,
/// which upstream labels "brute force method"): walk each baked segment, project
/// `to_point` onto it clamped to the segment, and keep the offset of whichever
/// projection lands nearest.
///
/// ⚠️ This is an exact per-segment projection, NOT the sampled search
/// [`Curve3D::get_closest_point`] does. That one evaluates ~32 positions along
/// the whole curve and returns the best of them, so its answer is quantised to
/// the sample spacing and gets worse as the curve gets longer. Projection is
/// both exact and cheaper. The two are not interchangeable and this is the one
/// Godot's API contract describes.
///
/// ⚠️ Zero-length segments are SKIPPED. Godot divides by the segment interval
/// without guarding, so a duplicated baked point yields a division by zero and
/// then a NaN offset — which propagates into whatever positions the caller
/// derives from it. Duplicated baked points are reachable here: a curve whose
/// control points coincide bakes them.
pub fn closest_offset(curve: &Curve3D, to_point: Vector3) -> f32 {
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
        // samples IS the chord between them.
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

/// Godot `Curve3D.sample_baked_with_rotation(offset, cubic, apply_tilt)` — the
/// full posture at an arc-length offset, not just its position
/// (`curve.cpp` `Curve3D::sample_baked_with_rotation` / `_sample_posture`).
///
/// The returned basis has **-Z along the tangent**, per [`looking_at_basis`].
/// `apply_tilt` twists it about that tangent by the baked tilt, which is
/// upstream's `Basis twist(tangent, tilt); return twist * frame` — the twist
/// pre-multiplies, so it rotates the finished frame rather than being composed
/// into it before the look-at.
///
/// ⚠️ Shares [`looking_at_basis`] with the PathFollow3D driver rather than
/// rebuilding the frame here. The forward axis is Godot's negated -Z, and a
/// second copy of that convention would be free to drift from the first — a
/// curve method that disagreed with the follower walking it would be worse than
/// no method at all.
///
/// ⚠️ **One documented deviation from upstream.** Godot composes a frame at each
/// bracketing baked sample and SLERPs the two frames, then orthonormalizes;
/// this interpolates the up vector and tangent first and builds one frame from
/// them. The two agree at every baked sample and differ by a fraction of a
/// degree between them, and this order is what the driver already uses — so
/// matching upstream's order here would make the method disagree with the
/// follower it describes. Consistency with the driver is the more useful
/// property; the difference is noted rather than hidden.
pub fn sample_baked_with_rotation(
    curve: &Curve3D,
    offset: f32,
    cubic: bool,
    apply_tilt: bool,
) -> Transform3D {
    // Upstream ERR_FAILs to an identity transform on an empty curve, and
    // returns a translation-only transform when there is exactly one point:
    // with no second point there is no tangent, so there is no frame to build.
    if curve.points.is_empty() {
        return Transform3D::IDENTITY;
    }
    if curve.points.len() == 1 {
        return Transform3D {
            basis: Basis::IDENTITY,
            origin: curve.points[0],
        };
    }

    let offset = offset.clamp(0.0, curve.baked_length());
    let origin = if cubic {
        curve.sample_baked_cubic(offset)
    } else {
        curve.sample_baked(offset)
    };

    let tangent = curve.sample_tangent(offset);
    let up = curve.sample_baked_up_vector(offset);
    let frame = crate::main_loop::looking_at_basis(tangent, up);
    if !apply_tilt {
        return Transform3D {
            basis: frame,
            origin,
        };
    }

    let tilt = curve.sample_baked_tilt(offset);
    let twist = Basis::from_quaternion(Quaternion::from_axis_angle(tangent.normalized(), tilt));
    Transform3D {
        basis: twist * frame,
        origin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near3(a: Vector3, b: Vector3) -> bool {
        (a - b).length() < 1e-3
    }

    /// 🔑 The frame points **-Z** along the tangent, which is Godot's negation.
    ///
    /// ⚠️ The paired negative assertion is the whole test. A frame built without
    /// the negation is still orthonormal, still tracks the curve, and still
    /// looks correct in every length or perpendicularity check — it just faces
    /// backwards, so every follower travels rear-first. Only comparing the sign
    /// of the Z column separates the two.
    #[test]
    fn sample_baked_with_rotation_points_negative_z_along_the_tangent() {
        let c = straight();
        let t = sample_baked_with_rotation(&c, 2.0, false, false);

        assert!(
            near3(t.origin, Vector3::new(2.0, 0.0, 0.0)),
            "the origin must be the sampled position, got {:?}",
            t.origin,
        );
        assert!(
            near3(t.basis.z, Vector3::new(-1.0, 0.0, 0.0)),
            "the Z column must be the NEGATED tangent; got {:?}. (1, 0, 0) here \
             means the look-at negation was dropped and the frame faces \
             backwards along the path",
            t.basis.z,
        );
        assert!(
            near3(t.basis.y, Vector3::new(0.0, 1.0, 0.0)),
            "an untilted straight run keeps world +Y as its up column, got {:?}",
            t.basis.y,
        );
    }

    /// 🔑 `apply_tilt` twists the frame about the tangent — and leaves the
    /// tangent itself alone.
    ///
    /// A tilt of PI/2 about +X carries the up column (0,1,0) to (0,0,1). The
    /// second assertion is the positive control: a twist that also moved the Z
    /// column would not be a twist about the tangent at all, and asserting only
    /// that "up changed" would accept it.
    #[test]
    fn apply_tilt_twists_the_frame_about_the_tangent() {
        let mut c = straight();
        c.tilts = vec![std::f32::consts::FRAC_PI_2; c.points.len()];

        let untilted = sample_baked_with_rotation(&c, 2.0, false, false);
        assert!(
            near3(untilted.basis.y, Vector3::new(0.0, 1.0, 0.0)),
            "with apply_tilt = false the baked tilt must be ignored, got {:?}",
            untilted.basis.y,
        );

        let tilted = sample_baked_with_rotation(&c, 2.0, false, true);
        assert!(
            near3(tilted.basis.y, Vector3::new(0.0, 0.0, 1.0)),
            "a PI/2 tilt about +X must carry the up column to (0, 0, 1), got {:?}",
            tilted.basis.y,
        );
        assert!(
            near3(tilted.basis.z, Vector3::new(-1.0, 0.0, 0.0)),
            "…while the tangent column is untouched, got {:?}. A twist that \
             moved this is rotating about the wrong axis",
            tilted.basis.z,
        );
        assert!(
            near3(tilted.origin, untilted.origin),
            "tilt rotates the frame, it does not move the point",
        );
    }

    /// Degenerate curves match upstream's guards rather than panicking.
    #[test]
    fn sample_baked_with_rotation_handles_degenerate_curves() {
        let empty = sample_baked_with_rotation(&Curve3D::new(), 1.0, false, true);
        assert_eq!(
            empty,
            Transform3D::IDENTITY,
            "an empty curve has no posture to report",
        );

        let mut one = Curve3D::new();
        one.add_point(Vector3::new(3.0, 4.0, 5.0));
        let single = sample_baked_with_rotation(&one, 1.0, false, true);
        assert!(
            near3(single.origin, Vector3::new(3.0, 4.0, 5.0)),
            "a one-point curve reports that point as the origin, got {:?}",
            single.origin,
        );
        assert_eq!(
            single.basis,
            Basis::IDENTITY,
            "…with no second point there is no tangent, so the basis stays identity",
        );
    }

    /// A straight run along +X, 4 units long, sampled every unit.
    fn straight() -> Curve3D {
        let mut c = Curve3D::new();
        for i in 0..5 {
            c.add_point(Vector3::new(i as f32, 0.0, 0.0));
        }
        c
    }

    /// 🔑 A point beside the curve answers with the offset of its foot.
    #[test]
    fn curve3d_closest_offset_projects_onto_the_segment() {
        let c = straight();
        // Directly above x = 2.5 — which is INSIDE a segment, not on a sample.
        let got = closest_offset(&c, Vector3::new(2.5, 7.0, 0.0));
        assert!(
            (got - 2.5).abs() < 1e-4,
            "the offset must be the projection's arc length, 2.5; got {got}. A \
             sampled search would answer 2.0 or 3.0 — whichever sample happened \
             to be nearest",
        );
    }

    /// 🔑 The endpoints clamp instead of running off the curve.
    #[test]
    fn curve3d_closest_offset_clamps_past_the_ends() {
        let c = straight();
        assert!(
            closest_offset(&c, Vector3::new(-10.0, 0.0, 0.0)).abs() < 1e-4,
            "a point before the start clamps to offset 0",
        );
        let end = closest_offset(&c, Vector3::new(99.0, 0.0, 0.0));
        assert!(
            (end - 4.0).abs() < 1e-4,
            "a point past the end clamps to the baked length, 4.0; got {end}",
        );
    }

    /// 🔑 A duplicated baked point must not produce NaN.
    ///
    /// ⚠️ Godot divides by the segment interval unguarded, so this input yields
    /// `0/0` upstream. A NaN offset does not fail loudly — it flows into the
    /// caller's sampled position and puts the follower nowhere.
    #[test]
    fn curve3d_closest_offset_survives_a_duplicated_point() {
        let mut c = Curve3D::new();
        c.add_point(Vector3::new(0.0, 0.0, 0.0));
        c.add_point(Vector3::new(1.0, 0.0, 0.0));
        c.add_point(Vector3::new(1.0, 0.0, 0.0)); // zero-length segment
        c.add_point(Vector3::new(2.0, 0.0, 0.0));

        let got = closest_offset(&c, Vector3::new(1.5, 1.0, 0.0));
        assert!(
            got.is_finite(),
            "a duplicated baked point must not yield NaN"
        );
        assert!(
            (got - 1.5).abs() < 1e-4,
            "and the zero-length segment must contribute no arc length, so the \
             answer is still 1.5; got {got}",
        );
    }

    /// An empty curve answers 0 rather than panicking.
    #[test]
    fn curve3d_closest_offset_of_an_empty_curve_is_zero() {
        assert_eq!(closest_offset(&Curve3D::new(), Vector3::ZERO), 0.0);
    }
}
