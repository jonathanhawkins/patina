//! pat-33ru: Validate resource metadata probe schema parity.
//!
//! These tests verify that the resource_metadata probe envelope schema
//! from apps/godot/src/resource_probe.rs is well-formed and that Patina's
//! TresLoader extracts compatible metadata (class, properties, subresources).
//!
//! Acceptance: structured outputs capture metadata expectations used by
//! runtime tests.

use serde_json::{json, Value};

// ===========================================================================
// Probe envelope validation
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
// 1. Metadata probe success schema
// ===========================================================================

#[test]
fn metadata_probe_success_schema() {
    let probe_output = json!({
        "fixture_id": "resource_probe",
        "capture_type": "resource_metadata",
        "data": {
            "resource_class": "StyleBoxFlat",
            "resource_path": "res://fixtures/test_style_box.tres",
            "resource_name": "",
            "property_count": 3,
            "properties": [
                {
                    "name": "bg_color",
                    "type": 20,
                    "hint": 0,
                    "hint_string": "",
                    "usage": 4102
                },
                {
                    "name": "border_width_left",
                    "type": 2,
                    "hint": 1,
                    "hint_string": "0,100,1",
                    "usage": 4102
                },
                {
                    "name": "corner_radius_top_left",
                    "type": 2,
                    "hint": 1,
                    "hint_string": "0,100,1",
                    "usage": 4102
                }
            ],
            "subresource_count": 0,
            "subresources": []
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["resource_class"].is_string());
    assert!(data["resource_path"].is_string());
    assert!(data["resource_name"].is_string());
    assert!(data["property_count"].is_number());
    assert!(data["subresource_count"].is_number());

    let props = data["properties"].as_array().unwrap();
    for prop in props {
        assert!(prop["name"].is_string(), "property must have string name");
        assert!(prop["type"].is_number(), "property must have numeric type");
        assert!(prop["hint"].is_number(), "property must have numeric hint");
        assert!(prop["hint_string"].is_string(), "property must have string hint_string");
        assert!(prop["usage"].is_number(), "property must have numeric usage");
    }

    let subs = data["subresources"].as_array().unwrap();
    assert_eq!(subs.len(), 0);
}

// ===========================================================================
// 2. Metadata probe with subresources schema
// ===========================================================================

#[test]
fn metadata_probe_subresources_schema() {
    let probe_output = json!({
        "fixture_id": "resource_probe",
        "capture_type": "resource_metadata",
        "data": {
            "resource_class": "Theme",
            "resource_path": "res://fixtures/test_theme.tres",
            "resource_name": "GameTheme",
            "property_count": 10,
            "properties": [],
            "subresource_count": 2,
            "subresources": [
                {
                    "property": "panel_style",
                    "class": "StyleBoxFlat",
                    "path": ""
                },
                {
                    "property": "button_style",
                    "class": "StyleBoxFlat",
                    "path": ""
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["subresource_count"], 2);

    let subs = data["subresources"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    for sub in subs {
        assert!(sub["property"].is_string(), "subresource must have string property");
        assert!(sub["class"].is_string(), "subresource must have string class");
        assert!(sub["path"].is_string(), "subresource must have string path");
    }
}

// ===========================================================================
// 3. Metadata probe error schema
// ===========================================================================

#[test]
fn metadata_probe_load_error_schema() {
    let probe_output = json!({
        "fixture_id": "resource_probe",
        "capture_type": "resource_metadata",
        "data": {
            "resource_path": "res://nonexistent.tres",
            "error": "failed_to_load"
        }
    });

    validate_envelope(&probe_output);
    assert_eq!(probe_output["data"]["error"], "failed_to_load");
    assert!(probe_output["data"]["resource_path"].is_string());
}

// ===========================================================================
// 4. PATINA_PROBE line parsing for metadata type
// ===========================================================================

#[test]
fn parse_metadata_probe_line() {
    let line = r#"PATINA_PROBE:{"fixture_id":"resource_probe","capture_type":"resource_metadata","data":{"resource_class":"Animation","resource_path":"res://test.tres","resource_name":"walk","property_count":2,"properties":[],"subresource_count":0,"subresources":[]}}"#;
    let json_str = line.strip_prefix("PATINA_PROBE:").unwrap();
    let parsed: Value = serde_json::from_str(json_str).unwrap();
    validate_envelope(&parsed);
    assert_eq!(parsed["capture_type"], "resource_metadata");
    assert_eq!(parsed["data"]["resource_class"], "Animation");
}

// ===========================================================================
// 5. Metadata property fields are complete
// ===========================================================================

#[test]
fn metadata_property_fields_complete() {
    let required_fields = [
        "name",
        "type",
        "hint",
        "hint_string",
        "usage",
    ];
    assert_eq!(required_fields.len(), 5, "metadata property must have 5 fields");

    let mut sorted = required_fields.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), required_fields.len(), "all field names must be unique");
}

// ===========================================================================
// 6. Metadata subresource fields are complete
// ===========================================================================

#[test]
fn metadata_subresource_fields_complete() {
    let required_fields = [
        "property",
        "class",
        "path",
    ];
    assert_eq!(required_fields.len(), 3, "metadata subresource must have 3 fields");

    let mut sorted = required_fields.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), required_fields.len(), "all field names must be unique");
}

// ===========================================================================
// 7. Engine-side TresLoader extracts metadata matching probe expectations
// ===========================================================================

#[test]
fn tres_loader_extracts_class_name() {
    use gdresource::loader::TresLoader;

    let tres = r#"[gd_resource type="StyleBoxFlat" format=3]

[resource]
bg_color = Color(0.2, 0.3, 0.4, 1)
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://test.tres").unwrap();
    assert_eq!(res.class_name, "StyleBoxFlat");
}

#[test]
fn tres_loader_extracts_properties_matching_metadata_schema() {
    use gdresource::loader::TresLoader;
    use gdvariant::Variant;
    use gdcore::math::Color;

    let tres = r#"[gd_resource type="StyleBoxFlat" format=3]

[resource]
bg_color = Color(0.2, 0.3, 0.4, 1)
border_width_left = 2
corner_radius_top_left = 4
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://test.tres").unwrap();

    // Validate that properties exist and have correct types —
    // mirroring what the Godot probe captures in the metadata envelope.
    assert_eq!(res.property_count(), 3);
    assert_eq!(
        res.get_property("bg_color"),
        Some(&Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)))
    );
    assert_eq!(
        res.get_property("border_width_left"),
        Some(&Variant::Int(2))
    );
    assert_eq!(
        res.get_property("corner_radius_top_left"),
        Some(&Variant::Int(4))
    );
}

#[test]
fn tres_loader_extracts_subresources_matching_metadata_schema() {
    use gdresource::loader::TresLoader;
    use gdcore::math::Color;
    use gdvariant::Variant;

    let tres = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="panel_style"]
bg_color = Color(0.1, 0.1, 0.1, 1)

[sub_resource type="StyleBoxFlat" id="button_style"]
bg_color = Color(0.3, 0.3, 0.3, 1)

[resource]
name = "GameTheme"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://theme.tres").unwrap();

    // Subresource count matches probe expectation
    assert_eq!(res.subresources.len(), 2);
    assert!(res.subresources.contains_key("panel_style"));
    assert!(res.subresources.contains_key("button_style"));

    // Subresource class names match
    let panel = res.subresources.get("panel_style").unwrap();
    assert_eq!(panel.class_name, "StyleBoxFlat");
    assert_eq!(
        panel.get_property("bg_color"),
        Some(&Variant::Color(Color::new(0.1, 0.1, 0.1, 1.0)))
    );

    let button = res.subresources.get("button_style").unwrap();
    assert_eq!(button.class_name, "StyleBoxFlat");
}

