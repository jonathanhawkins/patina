//! Method default argument values probe.
//!
//! For each core Godot class, captures method signatures including default
//! argument values. This feeds Patina's API parity checks — ensuring that
//! method calls with fewer arguments than the full signature use the correct
//! default values from upstream Godot.
//!
//! This probe captures data that `classdb_probe.rs` does not: specifically
//! the *default values* for optional method arguments.

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

/// Same 28 core classes as classdb_probe.
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

/// Emit method default argument values for each core class.
///
/// For every method with default arguments, captures:
/// - method name
/// - total argument count
/// - default argument count
/// - each default value as a string representation with its Variant type
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in CORE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "method_defaults_probe",
                    capture_type: "method_defaults",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        let method_list = db.class_get_method_list(&sn);
        let mut methods_with_defaults = Vec::new();

        for i in 0..method_list.len() {
            let Some(dict) = method_list.get(i) else {
                continue;
            };

            let method_name = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();

            // Extract argument count.
            let mut arg_count = 0usize;
            if let Some(args_var) = dict.get("args") {
                if let Ok(args_arr) = args_var.try_to::<Array<Dictionary>>() {
                    arg_count = args_arr.len();
                }
            }

            // Extract default arguments.
            let mut defaults = Vec::new();
            if let Some(defaults_var) = dict.get("default_args") {
                if let Ok(defaults_arr) = defaults_var.try_to::<Array<Variant>>() {
                    for j in 0..defaults_arr.len() {
                        let Some(default_val) = defaults_arr.get(j) else {
                            continue;
                        };
                        let val_type = default_val.get_type().ord();
                        let val_str = format!("{}", default_val);

                        defaults.push(json!({
                            "index": arg_count - defaults_arr.len() + j,
                            "value_string": val_str,
                            "value_type": val_type,
                        }));
                    }
                }
            }

            // Only emit methods that have at least one default argument.
            if !defaults.is_empty() {
                let flags: i32 = dict
                    .get("flags")
                    .map(|v: Variant| v.to::<i32>())
                    .unwrap_or(0);

                methods_with_defaults.push(json!({
                    "name": method_name,
                    "arg_count": arg_count,
                    "default_count": defaults.len(),
                    "defaults": defaults,
                    "flags": flags,
                }));
            }
        }

        let data = json!({
            "class": class_name,
            "methods_with_defaults_count": methods_with_defaults.len(),
            "methods_with_defaults": methods_with_defaults,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "method_defaults_probe",
            capture_type: "method_defaults",
            data,
        };

        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
    }
}
