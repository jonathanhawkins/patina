//! Resource roundtrip probe.
//!
//! Loads a resource, saves it to a temp path via ResourceSaver, reloads it,
//! and compares property values between the original and reloaded copies.
//! Emits machine-readable JSON showing which properties survive the roundtrip.

use godot::classes::{DirAccess, ResourceLoader, ResourceSaver};
use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

/// Run a roundtrip test on a resource: load → save → reload → compare.
///
/// For each storable property, captures the original and reloaded value
/// strings and whether they match. Emits a structured JSON envelope.
pub(crate) fn emit_roundtrip(resource_path: &str, expected_class: Option<&str>) {
    let mut loader = ResourceLoader::singleton();
    let res = loader.load(resource_path);

    let Some(original) = res else {
        emit_result(json!({
            "status": "error",
            "error": "failed_to_load",
            "resource_path": resource_path,
        }));
        return;
    };

    let actual_class = original.get_class().to_string();

    // Class check
    let class_ok = expected_class.map_or(true, |e| actual_class == e);
    if !class_ok {
        emit_result(json!({
            "status": "error",
            "error": "class_mismatch",
            "resource_path": resource_path,
            "expected_class": expected_class.unwrap_or(""),
            "actual_class": actual_class,
        }));
        return;
    }

    // Capture original storable properties
    let property_list = original.get_property_list();
    let mut original_props: Vec<(String, i32, i32, String, i32)> = Vec::new();

    for i in 0..property_list.len() {
        let Some(dict) = property_list.get(i) else {
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
        let usage: i32 = dict
            .get("usage")
            .map(|v: Variant| v.to::<i32>())
            .unwrap_or(0);

        // PROPERTY_USAGE_STORAGE = 2
        if usage & 2 != 0 {
            let value = original.get(name.as_str());
            let value_str = format!("{}", value);
            let value_type = value.get_type().ord();
            original_props.push((name, prop_type, usage, value_str, value_type));
        }
    }

    // Save to a temp path
    let temp_path = format!("user://roundtrip_probe_{}.tres", resource_path.replace('/', "_").replace(':', ""));
    let mut saver = ResourceSaver::singleton();
    let save_err = saver.save_ex(&original).path(&temp_path).done();

    if save_err != godot::global::Error::OK {
        emit_result(json!({
            "status": "error",
            "error": "save_failed",
            "resource_path": resource_path,
            "temp_path": temp_path,
            "error_code": format!("{:?}", save_err),
        }));
        return;
    }

    // Reload from temp path
    let reloaded_res = loader.load(&temp_path);
    let Some(reloaded) = reloaded_res else {
        emit_result(json!({
            "status": "error",
            "error": "reload_failed",
            "resource_path": resource_path,
            "temp_path": temp_path,
        }));
        return;
    };

    let reloaded_class = reloaded.get_class().to_string();

    // Compare properties
    let mut comparisons = Vec::new();
    let mut match_count = 0u32;
    let mut mismatch_count = 0u32;

    for (name, prop_type, usage, orig_str, orig_vtype) in &original_props {
        let reloaded_value = reloaded.get(name.as_str());
        let reloaded_str = format!("{}", reloaded_value);
        let reloaded_vtype = reloaded_value.get_type().ord();

        let values_match = orig_str == &reloaded_str;
        let types_match = orig_vtype == &reloaded_vtype;
        let roundtrip_ok = values_match && types_match;

        if roundtrip_ok {
            match_count += 1;
        } else {
            mismatch_count += 1;
        }

        comparisons.push(json!({
            "name": name,
            "type": prop_type,
            "usage": usage,
            "original_value": orig_str,
            "original_value_type": orig_vtype,
            "reloaded_value": reloaded_str,
            "reloaded_value_type": reloaded_vtype,
            "values_match": values_match,
            "types_match": types_match,
            "roundtrip_ok": roundtrip_ok,
        }));
    }

    let all_ok = mismatch_count == 0 && actual_class == reloaded_class;

    // Clean up temp file
    if let Some(mut dir) = DirAccess::open("user://") {
        let filename = temp_path.strip_prefix("user://").unwrap_or(&temp_path);
        let _ = dir.remove(filename);
    }

    emit_result(json!({
        "status": if all_ok { "pass" } else { "fail" },
        "resource_path": resource_path,
        "resource_class": actual_class,
        "reloaded_class": reloaded_class,
        "class_preserved": actual_class == reloaded_class,
        "property_count": comparisons.len(),
        "match_count": match_count,
        "mismatch_count": mismatch_count,
        "properties": comparisons,
    }));
}

fn emit_result(data: Value) {
    let envelope = ProbeEnvelope {
        fixture_id: "resource_roundtrip_probe",
        capture_type: "resource_roundtrip",
        data,
    };
    godot_print!(
        "PATINA_PROBE:{}",
        serde_json::to_string(&envelope).unwrap()
    );
}