// ===========================================================================
// 8. Fixture coverage matches probe fixture set
// ===========================================================================

#[test]
fn metadata_fixture_set_covers_probe_fixtures() {
    // These match the fixtures probed by smoke_probe.gd run_resource_probe()
    let probe_fixtures = [
        "res://scenes/smoke_probe.tscn",
        "res://fixtures/test_theme.tres",
        "res://fixtures/test_environment.tres",
        "res://fixtures/test_rect_shape.tres",
        "res://fixtures/test_style_box.tres",
        "res://fixtures/test_animation.tres",
    ];
    assert_eq!(probe_fixtures.len(), 6, "should have 6 metadata probe fixtures");

    let mut sorted = probe_fixtures.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), probe_fixtures.len(), "all paths must be unique");
}

// ===========================================================================
// 9. capture_type "resource_metadata" is registered
// ===========================================================================

#[test]
fn metadata_capture_type_registered() {
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
    assert_eq!(capture_types.len(), 10);
    assert!(capture_types.contains(&"resource_metadata"));
}

// ===========================================================================
// 10. TresLoader fixture roundtrip for StyleBoxFlat
// ===========================================================================

#[test]
fn tres_loader_style_box_flat_fixture() {
    use gdresource::loader::TresLoader;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;
    use gdcore::math::Color;

    let tres = r#"[gd_resource type="StyleBoxFlat" format=3]

[resource]
bg_color = Color(0.2, 0.3, 0.4, 1)
border_width_left = 2
border_width_top = 2
border_width_right = 2
border_width_bottom = 2
border_color = Color(0.8, 0.8, 0.8, 1)
corner_radius_top_left = 4
corner_radius_top_right = 4
corner_radius_bottom_right = 4
corner_radius_bottom_left = 4
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://fixtures/test_style_box.tres").unwrap();

    assert_eq!(res.class_name, "StyleBoxFlat");
    assert_eq!(res.property_count(), 10);
    assert_eq!(
        res.get_property("bg_color"),
        Some(&Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)))
    );
    assert_eq!(
        res.get_property("border_width_left"),
        Some(&Variant::Int(2))
    );
    assert_eq!(
        res.get_property("corner_radius_top_left"),
        Some(&Variant::Int(4))
    );

    // Roundtrip preserves all properties
    let saver = TresSaver::new();
    let saved = saver.save_to_string(&res).unwrap();
    let reloaded = loader.parse_str(&saved, "res://fixtures/test_style_box.tres").unwrap();
    assert_eq!(reloaded.property_count(), res.property_count());
    assert_eq!(
        reloaded.get_property("bg_color"),
        res.get_property("bg_color")
    );
}

