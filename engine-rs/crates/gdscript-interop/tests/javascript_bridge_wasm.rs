//! pat-u3l8c.115 — `JavaScriptBridge` is a real bridge, not a recorder.
//!
//! **Godot ground truth:** `platform/web/javascript_bridge.cpp` (the web
//! implementation) and `core/config/engine.cpp`'s `JavaScriptBridgeDummy` (what
//! every other platform links). Godot's contract is that the singleton *exists*
//! everywhere and only *does* anything on the web export.
//!
//! Before this bead Patina shipped the dummy on **every** target including
//! wasm32: `JsBridge` (gdplatform/src/web.rs) recorded calls into a `HashMap`
//! and returned `NotAvailable`/`Undefined`, so a web build could not reach
//! `localStorage`, `navigator`, or any other browser API.
//!
//! The browser half now lives in `gdplatform::js_bridge::WasmJsBackend` and can
//! only run under wasm32. What these tests pin is everything on *this* side of
//! that boundary — the part that was actually missing, and the part a
//! regression would silently break:
//!
//! - `eval` returns a **converted Variant** rather than a fixed `Nil`.
//! - `get_interface` yields an object handle that script code can keep and call
//!   methods on, which is what makes `localStorage.setItem(...)` expressible.
//! - a `Callable` registered through `create_callback` really runs when
//!   JavaScript invokes it.
//! - off the web, every return value is still Godot's dummy value, so
//!   pat-nzgdu.66's headless contract is unchanged.
//!
//! The JS half is doubled by `RecordingJsBackend`, which hands out real handles
//! and answers from a table. That is deliberate: a browser-only assertion could
//! not run in CI at all, and the wiring these tests cover is exactly the wiring
//! that was absent.
//!
//! These are integration tests on purpose: `-p gdscript-interop --lib` cannot
//! pass (a pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the
//! run), so unit tests here would be unverifiable.

use gdplatform::js_bridge::{self, JsValue, RecordingJsBackend};
use gdscript_interop::interpreter::Interpreter;
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

/// Installs a fresh double. The backend is thread-local, so every test has to
/// start from a known state even under nextest's process-per-test isolation.
fn install(backend: RecordingJsBackend) {
    js_bridge::reset_backend();
    js_bridge::set_backend(Box::new(backend));
}

// ---------------------------------------------------------------------------
// eval
// ---------------------------------------------------------------------------

/// `eval` returns what JavaScript produced, converted to a `Variant`.
///
/// The old implementation returned `Variant::Nil` unconditionally, so this is
/// the assertion that distinguishes "wired" from "stubbed".
#[test]
fn eval_returns_converted_variants() {
    install(
        RecordingJsBackend::new()
            .with_eval("1 + 2", JsValue::Number(3.0))
            .with_eval("'ab' + 'cd'", JsValue::String("abcd".into()))
            .with_eval("!!window", JsValue::Bool(true)),
    );

    // A JS number is a double — one numeric type — so it converts to Float even
    // when integral. Inferring Int from 3.0 would make the same expression
    // change Variant type with its runtime value.
    assert_eq!(
        run_val("return JavaScriptBridge.eval(\"1 + 2\")\n"),
        Variant::Float(3.0),
        "eval must return the value JavaScript produced, not Nil"
    );
    assert_eq!(
        run_val("return JavaScriptBridge.eval(\"'ab' + 'cd'\")\n"),
        Variant::String("abcd".to_string())
    );
    assert_eq!(
        run_val("return JavaScriptBridge.eval(\"!!window\")\n"),
        Variant::Bool(true)
    );

    // An expression the backend has no answer for is `null` — not an error.
    assert_eq!(
        run_val("return JavaScriptBridge.eval(\"noSuchThing()\")\n"),
        Variant::Nil
    );

    js_bridge::reset_backend();
}

/// Godot's second argument is `use_global_execution_context`; it has to reach
/// the backend, because global vs. local scope changes what the code can see.
#[test]
fn eval_forwards_the_global_execution_context_flag() {
    install(RecordingJsBackend::new());

    run_val("return JavaScriptBridge.eval(\"a\", true)\n");
    run_val("return JavaScriptBridge.eval(\"b\")\n");

    let calls = js_bridge::recorded_calls();
    assert!(
        calls.contains(&"eval:a:true".to_string()),
        "explicit `true` must reach the backend as global scope, got {calls:?}"
    );
    assert!(
        calls.contains(&"eval:b:false".to_string()),
        "the flag defaults to false, as in Godot, got {calls:?}"
    );

    js_bridge::reset_backend();
}

