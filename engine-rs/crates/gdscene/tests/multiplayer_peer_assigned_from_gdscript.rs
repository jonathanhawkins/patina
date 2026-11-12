//! Acceptance test for pat-s3rrv.16: a `MultiplayerPeer` constructed and
//! assigned entirely from GDScript.
//!
//! Godot ground truth: every multiplayer project opens with three lines —
//!
//! ```gdscript
//! var peer = WebSocketMultiplayerPeer.new()
//! peer.create_server(8080)
//! multiplayer.multiplayer_peer = peer
//! ```
//!
//! Patina's transports were real and complete (`OfflineMultiplayerPeer`,
//! `WebSocketMultiplayerPeer`, both implementing `NetworkPeer`), but nothing in
//! a script could reach them: no `MultiplayerPeer` subclass was constructible
//! (`grep '"WebSocketMultiplayerPeer"' interpreter.rs` → 0),
//! `call_multiplayer_method` had no `set_multiplayer_peer`, and `multiplayer`
//! was only a method-call receiver so `multiplayer.multiplayer_peer = peer`
//! could not even parse to anything. Every green multiplayer test injected the
//! peer from RUST.
//!
//! **This test contains no Rust call to `set_multiplayer_peer`** — that is the
//! whole point. The peer must arrive through the script.

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
    // `_ready` runs through the host-driven lifecycle, exactly as a loaded
    // scene's would.
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

/// **The acceptance test.** `OfflineMultiplayerPeer.new()` assigned to
/// `multiplayer.multiplayer_peer` in `_ready()` makes `multiplayer.is_server()`
/// and `get_unique_id()` report the peer's identity from `_process()`.
#[test]
fn multiplayer_peer_assigned_from_gdscript() {
    const SRC: &str = "\
extends Node
var had_peer_before = true
var is_server = false
var unique_id = 0
var has_peer = false
func _ready():
    self.had_peer_before = multiplayer.has_multiplayer_peer()
    var p = OfflineMultiplayerPeer.new()
    multiplayer.multiplayer_peer = p
func _process(delta):
    self.is_server = multiplayer.is_server()
    self.unique_id = multiplayer.get_unique_id()
    self.has_peer = multiplayer.has_multiplayer_peer()
";
    let (mut ml, node) = scripted_loop(SRC);

    // The control: before the assignment the tree had no peer, so the values
    // below cannot be a default that happens to look right.
    assert_eq!(
        script_var(&ml, node, "had_peer_before"),
        Variant::Bool(false),
        "the tree must start with NO multiplayer peer, or this test proves nothing"
    );

    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "has_peer"),
        Variant::Bool(true),
        "`multiplayer.multiplayer_peer = p` from _ready must install the peer \
         into the tree's MultiplayerAPI"
    );
    assert_eq!(
        script_var(&ml, node, "is_server"),
        Variant::Bool(true),
        "the OfflineMultiplayerPeer is always the server"
    );
    assert_eq!(
        script_var(&ml, node, "unique_id"),
        Variant::Int(1),
        "and always has unique id 1"
    );
}

/// The `WebSocketMultiplayerPeer` half: constructed and `create_server()`-ed
/// from GDScript, it reports `CONNECTION_CONNECTED` (2).
#[test]
fn websocket_multiplayer_peer_create_server_from_gdscript() {
    const SRC: &str = "\
extends Node
var err = -1
var status = -1
var uid = 0
func _ready():
    var p = WebSocketMultiplayerPeer.new()
    self.err = p.create_server(0)
    self.status = p.get_connection_status()
    self.uid = p.get_unique_id()
    multiplayer.multiplayer_peer = p
";
    let (ml, node) = scripted_loop(SRC);

    assert_eq!(
        script_var(&ml, node, "err"),
        Variant::Int(0),
        "create_server must return OK (0); port 0 asks the OS for a free port"
    );
    assert_eq!(
        script_var(&ml, node, "status"),
        Variant::Int(2),
        "a listening server peer reports CONNECTION_CONNECTED (2)"
    );
    assert_eq!(
        script_var(&ml, node, "uid"),
        Variant::Int(1),
        "the server's unique id is 1"
    );
}

/// The peer really left the handle table and is owned by the tree: assigning
/// the SAME handle a second time cannot install a second peer.
#[test]
fn assigning_a_peer_twice_does_not_double_install() {
    const SRC: &str = "\
extends Node
var first = false
var second = false
func _ready():
    var p = OfflineMultiplayerPeer.new()
    multiplayer.multiplayer_peer = p
    self.first = multiplayer.has_multiplayer_peer()
    multiplayer.multiplayer_peer = p
    self.second = multiplayer.has_multiplayer_peer()
";
    let (ml, node) = scripted_loop(SRC);

    assert_eq!(
        script_var(&ml, node, "first"),
        Variant::Bool(true),
        "the first assignment installs the peer"
    );
    // The tree still has the peer it adopted; the second assignment is a no-op
    // rather than an error or a swap with an empty slot.
    assert_eq!(
        script_var(&ml, node, "second"),
        Variant::Bool(true),
        "re-assigning a consumed handle must leave the installed peer alone"
    );
}

/// The method spelling Godot also exposes works alongside the property.
#[test]
fn set_multiplayer_peer_method_spelling_also_installs() {
    const SRC: &str = "\
extends Node
var has_peer = false
func _ready():
    var p = OfflineMultiplayerPeer.new()
    multiplayer.set_multiplayer_peer(p)
    self.has_peer = multiplayer.has_multiplayer_peer()
";
    let (ml, node) = scripted_loop(SRC);
    assert_eq!(
        script_var(&ml, node, "has_peer"),
        Variant::Bool(true),
        "`multiplayer.set_multiplayer_peer(p)` must install the peer too"
    );
}

/// A peer handle answers its own getters before it is ever assigned — the
/// script can inspect the transport it just built.
#[test]
fn a_fresh_peer_answers_its_getters_before_assignment() {
    const SRC: &str = "\
extends Node
var uid = -1
var server = false
func _ready():
    var p = OfflineMultiplayerPeer.new()
    self.uid = p.get_unique_id()
    self.server = p.is_server()
";
    let (ml, node) = scripted_loop(SRC);
    assert_eq!(
        script_var(&ml, node, "uid"),
        Variant::Int(1),
        "the peer object itself reports its unique id"
    );
    assert_eq!(
        script_var(&ml, node, "server"),
        Variant::Bool(true),
        "and its server role"
    );
}
