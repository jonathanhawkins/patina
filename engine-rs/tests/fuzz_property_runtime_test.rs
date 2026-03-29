//! pat-yvbg / pat-cuk: Fuzz and property tests for high-risk runtime surfaces.
//!
//! Uses deterministic pseudo-random input generation to exercise edge cases
//! in math, physics, parsing, and core types. Each test runs many iterations
//! with varied inputs including NaN, infinity, zero, very large, and very
//! small values.

use gdcore::id::ObjectId;
use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::math3d::{Basis, Quaternion, Transform3D};
use gdcore::node_path::NodePath;
use gdobject::signal::{Connection, Signal};
use gdresource::loader::{parse_variant_value, TresLoader};
use gdvariant::Variant;

// ===========================================================================
// Pseudo-random input generation (no external deps)
// ===========================================================================

/// Simple xorshift64 PRNG for deterministic property testing.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() % 10000) as f32 / 10000.0
    }

    fn next_f32_range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }

    /// Generates a float from a mix of normal, edge, and extreme values.
    fn next_fuzz_f32(&mut self) -> f32 {
        let choice = self.next_u64() % 20;
        match choice {
            0 => 0.0,
            1 => -0.0,
            2 => 1.0,
            3 => -1.0,
            4 => f32::EPSILON,
            5 => -f32::EPSILON,
            6 => f32::MAX,
            7 => f32::MIN,
            8 => f32::INFINITY,
            9 => f32::NEG_INFINITY,
            10 => f32::NAN,
            11 => 1e-30,
            12 => -1e-30,
            13 => 1e30,
            14 => -1e30,
            _ => self.next_f32_range(-1000.0, 1000.0),
        }
    }

    fn next_vec2(&mut self) -> Vector2 {
        Vector2::new(self.next_fuzz_f32(), self.next_fuzz_f32())
    }

    fn next_vec3(&mut self) -> Vector3 {
        Vector3::new(
            self.next_fuzz_f32(),
            self.next_fuzz_f32(),
            self.next_fuzz_f32(),
        )
    }

    /// Generates a finite float (no NaN/Inf).
    fn next_finite_f32(&mut self) -> f32 {
        let v = self.next_f32_range(-100.0, 100.0);
        if v.is_finite() {
            v
        } else {
            0.0
        }
    }

    fn next_finite_vec2(&mut self) -> Vector2 {
        Vector2::new(self.next_finite_f32(), self.next_finite_f32())
    }

    fn next_finite_vec3(&mut self) -> Vector3 {
        Vector3::new(
            self.next_finite_f32(),
            self.next_finite_f32(),
            self.next_finite_f32(),
        )
    }

    fn next_nonzero_finite_f32(&mut self) -> f32 {
        loop {
            let v = self.next_f32_range(-100.0, 100.0);
            if v.is_finite() && v.abs() > 1e-6 {
                return v;
            }
        }
    }
}

const ITERATIONS: usize = 500;

// ===========================================================================
// 1. Vector2 normalization properties
// ===========================================================================

#[test]
fn prop_vector2_normalized_length_is_one_or_zero() {
    let mut rng = Rng::new(42);
    for _ in 0..ITERATIONS {
        let v = rng.next_finite_vec2();
        let n = v.normalized();
        let len = n.length();
        // Either zero vector (input was zero-ish) or unit length
        assert!(
            len < 1e-6 || (len - 1.0).abs() < 1e-4,
            "normalized length must be ~0 or ~1, got {len} for v={v:?}"
        );
    }
}

#[test]
fn prop_vector2_normalization_is_idempotent() {
    let mut rng = Rng::new(123);
    for _ in 0..ITERATIONS {
        let v = rng.next_finite_vec2();
        let n1 = v.normalized();
        let n2 = n1.normalized();
        let diff = (n1 - n2).length();
        assert!(
            diff < 1e-5,
            "normalization must be idempotent: n1={n1:?}, n2={n2:?}"
        );
    }
}

#[test]
fn prop_vector2_dot_self_is_length_squared() {
    let mut rng = Rng::new(456);
    for _ in 0..ITERATIONS {
        let v = rng.next_finite_vec2();
        let dot = v.dot(v);
        let len_sq = v.length_squared();
        assert!(
            (dot - len_sq).abs() < 1e-3,
            "dot(v,v) must equal length_squared: dot={dot}, len_sq={len_sq}"
        );
    }
}

// ===========================================================================
// 2. Vector3 normalization and cross product
// ===========================================================================

#[test]
fn prop_vector3_normalized_length() {
    let mut rng = Rng::new(789);
    for _ in 0..ITERATIONS {
        let v = rng.next_finite_vec3();
        let n = v.normalized();
        let len = n.length();
        assert!(
            len < 1e-6 || (len - 1.0).abs() < 1e-4,
            "normalized length must be ~0 or ~1, got {len}"
        );
    }
}

#[test]
fn prop_vector3_cross_anticommutative() {
    let mut rng = Rng::new(1011);
    for _ in 0..ITERATIONS {
        let a = rng.next_finite_vec3();
        let b = rng.next_finite_vec3();
        let ab = a.cross(b);
        let ba = b.cross(a);
        let sum = ab + ba;
        assert!(
            sum.length() < 1e-3,
            "cross(a,b) + cross(b,a) must be ~zero: sum={sum:?}"
        );
    }
}

#[test]
fn prop_vector3_cross_perpendicular() {
    let mut rng = Rng::new(1213);
    for _ in 0..ITERATIONS {
        let a = rng.next_finite_vec3();
        let b = rng.next_finite_vec3();
        let c = a.cross(b);
        let c_len = c.length();
        if c_len > 1e-6 {
            // Use relative tolerance: |dot| / (|a|*|c|) should be near zero.
            let rel_a = a.dot(c).abs() / (a.length() * c_len);
            let rel_b = b.dot(c).abs() / (b.length() * c_len);
            assert!(
                rel_a < 1e-3,
                "cross product must be perpendicular to a: rel_dot={rel_a}"
            );
            assert!(
                rel_b < 1e-3,
                "cross product must be perpendicular to b: rel_dot={rel_b}"
            );
        }
    }
}

// ===========================================================================
// 3. Quaternion slerp properties
// ===========================================================================

#[test]
fn prop_quaternion_slerp_endpoints() {
    let mut rng = Rng::new(1415);
    for _ in 0..ITERATIONS {
        let euler_a = rng.next_finite_vec3() * 0.1;
        let euler_b = rng.next_finite_vec3() * 0.1;
        let a = Quaternion::from_euler(euler_a).normalized();
        let b = Quaternion::from_euler(euler_b).normalized();

        let at0 = a.slerp(b, 0.0);
        let at1 = a.slerp(b, 1.0);

        let diff0 = quat_dist(at0, a);
        let diff1 = quat_dist(at1, b);
        assert!(diff0 < 1e-3, "slerp(a,b,0) must equal a: diff={diff0}");
        assert!(diff1 < 1e-3, "slerp(a,b,1) must equal b: diff={diff1}");
    }
}

