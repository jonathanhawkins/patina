use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

pub(crate) fn emit(node: &Gd<Node>, probe_label: &GString, probe_count: i32) {
    let data = json!({
        "node_name": node.get_name().to_string(),
        "node_class": node.get_class().to_string(),
        "properties": {
            "probe_label": {
                "type": "String",
                "value": probe_label.to_string(),
            },
            "probe_count": {
                "type": "Int",
                "value": probe_count,
            }
        }
    });

    let envelope = ProbeEnvelope {
        fixture_id: "smoke_probe",
        capture_type: "properties",
        data,
    };

    godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
}
