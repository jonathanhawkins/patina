//! pat-nzgdu.30 — Object metadata + dynamic property access from GDScript.
//!
//! Before this bead, the interpreter's builtin/ObjectId dispatch matched a fixed
//! set of names and fell through to `UndefinedFunction` for `set_meta`/`get_meta`/
//! `has_meta` and the dynamic `set(property, value)`/`get(property)` accessors —
//! all of which are core `Object` API used by plugins, save systems, and generic
//! tooling. The fix wires them through the `SceneAccess` trait (against the Node's
//! meta bag and property bag) and dispatches them both as bare calls (`try_builtin`)
//! and as method calls (the `Variant::ObjectId` arm).
//!
//! This app-level test drives a scripted node through `MainLoop::step` and asserts
//! the full path: the script's `set_meta`/`get_meta`/`has_meta` reflect through to
//! the node's stored meta, and `set`/`get` round-trip a dynamically-set property —
//! with no `UndefinedFunction` raised.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

#[test]
fn object_metadata_and_dynamic_props_reflect_to_node() {
    // Scene: root -> Main (script). The script drives the Object meta + dynamic
    // set/get API against its own node.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node");
    let main_id = tree.add_child(root, main).unwrap();

    // Frame 1: exercise set_meta/get_meta/has_meta and dynamic set/get, recording
    // the read-back values into script vars so the Rust side can assert them.
    // (`+=` avoided for parser-independence, matching the sibling dispatch tests.)
    let script_src = "\
extends Node
var frame = 0
var meta_val = 0
var meta_present = false
var meta_absent = true
var prop_readback = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        set_meta(\"hp\", 7)
        self.meta_val = get_meta(\"hp\")
        self.meta_present = has_meta(\"hp\")
        self.meta_absent = has_meta(\"mana\")
        set(\"score\", 42)
        self.prop_readback = get(\"score\")
";
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    // One frame runs `_process` frame 1; no UndefinedFunction must be raised.
    ml.step(1.0 / 60.0);

    // The script observed its own meta + dynamic property through get_meta/get.
    let script = ml.tree().get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("meta_val"),
        Some(Variant::Int(7)),
        "get_meta(\"hp\") must return the value written by set_meta",
    );
    assert_eq!(
        script.get_property("meta_present"),
        Some(Variant::Bool(true)),
        "has_meta(\"hp\") must be true after set_meta",
    );
    assert_eq!(
        script.get_property("meta_absent"),
        Some(Variant::Bool(false)),
        "has_meta(\"mana\") must be false when never set",
    );
    assert_eq!(
        script.get_property("prop_readback"),
        Some(Variant::Int(42)),
        "get(\"score\") must round-trip the value written by set(\"score\", 42)",
    );

    // Node-level reflection: set_meta stored the entry on the node's own meta bag
    // (a namespace distinct from properties), not merely in the script.
    let node = ml.tree().get_node(main_id).expect("node exists");
    assert_eq!(
        node.get_meta("hp"),
        Variant::Int(7),
        "the node's stored meta must reflect set_meta after the frame",
    );
    assert!(
        node.has_meta("hp"),
        "the node must report has_meta(\"hp\") == true",
    );
    assert!(
        !node.has_meta("mana"),
        "the node must not report meta that was never set",
    );
}
