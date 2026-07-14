//! pat-6cdt: Resource UID, sub-resource, and external reference fidelity.
//!
//! Validates that:
//! 1. ext_resource headers are parsed with correct type, uid, path, and id
//! 2. ext_resource UIDs can be registered and resolved through UidRegistry
//! 3. Sub-resources are parsed and accessible by section ID
//! 4. The gd_resource header UID is extracted correctly
//! 5. ext_resources without UIDs (Godot < 4.x compat) are handled gracefully

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::loader::{parse_uid_string, TresLoader};
use gdresource::{Resource, ResourceLoader, UidRegistry, UnifiedLoader};

// ===========================================================================
// Fixture content — matches fixtures/resources/with_ext_refs.tres
// ===========================================================================

const FIXTURE_WITH_EXT_REFS: &str = r#"[gd_resource type="PackedScene" format=3 uid="uid://scene_with_refs"]

[ext_resource type="Texture2D" uid="uid://icon_texture" path="res://icon.png" id="1"]
[ext_resource type="Script" uid="uid://player_script" path="res://scripts/player.gd" id="2"]
[ext_resource type="PackedScene" path="res://scenes/enemy.tscn" id="3"]

[sub_resource type="StyleBoxFlat" id="inline_style"]
bg_color = Color(1.0, 0.0, 0.0, 1.0)
border_width = 3

[resource]
name = "TestScene"
"#;

// ===========================================================================
// 1. ext_resource header parsing fidelity
// ===========================================================================

#[test]
fn ext_resource_fields_parsed_correctly() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();

    // Should have 3 ext_resources
    assert_eq!(res.ext_resources.len(), 3, "expected 3 ext_resources");

    // ext_resource id="1" — Texture2D with UID
    let ext1 = res
        .ext_resources
        .get("1")
        .expect("ext_resource id=1 missing");
    assert_eq!(ext1.resource_type, "Texture2D");
    assert_eq!(ext1.uid, "uid://icon_texture");
    assert_eq!(ext1.path, "res://icon.png");
    assert_eq!(ext1.id, "1");

    // ext_resource id="2" — Script with UID
    let ext2 = res
        .ext_resources
        .get("2")
        .expect("ext_resource id=2 missing");
    assert_eq!(ext2.resource_type, "Script");
    assert_eq!(ext2.uid, "uid://player_script");
    assert_eq!(ext2.path, "res://scripts/player.gd");
    assert_eq!(ext2.id, "2");

    // ext_resource id="3" — PackedScene WITHOUT UID
    let ext3 = res
        .ext_resources
        .get("3")
        .expect("ext_resource id=3 missing");
    assert_eq!(ext3.resource_type, "PackedScene");
    assert_eq!(
        ext3.uid, "",
        "ext_resource without uid= should have empty uid"
    );
    assert_eq!(ext3.path, "res://scenes/enemy.tscn");
    assert_eq!(ext3.id, "3");
}

// ===========================================================================
// 2. ext_resource UIDs register and resolve through UidRegistry
// ===========================================================================

#[test]
fn ext_resource_uids_register_in_uid_registry() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();

    let mut registry = UidRegistry::new();

    // Register the main resource UID
    if res.uid.is_valid() {
        registry.register(res.uid, &res.path);
    }

    // Register ext_resource UIDs
    for ext in res.ext_resources.values() {
        if !ext.uid.is_empty() {
            let uid = parse_uid_string(&ext.uid);
            if uid.is_valid() {
                registry.register(uid, &ext.path);
            }
        }
    }

    // Main resource UID resolves
    assert_eq!(registry.lookup_uid(res.uid), Some("res://test.tscn"));

    // ext_resource UIDs resolve to their paths
    let icon_uid = parse_uid_string("uid://icon_texture");
    assert_eq!(registry.lookup_uid(icon_uid), Some("res://icon.png"));

    let script_uid = parse_uid_string("uid://player_script");
    assert_eq!(
        registry.lookup_uid(script_uid),
        Some("res://scripts/player.gd")
    );

    // ext_resource without UID should NOT be in registry
    // (id="3" has no uid= attribute)
    assert_eq!(registry.len(), 3); // main + 2 ext_resources with UIDs
}

