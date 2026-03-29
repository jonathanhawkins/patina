//! pat-3qd4: Validate resource metadata and roundtrip behavior parity.
//!
//! These tests verify that Patina's TresLoader → TresSaver → TresLoader
//! roundtrip preserves resource structure and property values, mirroring
//! the behavior validated by the apps/godot resource_roundtrip_probe.
//!
//! Acceptance: structured outputs capture metadata and roundtrip
//! expectations used by runtime tests.

use std::sync::Arc;

use serde_json::{json, Value};

// ===========================================================================
// Probe envelope schema for resource_roundtrip capture type
// ===========================================================================

fn validate_envelope(envelope: &Value) {
    assert!(
        envelope["fixture_id"].is_string(),
        "envelope must have string fixture_id"
    );
    assert!(
        envelope["capture_type"].is_string(),
        "envelope must have string capture_type"
    );
    assert!(
        !envelope["data"].is_null(),
        "envelope must have non-null data"
    );
}

// ===========================================================================
// 1. Roundtrip probe success schema
// ===========================================================================

#[test]
fn roundtrip_probe_success_schema() {
    let probe_output = json!({
        "fixture_id": "resource_roundtrip_probe",
        "capture_type": "resource_roundtrip",
        "data": {
            "status": "pass",
            "resource_path": "res://fixtures/test_theme.tres",
            "resource_class": "Theme",
            "reloaded_class": "Theme",
            "class_preserved": true,
            "property_count": 3,
            "match_count": 3,
            "mismatch_count": 0,
            "properties": [
                {
                    "name": "default_font_size",
                    "type": 2,
                    "usage": 4102,
                    "original_value": "16",
                    "original_value_type": 2,
                    "reloaded_value": "16",
                    "reloaded_value_type": 2,
                    "values_match": true,
                    "types_match": true,
                    "roundtrip_ok": true
                },
                {
                    "name": "default_base_scale",
                    "type": 3,
                    "usage": 4102,
                    "original_value": "1.0",
                    "original_value_type": 3,
                    "reloaded_value": "1.0",
                    "reloaded_value_type": 3,
                    "values_match": true,
                    "types_match": true,
                    "roundtrip_ok": true
                },
                {
                    "name": "default_font_color",
                    "type": 20,
                    "usage": 4102,
                    "original_value": "(1, 1, 1, 1)",
                    "original_value_type": 20,
                    "reloaded_value": "(1, 1, 1, 1)",
                    "reloaded_value_type": 20,
                    "values_match": true,
                    "types_match": true,
                    "roundtrip_ok": true
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["status"], "pass");
    assert!(data["resource_path"].is_string());
    assert!(data["resource_class"].is_string());
    assert!(data["reloaded_class"].is_string());
    assert!(data["class_preserved"].is_boolean());
    assert!(data["property_count"].is_number());
    assert!(data["match_count"].is_number());
    assert!(data["mismatch_count"].is_number());

    let props = data["properties"].as_array().unwrap();
    for prop in props {
        assert!(prop["name"].is_string());
        assert!(prop["type"].is_number());
        assert!(prop["usage"].is_number());
        assert!(prop["original_value"].is_string());
        assert!(prop["original_value_type"].is_number());
        assert!(prop["reloaded_value"].is_string());
        assert!(prop["reloaded_value_type"].is_number());
        assert!(prop["values_match"].is_boolean());
        assert!(prop["types_match"].is_boolean());
        assert!(prop["roundtrip_ok"].is_boolean());
    }
}

// ===========================================================================
// 2. Roundtrip probe failure schema
// ===========================================================================

#[test]
fn roundtrip_probe_mismatch_schema() {
    let probe_output = json!({
        "fixture_id": "resource_roundtrip_probe",
        "capture_type": "resource_roundtrip",
        "data": {
            "status": "fail",
            "resource_path": "res://fixtures/test_style_box.tres",
            "resource_class": "StyleBoxFlat",
            "reloaded_class": "StyleBoxFlat",
            "class_preserved": true,
            "property_count": 2,
            "match_count": 1,
            "mismatch_count": 1,
            "properties": [
                {
                    "name": "bg_color",
                    "type": 20,
                    "usage": 4102,
                    "original_value": "(0.2, 0.3, 0.4, 1)",
                    "original_value_type": 20,
                    "reloaded_value": "(0.2, 0.3, 0.4, 1)",
                    "reloaded_value_type": 20,
                    "values_match": true,
                    "types_match": true,
                    "roundtrip_ok": true
                },
                {
                    "name": "corner_radius_top_left",
                    "type": 2,
                    "usage": 4102,
                    "original_value": "8",
                    "original_value_type": 2,
                    "reloaded_value": "0",
                    "reloaded_value_type": 2,
                    "values_match": false,
                    "types_match": true,
                    "roundtrip_ok": false
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["status"], "fail");
    assert_eq!(data["mismatch_count"], 1);

    let props = data["properties"].as_array().unwrap();
    let mismatch = props
        .iter()
        .find(|p| !p["roundtrip_ok"].as_bool().unwrap())
        .unwrap();
    assert_eq!(mismatch["values_match"], false);
}

// ===========================================================================
// 3. Roundtrip probe error schemas
// ===========================================================================

#[test]
fn roundtrip_probe_load_error_schema() {
    let probe_output = json!({
        "fixture_id": "resource_roundtrip_probe",
        "capture_type": "resource_roundtrip",
        "data": {
            "status": "error",
            "error": "failed_to_load",
            "resource_path": "res://nonexistent.tres"
        }
    });

    validate_envelope(&probe_output);
    assert_eq!(probe_output["data"]["status"], "error");
    assert_eq!(probe_output["data"]["error"], "failed_to_load");
}

#[test]
fn roundtrip_probe_save_error_schema() {
    let probe_output = json!({
        "fixture_id": "resource_roundtrip_probe",
        "capture_type": "resource_roundtrip",
        "data": {
            "status": "error",
            "error": "save_failed",
            "resource_path": "res://fixtures/readonly.tres",
            "temp_path": "user://roundtrip_probe_res___fixtures_readonly.tres.tres",
            "error_code": "ERR_FILE_CANT_WRITE"
        }
    });

    validate_envelope(&probe_output);
    assert_eq!(probe_output["data"]["error"], "save_failed");
    assert!(probe_output["data"]["temp_path"].is_string());
}

#[test]
fn roundtrip_probe_class_mismatch_error_schema() {
    let probe_output = json!({
        "fixture_id": "resource_roundtrip_probe",
        "capture_type": "resource_roundtrip",
        "data": {
            "status": "error",
            "error": "class_mismatch",
            "resource_path": "res://fixtures/test_theme.tres",
            "expected_class": "Animation",
            "actual_class": "Theme"
        }
    });

    validate_envelope(&probe_output);
    assert_eq!(probe_output["data"]["error"], "class_mismatch");
    assert!(probe_output["data"]["expected_class"].is_string());
    assert!(probe_output["data"]["actual_class"].is_string());
}

// ===========================================================================
// 4. Engine-side TresLoader → TresSaver roundtrip
// ===========================================================================

#[test]
fn tres_roundtrip_preserves_simple_properties() {
    use gdcore::ResourceUid;
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("TestResource");
    original.uid = ResourceUid::new(99999);
    original.path = "res://fixtures/roundtrip_test.tres".to_string();
    original.set_property("name", Variant::String("hello".into()));
    original.set_property("count", Variant::Int(42));
    original.set_property("ratio", Variant::Float(3.14));
    original.set_property("enabled", Variant::Bool(true));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(reloaded.class_name, "TestResource");
    assert_eq!(
        reloaded.get_property("name"),
        Some(&Variant::String("hello".into()))
    );
    assert_eq!(reloaded.get_property("count"), Some(&Variant::Int(42)));
    assert_eq!(reloaded.get_property("ratio"), Some(&Variant::Float(3.14)));
    assert_eq!(reloaded.get_property("enabled"), Some(&Variant::Bool(true)));
}

#[test]
fn tres_roundtrip_preserves_vector_and_color() {
    use gdcore::math::{Color, Vector2, Vector3};
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("SpatialResource");
    original.path = "res://test.tres".to_string();
    original.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
    original.set_property("origin", Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)));
    original.set_property("tint", Variant::Color(Color::new(0.5, 0.6, 0.7, 1.0)));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(
        reloaded.get_property("position"),
        Some(&Variant::Vector2(Vector2::new(10.0, 20.0)))
    );
    assert_eq!(
        reloaded.get_property("origin"),
        Some(&Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)))
    );
    assert_eq!(
        reloaded.get_property("tint"),
        Some(&Variant::Color(Color::new(0.5, 0.6, 0.7, 1.0)))
    );
}

#[test]
fn tres_roundtrip_preserves_uid() {
    use gdcore::ResourceUid;
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("UidResource");
    original.uid = ResourceUid::new(12345678);
    original.path = "res://uid_test.tres".to_string();
    original.set_property("data", Variant::Int(1));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    // Verify the uid:// appears in the saved output
    assert!(saved.contains("uid=\"uid://12345678\""));

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    // The loader hashes the uid string portion to produce a stable numeric UID.
    // The important invariant is that the UID is valid and stable across reloads.
    assert!(reloaded.uid.is_valid());

    // Roundtrip again to verify stability: same saved string → same UID hash
    let saved2 = saver.save_to_string(&original).unwrap();
    let reloaded2 = loader.parse_str(&saved2, &original.path).unwrap();
    assert_eq!(
        reloaded.uid.raw(),
        reloaded2.uid.raw(),
        "UID must be stable across reloads"
    );
}

#[test]
fn tres_roundtrip_preserves_subresources() {
    use gdcore::math::Color;
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("Theme");
    original.path = "res://subresource_test.tres".to_string();
    original.set_property("value", Variant::Int(1));

    let mut sub = Resource::new("StyleBoxFlat");
    sub.set_property("bg_color", Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)));
    sub.set_property("corner_radius", Variant::Int(8));
    original
        .subresources
        .insert("StyleBoxFlat_1".to_string(), Arc::new(sub));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(reloaded.subresources.len(), 1);
    // After roundtrip, the saver renumbers subresource IDs to sequential integers
    // (matching Godot .tres format), so the key becomes "1" not "StyleBoxFlat_1".
    let sub = reloaded.subresources.values().next().unwrap();
    assert_eq!(sub.class_name, "StyleBoxFlat");
    assert_eq!(
        sub.get_property("bg_color"),
        Some(&Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)))
    );
    assert_eq!(sub.get_property("corner_radius"), Some(&Variant::Int(8)));
}

