//! pat-s3rrv.108 — IME state, display cutouts, and physical-keycode mapping.
//!
//! 🔑 The two that matter here are about **fallbacks that are easy to get
//! wrong in a way nothing notices**:
//!
//! * `keyboard_get_keycode_from_physical()` must return the keycode
//!   **unchanged** when there is no layout information. Godot implements the
//!   mapping only on Linux/macOS/Windows and hands the physical code back
//!   elsewhere. Answering `0` maps every physical key to `KEY_NONE`, which
//!   silently breaks every key-remapping UI that round-trips through it.
//! * `get_display_cutouts()` is typed **`Rect2`** (floats) while the closely
//!   related `get_display_safe_area()` is a `Rect2i`. The asymmetry is Godot's;
//!   assuming the two match is the easy mistake.
//!
//! One test function, as with the sibling files — `install_display_backend`
//! writes a process-wide slot and cargo runs a binary's tests in parallel.

use std::sync::Arc;

use gdcore::display_backend::{
    clear_display_backend, install_display_backend, HeadlessDisplayBackend,
};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func physical_letter():\n\
                   \x20   return DisplayServer.keyboard_get_keycode_from_physical(KEY_A)\n\
                   \n\
                   func physical_special():\n\
                   \x20   return DisplayServer.keyboard_get_keycode_from_physical(KEY_ESCAPE)\n\
                   \n\
                   func cutouts():\n\
                   \x20   return DisplayServer.get_display_cutouts()\n\
                   \n\
                   func ime_text():\n\
                   \x20   return DisplayServer.ime_get_text()\n\
                   \n\
                   func ime_sel():\n\
                   \x20   return DisplayServer.ime_get_selection()\n";

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
fn ime_state_cutouts_and_physical_keycode_mapping() {
    install_display_backend(Arc::new(HeadlessDisplayBackend::new(800, 600)));

    // 🔑 Identity fallback. KEY_A is 65 and must come back as 65, not 0.
    assert_eq!(
        call("physical_letter"),
        Variant::Int(65),
        "with no layout information the physical keycode must come back \
         UNCHANGED; a 0 here maps every key to KEY_NONE and breaks remapping"
    );
    // And the same for a special key, so the fallback is not accidentally
    // restricted to the ASCII range.
    assert_eq!(
        call("physical_special"),
        Variant::Int(4194305),
        "KEY_ESCAPE must round-trip too"
    );

    // A headless display has no notches; Godot answers an empty array rather
    // than null, so a script can iterate it without a guard.
    match call("cutouts") {
        Variant::Array(items) => assert!(
            items.is_empty(),
            "a device with no cutouts reports an EMPTY array, got {} entries",
            items.len()
        ),
        other => panic!("get_display_cutouts() must answer an Array, got {other:?}"),
    }

    assert_eq!(
        call("ime_text"),
        Variant::String(String::new()),
        "no IME composition is active, so the string is empty rather than absent"
    );

    // 🔑 Vector2i: x is the caret position, y the selection length.
    match call("ime_sel") {
        Variant::Vector2i(v) => assert_eq!(
            (v.x, v.y),
            (0, 0),
            "with no composition the caret and length are both zero"
        ),
        other => panic!("ime_get_selection() must answer a Vector2i, got {other:?}"),
    }

    clear_display_backend();
}
