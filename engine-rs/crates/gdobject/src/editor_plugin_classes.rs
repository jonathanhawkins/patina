//! pat-0q2kx.241 — the `@tool` `EditorPlugin` scripting classes.
//!
//! `EditorCommandPalette`, `EditorContextMenuPlugin`, `EditorDebuggerPlugin`,
//! `EditorDebuggerSession` and `EditorDock` are the classes a Godot editor
//! addon scripts against. None existed in Patina.
//!
//! ## Why this is a separate module rather than more of `class_db.rs`
//!
//! `class_db.rs` is already ~11k lines, and its registration functions are
//! pulled in on demand (`register_2d_classes`, `register_root_window_chain`,
//! `register_editor_classes`, …) rather than from one central startup call, so
//! a new family in its own file fits the existing shape. `EditorState::new()`
//! calls [`register_editor_plugin_classes`] alongside
//! `class_db::register_editor_classes()`, so these are live in the served
//! editor and not merely available to tests.
//!
//! **Follow-up worth doing:** add a single
//! `crate::editor_plugin_classes::register_editor_plugin_classes();` line to
//! the end of `class_db::register_editor_classes()`. Then any existing caller
//! of that function picks these up automatically, and the split stops being
//! something a reader has to know about. It is left undone here only because
//! `class_db.rs` was held under an exclusive file lease by another agent for
//! the duration of this bead.
//!
//! ## Signatures come from the vendored Godot docs
//!
//! Every method name, arity, parent and property default below was read out of
//! `upstream/godot/doc/classes/<Class>.xml`, not inferred from the class name.
//! That mattered: an earlier draft of this file invented `set_dock_title`,
//! `set_dock_icon` and `set_default_dock_slot` for `EditorDock`, none of which
//! exist — its real surface is `open`/`close`/`make_visible` plus a `title`
//! property. A registration that is *wrong* is worse than one that is missing,
//! because `class_exists` still answers true.

use gdcore::math::Color;
use gdvariant::Variant;

use crate::class_db::{class_exists, register_class, ClassRegistration, MethodInfo, PropertyInfo};

