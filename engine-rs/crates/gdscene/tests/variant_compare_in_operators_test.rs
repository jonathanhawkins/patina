//! pat-mgixk.11 part 2 acceptance: ordering and `in` beyond int/float/String,
//! driven by a GDScript attached to a node in a loaded scene.
//!
//! Godot registers `<`/`<=`/`>`/`>=` for Vector2/2i/3/3i/4/4i, Array, RID,
//! bool and StringName (`variant_op.cpp` OP_LESS), and `in` for every packed
//! array plus String/StringName against an Object (property membership,
//! `OperatorEvaluatorObjectHasPropertyString`, variant_op.cpp:1031).
//! `binary_cmp` ordered only Int/Float pairs, String/String and StringName
//! pairs; `BinOp::In` handled only Array, Dictionary and String-in-String. So
//! `Vector2(1,5) < Vector2(2,0)`, `[1,2] < [1,3]`,
//! `3 in PackedInt32Array([1,3])` and `"position" in self` all raised a
//! TypeError.
//!
//! ⚠️ Three assertions here exist to catch an implementation that is right by
//! accident, because "it returned true" is most of what this feature looks
//! like:
//!
//! * `Vector2(1,5) < Vector2(2,0)` is true **and** the swapped
//!   `Vector2(2,0) < Vector2(1,5)` is false. A relational assertion on its own
//!   survives swapping its operands, so the pair is what pins the direction —
//!   and this particular pair also pins *lexicographic* order, since the
//!   y components point the other way from the answer.
//! * `[1,2] < [1,2,3]` is true: on a shared prefix the SHORTER array sorts
//!   first (`array.cpp:179`, the `return a_len < b_len` tail). A size-first
//!   comparison would get `[1,2] < [1,3]` wrong instead.
//! * `2 in PackedInt32Array([1,3])` is false and
//!   `"no_such_property" in self` is false — otherwise a stub returning `true`
//!   would pass every positive case.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
"#;

const SCRIPT: &str = r#"
extends Node2D

var v2_lt
var v2_lt_swapped
var v2i_le
var v3_lt
var arr_lt
var arr_prefix_lt
var bool_lt
var str_in_str
var int_in_packed
var int_not_in_packed
var int_in_float_packed
var str_in_packed
var has_position
var has_nonsense
var ran = false

func _ready():
    self.v2_lt = Vector2(1, 5) < Vector2(2, 0)
    self.v2_lt_swapped = Vector2(2, 0) < Vector2(1, 5)
    self.v2i_le = Vector2i(1, 2) <= Vector2i(1, 2)
    self.v3_lt = Vector3(1, 2, 3) < Vector3(1, 2, 4)
    self.arr_lt = [1, 2] < [1, 3]
    self.arr_prefix_lt = [1, 2] < [1, 2, 3]
    self.bool_lt = false < true
    self.str_in_str = "ell" in "hello"
    self.int_in_packed = 3 in PackedInt32Array([1, 3])
    self.int_not_in_packed = 2 in PackedInt32Array([1, 3])
    self.int_in_float_packed = 3 in PackedFloat32Array([1.0, 3.0])
    self.str_in_packed = "b" in PackedStringArray(["a", "b"])
    self.has_position = "position" in self
    self.has_nonsense = "no_such_property_xyz" in self
    self.ran = true
"#;

/// Reads a script `var` — node properties and script vars are separate stores.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

#[test]
fn variant_compare_in_operators_test() {
    // ⚠️ `class_db`'s registry starts EMPTY and is only populated by this call
    // (see the note at scene_tree.rs:8508). `"position" in self` answers from
    // the class' property list, so without this the property arm would report
    // `false` for every name and the test would fail for the wrong reason.
    gdobject::class_db::register_all_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let script = GDScriptNodeInstance::from_source(SCRIPT, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);

    // First, and for a reason: a TypeError on any line of `_ready` aborts it
    // silently, which from here is indistinguishable from "that var was never
    // declared".
    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` must run to completion — an unsupported comparison or `in` \
         raises a TypeError and aborts it part-way"
    );

    let tree = ml.tree();
    let truth = |name: &str| match script_var(tree, scene_root, name) {
        Variant::Bool(b) => b,
        other => panic!("`{name}` should be a Bool, got {other:?}"),
    };

    // -- vector ordering is lexicographic, not componentwise -------------
    assert!(
        truth("v2_lt"),
        "Vector2(1,5) < Vector2(2,0): x decides, so the larger y does not matter"
    );
    assert!(
        !truth("v2_lt_swapped"),
        "the swapped operands must be false — a relational assertion alone \
         survives swapping, so this is what pins the direction"
    );
    assert!(truth("v2i_le"), "Vector2i(1,2) <= Vector2i(1,2)");
    assert!(
        truth("v3_lt"),
        "Vector3 compares x, then y, then z (vector3.h:484)"
    );

    // -- Array ordering: element-wise, shorter-first on a shared prefix ---
    assert!(truth("arr_lt"), "[1,2] < [1,3] compares element-wise");
    assert!(
        truth("arr_prefix_lt"),
        "[1,2] < [1,2,3]: on a shared prefix the shorter array sorts first"
    );

    assert!(truth("bool_lt"), "false < true");

    // -- `in` ------------------------------------------------------------
    assert!(
        truth("str_in_str"),
        "\"ell\" in \"hello\" is a substring test"
    );
    assert!(
        truth("int_in_packed"),
        "3 in PackedInt32Array([1,3]) searches the packed elements"
    );
    assert!(
        !truth("int_not_in_packed"),
        "2 in PackedInt32Array([1,3]) must be FALSE — otherwise a stub \
         returning true passes every positive case above"
    );
    assert!(
        truth("int_in_float_packed"),
        "an int needle is compared numerically against a float array, because \
         `INT in PACKED_FLOAT32_ARRAY` is itself a registered row"
    );
    assert!(
        truth("str_in_packed"),
        "\"b\" in PackedStringArray([\"a\",\"b\"])"
    );

    // -- `in` on an Object is PROPERTY membership, not a value search -----
    assert!(
        truth("has_position"),
        "\"position\" in self must be true for a Node2D — it is a registered \
         property of the class"
    );
    assert!(
        !truth("has_nonsense"),
        "an unregistered name must be false, or the property arm is answering \
         true for everything"
    );
}
