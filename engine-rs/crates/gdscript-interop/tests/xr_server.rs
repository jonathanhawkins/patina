//! pat-0q2kx.236 — `XRServer` and `WebXRInterface` reachable from GDScript.
//!
//! `gdplatform/src/xr.rs` has had a complete, natively-tested WebXR session
//! state machine since pat-nzgdu.150 — and **zero consumers**, because
//! `gdplatform/src/lib.rs` re-exported only `{XrPose, XrServer}`. It was
//! unreachable, not merely unused. The interpreter had no `XRServer` arm at
//! all, so any script touching it raised `UndefinedVariable`.
//!
//! Binary named `xr_server`: the acceptance's own
//! `xr_server_script_singleton` would make the verify command 70 characters,
//! past the 58-char truncation where the target silently resolves to nothing.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn call_check(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

/// `XRServer.get_hmd_transform()` returns a Transform3D rather than erroring.
#[test]
fn hmd_transform_is_reachable() {
    let src = "\
extends Node

func check():
    return XRServer.get_hmd_transform()
";
    assert!(
        matches!(call_check(src), Variant::Transform3D(_)),
        "XRServer.get_hmd_transform() must return a Transform3D",
    );
}

/// `find_interface` yields an object for WebXR and **null** for anything else,
/// matching Godot's answer for an unregistered interface.
#[test]
fn find_interface_returns_webxr_and_null_otherwise() {
    let src = "\
extends Node

func check():
    var xr = XRServer.find_interface(\"WebXR\")
    var nope = XRServer.find_interface(\"OpenXR\")
    return [xr != null, nope]
";
    match call_check(src) {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::Bool(true), "WebXR must resolve");
            assert_eq!(v[1], Variant::Nil, "an unknown interface must be null");
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// The same interface object comes back each time — Godot's XRServer owns its
/// interfaces, so a second lookup must not hand out a fresh session state.
#[test]
fn find_interface_is_stable_across_calls() {
    let src = "\
extends Node

func check():
    var a = XRServer.find_interface(\"WebXR\")
    a.set_session_mode(\"immersive-vr\")
    var b = XRServer.find_interface(\"WebXR\")
    return b.get_session_mode()
";
    assert_eq!(
        call_check(src),
        Variant::String("immersive-vr".to_string()),
        "a second find_interface must see the first one's state",
    );
}

/// Session properties round-trip through the handle.
#[test]
fn session_mode_and_features_round_trip() {
    let src = "\
extends Node

func check():
    var xr = XRServer.find_interface(\"WebXR\")
    xr.set_session_mode(\"immersive-ar\")
    xr.set_required_features([\"local-floor\"])
    return [xr.get_session_mode(), xr.get_required_features()]
";
    match call_check(src) {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::String("immersive-ar".to_string()));
            match &v[1] {
                Variant::PackedStringArray(f) => {
                    assert_eq!(f, &vec!["local-floor".to_string()])
                }
                other => panic!("expected a PackedStringArray, got {other:?}"),
            }
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

// ===========================================================================
// The acceptance: signals reach script callbacks
// ===========================================================================

/// **The acceptance test.** A script connects to `session_started`, stubs the
/// browser into granting the session, calls `initialize()`, and observes the
/// callback fire.
///
/// This is the part that needed new infrastructure: a handle is not a scene
/// node, so neither of the interpreter's existing `connect` paths could bind
/// to it.
#[test]
fn xr_server_script_singleton() {
    let src = "\
extends Node

var started = 0

func _on_started():
    started += 1

func check():
    var xr = XRServer.find_interface(\"WebXR\")
    xr.connect(\"session_started\", _on_started)
    xr.set_session_mode(\"immersive-vr\")
    xr.set_required_features([\"local-floor\"])
    xr._set_test_stub([\"immersive-vr\"], [\"local-floor\"], \"\")
    var ok = xr.initialize()
    return [ok, started, xr.is_initialized()]
";
    match call_check(src) {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::Bool(true), "initialize() must succeed");
            assert_eq!(
                v[1],
                Variant::Int(1),
                "the session_started callback must have fired exactly once",
            );
            assert_eq!(
                v[2],
                Variant::Bool(true),
                "the interface reports initialized"
            );
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// `session_failed` carries its message — a required feature the stubbed
/// browser will not grant must fail with a reason, not silently.
#[test]
fn session_failed_delivers_its_message() {
    let src = "\
extends Node

var reason = \"\"

func _on_failed(message):
    reason = message

func check():
    var xr = XRServer.find_interface(\"WebXR\")
    xr.connect(\"session_failed\", _on_failed)
    xr.set_session_mode(\"immersive-vr\")
    xr.set_required_features([\"hand-tracking\"])
    # The browser grants immersive-vr but NOT hand-tracking.
    xr._set_test_stub([\"immersive-vr\"], [\"local-floor\"], \"\")
    var ok = xr.initialize()
    return [ok, reason]
";
    match call_check(src) {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::Bool(false), "the session must fail");
            match &v[1] {
                Variant::String(msg) => assert!(
                    !msg.is_empty(),
                    "session_failed must carry a reason, got an empty string",
                ),
                other => panic!("expected a String message, got {other:?}"),
            }
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// `session_supported(mode, supported)` carries both arguments.
#[test]
fn session_supported_carries_mode_and_answer() {
    let src = "\
extends Node

var seen_mode = \"\"
var seen_ok = false

func _on_supported(mode, supported):
    seen_mode = mode
    seen_ok = supported

func check():
    var xr = XRServer.find_interface(\"WebXR\")
    xr.connect(\"session_supported\", _on_supported)
    xr._set_test_stub([\"immersive-vr\"], [], \"\")
    xr.is_session_supported(\"immersive-vr\")
    return [seen_mode, seen_ok]
";
    match call_check(src) {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::String("immersive-vr".to_string()));
            assert_eq!(v[1], Variant::Bool(true));
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}
