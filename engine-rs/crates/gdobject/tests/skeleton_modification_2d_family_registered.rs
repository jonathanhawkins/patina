//! pat-s3rrv.135 — the `Skeleton2D` modification-stack classes are registered.
//!
//! Godot ground truth: `scene/register_scene_types.cpp:959-964`,
//! `scene/resources/2d/skeleton/skeleton_modification_2d.cpp`,
//! `skeleton_modification_stack_2d.cpp`, `skeleton_modification_2d_twoboneik.cpp`,
//! and the matching `doc/classes/*.xml`.
//!
//! `Skeleton2D` and `Bone2D` were already registered, so a `.tscn` rig loaded
//! fine — and its `modification_stack` was inert data with no class behind it.
//!
//! ⚠️ **Scope: registration only.** The stack is still never *executed*. The
//! two-bone solver exists (`gdscene::ik_solver_2d`) but has exactly one
//! reference in the whole workspace — its own `pub mod` line — and the
//! node/stack layer its header defers to "the caller" was never written. A
//! green run here does NOT mean a Skeleton2D poses its bones.

use gdobject::class_db::{
    can_instantiate, class_exists, get_method_list, get_property_list, register_all_classes,
};
use gdobject::is_parent_class;
use gdvariant::Variant;

fn arity_of(class_name: &str, method: &str) -> Option<usize> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == method)
        .map(|m| m.argument_count)
}

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

fn accessors_of(class_name: &str, property: &str) -> (String, String) {
    (
        gdobject::class_db::get_property_getter(class_name, property),
        gdobject::class_db::get_property_setter(class_name, property),
    )
}

#[test]
fn the_three_stack_classes_are_registered_with_godots_parents() {
    register_all_classes();
    assert!(is_parent_class("SkeletonModification2D", "Resource"));
    assert!(is_parent_class("SkeletonModificationStack2D", "Resource"));
    assert!(
        is_parent_class("SkeletonModification2DTwoBoneIK", "SkeletonModification2D"),
        "TwoBoneIK is a modification, so it inherits `enabled` and \
         `execution_mode` from the base rather than declaring its own"
    );
    assert!(is_parent_class(
        "SkeletonModification2DTwoBoneIK",
        "Resource"
    ));
}

/// 🔑 **NONE of the three is abstract**, including the base.
///
/// ⚠️ This is the one the bead itself got wrong — it asks for
/// `SkeletonModification2D (abstract, parent Resource)`. Upstream registers all
/// three with plain `GDREGISTER_CLASS` (`register_scene_types.cpp:959-964`).
/// Marking the base abstract would make `SkeletonModification2D.new()` fail
/// where Godot allows it, and the mistake is easy to make because the class
/// looks like a pure base.
#[test]
fn every_stack_class_is_instantiable_including_the_base() {
    register_all_classes();
    for class in [
        "SkeletonModification2D",
        "SkeletonModificationStack2D",
        "SkeletonModification2DTwoBoneIK",
    ] {
        assert!(
            can_instantiate(class),
            "{class} is GDREGISTER_CLASS upstream — not abstract"
        );
    }
}

/// 🔑 `enabled` defaults **true** on the modification and **false** on the stack.
///
/// ⚠️ Same property name, opposite defaults, one inheriting from the other's
/// sibling — the single most likely thing to get wrong by copying. Getting it
/// backwards either silently enables every stack in every scene or disables
/// every modification in every rig, and neither shows up as an error.
#[test]
fn enabled_defaults_are_opposite_on_the_modification_and_the_stack() {
    register_all_classes();
    assert_eq!(
        default_of("SkeletonModification2D", "enabled"),
        Some(Variant::Bool(true)),
        "a modification is enabled by default"
    );
    assert_eq!(
        default_of("SkeletonModificationStack2D", "enabled"),
        Some(Variant::Bool(false)),
        "a stack is NOT — Godot requires the rig to opt in"
    );
    assert_eq!(
        default_of("SkeletonModificationStack2D", "strength"),
        Some(Variant::Float(1.0))
    );
    assert_eq!(
        default_of("SkeletonModification2D", "execution_mode"),
        Some(Variant::Int(0)),
        "0 == process, 1 == physics_process"
    );
}

