//! pat-s3rrv.57 — signals Godot documents that ClassDB did not carry.
//!
//! The bead's headline (reflection asks ClassDB rather than a hardcoded name
//! list) was already done. What was left is metadata depth: a class can be
//! registered, resolve its whole inheritance chain, and still report an
//! incomplete signal list, because `get_signal_list` can only return what was
//! declared at registration.
//!
//! That is the registered-but-inert shape again, one layer out — and for
//! `size_changed` it was an outright lie: `scene_tree.rs:671` emits it on the
//! root every time the viewport resizes, while `class_has_signal("Window",
//! "size_changed")` answered `false`. A script connecting to it via reflection
//! would be told the signal does not exist on a node that emits it.
//!
//! ⚠️ These assertions check ARGUMENT NAMES, not just presence. A signal
//! registered with the right name and the wrong arity still misreports itself
//! to `ClassDB.class_get_signal_list`, and a presence-only test is green for
//! it.

use gdobject::class_db::{self, SignalInfo};

/// `(class, signal, argument names)` for every signal Godot documents on the
/// classes this bead touched.
///
/// 🔑 GENERATED FROM `upstream/godot/doc/classes/*.xml`, not from `class_db.rs`.
/// A table read back out of our own registration source would assert only that
/// the file equals itself; the point here is agreement with Godot.
///
/// Godot's XML lists a class's OWN signals, so each entry is asserted against
/// the class that declares it — declaring `visibility_changed` on `Node2D`
/// instead of `CanvasItem` would still let a `Sprite2D` see it while
/// `class_get_signal_list("CanvasItem")` reported it missing.
const EXPECTED: &[(&str, &str, &[&str])] = &[
    // Node — doc/classes/Node.xml, 11 signal(s).
    ("Node", "child_entered_tree", &["node"]),
    ("Node", "child_exiting_tree", &["node"]),
    ("Node", "child_order_changed", &[]),
    ("Node", "editor_description_changed", &["node"]),
    ("Node", "editor_state_changed", &[]),
    ("Node", "ready", &[]),
    ("Node", "renamed", &[]),
    ("Node", "replacing_by", &["node"]),
    ("Node", "tree_entered", &[]),
    ("Node", "tree_exited", &[]),
    ("Node", "tree_exiting", &[]),
    // CanvasItem — doc/classes/CanvasItem.xml, 4 signal(s).
    ("CanvasItem", "draw", &[]),
    ("CanvasItem", "hidden", &[]),
    ("CanvasItem", "item_rect_changed", &[]),
    ("CanvasItem", "visibility_changed", &[]),
    // Sprite2D — doc/classes/Sprite2D.xml, 2 signal(s).
    ("Sprite2D", "frame_changed", &[]),
    ("Sprite2D", "texture_changed", &[]),
    // Viewport — doc/classes/Viewport.xml, 2 signal(s).
    ("Viewport", "gui_focus_changed", &["node"]),
    ("Viewport", "size_changed", &[]),
    // Window — doc/classes/Window.xml, 15 signal(s).
    ("Window", "about_to_popup", &[]),
    ("Window", "close_requested", &[]),
    ("Window", "dpi_changed", &[]),
    ("Window", "files_dropped", &["files"]),
    ("Window", "focus_entered", &[]),
    ("Window", "focus_exited", &[]),
    ("Window", "go_back_requested", &[]),
    ("Window", "mouse_entered", &[]),
    ("Window", "mouse_exited", &[]),
    ("Window", "nonclient_window_input", &["event"]),
    ("Window", "theme_changed", &[]),
    ("Window", "title_changed", &[]),
    ("Window", "titlebar_changed", &[]),
    ("Window", "visibility_changed", &[]),
    ("Window", "window_input", &["event"]),
    // Resource — doc/classes/Resource.xml, 2 signal(s).
    ("Resource", "changed", &[]),
    ("Resource", "setup_local_to_scene_requested", &[]),
    // Slider — doc/classes/Slider.xml, 2 signal(s).
    ("Slider", "drag_ended", &["value_changed"]),
    ("Slider", "drag_started", &[]),
    // Path3D — doc/classes/Path3D.xml, 2 signal(s).
    ("Path3D", "curve_changed", &[]),
    ("Path3D", "debug_color_changed", &[]),
    // Skeleton2D — doc/classes/Skeleton2D.xml, 1 signal(s).
    ("Skeleton2D", "bone_setup_changed", &[]),
    // XRController3D — doc/classes/XRController3D.xml, 5 signal(s).
    ("XRController3D", "button_pressed", &["name"]),
    ("XRController3D", "button_released", &["name"]),
    ("XRController3D", "input_float_changed", &["name", "value"]),
    (
        "XRController3D",
        "input_vector2_changed",
        &["name", "value"],
    ),
    ("XRController3D", "profile_changed", &["role"]),
    // AnimationLibrary — doc/classes/AnimationLibrary.xml, 4 signal(s).
    ("AnimationLibrary", "animation_added", &["name"]),
    ("AnimationLibrary", "animation_changed", &["name"]),
    ("AnimationLibrary", "animation_removed", &["name"]),
    (
        "AnimationLibrary",
        "animation_renamed",
        &["name", "to_name"],
    ),
    // EditorPlugin — doc/classes/EditorPlugin.xml, 6 signal(s).
    ("EditorPlugin", "main_screen_changed", &["screen_name"]),
    ("EditorPlugin", "project_settings_changed", &[]),
    ("EditorPlugin", "resource_saved", &["resource"]),
    ("EditorPlugin", "scene_changed", &["scene_root"]),
    ("EditorPlugin", "scene_closed", &["filepath"]),
    ("EditorPlugin", "scene_saved", &["filepath"]),
];

