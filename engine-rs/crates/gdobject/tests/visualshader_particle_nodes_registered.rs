//! pat-nzgdu.178 — the 11 `VisualShaderNodeParticle*` classes are registered.
//!
//! These are the `shader_type particles` graph vocabulary. Without them a
//! `.tres` VisualShader with a particles output cannot construct its nodes.
//! Schemas are generated from `extension_api.json` and diffed back.

// `is_abstract`/`can_instantiate` are `pub` but are not among lib.rs's
// re-exports (unlike every sibling query), so reach them through the module.
use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_get_integer_constant, get_property_list, instantiate, is_parent_class,
    register_3d_classes,
};
// `GenericObject::get_property` comes from the `GodotObject` trait, not an
// inherent impl — the trait must be in scope to call it.
use gdobject::GodotObject;
use gdvariant::Variant;

/// Every particle node and the base Godot gives it.
const PARTICLE_NODES: &[(&str, &str)] = &[
    ("VisualShaderNodeParticleAccelerator", "VisualShaderNode"),
    (
        "VisualShaderNodeParticleBoxEmitter",
        "VisualShaderNodeParticleEmitter",
    ),
    ("VisualShaderNodeParticleConeVelocity", "VisualShaderNode"),
    ("VisualShaderNodeParticleEmit", "VisualShaderNode"),
    ("VisualShaderNodeParticleEmitter", "VisualShaderNode"),
    (
        "VisualShaderNodeParticleMeshEmitter",
        "VisualShaderNodeParticleEmitter",
    ),
    (
        "VisualShaderNodeParticleMultiplyByAxisAngle",
        "VisualShaderNode",
    ),
    // The outlier: it is the graph's *output*, so it derives the output base.
    ("VisualShaderNodeParticleOutput", "VisualShaderNodeOutput"),
    ("VisualShaderNodeParticleRandomness", "VisualShaderNode"),
    (
        "VisualShaderNodeParticleRingEmitter",
        "VisualShaderNodeParticleEmitter",
    ),
    (
        "VisualShaderNodeParticleSphereEmitter",
        "VisualShaderNodeParticleEmitter",
    ),
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// The bead's inheritance clause.
#[test]
fn visualshader_particle_nodes_registered() {
    register_3d_classes();

    assert_eq!(
        PARTICLE_NODES.len(),
        11,
        "Godot ships 11 particle graph nodes"
    );
    for (class, base) in PARTICLE_NODES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, base),
            "{class} derives {base} in Godot"
        );
        // Every one of them is a VisualShaderNode somewhere up the chain — that
        // is what lets a graph hold it — and must reach Object. An unregistered
        // intermediate severs this silently (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "VisualShaderNode"),
            "{class} must be a VisualShaderNode"
        );
        assert!(
            is_parent_class(class, "Resource"),
            "VisualShaderNode is a Resource"
        );
        assert!(is_parent_class(class, "Object"));
    }
}

/// ⚠️ `VisualShaderNodeParticleOutput` derives `VisualShaderNodeOutput`, not
/// `VisualShaderNode` like its ten siblings — it is the particles graph's output
/// node. A uniform reading of the family gets this one wrong.
#[test]
fn particle_output_derives_the_output_base_not_the_plain_node() {
    register_3d_classes();
    assert!(is_parent_class(
        "VisualShaderNodeParticleOutput",
        "VisualShaderNodeOutput"
    ));
    // ...and none of the others do.
    for (class, _) in PARTICLE_NODES
        .iter()
        .filter(|(c, _)| *c != "VisualShaderNodeParticleOutput")
    {
        assert!(
            !is_parent_class(class, "VisualShaderNodeOutput"),
            "{class} is not an output node"
        );
    }
}