/// Registers the `@tool` editor-plugin scripting classes.
///
/// Idempotent: each class is guarded by `class_exists`, matching the idiom used
/// throughout `class_db.rs`. Note that the guard makes a *duplicate*
/// registration a silent no-op rather than an error, so if these classes are
/// ever also declared elsewhere the loser's properties vanish without warning.
pub fn register_editor_plugin_classes() {
    // -- EditorCommandPalette (ConfirmationDialog) --
    //
    // Wired, not merely declared: the interpreter's `add_command` writes into
    // `gdscript_interop::editor_commands`, which the editor server merges into
    // `GET /api/commands`.
    //
    // Godot's surface here really is just these two methods. Patina's
    // interpreter also answers `has_command` and `get_command_count`, which are
    // NOT Godot API — they are deliberately left unregistered so ClassDB keeps
    // describing Godot rather than describing Patina.
    if !class_exists("EditorCommandPalette") {
        register_class(
            ClassRegistration::new("EditorCommandPalette")
                .parent("ConfirmationDialog")
                .method(MethodInfo::new("add_command", 4))
                .method(MethodInfo::new("remove_command", 1))
                // Overrides AcceptDialog's `true`; a palette that hid itself on
                // OK would dismiss before running the chosen command.
                .property(PropertyInfo::new("dialog_hide_on_ok", Variant::Bool(false))),
        );
    }

    // -- EditorContextMenuPlugin (RefCounted) --
    if !class_exists("EditorContextMenuPlugin") {
        register_class(
            ClassRegistration::new("EditorContextMenuPlugin")
                .parent("RefCounted")
                .method(MethodInfo::new("_popup_menu", 1))
                .method(MethodInfo::new("add_context_menu_item", 3))
                .method(MethodInfo::new("add_context_menu_item_from_shortcut", 3))
                .method(MethodInfo::new("add_context_submenu_item", 3))
                .method(MethodInfo::new("add_menu_shortcut", 2)),
        );
    }

    // -- EditorDebuggerPlugin (RefCounted) --
    if !class_exists("EditorDebuggerPlugin") {
        register_class(
            ClassRegistration::new("EditorDebuggerPlugin")
                .parent("RefCounted")
                .method(MethodInfo::new("_breakpoint_set_in_tree", 3))
                .method(MethodInfo::new("_breakpoints_cleared_in_tree", 0))
                .method(MethodInfo::new("_capture", 3))
                .method(MethodInfo::new("_goto_script_line", 2))
                .method(MethodInfo::new("_has_capture", 1))
                .method(MethodInfo::new("_setup_session", 1))
                .method(MethodInfo::new("get_session", 1))
                .method(MethodInfo::new("get_sessions", 0)),
        );
    }

    // -- EditorDebuggerSession (RefCounted) --
    //
    // Godot hands these out from `EditorDebuggerPlugin.get_session()` rather
    // than letting a script construct one.
    if !class_exists("EditorDebuggerSession") {
        register_class(
            ClassRegistration::new("EditorDebuggerSession")
                .parent("RefCounted")
                .method(MethodInfo::new("add_session_tab", 1))
                .method(MethodInfo::new("is_active", 0))
                .method(MethodInfo::new("is_breaked", 0))
                .method(MethodInfo::new("is_debuggable", 0))
                .method(MethodInfo::new("remove_session_tab", 1))
                .method(MethodInfo::new("send_message", 2))
                .method(MethodInfo::new("set_breakpoint", 3))
                .method(MethodInfo::new("toggle_profiler", 3)),
        );
    }

    // -- EditorDock (MarginContainer) --
    //
    // Marked `experimental` upstream, so its surface may move; the defaults
    // below are Godot 4.6's. `default_slot = -1` and `global = true` are the
    // two that a zero-initialised table would get wrong — -1 means "no explicit
    // slot", which is not slot 0.
    if !class_exists("EditorDock") {
        register_class(
            ClassRegistration::new("EditorDock")
                .parent("MarginContainer")
                .method(MethodInfo::new("_load_layout_from_config", 2))
                .method(MethodInfo::new("_save_layout_to_config", 2))
                .method(MethodInfo::new("_update_layout", 1))
                .method(MethodInfo::new("close", 0))
                .method(MethodInfo::new("make_visible", 0))
                .method(MethodInfo::new("open", 0))
                .property(PropertyInfo::new("available_layouts", Variant::Int(5)))
                .property(PropertyInfo::new("closable", Variant::Bool(false)))
                .property(PropertyInfo::new("default_slot", Variant::Int(-1)))
                .property(PropertyInfo::new("force_show_icon", Variant::Bool(false)))
                .property(PropertyInfo::new("global", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "icon_name",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "layout_key",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("title", Variant::String(String::new())))
                .property(PropertyInfo::new(
                    "title_color",
                    Variant::Color(Color::new(0.0, 0.0, 0.0, 0.0)),
                ))
                .property(PropertyInfo::new("transient", Variant::Bool(false))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class_db::{inheritance_chain, register_2d_classes, register_root_window_chain};

    /// Seeds the ancestors these classes attach to. ClassDB is not populated
    /// for free in a test process, and `inheritance_chain` stops walking at the
    /// first unregistered name — so without this the parent assertions would
    /// fail on missing ancestors rather than on anything registered here.
    fn seed() {
        register_2d_classes();
        register_root_window_chain();
        register_editor_plugin_classes();
    }

    #[test]
    fn all_five_classes_register_with_godots_parents() {
        seed();
        for (class, parent) in [
            ("EditorCommandPalette", "ConfirmationDialog"),
            ("EditorContextMenuPlugin", "RefCounted"),
            ("EditorDebuggerPlugin", "RefCounted"),
            ("EditorDebuggerSession", "RefCounted"),
            ("EditorDock", "MarginContainer"),
        ] {
            assert!(class_exists(class), "{class} must be registered");
            let chain = inheritance_chain(class);
            assert_eq!(
                chain.get(1).map(String::as_str),
                Some(parent),
                "{class} must extend {parent} directly; chain was {chain:?}",
            );
        }
    }

    /// The palette's chain must reach all the way to `Window`, which is what
    /// proves the dialog ancestors resolve rather than dead-ending.
    #[test]
    fn the_command_palette_chain_reaches_window() {
        seed();
        let chain = inheritance_chain("EditorCommandPalette");
        for expected in ["ConfirmationDialog", "AcceptDialog", "Window"] {
            assert!(
                chain.iter().any(|c| c == expected),
                "{expected} must appear in {chain:?}",
            );
        }
    }

    /// Registering twice must not duplicate or clobber.
    #[test]
    fn registration_is_idempotent() {
        seed();
        register_editor_plugin_classes();
        let chain = inheritance_chain("EditorDock");
        assert_eq!(chain.get(1).map(String::as_str), Some("MarginContainer"));
    }

    /// `default_slot` is -1, not 0. A zero-initialised default would silently
    /// dock every plugin panel into the first slot.
    #[test]
    fn dock_defaults_match_godot() {
        seed();
        let props = crate::class_db::get_property_list("EditorDock");
        // `get_property_list` returns INHERITED properties too, base-first, and
        // does not dedupe by name. Searching forwards would therefore return an
        // ancestor's declaration whenever a name is shadowed; searching from the
        // end takes the most-derived one, which is what EditorDock declares.
        let find = |n: &str| {
            props
                .iter()
                .rev()
                .find(|p| p.name == n)
                .unwrap_or_else(|| panic!("EditorDock must declare {n}"))
                .default_value
                .clone()
        };
        assert_eq!(find("default_slot"), Variant::Int(-1));
        assert_eq!(find("global"), Variant::Bool(true));
        assert_eq!(find("closable"), Variant::Bool(false));
    }
}
