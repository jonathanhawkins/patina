//! pat-0q2kx.184 — native socket/DTLS **class surfaces** on the web target.
//!
//! A browser cannot open a listening TCP/UDP socket or a DTLS association, so
//! the *transports* are genuinely out of scope — Godot's own web export omits
//! them. But `docs/taxonomy.md:413-416` is explicit that this does **not** make
//! the classes disappear: "on web cite OUT_OF_SCOPE per web limitations, BUT the
//! CLASS surface should exist with correct errors."
//!
//! All seven were absent from ClassDB while `gdeditor`'s generated inspector
//! schema already listed them, so the editor and ClassDB disagreed about whether
//! they exist. A script calling `ClassDB.class_exists("TCPServer")` got `false`,
//! which is a different (and wrong) answer from "exists but cannot listen here".
//!
//! ⚠️ **Surface only.** No transport is implemented and none can be on this
//! target. These tests assert the classes resolve and their inheritance chains
//! reach the right roots — deliberately *not* that anything connects.

use gdobject::class_db::{get_class_info, register_networking_classes};
use gdobject::{class_exists, is_parent_class};

/// Godot's parents for each surface, matching `inspector_schema_generated.rs`.
const SURFACES: &[(&str, &str)] = &[
    ("SocketServer", "RefCounted"),
    ("TCPServer", "SocketServer"),
    ("UDPServer", "RefCounted"),
    ("DTLSServer", "RefCounted"),
    ("PacketPeerDTLS", "PacketPeer"),
    ("StreamPeerSocket", "StreamPeer"),
    ("StreamPeerUDS", "StreamPeerSocket"),
    // pat-s3rrv.238 — the family's last omission. `SocketServer` had a
    // registered subclass and a registered peer type with no server to hand it
    // out.
    ("UDSServer", "SocketServer"),
];

#[test]
fn native_socket_classes_exist_with_godots_parents() {
    register_networking_classes();

    for (class, parent) in SURFACES {
        assert!(
            class_exists(class),
            "{class} must resolve — the transport is out of scope on web, the class is not"
        );
        assert!(
            is_parent_class(class, parent),
            "{class} must derive {parent}, matching the generated inspector schema"
        );
    }
}

/// Every surface must reach `Object` through its chain. `TCPServer` is the case
/// that matters: it hangs off `SocketServer`, an intermediate that was **also**
/// missing, and registering the leaf without the intermediate would sever the
/// chain while `class_exists` still answered true.
#[test]
fn every_surface_reaches_object_through_its_chain() {
    register_networking_classes();

    for (class, _) in SURFACES {
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object — a missing intermediate severs the chain \
             silently, and class_exists alone would not notice"
        );
    }

    // The specific two-hop chain that needed the intermediate.
    assert!(is_parent_class("TCPServer", "SocketServer"));
    assert!(is_parent_class("TCPServer", "RefCounted"));
    // And the StreamPeer branch, likewise two hops.
    assert!(is_parent_class("StreamPeerUDS", "StreamPeerSocket"));
    assert!(is_parent_class("StreamPeerUDS", "StreamPeer"));
}

/// The surfaces declare **no methods**. Declaring `listen`/`accept` here would
/// advertise an implementation that cannot exist on this target — the honest
/// surface is the class and its chain, nothing more.
#[test]
fn surfaces_declare_no_transport_methods() {
    register_networking_classes();

    for class in [
        "TCPServer",
        "UDPServer",
        "DTLSServer",
        "StreamPeerUDS",
        "UDSServer",
    ] {
        let info = get_class_info(class).unwrap_or_else(|| panic!("{class} registered"));
        assert!(
            info.methods.is_empty(),
            "{class} must not advertise transport methods it cannot implement on web, got {:?}",
            info.methods.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
    }
}
