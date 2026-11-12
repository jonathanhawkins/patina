//! pat-s3rrv.71: every Control/Window descendant the bead named is registered,
//! with Godot's parent and defaults — and each is registered exactly ONCE.
//!
//! # What this actually guards
//!
//! ⚠️ The bead's premise — "these 24 classes are unregistered" — was **stale by
//! the time it was worked**: all 24 already had a `ClassRegistration`. What was
//! genuinely wrong is that three of them had **two**, both inside
//! `register_2d_classes()` behind `if !class_exists(..)`. The function runs top
//! to bottom, so the first always won and the second never executed. An edit
//! landing in the dead copy compiles, reads correctly, and does nothing — and it
//! already cost real time: the pat-s3rrv.57 signal pass skipped `Slider`
//! precisely because it could not tell which copy would win.
//!
//! A duplicate registration is invisible at runtime (the registry holds one
//! entry either way), so this cannot assert on the duplication directly. What it
//! can do — and does — is pin the properties that existed ONLY in the surviving
//! block. Delete the wrong copy and those vanish.

use gdobject::class_db::{
    class_exists, get_property_list, inheritance_chain, register_all_classes,
};
use gdvariant::Variant;

fn props(class_name: &str) -> Vec<String> {
    register_all_classes();
    get_property_list(class_name)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class_name: &str, prop: &str) -> Option<Variant> {
    register_all_classes();
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == prop)
        .map(|p| p.default_value)
}

/// Every class the bead named exists. A regression guard rather than a new
/// claim: they were already registered, and this keeps them that way.
#[test]
fn the_named_control_family_is_registered() {
    register_all_classes();
    for class_name in [
        "AspectRatioContainer",
        "CenterContainer",
        "CheckButton",
        "FlowContainer",
        "HFlowContainer",
        "VFlowContainer",
        "FoldableContainer",
        "LinkButton",
        "NinePatchRect",
        "TextureButton",
        "TextureProgressBar",
        "Slider",
        "HSlider",
        "VSlider",
        "ScrollBar",
        "HScrollBar",
        "VScrollBar",
        "SpinBox",
        "Tree",
        "FileDialog",
        "PopupPanel",
        "Separator",
        "HSeparator",
        "VSeparator",
    ] {
        assert!(class_exists(class_name), "{class_name} must be registered");
    }
}

/// ⚠️ **The regression test for the de-duplication.** These properties existed
/// only in the registration that WINS. If someone ever removes the surviving
/// block instead of the dead one — or re-adds a second block that shadows it —
/// they disappear here, which is the only runtime-visible symptom a duplicate
/// registration has.
#[test]
fn the_surviving_registration_is_the_complete_one() {
    for (class_name, only_in_the_winner) in [
        ("Slider", &["focus_mode", "step", "ticks_position"][..]),
        (
            "SpinBox",
            &[
                "alignment",
                "custom_arrow_round",
                "custom_arrow_step",
                "size_flags_vertical",
            ][..],
        ),
        (
            "TextureProgressBar",
            &[
                "radial_center_offset",
                "radial_fill_degrees",
                "radial_initial_angle",
                "stretch_margin_left",
                "texture_progress_offset",
            ][..],
        ),
    ] {
        let have = props(class_name);
        for p in only_in_the_winner {
            assert!(
                have.contains(&p.to_string()),
                "{class_name}.{p} is gone — the richer registration was lost to a \
                 duplicate; got {have:?}"
            );
        }
    }
}

/// Godot's parent chain, so a script's `is`/`get_parent_class` resolves.
///
/// ⚠️ Checked through `inheritance_chain` rather than one link: a severed chain
/// higher up (Slider -> Range -> Control) is what actually breaks `class_inherits`,
/// and asserting only the immediate parent would not see it.
#[test]
fn the_family_hangs_off_godots_parents() {
    register_all_classes();
    for (class_name, ancestor) in [
        ("AspectRatioContainer", "Container"),
        ("CenterContainer", "Container"),
        ("HFlowContainer", "Container"),
        ("CheckButton", "BaseButton"),
        ("LinkButton", "BaseButton"),
        ("TextureButton", "BaseButton"),
        ("HSlider", "Range"),
        ("VScrollBar", "Range"),
        ("SpinBox", "Range"),
        ("TextureProgressBar", "Range"),
        ("NinePatchRect", "Control"),
        ("Tree", "Control"),
        ("HSeparator", "Control"),
    ] {
        let chain = inheritance_chain(class_name);
        assert!(
            chain.iter().any(|c| c == ancestor),
            "{class_name} must descend from {ancestor}; chain was {chain:?}"
        );
        assert!(
            chain.iter().any(|c| c == "Control"),
            "{class_name} must reach Control; chain was {chain:?}"
        );
    }
}

/// ⚠️ Defaults taken from `upstream/godot/doc/classes/*.xml`, not from what the
/// code happens to hold — a default read back off the implementation proves only
/// that it is self-consistent. The non-zero ones are the point: a class whose
/// registration was written from a template tends to have every field at zero,
/// and `draw_center = true` / `ratio = 1.0` / `columns = 1` are exactly what such
/// a template gets wrong.
#[test]
fn the_documented_non_zero_defaults_are_carried() {
    assert_eq!(
        default_of("AspectRatioContainer", "ratio"),
        Some(Variant::Float(1.0)),
        "AspectRatioContainer.ratio defaults to 1.0, not 0.0"
    );
    assert_eq!(
        default_of("NinePatchRect", "draw_center"),
        Some(Variant::Bool(true)),
        "NinePatchRect.draw_center defaults to TRUE — a false default draws a \
         hollow frame for every authored nine-patch"
    );
    assert_eq!(
        default_of("Tree", "columns"),
        Some(Variant::Int(1)),
        "Tree.columns defaults to 1; 0 would mean a tree with no cells"
    );
    assert_eq!(
        default_of("TextureButton", "ignore_texture_size"),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        default_of("FileDialog", "show_hidden_files"),
        Some(Variant::Bool(false))
    );
}
