//! pat-0q2kx.50 — the affine/math builtin Variant types are reachable from a
//! running script.
//!
//! `Rect2`, `Transform2D`, `Transform3D`, `Quaternion`, `Basis`, `AABB` and
//! `Plane` all had tested `gdcore` implementations and their own `Variant`
//! arms, but the GDScript VM could not reach a single one of them: no
//! constructor arm, no member get/set, no method dispatch, and no `*` operator
//! product. `Transform2D(0.0, Vector2(10, 5))` halted with `UndefinedFunction`.
//!
//! Every test here drives a scripted class through the same dispatch path the
//! runner uses for `_ready` / `_process` (`run_class` -> `instantiate_class` ->
//! `call_instance_method`, which evaluates the body through `eval_expr`), so a
//! pass proves VM wiring rather than the presence of a `gdcore` function.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

/// Runs `body` as the statements of a `check()` method. Taking the body as one
/// line per statement keeps the GDScript indentation out of the Rust literal,
/// where a `\`-continuation would silently eat it.
fn run(body: &[&str]) -> Variant {
    let mut src = String::from("extends Node\nfunc check():\n");
    for line in body {
        src.push_str("    ");
        src.push_str(line);
        src.push('\n');
    }
    call_method(&src, "check")
}

fn items(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    }
}

const EPS: f32 = 1e-4;

#[track_caller]
fn assert_vec2(got: &Variant, x: f32, y: f32, what: &str) {
    match got {
        Variant::Vector2(v) => assert!(
            (v.x - x).abs() < EPS && (v.y - y).abs() < EPS,
            "{what}: expected Vector2({x}, {y}), got Vector2({}, {})",
            v.x,
            v.y
        ),
        other => panic!("{what}: expected Vector2, got {other:?}"),
    }
}

#[track_caller]
fn assert_vec3(got: &Variant, x: f32, y: f32, z: f32, what: &str) {
    match got {
        Variant::Vector3(v) => assert!(
            (v.x - x).abs() < EPS && (v.y - y).abs() < EPS && (v.z - z).abs() < EPS,
            "{what}: expected Vector3({x}, {y}, {z}), got Vector3({}, {}, {})",
            v.x,
            v.y,
            v.z
        ),
        other => panic!("{what}: expected Vector3, got {other:?}"),
    }
}

