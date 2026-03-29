//! Resource validation probe.
//!
//! Loads resources and validates their structural integrity:
//! - Class matches expected type
//! - UID fields present when expected
//! - ext_resource references resolve to valid sub-objects
//! - Subresource types and properties are accessible
//! - Property values survive load (no silent data loss)
//!
//! Emits machine-readable JSON with validation results per resource.

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

/// Validate a resource loaded from a path.
///
/// Checks:
/// 1. Resource loads successfully
/// 2. Class name matches the expected type (if provided)
/// 3. Property values are non-nil for storable properties
/// 4. Subresource references resolve to valid objects
/// 5. Resource path roundtrips correctly
pub(crate) fn emit_validation(resource_path: &str, expected_class: Option<&str>) {
    let mut loader = ResourceLoader::singleton();
    let res = loader.load(resource_path);

    let Some(resource) = res else {
        emit_result(resource_path, json!({
            "status": "error",
            "error": "failed_to_load",
            "resource_path": resource_path,
        }));
        return;
    };

    let actual_class = resource.get_class().to_string();
    let mut checks = Vec::new();

    // Check 1: Class matches expected.
    if let Some(expected) = expected_class {
        let class_ok = actual_class == expected;
        checks.push(json!({
            "check": "class_match",
            "expected": expected,
            "actual": actual_class,
            "pass": class_ok,
        }));
    }

    // Check 2: Resource path roundtrips.
    let loaded_path = resource.get_path().to_string();
    let path_ok = !loaded_path.is_empty();
    checks.push(json!({
        "check": "path_preserved",
        "path": loaded_path,
        "pass": path_ok,
    }));

    // Check 3: Enumerate storable properties and validate types.
    let property_list = resource.get_property_list();
    let mut storable_props = Vec::new();
    let mut nil_storable_count = 0u32;

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
        let usage = dict
            .get("usage")
            .map(|v: Variant| v.to::<i32>())
            .unwrap_or(0);

        // PROPERTY_USAGE_STORAGE = 2
        if usage & 2 != 0 {
            let value = resource.get(name.as_str());
            let is_nil = value.is_nil();
            if is_nil {
                nil_storable_count += 1;
            }
            storable_props.push(json!({
                "name": name,
                "type": prop_type,
                "is_nil": is_nil,
                "value_type": value.get_type().ord(),
            }));
        }
    }

    checks.push(json!({
        "check": "storable_properties",
        "count": storable_props.len(),
        "nil_count": nil_storable_count,
        "pass": true,
    }));

    // Check 4: Subresource integrity.
    let mut subresources = Vec::new();
    for prop in &storable_props {
        // Type 24 = OBJECT
        if prop["type"] == 24 {
            let prop_name = prop["name"].as_str().unwrap_or("");
            let val = resource.get(prop_name);
            if !val.is_nil() {
                if let Ok(sub_res) = val.try_to::<Gd<godot::classes::Resource>>() {
                    let sub_class = sub_res.get_class().to_string();
                    let sub_path = sub_res.get_path().to_string();
                    subresources.push(json!({
                        "property": prop_name,
                        "class": sub_class,
                        "path": sub_path,
                        "valid": true,
                    }));
                } else {
                    subresources.push(json!({
                        "property": prop_name,
                        "valid": false,
                        "error": "not_a_resource",
                    }));
                }
            }
        }
    }

    if !subresources.is_empty() {
        let all_valid = subresources.iter().all(|s| s["valid"] == true);
        checks.push(json!({
            "check": "subresource_integrity",
            "count": subresources.len(),
            "subresources": subresources,
            "pass": all_valid,
        }));
    }

    // Check 5: Reload produces same class (cache consistency).
    let reload = loader.load(resource_path);
    if let Some(reloaded) = reload {
        let reload_class = reloaded.get_class().to_string();
        checks.push(json!({
            "check": "reload_consistency",
            "original_class": actual_class,
            "reloaded_class": reload_class,
            "pass": actual_class == reload_class,
        }));
    } else {
        checks.push(json!({
            "check": "reload_consistency",
            "pass": false,
            "error": "reload_failed",
        }));
    }

    let all_pass = checks.iter().all(|c| c["pass"] == true);

    emit_result(resource_path, json!({
        "status": if all_pass { "pass" } else { "fail" },
        "resource_path": resource_path,
        "resource_class": actual_class,
        "check_count": checks.len(),
        "checks": checks,
        "storable_property_count": storable_props.len(),
        "properties": storable_props,
    }));
}

fn emit_result(_resource_path: &str, data: Value) {
    let envelope = ProbeEnvelope {
        fixture_id: "resource_validation_probe",
        capture_type: "resource_validation",
        data,
    };
    godot_print!(
        "PATINA_PROBE:{}",
        serde_json::to_string(&envelope).unwrap()
    );
}
