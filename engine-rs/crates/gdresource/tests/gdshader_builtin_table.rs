//! pat-s3rrv.36 (slice 3): the builtin function table Godot defines.
//!
//! `call_builtin` covered 24 names. A shader calling anything else — `exp`,
//! `cross`, `reflect`, `lessThan` — raised "unknown function", and because the
//! renderers' line matcher silently skipped statements it could not classify,
//! that surfaced as a shader which simply did nothing.
//!
//! Every case here goes through `parse_shader` + `eval_function`, i.e. real
//! dispatch, rather than calling the table directly.

use std::collections::HashMap;

use gdresource::shader_ast::{eval_function, parse_shader, ShaderValue, StageContext};

/// Runs `fragment()` over `body` and returns what it wrote to `OUT`.
fn eval(body: &str) -> ShaderValue {
    let src = format!("shader_type canvas_item;\nvoid fragment() {{\n{body}\n}}\n");
    let program = parse_shader(&src).unwrap_or_else(|e| panic!("parse failed: {e:?}\n{src}"));
    let mut builtins = HashMap::new();
    builtins.insert("OUT".to_string(), ShaderValue::Float(0.0));
    let mut ctx = StageContext::new(HashMap::new(), builtins);
    eval_function(&program, "fragment", &mut ctx)
        .unwrap_or_else(|e| panic!("eval failed: {e:?}\n{src}"));
    *ctx.builtins.get("OUT").expect("OUT is written")
}

fn scalar(body: &str) -> f32 {
    eval(body).as_f32()
}

#[test]
fn transcendental_scalars() {
    assert!((scalar("\tOUT = exp(0.0);") - 1.0).abs() < 1e-6);
    assert!((scalar("\tOUT = exp2(3.0);") - 8.0).abs() < 1e-6);
    assert!((scalar("\tOUT = log(1.0);")).abs() < 1e-6);
    assert!((scalar("\tOUT = log2(8.0);") - 3.0).abs() < 1e-6);
    assert!((scalar("\tOUT = inversesqrt(4.0);") - 0.5).abs() < 1e-6);
    assert!((scalar("\tOUT = asin(0.0);")).abs() < 1e-6);
    assert!((scalar("\tOUT = acos(1.0);")).abs() < 1e-6);
    assert!((scalar("\tOUT = trunc(1.7);") - 1.0).abs() < 1e-6);
    assert!((scalar("\tOUT = degrees(0.0);")).abs() < 1e-6);
}

/// GLSL rounds halfway cases to EVEN. Rust's `f32::round` rounds away from
/// zero, so a naive `unary(f32::round)` would give `round(0.5) == 1.0` where
/// GLSL says 0.0. This is the case that makes the difference observable.
#[test]
fn round_uses_glsl_round_half_to_even() {
    assert_eq!(scalar("\tOUT = round(0.5);"), 0.0, "0.5 rounds to even → 0");
    assert_eq!(scalar("\tOUT = round(1.5);"), 2.0, "1.5 rounds to even → 2");
    assert_eq!(scalar("\tOUT = round(2.5);"), 2.0, "2.5 rounds to even → 2");
    assert_eq!(
        scalar("\tOUT = round(1.4);"),
        1.0,
        "ordinary case unaffected"
    );
    assert_eq!(
        scalar("\tOUT = round(-0.5);"),
        0.0,
        "-0.5 rounds to even → -0"
    );
}

/// Godot spells two-argument arctangent `atan(y, x)`; the one-argument form
/// must keep working alongside it.
#[test]
fn atan_has_both_arities() {
    assert!((scalar("\tOUT = atan(0.0);")).abs() < 1e-6, "1-arg atan");
    let quarter = scalar("\tOUT = atan(1.0, 1.0);");
    assert!(
        (quarter - std::f32::consts::FRAC_PI_4).abs() < 1e-6,
        "atan(1,1) is pi/4, got {quarter}"
    );
}

#[test]
fn geometric_builtins() {
    assert!((scalar("\tOUT = distance(vec2(0.0, 0.0), vec2(3.0, 4.0));") - 5.0).abs() < 1e-6);

    match eval("\tOUT = cross(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0));") {
        ShaderValue::Vec3(v) => {
            assert!(
                (v[2] - 1.0).abs() < 1e-6 && v[0].abs() < 1e-6 && v[1].abs() < 1e-6,
                "x cross y is +z, got {v:?}"
            );
        }
        other => panic!("cross must return a vec3, got {other:?}"),
    }

    // Straight down onto a floor bounces straight up.
    match eval("\tOUT = reflect(vec3(0.0, -1.0, 0.0), vec3(0.0, 1.0, 0.0));") {
        ShaderValue::Vec3(v) => assert!(
            (v[1] - 1.0).abs() < 1e-6,
            "reflect off a +y normal flips y, got {v:?}"
        ),
        other => panic!("reflect must return a vec3, got {other:?}"),
    }
}

/// Total internal reflection must return the zero vector, as GLSL specifies —
/// not a NaN from the negative square root.
#[test]
fn refract_returns_zero_on_total_internal_reflection() {
    match eval("\tOUT = refract(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), 4.0);") {
        ShaderValue::Vec3(v) => assert!(
            v.iter().all(|c| c.abs() < 1e-6),
            "total internal reflection is the zero vector, got {v:?}"
        ),
        other => panic!("refract must return a vec3, got {other:?}"),
    }
}

