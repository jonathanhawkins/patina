//! pat-s3rrv.133 — the `JointLimitation3D` resource family exists, so
//! `IterateIK3D`'s limitation methods are no longer dangling signatures.
//!
//! Godot ground truth: `doc/classes/JointLimitation3D.xml` (inherits Resource),
//! `JointLimitationCone3D.xml` (inherits JointLimitation3D, member `angle`),
//! `LimitAngularVelocityModifier3D.xml` (inherits SkeletonModifier3D), and
//! `scene/register_scene_types.cpp:668-669`, which registers the base with
//! `GDREGISTER_VIRTUAL_CLASS` and the cone with `GDREGISTER_CLASS`.
//!
//! ⚠️ **A dangling signature is worse than a missing method.** `IterateIK3D`
//! already declared `set_joint_limitation` / `get_joint_limitation`, which take
//! and return a `JointLimitation3D` — a class ClassDB had never heard of. A
//! script could see the method existed and had no way to build its argument or
//! use what came back. Registering the methods without the type they traffic in
//! is what made this look finished; without the cone limit an IK chain solves
//! unconstrained and elbows bend the wrong way.

use gdobject::class_db::{
    can_instantiate, class_exists, get_method_list, get_property_list, instantiate,
    register_all_classes,
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

/// Clause 1 — the resource pair, with the cone's documented default.
#[test]
fn joint_limitation_family_is_registered_under_resource() {
    register_all_classes();
    assert!(class_exists("JointLimitation3D"));
    assert!(class_exists("JointLimitationCone3D"));
    assert!(
        is_parent_class("JointLimitationCone3D", "JointLimitation3D"),
        "the cone derives the abstract base"
    );
    assert!(
        is_parent_class("JointLimitationCone3D", "Resource"),
        "…and therefore Resource transitively — a limitation is a Resource, \
         which is what lets it be shared between joints and saved in a .tres"
    );
    // ⚠️ 1.5707964, transcribed from the doc XML, not written as `PI / 2.0`.
    // The documented value is the f32 rounding of a quarter turn; asserting the
    // f64 constant would disagree with the engine this mirrors.
    assert_eq!(
        default_of("JointLimitationCone3D", "angle"),
        Some(Variant::Float(1.5707964)),
        "JointLimitationCone3D.angle defaults to a quarter turn"
    );
}

/// Clause 3 — the base is abstract, the cone is not.
///
/// ⚠️ This pair is easy to get backwards, which is why both halves are
/// asserted. The base carries no properties and no bound methods at all, so it
/// reads like the trivial concrete one; upstream it is
/// `GDREGISTER_VIRTUAL_CLASS` while the cone beside it is `GDREGISTER_CLASS`.
#[test]
fn only_the_concrete_limitation_can_be_instantiated() {
    register_all_classes();
    assert!(
        can_instantiate("JointLimitationCone3D"),
        "JointLimitationCone3D is GDREGISTER_CLASS upstream and must be \
         constructible — a script has to be able to make one to pass it to \
         set_joint_limitation()"
    );
    assert!(
        instantiate("JointLimitationCone3D").is_some(),
        "…and instantiate() must actually hand one back"
    );
    assert!(
        !can_instantiate("JointLimitation3D"),
        "JointLimitation3D is GDREGISTER_VIRTUAL_CLASS upstream — an abstract \
         base. Reporting it constructible would let a script build a limitation \
         with no shape at all"
    );
}

/// Clause 2 — the modifier, with the two names that cannot be derived.
///
/// ⚠️ `exclude`'s getter is `is_exclude`, not `get_exclude`; and `joint_count`
/// is READ-ONLY, with the underscore-prefixed `_get_joint_count` as its getter
/// and no setter at all. Both come from
/// `limit_angular_velocity_modifier_3d.cpp:249-270`; deriving accessor names
/// from property names gets both wrong.
#[test]
fn limit_angular_velocity_modifier_carries_godots_surface() {
    register_all_classes();
    assert!(class_exists("LimitAngularVelocityModifier3D"));
    assert!(
        is_parent_class("LimitAngularVelocityModifier3D", "SkeletonModifier3D"),
        "it is a SkeletonModifier3D, so the skeleton's modifier pass runs it"
    );

    assert_eq!(
        default_of("LimitAngularVelocityModifier3D", "max_angular_velocity"),
        Some(Variant::Float(6.2831855)),
        "documented default is a full turn per second"
    );
    assert_eq!(
        default_of("LimitAngularVelocityModifier3D", "exclude"),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        default_of("LimitAngularVelocityModifier3D", "chain_count"),
        Some(Variant::Int(0))
    );

    for (method, argc) in [
        ("clear_chains", 0usize),
        ("reset", 0),
        ("set_root_bone", 2),
        ("get_root_bone", 1),
        ("set_root_bone_name", 2),
        ("get_root_bone_name", 1),
        ("set_end_bone", 2),
        ("get_end_bone", 1),
        ("set_end_bone_name", 2),
        ("get_end_bone_name", 1),
        ("set_chain_count", 1),
        ("get_chain_count", 0),
        ("set_max_angular_velocity", 1),
        ("get_max_angular_velocity", 0),
        ("set_exclude", 1),
        ("is_exclude", 0),
        ("_get_joint_count", 0),
    ] {
        assert_eq!(
            arity_of("LimitAngularVelocityModifier3D", method),
            Some(argc),
            "LimitAngularVelocityModifier3D.{method}() must take {argc} \
             argument(s) (`limit_angular_velocity_modifier_3d.cpp:249-270`)"
        );
    }
}

/// Clause 5 — 🔑 **no registered method may name a class ClassDB does not
/// know.** This is the check that would have caught the original bug.
///
/// ⚠️ It walks argument AND return `hint_string`s. Before this bead,
/// `IterateIK3D.set_joint_limitation` was registered with a bare arity and no
/// declared types, so a guard written only against declared types would have
/// passed *vacuously* on the very method that was broken. Typing those eight
/// methods is what makes this assertion bite — so the count of checked
/// references is asserted too, and a regression that drops the types back to
/// bare arities fails here rather than going quiet.
#[test]
fn no_registered_method_names_an_unknown_class() {
    register_all_classes();
    let mut checked = 0usize;
    for class in [
        "IterateIK3D",
        "ChainIK3D",
        "JointLimitationCone3D",
        "LimitAngularVelocityModifier3D",
    ] {
        for method in get_method_list(class) {
            let named = method
                .arguments
                .iter()
                .chain(method.return_value.iter())
                .filter(|p| !p.hint_string.is_empty());
            for p in named {
                // A hint_string is only a class name when it looks like one;
                // range hints and enum lists live in the same field.
                if !p.hint_string.starts_with(char::is_uppercase) || p.hint_string.contains(',') {
                    continue;
                }
                checked += 1;
                assert!(
                    class_exists(&p.hint_string),
                    "🔑 {class}.{}() names `{}`, which ClassDB does not know. \
                     That is a dangling signature: a script can see the method \
                     but cannot build its argument or use its return",
                    method.name,
                    p.hint_string
                );
            }
        }
    }
    assert!(
        checked >= 2,
        "the guard must have inspected at least the two JointLimitation3D \
         references on IterateIK3D; {checked} means the limitation methods lost \
         their declared types and this test went vacuous"
    );
}
