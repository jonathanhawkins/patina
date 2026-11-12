//! Godot 4 idiom passes `StringName` and `NodePath` LITERALS where the API is
//! typed that way, and the official demo projects do it everywhere — Dodge the
//! Creeps' `player.gd` reads `Input.is_action_pressed(&"move_right")` every
//! frame, and `main.gd` spawns mobs from `get_node(^"MobPath/MobSpawnLocation")`.
//! Both argument checks accepted only `String`, so the player could not move
//! (a type error every frame) and no mob could spawn.
//!
//! Driven through a live, stepped `MainLoop` with a real input map.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const SCRIPT: &str = "\
extends Node
var right = false
var right_just = false
var strength = 0.0
var spawn_name = \"\"
var spawn_by_name = \"\"
func _process(delta):
    self.right = Input.is_action_pressed(&\"move_right\")
    if Input.is_action_just_pressed(&\"move_right\"):
        self.right_just = true
    self.strength = Input.get_action_strength(&\"move_right\")
    self.spawn_name = get_node(^\"MobPath/MobSpawnLocation\").name
    self.spawn_by_name = get_node(&\"MobPath\").name
";

fn prop(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

#[test]
fn stringname_and_nodepath_literals_are_accepted_as_action_and_path_args() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let path = tree
        .add_child(main, Node::new("MobPath", "Path2D"))
        .unwrap();
    tree.add_child(path, Node::new("MobSpawnLocation", "PathFollow2D"))
        .unwrap();
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(SCRIPT, main).expect("script parses")),
    );

    let mut map = InputMap::new();
    map.add_action("move_right", 0.2);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);

    ml.step(1.0 / 60.0);
    assert_eq!(prop(ml.tree(), main, "right"), Variant::Bool(false));
    assert_eq!(
        prop(ml.tree(), main, "spawn_name"),
        Variant::String("MobSpawnLocation".into()),
        "get_node(^\"...\") resolves a NodePath literal"
    );
    assert_eq!(
        prop(ml.tree(), main, "spawn_by_name"),
        Variant::String("MobPath".into()),
        "get_node(&\"...\") resolves a StringName"
    );

    ml.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ml.step(1.0 / 60.0);
    assert_eq!(
        prop(ml.tree(), main, "right"),
        Variant::Bool(true),
        "Input.is_action_pressed(&\"move_right\") reads the held key"
    );
    assert_eq!(prop(ml.tree(), main, "right_just"), Variant::Bool(true));
    assert_eq!(prop(ml.tree(), main, "strength"), Variant::Float(1.0));
}
