//! pat-0q2kx.94: the SceneMultiplayer surface scripts actually bind to.
//!
//! Godot's default `MultiplayerAPI` is `SceneMultiplayer`
//! (upstream/godot/modules/multiplayer/scene_multiplayer.cpp): it emits
//! `peer_connected` / `peer_disconnected` / `connected_to_server` /
//! `connection_failed` / `server_disconnected`, gates peers behind an
//! authentication handshake with a timeout, and carries raw bytes through
//! `send_bytes` → `peer_packet`.
//!
//! Patina's `MultiplayerAPI` was a 130-line coordinator with **no signal
//! mechanism at all** — `grep signal|peer_connected network.rs` was 0 hits — so
//! `multiplayer.peer_connected.connect(...)`, the single most common
//! multiplayer line in any Godot project, had nothing to bind to.
//!
//! Every assertion below is on a signal a **script handler** actually received,
//! so "the roster is tracked but nothing is emitted" fails.

use gdplatform::network::{
    ConnectionStatus, MockNetwork, MultiplayerEvent, NetworkPeer, OfflineMultiplayerPeer, PeerId,
    TransferMode,
};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// A script that records every multiplayer signal it is connected to.
const LISTENER: &str = "\
extends Node2D
var connected_peer = -1
var disconnected_peer = -1
var joined_server = 0
var failed = 0
var lost_server = 0
var authenticating = -1
var auth_failed = -1
var got_packet_from = -1
func _on_peer_connected(id):
    self.connected_peer = id
func _on_peer_disconnected(id):
    self.disconnected_peer = id
func _on_connected_to_server():
    self.joined_server = self.joined_server + 1
func _on_connection_failed():
    self.failed = self.failed + 1
func _on_server_disconnected():
    self.lost_server = self.lost_server + 1
func _on_peer_authenticating(id):
    self.authenticating = id
func _on_peer_authentication_failed(id):
    self.auth_failed = id
func _on_peer_packet(id, packet):
    self.got_packet_from = id
";

/// Builds a tree whose `Listener` node has [`LISTENER`] attached, with every
/// multiplayer signal on the scene root wired into it.
///
/// The signals are emitted on the root because that is the object Godot's
/// `multiplayer` property resolves to; connecting from Rust rather than from a
/// `_ready` keeps this test's failure surface to the signal pipeline itself.
fn listener_tree() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let listener = tree
        .add_child(root, Node::new("Listener", "Node2D"))
        .unwrap();
    tree.attach_script(
        listener,
        Box::new(GDScriptNodeInstance::from_source(LISTENER, listener).unwrap()),
    );
    for signal in [
        "peer_connected",
        "peer_disconnected",
        "connected_to_server",
        "connection_failed",
        "server_disconnected",
        "peer_authenticating",
        "peer_authentication_failed",
        "peer_packet",
    ] {
        tree.connect_signal(
            root,
            signal,
            gdobject::signal::Connection::new(listener.object_id(), &format!("_on_{signal}")),
        );
    }
    (tree, listener)
}

