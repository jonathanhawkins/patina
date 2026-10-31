//! pat-mgixk.26 acceptance: `rpc()` reaches every peer, the server relays a
//! client's broadcast, and the receiver enforces the `@rpc` table.
//!
//! # What was wrong
//!
//! Godot's `Node.rpc()` targets peer **0** — every connected peer
//! (`scene_rpc_interface.cpp` `rpcp`, where `p_peer_id == 0` means all) — and
//! on a star topology a client reaches the other clients only because the
//! server forwards for it (`scene_multiplayer.cpp` `server_relay`, default
//! true). The receiver then checks the call before running it: a method with no
//! `@rpc` config is dropped, and an `RPC_MODE_AUTHORITY` method is dropped
//! unless the sender IS the node's multiplayer authority.
//!
//! Patina had three separate holes:
//!
//! 1. `MultiplayerAPI::rpc` routed by `authority_map`, which nothing outside
//!    the module's own tests ever wrote — so the target was ALWAYS peer 1. A
//!    server's `rpc()` was addressed to itself and the transport answered
//!    `PeerUnreachable`: `rpc()` reached nobody, ever.
//! 2. No server relay, so even once the target was peer 0 a client's broadcast
//!    died at the server and the other clients never heard it.
//! 3. Every inbound packet ran unconditionally, so any peer could invoke any
//!    script method on any node — remote code execution in a shipped game.
//!
//! # The path driven
//!
//! ```text
//! script on peer A: rpc("m", …)
//!   -> interpreter -> scripting.rs node_rpc -> MultiplayerAPI::rpc
//!     -> transport (the star below)
//!       -> MainLoop::step on peers B and C -> poll_multiplayer_with_delta
//!         -> the @rpc gates -> the script method runs there
//! ```
//!
//! Every peer is a real `SceneTree` + `MainLoop` running an attached GDScript,
//! and every assertion reads a script variable — what a game would observe.
//!
//! ⚠️ The transport double below moves bytes and nothing else. Broadcast
//! targeting, the relay envelope and the receive-side checks all live in
//! production code, so what these assertions measure is the engine's behaviour
//! and not the double's.

use gdplatform::multiplayer_peer_state::MultiplayerPeerSignal;
use gdplatform::network::{ConnectionStatus, NetworkPeer, Packet, PeerId, Result};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use std::sync::{Arc, Mutex};

/// 60 Hz, the tick the multiplayer poll runs on.
const DT: f64 = 1.0 / 60.0;

type Queue = Arc<Mutex<Vec<Packet>>>;

/// A star-topology transport: one server holding a link per client, and clients
/// holding a single link to the server.
///
/// `MockNetwork::create_pair` gives each peer exactly ONE remote inbox, so a
/// server built from it can only ever reach a single client. Two of this bead's
/// claims cannot be expressed against that shape at all: "`rpc()` reaches B
/// **and** C", and relay, which needs the server to forward one client's packet
/// to a *different* client. Hence a star.
///
/// It lives in this test file rather than beside `MockNetwork` so that the
/// acceptance keeps compiling when the verifier reverts the engine to HEAD —
/// the test then fails on the *behaviour* it is about, which is the point of
/// the check, instead of failing to build.
struct StarPeer {
    id: PeerId,
    /// Packets waiting for this peer.
    inbox: Queue,
    /// Server only: one inbox per client, keyed by peer id.
    routes: Vec<(PeerId, Queue)>,
    /// Client only: the server's inbox — its single link.
    uplink: Option<Queue>,
    /// Roster signals handed to `MultiplayerAPI::poll`.
    signals: Vec<MultiplayerPeerSignal>,
}

impl NetworkPeer for StarPeer {
    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()> {
        if let Some(uplink) = &self.uplink {
            // A client holds exactly one link, so its packet goes to the server
            // whatever it is addressed to — which is what the real client
            // transports do (`tcp_multiplayer.rs` overrides the target with
            // `PeerId::SERVER`). This is the whole reason a client's broadcast
            // can only reach the other clients if the server relays it.
            uplink.lock().expect("lock poisoned").push(packet);
            return Ok(());
        }
        // The server honours the address exactly as the real transports do:
        // peer 0 is every peer, any other id is one peer.
        //
        // Written as the literal `PeerId(0)` rather than the named broadcast
        // constant deliberately: that constant is part of what this bead added,
        // and this file has to keep COMPILING against the engine as it was, so
        // that the red check measures behaviour and not a missing symbol.
        if to == PeerId(0) {
            for (_, inbox) in &self.routes {
                inbox.lock().expect("lock poisoned").push(packet.clone());
            }
            return Ok(());
        }
        if let Some((_, inbox)) = self.routes.iter().find(|(id, _)| *id == to) {
            inbox.lock().expect("lock poisoned").push(packet);
        }
        // An id this star never had is dropped rather than failed: a real
        // transport does not error on a datagram with no route.
        Ok(())
    }

