//! API surface probe.
//!
//! For each core Godot class, captures the public API surface: method
//! signatures (name, arguments with types, return type, flags) and property
//! getter/setter pairs.  This feeds Patina's API parity checks — ensuring
//! that engine-rs ClassDB entries expose the same callable surface as
//! upstream Godot.
//!
//! Unlike classdb_probe (which captures full metadata), this probe focuses
//! on the *callable* surface: methods that game code would invoke, with
//! enough type detail to validate argument/return type parity.

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

/// Classes whose API surface we probe — same 28-class core set.
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

/// Emit the callable API surface for each core class.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in CORE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "api_surface_probe",
                    capture_type: "api_surface",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        // Collect methods with full signatures
        let method_list = db.class_get_method_list(&sn);
        let mut methods = Vec::new();

        for i in 0..method_list.len() {
            let Some(method_dict) = method_list.get(i) else {
                continue;
            };

            let method_name: String = method_dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();

            // Skip internal/private methods
            if method_name.starts_with('_') && method_name != "_ready"
                && method_name != "_process"
                && method_name != "_physics_process"
                && method_name != "_input"
                && method_name != "_unhandled_input"
                && method_name != "_enter_tree"
                && method_name != "_exit_tree"
                && method_name != "_draw"
            {
                continue;
            }

            let return_info = method_dict.get("return");
            let return_type: i32 = return_info
                .as_ref()
                .and_then(|v: &Variant| {
                    v.try_to::<Dictionary>()
                        .ok()
                        .and_then(|d| d.get("type").map(|t: Variant| t.to::<i32>()))
                })
                .unwrap_or(0);

            let flags: i32 = method_dict
                .get("flags")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);

            let args_variant = method_dict.get("args");
            let mut args = Vec::new();

            if let Some(args_v) = args_variant {
                if let Ok(args_arr) = args_v.try_to::<Array<Dictionary>>() {
                    for j in 0..args_arr.len() {
                        let Some(arg) = args_arr.get(j) else {
                            continue;
                        };
                        let arg_name: String = arg
                            .get("name")
                            .map(|v: Variant| v.to_string())
                            .unwrap_or_default();
                        let arg_type: i32 = arg
                            .get("type")
                            .map(|v: Variant| v.to::<i32>())
                            .unwrap_or(-1);
                        let arg_class: String = arg
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

            let default_args_variant = method_dict.get("default_args");
            let default_arg_count: usize = default_args_variant
                .and_then(|v: Variant| v.try_to::<Array<Variant>>().ok())
                .map(|a| a.len())
                .unwrap_or(0);

            methods.push(json!({
                "name": method_name,
                "args": args,
                "arg_count": args.len(),
                "return_type": return_type,
                "flags": flags,
                "default_arg_count": default_arg_count,
            }));
        }

        // Collect property getter/setter pairs for API surface validation
        let prop_list = db.class_get_property_list(&sn);
        let mut property_accessors = Vec::new();

        for i in 0..prop_list.len() {
            let Some(prop_dict) = prop_list.get(i) else {
                continue;
            };
            let prop_name: String = prop_dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            let prop_type: i32 = prop_dict
                .get("type")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(-1);
            let usage: i32 = prop_dict
                .get("usage")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);

            // Only include properties with PROPERTY_USAGE_EDITOR or PROPERTY_USAGE_STORAGE
            if usage & (2 | 4096) != 0 && !prop_name.starts_with('_') {
                // Check if getter/setter methods exist
                let getter = format!("get_{}", prop_name);
                let setter = format!("set_{}", prop_name);
                let has_getter = db.class_has_method(&sn, &StringName::from(getter.as_str()));
                let has_setter = db.class_has_method(&sn, &StringName::from(setter.as_str()));

                property_accessors.push(json!({
                    "name": prop_name,
                    "type": prop_type,
                    "has_getter": has_getter,
                    "has_setter": has_setter,
                }));
            }
        }

        let data = json!({
            "class": class_name,
            "method_count": methods.len(),
            "methods": methods,
            "property_accessor_count": property_accessors.len(),
            "property_accessors": property_accessors,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "api_surface_probe",
            capture_type: "api_surface",
            data,
        };

        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
    }
}
