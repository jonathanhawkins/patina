//! pat-nzgdu.166 — the `ResourceImporter` class family is ClassDB-registered.
//!
//! Covers the bead's registration clause. The remaining clauses (importing
//! .png/.wav/.ogg/.svg/.obj/.ttf with selected options, the Import dock listing
//! per-type options, and reimport regenerating outputs) are follow-up — see the
//! bead notes.
//!
//! **The bead asks for each class to be registered "with its import-option set".
//! Godot has no such thing**, and this file encodes that. All 17 classes expose
//! **zero** properties in Godot's `extension_api.json`; an importer's options
//! are not a ClassDB surface — the editor asks the importer for them at runtime
//! (`_get_import_options()`, not in the extension API) and stores the chosen
//! values in the asset's `.import` file. Registering invented `compress/mode`
//! properties would make `class_get_property_list` report fiction as truth, so
//! the tests below assert the family stays property-free.

use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, get_property_list, is_parent_class,
    register_editor_classes,
};
use gdobject::class_db::is_abstract;

/// The 16 concrete importers Godot ships (the base is handled separately).
const CONCRETE_IMPORTERS: &[&str] = &[
    "ResourceImporterBMFont",
    "ResourceImporterBitMap",
    "ResourceImporterCSVTranslation",
    "ResourceImporterDynamicFont",
    "ResourceImporterImage",
    "ResourceImporterImageFont",
    "ResourceImporterLayeredTexture",
    "ResourceImporterMP3",
    "ResourceImporterOBJ",
    "ResourceImporterOggVorbis",
    "ResourceImporterSVG",
    "ResourceImporterScene",
    "ResourceImporterShaderFile",
    "ResourceImporterTexture",
    "ResourceImporterTextureAtlas",
    "ResourceImporterWAV",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

#[test]
fn resource_importer_family_is_registered() {
    register_editor_classes();

    assert!(class_exists("ResourceImporter"), "the base must be registered");
    for class in CONCRETE_IMPORTERS {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "ResourceImporter"),
            "{class} derives ResourceImporter"
        );
        // The chain must actually reach the root: ResourceImporter is
        // RefCounted, and an unregistered base severs it silently (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} must reach RefCounted through ResourceImporter"
        );
        assert!(is_parent_class(class, "Object"), "{class} must reach Object");
    }
    assert_eq!(
        CONCRETE_IMPORTERS.len(),
        16,
        "Godot ships 16 concrete importers plus the base"
    );
}

/// An importer is a RefCounted helper, not a Resource — easy to assume wrong
/// given the name.
#[test]
fn resource_importer_is_a_refcounted_not_a_resource() {
    register_editor_classes();
    assert!(is_parent_class("ResourceImporter", "RefCounted"));
    assert!(
        !is_parent_class("ResourceImporter", "Resource"),
        "an importer *produces* resources; it is not one"
    );
    assert!(
        is_abstract("ResourceImporter"),
        "the base is abstract — the editor instantiates the concrete subclasses"
    );
    for class in CONCRETE_IMPORTERS {
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
    }
}

/// The bead says "registered with its import-option set". Godot's answer is that
/// there is no such property set — this test pins that, so nobody later "fixes"
/// the family by inventing options that Godot does not have.
#[test]
fn importers_expose_no_properties_because_godot_exposes_none() {
    register_editor_classes();
    assert_eq!(
        props("ResourceImporter"),
        Vec::<String>::new(),
        "ResourceImporter has zero properties in Godot"
    );
    for class in CONCRETE_IMPORTERS {
        assert_eq!(
            props(class),
            Vec::<String>::new(),
            "{class} has zero properties in Godot — import options live in the \
             editor's _get_import_options() and the asset's .import file, not in ClassDB"
        );
    }
}

#[test]
fn import_order_enum_matches_godot() {
    register_editor_classes();
    assert!(class_has_enum("ResourceImporter", "ImportOrder"));
    assert_eq!(
        class_get_integer_constant("ResourceImporter", "IMPORT_ORDER_DEFAULT"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("ResourceImporter", "IMPORT_ORDER_SCENE"),
        Some(100),
        "SCENE is 100, not 1 — scenes import last so their dependencies are ready first"
    );
    // Inherited by every concrete importer.
    assert_eq!(
        class_get_integer_constant("ResourceImporterScene", "IMPORT_ORDER_SCENE"),
        Some(100)
    );
}

/// Every source type the acceptance names has an importer class registered.
#[test]
fn the_asset_types_the_acceptance_names_all_have_importers() {
    register_editor_classes();
    // .png -> Texture, .wav -> WAV, .ogg -> OggVorbis, .svg -> SVG,
    // .obj -> OBJ, .ttf -> DynamicFont.
    for class in [
        "ResourceImporterTexture",
        "ResourceImporterWAV",
        "ResourceImporterOggVorbis",
        "ResourceImporterSVG",
        "ResourceImporterOBJ",
        "ResourceImporterDynamicFont",
    ] {
        assert!(class_exists(class), "{class} must exist for its source type");
    }
}

/// `EditorImportPlugin` is where the bead's "per-type import options" actually
/// live: a script subclasses it and overrides `_get_import_options` to describe
/// them. It derives ResourceImporter, so it is part of this family — and it is
/// the strongest evidence for the point above, because even *it* has zero
/// properties. Options are a virtual surface, never a schema.
#[test]
fn editor_import_plugin_is_the_real_options_surface() {
    register_editor_classes();
    assert!(class_exists("EditorImportPlugin"));
    assert!(
        is_parent_class("EditorImportPlugin", "ResourceImporter"),
        "it is part of the ResourceImporter family"
    );
    assert!(!is_abstract("EditorImportPlugin"), "scripts subclass it");

    // The virtuals that describe options — the answer to "expose per-type
    // import options".
    for m in [
        "_get_import_options",
        "_get_preset_count",
        "_get_preset_name",
        "_get_option_visibility",
        "_get_recognized_extensions",
        "_import",
    ] {
        assert!(
            gdobject::class_has_method("EditorImportPlugin", m),
            "EditorImportPlugin must declare {m}"
        );
    }

    // ...and still no properties. Options are described, not stored.
    assert_eq!(
        props("EditorImportPlugin"),
        Vec::<String>::new(),
        "even the import-options class models no options as properties"
    );
}

#[test]
fn registration_is_idempotent() {
    register_editor_classes();
    let before = props("ResourceImporterTexture");
    register_editor_classes();
    assert_eq!(before, props("ResourceImporterTexture"));
}