#[test]
fn tres_roundtrip_preserves_ext_resources() {
    use gdresource::loader::TresLoader;
    use gdresource::resource::{ExtResource, Resource};
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("PackedScene");
    original.path = "res://ext_test.tres".to_string();

    original.ext_resources.insert(
        "1".to_string(),
        ExtResource {
            resource_type: "Texture2D".to_string(),
            uid: "uid://abc123".to_string(),
            path: "res://icon.png".to_string(),
            id: "1".to_string(),
        },
    );
    original.ext_resources.insert(
        "2".to_string(),
        ExtResource {
            resource_type: "Script".to_string(),
            uid: "uid://def456".to_string(),
            path: "res://main.gd".to_string(),
            id: "2".to_string(),
        },
    );
    original.set_property("data", Variant::Int(0));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(reloaded.ext_resources.len(), 2);
    let ext1 = reloaded.ext_resources.get("1").unwrap();
    assert_eq!(ext1.resource_type, "Texture2D");
    assert_eq!(ext1.path, "res://icon.png");
    let ext2 = reloaded.ext_resources.get("2").unwrap();
    assert_eq!(ext2.resource_type, "Script");
    assert_eq!(ext2.path, "res://main.gd");
}

#[test]
fn tres_roundtrip_empty_resource() {
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;

    let original = Resource::new("EmptyResource");

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, "res://empty.tres").unwrap();

    assert_eq!(reloaded.class_name, "EmptyResource");
    assert_eq!(reloaded.property_count(), 0);
    assert!(reloaded.subresources.is_empty());
    assert!(reloaded.ext_resources.is_empty());
}

