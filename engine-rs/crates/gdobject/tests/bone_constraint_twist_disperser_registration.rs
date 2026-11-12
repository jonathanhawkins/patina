//! pat-nzgdu.182 — `SkeletonModifier3D` base + `BoneConstraint3D` /
//! `BoneTwistDisperser3D` class surface.
//!
//! **This does NOT satisfy the bead's acceptance.** That asks for a constraint
//! to drive a bone's global pose and a twist to disperse across a chain, run
//! from the SceneTree tick (`boneconstraint3d_copies_target_and_twist_disperses`
//! — which does not exist). This covers only the registration clause. See the
//! bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`; property
//! defaults against its `--doctool` XML (the API dump carries no defaults).

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, class_has_method, class_has_signal,
    get_method_list, get_property_list, instantiate, is_parent_class, register_3d_classes,
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

/// Properties `class` declares itself — `get_property_list` walks the whole
/// chain, so the inherited SkeletonModifier3D surface must be subtracted.
fn own_props(class: &str) -> Vec<String> {
    let base = props("SkeletonModifier3D");
    props(class)
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// Required-argument count of `method` on `class`. `get_method_list` returns the
/// chain base-first, so search from the end: the derived-most wins.
fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

#[test]
fn constraint_classes_are_registered_with_godot_parents() {
    register_3d_classes();

    for class in ["BoneConstraint3D", "BoneTwistDisperser3D"] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "SkeletonModifier3D"),
            "{class} is a SkeletonModifier3D — that is what puts it in the pose stack"
        );
        // Must reach the root: an unregistered intermediate severs the chain
        // silently, and `class_exists` alone would not notice (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "Node3D"),
            "{class} must reach Node3D"
        );
        assert!(is_parent_class(class, "Node"));
        assert!(is_parent_class(class, "Object"));

        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);

        // Both inherit the modifier controls — that is how a constraint is
        // toggled and blended rather than being all-or-nothing.
        for p in ["active", "influence"] {
            assert!(props(class).iter().any(|h| h == p), "{class} inherits {p}");
        }
    }
}

/// ⚠️ `SkeletonModifier3D` is NOT abstract. Patina previously marked it
/// `.abstract_class()`; Godot reports `is_instantiable=true` (contrast
/// `PhysicsBody3D`, which reports false). A bare modifier can sit in a scene and
/// simply modify nothing.
#[test]
fn skeleton_modifier_base_is_instantiable_not_abstract() {
    register_3d_classes();
    assert!(
        !is_abstract("SkeletonModifier3D"),
        "Godot's SkeletonModifier3D is instantiable"
    );
    assert!(can_instantiate("SkeletonModifier3D"));
    let obj = instantiate("SkeletonModifier3D").expect("the base instantiates");
    assert_eq!(obj.get_class(), "SkeletonModifier3D");

    // Sibling check: PhysicsBody3D really is abstract, so the flag is meaningful
    // and this assertion is not vacuously true.
    assert!(is_abstract("PhysicsBody3D"));

    assert_eq!(
        default_of("SkeletonModifier3D", "active"),
        Variant::Bool(true)
    );
    assert_eq!(
        default_of("SkeletonModifier3D", "influence"),
        Variant::Float(1.0),
        "a modifier applies fully until dialled down"
    );
    assert!(class_has_signal(
        "SkeletonModifier3D",
        "modification_processed"
    ));
}

/// The virtuals a modifier subclass overrides. `_process_modification` is the
/// pose-update hook the constraint/twist solvers must eventually run from, so a
/// derived modifier has to inherit it.
#[test]
fn base_declares_the_modification_virtuals() {
    register_3d_classes();
    for (m, argc) in [
        ("_process_modification", 0),
        ("_process_modification_with_delta", 1),
        ("_skeleton_changed", 2),
        ("_validate_bone_names", 0),
    ] {
        assert!(
            class_has_method("SkeletonModifier3D", m),
            "the base must declare {m}"
        );
        assert_eq!(arity("SkeletonModifier3D", m), argc, "{m} arity");
        assert!(
            class_has_method("BoneConstraint3D", m),
            "a derived modifier inherits {m} — that is what it overrides"
        );
    }
}

