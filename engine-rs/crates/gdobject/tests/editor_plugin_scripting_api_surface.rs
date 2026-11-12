//! pat-nzgdu.191 — the editor plugin scripting API's ClassDB surface.
//!
//! Covers the acceptance's `ClassDB.class_exists` clause for the 9 classes whose
//! parent chains already exist. The script binding over the real backends, the
//! `EditorInterface` accessors, and the 3 classes needing new intermediate
//! parents are follow-up — see the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, class_has_signal, get_method_list, get_property_list,
    instantiate, is_parent_class, register_editor_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

/// The 9 registered here. The other 3 (`EditorScriptPicker`, `EditorSpinSlider`,
/// `FileSystemDock`) need parents Patina lacks — see the bead notes.
const REGISTERED: &[(&str, &str)] = &[
    ("EditorSettings", "Resource"),
    ("EditorSelection", "Object"),
    ("EditorUndoRedoManager", "Object"),
    ("EditorScript", "RefCounted"),
    ("EditorVCSInterface", "Object"),
    ("EditorResourcePreviewGenerator", "RefCounted"),
    ("EditorResourceTooltipPlugin", "RefCounted"),
    ("EditorTranslationParserPlugin", "RefCounted"),
    ("EditorToaster", "HBoxContainer"),
];

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

/// The acceptance's clause, for the 9 with intact chains.
#[test]
fn editor_plugin_scripting_api_surface() {
    register_editor_classes();

    for (class, parent) in REGISTERED {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, parent),
            "{class} must derive {parent}"
        );
        // Must reach the root: an unregistered intermediate severs the chain
        // silently, and `class_exists` alone would not notice (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object"
        );
    }
}

