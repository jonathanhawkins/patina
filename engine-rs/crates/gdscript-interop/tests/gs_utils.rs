//! pat-0q2kx.228 — @GlobalScope utilities that used to raise
//! `UndefinedFunction`.
//!
//! Godot ground truth: `core/variant/variant_utility.cpp` and
//! `core/error/error_list.cpp`'s `error_names` table.
//!
//! ⚠️ SCOPE: `type_convert`, `error_string`, `is_instance_id_valid`,
//! `rid_allocate_id`, and `type_string(38)`. NOT delivered: `rid_from_int64`
//! (Patina's `Variant` has no RID variant, so there is nothing to return) and
//! `var_to_bytes_with_objects` / `bytes_to_var_with_objects`. See the
//! completion report.
//!
//! Integration tests on purpose: `-p gdscript-interop --lib` cannot pass (a
//! pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the run).

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `src` and returns its `return` value. The trailing newline matches
/// every script literal in the sibling interop tests.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(&format!("{src}\n"))
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// Each of these raised `UndefinedFunction` before — the script died rather
/// than returning a wrong answer.
#[test]
fn the_new_utilities_are_callable_at_all() {
    assert_eq!(
        run_val("return type_convert(\"5\", TYPE_INT)"),
        Variant::Int(5)
    );
    assert_eq!(
        run_val("return error_string(OK)"),
        Variant::String("OK".into())
    );
    assert_eq!(
        run_val("return is_instance_id_valid(0)"),
        Variant::Bool(false)
    );
    assert!(matches!(
        run_val("return rid_allocate_id()"),
        Variant::Int(_)
    ));
}

/// `error_string` returns Godot's HUMAN-READABLE text, not the enum spelling.
///
/// Only entry 0 coincides, which is why checking `error_string(OK)` alone —
/// as the bead's acceptance does — cannot tell the two implementations apart.
#[test]
fn error_string_returns_human_text_not_enum_names() {
    assert_eq!(
        run_val("return error_string(OK)"),
        Variant::String("OK".into())
    );

    // FAILED is "Failed", NOT "FAILED".
    assert_eq!(
        run_val("return error_string(FAILED)"),
        Variant::String("Failed".into()),
        "error_names[1] is \"Failed\"; returning the enum name would pass an \
         OK-only check while being wrong for all 48 other codes"
    );
    // ERR_UNAVAILABLE is "Unavailable" — the ERR_ prefix is dropped entirely.
    assert_eq!(
        run_val("return error_string(ERR_UNAVAILABLE)"),
        Variant::String("Unavailable".into())
    );
    // A multi-word entry, to pin that these are sentences not identifiers.
    assert_eq!(
        run_val("return error_string(7)"),
        Variant::String("File not found".into())
    );

    // Godot: out of range yields a fixed string rather than an error.
    assert_eq!(
        run_val("return error_string(49)"),
        Variant::String("(invalid error code)".into()),
        "ERR_MAX is 49, so 49 is already out of range"
    );
    assert_eq!(
        run_val("return error_string(-1)"),
        Variant::String("(invalid error code)".into())
    );
}

/// `type_convert` coerces scalars, including the string parse the acceptance
/// names — which the existing `to_int` helper refuses.
#[test]
fn type_convert_coerces_scalars() {
    assert_eq!(
        run_val("return type_convert(\"5\", TYPE_INT)"),
        Variant::Int(5)
    );
    assert_eq!(
        run_val("return type_convert(\"5.5\", TYPE_INT)"),
        Variant::Int(5)
    );
    assert_eq!(
        run_val("return type_convert(3.9, TYPE_INT)"),
        Variant::Int(3)
    );
    assert_eq!(
        run_val("return type_convert(true, TYPE_INT)"),
        Variant::Int(1)
    );

    // Godot's String::to_int yields 0 for a non-numeric string rather than
    // failing the call.
    assert_eq!(
        run_val("return type_convert(\"abc\", TYPE_INT)"),
        Variant::Int(0)
    );

    assert_eq!(
        run_val("return type_convert(7, TYPE_STRING)"),
        Variant::String("7".into())
    );
    assert_eq!(
        run_val("return type_convert(0, TYPE_BOOL)"),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val("return type_convert(2, TYPE_BOOL)"),
        Variant::Bool(true)
    );
    assert_eq!(run_val("return type_convert(5, TYPE_NIL)"), Variant::Nil);
}

/// `is_instance_id_valid(0)` is false — 0 is never a live object id.
#[test]
fn instance_id_zero_is_never_valid() {
    assert_eq!(
        run_val("return is_instance_id_valid(0)"),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val("return is_instance_id_valid(-1)"),
        Variant::Bool(false),
        "a negative id is not an object either"
    );
}

/// `rid_allocate_id` hands out fresh, non-reused, non-zero ids.
#[test]
fn rid_allocate_id_is_monotonic_and_never_zero() {
    let two = run_val("var a = rid_allocate_id()\nvar b = rid_allocate_id()\nreturn b - a");
    assert_eq!(
        two,
        Variant::Int(1),
        "consecutive allocations must differ by exactly 1"
    );

    match run_val("return rid_allocate_id()") {
        Variant::Int(id) => assert!(id > 0, "0 is the invalid-RID sentinel, got {id}"),
        other => panic!("expected Int, got {other:?}"),
    }
}

/// `type_string(38)` names PackedVector4Array; the table used to stop at 37 and
/// answer with an empty string.
#[test]
fn type_string_covers_packed_vector4_array() {
    assert_eq!(
        run_val("return type_string(38)"),
        Variant::String("PackedVector4Array".into()),
        "type code 38 was added in Godot 4.3 and returned \"\" here"
    );
    // The neighbour still reports correctly, so 38 was appended rather than
    // shifting the table.
    assert_eq!(
        run_val("return type_string(37)"),
        Variant::String("PackedColorArray".into())
    );
    // Out of range still yields the empty string, unchanged.
    assert_eq!(
        run_val("return type_string(39)"),
        Variant::String(String::new())
    );
}
