//! `SceneTree::drain_script_output` — the one place a host reads a running
//! game's stdout and runtime errors from.
//!
//! Before it existed, `print()` in a played scene reached nobody outside the
//! tests and a script call that failed went to `tracing::debug!` only, so a
//! `_process` that threw simply did nothing, visibly to no one.

use gdscene::node::Node;
use gdscene::scene_tree::SCRIPT_ERROR_LOG_CAP;
use gdscene::{GDScriptNodeInstance, LifecycleManager, SceneTree};

const SCRIPT: &str = "\
extends Node

func _ready():
    print(\"hello \", 1)
    push_error(\"bad thing\")

func explode():
    self.no_such_method()
";

fn boot() -> (SceneTree, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Scripted", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(SCRIPT, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    LifecycleManager::enter_tree(&mut tree, id);
    (tree, id)
}

#[test]
fn drain_returns_prints_errors_and_failed_calls_once() {
    let (mut tree, id) = boot();
    tree.call_script_method(id, "explode", &[]);
    let out = tree.drain_script_output();
    assert_eq!(out.prints.len(), 1, "{out:?}");
    assert!(out.prints[0].starts_with("hello"), "{out:?}");
    assert!(
        out.errors.iter().any(|e| e.contains("bad thing")),
        "push_error: {out:?}"
    );
    assert!(
        out.errors
            .iter()
            .any(|e| e.starts_with("/root/Scripted: explode(): ") && e.contains("no_such_method")),
        "a failed call names the node and method: {out:?}"
    );
    let again = tree.drain_script_output();
    assert!(
        again.prints.is_empty() && again.errors.is_empty(),
        "drained once: {again:?}"
    );
}

#[test]
fn the_failed_call_log_is_capped() {
    let (mut tree, id) = boot();
    for _ in 0..(SCRIPT_ERROR_LOG_CAP + 25) {
        tree.call_script_method(id, "explode", &[]);
    }
    let errors = tree.drain_script_errors();
    assert_eq!(
        errors.len(),
        SCRIPT_ERROR_LOG_CAP,
        "a script failing every frame must not grow the log without bound"
    );
}
