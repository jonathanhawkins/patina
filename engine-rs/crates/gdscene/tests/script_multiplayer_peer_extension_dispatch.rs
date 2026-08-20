//! pat-s3rrv.230 — a GDScript `extends MultiplayerPeerExtension` actually drives
//! the transport.
//!
//! Godot lets a project supply its own transport: `extends MultiplayerPeerExtension`,
//! assign it to `multiplayer.multiplayer_peer`, and the engine calls `_poll()`
//! every frame and moves bytes with `_put_packet` / `_get_packet`.
//!
//! Patina registered the class with all 23 virtuals and called none of them. The
//! assignment was dropped on the floor one layer earlier:
//! `SceneAccess::multiplayer_set_peer` returned `false` for anything that was not
//! a built-in peer handle, so `multiplayer.multiplayer_peer = self` did nothing at
//! all and `_poll` was never reached.
//!
//! ## Why the script's identity is its NODE
//!
//! A bare `ClassInstance` has no Variant form, so `MyPeer.new()` cannot be handed
//! across the interpreter boundary. `self` — a node `ObjectId` — is the only
//! script identity there is. That is the same conclusion `SCRIPT_TRANSLATIONS`
//! reached for `extends Translation` (pat-s3rrv.229), and this follows it rather
//! than inventing a second convention.
//!
//! ## Why there is a bridge rather than an impl
//!
//! `NetworkPeer` is `Send`; `ScriptInstance` is not. A script therefore cannot BE
//! a peer. Instead the API owns an ordinary loopback peer and the tree pumps bytes
//! between it and the script once per frame.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A transport written entirely in GDScript: two queues and a poll counter.
///
/// Untyped params and plain returns, matching the shape the interpreter is known
/// to accept (see `tail_extension_surfaces_registered_and_dispatched`).
const PEER_SCRIPT: &str = "\
extends MultiplayerPeerExtension

var inbox = []
var outbox = []
var polls = 0
var target = 0

func _ready():
\tmultiplayer.multiplayer_peer = self

func _poll():
\tpolls += 1

func _get_available_packet_count():
\treturn inbox.size()

func _get_packet():
\treturn inbox.pop_front()

func _put_packet(data):
\toutbox.append(data)
\treturn 0

func _set_target_peer(id):
\ttarget = id

func _get_unique_id():
\treturn 1
";

/// The node an `@rpc` lands on.
const PLAYER_SCRIPT: &str = "\
extends Node

var got = []

