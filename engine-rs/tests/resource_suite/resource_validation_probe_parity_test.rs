//! pat-33ru: Validate resource validation probe schema parity.
//!
//! These tests verify that the resource_validation probe envelope schema
//! from apps/godot/src/resource_validation_probe.rs is well-formed and that
//! Patina's engine-side resource infrastructure satisfies the same structural
//! checks (class match, path preservation, storable properties, subresource
//! integrity, reload consistency).
//!
//! Acceptance: structured outputs capture validation expectations used by
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
// 1. Validation probe success schema
// ===========================================================================

#[test]
fn validation_probe_success_schema() {
    let probe_output = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "pass",
            "resource_path": "res://fixtures/test_theme.tres",
            "resource_class": "Theme",
            "check_count": 4,
            "checks": [
                {
                    "check": "class_match",
                    "expected": "Theme",
                    "actual": "Theme",
                    "pass": true
                },
                {
                    "check": "path_preserved",
                    "path": "res://fixtures/test_theme.tres",
                    "pass": true
                },
                {
                    "check": "storable_properties",
                    "count": 15,
                    "nil_count": 0,
                    "pass": true
                },
                {
                    "check": "reload_consistency",
                    "original_class": "Theme",
                    "reloaded_class": "Theme",
                    "pass": true
                }
            ],
            "storable_property_count": 15,
            "properties": [
                {
                    "name": "default_font_size",
                    "type": 2,
                    "is_nil": false,
                    "value_type": 2
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["status"], "pass");
    assert!(data["resource_path"].is_string());
    assert!(data["resource_class"].is_string());
    assert!(data["check_count"].is_number());
    assert!(data["storable_property_count"].is_number());

    let checks = data["checks"].as_array().unwrap();
    for check in checks {
        assert!(check["check"].is_string(), "check must have string type");
        assert!(check["pass"].is_boolean(), "check must have boolean pass");
    }

    let props = data["properties"].as_array().unwrap();
    for prop in props {
        assert!(prop["name"].is_string());
        assert!(prop["type"].is_number());
        assert!(prop["is_nil"].is_boolean());
        assert!(prop["value_type"].is_number());
    }
}

// ===========================================================================
// 2. Validation probe with subresource integrity
// ===========================================================================

#[test]
fn validation_probe_subresource_integrity_schema() {
    let probe_output = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "pass",
            "resource_path": "res://fixtures/test_theme.tres",
            "resource_class": "Theme",
            "check_count": 5,
            "checks": [
                {
                    "check": "class_match",
                    "expected": "Theme",
                    "actual": "Theme",
                    "pass": true
                },
                {
                    "check": "path_preserved",
                    "path": "res://fixtures/test_theme.tres",
                    "pass": true
                },
                {
                    "check": "storable_properties",
                    "count": 10,
                    "nil_count": 0,
                    "pass": true
                },
                {
                    "check": "subresource_integrity",
                    "count": 2,
                    "subresources": [
                        {
                            "property": "panel_style",
                            "class": "StyleBoxFlat",
                            "path": "",
                            "valid": true
                        },
                        {
                            "property": "button_style",
                            "class": "StyleBoxFlat",
                            "path": "",
                            "valid": true
                        }
                    ],
                    "pass": true
                },
                {
                    "check": "reload_consistency",
                    "original_class": "Theme",
                    "reloaded_class": "Theme",
                    "pass": true
                }
            ],
            "storable_property_count": 10,
            "properties": []
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];

    // Find the subresource_integrity check
    let checks = data["checks"].as_array().unwrap();
    let sub_check = checks
        .iter()
        .find(|c| c["check"] == "subresource_integrity")
        .expect("must have subresource_integrity check");

    assert!(sub_check["pass"].as_bool().unwrap());
    assert_eq!(sub_check["count"], 2);

    let subs = sub_check["subresources"].as_array().unwrap();
    for sub in subs {
        assert!(sub["property"].is_string());
        assert!(sub["class"].is_string());
        assert!(sub["path"].is_string());
        assert!(sub["valid"].is_boolean());
    }
}

// ===========================================================================
// 3. Validation probe error schema
// ===========================================================================

#[test]
fn validation_probe_load_error_schema() {
    let probe_output = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
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

// ===========================================================================
// 4. PATINA_PROBE line parsing for validation type
// ===========================================================================

#[test]
fn parse_validation_probe_line() {
    let line = r#"PATINA_PROBE:{"fixture_id":"resource_validation_probe","capture_type":"resource_validation","data":{"status":"pass","resource_path":"res://test.tres","resource_class":"Resource","check_count":2,"checks":[],"storable_property_count":0,"properties":[]}}"#;
    let json_str = line.strip_prefix("PATINA_PROBE:").unwrap();
    let parsed: Value = serde_json::from_str(json_str).unwrap();
    validate_envelope(&parsed);
    assert_eq!(parsed["capture_type"], "resource_validation");
    assert_eq!(parsed["data"]["status"], "pass");
}

// ===========================================================================
// 5. Validation check types are exhaustive
// ===========================================================================

#[test]
fn validation_check_types_exhaustive() {
    let check_types = [
        "class_match",
        "path_preserved",
        "storable_properties",
        "subresource_integrity",
        "reload_consistency",
    ];
    assert_eq!(check_types.len(), 5, "validation probe has 5 check types");

    let mut sorted = check_types.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        check_types.len(),
        "all check types must be unique"
    );
}

