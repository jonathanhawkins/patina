//! Property-based tests for math type operations using proptest.
//!
//! Verifies algebraic invariants (commutativity, associativity, identity,
//! inverse, etc.) for Vector2, Vector3, Quaternion, Basis, Transform2D,
//! Transform3D, Rect2, and Color types.

use proptest::prelude::*;

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector2i, Vector3};
use gdcore::math3d::{Basis, Quaternion, Transform3D};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Finite f32 values in a reasonable range (avoids overflow/NaN in arithmetic).
fn finite_f32() -> impl Strategy<Value = f32> {
    (-1e4f32..1e4f32)
}

/// Small f32 for angles (avoid extreme values).
fn angle_f32() -> impl Strategy<Value = f32> {
    (-std::f32::consts::PI..std::f32::consts::PI)
}

fn arb_vector2() -> impl Strategy<Value = Vector2> {
    (finite_f32(), finite_f32()).prop_map(|(x, y)| Vector2::new(x, y))
}

fn arb_vector2i() -> impl Strategy<Value = Vector2i> {
    (-10000i32..10000i32, -10000i32..10000i32)
        .prop_map(|(x, y)| Vector2i::new(x, y))
}

fn arb_vector3() -> impl Strategy<Value = Vector3> {
    (finite_f32(), finite_f32(), finite_f32())
        .prop_map(|(x, y, z)| Vector3::new(x, y, z))
}

/// Non-zero vector for normalization tests.
fn nonzero_vector3() -> impl Strategy<Value = Vector3> {
    arb_vector3().prop_filter("non-zero", |v| v.length_squared() > 1e-6)
}

fn arb_unit_quaternion() -> impl Strategy<Value = Quaternion> {
    (finite_f32(), finite_f32(), finite_f32(), finite_f32())
        .prop_filter("non-zero", |(x, y, z, w)| {
            x * x + y * y + z * z + w * w > 1e-6
        })
        .prop_map(|(x, y, z, w)| Quaternion::new(x, y, z, w).normalized())
}

fn arb_color() -> impl Strategy<Value = Color> {
    (0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0)
        .prop_map(|(r, g, b, a)| Color::new(r, g, b, a))
}

fn arb_rect2() -> impl Strategy<Value = Rect2> {
    (arb_vector2(), (0.01f32..1e3, 0.01f32..1e3))
        .prop_map(|(pos, (w, h))| Rect2::new(pos, Vector2::new(w, h)))
}

/// Approximate equality for f32.
fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() < eps
}

/// Approximate equality for Vector2.
fn v2_approx_eq(a: Vector2, b: Vector2, eps: f32) -> bool {
    approx_eq(a.x, b.x, eps) && approx_eq(a.y, b.y, eps)
}

/// Approximate equality for Vector3.
fn v3_approx_eq(a: Vector3, b: Vector3, eps: f32) -> bool {
    approx_eq(a.x, b.x, eps) && approx_eq(a.y, b.y, eps) && approx_eq(a.z, b.z, eps)
}

// ---------------------------------------------------------------------------
// Vector2i properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn vector2i_add_commutative(a in arb_vector2i(), b in arb_vector2i()) {
        prop_assert_eq!(a + b, b + a);
    }

    #[test]
    fn vector2i_add_identity(a in arb_vector2i()) {
        prop_assert_eq!(a + Vector2i::ZERO, a);
    }

    #[test]
    fn vector2i_sub_inverse(a in arb_vector2i()) {
        prop_assert_eq!(a - a, Vector2i::ZERO);
    }

    #[test]
    fn vector2i_neg_double(a in arb_vector2i()) {
        prop_assert_eq!(-(-a), a);
    }
}

