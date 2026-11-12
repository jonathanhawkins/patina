//! pat-s3rrv.59 — `Vector4`/`Vector4i`'s remaining methods dispatch and answer
//! Godot's values.
//!
//! The bead's own note says these were "skipped, each needing a Godot semantic
//! read rather than guessed". Three of them are guessable-and-wrong, and each
//! wrong guess is the one a competent implementation reaches for first:
//!
//! * `posmod` is `Math::fposmod`, whose result takes the sign of the **divisor**
//!   (`core/math/math_funcs.h:279-286`). Rust's `rem_euclid` is the
//!   always-positive spelling — same idea, different answer, and they disagree
//!   on exactly the negative-divisor inputs a wrapping calculation uses.
//! * `snapped` is `floor(v/step + 0.5) * step` (`math_funcs.cpp:121-126`) —
//!   round-half-**up**, not `round()`'s half-away-from-zero. They differ on
//!   every negative half-step.
//! * `inverse` is the component-wise **reciprocal** (`vector4.cpp:199`), not a
//!   negation and not a normalise.
//!
//! ⚠️ Every expected value below was computed from a transcription of those
//! three C++ functions into Python that was run, not read off the Rust.

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

fn approx(got: Variant, want: [f32; 4], what: &str) {
    match got {
        Variant::Vector4(v) => {
            let d = [v.x - want[0], v.y - want[1], v.z - want[2], v.w - want[3]];
            assert!(
                d.iter().all(|c| c.abs() < 1e-5),
                "{what}: got ({}, {}, {}, {}), want {want:?}",
                v.x,
                v.y,
                v.z,
                v.w,
            );
        }
        other => panic!("{what}: expected a Vector4, got {other:?}"),
    }
}

/// ⚠️ `posmod` takes the sign of the DIVISOR.
///
/// `fposmod(5, -3)` is `-1`; `5f32.rem_euclid(-3.0)` is `2`. The positive
/// divisor case agrees with both spellings, which is why a fixture using only
/// positive divisors cannot tell them apart — this one uses both.
#[test]
fn posmod_takes_the_sign_of_the_divisor() {
    approx(
        eval("Vector4(5, -5, 7.5, 0).posmod(3)"),
        [2.0, 1.0, 1.5, 0.0],
        "posmod with a positive divisor",
    );
    approx(
        eval("Vector4(5, 5, 5, 5).posmod(-3)"),
        [-1.0, -1.0, -1.0, -1.0],
        "posmod with a NEGATIVE divisor — rem_euclid would say 2",
    );
    approx(
        eval("Vector4(5, -5, 8, -8).posmodv(Vector4(3, 3, -3, -3))"),
        [2.0, 1.0, -1.0, -2.0],
        "posmodv applies each component's own divisor",
    );
}

/// ⚠️ `snapped` rounds half UP, not half away from zero.
#[test]
fn snapped_rounds_half_up_not_half_away_from_zero() {
    approx(
        eval("Vector4(2.5, -2.5, 7.3, 0).snappedf(1)"),
        [3.0, -2.0, 7.0, 0.0],
        "-2.5 must snap to -2; `round()` would give -3",
    );
    approx(
        eval("Vector4(7.3, 7.3, 7.3, 7.3).snapped(Vector4(0.5, 1, 2, 5))"),
        [7.5, 7.0, 8.0, 5.0],
        "snapped uses each component's own step",
    );
    // A zero step returns the value untouched rather than dividing by zero.
    approx(
        eval("Vector4(1.7, 1.7, 1.7, 1.7).snappedf(0)"),
        [1.7, 1.7, 1.7, 1.7],
        "a zero step is a no-op",
    );
}

/// `inverse` is the reciprocal, and `clamp`/`clampf` are per-component.
#[test]
fn inverse_is_the_reciprocal_and_clamp_is_per_component() {
    approx(
        eval("Vector4(2, 4, 0.5, -1).inverse()"),
        [0.5, 0.25, 2.0, -1.0],
        "inverse is 1/x per component — not a negation, not a normalise",
    );
    approx(
        eval("Vector4(-5, 0.5, 9, 2).clampf(0, 1)"),
        [0.0, 0.5, 1.0, 1.0],
        "clampf clamps every component between the same scalars",
    );
    approx(
        eval("Vector4(-5, 0.5, 9, 2).clamp(Vector4(0, 0, 0, 0), Vector4(1, 2, 3, 4))"),
        [0.0, 0.5, 3.0, 2.0],
        "clamp uses each component's own bounds",
    );
    approx(
        eval("Vector4(-5, 0.5, 9, 2).minf(1)"),
        [-5.0, 0.5, 1.0, 1.0],
        "minf",
    );
    approx(
        eval("Vector4(-5, 0.5, 9, 2).maxf(1)"),
        [1.0, 1.0, 9.0, 2.0],
        "maxf",
    );
}

/// `Vector4i`'s integer surface, including the two distance accessors.
///
/// ⚠️ `distance_squared_to` returns an **Int**. Godot's signature is `int64_t`
/// (`core/math/vector4i.h:98`) because the squared length of an integer vector
/// is exact; handing back a Float loses that for the very comparison the method
/// exists to make cheap.
#[test]
fn vector4i_distances_snapping_and_scalar_clamps() {
    assert_eq!(
        eval("Vector4i(0, 0, 0, 0).distance_squared_to(Vector4i(3, 4, 0, 0))"),
        Variant::Int(25),
        "distance_squared_to must be an Int, not a Float",
    );
    match eval("Vector4i(0, 0, 0, 0).distance_to(Vector4i(3, 4, 0, 0))") {
        Variant::Float(d) => assert!((d - 5.0).abs() < 1e-5, "distance_to: {d}"),
        other => panic!("distance_to must be a Float, got {other:?}"),
    }

    // ⚠️ `snappedi(-5, 10)` is 0, not -10: the integer path promotes to double
    // and rounds half UP, so -0.5 floors to 0.
    assert_eq!(
        eval("Vector4i(-5, 5, 14, 0).snappedi(10)"),
        eval("Vector4i(0, 10, 10, 0)"),
        "-5 snaps UP to 0; a symmetric round-half-away would say -10",
    );
    assert_eq!(
        eval("Vector4i(-5, 9, 2, 7).clampi(0, 5)"),
        eval("Vector4i(0, 5, 2, 5)")
    );
    assert_eq!(
        eval("Vector4i(-5, 9, 2, 7).mini(3)"),
        eval("Vector4i(-5, 3, 2, 3)")
    );
    assert_eq!(
        eval("Vector4i(-5, 9, 2, 7).maxi(3)"),
        eval("Vector4i(3, 9, 3, 7)")
    );
}
