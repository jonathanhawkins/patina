//! pat-0q2kx.259 — the canvas-SDF / screen-space `VisualShaderNode` family.
//!
//! Six classes, none previously registered, all inheriting `VisualShaderNode`
//! directly: the `canvas_item` SDF nodes (`texture_sdf`, `texture_sdf_normal`,
//! the two screen-UV↔SDF coordinate conversions, and the raymarch helper) plus
//! the spatial `ScreenNormalWorldSpace`.
//!
//! **Scope is registration only**, as the bead states: the SDF builtins these
//! would emit are downstream of the missing VisualShader codegen and the
//! missing canvas SDF pass. So this asserts the ClassDB surface and nothing
//! about generated shader code.
//!
//! Ground truth is `upstream/godot/doc/classes/VisualShaderNode*.xml`. Every one
//! of the six declares **no methods and no properties** — they are pure marker
//! nodes whose behaviour lives in the code generator. Registering invented ports
//! or properties would be worse than registering none, since `class_exists`
//! answers true either way.
//!
//! Binary named `vshader_sdf` to keep the verify command inside the 58-char
//! truncation limit; the acceptance function keeps its full name.

use gdobject::class_db::{
    class_exists, inheritance_chain, instantiate, is_abstract, is_parent_class, register_3d_classes,
};

/// The six classes. All inherit `VisualShaderNode` **directly** per their XML.
const FAMILY: [&str; 6] = [
    "VisualShaderNodeTextureSDF",
    "VisualShaderNodeTextureSDFNormal",
    "VisualShaderNodeScreenUVToSDF",
    "VisualShaderNodeSDFToScreenUV",
    "VisualShaderNodeSDFRaymarch",
    "VisualShaderNodeScreenNormalWorldSpace",
];

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** All six are registered under `VisualShaderNode` and
/// each one instantiates.
#[test]
fn visual_shader_sdf_screenspace_family_registered() {
    register_3d_classes();

    for class in FAMILY {
        assert!(class_exists(class), "{class} must be registered");

        // The DIRECT parent, not merely an ancestor: `is_parent_class` is true
        // for anything in the chain, so it would pass even if the class were
        // attached at the wrong depth.
        let chain = inheritance_chain(class);
        assert_eq!(
            chain.get(1).map(String::as_str),
            Some("VisualShaderNode"),
            "{class} must inherit VisualShaderNode directly; chain was {chain:?}",
        );
        assert!(
            is_parent_class(class, "VisualShaderNode"),
            "{class} must reach VisualShaderNode",
        );

        // The acceptance requires instantiation, so none of these may be
        // abstract — Godot lets a graph create every one of them.
        assert!(
            !is_abstract(class),
            "{class} must not be abstract; a VisualShader graph instantiates it",
        );
        assert!(
            instantiate(class).is_some(),
            "{class} must instantiate through ClassDB",
        );
    }
}

// ===========================================================================
// Guards against over-registration
// ===========================================================================

/// Godot declares no properties on any of the six. Registering invented ones
/// would be silently wrong: `class_exists` still answers true, so the
/// acceptance above would pass while a `.tres` round-trip started emitting
/// fields Godot never wrote.
#[test]
fn none_of_the_family_declares_its_own_properties() {
    register_3d_classes();

    let inherited: Vec<String> = gdobject::class_db::get_property_list("VisualShaderNode")
        .into_iter()
        .map(|p| p.name)
        .collect();

    for class in FAMILY {
        let own: Vec<String> = gdobject::class_db::get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .filter(|n| !inherited.contains(n))
            .collect();
        assert!(
            own.is_empty(),
            "{class} declares {own:?}, but its doc XML lists no members",
        );
    }
}

/// The two coordinate-conversion nodes are distinct classes in both directions.
///
/// `ScreenUVToSDF` and `SDFToScreenUV` are near-mirror names, and a copy-paste
/// registration that named one twice would leave the other missing while this
/// file's other assertions still passed for the duplicated one.
#[test]
fn the_two_conversion_nodes_are_both_present_and_distinct() {
    register_3d_classes();

    assert!(class_exists("VisualShaderNodeScreenUVToSDF"));
    assert!(class_exists("VisualShaderNodeSDFToScreenUV"));

    assert!(instantiate("VisualShaderNodeScreenUVToSDF").is_some());
    assert!(instantiate("VisualShaderNodeSDFToScreenUV").is_some());

    // Both chains must start at their own name. If one registration had been
    // pasted twice under a single name, the other class would be absent and
    // `class_exists` above would already have failed — this pins that the two
    // chains are genuinely separate entries rather than aliases.
    assert_eq!(
        inheritance_chain("VisualShaderNodeScreenUVToSDF")
            .first()
            .map(String::as_str),
        Some("VisualShaderNodeScreenUVToSDF"),
    );
    assert_eq!(
        inheritance_chain("VisualShaderNodeSDFToScreenUV")
            .first()
            .map(String::as_str),
        Some("VisualShaderNodeSDFToScreenUV"),
    );
}
