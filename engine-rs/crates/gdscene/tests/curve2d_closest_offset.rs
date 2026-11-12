//! pat-s3rrv.241 — `Curve2D.get_closest_offset`'s algorithm.
//!
//! The 3D twin has had this since pat-s3rrv.91 (`curve3d_query::closest_offset`);
//! `Curve2D` had no equivalent at all, so the `get_closest_offset` the bead's
//! acceptance calls from GDScript had nothing to call.
//!
//! ⚠️ These live in `tests/` rather than in the module's own `#[cfg(test)]`
//! block so they can be named as a gate on their own target. A lib-level test
//! is only reachable through `-p gdscene --lib`, which drags every unrelated
//! failure in that crate into the verdict.

use gdcore::math::Vector2;
use gdscene::curve2d_query::closest_offset;
use gdscene::path3d::Curve2D;

/// A straight 4-unit curve sampled at unit spacing, so arc length and x agree
/// and every expected offset is readable by eye.
fn straight() -> Curve2D {
    let mut c = Curve2D::default();
    for x in 0..=4 {
        c.add_point(Vector2::new(x as f32, 0.0));
    }
    c
}

/// 🔑 A point beside the curve answers with the offset of its foot.
#[test]
fn curve2d_closest_offset_projects_onto_the_segment() {
    // Directly above x = 2.5 — INSIDE a segment, not on one of the points.
    let got = closest_offset(&straight(), Vector2::new(2.5, 7.0));
    assert!(
        (got - 2.5).abs() < 1e-4,
        "the offset must be the projection's arc length, 2.5; got {got}. A \
         sampled search would answer 2.0 or 3.0 — whichever point happened to \
         be nearest — which is the distinction this method exists to make"
    );
}

/// 🔑 The endpoints clamp instead of running off the curve.
#[test]
fn curve2d_closest_offset_clamps_past_the_ends() {
    let c = straight();
    assert!(
        closest_offset(&c, Vector2::new(-10.0, 0.0)).abs() < 1e-4,
        "a point before the start clamps to offset 0"
    );
    let end = closest_offset(&c, Vector2::new(99.0, 0.0));
    assert!(
        (end - 4.0).abs() < 1e-4,
        "a point past the end clamps to the baked length, 4.0; got {end}"
    );
}

/// ⚠️ A duplicated point must not produce NaN.
///
/// Godot divides by the segment interval without guarding, so a zero-length
/// segment yields `0/0`. The NaN then propagates silently into every position
/// the caller derives from the offset — and coincident control points are
/// ordinary authoring, not a pathological fixture.
#[test]
fn curve2d_closest_offset_skips_a_degenerate_segment() {
    let mut c = Curve2D::default();
    c.add_point(Vector2::new(0.0, 0.0));
    c.add_point(Vector2::new(1.0, 0.0));
    c.add_point(Vector2::new(1.0, 0.0)); // coincident with the previous point
    c.add_point(Vector2::new(2.0, 0.0));

    let got = closest_offset(&c, Vector2::new(1.5, 3.0));
    assert!(
        got.is_finite(),
        "a duplicated point must not divide by zero; got {got}"
    );
    assert!(
        (got - 1.5).abs() < 1e-4,
        "the duplicate contributes no arc length, so the offset is still 1.5; \
         got {got}"
    );
}

/// A curve with nothing to project onto answers 0 rather than panicking on an
/// empty range.
#[test]
fn curve2d_closest_offset_handles_under_two_points() {
    assert_eq!(
        closest_offset(&Curve2D::default(), Vector2::new(5.0, 5.0)),
        0.0
    );

    let mut one = Curve2D::default();
    one.add_point(Vector2::new(3.0, 3.0));
    assert_eq!(
        closest_offset(&one, Vector2::new(5.0, 5.0)),
        0.0,
        "a single point is offset 0 by definition — there is no segment"
    );
}
