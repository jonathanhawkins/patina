//! Singleton / global API probe.
//!
//! Captures the public API surface of Godot singleton classes that Patina
//! needs for runtime parity: ProjectSettings, Engine, OS, Time, Input,
//! DisplayServer. These are global entry points that game code calls
//! frequently and whose method signatures must match upstream exactly.

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

/// Singleton classes whose API surface we probe.
const SINGLETON_CLASSES: &[&str] = &[
    "ProjectSettings",
    "Engine",
    "OS",
    "Time",
    "Input",
    "DisplayServer",
    "RenderingServer",
    "PhysicsServer2D",
    "NavigationServer2D",
    "ResourceLoader",
    "ResourceSaver",
];

/// Emit API surface for each singleton class via ClassDB.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in SINGLETON_CLASSES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "singleton_probe",
                    capture_type: "singleton_api",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        // Collect methods
        let method_list = db.class_get_method_list(&sn);
        let mut methods = Vec::new();

        for i in 0..method_list.len() {
            let Some(dict) = method_list.get(i) else {
                continue;
            };
            let name: String = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            let return_type: i32 = dict
                .get("return")
                .and_then(|r: Variant| {
                    r.try_to::<Dictionary>()
                        .ok()
                        .and_then(|d| d.get("type").map(|v: Variant| v.to::<i32>()))
                })
                .unwrap_or(-1);

            // Collect argument info
            let args_variant = dict.get("args");
            let mut args = Vec::new();
            if let Some(args_array) = args_variant {
                if let Ok(arr) = args_array.try_to::<Array<Dictionary>>() {
                    for j in 0..arr.len() {
                        if let Some(arg_dict) = arr.get(j) {
                            let arg_name = arg_dict
                                .get("name")
                                .map(|v: Variant| v.to_string())
                                .unwrap_or_default();
                            let arg_type = arg_dict
                                .get("type")
                                .map(|v: Variant| v.to::<i32>())
                                .unwrap_or(-1);
                            args.push(json!({
                                "name": arg_name,
                                "type": arg_type,
                            }));
                        }
                    }
                }
            }

            methods.push(json!({
                "name": name,
                "return_type": return_type,
                "arg_count": args.len(),
                "args": args,
            }));
        }

        // Collect signals
        let signal_list = db.class_get_signal_list(&sn);
        let mut signals = Vec::new();

        for i in 0..signal_list.len() {
            let Some(dict) = signal_list.get(i) else {
                continue;
            };
            let name: String = dict
                .get("name")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            signals.push(json!({ "name": name }));
        }

        // Collect properties
        let prop_list = db.class_get_property_list(&sn);
        let mut properties = Vec::new();

        for i in 0..prop_list.len() {
            let Some(dict) = prop_list.get(i) else {
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
            properties.push(json!({
                "name": name,
                "type": prop_type,
            }));
        }

        let parent = db.get_parent_class(&sn).to_string();

        let data = json!({
            "class": class_name,
            "parent": parent,
            "method_count": methods.len(),
            "methods": methods,
            "signal_count": signals.len(),
            "signals": signals,
            "property_count": properties.len(),
            "properties": properties,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "singleton_probe",
            capture_type: "singleton_api",
            data,
        };

        godot_print!(
            "PATINA_PROBE:{}",
            serde_json::to_string(&envelope).unwrap()
        );
    }
}