/// Comparison builtins produce a boolean vector that `any`/`all` collapse.
/// Testing them together is the point — a bool vector is only useful as the
/// argument to one of those.
#[test]
fn comparisons_feed_any_and_all() {
    assert_eq!(
        eval("\tOUT = any(lessThan(vec3(0.0, 5.0, 5.0), vec3(1.0, 1.0, 1.0)));"),
        ShaderValue::Bool(true),
        "one component is less, so any() is true"
    );
    assert_eq!(
        eval("\tOUT = all(lessThan(vec3(0.0, 5.0, 5.0), vec3(1.0, 1.0, 1.0)));"),
        ShaderValue::Bool(false),
        "not all components are less, so all() is false"
    );
    assert_eq!(
        eval("\tOUT = all(greaterThan(vec2(2.0, 3.0), vec2(1.0, 1.0)));"),
        ShaderValue::Bool(true)
    );
    assert_eq!(
        eval("\tOUT = any(not(vec2(1.0, 1.0)));"),
        ShaderValue::Bool(false),
        "not() of all-nonzero is all-false"
    );
}

/// Screen-space derivatives have no neighbouring invocation in a per-fragment
/// CPU evaluator, so they are 0 rather than an error — a shader using `fwidth`
/// for antialiasing still runs, it just gets no gradient.
#[test]
fn derivatives_are_zero_not_an_error() {
    assert_eq!(scalar("\tOUT = dFdx(5.0);"), 0.0);
    assert_eq!(scalar("\tOUT = dFdy(5.0);"), 0.0);
    assert_eq!(scalar("\tOUT = fwidth(5.0);"), 0.0);
}

/// A genuinely unknown name is still an error. Without this, the additions
/// above could have been made by making `call_builtin` return 0 for everything.
#[test]
fn an_unknown_function_is_still_an_error() {
    let src =
        "shader_type canvas_item;\nvoid fragment() {\n\tOUT = definitely_not_a_builtin(1.0);\n}\n";
    let program = parse_shader(src).expect("parses");
    let mut builtins = HashMap::new();
    builtins.insert("OUT".to_string(), ShaderValue::Float(0.0));
    let mut ctx = StageContext::new(HashMap::new(), builtins);
    assert!(
        eval_function(&program, "fragment", &mut ctx).is_err(),
        "an undefined function must raise, not silently evaluate to zero"
    );
}

// ---------------------------------------------------------------------------
// Top-level declarations the evaluator tolerates (pat-s3rrv.36)
// ---------------------------------------------------------------------------

/// A `struct` declaration must not sink the whole shader.
///
/// `ShaderToken::Struct` existed but `parse_program` had no arm for it, so it
/// hit the "unexpected token at top level" error and the ENTIRE shader failed to
/// parse — taking a perfectly good `fragment()` with it. The struct itself is
/// not modelled (the value model has no struct type); it is skipped, which is
/// honest about what this evaluator covers.
#[test]
fn a_top_level_struct_does_not_break_the_shader() {
    let src = "shader_type spatial;

struct PointLight {
\tvec3 position;
\tfloat energy;
};

void fragment() {
\tOUT = 7.0;
}
";
    let program = parse_shader(src)
        .unwrap_or_else(|e| panic!("a shader declaring a struct must still parse; got {e:?}"));
    assert!(
        program.function("fragment").is_some(),
        "and its fragment() must survive the struct declaration"
    );
    assert_eq!(program.shader_type.as_deref(), Some("spatial"));
}

/// Two structs back to back, and one containing a nested braced initializer —
/// the brace-balanced skip must not stop early on an inner `}`.
#[test]
fn multiple_and_nested_braces_are_skipped_correctly() {
    let src = "shader_type spatial;

struct A {
\tvec3 v;
};

struct B {
\tfloat f;
};

void fragment() {
\tOUT = 3.0;
}
";
    let program = parse_shader(src).expect("both structs are skipped");
    assert!(program.function("fragment").is_some());
}

/// A `const` array declaration parses too — its `{ ... }` initializer contains
/// no semicolon, so the existing const arm's skip-to-semicolon handles it.
#[test]
fn a_const_array_declaration_parses() {
    let src = "shader_type canvas_item;

const float weights[3] = { 0.25, 0.5, 0.25 };

void fragment() {
\tOUT = 1.0;
}
";
    let program = parse_shader(src)
        .unwrap_or_else(|e| panic!("a const array declaration must parse; got {e:?}"));
    assert!(program.function("fragment").is_some());
}

/// The tolerance is bounded: a token that LEXES fine but has no business at top
/// level is still a parse error. Without this, "skip what we do not model"
/// could silently swallow real mistakes — the behaviour this evaluator replaces.
///
/// Deliberately a well-formed token (`]`) rather than junk like `@@@`: the
/// tokenizer rejects unknown CHARACTERS itself with `UnexpectedChar`, so junk
/// would only prove the lexer works and would never reach the parser's
/// top-level arm that this test is about.
#[test]
fn a_lexable_but_misplaced_top_level_token_is_still_an_error() {
    let src = "shader_type canvas_item;\n\n]\n\nvoid fragment() {\n\tOUT = 1.0;\n}\n";
    assert!(
        parse_shader(src).is_err(),
        "a stray `]` at top level must still be rejected, not skipped"
    );
}
