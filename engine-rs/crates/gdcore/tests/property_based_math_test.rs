//! Property-based tests for math type operations using proptest.
//!
//! Verifies algebraic invariants (commutativity, associativity, identity,
//! inverse, etc.) for Vector2, Vector3, Quaternion, AABB, and Rect2 types.

use proptest::prelude::*;

use gdcore::math::{Rect2, Vector2, Vector3};
use gdcore::math3d::{Aabb, Quaternion};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Finite f32 values in a reasonable range (avoids overflow/NaN in arithmetic).
fn finite_f32() -> impl Strategy<Value = f32> {
    -1e4f32..1e4f32
}

/// Small f32 for angles (avoid extreme values).
fn angle_f32() -> impl Strategy<Value = f32> {
    -std::f32::consts::PI..std::f32::consts::PI
}

fn arb_vector2() -> impl Strategy<Value = Vector2> {
    (finite_f32(), finite_f32()).prop_map(|(x, y)| Vector2::new(x, y))
}

fn arb_vector3() -> impl Strategy<Value = Vector3> {
    (finite_f32(), finite_f32(), finite_f32()).prop_map(|(x, y, z)| Vector3::new(x, y, z))
}

/// Non-zero vector2 for normalization tests.
fn nonzero_vector2() -> impl Strategy<Value = Vector2> {
    arb_vector2().prop_filter("non-zero", |v| v.length_squared() > 1e-6)
}

/// Non-zero vector3 for normalization tests.
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

fn arb_euler() -> impl Strategy<Value = Vector3> {
    (angle_f32(), angle_f32(), angle_f32()).prop_map(|(x, y, z)| Vector3::new(x, y, z))
}

fn arb_rect2() -> impl Strategy<Value = Rect2> {
    (arb_vector2(), (0.01f32..1e3, 0.01f32..1e3))
        .prop_map(|(pos, (w, h))| Rect2::new(pos, Vector2::new(w, h)))
}

