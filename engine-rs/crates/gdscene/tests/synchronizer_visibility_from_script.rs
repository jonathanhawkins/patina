//! pat-s3rrv.17 — per-peer visibility reachable from GDScript.
//!
//! # What was wrong
//!
//! Both halves of this feature existed and could not meet. The per-peer state
//! lives on the synchronizer (`set_visibility_for` / `is_visible_to`, with unit
//! tests), and `poll_outbound` already refuses to send to a peer it is hidden
//! from. But nothing could reach either from a script: `grep -rn "is_visible_to"
//! crates/gdscene/src` returned nothing at all.
//!
//! So `set_visibility_for(peer, false)` — the call a project makes to stop
//! broadcasting an entity to peers who cannot see it — did nothing.
//!
//! # The peer-0 rule
//!
//! Godot documents `set_visibility_for` as: "If [param peer] is `0`, the value
//! of [member public_visibility] will be updated instead."
//! (`doc_classes/MultiplayerSynchronizer.xml`.) Peer 0 is not an id. An
//! implementation that stores it as one looks correct — the call succeeds and a
//! map entry appears — while the default it was meant to change is untouched.
//! The second test pins exactly that.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A Player with a scripted `MultiplayerSynchronizer` child, a peer attached so
/// the tree's replication registry is driven exactly as in a real game.
fn scripted_sync_loop(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player = tree
        .add_child(root, Node::new("Player", "Node2D"))
        .expect("player added");
    if let Some(n) = tree.get_node_mut(player) {
        n.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    }

    let sync = tree
        .add_child(player, Node::new("Sync", "MultiplayerSynchronizer"))
        .expect("synchronizer added");
    if let Some(n) = tree.get_node_mut(sync) {
        n.set_property("root_path", Variant::String("..".to_string()));
        n.set_property("replication_interval", Variant::Float(0.0));
        n.set_property(
            "_sync_properties",
            Variant::Array(vec![Variant::String("position".to_string())]),
        );
    }

    tree.attach_script(
        sync,
        Box::new(GDScriptNodeInstance::from_source(src, sync).expect("script parses")),
    );

    let (server, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(sync);
    (ml, sync)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// **The headline.** A script hides the synchronizer from one peer and reads
/// the decision back.
#[test]
fn a_script_can_hide_a_synchronizer_from_one_peer() {
    // ⚠️ Driven from `_process`, not `_ready`: the synchronizer is entered into
    // the replication registry by the tree's own poll, so a `_ready` call could
    // land before it is registered and be a legitimate no-op.
    const SRC: &str = "\
extends Node
var ticks = 0
var v2 = true
var v3 = false
func _process(_delta):
    self.ticks = self.ticks + 1
    set_visibility_for(2, false)
    self.v2 = is_visible_to(2)
    self.v3 = is_visible_to(3)
";
    let (mut ml, sync) = scripted_sync_loop(SRC);
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    // ⚠️ Prove the script actually ran first. Every assertion below reads a
    // script var, and a var sitting at its initialiser is indistinguishable
    // from a var the engine deliberately set — so without this a script that
    // never executed would look like a passing test.
    match script_var(&ml, sync, "ticks") {
        Variant::Int(n) if n > 0 => {}
        other => panic!("_process never ran (ticks = {other:?}); the rest of this test is vacuous"),
    }

    assert_eq!(
        script_var(&ml, sync, "v2"),
        Variant::Bool(false),
        "set_visibility_for(2, false) must reach the registered synchronizer. \
         `true` is the old behaviour: the call had no script route at all"
    );
    assert_eq!(
        script_var(&ml, sync, "v3"),
        Variant::Bool(true),
        "only the named peer is hidden — peer 3 keeps the public default"
    );
}

/// Godot's documented special case: peer `0` sets `public_visibility` rather
/// than storing an override for a peer id of zero.
///
/// ⚠️ An implementation that treats 0 as an ordinary id passes nothing here:
/// peer 5 has no override, so it would still read the untouched public default
/// of `true`.
#[test]
fn peer_zero_sets_the_public_default_instead_of_an_override() {
    const SRC: &str = "\
extends Node
var ticks = 0
var v5 = true
func _process(_delta):
    self.ticks = self.ticks + 1
    set_visibility_for(0, false)
    self.v5 = is_visible_to(5)
";
    let (mut ml, sync) = scripted_sync_loop(SRC);
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    match script_var(&ml, sync, "ticks") {
        Variant::Int(n) if n > 0 => {}
        other => panic!("_process never ran (ticks = {other:?}); the assertion below is vacuous"),
    }

    assert_eq!(
        script_var(&ml, sync, "v5"),
        Variant::Bool(false),
        "peer 0 means `public_visibility`, so a peer with no override of its \
         own must now read hidden. `true` means 0 was stored as a peer id"
    );
}
