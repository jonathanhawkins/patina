use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

pub(crate) fn emit(node: &Gd<Node>) {
    let data = json!({
        "root": visit_node(node),
    });

    let envelope = ProbeEnvelope {
        fixture_id: "smoke_probe",
        capture_type: "scene_tree",
        data,
    };

    godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
}

fn visit_node(node: &Gd<Node>) -> Value {
    let mut children = Vec::new();
    let child_count = node.get_child_count();
    for index in 0..child_count {
        if let Some(child) = node.get_child(index) {
            children.push(visit_node(&child));
        }
    }

    json!({
        "name": node.get_name().to_string(),
        "class": node.get_class().to_string(),
        "path": node.get_path().to_string(),
        "children": children,
    })
}
