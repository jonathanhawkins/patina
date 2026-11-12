//! pat-s3rrv.59 — `Vector4.cubic_interpolate` / `cubic_interpolate_in_time`.
//!
//! ⚠️ A DELIBERATELY NEW FILE. Appending to an existing test binary silently
//! adopts every other test in it as this work's gate, and nextest cancels on the
//! first failure — so one pre-existing red assertion elsewhere sinks the report
//! and hides whether these tests even ran. That cost two beads earlier today.
//!
//! Godot ground truth: `core/math/vector4.cpp:147-163` applies
//! `Math::cubic_interpolate` (Catmull-Rom, `math_funcs.h:343-349`) and
//! `Math::cubic_interpolate_in_time` (Barry-Goldman) per component.
//!
//! The expected values were produced by re-transcribing both formulas into
//! Python and RUNNING it — not by printing what this implementation returns.

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

/// The four control points used throughout, in Godot's argument order:
/// `a.cubic_interpolate(b, pre_a, post_b, weight)`.
const A: &str = "Vector4(0, 1, 2, 3)";
const B: &str = "Vector4(4, 5, 6, 7)";
const PRE: &str = "Vector4(-1, 0, 1, 2)";
const POST: &str = "Vector4(8, 9, 10, 11)";

fn approx(got: &Variant, expect: [f32; 4], what: &str) {
    match got {
        Variant::Vector4(v) => {
            let d = [
                (v.x - expect[0]).abs(),
                (v.y - expect[1]).abs(),
                (v.z - expect[2]).abs(),
                (v.w - expect[3]).abs(),
            ];
            assert!(
                d.iter().all(|e| *e < 1e-4),
                "{what}: got ({}, {}, {}, {}), expected {expect:?}",
                v.x,
                v.y,
                v.z,
                v.w
            );
        }
        other => panic!("{what}: expected a Vector4, got {other:?}"),
    }
}

/// 🔑 THE ENDPOINTS ARE THE ARGUMENT-ORDER TEST.
///
/// Catmull-Rom passes exactly through its two middle control points: at
/// `weight == 0` the result is `self`, at `weight == 1` it is `b`. The method
/// takes `(b, pre_a, post_b, w)` while the scalar it delegates to takes
/// `(from, to, pre, post, w)` — so `b` and `pre_a` are easy to transpose, and a
/// transposition still yields a smooth curve through plausible values. It
/// cannot survive both endpoint assertions.
#[test]
fn cubic_interpolate_passes_through_its_endpoints() {
    approx(
        &eval(&format!("{A}.cubic_interpolate({B}, {PRE}, {POST}, 0.0)")),
        [0.0, 1.0, 2.0, 3.0],
        "weight 0 must return `self` exactly",
    );
    approx(
        &eval(&format!("{A}.cubic_interpolate({B}, {PRE}, {POST}, 1.0)")),
        [4.0, 5.0, 6.0, 7.0],
        "weight 1 must return `b` exactly",
    );
}

/// The midpoint value, which is where the basis actually shows.
///
/// An endpoint-only test passes against a plain `lerp`; 1.8125 is not the linear
/// midpoint (2.0), so this is what distinguishes the cubic from a straight line.
#[test]
fn cubic_interpolate_uses_the_catmull_rom_basis_not_a_lerp() {
    approx(
        &eval(&format!("{A}.cubic_interpolate({B}, {PRE}, {POST}, 0.5)")),
        [1.8125, 2.8125, 3.8125, 4.8125],
        "midpoint follows the Catmull-Rom basis",
    );
    // The linear midpoint would be 2.0 — proving the two differ here is what
    // makes the assertion above meaningful rather than coincidental.
    approx(
        &eval(&format!("{A}.lerp({B}, 0.5)")),
        [2.0, 3.0, 4.0, 5.0],
        "lerp's midpoint is different, so the cubic value is not a lerp",
    );
}

/// With UNIFORM times the Barry-Goldman form must agree with the plain cubic.
///
/// That equivalence is the cheapest correctness check available for `in_time`:
/// it ties a seven-argument formula back to a four-argument one already pinned
/// above, so an error in the time handling shows up as a disagreement.
#[test]
fn cubic_interpolate_in_time_agrees_with_the_plain_cubic_for_uniform_times() {
    approx(
        &eval(&format!(
            "{A}.cubic_interpolate_in_time({B}, {PRE}, {POST}, 0.5, 1.0, -1.0, 2.0)"
        )),
        [1.8125, 2.8125, 3.8125, 4.8125],
        "uniform times reduce to the Catmull-Rom result",
    );
}

/// ⚠️ THE DEGENERATE TIME SPANS, which is where a naive port breaks.
///
/// Godot guards five divisions, each falling back to a DIFFERENT constant
/// (`0.0`, `0.5`, `1.0`, `0.0`, `1.0`, `0.5`). A port that used one value for
/// all of them — or that let the division run and produced NaN — still behaves
/// correctly for evenly spaced keyframes and only breaks on the degenerate ones
/// animation actually produces.
#[test]
fn cubic_interpolate_in_time_handles_zero_length_spans() {
    // b_t == 0 exercises the `0.5` fallbacks.
    approx(
        &eval(&format!(
            "{A}.cubic_interpolate_in_time({B}, {PRE}, {POST}, 0.5, 0.0, -1.0, 2.0)"
        )),
        [2.0, 3.0, 4.0, 5.0],
        "b_t == 0 must fall back rather than divide by zero",
    );
    // pre_a_t == 0 exercises the first guard specifically.
    approx(
        &eval(&format!(
            "{A}.cubic_interpolate_in_time({B}, {PRE}, {POST}, 0.5, 1.0, 0.0, 2.0)"
        )),
        [1.25, 2.25, 3.25, 4.25],
        "pre_a_t == 0 must fall back rather than divide by zero",
    );
}