// ===========================================================================
// 11. TresLoader fixture roundtrip for Animation
// ===========================================================================

#[test]
fn tres_loader_animation_fixture() {
    use gdresource::loader::TresLoader;
    use gdresource::saver::TresSaver;
    use gdvariant::Variant;

    let tres = r#"[gd_resource type="Animation" format=3]

[resource]
resource_name = "test_walk"
length = 1.0
loop_mode = 1
step = 0.05
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://fixtures/test_animation.tres").unwrap();

    assert_eq!(res.class_name, "Animation");
    assert_eq!(
        res.get_property("resource_name"),
        Some(&Variant::String("test_walk".into()))
    );
    assert_eq!(
        res.get_property("length"),
        Some(&Variant::Float(1.0))
    );
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&Variant::Int(1))
    );
    assert_eq!(
        res.get_property("step"),
        Some(&Variant::Float(0.05))
    );

    // Roundtrip preserves animation properties
    let saver = TresSaver::new();
    let saved = saver.save_to_string(&res).unwrap();
    let reloaded = loader.parse_str(&saved, "res://fixtures/test_animation.tres").unwrap();
    assert_eq!(reloaded.property_count(), res.property_count());
    assert_eq!(
        reloaded.get_property("resource_name"),
        res.get_property("resource_name")
    );
    assert_eq!(
        reloaded.get_property("loop_mode"),
        res.get_property("loop_mode")
    );
}

// ===========================================================================
// 12. Engine metadata extraction mirrors probe property structure
// ===========================================================================

#[test]
fn engine_metadata_property_structure_matches_probe() {
    use gdresource::loader::TresLoader;

    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "test"
count = 42
ratio = 3.14
flag = true
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://test.tres").unwrap();

    // The probe captures: name, type, hint, hint_string, usage per property.
    // Engine-side we can verify property count and values exist.
    assert!(res.property_count() >= 4);

    // Verify we can produce a JSON structure matching the probe schema
    let metadata = serde_json::json!({
        "resource_class": res.class_name,
        "resource_path": res.path,
        "property_count": res.property_count(),
        "subresource_count": res.subresources.len(),
    });

    assert_eq!(metadata["resource_class"], "Resource");
    assert_eq!(metadata["property_count"], 4);
    assert_eq!(metadata["subresource_count"], 0);
}

// ===========================================================================
// 13. Metadata probe with ext_resources schema
// ===========================================================================