// ---------------------------------------------------------------------------
// Vector2 properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn vector2_add_commutative(a in arb_vector2(), b in arb_vector2()) {
        let sum_ab = a + b;
        let sum_ba = b + a;
        prop_assert!(v2_approx_eq(sum_ab, sum_ba, 1e-5));
    }

    #[test]
    fn vector2_add_identity(a in arb_vector2()) {
        let result = a + Vector2::ZERO;
        prop_assert!(v2_approx_eq(result, a, 1e-10));
    }

    #[test]
    fn vector2_sub_self_is_zero(a in arb_vector2()) {
        let result = a - a;
        prop_assert!(v2_approx_eq(result, Vector2::ZERO, 1e-5));
    }

    #[test]
    fn vector2_neg_double(a in arb_vector2()) {
        let result = -(-a);
        prop_assert!(v2_approx_eq(result, a, 1e-10));
    }

    #[test]
    fn vector2_scalar_mul_distributes(a in arb_vector2(), b in arb_vector2(), s in finite_f32()) {
        let lhs = (a + b) * s;
        let rhs = a * s + b * s;
        // Use relative tolerance: large values have more float rounding
        let scale = lhs.length().max(rhs.length()).max(1.0);
        prop_assert!(v2_approx_eq(lhs, rhs, scale * 1e-4),
            "distributive: ({:?} + {:?}) * {} = {:?} vs {:?}", a, b, s, lhs, rhs);
    }

    #[test]
    fn vector2_length_non_negative(a in arb_vector2()) {
        prop_assert!(a.length() >= 0.0);
    }

    #[test]
    fn vector2_length_squared_is_length_sq(a in arb_vector2()) {
        let len = a.length();
        let rel_eps = 1e-4 * a.length_squared().max(1.0);
        prop_assert!(approx_eq(a.length_squared(), len * len, rel_eps));
    }

    #[test]
    fn vector2_normalized_is_unit_or_zero(v in arb_vector2()) {
        let n = v.normalized();
        if v.length() < 1e-10 {
            prop_assert!(v2_approx_eq(n, Vector2::ZERO, 1e-5));
        } else {
            prop_assert!(approx_eq(n.length(), 1.0, 1e-4),
                "normalized length: {} for {:?}", n.length(), v);
        }
    }

    #[test]
    fn vector2_dot_commutative(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(approx_eq(a.dot(b), b.dot(a), 1e-2));
    }

    #[test]
    fn vector2_lerp_endpoints(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(v2_approx_eq(a.lerp(b, 0.0), a, 1e-5));
        prop_assert!(v2_approx_eq(a.lerp(b, 1.0), b, 1e-2));
    }

    #[test]
    fn vector2_distance_non_negative(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(a.distance_to(b) >= 0.0);
    }

    #[test]
    fn vector2_distance_symmetric(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(approx_eq(a.distance_to(b), b.distance_to(a), 1e-3));
    }
}

// ---------------------------------------------------------------------------
// Vector3 properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn vector3_add_commutative(a in arb_vector3(), b in arb_vector3()) {
        let r = a + b;
        let s = b + a;
        prop_assert!(v3_approx_eq(r, s, 1e-5));
    }

    #[test]
    fn vector3_sub_self_is_zero(a in arb_vector3()) {
        prop_assert!(v3_approx_eq(a - a, Vector3::ZERO, 1e-5));
    }

    #[test]
    fn vector3_cross_anticommutative(a in arb_vector3(), b in arb_vector3()) {
        let ab = a.cross(b);
        let ba = b.cross(a);
        prop_assert!(v3_approx_eq(ab, -ba, 1e-2),
            "cross anticommutativity: {:?} vs {:?}", ab, -ba);
    }

    #[test]
    fn vector3_cross_self_is_zero(a in arb_vector3()) {
        let c = a.cross(a);
        prop_assert!(v3_approx_eq(c, Vector3::ZERO, 1e-3));
    }

    #[test]
    fn vector3_dot_commutative(a in arb_vector3(), b in arb_vector3()) {
        prop_assert!(approx_eq(a.dot(b), b.dot(a), 1e-2));
    }

    #[test]
    fn vector3_normalized_is_unit_or_zero(v in nonzero_vector3()) {
        let n = v.normalized();
        prop_assert!(approx_eq(n.length(), 1.0, 1e-4),
            "normalized length: {} for {:?}", n.length(), v);
    }

    #[test]
    fn vector3_lerp_endpoints(a in arb_vector3(), b in arb_vector3()) {
        prop_assert!(v3_approx_eq(a.lerp(b, 0.0), a, 1e-5));
        prop_assert!(v3_approx_eq(a.lerp(b, 1.0), b, 1e-2));
    }
}

