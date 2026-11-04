//! pat-s3rrv.71 — the rest of Godot's Control tree exists in ClassDB.
//!
//! Diffing every `ClassRegistration::new("...")` against the 4.6.1
//! extension_api Control descendants left ~19 classes unregistered. Several
//! were ALREADY special-cased by name in the runtime — `control.rs` lays out
//! `HFlowContainer`, `control_render.rs` draws `NinePatchRect` — so the engine
//! could draw a class the editor could not create and `is_class` could not
//! resolve. Registration is what the Create Node dialog enumerates
//! (`create_dialog.rs` reads `get_class_list()`) and what `Node::is_class`
//! walks.
//!
//! ⚠️ These tests pin DEFAULTS, not just presence. A registered class with a
//! wrong default is worse than a missing one: `.tscn` loading fills unspecified
//! properties from ClassDB, so a `draw_center` that defaults to false blanks
//! the middle of every nine-patch panel and nothing reports an error.

use gdobject::class_db::{
    class_exists, get_class_info, is_parent_class, register_root_window_chain, PropertyInfo,
};
use gdobject::register_2d_classes;
use gdvariant::Variant;

/// Seeds the tables these classes live in.
///
/// ⚠️ `register_root_window_chain()` is needed as well as the 2D table: `Popup`
/// (and therefore `PopupPanel`) parents to `Window`, and `FileDialog` to
/// `ConfirmationDialog`, both of which are registered there. This split is
/// pre-existing — `Popup` already had it — and an unregistered base silently
/// severs the chain rather than erroring (pat-nzgdu.98).
fn seed() {
    register_2d_classes();
    register_root_window_chain();
}

fn property<'a>(class: &str, name: &'a str) -> PropertyInfo {
    let info = get_class_info(class).unwrap_or_else(|| panic!("{class} is not registered"));
    info.properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| {
            let have: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();
            panic!("{class}.{name} is not registered; has {have:?}")
        })
        .clone()
}

/// **The acceptance test**: every class exists, under Godot's parent.
#[test]
fn gui_control_classes_registered_with_godot_defaults() {
    seed();

    for (class, parent) in [
        // Range family.
        ("Range", "Control"),
        ("Slider", "Range"),
        ("HSlider", "Slider"),
        ("VSlider", "Slider"),
        ("ScrollBar", "Range"),
        ("HScrollBar", "ScrollBar"),
        ("VScrollBar", "ScrollBar"),
        ("SpinBox", "Range"),
        ("TextureProgressBar", "Range"),
        // Containers.
        ("AspectRatioContainer", "Container"),
        ("FlowContainer", "Container"),
        ("HFlowContainer", "FlowContainer"),
        ("VFlowContainer", "FlowContainer"),
        ("FoldableContainer", "Container"),
        // Buttons and rects.
        ("LinkButton", "BaseButton"),
        ("TextureButton", "BaseButton"),
        ("NinePatchRect", "Control"),
        // Tree and dialogs.
        ("Tree", "Control"),
        ("PopupPanel", "Popup"),
        ("FileDialog", "ConfirmationDialog"),
    ] {
        assert!(class_exists(class), "{class} must be registered");
        let info = get_class_info(class).unwrap_or_else(|| panic!("{class} info"));
        assert_eq!(
            info.parent_class, parent,
            "{class} must inherit {parent} — a wrong parent severs the chain \
             just as an absent one does",
        );
    }

    // The chain resolves all the way to Control, not merely one link. An
    // `is_class("Control")` that answers false is how an unregistered
    // intermediate shows up.
    for class in [
        "HSlider",
        "VScrollBar",
        "SpinBox",
        "HFlowContainer",
        "TextureButton",
        "Tree",
        "NinePatchRect",
        "AspectRatioContainer",
    ] {
        assert!(
            is_parent_class(class, "Control"),
            "{class} must resolve to Control through the full chain",
        );
    }
}

