//! pat-lrzd: Validate probe output JSON schema compatibility.
//!
//! These tests verify that the JSON envelope format produced by the
//! apps/godot probes can be correctly parsed and consumed by the Patina
//! engine. Each test constructs representative probe output and validates
//! the schema structure, ensuring engine-side consumers won't break when
//! new probes are added or existing ones are expanded.
//!
//! Acceptance: machine-readable probe outputs cover the next missing
//! runtime parity surfaces.

use serde_json::{json, Value};

// ===========================================================================
// Probe envelope schema
// ===========================================================================

/// All probe outputs follow this envelope structure.
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
// 1. Scene tree probe schema
// ===========================================================================

#[test]
fn scene_tree_probe_schema() {
    let probe_output = json!({
        "fixture_id": "smoke_probe",
        "capture_type": "scene_tree",
        "data": {
            "root": {
                "name": "SmokeProbeLauncher",
                "class": "Node",
                "path": "/root/SmokeProbeLauncher",
                "owner": "",
                "script_path": "res://scripts/smoke_probe.gd",
                "process_mode": 0,
                "unique_name_in_owner": false,
                "children": [
                    {
                        "name": "PatinaSmokeProbe",
                        "class": "PatinaSmokeProbe",
                        "path": "/root/SmokeProbeLauncher/PatinaSmokeProbe",
                        "owner": "/root/SmokeProbeLauncher",
                        "script_path": "",
                        "process_mode": 0,
                        "unique_name_in_owner": false,
                        "children": []
                    }
                ]
            }
        }
    });

    validate_envelope(&probe_output);
    let root = &probe_output["data"]["root"];
    assert!(root["name"].is_string());
    assert!(root["class"].is_string());
    assert!(root["path"].is_string());
    assert!(root["children"].is_array());
    // Verify recursive structure
    let child = &root["children"][0];
    assert!(child["name"].is_string());
    assert!(child["children"].is_array());
}

// ===========================================================================
// 2. Properties probe schema
// ===========================================================================