@rpc(\"any_peer\")
func ping(v):
\tgot.append(v)

func send_ping():
\trpc(\"ping\", 7)
";

/// A script that replaces the whole multiplayer stack.
const API_SCRIPT: &str = "\
extends MultiplayerAPIExtension

var rpcs = []

func _rpc(peer, object, method, args):
\trpcs.append(method)
\treturn 0

func _poll():
\treturn 0
";

/// One tree with a script peer on `Net` and an rpc target on `Player`.
struct Host {
    ml: MainLoop,
    net: NodeId,
    player: NodeId,
}

fn host() -> Host {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let net = tree.add_child(root, Node::new("Net", "Node")).unwrap();
    tree.attach_script(
        net,
        Box::new(GDScriptNodeInstance::from_source(PEER_SCRIPT, net).expect("peer script parses")),
    );
    let player = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    tree.attach_script(
        player,
        Box::new(
            GDScriptNodeInstance::from_source(PLAYER_SCRIPT, player).expect("player script parses"),
        ),
    );

    let mut ml = MainLoop::new(tree);
    // `_ready` runs through the host-driven lifecycle, which is where the script
    // assigns itself as the transport.
    ml.enter_tree_with_scripts(net);
    ml.enter_tree_with_scripts(player);
    Host { ml, net, player }
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn array_of(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(a) => a,
        _ => Vec::new(),
    }
}

/// **The acceptance.** A script transport on two trees carries an `@rpc` from one
/// to the other, with `_poll` driven once per frame and the payload travelling
/// through `_put_packet` / `_get_packet`.
#[test]
fn script_multiplayer_peer_extension_dispatch() {
    let mut a = host();
    let mut b = host();

    // The assignment in `_ready` must have been ACCEPTED. Before this bead it
    // returned false and nothing below could work.
    assert_eq!(
        a.ml.tree().script_multiplayer_peer(),
        Some(a.net),
        "`multiplayer.multiplayer_peer = self` must install the script as the transport"
    );

    // --- `_poll` runs once per frame, per tree -----------------------------
    assert_eq!(
        prop(&a.ml, a.net, "polls").to_int(),
        0,
        "no frame has run yet"
    );
    a.ml.tree_mut().poll_multiplayer();
    assert_eq!(
        prop(&a.ml, a.net, "polls").to_int(),
        1,
        "_poll must be called exactly once per frame — a level-triggered drive would \
         count more, and an unwired one zero"
    );
    a.ml.tree_mut().poll_multiplayer();
    assert_eq!(
        prop(&a.ml, a.net, "polls").to_int(),
        2,
        "and once more on the next frame"
    );
    // Tree B polls independently: the counters must not be shared.
    b.ml.tree_mut().poll_multiplayer();
    assert_eq!(
        prop(&b.ml, b.net, "polls").to_int(),
        1,
        "each tree drives its OWN peer — a process-wide registry would show 3 here"
    );

    // --- an rpc on A leaves through `_put_packet` --------------------------
    // Fired FROM the script, which is the only path a real project uses — and the
    // `@rpc` annotation is what registers it, so no manual registration here.
    a.ml.tree_mut()
        .call_script_method(a.player, "send_ping", &[]);
    // `rpc()` runs inside the node's own script, so the send is queued until the
    // script is back in the arena; a frame step flushes it.
    a.ml.step(1.0 / 60.0);
    a.ml.tree_mut().poll_multiplayer();

    let outbox = array_of(prop(&a.ml, a.net, "outbox"));
    assert!(
        !outbox.is_empty(),
        "the rpc must reach the script transport through _put_packet — an empty \
         outbox means the bridge never handed the packet over"
    );
    // The payload is real bytes, not an empty placeholder.
    let payload = match &outbox[0] {
        Variant::PackedByteArray(bytes) => bytes.clone(),
        other => panic!("_put_packet must receive a PackedByteArray, got {other:?}"),
    };
    assert!(
        !payload.is_empty(),
        "the packet handed to _put_packet must carry the rpc payload"
    );

    // --- the test is the wire: hand A's bytes to B -------------------------
    {
        let script =
            b.ml.tree_mut()
                .get_script_mut(b.net)
                .expect("peer script attached");
        script.set_property(
            "inbox",
            Variant::Array(vec![Variant::PackedByteArray(payload)]),
        );
    }

    // B drains its script through `_get_available_packet_count` / `_get_packet`
    // and the rpc lands on B's Player.
    b.ml.tree_mut().poll_multiplayer();
    b.ml.tree_mut().poll_multiplayer();

    let inbox_left = array_of(prop(&b.ml, b.net, "inbox"));
    assert!(
        inbox_left.is_empty(),
        "the engine must DRAIN the script's inbox via _get_packet; {} left",
        inbox_left.len()
    );

    let got = array_of(prop(&b.ml, b.player, "got"));
    assert_eq!(
        got.len(),
        1,
        "the @rpc must arrive on tree B's node — this is the end-to-end clause"
    );
    assert_eq!(
        got[0].to_int(),
        7,
        "and it must carry the argument that was sent"
    );
}

/// A node with no `_poll` is refused rather than installed as a transport that
/// could never carry anything.
///
/// Without this the failure is invisible: assignment appears to succeed and every
/// later frame silently moves nothing.
#[test]
fn script_multiplayer_peer_extension_dispatch_refuses_a_non_peer_script() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree.add_child(root, Node::new("Plain", "Node")).unwrap();
    tree.attach_script(
        n,
        Box::new(GDScriptNodeInstance::from_source(PLAYER_SCRIPT, n).expect("parses")),
    );

    assert!(
        !tree.set_script_multiplayer_peer(n),
        "a script with no _poll/_get_available_packet_count is not a transport"
    );
    assert_eq!(
        tree.script_multiplayer_peer(),
        None,
        "and nothing may be installed"
    );
}

/// A script `extends MultiplayerAPIExtension` takes over from the built-in stack:
/// its `_rpc` runs and the engine's own path does not.
#[test]
fn script_multiplayer_peer_extension_dispatch_api_extension_takes_over_rpc() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let api = tree.add_child(root, Node::new("Api", "Node")).unwrap();
    tree.attach_script(
        api,
        Box::new(GDScriptNodeInstance::from_source(API_SCRIPT, api).expect("api script parses")),
    );
    let player = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(PLAYER_SCRIPT, player).expect("parses")),
    );

    assert!(
        tree.set_script_multiplayer_api(api),
        "a script with _rpc must be accepted as a MultiplayerAPIExtension"
    );

    let handled = tree.rpc_through_script_api(0, player, "ping", &[Variant::Int(3)]);
    assert!(handled, "the script API must handle the call");

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(api);
    let rpcs = array_of(prop(&ml, api, "rpcs"));
    assert_eq!(
        rpcs.len(),
        1,
        "_rpc must have been called once, replacing the built-in path"
    );
    assert_eq!(
        rpcs[0],
        Variant::String("ping".to_string()),
        "_rpc receives the method name Godot passes"
    );
}