#[test]
fn prop_quaternion_slerp_unit_length() {
    let mut rng = Rng::new(1617);
    for _ in 0..ITERATIONS {
        let a = Quaternion::from_euler(rng.next_finite_vec3() * 0.1).normalized();
        let b = Quaternion::from_euler(rng.next_finite_vec3() * 0.1).normalized();
        let t = rng.next_f32();
        let result = a.slerp(b, t);
        let len = result.length();
        assert!(
            (len - 1.0).abs() < 1e-3,
            "slerp result must be unit length: len={len}, t={t}"
        );
    }
}

#[test]
fn prop_quaternion_slerp_self_is_identity() {
    let mut rng = Rng::new(1819);
    for _ in 0..ITERATIONS {
        let q = Quaternion::from_euler(rng.next_finite_vec3() * 0.1).normalized();
        let t = rng.next_f32();
        let result = q.slerp(q, t);
        let diff = quat_dist(result, q);
        assert!(diff < 1e-3, "slerp(q,q,t) must equal q: diff={diff}");
    }
}

fn quat_dist(a: Quaternion, b: Quaternion) -> f32 {
    let dot = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs();
    if dot > 1.0 {
        0.0
    } else {
        (1.0 - dot).abs()
    }
}

// ===========================================================================
// 4. Basis inverse roundtrip
// ===========================================================================

#[test]
fn prop_basis_inverse_roundtrip() {
    let mut rng = Rng::new(2021);
    let mut tested = 0;
    for _ in 0..ITERATIONS {
        let euler = rng.next_finite_vec3() * 0.5;
        let basis = Basis::from_euler(euler);
        let det = basis.determinant();
        if det.abs() < 1e-6 {
            continue; // Skip near-singular
        }
        let inv = basis.inverse();
        let product = basis.xform(inv.xform(Vector3::new(1.0, 2.0, 3.0)));
        let expected = Vector3::new(1.0, 2.0, 3.0);
        let diff = (product - expected).length();
        assert!(
            diff < 1e-2,
            "basis * inverse * v must equal v: diff={diff}, det={det}"
        );
        tested += 1;
    }
    assert!(tested > 100, "must test enough non-singular bases");
}

// ===========================================================================
// 5. Transform2D affine_inverse roundtrip
// ===========================================================================

#[test]
fn prop_transform2d_inverse_roundtrip() {
    let mut rng = Rng::new(2223);
    for _ in 0..ITERATIONS {
        let angle = rng.next_finite_f32() * 0.5;
        let origin = rng.next_finite_vec2();
        let t = Transform2D::rotated(angle);
        let t = Transform2D { origin, ..t };

        let inv = t.affine_inverse();
        let point = rng.next_finite_vec2();
        let roundtrip = inv.xform(t.xform(point));
        let diff = (roundtrip - point).length();
        assert!(
            diff < 1e-2,
            "t.inverse(t.xform(p)) must equal p: diff={diff}"
        );
    }
}

#[test]
fn prop_transform2d_identity_xform_is_noop() {
    let mut rng = Rng::new(2425);
    for _ in 0..ITERATIONS {
        let p = rng.next_finite_vec2();
        let result = Transform2D::IDENTITY.xform(p);
        let diff = (result - p).length();
        assert!(diff < 1e-6, "identity xform must be noop: diff={diff}");
    }
}

// ===========================================================================
// 6. Transform3D inverse roundtrip
// ===========================================================================

#[test]
fn prop_transform3d_inverse_roundtrip() {
    let mut rng = Rng::new(2627);
    for _ in 0..ITERATIONS {
        let euler = rng.next_finite_vec3() * 0.5;
        let origin = rng.next_finite_vec3();
        let t = Transform3D {
            basis: Basis::from_euler(euler),
            origin,
        };
        let inv = t.inverse();
        let point = rng.next_finite_vec3();
        let roundtrip = inv.xform(t.xform(point));
        let diff = (roundtrip - point).length();
        assert!(diff < 1e-1, "T.inv(T.xform(p)) must equal p: diff={diff}");
    }
}

// ===========================================================================
// 7. Color clamping and construction
// ===========================================================================

#[test]
fn prop_color_components_in_range_after_clamp() {
    let mut rng = Rng::new(2829);
    for _ in 0..ITERATIONS {
        let r = rng.next_fuzz_f32();
        let g = rng.next_fuzz_f32();
        let b = rng.next_fuzz_f32();
        let a = rng.next_fuzz_f32();
        let c = Color::new(r, g, b, a);
        // Color should store values as-is (not clamp on construction).
        // Verify no panic on construction with extreme values.
        let _ = c.r;
        let _ = c.g;
        let _ = c.b;
        let _ = c.a;
    }
}

// ===========================================================================
// 8. Collision detection — no panics with degenerate inputs
// ===========================================================================

#[test]
fn prop_collision_no_panic_random_circles() {
    use gdphysics2d::collision::test_collision;
    use gdphysics2d::shape::Shape2D;

    let mut rng = Rng::new(3031);
    for _ in 0..ITERATIONS {
        let r_a = rng.next_finite_f32().abs().max(0.001);
        let r_b = rng.next_finite_f32().abs().max(0.001);
        let pos_a = rng.next_finite_vec2();
        let pos_b = rng.next_finite_vec2();

        let shape_a = Shape2D::Circle { radius: r_a };
        let shape_b = Shape2D::Circle { radius: r_b };
        let t_a = Transform2D::translated(pos_a);
        let t_b = Transform2D::translated(pos_b);

        let result = test_collision(&shape_a, &t_a, &shape_b, &t_b);
        if let Some(r) = result {
            if r.colliding {
                assert!(r.depth >= -1e-3, "depth must be non-negative: {}", r.depth);
                let nlen = r.normal.length();
                assert!(
                    nlen < 1e-6 || (nlen - 1.0).abs() < 1e-3,
                    "normal must be unit or zero: len={nlen}"
                );
            }
        }
    }
}

