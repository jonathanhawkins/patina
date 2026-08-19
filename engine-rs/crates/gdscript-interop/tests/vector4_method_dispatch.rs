//! pat-s3rrv.59: `Vector4` / `Vector4i` instance methods reach a script.
//!
//! Both kinds could be constructed and their `x`/`y`/`z`/`w` read, but
//! `call_method` had no arm for either, so every `v.length()` fell through to
//! the interpreter's "cannot call method on Vector4" catch-all. `Vector2` and
//! `Vector3` are handled inline in the interpreter, so these two had no arm
//! anywhere — the same dispatch-shaped gap `Color` had before pat-s3rrv.58.
//!
//! These drive real GDScript rather than calling `variant_math` directly, so a
//! table that exists but is not delegated to still fails.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Evaluates `expr` inside a script and returns its value.
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

fn as_f64(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        other => panic!("expected Float, got {other:?}"),
    }
}

/// **The acceptance case**: the methods dispatch at all.
#[test]
fn vector4_instance_methods_dispatch() {
    // sqrt(1 + 4 + 9 + 16) = sqrt(30).
    let len = as_f64(eval("Vector4(1.0, 2.0, 3.0, 4.0).length()"));
    assert!(
        (len - 30.0f64.sqrt()).abs() < 1e-5,
        "Vector4.length() must be sqrt(30), got {len}"
    );

    assert!((as_f64(eval("Vector4(1.0, 2.0, 3.0, 4.0).length_squared()")) - 30.0).abs() < 1e-6);
    assert!(
        (as_f64(eval(
            "Vector4(1.0, 2.0, 3.0, 4.0).dot(Vector4(1.0, 1.0, 1.0, 1.0))"
        )) - 10.0)
            .abs()
            < 1e-6,
        "dot must sum all FOUR products; 6.0 would mean w was dropped",
    );
    assert!(
        (as_f64(eval(
            "Vector4(1.0, 2.0, 3.0, 4.0).distance_to(Vector4(1.0, 2.0, 3.0, 5.0))"
        )) - 1.0)
            .abs()
            < 1e-6,
    );

    assert_eq!(
        eval("Vector4(1.0, 2.0, 3.0, 4.0).is_finite()"),
        Variant::Bool(true)
    );
    assert_eq!(
        eval("Vector4(0.0, 0.0, 0.0, 0.0).is_zero_approx()"),
        Variant::Bool(true)
    );
    // ⚠️ Only `w` is non-zero, so a check that forgot the fourth component
    // would answer `true` here.
    assert_eq!(
        eval("Vector4(0.0, 0.0, 0.0, 1.0).is_zero_approx()"),
        Variant::Bool(false),
        "is_zero_approx must consider w",
    );
}

/// 🔑 Godot's two axis scans are DELIBERATELY asymmetric
/// (`core/math/vector4.cpp:37-59`): `min_axis_index` compares with `<=` so a
/// tie picks the **last** axis, `max_axis_index` with `>` so a tie picks the
/// **first**.
///
/// Writing both the same way — the obvious thing to do — is wrong on ties and
/// completely invisible to any fixture whose components are distinct. Hence the
/// tie vector: a symmetric implementation answers min = 2 here, not 3.
#[test]
fn axis_index_tie_breaking_matches_godot() {
    // Distinct components first, so a total failure to dispatch is separated
    // from a tie-breaking error.
    assert_eq!(
        eval("Vector4(1.0, 2.0, 3.0, 4.0).max_axis_index()"),
        Variant::Int(3)
    );
    assert_eq!(
        eval("Vector4(1.0, 2.0, 3.0, 4.0).min_axis_index()"),
        Variant::Int(0)
    );

    // (5, 5, 1, 1): the max ties between x and y, the min between z and w.
    assert_eq!(
        eval("Vector4(5.0, 5.0, 1.0, 1.0).max_axis_index()"),
        Variant::Int(0),
        "a tied maximum takes the FIRST axis (`>`), so x wins over y",
    );
    assert_eq!(
        eval("Vector4(5.0, 5.0, 1.0, 1.0).min_axis_index()"),
        Variant::Int(3),
        "a tied minimum takes the LAST axis (`<=`), so w wins over z — a \
         symmetric implementation answers 2 here",
    );
}

/// `Vector4i` keeps integer results, and only the square root widens.
///
/// ⚠️ `length_squared` must be an **Int**: handing back a Float would make
/// `v.length_squared() == 30` fail for a value that IS 30, the same way an int
/// vector silently widened to float has bitten this codebase before.
#[test]
fn vector4i_methods_keep_integer_results() {
    assert_eq!(
        eval("Vector4i(1, 2, 3, 4).length_squared()"),
        Variant::Int(30),
        "length_squared stays exact and integral",
    );

    let len = as_f64(eval("Vector4i(1, 2, 3, 4).length()"));
    assert!(
        (len - 30.0f64.sqrt()).abs() < 1e-5,
        "…while length() is the widened square root, got {len}"
    );

    assert_eq!(
        eval("Vector4i(-1, 2, -3, 4).abs()"),
        eval("Vector4i(1, 2, 3, 4)"),
    );
    assert_eq!(
        eval("Vector4i(-7, 0, 9, -2).sign()"),
        eval("Vector4i(-1, 0, 1, -1)"),
        "sign is per-component and 0 stays 0",
    );
    assert_eq!(
        eval("Vector4i(5, -5, 0, 9).clamp(Vector4i(0, 0, 0, 0), Vector4i(3, 3, 3, 3))"),
        eval("Vector4i(3, 0, 0, 3)"),
    );
}

/// An unknown method names itself rather than answering Nil.
#[test]
fn an_unknown_method_is_an_error() {
    let src = "extends Node\n\nfunc check():\n    return Vector4(1.0, 1.0, 1.0, 1.0).no_such()\n";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let err = interp
        .call_instance_method(&mut inst, "check", &[])
        .expect_err("an unknown Vector4 method must raise");
    let text = format!("{err:?}");
    assert!(
        text.contains("no_such") && text.contains("Vector4"),
        "the error must name the class and method, got {text}",
    );
}
