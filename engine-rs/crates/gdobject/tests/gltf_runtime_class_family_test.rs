//! pat-nzgdu.164 — the 19 script-facing `GLTF*` classes are ClassDB-registered.
//!
//! This covers the bead's inheritance clause ("all 19 GLTF* classes are
//! ClassDB-registered with correct inheritance (Resource/RefCounted)"). The
//! runtime API itself — constructing them from GDScript, routing
//! `gdresource`'s `gltf_import.rs` parser through
//! `GLTFDocument.append_from_buffer`, and `generate_scene` — is follow-up; see
//! the bead notes.
//!
//! Schemas are generated from `extension_api.json` and diffed back: 135
//! properties across the family. That is not a surface anyone should hand-type,
//! and it is not one a reviewer can eyeball.

use gdobject::{class_exists, get_property_list, is_parent_class, register_io_classes};
use gdvariant::Variant;

/// Every GLTF* class Godot ships, with the base it must derive.
const GLTF_CLASSES: &[(&str, &str)] = &[
    ("GLTFAccessor", "Resource"),
    ("GLTFAnimation", "Resource"),
    ("GLTFBufferView", "Resource"),
    ("GLTFCamera", "Resource"),
    ("GLTFDocument", "Resource"),
    ("GLTFDocumentExtension", "Resource"),
    ("GLTFDocumentExtensionConvertImporterMesh", "GLTFDocumentExtension"),
    ("GLTFLight", "Resource"),
    ("GLTFMesh", "Resource"),
    ("GLTFNode", "Resource"),
    // The one outlier: RefCounted, not Resource.
    ("GLTFObjectModelProperty", "RefCounted"),
    ("GLTFPhysicsBody", "Resource"),
    ("GLTFPhysicsShape", "Resource"),
    ("GLTFSkeleton", "Resource"),
    ("GLTFSkin", "Resource"),
    ("GLTFSpecGloss", "Resource"),
    ("GLTFState", "Resource"),
    ("GLTFTexture", "Resource"),
    ("GLTFTextureSampler", "Resource"),
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn assert_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

/// The bead's inheritance clause, verbatim.
#[test]
fn all_nineteen_gltf_classes_are_registered_with_correct_inheritance() {
    register_io_classes();

    assert_eq!(GLTF_CLASSES.len(), 19, "Godot ships 19 GLTF* classes");
    for (class, base) in GLTF_CLASSES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, base),
            "{class} derives {base} in Godot"
        );
        // The chain must actually reach the root. An unregistered intermediate
        // severs it silently, and `class_exists` alone would not notice
        // (pat-nzgdu.98) — `register_io_classes` does not bootstrap Resource on
        // its own, which is exactly this trap.
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object through {base} — is {base} registered?"
        );
    }
}

/// ⚠️ 18 of the 19 are Resources; `GLTFObjectModelProperty` is the lone
/// RefCounted. Assuming the family is uniform gets that one wrong.
#[test]
fn object_model_property_is_the_lone_refcounted() {
    register_io_classes();
    assert!(is_parent_class("GLTFObjectModelProperty", "RefCounted"));
    assert!(
        !is_parent_class("GLTFObjectModelProperty", "Resource"),
        "it is RefCounted, not a Resource — the only one in the family"
    );
    // ...and every other one really is a Resource.
    for (class, _) in GLTF_CLASSES.iter().filter(|(c, _)| *c != "GLTFObjectModelProperty") {
        assert!(
            is_parent_class(class, "Resource"),
            "{class} is a Resource"
        );
    }
}

/// The extension hook derives the extension base, not Resource directly — the
/// one two-level chain in the family.
#[test]
fn convert_importer_mesh_extends_the_extension_base() {
    register_io_classes();
    assert!(is_parent_class(
        "GLTFDocumentExtensionConvertImporterMesh",
        "GLTFDocumentExtension"
    ));
    assert!(is_parent_class(
        "GLTFDocumentExtensionConvertImporterMesh",
        "Resource"
    ));
}

/// `GLTFState` is the parse target the acceptance test reads back — it must
/// carry the collections a populated state exposes.
#[test]
fn gltf_state_carries_the_parse_output_collections() {
    register_io_classes();
    assert_props(
        "GLTFState",
        &["json", "nodes", "meshes", "accessors", "buffer_views", "animations", "skins", "skeletons", "cameras", "lights", "textures", "materials"],
    );
    // The collections default to empty rather than Nil, so a fresh state is
    // readable before a parse rather than erroring.
    let list = get_property_list("GLTFState");
    let nodes = list
        .iter()
        .find(|p| p.name == "nodes")
        .expect("GLTFState has nodes");
    assert_eq!(
        nodes.default_value,
        Variant::Array(Vec::new()),
        "an unparsed state has an empty node array, not null"
    );
}

/// `GLTFDocument` is the runtime entry point.
#[test]
fn gltf_document_is_a_registered_resource() {
    register_io_classes();
    assert!(class_exists("GLTFDocument"));
    assert!(is_parent_class("GLTFDocument", "Resource"));
    // Its own settings surface (the methods — append_from_buffer/generate_scene
    // — are deliberately not registered; nothing dispatches them yet).
    assert!(
        !props("GLTFDocument").is_empty(),
        "GLTFDocument has its own properties in Godot"
    );
}

/// A spot-check that the generated schemas carry real per-class fields rather
/// than a uniform stub: each of these is specific to its class.
#[test]
fn data_holders_carry_their_own_distinct_schemas() {
    register_io_classes();
    assert_props("GLTFNode", &["parent", "mesh", "skin", "position", "rotation", "scale"]);
    assert_props("GLTFAccessor", &["buffer_view", "component_type", "count", "min", "max"]);
    assert_props("GLTFBufferView", &["buffer", "byte_offset", "byte_length"]);
    assert_props("GLTFTextureSampler", &["mag_filter", "min_filter", "wrap_s", "wrap_t"]);
    assert_props("GLTFSkeleton", &["joints", "roots"]);
}

#[test]
fn registration_is_idempotent() {
    register_io_classes();
    let before = props("GLTFState");
    register_io_classes();
    assert_eq!(before, props("GLTFState"));
}
