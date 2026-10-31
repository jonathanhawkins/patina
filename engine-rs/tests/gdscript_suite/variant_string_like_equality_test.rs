//! pat-mgixk.12 (part 1): `String` == `StringName`, and Godot's
//! `StringLikeVariantComparator` for Dictionary keys and Array searches.
//!
//! **Wiring**: the stimulus is a `.tscn` that authors `animation = &"walk"`.
//! `gdresource::loader` turns `&"..."` into a `Variant::StringName`
//! (loader.rs:674), the node keeps it in its property bag, and the GDScript
//! attached to the scene root reads it back through `$Sprite.animation`. So
//! every assertion below is about a value that entered the engine the way a
//! game's scene does — not one the test constructed.
//!
//! **Godot ground truth**
//! * `==`/`!=` are registered for all four String/StringName pairings
//!   (`register_string_op(OperatorEvaluatorEqual, OP_EQUAL)`,
//!   `core/variant/variant_op.cpp:493` over the macro at :196-201), so
//!   `&"walk" == "walk"`.
//! * Dictionary keys use `StringLikeVariantComparator`
//!   (`core/variant/dictionary.cpp:50`) and `Array.find`/`rfind`/`count`/`has`
//!   use it too (`array.cpp:383/437/489/497`). Its first step is
//!   `Variant::hash_compare`, which bails on `type != p_variant.type`
//!   (`variant.cpp:3172`) — so `int` and `float` keys/elements are DISTINCT —
//!   and its fallback compares a `String` against a `StringName` by text
//!   (`variant.cpp:3396-3405`) — so those two unify.
//!
//! Before this bead Patina had both halves inverted: `variant_eq` promoted
//! int/float and left String/StringName apart, which silently broke every
//! comparison and lookup against a scene-authored `&"name"`.

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::SceneTree;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// An `AnimatedSprite2D` whose `animation` is authored the way Godot writes a
/// StringName. No `autoplay` and no `playing`, so the sprite-frame advance in
/// `SceneTree` skips the node and never rewrites the property.
const SCENE: &str = r#"[gd_scene format=3 uid="uid://pat_mgixk_12_string_like"]

[node name="Root" type="Node2D"]

[node name="Sprite" type="AnimatedSprite2D" parent="."]
animation = &"walk"
"#;

const SCRIPT: &str = r#"extends Node2D

var ran = false
var anim_type = -99
var anim_eq_string = false
var anim_eq_string_reversed = false
var anim_ne_other = false
var dict_string_key_has_string_name = false
var dict_string_name_key_has_string = false
var dict_scene_name_lookup = 0
var dict_int_key_has_float = true
var array_int_has_float = true
var array_string_has_string_name = false
var array_find_string_name = -99
var array_count_mixed = -99
var in_operator_string_name = false

func _ready():
    var anim = $Sprite.animation
    # Recorded FIRST, before anything can raise: proof that the script really
    # received TYPE_STRING_NAME (21) and not a String. Without it every
    # assertion below could pass for the wrong reason.
    anim_type = typeof(anim)

    # `==` across the two kinds, both operand orders.
    anim_eq_string = anim == "walk"
    anim_eq_string_reversed = "walk" == anim
    anim_ne_other = anim != "run"

    # Dictionary keys: String and StringName are ONE key.
    var d = {"a": 1}
    dict_string_key_has_string_name = d.has(&"a")
    var e = {}
    e[&"b"] = 2
    dict_string_name_key_has_string = e.has("b")
    var clips = {"walk": 7, "run": 9}
    dict_scene_name_lookup = clips[anim]

    # ...but an int key and a float key are NOT.
    var f = {1: "x"}
    dict_int_key_has_float = f.has(1.0)

    # Array searches use the same comparator.
    array_int_has_float = [1].has(1.0)
    array_string_has_string_name = ["a"].has(&"a")
    array_find_string_name = ["x", "a"].find(&"a")
    array_count_mixed = ["a", &"a", "b"].count("a")
    in_operator_string_name = &"a" in ["a"]

    ran = true
"#;

