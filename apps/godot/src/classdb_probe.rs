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

/// The 17 core classes that Patina's ClassDB parity tests cover.
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
    "RigidBody2D",
    "StaticBody2D",
    "CharacterBody2D",
    "Area2D",
    "CollisionShape2D",
    "Timer",
    "TileMap",
    "CPUParticles2D",
];

/// Emit ClassDB metadata for each core class.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in CORE_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "classdb_probe",
                    capture_type: "classdb",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        let parent = db
            .get_parent_class(&sn)
            .to_string();

        // --- Methods ---
        let method_list = db.class_get_method_list(&sn);
        let mut methods = Vec::new();
        for i in 0..method_list.len() {
            let Some(dict) = method_list.get(i) else {
                continue;
            };
            let method_name = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();

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

            methods.push(json!({
                "name": method_name,
                "args": args,
                "return_type": return_type,
            }));
        }

        // --- Properties ---
        let prop_list = db.class_get_property_list(&sn);
        let mut properties = Vec::new();
        for i in 0..prop_list.len() {
            let Some(dict) = prop_list.get(i) else {
                continue;
            };
            let prop_name = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            let prop_type = dict
                .get("type")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(-1);
            let hint = dict
                .get("hint")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);
            let hint_string = dict
                .get("hint_string")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            let usage = dict
                .get("usage")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);

            properties.push(json!({
                "name": prop_name,
                "type": prop_type,
                "hint": hint,
                "hint_string": hint_string,
                "usage": usage,
            }));
        }

        // --- Signals ---
        let signal_list = db.class_get_signal_list(&sn);
        let mut signals = Vec::new();
        for i in 0..signal_list.len() {
            let Some(dict) = signal_list.get(i) else {
                continue;
            };
            let sig_name = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();

            let mut sig_args = Vec::new();
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
                        sig_args.push(json!({
                            "name": arg_name,
                            "type": arg_type,
                        }));
                    }
                }
            }

            signals.push(json!({
                "name": sig_name,
                "args": sig_args,
            }));
        }

        let data = json!({
            "class": class_name,
            "parent": parent,
            "method_count": methods.len(),
            "methods": methods,
            "property_count": properties.len(),
            "properties": properties,
            "signal_count": signals.len(),
            "signals": signals,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "classdb_probe",
            capture_type: "classdb",
            data,
        };

        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
    }
}