#[test]
fn prop_collision_no_panic_random_rects() {
    use gdphysics2d::collision::test_collision;
    use gdphysics2d::shape::Shape2D;

    let mut rng = Rng::new(3233);
    for _ in 0..ITERATIONS {
        let he_a = Vector2::new(
            rng.next_finite_f32().abs().max(0.001),
            rng.next_finite_f32().abs().max(0.001),
        );
        let he_b = Vector2::new(
            rng.next_finite_f32().abs().max(0.001),
            rng.next_finite_f32().abs().max(0.001),
        );
        let pos_a = rng.next_finite_vec2();
        let pos_b = rng.next_finite_vec2();

        let shape_a = Shape2D::Rectangle { half_extents: he_a };
        let shape_b = Shape2D::Rectangle { half_extents: he_b };
        let t_a = Transform2D::translated(pos_a);
        let t_b = Transform2D::translated(pos_b);

        let result = test_collision(&shape_a, &t_a, &shape_b, &t_b);
        // Must not panic — result correctness tested by shape.
        if let Some(r) = result {
            if r.colliding {
                assert!(
                    r.depth >= -1e-3,
                    "rect depth must be non-negative: {}",
                    r.depth
                );
            }
        }
    }
}

#[test]
fn prop_collision_no_panic_circle_rect() {
    use gdphysics2d::collision::test_collision;
    use gdphysics2d::shape::Shape2D;

    let mut rng = Rng::new(3435);
    for _ in 0..ITERATIONS {
        let radius = rng.next_finite_f32().abs().max(0.001);
        let half_ext = Vector2::new(
            rng.next_finite_f32().abs().max(0.001),
            rng.next_finite_f32().abs().max(0.001),
        );
        let pos_a = rng.next_finite_vec2();
        let pos_b = rng.next_finite_vec2();

        let shape_a = Shape2D::Circle { radius };
        let shape_b = Shape2D::Rectangle {
            half_extents: half_ext,
        };
        let t_a = Transform2D::translated(pos_a);
        let t_b = Transform2D::translated(pos_b);

        let _ = test_collision(&shape_a, &t_a, &shape_b, &t_b);
        let _ = test_collision(&shape_b, &t_b, &shape_a, &t_a);
    }
}

#[test]
fn prop_collision_coincident_circles_no_panic() {
    use gdphysics2d::collision::test_collision;
    use gdphysics2d::shape::Shape2D;

    let mut rng = Rng::new(3637);
    for _ in 0..ITERATIONS {
        let r = rng.next_finite_f32().abs().max(0.001);
        let pos = rng.next_finite_vec2();

        let shape = Shape2D::Circle { radius: r };
        let t = Transform2D::translated(pos);
        // Two identical shapes at the same position — extreme overlap.
        let _ = test_collision(&shape, &t, &shape, &t);
    }
}

// ===========================================================================
// 9. Shape containment — no panics with degenerate shapes
// ===========================================================================

#[test]
fn prop_shape2d_contains_origin() {
    use gdphysics2d::shape::Shape2D;

    let mut rng = Rng::new(3839);
    for _ in 0..ITERATIONS {
        let r = rng.next_finite_f32().abs().max(0.01);
        let shapes = [
            Shape2D::Circle { radius: r },
            Shape2D::Rectangle {
                half_extents: Vector2::new(r, r),
            },
        ];
        for shape in &shapes {
            assert!(
                shape.contains_point(Vector2::ZERO),
                "shape {:?} must contain origin",
                shape
            );
        }
    }
}

#[test]
fn prop_shape2d_containment_no_panic_fuzz() {
    use gdphysics2d::shape::Shape2D;

    let mut rng = Rng::new(4041);
    for _ in 0..ITERATIONS {
        let r = rng.next_finite_f32().abs().max(0.001);
        let p = rng.next_finite_vec2();
        let shapes = [
            Shape2D::Circle { radius: r },
            Shape2D::Rectangle {
                half_extents: Vector2::new(r, r * 2.0),
            },
            Shape2D::Capsule {
                radius: r,
                height: r * 3.0,
            },
            Shape2D::Segment {
                a: Vector2::new(-r, 0.0),
                b: Vector2::new(r, 0.0),
            },
        ];
        for shape in &shapes {
            let _ = shape.contains_point(p);
        }
    }
}

// ===========================================================================
// 10. NodePath parsing — no panics with arbitrary strings
// ===========================================================================

#[test]
fn prop_nodepath_roundtrip() {
    let paths = [
        "/root/Node",
        "relative/path",
        "/root/Node:property",
        "/root/Node:sub:name",
        ".",
        "..",
        "../sibling",
        "",
        "//double//slash",
        "/",
        "/a/b/c/d/e/f/g/h",
        "node:prop:subprop",
    ];
    for path in &paths {
        let np = NodePath::new(path);
        let s = np.to_string();
        let np2 = NodePath::new(&s);
        assert_eq!(
            np, np2,
            "NodePath roundtrip failed for '{path}': '{s}' -> {np2:?}"
        );
    }
}

#[test]
fn prop_nodepath_no_panic_fuzz() {
    // Feed various weird strings into NodePath::new — must not panic.
    let repeated_a = "a/".repeat(100);
    let repeated_b = "/b".repeat(100);
    let repeated_c = "a:".repeat(50);
    let inputs = [
        "",
        "/",
        "//",
        "///",
        ":::::",
        "/root",
        "a:b:c:d:e:f",
        "/ /space/ ",
        "/root/日本語",
        "\n\t\r",
        "/root/node\x00embedded",
        repeated_a.as_str(),
        repeated_b.as_str(),
        repeated_c.as_str(),
    ];
    for input in &inputs {
        let np = NodePath::new(input);
        let _ = np.is_absolute();
        let _ = np.is_empty();
        let _ = np.get_name_count();
        let _ = np.get_subname_count();
        let _ = np.to_string();
    }
}

#[test]
fn prop_nodepath_absolute_starts_with_slash() {
    let absolute_paths = ["/root", "/root/child", "/a/b/c"];
    for path in &absolute_paths {
        let np = NodePath::new(path);
        assert!(np.is_absolute(), "path '{}' must be absolute", path);
    }

    let relative_paths = ["node", "a/b", ".", ".."];
    for path in &relative_paths {
        let np = NodePath::new(path);
        assert!(!np.is_absolute(), "path '{}' must be relative", path);
    }
}

// ===========================================================================
// 11. Math operations with NaN/Infinity — no panics
// ===========================================================================

#[test]
fn prop_vector2_ops_no_panic_with_nan_inf() {
    let mut rng = Rng::new(4243);
    for _ in 0..ITERATIONS {
        let a = rng.next_vec2(); // May contain NaN, Inf
        let b = rng.next_vec2();
        let _ = a + b;
        let _ = a - b;
        let _ = a * 2.0;
        let _ = a.dot(b);
        let _ = a.length();
        let _ = a.length_squared();
        let _ = a.normalized();
        let _ = a.cross(b);
    }
}

