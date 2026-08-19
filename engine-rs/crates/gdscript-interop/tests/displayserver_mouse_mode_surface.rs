//! pat-s3rrv.108 — `DisplayServer.get_name()` and the mouse-mode / dark-mode
//! surface, routed through the installed `DisplayBackend`.
//!
//! # The gap
//!
//! The DisplayServer method table stopped at ~20 entries and everything else
//! fell to `UndefinedFunction("DisplayServer.{other}")`. Among the casualties:
//! `get_name()` — which Godot always answers, and which is the normal way a
//! script detects the web export — and the whole pointer surface, so
//! `mouse_set_mode(MOUSE_MODE_CAPTURED)`, the first line of every mouse-look
//! game's `_ready`, raised instead of doing anything.
//!
//! # ⚠️ Why this is ONE test
//!
//! `install_display_backend` writes a PROCESS-WIDE slot and cargo runs a
//! binary's tests on parallel threads. Two tests each installing a backend
//! would race, and one asserting the default pointer mode while another had
//! just captured the pointer would fail intermittently. Everything therefore
//! runs sequentially in one test, and the backend is cleared at the end so it
//! cannot leak — `clear_display_backend` exists for exactly this reason.

use std::sync::Arc;

use gdcore::display_backend::{
    clear_display_backend, install_display_backend, HeadlessDisplayBackend,
};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// A probe class whose methods exercise one DisplayServer call each.
const SRC: &str = "extends Node\n\
                   \n\
                   func name_of():\n\
                   \x20   return DisplayServer.get_name()\n\
                   \n\
                   func mode():\n\
                   \x20   return DisplayServer.mouse_get_mode()\n\
                   \n\
                   func capture():\n\
                   \x20   DisplayServer.mouse_set_mode(2)\n\
                   \x20   return DisplayServer.mouse_get_mode()\n\
                   \n\
                   func out_of_range():\n\
                   \x20   DisplayServer.mouse_set_mode(99)\n\
                   \x20   return DisplayServer.mouse_get_mode()\n\
                   \n\
                   func dark_supported():\n\
                   \x20   return DisplayServer.is_dark_mode_supported()\n\
                   \n\
                   func dark():\n\
                   \x20   return DisplayServer.is_dark_mode()\n\
                   \n\
                   func pointer():\n\
                   \x20   return DisplayServer.mouse_get_position()\n";

/// Runs one probe method.
///
/// Mirrors the call sequence the sibling `classdb_call_static_*` test uses —
/// `run_class` → `instantiate_class` → `call_instance_method` — rather than
/// inventing one.
fn call(method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("DisplayServer.{method} raised: {e:?}"))
}

#[test]
fn displayserver_reports_its_name_and_round_trips_mouse_mode() {
    install_display_backend(Arc::new(HeadlessDisplayBackend::new(800, 600)));

    // `get_name()` used to raise UndefinedFunction. Godot's headless display
    // server answers "headless"; a web build answers "web", which is what a
    // script branches on to detect the web export.
    assert_eq!(
        call("name_of"),
        Variant::String("headless".to_string()),
        "DisplayServer.get_name() must answer the driver name, not raise"
    );

    // MOUSE_MODE_VISIBLE is 0 and is the default.
    assert_eq!(
        call("mode"),
        Variant::Int(0),
        "the pointer starts VISIBLE (MouseMode 0)"
    );

    // 🔑 The round trip: MOUSE_MODE_CAPTURED is 2.
    assert_eq!(
        call("capture"),
        Variant::Int(2),
        "mouse_set_mode must be observable through mouse_get_mode; Nil or 0 \
         here means the call was accepted and dropped"
    );

    // ⚠️ Out of range is IGNORED, not echoed. Godot's enum stops at
    // MOUSE_MODE_MAX (5); storing 99 would make mouse_get_mode() report a mode
    // the engine has no meaning for, which is worse than reporting the mode
    // actually in force.
    //
    // Note this runs on a FRESH interpreter whose backend is the one installed
    // above, so the mode it reads back is the captured 2 from the previous
    // call — the state lives in the backend, not the interpreter.
    assert_eq!(
        call("out_of_range"),
        Variant::Int(2),
        "an out-of-range mode must leave the previous one in force rather than \
         being stored and echoed back"
    );

    // 🔑 `is_dark_mode` and `is_dark_mode_supported` are DIFFERENT questions.
    // Godot answers false from is_dark_mode() both when the system is light and
    // when it cannot tell; the supported flag distinguishes them. Aliasing the
    // two would make "cannot tell" look like "light".
    assert_eq!(
        call("dark_supported"),
        Variant::Bool(false),
        "a headless backend cannot tell, so it must report dark mode UNSUPPORTED"
    );
    assert_eq!(
        call("dark"),
        Variant::Bool(false),
        "and it must not claim dark mode"
    );

    // The pointer position answers a Vector2 rather than raising.
    match call("pointer") {
        Variant::Vector2(_) => {}
        other => panic!("mouse_get_position() must answer a Vector2, got {other:?}"),
    }

    clear_display_backend();
}
