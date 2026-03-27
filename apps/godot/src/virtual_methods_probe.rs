//! Virtual methods probe.
//!
//! For each core Godot class, enumerates the virtual methods available for
//! override (_ready, _process, _input, etc.). This feeds Patina's runtime
//! parity — ensuring the engine knows which callbacks to dispatch and their
//! exact signatures.
//!
//! Virtual methods are critical because they define the contract between the
//! engine and user scripts. Missing or mismatched virtual methods break
//! gameplay logic silently.

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

/// Emit virtual method signatures for each core class.
///
/// For every virtual method (METHOD_FLAG_VIRTUAL = 32), captures:
/// - method name
/// - argument signatures (name, type, class_name)
/// - return type
/// - whether it is const
/// - the declaring class (to distinguish inherited vs own virtuals)
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in CORE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "virtual_methods_probe",
                    capture_type: "virtual_methods",
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
        let mut virtual_methods = Vec::new();

        for i in 0..method_list.len() {
            let Some(dict) = method_list.get(i) else {
                continue;
            };

            let flags: i32 = dict
                .get("flags")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);

            // METHOD_FLAG_VIRTUAL = 32
            if flags & 32 == 0 {
                continue;
            }

            let method_name = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();

            let is_const = flags & 4 != 0;

            // Extract arguments.
            let mut args = Vec::new();
            if let Some(args_var) = dict.get("args") {
                if let Ok(args_arr) = args_var.try_to::<Array<Dictionary>>() {
                    for j in 0..args_arr.len() {
                        let Some(arg_d) = args_arr.get(j) else {
                            continue;
                        };
                        let arg_name = arg_d
                            .get("name")
                            .map(|v: Variant| v.to_string())
                            .unwrap_or_default();
                        let arg_type = arg_d
                            .get("type")
                            .map(|v: Variant| v.to::<i32>())
                            .unwrap_or(-1);
                        let arg_class = arg_d
                            .get("class_name")
                            .map(|v: Variant| v.to_string())
                            .unwrap_or_default();
                        args.push(json!({
                            "name": arg_name,
                            "type": arg_type,
                            "class_name": arg_class,
                        }));
                    }
                }
            }

            let return_type = dict
                .get("return")
                .and_then(|v: Variant| v.try_to::<Dictionary>().ok())
                .and_then(|d| d.get("type").map(|v: Variant| v.to::<i32>()))
                .unwrap_or(-1);

            virtual_methods.push(json!({
                "name": method_name,
                "args": args,
                "arg_count": args.len(),
                "return_type": return_type,
                "is_const": is_const,
                "flags": flags,
            }));
        }

        let data = json!({
            "class": class_name,
            "virtual_method_count": virtual_methods.len(),
            "virtual_methods": virtual_methods,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "virtual_methods_probe",
            capture_type: "virtual_methods",
            data,
        };

        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
    }
}
