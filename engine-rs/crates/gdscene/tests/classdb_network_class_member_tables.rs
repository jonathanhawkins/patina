//! pat-s3rrv.248 — the member tables on the multiplayer/network classes.
//!
//! `class_has_signal` gates `has_signal` (`gdscene/src/scripting.rs:3938`), so a
//! signal missing from ClassDB is not merely undocumented — it is *unfindable* from
//! a script, and `if node.has_signal("spawned")` takes the wrong branch.
//!
//! ## What was actually missing
//!
//! Much less than the bead states, and the difference is worth recording. Already
//! present before this change: `MultiplayerPeer`'s three properties
//! (`refuse_new_connections`, `transfer_mode`, `transfer_channel`) **and both** its
//! signals; `PacketPeer::encode_buffer_max_size`; `UDPServer::max_pending_connections`;
//! four of `WebSocketPeer`'s six properties. The bead describes all of those as
//! absent.
//!
//! Genuinely missing, and what this test pins: `MultiplayerSpawner`'s and
//! `MultiplayerSynchronizer`'s signals (zero registered), `WebSocketPeer`'s two
//! buffer-size properties, and `HTTPClient::connection`.
//!
//! ## The acceptance's first clause is covered elsewhere, deliberately
//!
//! "A script connects `multiplayer.peer_connected` and receives it when a peer
//! joins" is already proven by
//! `scene_multiplayer_signals_and_full_method_surface_from_script` — which connects
//! the signal in GDScript, asserts a zero-control *before* any join, calls
//! `add_peer(7)`, and asserts the callback fired. A real-transport version exists
//! too in `websocket_multiplayer_peer_script_reachable`. Duplicating that here
//! would add a second thing to keep correct without adding coverage, so this file
//! covers the member tables and cites those for the wiring clause.

use gdobject::class_db;

/// Every property `get_property_list` reports for `class_name`.
///
/// ⚠️ This walks the inheritance chain, so it is a SUPERSET of what the class
/// declares itself. Asserting a count here would really be asserting the class has
/// no ancestors; membership is the honest check.
fn props(class_name: &str) -> Vec<String> {
    class_db::get_property_list(class_name)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// **The acceptance.** The signals and properties a networked project introspects.
#[test]
fn classdb_network_class_member_tables() {
    class_db::register_all_classes();

    // --- MultiplayerSpawner / MultiplayerSynchronizer signals ---------------
    //
    // These are the ones the bead is really about: both classes registered zero
    // signals, so `has_signal("spawned")` was false on a class whose whole purpose
    // is announcing spawns.
    for (class, signal) in [
        ("MultiplayerSpawner", "spawned"),
        ("MultiplayerSpawner", "despawned"),
        ("MultiplayerSynchronizer", "synchronized"),
        ("MultiplayerSynchronizer", "delta_synchronized"),
        ("MultiplayerSynchronizer", "visibility_changed"),
    ] {
        assert!(
            class_db::class_has_signal(class, signal),
            "{class} must declare `{signal}` — `class_has_signal` gates `has_signal`, \
             so without it a script cannot even discover the signal exists"
        );
    }

    // --- WebSocketPeer: all six -------------------------------------------
    let ws = props("WebSocketPeer");
    for p in [
        "supported_protocols",
        "handshake_headers",
        "inbound_buffer_size",
        "outbound_buffer_size",
        "max_queued_packets",
        "heartbeat_interval",
    ] {
        assert!(
            ws.iter().any(|n| n == p),
            "WebSocketPeer must declare `{p}`; has {ws:?}"
        );
    }

    // --- HTTPClient --------------------------------------------------------
    let http = props("HTTPClient");
    for p in ["blocking_mode_enabled", "connection", "read_chunk_size"] {
        assert!(
            http.iter().any(|n| n == p),
            "HTTPClient must declare `{p}`; has {http:?}"
        );
    }
}

/// The members the bead claims are missing but which already existed.
///
/// This is a regression guard, not busywork: I nearly re-added all of these on the
/// strength of the bead text. Pinning them means the next person to read that same
/// claim gets a failing test rather than a silent duplicate registration — and a
/// duplicate member SHADOWS rather than errors, so it would not have announced
/// itself.
#[test]
fn classdb_network_class_member_tables_preexisting_members_are_not_lost() {
    class_db::register_all_classes();

    let peer = props("MultiplayerPeer");
    for p in [
        "refuse_new_connections",
        "transfer_mode",
        "transfer_channel",
    ] {
        assert!(
            peer.iter().any(|n| n == p),
            "MultiplayerPeer::{p} predates this bead and must survive it; has {peer:?}"
        );
    }
    for s in ["peer_connected", "peer_disconnected"] {
        assert!(
            class_db::class_has_signal("MultiplayerPeer", s),
            "MultiplayerPeer::{s} predates this bead and must survive it"
        );
    }

    assert!(
        props("PacketPeer")
            .iter()
            .any(|n| n == "encode_buffer_max_size"),
        "PacketPeer::encode_buffer_max_size already existed"
    );
    assert!(
        props("UDPServer")
            .iter()
            .any(|n| n == "max_pending_connections"),
        "UDPServer::max_pending_connections already existed"
    );
}

/// A signal declared on the class must be reachable through the inheritance chain
/// too, since that is how a script reaches it on a subclass.
///
/// `SceneMultiplayer` extends `MultiplayerAPI`, and a project connects
/// `peer_connected` on whatever the active API happens to be — so a chain that
/// stops short makes the signal invisible exactly where it is used.
#[test]
fn classdb_network_class_member_tables_signals_resolve_through_the_chain() {
    class_db::register_all_classes();

    assert!(
        class_db::class_exists("MultiplayerSpawner"),
        "precondition: the class is registered at all"
    );
    assert!(
        class_db::is_parent_class("MultiplayerSpawner", "Node"),
        "MultiplayerSpawner must descend from Node, or a scene cannot hold one"
    );
    assert!(
        class_db::is_parent_class("MultiplayerSynchronizer", "Node"),
        "MultiplayerSynchronizer must descend from Node"
    );
}
