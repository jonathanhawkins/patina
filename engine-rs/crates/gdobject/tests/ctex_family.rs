//! pat-0q2kx.215 — the compressed (`.ctex`) texture family in the ClassDB.
//!
//! Godot ground truth: `upstream/godot/scene/register_scene_types.cpp:1046`,
//! `:1061`, `:1065-1068`, plus each class's doc XML for the hierarchy and the
//! `load_path` member.
//!
//! These are the on-disk counterparts of the `Image*` textures: a `.ctex` holds
//! already-compressed data, `load(path)` reads it and `load_path` reports where
//! it came from. None of the six were registered, so the importer path had no
//! class to instantiate and a compressed cubemap could not become a sky or
//! reflection source.
//!
//! ⚠️ SCOPE: this covers registration and introspection — the half that was
//! missing. The acceptance criterion also asks that a real `.ctex` cubemap load
//! as a usable sky/reflection source, which is decoder work in `gdresource`,
//! not ClassDB. See the completion report.

use gdobject::class_db::{
    class_exists, get_property_list, instantiate, is_abstract, is_parent_class,
    register_viewport_classes,
};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

/// Every class Godot registers in this family, with the parent its doc XML
/// declares.
const FAMILY: [(&str, &str); 6] = [
    ("CompressedTexture2D", "Texture2D"),
    ("CompressedTexture3D", "Texture3D"),
    ("CompressedTextureLayered", "TextureLayered"),
    ("CompressedTexture2DArray", "CompressedTextureLayered"),
    ("CompressedCubemap", "CompressedTextureLayered"),
    ("CompressedCubemapArray", "CompressedTextureLayered"),
];

/// **The bead's headline.** `CompressedCubemap` and its whole family exist and
/// sit under the right parents.
#[test]
fn the_compressed_texture_family_is_registered() {
    register_viewport_classes();

    for (class, parent) in FAMILY {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, parent),
            "{class} must inherit {parent}"
        );
    }

    // The three layered leaves must reach TextureLayered *through*
    // CompressedTextureLayered. Asserting only the direct parent would pass
    // even if the intermediate were parented somewhere wrong, which is the
    // failure that severs a chain (pat-nzgdu.98).
    for leaf in [
        "CompressedTexture2DArray",
        "CompressedCubemap",
        "CompressedCubemapArray",
    ] {
        assert!(
            is_parent_class(leaf, "TextureLayered"),
            "{leaf} must reach TextureLayered through CompressedTextureLayered"
        );
        assert!(
            is_parent_class(leaf, "Texture"),
            "{leaf} must reach the Texture root"
        );
    }
}

/// Abstractness is per-class here and does NOT follow the uncompressed
/// siblings, so it cannot be inferred from the hierarchy.
#[test]
fn only_compressed_texture_layered_is_abstract() {
    register_viewport_classes();

    // GDREGISTER_ABSTRACT_CLASS (register_scene_types.cpp:1065).
    assert!(
        is_abstract("CompressedTextureLayered"),
        "CompressedTextureLayered is GDREGISTER_ABSTRACT_CLASS"
    );

    // Its own parent TextureLayered is GDREGISTER_VIRTUAL_CLASS, i.e. NOT
    // abstract — the pair is the reason this cannot be copied from the parent.
    assert!(
        !is_abstract("TextureLayered"),
        "TextureLayered is VIRTUAL, not abstract — if this flips, the \
         compressed subclass was copied from the wrong sibling"
    );

    // All five leaves are plain GDREGISTER_CLASS.
    for class in [
        "CompressedTexture2D",
        "CompressedTexture3D",
        "CompressedTexture2DArray",
        "CompressedCubemap",
        "CompressedCubemapArray",
    ] {
        assert!(
            !is_abstract(class),
            "{class} is a plain GDREGISTER_CLASS and must be instantiable"
        );
    }
}

/// `load_path` is introspectable and defaults to the empty string, and `load`
/// is callable — the two the doc XML declares.
#[test]
fn load_path_and_load_are_exposed() {
    register_viewport_classes();

    for (class, _) in FAMILY {
        // CompressedTextureLayered is GDREGISTER_ABSTRACT_CLASS upstream; its
        // load_path/load surface is reached through the concrete subclasses.
        if class == "CompressedTextureLayered" {
            assert!(instantiate(class).is_none(), "{class}: Godot registers it with GDREGISTER_ABSTRACT_CLASS");
            continue;
        }
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(
            obj.get_property("load_path"),
            Variant::String(String::new()),
            "{class}.load_path defaults to \"\" (doc XML default=\"&quot;&quot;\")"
        );

        let info = gdobject::class_db::get_class_info(class)
            .unwrap_or_else(|| panic!("{class} has class info"));
        // `parent_class` is a plain `String`, not an `Option` — the root's is
        // empty, and `get_class_info("")` returns None, which ends the walk.
        let has_load = |name: &str| {
            std::iter::successors(Some(info.clone()), |i| {
                gdobject::class_db::get_class_info(&i.parent_class)
            })
            .any(|i| i.methods.iter().any(|m| m.name == name))
        };
        assert!(has_load("load"), "{class} must expose `load`");
        assert!(
            has_load("get_load_path"),
            "{class} must expose `get_load_path`"
        );
    }
}

/// The three leaves inherit `load_path` rather than redeclaring it.
///
/// This is a real hazard, not a style point: `get_property_list` concatenates
/// the inheritance chain WITHOUT deduping, so a redundant redeclaration would
/// make the property appear twice for every leaf — the defect that produced the
/// FAIL on pat-0q2kx.148.
#[test]
fn leaves_do_not_duplicate_the_inherited_load_path() {
    register_viewport_classes();

    for leaf in [
        "CompressedTexture2DArray",
        "CompressedCubemap",
        "CompressedCubemapArray",
    ] {
        let count = get_property_list(leaf)
            .iter()
            .filter(|p| p.name == "load_path")
            .count();
        assert_eq!(
            count, 1,
            "{leaf} must report `load_path` exactly once — it comes from \
             CompressedTextureLayered and must not be re-registered on the leaf"
        );
    }

    // The declaring class still has it exactly once too.
    assert_eq!(
        get_property_list("CompressedTextureLayered")
            .iter()
            .filter(|p| p.name == "load_path")
            .count(),
        1
    );
}

/// Registration is idempotent, since these run inside a shared `if
/// !class_exists` pass that other tests also trigger.
#[test]
fn re_registering_does_not_duplicate_the_family() {
    register_viewport_classes();
    register_viewport_classes();

    assert!(class_exists("CompressedCubemap"));
    assert_eq!(
        get_property_list("CompressedCubemap")
            .iter()
            .filter(|p| p.name == "load_path")
            .count(),
        1,
        "a second registration pass must not double the property list"
    );
}
