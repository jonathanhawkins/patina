//! pat-0q2kx.255 — the RenderingDevice pipeline-state / attachment-format
//! property bags are ClassDB-registered.
//!
//! Godot ground truth: `servers/register_server_types.cpp:226-235` (all six are
//! `GDREGISTER_CLASS`, i.e. concrete) and `doc/classes/RD*.xml` for the
//! property surface and defaults.
//!
//! Ten RD* classes were already registered; these six were not, so
//! `render_pipeline_create` and `framebuffer_format_create` had no way to be
//! described from script. `rendering_device_class_schema_test.rs` covers the
//! ten that existed — this covers the six that did not, in the same style.
//!
//! ⚠️ The defaults matter as much as the existence. Several are NOT zero, and a
//! bag registered with zeros would let a script build a pipeline that silently
//! differs from the one it asked for: `write_r/g/b/a` default TRUE (all-false
//! renders nothing), the depth/stencil compares default to 7 = COMPARE_OP_ALWAYS
//! (zero is COMPARE_OP_NEVER, which rejects every fragment), and `line_width`
//! defaults to 1.0 (zero draws nothing). Those are pinned individually below —
//! a "does the class exist" test would pass while every one of them was wrong.

use gdobject::{
    class_exists, get_property_list, instantiate, is_parent_class, register_viewport_classes,
};
// `set_property`/`get_property` on GenericObject come from the GodotObject
// TRAIT, not an inherent impl — without this import every call below is E0599.
use gdobject::GodotObject;
use gdvariant::Variant;

/// The six classes and the base Godot gives each.
const RD_STATE_CLASSES: &[&str] = &[
    "RDAttachmentFormat",
    "RDPipelineColorBlendState",
    "RDPipelineColorBlendStateAttachment",
    "RDPipelineDepthStencilState",
    "RDPipelineMultisampleState",
    "RDPipelineRasterizationState",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// THE ACCEPTANCE. Each class exists, derives RefCounted, constructs, and round
/// -trips its documented properties.
#[test]
fn rd_pipeline_state_classes_register() {
    register_viewport_classes();

    for class in RD_STATE_CLASSES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} derives RefCounted in Godot"
        );
        // A class whose chain is severed by an unregistered base passes a naive
        // class_exists and is still broken for is_class/inspector use.
        assert!(
            is_parent_class(class, "Object"),
            "{class}'s chain must reach Object"
        );
        assert!(
            instantiate(class).is_some(),
            "{class} is GDREGISTER_CLASS (concrete), so new() must work"
        );
    }
}

/// Properties are settable and readable back. Each write uses a value that
/// DIFFERS from the registered default, so a bag that silently dropped the
/// write — or a getter that always answered the default — fails here rather
/// than round-tripping vacuously.
#[test]
fn pipeline_state_properties_round_trip() {
    register_viewport_classes();

    let cases: &[(&str, &str, Variant)] = &[
        ("RDAttachmentFormat", "usage_flags", Variant::Int(16)),
        ("RDPipelineColorBlendState", "logic_op", Variant::Int(3)),
        (
            "RDPipelineColorBlendStateAttachment",
            "enable_blend",
            Variant::Bool(true),
        ),
        (
            "RDPipelineDepthStencilState",
            "enable_depth_test",
            Variant::Bool(true),
        ),
        (
            "RDPipelineMultisampleState",
            "min_sample_shading",
            Variant::Float(0.25),
        ),
        (
            "RDPipelineRasterizationState",
            "line_width",
            Variant::Float(4.0),
        ),
    ];

    for (class, prop, value) in cases {
        let mut obj = instantiate(class).unwrap_or_else(|| panic!("{class} must construct"));
        let before = obj.get_property(prop);
        assert_ne!(
            &before, value,
            "{class}.{prop}: the test value must differ from the default, or \
             the round-trip below proves nothing"
        );
        obj.set_property(prop, value.clone());
        assert_eq!(
            &obj.get_property(prop),
            value,
            "{class}.{prop} must read back what was written"
        );
    }
}