fn find(class: &str, signal: &str) -> Option<SignalInfo> {
    class_db::get_signal_list(class)
        .into_iter()
        .find(|s| s.name == signal)
}

/// Each documented signal is registered, carrying exactly its documented
/// arguments.
#[test]
fn documented_signals_are_registered_with_their_arguments() {
    class_db::register_all_classes();

    for (class, signal, args) in EXPECTED {
        let info = find(class, signal).unwrap_or_else(|| {
            panic!(
                "{class}.{signal} is documented in Godot but absent from \
                 ClassDB; get_signal_list({class}) returned {:?}",
                class_db::get_signal_list(class)
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(
            info.arguments, *args,
            "{class}.{signal} must carry exactly the documented arguments",
        );
    }
}

/// 🔑 The signal a script can actually observe being emitted.
///
/// `SceneTree` emits `size_changed` on the root, which is a `Window`. Godot
/// declares it on `Viewport`, so reflection only answers correctly if the
/// chain carries it down — asserting it on `Viewport` alone would pass while
/// the class scripts actually hold still reported `false`.
#[test]
fn size_changed_resolves_through_the_window_chain() {
    class_db::register_all_classes();

    for class in ["Viewport", "Window", "SubViewport"] {
        assert!(
            class_db::class_has_signal(class, "size_changed"),
            "{class} must report size_changed; the engine emits it on the root",
        );
    }
}

/// Registering a signal must not disturb the ones already there.
///
/// `register()` merges by member name and skips a signal whose name is already
/// present, so adding to a class with several registration sites is a no-op
/// after the first. This pins that: the classes touched above keep the signals
/// they already had.
#[test]
fn preexisting_signals_survive_the_additions() {
    class_db::register_all_classes();

    for (class, signal) in [
        ("Node", "tree_entered"),
        ("Node", "ready"),
        ("Resource", "changed"),
        ("Range", "value_changed"),
    ] {
        assert!(
            class_db::class_has_signal(class, signal),
            "{class}.{signal} must still be registered",
        );
    }

    // No class may register the same signal name twice: `get_signal_list`
    // concatenates the chain without deduping across ancestors, but a single
    // class declaring a duplicate would mean the merge guard failed.
    for class in [
        // Window and Viewport are declared at TWO registration sites each,
        // and CanvasItem's signals were added to a site guarded by
        // `!class_exists`. These are the cases that would duplicate if the
        // merge in `register()` stopped deduping by name.
        "Window",
        "Viewport",
        "CanvasItem",
        "Slider",
        "Path3D",
        "XRController3D",
    ] {
        let names: Vec<String> = class_db::get_class_info(class)
            .unwrap_or_else(|| panic!("{class} is registered"))
            .signals
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "{class} declares a duplicate signal: {names:?}",
        );
    }
}
