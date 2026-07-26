//! pat-0q2kx.136 — `.tscn` literals parse with the right type, and the ones
//! `Variant` cannot represent yet survive a round-trip instead of vanishing.
//!
//! Godot's `VariantParser` (core/variant/variant_parser.cpp) accepts every
//! builtin literal, and an unparseable value aborts the load with a
//! line-numbered error rather than being dropped.
//!
//! Patina's `parse_variant_value` was missing `Vector4`/`Vector4i`/`Rect2i`/
//! `Projection`/`PackedVector4Array`, the `Object(Class, …)` constructor, and
//! the `^"path"` NodePath sigil; `packed_scene` then silently skipped whatever
//! failed to parse, so a real Godot scene lost properties invisibly and an
//! editor round-trip persisted the loss.

use gdresource::loader::{
    object_literal_class, parse_variant_value, parse_variant_value_typed, unparsed_literal,
};
use gdvariant::Variant;

fn parse(s: &str) -> Variant {
    parse_variant_value(s).unwrap_or_else(|e| panic!("`{s}` must parse, got {e:?}"))
}

/// **The acceptance test.** Every builtin literal in the bead's fixture list
/// parses, and each carries the correct type tag.
#[test]
fn tscn_value_parser_fidelity() {
    // -- kinds `Variant` already has --------------------------------------
    assert_eq!(parse("true"), Variant::Bool(true));
    assert_eq!(parse("42"), Variant::Int(42));
    assert_eq!(parse("\"hello\""), Variant::String("hello".into()));

    // Integer vectors stay integral. A float `Vector2` here would be the
    // silent retype pat-nzgdu.78 fixed, and this pins it.
    match parse("Vector2i(3, 4)") {
        Variant::Vector2i(v) => assert_eq!((v.x, v.y), (3, 4)),
        other => panic!("Vector2i must keep its int kind, got {other:?}"),
    }
    match parse("Vector3i(1, 2, 3)") {
        Variant::Vector3i(v) => assert_eq!((v.x, v.y, v.z), (1, 2, 3)),
        other => panic!("Vector3i must keep its int kind, got {other:?}"),
    }

    // Non-finite floats — Godot writes these bare.
    match parse("inf") {
        Variant::Float(f) => assert!(f.is_infinite() && f > 0.0, "got {f}"),
        other => panic!("`inf` must parse as a float, got {other:?}"),
    }
    match parse("-inf") {
        Variant::Float(f) => assert!(f.is_infinite() && f < 0.0, "got {f}"),
        other => panic!("`-inf` must parse as a float, got {other:?}"),
    }
    match parse("nan") {
        Variant::Float(f) => assert!(f.is_nan(), "got {f}"),
        other => panic!("`nan` must parse as a float, got {other:?}"),
    }

    // The `^"…"` NodePath sigil, the twin of `&"…"` for StringName.
    match parse("^\"Player/Sprite\"") {
        Variant::NodePath(p) => assert_eq!(p.to_string(), "Player/Sprite"),
        other => panic!("`^\"…\"` must parse as a NodePath, got {other:?}"),
    }
    match parse("&\"some_name\"") {
        Variant::StringName(n) => assert_eq!(n.as_str(), "some_name"),
        other => panic!("`&\"…\"` must parse as a StringName, got {other:?}"),
    }

    // -- builtins with no `Variant` kind yet ------------------------------
    //
    // Preserved verbatim and tagged, so nothing is lost and a later
    // core-variant bead can promote them without any data having been dropped.
    for (literal, ty) in [
        ("Vector4(1, 2, 3, 4)", "Vector4"),
        ("Vector4i(1, 2, 3, 4)", "Vector4i"),
        ("Rect2i(0, 0, 16, 16)", "Rect2i"),
        ("PackedVector4Array(1, 2, 3, 4)", "PackedVector4Array"),
    ] {
        let v = parse(literal);
        let (got_ty, got_lit) = unparsed_literal(&v)
            .unwrap_or_else(|| panic!("`{literal}` must be preserved as a tagged literal"));
        assert_eq!(got_ty, ty, "wrong type tag for {literal}");
        assert_eq!(got_lit, literal, "the literal must survive byte-for-byte");
    }

    // `Vector4i` must not be swallowed by the `Vector4` prefix — the longer
    // name is matched first, and this is what proves it.
    assert_eq!(
        unparsed_literal(&parse("Vector4i(1, 2, 3, 4)")).map(|(t, _)| t),
        Some("Vector4i"),
    );

    // -- Object(Class, "prop": value) --------------------------------------
    let obj = parse("Object(LabelSettings, \"font_size\": 20)");
    assert_eq!(
        object_literal_class(&obj),
        Some("LabelSettings"),
        "the Object literal's class must be recorded",
    );
    match &obj {
        Variant::Dictionary(d) => assert_eq!(d.get("font_size"), Some(&Variant::Int(20))),
        other => panic!("Object(...) becomes a tagged dictionary, got {other:?}"),
    }

    // -- typed arrays -------------------------------------------------------
    //
    // The value stays a plain Array so every existing consumer keeps working,
    // and the element type is available to whoever wants it.
    let (value, elem) =
        parse_variant_value_typed("Array[int]([1, 2])").expect("typed array parses");
    match value {
        Variant::Array(items) => assert_eq!(
            items,
            vec![Variant::Int(1), Variant::Int(2)],
            "elements must survive",
        ),
        other => panic!("a typed array is still an Array, got {other:?}"),
    }
    assert_eq!(
        elem.as_deref(),
        Some("int"),
        "the element type annotation must be retained, not discarded",
    );
    // A plain array reports no element type.
    let (_, plain) = parse_variant_value_typed("[1, 2]").expect("plain array parses");
    assert_eq!(plain, None);

    // -- a genuinely malformed value is an error, not a silent drop --------
    assert!(
        parse_variant_value("Vector2(1").is_err(),
        "an unterminated literal must be an error",
    );
    assert!(
        parse_variant_value("¬not a value¬").is_err(),
        "an unrecognized value must be an error",
    );
}

/// A preserved literal must re-emit as its original text, or a load/save
/// round-trip would write the preservation dictionary into the file — a worse
/// corruption than the drop it replaced.
#[test]
fn preserved_literals_round_trip_through_the_saver() {
    for literal in [
        "Vector4(1, 2, 3, 4)",
        "Rect2i(0, 0, 16, 16)",
        "Projection(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
    ] {
        let v = parse(literal);
        let (_, text) = unparsed_literal(&v).expect("tagged");
        assert_eq!(
            text, literal,
            "the saver re-emits this text verbatim, so it must match the input",
        );
    }
}