/// 🔑 `target_nodepath`'s accessors are `set_target_node` / `get_target_node`.
///
/// ⚠️ The member name and the accessor names disagree, so deriving either from
/// the other is wrong in both directions. Pinned because the derivation is the
/// obvious shortcut and produces a plausible-looking `set_target_nodepath` that
/// nothing calls.
#[test]
fn two_bone_ik_target_property_names_its_real_accessors() {
    register_all_classes();
    assert_eq!(
        accessors_of("SkeletonModification2DTwoBoneIK", "target_nodepath"),
        ("get_target_node".to_string(), "set_target_node".to_string()),
        "member is `target_nodepath`, accessors are `*_target_node`"
    );
    assert_eq!(
        default_of("SkeletonModification2DTwoBoneIK", "flip_bend_direction"),
        Some(Variant::Bool(false))
    );
    for member in ["target_minimum_distance", "target_maximum_distance"] {
        assert_eq!(
            default_of("SkeletonModification2DTwoBoneIK", member),
            Some(Variant::Float(0.0)),
            "{member} defaults to 0 — i.e. no clamp"
        );
    }
}

/// The bound method surface of all three, with arities.
#[test]
fn the_bound_method_surfaces_match_upstream() {
    register_all_classes();
    // `skeleton_modification_2d.cpp:213-226` — 10 bound + 3 GDVIRTUAL_BIND.
    for (method, argc) in [
        ("_execute", 1usize),
        ("_setup_modification", 1),
        ("_draw_editor_gizmo", 0),
        ("set_enabled", 1),
        ("get_enabled", 0),
        ("get_modification_stack", 0),
        ("set_is_setup", 1),
        ("get_is_setup", 0),
        ("set_execution_mode", 1),
        ("get_execution_mode", 0),
        ("clamp_angle", 4),
        ("set_editor_draw_gizmo", 1),
        ("get_editor_draw_gizmo", 0),
    ] {
        assert_eq!(
            arity_of("SkeletonModification2D", method),
            Some(argc),
            "SkeletonModification2D.{method}() takes {argc}"
        );
    }
    // `skeleton_modification_stack_2d.cpp` — 15 bound.
    for (method, argc) in [
        ("setup", 0usize),
        ("execute", 2),
        ("enable_all_modifications", 1),
        ("get_modification", 1),
        ("add_modification", 1),
        ("delete_modification", 1),
        ("set_modification", 2),
        ("set_modification_count", 1),
        ("get_modification_count", 0),
        ("get_is_setup", 0),
        ("set_enabled", 1),
        ("get_enabled", 0),
        ("set_strength", 1),
        ("get_strength", 0),
        ("get_skeleton", 0),
    ] {
        assert_eq!(
            arity_of("SkeletonModificationStack2D", method),
            Some(argc),
            "SkeletonModificationStack2D.{method}() takes {argc}"
        );
    }
    // `skeleton_modification_2d_twoboneik.cpp:471` — 16 bound.
    for (method, argc) in [
        ("set_target_node", 1usize),
        ("get_target_node", 0),
        ("set_target_minimum_distance", 1),
        ("get_target_minimum_distance", 0),
        ("set_target_maximum_distance", 1),
        ("get_target_maximum_distance", 0),
        ("set_flip_bend_direction", 1),
        ("get_flip_bend_direction", 0),
        ("set_joint_one_bone2d_node", 1),
        ("get_joint_one_bone2d_node", 0),
        ("set_joint_one_bone_idx", 1),
        ("get_joint_one_bone_idx", 0),
        ("set_joint_two_bone2d_node", 1),
        ("get_joint_two_bone2d_node", 0),
        ("set_joint_two_bone_idx", 1),
        ("get_joint_two_bone_idx", 0),
    ] {
        assert_eq!(
            arity_of("SkeletonModification2DTwoBoneIK", method),
            Some(argc),
            "SkeletonModification2DTwoBoneIK.{method}() takes {argc}"
        );
    }
}

/// The three virtuals are marked virtual, not concrete.
///
/// ⚠️ `_execute` is what a script overrides to write a custom modification. A
/// concrete registration would make `class_get_method_list` describe it as
/// callable engine API instead of an override point.
#[test]
fn the_modification_virtuals_are_marked_virtual() {
    register_all_classes();
    // OWN virtuals: `get_method_list` walks the chain, so it also returns
    // Resource's `_setup_local_to_scene`/`_get_rid`/`_reset_state`/
    // `_set_path_cache` and Object's virtuals — inherited override points, not
    // this class's. doc/classes/SkeletonModification2D.xml declares exactly 3.
    let virtuals: Vec<String> = gdobject::class_db::get_class_info("SkeletonModification2D")
        .expect("registered")
        .methods
        .into_iter()
        .filter(|m| m.is_virtual)
        .map(|m| m.name)
        .collect();
    for name in ["_execute", "_setup_modification", "_draw_editor_gizmo"] {
        assert!(
            virtuals.iter().any(|v| v == name),
            "{name} is GDVIRTUAL_BIND upstream and must be registered virtual; \
             got {virtuals:?}"
        );
    }
    assert_eq!(
        virtuals.len(),
        3,
        "exactly three virtuals — a concrete method marked virtual would \
         advertise an override point Godot does not have"
    );
}