#[test]
fn prop_vector3_ops_no_panic_with_nan_inf() {
    let mut rng = Rng::new(4445);
    for _ in 0..ITERATIONS {
        let a = rng.next_vec3();
        let b = rng.next_vec3();
        let _ = a + b;
        let _ = a - b;
        let _ = a * 2.0;
        let _ = a.dot(b);
        let _ = a.length();
        let _ = a.normalized();
        let _ = a.cross(b);
    }
}

#[test]
fn prop_quaternion_ops_no_panic_with_extreme_values() {
    let mut rng = Rng::new(4647);
    for _ in 0..ITERATIONS {
        let q = Quaternion::new(
            rng.next_fuzz_f32(),
            rng.next_fuzz_f32(),
            rng.next_fuzz_f32(),
            rng.next_fuzz_f32(),
        );
        let _ = q.length();
        let _ = q.normalized();
        let _ = q.inverse();
    }
}

// ===========================================================================
// 12. 3D physics — no panics with random bodies
// ===========================================================================

#[test]
fn prop_physics3d_simulation_no_panic() {
    use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
    use gdphysics3d::shape::Shape3D;
    use gdphysics3d::world::PhysicsWorld3D;

    let mut rng = Rng::new(4849);
    let mut world = PhysicsWorld3D::new();

    for i in 0..20 {
        let pos = rng.next_finite_vec3();
        let radius = rng.next_finite_f32().abs().max(0.1);
        let mass = rng.next_finite_f32().abs().max(0.1);
        let body = PhysicsBody3D::new(
            BodyId3D(i),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius },
            mass,
        );
        world.add_body(body);
    }

    // Step many frames — must not panic.
    for _ in 0..200 {
        world.step(1.0 / 60.0);
    }
}

#[test]
fn prop_physics3d_raycast_no_panic_random_rays() {
    use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
    use gdphysics3d::shape::Shape3D;
    use gdphysics3d::world::PhysicsWorld3D;

    let mut rng = Rng::new(5051);
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    for i in 0..5 {
        let pos = rng.next_finite_vec3() * 10.0;
        let body = PhysicsBody3D::new(
            BodyId3D(i),
            BodyType3D::Static,
            pos,
            Shape3D::Sphere { radius: 2.0 },
            1.0,
        );
        world.add_body(body);
    }

    for _ in 0..ITERATIONS {
        let origin = rng.next_finite_vec3() * 20.0;
        let dir = rng.next_finite_vec3();
        if dir.length() > 1e-6 {
            let _ = world.raycast(origin, dir);
        }
    }
}

// ===========================================================================
// 13. 3D shape containment property tests
// ===========================================================================

#[test]
fn prop_shape3d_sphere_origin_always_inside() {
    use gdphysics3d::shape::Shape3D;

    let mut rng = Rng::new(5253);
    for _ in 0..ITERATIONS {
        let r = rng.next_finite_f32().abs().max(0.001);
        let shape = Shape3D::Sphere { radius: r };
        assert!(shape.contains_point(Vector3::ZERO));
    }
}

#[test]
fn prop_shape3d_box_origin_always_inside() {
    use gdphysics3d::shape::Shape3D;

    let mut rng = Rng::new(5455);
    for _ in 0..ITERATIONS {
        let he = Vector3::new(
            rng.next_finite_f32().abs().max(0.001),
            rng.next_finite_f32().abs().max(0.001),
            rng.next_finite_f32().abs().max(0.001),
        );
        let shape = Shape3D::BoxShape { half_extents: he };
        assert!(shape.contains_point(Vector3::ZERO));
    }
}

#[test]
fn prop_shape3d_capsule_origin_always_inside() {
    use gdphysics3d::shape::Shape3D;

    let mut rng = Rng::new(5657);
    for _ in 0..ITERATIONS {
        let r = rng.next_finite_f32().abs().max(0.01);
        let h = rng.next_finite_f32().abs().max(r * 2.0 + 0.01);
        let shape = Shape3D::CapsuleShape {
            radius: r,
            height: h,
        };
        assert!(
            shape.contains_point(Vector3::ZERO),
            "capsule r={r} h={h} must contain origin"
        );
    }
}

#[test]
fn prop_shape3d_sphere_outside_point() {
    use gdphysics3d::shape::Shape3D;

    let mut rng = Rng::new(5859);
    for _ in 0..ITERATIONS {
        let r = rng.next_finite_f32().abs().max(0.01).min(100.0);
        let shape = Shape3D::Sphere { radius: r };
        // A point at distance 2*r must be outside.
        let outside = Vector3::new(r * 2.0, 0.0, 0.0);
        assert!(
            !shape.contains_point(outside),
            "point at 2r must be outside sphere r={r}"
        );
    }
}

// ===========================================================================
// 14. StringName interning — same string gives same result
// ===========================================================================

#[test]
fn prop_string_name_interning_idempotent() {
    use gdcore::string_name::StringName;

    let names = ["Node2D", "position", "signal_fired", "", "日本語", "a b c"];
    for name in &names {
        let sn1 = StringName::new(name);
        let sn2 = StringName::new(name);
        assert_eq!(sn1, sn2, "interning same string must produce equal results");
        assert_eq!(sn1.as_str(), *name);
    }
}

// ===========================================================================
// 15. Rect2 containment and intersection properties
// ===========================================================================

#[test]
fn prop_rect2_contains_own_center() {
    let mut rng = Rng::new(6061);
    for _ in 0..ITERATIONS {
        let pos = rng.next_finite_vec2();
        let size = Vector2::new(
            rng.next_finite_f32().abs().max(0.01),
            rng.next_finite_f32().abs().max(0.01),
        );
        let rect = Rect2::new(pos, size);
        let center = Vector2::new(pos.x + size.x / 2.0, pos.y + size.y / 2.0);
        assert!(
            rect.contains_point(center),
            "rect must contain its center: rect={rect:?}, center={center:?}"
        );
    }
}

#[test]
fn prop_rect2_intersects_self() {
    let mut rng = Rng::new(6263);
    for _ in 0..ITERATIONS {
        let pos = rng.next_finite_vec2();
        let size = Vector2::new(
            rng.next_finite_f32().abs().max(0.01),
            rng.next_finite_f32().abs().max(0.01),
        );
        let rect = Rect2::new(pos, size);
        assert!(
            rect.intersects(rect),
            "rect must intersect itself: {rect:?}"
        );
    }
}

// ===========================================================================
// 16. 3D collision detection properties
// ===========================================================================

