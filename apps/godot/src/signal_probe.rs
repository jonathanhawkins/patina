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

pub(crate) fn emit(probe: &mut PatinaSmokeProbe, node: &mut Gd<Node>) {
    probe.push_signal_event("before_connect");
    let callable = node.callable("record_probe_signal");
    let _ = node.connect("probe_signal", &callable);
    probe.push_signal_event("after_connect");
    probe
        .base_mut()
        .emit_signal("probe_signal", &[Variant::from("emitted")]);
    probe.push_signal_event("after_emit");

    let data = json!({
        "node_name": node.get_name().to_string(),
        "events": probe.signal_events(),
    });

    let envelope = ProbeEnvelope {
        fixture_id: "smoke_probe",
        capture_type: "signals",
        data,
    };

    godot_print!("PATINA_PROBE:{}", serde_json::to_string(&envelope).unwrap());
}