/// The four axis/direction enums live on the base so every modifier shares one
/// vocabulary — and are therefore reachable from the derived classes.
#[test]
fn base_axis_enums_are_shared_with_derived_modifiers() {
    register_3d_classes();
    for e in [
        "BoneAxis",
        "BoneDirection",
        "SecondaryDirection",
        "RotationAxis",
    ] {
        assert!(class_has_enum("SkeletonModifier3D", e), "base declares {e}");
        assert!(
            class_has_enum("BoneConstraint3D", e),
            "BoneConstraint3D inherits {e} from the base"
        );
    }
    assert_eq!(
        class_get_integer_constant("BoneTwistDisperser3D", "BONE_AXIS_PLUS_X"),
        Some(0),
        "a derived modifier resolves the base's constants"
    );
    // BoneDirection mirrors BoneAxis but adds FROM_PARENT — no fixed-axis twin.
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "BONE_DIRECTION_FROM_PARENT"),
        Some(6)
    );
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "ROTATION_AXIS_ALL"),
        Some(3)
    );
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "ROTATION_AXIS_CUSTOM"),
        Some(4)
    );
}

/// ⚠️ `SecondaryDirection` is offset by one against `BoneAxis`: it reserves 0
/// for NONE. Copying BoneAxis's numbering would make PLUS_X silently mean "no
/// secondary direction".
#[test]
fn secondary_direction_reserves_zero_for_none() {
    register_3d_classes();
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "BONE_AXIS_PLUS_X"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "SECONDARY_DIRECTION_NONE"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "SECONDARY_DIRECTION_PLUS_X"),
        Some(1),
        "PLUS_X is 1 here but 0 in BoneAxis — the enums are NOT interchangeable"
    );
    assert_eq!(
        class_get_integer_constant("SkeletonModifier3D", "SECONDARY_DIRECTION_CUSTOM"),
        Some(7)
    );
}

