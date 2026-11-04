//! pat-0q2kx.220 — the scene/main ClassDB surface.
//!
//! Godot ground truth: `register_scene_types.cpp:431/456/459/1044/1190/1191`,
//! `core/register_core_types.cpp:253`, and each class's doc XML.
//!
//! Seven classes the audit listed as absent. Six genuinely were; `World3D` was
//! already registered, so this adds `MainLoop`, `SceneTree`, `SceneTreeTimer`,
//! `World2D`, `MissingNode`, `ResourcePreloader` and `StatusIndicator`.
//!
//! ⚠️ SCOPE: registration and introspection only. The bead also asks for a
//! runtime `auto_accept_quit` path — a close-request delivering
//! `NOTIFICATION_WM_CLOSE_REQUEST` and ending the loop — which lives in
//! `gdscene/src/main_loop.rs` plus the runner/editor stop path. Not delivered;
//! the acceptance test `scene_main_class_surface_misc` does not exist. See the
//! completion report.

use gdobject::class_db::{
    class_exists, get_class_info, get_property_list, instantiate, is_abstract, is_parent_class,
    register_3d_classes,
};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

/// Each class and the parent its doc XML declares.
const SCENE_MAIN: [(&str, &str); 8] = [
    ("MainLoop", "Object"),
    ("SceneTree", "MainLoop"),
    ("SceneTreeTimer", "RefCounted"),
    ("World2D", "Resource"),
    ("World3D", "Resource"),
    ("MissingNode", "Node"),
    ("ResourcePreloader", "Node"),
    ("StatusIndicator", "Node"),
];

#[test]
fn scene_main_classes_are_registered_under_the_right_parents() {
    register_3d_classes();

    for (class, parent) in SCENE_MAIN {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, parent),
            "{class} must inherit {parent}"
        );
        // Every one of them must reach Object, which is what proves the chain
        // is intact rather than just the immediate link (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach the Object root"
        );
    }

    // SceneTree reaches Object *through* MainLoop. Asserting only the direct
    // parent would pass even if MainLoop itself were parented wrongly.
    assert!(
        is_parent_class("SceneTree", "MainLoop"),
        "SceneTree's chain must pass through MainLoop"
    );
    assert_eq!(
        get_class_info("SceneTree")
            .expect("registered")
            .parent_class,
        "MainLoop"
    );
}

/// Only `SceneTreeTimer` is abstract. Godot's comment on that line is literally
/// "sorry, you can't create it", while `SceneTree` beside it is a plain
/// `GDREGISTER_CLASS`.
#[test]
fn only_scene_tree_timer_is_abstract() {
    register_3d_classes();

    assert!(
        is_abstract("SceneTreeTimer"),
        "SceneTreeTimer is GDREGISTER_ABSTRACT_CLASS (register_scene_types.cpp:1191)"
    );
    for class in [
        "MainLoop",
        "SceneTree",
        "World2D",
        "MissingNode",
        "ResourcePreloader",
        "StatusIndicator",
    ] {
        assert!(
            !is_abstract(class),
            "{class} is a plain GDREGISTER_CLASS and must stay instantiable — \
             the bead's description calls SceneTree non-instantiable, which \
             contradicts register_scene_types.cpp:1190"
        );
    }
}

/// The three SceneTree flags that default to TRUE. A table initialised to
/// zero-values gets exactly these wrong, and silently: a scene would simply
/// refuse to quit on a window close.
#[test]
fn scene_tree_flag_defaults_match_the_doc_xml() {
    register_3d_classes();
    let tree = instantiate("SceneTree").expect("SceneTree instantiates");

    for flag in ["auto_accept_quit", "quit_on_go_back", "multiplayer_poll"] {
        assert_eq!(
            tree.get_property(flag),
            Variant::Bool(true),
            "{flag} defaults to TRUE in SceneTree.xml"
        );
    }
    for flag in [
        "paused",
        "physics_interpolation",
        "debug_collisions_hint",
        "debug_navigation_hint",
        "debug_paths_hint",
    ] {
        assert_eq!(
            tree.get_property(flag),
            Variant::Bool(false),
            "{flag} defaults to false"
        );
    }
}

