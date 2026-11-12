//! pat-0q2kx.227 — the VisualShader 3D/array texture sampler + parameter family.
//!
//! Godot ground truth: `register_scene_types.cpp` (the GDREGISTER macros) and
//! each class's doc XML for the hierarchy, the `source` default and the
//! `Source` enum.
//!
//! Seven classes, none of them previously registered, while their shared parent
//! `VisualShaderNodeTextureParameter` already was.

use gdobject::class_db::{
    class_exists, get_class_info, get_property_list, instantiate, is_abstract, is_parent_class,
    register_3d_classes,
};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

/// Each new class and the parent its doc XML declares.
const FAMILY: [(&str, &str); 7] = [
    ("VisualShaderNodeSample3D", "VisualShaderNode"),
    ("VisualShaderNodeTexture2DArray", "VisualShaderNodeSample3D"),
    ("VisualShaderNodeTexture3D", "VisualShaderNodeSample3D"),
    (
        "VisualShaderNodeTexture2DParameter",
        "VisualShaderNodeTextureParameter",
    ),
    (
        "VisualShaderNodeTexture2DArrayParameter",
        "VisualShaderNodeTextureParameter",
    ),
    (
        "VisualShaderNodeTexture3DParameter",
        "VisualShaderNodeTextureParameter",
    ),
    (
        "VisualShaderNodeTextureParameterTriplanar",
        "VisualShaderNodeTextureParameter",
    ),
];

#[test]
fn the_family_is_registered_under_the_right_parents() {
    register_3d_classes();

    for (class, parent) in FAMILY {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, parent),
            "{class} must inherit {parent}"
        );
        // Reaching VisualShaderNode proves the whole chain, not just one link —
        // a wrongly-parented intermediate still satisfies the direct check.
        assert!(
            is_parent_class(class, "VisualShaderNode"),
            "{class} must reach VisualShaderNode"
        );
    }
}

/// Only `Sample3D` is abstract. Its two subclasses are plain
/// `GDREGISTER_CLASS`, so the abstractness does NOT propagate downward.
#[test]
fn only_sample3d_is_abstract() {
    register_3d_classes();

    assert!(
        is_abstract("VisualShaderNodeSample3D"),
        "GDREGISTER_ABSTRACT_CLASS(VisualShaderNodeSample3D)"
    );
    for class in [
        "VisualShaderNodeTexture2DArray",
        "VisualShaderNodeTexture3D",
        "VisualShaderNodeTexture2DParameter",
        "VisualShaderNodeTexture2DArrayParameter",
        "VisualShaderNodeTexture3DParameter",
        "VisualShaderNodeTextureParameterTriplanar",
    ] {
        assert!(
            !is_abstract(class),
            "{class} is a plain GDREGISTER_CLASS and must stay instantiable \
             even though its base is abstract"
        );
    }

    // The already-registered TextureParameter base stays abstract; adding
    // concrete leaves under it must not have changed that.
    assert!(is_abstract("VisualShaderNodeTextureParameter"));
}

/// `source` defaults to 0 (SOURCE_TEXTURE) and the `Source` enum is exposed.
#[test]
fn sample3d_source_default_and_enum() {
    register_3d_classes();

    // Reached through a concrete subclass, which is how a scene would see it.
    let node = instantiate("VisualShaderNodeTexture3D").expect("instantiates");
    assert_eq!(
        node.get_property("source"),
        Variant::Int(0),
        "source defaults to SOURCE_TEXTURE (VisualShaderNodeSample3D.xml)"
    );

    let info = get_class_info("VisualShaderNodeSample3D").expect("registered");
    let src = info
        .enums
        .iter()
        .find(|e| e.name == "Source")
        .expect("Sample3D must expose its Source enum");
    for (name, value) in [
        ("SOURCE_TEXTURE", 0i64),
        ("SOURCE_PORT", 1),
        ("SOURCE_MAX", 2),
    ] {
        let c = src
            .constants
            .iter()
            .find(|c| c.0 == name)
            .unwrap_or_else(|| panic!("Source must define {name}"));
        assert_eq!(c.1, value, "{name} value");
    }
}

/// The two samplers carry their own texture property; the four parameter leaves
/// carry none, inheriting the whole schema.
#[test]
fn texture_properties_are_declared_once_and_in_the_right_place() {
    register_3d_classes();

    let arr = instantiate("VisualShaderNodeTexture2DArray").expect("instantiates");
    assert_eq!(arr.get_property("texture_array"), Variant::Nil);
    let vol = instantiate("VisualShaderNodeTexture3D").expect("instantiates");
    assert_eq!(vol.get_property("texture"), Variant::Nil);

    // The parameter leaves must inherit `texture_type` exactly once. Godot gives
    // them no members of their own; re-declaring would double the entry, since
    // get_property_list concatenates the chain without deduping.
    for leaf in [
        "VisualShaderNodeTexture2DParameter",
        "VisualShaderNodeTexture2DArrayParameter",
        "VisualShaderNodeTexture3DParameter",
        "VisualShaderNodeTextureParameterTriplanar",
    ] {
        let props = get_property_list(leaf);
        assert_eq!(
            props.iter().filter(|p| p.name == "texture_type").count(),
            1,
            "{leaf} must report `texture_type` exactly once — it comes from \
             VisualShaderNodeTextureParameter"
        );
        assert!(
            props.iter().any(|p| p.name == "texture_source"),
            "{leaf} must inherit the full parameter schema, not just some of it"
        );
        // And it must genuinely be inherited, not declared locally.
        let own = get_class_info(leaf).expect("registered");
        assert!(
            own.properties.is_empty(),
            "{leaf} declares no members of its own in Godot, got {:?}",
            own.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}