/// Godot's non-zero defaults, which are the ones a wrong guess silently breaks.
#[test]
fn the_non_zero_defaults_match_godot() {
    seed();

    // A nine-patch paints its centre unless told otherwise. `false` here would
    // blank the middle of every panel drawn through it.
    assert_eq!(
        property("NinePatchRect", "draw_center").default_value,
        Variant::Bool(true),
    );
    // A zero-column Tree can never draw a row.
    assert_eq!(property("Tree", "columns").default_value, Variant::Int(1));
    // AspectRatioContainer's alignment enums default to CENTER (1), not the
    // enum's zero BEGIN — seeding 0 would left-align every one of them.
    assert_eq!(
        property("AspectRatioContainer", "alignment_horizontal").default_value,
        Variant::Int(1),
    );
    assert_eq!(
        property("AspectRatioContainer", "alignment_vertical").default_value,
        Variant::Int(1),
    );
    assert_eq!(
        property("AspectRatioContainer", "ratio").default_value,
        Variant::Float(1.0),
    );
    // STRETCH_FIT (2) is Godot's default, not the enum's zero.
    assert_eq!(
        property("AspectRatioContainer", "stretch_mode").default_value,
        Variant::Int(2),
    );
    // Range spans 0..100 with a 0.01 step.
    assert_eq!(
        property("Range", "max_value").default_value,
        Variant::Float(100.0),
    );
    assert_eq!(
        property("Range", "step").default_value,
        Variant::Float(0.01)
    );
    // SpinBox overrides the step to 1.0 — it edits whole numbers by default.
    assert_eq!(
        property("SpinBox", "step").default_value,
        Variant::Float(1.0),
    );
    // A fresh FileDialog browses `res://` (ACCESS_RESOURCES = 0).
    assert_eq!(
        property("FileDialog", "access").default_value,
        Variant::Int(0)
    );
    // ScrollBar's custom_step is -1 (meaning "derive it"), not 0.
    assert_eq!(
        property("ScrollBar", "custom_step").default_value,
        Variant::Float(-1.0),
    );
}

/// Godot's abstract bases must not be instantiable (pat-s3rrv.71).
///
/// `Slider`, `ScrollBar`, `FlowContainer`, `Range` and `Separator` exist to hold
/// a shared surface; a script that could `.new()` one would get a control with
/// no orientation and no draw path.
#[test]
fn the_shared_bases_are_abstract() {
    seed();
    // ⚠️ NOT Range: it is GDREGISTER_VIRTUAL_CLASS upstream
    // (scene/register_scene_types.cpp:469), and a VIRTUAL class keeps its
    // creation function — `ClassDB.instantiate("Range")` works in Godot.
    assert!(
        !get_class_info("Range").expect("Range info").is_abstract,
        "Range is VIRTUAL (instantiable) in Godot, not ABSTRACT"
    );
    // Nor FlowContainer: `GDREGISTER_CLASS(FlowContainer)` upstream — it is a
    // usable container in its own right (vertical flag).
    for base in ["Slider", "ScrollBar", "Separator"] {
        let info = get_class_info(base).unwrap_or_else(|| panic!("{base} info"));
        assert!(
            info.is_abstract,
            "{base} is abstract in Godot and must not be instantiable",
        );
    }
    // …while the concrete leaves ARE instantiable, so the flag is real rather
    // than everything being marked abstract.
    for leaf in ["HSlider", "VScrollBar", "HFlowContainer", "HSeparator"] {
        let info = get_class_info(leaf).unwrap_or_else(|| panic!("{leaf} info"));
        assert!(!info.is_abstract, "{leaf} must be instantiable");
    }
}

/// The separators are reachable from the 2D table alone (pat-s3rrv.71).
///
/// They used to be registered only inside `register_editor_classes()`, so a
/// non-editor host that seeds the 2D table had no `HSeparator` at all. This
/// test calls ONLY `register_2d_classes()` — adding the editor registrar would
/// hide exactly the regression it exists to catch.
#[test]
fn separators_come_from_the_2d_table_not_the_editor_one() {
    register_2d_classes();
    for sep in ["Separator", "HSeparator", "VSeparator"] {
        assert!(
            class_exists(sep),
            "{sep} must be registered by register_2d_classes, not only by the \
             editor registrar",
        );
    }
    assert!(is_parent_class("HSeparator", "Control"));
}
