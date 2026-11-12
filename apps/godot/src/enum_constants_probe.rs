//! Enum/integer constants probe.
//!
//! For each core Godot class, captures the integer constants (enums) exposed
//! via ClassDB. These feed Patina's API parity checks — ensuring that enum
//! values like `Node.PROCESS_MODE_PAUSABLE` or `Control.PRESET_FULL_RECT`
//! match upstream Godot exactly.

use godot::classes::ClassDb;
use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

/// Classes whose integer constants we probe — same set as classdb_probe.
const CORE_CLASSES: &[&str] = &[
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

/// Emit integer constants for each core class.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in CORE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "enum_constants_probe",
                    capture_type: "enum_constants",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        // Collect integer constants
        let const_list = db.class_get_integer_constant_list(&sn);
        let mut constants = Vec::new();

        for i in 0..const_list.len() {
            let const_name = const_list.get(i).unwrap_or_default().to_string();
            let value = db.class_get_integer_constant(&sn, &StringName::from(const_name.as_str()));

            // Try to find which enum this constant belongs to
            let enum_name = db
                .class_get_integer_constant_enum(&sn, &StringName::from(const_name.as_str()));
            let enum_str = enum_name.to_string();

            constants.push(json!({
                "name": const_name,
                "value": value,
                "enum": if enum_str.is_empty() { Value::Null } else { json!(enum_str) },
            }));
        }

        // Collect enum names
        let enum_list = db.class_get_enum_list(&sn);
        let mut enums = Vec::new();

        for i in 0..enum_list.len() {
            let enum_name = enum_list.get(i).unwrap_or_default().to_string();
            let enum_sn = StringName::from(enum_name.as_str());
            let enum_constants = db.class_get_enum_constants(&sn, &enum_sn);
            let mut members = Vec::new();

            for j in 0..enum_constants.len() {
                members.push(enum_constants.get(j).unwrap_or_default().to_string());
            }

            enums.push(json!({
                "name": enum_name,
                "members": members,
            }));
        }

        let data = json!({
            "class": class_name,
            "constant_count": constants.len(),
            "constants": constants,
            "enum_count": enums.len(),
            "enums": enums,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "enum_constants_probe",
            capture_type: "enum_constants",
            data,
        };

        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
    }
}