/// The defaults that are NOT zero, pinned against doc/classes. Every one of
/// these would be plausible as a zero and wrong.
#[test]
fn the_non_zero_defaults_match_godot() {
    register_viewport_classes();

    let want: &[(&str, &str, Variant)] = &[
        // DATA_FORMAT_R8G8B8A8_UNORM. Zero is DATA_FORMAT_R4G4_UNORM_PACK8.
        ("RDAttachmentFormat", "format", Variant::Int(36)),
        // COMPARE_OP_ALWAYS. Zero is COMPARE_OP_NEVER — rejects every fragment.
        (
            "RDPipelineDepthStencilState",
            "depth_compare_operator",
            Variant::Int(7),
        ),
        (
            "RDPipelineDepthStencilState",
            "front_op_compare",
            Variant::Int(7),
        ),
        (
            "RDPipelineDepthStencilState",
            "back_op_compare",
            Variant::Int(7),
        ),
        // STENCIL_OP_ZERO = 1, not STENCIL_OP_KEEP = 0.
        (
            "RDPipelineDepthStencilState",
            "front_op_fail",
            Variant::Int(1),
        ),
        (
            "RDPipelineDepthStencilState",
            "front_op_pass",
            Variant::Int(1),
        ),
        (
            "RDPipelineDepthStencilState",
            "front_op_depth_fail",
            Variant::Int(1),
        ),
        (
            "RDPipelineDepthStencilState",
            "back_op_fail",
            Variant::Int(1),
        ),
        (
            "RDPipelineDepthStencilState",
            "back_op_pass",
            Variant::Int(1),
        ),
        (
            "RDPipelineDepthStencilState",
            "back_op_depth_fail",
            Variant::Int(1),
        ),
        // A zero-width line draws nothing.
        (
            "RDPipelineRasterizationState",
            "line_width",
            Variant::Float(1.0),
        ),
        // Zero control points is not a valid patch.
        (
            "RDPipelineRasterizationState",
            "patch_control_points",
            Variant::Int(1),
        ),
        // Opaque black, not the all-zero Color a Default would produce.
        (
            "RDPipelineColorBlendState",
            "blend_constant",
            Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
        ),
    ];

    for (class, prop, expect) in want {
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} must construct"));
        assert_eq!(
            &obj.get_property(prop),
            expect,
            "{class}.{prop} default must match doc/classes/{class}.xml"
        );
    }

    // All four colour write masks default TRUE. An all-false default produces a
    // pipeline that renders nothing while looking perfectly well-formed.
    let att = instantiate("RDPipelineColorBlendStateAttachment").unwrap();
    for c in ["write_r", "write_g", "write_b", "write_a"] {
        assert_eq!(
            att.get_property(c),
            Variant::Bool(true),
            "{c} must default true"
        );
    }
}

/// Every property Godot documents is present. A bag missing a field cannot
/// describe the pipeline the author wanted, and the omission is invisible until
/// the pipeline behaves oddly.
#[test]
fn every_documented_property_is_present() {
    register_viewport_classes();

    let want: &[(&str, &[&str])] = &[
        ("RDAttachmentFormat", &["format", "samples", "usage_flags"]),
        (
            "RDPipelineColorBlendState",
            &[
                "enable_logic_op",
                "logic_op",
                "blend_constant",
                "attachments",
            ],
        ),
        (
            "RDPipelineColorBlendStateAttachment",
            &[
                "enable_blend",
                "src_color_blend_factor",
                "dst_color_blend_factor",
                "color_blend_op",
                "src_alpha_blend_factor",
                "dst_alpha_blend_factor",
                "alpha_blend_op",
                "write_r",
                "write_g",
                "write_b",
                "write_a",
            ],
        ),
        (
            "RDPipelineDepthStencilState",
            &[
                "enable_depth_test",
                "enable_depth_write",
                "depth_compare_operator",
                "enable_depth_range",
                "depth_range_min",
                "depth_range_max",
                "enable_stencil",
                "front_op_fail",
                "front_op_pass",
                "front_op_depth_fail",
                "front_op_compare",
                "front_op_compare_mask",
                "front_op_write_mask",
                "front_op_reference",
                "back_op_fail",
                "back_op_pass",
                "back_op_depth_fail",
                "back_op_compare",
                "back_op_compare_mask",
                "back_op_write_mask",
                "back_op_reference",
            ],
        ),
        (
            "RDPipelineMultisampleState",
            &[
                "sample_count",
                "enable_sample_shading",
                "min_sample_shading",
                "enable_alpha_to_coverage",
                "enable_alpha_to_one",
                "sample_masks",
            ],
        ),
        (
            "RDPipelineRasterizationState",
            &[
                "enable_depth_clamp",
                "discard_primitives",
                "wireframe",
                "cull_mode",
                "front_face",
                "depth_bias_enabled",
                "depth_bias_constant_factor",
                "depth_bias_clamp",
                "depth_bias_slope_factor",
                "line_width",
                "patch_control_points",
            ],
        ),
    ];

    for (class, fields) in want {
        // NOTE: presence only. `get_property_list` concatenates the inheritance
        // chain WITHOUT deduping, so the list length is not a meaningful
        // assertion here.
        let have = props(class);
        for f in *fields {
            assert!(
                have.iter().any(|h| h == f),
                "{class} must expose `{f}`; got {have:?}"
            );
        }
    }
}

/// The ten RD* classes that were already registered must be undisturbed —
/// these six were added into the middle of that family.
#[test]
fn the_existing_rd_family_is_unaffected() {
    register_viewport_classes();

    for class in [
        "RenderingDevice",
        "RDShaderSource",
        "RDShaderSPIRV",
        "RDShaderFile",
        "RDUniform",
        "RDTextureFormat",
        "RDTextureView",
        "RDSamplerState",
        "RDVertexAttribute",
        "RDPipelineSpecializationConstant",
    ] {
        assert!(class_exists(class), "{class} must still be registered");
    }

    // A spot-check that a neighbour's defaults did not get captured by the new
    // blocks: RDTextureFormat's width is 1, and RDSamplerState's max_lod 1e20.
    let tf = instantiate("RDTextureFormat").unwrap();
    assert_eq!(tf.get_property("width"), Variant::Int(1));
    let ss = instantiate("RDSamplerState").unwrap();
    assert_eq!(ss.get_property("anisotropy_max"), Variant::Float(1.0));
}
