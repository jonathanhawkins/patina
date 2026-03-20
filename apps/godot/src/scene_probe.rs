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

    let owner = node
        .get_owner()
        .map(|o| o.get_path().to_string())
        .unwrap_or_default();

    let script_path = {
        let script_var = node.get_script();
        if script_var.is_nil() {
            String::new()
        } else if let Ok(script) = script_var.try_to::<Gd<godot::classes::Script>>() {
            script.get_path().to_string()
        } else {
            String::new()
        }
    };

    let process_mode = node.get_process_mode().ord();
    let unique_name = node.is_unique_name_in_owner();

    json!({
        "name": node.get_name().to_string(),
        "class": node.get_class().to_string(),
        "path": node.get_path().to_string(),
        "owner": owner,
        "script_path": script_path,
        "process_mode": process_mode,
        "unique_name_in_owner": unique_name,
        "children": children,
    })
}
