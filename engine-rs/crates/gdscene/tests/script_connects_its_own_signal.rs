//! A script connecting a signal it declares itself, from its own `_ready` —
//! the most common way GDScript wires signals (`pinged.connect(_on_pinged)`).
//!
//! The connect pre-check (`node_has_signal`, which answers Godot's
//! `ERR_INVALID_PARAMETER` for a signal the object does not have) looked the
//! script up through `SceneTree::get_script`. A script is OUT of the tree's
//! script map while one of its own methods runs, so from inside `_ready` its own
//! `signal` declarations were invisible: every such connect returned 31 and the
//! handler never fired.

use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, LifecycleManager, SceneTree};
use gdvariant::Variant;

const SCRIPT: &str = "\
extends Node

signal pinged(n)
var got := 0
var connect_code := -1

func _ready():
    connect_code = pinged.connect(_on_pinged)

func _on_pinged(n):
    got += n
";

#[test]
fn a_script_connects_its_own_signal_from_ready() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Emitter", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(SCRIPT, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    LifecycleManager::enter_tree(&mut tree, id);

    let read = |tree: &SceneTree, name: &str| tree.get_script(id).unwrap().get_property(name);
    assert_eq!(
        read(&tree, "connect_code"),
        Some(Variant::Int(0)),
        "connect() to a signal the script declares must return OK"
    );

    // Emitted from outside the script, after `_ready` has returned.
    tree.emit_signal(id, "pinged", &[Variant::Int(5)]);
    assert_eq!(
        read(&tree, "got"),
        Some(Variant::Int(5)),
        "the method handler ran"
    );
    assert_eq!(
        tree.drain_script_output().errors,
        Vec::<String>::new(),
        "no script call failed"
    );
}

/// The pre-check must still refuse a signal nobody declares, as Godot does.
#[test]
fn connecting_an_undeclared_signal_is_still_refused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Emitter", "Node")).unwrap();
    let src = "extends Node\nvar code := -1\nfunc _ready():\n    code = connect(\"no_such_signal\", _ready)\n";
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    LifecycleManager::enter_tree(&mut tree, id);
    assert_eq!(
        tree.get_script(id).unwrap().get_property("code"),
        Some(Variant::Int(31)),
        "ERR_INVALID_PARAMETER for a signal the object does not have"
    );
}
