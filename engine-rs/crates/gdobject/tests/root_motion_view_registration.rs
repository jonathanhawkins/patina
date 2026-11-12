//! pat-nzgdu.184 — the `RootMotionView` class surface.
//!
//! Covers the acceptance's ClassDB clause. The grid's accumulation is tested in
//! gdscene's `root_motion_view_grid_test`; the editor-viewport drawing is
//! follow-up. See the bead notes.
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json`; defaults
//! against its `--doctool` XML (the API dump carries no defaults).

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_property_list, instantiate, is_parent_class,
    register_3d_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// The acceptance's clause: `class_exists('RootMotionView')` with a
/// VisualInstance3D parent.
#[test]
fn root_motion_view_is_registered_under_visualinstance3d() {
    register_3d_classes();

    assert!(class_exists("RootMotionView"));
    assert!(
        is_parent_class("RootMotionView", "VisualInstance3D"),
        "RootMotionView DRAWS — it is a VisualInstance3D, not a bare Node3D"
    );
    // Must reach the root: an unregistered intermediate severs the chain
    // silently (pat-nzgdu.98).
    assert!(is_parent_class("RootMotionView", "Node3D"));
    assert!(is_parent_class("RootMotionView", "Node"));
    assert!(is_parent_class("RootMotionView", "Object"));

    // pat-mgixk.97: this asserted `is_abstract("VisualInstance3D")`.
    // `GDREGISTER_VIRTUAL_CLASS(VisualInstance3D)`
    // (scene/register_scene_types.cpp:621) — virtual keeps a creation func, so
    // `is_abstract` is false and extension_api.json reports
    // is_instantiable = true. RootMotionView is a plain GDREGISTER_CLASS, so
    // both are constructible and the contrast this line drew was not real.
    assert!(!is_abstract("VisualInstance3D"));
    assert!(!is_abstract("RootMotionView"));
    assert!(can_instantiate("RootMotionView"));
    let obj = instantiate("RootMotionView").expect("RootMotionView instantiates");
    assert_eq!(obj.get_class(), "RootMotionView");

    // It inherits the render layers — that is what a VisualInstance3D is for.
    assert!(props("RootMotionView").iter().any(|p| p == "layers"));
}

/// ⚠️ The bead's description lists `use_custom_color`. **Godot 4.6.1 has no such
/// property** — RootMotionView exposes exactly five. Registering it would invent
/// a schema, and a `.tscn` written against it would not round-trip in Godot.
#[test]
fn property_surface_is_exactly_godots_five() {
    register_3d_classes();

    let base = props("VisualInstance3D");
    let mut own: Vec<String> = props("RootMotionView")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    own.sort();

    // Exact, not a subset: this is what refuses the bead's invented
    // `use_custom_color`. A separate `!contains("use_custom_color")` assertion
    // would be unable to fail once this one passes.
    assert_eq!(
        own,
        vec![
            "animation_path".to_string(),
            "cell_size".to_string(),
            "color".to_string(),
            "radius".to_string(),
            "zero_y".to_string(),
        ],
        "these five are the whole surface Godot declares — note NO `use_custom_color`, \
         which the bead's description invents"
    );
}

/// Defaults machine-checked against the doc XML, not recalled.
#[test]
fn defaults_match_godot() {
    register_3d_classes();

    // A fresh gizmo tracks nothing until wired to an AnimationMixer.
    match default_of("RootMotionView", "animation_path") {
        // `is_empty()` asks the path itself rather than its rendering — a path
        // that merely *prints* empty would still track nothing but is a
        // different bug.
        Variant::NodePath(p) => assert!(
            p.is_empty(),
            "animation_path starts empty — the gizmo tracks nothing until wired, got {p:?}"
        ),
        other => panic!("animation_path must be a NodePath, got {other:?}"),
    }

    assert_eq!(
        default_of("RootMotionView", "cell_size"),
        Variant::Float(1.0)
    );
    assert_eq!(
        default_of("RootMotionView", "radius"),
        Variant::Float(10.0),
        "the grid extends 10 units, not 1 — a 1-unit grid would be invisible"
    );
    // ⚠️ Godot's default is a pale blue, NOT white: the grid must read as a
    // gizmo against the scene rather than as geometry.
    match default_of("RootMotionView", "color") {
        Variant::Color(c) => {
            assert!(
                (c.r - 0.5).abs() < 1e-6
                    && (c.g - 0.5).abs() < 1e-6
                    && (c.b - 1.0).abs() < 1e-6
                    && (c.a - 1.0).abs() < 1e-6,
                "Godot's default is Color(0.5, 0.5, 1, 1), got {c:?}"
            );
        }
        other => panic!("color must be a Color, got {other:?}"),
    }
    // True by default: a jump must not carry the ground plane up with it.
    assert_eq!(
        default_of("RootMotionView", "zero_y"),
        Variant::Bool(true),
        "the grid stays flat on XZ by default"
    );
}

/// ⚠️ Godot spells the bool getter `get_zero_y`, breaking its own `is_*` idiom
/// (contrast `is_end_bone_extended`, `are_bone_axes_mutable`). "Correcting" it
/// to `is_zero_y` would break parity.
#[test]
fn accessors_match_godots_naming() {
    register_3d_classes();

    for m in [
        "set_animation_path",
        "get_animation_path",
        "set_color",
        "get_color",
        "set_cell_size",
        "get_cell_size",
        "set_radius",
        "get_radius",
        "set_zero_y",
        "get_zero_y",
    ] {
        assert!(class_has_method("RootMotionView", m), "missing {m}");
    }
    assert!(
        !class_has_method("RootMotionView", "is_zero_y"),
        "Godot spells this getter `get_zero_y`, not `is_zero_y`"
    );
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("RootMotionView");
    register_3d_classes();
    assert_eq!(before, props("RootMotionView"));
}