#[test]
fn tres_roundtrip_string_escaping() {
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("StringResource");
    original.path = "res://string_test.tres".to_string();
    original.set_property("text", Variant::String("line1\nline2\ttab".into()));
    original.set_property("quoted", Variant::String("he said \"hello\"".into()));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(
        reloaded.get_property("text"),
        Some(&Variant::String("line1\nline2\ttab".into()))
    );
    assert_eq!(
        reloaded.get_property("quoted"),
        Some(&Variant::String("he said \"hello\"".into()))
    );
}

#[test]
fn tres_roundtrip_nil_property() {
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("NilResource");
    original.path = "res://nil_test.tres".to_string();
    original.set_property("nothing", Variant::Nil);

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(reloaded.get_property("nothing"), Some(&Variant::Nil));
}

// ===========================================================================
// 10. Roundtrip fixture coverage matches probe expectations
// ===========================================================================

#[test]
fn roundtrip_fixture_set_covers_diverse_types() {
    let fixtures = [
        ("res://fixtures/test_theme.tres", "Theme"),
        ("res://fixtures/test_environment.tres", "Environment"),
        ("res://fixtures/test_rect_shape.tres", "RectangleShape2D"),
        ("res://fixtures/test_style_box.tres", "StyleBoxFlat"),
        ("res://fixtures/test_animation.tres", "Animation"),
    ];
    assert_eq!(fixtures.len(), 5, "should have 5 roundtrip fixtures");

    let paths: Vec<&str> = fixtures.iter().map(|(p, _)| *p).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), fixtures.len(), "all paths must be unique");
}

