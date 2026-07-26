//! pat-0q2kx.153 — `var_to_str`/`str_to_var` over the remaining Variant types,
//! plus Godot's dictionary layout and string escapes.
//!
//! Godot's `VariantWriter` (`core/variant/variant_parser.cpp`) emits
//! re-parseable constructor text for every type, writes dictionaries
//! newline-delimited with sorted keys (lines 2306-2329), and escapes
//! `\n \t \r \b \f` inside strings.
//!
//! pat-0q2kx.69 covered most constructors. What remained: `Transform3D` and
//! four of the packed arrays still fell through to `Display` (which prints
//! Godot's *display* shape, not constructor text), dictionaries serialized
//! single-line and unsorted, and the escaper handled only `"` and `\`.
//!
//! These drive the real VM dispatch, so a pass proves the builtins are wired.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

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

/// Round-trips `expr` through `var_to_str` → `str_to_var` inside the VM.
fn roundtrips(expr: &str) -> bool {
    let src = format!(
        "extends Node\n\nfunc check():\n    var v = {expr}\n    var s = var_to_str(v)\n    var back = str_to_var(s)\n    return back == v\n"
    );
    match call_method(&src, "check") {
        Variant::Bool(b) => b,
        other => panic!("check() should return a bool, got {other:?} for {expr}"),
    }
}

/// The serialized text, for asserting shape rather than just the round trip.
fn serialized(expr: &str) -> String {
    let src = format!("extends Node\n\nfunc check():\n    return var_to_str({expr})\n");
    match call_method(&src, "check") {
        Variant::String(s) => s,
        other => panic!("var_to_str should return a String, got {other:?}"),
    }
}

/// **The acceptance test.** Every type the bead names round-trips, and the
/// dictionary layout matches Godot's.
///
/// Each case asserts the serialized text NAMES ITS TYPE as well as round-trips.
/// That second assertion is what makes the first one mean anything: if a
/// constructor is missing the expression would evaluate to `Nil`, `var_to_str`
/// would emit `"null"`, `str_to_var` would read `Nil` back, and the round trip
/// would pass vacuously. `Vector2i` was exactly that case — it had no script
/// constructor at all.
#[test]
fn util_var_to_str_roundtrip_types() {
    for (expr, type_name) in [
        // Types whose constructor text was already correct.
        ("Vector2i(1, 2)", "Vector2i("),
        ("Vector3i(1, 2, 3)", "Vector3i("),
        ("Quaternion(0, 0, 0, 1)", "Quaternion("),
        ("Plane(0, 1, 0, 5)", "Plane("),
        ("AABB(Vector3(0, 0, 0), Vector3(1, 2, 3))", "AABB("),
        ("Transform2D(1, 0, 0, 1, 5, 6)", "Transform2D("),
        ("PackedInt32Array([1, 2, 3])", "PackedInt32Array("),
        ("PackedStringArray([\"a\", \"b\"])", "PackedStringArray("),
        // The types that previously fell through to `Display`.
        (
            "Transform3D(Basis(Vector3(1, 0, 0), Vector3(0, 1, 0), Vector3(0, 0, 1)), Vector3(7, 8, 9))",
            "Transform3D(",
        ),
        ("PackedByteArray([1, 2, 250])", "PackedByteArray("),
        (
            "PackedVector2Array([Vector2(1, 2), Vector2(3, 4)])",
            "PackedVector2Array(",
        ),
        ("PackedVector3Array([Vector3(1, 2, 3)])", "PackedVector3Array("),
        ("PackedColorArray([Color(1, 0, 0, 1)])", "PackedColorArray("),
    ] {
        let text = serialized(expr);
        assert!(
            text.starts_with(type_name),
            "{expr} must serialize as constructor text starting `{type_name}`, got `{text}`",
        );
        assert!(roundtrips(expr), "{expr} must round-trip");
    }

    // Values, not just type tags, survive the trip.
    let t3d = serialized(
        "Transform3D(Basis(Vector3(1, 0, 0), Vector3(0, 1, 0), Vector3(0, 0, 1)), Vector3(7, 8, 9))",
    );
    assert!(
        t3d.contains('7') && t3d.contains('8') && t3d.contains('9'),
        "the origin must survive, got `{t3d}`",
    );

    // Nested dictionaries round-trip.
    assert!(
        roundtrips("{\"outer\": {\"inner\": Vector2i(3, 4)}, \"n\": 7}"),
        "a nested dictionary must round-trip",
    );
}

