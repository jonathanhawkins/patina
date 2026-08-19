//! pat-s3rrv.108 — V-Sync mode, screen orientation, display safe area and
//! accent colour, routed through the installed `DisplayBackend`.
//!
//! ⚠️ Every assertion here is about a **documented default or sentinel**, not a
//! value a headless process could invent:
//!
//! * V-Sync defaults to `1` (`VSYNC_ENABLED`) — Godot's
//!   `display/window/vsync/vsync_mode` project-setting default, not "off".
//! * Orientation falls back to `SCREEN_LANDSCAPE` (0) even for an invalid
//!   screen (`DisplayServer.xml:1762`).
//! * The safe area defaults to the WHOLE screen. A zero rect would tell every
//!   caller there is nowhere safe to draw.
//! * The accent colour is `Color(0, 0, 0, 0)` when unknown
//!   (`DisplayServer.xml:847`) — transparent, so callers test the alpha. A
//!   plausible blue would be indistinguishable from a real theme colour.
//!
//! One test function, as with the sibling files: `install_display_backend`
//! writes a process-wide slot and cargo runs a binary's tests in parallel.

use std::sync::Arc;

use gdcore::display_backend::{
    clear_display_backend, install_display_backend, HeadlessDisplayBackend,
};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func vsync():\n\
                   \x20   return DisplayServer.window_get_vsync_mode()\n\
                   \n\
                   func set_vsync():\n\
                   \x20   DisplayServer.window_set_vsync_mode(0)\n\
                   \x20   return DisplayServer.window_get_vsync_mode()\n\
                   \n\
                   func bad_vsync():\n\
                   \x20   DisplayServer.window_set_vsync_mode(42)\n\
                   \x20   return DisplayServer.window_get_vsync_mode()\n\
                   \n\
                   func orient():\n\
                   \x20   return DisplayServer.screen_get_orientation()\n\
                   \n\
                   func set_orient():\n\
                   \x20   DisplayServer.screen_set_orientation(1)\n\
                   \x20   return DisplayServer.screen_get_orientation()\n\
                   \n\
                   func safe_area():\n\
                   \x20   return DisplayServer.get_display_safe_area()\n\
                   \n\
                   func accent():\n\
                   \x20   return DisplayServer.get_accent_color()\n";

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
fn vsync_orientation_safe_area_and_accent_colour() {
    install_display_backend(Arc::new(HeadlessDisplayBackend::new(800, 600)));

    // Godot's project default is ENABLED (1), not disabled.
    assert_eq!(
        call("vsync"),
        Variant::Int(1),
        "V-Sync must default to VSYNC_ENABLED (1), matching Godot's \
         display/window/vsync/vsync_mode default"
    );
    assert_eq!(
        call("set_vsync"),
        Variant::Int(0),
        "window_set_vsync_mode must be observable through the getter"
    );
    // VSyncMode runs 0..=3.
    assert_eq!(
        call("bad_vsync"),
        Variant::Int(0),
        "an out-of-range V-Sync mode must leave the previous one in force"
    );

    assert_eq!(
        call("orient"),
        Variant::Int(0),
        "orientation falls back to SCREEN_LANDSCAPE (0)"
    );
    assert_eq!(
        call("set_orient"),
        Variant::Int(1),
        "screen_set_orientation must round-trip"
    );

    // 🔑 The safe area is the whole 800x600 screen, not a zero rect.
    match call("safe_area") {
        Variant::Rect2i(r) => {
            assert_eq!(
                (r.position.x, r.position.y, r.size.x, r.size.y),
                (0, 0, 800, 600),
                "with no cutouts the safe area is the entire screen; a zero \
                 rect would tell callers there is nowhere safe to draw"
            );
        }
        other => panic!("get_display_safe_area() must answer a Rect2i, got {other:?}"),
    }

    // 🔑 The unknown-accent sentinel is fully TRANSPARENT, so callers can test
    // the alpha. Any opaque colour here would read as a real theme colour.
    match call("accent") {
        Variant::Color(c) => {
            assert_eq!(
                (c.r, c.g, c.b, c.a),
                (0.0, 0.0, 0.0, 0.0),
                "an unknown accent colour must be Color(0,0,0,0); an opaque \
                 value is indistinguishable from a real one"
            );
        }
        other => panic!("get_accent_color() must answer a Color, got {other:?}"),
    }

    clear_display_backend();
}
