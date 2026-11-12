//! pat-mgixk.19: a typed var starts at its type's zero value, and a typed slot
//! converts or refuses on assignment.
//!
//! **Wiring**: the script is attached to a node loaded from a `.tscn` and run
//! through `SceneTree`'s lifecycle inside a `MainLoop`, so the instance defaults
//! are the ones `GDScriptNodeInstance` really hands the engine.
//!
//! **Godot ground truth**: `modules/gdscript/gdscript_analyzer.cpp` default-
//! initialises a typed member to the type's zero value, and
//! `gdscript_vm.cpp`'s `OPCODE_ASSIGN_TYPED_BUILTIN` converts the compatible
//! builtins on assignment and raises on the rest.
//!
//! What was wrong:
//! * `instantiate_class` gave every initialiser-less var `Variant::Nil`
//!   regardless of its hint, so `var count: int` then `count += 1` operated on
//!   null instead of 0.
//! * the declaration path only ran `coerce_to_declared_type`, which returns a
//!   non-container untouched, so `var speed: float = 200` stayed an **Int** and
//!   `speed / 3` integer-divided to 66 where Godot gives 66.666….
//! * `var n: int = "x"` and `n = "x"` were accepted silently.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::SceneTree;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

const SCENE: &str = r#"[gd_scene format=3 uid="uid://pat_mgixk_19_typed_vars"]

[node name="Root" type="Node2D"]
"#;

/// `typeof()` is recorded BEFORE anything is assigned, which is what separates
/// "the default is the type's zero" from "the default is null and the first
/// arithmetic happened to work".
const SCRIPT: &str = r#"extends Node2D

var count: int
var speed: float = 200
var pos: Vector2
var flag: bool
var label: String

var count_type = -1
var speed_type = -1
var third = -1.0
var label_is_empty = false
var ran = false

func _ready():
    count_type = typeof(count)
    speed_type = typeof(speed)
    count += 1
    third = speed / 3
    label_is_empty = label == ""
    ran = true
"#;

/// A typed slot refuses a value it cannot convert, which aborts `_ready`
/// before the sentinel is set.
const BAD_ASSIGN_SCRIPT: &str = r#"extends Node2D

var count: int
var reached_end = false

func _ready():
    count = "x"
    reached_end = true
"#;

fn run_script(source: &str) -> (MainLoop, gdscene::NodeId) {
    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene should parse");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let scene_root =
        add_packed_scene_to_tree(&mut tree, tree_root, &packed).expect("scene should instance");

    let instance =
        GDScriptNodeInstance::from_source(source, scene_root).expect("script should parse");
    tree.attach_script(scene_root, Box::new(instance));

    for nid in tree.all_nodes_in_tree_order() {
        tree.process_script_enter_tree(nid);
    }
    for nid in tree.all_nodes_in_tree_order() {
        if tree.has_script(nid) {
            tree.process_script_ready(nid);
        }
    }

    let mut main_loop = MainLoop::new(tree);
    for _ in 0..3 {
        main_loop.step(DT);
    }
    (main_loop, scene_root)
}

fn script_var(main_loop: &MainLoop, node: gdscene::NodeId, name: &str) -> Variant {
    main_loop
        .tree()
        .get_script(node)
        .expect("script should be attached")
        .get_property(name)
        .unwrap_or_else(|| panic!("script should declare `{name}`"))
}

// ===========================================================================
// 1. The acceptance
// ===========================================================================

#[test]
fn gdscript_typed_var_defaults_and_conversion() {
    let (main_loop, root) = run_script(SCRIPT);

    assert_eq!(
        script_var(&main_loop, root, "ran"),
        Variant::Bool(true),
        "_ready must run to completion — a false here means a statement raised"
    );

    // TYPE_INT is 2 and TYPE_FLOAT is 3 (@GlobalScope `Variant.Type`); TYPE_NIL
    // is 0, which is what both of these reported before the fix.
    assert_eq!(
        script_var(&main_loop, root, "count_type"),
        Variant::Int(2),
        "`var count: int` with no initialiser must already BE an int, not null"
    );
    assert_eq!(
        script_var(&main_loop, root, "speed_type"),
        Variant::Int(3),
        "`var speed: float = 200` must hold a FLOAT — storing the Int is what \
         made `speed / 3` integer-divide"
    );

    assert_eq!(
        script_var(&main_loop, root, "count"),
        Variant::Int(1),
        "`count += 1` on a zero-initialised int is 1 (it was Nil + 1 before)"
    );

    // The consequence the bead is really about. Asserted numerically with an
    // epsilon rather than against `66.6666666666667`: that string is a float
    // formatting detail, and pinning it would make the test about `print`.
    match script_var(&main_loop, root, "third") {
        Variant::Float(f) => assert!(
            (f - 200.0 / 3.0).abs() < 1e-9,
            "speed / 3 must be true division, got {f}"
        ),
        other => panic!("speed / 3 must yield a Float, got {other:?}"),
    }

    assert_eq!(
        script_var(&main_loop, root, "pos"),
        Variant::Vector2(Vector2::ZERO),
        "`var pos: Vector2` defaults to (0, 0)"
    );
    assert_eq!(
        script_var(&main_loop, root, "flag"),
        Variant::Bool(false),
        "`var flag: bool` defaults to false"
    );
    assert_eq!(
        script_var(&main_loop, root, "label_is_empty"),
        Variant::Bool(true),
        "`var label: String` defaults to the empty string"
    );
}

// ===========================================================================
// 2. The defaults reach the NODE, which is the wiring the bead names
// ===========================================================================

#[test]
fn typed_defaults_reach_the_node_properties_the_tree_syncs() {
    let (main_loop, root) = run_script(SCRIPT);

    // `SceneTree::call_script_method` copies the script's vars onto the node
    // after each callback, so this is the value the rest of the engine — the
    // inspector, a save, another script's `$Root.count` — actually sees.
    let node = main_loop
        .tree()
        .get_node(root)
        .expect("scene root should exist");
    assert_eq!(
        node.get_property("count"),
        Variant::Int(1),
        "the engine-visible property must be the int, not a Nil the tree copied"
    );
    assert_eq!(
        node.get_property("pos"),
        Variant::Vector2(Vector2::ZERO),
        "a typed default must survive the sync to node properties"
    );
}

// ===========================================================================
// 3. An unconvertible assignment raises instead of storing a wrong type
// ===========================================================================

#[test]
fn assigning_an_incompatible_value_to_a_typed_member_raises() {
    let (main_loop, root) = run_script(BAD_ASSIGN_SCRIPT);

    assert_eq!(
        script_var(&main_loop, root, "reached_end"),
        Variant::Bool(false),
        "`count = \"x\"` on `var count: int` must raise, which aborts _ready \
         before this sentinel is set — it used to be accepted silently"
    );
    // ...and the slot keeps its typed value rather than holding the String.
    assert_eq!(
        script_var(&main_loop, root, "count"),
        Variant::Int(0),
        "the refused assignment must leave the int at its default"
    );
}
