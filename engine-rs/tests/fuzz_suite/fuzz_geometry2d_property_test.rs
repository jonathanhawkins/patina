//! pat-03sbm: Proptest-based fuzz coverage for geometry2d primitives.
//!
//! Asserts algebraic invariants on Rect2, Vector2, and Transform2D under
//! randomly generated inputs. Uses bounded ranges to keep arithmetic in a
//! numerically stable region.

use gdcore::math::{Rect2, Transform2D, Vector2};
use proptest::prelude::*;

const COORD_RANGE: f32 = 1_000.0;
const NONZERO_LO: f32 = 0.01;
const EPS: f32 = 1e-3;
const ROT_EPS: f32 = 1e-2;

fn vec2() -> impl Strategy<Value = Vector2> {
    (-COORD_RANGE..COORD_RANGE, -COORD_RANGE..COORD_RANGE).prop_map(|(x, y)| Vector2::new(x, y))
}

fn nonzero_vec2() -> impl Strategy<Value = Vector2> {
    vec2().prop_filter("length must be non-trivial", |v| v.length() > NONZERO_LO)
}

fn pos_vec2() -> impl Strategy<Value = Vector2> {
    (NONZERO_LO..COORD_RANGE, NONZERO_LO..COORD_RANGE).prop_map(|(x, y)| Vector2::new(x, y))
}

fn rect2() -> impl Strategy<Value = Rect2> {
    (vec2(), pos_vec2()).prop_map(|(p, s)| Rect2::new(p, s))
}

fn invertible_basis() -> impl Strategy<Value = (Vector2, Vector2)> {
    (nonzero_vec2(), nonzero_vec2()).prop_filter("basis must be invertible", |(x, y)| {
        let det = x.x * y.y - x.y * y.x;
        det.abs() > NONZERO_LO
    })
}

fn invertible_transform() -> impl Strategy<Value = Transform2D> {
    (invertible_basis(), vec2()).prop_map(|((x, y), origin)| Transform2D { x, y, origin })
}

fn approx(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps + eps * a.abs().max(b.abs())
}

fn vec_approx(a: Vector2, b: Vector2, eps: f32) -> bool {
    approx(a.x, b.x, eps) && approx(a.y, b.y, eps)
}

// ---------------------------------------------------------------------------
// Vector2 properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn vector2_normalized_has_unit_length(v in nonzero_vec2()) {
        let n = v.normalized();
        prop_assert!((n.length() - 1.0).abs() < EPS, "len = {}", n.length());
    }

    #[test]
    fn vector2_length_squared_matches_length(v in vec2()) {
        let l = v.length();
        prop_assert!(approx(l * l, v.length_squared(), EPS));
    }

    #[test]
    fn vector2_dot_is_commutative(a in vec2(), b in vec2()) {
        prop_assert!(approx(a.dot(b), b.dot(a), EPS));
    }

    #[test]
    fn vector2_cross_is_antisymmetric(a in vec2(), b in vec2()) {
        prop_assert!(approx(a.cross(b), -b.cross(a), EPS));
    }

    #[test]
    fn vector2_lerp_endpoints(a in vec2(), b in vec2()) {
        prop_assert!(vec_approx(a.lerp(b, 0.0), a, EPS));
        prop_assert!(vec_approx(a.lerp(b, 1.0), b, EPS));
    }

    #[test]
    fn vector2_abs_components_nonnegative(v in vec2()) {
        let a = v.abs();
        prop_assert!(a.x >= 0.0 && a.y >= 0.0);
    }

    #[test]
    fn vector2_distance_to_is_symmetric(a in vec2(), b in vec2()) {
        prop_assert!(approx(a.distance_to(b), b.distance_to(a), EPS));
    }

    #[test]
    fn vector2_cauchy_schwarz(a in vec2(), b in vec2()) {
        // |a · b| <= |a| * |b|
        let lhs = a.dot(b).abs();
        let rhs = a.length() * b.length();
        prop_assert!(lhs <= rhs + EPS);
    }
}

