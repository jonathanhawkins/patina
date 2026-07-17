//! pat-nzgdu.199 — the `MultiplayerAPIExtension` / `MultiplayerPeerExtension`
//! class surface.
//!
//! Covers the bead's "register both as ClassDB classes" clause. Its **wiring
//! requirement** — a GDScript *extending* MultiplayerPeerExtension being accepted
//! as `multiplayer.multiplayer_peer` and having its virtuals polled — is NOT
//! done, and is blocked on a missing layer rather than on effort. See the bead
//! notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_networking_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The bead's clause: both registered, on the right bases.
#[test]
fn multiplayer_extension_bases_test() {
    register_networking_classes();

    assert!(class_exists("MultiplayerPeerExtension"));
    assert!(class_exists("MultiplayerAPIExtension"));

    // ⚠️ The Extension is the CHILD of the base it extends, not a sibling. That
    // is what lets a subclass be handed straight to `multiplayer_peer`: it IS-A
    // MultiplayerPeer.
    assert!(
        is_parent_class("MultiplayerPeerExtension", "MultiplayerPeer"),
        "a custom peer must BE a MultiplayerPeer to be assignable"
    );
    assert!(is_parent_class("MultiplayerAPIExtension", "MultiplayerAPI"));

    // The chains must reach the root — an unregistered intermediate severs them
    // silently (pat-nzgdu.98). PacketPeer is the one in the middle here.
    assert!(is_parent_class("MultiplayerPeerExtension", "PacketPeer"));
    assert!(is_parent_class("MultiplayerPeerExtension", "RefCounted"));
    assert!(is_parent_class("MultiplayerPeerExtension", "Object"));
    assert!(is_parent_class("MultiplayerAPIExtension", "RefCounted"));
    assert!(is_parent_class("MultiplayerAPIExtension", "Object"));
}

/// ⚠️ The bases are abstract; the Extensions are NOT. That asymmetry is the
/// whole point — you cannot construct a bare MultiplayerPeer, but you *can*
/// construct the thing you subclass.
#[test]
fn the_extensions_are_instantiable_but_their_bases_are_not() {
    register_networking_classes();

    assert!(
        is_abstract("MultiplayerPeer"),
        "the base transport is abstract in Godot"
    );
    assert!(!can_instantiate("MultiplayerPeer"));

    for class in ["MultiplayerPeerExtension", "MultiplayerAPIExtension"] {
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ BOTH are PURE VIRTUAL SURFACES — zero non-virtual methods between them.
///
/// That inverts how a registration usually reads: nothing here is callable *on*
/// the class; every entry is a hook the engine calls *into* a subclass. A
/// non-virtual method appearing here would be invented.
#[test]
fn both_are_pure_virtual_surfaces() {
    register_networking_classes();

    for (class, base) in [
        ("MultiplayerPeerExtension", "MultiplayerPeer"),
        ("MultiplayerAPIExtension", "MultiplayerAPI"),
    ] {
        let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
        let own: Vec<String> = get_method_list(class)
            .into_iter()
            .map(|m| m.name)
            .filter(|m| !base_methods.contains(m))
            .collect();

        assert!(!own.is_empty(), "{class} declares its own hooks");
        assert!(
            own.iter().all(|m| m.starts_with('_')),
            "{class} declares ONLY virtuals in Godot; found non-virtual: {:?}",
            own.iter()
                .filter(|m| !m.starts_with('_'))
                .collect::<Vec<_>>()
        );

        // ...and no properties of their own either.
        let base_props = props(base);
        let own_props: Vec<String> = props(class)
            .into_iter()
            .filter(|p| !base_props.contains(p))
            .collect();
        assert!(
            own_props.is_empty(),
            "{class} declares no properties in Godot; found {own_props:?}"
        );
    }
}

/// The transport hooks a custom peer must provide — the bead names `_poll` and
/// `_get_packet` specifically.
#[test]
fn peer_extension_declares_the_transport_hooks() {
    register_networking_classes();

    // The pump the SceneTree drives each frame.
    assert_eq!(arity("MultiplayerPeerExtension", "_poll"), 0);
    // (r_buffer, r_buffer_size) — the raw pointer form.
    assert_eq!(arity("MultiplayerPeerExtension", "_get_packet"), 2);
    assert_eq!(arity("MultiplayerPeerExtension", "_put_packet"), 2);
    assert_eq!(
        arity("MultiplayerPeerExtension", "_get_available_packet_count"),
        0
    );
    assert_eq!(
        arity("MultiplayerPeerExtension", "_get_connection_status"),
        0
    );
    assert_eq!(arity("MultiplayerPeerExtension", "_get_unique_id"), 0);
    assert_eq!(arity("MultiplayerPeerExtension", "_is_server"), 0);
    assert_eq!(arity("MultiplayerPeerExtension", "_disconnect_peer"), 2);

    // ⚠️ The `_script` variants exist ALONGSIDE the raw ones, not instead of
    // them: Godot calls these when the override comes from a script rather than
    // GDExtension. Dropping them would silently make script peers unreachable —
    // which is exactly this bead's use case.
    assert!(class_has_method(
        "MultiplayerPeerExtension",
        "_get_packet_script"
    ));
    assert!(class_has_method(
        "MultiplayerPeerExtension",
        "_put_packet_script"
    ));
    assert_eq!(arity("MultiplayerPeerExtension", "_get_packet_script"), 0);
    assert_eq!(arity("MultiplayerPeerExtension", "_put_packet_script"), 1);
}

/// The API extension replaces the whole multiplayer stack, so it has to own the
/// RPC path and the spawner/synchronizer registration.
#[test]
fn api_extension_declares_the_rpc_and_configuration_hooks() {
    register_networking_classes();

    assert_eq!(arity("MultiplayerAPIExtension", "_poll"), 0);
    // (object, method, peer_id, args) — the whole RPC path, reimplemented.
    assert_eq!(arity("MultiplayerAPIExtension", "_rpc"), 4);
    assert_eq!(arity("MultiplayerAPIExtension", "_get_remote_sender_id"), 0);
    assert_eq!(arity("MultiplayerAPIExtension", "_set_multiplayer_peer"), 1);
    assert_eq!(arity("MultiplayerAPIExtension", "_get_multiplayer_peer"), 0);
    assert_eq!(arity("MultiplayerAPIExtension", "_get_peer_ids"), 0);
    // How MultiplayerSpawner/Synchronizer register with a custom API.
    assert_eq!(
        arity("MultiplayerAPIExtension", "_object_configuration_add"),
        2
    );
    assert_eq!(
        arity("MultiplayerAPIExtension", "_object_configuration_remove"),
        2
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["MultiplayerPeerExtension", "MultiplayerAPIExtension"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_networking_classes();
    let first = ids();
    register_networking_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
