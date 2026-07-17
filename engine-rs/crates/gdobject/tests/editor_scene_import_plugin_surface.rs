//! pat-nzgdu.192 — the scene-import plugin ClassDB surface.
//!
//! Covers the acceptance's `ClassDB.class_exists` clause. The script binding
//! over `gdeditor/src/import.rs`, a real `.glb` import through
//! `EditorSceneFormatImporterGLTF`, and the unsupported-on-web errors for
//! Blend/FBX2GLTF are follow-up — see the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, class_has_method, get_method_list,
    get_property_list, instantiate, is_parent_class, register_editor_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

/// The concrete format importers. All four derive the same base and add nothing.
const IMPORTERS: &[&str] = &[
    "EditorSceneFormatImporterGLTF",
    "EditorSceneFormatImporterBlend",
    "EditorSceneFormatImporterFBX2GLTF",
    "EditorSceneFormatImporterUFBX",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's clause: the base, the GLTF importer, and the post-import
/// classes all exist.
#[test]
fn editor_scene_import_plugin_surface() {
    register_editor_classes();

    for class in [
        "EditorSceneFormatImporter",
        "EditorScenePostImport",
        "EditorScenePostImportPlugin",
    ] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} is a RefCounted plugin object"
        );
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object"
        );
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }

    for importer in IMPORTERS {
        assert!(class_exists(importer), "ClassDB must know {importer}");
        assert!(
            is_parent_class(importer, "EditorSceneFormatImporter"),
            "{importer} must derive the format-importer base"
        );
        // Must reach the root: an unregistered intermediate severs the chain
        // silently (pat-nzgdu.98).
        assert!(is_parent_class(importer, "RefCounted"));
        assert!(is_parent_class(importer, "Object"));
        assert!(!is_abstract(importer));
        assert!(can_instantiate(importer));
    }
}

/// ⚠️ These are `RefCounted`, NOT `ResourceImporter`.
///
/// `EditorImportPlugin` derives `ResourceImporter` and imports *any* resource;
/// this family plugs into the **scene** importer specifically and shares no base
/// with it beyond RefCounted. Deriving them from ResourceImporter is the
/// plausible-looking mistake this pins.
#[test]
fn scene_importers_are_not_resource_importers() {
    register_editor_classes();

    for class in [
        "EditorSceneFormatImporter",
        "EditorSceneFormatImporterGLTF",
        "EditorScenePostImport",
        "EditorScenePostImportPlugin",
    ] {
        assert!(
            !is_parent_class(class, "ResourceImporter"),
            "{class} is NOT a ResourceImporter — it plugs into the scene importer"
        );
        assert!(!is_parent_class(class, "EditorImportPlugin"));
    }

    // The contrast is real, not vacuous: EditorImportPlugin genuinely IS one,
    // and ResourceImporter is abstract.
    assert!(is_parent_class("EditorImportPlugin", "ResourceImporter"));
    assert!(is_abstract("ResourceImporter"));
}

/// The two virtuals that make an importer an importer: which files it claims,
/// and how it turns one into a scene.
#[test]
fn format_importer_declares_the_extension_and_import_hooks() {
    register_editor_classes();

    assert_eq!(
        arity("EditorSceneFormatImporter", "_get_extensions"),
        0,
        "_get_extensions() takes nothing and returns the claimed extensions"
    );
    // (path, flags, options) — the shape Godot hands an importer.
    assert_eq!(arity("EditorSceneFormatImporter", "_import_scene"), 3);
    assert_eq!(arity("EditorSceneFormatImporter", "_get_import_options"), 1);
    assert_eq!(
        arity("EditorSceneFormatImporter", "_get_option_visibility"),
        3
    );

    // Declared inside _get_import_options, hence on the importer itself.
    assert_eq!(arity("EditorSceneFormatImporter", "add_import_option"), 2);
    assert_eq!(
        arity("EditorSceneFormatImporter", "add_import_option_advanced"),
        3
    );
}

