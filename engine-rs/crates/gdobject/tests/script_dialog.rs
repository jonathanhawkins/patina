//! pat-0q2kx.242 — `ScriptCreateDialog` and its dialog chain in the ClassDB.
//!
//! Godot ground truth: `editor/register_editor_types.cpp`
//! (`GDREGISTER_CLASS(ScriptCreateDialog)`) and the doc XML for each class.
//!
//! ⚠️ SCOPE: the class surface. The bead's acceptance also wants configuring
//! the dialog and confirming to PRODUCE a script resource and emit
//! `script_created`; that is editor-runtime behaviour and is NOT delivered, so
//! `script_create_dialog_registered_and_creates` does not exist. See the report.

use gdobject::class_db::{
    class_exists, get_class_info, instantiate, is_parent_class, register_root_window_chain,
};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

/// The bead asks only for ScriptCreateDialog, but both intermediates were
/// missing too. Registering under an absent parent severs the chain, so all
/// three must exist for the class to be usable at all.
#[test]
fn the_whole_dialog_chain_is_registered() {
    register_root_window_chain();

    for (child, parent) in [
        ("AcceptDialog", "Window"),
        ("ConfirmationDialog", "AcceptDialog"),
        ("ScriptCreateDialog", "ConfirmationDialog"),
    ] {
        assert!(class_exists(child), "{child} must be registered");
        assert!(
            is_parent_class(child, parent),
            "{child} must inherit {parent}"
        );
    }

    // The end-to-end chain, which is what a severed intermediate breaks while
    // every direct-parent check above still passes.
    assert!(
        is_parent_class("ScriptCreateDialog", "Window"),
        "ScriptCreateDialog must reach Window through both dialog bases"
    );
}

/// `config(inherits, path, built_in_enabled, load_enabled)` and the
/// `script_created` signal — the surface EditorPlugin code references.
#[test]
fn config_and_script_created_are_declared() {
    register_root_window_chain();
    let info = get_class_info("ScriptCreateDialog").expect("registered");

    let config = info
        .methods
        .iter()
        .find(|m| m.name == "config")
        .expect("ScriptCreateDialog must declare `config`");
    assert_eq!(
        config.argument_count, 4,
        "config takes (inherits, path, built_in_enabled, load_enabled)"
    );

    assert!(
        info.signals.iter().any(|s| s.name == "script_created"),
        "the `script_created` signal is how a plugin learns a script was made"
    );
}

/// AcceptDialog's defaults are mostly TRUE, and its title is "Alert!" — a
/// zero-initialised table would make every dialog non-exclusive, non-transient
/// and un-escapable, with no error anywhere.
#[test]
fn accept_dialog_defaults_are_not_zero_values() {
    register_root_window_chain();
    let d = instantiate("AcceptDialog").expect("instantiates");

    assert_eq!(
        d.get_property("title"),
        Variant::String("Alert!".to_string()),
        "AcceptDialog's title default is \"Alert!\", not empty"
    );
    for flag in [
        "dialog_close_on_escape",
        "dialog_hide_on_ok",
        "exclusive",
        "keep_title_visible",
        "maximize_disabled",
        "minimize_disabled",
        "transient",
        "wrap_controls",
    ] {
        assert_eq!(
            d.get_property(flag),
            Variant::Bool(true),
            "{flag} defaults to TRUE"
        );
    }
    // The two that genuinely are false, so the test is not just asserting
    // "everything is true".
    assert_eq!(d.get_property("visible"), Variant::Bool(false));
    assert_eq!(d.get_property("dialog_autowrap"), Variant::Bool(false));
}

/// ConfirmationDialog OVERRIDES the inherited title. A confirmation dialog
/// reading "Alert!" is the visible symptom of skipping the override.
#[test]
fn confirmation_dialog_overrides_the_inherited_title() {
    register_root_window_chain();

    let c = instantiate("ConfirmationDialog").expect("instantiates");
    assert_eq!(
        c.get_property("title"),
        Variant::String("Please Confirm...".to_string()),
        "ConfirmationDialog re-declares title; inheriting AcceptDialog's \
         \"Alert!\" would be wrong"
    );
    assert_eq!(
        c.get_property("cancel_button_text"),
        Variant::String("Cancel".to_string())
    );
    assert_eq!(
        c.get_property("min_size"),
        Variant::Vector2i(gdcore::math::Vector2i::new(200, 70))
    );

    // Inherited flags still come through the chain.
    assert_eq!(c.get_property("exclusive"), Variant::Bool(true));

    // ScriptCreateDialog inherits the confirmation title rather than the alert
    // one — proof the override resolves through two levels.
    let s = instantiate("ScriptCreateDialog").expect("instantiates");
    assert_eq!(
        s.get_property("title"),
        Variant::String("Please Confirm...".to_string()),
        "the nearer override must win over AcceptDialog's default"
    );
}