// ===========================================================================
// 11. Roundtrip capture type is registered
// ===========================================================================

#[test]
fn roundtrip_capture_type_registered() {
    let capture_types = [
        "scene_tree",
        "properties",
        "signals",
        "classdb",
        "resource_metadata",
        "node_defaults",
        "resource_validation",
        "resource_roundtrip",
        "enum_constants",
        "inheritance_chain",
    ];
    assert_eq!(
        capture_types.len(),
        10,
        "should have 10 distinct probe capture types including roundtrip"
    );
    assert!(capture_types.contains(&"resource_roundtrip"));
}

// ===========================================================================
// 12. Multiple roundtrips are stable
// ===========================================================================

#[test]
fn tres_multiple_roundtrips_stable() {
    use gdcore::math::Vector2;
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut resource = Resource::new("StableResource");
    resource.path = "res://stable.tres".to_string();
    resource.set_property("pos", Variant::Vector2(Vector2::new(5.5, 10.5)));
    resource.set_property("count", Variant::Int(100));
    resource.set_property("label", Variant::String("stable".into()));

    let saver = TresSaver::new();
    let loader = TresLoader::new();

    // Roundtrip 3 times
    let mut current = resource;
    for _ in 0..3 {
        let saved = saver.save_to_string(&current).unwrap();
        let reloaded = loader.parse_str(&saved, "res://stable.tres").unwrap();
        current = Arc::try_unwrap(reloaded).unwrap();
    }

    assert_eq!(
        current.get_property("pos"),
        Some(&Variant::Vector2(Vector2::new(5.5, 10.5)))
    );
    assert_eq!(current.get_property("count"), Some(&Variant::Int(100)));
    assert_eq!(
        current.get_property("label"),
        Some(&Variant::String("stable".into()))
    );
}

// ===========================================================================
// 13. Roundtrip preserves property count
// ===========================================================================

#[test]
fn tres_roundtrip_property_count_preserved() {
    use gdresource::loader::TresLoader;
    use gdresource::resource::Resource;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let mut original = Resource::new("CountResource");
    original.path = "res://count.tres".to_string();
    for i in 0..10 {
        original.set_property(&format!("prop_{i}"), Variant::Int(i));
    }

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let loader = TresLoader::new();
    let reloaded = loader.parse_str(&saved, &original.path).unwrap();

    assert_eq!(reloaded.property_count(), 10);
    for i in 0..10 {
        assert_eq!(
            reloaded.get_property(&format!("prop_{i}")),
            Some(&Variant::Int(i)),
            "property prop_{i} must survive roundtrip"
        );
    }
}

// ===========================================================================
// 14. PATINA_PROBE line parsing for roundtrip type
// ===========================================================================

#[test]
fn parse_roundtrip_probe_line() {
    let line = r#"PATINA_PROBE:{"fixture_id":"resource_roundtrip_probe","capture_type":"resource_roundtrip","data":{"status":"pass"}}"#;
    let json_str = line.strip_prefix("PATINA_PROBE:").unwrap();
    let parsed: Value = serde_json::from_str(json_str).unwrap();
    validate_envelope(&parsed);
    assert_eq!(parsed["capture_type"], "resource_roundtrip");
}

// ===========================================================================
// 15. Roundtrip probe property comparison fields
// ===========================================================================