/// The four emitters share an abstract base carrying `mode_2d`.
#[test]
fn the_four_emitters_share_an_abstract_base() {
    register_3d_classes();
    assert!(
        is_abstract("VisualShaderNodeParticleEmitter"),
        "the emitter base is abstract — only the shapes are instantiable"
    );
    for shape in [
        "VisualShaderNodeParticleBoxEmitter",
        "VisualShaderNodeParticleSphereEmitter",
        "VisualShaderNodeParticleRingEmitter",
        "VisualShaderNodeParticleMeshEmitter",
    ] {
        assert!(is_parent_class(shape, "VisualShaderNodeParticleEmitter"));
        assert!(!is_abstract(shape), "{shape} is instantiable");
        // `mode_2d` is inherited from the base, not redeclared.
        assert!(
            props(shape).iter().any(|p| p == "mode_2d"),
            "{shape} inherits mode_2d from the emitter base"
        );
    }
}

/// The bead's instantiate clause — but stated honestly.
///
/// ⚠️ The acceptance says "ClassDB.instantiate() succeeds for each of the 11".
/// It cannot: `VisualShaderNodeParticleEmitter` is **abstract**, and Godot's
/// `ClassDB.instantiate()` returns null for an abstract class. This engine's
/// `instantiate()` currently ignores `is_abstract` and would happily hand one
/// back, so asserting the acceptance verbatim would be asserting a divergence
/// from Godot. Asserted here as: the 10 concrete nodes instantiate, the abstract
/// base reports `can_instantiate == false`.
#[test]
fn every_concrete_particle_node_instantiates() {
    register_3d_classes();

    for (class, _) in PARTICLE_NODES
        .iter()
        .filter(|(c, _)| *c != "VisualShaderNodeParticleEmitter")
    {
        assert!(can_instantiate(class), "{class} is instantiable in Godot");
        let obj = instantiate(class);
        assert!(obj.is_some(), "ClassDB.instantiate({class}) must succeed");
    }

    assert!(
        !can_instantiate("VisualShaderNodeParticleEmitter"),
        "the abstract emitter base must not be instantiable — Godot's \
         ClassDB.instantiate() returns null for it"
    );
}

/// A `.tres` carrying a BoxEmitter must construct a node with its properties —
/// the acceptance's round-trip clause, at the ClassDB level: instantiate applies
/// the registered defaults down the chain.
#[test]
fn box_emitter_instantiates_with_its_inherited_properties() {
    register_3d_classes();
    let obj = instantiate("VisualShaderNodeParticleBoxEmitter").expect("BoxEmitter instantiates");
    // Inherited from the abstract emitter base — the property a .tres would
    // carry, and the thing that would be silently dropped if the base were
    // unregistered or the parent wrong. (`get_property` returns a bare Variant;
    // an absent property reads back as `Nil`, so this also proves it is set.)
    assert_eq!(
        obj.get_property("mode_2d"),
        Variant::Bool(false),
        "an instantiated BoxEmitter carries mode_2d from its base"
    );
}

/// The three selector enums, with the values Godot uses.
#[test]
fn selector_enums_match_godot() {
    register_3d_classes();

    // Accelerator modes.
    for (name, value) in [
        ("MODE_LINEAR", 0),
        ("MODE_RADIAL", 1),
        ("MODE_TANGENTIAL", 2),
        ("MODE_MAX", 3),
    ] {
        assert_eq!(
            class_get_integer_constant("VisualShaderNodeParticleAccelerator", name),
            Some(value)
        );
    }
    // Emit flags select which particle fields the emit writes.
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeParticleEmit", "EMIT_FLAG_POSITION"),
        Some(1)
    );
    // Randomness op type.
    assert!(
        class_get_integer_constant("VisualShaderNodeParticleRandomness", "OP_TYPE_SCALAR")
            .is_some()
    );
}

#[test]
fn mesh_emitter_carries_its_own_surface_selection() {
    register_3d_classes();
    // The mesh emitter is the only one with extra state.
    for want in ["mesh", "use_all_surfaces", "surface_index"] {
        assert!(
            props("VisualShaderNodeParticleMeshEmitter")
                .iter()
                .any(|p| p == want),
            "MeshEmitter must expose {want}"
        );
    }
    // Its siblings do not.
    assert!(
        !props("VisualShaderNodeParticleBoxEmitter")
            .iter()
            .any(|p| p == "mesh"),
        "a box emitter has no mesh"
    );
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("VisualShaderNodeParticleMeshEmitter");
    register_3d_classes();
    assert_eq!(before, props("VisualShaderNodeParticleMeshEmitter"));
}
