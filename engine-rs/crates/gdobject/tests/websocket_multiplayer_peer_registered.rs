//! pat-s3rrv.81: `WebSocketMultiplayerPeer` is a real class in ClassDB.
//!
//! The transport was never the gap. `gdplatform::websocket_multiplayer` speaks
//! the full opening handshake, `multiplayer_peer_handles` hands the peer to the
//! tree, the interpreter answers `WebSocketMultiplayerPeer.new()`, and
//! `multiplayer.multiplayer_peer = p` installs it — all of that is green in
//! `gdplatform/tests/websocket_multiplayer_e2e_test.rs` and
//! `gdscene/tests/multiplayer_peer_assigned_from_gdscript.rs`.
//!
//! What was missing is narrow and total: `grep '"WebSocketMultiplayerPeer"'`
//! over `class_db.rs` returned **0**, while `WebSocketPeer`, `MultiplayerPeer`
//! and `ENetMultiplayerPeer` were all registered. So
//! `ClassDB.class_exists("WebSocketMultiplayerPeer")` answered **false** for a
//! class you could already construct and use — and everything that reads the
//! registry rather than the interpreter (`class_get_method_list`, the
//! inspector, an editor class list) could not see it at all.
//!
//! Surface transcribed from `upstream/godot/modules/websocket/doc_classes/`.
//! Module classes keep their docs beside the module, NOT in `doc/classes/`;
//! looking only in the usual place makes it seem there is no oracle.

use gdobject::class_db::{
    class_exists, get_class_info, is_abstract, is_parent_class, register_networking_classes,
};
use gdvariant::Variant;