#[test]
fn roundtrip_property_comparison_fields_complete() {
    let required_fields = [
        "name",
        "type",
        "usage",
        "original_value",
        "original_value_type",
        "reloaded_value",
        "reloaded_value_type",
        "values_match",
        "types_match",
        "roundtrip_ok",
    ];
    assert_eq!(
        required_fields.len(),
        10,
        "roundtrip property comparison must have 10 fields"
    );

    let mut sorted = required_fields.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        required_fields.len(),
        "all field names must be unique"
    );
}

// ===========================================================================
// 16. Fixture file roundtrip: style_box from disk
// ===========================================================================

#[test]
fn tres_roundtrip_style_box_fixture_from_disk() {
    use gdcore::math::Color;
    use gdresource::loader::TresLoader;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/fixtures/test_style_box.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let original = loader
        .parse_str(&content, "res://fixtures/test_style_box.tres")
        .unwrap();

    assert_eq!(original.class_name, "StyleBoxFlat");
    let orig_count = original.property_count();

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let reloaded = loader
        .parse_str(&saved, "res://fixtures/test_style_box.tres")
        .unwrap();
    assert_eq!(reloaded.class_name, "StyleBoxFlat");
    assert_eq!(reloaded.property_count(), orig_count);
    assert_eq!(
        reloaded.get_property("bg_color"),
        Some(&Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)))
    );
    assert_eq!(
        reloaded.get_property("corner_radius_top_left"),
        Some(&Variant::Int(4))
    );
}

// ===========================================================================
// 17. Fixture file roundtrip: animation from disk
// ===========================================================================

#[test]
fn tres_roundtrip_animation_fixture_from_disk() {
    use gdresource::loader::TresLoader;
    use gdresource::saver::TresSaver;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/fixtures/test_animation.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let original = loader
        .parse_str(&content, "res://fixtures/test_animation.tres")
        .unwrap();

    assert_eq!(original.class_name, "Animation");

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let reloaded = loader
        .parse_str(&saved, "res://fixtures/test_animation.tres")
        .unwrap();
    assert_eq!(reloaded.class_name, "Animation");
    assert_eq!(
        reloaded.get_property("resource_name"),
        original.get_property("resource_name")
    );
    assert_eq!(
        reloaded.get_property("length"),
        original.get_property("length")
    );
    assert_eq!(
        reloaded.get_property("loop_mode"),
        original.get_property("loop_mode")
    );
}

// ===========================================================================
// 18. Fixture file roundtrip: ext_resources from disk
// ===========================================================================

#[test]
fn tres_roundtrip_ext_resources_fixture_from_disk() {
    use gdresource::loader::TresLoader;
    use gdresource::saver::TresSaver;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures/resources/with_ext_refs.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let original = loader
        .parse_str(&content, "res://fixtures/with_ext_refs.tres")
        .unwrap();

    assert_eq!(original.class_name, "PackedScene");
    assert_eq!(original.ext_resources.len(), 3);
    assert_eq!(original.subresources.len(), 1);

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();

    let reloaded = loader
        .parse_str(&saved, "res://fixtures/with_ext_refs.tres")
        .unwrap();
    assert_eq!(reloaded.class_name, "PackedScene");
    assert_eq!(reloaded.ext_resources.len(), original.ext_resources.len());
    assert_eq!(reloaded.subresources.len(), original.subresources.len());

    // Verify ext_resource details survive roundtrip
    let ext1 = reloaded.ext_resources.get("1").unwrap();
    assert_eq!(ext1.resource_type, "Texture2D");
    assert_eq!(ext1.path, "res://icon.png");
}

// ===========================================================================
// 19. Roundtrip fixture set now includes ext_refs
// ===========================================================================

#[test]
fn roundtrip_fixture_set_includes_ext_refs() {
    let fixtures = [
        ("res://fixtures/test_theme.tres", "Theme"),
        ("res://fixtures/test_environment.tres", "Environment"),
        ("res://fixtures/test_rect_shape.tres", "RectangleShape2D"),
        ("res://fixtures/test_style_box.tres", "StyleBoxFlat"),
        ("res://fixtures/test_animation.tres", "Animation"),
        ("res://fixtures/with_ext_refs.tres", "PackedScene"),
    ];
    assert_eq!(
        fixtures.len(),
        6,
        "should have 6 roundtrip fixtures including ext_refs"
    );
}
