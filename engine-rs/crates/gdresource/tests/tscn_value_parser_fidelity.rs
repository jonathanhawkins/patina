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

use gdcore::math::{Rect2i, Vector4, Vector4i};
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

    // -- the five builtins that used to have no `Variant` kind --------------
    //
    // These parsed into a *tagged dictionary* until pat-s3rrv.59: the literal
    // was preserved verbatim so the saver could re-emit it, but nothing could
    // read the value — a script asking for `.x` got a Dictionary. That was
    // always meant as a holding pattern ("a later core-variant bead can
    // promote them"), and this is that bead, so the assertions now pin the
    // real kinds.
    //
    // ⚠️ `unparsed_literal` must return `None` for every one of them. A `Some`
    // here would mean the placeholder path is still winning the race against
    // the real parse arm, and the value would reach consumers as a Dictionary
    // that merely *looks* right when printed.
    assert_eq!(
        parse("Vector4(1, 2, 3, 4)"),
        Variant::Vector4(Vector4::new(1.0, 2.0, 3.0, 4.0)),
    );
    assert_eq!(
        parse("Rect2i(0, 0, 16, 16)"),
        Variant::Rect2i(Rect2i::from_components(0, 0, 16, 16)),
    );
    assert_eq!(
        parse("PackedVector4Array(1, 2, 3, 4)"),
        Variant::PackedVector4Array(vec![Vector4::new(1.0, 2.0, 3.0, 4.0)]),
    );
    for literal in [
        "Vector4(1, 2, 3, 4)",
        "Vector4i(1, 2, 3, 4)",
        "Rect2i(0, 0, 16, 16)",
        "Projection(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        "PackedVector4Array(1, 2, 3, 4)",
    ] {
        assert_eq!(
            unparsed_literal(&parse(literal)),
            None,
            "`{literal}` is a real Variant kind now — a tagged dictionary here \
             means the placeholder path still shadows the parse arm",
        );
    }

    // `Vector4i` must not be swallowed by the `Vector4` prefix. It is the
    // INTEGER kind that has to come back: a `Variant::Vector4` would still
    // print as `(1, 2, 3, 4)` while having silently retyped the value, which
    // is the pat-nzgdu.78 corruption one dimension up.
    assert_eq!(
        parse("Vector4i(1, 2, 3, 4)"),
        Variant::Vector4i(Vector4i::new(1, 2, 3, 4)),
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

/// A `Projection` literal's sixteen numbers land COLUMN-MAJOR.
///
/// This is the one place the round-trip cannot catch its own mistake: a
/// transposed projection re-saves to the same sixteen numbers in the same
/// order, so `parse(format(p)) == p` holds perfectly while the matrix
/// disagrees with Godot. Only an asymmetric literal pins the convention.
#[test]
fn projection_literal_is_column_major() {
    // Columns (1,2,3,4), (5,6,7,8), (9,10,11,12), (13,14,15,16).
    let v = parse("Projection(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)");
    let Variant::Projection(p) = v else {
        panic!("must parse as a Projection, got {v:?}");
    };
    assert_eq!(
        p.m[0],
        [1.0, 2.0, 3.0, 4.0],
        "the FIRST four numbers are the x COLUMN (m[0]); getting [1, 5, 9, 13] \
         here means the literal was read row-major and every camera matrix is \
         transposed",
    );
    assert_eq!(
        p.m[3],
        [13.0, 14.0, 15.0, 16.0],
        "the last four are the w column"
    );
}
