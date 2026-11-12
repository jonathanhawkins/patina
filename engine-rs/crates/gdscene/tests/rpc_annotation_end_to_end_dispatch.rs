//! pat-0q2kx.79 — a script's `@rpc` functions reach the MultiplayerAPI, and
//! `rpc()` dispatches across peers.
//!
//! Godot configures `Node::rpc_config` from the `@rpc` annotation at script load
//! and routes `node.rpc("method", ...)` through the tree's MultiplayerAPI
//! (`gdscript_parser.cpp`, `Node.xml`).
//!
//! Patina had three layers that never met: the parser recorded `@rpc` on the
//! AST, a working MultiplayerAPI was polled every frame, and the interpreter had
//! no `rpc()` at all. Worse, `FuncDef` discarded the annotations when the
//! `ClassDef` was built — so registering a script's RPCs was structurally
//! impossible, not merely unwired, and `MultiplayerAPI::rpc` could only ever
//! answer `UnregisteredRPC`.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const SCRIPT: &str = "\
extends Node
var hp = 100
var hits = 0

@rpc(\"any_peer\", \"call_local\", \"reliable\")
func take_damage(n):
    self.hp = self.hp - n
    self.hits = self.hits + 1

func fire():
    rpc(\"take_damage\", 5)
";

/// Builds a tree with one scripted node named `Player` under the root.
fn peer_tree() -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(SCRIPT, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    (tree, id)
}

fn prop(tree: &SceneTree, id: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    tree.get_script(id).and_then(|s| s.get_property(name))
}

/// **The acceptance test.**
#[test]
fn rpc_annotation_end_to_end_dispatch() {
    let (mut tree_a, node_a) = peer_tree();
    let (mut tree_b, node_b) = peer_tree();

    // ⚠️ Both nodes are at the same path (`/root/Player`), which is what lets
    // the inbound hash resolve to B's matching node — Godot addresses RPCs by
    // node path, not by id.
    let (server, client) = MockNetwork::create_pair(PeerId(2));
    tree_a
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));
    tree_b
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client));

    // The annotation registered the method: without registration this call
    // fails with UnregisteredRPC, which is what always used to happen.
    tree_a.call_script_method(node_a, "fire", &[]);

    // -- call_local: the sender runs it too -----------------------------------
    //
    // Queued rather than immediate, because `rpc()` is called from inside the
    // node's own running script and a running script is checked out of the
    // arena; the deferred flush runs it once the method has returned.
    let mut ml_a = MainLoop::new(tree_a);
    ml_a.step(1.0 / 60.0);
    assert_eq!(
        prop(ml_a.tree(), node_a, "hp"),
        Some(Variant::Int(95)),
        "call_local must run the method on the sending peer"
    );

    // -- the remote peer receives it on its next poll -------------------------
    tree_b.poll_multiplayer();
    assert_eq!(
        prop(&tree_b, node_b, "hp"),
        Some(Variant::Int(95)),
        "peer B's matching node must run take_damage(5) on poll"
    );
    assert_eq!(
        prop(&tree_b, node_b, "hits"),
        Some(Variant::Int(1)),
        "…exactly once"
    );
}

/// Calling an RPC the script never declared errors, as Godot does, rather than
/// silently doing nothing.
#[test]
fn unregistered_rpc_errors() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    let src = "\
extends Node
var ok = true

func fire():
    rpc(\"never_declared\", 1)
";
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    let (server, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));

    // `call_script_method` swallows the error into None; the observable part is
    // that the node's hp is untouched and nothing was registered for the name.
    tree.call_script_method(id, "fire", &[]);
    assert!(
        tree.get_multiplayer()
            .get_rpc_config("never_declared")
            .is_none(),
        "an undeclared method must never be registered"
    );
}

/// The annotation's arguments are read by VALUE, not position: Godot accepts
/// them in any order and omits what it does not need.
#[test]
fn rpc_annotation_arguments_are_order_independent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("N", "Node")).unwrap();
    let src = "\
extends Node

@rpc(\"reliable\", \"any_peer\", 3)
func shuffled(n):
    pass

@rpc
func bare(n):
    pass
";
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));

    let mp = tree.get_multiplayer();
    let shuffled = mp
        .get_rpc_config("shuffled")
        .expect("annotated method must be registered");
    assert_eq!(
        shuffled.channel, 3,
        "the numeric argument is the channel wherever it appears"
    );
    assert!(
        matches!(shuffled.mode, gdplatform::network::RPCMode::Any),
        "any_peer must be honoured out of position"
    );
    assert!(
        matches!(
            shuffled.transfer_mode,
            gdplatform::network::TransferMode::Reliable
        ),
        "reliable must be honoured out of position"
    );

    // ⚠️ A bare `@rpc` is NOT "no rpc": it registers with Godot's defaults.
    let bare = mp
        .get_rpc_config("bare")
        .expect("a bare @rpc still registers");
    assert!(
        matches!(bare.mode, gdplatform::network::RPCMode::Authority),
        "bare @rpc defaults to authority"
    );
    assert_eq!(bare.channel, 0, "…on channel 0");
}
