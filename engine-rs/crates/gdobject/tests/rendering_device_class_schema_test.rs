//! pat-nzgdu.162 — `RenderingDevice` and the RD* parameter classes are
//! ClassDB-registered with their property schemas.
//!
//! This covers the bead's *schema* acceptance clause. The other two clauses
//! (`RenderingServer.get_rendering_device()` returning a live device, and a
//! GDScript compute dispatch with buffer readback) are not implemented and are
//! reported as blocked: there is no GLSL→SPIRV compiler anywhere in the
//! workspace, `gdrender3d::wgpu_pipeline` has no compute path, and all of wgpu
//! sits behind the non-default `gpu` feature. See the bead notes.
//!
//! The assertions here are deliberately about *reachability from script*: a
//! class is only useful to a GDScript author if `class_exists` finds it, its
//! inheritance chain reaches `Object`, and `class_get_property_list` reports the
//! fields Godot documents. An RD* class that exists but whose chain is severed
//! (an unregistered base — the pat-nzgdu.98 trap) would pass a naive
//! `class_exists` check and still be broken for `is_class`/inspector use.

use gdobject::{class_exists, get_property_list, is_parent_class, register_viewport_classes};

/// Every RD* class and the base Godot gives it.
const RD_CLASSES: &[(&str, &str)] = &[
    ("RenderingDevice", "Object"),
    ("RDShaderSource", "RefCounted"),
    ("RDShaderSPIRV", "Resource"),
    ("RDShaderFile", "Resource"),
    ("RDUniform", "RefCounted"),
    ("RDTextureFormat", "RefCounted"),
    ("RDTextureView", "RefCounted"),
    ("RDSamplerState", "RefCounted"),
    ("RDVertexAttribute", "RefCounted"),
    ("RDPipelineSpecializationConstant", "RefCounted"),
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// Asserts `class` reports every property in `want`.
fn assert_has_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

#[test]
fn rendering_device_family_is_registered_with_godot_bases() {
    register_viewport_classes();

    for (class, base) in RD_CLASSES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, base),
            "{class} derives {base} in Godot"
        );
        // The chain must actually reach the root. An unregistered intermediate
        // base severs it silently, which `class_exists` alone would not catch.
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object through {base} — is {base} registered?"
        );
    }
}

#[test]
fn refcounted_and_resource_bases_are_registered() {
    register_viewport_classes();

    // The RD* classes parent to these; if either is missing every chain above
    // breaks. RefCounted in particular was not registered on this path before.
    assert!(class_exists("RefCounted"));
    assert!(is_parent_class("RefCounted", "Object"));
    assert!(class_exists("Resource"));
    assert!(is_parent_class("Resource", "Object"));
}

#[test]
fn rd_shader_source_carries_every_stage_and_language() {
    register_viewport_classes();
    assert_has_props(
        "RDShaderSource",
        &[
            "source_vertex",
            "source_fragment",
            "source_tesselation_control",
            "source_tesselation_evaluation",
            "source_compute",
            "language",
        ],
    );
}

#[test]
fn rd_shader_spirv_carries_bytecode_and_errors_per_stage() {
    register_viewport_classes();
    // Godot pairs each stage's bytecode with that stage's compile error.
    assert_has_props(
        "RDShaderSPIRV",
        &[
            "bytecode_vertex",
            "bytecode_fragment",
            "bytecode_tesselation_control",
            "bytecode_tesselation_evaluation",
            "bytecode_compute",
            "compile_error_vertex",
            "compile_error_fragment",
            "compile_error_tesselation_control",
            "compile_error_tesselation_evaluation",
            "compile_error_compute",
        ],
    );
}

#[test]
fn rd_texture_format_describes_an_allocation() {
    register_viewport_classes();
    assert_has_props(
        "RDTextureFormat",
        &[
            "format",
            "width",
            "height",
            "depth",
            "array_layers",
            "mipmaps",
            "texture_type",
            "samples",
            "usage_bits",
            "is_resolve_buffer",
            "is_discardable",
        ],
    );
}

#[test]
fn rd_sampler_state_carries_filter_wrap_and_compare_state() {
    register_viewport_classes();
    assert_has_props(
        "RDSamplerState",
        &[
            "mag_filter",
            "min_filter",
            "mip_filter",
            "repeat_u",
            "repeat_v",
            "repeat_w",
            "lod_bias",
            "use_anisotropy",
            "anisotropy_max",
            "enable_compare",
            "compare_op",
            "min_lod",
            "max_lod",
            "border_color",
            "unnormalized_uvw",
        ],
    );
}

#[test]
fn small_rd_param_classes_carry_their_schemas() {
    register_viewport_classes();
    assert_has_props("RDUniform", &["uniform_type", "binding"]);
    assert_has_props(
        "RDTextureView",
        &[
            "format_override",
            "swizzle_r",
            "swizzle_g",
            "swizzle_b",
            "swizzle_a",
        ],
    );
    assert_has_props(
        "RDVertexAttribute",
        // `binding` is the vertex-buffer slot; `location` is the shader
        // attribute location. Godot has both and they are not the same thing.
        &[
            "binding",
            "location",
            "offset",
            "format",
            "stride",
            "frequency",
        ],
    );
    assert_has_props(
        "RDPipelineSpecializationConstant",
        &["value", "constant_id"],
    );
    assert_has_props("RDShaderFile", &["base_error"]);
}

#[test]
fn registration_is_idempotent() {
    // The whole module is `if !class_exists { register }`; a second call from
    // another subsystem's bootstrap must not duplicate or panic.
    register_viewport_classes();
    let before = props("RDTextureFormat");
    register_viewport_classes();
    let after = props("RDTextureFormat");
    assert_eq!(
        before, after,
        "re-registering must not duplicate properties"
    );
}

/// `RDTextureFormat`'s defaults describe a real 1x1 2D texture, not zeroes —
/// a script that allocates without touching the format should get something
/// valid rather than a zero-sized surface.
#[test]
fn rd_texture_format_defaults_describe_a_usable_texture() {
    use gdvariant::Variant;
    register_viewport_classes();
    let list = get_property_list("RDTextureFormat");
    let get = |name: &str| -> Variant {
        list.iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("RDTextureFormat has no `{name}`"))
            .default_value
            .clone()
    };
    assert_eq!(
        get("width"),
        Variant::Int(1),
        "extent defaults are 1, not 0"
    );
    assert_eq!(get("height"), Variant::Int(1));
    assert_eq!(get("depth"), Variant::Int(1));
    assert_eq!(get("array_layers"), Variant::Int(1));
    assert_eq!(get("mipmaps"), Variant::Int(1));
    // TextureType 2D == 1 in Godot's enum; 0 would be TEXTURE_TYPE_1D.
    assert_eq!(
        get("texture_type"),
        Variant::Int(1),
        "a default RDTextureFormat is a 2D texture"
    );
    assert_eq!(get("is_resolve_buffer"), Variant::Bool(false));
}
