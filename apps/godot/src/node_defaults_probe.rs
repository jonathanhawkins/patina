//! Node default property values probe.
//!
//! For each core Godot class, instantiates a fresh node and captures every
//! default property value with its Variant type. This feeds the Patina
//! engine's `class_defaults.rs` registry and validates that default values
//! match upstream Godot 4.6.1 behavior.

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

/// Classes whose default property values we probe.
/// Covers the core Patina parity surface: scene nodes, physics bodies,
/// UI controls, and utility nodes.
const DEFAULT_PROBE_CLASSES: &[&str] = &[
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
    // --- Newly added classes for broader parity ---
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

/// Emit default property values for each class.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in DEFAULT_PROBE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) || !db.can_instantiate(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "node_defaults_probe",
                    capture_type: "node_defaults",
                    data: json!({
                        "class": class_name,
                        "error": "cannot_instantiate",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        // Instantiate the class to read its defaults.
        let instance: Variant = db.instantiate(&sn);
        if instance.is_nil() {
            continue;
        }

        let Ok(node) = instance.try_to::<Gd<Node>>() else {
            // Not a Node subclass — skip.
            continue;
        };

        let property_list: Array<Dictionary> = node.get_property_list();
        let mut defaults = Vec::new();

        for i in 0..property_list.len() {
            let Some(dict) = property_list.get(i) else {
                continue;
            };
            let name: String = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            let prop_type: i32 = dict
                .get("type")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(-1);
            let usage: i32 = dict
                .get("usage")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);

            // Skip internal/editor-only properties (usage bit 2 = PROPERTY_USAGE_EDITOR)
            // We focus on PROPERTY_USAGE_STORAGE (bit 1 = 2) and PROPERTY_USAGE_SCRIPT_VARIABLE.
            if usage & 2 == 0 && usage & 8192 == 0 {
                continue;
            }

            let value = node.get(name.as_str());
            let value_str = format!("{}", value);

            defaults.push(json!({
                "name": name,
                "type": prop_type,
                "usage": usage,
                "value_string": value_str,
                "value_type": value.get_type().ord(),
            }));
        }

        let parent = db.get_parent_class(&sn).to_string();

        let data = json!({
            "class": class_name,
            "parent": parent,
            "default_count": defaults.len(),
            "defaults": defaults,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "node_defaults_probe",
            capture_type: "node_defaults",
            data,
        };

        godot_print!(
            "PATINA_PROBE:{}",
            serde_json::to_string(&envelope).unwrap()
        );

        // Free the temporary node.
        node.clone().upcast::<Object>().free();
    }
}
