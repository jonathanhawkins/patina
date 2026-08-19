//! pat-s3rrv.81 — a real WebSocket session opened entirely from GDScript.
//!
//! # What this covers that the siblings do not
//!
//! `multiplayer_peer_assigned_from_gdscript.rs` proves a script can build a peer
//! and assign it; `scene_multiplayer_signals_and_full_method_surface_from_script.rs`
//! proves `multiplayer.peer_connected.connect(cb)` fires — but it announces the
//! peer with a Rust-side `add_peer(7)`, so no transport ever runs.
//! `gdplatform/tests/websocket_multiplayer_e2e_test.rs` drives a real handshake,
//! but pumps the peers by hand in Rust.
//!
//! This is the join of those three: **two trees, a real TCP handshake, and the
//! only thing driving it is `MainLoop::step`.** Nothing here touches the peer
//! after the script assigns it.
//!
//! # Why the port is chosen in Rust
//!
//! Godot's `create_server(port)` takes an explicit port and offers no API for
//! reading back an ephemeral one, so a pure-GDScript loopback cannot discover
//! `create_server(0)`'s address. Rather than invent a non-Godot getter, the test
//! binds port 0 to learn a free port, drops the listener, and formats that port
//! into both scripts — which is exactly what a project does with a configured
//! port.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use std::net::TcpListener;

/// A free loopback port. The listener is dropped before returning, so the
/// script's own `create_server` can bind it.
fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("a loopback port is available");
    l.local_addr().expect("bound addr").port()
}

