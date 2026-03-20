use godot::classes::ResourceLoader;
use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

/// Load a resource by path and emit its metadata as machine-readable JSON.
pub(crate) fn emit_for_path(resource_path: &str) {
    let mut loader = ResourceLoader::singleton();
    let res = loader.load(resource_path);

    let Some(resource) = res else {
        let envelope = ProbeEnvelope {
            fixture_id: "resource_probe",
            capture_type: "resource_metadata",
            data: json!({
                "resource_path": resource_path,
                "error": "failed_to_load",
            }),
        };
        godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
        return;
    };

    let resource_class = resource.get_class().to_string();

    // Enumerate all properties on the resource
    let property_list = resource.get_property_list();
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

        properties.push(json!({
            "name": name,
            "type": prop_type,
            "hint": hint,
            "hint_string": hint_string,
            "usage": usage,
        }));
    }

    // Collect subresource references
    let subresources = collect_subresources(&resource, &property_list);

    let data = json!({
        "resource_class": resource_class,
        "resource_path": resource_path,
        "resource_name": resource.get_name().to_string(),
        "property_count": properties.len(),
        "properties": properties,
        "subresource_count": subresources.len(),
        "subresources": subresources,
    });

    let envelope = ProbeEnvelope {
        fixture_id: "resource_probe",
        capture_type: "resource_metadata",
        data,
    };

    godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
}

/// Walk the resource's properties and collect references to sub-resources.
fn collect_subresources(
    resource: &Gd<godot::classes::Resource>,
    property_list: &Array<Dictionary>,
) -> Vec<Value> {
    let mut subs = Vec::new();

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

        // Type 24 = OBJECT in Godot's Variant.Type
        if prop_type == 24 {
            let val = resource.get(name.as_str());
            if !val.is_nil() {
                if let Ok(sub_res) = val.try_to::<Gd<godot::classes::Resource>>() {
                    subs.push(json!({
                        "property": name,
                        "class": sub_res.get_class().to_string(),
                        "path": sub_res.get_path().to_string(),
                    }));
                }
            }
        }
    }

    subs
}