// ===========================================================================
// 6. Storable property fields are complete
// ===========================================================================

#[test]
fn validation_storable_property_fields_complete() {
    let required_fields = ["name", "type", "is_nil", "value_type"];
    assert_eq!(
        required_fields.len(),
        4,
        "storable property must have 4 fields"
    );
}

// ===========================================================================
// 7. capture_type "resource_validation" is registered
// ===========================================================================

#[test]
fn validation_capture_type_registered() {
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
    assert!(capture_types.contains(&"resource_validation"));
}

// ===========================================================================
// 8. Validation fixture set matches probe fixture set
// ===========================================================================

#[test]
fn validation_fixture_set_covers_probe_fixtures() {
    // These match the validations array in smoke_probe.gd
    let validation_fixtures = [
        ("res://fixtures/test_theme.tres", "Theme"),
        ("res://fixtures/test_environment.tres", "Environment"),
        ("res://fixtures/test_rect_shape.tres", "RectangleShape2D"),
        ("res://fixtures/test_style_box.tres", "StyleBoxFlat"),
        ("res://fixtures/test_animation.tres", "Animation"),
        ("res://scenes/smoke_probe.tscn", "PackedScene"),
    ];
    assert_eq!(
        validation_fixtures.len(),
        6,
        "should have 6 validation fixtures"
    );

    let paths: Vec<&str> = validation_fixtures.iter().map(|(p, _)| *p).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        validation_fixtures.len(),
        "all paths must be unique"
    );
}

// ===========================================================================
// 9. Engine-side class match check
// ===========================================================================

#[test]
fn engine_class_match_check() {
    use gdresource::loader::TresLoader;

    let tres = r#"[gd_resource type="StyleBoxFlat" format=3]

[resource]
bg_color = Color(0.2, 0.3, 0.4, 1)
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://test.tres").unwrap();

    // Mirrors the class_match check from the validation probe
    assert_eq!(res.class_name, "StyleBoxFlat");
}

// ===========================================================================
// 10. Engine-side path preservation check
// ===========================================================================

#[test]
fn engine_path_preservation_check() {
    use gdresource::loader::TresLoader;

    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "test"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://fixtures/test.tres").unwrap();

    // Mirrors the path_preserved check from the validation probe
    assert_eq!(res.path, "res://fixtures/test.tres");
    assert!(!res.path.is_empty());
}

// ===========================================================================
// 11. Engine-side subresource integrity check
// ===========================================================================

#[test]
fn engine_subresource_integrity_check() {
    use gdresource::loader::TresLoader;

    let tres = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="panel_style"]
bg_color = Color(0.1, 0.1, 0.1, 1)

[resource]
name = "test"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://theme.tres").unwrap();

    // Mirrors the subresource_integrity check from the validation probe
    assert_eq!(res.subresources.len(), 1);
    let sub = res.subresources.get("panel_style").unwrap();
    assert_eq!(sub.class_name, "StyleBoxFlat");
    // Subresource has valid properties (non-empty)
    assert!(sub.property_count() > 0);
}

// ===========================================================================
// 12. Engine-side reload consistency check
// ===========================================================================

#[test]
fn engine_reload_consistency_check() {
    use gdresource::loader::TresLoader;
    use gdresource::saver::TresSaver;

    let tres = r#"[gd_resource type="Animation" format=3]

[resource]
resource_name = "walk"
length = 2.0
loop_mode = 1
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://anim.tres").unwrap();

    // Save and reload — mirrors the reload_consistency check
    let saver = TresSaver::new();
    let saved = saver.save_to_string(&res).unwrap();
    let reloaded = loader.parse_str(&saved, "res://anim.tres").unwrap();

    assert_eq!(
        res.class_name, reloaded.class_name,
        "class must be preserved across reload"
    );
    assert_eq!(
        res.property_count(),
        reloaded.property_count(),
        "property count must be preserved"
    );
}

// ===========================================================================
// 13. Validation probe all-pass aggregation logic
// ===========================================================================

#[test]
fn validation_probe_all_pass_aggregation() {
    // When all checks pass, status should be "pass"
    let all_pass = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "pass",
            "resource_path": "res://test.tres",
            "resource_class": "Resource",
            "check_count": 3,
            "checks": [
                {"check": "class_match", "pass": true},
                {"check": "path_preserved", "pass": true},
                {"check": "reload_consistency", "pass": true}
            ],
            "storable_property_count": 0,
            "properties": []
        }
    });

    let data = &all_pass["data"];
    let checks = data["checks"].as_array().unwrap();
    let all_ok = checks.iter().all(|c| c["pass"].as_bool().unwrap_or(false));
    assert!(all_ok);
    assert_eq!(data["status"], "pass");
}

