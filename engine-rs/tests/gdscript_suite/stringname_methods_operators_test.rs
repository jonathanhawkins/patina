//! pat-mgixk.12 (part 2): the `StringName` receiver and operator surface.
//!
//! **Wiring**: same shape as the part-1 test — a `.tscn` authors
//! `animation = &"walk"`, so `$Sprite.animation` hands the attached GDScript a
//! `Variant::StringName` that the loader produced (gdresource/src/loader.rs:674),
//! and `$Sprite.animation.to_upper()` is a method call on it.
//!
//! **Godot ground truth**
//! * `extension_api`'s `StringName` builtin mirrors `String`'s method set, and
//!   those methods return `String` (`variant.cpp:1645-1646` converts a
//!   StringName to its bare name — no `&` sigil).
//! * `register_string_op` registers `+` (`variant_op.cpp:222`), `==`/`!=`
//!   (:493/:615) and `in` (:922) for all four String/StringName pairings;
//!   `register_string_modulo_op` (:204-208) registers `%` with a String *or* a
//!   StringName format; ordering is registered for StringName x StringName only
//!   (:737/:752/:768/:783).
//!
//! Before this bead `call_method_on` had no `Variant::StringName` receiver arm
//! at all, so every method on a scene-loaded name hit
//! "cannot call method on StringName", and `binary_add`/`binary_mod`/`binary_cmp`
//! had no StringName rows.

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::SceneTree;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

const SCENE: &str = r#"[gd_scene format=3 uid="uid://pat_mgixk_12_stringname_methods"]

[node name="Root" type="Node2D"]

[node name="Sprite" type="AnimatedSprite2D" parent="."]
animation = &"walk"
"#;

const SCRIPT: &str = r#"extends Node2D

var ran = false
var anim_type = -99
var begins_with_result = false
var ends_with_result = false
var length_result = -99
var contains_string_name_arg = false
var concat_name_then_string = ""
var concat_string_then_name = ""
var format_name_as_value = ""
var format_name_as_format = ""
var less_result = false
var greater_result = true
var upper_from_scene = ""
var substring_in_name = false

func _ready():
    # Recorded FIRST, before any StringName method can raise: proof the script
    # received TYPE_STRING_NAME (21) from the scene.
    anim_type = typeof($Sprite.animation)

    var idle = &"idle_left"
    begins_with_result = idle.begins_with("idle")
    ends_with_result = idle.ends_with("left")

    var abc = &"abc"
    length_result = abc.length()

    # A StringName passed as a *string argument* must arrive as `b`, not `&b`.
    contains_string_name_arg = "abc".contains(&"b")

    concat_name_then_string = &"a" + "b"
    concat_string_then_name = "a" + &"b"

    format_name_as_value = "%s!" % &"hi"
    var fmt = &"%s?"
    format_name_as_format = fmt % "hi"

    less_result = &"a" < &"b"
    greater_result = &"a" > &"b"

    upper_from_scene = $Sprite.animation.to_upper()
    substring_in_name = "id" in idle

    ran = true
"#;

fn run_scene() -> (MainLoop, gdscene::NodeId) {
    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene should parse");

    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let scene_root =
        add_packed_scene_to_tree(&mut tree, tree_root, &packed).expect("scene should instance");

    // The whole point is that the receiver is a StringName the *scene* created.
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

    let instance = GDScriptNodeInstance::from_source(SCRIPT, scene_root)
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
fn string_name_answers_the_string_method_surface() {
    let (main_loop, root) = run_scene();

    assert_eq!(
        script_var(&main_loop, root, "anim_type"),
        Variant::Int(21),
        "the script must see TYPE_STRING_NAME (21) for $Sprite.animation"
    );
    assert_eq!(
        script_var(&main_loop, root, "ran"),
        Variant::Bool(true),
        "_ready must run to completion — a false here means a StringName method \
         or operator raised instead of answering"
    );
    assert_eq!(
        script_var(&main_loop, root, "begins_with_result"),
        Variant::Bool(true),
        "(&\"idle_left\").begins_with(\"idle\")"
    );
    assert_eq!(
        script_var(&main_loop, root, "ends_with_result"),
        Variant::Bool(true),
        "(&\"idle_left\").ends_with(\"left\")"
    );
    assert_eq!(
        script_var(&main_loop, root, "length_result"),
        Variant::Int(3),
        "(&\"abc\").length()"
    );
    assert_eq!(
        script_var(&main_loop, root, "upper_from_scene"),
        Variant::String("WALK".into()),
        "$Sprite.animation.to_upper() on the .tscn-loaded &\"walk\" — and the \
         result is a String, as the extension_api return types say"
    );
}

#[test]
fn a_string_name_argument_arrives_without_its_sigil() {
    let (main_loop, root) = run_scene();

    assert_eq!(
        script_var(&main_loop, root, "contains_string_name_arg"),
        Variant::Bool(true),
        "\"abc\".contains(&\"b\") — Godot converts the argument with \
         `operator String()` (variant.cpp:1645), so the needle is `b`, never `&b`"
    );
    assert_eq!(
        script_var(&main_loop, root, "format_name_as_value"),
        Variant::String("hi!".into()),
        "`%s` renders a StringName as its bare name"
    );
}

#[test]
fn string_name_operators_match_the_registered_rows() {
    let (main_loop, root) = run_scene();

    assert_eq!(
        script_var(&main_loop, root, "concat_name_then_string"),
        Variant::String("ab".into()),
        "&\"a\" + \"b\" yields a String (variant_op.h:717 `ReturnType = String`)"
    );
    assert_eq!(
        script_var(&main_loop, root, "concat_string_then_name"),
        Variant::String("ab".into()),
        "the reverse pairing is registered too"
    );
    assert_eq!(
        script_var(&main_loop, root, "format_name_as_format"),
        Variant::String("hi?".into()),
        "a StringName is a valid format string (register_string_modulo_op)"
    );
    assert_eq!(
        script_var(&main_loop, root, "less_result"),
        Variant::Bool(true),
        "&\"a\" < &\"b\" — StringName x StringName ordering is registered \
         (variant_op.cpp:737)"
    );
    assert_eq!(
        script_var(&main_loop, root, "greater_result"),
        Variant::Bool(false),
        "...and it is a real ordering, not a constant"
    );
    assert_eq!(
        script_var(&main_loop, root, "substring_in_name"),
        Variant::Bool(true),
        "`\"id\" in &\"idle_left\"` is a substring test \
         (OperatorEvaluatorInStringFind, variant_op.cpp:922)"
    );
}