fn scripted_loop(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Net", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(node);
    (ml, node)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// **The acceptance test.** A server and a client, each built and assigned by
/// its own script, complete a real handshake and both see `peer_connected`.
#[test]
fn websocket_multiplayer_peer_script_reachable() {
    let port = free_port();

    let server_src = format!(
        "\
extends Node
var err = -1
var joined = 0
func _ready():
    var p = WebSocketMultiplayerPeer.new()
    self.err = p.create_server({port})
    multiplayer.multiplayer_peer = p
    multiplayer.peer_connected.connect(_on_peer)
func _on_peer(id):
    self.joined = id
"
    );
    let client_src = format!(
        "\
extends Node
var err = -1
var joined = 0
func _ready():
    var p = WebSocketMultiplayerPeer.new()
    self.err = p.create_client(\"ws://127.0.0.1:{port}/\")
    multiplayer.multiplayer_peer = p
    multiplayer.peer_connected.connect(_on_peer)
func _on_peer(id):
    self.joined = id
"
    );

    let (mut server, server_node) = scripted_loop(&server_src);
    let (mut client, client_node) = scripted_loop(&client_src);

    // ⚠️ Both calls must have succeeded before anything below means anything: a
    // failed bind or dial would leave both `joined` at 0, which is also what a
    // broken handshake looks like.
    assert_eq!(
        script_var(&server, server_node, "err"),
        Variant::Int(0),
        "create_server must return OK (0) — port {port} was free a moment ago"
    );
    assert_eq!(
        script_var(&client, client_node, "err"),
        Variant::Int(0),
        "create_client must return OK (0)"
    );

    // The ONLY thing driving the transport is the frame loop. No `poll()`, no
    // `pump_until_connected`, no direct peer access — that is the claim.
    let dt = 1.0 / 60.0;
    for _ in 0..240 {
        server.step(dt);
        client.step(dt);
        if !matches!(script_var(&server, server_node, "joined"), Variant::Int(0))
            && !matches!(script_var(&client, client_node, "joined"), Variant::Int(0))
        {
            break;
        }
    }

    // Godot assigns the server id 1 and hands each client a fresh id, so the
    // two sides see DIFFERENT numbers — asserting both is what proves the
    // signal carries the real peer id rather than a constant.
    assert_eq!(
        script_var(&client, client_node, "joined"),
        Variant::Int(1),
        "the client must see the SERVER (id 1) join. 0 means the handshake never \
         completed under frame-loop polling alone"
    );
    match script_var(&server, server_node, "joined") {
        Variant::Int(id) if id > 1 => {}
        other => panic!(
            "the server must see the client join with its assigned id (> 1), got \
             {other:?}. 0 means the accept side never ran from the frame loop"
        ),
    }
}

/// The control: with no client dialling, the server's frame loop must NOT
/// invent a peer.
///
/// ⚠️ Without this, the assertions above would still pass if `peer_connected`
/// fired spuriously on every poll — the callback would set `joined` regardless
/// of whether anything connected.
#[test]
fn a_server_alone_sees_nobody_join() {
    let port = free_port();
    let src = format!(
        "\
extends Node
var joined = 0
func _ready():
    var p = WebSocketMultiplayerPeer.new()
    p.create_server({port})
    multiplayer.multiplayer_peer = p
    multiplayer.peer_connected.connect(_on_peer)
func _on_peer(id):
    self.joined = id
"
    );
    let (mut server, node) = scripted_loop(&src);
    for _ in 0..60 {
        server.step(1.0 / 60.0);
    }
    assert_eq!(
        script_var(&server, node, "joined"),
        Variant::Int(0),
        "nothing connected, so no peer_connected may fire"
    );
}

// ---------------------------------------------------------------------------
// pat-s3rrv.81 — the acceptance's last clause: an @rpc call CROSSES the wire
// ---------------------------------------------------------------------------

/// 🔑 THE RPC MUST NOT BE `call_local`.
///
/// With `"call_local"` Godot also runs the method on the CALLER, so a test using
/// it cannot tell "the packet crossed the transport" from "the sender executed
/// its own method and nothing was sent at all". Declaring it without
/// `call_local` and asserting the sender's own counter stays at 0 is what makes
/// this a transport test rather than a local-dispatch test.
///
/// Both peers name the node `Net`, so both sit at `/root/Net`. RPCs are
/// addressed by node-path hash (`SceneTree::node_for_rpc_hash`), so a receiver
/// whose node lived at a different path would silently drop the call.
fn rpc_peer_src(setup: &str) -> String {
    format!(
        "\
extends Node
var err = -1
var joined = 0
var hits = 0
var last = -1
func _ready():
{setup}
    multiplayer.peer_connected.connect(_on_peer)
func _on_peer(id):
    self.joined = id
@rpc(\"any_peer\")
func remote_ping(n):
    self.hits = self.hits + 1
    self.last = n
func fire():
    rpc(\"remote_ping\", 7)
"
    )
}

/// **Acceptance clause: "an @rpc call made on one peer executing on the other".**
#[test]
fn an_rpc_from_the_client_executes_on_the_server() {
    let port = free_port();

    let (mut server, server_node) = scripted_loop(&rpc_peer_src(&format!(
        "    var p = WebSocketMultiplayerPeer.new()\n\
         \x20   self.err = p.create_server({port})\n\
         \x20   multiplayer.multiplayer_peer = p"
    )));
    let (mut client, client_node) = scripted_loop(&rpc_peer_src(&format!(
        "    var p = WebSocketMultiplayerPeer.new()\n\
         \x20   self.err = p.create_client(\"ws://127.0.0.1:{port}/\")\n\
         \x20   multiplayer.multiplayer_peer = p"
    )));

    assert_eq!(
        script_var(&server, server_node, "err"),
        Variant::Int(0),
        "create_server must return OK"
    );
    assert_eq!(
        script_var(&client, client_node, "err"),
        Variant::Int(0),
        "create_client must return OK"
    );

    // Handshake first — an RPC sent before the peer is connected has nowhere to
    // go, and the failure would look identical to a broken RPC path.
    let dt = 1.0 / 60.0;
    for _ in 0..240 {
        server.step(dt);
        client.step(dt);
        if !matches!(script_var(&server, server_node, "joined"), Variant::Int(0)) {
            break;
        }
    }
    match script_var(&server, server_node, "joined") {
        Variant::Int(id) if id > 1 => {}
        other => panic!("handshake never completed (joined = {other:?}); the RPC assertions below would be vacuous"),
    }

    // The client fires. Only the frame loop moves the packet from here on.
    client
        .tree_mut()
        .call_script_method(client_node, "fire", &[]);
    for _ in 0..240 {
        client.step(dt);
        server.step(dt);
        if !matches!(script_var(&server, server_node, "hits"), Variant::Int(0)) {
            break;
        }
    }

    assert_eq!(
        script_var(&server, server_node, "hits"),
        Variant::Int(1),
        "the server's @rpc method must have run exactly once. 0 means the call \
         never crossed the transport"
    );
    assert_eq!(
        script_var(&server, server_node, "last"),
        Variant::Int(7),
        "the argument must survive the trip — a call that arrived with a default \
         or garbled payload is not a working RPC"
    );

    // 🔑 THE CONTROL. Without `call_local` the SENDER must not have run it, so a
    // passing `hits == 1` above cannot be the client executing its own method
    // while nothing was ever sent.
    assert_eq!(
        script_var(&client, client_node, "hits"),
        Variant::Int(0),
        "the caller must NOT run a non-call_local @rpc locally; if this is 1 the \
         test proves local dispatch, not transport"
    );
}