#[track_caller]
fn assert_float(got: &Variant, want: f64, what: &str) {
    match got {
        Variant::Float(f) => assert!((f - want).abs() < 1e-4, "{what}: expected {want}, got {f}"),
        Variant::Int(i) => assert!(
            (*i as f64 - want).abs() < 1e-4,
            "{what}: expected {want}, got {i}"
        ),
        other => panic!("{what}: expected a number, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance criteria
// ---------------------------------------------------------------------------

/// The four snippets named in the bead, run end-to-end through the VM.
#[test]
fn variant_affine_types_runtime() {
    let got = items(run(&[
        "var t = Transform2D(0.0, Vector2(10, 5))",
        "t.origin.x += 3",
        "var q = Quaternion(Vector3.UP, PI / 2)",
        "var r = Rect2(0, 0, 10, 10)",
        "var a = AABB(Vector3(), Vector3(2, 2, 2))",
        "return [t * Vector2(1, 0), q.slerp(Quaternion(), 0.5), \
         r.has_point(Vector2(5, 5)), r.grow(2).size, a.get_center()]",
    ]));

    assert_eq!(got.len(), 5, "all five expressions must evaluate");

    // A transform with no rotation and origin (13, 5) after the `+= 3` moves
    // the point (1, 0) to (14, 5).
    assert_vec2(&got[0], 14.0, 5.0, "t * Vector2(1, 0)");

    // Half of a 90° turn about +Y is a 45° turn: (0, sin(pi/8), 0, cos(pi/8)).
    match &got[1] {
        Variant::Quaternion(q) => assert!(
            q.x.abs() < EPS
                && (q.y - (std::f32::consts::PI / 8.0).sin()).abs() < EPS
                && q.z.abs() < EPS
                && (q.w - (std::f32::consts::PI / 8.0).cos()).abs() < EPS,
            "q.slerp(Quaternion(), 0.5): expected a 45° turn about +Y, got {q:?}"
        ),
        other => panic!("q.slerp(...) must return a Quaternion, got {other:?}"),
    }

    assert_eq!(
        got[2],
        Variant::Bool(true),
        "Rect2(0, 0, 10, 10).has_point(Vector2(5, 5))"
    );
    // grow(2) pushes every side out by 2, so a 10×10 rect becomes 14×14.
    assert_vec2(&got[3], 14.0, 14.0, "r.grow(2).size");
    assert_vec3(&got[4], 1.0, 1.0, 1.0, "AABB(...).get_center()");
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// Each type builds from its Godot constructor overloads, including the
/// zero-argument default.
#[test]
fn constructors_cover_godot_overloads() {
    let got = items(run(&[
        "return [Rect2(), Rect2(Vector2(1, 2), Vector2(3, 4)), Rect2(1, 2, 3, 4), \
         Transform2D(), Transform2D(Vector2(2, 0), Vector2(0, 3), Vector2(4, 5)), \
         Transform3D(), Basis(), Quaternion(), AABB(), Plane(Vector3(0, 1, 0), 2.0), \
         Plane(Vector3(0, 0, 0), Vector3(0, 1, 0), Vector3(1, 0, 0))]",
    ]));

    assert_eq!(got.len(), 11);
    match &got[0] {
        Variant::Rect2(r) => assert!(
            r.position.x == 0.0 && r.size.x == 0.0,
            "Rect2() is the zero rect, got {r:?}"
        ),
        other => panic!("Rect2() must return a Rect2, got {other:?}"),
    }
    // The (position, size) and the four-scalar forms agree.
    assert_eq!(got[1], got[2], "Rect2(Vector2, Vector2) == Rect2(x,y,w,h)");
    match &got[3] {
        Variant::Transform2D(t) => assert!(
            t.x.x == 1.0 && t.y.y == 1.0 && t.origin.x == 0.0,
            "Transform2D() is the identity, got {t:?}"
        ),
        other => panic!("Transform2D() must return a Transform2D, got {other:?}"),
    }
    match &got[4] {
        Variant::Transform2D(t) => assert!(
            t.x.x == 2.0 && t.y.y == 3.0 && t.origin.x == 4.0 && t.origin.y == 5.0,
            "Transform2D(x, y, origin) keeps its columns, got {t:?}"
        ),
        other => panic!("expected Transform2D, got {other:?}"),
    }
    assert!(matches!(got[5], Variant::Transform3D(_)));
    assert!(matches!(got[6], Variant::Basis(_)));
    match &got[7] {
        Variant::Quaternion(q) => assert!(
            q.w == 1.0 && q.x == 0.0,
            "Quaternion() is the identity, got {q:?}"
        ),
        other => panic!("expected Quaternion, got {other:?}"),
    }
    assert!(matches!(got[8], Variant::Aabb(_)));
    match &got[9] {
        Variant::Plane(p) => assert!(
            p.normal.y == 1.0 && p.d == 2.0,
            "Plane(normal, d) keeps both, got {p:?}"
        ),
        other => panic!("expected Plane, got {other:?}"),
    }
    // Three points on the XY plane through the origin: the normal is ±Z, d = 0.
    match &got[10] {
        Variant::Plane(p) => assert!(
            p.normal.x.abs() < EPS && p.normal.y.abs() < EPS && p.normal.z.abs() == 1.0,
            "Plane(a, b, c) must derive a ±Z normal, got {p:?}"
        ),
        other => panic!("expected Plane, got {other:?}"),
    }
}

/// The zero-argument `Vector2()` / `Vector3()` / `Color()` defaults that nested
/// constructor calls rely on.
#[test]
fn zero_arg_vector_and_color_constructors() {
    let got = items(run(&["return [Vector2(), Vector3(), Color()]"]));
    assert_vec2(&got[0], 0.0, 0.0, "Vector2()");
    assert_vec3(&got[1], 0.0, 0.0, 0.0, "Vector3()");
    match &got[2] {
        Variant::Color(c) => assert!(
            c.r == 0.0 && c.g == 0.0 && c.b == 0.0 && c.a == 1.0,
            "Color() is opaque black, got {c:?}"
        ),
        other => panic!("expected Color, got {other:?}"),
    }
}

/// Static constants resolve on the type name.
#[test]
fn static_constants_resolve() {
    let got = items(run(&[
        "return [Transform2D.IDENTITY, Transform3D.IDENTITY, Basis.IDENTITY, \
         Quaternion.IDENTITY, Plane.PLANE_XY]",
    ]));
    assert!(matches!(got[0], Variant::Transform2D(_)));
    assert!(matches!(got[1], Variant::Transform3D(_)));
    assert!(matches!(got[2], Variant::Basis(_)));
    match &got[3] {
        Variant::Quaternion(q) => assert_eq!((q.x, q.y, q.z, q.w), (0.0, 0.0, 0.0, 1.0)),
        other => panic!("expected Quaternion, got {other:?}"),
    }
    match &got[4] {
        Variant::Plane(p) => assert_eq!((p.normal.z, p.d), (1.0, 0.0)),
        other => panic!("expected Plane, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------

/// Every type's members read back the value it was built with.
#[test]
fn members_read_on_every_type() {
    let got = items(run(&[
        "var r = Rect2(1, 2, 10, 20)",
        "var t2 = Transform2D(Vector2(2, 0), Vector2(0, 3), Vector2(4, 5))",
        "var b = Basis()",
        "var t3 = Transform3D(b, Vector3(7, 8, 9))",
        "var q = Quaternion(0.1, 0.2, 0.3, 0.4)",
        "var a = AABB(Vector3(1, 1, 1), Vector3(2, 4, 6))",
        "var p = Plane(Vector3(0, 1, 0), 3.0)",
        "return [r.position, r.size, r.end, t2.x, t2.y, t2.origin, t3.basis, \
         t3.origin, q.x, q.w, a.position, a.end, p.normal, p.d]",
    ]));

    assert_vec2(&got[0], 1.0, 2.0, "Rect2.position");
    assert_vec2(&got[1], 10.0, 20.0, "Rect2.size");
    assert_vec2(&got[2], 11.0, 22.0, "Rect2.end");
    assert_vec2(&got[3], 2.0, 0.0, "Transform2D.x");
    assert_vec2(&got[4], 0.0, 3.0, "Transform2D.y");
    assert_vec2(&got[5], 4.0, 5.0, "Transform2D.origin");
    assert!(matches!(got[6], Variant::Basis(_)), "Transform3D.basis");
    assert_vec3(&got[7], 7.0, 8.0, 9.0, "Transform3D.origin");
    assert_float(&got[8], 0.1, "Quaternion.x");
    assert_float(&got[9], 0.4, "Quaternion.w");
    assert_vec3(&got[10], 1.0, 1.0, 1.0, "AABB.position");
    assert_vec3(&got[11], 3.0, 5.0, 7.0, "AABB.end");
    assert_vec3(&got[12], 0.0, 1.0, 0.0, "Plane.normal");
    assert_float(&got[13], 3.0, "Plane.d");
}

/// Member writes go back into the value, including the nested
/// `value.member.component` form that needs two read-modify-write hops.
#[test]
fn members_write_back_into_the_value() {
    let got = items(run(&[
        "var r = Rect2(0, 0, 4, 4)",
        "r.size = Vector2(8, 9)",
        "r.position.y = 2",
        "var t = Transform2D()",
        "t.origin = Vector2(1, 1)",
        "t.origin.x += 3",
        "var q = Quaternion()",
        "q.w = 0.5",
        "var a = AABB()",
        "a.size = Vector3(2, 2, 2)",
        "var p = Plane(Vector3(1, 0, 0), 0.0)",
        "p.d = 4.0",
        "var t3 = Transform3D()",
        "t3.origin.z -= 2",
        "return [r.size, r.position, t.origin, q.w, a.size, p.d, t3.origin]",
    ]));

    assert_vec2(&got[0], 8.0, 9.0, "Rect2.size = ...");
    assert_vec2(&got[1], 0.0, 2.0, "Rect2.position.y = 2");
    assert_vec2(&got[2], 4.0, 1.0, "Transform2D.origin.x += 3");
    assert_float(&got[3], 0.5, "Quaternion.w = 0.5");
    assert_vec3(&got[4], 2.0, 2.0, 2.0, "AABB.size = ...");
    assert_float(&got[5], 4.0, "Plane.d = 4.0");
    assert_vec3(&got[6], 0.0, 0.0, -2.0, "Transform3D.origin.z -= 2");
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

#[test]
fn rect2_method_surface() {
    let got = items(run(&[
        "var r = Rect2(0, 0, 10, 10)",
        "return [r.get_center(), r.get_area(), r.has_area(), \
         r.has_point(Vector2(11, 1)), r.encloses(Rect2(1, 1, 2, 2)), \
         r.intersects(Rect2(5, 5, 10, 10)), r.intersection(Rect2(5, 5, 10, 10)).size, \
         r.merge(Rect2(10, 10, 5, 5)).size, r.expand(Vector2(-5, 0)).position, \
         r.grow(2).position, r.grow_individual(1, 0, 0, 0).size, \
         r.grow_side(SIDE_LEFT, 5).position, Rect2(10, 10, -4, -4).abs().position]",
    ]));

    assert_vec2(&got[0], 5.0, 5.0, "get_center");
    assert_float(&got[1], 100.0, "get_area");
    assert_eq!(got[2], Variant::Bool(true), "has_area");
    assert_eq!(got[3], Variant::Bool(false), "has_point outside");
    assert_eq!(got[4], Variant::Bool(true), "encloses");
    assert_eq!(got[5], Variant::Bool(true), "intersects");
    assert_vec2(&got[6], 5.0, 5.0, "intersection size");
    assert_vec2(&got[7], 15.0, 15.0, "merge size");
    assert_vec2(&got[8], -5.0, 0.0, "expand position");
    assert_vec2(&got[9], -2.0, -2.0, "grow position");
    assert_vec2(&got[10], 11.0, 10.0, "grow_individual size");
    assert_vec2(&got[11], -5.0, 0.0, "grow_side(SIDE_LEFT) position");
    assert_vec2(&got[12], 6.0, 6.0, "abs position of a negative-size rect");
}

#[test]
fn transform2d_method_surface() {
    let got = items(run(&[
        "var t = Transform2D(0.0, Vector2(10, 5))",
        "var s = Transform2D(Vector2(2, 0), Vector2(0, 2), Vector2())",
        "return [t.affine_inverse().origin, t.basis_xform(Vector2(1, 2)), \
         t.translated(Vector2(1, 1)).origin, s.get_scale(), t.get_rotation(), \
         s.determinant(), t.get_origin(), s.orthonormalized().get_scale(), \
         t.interpolate_with(Transform2D(0.0, Vector2(20, 5)), 0.5).origin]",
    ]));

    // Undoing a pure translation moves the origin to its negation.
    assert_vec2(&got[0], -10.0, -5.0, "affine_inverse origin");
    // basis_xform ignores the translation.
    assert_vec2(&got[1], 1.0, 2.0, "basis_xform");
    assert_vec2(&got[2], 11.0, 6.0, "translated origin");
    assert_vec2(&got[3], 2.0, 2.0, "get_scale");
    assert_float(&got[4], 0.0, "get_rotation");
    assert_float(&got[5], 4.0, "determinant");
    assert_vec2(&got[6], 10.0, 5.0, "get_origin");
    assert_vec2(&got[7], 1.0, 1.0, "orthonormalized scale");
    assert_vec2(&got[8], 15.0, 5.0, "interpolate_with origin");
}

#[test]
fn transform3d_and_basis_method_surface() {
    let got = items(run(&[
        "var t = Transform3D(Basis(), Vector3(1, 2, 3))",
        "var b = Basis(Vector3(2, 0, 0), Vector3(0, 3, 0), Vector3(0, 0, 4))",
        "return [t.inverse().origin, t.translated(Vector3(1, 0, 0)).origin, \
         t.xform(Vector3(1, 1, 1)), \
         t.interpolate_with(Transform3D(Basis(), Vector3(3, 2, 3)), 0.5).origin, \
         b.determinant(), b.get_scale(), b.transposed().x, \
         b.xform(Vector3(1, 1, 1)), b.inverse().xform(Vector3(2, 3, 4))]",
    ]));

    assert_vec3(&got[0], -1.0, -2.0, -3.0, "Transform3D.inverse origin");
    assert_vec3(&got[1], 2.0, 2.0, 3.0, "translated origin");
    assert_vec3(&got[2], 2.0, 3.0, 4.0, "xform");
    assert_vec3(&got[3], 2.0, 2.0, 3.0, "interpolate_with origin");
    assert_float(&got[4], 24.0, "Basis.determinant");
    assert_vec3(&got[5], 2.0, 3.0, 4.0, "Basis.get_scale");
    assert_vec3(&got[6], 2.0, 0.0, 0.0, "Basis.transposed.x");
    assert_vec3(&got[7], 2.0, 3.0, 4.0, "Basis.xform");
    assert_vec3(&got[8], 1.0, 1.0, 1.0, "Basis.inverse.xform");
}

#[test]
fn quaternion_method_surface() {
    let got = items(run(&[
        "var q = Quaternion(Vector3(0, 1, 0), PI / 2)",
        "return [q.length(), q.is_normalized(), \
         q.inverse().xform(q.xform(Vector3(1, 0, 0))), q.dot(q), q.get_angle(), \
         q.get_axis(), q.xform(Vector3(1, 0, 0)), q.angle_to(Quaternion())]",
    ]));

    assert_float(&got[0], 1.0, "length");
    assert_eq!(got[1], Variant::Bool(true), "is_normalized");
    // Rotating and then un-rotating is the identity.
    assert_vec3(&got[2], 1.0, 0.0, 0.0, "inverse ∘ xform");
    assert_float(&got[3], 1.0, "dot with itself");
    assert_float(&got[4], (std::f32::consts::PI / 2.0) as f64, "get_angle");
    assert_vec3(&got[5], 0.0, 1.0, 0.0, "get_axis");
    // A quarter turn about +Y takes +X to -Z.
    assert_vec3(&got[6], 0.0, 0.0, -1.0, "xform(+X)");
    assert_float(
        &got[7],
        (std::f32::consts::PI / 2.0) as f64,
        "angle_to identity",
    );
}

#[test]
fn aabb_and_plane_method_surface() {
    let got = items(run(&[
        "var a = AABB(Vector3(0, 0, 0), Vector3(2, 2, 2))",
        "var p = Plane(Vector3(0, 1, 0), 1.0)",
        "return [a.get_center(), a.get_volume(), a.has_volume(), \
         a.has_point(Vector3(1, 1, 1)), \
         a.intersects(AABB(Vector3(1, 1, 1), Vector3(2, 2, 2))), a.grow(1).size, \
         a.merge(AABB(Vector3(2, 2, 2), Vector3(2, 2, 2))).size, a.get_end(), \
         p.distance_to(Vector3(0, 4, 0)), p.is_point_over(Vector3(0, 4, 0)), \
         p.project(Vector3(0, 4, 0)), \
         p.intersects_ray(Vector3(0, 4, 0), Vector3(0, -1, 0)), \
         p.intersects_ray(Vector3(0, 4, 0), Vector3(0, 1, 0))]",
    ]));

    assert_vec3(&got[0], 1.0, 1.0, 1.0, "AABB.get_center");
    assert_float(&got[1], 8.0, "get_volume");
    assert_eq!(got[2], Variant::Bool(true), "has_volume");
    assert_eq!(got[3], Variant::Bool(true), "has_point");
    assert_eq!(got[4], Variant::Bool(true), "intersects");
    assert_vec3(&got[5], 4.0, 4.0, 4.0, "grow size");
    assert_vec3(&got[6], 4.0, 4.0, 4.0, "merge size");
    assert_vec3(&got[7], 2.0, 2.0, 2.0, "get_end");
    assert_float(&got[8], 3.0, "Plane.distance_to");
    assert_eq!(got[9], Variant::Bool(true), "is_point_over");
    assert_vec3(&got[10], 0.0, 1.0, 0.0, "project");
    assert_vec3(&got[11], 0.0, 1.0, 0.0, "intersects_ray hit");
    // Godot returns null when the ray points away from the plane.
    assert_eq!(got[12], Variant::Nil, "intersects_ray miss");
}

// ---------------------------------------------------------------------------
// Operators and type checks
// ---------------------------------------------------------------------------

/// `*` applies a transform to a point and composes two transforms, in Godot's
/// right-to-left order.
#[test]
fn multiplication_transforms_and_composes() {
    let got = items(run(&[
        "var move = Transform2D(0.0, Vector2(10, 0))",
        "var scale = Transform2D(Vector2(2, 0), Vector2(0, 2), Vector2())",
        "var t3 = Transform3D(Basis(), Vector3(0, 5, 0))",
        "var q = Quaternion(Vector3(0, 1, 0), PI / 2)",
        "var b = Basis(Vector3(2, 0, 0), Vector3(0, 2, 0), Vector3(0, 0, 2))",
        "return [move * Vector2(1, 1), (scale * move) * Vector2(1, 0), \
         (move * scale) * Vector2(1, 0), t3 * Vector3(1, 1, 1), \
         q * Vector3(1, 0, 0), b * Vector3(1, 2, 3), (q * Quaternion()).w, \
         (t3 * t3).origin]",
    ]));

    assert_vec2(&got[0], 11.0, 1.0, "transform * point");
    // scale ∘ move: translate first, then scale -> (10 + 1) * 2 = 22.
    assert_vec2(&got[1], 22.0, 0.0, "(scale * move) * point");
    // move ∘ scale: scale first, then translate -> 1 * 2 + 10 = 12.
    assert_vec2(&got[2], 12.0, 0.0, "(move * scale) * point");
    assert_vec3(&got[3], 1.0, 6.0, 1.0, "Transform3D * point");
    assert_vec3(&got[4], 0.0, 0.0, -1.0, "Quaternion * vector");
    assert_vec3(&got[5], 2.0, 4.0, 6.0, "Basis * vector");
    assert_float(
        &got[6],
        std::f32::consts::FRAC_PI_4.cos() as f64,
        "Quaternion * identity keeps w",
    );
    assert_vec3(&got[7], 0.0, 10.0, 0.0, "Transform3D * Transform3D");
}

/// `is` recognises each of the seven types.
#[test]
fn is_operator_recognises_the_math_builtins() {
    let got = items(run(&[
        "return [Rect2() is Rect2, Transform2D() is Transform2D, \
         Transform3D() is Transform3D, Quaternion() is Quaternion, \
         Basis() is Basis, AABB() is AABB, \
         Plane(Vector3(0, 1, 0), 0.0) is Plane, Vector2() is Rect2]",
    ]));

    for (i, v) in got.iter().take(7).enumerate() {
        assert_eq!(*v, Variant::Bool(true), "type check {i} must hold");
    }
    assert_eq!(got[7], Variant::Bool(false), "a Vector2 is not a Rect2");
}

/// An unknown method on a math builtin reports the type it was called on
/// rather than silently returning null.
#[test]
fn unknown_method_is_an_error() {
    let src = "extends Node\nfunc check():\n    return Rect2().no_such_method()\n";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let err = interp
        .call_instance_method(&mut inst, "check", &[])
        .expect_err("an unknown Rect2 method must fail");
    let text = format!("{err}");
    assert!(
        text.contains("Rect2.no_such_method"),
        "error must name the method on its type, got: {text}"
    );
}