// ---------------------------------------------------------------------------
// Quaternion properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn quaternion_identity_is_noop(v in arb_vector3()) {
        let result = Quaternion::IDENTITY.xform(v);
        prop_assert!(v3_approx_eq(result, v, 1e-4));
    }

    #[test]
    fn quaternion_normalized_is_unit(q in arb_unit_quaternion()) {
        prop_assert!(approx_eq(q.length(), 1.0, 1e-4),
            "quaternion length: {}", q.length());
    }

    #[test]
    fn quaternion_inverse_roundtrip(q in arb_unit_quaternion(), v in arb_vector3()) {
        let rotated = q.xform(v);
        let restored = q.inverse().xform(rotated);
        prop_assert!(v3_approx_eq(restored, v, 1e-2),
            "q*q^-1 roundtrip: {:?} -> {:?} -> {:?}", v, rotated, restored);
    }

    #[test]
    fn quaternion_mul_identity(q in arb_unit_quaternion()) {
        let result = q * Quaternion::IDENTITY;
        prop_assert!(approx_eq(q.x, result.x, 1e-5));
        prop_assert!(approx_eq(q.y, result.y, 1e-5));
        prop_assert!(approx_eq(q.z, result.z, 1e-5));
        prop_assert!(approx_eq(q.w, result.w, 1e-5));
    }

    #[test]
    fn quaternion_rotation_preserves_length(q in arb_unit_quaternion(), v in nonzero_vector3()) {
        let rotated = q.xform(v);
        prop_assert!(approx_eq(v.length(), rotated.length(), 1e-2),
            "rotation changed length: {} -> {}", v.length(), rotated.length());
    }

    #[test]
    fn quaternion_slerp_endpoints(a in arb_unit_quaternion(), b in arb_unit_quaternion()) {
        let at0 = a.slerp(b, 0.0);
        // slerp(a, b, 0) should be approximately a
        prop_assert!(approx_eq(at0.dot(a).abs(), 1.0, 1e-3),
            "slerp(a,b,0) != a: dot = {}", at0.dot(a));
        let at1 = a.slerp(b, 1.0);
        // slerp(a, b, 1) should be approximately b (or -b, same rotation)
        prop_assert!(approx_eq(at1.dot(b).abs(), 1.0, 1e-3),
            "slerp(a,b,1) != b: dot = {}", at1.dot(b));
    }

    #[test]
    fn quaternion_from_axis_angle_roundtrip(
        axis in nonzero_vector3(),
        angle in angle_f32()
    ) {
        let norm_axis = axis.normalized();
        let q = Quaternion::from_axis_angle(norm_axis, angle);
        prop_assert!(approx_eq(q.length(), 1.0, 1e-4),
            "from_axis_angle produced non-unit quaternion: {}", q.length());
    }
}

// ---------------------------------------------------------------------------
// Basis properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn basis_identity_is_noop(v in arb_vector3()) {
        let result = Basis::IDENTITY.xform(v);
        prop_assert!(v3_approx_eq(result, v, 1e-5));
    }

    #[test]
    fn basis_transpose_double(
        euler in (angle_f32(), angle_f32(), angle_f32())
    ) {
        let b = Basis::from_euler(Vector3::new(euler.0, euler.1, euler.2));
        let tt = b.transposed().transposed();
        // Should be approximately b
        prop_assert!(v3_approx_eq(b.x, tt.x, 1e-4));
        prop_assert!(v3_approx_eq(b.y, tt.y, 1e-4));
        prop_assert!(v3_approx_eq(b.z, tt.z, 1e-4));
    }

    #[test]
    fn basis_from_quaternion_is_orthonormal(q in arb_unit_quaternion()) {
        let b = Basis::from_quaternion(q);
        prop_assert!(b.is_orthonormal(1e-3),
            "basis from quaternion is not orthonormal");
    }

    #[test]
    fn basis_inverse_roundtrip(
        euler in (angle_f32(), angle_f32(), angle_f32()),
        v in arb_vector3()
    ) {
        let b = Basis::from_euler(Vector3::new(euler.0, euler.1, euler.2));
        let transformed = b.xform(v);
        let restored = b.inverse().xform(transformed);
        prop_assert!(v3_approx_eq(restored, v, 1e-1),
            "basis inverse roundtrip: {:?} -> {:?} -> {:?}", v, transformed, restored);
    }

    #[test]
    fn basis_determinant_rotation_is_one(
        euler in (angle_f32(), angle_f32(), angle_f32())
    ) {
        let b = Basis::from_euler(Vector3::new(euler.0, euler.1, euler.2));
        prop_assert!(approx_eq(b.determinant(), 1.0, 1e-3),
            "rotation basis determinant: {}", b.determinant());
    }

    #[test]
    fn basis_orthonormalized_is_orthonormal(
        euler in (angle_f32(), angle_f32(), angle_f32())
    ) {
        let b = Basis::from_euler(Vector3::new(euler.0, euler.1, euler.2));
        let on = b.orthonormalized();
        prop_assert!(on.is_orthonormal(1e-3));
    }
}