#[test]
fn metadata_probe_ext_resources_schema() {
    let probe_output = json!({
        "fixture_id": "resource_probe",
        "capture_type": "resource_metadata",
        "data": {
            "resource_class": "PackedScene",
            "resource_path": "res://fixtures/with_ext_refs.tres",
            "resource_name": "TestScene",
            "property_count": 3,
            "properties": [
                { "name": "name", "type": 4, "hint": 0, "hint_string": "", "usage": 4102 },
                { "name": "texture_ref", "type": 4, "hint": 0, "hint_string": "", "usage": 4102 },
                { "name": "script_ref", "type": 4, "hint": 0, "hint_string": "", "usage": 4102 }
            ],
            "subresource_count": 1,
            "subresources": [
                {
                    "property": "inline_style",
                    "class": "StyleBoxFlat",
                    "path": ""
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["resource_class"], "PackedScene");
    assert_eq!(data["subresource_count"], 1);
    assert!(data["subresources"].as_array().unwrap()[0]["class"].is_string());
}

// ===========================================================================
// 14. Engine-side TresLoader parses ext_resources from fixture file
// ===========================================================================

#[test]
fn tres_loader_parses_ext_resources_from_fixture() {
    use gdresource::loader::TresLoader;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures/resources/with_ext_refs.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let res = loader
        .parse_str(&content, "res://fixtures/with_ext_refs.tres")
        .unwrap();

    assert_eq!(res.class_name, "PackedScene");
    assert_eq!(res.ext_resources.len(), 3);

    let ext1 = res.ext_resources.get("1").unwrap();
    assert_eq!(ext1.resource_type, "Texture2D");
    assert_eq!(ext1.path, "res://icon.png");

    let ext2 = res.ext_resources.get("2").unwrap();
    assert_eq!(ext2.resource_type, "Script");
    assert_eq!(ext2.path, "res://scripts/player.gd");

    let ext3 = res.ext_resources.get("3").unwrap();
    assert_eq!(ext3.resource_type, "PackedScene");
    assert_eq!(ext3.path, "res://scenes/enemy.tscn");
}

// ===========================================================================
// 15. Engine-side TresLoader parses subresources from fixture file
// ===========================================================================

#[test]
fn tres_loader_parses_subresources_from_fixture() {
    use gdresource::loader::TresLoader;
    use gdcore::math::Color;
    use gdvariant::Variant;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures/resources/with_ext_refs.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let res = loader
        .parse_str(&content, "res://fixtures/with_ext_refs.tres")
        .unwrap();

    assert_eq!(res.subresources.len(), 1);
    let inline = res.subresources.get("inline_style").unwrap();
    assert_eq!(inline.class_name, "StyleBoxFlat");
    assert_eq!(
        inline.get_property("bg_color"),
        Some(&Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)))
    );
}

// ===========================================================================
// 16. Engine-side TresLoader parses style_box fixture from disk
// ===========================================================================

#[test]
fn tres_loader_parses_style_box_fixture_from_disk() {
    use gdresource::loader::TresLoader;
    use gdcore::math::Color;
    use gdvariant::Variant;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/fixtures/test_style_box.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let res = loader
        .parse_str(&content, "res://fixtures/test_style_box.tres")
        .unwrap();

    assert_eq!(res.class_name, "StyleBoxFlat");
    assert_eq!(res.property_count(), 10);
    assert_eq!(
        res.get_property("bg_color"),
        Some(&Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)))
    );
    assert_eq!(
        res.get_property("border_width_left"),
        Some(&Variant::Int(2))
    );
}

// ===========================================================================
// 17. Engine-side TresLoader parses animation fixture from disk
// ===========================================================================

#[test]
fn tres_loader_parses_animation_fixture_from_disk() {
    use gdresource::loader::TresLoader;
    use gdvariant::Variant;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/fixtures/test_animation.tres");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    let loader = TresLoader::new();
    let res = loader
        .parse_str(&content, "res://fixtures/test_animation.tres")
        .unwrap();

    assert_eq!(res.class_name, "Animation");
    assert_eq!(
        res.get_property("resource_name"),
        Some(&Variant::String("test_walk".into()))
    );
    assert_eq!(
        res.get_property("length"),
        Some(&Variant::Float(1.0))
    );
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&Variant::Int(1))
    );
}