fn var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn int_var(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match var(tree, node, name) {
        Some(Variant::Int(n)) => n,
        other => panic!("{name} should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.** A server/client pair driven through two SceneTrees.
#[test]
fn scene_multiplayer_api_surface() {
    let (mut server_tree, server_listener) = listener_tree();
    let (mut client_tree, client_listener) = listener_tree();

    let (server_peer, client_peer) = MockNetwork::create_pair(PeerId(2));
    server_tree
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    client_tree
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client_peer));

    // -- (1) both ends learn about the peer, with the right id ---------------
    //
    // The transport publishes its roster changes into the API, which is what
    // `poll_multiplayer` promotes to script-visible signals.
    server_tree
        .get_multiplayer_mut()
        .peer_state_mut()
        .add_peer(2);
    client_tree
        .get_multiplayer_mut()
        .peer_state_mut()
        .add_peer(1);

    server_tree.poll_multiplayer();
    client_tree.poll_multiplayer();

    assert_eq!(
        int_var(&server_tree, server_listener, "connected_peer"),
        2,
        "the server's script must receive peer_connected with the CLIENT's id"
    );
    assert_eq!(
        int_var(&client_tree, client_listener, "connected_peer"),
        1,
        "the client's script must receive peer_connected with the SERVER's id"
    );

    // -- (2) the client — and only the client — sees connected_to_server -----
    assert_eq!(
        int_var(&client_tree, client_listener, "joined_server"),
        1,
        "a client reaching the server must emit connected_to_server once"
    );
    assert_eq!(
        int_var(&server_tree, server_listener, "joined_server"),
        0,
        "the server must NOT emit connected_to_server — it dialled nobody"
    );

    // -- (3) send_bytes arrives as a peer_packet on the far side -------------
    server_tree
        .get_multiplayer_mut()
        .send_bytes(b"hello", 2, TransferMode::Reliable, 0)
        .expect("send_bytes to a connected peer");
    client_tree.poll_multiplayer();

    assert_eq!(
        int_var(&client_tree, client_listener, "got_packet_from"),
        1,
        "send_bytes from the server must reach the client script as a \
         peer_packet carrying the sender id"
    );

    // -- (4) OfflineMultiplayerPeer is the always-server loopback ------------
    let offline = OfflineMultiplayerPeer::new();
    assert!(
        offline.is_server(),
        "OfflineMultiplayerPeer must report is_server() == true so the same \
         `multiplayer.is_server()` branches work in single-player"
    );
    assert_eq!(offline.get_unique_id(), PeerId::SERVER);
    assert_eq!(offline.get_connection_status(), ConnectionStatus::Connected);

    // …and a *script* sees the same thing, which is the form the acceptance
    // names: single-player code asks `multiplayer.is_server()` and must get
    // `true` with no sockets anywhere.
    let mut solo = SceneTree::new();
    let solo_root = solo.root_id();
    let probe = solo
        .add_child(solo_root, Node::new("Solo", "Node2D"))
        .unwrap();
    solo.attach_script(
        probe,
        Box::new(
            GDScriptNodeInstance::from_source(
                "\
extends Node2D
var server = false
var uid = -1
func probe():
    self.server = multiplayer.is_server()
    self.uid = multiplayer.get_unique_id()
",
                probe,
            )
            .unwrap(),
        ),
    );
    solo.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(OfflineMultiplayerPeer::new()));
    assert!(
        solo.get_multiplayer().is_server(),
        "a tree running the offline peer is its own server, with no sockets"
    );

    let _ = solo.call_script_method(probe, "probe", &[]);
    assert_eq!(
        var(&solo, probe, "server"),
        Some(Variant::Bool(true)),
        "a script must read is_server() == true from the offline peer"
    );
    assert_eq!(
        int_var(&solo, probe, "uid"),
        1,
        "a script must read unique_id == 1 from the offline peer"
    );
}

/// The auth handshake gates the peer: it stays `authenticating` until
/// `complete_auth`, and only then counts as connected.
#[test]
fn auth_callback_gates_the_connection() {
    let (mut tree, listener) = listener_tree();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().set_auth_required(true);

    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();

    assert_eq!(
        int_var(&tree, listener, "authenticating"),
        2,
        "with auth required the peer must surface as peer_authenticating"
    );
    assert_eq!(
        int_var(&tree, listener, "connected_peer"),
        -1,
        "an unauthenticated peer must NOT be reported as connected"
    );
    assert!(tree.get_multiplayer().is_peer_authenticating(2));

    // Accepting promotes it.
    assert!(tree.get_multiplayer_mut().complete_auth(2, true));
    tree.poll_multiplayer();
    assert_eq!(
        int_var(&tree, listener, "connected_peer"),
        2,
        "complete_auth(accepted) must finally emit peer_connected"
    );
    assert!(!tree.get_multiplayer().is_peer_authenticating(2));

    // A second complete_auth is a no-op rather than a duplicate signal.
    assert!(
        !tree.get_multiplayer_mut().complete_auth(2, true),
        "completing an already-completed peer must report that it did nothing"
    );
}

