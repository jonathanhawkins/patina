//! pat-s3rrv.260 — the `VisualShaderNodeVarying` family in ClassDB.
//!
//! Godot ground truth: `upstream/godot/doc/classes/VisualShaderNodeVarying.xml`
//! and its Getter/Setter siblings. The base carries both members; the two leaves
//! declare none of their own and inherit them.
//!
//! ## The default that matters
//!
//! ⚠️ `varying_name` defaults to the literal string **`[None]`** — brackets
//! included. The doc XML spells it `default="&quot;[None]&quot;"`, so the quotes
//! are XML escaping and the brackets are the value. Godot uses it as the "no
//! varying selected" sentinel, which means an **empty** default is not a
//! harmless approximation: it reads as a real varying whose name happens to be
//! blank, and a graph would look like it had one selected.

use gdobject::class_db::{
    class_exists, get_property_list, is_parent_class, register_3d_classes,
};
use gdvariant::Variant;

/// Every property `get_property_list` reports for `class_name`, by name.
fn prop_names(class_name: &str) -> Vec<String> {
    get_property_list(class_name)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

/// **The acceptance.**
#[test]
fn visualshadernode_varying_family_registered() {
    register_3d_classes();

    // -- the three classes and their chain -----------------------------------
    for name in [
        "VisualShaderNodeVarying",
        "VisualShaderNodeVaryingGetter",
        "VisualShaderNodeVaryingSetter",
    ] {
        assert!(class_exists(name), "{name} must be registered");
    }
    assert!(
        is_parent_class("VisualShaderNodeVarying", "VisualShaderNode"),
        "the base inherits VisualShaderNode"
    );
    for leaf in [
        "VisualShaderNodeVaryingGetter",
        "VisualShaderNodeVaryingSetter",
    ] {
        assert!(
            is_parent_class(leaf, "VisualShaderNodeVarying"),
            "{leaf} must inherit VisualShaderNodeVarying — parenting it straight \
             to VisualShaderNode satisfies every other check here while losing \
             both inherited members"
        );
        assert!(
            is_parent_class(leaf, "VisualShaderNode"),
            "{leaf} must still reach VisualShaderNode through the chain"
        );
    }

    // -- the defaults on the base --------------------------------------------
    assert_eq!(
        default_of("VisualShaderNodeVarying", "varying_name"),
        Some(Variant::String(String::from("[None]"))),
        "varying_name defaults to the literal \"[None]\", brackets included — \
         an empty string is Godot's 'a varying with a blank name', not its \
         'no varying selected'"
    );
    assert_eq!(
        default_of("VisualShaderNodeVarying", "varying_type"),
        Some(Variant::Int(0)),
        "varying_type defaults to 0"
    );

    // -- the leaves inherit both, exactly once -------------------------------
    for leaf in [
        "VisualShaderNodeVaryingGetter",
        "VisualShaderNodeVaryingSetter",
    ] {
        let names = prop_names(leaf);
        for member in ["varying_name", "varying_type"] {
            let count = names.iter().filter(|n| *n == member).count();
            assert_eq!(
                count, 1,
                "{leaf} must report {member} exactly once, got {count}; full \
                 list {names:?}"
            );
        }
        // Inherited means the VALUE comes through too, not just the name.
        assert_eq!(
            default_of(leaf, "varying_name"),
            Some(Variant::String(String::from("[None]"))),
            "{leaf} inherits the base's default, not a fresh empty one"
        );
    }
}

/// The leaves declare nothing of their own.
///
/// Their property lists must be identical to the base's — if a leaf grew its own
/// copy of a member, the `exactly once` assertion above would still pass
/// (`get_property_list` dedupes by name since pat-s3rrv.74), so that check alone
/// cannot see it. Comparing the whole list can.
#[test]
fn visualshadernode_varying_family_registered_leaves_add_nothing() {
    register_3d_classes();

    let base = prop_names("VisualShaderNodeVarying");
    assert!(
        base.iter().any(|n| n == "varying_name") && base.iter().any(|n| n == "varying_type"),
        "precondition: the base declares both members; got {base:?}"
    );

    for leaf in [
        "VisualShaderNodeVaryingGetter",
        "VisualShaderNodeVaryingSetter",
    ] {
        assert_eq!(
            prop_names(leaf),
            base,
            "{leaf} declares no members of its own, so its property list must \
             match the base's exactly"
        );
    }
}
