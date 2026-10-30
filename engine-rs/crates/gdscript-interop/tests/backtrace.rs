//! pat-0q2kx.253 — `ScriptBacktrace` and `Engine.capture_script_backtraces`.
//!
//! Godot's `Engine.capture_script_backtraces(include_variables)` returns one
//! `ScriptBacktrace` per script language — a snapshot of the frames live at the
//! moment of capture. Patina had neither the class nor the producer.
//!
//! The interpreter already tracked `call_stack_frames` for error messages
//! (`get_stack()` exposes it, pat-0q2kx.174), so these tests are about the
//! snapshot and its script-visible surface, not about new instrumentation.
//!
//! Binary named `backtrace`: the fuller `--test script_backtrace` makes the
//! verify command 61 characters, past the 58-char truncation where the target
//! silently resolves to nothing. The acceptance function keeps its full name.
//!
//! ## Why the scripts nest two levels below `check()`
//!
//! `call_stack_frames` is pushed **only** in `call_user_func_impl`
//! (`interpreter.rs:6030`). `call_instance_method_impl` pushes nothing, so the
//! entry method a test calls never appears in the backtrace — only functions it
//! goes on to call. Nesting `check -> level_two -> level_three` therefore yields
//! two frames, which is what makes "in order" observable at all.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `check()` on a script and returns its value.
///
/// Seeds ClassDB first: a bare interpreter starts with an **empty** registry —
/// nothing populates it in production, callers do — so
/// `ClassDB.class_exists("ScriptBacktrace")` would answer false however the
/// class is registered. `ScriptBacktrace` lives in `register_3d_classes`
/// alongside the other `RefCounted` handle classes, despite the name.
/// Registration is `class_exists`-guarded and therefore idempotent.
fn call_check(src: &str) -> Variant {
    gdobject::class_db::register_3d_classes();
    gdscript_interop::script_backtrace::clear();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

fn array(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(a) => a.to_vec(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** `ClassDB.class_exists("ScriptBacktrace")` is true,
/// and a capture taken inside a nested call names the enclosing functions in
/// order.
///
/// The backtrace is read **after the stack has unwound** — `check` receives it
/// back through two returns. That is deliberate: a `ScriptBacktrace` is a
/// snapshot, not a live view, and an implementation that read the interpreter's
/// current stack lazily would report an empty or wrong stack here while passing
/// a naive same-frame test.
#[test]
fn script_backtrace_captures_gdscript_stack() {
    let src = "\
extends Node

func level_three():
    return Engine.capture_script_backtraces()

func level_two():
    return level_three()

func check():
    var registered = ClassDB.class_exists(\"ScriptBacktrace\")
    var traces = level_two()
    var bt = traces[0]
    return [registered, traces.size(), bt.get_frame_count(), bt.get_frame_function(0), bt.get_frame_function(1), bt.is_empty()]
";
    let v = array(call_check(src));
    assert_eq!(
        v[0],
        Variant::Bool(true),
        "ClassDB.class_exists(\"ScriptBacktrace\") must be true",
    );
    assert_eq!(
        v[1],
        Variant::Int(1),
        "one backtrace per script language, and there is one language here",
    );
    assert_eq!(
        v[2],
        Variant::Int(2),
        "level_two and level_three are on the stack; check() is the entry \
         method and pushes no frame",
    );
    assert_eq!(
        v[3],
        Variant::String("level_three".to_string()),
        "frame 0 must be the INNERMOST call, where the capture happened",
    );
    assert_eq!(
        v[4],
        Variant::String("level_two".to_string()),
        "frame 1 must be its caller",
    );
    assert_eq!(v[5], Variant::Bool(false), "the capture is not empty");
}

// ===========================================================================
// The rest of the surface
// ===========================================================================

/// A capture taken at top level, with no nested call, has no frames — and must
/// say so rather than inventing one.
#[test]
fn a_capture_with_no_nested_call_is_empty() {
    let src = "\
extends Node

func check():
    var bt = Engine.capture_script_backtraces()[0]
    return [bt.get_frame_count(), bt.is_empty()]
";
    let v = array(call_check(src));
    assert_eq!(v[0], Variant::Int(0));
    assert_eq!(v[1], Variant::Bool(true));
}

/// Line numbers and the language name come through, and an out-of-range frame
/// index is inert rather than fatal — a script holding a stale index must not
/// take the VM down.
#[test]
fn frame_metadata_and_out_of_range_indices() {
    let src = "\
extends Node

func inner():
    return Engine.capture_script_backtraces()

func check():
    var bt = inner()[0]
    return [bt.get_language_name(), bt.get_frame_line(0) > 0, bt.get_frame_function(99), bt.get_frame_line(99), bt.get_frame_file(99)]
";
    let v = array(call_check(src));
    assert_eq!(v[0], Variant::String("GDScript".to_string()));
    assert_eq!(
        v[1],
        Variant::Bool(true),
        "a captured frame must carry a real source line",
    );
    assert_eq!(v[2], Variant::String(String::new()));
    assert_eq!(v[3], Variant::Int(0));
    assert_eq!(v[4], Variant::String(String::new()));
}

/// `format()` renders the frames innermost-first, so the top line is where the
/// capture happened.
#[test]
fn format_renders_the_frames() {
    let src = "\
extends Node

func inner():
    return Engine.capture_script_backtraces()

func check():
    return inner()[0].format(0, 2)
";
    let text = match call_check(src) {
        Variant::String(s) => s,
        other => panic!("format() must return a String, got {other:?}"),
    };
    assert!(text.contains("GDScript"), "got {text:?}");
    assert!(text.contains("inner"), "got {text:?}");
}

/// Two captures are independent snapshots: taking one deeper in the stack must
/// not mutate one taken earlier.
#[test]
fn captures_are_independent_snapshots() {
    let src = "\
extends Node

func deep():
    return Engine.capture_script_backtraces()[0]

func shallow():
    return Engine.capture_script_backtraces()[0]

func check():
    var a = shallow()
    var b = deep()
    return [a.get_frame_count(), b.get_frame_count(), a.get_frame_function(0)]
";
    let v = array(call_check(src));
    assert_eq!(v[0], Variant::Int(1), "shallow() is one frame deep");
    assert_eq!(v[1], Variant::Int(1), "deep() is also one frame deep");
    assert_eq!(
        v[2],
        Variant::String("shallow".to_string()),
        "the first capture must still name its own frame after the second",
    );
}

/// A frame reports the **exact** line the statement it is executing starts on,
/// not merely a non-zero one.
///
/// `frame_metadata_and_out_of_range_indices` only asserts `get_frame_line(0) > 0`.
/// That is satisfiable by accident: before pat-0q2kx.253 every frame reported 0,
/// and the cheapest way to make it pass would have been to hardcode any constant.
/// On a one-line function body even a correct-looking fix cannot be distinguished
/// from `return 1`.
///
/// So this pins two different capture sites in one script, several lines apart,
/// and checks each against the line its own `return` sits on. The literals below
/// are counted from the `extends Node` line as line 1.
#[test]
fn frame_line_is_the_line_of_the_executing_statement() {
    //  1: extends Node
    //  2:
    //  3: func early():
    //  4:     return Engine.capture_script_backtraces()
    //  5:
    //  6: func late():
    //  7:     var padding = 1
    //  8:     padding += 1
    //  9:     return Engine.capture_script_backtraces()
    // 10:
    // 11: func check():
    let src = "\
extends Node

func early():
    return Engine.capture_script_backtraces()

func late():
    var padding = 1
    padding += 1
    return Engine.capture_script_backtraces()

func check():
    return [early()[0].get_frame_line(0), late()[0].get_frame_line(0)]
";
    let v = array(call_check(src));
    assert_eq!(
        v[0],
        Variant::Int(4),
        "early()'s only statement is the return on line 4",
    );
    assert_eq!(
        v[1],
        Variant::Int(9),
        "late() captures on line 9, after two padding statements — a frame that \
         reported its function's first line, or any constant, would fail here",
    );
    assert_ne!(
        v[0], v[1],
        "two capture sites must not report the same line"
    );
}