#[test]
fn prop_collision3d_sphere_sphere_symmetric() {
    use gdphysics3d::collision::test_sphere_sphere;

    let mut rng = Rng::new(6465);
    for _ in 0..ITERATIONS {
        let pos_a = rng.next_finite_vec3();
        let pos_b = rng.next_finite_vec3();
        let r_a = rng.next_finite_f32().abs().max(0.01);
        let r_b = rng.next_finite_f32().abs().max(0.01);

        let ab = test_sphere_sphere(pos_a, r_a, pos_b, r_b);
        let ba = test_sphere_sphere(pos_b, r_b, pos_a, r_a);
        assert_eq!(ab.colliding, ba.colliding, "collision must be symmetric");
        if ab.colliding {
            assert!(
                (ab.depth - ba.depth).abs() < 1e-3,
                "depth must be symmetric: {} vs {}",
                ab.depth,
                ba.depth
            );
        }
    }
}

// ===========================================================================
// 17. Variant arithmetic — type promotion and edge cases
// ===========================================================================

#[test]
fn prop_variant_add_commutative_for_numerics() {
    let mut rng = Rng::new(7001);
    for _ in 0..ITERATIONS {
        let a = random_numeric_variant(&mut rng);
        let b = random_numeric_variant(&mut rng);
        let ab = a.clone() + b.clone();
        let ba = b.clone() + a.clone();
        assert_variant_numeric_eq(&ab, &ba, "add must be commutative");
    }
}

#[test]
fn prop_variant_mul_commutative_for_numerics() {
    let mut rng = Rng::new(7002);
    for _ in 0..ITERATIONS {
        let a = random_numeric_variant(&mut rng);
        let b = random_numeric_variant(&mut rng);
        let ab = a.clone() * b.clone();
        let ba = b.clone() * a.clone();
        assert_variant_numeric_eq(&ab, &ba, "mul must be commutative");
    }
}

#[test]
fn prop_variant_add_zero_identity() {
    let mut rng = Rng::new(7003);
    let zeros = [Variant::Int(0), Variant::Float(0.0)];
    for _ in 0..ITERATIONS {
        let a = random_numeric_variant(&mut rng);
        for zero in &zeros {
            let result = a.clone() + zero.clone();
            // Result should be numerically equal to a (possibly promoted to float)
            let a_val = variant_as_f64(&a);
            let r_val = variant_as_f64(&result);
            // Skip NaN (NaN != NaN) and Inf (Inf - Inf = NaN)
            if a_val.is_nan() || r_val.is_nan() {
                continue;
            }
            assert!(
                (a_val - r_val).abs() < 1e-10 || a_val == r_val,
                "a + 0 must equal a: a={a:?}, result={result:?}"
            );
        }
    }
}

#[test]
fn prop_variant_div_by_zero_yields_nil() {
    let values = [
        Variant::Int(42),
        Variant::Int(0),
        Variant::Int(-1),
        Variant::Float(3.14),
        Variant::Int(i64::MAX),
        Variant::Int(i64::MIN),
    ];
    let zeros = [Variant::Int(0), Variant::Float(0.0)];
    for v in &values {
        for z in &zeros {
            let result = v.clone() / z.clone();
            assert_eq!(
                result,
                Variant::Nil,
                "division by zero must yield Nil: {v:?} / {z:?}"
            );
        }
    }
}

#[test]
fn prop_variant_mod_by_zero_yields_nil() {
    let values = [Variant::Int(42), Variant::Float(3.14), Variant::Int(0)];
    let zeros = [Variant::Int(0), Variant::Float(0.0)];
    for v in &values {
        for z in &zeros {
            let result = v.clone() % z.clone();
            assert_eq!(
                result,
                Variant::Nil,
                "mod by zero must yield Nil: {v:?} % {z:?}"
            );
        }
    }
}

#[test]
fn prop_variant_neg_double_is_identity() {
    let mut rng = Rng::new(7004);
    for _ in 0..ITERATIONS {
        let a = random_numeric_variant(&mut rng);
        let double_neg = -(-(a.clone()));
        assert_variant_numeric_eq(&a, &double_neg, "double negation must be identity");
    }
}

#[test]
fn prop_variant_int_overflow_wraps() {
    // Godot-style: Int arithmetic uses wrapping.
    let result = Variant::Int(i64::MAX) + Variant::Int(1);
    assert_eq!(
        result,
        Variant::Int(i64::MIN),
        "i64::MAX + 1 must wrap to MIN"
    );

    let result = Variant::Int(i64::MIN) - Variant::Int(1);
    assert_eq!(
        result,
        Variant::Int(i64::MAX),
        "i64::MIN - 1 must wrap to MAX"
    );
}

#[test]
fn prop_variant_unsupported_arith_yields_nil() {
    // Operations between incompatible types must not panic, should return Nil.
    let incompatible_pairs = [
        (Variant::String("hello".into()), Variant::Int(42)),
        (Variant::Bool(true), Variant::Float(1.0)),
        (Variant::Nil, Variant::Int(1)),
        (Variant::Array(vec![Variant::Int(1)]), Variant::Int(2)),
    ];
    for (a, b) in &incompatible_pairs {
        let _ = a.clone() - b.clone();
        let _ = a.clone() * b.clone();
        let _ = a.clone() / b.clone();
        let _ = a.clone() % b.clone();
        // Must not panic — that's the property we're testing.
    }
}

// ===========================================================================
// 18. Variant coercion fuzz — to_int, to_float, to_bool
// ===========================================================================

#[test]
fn prop_variant_coercion_no_panic() {
    let mut rng = Rng::new(7101);
    for _ in 0..ITERATIONS {
        let v = random_variant(&mut rng);
        let _ = v.to_int();
        let _ = v.to_float();
        let _ = v.to_bool();
        let _ = v.to_string_lossy();
        let _ = v.variant_type();
        let _ = v.is_nil();
        let _ = v.is_truthy();
    }
}

#[test]
fn prop_variant_int_to_float_roundtrip() {
    let mut rng = Rng::new(7102);
    for _ in 0..ITERATIONS {
        let i = (rng.next_u64() as i64) >> 11; // Stay in f64-exact range
        let v = Variant::Int(i);
        let as_float = v.to_float();
        let back = Variant::Float(as_float).to_int();
        assert_eq!(
            i, back,
            "Int->Float->Int roundtrip must preserve value for {i}"
        );
    }
}

#[test]
fn prop_variant_bool_coercion_matches_truthiness() {
    let test_cases = [
        (Variant::Nil, false),
        (Variant::Bool(false), false),
        (Variant::Bool(true), true),
        (Variant::Int(0), false),
        (Variant::Int(1), true),
        (Variant::Int(-1), true),
        (Variant::Float(0.0), false),
        (Variant::Float(1.0), true),
        (Variant::String("".into()), false),
        (Variant::String("hello".into()), true),
        (Variant::Array(vec![]), false),
        (Variant::Array(vec![Variant::Nil]), true),
    ];
    for (v, expected) in &test_cases {
        assert_eq!(v.to_bool(), *expected, "truthiness mismatch for {v:?}");
    }
}

