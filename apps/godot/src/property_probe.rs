use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

/// Emit a full property list snapshot for the given node using `get_property_list()`.
pub(crate) fn emit(node: &Gd<Node>, _probe_label: &GString, _probe_count: i32) {
    let property_list = node.get_property_list();
    let mut properties = Vec::new();

    for i in 0..property_list.len() {
        let Some(dict) = property_list.get(i) else {
            continue;
        };
        let name = dict
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
        let class_name = dict
            .get("class_name")
            .map(|v: Variant| v.to_string())
            .unwrap_or_default();

        properties.push(json!({
            "name": name,
            "type": prop_type,
            "hint": hint,
            "hint_string": hint_string,
            "usage": usage,
            "class_name": class_name,
        }));
    }

    let data = json!({
        "node_name": node.get_name().to_string(),
        "node_class": node.get_class().to_string(),
        "property_count": properties.len(),
        "properties": properties,
    });

    let envelope = ProbeEnvelope {
        fixture_id: "smoke_probe",
        capture_type: "properties",
        data,
    };

    godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
}