/// An unanswered handshake times out into `peer_authentication_failed` — the
/// reason Godot has a timeout at all. Without the timer aging, a silent peer
/// would hold its slot forever.
#[test]
fn unanswered_auth_times_out() {
    let (mut tree, listener) = listener_tree();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().set_auth_required(true);
    tree.get_multiplayer_mut().set_auth_timeout(1.0);
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();
    assert_eq!(int_var(&tree, listener, "authenticating"), 2);

    // Well short of the timeout: still pending, no failure yet.
    tree.poll_multiplayer_with_delta(0.4);
    assert_eq!(
        int_var(&tree, listener, "auth_failed"),
        -1,
        "the peer must not fail before the timeout elapses"
    );
    assert!(tree.get_multiplayer().is_peer_authenticating(2));

    // Past it.
    tree.poll_multiplayer_with_delta(0.7);
    assert_eq!(
        int_var(&tree, listener, "auth_failed"),
        2,
        "an unanswered handshake must emit peer_authentication_failed"
    );
    assert!(!tree.get_multiplayer().is_peer_authenticating(2));
    assert_eq!(
        int_var(&tree, listener, "connected_peer"),
        -1,
        "a peer that failed auth must never be reported as connected"
    );
}

/// A timeout of `0` disables the timer, as Godot's does — otherwise a project
/// that turns the timeout off would have every peer fail instantly.
#[test]
fn zero_auth_timeout_never_expires() {
    let (mut tree, listener) = listener_tree();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().set_auth_required(true);
    tree.get_multiplayer_mut().set_auth_timeout(0.0);
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();

    tree.poll_multiplayer_with_delta(60.0);
    assert_eq!(
        int_var(&tree, listener, "auth_failed"),
        -1,
        "auth_timeout = 0 must disable the timeout entirely"
    );
    assert!(tree.get_multiplayer().is_peer_authenticating(2));
}

/// `connection_failed` and `server_disconnected` are different events, and the
/// difference is whether the connection was ever established. A game reacts
/// differently to each, so collapsing them would be a real parity bug.
#[test]
fn failed_and_dropped_connections_are_distinct() {
    // Gave up mid-handshake -> connection_failed.
    let (mut tree, listener) = listener_tree();
    let mut peer = gdplatform::network::MockNetworkPeer::new(PeerId(2));
    peer.set_status(ConnectionStatus::Connecting);
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(peer));
    tree.poll_multiplayer();
    assert_eq!(int_var(&tree, listener, "joined_server"), 0);

    tree.get_multiplayer_mut().peer_mut().unwrap().close();
    tree.poll_multiplayer();
    assert_eq!(
        int_var(&tree, listener, "failed"),
        1,
        "dropping out of Connecting is a connection_failed"
    );
    assert_eq!(
        int_var(&tree, listener, "lost_server"),
        0,
        "a connection that never established cannot be a server_disconnected"
    );

    // Established, then lost -> server_disconnected.
    let (mut tree2, listener2) = listener_tree();
    let peer2 = gdplatform::network::MockNetworkPeer::new(PeerId(2));
    tree2
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(peer2));
    tree2.poll_multiplayer();
    assert_eq!(
        int_var(&tree2, listener2, "joined_server"),
        1,
        "reaching the server emits connected_to_server"
    );

    tree2.get_multiplayer_mut().peer_mut().unwrap().close();
    tree2.poll_multiplayer();
    assert_eq!(
        int_var(&tree2, listener2, "lost_server"),
        1,
        "losing an established connection is a server_disconnected"
    );
    assert_eq!(
        int_var(&tree2, listener2, "failed"),
        0,
        "an established connection that drops is not a connection_failed"
    );
}

