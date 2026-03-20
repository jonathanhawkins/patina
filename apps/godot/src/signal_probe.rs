use godot::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

use crate::PatinaSmokeProbe;

#[derive(Serialize)]
struct ProbeEnvelope {
    fixture_id: &'static str,
    capture_type: &'static str,
    data: Value,
}

/// Emit signal connection/emit ordering AND a full signal list enumeration.
pub(crate) fn emit(probe: &mut PatinaSmokeProbe, node: &mut Gd<Node>) {
    // --- Part 1: signal ordering test (existing behavior) ---
    probe.push_signal_event("before_connect");
    let callable = node.callable("record_probe_signal");
    let _ = node.connect("probe_signal", &callable);
    probe.push_signal_event("after_connect");
    probe
        .base_mut()
        .emit_signal("probe_signal", &[Variant::from("emitted")]);
    probe.push_signal_event("after_emit");

    // --- Part 2: enumerate all signals via get_signal_list() ---
    let signal_list = node.get_signal_list();
    let mut signals = Vec::new();

    for i in 0..signal_list.len() {
        let Some(dict) = signal_list.get(i) else {
            continue;
        };
        let name = dict
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
                    args.push(json!({
                        "name": arg_name,
                        "type": arg_type,
                    }));
                }
            }
        }

        signals.push(json!({
            "name": name,
            "args": args,
        }));
    }

    let data = json!({
        "node_name": node.get_name().to_string(),
        "node_class": node.get_class().to_string(),
        "ordering_events": probe.signal_events(),
        "signal_count": signals.len(),
        "signals": signals,
    });

    let envelope = ProbeEnvelope {
        fixture_id: "smoke_probe",
        capture_type: "signals",
        data,
    };

    godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
}
