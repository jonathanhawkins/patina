//! pat-s3rrv.8 — Array and Dictionary are reference types, as in Godot.
//!
//! Godot's `Array`/`Dictionary` are handles to refcounted storage
//! (core/variant/array.h, dictionary.h; doc/classes/Array.xml: "Arrays are always
//! passed by reference"). Patina modelled both as value types and papered over it
//! with a copy-then-write-back in the interpreter, so `var b = a; b.append(1)`
//! left `a` empty and a helper that appended to its parameter mutated a copy.
//!
//! Everything here is observed the way a game observes it: a `.tscn` is
//! instanced, a GDScript is attached through `SceneTree::attach_script` (the
//! runner's funnel), `_ready` runs, and the results are read back from the
//! script's own properties. No Rust-level Array API is used to make the
//! aliasing assertions.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Node2D" parent="."]
"#;

const SCRIPT: &str = "\
extends Node2D
var stored = []
var a_after_alias = null
var caller_after_helper = null
var dict_after_alias = null
var index_write_through_alias = null
var dup_independent = null
var nested_shared = null
var assign_identity = null
var stored_alias_len = 0

func append_one(arr):
    arr.append(1)

func put_key(d):
    d[\"k\"] = 2

func _ready():
    var a = []
    var b = a
    b.append(1)
    self.a_after_alias = a

    var c = []
    append_one(c)
    self.caller_after_helper = c

    var d = {}
    var e = d
    e[\"x\"] = 1
    put_key(d)
    self.dict_after_alias = d

    var m = [0]
    var n = m
    n[0] = 5
    self.index_write_through_alias = m

    var orig = [1, 2]
    var copy = orig.duplicate()
    copy.append(3)
    self.dup_independent = orig

    var inner = [1]
    var outer = [inner]
    var shallow = outer.duplicate()
    shallow[0].append(2)
    self.nested_shared = inner

    var target = [9]
    var t_alias = target
    target.assign([7, 8])
    self.assign_identity = t_alias

    stored.append(5)
    var s_alias = stored
    s_alias.append(6)
    self.stored_alias_len = stored.size()
";

fn arr(items: &[Variant]) -> Variant {
    Variant::Array(items.to_vec().into())
}

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// Instances the scene, attaches the same script source to `A` and `B`, and
/// runs `_ready` — the runner's order.
fn boot() -> (SceneTree, NodeId, NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let a = tree.get_node_or_null(scene_root, "A").expect("A");
    let b = tree.get_node_or_null(scene_root, "B").expect("B");
    for node in [a, b] {
        let script = GDScriptNodeInstance::from_source(SCRIPT, node).expect("script parses");
        tree.attach_script(node, Box::new(script));
    }
    LifecycleManager::enter_tree(&mut tree, scene_root);
    (tree, a, b)
}

#[test]
fn array_dictionary_reference_semantics_from_scene() {
    let (tree, a, _) = boot();

    assert_eq!(
        script_prop(&tree, a, "a_after_alias"),
        arr(&[Variant::Int(1)]),
        "`var b = a; b.append(1)` must be visible through `a`"
    );
    assert_eq!(
        script_prop(&tree, a, "caller_after_helper"),
        arr(&[Variant::Int(1)]),
        "an Array passed to a func that appends must be mutated for the caller"
    );

    let Variant::Dictionary(d) = script_prop(&tree, a, "dict_after_alias") else {
        panic!("dict_after_alias is not a Dictionary");
    };
    assert_eq!(
        d.get("x"),
        Some(Variant::Int(1)),
        "key set through an alias"
    );
    assert_eq!(
        d.get("k"),
        Some(Variant::Int(2)),
        "key set inside a helper func"
    );
    assert_eq!(d.len(), 2);

    assert_eq!(
        script_prop(&tree, a, "index_write_through_alias"),
        arr(&[Variant::Int(5)]),
        "`n[0] = 5` through an alias must write the shared storage"
    );
    assert_eq!(
        script_prop(&tree, a, "dup_independent"),
        arr(&[Variant::Int(1), Variant::Int(2)]),
        "duplicate() must still produce an independent array"
    );
    assert_eq!(
        script_prop(&tree, a, "nested_shared"),
        arr(&[Variant::Int(1), Variant::Int(2)]),
        "a SHALLOW duplicate shares nested arrays (Godot's duplicate(false))"
    );
    assert_eq!(
        script_prop(&tree, a, "assign_identity"),
        arr(&[Variant::Int(7), Variant::Int(8)]),
        "assign() overwrites the contents in place, so aliases see them"
    );
}

/// The same array read back through the script's member property shows the
/// mutation made through a local alias — the "read back through the node
/// property" clause of the bead.
#[test]
fn member_array_mutated_through_alias_reads_back_through_the_property() {
    let (tree, a, _) = boot();
    assert_eq!(
        script_prop(&tree, a, "stored"),
        arr(&[Variant::Int(5), Variant::Int(6)])
    );
    assert_eq!(script_prop(&tree, a, "stored_alias_len"), Variant::Int(2));
}

/// Guard for the per-instance side of the flip: `var stored = []` is evaluated
/// per instance in Godot, so two nodes running the same script must NOT share
/// it. Sharing would show as `[5, 6, 5, 6]` on whichever ran second.
#[test]
fn two_instances_of_one_script_do_not_share_member_arrays() {
    let (tree, a, b) = boot();
    let expected = arr(&[Variant::Int(5), Variant::Int(6)]);
    assert_eq!(script_prop(&tree, a, "stored"), expected);
    assert_eq!(script_prop(&tree, b, "stored"), expected);
}

/// Guard for scene instancing: a `PackedScene` is parsed once and instanced
/// many times, and Godot gives every instance its own shallow copy of an
/// Array/Dictionary property (SceneState::instantiate). Mutating one
/// instance's array must not leak into another's.
#[test]
fn two_instances_of_one_scene_do_not_share_array_properties() {
    let scene = PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="Holder" type="Node"]
metadata/items = [1, 2]
"#,
    )
    .expect("scene parses");
    let first = scene.instance().expect("first instance");
    let second = scene.instance().expect("second instance");

    let Variant::Array(items) = first[0].get_property("metadata/items") else {
        panic!("first instance has no items array");
    };
    items.push(Variant::Int(3)).unwrap();

    assert_eq!(
        second[0].get_property("metadata/items"),
        arr(&[Variant::Int(1), Variant::Int(2)]),
        "an Array property from one PackedScene must not be shared across instances"
    );
}
