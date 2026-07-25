//! pat-0q2kx.65 — a `.tscn` `[connection]`'s `flags`, `binds` and `unbinds` are
//! applied when the scene is instantiated.
//!
//! The parser already captured `flags=` and `binds=` into `SceneConnection`, and
//! `Connection` already supported deferred/one-shot/binds/unbinds. But
//! `wire_connections` built a bare `Connection::new` and dropped all of it, and
//! `unbinds=` was never parsed at all. The visible result: a `flags=1`
//! (DEFERRED) connection fired synchronously inside `emit_signal` instead of at
//! the frame's deferred flush, a `flags=4` (ONE_SHOT) connection fired on every
//! emission forever, and bound arguments never reached the callback.
//!
//! Each test authors a real `.tscn`, loads it, and asserts on script state after
//! emitting — so a regression to "parsed but ignored" fails here.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Records every call and the first argument it was handed, so the tests can
/// assert on *when* the handler ran and *what* it received.
const LISTENER_GD: &str = "\
extends Node
var calls = 0
var last = -1
var arg_count = -1
func _on_hit(a = -1, b = -1):
    self.calls = self.calls + 1
    self.last = a
    if b == -1:
        self.arg_count = 1
    else:
        self.arg_count = 2
";

/// Builds a scene whose root emits `hit` to a `L` child, with the given
/// attributes on the `[connection]` line.
fn scene_source(conn_attrs: &str) -> String {
    format!(
        "[gd_scene format=3]\n\
         \n\
         [node name=\"Root\" type=\"Node\"]\n\
         \n\
         [node name=\"L\" type=\"Node\" parent=\".\"]\n\
         \n\
         [connection signal=\"hit\" from=\".\" to=\"L\" method=\"_on_hit\"{conn_attrs}]\n"
    )
}

/// Loads the scene, attaches the listener script to `L`, and returns the tree
/// plus the root and listener ids.
fn load_scene(conn_attrs: &str) -> (SceneTree, NodeId, NodeId) {
    let scene = PackedScene::from_tscn(&scene_source(conn_attrs)).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("scene instantiates");
    let listener = tree
        .get_node_relative(root, "L")
        .expect("the L child exists");

    let script = GDScriptNodeInstance::from_source(LISTENER_GD, listener).expect("script parses");
    tree.attach_script(listener, Box::new(script));
    (tree, root, listener)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// The bead's headline: `flags=1 binds=[42]` defers the call to the flush and
/// appends the bound argument.
#[test]
fn tscn_connection_flags_binds_applied() {
    let (mut tree, root, listener) = load_scene(" flags=1 binds=[42]");

    tree.emit_signal(root, "hit", &[]);
    assert_eq!(
        script_var(&tree, listener, "calls"),
        Variant::Int(0),
        "a DEFERRED connection must NOT run inside emit_signal"
    );

    tree.process_deferred_calls();
    assert_eq!(
        script_var(&tree, listener, "calls"),
        Variant::Int(1),
        "the deferred call must run at the flush"
    );
    assert_eq!(
        script_var(&tree, listener, "last"),
        Variant::Int(42),
        "the authored bind must reach the callback"
    );
}

/// `flags=4` (ONE_SHOT) disconnects after the first emission.
#[test]
fn one_shot_connection_fires_exactly_once() {
    let (mut tree, root, listener) = load_scene(" flags=4");

    tree.emit_signal(root, "hit", &[]);
    tree.emit_signal(root, "hit", &[]);
    tree.process_deferred_calls();

    assert_eq!(
        script_var(&tree, listener, "calls"),
        Variant::Int(1),
        "a ONE_SHOT connection must fire once across two emissions"
    );
}

/// `unbinds=1` drops the trailing signal argument before the callback sees it.
#[test]
fn unbinds_drops_the_trailing_signal_argument() {
    let (mut tree, root, listener) = load_scene(" unbinds=1");

    tree.emit_signal(root, "hit", &[Variant::Int(7), Variant::Int(9)]);
    tree.process_deferred_calls();

    assert_eq!(
        script_var(&tree, listener, "calls"),
        Variant::Int(1),
        "the connection still fires"
    );
    assert_eq!(
        script_var(&tree, listener, "arg_count"),
        Variant::Int(1),
        "unbinds=1 must drop the trailing arg, leaving one"
    );
    assert_eq!(
        script_var(&tree, listener, "last"),
        Variant::Int(7),
        "the surviving argument is the leading one"
    );
}

/// A plain connection is unaffected — it still fires synchronously, every time,
/// with the emitted arguments untouched.
#[test]
fn plain_connection_is_unchanged() {
    let (mut tree, root, listener) = load_scene("");

    tree.emit_signal(root, "hit", &[Variant::Int(5)]);
    assert_eq!(
        script_var(&tree, listener, "calls"),
        Variant::Int(1),
        "an unflagged connection fires synchronously"
    );
    assert_eq!(script_var(&tree, listener, "last"), Variant::Int(5));

    tree.emit_signal(root, "hit", &[Variant::Int(6)]);
    assert_eq!(
        script_var(&tree, listener, "calls"),
        Variant::Int(2),
        "and it is not one-shot"
    );
}

/// Saving re-emits the attributes, so a load → save round trip keeps the
/// behavior the scene was authored with.
#[test]
fn saving_round_trips_flags_binds_and_unbinds() {
    let (tree, root, _listener) = load_scene(" flags=5 binds=[42] unbinds=1");
    let saved = TscnSaver::save_tree(&tree, root);

    let line = saved
        .lines()
        .find(|l| l.starts_with("[connection"))
        .expect("a [connection] section is written");

    assert!(
        line.contains("flags=5"),
        "DEFERRED|ONE_SHOT must survive the round trip: {line}"
    );
    assert!(
        line.contains("binds=[42]"),
        "binds must survive the round trip: {line}"
    );
    assert!(
        line.contains("unbinds=1"),
        "unbinds must survive the round trip: {line}"
    );
}

/// ...and a connection with nothing to say still writes the plain form, so
/// ordinary scenes do not churn.
#[test]
fn saving_omits_empty_connection_attributes() {
    let (tree, root, _listener) = load_scene("");
    let saved = TscnSaver::save_tree(&tree, root);

    let line = saved
        .lines()
        .find(|l| l.starts_with("[connection"))
        .expect("a [connection] section is written");

    assert!(
        !line.contains("flags="),
        "no flags on a plain connection: {line}"
    );
    assert!(
        !line.contains("binds="),
        "no binds on a plain connection: {line}"
    );
}