// ---------------------------------------------------------------------------
// get_interface / JavaScriptObject
// ---------------------------------------------------------------------------

/// `get_interface` hands back a live object the script can keep and call.
///
/// This is the shape every real use of the bridge takes — `localStorage`,
/// `navigator`, `console` — and it was entirely absent: `get_interface`
/// returned `Nil`, so there was nothing to call a method on.
#[test]
fn get_interface_returns_an_object_handle_script_can_call() {
    install(RecordingJsBackend::new().with_interface("localStorage", "Storage"));

    // A handle is an Object, not Nil.
    let handle = run_val("return JavaScriptBridge.get_interface(\"localStorage\")\n");
    assert!(
        matches!(handle, Variant::ObjectId(_)),
        "get_interface must yield an object handle, got {handle:?}"
    );

    // Arbitrary method names forward to the object, as Godot's
    // JavaScriptObject does — there is no whitelist.
    run_val(
        "var ls = JavaScriptBridge.get_interface(\"localStorage\")\n\
         ls.setItem(\"theme\", \"dark\")\n\
         return 0\n",
    );
    let calls = js_bridge::recorded_calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("call:") && c.contains(".setItem")),
        "a method call on the handle must reach the backend, got {calls:?}"
    );

    js_bridge::reset_backend();
}

/// `set` then `get` on the same handle round-trips a property.
#[test]
fn javascript_object_property_round_trip() {
    install(RecordingJsBackend::new().with_interface("localStorage", "Storage"));

    assert_eq!(
        run_val(
            "var ls = JavaScriptBridge.get_interface(\"localStorage\")\n\
             ls.set(\"theme\", \"dark\")\n\
             return ls.get(\"theme\")\n"
        ),
        Variant::String("dark".to_string()),
        "a property written through the handle must read back through it"
    );

    js_bridge::reset_backend();
}

/// An unresolvable global is `null`, matching a browser lookup that misses.
#[test]
fn unknown_interface_is_nil_not_an_error() {
    install(RecordingJsBackend::new());
    assert_eq!(
        run_val("return JavaScriptBridge.get_interface(\"definitelyNotAGlobal\")\n"),
        Variant::Nil
    );
    js_bridge::reset_backend();
}

// ---------------------------------------------------------------------------
// Callbacks — the acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** A `Callable` registered from GDScript runs when
/// JavaScript invokes the function `create_callback` handed back, and receives
/// the argument JavaScript passed.
///
/// Registration must *not* run it — that is the bug this pins on the other
/// side: a bridge that fired the callback eagerly would look like it worked
/// while never being driven by a real event.
///
/// The lambda's body calls `eval` because that is the one effect observable
/// from outside the interpreter: a lambda captures by value, so a variable it
/// assigns never escapes (see the same pattern in `camera_feed.rs`).
#[test]
fn callback_registration_reaches_the_script() {
    install(RecordingJsBackend::new());

    let mut interp = Interpreter::new();
    interp
        .run(
            "JavaScriptBridge.create_callback(func(v): JavaScriptBridge.eval(v))\n\
             return 0\n",
        )
        .expect("registration script runs");

    // Link 1 — registration reached the backend and minted an id ...
    let calls = js_bridge::recorded_calls();
    let registered: Vec<&String> = calls
        .iter()
        .filter(|c| c.starts_with("create_callback:"))
        .collect();
    assert_eq!(
        registered.len(),
        1,
        "create_callback must register exactly once, got {calls:?}"
    );
    let id: u64 = registered[0]
        .trim_start_matches("create_callback:")
        .parse()
        .expect("the log carries the callback id");

    // Link 2 — ... and registering alone did not run the Callable.
    assert!(
        !js_bridge::recorded_calls()
            .iter()
            .any(|c| c.starts_with("eval:")),
        "registration must not invoke the callback"
    );
    assert_eq!(interp.pump_javascript_callbacks(), 0, "nothing queued yet");

    // Link 3 — JavaScript invokes the function it was given. In a browser this
    // is the js-sys closure; here it is the same queue that closure writes to.
    js_bridge::queue_callback_invocation(id, vec![JsValue::String("from-js".into())]);
    assert_eq!(
        interp.pump_javascript_callbacks(),
        1,
        "the pump must run the registered Callable"
    );

    // Link 4 — the Callable ran *with the argument JavaScript supplied*.
    let after = js_bridge::recorded_calls();
    assert!(
        after.contains(&"eval:from-js:false".to_string()),
        "the callback body must run and receive the JS argument, got {after:?}"
    );

    // A drained invocation does not run twice.
    assert_eq!(interp.pump_javascript_callbacks(), 0);

    js_bridge::reset_backend();
}