    fn poll(&mut self) -> Vec<Packet> {
        std::mem::take(&mut *self.inbox.lock().expect("lock poisoned"))
    }

    fn get_unique_id(&self) -> PeerId {
        self.id
    }

    fn is_server(&self) -> bool {
        self.id == PeerId::SERVER
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        ConnectionStatus::Connected
    }

    fn close(&mut self) {}

    fn drain_signals(&mut self) -> Vec<MultiplayerPeerSignal> {
        std::mem::take(&mut self.signals)
    }
}

/// Wire a server to one client per entry of `client_ids`.
///
/// Each peer is handed the `PeerConnected` signals a real transport would emit,
/// so the server's roster is filled by the engine's own handshake path
/// (`MultiplayerAPI::poll`) rather than by a test reaching into it. The relay
/// fan-out reads that roster, so a backdoor here would be measuring itself.
fn build_star(client_ids: &[PeerId]) -> (StarPeer, Vec<StarPeer>) {
    let server_inbox: Queue = Arc::new(Mutex::new(Vec::new()));
    let mut routes = Vec::new();
    let mut clients = Vec::new();
    for id in client_ids {
        let inbox: Queue = Arc::new(Mutex::new(Vec::new()));
        routes.push((*id, Arc::clone(&inbox)));
        clients.push(StarPeer {
            id: *id,
            inbox,
            routes: Vec::new(),
            uplink: Some(Arc::clone(&server_inbox)),
            signals: vec![MultiplayerPeerSignal::PeerConnected(
                PeerId::SERVER.0 as i32,
            )],
        });
    }
    let server = StarPeer {
        id: PeerId::SERVER,
        inbox: server_inbox,
        routes,
        uplink: None,
        signals: client_ids
            .iter()
            .map(|id| MultiplayerPeerSignal::PeerConnected(id.0 as i32))
            .collect(),
    };
    (server, clients)
}

/// The server's script. It SENDS `sync_state` and `unannotated_on_client`, and
/// RECEIVES `client_shout`.
///
/// `authority_only` is a bare `@rpc`, which registers as Godot's default
/// `RPCMode::Authority` — and this node keeps the default authority (peer 1,
/// the server), so a call from a client must be refused.
const SERVER_GD: &str = "\
extends Node
var state = 0
var state_hits = 0
var shout = 0
var shout_sender = 0
var authority_hits = 0
var unannotated_hits = 0
var remote_echo = 0
var local_echo = 0

