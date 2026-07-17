//! pat-nzgdu.183 — the `ChainIK3D` / `IterateIK3D` / `FABRIK3D` class surface.
//!
//! Covers the acceptance's **"ClassDB reports FABRIK3D : IterateIK3D"** clause.
//! The solve itself is pose-verified in gdscene's
//! `fabrik3d_chain_reaches_target`. See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`; defaults
//! against its `--doctool` XML (the API dump carries no defaults).

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_3d_classes,
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

/// Required-argument count. `get_method_list` returns the chain base-first, so
/// search from the end: the derived-most wins.
fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's ClassDB clause — and the full chain behind it. The bead's
/// title says "IterateIK3D base ... as a SkeletonModifier3D", but Godot puts
/// **two** classes in between.
#[test]
fn fabrik3d_reports_iterateik3d_as_its_parent() {
    register_3d_classes();

    assert!(class_exists("FABRIK3D"));
    assert!(
        is_parent_class("FABRIK3D", "IterateIK3D"),
        "the acceptance's clause: ClassDB reports FABRIK3D : IterateIK3D"
    );

    // The real chain, link by link. An unregistered intermediate would sever it
    // silently and `class_exists` alone would not notice (pat-nzgdu.98).
    assert!(is_parent_class("IterateIK3D", "ChainIK3D"));
    assert!(is_parent_class("ChainIK3D", "IKModifier3D"));
    assert!(is_parent_class("IKModifier3D", "SkeletonModifier3D"));
    // ...so FABRIK3D transitively reaches the modifier stack and the root.
    assert!(
        is_parent_class("FABRIK3D", "SkeletonModifier3D"),
        "FABRIK3D must reach SkeletonModifier3D — that is what puts it in the pose stack"
    );
    assert!(is_parent_class("FABRIK3D", "Node3D"));
    assert!(is_parent_class("FABRIK3D", "Object"));
}

/// ⚠️ Only the leaf is instantiable. `ChainIK3D` and `IterateIK3D` are abstract
/// bases that exist to share the chain and the iteration budget — Godot returns
/// null for them.
#[test]
fn only_the_leaf_solver_is_instantiable() {
    register_3d_classes();

    assert!(
        is_abstract("ChainIK3D"),
        "ChainIK3D is an abstract base in Godot (is_instantiable=false)"
    );
    assert!(!can_instantiate("ChainIK3D"));
    assert!(
        is_abstract("IterateIK3D"),
        "IterateIK3D is an abstract base in Godot (is_instantiable=false)"
    );
    assert!(!can_instantiate("IterateIK3D"));

    // The leaf is the one you can actually put in a scene.
    assert!(!is_abstract("FABRIK3D"));
    assert!(can_instantiate("FABRIK3D"));
    let obj = instantiate("FABRIK3D").expect("FABRIK3D instantiates");
    assert_eq!(obj.get_class(), "FABRIK3D");
}

/// ⚠️ `FABRIK3D` declares NOTHING of its own — it is purely the choice of
/// algorithm; every knob is inherited. Giving it `target`/`iterations`
/// properties would invent a schema Godot does not have.
#[test]
fn fabrik3d_declares_nothing_of_its_own() {
    register_3d_classes();
    let inherited = props("IterateIK3D");
    let own: Vec<String> = props("FABRIK3D")
        .into_iter()
        .filter(|p| !inherited.contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "FABRIK3D adds no properties in Godot; found {own:?}"
    );

    // But it inherits the whole configuration surface — that is the point of
    // the hierarchy.
    for p in ["max_iterations", "min_distance", "deterministic"] {
        assert!(props("FABRIK3D").iter().any(|h| h == p), "inherits {p}");
    }
    for m in ["set_target_node", "get_root_bone", "get_joint_count"] {
        assert!(class_has_method("FABRIK3D", m), "inherits {m}");
    }
    // ...and the modifier controls, which is how an IK solve is blended.
    for p in ["active", "influence"] {
        assert!(props("FABRIK3D").iter().any(|h| h == p), "inherits {p}");
    }
}

/// The iteration budget lives on IterateIK3D, shared with FABRIK3D's siblings
/// CCDIK3D and JacobianIK3D.
#[test]
fn iterate_ik3d_defaults_match_godot() {
    register_3d_classes();

    // ⚠️ 4, not some large number: these solvers run every frame and converge
    // ACROSS frames. A big default would make one frame expensive and hide
    // non-convergence.
    assert_eq!(
        default_of("IterateIK3D", "max_iterations"),
        Variant::Int(4),
        "Godot's default is 4"
    );
    assert_eq!(
        default_of("IterateIK3D", "min_distance"),
        Variant::Float(0.001),
        "the arrival tolerance"
    );
    assert_eq!(
        default_of("IterateIK3D", "deterministic"),
        Variant::Bool(false)
    );
    assert_eq!(default_of("IterateIK3D", "setting_count"), Variant::Int(0));

    // ⚠️ 0.034906585 rad is exactly 2 degrees — the per-frame rotation cap that
    // stops a solve from snapping. Recall would never produce this constant.
    match default_of("IterateIK3D", "angular_delta_limit") {
        Variant::Float(f) => {
            let two_degrees = 2.0_f64.to_radians();
            assert!(
                (f - two_degrees).abs() < 1e-6,
                "angular_delta_limit must be 2 degrees in radians ({two_degrees}), got {f}"
            );
        }
        other => panic!("angular_delta_limit must be a Float, got {other:?}"),
    }
}

/// Like BoneConstraint3D, the chain is defined by *indexed* methods rather than
/// properties — one modifier can drive several chains.
#[test]
fn chain_is_defined_by_indexed_methods_not_properties() {
    register_3d_classes();

    // ChainIK3D declares no properties of its own; the chain lives in methods.
    let base = props("IKModifier3D");
    let own: Vec<String> = props("ChainIK3D")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "ChainIK3D's chain is indexed methods, not properties; found {own:?}"
    );

    // Setters take (index, value); getters take (index).
    for m in ["set_root_bone", "set_end_bone", "set_end_bone_length"] {
        assert_eq!(arity("ChainIK3D", m), 2, "{m}(index, value)");
    }
    for m in ["get_root_bone", "get_end_bone", "is_end_bone_extended"] {
        assert_eq!(arity("ChainIK3D", m), 1, "{m}(index)");
    }
    // Joints are derived from the chain, so they are read-only and addressed by
    // (setting_index, joint_index).
    assert_eq!(arity("ChainIK3D", "get_joint_count"), 1);
    assert_eq!(arity("ChainIK3D", "get_joint_bone"), 2);
    assert!(
        !class_has_method("ChainIK3D", "set_joint_bone"),
        "joints are derived from the root->end chain, never set directly"
    );

    // The target is a NODE per setting, not a bare position.
    assert_eq!(arity("IterateIK3D", "set_target_node"), 2);
    assert_eq!(arity("IterateIK3D", "get_target_node"), 1);
    // Per-joint limits are (setting, joint, value).
    assert_eq!(arity("IterateIK3D", "set_joint_limitation"), 3);
    assert_eq!(arity("IterateIK3D", "get_joint_limitation"), 2);
    // The iteration budget is per-modifier, so it is NOT indexed.
    assert_eq!(arity("IterateIK3D", "set_max_iterations"), 1);
    assert_eq!(arity("IterateIK3D", "get_max_iterations"), 0);
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = (props("IterateIK3D"), get_method_list("ChainIK3D").len());
    register_3d_classes();
    assert_eq!(
        before,
        (props("IterateIK3D"), get_method_list("ChainIK3D").len())
    );
}
