//! pat-s3rrv.17 — `Node.set_multiplayer_authority()` and friends from GDScript.
//!
//! The authority model landed as a Rust API only, so a project could not assign
//! authority at runtime — and assigning it at runtime is the entire point:
//! a server spawns a player and hands that player's node to the peer who owns
//! it, which is how every Godot multiplayer project distributes control.
//!
//! ⚠️ Two things had to be true for the script call to resolve, and only one is
//! obvious. The accessor arm is the obvious half; the other is that
//! `class_has_method` consults **declared `MethodInfo` only**, so the three
//! methods also had to be registered on `Node` in ClassDB. Without that the arm
//! exists and `set_multiplayer_authority(2)` still fails to resolve — the same
//! trap the `Range` value model hit, where the behaviour was registered as
//! properties and the method never dispatched.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads authority, transfers it away, then reads it again — all in one frame,
/// because these calls are synchronous rather than queued like the `Input`
/// mutators.
const OWNER: &str = "\
extends Node
var frame = 0
var authority_before = -1
var mine_before = false
var authority_after = -1
var mine_after = true
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        self.authority_before = get_multiplayer_authority()
        self.mine_before = is_multiplayer_authority()
        set_multiplayer_authority(2)
        self.authority_after = get_multiplayer_authority()
        self.mine_after = is_multiplayer_authority()
";

fn var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

#[test]
fn a_script_can_read_and_transfer_multiplayer_authority() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Owner", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(OWNER, node).expect("script parses")),
    );

    // A peer is required for `is_multiplayer_authority()` to mean anything:
    // with none set every tree owns everything, so the before/after pair below
    // would read `true` twice and prove nothing.
    let (server, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        var(&ml, node, "authority_before"),
        Variant::Int(1),
        "an unauthored node reports the server as its authority — a -1 means the \
         script call never resolved, which is what a missing ClassDB MethodInfo \
         looks like from here"
    );
    assert_eq!(
        var(&ml, node, "mine_before"),
        Variant::Bool(true),
        "this peer IS the server, so it starts as the authority"
    );

    assert_eq!(
        var(&ml, node, "authority_after"),
        Variant::Int(2),
        "`set_multiplayer_authority(2)` must transfer ownership to peer 2"
    );
    // ⚠️ The load-bearing assertion: the SAME call returns true above and false
    // here. A binding that stored the value without consulting the peer would
    // satisfy `authority_after` and still report `true` here.
    assert_eq!(
        var(&ml, node, "mine_after"),
        Variant::Bool(false),
        "after handing the node to peer 2 this peer must no longer be its \
         authority"
    );
}
