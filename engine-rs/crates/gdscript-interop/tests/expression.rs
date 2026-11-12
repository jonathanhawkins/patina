//! pat-0q2kx.207 — Godot's `Expression` class is constructible and executable
//! from GDScript.
//!
//! The class was absent everywhere: `grep '"Expression"'` (excluding
//! `VisualShaderNodeExpression`) returned nothing, there was no `.new()` arm and
//! no ClassDB entry. Every building block existed though — `parser.rs` already
//! parses the full GDScript expression grammar and `interpreter.rs` evaluates
//! `Expr` trees — so this binds those together rather than growing a second
//! expression engine beside them.
//!
//! `Expression` is `RefCounted`, so it is a handle-table object here rather than
//! a scene node — the same shape as ConfigFile / RegEx / UndoRedo.
//!
//! Godot: `doc/classes/Expression.xml`, `core/math/expression.cpp`.

use gdobject::class_db::class_exists;
use gdobject::register_3d_classes;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `method` on a fresh instance of `src` through the real VM dispatch.
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

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** Parse with input names, execute with positional
/// inputs, and report both kinds of failure as state rather than a crash.
#[test]
fn expression_parse_execute() {
    // -- parse() returns OK, execute() computes ------------------------------
    //
    // x * 2 + abs(y) with x = 3, y = -4  ->  6 + 4  ->  10. `abs` also proves a
    // builtin call resolves inside the compiled expression.
    let src = "\
extends Node

func check():
    var e = Expression.new()
    var err = e.parse(\"x * 2 + abs(y)\", [\"x\", \"y\"])
    if err != 0:
        return -1
    var v = e.execute([3, -4])
    if e.has_execute_failed():
        return -2
    return v
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Int(10),
        "parse() must return OK (0) and execute() must evaluate to 10",
    );

    // -- a malformed expression fails to parse, with error text --------------
    let bad = "\
extends Node

func check():
    var e = Expression.new()
    var err = e.parse(\"1 +* 2\", [])
    if err == 0:
        return \"parse wrongly reported OK\"
    return e.get_error_text()
";
    match call_method(bad, "check") {
        Variant::String(text) => assert!(
            !text.is_empty(),
            "a failed parse must leave non-empty get_error_text(), got {text:?}",
        ),
        other => panic!("expected the error text, got {other:?}"),
    }

    // -- an unknown identifier fails the EXECUTE, not the interpreter --------
    //
    // This is the whole reason Expression exists: it evaluates untrusted,
    // user-authored formulas, so a bad one must be reportable rather than
    // taking the calling script down.
    let missing = "\
extends Node

func check():
    var e = Expression.new()
    var err = e.parse(\"nope + 1\", [])
    if err != 0:
        return false
    e.execute([])
    return e.has_execute_failed()
";
    assert_eq!(
        call_method(missing, "check"),
        Variant::Bool(true),
        "an undefined identifier must set has_execute_failed() without aborting",
    );
}

// ===========================================================================
// Details
// ===========================================================================

/// The class must be registered, or `.tscn`-attached scripts and the inspector
/// cannot see it even though the interpreter can construct one.
#[test]
fn expression_is_registered() {
    register_3d_classes();
    assert!(
        class_exists("Expression"),
        "Expression must be registered in ClassDB",
    );

    let info = gdobject::class_db::get_class_info("Expression").expect("registered");
    let methods: Vec<&str> = info.methods.iter().map(|m| m.name.as_str()).collect();
    for expected in ["parse", "execute", "has_execute_failed", "get_error_text"] {
        assert!(
            methods.contains(&expected),
            "Expression must expose `{expected}`; has {methods:?}",
        );
    }
    assert_eq!(
        info.parent_class, "RefCounted",
        "Godot's Expression is RefCounted, so `.new()` yields a handle not a node",
    );
}

/// Inputs bind **positionally** to the names given at parse time — that is the
/// contract, and getting the order wrong would silently compute nonsense.
#[test]
fn inputs_bind_positionally_by_name() {
    let src = "\
extends Node

func check():
    var e = Expression.new()
    e.parse(\"a - b\", [\"a\", \"b\"])
    return e.execute([10, 4])
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Int(6),
        "a=10, b=4 must give 10-4, not 4-10",
    );
}

/// A second `parse()` replaces the program. A failed re-parse must invalidate
/// the old one rather than leaving the previous expression quietly executable.
#[test]
fn failed_reparse_invalidates_the_previous_program() {
    let src = "\
extends Node

func check():
    var e = Expression.new()
    e.parse(\"1 + 1\", [])
    var first = e.execute([])
    var err = e.parse(\"1 +* 2\", [])
    if err == 0:
        return -1
    e.execute([])
    if not e.has_execute_failed():
        return -2
    return first
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Int(2),
        "the first program evaluates; after a failed re-parse execute() must fail",
    );
}

/// Executing before any successful parse reports failure instead of panicking.
#[test]
fn execute_before_parse_reports_failure() {
    let src = "\
extends Node

func check():
    var e = Expression.new()
    e.execute([])
    return e.has_execute_failed()
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Bool(true),
        "execute() with no parsed program must set has_execute_failed()",
    );
}

/// Two Expressions are independent — the handle table must not share state.
#[test]
fn expressions_are_independent() {
    let src = "\
extends Node

func check():
    var a = Expression.new()
    var b = Expression.new()
    a.parse(\"1 + 1\", [])
    b.parse(\"10 * 10\", [])
    return a.execute([]) + b.execute([])
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Int(102),
        "each handle keeps its own program (2 + 100)",
    );
}
