//! pat-s3rrv.67 — a lambda captures its enclosing locals by value.
//!
//! `CallableRef::Lambda` carried parameters and a body and nothing else, and
//! invocation pushed a scope onto whatever environment happened to be current
//! at CALL time. A lambda stored in an Array, a Tween or a signal connection
//! therefore read whatever the name meant later — a stale value, or nothing at
//! all if the scope had gone.
//!
//! Godot's `GDScriptLambdaCallable` (modules/gdscript/gdscript_lambda_callable.cpp)
//! stores the captured Variants when the `func(...)` expression evaluates and
//! rebinds them on every invocation.
//!
//! ⚠️ Array and Dictionary are deliberately NOT captured — see
//! `the_array_accumulator_idiom_still_works` at the bottom for why, and why
//! that is the closer approximation rather than a shortcut.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `body` as a method and returns its value.
fn run(body: &str) -> Variant {
    let src = format!("extends Node\nfunc run():\n{body}");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp.instantiate_class(&class_def).expect("instantiates");
    interp
        .call_instance_method(&mut inst, "run", &[])
        .expect("run() executes")
}

fn as_ints(v: Variant) -> Vec<i64> {
    match v {
        Variant::Array(a) => a
            .to_vec()
            .into_iter()
            .map(|x| match x {
                Variant::Int(i) => i,
                other => panic!("expected Int, got {other:?}"),
            })
            .collect(),
        other => panic!("expected Array, got {other:?}"),
    }
}

/// **The acceptance test** — the canonical Godot pattern.
///
/// Each lambda made inside the loop must remember ITS OWN `i`. Before this,
/// every one of them read the same `i` at call time — the classic closure bug,
/// and the exact example the bead cites.
#[test]
fn lambda_captures_enclosing_locals() {
    let got = run("\x20   var cbs = []\n\
                   \x20   for i in range(3):\n\
                   \x20       cbs.append(func(): return i)\n\
                   \x20   var out = []\n\
                   \x20   for cb in cbs:\n\
                   \x20       out.append(cb.call())\n\
                   \x20   return out\n");
    assert_eq!(
        as_ints(got),
        vec![0, 1, 2],
        "each lambda must remember the `i` it was created with; [2, 2, 2] means \
         they all read the loop variable at call time, and an error means the \
         name was gone by then",
    );
}

/// Capture is BY VALUE: changing the local afterwards does not change what the
/// lambda sees (pat-s3rrv.67).
///
/// This is the half that a "capture by reference to the live scope"
/// implementation would get wrong while still passing the loop test.
#[test]
fn capture_is_by_value_not_by_reference() {
    let got = run("\x20   var n = 1\n\
                   \x20   var f = func(): return n\n\
                   \x20   n = 99\n\
                   \x20   return f.call()\n");
    assert_eq!(
        got,
        Variant::Int(1),
        "the lambda captured n = 1 at creation; 99 would mean it re-read the \
         live variable",
    );
}

/// A lambda outlives the scope its captures came from (pat-s3rrv.67).
#[test]
fn a_captured_local_survives_its_scope() {
    let got = run("\x20   var fs = []\n\
                   \x20   if true:\n\
                   \x20       var inner = 7\n\
                   \x20       fs.append(func(): return inner)\n\
                   \x20   return fs[0].call()\n");
    assert_eq!(
        got,
        Variant::Int(7),
        "`inner` is out of scope by the time the lambda runs, which is exactly \
         when the old behaviour raised",
    );
}

/// A parameter shadows a captured local of the same name (pat-s3rrv.67).
///
/// Captures are bound BEFORE parameters for this reason; binding them after
/// would let an enclosing local silently override an argument.
#[test]
fn a_parameter_shadows_a_capture_of_the_same_name() {
    let got = run("\x20   var x = 1\n\
                   \x20   var f = func(x): return x\n\
                   \x20   return f.call(42)\n");
    assert_eq!(
        got,
        Variant::Int(42),
        "the argument must win over the captured `x`",
    );
}

/// Arrays are NOT captured, and that is deliberate (pat-s3rrv.67).
///
/// In Godot an Array is a refcounted handle, so `log.append(x)` inside a lambda
/// mutates the caller's array — which is how every callback test in this repo
/// observes that a callback fired. Patina still models Array as a VALUE
/// (`Variant::Array(Vec<Variant>)`; the flip is pat-s3rrv.8 and is not done),
/// so snapshotting one would hand the lambda a private copy and silently drop
/// every append.
///
/// Leaving Array/Dictionary to resolve live is the CLOSER approximation of
/// Godot, not a shortcut: shared-with-the-enclosing-scope is nearer to "shared"
/// than a detached copy is. This test is the regression guard for that choice.
#[test]
fn the_array_accumulator_idiom_still_works() {
    let got = run("\x20   var log = []\n\
                   \x20   var f = func(): log.append(5)\n\
                   \x20   f.call()\n\
                   \x20   f.call()\n\
                   \x20   return log\n");
    assert_eq!(
        as_ints(got),
        vec![5, 5],
        "appending to an enclosing Array from inside a lambda must still reach \
         the caller — capturing it by value would silently drop both appends \
         and every callback test in this repo with them",
    );
}
