//! pat-s3rrv.78 — `HTTPRequest.get_http_client_status()` from a script.
//!
//! Godot ground truth: `HTTPRequest::get_http_client_status()` returns
//! `client->get_status()`, an `HTTPClient.Status`. The ordinals are positional
//! in `core/io/http_client.h`'s `enum Status`: DISCONNECTED 0, RESOLVING 1,
//! CANT_RESOLVE 2, CONNECTING 3, CANT_CONNECT 4, CONNECTED 5, REQUESTING 6,
//! BODY 7, CONNECTION_ERROR 8, TLS_HANDSHAKE_ERROR 9.
//!
//! 🔑 This method was **registered in ClassDB with no dispatch arm**, which is a
//! worse state than being absent: `has_method("get_http_client_status")`
//! answered `true`, and the call returned `Nil`. A script writing
//! `if r.get_http_client_status() == HTTPClient.STATUS_DISCONNECTED:` compared
//! against `Nil` and got `false` **even when idle**, with no error to debug.
//!
//! ⚠️ Only DISCONNECTED and REQUESTING are reachable. `HttpClient::perform` is a
//! blocking call on a worker thread — it walks the intermediate states
//! internally and surfaces nothing until done, so RESOLVING/CONNECTING/BODY are
//! not observable from the node at all. These tests pin the two honest
//! endpoints; they deliberately do NOT assert that the others never appear,
//! because a future streaming transport should be free to report them.

use gdscene::http_request_node::{STATUS_DISCONNECTED, STATUS_REQUESTING};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A tree with one HTTPRequest child named `Http`, plus `src` attached to root.
///
/// Returns `(tree, root, http)` — the child id comes straight from `add_child`
/// rather than a later lookup, so the tests never depend on name resolution.
fn tree_with_script(src: &str) -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");
    tree.attach_script(
        root,
        Box::new(gdscene::GDScriptNodeInstance::from_source(src, root).expect("script parses")),
    );
    (tree, root, http)
}

fn script_var(tree: &SceneTree, root: NodeId, name: &str) -> Variant {
    tree.get_script(root)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// The ordinals are transcribed from the C++ enum, so pin them here too: a
/// silent renumbering would otherwise only show up as a wrong status at runtime.
#[test]
fn the_status_ordinals_match_the_godot_enum() {
    assert_eq!(STATUS_DISCONNECTED, 0);
    assert_eq!(STATUS_REQUESTING, 6);
}

/// 🔑 The regression this arm exists for: an idle node reports DISCONNECTED,
/// **not** `Nil`.
///
/// ⚠️ The `Variant::Int` check is the whole point. Before the arm existed the
/// call returned `Variant::Nil`, and `Nil == 0` is false in GDScript, so a
/// script's idle check silently never fired. Asserting the value alone would not
/// catch a regression back to `Nil` unless the type is asserted with it.
#[test]
fn an_idle_node_reports_disconnected_rather_than_nil() {
    let (mut tree, root, _http) = tree_with_script(
        "extends Node\nvar st = null\nfunc run():\n    self.st = get_node(\"Http\").get_http_client_status()\n",
    );
    assert!(
        tree.call_script_method(root, "run", &[]).is_some(),
        "run() raised — get_http_client_status did not resolve on the node"
    );
    assert_eq!(
        script_var(&tree, root, "st"),
        Variant::Int(STATUS_DISCONNECTED),
        "an idle HTTPRequest must report STATUS_DISCONNECTED as an Int"
    );
}

/// The same call twice must not drift — a getter has no side effects.
///
/// ⚠️ Specifically guards the lazy-allocation trap: `http_request_for` is
/// `entry().or_default()`, so an unguarded getter would create per-node
/// transport state on first read and could answer differently afterwards.
#[test]
fn polling_the_status_is_side_effect_free() {
    let (mut tree, root, _http) = tree_with_script(
        "extends Node\nvar a = null\nvar b = null\nfunc run():\n    var r = get_node(\"Http\")\n    self.a = r.get_http_client_status()\n    self.b = r.get_http_client_status()\n",
    );
    assert!(tree.call_script_method(root, "run", &[]).is_some());
    assert_eq!(
        script_var(&tree, root, "a"),
        Variant::Int(STATUS_DISCONNECTED)
    );
    assert_eq!(
        script_var(&tree, root, "b"),
        Variant::Int(STATUS_DISCONNECTED),
        "a getter must not change what it reports by being called"
    );
}

/// …and it stays side-effect free at the tree level: merely polling must not
/// allocate the per-node HTTP state.
#[test]
fn polling_the_status_does_not_create_transport_state() {
    let (mut tree, root, http) = tree_with_script(
        "extends Node\nfunc run():\n    get_node(\"Http\").get_http_client_status()\n",
    );
    assert!(
        !tree.has_http_request(http),
        "precondition: no HTTP state before anything runs"
    );
    assert!(tree.call_script_method(root, "run", &[]).is_some());
    assert!(
        !tree.has_http_request(http),
        "a getter must not lazily allocate per-node transport state"
    );
}

/// A node that never called `request()` is still DISCONNECTED after a frame —
/// the poll pass must not invent a status either.
#[test]
fn a_frame_tick_does_not_change_an_idle_status() {
    let (mut tree, root, _http) = tree_with_script(
        "extends Node\nvar st = null\nfunc run():\n    self.st = get_node(\"Http\").get_http_client_status()\n",
    );
    tree.process_http_requests();
    assert!(tree.call_script_method(root, "run", &[]).is_some());
    assert_eq!(
        script_var(&tree, root, "st"),
        Variant::Int(STATUS_DISCONNECTED)
    );
}
