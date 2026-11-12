//! pat-s3rrv.80 — the GDExtension peer base classes are in ClassDB.
//!
//! ⚠️ A NEW FILE ON PURPOSE. Appending to an existing test binary adopts every
//! other test in it as this work's gate, and nextest cancels on the first
//! failure — which is how two beads got parked earlier today on reds I did not
//! write.
//!
//! 🔑 WHY THESE TWO AND NOT THE OTHER TWELVE unregistered networking classes on
//! this bead: `StreamPeerExtension` and `PacketPeerExtension` are declared in
//! Godot with NOTHING but virtual methods and zero members — they exist so a
//! GDExtension can implement a StreamPeer/PacketPeer. Registering them is
//! therefore the WHOLE port; there is no behaviour to be missing, because Godot
//! has none either. That is the opposite of `WebRTCPeerConnection`, where
//! registering without an implementation would advertise a class that raises.

use gdobject::class_db::{class_exists, get_class_info, inheritance_chain, register_all_classes};

fn method_arity(class: &str, method: &str) -> usize {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .methods
        .iter()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} must declare `{method}`"))
        .argument_count
}

fn is_virtual(class: &str, method: &str) -> bool {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .methods
        .iter()
        .find(|m| m.name == method)
        .map(|m| m.is_virtual)
        .unwrap_or(false)
}

/// Both classes exist and their chains resolve all the way to `Object`.
///
/// ⚠️ The chain is the point, not just `class_exists`. An unregistered base
/// silently severs it — the class would answer `true` while
/// `inheritance_chain` stopped short — so the parents (`StreamPeer`,
/// `PacketPeer`) being registered is what makes these two meaningful.
#[test]
fn the_extension_bases_resolve_to_object() {
    register_all_classes();

    for (class, parent) in [
        ("StreamPeerExtension", "StreamPeer"),
        ("PacketPeerExtension", "PacketPeer"),
    ] {
        assert!(class_exists(class), "{class} must be registered");
        let chain = inheritance_chain(class);
        assert_eq!(
            chain.get(1).map(String::as_str),
            Some(parent),
            "{class} must inherit {parent}; got {chain:?}"
        );
        assert_eq!(
            chain.last().map(String::as_str),
            Some("Object"),
            "{class}'s chain must terminate at Object — a stop short of it means \
             a base along the way is unregistered; got {chain:?}"
        );
    }
}

/// Every declared method is VIRTUAL, with Godot's arity.
///
/// ⚠️ `is_virtual` is the load-bearing flag. These classes have no
/// implementation and are not meant to: a method registered as concrete would
/// tell the editor's override-virtual dialog, and anything else reading
/// `class_get_method_list`, that there is something to call rather than
/// something to implement.
#[test]
fn every_declared_method_is_virtual_with_godots_arity() {
    register_all_classes();

    for (class, method, arity) in [
        ("StreamPeerExtension", "_get_available_bytes", 0),
        ("StreamPeerExtension", "_get_data", 3),
        ("StreamPeerExtension", "_get_partial_data", 3),
        ("StreamPeerExtension", "_put_data", 3),
        ("StreamPeerExtension", "_put_partial_data", 3),
        ("PacketPeerExtension", "_get_available_packet_count", 0),
        ("PacketPeerExtension", "_get_max_packet_size", 0),
        ("PacketPeerExtension", "_get_packet", 2),
        ("PacketPeerExtension", "_put_packet", 2),
    ] {
        assert_eq!(
            method_arity(class, method),
            arity,
            "{class}.{method} takes {arity} argument(s) per its doc XML"
        );
        assert!(
            is_virtual(class, method),
            "{class}.{method} must be marked virtual — it is `qualifiers=\"virtual\"` \
             upstream and has no implementation to call"
        );
    }
}

/// ⚠️ The WebRTC Extension pair must stay OUT until their parents land.
///
/// `WebRTCPeerConnectionExtension` and `WebRTCDataChannelExtension` inherit from
/// `WebRTCPeerConnection` / `WebRTCDataChannel`, neither of which is registered.
/// Registering the children first would make `class_exists` answer true while
/// their chain stopped short of `Object` — a class that reflects as present and
/// resolves as broken. This pins that decision so a later sweep does not add
/// them ahead of their bases.
#[test]
fn the_webrtc_extension_bases_wait_for_their_parents() {
    register_all_classes();

    for child in [
        "WebRTCPeerConnectionExtension",
        "WebRTCDataChannelExtension",
    ] {
        if class_exists(child) {
            let chain = inheritance_chain(child);
            assert_eq!(
                chain.last().map(String::as_str),
                Some("Object"),
                "{child} was registered, so its parent must be too — a chain that \
                 stops short means the base is still missing; got {chain:?}"
            );
        }
    }
}