// ---------------------------------------------------------------------------
// Transform2D properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn transform2d_identity_is_noop(p in arb_vector2()) {
        let result = Transform2D::IDENTITY.xform(p);
        prop_assert!(v2_approx_eq(result, p, 1e-5));
    }

    #[test]
    fn transform2d_inverse_roundtrip(
        angle in angle_f32(),
        offset in arb_vector2(),
        p in arb_vector2()
    ) {
        let t = Transform2D::rotated(angle) * Transform2D::translated(offset);
        let result = t.affine_inverse().xform(t.xform(p));
        prop_assert!(v2_approx_eq(result, p, 1e-1),
            "transform2d inverse roundtrip: {:?} -> {:?}", p, result);
    }

    #[test]
    fn transform2d_mul_identity(angle in angle_f32()) {
        let t = Transform2D::rotated(angle);
        let result = t * Transform2D::IDENTITY;
        prop_assert!(v2_approx_eq(result.x, t.x, 1e-5));
        prop_assert!(v2_approx_eq(result.y, t.y, 1e-5));
        prop_assert!(v2_approx_eq(result.origin, t.origin, 1e-5));
    }

    #[test]
    fn transform2d_translation_applies_offset(offset in arb_vector2(), p in arb_vector2()) {
        let t = Transform2D::translated(offset);
        let result = t.xform(p);
        prop_assert!(v2_approx_eq(result, p + offset, 1e-5));
    }

    #[test]
    fn transform2d_rotation_preserves_length(angle in angle_f32(), v in arb_vector2()) {
        let t = Transform2D::rotated(angle);
        let rotated = t.basis_xform(v);
        let eps = v.length().max(1.0) * 1e-4;
        prop_assert!(approx_eq(v.length(), rotated.length(), eps),
            "rotation changed length: {} -> {}", v.length(), rotated.length());
    }
}

// ---------------------------------------------------------------------------
// Transform3D properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn transform3d_identity_is_noop(v in arb_vector3()) {
        let result = Transform3D::IDENTITY.xform(v);
        prop_assert!(v3_approx_eq(result, v, 1e-5));
    }

    #[test]
    fn transform3d_inverse_roundtrip(
        euler in (angle_f32(), angle_f32(), angle_f32()),
        offset in arb_vector3(),
        v in arb_vector3()
    ) {
        let basis = Basis::from_euler(Vector3::new(euler.0, euler.1, euler.2));
        let t = Transform3D { basis, origin: offset };
        let result = t.inverse().xform(t.xform(v));
        prop_assert!(v3_approx_eq(result, v, 1e-1),
            "transform3d inverse roundtrip: {:?} -> {:?}", v, result);
    }
}

// ---------------------------------------------------------------------------
// Rect2 properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn rect2_contains_own_position(r in arb_rect2()) {
        // A rect with positive size should contain its own position
        prop_assert!(r.contains_point(r.position),
            "rect {:?} doesn't contain its own position", r);
    }

    #[test]
    fn rect2_area_non_negative(r in arb_rect2()) {
        prop_assert!(r.area() >= 0.0);
    }

    #[test]
    fn rect2_intersects_self(r in arb_rect2()) {
        prop_assert!(r.intersects(r));
    }

    #[test]
    fn rect2_end_equals_position_plus_size(r in arb_rect2()) {
        let end = r.end();
        prop_assert!(v2_approx_eq(end, r.position + r.size, 1e-5));
    }
}

// ---------------------------------------------------------------------------
// Color properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn color_lerp_endpoints(a in arb_color(), b in arb_color()) {
        let at0 = a.lerp(b, 0.0);
        prop_assert!(approx_eq(at0.r, a.r, 1e-5));
        prop_assert!(approx_eq(at0.g, a.g, 1e-5));
        prop_assert!(approx_eq(at0.b, a.b, 1e-5));
        prop_assert!(approx_eq(at0.a, a.a, 1e-5));

        let at1 = a.lerp(b, 1.0);
        prop_assert!(approx_eq(at1.r, b.r, 1e-5));
        prop_assert!(approx_eq(at1.g, b.g, 1e-5));
        prop_assert!(approx_eq(at1.b, b.b, 1e-5));
        prop_assert!(approx_eq(at1.a, b.a, 1e-5));
    }

    #[test]
    fn color_lerp_midpoint_is_average(a in arb_color(), b in arb_color()) {
        let mid = a.lerp(b, 0.5);
        prop_assert!(approx_eq(mid.r, (a.r + b.r) / 2.0, 1e-5));
        prop_assert!(approx_eq(mid.g, (a.g + b.g) / 2.0, 1e-5));
        prop_assert!(approx_eq(mid.b, (a.b + b.b) / 2.0, 1e-5));
        prop_assert!(approx_eq(mid.a, (a.a + b.a) / 2.0, 1e-5));
    }
}