/// `ResourcePreloader`'s methods are declared, so the existing
/// `gdscene/src/resource_preloader.rs` implementation is finally reachable for
/// introspection.
#[test]
fn resource_preloader_declares_its_full_method_surface() {
    register_3d_classes();
    let info = get_class_info("ResourcePreloader").expect("registered");

    for (name, arity) in [
        ("add_resource", 2usize),
        ("remove_resource", 1),
        ("rename_resource", 2),
        ("has_resource", 1),
        ("get_resource", 1),
        ("get_resource_list", 0),
    ] {
        let m = info
            .methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("ResourcePreloader must declare `{name}`"));
        assert_eq!(
            m.argument_count, arity,
            "`{name}` takes {arity} arguments (resource_preloader.rs)"
        );
    }
}

/// MainLoop declares the four script-overridable hooks a custom
/// `application/run/main_loop_type` needs.
#[test]
fn main_loop_declares_its_virtual_hooks() {
    register_3d_classes();
    let info = get_class_info("MainLoop").expect("registered");

    for (name, arity) in [
        ("_initialize", 0usize),
        ("_process", 1),
        ("_physics_process", 1),
        ("_finalize", 0),
    ] {
        let m = info
            .methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("MainLoop must declare `{name}`"));
        assert_eq!(m.argument_count, arity, "`{name}` arity");
    }
}

/// Typed defaults on the remaining classes. `PropertyInfo`'s default doubles as
/// the property's TYPE here, so a wrong-typed default misreports the property.
#[test]
fn remaining_class_defaults_are_type_correct() {
    register_3d_classes();

    // SceneTreeTimer is GDREGISTER_ABSTRACT_CLASS upstream ("sorry, you can't
    // create it", scene/register_scene_types.cpp:1191) — it comes from
    // `SceneTree.create_timer`. Its typed default is checked on the metadata.
    assert!(instantiate("SceneTreeTimer").is_none());
    assert_eq!(
        get_property_list("SceneTreeTimer")
            .into_iter()
            .find(|p| p.name == "time_left")
            .expect("time_left")
            .default_value,
        Variant::Float(0.0)
    );

    let missing = instantiate("MissingNode").expect("instantiates");
    assert_eq!(
        missing.get_property("original_class"),
        Variant::String(String::new())
    );
    assert_eq!(
        missing.get_property("recording_properties"),
        Variant::Bool(false)
    );

    let indicator = instantiate("StatusIndicator").expect("instantiates");
    assert_eq!(indicator.get_property("visible"), Variant::Bool(true));
    assert_eq!(
        indicator.get_property("tooltip"),
        Variant::String(String::new())
    );
    assert!(
        matches!(indicator.get_property("menu"), Variant::NodePath(_)),
        "`menu` is a NodePath, not a String — the default carries the type"
    );

    // World2D's RID-typed members register as Nil, matching World3D's existing
    // convention, because Patina's Variant has no RID.
    //
    // Asserted through the property LIST, not through get_property: a missing
    // property also reads back as Nil, so `get_property("canvas") == Nil` would
    // pass just as happily if World2D had never been registered at all.
    let props = gdobject::class_db::get_property_list("World2D");
    for name in ["canvas", "space", "navigation_map", "direct_space_state"] {
        let p = props
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("World2D must declare `{name}`"));
        assert_eq!(
            p.default_value,
            Variant::Nil,
            "`{name}` is an RID/object in Godot and has no Variant counterpart"
        );
    }
}

/// Re-registering must not duplicate entries in the property list.
#[test]
fn re_registering_does_not_duplicate_properties() {
    register_3d_classes();
    register_3d_classes();

    let count = gdobject::class_db::get_property_list("SceneTree")
        .iter()
        .filter(|p| p.name == "auto_accept_quit")
        .count();
    assert_eq!(count, 1, "a second pass must not double the property list");
}
