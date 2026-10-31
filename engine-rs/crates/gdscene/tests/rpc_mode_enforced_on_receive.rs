//! pat-mgixk.26 (item 3): the RECEIVER enforces `@rpc` config and `RPCMode`.
//!
//! Godot's `_process_rpc` (`scene_rpc_interface.cpp`) checks an inbound call
//! before running it: a method with no `@rpc` config is dropped, and an
//! `RPC_MODE_AUTHORITY` method is dropped unless the sender IS the node's
//! multiplayer authority.
//!
//! ⚠️ Patina ran **every** inbound packet unconditionally
//! (`SceneTree::poll_multiplayer_with_delta` called `call_script_with_access`
//! straight from the decoded packet). So any connected peer could invoke **any
//! script method on any node**, annotated or not. In a shipped multiplayer game
//! that is not a parity gap, it is remote code execution — and it is invisible
//! to a test that only ever sends well-formed calls from a cooperating peer,
//! which is why it survived `rpc_annotation_end_to_end_dispatch`.
//!
//! The enforcement is deliberately on the **receive** side, as in Godot: it is
//! the receiver's own `@rpc` table that decides, never the sender's. That is
//! what makes these tests possible — peer A can be given a script that happily
//! sends a call peer B refuses to run.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// The SENDER's script: every method is `any_peer`, so A will happily send
/// all three calls. What B does with them is the point.
const SENDER: &str = "\
extends Node
var ran = 0

@rpc(\"any_peer\", \"reliable\")
func open_method(n):
    self.ran = self.ran + n

@rpc(\"any_peer\", \"reliable\")
func unannotated_on_receiver(n):
    self.ran = self.ran + n

@rpc(\"any_peer\", \"reliable\")
func authority_only(n):
    self.ran = self.ran + n

func fire_open():
    rpc(\"open_method\", 1)

func fire_unannotated():
    rpc(\"unannotated_on_receiver\", 1)

func fire_authority():
    rpc(\"authority_only\", 1)
";

/// The RECEIVER's script. Note what is missing and what differs:
///
/// * `open_method` is `any_peer` -> must RUN.
/// * `unannotated_on_receiver` has **no annotation** -> must be DROPPED.
/// * `authority_only` is a bare `@rpc`, which registers as
///   `RPCMode::Authority` (Godot's default) -> must be DROPPED unless the
///   sender is this node's authority.
const RECEIVER: &str = "\
extends Node
var open_hits = 0
var unannotated_hits = 0
var authority_hits = 0

@rpc(\"any_peer\", \"reliable\")
func open_method(n):
    self.open_hits = self.open_hits + n

func unannotated_on_receiver(n):
    self.unannotated_hits = self.unannotated_hits + n

@rpc(\"reliable\")
func authority_only(n):
    self.authority_hits = self.authority_hits + n
";

fn peer_tree(script_src: &str) -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // ⚠️ Both peers' nodes must sit at the same path (`/root/Player`): Godot
    // addresses an RPC by node path, and the inbound hash is what resolves to
    // the receiver's matching node.
    let id = tree.add_child(root, Node::new("Player", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(script_src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    (tree, id)
}

fn prop(tree: &SceneTree, id: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    tree.get_script(id).and_then(|s| s.get_property(name))
}

#[test]
fn rpc_mode_enforced_on_receive() {
    let (mut tree_a, node_a) = peer_tree(SENDER);
    let (mut tree_b, node_b) = peer_tree(RECEIVER);

    // A is the server (peer 1); B is the client (peer 2).
    let (server, client) = MockNetwork::create_pair(PeerId(2));
    tree_a
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));
    tree_b
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client));

    // ⚠️ Make B's node answer to peer 2, i.e. B ITSELF — so the server's calls
    // are NOT from the authority. Left at the default this node would answer
    // to peer 1 and the authority gate would (correctly) let the server
    // through, and the third assertion below would prove nothing.
    tree_b
        .get_node_mut(node_b)
        .expect("receiver node")
        .set_property("multiplayer_authority", Variant::Int(2));

    // -- 1. an `any_peer` method still runs: the gate must not block the
    //       legitimate case, which is the regression this guards --------------
    tree_a.call_script_method(node_a, "fire_open", &[]);
    // Pump the SENDER's side too: a real frame polls both ends, and the
    // outbound packet is only on the wire once A has been polled.
    tree_a.poll_multiplayer();
    tree_b.poll_multiplayer();
    assert_eq!(
        prop(&tree_b, node_b, "open_hits"),
        Some(Variant::Int(1)),
        "an @rpc(\"any_peer\") method must still be dispatched — a gate that \
         blocks everything would pass the two drop assertions below"
    );

    // -- 2. a method the RECEIVER never annotated is dropped -----------------
    tree_a.call_script_method(node_a, "fire_unannotated", &[]);
    // Pump the SENDER's side too: a real frame polls both ends, and the
    // outbound packet is only on the wire once A has been polled.
    tree_a.poll_multiplayer();
    tree_b.poll_multiplayer();
    assert_eq!(
        prop(&tree_b, node_b, "unannotated_hits"),
        Some(Variant::Int(0)),
        "a method with no @rpc config on the RECEIVER must be dropped. It ran \
         before this change, which let any peer invoke any script method on \
         any node"
    );

    // -- 3. an authority-only method from a non-authority sender is dropped --
    tree_a.call_script_method(node_a, "fire_authority", &[]);
    // Pump the SENDER's side too: a real frame polls both ends, and the
    // outbound packet is only on the wire once A has been polled.
    tree_a.poll_multiplayer();
    tree_b.poll_multiplayer();
    assert_eq!(
        prop(&tree_b, node_b, "authority_hits"),
        Some(Variant::Int(0)),
        "a bare @rpc registers as RPCMode::Authority, so a call from peer 1 to \
         a node whose authority is peer 2 must be dropped"
    );

    // -- 4. ...and the same call IS accepted once the sender is the authority,
    //       so the gate is checking the sender and not simply refusing
    //       authority-mode methods outright ---------------------------------
    tree_b
        .get_node_mut(node_b)
        .expect("receiver node")
        .set_property("multiplayer_authority", Variant::Int(1));
    tree_a.call_script_method(node_a, "fire_authority", &[]);
    // Pump the SENDER's side too: a real frame polls both ends, and the
    // outbound packet is only on the wire once A has been polled.
    tree_a.poll_multiplayer();
    tree_b.poll_multiplayer();
    assert_eq!(
        prop(&tree_b, node_b, "authority_hits"),
        Some(Variant::Int(1)),
        "with the node's authority set to the sending peer the SAME call must \
         now run — otherwise the gate is rejecting the mode, not the sender"
    );
}