/// ⚠️ Every concrete importer declares NOTHING of its own — the format is the
/// only difference, and it lives in `_get_extensions`. Giving GLTF a property
/// would invent schema.
#[test]
fn concrete_importers_add_nothing_of_their_own() {
    register_editor_classes();

    let base_methods: Vec<String> = get_method_list("EditorSceneFormatImporter")
        .into_iter()
        .map(|m| m.name)
        .collect();
    let base_props = props("EditorSceneFormatImporter");

    for importer in IMPORTERS {
        let own_methods: Vec<String> = get_method_list(importer)
            .into_iter()
            .map(|m| m.name)
            .filter(|m| !base_methods.contains(m))
            .collect();
        assert!(
            own_methods.is_empty(),
            "{importer} adds no methods in Godot; found {own_methods:?}"
        );

        let own_props: Vec<String> = props(importer)
            .into_iter()
            .filter(|p| !base_props.contains(p))
            .collect();
        assert!(
            own_props.is_empty(),
            "{importer} adds no properties; found {own_props:?}"
        );

        // But each inherits the hooks — that is the point of the base.
        assert!(class_has_method(importer, "_get_extensions"));
        assert!(class_has_method(importer, "_import_scene"));
    }
}

/// `EditorScenePostImport` is the small hook — mutate the finished scene.
/// `EditorScenePostImportPlugin` is the richer one, and they are SIBLINGS, not
/// a chain: neither derives the other.
#[test]
fn the_two_post_import_classes_are_siblings_not_a_chain() {
    register_editor_classes();

    assert_eq!(
        arity("EditorScenePostImport", "_post_import"),
        1,
        "_post_import(scene) is the hook the acceptance names"
    );
    assert_eq!(
        arity("EditorScenePostImport", "get_source_file"),
        0,
        "...and where the scene came from — its whole other method"
    );

    assert!(
        !is_parent_class("EditorScenePostImportPlugin", "EditorScenePostImport"),
        "the plugin is a SIBLING of the simple hook, not a subclass"
    );
    assert!(!is_parent_class(
        "EditorScenePostImport",
        "EditorScenePostImportPlugin"
    ));

    // The plugin's extra reach: it touches each internal resource as it is
    // built, not just the finished scene.
    assert_eq!(arity("EditorScenePostImportPlugin", "_internal_process"), 4);
    assert_eq!(arity("EditorScenePostImportPlugin", "_pre_process"), 1);
    assert_eq!(arity("EditorScenePostImportPlugin", "_post_process"), 1);
    assert_eq!(arity("EditorScenePostImportPlugin", "get_option_value"), 1);
}

/// The category `_internal_process` is handed. ⚠️ `_MAX` is a count sentinel,
/// not a category — an implementation that treats it as one would process a
/// category that does not exist.
#[test]
fn internal_import_category_enum_matches_godot() {
    register_editor_classes();

    assert!(class_has_enum(
        "EditorScenePostImportPlugin",
        "InternalImportCategory"
    ));
    for (name, value) in [
        ("INTERNAL_IMPORT_CATEGORY_NODE", 0),
        ("INTERNAL_IMPORT_CATEGORY_MESH_3D_NODE", 1),
        ("INTERNAL_IMPORT_CATEGORY_MESH", 2),
        ("INTERNAL_IMPORT_CATEGORY_MATERIAL", 3),
        ("INTERNAL_IMPORT_CATEGORY_ANIMATION", 4),
        ("INTERNAL_IMPORT_CATEGORY_ANIMATION_NODE", 5),
        ("INTERNAL_IMPORT_CATEGORY_SKELETON_3D_NODE", 6),
        ("INTERNAL_IMPORT_CATEGORY_MAX", 7),
    ] {
        assert_eq!(
            class_get_integer_constant("EditorScenePostImportPlugin", name),
            Some(value),
            "InternalImportCategory::{name}"
        );
    }
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        [
            "EditorSceneFormatImporter",
            "EditorSceneFormatImporterGLTF",
            "EditorScenePostImport",
            "EditorScenePostImportPlugin",
        ]
        .iter()
        .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
        .collect()
    };

    register_editor_classes();
    let first = ids();
    register_editor_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
