//! pat-s3rrv.59: `Rect2i` and `Projection` instance methods reach a script.
//!
//! Same dispatch-shaped gap the `Vector4` pair had: both kinds round-trip
//! through a `.tscn` and their members read, but `call_method` had no arm for
//! either, so `r.has_point(p)` hit the "cannot call method on Rect2i"
//! catch-all.
//!
//! These drive real GDScript, so a method table that exists but is not
//! delegated to still fails.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn eval(expr: &str) -> Variant {
    let src = format!("extends Node\n\nfunc check():\n    return {expr}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("`{expr}` failed: {e:?}"))
}

/// **The acceptance case**: Rect2i's methods dispatch and answer Godot's values.
#[test]
fn rect2i_instance_methods_dispatch() {
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).get_area()"),
        Variant::Int(16),
        "Godot spells this `get_area`; gdcore's method is `area`, and the \
         script-facing name has to be Godot's",
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).get_center()"),
        eval("Vector2i(2, 2)")
    );
    assert_eq!(eval("Rect2i(0, 0, 4, 4).has_area()"), Variant::Bool(true));
    assert_eq!(eval("Rect2i(0, 0, 0, 0).has_area()"), Variant::Bool(false));

    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).intersection(Rect2i(2, 2, 4, 4))"),
        eval("Rect2i(2, 2, 2, 2)"),
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).merge(Rect2i(2, 2, 4, 4))"),
        eval("Rect2i(0, 0, 6, 6)"),
    );
    assert_eq!(
        eval("Rect2i(0, 0, 10, 10).encloses(Rect2i(2, 2, 3, 3))"),
        Variant::Bool(true),
    );
    assert_eq!(
        eval("Rect2i(1, 1, 2, 2).grow(1)"),
        eval("Rect2i(0, 0, 4, 4)"),
        "grow moves the position back by the amount AND adds twice to the size",
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).expand(Vector2i(6, 6))"),
        eval("Rect2i(0, 0, 6, 6)"),
    );
}

/// 🔑 `has_point` EXCLUDES the far edge.
///
/// A rect at (0,0) sized 4×4 contains (3,3) and not (4,4) — the test is
/// `>= position && < end`. An implementation using `<=` passes every
/// interior-point check and only differs exactly on the boundary, which is
/// where tile and pixel maths actually lands.
#[test]
fn has_point_excludes_the_far_edge() {
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).has_point(Vector2i(0, 0))"),
        Variant::Bool(true),
        "the near corner IS inside",
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).has_point(Vector2i(3, 3))"),
        Variant::Bool(true),
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).has_point(Vector2i(4, 4))"),
        Variant::Bool(false),
        "…but the far corner is NOT — `end` is exclusive",
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).has_point(Vector2i(-1, 2))"),
        Variant::Bool(false),
    );
}

/// Two rects that miss entirely intersect to the EMPTY rect, not to a negative
/// or wrapped one.
#[test]
fn a_disjoint_intersection_is_empty() {
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).intersection(Rect2i(10, 10, 2, 2))"),
        eval("Rect2i(0, 0, 0, 0)"),
    );
    assert_eq!(
        eval("Rect2i(0, 0, 4, 4).intersects(Rect2i(10, 10, 2, 2))"),
        Variant::Bool(false),
    );
}

/// Projection's three backed instance methods.
///
/// ⚠️ The `is_orthogonal` line here is weak ON ITS OWN — `Projection()` is the
/// identity, which IS orthogonal, so a hardcoded `true` satisfies it. That is
/// deliberate now rather than a gap: the negative case lives in
/// [`a_perspective_projection_is_not_orthogonal`] below, which became
/// expressible once the `create_*` statics were wired. Keep both — this one
/// pins the identity, that one pins the discrimination.
#[test]
fn projection_instance_methods_dispatch() {
    match eval("Projection().determinant()") {
        Variant::Float(d) => assert!(
            (d - 1.0).abs() < 1e-6,
            "the identity projection has determinant 1, got {d}"
        ),
        other => panic!("expected Float, got {other:?}"),
    }
    assert_eq!(
        eval("Projection().inverse()"),
        eval("Projection()"),
        "the identity is its own inverse",
    );
    assert_eq!(
        eval("Projection().is_orthogonal()"),
        Variant::Bool(true),
        "identity has no perspective divide",
    );
}