/// ⚠️ Two of these are ABSTRACT, and the bead says so of neither.
///
/// A script never constructs an `EditorUndoRedoManager` — it gets the editor's
/// via `EditorPlugin.get_undo_redo()`. Likewise the editor owns the one
/// `EditorToaster`; a plugin only pushes to it.
#[test]
fn the_editor_owned_singletons_are_abstract() {
    register_editor_classes();

    assert!(
        is_abstract("EditorUndoRedoManager"),
        "Godot reports is_instantiable=false — a script gets the editor's, never its own"
    );
    assert!(!can_instantiate("EditorUndoRedoManager"));
    assert!(is_abstract("EditorToaster"));
    assert!(!can_instantiate("EditorToaster"));

    // The rest a plugin really does construct.
    for class in [
        "EditorSettings",
        "EditorSelection",
        "EditorScript",
        "EditorVCSInterface",
        "EditorResourcePreviewGenerator",
        "EditorResourceTooltipPlugin",
        "EditorTranslationParserPlugin",
    ] {
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ The bead's description names methods Godot does not have.
///
/// `EditorSettings.get_setting_list` and `property_get_revert` do not exist —
/// the real reader is `get_setting`/`has_setting`, and revert lives on `Object`.
/// Registering them would promise a plugin an API that errors on call.
#[test]
fn editor_settings_surface_is_godots_not_the_beads() {
    register_editor_classes();

    // What actually exists, with real arities.
    assert_eq!(arity("EditorSettings", "has_setting"), 1);
    assert_eq!(arity("EditorSettings", "set_setting"), 2);
    assert_eq!(arity("EditorSettings", "get_setting"), 1);
    assert_eq!(arity("EditorSettings", "erase"), 1);
    // Godot's settings emit this when anything changes — the hook a plugin
    // watches to react to preference edits.
    assert!(class_has_signal("EditorSettings", "settings_changed"));

    for invented in ["get_setting_list", "property_get_revert"] {
        assert!(
            !class_has_method("EditorSettings", invented),
            "the bead invents `{invented}`; Godot's EditorSettings has no such method"
        );
    }

    // ⚠️ It is a Resource, not a plain Object — that is what lets the editor
    // save it to disk. (Patina's `Resource` currently declares no properties of
    // its own, so there is nothing inherited to assert here; that gap is
    // pre-existing and not this bead's.)
    assert!(is_parent_class("EditorSettings", "Resource"));
    // ⚠️ This used to assert the NEGATION, locking in a real bug: Patina
    // registered `Resource` under `Object`, making `Resource` and `RefCounted`
    // siblings. Godot has `class Resource : public RefCounted`
    // (core/io/resource.h; extension_api Resource <- RefCounted <- Object), so
    // every Resource — this one included — IS a RefCounted (pat-0q2kx.7).
    assert!(is_parent_class("EditorSettings", "RefCounted"));

    // EditorSettings declares no properties in Godot — its whole surface is the
    // get/set/has methods above. Inventing one would be schema fiction.
    let base = props("Resource");
    let own: Vec<String> = props("EditorSettings")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "EditorSettings declares no properties in Godot; found {own:?}"
    );
}

/// ⚠️ `add_node`/`remove_node`, not `add`/`remove` (the bead's shorthand). And
/// `get_top_selected_nodes` is a THIRD getter, distinct from the two the bead
/// names — it drops nodes whose parent is also selected.
#[test]
fn editor_selection_has_godots_three_getters_and_node_suffixed_mutators() {
    register_editor_classes();

    assert_eq!(arity("EditorSelection", "add_node"), 1);
    assert_eq!(arity("EditorSelection", "remove_node"), 1);
    assert_eq!(arity("EditorSelection", "clear"), 0);
    for shorthand in ["add", "remove"] {
        assert!(
            !class_has_method("EditorSelection", shorthand),
            "Godot spells it `{shorthand}_node`, not `{shorthand}`"
        );
    }

    for getter in [
        "get_selected_nodes",
        "get_top_selected_nodes",
        "get_transformable_selected_nodes",
    ] {
        assert_eq!(arity("EditorSelection", getter), 0, "{getter}");
    }
    assert!(class_has_signal("EditorSelection", "selection_changed"));
}

/// The undo manager's per-history API — the part that makes editor undo work
/// across scenes rather than one global stack.
#[test]
fn undo_redo_manager_exposes_per_history_actions() {
    register_editor_classes();

    assert_eq!(arity("EditorUndoRedoManager", "create_action"), 1);
    assert_eq!(arity("EditorUndoRedoManager", "commit_action"), 0);
    // (object, method) then (object, property, value) — the shapes a plugin
    // records with.
    assert_eq!(arity("EditorUndoRedoManager", "add_do_method"), 2);
    assert_eq!(arity("EditorUndoRedoManager", "add_undo_method"), 2);
    assert_eq!(arity("EditorUndoRedoManager", "add_do_property"), 3);
    assert_eq!(arity("EditorUndoRedoManager", "add_undo_property"), 3);
    // The editor keeps a separate history per scene/resource; these find it.
    assert_eq!(arity("EditorUndoRedoManager", "get_object_history_id"), 1);
    assert_eq!(arity("EditorUndoRedoManager", "get_history_undo_redo"), 1);

    assert!(class_has_signal("EditorUndoRedoManager", "history_changed"));
    assert!(class_has_signal("EditorUndoRedoManager", "version_changed"));
}

/// `EditorScript._run()` is the one-shot entry point the acceptance names.
#[test]
fn editor_script_declares_the_run_hook() {
    register_editor_classes();
    assert!(
        class_has_method("EditorScript", "_run"),
        "_run is the hook a @tool one-shot overrides"
    );
    assert_eq!(arity("EditorScript", "_run"), 0);
    assert_eq!(arity("EditorScript", "add_root_node"), 1);
    assert_eq!(arity("EditorScript", "get_scene"), 0);
    assert_eq!(arity("EditorScript", "get_editor_interface"), 0);

    // ⚠️ RefCounted, not Resource: an EditorScript is run and dropped, never
    // saved as an asset.
    assert!(is_parent_class("EditorScript", "RefCounted"));
    assert!(!is_parent_class("EditorScript", "Resource"));
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        REGISTERED
            .iter()
            .map(|(c, _)| gdobject::get_class_info(c).expect("registered").class_id)
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
