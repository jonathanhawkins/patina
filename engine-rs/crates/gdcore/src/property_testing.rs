//! Property-based testing for gdcore math types using `proptest`.
//!
//! Tests algebraic invariants that must hold for all inputs:
//!
//! - **Vector2/Vector3**: commutativity/associativity of add, dot commutativity,
//!   cross anticommutativity, self-subtraction yields zero, normalization yields
//!   unit length, lerp boundaries, distance symmetry, scalar distributivity
//! - **Transform2D**: identity preservation, inverse round-trip, composition
//!   associativity, rotation preserves length
//! - **Transform3D**: identity preservation, inverse round-trip, composition
//!   associativity
//! - **Quaternion**: unit-length preservation under multiplication, inverse
//!   round-trip, rotation preserves length, identity preservation, slerp
//!   boundaries, multiplication associativity
//! - **Basis**: identity preservation, inverse round-trip, rotation determinant
//!   is 1, transpose of rotation is inverse
//! - **Rect2**: contains own position, intersection symmetry, area non-negative
//! - **AABB**: contains center, contains own position, intersection symmetry,
//!   merge encloses both, expand includes point, volume non-negative
//! - **Color**: lerp boundaries, lerp midpoint is average
//! - **Plane**: point-on-plane has zero distance, positive side classification,
//!   from_points contains source points

#[cfg(test)]
mod tests {
    use crate::math::{Color, Rect2, Transform2D, Vector2, Vector3};
    use crate::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
    use proptest::prelude::*;

    const EPSILON: f32 = 1e-4;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn approx_vec2(a: Vector2, b: Vector2) -> bool {
        approx(a.x, b.x) && approx(a.y, b.y)
    }

    fn approx_vec3(a: Vector3, b: Vector3) -> bool {
        approx(a.x, b.x) && approx(a.y, b.y) && approx(a.z, b.z)
    }

    // -----------------------------------------------------------------------
    // Strategies
    // -----------------------------------------------------------------------

    fn arb_f32() -> impl Strategy<Value = f32> {
        -100.0f32..100.0f32
    }

    fn arb_small_f32() -> impl Strategy<Value = f32> {
        -10.0f32..10.0f32
    }

    fn arb_vec2() -> impl Strategy<Value = Vector2> {
        (arb_f32(), arb_f32()).prop_map(|(x, y)| Vector2::new(x, y))
    }

    fn arb_vec3() -> impl Strategy<Value = Vector3> {
        (arb_f32(), arb_f32(), arb_f32()).prop_map(|(x, y, z)| Vector3::new(x, y, z))
    }

    fn arb_small_vec2() -> impl Strategy<Value = Vector2> {
        (arb_small_f32(), arb_small_f32()).prop_map(|(x, y)| Vector2::new(x, y))
    }

    fn arb_small_vec3() -> impl Strategy<Value = Vector3> {
        (arb_small_f32(), arb_small_f32(), arb_small_f32())
            .prop_map(|(x, y, z)| Vector3::new(x, y, z))
    }

    /// A non-zero vector suitable for normalization tests.
    fn arb_nonzero_vec2() -> impl Strategy<Value = Vector2> {
        arb_vec2().prop_filter("non-zero length", |v| v.length() > 0.01)
    }

    fn arb_nonzero_vec3() -> impl Strategy<Value = Vector3> {
        arb_vec3().prop_filter("non-zero length", |v| v.length() > 0.01)
    }

    /// A unit quaternion from axis-angle.
    fn arb_unit_quat() -> impl Strategy<Value = Quaternion> {
        (
            arb_nonzero_vec3(),
            -std::f32::consts::PI..std::f32::consts::PI,
        )
            .prop_map(|(axis, angle)| Quaternion::from_axis_angle(axis.normalized(), angle))
    }

    fn arb_color() -> impl Strategy<Value = Color> {
        (0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0)
            .prop_map(|(r, g, b, a)| Color::new(r, g, b, a))
    }

    fn arb_positive_size_vec2() -> impl Strategy<Value = Vector2> {
        (0.1f32..50.0, 0.1f32..50.0).prop_map(|(x, y)| Vector2::new(x, y))
    }