/// Builds the scene, attaches the script to its root, runs enter_tree + ready,
/// and steps a MainLoop. Returns the loop and the scene root.
fn run_scene(script: &str) -> (MainLoop, gdscene::NodeId) {
    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene should parse");

    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let scene_root =
        add_packed_scene_to_tree(&mut tree, tree_root, &packed).expect("scene should instance");

    // Precondition: the `.tscn` really did deliver a StringName. If the loader
    // ever normalized `&"walk"` to a String, every assertion below would pass
    // for the wrong reason, so this is checked before anything runs.
    let animation = {
        let sprite = tree
            .get_node_relative(scene_root, "Sprite")
            .expect("Sprite child should exist");
        tree.get_node(sprite)
            .expect("Sprite node should exist")
            .get_property("animation")
    };
    assert!(
        matches!(&animation, Variant::StringName(sn) if sn.as_str() == "walk"),
        "fixture precondition: `animation = &\"walk\"` must load as a StringName, got {animation:?}"
    );

    let instance = GDScriptNodeInstance::from_source(script, scene_root)
        .expect("acceptance script should parse");
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

#[test]
fn scene_loaded_string_name_equals_the_same_text_as_a_string() {
    let (main_loop, root) = run_scene(SCRIPT);

    assert_eq!(
        script_var(&main_loop, root, "ran"),
        Variant::Bool(true),
        "_ready must run to completion — a Nil/false here means a statement errored out"
    );
    assert_eq!(
        script_var(&main_loop, root, "anim_type"),
        Variant::Int(21),
        "the script must see TYPE_STRING_NAME (21) for $Sprite.animation — if the \
         read path coerced it to a String, the equality assertions would be vacuous"
    );
    assert_eq!(
        script_var(&main_loop, root, "anim_eq_string"),
        Variant::Bool(true),
        "`$Sprite.animation == \"walk\"` on a .tscn-authored &\"walk\" (variant_op.cpp:493)"
    );
    assert_eq!(
        script_var(&main_loop, root, "anim_eq_string_reversed"),
        Variant::Bool(true),
        "the reverse operand order is registered too (StringName, String)"
    );
    assert_eq!(
        script_var(&main_loop, root, "anim_ne_other"),
        Variant::Bool(true),
        "`!=` must not answer true for equal text and false for different text"
    );
}

#[test]
fn dictionary_keys_unify_string_and_string_name() {
    let (main_loop, root) = run_scene(SCRIPT);

    assert_eq!(
        script_var(&main_loop, root, "dict_string_key_has_string_name"),
        Variant::Bool(true),
        "{{\"a\": 1}}.has(&\"a\") — StringLikeVariantComparator (dictionary.cpp:50)"
    );
    assert_eq!(
        script_var(&main_loop, root, "dict_string_name_key_has_string"),
        Variant::Bool(true),
        "the reverse: a StringName-keyed entry found with a String"
    );
    assert_eq!(
        script_var(&main_loop, root, "dict_scene_name_lookup"),
        Variant::Int(7),
        "indexing a clip table with the scene's own &\"walk\" must find the \"walk\" row"
    );
}

#[test]
fn int_and_float_stay_distinct_keys_and_elements() {
    let (main_loop, root) = run_scene(SCRIPT);

    assert_eq!(
        script_var(&main_loop, root, "dict_int_key_has_float"),
        Variant::Bool(false),
        "{{1: \"x\"}}.has(1.0) is false — hash_compare bails on a type mismatch \
         (variant.cpp:3172), even though `1 == 1.0`"
    );
    assert_eq!(
        script_var(&main_loop, root, "array_int_has_float"),
        Variant::Bool(false),
        "[1].has(1.0) is false — Array.has goes through find/StringLike (array.cpp:383/497)"
    );
}

#[test]
fn array_searches_use_the_string_like_comparator() {
    let (main_loop, root) = run_scene(SCRIPT);

    assert_eq!(
        script_var(&main_loop, root, "array_string_has_string_name"),
        Variant::Bool(true),
        "[\"a\"].has(&\"a\") is true"
    );
    assert_eq!(
        script_var(&main_loop, root, "array_find_string_name"),
        Variant::Int(1),
        "find() must locate the String element from a StringName needle"
    );
    assert_eq!(
        script_var(&main_loop, root, "array_count_mixed"),
        Variant::Int(2),
        "count(\"a\") counts both \"a\" and &\"a\" — one comparator, two spellings"
    );
    assert_eq!(
        script_var(&main_loop, root, "in_operator_string_name"),
        Variant::Bool(true),
        "`&\"a\" in [\"a\"]` is Array::find (OperatorEvaluatorInArrayFind, variant_op.h:1142)"
    );
}