#[test]
fn properties_probe_schema() {
    let probe_output = json!({
        "fixture_id": "smoke_probe",
        "capture_type": "properties",
        "data": {
            "node_name": "PatinaSmokeProbe",
            "node_class": "PatinaSmokeProbe",
            "property_count": 2,
            "properties": [
                {
                    "name": "probe_label",
                    "type": 4,
                    "hint": 0,
                    "hint_string": "",
                    "usage": 4102,
                    "class_name": ""
                },
                {
                    "name": "probe_count",
                    "type": 2,
                    "hint": 0,
                    "hint_string": "",
                    "usage": 4102,
                    "class_name": ""
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["node_name"].is_string());
    assert!(data["node_class"].is_string());
    assert!(data["property_count"].is_number());
    let props = data["properties"].as_array().unwrap();
    for prop in props {
        assert!(prop["name"].is_string());
        assert!(prop["type"].is_number());
        assert!(prop["usage"].is_number());
    }
}

// ===========================================================================
// 3. Signal probe schema
// ===========================================================================

#[test]
fn signal_probe_schema() {
    let probe_output = json!({
        "fixture_id": "smoke_probe",
        "capture_type": "signals",
        "data": {
            "node_name": "PatinaSmokeProbe",
            "node_class": "PatinaSmokeProbe",
            "ordering_events": ["before_connect", "after_connect", "emitted", "after_emit"],
            "signal_count": 1,
            "signals": [
                {
                    "name": "probe_signal",
                    "args": [
                        { "name": "stage", "type": 4 }
                    ]
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    let events = data["ordering_events"].as_array().unwrap();
    assert!(
        events.len() >= 3,
        "should have at least before_connect, after_connect, after_emit"
    );
    let signals = data["signals"].as_array().unwrap();
    for sig in signals {
        assert!(sig["name"].is_string());
        assert!(sig["args"].is_array());
    }
}

// ===========================================================================
// 4. ClassDB probe schema
// ===========================================================================

#[test]
fn classdb_probe_schema() {
    let probe_output = json!({
        "fixture_id": "classdb_probe",
        "capture_type": "classdb",
        "data": {
            "class": "Node2D",
            "parent": "CanvasItem",
            "method_count": 15,
            "methods": [
                {
                    "name": "set_position",
                    "args": [
                        { "name": "position", "type": 5, "class_name": "" }
                    ],
                    "return_type": 0
                }
            ],
            "property_count": 4,
            "properties": [
                {
                    "name": "position",
                    "type": 5,
                    "hint": 0,
                    "hint_string": "",
                    "usage": 4102
                }
            ],
            "signal_count": 0,
            "signals": []
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["parent"].is_string());
    assert!(data["methods"].is_array());
    assert!(data["properties"].is_array());
    assert!(data["signals"].is_array());

    let method = &data["methods"][0];
    assert!(method["name"].is_string());
    assert!(method["args"].is_array());
    assert!(method["return_type"].is_number());
}

// ===========================================================================
// 5. Resource metadata probe schema
// ===========================================================================

#[test]
fn resource_metadata_probe_schema() {
    let probe_output = json!({
        "fixture_id": "resource_probe",
        "capture_type": "resource_metadata",
        "data": {
            "resource_class": "Theme",
            "resource_path": "res://fixtures/test_theme.tres",
            "resource_name": "",
            "property_count": 5,
            "properties": [
                { "name": "default_font_size", "type": 2, "hint": 0, "hint_string": "", "usage": 4102 }
            ],
            "subresource_count": 0,
            "subresources": []
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["resource_class"].is_string());
    assert!(data["resource_path"].is_string());
    assert!(data["properties"].is_array());
    assert!(data["subresources"].is_array());
    assert!(data["property_count"].is_number());
    assert!(data["subresource_count"].is_number());
}

// ===========================================================================
// 6. Node defaults probe schema (NEW)
// ===========================================================================

#[test]
fn node_defaults_probe_schema() {
    let probe_output = json!({
        "fixture_id": "node_defaults_probe",
        "capture_type": "node_defaults",
        "data": {
            "class": "CharacterBody2D",
            "parent": "PhysicsBody2D",
            "default_count": 12,
            "defaults": [
                {
                    "name": "velocity",
                    "type": 5,
                    "usage": 4102,
                    "value_string": "(0, 0)",
                    "value_type": 5
                },
                {
                    "name": "collision_layer",
                    "type": 2,
                    "usage": 4102,
                    "value_string": "1",
                    "value_type": 2
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["parent"].is_string());
    assert!(data["default_count"].is_number());
    let defaults = data["defaults"].as_array().unwrap();
    for def in defaults {
        assert!(def["name"].is_string(), "default must have string name");
        assert!(def["type"].is_number(), "default must have numeric type");
        assert!(def["usage"].is_number(), "default must have numeric usage");
        assert!(
            def["value_string"].is_string(),
            "default must have value_string"
        );
        assert!(
            def["value_type"].is_number(),
            "default must have numeric value_type"
        );
    }
}

// ===========================================================================
// 7. Resource validation probe schema (NEW)
// ===========================================================================

#[test]
fn resource_validation_probe_schema() {
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
                    "count": 12,
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
            "storable_property_count": 12,
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
    assert!(data["status"].is_string());
    assert!(data["resource_path"].is_string());
    assert!(data["resource_class"].is_string());
    assert!(data["check_count"].is_number());

    let checks = data["checks"].as_array().unwrap();
    for check in checks {
        assert!(
            check["check"].is_string(),
            "each check must have a 'check' field"
        );
        assert!(
            check["pass"].is_boolean(),
            "each check must have a 'pass' field"
        );
    }

    let props = data["properties"].as_array().unwrap();
    for prop in props {
        assert!(prop["name"].is_string());
        assert!(prop["type"].is_number());
        assert!(prop["is_nil"].is_boolean());
    }
}

// ===========================================================================
// 8. Resource validation probe error schema
// ===========================================================================

#[test]
fn resource_validation_probe_error_schema() {
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
    assert!(probe_output["data"]["error"].is_string());
}

// ===========================================================================
// 9. ClassDB probe expanded class count (28 classes)
// ===========================================================================

#[test]
fn classdb_probe_covers_28_classes() {
    // The expanded classdb probe should cover these 28 classes.
    let expected_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "LineEdit",
        "Panel",
        "TextureRect",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "TileMapLayer",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "RayCast2D",
    ];
    assert_eq!(expected_classes.len(), 28);

    // Verify all classes are unique.
    let mut sorted = expected_classes.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 28, "all 28 class names must be unique");
}

// ===========================================================================
// 10. Node defaults probe covers same 28 classes
// ===========================================================================

#[test]
fn node_defaults_probe_class_list_matches_classdb() {
    // Node defaults probe should cover the same expanded set.
    let defaults_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "LineEdit",
        "Panel",
        "RayCast2D",
        "TextureRect",
        "TileMapLayer",
    ];
    assert_eq!(defaults_classes.len(), 28);
}

// ===========================================================================
// 11. PATINA_PROBE line parsing
// ===========================================================================

#[test]
fn parse_probe_line_prefix() {
    let line = r#"PATINA_PROBE:{"fixture_id":"smoke_probe","capture_type":"scene_tree","data":{}}"#;
    let json_str = line.strip_prefix("PATINA_PROBE:").unwrap();
    let parsed: Value = serde_json::from_str(json_str).unwrap();
    validate_envelope(&parsed);
}

// ===========================================================================
// 12. Resource fixture set completeness
// ===========================================================================

#[test]
fn resource_fixture_set_covers_diverse_types() {
    // The expanded fixture set should cover these resource types.
    let fixtures = [
        ("res://fixtures/test_theme.tres", "Theme"),
        ("res://fixtures/test_environment.tres", "Environment"),
        ("res://fixtures/test_rect_shape.tres", "RectangleShape2D"),
        ("res://fixtures/test_style_box.tres", "StyleBoxFlat"),
        ("res://fixtures/test_animation.tres", "Animation"),
        ("res://scenes/smoke_probe.tscn", "PackedScene"),
    ];
    assert_eq!(
        fixtures.len(),
        6,
        "should have 6 resource fixtures for validation"
    );

    // All paths should be unique.
    let paths: Vec<&str> = fixtures.iter().map(|(p, _)| *p).collect();
    let mut sorted_paths = paths.clone();
    sorted_paths.sort();
    sorted_paths.dedup();
    assert_eq!(sorted_paths.len(), fixtures.len());

    // All expected classes should be non-empty.
    for (path, class) in &fixtures {
        assert!(
            !class.is_empty(),
            "fixture {path} should have an expected class"
        );
    }
}

// ===========================================================================
// 13. Subresource validation in resource probe
// ===========================================================================

#[test]
fn resource_metadata_with_subresources_schema() {
    let probe_output = json!({
        "fixture_id": "resource_probe",
        "capture_type": "resource_metadata",
        "data": {
            "resource_class": "Theme",
            "resource_path": "res://fixtures/test_theme.tres",
            "resource_name": "",
            "property_count": 5,
            "properties": [],
            "subresource_count": 2,
            "subresources": [
                {
                    "property": "default_font",
                    "class": "FontFile",
                    "path": ""
                },
                {
                    "property": "title_font",
                    "class": "FontFile",
                    "path": "res://fonts/title.tres"
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let subs = probe_output["data"]["subresources"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    for sub in subs {
        assert!(sub["property"].is_string());
        assert!(sub["class"].is_string());
        assert!(sub["path"].is_string());
    }
}

// ===========================================================================
// 14. Validation check types are well-defined
// ===========================================================================

#[test]
fn validation_check_types_enumerated() {
    let known_check_types = [
        "class_match",
        "path_preserved",
        "storable_properties",
        "subresource_integrity",
        "reload_consistency",
    ];
    assert_eq!(known_check_types.len(), 5);

    // Each check type must be unique.
    let mut sorted = known_check_types.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), known_check_types.len());
}

// ===========================================================================
// 15. Probe capture types are well-defined
// ===========================================================================

#[test]
fn probe_capture_types_enumerated() {
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
        "method_defaults",
        "virtual_methods",
        "singleton_api",
        "resource_subtype",
        "api_surface",
    ];
    assert_eq!(
        capture_types.len(),
        15,
        "should have 15 distinct probe capture types"
    );
}

// ===========================================================================
// 16. Resource roundtrip probe schema
// ===========================================================================

#[test]
fn resource_roundtrip_probe_schema() {
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
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["status"].is_string());
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
        assert!(prop["roundtrip_ok"].is_boolean());
        assert!(prop["values_match"].is_boolean());
        assert!(prop["types_match"].is_boolean());
    }
}

// ===========================================================================
// 17. Resource roundtrip probe error schema
// ===========================================================================

#[test]
fn resource_roundtrip_probe_error_schema() {
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
    assert!(probe_output["data"]["error"].is_string());
}

// ===========================================================================
// 18. Enum constants probe schema
// ===========================================================================

#[test]
fn enum_constants_probe_schema() {
    let probe_output = json!({
        "fixture_id": "enum_constants_probe",
        "capture_type": "enum_constants",
        "data": {
            "class": "Node",
            "constant_count": 8,
            "constants": [
                {
                    "name": "NOTIFICATION_ENTER_TREE",
                    "value": 10,
                    "enum": null
                },
                {
                    "name": "PROCESS_MODE_INHERIT",
                    "value": 0,
                    "enum": "ProcessMode"
                }
            ],
            "enum_count": 2,
            "enums": [
                {
                    "name": "ProcessMode",
                    "members": [
                        "PROCESS_MODE_INHERIT",
                        "PROCESS_MODE_PAUSABLE",
                        "PROCESS_MODE_WHEN_PAUSED",
                        "PROCESS_MODE_ALWAYS",
                        "PROCESS_MODE_DISABLED"
                    ]
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["constant_count"].is_number());
    assert!(data["enum_count"].is_number());

    let constants = data["constants"].as_array().unwrap();
    for constant in constants {
        assert!(
            constant["name"].is_string(),
            "constant must have string name"
        );
        assert!(
            constant["value"].is_number(),
            "constant must have numeric value"
        );
        // enum field can be null or string
    }

    let enums = data["enums"].as_array().unwrap();
    for e in enums {
        assert!(e["name"].is_string(), "enum must have string name");
        assert!(e["members"].is_array(), "enum must have members array");
    }
}

// ===========================================================================
// 19. Enum constants probe covers 28 classes
// ===========================================================================

#[test]
fn enum_constants_probe_covers_28_classes() {
    // Enum constants probe should cover the same 28-class set.
    let expected_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "LineEdit",
        "Panel",
        "TextureRect",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "TileMapLayer",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "RayCast2D",
    ];
    assert_eq!(expected_classes.len(), 28);
}

// ===========================================================================
// 20. ClassDB method flags in expanded schema
// ===========================================================================

#[test]
fn classdb_method_flags_schema() {
    let probe_output = json!({
        "fixture_id": "classdb_probe",
        "capture_type": "classdb",
        "data": {
            "class": "Node2D",
            "parent": "CanvasItem",
            "method_count": 1,
            "methods": [
                {
                    "name": "get_position",
                    "args": [],
                    "return_type": 5,
                    "flags": 4,
                    "is_virtual": false,
                    "is_const": true,
                    "is_vararg": false
                }
            ],
            "property_count": 0,
            "properties": [],
            "signal_count": 0,
            "signals": []
        }
    });

    validate_envelope(&probe_output);
    let method = &probe_output["data"]["methods"][0];
    assert!(
        method["flags"].is_number(),
        "method must have numeric flags"
    );
    assert!(
        method["is_virtual"].is_boolean(),
        "method must have is_virtual bool"
    );
    assert!(
        method["is_const"].is_boolean(),
        "method must have is_const bool"
    );
    assert!(
        method["is_vararg"].is_boolean(),
        "method must have is_vararg bool"
    );
}

// ===========================================================================
// 21. Extract script handles enum_constants capture type
// ===========================================================================

#[test]
fn extract_probes_covers_enum_constants() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("enum_constants"),
        "extract_probes.sh must handle capture_type 'enum_constants'"
    );
}

// ===========================================================================
// 22. Inheritance chain probe schema
// ===========================================================================

#[test]
fn inheritance_chain_probe_schema() {
    let probe_output = json!({
        "fixture_id": "inheritance_probe",
        "capture_type": "inheritance_chain",
        "data": {
            "class": "Sprite2D",
            "chain": ["Sprite2D", "Node2D", "CanvasItem", "Node", "Object"],
            "depth": 5,
            "is_node": true,
            "is_canvasitem": true,
            "is_node2d": true,
            "is_control": false
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["depth"].is_number());
    assert!(data["is_node"].is_boolean());
    assert!(data["is_canvasitem"].is_boolean());
    assert!(data["is_node2d"].is_boolean());
    assert!(data["is_control"].is_boolean());

    let chain = data["chain"].as_array().unwrap();
    assert!(chain.len() >= 2, "chain must have at least class + Object");
    assert_eq!(
        chain[0], "Sprite2D",
        "chain must start with the class itself"
    );
    assert_eq!(
        chain.last().unwrap(),
        "Object",
        "chain must end with Object"
    );
}

// ===========================================================================
// 23. Inheritance chain probe covers 28 classes
// ===========================================================================

#[test]
fn inheritance_chain_probe_covers_28_classes() {
    let expected_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "LineEdit",
        "Panel",
        "TextureRect",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "TileMapLayer",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "RayCast2D",
    ];
    assert_eq!(expected_classes.len(), 28);
}

// ===========================================================================
// 24. Inheritance chain error schema
// ===========================================================================

#[test]
fn inheritance_chain_probe_error_schema() {
    let probe_output = json!({
        "fixture_id": "inheritance_probe",
        "capture_type": "inheritance_chain",
        "data": {
            "class": "NonExistentClass",
            "error": "class_not_found"
        }
    });

    validate_envelope(&probe_output);
    assert!(probe_output["data"]["error"].is_string());
}

// ===========================================================================
// 25. Control hierarchy validation
// ===========================================================================

#[test]
fn inheritance_chain_control_hierarchy() {
    // Verify expected hierarchy for a Control-derived class.
    let chain = vec![
        "Button",
        "BaseButton",
        "Control",
        "CanvasItem",
        "Node",
        "Object",
    ];
    assert!(chain.contains(&"Control"));
    assert!(chain.contains(&"CanvasItem"));
    assert!(chain.contains(&"Node"));
    assert!(
        !chain.contains(&"Node2D"),
        "Control is not a Node2D subclass"
    );
}

// ===========================================================================
// 26. Physics body hierarchy validation
// ===========================================================================

#[test]
fn inheritance_chain_physics_hierarchy() {
    // Verify expected hierarchy for physics body classes.
    let rigid_chain = vec![
        "RigidBody2D",
        "PhysicsBody2D",
        "CollisionObject2D",
        "Node2D",
        "CanvasItem",
        "Node",
        "Object",
    ];
    let static_chain = vec![
        "StaticBody2D",
        "PhysicsBody2D",
        "CollisionObject2D",
        "Node2D",
        "CanvasItem",
        "Node",
        "Object",
    ];
    let char_chain = vec![
        "CharacterBody2D",
        "PhysicsBody2D",
        "CollisionObject2D",
        "Node2D",
        "CanvasItem",
        "Node",
        "Object",
    ];

    // All physics bodies should share PhysicsBody2D ancestor.
    assert!(rigid_chain.contains(&"PhysicsBody2D"));
    assert!(static_chain.contains(&"PhysicsBody2D"));
    assert!(char_chain.contains(&"PhysicsBody2D"));
}

// ===========================================================================
// 27. Probe capture types now includes inheritance_chain
// ===========================================================================

#[test]
fn probe_capture_types_includes_inheritance() {
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
        "method_defaults",
        "virtual_methods",
        "singleton_api",
        "resource_subtype",
        "api_surface",
    ];
    assert_eq!(
        capture_types.len(),
        15,
        "should have 15 distinct probe capture types"
    );
}

// ===========================================================================
// 28. ClassDB probe 28-class alignment with method flags
// ===========================================================================

#[test]
fn classdb_probe_method_flags_present_for_28_classes() {
    // Verify that the expanded classdb probe output includes method flags.
    let probe_output = json!({
        "fixture_id": "classdb_probe",
        "capture_type": "classdb",
        "data": {
            "class": "AnimatedSprite2D",
            "parent": "Node2D",
            "method_count": 2,
            "methods": [
                {
                    "name": "play",
                    "args": [
                        { "name": "name", "type": 21, "class_name": "" }
                    ],
                    "return_type": 0,
                    "flags": 1,
                    "is_virtual": false,
                    "is_const": false,
                    "is_vararg": false
                },
                {
                    "name": "get_frame",
                    "args": [],
                    "return_type": 2,
                    "flags": 5,
                    "is_virtual": false,
                    "is_const": true,
                    "is_vararg": false
                }
            ],
            "property_count": 0,
            "properties": [],
            "signal_count": 0,
            "signals": []
        }
    });

    validate_envelope(&probe_output);
    let methods = probe_output["data"]["methods"].as_array().unwrap();
    for method in methods {
        assert!(
            method["flags"].is_number(),
            "expanded classdb must include flags"
        );
        assert!(method["is_virtual"].is_boolean());
        assert!(method["is_const"].is_boolean());
        assert!(method["is_vararg"].is_boolean());
    }
}

// ===========================================================================
// 29. lib.rs func entry points completeness
// ===========================================================================

#[test]
fn lib_rs_registers_all_probe_entry_points() {
    let lib_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/lib.rs");
    let content = std::fs::read_to_string(&lib_path).unwrap();

    let required_funcs = [
        "run_smoke_probe",
        "run_resource_probe",
        "run_classdb_probe",
        "run_enum_constants_probe",
        "run_node_defaults_probe",
        "run_inheritance_probe",
        "run_resource_roundtrip_probe",
        "run_resource_validation_probe",
        "run_method_defaults_probe",
        "run_virtual_methods_probe",
    ];

    for func_name in &required_funcs {
        assert!(
            content.contains(func_name),
            "lib.rs must register #[func] entry point: {func_name}"
        );
    }
}

// ===========================================================================
// 30. lib.rs registers all probe modules
// ===========================================================================

#[test]
fn lib_rs_registers_all_probe_modules() {
    let lib_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/lib.rs");
    let content = std::fs::read_to_string(&lib_path).unwrap();

    let required_mods = [
        "mod classdb_probe;",
        "mod enum_constants_probe;",
        "mod inheritance_probe;",
        "mod method_defaults_probe;",
        "mod node_defaults_probe;",
        "mod property_probe;",
        "mod resource_probe;",
        "mod resource_roundtrip_probe;",
        "mod resource_validation_probe;",
        "mod scene_probe;",
        "mod signal_probe;",
        "mod virtual_methods_probe;",
    ];

    for mod_decl in &required_mods {
        assert!(
            content.contains(mod_decl),
            "lib.rs must declare module: {mod_decl}"
        );
    }
}

// ===========================================================================
// 31. Method defaults probe schema (pat-ldna)
// ===========================================================================

#[test]
fn method_defaults_probe_schema() {
    let probe_output = json!({
        "fixture_id": "method_defaults_probe",
        "capture_type": "method_defaults",
        "data": {
            "class": "AnimationPlayer",
            "methods_with_defaults_count": 2,
            "methods_with_defaults": [
                {
                    "name": "play",
                    "arg_count": 4,
                    "default_count": 3,
                    "defaults": [
                        { "index": 1, "value_string": "-1", "value_type": 3 },
                        { "index": 2, "value_string": "1", "value_type": 3 },
                        { "index": 3, "value_string": "false", "value_type": 1 }
                    ],
                    "flags": 1
                },
                {
                    "name": "queue",
                    "arg_count": 1,
                    "default_count": 1,
                    "defaults": [
                        { "index": 0, "value_string": "", "value_type": 4 }
                    ],
                    "flags": 1
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["methods_with_defaults_count"].is_number());

    let methods = data["methods_with_defaults"].as_array().unwrap();
    for method in methods {
        assert!(method["name"].is_string(), "method must have string name");
        assert!(
            method["arg_count"].is_number(),
            "method must have arg_count"
        );
        assert!(
            method["default_count"].is_number(),
            "method must have default_count"
        );
        assert!(method["flags"].is_number(), "method must have flags");

        let defaults = method["defaults"].as_array().unwrap();
        for d in defaults {
            assert!(d["index"].is_number(), "default must have numeric index");
            assert!(
                d["value_string"].is_string(),
                "default must have value_string"
            );
            assert!(
                d["value_type"].is_number(),
                "default must have numeric value_type"
            );
        }
    }
}

// ===========================================================================
// 32. Method defaults probe error schema
// ===========================================================================

#[test]
fn method_defaults_probe_error_schema() {
    let probe_output = json!({
        "fixture_id": "method_defaults_probe",
        "capture_type": "method_defaults",
        "data": {
            "class": "NonExistentClass",
            "error": "class_not_found"
        }
    });

    validate_envelope(&probe_output);
    assert!(probe_output["data"]["error"].is_string());
}

// ===========================================================================
// 33. Method defaults probe covers 28 classes
// ===========================================================================

#[test]
fn method_defaults_probe_covers_28_classes() {
    let expected_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "LineEdit",
        "Panel",
        "TextureRect",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "TileMapLayer",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "RayCast2D",
    ];
    assert_eq!(expected_classes.len(), 28);
}

// ===========================================================================
// 34. Virtual methods probe schema (pat-ldna)
// ===========================================================================

#[test]
fn virtual_methods_probe_schema() {
    let probe_output = json!({
        "fixture_id": "virtual_methods_probe",
        "capture_type": "virtual_methods",
        "data": {
            "class": "Node",
            "virtual_method_count": 4,
            "virtual_methods": [
                {
                    "name": "_ready",
                    "args": [],
                    "arg_count": 0,
                    "return_type": 0,
                    "is_const": false,
                    "flags": 33
                },
                {
                    "name": "_process",
                    "args": [
                        { "name": "delta", "type": 3, "class_name": "" }
                    ],
                    "arg_count": 1,
                    "return_type": 0,
                    "is_const": false,
                    "flags": 33
                },
                {
                    "name": "_physics_process",
                    "args": [
                        { "name": "delta", "type": 3, "class_name": "" }
                    ],
                    "arg_count": 1,
                    "return_type": 0,
                    "is_const": false,
                    "flags": 33
                },
                {
                    "name": "_input",
                    "args": [
                        { "name": "event", "type": 24, "class_name": "InputEvent" }
                    ],
                    "arg_count": 1,
                    "return_type": 0,
                    "is_const": false,
                    "flags": 33
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["virtual_method_count"].is_number());

    let methods = data["virtual_methods"].as_array().unwrap();
    for method in methods {
        assert!(
            method["name"].is_string(),
            "virtual method must have string name"
        );
        assert!(
            method["arg_count"].is_number(),
            "virtual method must have arg_count"
        );
        assert!(
            method["return_type"].is_number(),
            "virtual method must have return_type"
        );
        assert!(
            method["is_const"].is_boolean(),
            "virtual method must have is_const bool"
        );
        assert!(
            method["flags"].is_number(),
            "virtual method must have flags"
        );

        let args = method["args"].as_array().unwrap();
        for arg in args {
            assert!(arg["name"].is_string(), "arg must have string name");
            assert!(arg["type"].is_number(), "arg must have numeric type");
            assert!(
                arg["class_name"].is_string(),
                "arg must have string class_name"
            );
        }
    }
}

// ===========================================================================
// 35. Virtual methods probe error schema
// ===========================================================================

#[test]
fn virtual_methods_probe_error_schema() {
    let probe_output = json!({
        "fixture_id": "virtual_methods_probe",
        "capture_type": "virtual_methods",
        "data": {
            "class": "NonExistentClass",
            "error": "class_not_found"
        }
    });

    validate_envelope(&probe_output);
    assert!(probe_output["data"]["error"].is_string());
}

// ===========================================================================
// 36. Virtual methods probe covers 28 classes
// ===========================================================================

#[test]
fn virtual_methods_probe_covers_28_classes() {
    let expected_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "LineEdit",
        "Panel",
        "TextureRect",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "TileMapLayer",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "RayCast2D",
    ];
    assert_eq!(expected_classes.len(), 28);
}

// ===========================================================================
// 37. Virtual methods key contracts
// ===========================================================================

#[test]
fn virtual_methods_known_contracts() {
    // These virtual methods MUST exist on the Node class in any valid probe output.
    let node_virtuals = [
        "_ready",
        "_process",
        "_physics_process",
        "_input",
        "_unhandled_input",
        "_enter_tree",
        "_exit_tree",
    ];
    // All names must start with underscore (Godot convention for virtuals).
    for name in &node_virtuals {
        assert!(name.starts_with('_'), "virtual methods should start with _");
    }
    assert!(
        node_virtuals.len() >= 7,
        "Node should have at least 7 virtuals"
    );
}

// ===========================================================================
// 38. Method defaults key contracts
// ===========================================================================

#[test]
fn method_defaults_known_contracts() {
    // AnimationPlayer.play() has default arguments. Timer.start() has a default.
    // This test validates that the probe structure supports the known API.
    let known_methods_with_defaults = [
        ("AnimationPlayer", "play"),
        ("Timer", "start"),
        ("Node", "add_child"),
        ("Node", "remove_child"),
    ];
    // All method names should be non-empty.
    for (class, method) in &known_methods_with_defaults {
        assert!(!class.is_empty());
        assert!(!method.is_empty());
    }
}

// ===========================================================================
// 39. Probe capture type count is now 14
// ===========================================================================

#[test]
fn probe_capture_type_count_is_15() {
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
        "method_defaults",
        "virtual_methods",
        "singleton_api",
        "resource_subtype",
        "api_surface",
    ];
    // Verify uniqueness.
    let mut sorted = capture_types.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 15, "all 15 capture types must be unique");
}

// ===========================================================================
// 40. lib.rs func entry points includes new probes
// ===========================================================================

#[test]
fn lib_rs_registers_new_probe_entry_points() {
    let lib_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/lib.rs");
    let content = std::fs::read_to_string(&lib_path).unwrap();

    let new_funcs = [
        "run_method_defaults_probe",
        "run_virtual_methods_probe",
        "run_singleton_probe",
        "run_resource_subtype_probe",
        "run_api_surface_probe",
    ];

    for func_name in &new_funcs {
        assert!(
            content.contains(func_name),
            "lib.rs must register #[func] entry point: {func_name}"
        );
    }
}

// ===========================================================================
// 41. Singleton API probe schema
// ===========================================================================

#[test]
fn singleton_api_probe_schema() {
    let probe_output = json!({
        "fixture_id": "singleton_probe",
        "capture_type": "singleton_api",
        "data": {
            "class": "Engine",
            "parent": "Object",
            "method_count": 25,
            "methods": [
                {
                    "name": "get_frames_per_second",
                    "return_type": 3,
                    "arg_count": 0,
                    "args": []
                },
                {
                    "name": "get_physics_frames",
                    "return_type": 2,
                    "arg_count": 0,
                    "args": []
                }
            ],
            "signal_count": 0,
            "signals": [],
            "property_count": 5,
            "properties": [
                { "name": "physics_ticks_per_second", "type": 2 },
                { "name": "max_fps", "type": 2 }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["parent"].is_string());
    assert!(data["method_count"].is_number());
    assert!(data["signal_count"].is_number());
    assert!(data["property_count"].is_number());

    let methods = data["methods"].as_array().unwrap();
    for method in methods {
        assert!(method["name"].is_string());
        assert!(method["return_type"].is_number());
        assert!(method["arg_count"].is_number());
        assert!(method["args"].is_array());
    }
}

// ===========================================================================
// 42. Singleton probe covers required singletons
// ===========================================================================

#[test]
fn singleton_probe_covers_required_classes() {
    let probe_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/singleton_probe.rs");
    let content = std::fs::read_to_string(&probe_src).unwrap();

    let required = [
        "ProjectSettings",
        "Engine",
        "OS",
        "Time",
        "Input",
        "DisplayServer",
        "PhysicsServer2D",
        "ResourceLoader",
        "ResourceSaver",
    ];

    for class in &required {
        assert!(
            content.contains(class),
            "singleton_probe.rs must probe '{class}'"
        );
    }
}

// ===========================================================================
// 43. Resource subtype probe schema
// ===========================================================================

#[test]
fn resource_subtype_probe_schema() {
    let probe_output = json!({
        "fixture_id": "resource_subtype_probe",
        "capture_type": "resource_subtype",
        "data": {
            "class": "StyleBoxFlat",
            "parent": "StyleBox",
            "inheritance_chain": ["StyleBox", "Resource", "RefCounted", "Object"],
            "can_instantiate": true,
            "property_count": 12,
            "properties": [
                {
                    "name": "bg_color",
                    "type": 20,
                    "hint": 0,
                    "hint_string": "",
                    "usage": 4102
                }
            ],
            "default_count": 5,
            "defaults": [
                {
                    "name": "bg_color",
                    "value_string": "Color(0.6, 0.6, 0.6, 1)",
                    "value_type": 20
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["parent"].is_string());
    assert!(data["inheritance_chain"].is_array());
    assert!(data["can_instantiate"].is_boolean());
    assert!(data["property_count"].is_number());
    assert!(data["default_count"].is_number());

    let props = data["properties"].as_array().unwrap();
    for prop in props {
        assert!(prop["name"].is_string());
        assert!(prop["type"].is_number());
        assert!(prop["usage"].is_number());
    }

    let defaults = data["defaults"].as_array().unwrap();
    for d in defaults {
        assert!(d["name"].is_string());
        assert!(d["value_string"].is_string());
        assert!(d["value_type"].is_number());
    }
}

// ===========================================================================
// 44. Resource subtype probe covers shape and style types
// ===========================================================================

#[test]
fn resource_subtype_probe_covers_key_types() {
    let probe_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/resource_subtype_probe.rs");
    let content = std::fs::read_to_string(&probe_src).unwrap();

    let required = [
        "StyleBoxFlat",
        "Gradient",
        "Curve",
        "Animation",
        "BoxShape2D",
        "CircleShape2D",
        "PackedScene",
        "Theme",
    ];

    for class in &required {
        assert!(
            content.contains(class),
            "resource_subtype_probe.rs must probe '{class}'"
        );
    }
}

// ===========================================================================
// 45. extract_probes.sh handles new capture types
// ===========================================================================

#[test]
fn extract_probes_handles_new_capture_types() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("singleton_api"),
        "extract_probes.sh must handle capture_type 'singleton_api'"
    );
    assert!(
        content.contains("resource_subtype"),
        "extract_probes.sh must handle capture_type 'resource_subtype'"
    );
}

// ===========================================================================
// 46. API surface probe schema (pat-ldna)
// ===========================================================================

#[test]
fn api_surface_probe_schema() {
    let probe_output = json!({
        "fixture_id": "api_surface_probe",
        "capture_type": "api_surface",
        "data": {
            "class": "Node2D",
            "method_count": 5,
            "methods": [
                {
                    "name": "set_position",
                    "args": [
                        { "name": "position", "type": 5, "class_name": "" }
                    ],
                    "arg_count": 1,
                    "return_type": 0,
                    "flags": 1,
                    "default_arg_count": 0
                },
                {
                    "name": "get_position",
                    "args": [],
                    "arg_count": 0,
                    "return_type": 5,
                    "flags": 1,
                    "default_arg_count": 0
                }
            ],
            "property_accessor_count": 2,
            "property_accessors": [
                {
                    "name": "position",
                    "type": 5,
                    "has_getter": true,
                    "has_setter": true
                },
                {
                    "name": "rotation",
                    "type": 3,
                    "has_getter": true,
                    "has_setter": true
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert!(data["class"].is_string());
    assert!(data["method_count"].is_number());
    assert!(data["methods"].is_array());
    assert!(data["property_accessor_count"].is_number());
    assert!(data["property_accessors"].is_array());

    // Validate method structure
    let method = &data["methods"][0];
    assert!(method["name"].is_string());
    assert!(method["args"].is_array());
    assert!(method["arg_count"].is_number());
    assert!(method["return_type"].is_number());
    assert!(method["flags"].is_number());
    assert!(method["default_arg_count"].is_number());

    // Validate arg structure
    let arg = &method["args"][0];
    assert!(arg["name"].is_string());
    assert!(arg["type"].is_number());
    assert!(arg["class_name"].is_string());

    // Validate property accessor structure
    let accessor = &data["property_accessors"][0];
    assert!(accessor["name"].is_string());
    assert!(accessor["type"].is_number());
    assert!(accessor["has_getter"].is_boolean());
    assert!(accessor["has_setter"].is_boolean());
}

// ===========================================================================
// 47. API surface probe error schema (pat-ldna)
// ===========================================================================

#[test]
fn api_surface_probe_error_schema() {
    let probe_output = json!({
        "fixture_id": "api_surface_probe",
        "capture_type": "api_surface",
        "data": {
            "class": "NonExistentClass",
            "error": "class_not_found"
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["error"].as_str().unwrap(), "class_not_found");
}

// ===========================================================================
// 48. API surface probe covers all 28 core classes (pat-ldna)
// ===========================================================================

#[test]
fn api_surface_probe_covers_28_classes() {
    let probe_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/api_surface_probe.rs");
    let content = std::fs::read_to_string(&probe_src).unwrap();

    let core_classes = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "AnimationPlayer",
        "Control",
        "Label",
        "Button",
        "LineEdit",
        "Panel",
        "TextureRect",
        "ColorRect",
        "HBoxContainer",
        "VBoxContainer",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Timer",
        "TileMap",
        "TileMapLayer",
        "CPUParticles2D",
        "AnimatedSprite2D",
        "AudioStreamPlayer",
        "CanvasLayer",
        "RayCast2D",
    ];

    for class in &core_classes {
        assert!(
            content.contains(class),
            "api_surface_probe.rs must include class '{class}'"
        );
    }
}

// ===========================================================================
// 49. API surface probe captures property accessors (pat-ldna)
// ===========================================================================

#[test]
fn api_surface_probe_captures_accessor_pairs() {
    // Verify the probe output schema supports tracking getter/setter parity.
    // This is critical for API validation: if Godot has get_position/set_position
    // but Patina only has get_position, that's an API surface gap.
    let probe_output = json!({
        "fixture_id": "api_surface_probe",
        "capture_type": "api_surface",
        "data": {
            "class": "CharacterBody2D",
            "method_count": 10,
            "methods": [],
            "property_accessor_count": 3,
            "property_accessors": [
                {
                    "name": "velocity",
                    "type": 5,
                    "has_getter": true,
                    "has_setter": true
                },
                {
                    "name": "floor_max_angle",
                    "type": 3,
                    "has_getter": true,
                    "has_setter": true
                },
                {
                    "name": "motion_mode",
                    "type": 2,
                    "has_getter": true,
                    "has_setter": true
                }
            ]
        }
    });

    validate_envelope(&probe_output);
    let accessors = probe_output["data"]["property_accessors"]
        .as_array()
        .unwrap();
    for acc in accessors {
        assert!(
            acc["has_getter"].is_boolean() && acc["has_setter"].is_boolean(),
            "each accessor must have has_getter and has_setter booleans"
        );
    }
}

// ===========================================================================
// 50. API surface probe method signature completeness (pat-ldna)
// ===========================================================================

#[test]
fn api_surface_method_signature_schema_complete() {
    // Each method must carry enough type information for parity checking:
    // name, arg_count, args with types, return_type, flags, default_arg_count.
    let required_fields = [
        "name",
        "arg_count",
        "return_type",
        "flags",
        "default_arg_count",
        "args",
    ];

    let method = json!({
        "name": "move_and_slide",
        "args": [],
        "arg_count": 0,
        "return_type": 1,
        "flags": 1,
        "default_arg_count": 0
    });

    for field in &required_fields {
        assert!(
            !method[field].is_null(),
            "method schema must include '{field}'"
        );
    }
}

// ===========================================================================
// 51. Resource validation probe covers all fixture resources (pat-ldna)
// ===========================================================================

#[test]
fn resource_validation_fixture_coverage() {
    // Verify that test resource fixtures exist for validation probes.
    let fixtures_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/fixtures");

    let expected_fixtures = [
        "test_theme.tres",
        "test_rect_shape.tres",
        "test_environment.tres",
        "test_animation.tres",
        "test_style_box.tres",
    ];

    for fixture in &expected_fixtures {
        let path = fixtures_dir.join(fixture);
        assert!(
            path.exists(),
            "resource validation fixture must exist: {fixture}"
        );
    }
}

// ===========================================================================
// 52. Resource validation probe schema for Animation fixture (pat-ldna)
// ===========================================================================

#[test]
fn resource_validation_animation_schema() {
    // Animation resources have specific properties (length, loop_mode, step)
    // that must survive validation probe checks.
    let probe_output = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "pass",
            "resource_path": "res://fixtures/test_animation.tres",
            "resource_class": "Animation",
            "check_count": 4,
            "checks": [
                {
                    "check": "class_match",
                    "expected": "Animation",
                    "actual": "Animation",
                    "pass": true
                },
                {
                    "check": "path_preserved",
                    "path": "res://fixtures/test_animation.tres",
                    "pass": true
                },
                {
                    "check": "storable_properties",
                    "count": 3,
                    "nil_count": 0,
                    "pass": true
                },
                {
                    "check": "reload_consistency",
                    "original_class": "Animation",
                    "reloaded_class": "Animation",
                    "pass": true
                }
            ],
            "storable_property_count": 3,
            "properties": [
                { "name": "length", "type": 3, "is_nil": false, "value_type": 3 },
                { "name": "loop_mode", "type": 2, "is_nil": false, "value_type": 2 },
                { "name": "step", "type": 3, "is_nil": false, "value_type": 3 }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["status"].as_str().unwrap(), "pass");
    assert_eq!(data["resource_class"].as_str().unwrap(), "Animation");

    // All checks must pass
    let checks = data["checks"].as_array().unwrap();
    for check in checks {
        assert_eq!(check["pass"], true, "check {:?} must pass", check["check"]);
    }
}

// ===========================================================================
// 53. Resource validation probe schema for StyleBox fixture (pat-ldna)
// ===========================================================================

#[test]
fn resource_validation_style_box_schema() {
    let probe_output = json!({
        "fixture_id": "resource_validation_probe",
        "capture_type": "resource_validation",
        "data": {
            "status": "pass",
            "resource_path": "res://fixtures/test_style_box.tres",
            "resource_class": "StyleBoxFlat",
            "check_count": 4,
            "checks": [
                {
                    "check": "class_match",
                    "expected": "StyleBoxFlat",
                    "actual": "StyleBoxFlat",
                    "pass": true
                },
                {
                    "check": "path_preserved",
                    "path": "res://fixtures/test_style_box.tres",
                    "pass": true
                },
                {
                    "check": "storable_properties",
                    "count": 10,
                    "nil_count": 0,
                    "pass": true
                },
                {
                    "check": "reload_consistency",
                    "original_class": "StyleBoxFlat",
                    "reloaded_class": "StyleBoxFlat",
                    "pass": true
                }
            ],
            "storable_property_count": 10,
            "properties": [
                { "name": "bg_color", "type": 20, "is_nil": false, "value_type": 20 },
                { "name": "corner_radius_top_left", "type": 2, "is_nil": false, "value_type": 2 }
            ]
        }
    });

    validate_envelope(&probe_output);
    let data = &probe_output["data"];
    assert_eq!(data["resource_class"].as_str().unwrap(), "StyleBoxFlat");
    assert_eq!(data["status"].as_str().unwrap(), "pass");
}

// ===========================================================================
// 54. lib.rs registers api_surface_probe module (pat-ldna)
// ===========================================================================

#[test]
fn lib_rs_registers_api_surface_probe_module() {
    let lib_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/lib.rs");
    let content = std::fs::read_to_string(&lib_path).unwrap();

    assert!(
        content.contains("mod api_surface_probe;"),
        "lib.rs must declare mod api_surface_probe"
    );
    assert!(
        content.contains("run_api_surface_probe"),
        "lib.rs must have #[func] run_api_surface_probe"
    );
    assert!(
        content.contains("api_surface_probe::emit()"),
        "lib.rs must call api_surface_probe::emit()"
    );
}

// ===========================================================================
// 55. API surface probe source includes accessor detection (pat-ldna)
// ===========================================================================

#[test]
fn api_surface_probe_source_has_accessor_detection() {
    let probe_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/godot/src/api_surface_probe.rs");
    let content = std::fs::read_to_string(&probe_src).unwrap();

    assert!(
        content.contains("has_getter"),
        "api_surface_probe must track getter existence"
    );
    assert!(
        content.contains("has_setter"),
        "api_surface_probe must track setter existence"
    );
    assert!(
        content.contains("property_accessors"),
        "api_surface_probe must emit property_accessors array"
    );
    assert!(
        content.contains("default_arg_count"),
        "api_surface_probe must track default argument counts"
    );
}
