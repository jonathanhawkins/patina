//! Acceptance test for pat-s3rrv.18: the `SceneMultiplayer` script surface —
//! signal binding, the auth callback, and every ClassDB-advertised method.
//!
//! Godot ground truth: all five `MultiplayerAPI` connection signals plus
//! SceneMultiplayer's `peer_packet` / `peer_authenticating` /
//! `peer_authentication_failed` live on the API object, so
//! `multiplayer.peer_connected.connect(_on_peer)` is THE idiom for joining a
//! session (`upstream/godot/modules/multiplayer/scene_multiplayer.cpp`).
//!
//! Patina's gap was a reflection surface the runtime could not serve:
//! `class_db.rs` registers `send_bytes` / `set_auth_callback` /
//! `get_auth_callback` / `send_auth` / `complete_auth` /
//! `get_authenticating_peers` / `disconnect_peer` / `clear` on
//! `SceneMultiplayer`, while `call_multiplayer_method` implemented ten methods
//! and answered `UndefinedFunction` for the rest. The signals were emitted on
//! the scene root and `multiplayer` was only a method-call receiver, so
//! `multiplayer.peer_connected.connect(...)` — which a code comment claimed to
//! support — had no binding path at all (`grep 'peer_connected'
//! crates/gdscript-interop/src` returned 0).

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

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

/// **The acceptance test.** `multiplayer.peer_connected.connect(_on_peer)` in
/// `_ready()` receives `_on_peer(id)` when a peer joins, driven only by
/// MainLoop frames.
#[test]
fn scene_multiplayer_signals_and_full_method_surface_from_script() {
    const SRC: &str = "\
extends Node
var joined = 0
var left = 0
func _ready():
    var p = OfflineMultiplayerPeer.new()
    multiplayer.multiplayer_peer = p
    multiplayer.peer_connected.connect(_on_peer)
    multiplayer.peer_disconnected.connect(_on_gone)
func _on_peer(id):
    self.joined = id
func _on_gone(id):
    self.left = id
";
    let (mut ml, node) = scripted_loop(SRC);

    // Nothing has joined yet — the control, so the assertion below cannot pass
    // on a default.
    assert_eq!(
        script_var(&ml, node, "joined"),
        Variant::Int(0),
        "no peer has joined before the event is queued"
    );

    // `MultiplayerAPI::poll` returns early with no transport assigned, so the
    // script assigns one first (pat-s3rrv.16's work, composing with this bead).
    // A peer joins the way the transports announce one — `add_peer` queues the
    // same `PeerConnected` signal the real handshake does, and `MultiplayerAPI::poll`
    // turns it into the event the drain emits. No direct `emit_signal` here.
    ml.tree_mut()
        .get_multiplayer_mut()
        .peer_state_mut()
        .add_peer(7);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "joined"),
        Variant::Int(7),
        "`multiplayer.peer_connected.connect(cb)` must fire the callback when a \
         peer joins — this is THE Godot multiplayer idiom"
    );

    ml.tree_mut()
        .get_multiplayer_mut()
        .peer_state_mut()
        .disconnect_peer(7);
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, node, "left"),
        Variant::Int(7),
        "peer_disconnected must bind the same way"
    );
}

/// Every method ClassDB advertises on `SceneMultiplayer` answers with a value
/// instead of raising `UndefinedFunction`. A raise would abort the script, so
/// reaching the final assignment is itself the assertion.
#[test]
fn every_class_db_scene_multiplayer_method_is_callable() {
    const SRC: &str = "\
extends Node
var reached = false
var peers_after = 0
func _auth_cb(id, data):
    pass
func _ready():
    multiplayer.set_auth_callback(_auth_cb)
    var cb = multiplayer.get_auth_callback()
    multiplayer.send_auth(2, PackedByteArray())
    multiplayer.get_authenticating_peers()
    multiplayer.complete_auth(2)
    multiplayer.disconnect_peer(2)
    multiplayer.clear()
    self.peers_after = multiplayer.get_peers().size()
    self.reached = true
";
    let (ml, node) = scripted_loop(SRC);

    assert_eq!(
        script_var(&ml, node, "reached"),
        Variant::Bool(true),
        "every ClassDB-registered SceneMultiplayer method must return a value; \
         an UndefinedFunction anywhere aborts the script before this line"
    );
    assert_eq!(
        script_var(&ml, node, "peers_after"),
        Variant::Int(0),
        "clear() leaves no connected peers"
    );
}

/// `disconnect_peer(id)` really removes the peer from `get_peers()`.
#[test]
fn disconnect_peer_removes_it_from_get_peers() {
    const SRC: &str = "\
extends Node
var before = 0
var after = 0
func _ready():
    self.before = multiplayer.get_peers().size()
    multiplayer.disconnect_peer(2)
    self.after = multiplayer.get_peers().size()
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Net", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(SRC, node).expect("script parses")),
    );
    // Two peers known to the API before the script runs, so the removal is
    // observable rather than a no-op on an empty set.
    tree.get_multiplayer_mut().peer_state_mut().add_peer(2);
    tree.get_multiplayer_mut().peer_state_mut().add_peer(3);

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(node);

    assert_eq!(
        script_var(&ml, node, "before"),
        Variant::Int(2),
        "both peers must be present first, or the removal proves nothing"
    );
    assert_eq!(
        script_var(&ml, node, "after"),
        Variant::Int(1),
        "disconnect_peer(2) must remove exactly that peer from get_peers()"
    );
}

/// The auth callback runs when a peer starts authenticating — before
/// `peer_connected`, which Godot only fires once the script accepts.
#[test]
fn auth_callback_runs_before_peer_connected() {
    const SRC: &str = "\
extends Node
var auth_id = 0
var auth_ran_first = false
var joined = 0
func _ready():
    var p = OfflineMultiplayerPeer.new()
    multiplayer.multiplayer_peer = p
    multiplayer.set_auth_callback(_on_auth)
    multiplayer.peer_connected.connect(_on_peer)
func _on_auth(id, data):
    self.auth_id = id
    if self.joined == 0:
        self.auth_ran_first = true
func _on_peer(id):
    self.joined = id
";
    let (mut ml, node) = scripted_loop(SRC);

    // With auth required, a joining peer produces `PeerAuthenticating` instead
    // of `PeerConnected` — which is exactly the branch the callback guards.
    ml.tree_mut().get_multiplayer_mut().set_auth_required(true);
    ml.tree_mut()
        .get_multiplayer_mut()
        .peer_state_mut()
        .add_peer(5);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "auth_id"),
        Variant::Int(5),
        "set_auth_callback must actually be invoked when a peer authenticates — \
         it had no effect whatsoever before this bead"
    );
    assert_eq!(
        script_var(&ml, node, "auth_ran_first"),
        Variant::Bool(true),
        "and must run BEFORE peer_connected, which is what makes it a decision \
         point rather than a notification"
    );
}