fn arb_aabb() -> impl Strategy<Value = Aabb> {
    (arb_vector3(), (0.01f32..1e3, 0.01f32..1e3, 0.01f32..1e3))
        .prop_map(|(pos, (sx, sy, sz))| Aabb::new(pos, Vector3::new(sx, sy, sz)))
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
// Vector2 algebraic properties
// ---------------------------------------------------------------------------

proptest! {
    // 1. Addition commutativity
    #[test]
    fn vector2_add_commutative(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(v2_approx_eq(a + b, b + a, 1e-5));
    }

    // 2. Addition associativity
    #[test]
    fn vector2_add_associative(
        a in arb_vector2(),
        b in arb_vector2(),
        c in arb_vector2()
    ) {
        let lhs = (a + b) + c;
        let rhs = a + (b + c);
        let scale = lhs.length().max(rhs.length()).max(1.0);
        prop_assert!(v2_approx_eq(lhs, rhs, scale * 1e-4),
            "associativity: {:?} vs {:?}", lhs, rhs);
    }

    // 3. Additive identity (ZERO)
    #[test]
    fn vector2_add_identity(a in arb_vector2()) {
        prop_assert!(v2_approx_eq(a + Vector2::ZERO, a, 1e-10));
    }

    // 4. Self-subtraction = zero
    #[test]
    fn vector2_sub_self_is_zero(a in arb_vector2()) {
        prop_assert!(v2_approx_eq(a - a, Vector2::ZERO, 1e-5));
    }

    // 5. Multiplicative identity (ONE for scale)
    #[test]
    fn vector2_scale_by_one_is_identity(a in arb_vector2()) {
        prop_assert!(v2_approx_eq(a * 1.0, a, 1e-10));
    }

    // 6. Dot product commutativity
    #[test]
    fn vector2_dot_commutative(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(approx_eq(a.dot(b), b.dot(a), 1e-2));
    }

    // 7. Normalize yields unit length for non-zero vectors
    #[test]
    fn vector2_normalized_is_unit(v in nonzero_vector2()) {
        let n = v.normalized();
        prop_assert!(approx_eq(n.length(), 1.0, 1e-4),
            "normalized length: {} for {:?}", n.length(), v);
    }

    // 8. Length is non-negative
    #[test]
    fn vector2_length_non_negative(a in arb_vector2()) {
        prop_assert!(a.length() >= 0.0);
    }

    // 9. Distance is symmetric
    #[test]
    fn vector2_distance_symmetric(a in arb_vector2(), b in arb_vector2()) {
        prop_assert!(approx_eq(a.distance_to(b), b.distance_to(a), 1e-3));
    }

    // 10. Distance to self is zero
    #[test]
    fn vector2_distance_to_self_is_zero(a in arb_vector2()) {
        prop_assert!(approx_eq(a.distance_to(a), 0.0, 1e-5));
    }
}

// ---------------------------------------------------------------------------
// Vector3 algebraic properties
// ---------------------------------------------------------------------------

proptest! {
    // 11. Addition commutativity
    #[test]
    fn vector3_add_commutative(a in arb_vector3(), b in arb_vector3()) {
        prop_assert!(v3_approx_eq(a + b, b + a, 1e-5));
    }

    // 12. Addition associativity
    #[test]
    fn vector3_add_associative(
        a in arb_vector3(),
        b in arb_vector3(),
        c in arb_vector3()
    ) {
        let lhs = (a + b) + c;
        let rhs = a + (b + c);
        let scale = lhs.length().max(rhs.length()).max(1.0);
        prop_assert!(v3_approx_eq(lhs, rhs, scale * 1e-4),
            "associativity: {:?} vs {:?}", lhs, rhs);
    }

    // 13. Additive identity (ZERO)
    #[test]
    fn vector3_add_identity(a in arb_vector3()) {
        prop_assert!(v3_approx_eq(a + Vector3::ZERO, a, 1e-10));
    }

    // 14. Self-subtraction = zero
    #[test]
    fn vector3_sub_self_is_zero(a in arb_vector3()) {
        prop_assert!(v3_approx_eq(a - a, Vector3::ZERO, 1e-5));
    }

    // 15. Multiplicative identity (ONE for scale)
    #[test]
    fn vector3_scale_by_one_is_identity(a in arb_vector3()) {
        prop_assert!(v3_approx_eq(a * 1.0, a, 1e-10));
    }

    // 16. Dot product commutativity
    #[test]
    fn vector3_dot_commutative(a in arb_vector3(), b in arb_vector3()) {
        prop_assert!(approx_eq(a.dot(b), b.dot(a), 1e-2));
    }

    // 17. Normalize yields unit length for non-zero vectors
    #[test]
    fn vector3_normalized_is_unit(v in nonzero_vector3()) {
        let n = v.normalized();
        prop_assert!(approx_eq(n.length(), 1.0, 1e-4),
            "normalized length: {} for {:?}", n.length(), v);
    }

    // 18. Cross product anticommutativity
    #[test]
    fn vector3_cross_anticommutative(a in arb_vector3(), b in arb_vector3()) {
        let ab = a.cross(b);
        let ba = b.cross(a);
        prop_assert!(v3_approx_eq(ab, -ba, 1e-2),
            "cross anticommutativity: {:?} vs {:?}", ab, -ba);
    }

    // 19. Cross product of self is zero
    #[test]
    fn vector3_cross_self_is_zero(a in arb_vector3()) {
        let c = a.cross(a);
        prop_assert!(v3_approx_eq(c, Vector3::ZERO, 1e-3));
    }

    // 20. Cross product is orthogonal to both inputs
    #[test]
    fn vector3_cross_orthogonal(a in nonzero_vector3(), b in nonzero_vector3()) {
        let c = a.cross(b);
        if c.length_squared() > 1e-6 {
            let scale = c.length() * a.length();
            prop_assert!(approx_eq(c.dot(a), 0.0, scale * 1e-4),
                "cross not orthogonal to a: dot={}", c.dot(a));
            let scale_b = c.length() * b.length();
            prop_assert!(approx_eq(c.dot(b), 0.0, scale_b * 1e-4),
                "cross not orthogonal to b: dot={}", c.dot(b));
        }
    }

    // 21. Length is non-negative
    #[test]
    fn vector3_length_non_negative(a in arb_vector3()) {
        prop_assert!(a.length() >= 0.0);
    }

    // 22. Distance is symmetric
    #[test]
    fn vector3_distance_symmetric(a in arb_vector3(), b in arb_vector3()) {
        prop_assert!(approx_eq(a.distance_to(b), b.distance_to(a), 1e-3));
    }

    // 23. Distance to self is zero
    #[test]
    fn vector3_distance_to_self_is_zero(a in arb_vector3()) {
        prop_assert!(approx_eq(a.distance_to(a), 0.0, 1e-5));
    }

    // 24. Scalar distribution over addition
    #[test]
    fn vector3_scalar_mul_distributes(a in arb_vector3(), b in arb_vector3(), s in finite_f32()) {
        let lhs = (a + b) * s;
        let rhs = a * s + b * s;
        let scale = lhs.length().max(rhs.length()).max(1.0);
        prop_assert!(v3_approx_eq(lhs, rhs, scale * 1e-4),
            "distributive: ({:?} + {:?}) * {} = {:?} vs {:?}", a, b, s, lhs, rhs);
    }
}

// ---------------------------------------------------------------------------
// Quaternion properties
// ---------------------------------------------------------------------------

proptest! {
    // 25. Identity quaternion preserves vectors
    #[test]
    fn quaternion_identity_preserves_vector(v in arb_vector3()) {
        let result = Quaternion::IDENTITY.xform(v);
        prop_assert!(v3_approx_eq(result, v, 1e-4));
    }

    // 26. from_euler produces unit quaternion
    #[test]
    fn quaternion_from_euler_is_unit(euler in arb_euler()) {
        let q = Quaternion::from_euler(euler);
        prop_assert!(approx_eq(q.length(), 1.0, 1e-4),
            "from_euler produced non-unit quaternion: len={} for euler={:?}", q.length(), euler);
    }

    // 27. Rotation preserves vector length
    #[test]
    fn quaternion_rotation_preserves_length(q in arb_unit_quaternion(), v in nonzero_vector3()) {
        let rotated = q.xform(v);
        prop_assert!(approx_eq(v.length(), rotated.length(), 1e-2),
            "rotation changed length: {} -> {}", v.length(), rotated.length());
    }

    // 28. Inverse roundtrip: q^-1 * q * v = v
    #[test]
    fn quaternion_inverse_roundtrip(q in arb_unit_quaternion(), v in arb_vector3()) {
        let rotated = q.xform(v);
        let restored = q.inverse().xform(rotated);
        let mag = v.x.abs().max(v.y.abs()).max(v.z.abs()).max(1.0);
        let eps = mag * 1e-4;
        prop_assert!(v3_approx_eq(restored, v, eps),
            "q*q^-1 roundtrip: {:?} -> {:?} -> {:?}", v, rotated, restored);
    }

    // 29. Multiplication associativity
    #[test]
    fn quaternion_mul_associative(
        a in arb_unit_quaternion(),
        b in arb_unit_quaternion(),
        c in arb_unit_quaternion()
    ) {
        let lhs = (a * b) * c;
        let rhs = a * (b * c);
        prop_assert!(approx_eq(lhs.x, rhs.x, 1e-3), "x: {} vs {}", lhs.x, rhs.x);
        prop_assert!(approx_eq(lhs.y, rhs.y, 1e-3), "y: {} vs {}", lhs.y, rhs.y);
        prop_assert!(approx_eq(lhs.z, rhs.z, 1e-3), "z: {} vs {}", lhs.z, rhs.z);
        prop_assert!(approx_eq(lhs.w, rhs.w, 1e-3), "w: {} vs {}", lhs.w, rhs.w);
    }

    // 30. Multiplication by identity is identity
    #[test]
    fn quaternion_mul_identity(q in arb_unit_quaternion()) {
        let result = q * Quaternion::IDENTITY;
        prop_assert!(approx_eq(q.x, result.x, 1e-5));
        prop_assert!(approx_eq(q.y, result.y, 1e-5));
        prop_assert!(approx_eq(q.z, result.z, 1e-5));
        prop_assert!(approx_eq(q.w, result.w, 1e-5));
    }

    // 31. q * q^-1 = identity
    #[test]
    fn quaternion_mul_inverse_is_identity(q in arb_unit_quaternion()) {
        let result = q * q.inverse();
        prop_assert!(approx_eq(result.x, 0.0, 1e-4));
        prop_assert!(approx_eq(result.y, 0.0, 1e-4));
        prop_assert!(approx_eq(result.z, 0.0, 1e-4));
        prop_assert!(approx_eq(result.w, 1.0, 1e-4));
    }

    // 32. Normalized quaternion has unit length
    #[test]
    fn quaternion_normalized_is_unit(q in arb_unit_quaternion()) {
        prop_assert!(approx_eq(q.length(), 1.0, 1e-4),
            "quaternion length: {}", q.length());
    }

    // 33. from_axis_angle produces unit quaternion
    #[test]
    fn quaternion_from_axis_angle_is_unit(
        axis in nonzero_vector3(),
        angle in angle_f32()
    ) {
        let q = Quaternion::from_axis_angle(axis.normalized(), angle);
        prop_assert!(approx_eq(q.length(), 1.0, 1e-4),
            "from_axis_angle non-unit: {}", q.length());
    }

    // 34. Slerp at endpoints
    #[test]
    fn quaternion_slerp_endpoints(a in arb_unit_quaternion(), b in arb_unit_quaternion()) {
        let at0 = a.slerp(b, 0.0);
        prop_assert!(approx_eq(at0.dot(a).abs(), 1.0, 1e-3),
            "slerp(a,b,0) != a: dot = {}", at0.dot(a));
        let at1 = a.slerp(b, 1.0);
        prop_assert!(approx_eq(at1.dot(b).abs(), 1.0, 1e-3),
            "slerp(a,b,1) != b: dot = {}", at1.dot(b));
    }
}

// ---------------------------------------------------------------------------
// AABB properties
// ---------------------------------------------------------------------------

proptest! {
    // 35. AABB contains its own center
    #[test]
    fn aabb_contains_center(aabb in arb_aabb()) {
        let center = aabb.get_center();
        prop_assert!(aabb.contains_point(center),
            "AABB {:?} does not contain its center {:?}", aabb, center);
    }

    // 36. AABB volume is non-negative (positive-size AABBs)
    #[test]
    fn aabb_volume_non_negative(aabb in arb_aabb()) {
        prop_assert!(aabb.get_volume() >= 0.0,
            "AABB volume is negative: {}", aabb.get_volume());
    }

    // 37. AABB with positive size has volume
    #[test]
    fn aabb_positive_size_has_volume(aabb in arb_aabb()) {
        prop_assert!(aabb.has_volume());
    }

    // 38. AABB intersects itself
    #[test]
    fn aabb_intersects_self(aabb in arb_aabb()) {
        prop_assert!(aabb.intersects(aabb));
    }

    // 39. AABB merge contains both originals' centers
    #[test]
    fn aabb_merge_contains_both_centers(a in arb_aabb(), b in arb_aabb()) {
        let merged = a.merge(b);
        prop_assert!(merged.contains_point(a.get_center()),
            "merged AABB does not contain center of a");
        prop_assert!(merged.contains_point(b.get_center()),
            "merged AABB does not contain center of b");
    }
}

// ---------------------------------------------------------------------------
// Rect2 properties
// ---------------------------------------------------------------------------

proptest! {
    // 40. Rect2 area is non-negative (for positive-size rects)
    #[test]
    fn rect2_area_non_negative(r in arb_rect2()) {
        prop_assert!(r.area() >= 0.0);
    }

    // 41. Rect2 contains its own position
    #[test]
    fn rect2_contains_own_position(r in arb_rect2()) {
        prop_assert!(r.contains_point(r.position),
            "rect {:?} doesn't contain its own position", r);
    }

    // 42. Rect2 intersects itself
    #[test]
    fn rect2_intersects_self(r in arb_rect2()) {
        prop_assert!(r.intersects(r));
    }

    // 43. Rect2 end = position + size
    #[test]
    fn rect2_end_equals_position_plus_size(r in arb_rect2()) {
        let end = r.end();
        prop_assert!(v2_approx_eq(end, r.position + r.size, 1e-5));
    }
}