/// `==` inside the VM must compare composite values structurally.
///
/// This is the bug the round trips above tripped over: the interpreter's
/// equality enumerated only Nil/Bool/Int/Float/String/Vector2/Vector3/Color and
/// answered **false** for every other pair, so two identical Dictionaries,
/// Arrays, Vector2is or Quaternions never compared equal. Nothing here involves
/// serialization — the defect was in `==` itself, and it made `match`, `in`,
/// and `Array.has`/`find`/`count`/`erase` wrong for those types too.
#[test]
fn equality_compares_composite_values_structurally() {
    fn eval_bool(body: &str) -> bool {
        let src = format!("extends Node\n\nfunc check():\n{body}\n");
        match call_method(&src, "check") {
            Variant::Bool(b) => b,
            other => panic!("check() should return a bool, got {other:?}"),
        }
    }

    for expr in [
        "Vector2i(1, 2)",
        "Vector3i(1, 2, 3)",
        "Quaternion(0, 0, 0, 1)",
        "Transform2D(1, 0, 0, 1, 5, 6)",
        "[1, 2, 3]",
        "{\"a\": 1}",
        "PackedInt32Array([1, 2, 3])",
    ] {
        assert!(
            eval_bool(&format!("    return {expr} == {expr}")),
            "two equal `{expr}` values must compare equal",
        );
        assert!(
            !eval_bool(&format!("    return {expr} != {expr}")),
            "`!=` must agree with `==` for `{expr}`",
        );
    }

    // Different values of the same type still differ.
    assert!(eval_bool("    return Vector2i(1, 2) != Vector2i(3, 4)"));
    assert!(eval_bool("    return {\"a\": 1} != {\"a\": 2}"));
    // Dictionary equality is order-independent, as Godot's recursive `==` is.
    assert!(eval_bool(
        "    return {\"a\": 1, \"b\": 2} == {\"b\": 2, \"a\": 1}"
    ));
    // Mismatched types are still unequal, and int/float still compare
    // numerically across the two types.
    assert!(eval_bool("    return Vector2i(1, 2) != Vector2(1, 2)"));
    assert!(eval_bool("    return 1 == 1.0"));

    // `in` and the Array lookups run through the same comparison.
    assert!(eval_bool("    return Vector2i(1, 2) in [Vector2i(1, 2)]"));
    assert!(eval_bool(
        "    var a = [Vector2i(1, 2)]\n    return a.has(Vector2i(1, 2))"
    ));
}

/// `Vector2i`/`Vector3i` are constructible from a script, including Godot's
/// truncating conversion from the float vector.
#[test]
fn int_vector_constructors() {
    fn eval(expr: &str) -> Variant {
        let src = format!("extends Node\n\nfunc check():\n    return {expr}\n");
        call_method(&src, "check")
    }

    assert_eq!(
        eval("Vector2i(3, 4)"),
        Variant::Vector2i(gdcore::math::Vector2i::new(3, 4)),
    );
    assert_eq!(
        eval("Vector3i(3, 4, 5)"),
        Variant::Vector3i(gdcore::math::Vector3i::new(3, 4, 5)),
    );
    assert_eq!(
        eval("Vector2i()"),
        Variant::Vector2i(gdcore::math::Vector2i::ZERO),
    );
    // Godot truncates toward zero when narrowing the float vector.
    assert_eq!(
        eval("Vector2i(Vector2(3.7, -4.7))"),
        Variant::Vector2i(gdcore::math::Vector2i::new(3, -4)),
    );
    assert_eq!(
        eval("Vector3i(Vector3(1.9, 2.9, 3.9))"),
        Variant::Vector3i(gdcore::math::Vector3i::new(1, 2, 3)),
    );
    // …and the `is` operator answers from the Variant kind.
    let src = "extends Node\n\nfunc check():\n    return Vector2i(1, 2) is Vector2i\n";
    assert_eq!(call_method(src, "check"), Variant::Bool(true));
}

/// Godot writes dictionaries newline-delimited with the keys sorted, so two
/// equal dictionaries serialize identically regardless of insertion order.
#[test]
fn dictionary_serializes_multiline_and_sorted() {
    let text = serialized("{\"b\": 2, \"a\": 1, \"c\": 3}");

    assert!(
        text.contains('\n'),
        "a non-empty dictionary must be newline-delimited, got `{text}`",
    );

    // Keys in sorted order, not insertion order.
    let a = text.find("\"a\"").expect("key a present");
    let b = text.find("\"b\"").expect("key b present");
    let c = text.find("\"c\"").expect("key c present");
    assert!(a < b && b < c, "keys must be sorted, got `{text}`",);

    // Insertion order must not change the output — that is the whole point.
    let reordered = serialized("{\"c\": 3, \"a\": 1, \"b\": 2}");
    assert_eq!(
        text, reordered,
        "two equal dictionaries must serialize identically",
    );

    // An empty dictionary stays compact, as Godot writes it.
    assert_eq!(serialized("{}"), "{}");

    // And the multi-line form still parses back.
    assert!(roundtrips("{\"b\": 2, \"a\": 1}"));
}

/// Control characters are escaped rather than emitted raw — a raw newline
/// inside a string would be indistinguishable from the dictionary layout.
#[test]
fn strings_escape_control_characters() {
    let text = serialized("\"a\\nb\\tc\"");
    assert!(
        text.contains("\\n") && text.contains("\\t"),
        "newline and tab must be escaped, got `{text}`",
    );
    assert!(
        !text.contains('\n'),
        "no raw newline may appear in the serialized string, got `{text}`",
    );

    // Quotes and backslashes were already escaped; keep them covered.
    assert!(roundtrips("\"quote \\\" and backslash \\\\\""));
    // And the escapes decode back to the same string.
    assert!(roundtrips("\"a\\nb\\tc\""));
}