@rpc(\"any_peer\", \"reliable\")
func sync_state(n):
    self.state = n
    self.state_hits = self.state_hits + 1

@rpc(\"any_peer\", \"reliable\")
func client_shout(n):
    self.shout = n
    self.shout_sender = multiplayer.get_remote_sender_id()

@rpc(\"reliable\")
func authority_only(n):
    self.authority_hits = self.authority_hits + n

@rpc(\"any_peer\", \"reliable\")
func unannotated_on_client(n):
    self.unannotated_hits = self.unannotated_hits + n

@rpc(\"any_peer\", \"reliable\")
func remote_only(n):
    self.remote_echo = self.remote_echo + n

@rpc(\"any_peer\", \"call_local\", \"reliable\")
func local_too(n):
    self.local_echo = self.local_echo + n

func fire_sync():
    rpc(\"sync_state\", 5)

func fire_unannotated():
    rpc(\"unannotated_on_client\", 1)

func fire_remote_only():
    rpc(\"remote_only\", 1)

func fire_local_too():
    rpc(\"local_too\", 1)
";

/// Both clients' script.
///
/// ⚠️ `unannotated_on_client` carries **no** annotation here. The server sends
/// it annotated, so the drop has to happen on the receiver — which is where
/// Godot decides, and the only place it can be decided safely.
const CLIENT_GD: &str = "\
extends Node
var state = 0
var state_hits = 0
var shout = 0
var shout_sender = 0
var authority_hits = 0
var unannotated_hits = 0
var remote_echo = 0
var local_echo = 0

@rpc(\"any_peer\", \"reliable\")
func sync_state(n):
    self.state = n
    self.state_hits = self.state_hits + 1

@rpc(\"any_peer\", \"reliable\")
func client_shout(n):
    self.shout = n
    self.shout_sender = multiplayer.get_remote_sender_id()

@rpc(\"reliable\")
func authority_only(n):
    self.authority_hits = self.authority_hits + n

func unannotated_on_client(n):
    self.unannotated_hits = self.unannotated_hits + n

@rpc(\"any_peer\", \"reliable\")
func remote_only(n):
    self.remote_echo = self.remote_echo + n

@rpc(\"any_peer\", \"call_local\", \"reliable\")
func local_too(n):
    self.local_echo = self.local_echo + n

func fire_shout():
    rpc(\"client_shout\", 7)

func fire_authority():
    rpc(\"authority_only\", 1)
";

/// One peer: a tree with a scripted `Player`, the given transport, and a loop.
///
/// ⚠️ Every peer's node sits at the same path (`/root/Player`). Godot addresses
/// an RPC by node path, and the inbound hash is what resolves to the receiver's
/// matching node.
fn peer(script_src: &str, transport: StarPeer) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(script_src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(transport));
    (MainLoop::new(tree), id)
}

fn prop(ml: &MainLoop, id: NodeId, name: &str) -> Option<Variant> {
    ml.tree().get_script(id).and_then(|s| s.get_property(name))
}

fn int(ml: &MainLoop, id: NodeId, name: &str) -> i64 {
    match prop(ml, id, name) {
        Some(Variant::Int(v)) => v,
        other => panic!("{name} should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.** All five claims, on one three-peer session.
#[test]
fn multiplayer_rpc_broadcast_authority_test() {
    let (server_transport, mut client_transports) = build_star(&[PeerId(2), PeerId(3)]);
    let t3 = client_transports.pop().expect("client 3 transport");
    let t2 = client_transports.pop().expect("client 2 transport");

    let (mut server, server_node) = peer(SERVER_GD, server_transport);
    let (mut client2, node2) = peer(CLIENT_GD, t2);
    let (mut client3, node3) = peer(CLIENT_GD, t3);

    // One frame everywhere, so each script's `@rpc` table is registered and the
    // roster signals have reached the API.
    server.step(DT);
    client2.step(DT);
    client3.step(DT);

    // Fixture check: the server really knows both clients. The relay fan-out
    // reads this roster, so an empty one would make the relay assertions below
    // pass for the wrong reason — nothing to forward to, nothing to contradict.
    let mut roster = server.tree().get_multiplayer().get_peers();
    roster.sort_unstable();
    assert_eq!(
        roster,
        vec![2, 3],
        "the server must have both clients on its roster before any RPC"
    );

    // -- (a) the server's rpc() reaches BOTH clients -------------------------
    server
        .tree_mut()
        .call_script_method(server_node, "fire_sync", &[]);
    // `rpc()` puts the packet on the wire synchronously (`scripting.rs`
    // `node_rpc` calls `MultiplayerAPI::rpc` inline). The sender's own frame is
    // still stepped because a `call_local` method is queued rather than run
    // inline — a running script is checked out of the arena, so re-entering it
    // would find nothing — and that queue is flushed by the frame.
    server.step(DT);
    client2.step(DT);
    client3.step(DT);

    for (ml, node, who) in [(&client2, node2, "client 2"), (&client3, node3, "client 3")] {
        assert_eq!(
            int(ml, node, "state"),
            5,
            "{who} must run sync_state(5): `rpc()` targets peer 0, every \
             connected peer. Before this bead it was addressed to the node's \
             authority — a map nothing ever wrote — so it went to peer 1, the \
             server itself, and no client ever heard it."
        );
        assert_eq!(
            int(ml, node, "state_hits"),
            1,
            "{who} must run it exactly once"
        );
    }

    // -- (b) a client's rpc() reaches the server AND, via relay, the other
    //        client — carrying the ORIGINAL sender ---------------------------
    client2
        .tree_mut()
        .call_script_method(node2, "fire_shout", &[]);
    client2.step(DT); // client 2 -> (relay envelope) -> server
    server.step(DT); // server runs it, and forwards to client 3
    client3.step(DT); // client 3 receives the forwarded packet

    assert_eq!(
        int(&server, server_node, "shout"),
        7,
        "the server must run a client's any_peer broadcast"
    );
    assert_eq!(
        int(&client3, node3, "shout"),
        7,
        "client 3 must receive client 2's broadcast THROUGH THE SERVER. A \
         client has exactly one link — to the server — so without server_relay \
         this call dies at the server and the other clients never hear it. \
         This is the assertion the relay exists for."
    );
    assert_eq!(
        int(&client3, node3, "shout_sender"),
        2,
        "the relayed call must report the ORIGINAL sender (peer 2), not the \
         server that passed it on. Rewriting the sender here would both lie to \
         the script and hand a client the server's authority on the next \
         authority-gated call."
    );
    assert_eq!(
        int(&server, server_node, "shout_sender"),
        2,
        "the server must see peer 2 as the sender too"
    );
    assert_eq!(
        int(&client2, node2, "shout"),
        0,
        "client_shout is call_remote, so it must NOT run on the sender"
    );

    // -- (c) a client calling an authority-only method on a server-owned node
    //        is refused ----------------------------------------------------
    client2
        .tree_mut()
        .call_script_method(node2, "fire_authority", &[]);
    client2.step(DT);
    server.step(DT);
    client3.step(DT);

    assert_eq!(
        int(&server, server_node, "authority_hits"),
        0,
        "a bare @rpc is RPCMode::Authority, and this node's authority is the \
         server (peer 1), so peer 2's call must be dropped. Patina used to run \
         every inbound packet unconditionally — any peer could call any method \
         on any node."
    );
    assert_eq!(
        int(&client3, node3, "authority_hits"),
        0,
        "the relayed copy must be refused on client 3 for the same reason: \
         relaying must not launder a call past the authority check"
    );

    // -- (d) a method the RECEIVER never annotated is refused ----------------
    server
        .tree_mut()
        .call_script_method(server_node, "fire_unannotated", &[]);
    server.step(DT);
    client2.step(DT);
    client3.step(DT);

    for (ml, node, who) in [(&client2, node2, "client 2"), (&client3, node3, "client 3")] {
        assert_eq!(
            int(ml, node, "unannotated_hits"),
            0,
            "{who} never annotated unannotated_on_client, so the call must be \
             dropped however well-formed it is — a method nobody marked @rpc \
             is not remotely callable, however reachable it is locally"
        );
    }

    // -- (e) call_remote skips the sender; call_local does not ---------------
    server
        .tree_mut()
        .call_script_method(server_node, "fire_remote_only", &[]);
    server.step(DT);
    client2.step(DT);

    assert_eq!(
        int(&server, server_node, "remote_echo"),
        0,
        "call_remote (the default) must not run on the sending peer"
    );
    assert_eq!(
        int(&client2, node2, "remote_echo"),
        1,
        "…but it must still run on the receiver, or the assertion above would \
         pass simply because the call went nowhere"
    );

    server
        .tree_mut()
        .call_script_method(server_node, "fire_local_too", &[]);
    server.step(DT);
    client2.step(DT);

    assert_eq!(
        int(&server, server_node, "local_echo"),
        1,
        "@rpc(…, \"call_local\") must run on the sending peer too"
    );
    assert_eq!(
        int(&client2, node2, "local_echo"),
        1,
        "…and on the receiver, exactly once"
    );
}

/// The harness guard: a client must NOT be able to reach another client
/// directly.
///
/// If the double ever delivered a client's packet straight to its sibling, the
/// relay assertion in the acceptance would pass with the relay deleted — the
/// test would be measuring the transport double instead of the engine. This
/// pins the one property that makes claim (b) meaningful.
#[test]
fn a_client_transport_reaches_only_the_server() {
    let (mut server, mut clients) = build_star(&[PeerId(2), PeerId(3)]);
    let mut c2 = clients.remove(0);
    let mut c3 = clients.remove(0);

    let packet = Packet {
        sender: PeerId(2),
        channel: 0,
        data: vec![0xAB],
        transfer_mode: gdplatform::network::TransferMode::Reliable,
    };

    // Client 2 addresses peer 3 as directly as it can.
    c2.send_packet(PeerId(3), packet.clone()).expect("send ok");
    assert!(
        c3.poll().is_empty(),
        "a client must not reach another client directly — if it could, the \
         relay claim would be unfalsifiable"
    );
    assert_eq!(
        server.poll().len(),
        1,
        "the packet must land on the server instead, as a client's single link \
         requires"
    );

    // And the server's broadcast does reach both, or claim (a) would be
    // vacuous.
    server.send_packet(PeerId(0), packet).expect("broadcast ok");
    assert_eq!(c2.poll().len(), 1, "broadcast must reach client 2");
    assert_eq!(c3.poll().len(), 1, "broadcast must reach client 3");
}