// ---------------------------------------------------------------------------
// Rect2 properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn rect2_self_intersects(r in rect2()) {
        prop_assert!(r.intersects(r));
    }

    #[test]
    fn rect2_intersects_is_symmetric(a in rect2(), b in rect2()) {
        prop_assert_eq!(a.intersects(b), b.intersects(a));
    }

    #[test]
    fn rect2_contains_position_implies_inside(r in rect2()) {
        // The position corner is inclusive in this engine (>= position, < end)
        // so a strict-inside check uses (position + small offset).
        let inside = r.position + Vector2::new(r.size.x * 0.5, r.size.y * 0.5);
        prop_assert!(r.contains_point(inside));
    }

    #[test]
    fn rect2_does_not_contain_far_point(r in rect2()) {
        let far = r.end() + Vector2::new(r.size.x + 1.0, r.size.y + 1.0);
        prop_assert!(!r.contains_point(far));
    }

    #[test]
    fn rect2_area_matches_size_product(r in rect2()) {
        prop_assert!(approx(r.area(), r.size.x * r.size.y, EPS));
    }

    #[test]
    fn rect2_end_equals_position_plus_size(r in rect2()) {
        prop_assert!(vec_approx(r.end(), r.position + r.size, EPS));
    }

    #[test]
    fn rect2_disjoint_rects_do_not_intersect(p in vec2(), s in pos_vec2()) {
        let a = Rect2::new(p, s);
        // Place b strictly to the right of a with a gap.
        let b = Rect2::new(p + Vector2::new(s.x + 10.0, 0.0), s);
        prop_assert!(!a.intersects(b));
    }
}

// ---------------------------------------------------------------------------
// Transform2D properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn transform2d_identity_is_no_op(p in vec2()) {
        prop_assert!(vec_approx(Transform2D::IDENTITY.xform(p), p, EPS));
    }

    #[test]
    fn transform2d_xform_includes_origin(o in vec2()) {
        let t = Transform2D::translated(o);
        prop_assert!(vec_approx(t.xform(Vector2::ZERO), o, EPS));
    }

    #[test]
    fn transform2d_compose_is_associative_on_points(
        t1 in invertible_transform(),
        t2 in invertible_transform(),
        p in vec2(),
    ) {
        let lhs = (t1 * t2).xform(p);
        let rhs = t1.xform(t2.xform(p));
        prop_assert!(vec_approx(lhs, rhs, ROT_EPS));
    }

    #[test]
    fn transform2d_inverse_round_trip_point(t in invertible_transform(), p in vec2()) {
        let inv = t.affine_inverse();
        let round = inv.xform(t.xform(p));
        prop_assert!(vec_approx(round, p, ROT_EPS));
    }

    #[test]
    fn transform2d_inverse_compose_is_identity_on_origin(t in invertible_transform()) {
        let composed = t.affine_inverse() * t;
        // Composed identity should map ZERO to ZERO.
        let mapped_zero = composed.xform(Vector2::ZERO);
        prop_assert!(vec_approx(mapped_zero, Vector2::ZERO, ROT_EPS));
    }

    #[test]
    fn transform2d_basis_xform_is_linear(
        t in invertible_transform(),
        a in vec2(),
        b in vec2(),
    ) {
        let lhs = t.basis_xform(a + b);
        let rhs = t.basis_xform(a) + t.basis_xform(b);
        prop_assert!(vec_approx(lhs, rhs, ROT_EPS));
    }

    #[test]
    fn transform2d_rotated_preserves_length(angle in -std::f32::consts::TAU..std::f32::consts::TAU, v in nonzero_vec2()) {
        let rot = Transform2D::rotated(angle);
        let rotated = rot.basis_xform(v);
        prop_assert!(approx(rotated.length(), v.length(), ROT_EPS));
    }

    #[test]
    fn transform2d_scaled_scales_basis(s in pos_vec2(), p in vec2()) {
        let t = Transform2D::scaled(s);
        let mapped = t.xform(p);
        prop_assert!(approx(mapped.x, p.x * s.x, EPS));
        prop_assert!(approx(mapped.y, p.y * s.y, EPS));
    }
}
