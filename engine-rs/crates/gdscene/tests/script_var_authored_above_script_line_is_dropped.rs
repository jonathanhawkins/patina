//! Godot applies a `.tscn` node's properties in FILE ORDER
//! (packed_scene.cpp `SceneState::instantiate`). A script variable authored
//! ABOVE `script = …` is set on the still script-less node, is not a property
//! of its class, and is lost: the variable keeps its initializer. Authored
//! BELOW the script line (what Godot's own saver writes) it overrides.
//! Found by the refreshed Godot 4.6.1 oracle for `fixtures/scenes/space_shooter`.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

const SRC: &str = "extends Node2D\nvar speed = 200.0\n";

fn speed_after_load(node_body: &str) -> (Variant, bool) {
    gdobject::class_db::register_all_classes();
    let tscn = format!(
        "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://p.gd\" id=\"1\"]\n\n\
         [node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"Player\" type=\"Node2D\" parent=\".\"]\n{node_body}\n"
    );
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let player = tree.get_node(scene).unwrap().children()[0];
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(SRC, player).unwrap()),
    );
    let speed = tree
        .get_script(player)
        .unwrap()
        .get_property("speed")
        .unwrap_or(Variant::Nil);
    let marker_left = tree
        .get_node(player)
        .unwrap()
        .has_property("_props_before_script");
    (speed, marker_left)
}

#[test]
fn a_var_above_the_script_line_keeps_its_initializer() {
    let (speed, marker_left) =
        speed_after_load("position = Vector2(1, 2)\nspeed = 200\nscript = ExtResource(\"1\")");
    assert_eq!(
        speed,
        Variant::Float(200.0),
        "Godot drops `speed = 200` set before the script"
    );
    assert!(!marker_left, "the marker is consumed on attach");
}

#[test]
fn a_var_below_the_script_line_overrides() {
    let (speed, _) =
        speed_after_load("position = Vector2(1, 2)\nscript = ExtResource(\"1\")\nspeed = 300");
    assert_eq!(speed, Variant::Int(300));
}

#[test]
fn native_properties_above_the_script_line_are_kept() {
    // Edge: a class property authored first (Godot's normal layout) must not be
    // marked or dropped.
    gdobject::class_db::register_all_classes();
    let tscn = "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://p.gd\" id=\"1\"]\n\n\
                [node name=\"Root\" type=\"Node2D\"]\nposition = Vector2(5, 6)\nscript = ExtResource(\"1\")\n";
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let node = tree.get_node(scene).unwrap();
    assert!(!node.has_property("_props_before_script"));
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(gdcore::math::Vector2::new(5.0, 6.0))
    );
}
