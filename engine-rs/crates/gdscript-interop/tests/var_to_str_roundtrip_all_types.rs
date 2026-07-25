//! pat-0q2kx.69 acceptance: `str_to_var(var_to_str(v))` round-trips every
//! representable Variant type.
//!
//! Godot's `var_to_str` emits re-parseable CONSTRUCTOR text for all types
//! (`core/variant/variant_parser.cpp`'s `VariantWriter`) and `str_to_var` reads
//! it back. Patina's `var_to_str` covered 10 types then fell through to
//! `Display` — which prints Godot's *display* shapes (`(1, 2)`,
//! `[X: …, Y: …, O: …]`), not constructor text — so a Transform2D or a packed
//! array serialized to something `str_to_var` could not parse, and `str_to_var`
//! silently returned `Nil` on failure rather than reporting it.
//!
//! These drive the real VM dispatch (`run_class` → `call_instance_method`), so a
//! pass proves the builtins are wired, not merely that a Rust helper exists.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Parse `src` as a class, instantiate it, and call `method` through the real
/// interpreter dispatch (mirrors `variant_affine_types_runtime.rs`).
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

/// Round-trips `expr` through `var_to_str` → `str_to_var` and returns whether the
/// result still equals the original, evaluated inside the VM.
fn roundtrips(expr: &str) -> bool {
    let src = format!(
        "extends Node\n\nfunc check():\n    var v = {expr}\n    var s = var_to_str(v)\n    var back = str_to_var(s)\n    return back == v\n"
    );
    match call_method(&src, "check") {
        Variant::Bool(b) => b,
        other => panic!("check() should return a bool, got {other:?} for {expr}"),
    }
}

/// The serialized text, for asserting the SHAPE rather than just the round trip.
fn serialized(expr: &str) -> String {
    let src = format!("extends Node\n\nfunc check():\n    return var_to_str({expr})\n");
    match call_method(&src, "check") {
        Variant::String(s) => s,
        other => panic!("var_to_str should return a String, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn var_to_str_roundtrip_all_types() {
    // Scalars and strings were already covered; included so a regression in the
    // shared float/escape formatting shows up here too.
    for expr in ["null", "true", "42", "3.5", "\"hi\""] {
        assert!(roundtrips(expr), "{expr} must round-trip");
    }

    // The types that previously fell through to Display and could NOT be parsed
    // back — this is the bead's substance.
    for expr in [
        "Vector2(1, 2)",
        "Vector3(1, 2, 3)",
        "Color(1, 0.5, 0, 1)",
        "Rect2(1, 2, 3, 4)",
        "Transform2D(1, 0, 0, 1, 5, 6)",
        "Quaternion(0, 0, 0, 1)",
        "Plane(0, 1, 0, 5)",
        "AABB(1, 2, 3, 4, 5, 6)",
    ] {
        assert!(
            roundtrips(expr),
            "{expr} must survive var_to_str -> str_to_var"
        );
    }

    // Containers, including a nested one.
    for expr in ["[1, 2, 3]", "{\"a\": 1}", "[1, {\"a\": Vector2(1, 2)}]"] {
        assert!(roundtrips(expr), "{expr} must round-trip");
    }
}

/// `var_to_str` emits CONSTRUCTOR text, which is a different format from
/// `print`. This is the distinction that was collapsed before.
#[test]
fn var_to_str_emits_constructor_text_not_display_text() {
    assert_eq!(
        serialized("Transform2D(1, 0, 0, 1, 5, 6)"),
        // `format_gd_float` appends `.0` to an integral float, matching the
        // `.tscn`/`.tres` writer's convention — the engine is right here, my
        // first expectation was not.
        "Transform2D(1.0, 0.0, 0.0, 1.0, 5.0, 6.0)",
        "a transform serializes as its constructor, NOT as Display's [X: …, Y: …, O: …]"
    );
    assert_eq!(
        serialized("Quaternion(0, 0, 0, 1)"),
        "Quaternion(0.0, 0.0, 0.0, 1.0)",
        "…and a quaternion carries its type name, unlike Display's bare (0, 0, 0, 1)"
    );
    assert_eq!(
        serialized("Vector2(1, 2)"),
        "Vector2(1.0, 2.0)",
        "Display would print (1, 2); var_to_str must name the type"
    );
}
