//! pat-s3rrv.79 — `WebSocketPeer`'s connection accessors are implemented AND
//! declared, in the same change.
//!
//! The registration block for this class carries an explicit rule: register
//! what the engine can actually do, in the change that makes it able. Claiming
//! a capability that fails on call is the same lie as denying one that works.
//! Four methods moved from the second category to the first — they are now
//! implemented in `gdplatform/src/websocket_peer.rs` — and two properties turned
//! out to have been in the *first* category all along.
//!
//! ⚠️ `handshake_headers` and `supported_protocols` were recorded as
//! unimplemented. Both have gdplatform accessors AND interpreter dispatch arms;
//! only the ClassDB declaration was missing, so `class_get_property_list`
//! omitted two settings a script could already set. That is the inverse of the
//! usual failure here, and it is why this test asserts the properties by name
//! rather than by count.
//!
//! ⚠️ A property is registered only in the change that implements it: a
//! declared-but-unbacked property is exactly the inert state this codebase
//! keeps producing. All six are implemented now — `heartbeat_interval` and
//! `max_queued_packets` in pat-s3rrv.79, the two buffer sizes after.
//!
//! Surface from `upstream/godot/modules/websocket/doc_classes/WebSocketPeer.xml`.
//! Module classes keep their docs beside the module, not in `doc/classes/`.

use gdobject::class_db::{get_class_info, register_networking_classes};
use gdvariant::Variant;

fn info(class: &str) -> gdobject::class_db::ClassInfo {
    register_networking_classes();
    get_class_info(class).unwrap_or_else(|| panic!("{class} is not registered"))
}

fn method_arity(class: &str, method: &str) -> usize {
    info(class)
        .methods
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

fn has_property(class: &str, property: &str) -> bool {
    info(class).properties.iter().any(|p| p.name == property)
}

/// The four connection accessors are declared, with Godot's arities.
#[test]
fn the_connection_accessors_are_registered() {
    assert_eq!(method_arity("WebSocketPeer", "get_connected_host"), 0);
    assert_eq!(method_arity("WebSocketPeer", "get_connected_port"), 0);
    assert_eq!(
        method_arity("WebSocketPeer", "get_current_outbound_buffered_amount"),
        0,
    );
    // ⚠️ Takes ONE argument. A zero-arity registration would let
    // `class_get_method_list` describe it as a getter, which it is not — it is
    // the only setter of the four.
    assert_eq!(method_arity("WebSocketPeer", "set_no_delay"), 1);
}

/// The two implemented properties are declared, with Godot's defaults.
#[test]
fn the_two_implemented_properties_are_registered() {
    for name in ["handshake_headers", "supported_protocols"] {
        assert!(
            has_property("WebSocketPeer", name),
            "`{name}` is implemented in gdplatform and dispatched to scripts; \
             leaving it out of ClassDB makes class_get_property_list omit a \
             setting a script can already set",
        );
        let default = info("WebSocketPeer")
            .properties
            .iter()
            .rev()
            .find(|p| p.name == name)
            .unwrap()
            .default_value
            .clone();
        assert_eq!(
            default,
            Variant::PackedStringArray(Vec::new()),
            "Godot's default for `{name}` is an empty PackedStringArray",
        );
    }
}

/// The two properties implemented in the same bead, with Godot's defaults.
///
/// ⚠️ `heartbeat_interval` defaults to `0.0` — DISABLED. A non-zero default
/// would start pinging every connection in every project, and, because the
/// heartbeat is a liveness timeout, would start dropping connections whose
/// servers do not answer pings.
#[test]
fn the_heartbeat_and_queue_cap_are_registered_with_godots_defaults() {
    assert_eq!(
        info("WebSocketPeer")
            .properties
            .iter()
            .rev()
            .find(|p| p.name == "heartbeat_interval")
            .expect("heartbeat_interval must be registered once implemented")
            .default_value,
        Variant::Float(0.0),
        "0.0 means disabled; anything else pings every connection by default",
    );
    assert_eq!(
        info("WebSocketPeer")
            .properties
            .iter()
            .rev()
            .find(|p| p.name == "max_queued_packets")
            .expect("max_queued_packets must be registered once implemented")
            .default_value,
        Variant::Int(4096),
        "Godot's documented default",
    );
}

/// The two buffer sizes are registered NOW, in the change that implemented
/// them in gdplatform (`set_inbound_buffer_size` / `set_outbound_buffer_size`,
/// behaviour-tested there): inbound = max message length (1009 past it) plus
/// the inbound queue's byte budget, outbound = per-send bound. They used to be
/// asserted absent because nothing backed them; registering them without that
/// was the inert-property state this test guards against.
#[test]
fn the_buffer_sizes_are_registered_with_accessors() {
    for name in ["inbound_buffer_size", "outbound_buffer_size"] {
        let class = info("WebSocketPeer");
        let prop = class
            .properties
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("`{name}` must be registered"));
        assert_eq!(prop.default_value, Variant::Int(65535), "{name} default");
        assert_eq!(prop.setter, format!("set_{name}"), "{name} setter");
        assert_eq!(prop.getter, format!("get_{name}"), "{name} getter");
    }
}