/// ⚠️ `BoneConstraint3D` declares NO properties of its own. Its settings are
/// per-index and Godot synthesises `settings/N/...` at runtime from
/// `setting_count`; registering `amount`/`apply_bone` as plain properties would
/// invent a schema Godot does not have.
#[test]
fn bone_constraint_declares_no_static_properties() {
    register_3d_classes();
    let own = own_props("BoneConstraint3D");
    assert!(
        own.is_empty(),
        "BoneConstraint3D's surface is indexed methods, not properties; found {own:?}"
    );
    assert!(class_has_enum("BoneConstraint3D", "ReferenceType"));
    // A setting references either a bone or an arbitrary node — hence both
    // reference_bone and reference_node accessors exist.
    assert_eq!(
        class_get_integer_constant("BoneConstraint3D", "REFERENCE_TYPE_BONE"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("BoneConstraint3D", "REFERENCE_TYPE_NODE"),
        Some(1)
    );
    for m in ["set_reference_bone", "set_reference_node"] {
        assert!(class_has_method("BoneConstraint3D", m));
    }
}

/// Every constraint accessor is indexed by its setting: setters take
/// (index, value), getters take (index). An unindexed `set_amount(amount)`
/// would address the wrong setting.
#[test]
fn constraint_accessors_are_indexed_by_setting() {
    register_3d_classes();
    for m in [
        "set_amount",
        "set_apply_bone_name",
        "set_apply_bone",
        "set_reference_type",
        "set_reference_bone_name",
        "set_reference_bone",
        "set_reference_node",
    ] {
        assert_eq!(arity("BoneConstraint3D", m), 2, "{m}(index, value)");
    }
    for m in [
        "get_amount",
        "get_apply_bone_name",
        "get_apply_bone",
        "get_reference_type",
        "get_reference_bone_name",
        "get_reference_bone",
        "get_reference_node",
    ] {
        assert_eq!(arity("BoneConstraint3D", m), 1, "{m}(index)");
    }
    // The count itself is not per-setting.
    assert_eq!(arity("BoneConstraint3D", "set_setting_count"), 1);
    assert_eq!(arity("BoneConstraint3D", "get_setting_count"), 0);
}

/// ⚠️ Godot names these inconsistently: `clear_setting` (singular) on
/// BoneConstraint3D, `clear_settings` (plural) on BoneTwistDisperser3D. Both
/// take no arguments. "Normalising" either name breaks parity.
#[test]
fn clear_setting_naming_asymmetry_is_godots_own() {
    register_3d_classes();
    assert!(class_has_method("BoneConstraint3D", "clear_setting"));
    assert!(class_has_method("BoneTwistDisperser3D", "clear_settings"));
    assert_eq!(arity("BoneConstraint3D", "clear_setting"), 0);
    assert_eq!(arity("BoneTwistDisperser3D", "clear_settings"), 0);

    // ...and each lacks the other's spelling. `class_has_method` walks the
    // chain, and these are siblings, so neither inherits the other's name.
    assert!(
        !class_has_method("BoneConstraint3D", "clear_settings"),
        "BoneConstraint3D uses the singular"
    );
    assert!(
        !class_has_method("BoneTwistDisperser3D", "clear_setting"),
        "BoneTwistDisperser3D uses the plural"
    );
}

/// ⚠️ `mutable_bone_axes` defaults to **true** (Godot's doc XML). Defaulting it
/// to false would freeze the bone axes as the chain moves and read as "the twist
/// is distributed wrongly" rather than as a wrong constant.
#[test]
fn twist_disperser_static_properties_match_godot() {
    register_3d_classes();
    let mut own = own_props("BoneTwistDisperser3D");
    own.sort();
    assert_eq!(
        own,
        vec!["mutable_bone_axes".to_string(), "setting_count".to_string()],
        "these two are the only static properties Godot declares here"
    );
    assert_eq!(
        default_of("BoneTwistDisperser3D", "mutable_bone_axes"),
        Variant::Bool(true)
    );
    assert_eq!(
        default_of("BoneTwistDisperser3D", "setting_count"),
        Variant::Int(0),
        "a fresh disperser has no settings"
    );
    // The count is a property here but NOT on BoneConstraint3D, even though both
    // expose set/get_setting_count methods — Godot's own asymmetry.
    assert!(!own_props("BoneConstraint3D")
        .iter()
        .any(|p| p == "setting_count"));
}

#[test]
fn disperse_mode_enum_matches_godot() {
    register_3d_classes();
    assert!(class_has_enum("BoneTwistDisperser3D", "DisperseMode"));
    for (name, value) in [
        ("DISPERSE_MODE_EVEN", 0),
        ("DISPERSE_MODE_WEIGHTED", 1),
        ("DISPERSE_MODE_CUSTOM", 2),
    ] {
        assert_eq!(
            class_get_integer_constant("BoneTwistDisperser3D", name),
            Some(value),
            "DisperseMode::{name}"
        );
    }
}

/// Joints are addressed by (setting_index, joint_index) — one argument deeper
/// than the per-setting accessors.
#[test]
fn twist_joint_accessors_take_setting_and_joint_indices() {
    register_3d_classes();
    assert_eq!(arity("BoneTwistDisperser3D", "get_joint_count"), 1);
    for m in [
        "get_joint_bone_name",
        "get_joint_bone",
        "get_joint_twist_amount",
    ] {
        assert_eq!(arity("BoneTwistDisperser3D", m), 2, "{m}(setting, joint)");
    }
    assert_eq!(
        arity("BoneTwistDisperser3D", "set_joint_twist_amount"),
        3,
        "set_joint_twist_amount(setting, joint, amount)"
    );
    // The reference bone is derived from the chain, so it is read-only.
    assert!(class_has_method(
        "BoneTwistDisperser3D",
        "get_reference_bone"
    ));
    assert!(!class_has_method(
        "BoneTwistDisperser3D",
        "set_reference_bone"
    ));
    // Bool getters read as questions in Godot, not as `get_`.
    assert!(class_has_method(
        "BoneTwistDisperser3D",
        "are_bone_axes_mutable"
    ));
    assert!(class_has_method(
        "BoneTwistDisperser3D",
        "is_end_bone_extended"
    ));
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = (
        props("BoneTwistDisperser3D"),
        get_method_list("BoneConstraint3D").len(),
    );
    register_3d_classes();
    assert_eq!(
        before,
        (
            props("BoneTwistDisperser3D"),
            get_method_list("BoneConstraint3D").len()
        )
    );
}