/// `send_bytes` addressing follows Godot's peer ids: `0` broadcasts and a
/// negative id means "everyone except that peer".
#[test]
fn send_bytes_honours_broadcast_and_except_addressing() {
    let mut api = gdplatform::network::MultiplayerAPI::new();
    let (mut server_peer, mut client) = MockNetwork::create_pair(PeerId(2));
    // The default double forwards every packet to its one remote inbox
    // regardless of address, so "peer 2 was skipped" could never fail. Make it
    // honour `to` for this test; peer 3 is on the roster but has no transport,
    // so packets addressed there are dropped rather than landing on peer 2.
    server_peer.set_addressed_delivery(Some(PeerId(2)));
    api.set_multiplayer_peer(Box::new(server_peer));
    api.peer_state_mut().add_peer(2);
    api.peer_state_mut().add_peer(3);
    api.take_events();

    // Broadcast reaches peer 2.
    api.send_bytes(b"all", 0, TransferMode::Reliable, 0)
        .unwrap();
    assert!(
        !client.poll().is_empty(),
        "a broadcast must reach every connected peer"
    );

    // "Everyone except 2" must not.
    api.send_bytes(b"not-two", -2, TransferMode::Reliable, 0)
        .unwrap();
    assert!(
        client.poll().is_empty(),
        "a negative target means 'all except that peer', so peer 2 must be \
         skipped"
    );
}

/// An empty `send_bytes` still round-trips as an empty packet rather than being
/// reclassified as an unknown packet type.
#[test]
fn empty_send_bytes_round_trips() {
    let (mut tree, listener) = listener_tree();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();

    assert_eq!(
        gdplatform::network::decode_raw_packet(&gdplatform::network::encode_raw_packet(&[])),
        Some(Vec::new()),
        "an empty raw payload must decode as an empty packet, not as None"
    );
    let _ = listener;
}

/// The event queue is drained, not re-read: a signal must fire once per event.
#[test]
fn events_are_drained_not_replayed() {
    let (mut tree, listener) = listener_tree();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);

    tree.poll_multiplayer();
    assert_eq!(int_var(&tree, listener, "connected_peer"), 2);

    // A second poll with nothing new must not re-emit; proven by watching a
    // counting signal rather than a last-value one.
    let before = int_var(&tree, listener, "joined_server");
    tree.poll_multiplayer();
    assert_eq!(
        int_var(&tree, listener, "joined_server"),
        before,
        "polling again with no new events must not replay the queue"
    );
    assert!(
        tree.get_multiplayer_mut().take_events().is_empty(),
        "the queue must be empty after a poll drained it"
    );
}