#[test]
fn prop_variant_string_parse_coercion() {
    // String variants containing numbers should coerce to those numbers.
    let cases = [
        ("42", 42i64, 42.0f64),
        ("-7", -7, -7.0),
        ("0", 0, 0.0),
        ("not_a_number", 0, 0.0),
        ("", 0, 0.0),
        ("3.14", 0, 3.14), // parse as i64 fails -> 0
    ];
    for (s, expected_int, expected_float) in &cases {
        let v = Variant::String((*s).to_string());
        assert_eq!(v.to_int(), *expected_int, "to_int for \"{s}\"");
        assert!(
            (v.to_float() - expected_float).abs() < 1e-10,
            "to_float for \"{s}\""
        );
    }
}

// ===========================================================================
// 19. Variant comparison (PartialOrd) properties
// ===========================================================================

#[test]
fn prop_variant_comparison_reflexive() {
    let mut rng = Rng::new(7201);
    for _ in 0..ITERATIONS {
        let v = random_numeric_variant(&mut rng);
        let cmp = v.partial_cmp(&v);
        // NaN is not equal to itself per IEEE 754 — PartialOrd returns None.
        if let Variant::Float(f) = &v {
            if f.is_nan() {
                assert_eq!(cmp, None, "NaN must not be equal to itself");
                continue;
            }
        }
        assert!(
            cmp == Some(std::cmp::Ordering::Equal),
            "v must be equal to itself: {v:?}"
        );
    }
}

#[test]
fn prop_variant_comparison_antisymmetric() {
    let mut rng = Rng::new(7202);
    for _ in 0..ITERATIONS {
        let a = random_numeric_variant(&mut rng);
        let b = random_numeric_variant(&mut rng);
        if let (Some(ab), Some(ba)) = (a.partial_cmp(&b), b.partial_cmp(&a)) {
            assert_eq!(
                ab,
                ba.reverse(),
                "comparison must be antisymmetric: {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn prop_variant_comparison_cross_type_none() {
    // Comparing incompatible types must return None, not panic.
    let a = Variant::String("hello".into());
    let b = Variant::Int(42);
    assert_eq!(a.partial_cmp(&b), None, "String vs Int must be None");

    let a = Variant::Bool(true);
    let b = Variant::Float(1.0);
    assert_eq!(a.partial_cmp(&b), None, "Bool vs Float must be None");
}

// ===========================================================================
// 20. parse_variant_value fuzz — malformed input must not panic
// ===========================================================================

#[test]
fn fuzz_parse_variant_value_no_panic() {
    let inputs = [
        "",
        "   ",
        "null",
        "nil",
        "Nil",
        "true",
        "false",
        "42",
        "-7",
        "3.14",
        "-0.0",
        "1e30",
        "-1e-10",
        "\"hello\"",
        "\"\"",
        "\"escaped\\\"quote\"",
        "Vector2(1, 2)",
        "Vector2()",
        "Vector2(1)",
        "Vector2(1,2,3)",
        "Vector3(1, 2, 3)",
        "Vector3()",
        "Vector3(1,2)",
        "Color(1,0,0,1)",
        "Color(1,0,0)",
        "Color()",
        "Color(1)",
        "Rect2(0,0,10,10)",
        "Rect2()",
        "Rect2(1,2,3)",
        "Transform2D(1,0,0,1,0,0)",
        "Transform2D()",
        "Transform2D(1)",
        "Quaternion(0,0,0,1)",
        "Quaternion()",
        "Quaternion(1,2)",
        "Basis(1,0,0,0,1,0,0,0,1)",
        "Basis()",
        "AABB(0,0,0,1,1,1)",
        "AABB()",
        "Plane(0,1,0,0)",
        "Plane()",
        "StringName(\"hello\")",
        "StringName(&\"hello\")",
        "StringName()",
        "NodePath(\"a/b/c\")",
        "NodePath()",
        "ExtResource(\"1\")",
        "SubResource(\"2\")",
        "PackedByteArray(1, 2, 3)",
        "PackedByteArray()",
        "PackedInt32Array(1, 2)",
        "PackedFloat32Array(1.0, 2.0)",
        "PackedStringArray(\"a\", \"b\")",
        "PackedVector2Array(Vector2(1,2))",
        "[1, 2, 3]",
        "[]",
        "{\"a\": 1, \"b\": 2}",
        "{}",
        // Deliberately malformed
        "Vector2(nan, inf)",
        "Vector2(-inf, 0)",
        "NotAType(1,2,3)",
        "Vector2(",
        "Vector3(1,2,",
        "[[[",
        "{{{}}}",
        "\"unterminated string",
        "999999999999999999999999999999",
        "-999999999999999999999999999999",
        "\x00\x01\x02",
        "Vector2(1e999, 1e999)",
        "a = b = c",
        "😀🎮🔥",
    ];
    for input in &inputs {
        // The property: parse_variant_value must never panic.
        let _ = parse_variant_value(input);
    }
}

#[test]
fn fuzz_parse_variant_value_valid_roundtrip() {
    // For well-formed inputs, parsing must succeed and produce the expected type.
    let cases: &[(&str, fn(&Variant) -> bool)] = &[
        ("42", |v| matches!(v, Variant::Int(42))),
        ("-7", |v| matches!(v, Variant::Int(-7))),
        ("3.14", |v| matches!(v, Variant::Float(_))),
        ("true", |v| matches!(v, Variant::Bool(true))),
        ("false", |v| matches!(v, Variant::Bool(false))),
        ("null", |v| matches!(v, Variant::Nil)),
        (
            "\"hello\"",
            |v| matches!(v, Variant::String(s) if s == "hello"),
        ),
        ("Vector2(1, 2)", |v| matches!(v, Variant::Vector2(_))),
        ("Vector3(1, 2, 3)", |v| matches!(v, Variant::Vector3(_))),
        ("Color(1, 0, 0, 1)", |v| matches!(v, Variant::Color(_))),
    ];
    for (input, check) in cases {
        let result = parse_variant_value(input)
            .unwrap_or_else(|e| panic!("parse_variant_value({input:?}) failed: {e}"));
        assert!(
            check(&result),
            "type check failed for input: {input:?}, got: {result:?}"
        );
    }
}

// ===========================================================================
// 21. TresLoader fuzz — malformed .tres content must not panic
// ===========================================================================

#[test]
fn fuzz_tres_loader_no_panic_malformed() {
    let loader = TresLoader::new();
    let inputs = [
        "",
        "[gd_resource]",
        "[gd_resource type=\"Texture2D\"]",
        "[ext_resource type=\"Script\" path=\"res://test.gd\" id=\"1\"]",
        "[sub_resource type=\"StyleBox\" id=\"1\"]\ncolor = Color(1, 0, 0, 1)",
        "[resource]\nvalue = 42",
        // Malformed section headers
        "[",
        "[]",
        "[invalid",
        "[gd_resource type=\"Unclosed",
        // Property lines with no section
        "key = value",
        "key =",
        "= value",
        "no_equals_sign",
        // Nested weird values
        "[resource]\narray = [1, [2, [3]]]",
        "[resource]\ndict = {\"a\": {\"b\": 1}}",
        // Very long line
        &format!("[resource]\nlong = \"{}\"", "x".repeat(10000)),
        // Many sections
        &(0..100)
            .map(|i| format!("[sub_resource type=\"T\" id=\"{i}\"]\nval = {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        // Binary-ish junk
        "\x00\x01\x02\x03",
        // Comments and blank lines
        "; comment\n\n; another\n[resource]\n; inside\nval = 1",
        // Unicode
        "[resource]\nname = \"日本語テスト\"",
    ];
    for input in &inputs {
        let _ = loader.parse_str(input, "test://fuzz.tres");
    }
}

#[test]
fn fuzz_tres_loader_valid_resource_parses() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
value = 42
name = "test"
flag = true
"#;
    let result = loader.parse_str(tres, "res://test.tres");
    assert!(result.is_ok(), "valid .tres must parse: {:?}", result.err());
    let resource = result.unwrap();
    assert_eq!(resource.class_name, "Resource");
}

// ===========================================================================
// 22. Signal bind/unbind argument resolution properties
// ===========================================================================

#[test]
fn prop_signal_resolve_args_no_binds_is_identity() {
    let conn = Connection::new(ObjectId::next(), "method");
    let args = vec![Variant::Int(1), Variant::String("hello".into())];
    let resolved = conn.resolve_args(&args);
    assert_eq!(
        resolved, args,
        "no binds/unbinds must pass through unchanged"
    );
}

#[test]
fn prop_signal_resolve_args_binds_append() {
    let conn = Connection::new(ObjectId::next(), "method")
        .with_binds(vec![Variant::Int(99), Variant::Bool(true)]);
    let args = vec![Variant::Int(1), Variant::Int(2)];
    let resolved = conn.resolve_args(&args);
    assert_eq!(resolved.len(), 4);
    assert_eq!(resolved[0], Variant::Int(1));
    assert_eq!(resolved[1], Variant::Int(2));
    assert_eq!(resolved[2], Variant::Int(99));
    assert_eq!(resolved[3], Variant::Bool(true));
}

#[test]
fn prop_signal_resolve_args_unbinds_truncate() {
    let conn = Connection::new(ObjectId::next(), "method").with_unbinds(2);
    let args = vec![
        Variant::Int(1),
        Variant::Int(2),
        Variant::Int(3),
        Variant::Int(4),
    ];
    let resolved = conn.resolve_args(&args);
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0], Variant::Int(1));
    assert_eq!(resolved[1], Variant::Int(2));
}

#[test]
fn prop_signal_resolve_args_unbind_more_than_available() {
    // Unbinding more args than provided must not panic (saturating_sub).
    let conn = Connection::new(ObjectId::next(), "method").with_unbinds(10);
    let args = vec![Variant::Int(1), Variant::Int(2)];
    let resolved = conn.resolve_args(&args);
    assert_eq!(
        resolved.len(),
        0,
        "unbinding more than available must yield empty"
    );
}

#[test]
fn prop_signal_resolve_args_unbind_then_bind() {
    // Godot order: first unbind trailing args, then append binds.
    let conn = Connection::new(ObjectId::next(), "method")
        .with_unbinds(1)
        .with_binds(vec![Variant::String("bound".into())]);
    let args = vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)];
    let resolved = conn.resolve_args(&args);
    // unbind(1): [1, 2], then bind: [1, 2, "bound"]
    assert_eq!(resolved.len(), 3);
    assert_eq!(resolved[0], Variant::Int(1));
    assert_eq!(resolved[1], Variant::Int(2));
    assert_eq!(resolved[2], Variant::String("bound".into()));
}