fn method_arity(class: &str, method: &str) -> usize {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is not registered"))
        .methods
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

fn property_default(class: &str, property: &str) -> Variant {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is not registered"))
        .properties
        .iter()
        .rev()
        .find(|p| p.name == property)
        .unwrap_or_else(|| panic!("{class} has no property `{property}`"))
        .default_value
        .clone()
}

/// **The acceptance clause.** The class exists, on the parent the bead names.
#[test]
fn websocket_multiplayer_peer_registered() {
    register_networking_classes();

    assert!(
        class_exists("WebSocketMultiplayerPeer"),
        "the class a multiplayer project opens with must be in ClassDB"
    );
    assert_eq!(
        get_class_info("WebSocketMultiplayerPeer")
            .expect("registered")
            .parent_class,
        "MultiplayerPeer",
        "doc XML says inherits=\"MultiplayerPeer\""
    );
}

/// The chain must reach the root. An unregistered intermediate severs it
/// silently, and a peer that is not an `Object` cannot be handed to
/// `multiplayer.multiplayer_peer`.
#[test]
fn its_ancestry_reaches_object() {
    register_networking_classes();

    for ancestor in ["MultiplayerPeer", "PacketPeer", "RefCounted", "Object"] {
        assert!(
            is_parent_class("WebSocketMultiplayerPeer", ancestor),
            "WebSocketMultiplayerPeer must BE a {ancestor}"
        );
    }
}

/// ⚠️ The asymmetry that matters: the base is abstract, this class is not.
/// `MultiplayerPeer.new()` is not a thing; `WebSocketMultiplayerPeer.new()` is
/// the first line of every multiplayer script, and a class marked abstract
/// would be rejected by a registry-driven constructor.
#[test]
fn it_is_instantiable_though_its_base_is_not() {
    register_networking_classes();

    assert!(is_abstract("MultiplayerPeer"), "the base stays abstract");
    assert!(
        !is_abstract("WebSocketMultiplayerPeer"),
        "the concrete transport must be constructible"
    );
}

/// The five methods the class declares ITSELF, with the doc XML's arities.
#[test]
fn it_declares_its_own_methods() {
    register_networking_classes();

    assert_eq!(method_arity("WebSocketMultiplayerPeer", "create_client"), 1);
    assert_eq!(method_arity("WebSocketMultiplayerPeer", "create_server"), 1);
    assert_eq!(method_arity("WebSocketMultiplayerPeer", "get_peer"), 1);
    assert_eq!(
        method_arity("WebSocketMultiplayerPeer", "get_peer_address"),
        1
    );
    assert_eq!(method_arity("WebSocketMultiplayerPeer", "get_peer_port"), 1);
}

/// ⚠️ `poll`, `close`, `get_unique_id` and `get_connection_status` are NOT
/// redeclared here — they are inherited. This test pins both halves of that
/// claim, because getting it wrong is invisible either way: redeclaring makes
/// `class_get_method_list` report a method twice, and the base failing to
/// declare them would leave a hole nothing else would notice.
#[test]
fn inherited_methods_are_not_redeclared() {
    register_networking_classes();

    // Bind the owned ClassInfo first: `get_class_info` returns a VALUE, so
    // borrowing `.methods` straight off the call is E0716 (the temporary dies
    // at the end of the statement).
    let info = get_class_info("WebSocketMultiplayerPeer").expect("registered");
    let own = &info.methods;
    for inherited in ["poll", "close", "get_unique_id", "get_connection_status"] {
        assert!(
            !own.iter().any(|m| m.name == inherited),
            "`{inherited}` is inherited from MultiplayerPeer; declaring it here \
             double-reports it in class_get_method_list"
        );
        // ...and the base really does supply it, so the omission is inheritance
        // rather than an outright gap.
        assert_eq!(
            get_class_info("MultiplayerPeer")
                .expect("base registered")
                .methods
                .iter()
                .filter(|m| m.name == inherited)
                .count(),
            1,
            "MultiplayerPeer must declare `{inherited}` exactly once"
        );
    }
}

/// The members carry Godot's documented defaults. The default doubles as the
/// property's TYPE, so a wrong Variant here misreports the type as well as the
/// value — `handshake_timeout` is a float, not an int.
#[test]
fn its_members_carry_the_documented_defaults() {
    register_networking_classes();

    let p = |n: &str| property_default("WebSocketMultiplayerPeer", n);
    assert_eq!(
        p("handshake_headers"),
        Variant::PackedStringArray(Vec::new())
    );
    assert_eq!(p("handshake_timeout"), Variant::Float(3.0));
    assert_eq!(p("inbound_buffer_size"), Variant::Int(65535));
    assert_eq!(p("max_queued_packets"), Variant::Int(4096));
    assert_eq!(p("outbound_buffer_size"), Variant::Int(65535));
    assert_eq!(
        p("supported_protocols"),
        Variant::PackedStringArray(Vec::new())
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["WebSocketMultiplayerPeer", "MultiplayerPeer"]
            .iter()
            .map(|c| get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_networking_classes();
    let first = ids();
    register_networking_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — the `if !class_exists` \
         guard is missing if it does"
    );
}

// ---------------------------------------------------------------------------
// pat-s3rrv.79 — WebSocketPeer's own reflection
// ---------------------------------------------------------------------------

/// 🔑 Two methods a script can call TODAY were invisible to ClassDB.
///
/// `get_requested_url` and `get_selected_protocol` are implemented in
/// `gdplatform/src/websocket_peer.rs` (:680, :440) and already dispatched by the
/// interpreter (:21373, :21381) — but `WebSocketPeer`'s registration listed
/// neither, so `ClassDB.class_get_method_list` and `class_has_method` denied a
/// method that works. That is the same shape as the emitted-but-undeclared
/// signals in pat-s3rrv.57: reflection contradicting the engine, which is worse
/// than a missing feature because every assertion except the behaviour agrees.
///
/// ⚠️ This test deliberately does NOT assert the absence of the remaining
/// documented members (`get_connected_host`, `get_connected_port`,
/// `get_current_outbound_buffered_amount`, `set_no_delay`, and the six
/// properties). Nothing implements those yet, so they are correctly unregistered
/// today — but pinning their absence would turn this into a tripwire for whoever
/// implements them properly, which is the opposite of helpful.
#[test]
fn websocket_peer_reflects_the_methods_it_actually_dispatches() {
    register_networking_classes();

    // `method_arity` panics with the class and method name if the entry is
    // missing, which is the assertion — a zero arity is also the correct answer
    // for both, so checking the value alone could not tell "registered with 0
    // args" from "not registered".
    assert_eq!(
        method_arity("WebSocketPeer", "get_requested_url"),
        0,
        "get_requested_url takes no arguments (doc_classes/WebSocketPeer.xml:108)"
    );
    assert_eq!(
        method_arity("WebSocketPeer", "get_selected_protocol"),
        0,
        "get_selected_protocol takes no arguments (doc_classes/WebSocketPeer.xml:114)"
    );

    // The pre-existing surface must survive the addition — `register()` merges
    // by member name, and a duplicate registration that dropped entries would
    // show up here rather than at runtime.
    for method in ["connect_to_url", "send", "send_text", "poll", "close"] {
        let _ = method_arity("WebSocketPeer", method);
    }
    assert!(
        class_exists("WebSocketPeer"),
        "WebSocketPeer must still be registered"
    );
}
