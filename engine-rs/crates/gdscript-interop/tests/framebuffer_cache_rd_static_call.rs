//! pat-nzgdu.210 — `FramebufferCacheRD.get_cache_multipass(...)` answers a
//! defined value from script instead of erroring as an undefined variable.
//!
//! This is the acceptance's second clause ("returns a defined result ... without
//! panicking"). ClassDB registration alone cannot satisfy it: ClassDB is
//! schema-only (`MethodInfo` is a name plus an arity, `GodotObject` has no
//! `call`), so the call path a script actually takes is the interpreter's
//! static-dispatch chain. Before this bead there was no arm for the class, so
//! `FramebufferCacheRD.get_cache_multipass(...)` raised
//! `UndefinedVariable("FramebufferCacheRD")` — the class resolved for reflection
//! while the call did not.
//!
//! ⚠️ **`Nil` is the documented unsupported value, not a stub.** Godot returns an
//! `RID` for the cached framebuffer; Patina's `Variant` has **no `RID` variant**
//! (`gdvariant::binary` lists `RID` among the types the enum lacks), and there is
//! no `RenderingDevice` behind it to cache anything (pat-nzgdu.162). Nil is the
//! only value this can honestly name — when both exist, this test should change.
//!
//! These are integration tests on purpose: `-p gdscript-interop --lib` cannot
//! pass (a pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the
//! run), so unit tests here would be unverifiable.

use gdscript_interop::interpreter::{Interpreter, RuntimeError, RuntimeErrorKind};
use gdvariant::Variant;

/// Runs `src` and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// Runs `src` expecting a runtime error.
fn run_err(src: &str) -> RuntimeError {
    let mut interp = Interpreter::new();
    interp.run(src).expect_err("script should have failed")
}

/// The acceptance's clause: a defined result, and no panic.
#[test]
fn get_cache_multipass_returns_the_documented_unsupported_value() {
    assert_eq!(
        run_val("return FramebufferCacheRD.get_cache_multipass([], [], 1)\n"),
        Variant::Nil,
        "no RenderingDevice and no RID type — Nil is the documented unsupported value"
    );
}

/// ⚠️ The point of the arm: the call must not fall through to
/// `UndefinedVariable`. That error is what a script got before this bead, and it
/// is indistinguishable from a typo in the class name.
#[test]
fn the_call_is_no_longer_an_undefined_variable() {
    let mut interp = Interpreter::new();
    let outcome = interp.run("return FramebufferCacheRD.get_cache_multipass([], [], 1)\n");
    assert!(
        outcome.is_ok(),
        "the class must resolve from script, got {:?}",
        outcome.err().map(|e| e.kind)
    );

    // Not vacuous: a class with no dispatch arm really does raise
    // UndefinedVariable, so the assertion above discriminates.
    let err = run_err("return NoSuchClassRD.get_cache_multipass([], [], 1)\n");
    assert!(
        matches!(err.kind, RuntimeErrorKind::UndefinedVariable(_)),
        "an unregistered class still raises UndefinedVariable, got {:?}",
        err.kind
    );
}

/// ⚠️ A miscall is a TypeError, never a panic — the acceptance's "without
/// panicking" is about the *wrong* calls as much as the right one.
#[test]
fn a_wrong_arity_call_is_an_error_not_a_panic() {
    for src in [
        "return FramebufferCacheRD.get_cache_multipass()\n",
        "return FramebufferCacheRD.get_cache_multipass([])\n",
        "return FramebufferCacheRD.get_cache_multipass([], [])\n",
        "return FramebufferCacheRD.get_cache_multipass([], [], 1, 2)\n",
    ] {
        let err = run_err(src);
        assert!(
            matches!(err.kind, RuntimeErrorKind::TypeError(_)),
            "{src:?} must be a TypeError, got {:?}",
            err.kind
        );
    }
}

/// An unknown method on the class is an UndefinedFunction, matching the other
/// static singletons — and naming the class, so the message is actionable.
#[test]
fn an_unknown_method_is_an_undefined_function() {
    let err = run_err("return FramebufferCacheRD.get_cache(1)\n");
    match err.kind {
        RuntimeErrorKind::UndefinedFunction(name) => {
            assert!(
                name.contains("FramebufferCacheRD") && name.contains("get_cache"),
                "the error must name the class and method, got {name:?}"
            );
        }
        other => panic!("expected UndefinedFunction, got {other:?}"),
    }
}

/// The arguments are accepted in the shapes Godot's signature describes
/// (`RID[]`, `RDFramebufferPass[]`, `int`) — the arm must not care what is
/// *inside* the arrays, since it cannot use them yet.
#[test]
fn it_accepts_godots_argument_shapes_without_inspecting_them() {
    for src in [
        "return FramebufferCacheRD.get_cache_multipass([], [], 0)\n",
        "return FramebufferCacheRD.get_cache_multipass([1, 2], [], 1)\n",
        "return FramebufferCacheRD.get_cache_multipass([], [], 4)\n",
    ] {
        assert_eq!(run_val(src), Variant::Nil, "{src:?}");
    }
}
