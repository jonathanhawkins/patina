//! pat-s3rrv.108 — the screen / window / cursor half of the DisplayServer
//! surface, routed through the installed `DisplayBackend`.
//!
//! ⚠️ The interesting assertions here are about **sentinels and types**, not
//! about values a headless process could invent:
//!
//! * `screen_get_refresh_rate()` must answer Godot's `-1.0` for "cannot
//!   determine", not a plausible `60.0`.
//! * `window_get_position()` must answer a `Vector2i`, which is what the docs
//!   specify.
//! * `cursor_set_shape` / `cursor_get_shape` must round-trip, and must ignore a
//!   shape outside the `CursorShape` enum rather than echoing it back.
//!
//! Same single-test structure as the sibling mouse-mode file, and for the same
//! reason: `install_display_backend` writes a process-wide slot and cargo runs
//! a binary's tests in parallel.

use std::sync::Arc;

use gdcore::display_backend::{
    clear_display_backend, install_display_backend, HeadlessDisplayBackend,
};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func refresh():\n\
                   \x20   return DisplayServer.screen_get_refresh_rate()\n\
                   \n\
                   func win_pos():\n\
                   \x20   return DisplayServer.window_get_position()\n\
                   \n\
                   func touch():\n\
                   \x20   return DisplayServer.is_touchscreen_available()\n\
                   \n\
                   func swap():\n\
                   \x20   return DisplayServer.get_swap_cancel_ok()\n\
                   \n\
                   func shape():\n\
                   \x20   return DisplayServer.cursor_get_shape()\n\
                   \n\
                   func set_shape():\n\
                   \x20   DisplayServer.cursor_set_shape(3)\n\
                   \x20   return DisplayServer.cursor_get_shape()\n\
                   \n\
                   func bad_shape():\n\
                   \x20   DisplayServer.cursor_set_shape(99)\n\
                   \x20   return DisplayServer.cursor_get_shape()\n";

/// Runs one probe method, mirroring the sibling tests' call sequence.
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
fn displayserver_reports_screen_window_and_cursor_state() {
    install_display_backend(Arc::new(HeadlessDisplayBackend::new(800, 600)));

    // 🔑 Godot's sentinel for "cannot determine the refresh rate" is -1.0
    // (`DisplayServer.xml:1797`). A headless process genuinely cannot know, and
    // answering a plausible 60.0 would have callers computing frame budgets
    // from a number nobody measured.
    assert_eq!(
        call("refresh"),
        Variant::Float(-1.0),
        "an unknown refresh rate must be Godot's -1.0 sentinel, not 0 and not a \
         made-up 60"
    );

    // 🔑 Vector2**i** — the docs specify integer screen coordinates.
    match call("win_pos") {
        Variant::Vector2i(_) => {}
        other => panic!(
            "window_get_position() must answer a Vector2i per the docs, got \
             {other:?}"
        ),
    }

    assert_eq!(
        call("touch"),
        Variant::Bool(false),
        "a headless backend has no touchscreen"
    );
    // Godot enables this on Windows; a headless backend follows no dialog
    // convention, so it must not claim the swap.
    assert_eq!(call("swap"), Variant::Bool(false));

    // CURSOR_ARROW is 0 and is the default.
    assert_eq!(
        call("shape"),
        Variant::Int(0),
        "the default cursor shape is CURSOR_ARROW (0)"
    );
    assert_eq!(
        call("set_shape"),
        Variant::Int(3),
        "cursor_set_shape must be observable through cursor_get_shape"
    );
    // ⚠️ CursorShape runs 0..=16 (CURSOR_MAX is 17), so 99 must leave the
    // previous shape in force rather than being stored and echoed back.
    assert_eq!(
        call("bad_shape"),
        Variant::Int(3),
        "an out-of-range cursor shape must not overwrite the shape in force"
    );

    clear_display_backend();
}
