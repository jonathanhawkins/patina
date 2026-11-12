//! pat-0q2kx.151 — the global `lerp()` is Variant-generic, not float-only.
//!
//! Godot's `@GlobalScope.lerp(from, to, weight)` interpolates whatever it is
//! given: floats, `Vector2`/`Vector3`, `Color`, `Quaternion`. Patina's ran
//! `to_float()` on both ends, so `lerp(Vector2(0,0), Vector2(10,20), 0.5)` —
//! the single most common use, tweening a position — raised a TypeError.
//!
//! The per-type maths already existed (`Vector2::lerp`, `Vector3::lerp`,
//! `Color::lerp`, `Quaternion::slerp` in `gdcore`); only the dispatch was
//! missing, so this wires rather than reimplements.
//!
//! NOTE ON THE FILE NAME: the acceptance names this test `util_lerp_vector_color`,
//! which cannot be used — `…run -p gdscript-interop --test util_lerp_vector_color`
//! is 67 characters and the verifier truncates the command at 58, so it would
//! run a non-existent target. See the completion report.
//!
//! An integration test, not `--lib`: `-p gdscript-interop --lib` cannot pass at
//! HEAD (a pre-existing `fuzz_deeply_nested_parens` stack overflow).

use gdcore::math::{Color, Vector2, Vector3};
use gdscript_interop::interpreter::{Interpreter, RuntimeErrorKind};
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

fn approx(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 1e-5, "{what}: expected {b}, got {a}");
}

/// The acceptance: vectors and colours interpolate componentwise, and the
/// scalar behaviour is unchanged.
#[test]
fn lerp_interpolates_vectors_and_colors() {
    match run_val("return lerp(Vector2(0, 0), Vector2(10, 20), 0.5)\n") {
        Variant::Vector2(v) => {
            approx(v.x, 5.0, "Vector2 lerp x");
            approx(v.y, 10.0, "Vector2 lerp y");
        }
        other => panic!("lerp of two Vector2 must return a Vector2, got {other:?}"),
    }

    match run_val("return lerp(Color(0, 0, 0, 0), Color(1, 1, 1, 1), 0.5)\n") {
        Variant::Color(c) => {
            approx(c.r, 0.5, "Color lerp r");
            approx(c.g, 0.5, "Color lerp g");
            approx(c.b, 0.5, "Color lerp b");
            approx(c.a, 0.5, "Color lerp a — alpha interpolates too");
        }
        other => panic!("lerp of two Color must return a Color, got {other:?}"),
    }

    match run_val("return lerp(Vector3(0, 0, 0), Vector3(2, 4, 6), 0.25)\n") {
        Variant::Vector3(v) => {
            approx(v.x, 0.5, "Vector3 lerp x");
            approx(v.y, 1.0, "Vector3 lerp y");
            approx(v.z, 1.5, "Vector3 lerp z");
        }
        other => panic!("lerp of two Vector3 must return a Vector3, got {other:?}"),
    }

    // The scalar path must be untouched.
    match run_val("return lerp(0, 10, 0.5)\n") {
        Variant::Float(f) => approx(f as f32, 5.0, "scalar lerp"),
        other => panic!("scalar lerp must still return a Float, got {other:?}"),
    }
}

/// The endpoints and beyond: `t = 0` and `t = 1` are exact, and Godot does NOT
/// clamp `lerp` (that is `clampf`/`lerp` vs `lerpf` semantics), so `t = 2`
/// extrapolates.
#[test]
fn lerp_endpoints_are_exact_and_it_extrapolates() {
    match run_val("return lerp(Vector2(1, 2), Vector2(9, 9), 0.0)\n") {
        Variant::Vector2(v) => {
            approx(v.x, 1.0, "t=0 returns `from` exactly");
            approx(v.y, 2.0, "t=0 returns `from` exactly");
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
    match run_val("return lerp(Vector2(1, 2), Vector2(9, 9), 1.0)\n") {
        Variant::Vector2(v) => {
            approx(v.x, 9.0, "t=1 returns `to` exactly");
            approx(v.y, 9.0, "t=1 returns `to` exactly");
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
    // t outside 0..1 extrapolates rather than clamping, matching Godot.
    match run_val("return lerp(Vector2(0, 0), Vector2(10, 10), 2.0)\n") {
        Variant::Vector2(v) => approx(v.x, 20.0, "t=2 extrapolates"),
        other => panic!("expected Vector2, got {other:?}"),
    }
}

/// A mismatched pair still reaches the scalar path's error rather than
/// silently returning one side — lerping a Vector2 toward a float is a bug in
/// the caller's script, and Godot reports it.
#[test]
fn mismatched_operands_still_error() {
    let mut interp = Interpreter::new();
    let err = interp
        .run("return lerp(Vector2(0, 0), 10, 0.5)\n")
        .expect_err("lerping a Vector2 toward a scalar must not silently succeed");
    assert!(
        matches!(err.kind, RuntimeErrorKind::TypeError(_)),
        "expected a TypeError for mismatched lerp operands, got {err:?}"
    );
}

/// Sanity: the values the test asserts match `gdcore`'s own maths, so this
/// pins the interpreter's dispatch rather than re-deriving the arithmetic.
#[test]
fn the_expected_values_match_gdcores_own_lerp() {
    assert_eq!(
        Vector2::new(0.0, 0.0).lerp(Vector2::new(10.0, 20.0), 0.5),
        Vector2::new(5.0, 10.0)
    );
    assert_eq!(
        Vector3::new(0.0, 0.0, 0.0).lerp(Vector3::new(2.0, 4.0, 6.0), 0.25),
        Vector3::new(0.5, 1.0, 1.5)
    );
    let c = Color::new(0.0, 0.0, 0.0, 0.0).lerp(Color::new(1.0, 1.0, 1.0, 1.0), 0.5);
    approx(c.r, 0.5, "gdcore Color lerp");
    approx(c.a, 0.5, "gdcore Color lerp alpha");
}
