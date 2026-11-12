//! pat-0q2kx.234 (partial) — the trail/alignment surface on GPUParticles3D.
//!
//! Godot ground truth: `doc/classes/GPUParticles3D.xml` and
//! `GPUParticles2D.xml`.
//!
//! `GPUParticles3D` was registered BARE — name and parent only — so a `.tscn`
//! 3D emitter round-tripped with none of its properties. Its 2D sibling already
//! carried the trail set.
//!
//! ⚠️ SCOPE: schema only. The bead's substance — a per-particle transform
//! history ring buffer in `ParticleSimulator` and ribbon geometry in the render
//! arm — is NOT delivered, so the acceptance test
//! `particle_trails_render_ribbon_geometry` does not exist. See the report.

use gdobject::class_db::{get_class_info, get_property_list, instantiate, register_3d_classes};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

#[test]
fn gpu_particles_3d_carries_the_trail_surface() {
    register_3d_classes();

    let p = instantiate("GPUParticles3D").expect("GPUParticles3D instantiates");
    assert_eq!(
        p.get_property("trail_enabled"),
        Variant::Bool(false),
        "trails are off by default"
    );
    assert_eq!(
        p.get_property("trail_lifetime"),
        Variant::Float(0.3),
        "trail_lifetime defaults to 0.3 s, not 0 — a zero-length trail would \
         render nothing even with trail_enabled on"
    );
    assert_eq!(p.get_property("transform_align"), Variant::Int(0));
}

/// `TransformAlign` is an ORDINAL enum (0..3), unlike the EmitFlags bitmask on
/// the 2D node — these are mutually exclusive modes, not combinable flags.
#[test]
fn transform_align_is_an_ordinal_enum() {
    register_3d_classes();

    let info = get_class_info("GPUParticles3D").expect("registered");
    let e = info
        .enums
        .iter()
        .find(|e| e.name == "TransformAlign")
        .expect("GPUParticles3D must expose TransformAlign");

    for (name, value) in [
        ("TRANSFORM_ALIGN_DISABLED", 0i64),
        ("TRANSFORM_ALIGN_Z_BILLBOARD", 1),
        ("TRANSFORM_ALIGN_Y_TO_VELOCITY", 2),
        ("TRANSFORM_ALIGN_Z_BILLBOARD_Y_TO_VELOCITY", 3),
    ] {
        let c = e
            .constants
            .iter()
            .find(|c| c.0 == name)
            .unwrap_or_else(|| panic!("TransformAlign must define {name}"));
        assert_eq!(c.1, value, "{name} value");
    }
    assert_eq!(e.constants.len(), 4);

    // The combined mode is 3 — the ordinal successor — NOT 1|2. Treating this
    // family as a bitmask would coincidentally agree here and diverge the
    // moment a fifth mode is added.
    assert_eq!(
        e.constants
            .iter()
            .find(|c| c.0 == "TRANSFORM_ALIGN_Z_BILLBOARD_Y_TO_VELOCITY")
            .map(|c| c.1),
        Some(3)
    );
}

/// The 3D node has NO `trail_sections` pair — Godot resamples against the mesh
/// instead. Declaring them would advertise properties that cannot be honoured.
#[test]
fn the_3d_node_has_no_trail_sections_pair() {
    register_3d_classes();

    let props = get_property_list("GPUParticles3D");
    for absent in ["trail_sections", "trail_section_subdivisions"] {
        assert!(
            !props.iter().any(|p| p.name == absent),
            "GPUParticles3D has no `{absent}` in Godot — that pair is 2D-only"
        );
    }

    // Each trail property it DOES have appears exactly once; the property list
    // concatenates the inheritance chain without deduping.
    for name in ["trail_enabled", "trail_lifetime", "transform_align"] {
        assert_eq!(
            props.iter().filter(|p| p.name == name).count(),
            1,
            "GPUParticles3D must report `{name}` exactly once"
        );
    }
}