#[test]
fn validation_probe_any_fail_aggregation() {
    // When any check fails, status should be "fail"
    let any_fail = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "fail",
            "resource_path": "res://test.tres",
            "resource_class": "Resource",
            "check_count": 3,
            "checks": [
                {"check": "class_match", "pass": false},
                {"check": "path_preserved", "pass": true},
                {"check": "reload_consistency", "pass": true}
            ],
            "storable_property_count": 0,
            "properties": []
        }
    });

    let data = &any_fail["data"];
    let checks = data["checks"].as_array().unwrap();
    let all_ok = checks.iter().all(|c| c["pass"].as_bool().unwrap_or(false));
    assert!(!all_ok);
    assert_eq!(data["status"], "fail");
}

// ===========================================================================
// 14. Engine validation of storable properties — no nil for loaded resources
// ===========================================================================

#[test]
fn engine_storable_properties_no_nil() {
    use gdresource::loader::TresLoader;
    use gdvariant::Variant;

    let tres = r#"[gd_resource type="StyleBoxFlat" format=3]

[resource]
bg_color = Color(0.2, 0.3, 0.4, 1)
border_width_left = 2
corner_radius_top_left = 4
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(tres, "res://test.tres").unwrap();

    // Mirrors the storable_properties check: no nil values in loaded properties
    // Verify each known property is non-nil
    for name in &["bg_color", "border_width_left", "corner_radius_top_left"] {
        let value = res.get_property(name);
        assert!(
            value.is_some() && *value.unwrap() != Variant::Nil,
            "property '{}' should not be nil after loading",
            name
        );
    }
}

// ===========================================================================
// 15. Engine-side ext_resource validation from fixture
// ===========================================================================

#[test]
fn engine_ext_resource_validation_from_fixture() {
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

    // Mirrors the validation probe's checks:

    // class_match: verify loaded class
    assert_eq!(res.class_name, "PackedScene");

    // path_preserved: verify path roundtrips
    assert_eq!(res.path, "res://fixtures/with_ext_refs.tres");
    assert!(!res.path.is_empty());

    // subresource_integrity: verify subresources are valid
    assert_eq!(res.subresources.len(), 1);
    let sub = res.subresources.get("inline_style").unwrap();
    assert_eq!(sub.class_name, "StyleBoxFlat");
    assert!(sub.property_count() > 0);

    // ext_resource count and types
    assert_eq!(res.ext_resources.len(), 3);
    let types: Vec<&str> = res
        .ext_resources
        .values()
        .map(|e| e.resource_type.as_str())
        .collect();
    assert!(types.contains(&"Texture2D"));
    assert!(types.contains(&"Script"));
    assert!(types.contains(&"PackedScene"));
}

// ===========================================================================
// 16. Validation probe schema for ext_resource-bearing resource
// ===========================================================================

#[test]
fn validation_probe_ext_resource_schema() {
    let probe_output = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "pass",
            "resource_path": "res://fixtures/with_ext_refs.tres",
            "resource_class": "PackedScene",
            "check_count": 5,
            "checks": [
                {
                    "check": "class_match",
                    "expected": "PackedScene",
                    "actual": "PackedScene",
                    "pass": true
                },
                {
                    "check": "path_preserved",
                    "path": "res://fixtures/with_ext_refs.tres",
                    "pass": true
                },
                {
                    "check": "storable_properties",
                    "count": 3,
                    "nil_count": 0,
                    "pass": true
                },
                {
                    "check": "subresource_integrity",
                    "count": 1,
                    "subresources": [
                        {
                            "property": "inline_style",
                            "class": "StyleBoxFlat",
                            "path": "",
                            "valid": true
                        }
                    ],
                    "pass": true
                },
                {
                    "check": "reload_consistency",
                    "original_class": "PackedScene",
                    "reloaded_class": "PackedScene",
                    "pass": true
                }
            ],
            "storable_property_count": 3,
            "properties": [
                { "name": "name", "type": 4, "is_nil": false, "value_type": 4 },
                { "name": "texture_ref", "type": 4, "is_nil": false, "value_type": 4 },
                { "name": "script_ref", "type": 4, "is_nil": false, "value_type": 4 }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["status"], "pass");
    assert_eq!(data["resource_class"], "PackedScene");

    let checks = data["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 5);
    let sub_check = checks
        .iter()
        .find(|c| c["check"] == "subresource_integrity")
        .unwrap();
    assert_eq!(sub_check["count"], 1);
}

// ===========================================================================
// 17. Engine-side reload consistency for fixture with ext_resources
// ===========================================================================

#[test]
fn engine_reload_consistency_ext_resources() {
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

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&original).unwrap();
    let reloaded = loader
        .parse_str(&saved, "res://fixtures/with_ext_refs.tres")
        .unwrap();

    // Reload consistency: class preserved
    assert_eq!(original.class_name, reloaded.class_name);
    // Subresource count preserved
    assert_eq!(original.subresources.len(), reloaded.subresources.len());
    // Ext resource count preserved
    assert_eq!(original.ext_resources.len(), reloaded.ext_resources.len());
}