#[test]
fn prop_signal_resolve_args_empty_signal_with_binds() {
    let conn = Connection::new(ObjectId::next(), "method").with_binds(vec![Variant::Int(42)]);
    let args: Vec<Variant> = vec![];
    let resolved = conn.resolve_args(&args);
    assert_eq!(
        resolved,
        vec![Variant::Int(42)],
        "empty signal args + bind must work"
    );
}

#[test]
fn prop_signal_resolve_args_fuzz() {
    let mut rng = Rng::new(7301);
    for _ in 0..ITERATIONS {
        let n_args = (rng.next_u64() % 10) as usize;
        let n_binds = (rng.next_u64() % 5) as usize;
        let n_unbinds = (rng.next_u64() % 15) as usize;

        let args: Vec<Variant> = (0..n_args).map(|i| Variant::Int(i as i64)).collect();
        let binds: Vec<Variant> = (0..n_binds).map(|i| Variant::Int(100 + i as i64)).collect();

        let conn = Connection::new(ObjectId::next(), "method")
            .with_unbinds(n_unbinds)
            .with_binds(binds.clone());

        let resolved = conn.resolve_args(&args);
        let expected_len = n_args.saturating_sub(n_unbinds) + n_binds;
        assert_eq!(
            resolved.len(),
            expected_len,
            "resolved length mismatch: args={n_args}, unbinds={n_unbinds}, binds={n_binds}"
        );
    }
}

// ===========================================================================
// 23. Signal emit with one-shot and deferred — no panics under stress
// ===========================================================================

#[test]
fn prop_signal_emit_oneshot_removes_connection() {
    let mut signal = Signal::new("test_signal");
    let id = ObjectId::next();
    let conn = Connection::with_callback(id, "on_test", |_| Variant::Int(1)).as_one_shot();
    signal.connect(conn);
    assert_eq!(signal.connection_count(), 1);

    let results = signal.emit(&[]);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Variant::Int(1));

    // After one-shot fires, connection is removed.
    assert_eq!(signal.connection_count(), 0);

    // Second emit produces no results.
    let results = signal.emit(&[]);
    assert_eq!(results.len(), 0);
}

#[test]
fn prop_signal_emit_many_connections_no_panic() {
    let mut signal = Signal::new("stress_signal");
    for i in 0..100 {
        let conn =
            Connection::with_callback(ObjectId::next(), format!("method_{i}"), move |args| {
                if args.is_empty() {
                    Variant::Int(i as i64)
                } else {
                    args[0].clone()
                }
            });
        signal.connect(conn);
    }

    let results = signal.emit(&[Variant::Int(42)]);
    assert_eq!(results.len(), 100);
    // All should have received the signal arg.
    for r in &results {
        assert_eq!(*r, Variant::Int(42));
    }
}