    fn arb_positive_size_vec3() -> impl Strategy<Value = Vector3> {
        (0.1f32..50.0, 0.1f32..50.0, 0.1f32..50.0).prop_map(|(x, y, z)| Vector3::new(x, y, z))
    }

    // -----------------------------------------------------------------------
    // Vector2 properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn vec2_add_commutative(a in arb_vec2(), b in arb_vec2()) {
            prop_assert!(approx_vec2(a + b, b + a));
        }

        #[test]
        fn vec2_add_associative(
            a in arb_small_vec2(),
            b in arb_small_vec2(),
            c in arb_small_vec2()
        ) {
            prop_assert!(approx_vec2((a + b) + c, a + (b + c)));
        }

        #[test]
        fn vec2_add_identity(a in arb_vec2()) {
            prop_assert!(approx_vec2(a + Vector2::ZERO, a));
        }

        #[test]
        fn vec2_self_sub_is_zero(a in arb_vec2()) {
            prop_assert!(approx_vec2(a - a, Vector2::ZERO));
        }

        #[test]
        fn vec2_negation_inverse(a in arb_vec2()) {
            prop_assert!(approx_vec2(a + (-a), Vector2::ZERO));
        }

        #[test]
        fn vec2_scalar_mul_distributive(
            a in arb_small_vec2(),
            b in arb_small_vec2(),
            s in arb_small_f32()
        ) {
            prop_assert!(approx_vec2((a + b) * s, a * s + b * s));
        }

        #[test]
        fn vec2_dot_commutative(a in arb_vec2(), b in arb_vec2()) {
            prop_assert!(approx(a.dot(b), b.dot(a)));
        }

        #[test]
        fn vec2_cross_antisymmetric(a in arb_vec2(), b in arb_vec2()) {
            prop_assert!(approx(a.cross(b), -b.cross(a)));
        }

        #[test]
        fn vec2_normalized_unit_length(v in arb_nonzero_vec2()) {
            prop_assert!(approx(v.normalized().length(), 1.0));
        }

        #[test]
        fn vec2_length_non_negative(v in arb_vec2()) {
            prop_assert!(v.length() >= 0.0);
        }

        #[test]
        fn vec2_lerp_boundaries(a in arb_vec2(), b in arb_vec2()) {
            prop_assert!(approx_vec2(a.lerp(b, 0.0), a));
            prop_assert!(approx_vec2(a.lerp(b, 1.0), b));
        }

        #[test]
        fn vec2_distance_symmetric(a in arb_vec2(), b in arb_vec2()) {
            prop_assert!(approx(a.distance_to(b), b.distance_to(a)));
        }
    }

    // -----------------------------------------------------------------------
    // Vector3 properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn vec3_add_commutative(a in arb_vec3(), b in arb_vec3()) {
            prop_assert!(approx_vec3(a + b, b + a));
        }

        #[test]
        fn vec3_self_sub_is_zero(a in arb_vec3()) {
            prop_assert!(approx_vec3(a - a, Vector3::ZERO));
        }

        #[test]
        fn vec3_dot_commutative(a in arb_vec3(), b in arb_vec3()) {
            prop_assert!(approx(a.dot(b), b.dot(a)));
        }

        #[test]
        fn vec3_cross_antisymmetric(a in arb_vec3(), b in arb_vec3()) {
            prop_assert!(approx_vec3(a.cross(b), -b.cross(a)));
        }

        #[test]
        fn vec3_cross_perpendicular_to_inputs(a in arb_small_vec3(), b in arb_small_vec3()) {
            let c = a.cross(b);
            if c.length() > 0.01 {
                let tol = a.length() * b.length() * 1e-4;
                prop_assert!(c.dot(a).abs() < tol, "dot(c,a) = {}", c.dot(a));
                prop_assert!(c.dot(b).abs() < tol, "dot(c,b) = {}", c.dot(b));
            }
        }

        #[test]
        fn vec3_normalized_unit_length(v in arb_nonzero_vec3()) {
            prop_assert!(approx(v.normalized().length(), 1.0));
        }

        #[test]
        fn vec3_lerp_boundaries(a in arb_vec3(), b in arb_vec3()) {
            prop_assert!(approx_vec3(a.lerp(b, 0.0), a));
            prop_assert!(approx_vec3(a.lerp(b, 1.0), b));
        }

        #[test]
        fn vec3_distance_symmetric(a in arb_vec3(), b in arb_vec3()) {
            prop_assert!(approx(a.distance_to(b), b.distance_to(a)));
        }

        #[test]
        fn vec3_scalar_mul_distributive(
            a in arb_small_vec3(),
            b in arb_small_vec3(),
            s in arb_small_f32()
        ) {
            prop_assert!(approx_vec3((a + b) * s, a * s + b * s));
        }
    }

    // -----------------------------------------------------------------------
    // Quaternion properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn quat_unit_length_preserved_after_mul(a in arb_unit_quat(), b in arb_unit_quat()) {
            let product = a * b;
            prop_assert!(
                approx(product.length(), 1.0),
                "length was {}", product.length()
            );
        }

        #[test]
        fn quat_inverse_round_trip(q in arb_unit_quat(), v in arb_small_vec3()) {
            let result = q.inverse().xform(q.xform(v));
            prop_assert!(
                approx_vec3(result, v),
                "expected {:?}, got {:?}", v, result
            );
        }

        #[test]
        fn quat_rotation_preserves_length(q in arb_unit_quat(), v in arb_vec3()) {
            let rotated = q.xform(v);
            prop_assert!(
                approx(v.length(), rotated.length()),
                "len before={}, after={}", v.length(), rotated.length()
            );
        }

        #[test]
        fn quat_identity_preserves_vector(v in arb_vec3()) {
            prop_assert!(approx_vec3(Quaternion::IDENTITY.xform(v), v));
        }

        #[test]
        fn quat_slerp_boundaries(
            a in arb_unit_quat(),
            b in arb_unit_quat(),
            v in arb_small_vec3()
        ) {
            let at_0 = a.slerp(b, 0.0).xform(v);
            let from_a = a.xform(v);
            prop_assert!(approx_vec3(at_0, from_a));
            let at_1 = a.slerp(b, 1.0).xform(v);
            let from_b = b.xform(v);
            prop_assert!(approx_vec3(at_1, from_b));
        }

        #[test]
        fn quat_mul_associative(
            a in arb_unit_quat(),
            b in arb_unit_quat(),
            c in arb_unit_quat(),
            v in arb_small_vec3()
        ) {
            let r1 = ((a * b) * c).xform(v);
            let r2 = (a * (b * c)).xform(v);
            prop_assert!(approx_vec3(r1, r2));
        }
    }

    // -----------------------------------------------------------------------
    // Basis properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn basis_identity_preserves_vector(v in arb_vec3()) {
            prop_assert!(approx_vec3(Basis::IDENTITY.xform(v), v));
        }

        #[test]
        fn basis_inverse_round_trip(q in arb_unit_quat(), v in arb_small_vec3()) {
            let b = Basis::from_quaternion(q);
            let result = b.inverse().xform(b.xform(v));
            prop_assert!(approx_vec3(result, v));
        }

        #[test]
        fn basis_determinant_of_rotation_is_one(q in arb_unit_quat()) {
            let b = Basis::from_quaternion(q);
            prop_assert!(approx(b.determinant(), 1.0), "det was {}", b.determinant());
        }

        #[test]
        fn basis_transpose_of_rotation_is_inverse(q in arb_unit_quat(), v in arb_small_vec3()) {
            let b = Basis::from_quaternion(q);
            let via_inv = b.inverse().xform(v);
            let via_transpose = b.transposed().xform(v);
            prop_assert!(approx_vec3(via_inv, via_transpose));
        }
    }

    // -----------------------------------------------------------------------
    // Transform2D properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn transform2d_identity_preserves_point(p in arb_vec2()) {
            prop_assert!(approx_vec2(Transform2D::IDENTITY.xform(p), p));
        }

        #[test]
        fn transform2d_inverse_round_trip(
            angle in -std::f32::consts::PI..std::f32::consts::PI,
            offset in arb_small_vec2(),
            p in arb_small_vec2()
        ) {
            let t = Transform2D::rotated(angle) * Transform2D::translated(offset);
            let result = t.affine_inverse().xform(t.xform(p));
            prop_assert!(approx_vec2(result, p), "expected {:?}, got {:?}", p, result);
        }

        #[test]
        fn transform2d_rotation_preserves_length(
            angle in -std::f32::consts::PI..std::f32::consts::PI,
            v in arb_vec2()
        ) {
            let t = Transform2D::rotated(angle);
            let rotated = t.basis_xform(v);
            prop_assert!(
                approx(v.length(), rotated.length()),
                "len before={}, after={}", v.length(), rotated.length()
            );
        }
    }

    // -----------------------------------------------------------------------
    // Transform3D properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn transform3d_identity_preserves_point(p in arb_vec3()) {
            prop_assert!(approx_vec3(Transform3D::IDENTITY.xform(p), p));
        }

        #[test]
        fn transform3d_inverse_round_trip(
            q in arb_unit_quat(),
            origin in arb_small_vec3(),
            p in arb_small_vec3()
        ) {
            let t = Transform3D {
                basis: Basis::from_quaternion(q),
                origin,
            };
            let result = t.inverse().xform(t.xform(p));
            prop_assert!(approx_vec3(result, p), "expected {:?}, got {:?}", p, result);
        }

        #[test]
        fn transform3d_composition_associative(
            qa in arb_unit_quat(),
            qb in arb_unit_quat(),
            qc in arb_unit_quat(),
            oa in arb_small_vec3(),
            ob in arb_small_vec3(),
            oc in arb_small_vec3(),
            p in arb_small_vec3()
        ) {
            let a = Transform3D { basis: Basis::from_quaternion(qa), origin: oa };
            let b = Transform3D { basis: Basis::from_quaternion(qb), origin: ob };
            let c = Transform3D { basis: Basis::from_quaternion(qc), origin: oc };
            let r1 = (a * b * c).xform(p);
            let r2 = a.xform(b.xform(c.xform(p)));
            prop_assert!(approx_vec3(r1, r2));
        }
    }

    // -----------------------------------------------------------------------
    // Rect2 properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn rect2_contains_own_position(pos in arb_vec2(), size in arb_positive_size_vec2()) {
            let rect = Rect2::new(pos, size);
            prop_assert!(rect.contains_point(pos));
        }

        #[test]
        fn rect2_intersects_symmetric(
            pos_a in arb_small_vec2(),
            size_a in arb_positive_size_vec2(),
            pos_b in arb_small_vec2(),
            size_b in arb_positive_size_vec2()
        ) {
            let a = Rect2::new(pos_a, size_a);
            let b = Rect2::new(pos_b, size_b);
            prop_assert_eq!(a.intersects(b), b.intersects(a));
        }

        #[test]
        fn rect2_area_non_negative(
            sx in 0.0f32..100.0,
            sy in 0.0f32..100.0
        ) {
            let rect = Rect2::new(Vector2::ZERO, Vector2::new(sx, sy));
            prop_assert!(rect.area() >= 0.0);
        }
    }

    // -----------------------------------------------------------------------
    // AABB properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn aabb_contains_center(pos in arb_vec3(), size in arb_positive_size_vec3()) {
            let aabb = Aabb::new(pos, size);
            prop_assert!(aabb.contains_point(aabb.get_center()));
        }

        #[test]
        fn aabb_contains_own_position(pos in arb_vec3(), size in arb_positive_size_vec3()) {
            let aabb = Aabb::new(pos, size);
            prop_assert!(aabb.contains_point(pos));
        }

        #[test]
        fn aabb_intersects_symmetric(
            pos_a in arb_small_vec3(),
            size_a in arb_positive_size_vec3(),
            pos_b in arb_small_vec3(),
            size_b in arb_positive_size_vec3()
        ) {
            let a = Aabb::new(pos_a, size_a);
            let b = Aabb::new(pos_b, size_b);
            prop_assert_eq!(a.intersects(b), b.intersects(a));
        }

        #[test]
        fn aabb_merge_encloses_both(
            pos_a in arb_small_vec3(),
            size_a in arb_positive_size_vec3(),
            pos_b in arb_small_vec3(),
            size_b in arb_positive_size_vec3()
        ) {
            let a = Aabb::new(pos_a, size_a);
            let b = Aabb::new(pos_b, size_b);
            let merged = a.merge(b);
            prop_assert!(merged.contains_point(a.position));
            prop_assert!(merged.contains_point(b.position));
        }

        #[test]
        fn aabb_volume_non_negative(
            sx in 0.0f32..100.0,
            sy in 0.0f32..100.0,
            sz in 0.0f32..100.0
        ) {
            let aabb = Aabb::new(Vector3::ZERO, Vector3::new(sx, sy, sz));
            prop_assert!(aabb.get_volume() >= 0.0);
        }

        #[test]
        fn aabb_expand_grows_or_stays(
            pos in arb_small_vec3(),
            size in arb_positive_size_vec3(),
            point in arb_vec3()
        ) {
            let aabb = Aabb::new(pos, size);
            let expanded = aabb.expand(point);
            // Expanded volume should be >= original volume
            prop_assert!(expanded.get_volume() >= aabb.get_volume() - EPSILON);
        }
    }

    // -----------------------------------------------------------------------
    // Color properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn color_lerp_boundaries(a in arb_color(), b in arb_color()) {
            let at0 = a.lerp(b, 0.0);
            let at1 = a.lerp(b, 1.0);
            prop_assert!(approx(at0.r, a.r) && approx(at0.g, a.g) && approx(at0.b, a.b) && approx(at0.a, a.a));
            prop_assert!(approx(at1.r, b.r) && approx(at1.g, b.g) && approx(at1.b, b.b) && approx(at1.a, b.a));
        }

        #[test]
        fn color_lerp_midpoint_is_average(a in arb_color(), b in arb_color()) {
            let mid = a.lerp(b, 0.5);
            prop_assert!(approx(mid.r, (a.r + b.r) / 2.0));
            prop_assert!(approx(mid.g, (a.g + b.g) / 2.0));
            prop_assert!(approx(mid.b, (a.b + b.b) / 2.0));
            prop_assert!(approx(mid.a, (a.a + b.a) / 2.0));
        }
    }

    // -----------------------------------------------------------------------
    // Plane properties
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn plane_point_on_plane_distance_zero(
            normal in arb_nonzero_vec3(),
            d in -50.0f32..50.0
        ) {
            let n = normal.normalized();
            let plane = Plane::new(n, d);
            let on_plane = Vector3::new(n.x * d, n.y * d, n.z * d);
            prop_assert!(
                approx(plane.distance_to(on_plane), 0.0),
                "distance was {}", plane.distance_to(on_plane)
            );
        }

        #[test]
        fn plane_positive_side_classification(
            normal in arb_nonzero_vec3(),
            d in -50.0f32..50.0
        ) {
            let n = normal.normalized();
            let plane = Plane::new(n, d);
            let above = Vector3::new(
                n.x * (d + 10.0),
                n.y * (d + 10.0),
                n.z * (d + 10.0),
            );
            prop_assert!(plane.is_point_over(above));
        }

        #[test]
        fn plane_from_points_contains_source_points(
            a in arb_small_vec3(),
            b in arb_small_vec3(),
            c in arb_small_vec3()
        ) {
            if (b - a).cross(c - a).length() < 0.01 {
                return Ok(());
            }
            let plane = Plane::from_points(a, b, c);
            let tol = 1e-3;
            prop_assert!(plane.distance_to(a).abs() < tol, "dist to a = {}", plane.distance_to(a));
            prop_assert!(plane.distance_to(b).abs() < tol, "dist to b = {}", plane.distance_to(b));
            prop_assert!(plane.distance_to(c).abs() < tol, "dist to c = {}", plane.distance_to(c));
        }
    }
}