#[test]
fn ext_resource_uid_round_trips_through_unified_loader() {
    struct FakeLoader;
    impl ResourceLoader for FakeLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            let mut r = Resource::new("Fake");
            r.path = path.to_string();
            Ok(Arc::new(r))
        }
    }

    let tres_loader = TresLoader::new();
    let res = tres_loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();

    let mut unified = UnifiedLoader::new(FakeLoader);

    // Register ext_resource UIDs in the unified loader
    for ext in res.ext_resources.values() {
        if !ext.uid.is_empty() {
            unified.register_uid_str(&ext.uid, &ext.path);
        }
    }

    // Load by uid:// should resolve to the ext_resource path
    let by_uid = unified.load("uid://icon_texture").unwrap();
    assert_eq!(by_uid.path, "res://icon.png");

    // Load by res:// path should return the same Arc
    let by_path = unified.load("res://icon.png").unwrap();
    assert!(
        Arc::ptr_eq(&by_uid, &by_path),
        "uid:// and res:// for same ext_resource must resolve to same Arc"
    );

    // uid:// for the script ext_resource
    let script = unified.load("uid://player_script").unwrap();
    assert_eq!(script.path, "res://scripts/player.gd");
}

// ===========================================================================
// 3. Sub-resource parsing fidelity
// ===========================================================================

#[test]
fn sub_resource_parsed_with_properties() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();

    assert_eq!(res.subresources.len(), 1, "expected 1 sub_resource");

    let style = res
        .subresources
        .get("inline_style")
        .expect("sub_resource 'inline_style' missing");
    assert_eq!(style.class_name, "StyleBoxFlat");
    assert_eq!(
        style.get_property("border_width"),
        Some(&gdvariant::Variant::Int(3))
    );
}

#[test]
fn sub_resource_in_theme_fixture() {
    let loader = TresLoader::new();
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/resources/theme.tres"
    ));
    // Only run if fixture is accessible
    if let Ok(source) = source {
        let res = loader.parse_str(&source, "res://theme.tres").unwrap();
        assert!(res.uid.is_valid(), "theme.tres should have a UID");
        assert_eq!(res.subresources.len(), 2, "theme.tres has 2 sub_resources");
        assert!(res.subresources.contains_key("panel_style"));
        assert!(res.subresources.contains_key("button_style"));
    }
}

// ===========================================================================
// 4. gd_resource header UID extraction
// ===========================================================================

#[test]
fn gd_resource_uid_extracted() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();

    assert!(res.uid.is_valid(), "gd_resource uid should be extracted");
    assert_eq!(res.uid, parse_uid_string("uid://scene_with_refs"));
}

#[test]
fn gd_resource_class_name_extracted() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();
    assert_eq!(res.class_name, "PackedScene");
}

// ===========================================================================
// 5. ext_resource without UID — graceful handling
// ===========================================================================

#[test]
fn ext_resource_without_uid_has_empty_string() {
    let tres = r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" path="res://old_style.png" id="1"]

[resource]
name = "OldFormat"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://old.tres").unwrap();

    let ext = res
        .ext_resources
        .get("1")
        .expect("ext_resource id=1 missing");
    assert_eq!(
        ext.uid, "",
        "ext_resource without uid= should have empty uid"
    );
    assert_eq!(ext.path, "res://old_style.png");
    assert_eq!(ext.resource_type, "Texture2D");
}

#[test]
fn ext_resource_empty_uid_not_registered() {
    let tres = r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" path="res://no_uid.png" id="1"]

[resource]
name = "NoUid"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://no_uid.tres").unwrap();

    let mut registry = UidRegistry::new();
    for ext in res.ext_resources.values() {
        if !ext.uid.is_empty() {
            let uid = parse_uid_string(&ext.uid);
            if uid.is_valid() {
                registry.register(uid, &ext.path);
            }
        }
    }

    assert!(
        registry.is_empty(),
        "ext_resource with no uid= should not produce registry entries"
    );
}

// ===========================================================================
// 6. Multiple ext_resources with UIDs — no cross-contamination
// ===========================================================================

#[test]
fn multiple_ext_resource_uids_distinct() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(FIXTURE_WITH_EXT_REFS, "res://test.tscn")
        .unwrap();

    let ext1 = &res.ext_resources["1"];
    let ext2 = &res.ext_resources["2"];

    let uid1 = parse_uid_string(&ext1.uid);
    let uid2 = parse_uid_string(&ext2.uid);

    assert_ne!(
        uid1, uid2,
        "different uid:// strings must produce different UIDs"
    );
    assert!(uid1.is_valid());
    assert!(uid2.is_valid());
}