/// An invocation for an id nobody registered is dropped, not an error — a
/// browser event must not be able to abort the frame that pumped it.
#[test]
fn unregistered_callback_invocation_is_dropped() {
    install(RecordingJsBackend::new());
    let mut interp = Interpreter::new();
    js_bridge::queue_callback_invocation(9999, vec![JsValue::Number(1.0)]);
    assert_eq!(interp.pump_javascript_callbacks(), 0);
    js_bridge::reset_backend();
}

// ---------------------------------------------------------------------------
// Off-web parity (pat-nzgdu.66 regression guard)
// ---------------------------------------------------------------------------

/// With no backend installed the default is the null one, and every call still
/// resolves to Godot's dummy value rather than `UndefinedFunction`.
///
/// This is the contract pat-nzgdu.66 established; making the bridge real must
/// not change what a headless run sees.
#[test]
fn off_web_still_matches_godots_dummy() {
    js_bridge::reset_backend();

    assert_eq!(
        run_val("return JavaScriptBridge.eval(\"1+2\")\n"),
        Variant::Nil
    );
    assert_eq!(
        run_val("return JavaScriptBridge.eval(\"1+2\", true)\n"),
        Variant::Nil
    );
    assert_eq!(
        run_val("return JavaScriptBridge.get_interface(\"localStorage\")\n"),
        Variant::Nil
    );
    assert_eq!(
        run_val("return JavaScriptBridge.create_object(\"Object\")\n"),
        Variant::Nil
    );
    assert_eq!(
        run_val("return JavaScriptBridge.download_buffer(0, \"f.txt\", \"text/plain\")\n"),
        Variant::Nil
    );
    assert_eq!(
        run_val("return JavaScriptBridge.pwa_needs_update()\n"),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val("return JavaScriptBridge.is_js_buffer(0)\n"),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val("return JavaScriptBridge.js_buffer_to_packed_byte_array(0)\n"),
        Variant::PackedByteArray(Vec::new())
    );

    js_bridge::reset_backend();
}

/// An unknown method is still `UndefinedFunction` — routing through a backend
/// must not turn typos into silent no-ops.
#[test]
fn unknown_method_still_errors() {
    js_bridge::reset_backend();
    let mut interp = Interpreter::new();
    let err = interp
        .run("return JavaScriptBridge.no_such_method()\n")
        .expect_err("an unknown JavaScriptBridge method must error");
    assert!(
        format!("{err:?}").contains("no_such_method"),
        "the error must name the method, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

/// The ArrayBuffer helpers (pat-nzgdu.146) now answer from the backend rather
/// than from a hardcoded `false` / empty array.
#[test]
fn buffer_helpers_answer_from_the_backend() {
    install(RecordingJsBackend::new());

    // A PackedByteArray crosses as an ArrayBuffer, so it is a JS buffer and
    // converts straight back.
    assert_eq!(
        run_val("return JavaScriptBridge.is_js_buffer(PackedByteArray([1, 2, 3]))\n"),
        Variant::Bool(true)
    );
    assert_eq!(
        run_val(
            "return JavaScriptBridge.js_buffer_to_packed_byte_array(PackedByteArray([1, 2, 3]))\n"
        ),
        Variant::PackedByteArray(vec![1, 2, 3])
    );
    // A number is not a buffer.
    assert_eq!(
        run_val("return JavaScriptBridge.is_js_buffer(7)\n"),
        Variant::Bool(false)
    );

    js_bridge::reset_backend();
}
