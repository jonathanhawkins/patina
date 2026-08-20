//! pat-s3rrv.156 — every `EditorPlugin` virtual the editor DISPATCHES must stay
//! declared in ClassDB.
//!
//! `editor_server.rs` runs plugin virtuals through a generic dispatcher
//! (`:5071`) that iterates `state.active_plugins` and **skips any instance
//! where `instance.has_method(method)` is false**. `has_method` resolves
//! through ClassDB. So a declaration removed or renamed here does not produce a
//! compile error or a failing call — it makes the dispatcher quietly skip every
//! plugin, and the override stops running with nothing reported.
//!
//! ⚠️ This guards the direction that actually breaks. The bead this came from
//! asks for *more* declarations; the real risk is losing one the engine already
//! depends on.
//!
//! ⚠️ **Deliberately not the reverse check.** ClassDB currently declares 22
//! EditorPlugin virtuals while the editor dispatches 12 of them (plus `_process`
//! and `_ready`, which are inherited from Node rather than declared here). The
//! other ten are override points nothing calls yet. Asserting "declared implies
//! dispatched" would fail today and would be asserting an aspiration, not a
//! contract — so this file checks only the implication that must hold.

use gdobject::class_db::{class_has_method, get_method_list, register_all_classes};

/// The `EditorPlugin` virtuals `editor_server.rs` actually calls, with the
/// arity ClassDB records for each.
///
/// ⚠️ Hand-maintained against the dispatch sites, because the dispatcher takes
/// the method name as a `&str` parameter — there is nothing for a test to
/// enumerate. If you add a dispatch site, add it here; that is the point of the
/// list, not an oversight.
const DISPATCHED: &[(&str, usize)] = &[
    ("_apply_changes", 0),
    ("_build", 0),
    ("_clear", 0),
    ("_edit", 1),
    ("_forward_canvas_gui_input", 1),
    ("_get_plugin_name", 0),
    ("_get_state", 0),
    ("_handles", 1),
    ("_has_main_screen", 0),
    ("_make_visible", 1),
    ("_save_external_data", 0),
    ("_set_state", 1),
];

/// 🔑 Losing any of these silently disables the override.
#[test]
fn every_dispatched_editor_plugin_virtual_is_declared() {
    register_all_classes();
    for (name, _) in DISPATCHED {
        assert!(
            class_has_method("EditorPlugin", name),
            "🔑 `editor_server.rs` dispatches `EditorPlugin.{name}()`, and its \
             dispatcher skips any plugin whose `has_method` says no. Dropping \
             this declaration does not fail a build or raise at runtime — it \
             makes the override stop running, silently"
        );
    }
}

/// The arities must match too.
///
/// ⚠️ A declaration with the wrong argument count still satisfies
/// `has_method`, so the dispatcher would call it and the script would receive
/// the wrong number of arguments — a failure that surfaces inside the plugin
/// rather than at the seam.
#[test]
fn the_dispatched_virtuals_keep_their_arities() {
    register_all_classes();
    let methods = get_method_list("EditorPlugin");
    for (name, argc) in DISPATCHED {
        let m = methods
            .iter()
            .find(|m| &m.name == name)
            .unwrap_or_else(|| panic!("EditorPlugin.{name}() must be declared"));
        assert_eq!(
            m.argument_count, *argc,
            "EditorPlugin.{name}() is dispatched expecting {argc} argument(s)"
        );
    }
}

/// They are declared as VIRTUALS, not as concrete engine methods.
///
/// ⚠️ These are override points a plugin script supplies. Registered concrete,
/// `class_get_method_list` would describe them as callable engine API and the
/// editor's own class docs would invite scripts to call them.
#[test]
fn the_dispatched_virtuals_are_marked_virtual() {
    register_all_classes();
    let methods = get_method_list("EditorPlugin");
    for (name, _) in DISPATCHED {
        let m = methods.iter().find(|m| &m.name == name).expect("declared");
        assert!(
            m.is_virtual,
            "EditorPlugin.{name}() is an override point, not engine API"
        );
    }
}