/// The script-facing `multiplayer.*` surface — Godot's `Node.multiplayer`.
///
/// This is what makes the whole pipeline usable from GDScript: without it a
/// script could receive the signals but could not ask a single question about
/// the session or send a byte.
#[test]
fn scripts_can_query_and_drive_multiplayer() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Net", "Node2D")).unwrap();
    tree.attach_script(
        node,
        Box::new(
            GDScriptNodeInstance::from_source(
                "\
extends Node2D
var server = false
var uid = -1
var has_peer = false
var peer_count = -1
var sent = -1
func probe():
    self.server = multiplayer.is_server()
    self.uid = multiplayer.get_unique_id()
    self.has_peer = multiplayer.has_multiplayer_peer()
    self.peer_count = multiplayer.get_peers().size()
    self.sent = multiplayer.send_bytes(\"hi\", 2)
",
                node,
            )
            .unwrap(),
        ),
    );

    // With no peer at all, Godot's no-peer answers.
    let _ = tree.call_script_method(node, "probe", &[]);
    assert_eq!(int_var(&tree, node, "uid"), 0, "no peer -> unique_id 0");
    assert_eq!(
        var(&tree, node, "has_peer"),
        Some(Variant::Bool(false)),
        "no peer -> has_multiplayer_peer() false"
    );
    assert_eq!(
        var(&tree, node, "server"),
        Some(Variant::Bool(false)),
        "no peer -> is_server() false, not a crash"
    );

    // Now with a server peer and one client on the roster.
    let (server_peer, mut client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();

    let _ = tree.call_script_method(node, "probe", &[]);
    assert_eq!(
        var(&tree, node, "server"),
        Some(Variant::Bool(true)),
        "a script must be able to ask multiplayer.is_server()"
    );
    assert_eq!(
        int_var(&tree, node, "uid"),
        1,
        "the server's unique id is 1"
    );
    assert_eq!(
        int_var(&tree, node, "peer_count"),
        1,
        "get_peers() must report the connected client"
    );
    assert_eq!(
        int_var(&tree, node, "sent"),
        0,
        "send_bytes must report OK (0) from a script"
    );
    assert!(
        !client.poll().is_empty(),
        "the bytes a script sent must actually reach the peer"
    );
}

/// A script can drive the auth handshake, which is the point of exposing it.
#[test]
fn scripts_can_complete_auth() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Net", "Node2D")).unwrap();
    tree.attach_script(
        node,
        Box::new(
            GDScriptNodeInstance::from_source(
                "\
extends Node2D
var pending = -1
var accepted = -1
func check():
    self.pending = multiplayer.get_authenticating_peers().size()
func accept():
    self.accepted = multiplayer.complete_auth(2)
",
                node,
            )
            .unwrap(),
        ),
    );

    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().set_auth_required(true);
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();

    let _ = tree.call_script_method(node, "check", &[]);
    assert_eq!(
        int_var(&tree, node, "pending"),
        1,
        "get_authenticating_peers() must show the waiting peer to a script"
    );

    let _ = tree.call_script_method(node, "accept", &[]);
    assert_eq!(
        int_var(&tree, node, "accepted"),
        0,
        "complete_auth() from a script must report OK"
    );
    assert!(!tree.get_multiplayer().is_peer_authenticating(2));
}

/// `refuse_new_connections` must actually refuse. A stored-but-ignored flag
/// would pass any test that only reads the property back.
#[test]
fn refuse_new_connections_actually_refuses() {
    let (mut tree, listener) = listener_tree();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    tree.get_multiplayer_mut().set_refuse_new_connections(true);

    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.poll_multiplayer();

    assert_eq!(
        int_var(&tree, listener, "connected_peer"),
        -1,
        "a refused peer must never surface as peer_connected"
    );
    assert_eq!(
        int_var(&tree, listener, "disconnected_peer"),
        -1,
        "nor as peer_disconnected — from the script's view it never joined"
    );
    assert!(
        tree.get_multiplayer().get_peers().is_empty(),
        "a refused peer must not stay on the roster"
    );

    // Turning it back off admits the next peer normally.
    tree.get_multiplayer_mut().set_refuse_new_connections(false);
    tree.get_multiplayer_mut().peer_state_mut().add_peer(3);
    tree.poll_multiplayer();
    assert_eq!(
        int_var(&tree, listener, "connected_peer"),
        3,
        "clearing the flag must let the next peer connect"
    );
}

/// Sanity on the event model itself, independent of the tree: the API queues
/// exactly the events the transport justifies.
#[test]
fn api_queues_expected_events() {
    let mut api = gdplatform::network::MultiplayerAPI::new();
    let (server_peer, _client) = MockNetwork::create_pair(PeerId(2));
    api.set_multiplayer_peer(Box::new(server_peer));
    api.peer_state_mut().add_peer(2);
    api.poll();

    let events = api.take_events();
    assert!(
        events.contains(&MultiplayerEvent::PeerConnected(2)),
        "a roster addition must queue PeerConnected, got {events:?}"
    );
    assert_eq!(api.get_peers(), vec![2]);
    assert!(
        api.take_events().is_empty(),
        "take_events must drain rather than copy"
    );
}
