//! Inheritance chain probe.
//!
//! For each core Godot class, captures the full parent chain from the class
//! up to `Object`. This feeds Patina's class hierarchy validation — ensuring
//! that `is_class()`, `get_parent_class()`, and method/signal inheritance
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

/// Classes whose inheritance chains we probe — same 28-class set.
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

/// Emit the full inheritance chain for each core class.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in CORE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "inheritance_probe",
                    capture_type: "inheritance_chain",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        // Walk parent chain up to the root.
        let mut chain = vec![class_name.to_string()];
        let mut current = sn.clone();
        loop {
            let parent = db.get_parent_class(&current);
            let parent_str = parent.to_string();
            if parent_str.is_empty() {
                break;
            }
            chain.push(parent_str.clone());
            current = StringName::from(parent_str.as_str());
        }

        let depth = chain.len();

        let data = json!({
            "class": class_name,
            "chain": chain,
            "depth": depth,
            "is_node": chain.contains(&"Node".to_string()),
            "is_canvasitem": chain.contains(&"CanvasItem".to_string()),
            "is_node2d": chain.contains(&"Node2D".to_string()),
            "is_control": chain.contains(&"Control".to_string()),
        });

        let envelope = ProbeEnvelope {
            fixture_id: "inheritance_probe",
            capture_type: "inheritance_chain",
            data,
        };

        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
    }
}
