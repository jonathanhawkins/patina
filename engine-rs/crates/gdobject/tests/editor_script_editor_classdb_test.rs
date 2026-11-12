//! pat-nzgdu.195 — the script-editor ClassDB surface.
//!
//! Covers 2 of the acceptance's 3 classes. **`ScriptCreateDialog` is NOT
//! registered** — its chain (`ConfirmationDialog ← AcceptDialog ← Window`) is
//! missing two substantial classes. The `EditorInterface.get_script_editor()`
//! binding is also follow-up. See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_signal, get_method_list, get_property_list, is_parent_class,
    register_editor_classes,
};

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's clause, for the two whose chains are intact.
#[test]
fn editor_script_editor_classdb_test() {
    register_editor_classes();

    assert!(class_exists("ScriptEditor"));
    assert!(
        is_parent_class("ScriptEditor", "PanelContainer"),
        "the bead's parent is right: ScriptEditor is a PanelContainer"
    );
    assert!(class_exists("ScriptEditorBase"));
    assert!(
        is_parent_class("ScriptEditorBase", "VBoxContainer"),
        "the bead's parent is right: ScriptEditorBase is a VBoxContainer"
    );

    // The chains must reach the root — an unregistered intermediate severs them
    // silently, and `class_exists` alone would not notice (pat-nzgdu.98). This
    // is exactly why PanelContainer had to be registered first.
    for class in ["ScriptEditor", "ScriptEditorBase"] {
        assert!(
            is_parent_class(class, "Container"),
            "{class} reaches Container"
        );
        assert!(is_parent_class(class, "Control"), "{class} reaches Control");
        assert!(is_parent_class(class, "Object"), "{class} reaches Object");
    }
}

/// ⚠️ BOTH are ABSTRACT, and the bead says so of neither.
///
/// A script never constructs the script editor — it gets the editor's one via
/// `EditorInterface.get_script_editor()`. `ScriptEditorBase` is a pane a plugin
/// *implements* for its own language, not one it instantiates.
#[test]
fn both_script_editor_classes_are_abstract() {
    register_editor_classes();

    assert!(
        is_abstract("ScriptEditor"),
        "Godot reports is_instantiable=false — you get the editor's, never your own"
    );
    assert!(!can_instantiate("ScriptEditor"));
    assert!(is_abstract("ScriptEditorBase"));
    assert!(!can_instantiate("ScriptEditorBase"));

    // Not vacuous: the container I had to add for the chain IS instantiable, so
    // `is_abstract` is discriminating here rather than always-true.
    assert!(!is_abstract("PanelContainer"));
    assert!(can_instantiate("PanelContainer"));
}

/// `PanelContainer` declares nothing of its own — a Container that paints a
/// panel. It was missing, which is what kept ScriptEditor out.
#[test]
fn panel_container_is_a_bare_container() {
    register_editor_classes();

    assert!(class_exists("PanelContainer"));
    assert!(is_parent_class("PanelContainer", "Container"));

    let base = props("Container");
    let own: Vec<String> = props("PanelContainer")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "PanelContainer declares no properties in Godot; found {own:?}"
    );
}

/// The API a plugin drives the script editor with — including the two the bead
/// names by hand.
#[test]
fn script_editor_exposes_the_plugin_api() {
    register_editor_classes();

    // The bead's two: get_open_scripts and open_script_create_dialog.
    assert_eq!(arity("ScriptEditor", "get_open_scripts"), 0);
    assert_eq!(
        arity("ScriptEditor", "open_script_create_dialog"),
        2,
        "open_script_create_dialog(base_path, base_name)"
    );

    assert_eq!(arity("ScriptEditor", "get_current_script"), 0);
    assert_eq!(arity("ScriptEditor", "get_current_editor"), 0);
    assert_eq!(arity("ScriptEditor", "get_open_script_editors"), 0);
    assert_eq!(arity("ScriptEditor", "get_breakpoints"), 0);
    assert_eq!(arity("ScriptEditor", "goto_line"), 1);

    // ⚠️ This is the hook pat-nzgdu.190's SyntaxHighlighter family plugs into —
    // a plugin adds a highlighter to every open script through here.
    assert_eq!(arity("ScriptEditor", "register_syntax_highlighter"), 1);
    assert_eq!(arity("ScriptEditor", "unregister_syntax_highlighter"), 1);

    assert!(class_has_signal("ScriptEditor", "editor_script_changed"));
    assert!(class_has_signal("ScriptEditor", "script_close"));
}

/// ⚠️ `ScriptEditorBase` is almost entirely a NOTIFIER: 2 methods, **10
/// signals**. The editor listens to a pane, not the other way round — an
/// implementation that only provides methods would be silent.
#[test]
fn script_editor_base_is_mostly_signals() {
    register_editor_classes();

    assert_eq!(arity("ScriptEditorBase", "get_base_editor"), 0);
    assert_eq!(arity("ScriptEditorBase", "add_syntax_highlighter"), 1);

    for sig in [
        "name_changed",
        "edited_script_changed",
        "request_help",
        "request_open_script_at_line",
        "request_save_history",
        "request_save_previous_state",
        "go_to_help",
        "search_in_files_requested",
        "replace_in_files_requested",
        "go_to_method",
    ] {
        assert!(
            class_has_signal("ScriptEditorBase", sig),
            "ScriptEditorBase must declare {sig}"
        );
    }

    // ⚠️ `request_help` and `go_to_help` are BOTH real and distinct — the pair
    // looks like a duplicate and invites deleting one.
    assert!(class_has_signal("ScriptEditorBase", "request_help"));
    assert!(class_has_signal("ScriptEditorBase", "go_to_help"));
}

/// If `ScriptCreateDialog` ever IS registered, its chain must be intact.
///
/// It is absent today (its parent `ConfirmationDialog ← AcceptDialog ← Window`
/// is missing — see the bead notes). This deliberately does NOT assert that
/// absence: a test that fails the moment someone correctly registers `Window`
/// would punish the very work this bead is asking for. It asserts the invariant
/// that actually matters instead — present or not, the chain must reach the root.
#[test]
fn script_create_dialog_chain_is_intact_if_present() {
    register_editor_classes();

    if class_exists("ScriptCreateDialog") {
        assert!(
            is_parent_class("ScriptCreateDialog", "ConfirmationDialog"),
            "ScriptCreateDialog is a ConfirmationDialog in Godot"
        );
        // The whole chain, or it is severed and class_exists is lying about
        // usability (pat-nzgdu.98).
        for ancestor in ["AcceptDialog", "Window", "Viewport", "Object"] {
            assert!(
                is_parent_class("ScriptCreateDialog", ancestor),
                "ScriptCreateDialog must reach {ancestor} — an unregistered \
                 intermediate severs the chain silently"
            );
        }
    }
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["PanelContainer", "ScriptEditor", "ScriptEditorBase"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_editor_classes();
    let first = ids();
    register_editor_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