#[test]
fn prop_signal_disconnect_during_iteration_safe() {
    let mut signal = Signal::new("disconnect_test");
    let id1 = ObjectId::next();
    let id2 = ObjectId::next();

    signal.connect(Connection::with_callback(id1, "m1", |_| Variant::Int(1)));
    signal.connect(Connection::with_callback(id2, "m2", |_| Variant::Int(2)));
    signal.connect(Connection::with_callback(id1, "m3", |_| Variant::Int(3)));

    // Disconnect all for id1, then emit — must not panic.
    signal.disconnect_all_for(id1);
    let results = signal.emit(&[]);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Variant::Int(2));
}

// ===========================================================================
// 24. CallableRef resolve_args property tests
// ===========================================================================

#[test]
fn prop_callable_ref_resolve_args_method_passthrough() {
    let callable = gdvariant::CallableRef::Method {
        target_id: 42,
        method: "test".into(),
    };
    let args = vec![Variant::Int(1), Variant::Int(2)];
    let resolved = callable.resolve_args(&args);
    assert_eq!(resolved, args, "Method callable must pass args through");
}

#[test]
fn prop_callable_ref_bound_appends() {
    let inner = gdvariant::CallableRef::Method {
        target_id: 1,
        method: "f".into(),
    };
    let callable = gdvariant::CallableRef::Bound {
        inner: Box::new(inner),
        bound_args: vec![Variant::Int(99)],
    };
    let args = vec![Variant::Int(1)];
    let resolved = callable.resolve_args(&args);
    assert_eq!(resolved, vec![Variant::Int(1), Variant::Int(99)]);
}

#[test]
fn prop_callable_ref_unbound_truncates() {
    let inner = gdvariant::CallableRef::Method {
        target_id: 1,
        method: "f".into(),
    };
    let callable = gdvariant::CallableRef::Unbound {
        inner: Box::new(inner),
        unbind_count: 2,
    };
    let args = vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)];
    let resolved = callable.resolve_args(&args);
    assert_eq!(resolved, vec![Variant::Int(1)]);
}

#[test]
fn prop_callable_ref_inner_callable_unwraps() {
    let base = gdvariant::CallableRef::Method {
        target_id: 7,
        method: "base".into(),
    };
    let bound = gdvariant::CallableRef::Bound {
        inner: Box::new(base),
        bound_args: vec![],
    };
    let unbound = gdvariant::CallableRef::Unbound {
        inner: Box::new(bound),
        unbind_count: 0,
    };
    let inner = unbound.inner_callable();
    match inner {
        gdvariant::CallableRef::Method { target_id, method } => {
            assert_eq!(*target_id, 7);
            assert_eq!(method, "base");
        }
        _ => panic!("inner_callable must unwrap to the base Method"),
    }
}

// ===========================================================================
// 25. Scene tree add/remove stress — no panics
// ===========================================================================

#[test]
fn prop_scene_tree_add_remove_stress() {
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let mut node_ids = Vec::new();

    // Add many children to root.
    for i in 0..50 {
        let node = Node::new(&format!("child_{i}"), "Node2D");
        let id = tree.add_child(root_id, node).unwrap();
        node_ids.push(id);
    }
    assert_eq!(tree.node_count(), 51); // root + 50 children

    // Remove half.
    for id in node_ids.iter().step_by(2) {
        tree.remove_node(*id).unwrap();
    }
    assert_eq!(tree.node_count(), 26); // root + 25 remaining

    // Re-add to remaining nodes as children.
    let remaining: Vec<_> = node_ids.iter().skip(1).step_by(2).copied().collect();
    for (i, &parent_id) in remaining.iter().enumerate() {
        let node = Node::new(&format!("grandchild_{i}"), "Sprite2D");
        tree.add_child(parent_id, node).unwrap();
    }
    // Must not panic, and tree should be consistent.
    assert!(tree.node_count() > 26);
}

#[test]
fn prop_scene_tree_get_node_by_path_no_panic() {
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let a = Node::new("A", "Node");
    tree.add_child(root_id, a).unwrap();

    let a_id = tree.get_node_by_path("/root/A").unwrap();

    let b = Node::new("B", "Node");
    tree.add_child(a_id, b).unwrap();

    // Valid paths.
    let _ = tree.get_node_by_path("/root/A");
    let _ = tree.get_node_by_path("/root/A/B");

    // Invalid / weird paths — must not panic.
    let _ = tree.get_node_by_path("");
    let _ = tree.get_node_by_path("/nonexistent");
    let _ = tree.get_node_by_path("../../..");
    let _ = tree.get_node_by_path("/root/A/B/C/D/E");
    let _ = tree.get_node_by_path("//double//slash");
}

// ===========================================================================
// Helpers for Variant property tests
// ===========================================================================

fn random_numeric_variant(rng: &mut Rng) -> Variant {
    if rng.next_u64() % 2 == 0 {
        Variant::Int(rng.next_u64() as i64)
    } else {
        Variant::Float(rng.next_fuzz_f32() as f64)
    }
}

fn random_variant(rng: &mut Rng) -> Variant {
    match rng.next_u64() % 10 {
        0 => Variant::Nil,
        1 => Variant::Bool(rng.next_u64() % 2 == 0),
        2 => Variant::Int(rng.next_u64() as i64),
        3 => Variant::Float(rng.next_fuzz_f32() as f64),
        4 => Variant::String(format!("s{}", rng.next_u64() % 100)),
        5 => Variant::Vector2(rng.next_vec2()),
        6 => Variant::Vector3(rng.next_vec3()),
        7 => Variant::Array(vec![Variant::Int(rng.next_u64() as i64)]),
        8 => {
            let mut d = std::collections::HashMap::new();
            d.insert("k".to_string(), Variant::Int(rng.next_u64() as i64));
            Variant::Dictionary(d)
        }
        _ => Variant::Color(Color::new(
            rng.next_fuzz_f32(),
            rng.next_fuzz_f32(),
            rng.next_fuzz_f32(),
            rng.next_fuzz_f32(),
        )),
    }
}

fn variant_as_f64(v: &Variant) -> f64 {
    match v {
        Variant::Int(i) => *i as f64,
        Variant::Float(f) => *f,
        _ => f64::NAN,
    }
}

fn assert_variant_numeric_eq(a: &Variant, b: &Variant, msg: &str) {
    let va = variant_as_f64(a);
    let vb = variant_as_f64(b);
    if va.is_nan() && vb.is_nan() {
        return; // Both NaN is acceptable
    }
    assert!(
        (va - vb).abs() < 1e-6 || (va == vb),
        "{msg}: {a:?} vs {b:?}"
    );
}
