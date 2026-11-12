//! pat-nzgdu.7 — @GlobalScope constants resolve end-to-end through the real
//! interpreter dispatch.
//!
//! Before this bead the GDScript VM resolved none of Godot's 512 @GlobalScope
//! named constants: a bareword like `KEY_SPACE` fell through the `Expr::Ident`
//! resolver and halted with `UndefinedVariable`. This test drives a scripted
//! class through the same dispatch path the runner uses to fire `_ready` /
//! `_process` (`run_class` -> `instantiate_class` -> `call_instance_method`,
//! which evaluates the method body via `eval_expr`) and asserts the constants
//! resolve to the correct integers — proving VM wiring, not just table lookup.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch (the path `_ready`/`_process` take).
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

fn as_ints(v: Variant) -> Vec<i64> {
    match v {
        Variant::Array(items) => items
            .to_vec()
            .into_iter()
            .map(|it| match it {
                Variant::Int(i) => i,
                other => panic!("array element not an Int: {other:?}"),
            })
            .collect(),
        other => panic!("expected Array, got {other:?}"),
    }
}

/// The bead's headline acceptance: a script returning a mix of @GlobalScope
/// barewords plus one enum-qualified reference resolves to exact integers.
#[test]
fn globalscope_constants_resolve() {
    let src = "\
extends Node
func check() -> Array:
    return [KEY_SPACE, KEY_ESCAPE, KEY_A, OK, FAILED, ERR_FILE_NOT_FOUND, \
MOUSE_BUTTON_LEFT, HORIZONTAL_ALIGNMENT_CENTER, VERTICAL_ALIGNMENT_CENTER, \
SIDE_LEFT, JOY_BUTTON_A, PROPERTY_HINT_RANGE, TYPE_VECTOR2, Key.KEY_SPACE]
";
    let got = as_ints(call_method(src, "check"));
    assert_eq!(
        got,
        vec![32, 4194305, 65, 0, 1, 7, 1, 1, 1, 0, 0, 1, 5, 32],
        "GlobalScope constants (incl. enum-qualified Key.KEY_SPACE) must resolve to Godot values",
    );
}

/// `typeof()` returns the `Variant.Type` integer, comparable against the
/// `TYPE_*` constants. (Uses `Vector2(0.0, 0.0)` — the zero-arg `Vector2()`
/// constructor is a separate arity gap unrelated to constant resolution.)
#[test]
fn typeof_matches_type_constants() {
    let src = "\
extends Node
func check() -> Array:
    return [typeof(Vector2(0.0, 0.0)) == TYPE_VECTOR2, \
typeof(42) == TYPE_INT, \
typeof(3.14) == TYPE_FLOAT, \
typeof(\"hi\") == TYPE_STRING, \
typeof(true) == TYPE_BOOL]
";
    let got = call_method(src, "check");
    let bools = match got {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    };
    assert_eq!(
        bools,
        vec![
            Variant::Bool(true),
            Variant::Bool(true),
            Variant::Bool(true),
            Variant::Bool(true),
            Variant::Bool(true),
        ]
        .into(),
        "typeof(x) must equal the matching TYPE_* constant",
    );
}

/// Enum-qualified form (`Error.OK`, `Key.KEY_ESCAPE`, ...) resolves to the same
/// flat table as the barewords.
#[test]
fn enum_qualified_constants_resolve() {
    let src = "\
extends Node
func check() -> Array:
    return [Error.OK, Error.FAILED, Error.ERR_FILE_NOT_FOUND, Key.KEY_ESCAPE, \
MouseButton.MOUSE_BUTTON_LEFT, PropertyHint.PROPERTY_HINT_RANGE]
";
    let got = as_ints(call_method(src, "check"));
    assert_eq!(got, vec![0, 1, 7, 4194305, 1, 1]);
}

/// Godot precedence: a local variable named like a global constant shadows it
/// (the global table is the last lookup before `UndefinedVariable`).
#[test]
fn local_shadows_global_constant() {
    let src = "\
extends Node
func check() -> int:
    var OK = 99
    return OK
";
    assert_eq!(call_method(src, "check"), Variant::Int(99));
}

/// A representative spread across several of the 22 enums, to guard the
/// generated table beyond the headline names.
#[test]
fn representative_constants_across_enums() {
    let src = "\
extends Node
func check() -> Array:
    return [PROPERTY_USAGE_DEFAULT, JOY_BUTTON_A, MIDI_MESSAGE_NOTE_ON, \
CORNER_TOP_LEFT, EULER_ORDER_XYZ, KEY_MASK_CTRL, TYPE_MAX, OP_EQUAL]
";
    // Values sourced 1:1 from godot_extension_api.json .global_enums:
    //   PROPERTY_USAGE_DEFAULT=6, JOY_BUTTON_A=0, MIDI_MESSAGE_NOTE_ON=9,
    //   CORNER_TOP_LEFT=0, EULER_ORDER_XYZ=0, KEY_MASK_CTRL=268435456,
    //   TYPE_MAX=39, OP_EQUAL=0.
    let got = as_ints(call_method(src, "check"));
    assert_eq!(got.len(), 8, "all eight constants must resolve to integers");
    assert_eq!(got[0], 6, "PROPERTY_USAGE_DEFAULT");
    assert_eq!(got[1], 0, "JOY_BUTTON_A");
    assert_eq!(got[2], 9, "MIDI_MESSAGE_NOTE_ON");
    assert_eq!(got[3], 0, "CORNER_TOP_LEFT");
    assert_eq!(got[5], 268435456, "KEY_MASK_CTRL");
    assert_eq!(got[6], 39, "TYPE_MAX");
    assert_eq!(got[7], 0, "OP_EQUAL");
}
