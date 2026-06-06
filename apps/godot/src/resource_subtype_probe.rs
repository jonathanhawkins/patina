//! Resource subtype probe.
//!
//! Enumerates all Resource subclasses known to ClassDB and captures their
//! default property values. This feeds Patina's resource loader parity —
//! ensuring that every Resource subclass we encounter in .tres/.tscn files
//! has its property schema correctly mapped.

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

/// Resource subtypes commonly encountered in Godot scenes and resources.
/// These are the types that Patina's resource loader must understand.
const RESOURCE_SUBTYPES: &[&str] = &[
    "StyleBoxFlat",
    "StyleBoxEmpty",
    "StyleBoxLine",
    "StyleBoxTexture",
    "Theme",
    "Gradient",
    "GradientTexture1D",
    "GradientTexture2D",
    "Curve",
    "Curve2D",
    "Curve3D",
    "Animation",
    "AnimationLibrary",
    "SpriteFrames",
    "TileSet",
    "ShaderMaterial",
    "CanvasItemMaterial",
    "Environment",
    "CameraAttributes",
    "AudioBusLayout",
    "Font",
    "FontFile",
    "LabelSettings",
    "PackedScene",
    "BoxShape2D",
    "CircleShape2D",
    "CapsuleShape2D",
    "RectangleShape2D",
    "WorldBoundaryShape2D",
    "SegmentShape2D",
    "ConvexPolygonShape2D",
    "ConcavePolygonShape2D",
];

/// Emit resource subtype property schemas.
pub(crate) fn emit() {
    let db = ClassDb::singleton();

    for &class_name in RESOURCE_SUBTYPES {
        let sn = StringName::from(class_name);

        if !db.class_exists(&sn) {
            godot_print!(
                "PATINA_PROBE:{}",
                serde_json::to_string(&ProbeEnvelope {
                    fixture_id: "resource_subtype_probe",
                    capture_type: "resource_subtype",
                    data: json!({
                        "class": class_name,
                        "error": "class_not_found",
                    }),
                })
                .unwrap()
            );
            continue;
        }

        let can_instantiate = db.can_instantiate(&sn);

        // Collect property schema from ClassDB (no instantiation needed)
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
            let hint: i32 = dict
                .get("hint")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);
            let hint_string: String = dict
                .get("hint_string")
                .map(|v: Variant| v.to_string())
                .unwrap_or_default();
            let usage: i32 = dict
                .get("usage")
                .map(|v: Variant| v.to::<i32>())
                .unwrap_or(0);

            properties.push(json!({
                "name": name,
                "type": prop_type,
                "hint": hint,
                "hint_string": hint_string,
                "usage": usage,
            }));
        }

        // Collect default values if instantiable
        let mut defaults = Vec::new();
        if can_instantiate {
            let instance: Variant = db.instantiate(&sn);
            if !instance.is_nil() {
                if let Ok(res) = instance.try_to::<Gd<godot::classes::Resource>>() {
                    for i in 0..prop_list.len() {
                        let Some(dict) = prop_list.get(i) else {
                            continue;
                        };
                        let name: String = dict
                            .get("name")
                            .map(|v: Variant| v.to_string())
                            .unwrap_or_default();
                        let usage: i32 = dict
                            .get("usage")
                            .map(|v: Variant| v.to::<i32>())
                            .unwrap_or(0);

                        // Only storable properties
                        if usage & 2 == 0 {
                            continue;
                        }

                        let value = res.get(name.as_str());
                        defaults.push(json!({
                            "name": name,
                            "value_string": format!("{}", value),
                            "value_type": value.get_type().ord(),
                        }));
                    }
                }
            }
        }

        let parent = db.get_parent_class(&sn).to_string();
        let inheritance = collect_inheritance_chain(&db, &sn);

        let data = json!({
            "class": class_name,
            "parent": parent,
            "inheritance_chain": inheritance,
            "can_instantiate": can_instantiate,
            "property_count": properties.len(),
            "properties": properties,
            "default_count": defaults.len(),
            "defaults": defaults,
        });

        let envelope = ProbeEnvelope {
            fixture_id: "resource_subtype_probe",
            capture_type: "resource_subtype",
            data,
        };

        godot_print!(
            "PATINA_PROBE:{}",
            serde_json::to_string(&envelope).unwrap()
        );
    }
}

/// Walk the class hierarchy to build an inheritance chain.
fn collect_inheritance_chain(db: &Gd<ClassDb>, class: &StringName) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = db.get_parent_class(class).to_string();
    while !current.is_empty() {
        chain.push(current.clone());
        current = db
            .get_parent_class(&StringName::from(current.as_str()))
            .to_string();
    }
    chain
}