/// 🔑 The negative case my previous pass could not express.
///
/// `is_orthogonal` was asserted only on the identity, which IS orthogonal — so
/// a hardcoded `true` would have passed. Now that the `create_*` statics are
/// reachable, a real perspective matrix answers `false`: `create_frustum` sets
/// `m[2][3] = -1` and leaves `m[3][3]` at 0, which is exactly what
/// `is_orthogonal` tests for.
#[test]
fn a_perspective_projection_is_not_orthogonal() {
    assert_eq!(
        eval("Projection.create_perspective(90.0, 1.6, 0.1, 100.0).is_orthogonal()"),
        Variant::Bool(false),
        "a perspective divide means not orthogonal — this is the case the \
         identity-only assertion could never fail on",
    );
    assert_eq!(
        eval("Projection.create_orthogonal(-1.0, 1.0, -1.0, 1.0, 0.1, 100.0).is_orthogonal()"),
        Variant::Bool(true),
    );
    assert_eq!(
        eval("Projection.create_frustum(-1.0, 1.0, -1.0, 1.0, 0.1, 100.0).is_orthogonal()"),
        Variant::Bool(false),
    );
}

/// 🔑 `flip_fov` reads INVERTED on `create_orthogonal_aspect`.
///
/// Godot's `set_orthogonal` does `if (!p_flip_fov) { p_size *= p_aspect; }`
/// (`projection.cpp:356-362`) — the widening happens when the flag is **false**.
/// Implementing it the intuitive way round produces a projection that is wrong
/// by exactly the aspect ratio, and every default-argument call still looks
/// right.
///
/// Observed through `proj[0].x`, which is `m[0][0] = 2/(right-left)`: with
/// size 2 and aspect 2 the default spans x∈[-2,2] giving 0.5, while the flipped
/// form spans x∈[-1,1] giving 1.0.
#[test]
fn orthogonal_aspect_flip_fov_is_inverted() {
    match eval("Projection.create_orthogonal_aspect(2.0, 2.0, 0.1, 100.0)[0]") {
        Variant::Vector4(c) => assert!(
            (c.x - 0.5).abs() < 1e-5,
            "default (flip_fov false) widens by aspect, so m[0][0] is 0.5, got {}",
            c.x
        ),
        other => panic!("indexing a Projection must yield a Vector4 column, got {other:?}"),
    }
    match eval("Projection.create_orthogonal_aspect(2.0, 2.0, 0.1, 100.0, true)[0]") {
        Variant::Vector4(c) => assert!(
            (c.x - 1.0).abs() < 1e-5,
            "flip_fov true does NOT widen, so m[0][0] is 1.0, got {}",
            c.x
        ),
        other => panic!("expected a Vector4 column, got {other:?}"),
    }
}

/// `flip_fov` on `create_perspective` reinterprets the angle as HORIZONTAL via
/// Godot's `get_fovy(fovx, 1/aspect)` (`projection.h:103`) — not by swapping the
/// aspect, which is the tempting shortcut and gives a different matrix.
///
/// With fov 90 and aspect 1.6, `tan(fovy/2) == tan(45°)/1.6 == 0.625`, so
/// `m[1][1]` goes from 1.0 to exactly 1.6.
#[test]
fn perspective_flip_fov_converts_the_angle() {
    match eval("Projection.create_perspective(90.0, 1.6, 0.1, 100.0)[1]") {
        Variant::Vector4(c) => assert!(
            (c.y - 1.0).abs() < 1e-5,
            "a 90 degree VERTICAL fov gives m[1][1] = 1.0, got {}",
            c.y
        ),
        other => panic!("expected a Vector4 column, got {other:?}"),
    }
    match eval("Projection.create_perspective(90.0, 1.6, 0.1, 100.0, true)[1]") {
        Variant::Vector4(c) => assert!(
            (c.y - 1.6).abs() < 1e-4,
            "…and read as HORIZONTAL it gives exactly the aspect, 1.6, got {}",
            c.y
        ),
        other => panic!("expected a Vector4 column, got {other:?}"),
    }
}

/// An unknown method names itself rather than answering Nil.
#[test]
fn unknown_methods_are_errors() {
    for (expr, class) in [
        ("Rect2i(0, 0, 1, 1).no_such()", "Rect2i"),
        ("Projection().no_such()", "Projection"),
    ] {
        let src = format!("extends Node\n\nfunc check():\n    return {expr}\n");
        let mut interp = Interpreter::new();
        let class_def = interp.run_class(&src).expect("script parses");
        let mut inst = interp
            .instantiate_class(&class_def)
            .expect("class instantiates");
        let err = interp
            .call_instance_method(&mut inst, "check", &[])
            .expect_err("an unknown method must raise");
        let text = format!("{err:?}");
        assert!(
            text.contains("no_such") && text.contains(class),
            "the error must name the class and method, got {text}",
        );
    }
}