/// The script spelling of the same install (pat-s3rrv.230).
///
/// `get_tree().set_multiplayer(api)` is how a project actually writes this in
/// Godot. The capability above was reachable only from Rust — the SceneTree
/// method existed and no script could name it — so this is the half of the
/// bead's acceptance that "installed via set_multiplayer" is asking for.
const API_SCRIPT_SELF_INSTALLING: &str = "\
extends MultiplayerAPIExtension

var rpcs = []

func _ready():
\tget_tree().set_multiplayer(self)

func _rpc(peer, object, method, args):
\trpcs.append(method)
\treturn 0

func _poll():
\treturn 0
";

#[test]
fn script_multiplayer_peer_extension_dispatch_set_multiplayer_from_script() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let api = tree.add_child(root, Node::new("Api", "Node")).unwrap();
    tree.attach_script(
        api,
        Box::new(
            GDScriptNodeInstance::from_source(API_SCRIPT_SELF_INSTALLING, api)
                .expect("api script parses"),
        ),
    );
    let player = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(PLAYER_SCRIPT, player).expect("parses")),
    );

    // The control: nothing is installed until the script asks. Without this the
    // assertion below would pass on an engine that installed every script with
    // an `_rpc`, which is not what `set_multiplayer` means.
    assert_eq!(
        tree.script_multiplayer_api(),
        None,
        "no API is installed before _ready runs"
    );

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(api);

    assert_eq!(
        ml.tree().script_multiplayer_api(),
        Some(api),
        "get_tree().set_multiplayer(self) must install the script as the tree's API"
    );

    // And it is the same install, not merely a recorded intention: the RPC path
    // has to reach `_rpc`.
    let handled = ml
        .tree_mut()
        .rpc_through_script_api(0, player, "ping", &[Variant::Int(7)]);
    assert!(
        handled,
        "the script API installed from GDScript must handle the call"
    );

    let rpcs = array_of(prop(&ml, api, "rpcs"));
    assert_eq!(
        rpcs.len(),
        1,
        "_rpc must have been called exactly once through the script-installed API"
    );
    assert_eq!(
        rpcs[0],
        Variant::String("ping".to_string()),
        "_rpc receives the method name Godot passes"
    );
}
