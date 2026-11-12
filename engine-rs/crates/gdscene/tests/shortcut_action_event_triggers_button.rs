//! A Button whose `Shortcut` holds an `InputEventAction` fires when anything
//! bound to that action is pressed — and not while the button is hidden.
//!
//! Found playing the official Godot demo "Dodge the Creeps": its HUD's Start
//! button carries `Shortcut(events = [InputEventAction(action = &"start_game")])`
//! and `start_game` is bound to Enter and Space, so either key starts a run.
//! The loader flattened only `InputEventKey` (it read `keycode`, which an
//! action event lacks), so the shortcut was empty and neither key did anything.
//!
//! Godot ground truth: `BaseButton::shortcut_input` requires the button to be
//! enabled AND `is_visible_in_tree()`, and `Shortcut::matches_event` defers to
//! `InputEventAction::is_match`, i.e. the action's InputMap bindings.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key};
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

const LISTENER_GD: &str = "\
extends Node
var fired = 0
func _on_pressed():
    self.fired = self.fired + 1
";

/// Dodge the Creeps' `hud.tscn`, reduced to the Start button.
const HUD: &str = r#"[gd_scene format=3]

[sub_resource type="InputEventAction" id="InputEventAction_fopy7"]
action = &"start_game"

[sub_resource type="Shortcut" id="4"]
events = [SubResource("InputEventAction_fopy7")]

[node name="HUD" type="CanvasLayer"]

[node name="StartButton" type="Button" parent="."]
shortcut = SubResource("4")
text = "Start"

[connection signal="pressed" from="StartButton" to="." method="_on_pressed"]
"#;

fn press(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn release(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn boot() -> (MainLoop, NodeId, NodeId) {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(HUD).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let hud = add_packed_scene_to_tree(&mut tree, tree_root, &scene).unwrap();
    tree.attach_script(
        hud,
        Box::new(GDScriptNodeInstance::from_source(LISTENER_GD, hud).unwrap()),
    );
    let button = tree.get_node_relative(hud, "StartButton").unwrap();

    // project.godot's `start_game`: Enter and Space.
    let mut map = InputMap::new();
    map.add_action("start_game", 0.2);
    map.action_add_event("start_game", ActionBinding::KeyBinding(Key::Enter));
    map.action_add_event("start_game", ActionBinding::KeyBinding(Key::Space));
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);
    ml.step(1.0 / 60.0);
    (ml, hud, button)
}

fn fired(ml: &MainLoop, hud: NodeId) -> Variant {
    ml.tree()
        .get_script(hud)
        .and_then(|s| s.get_property("fired"))
        .unwrap_or(Variant::Nil)
}

fn tap(ml: &mut MainLoop, key: Key) {
    ml.push_event(press(key));
    ml.step(1.0 / 60.0);
    ml.push_event(release(key));
    ml.step(1.0 / 60.0);
}

#[test]
fn an_action_shortcut_fires_on_every_key_bound_to_the_action() {
    let (mut ml, hud, _) = boot();
    assert_eq!(fired(&ml, hud), Variant::Int(0));

    tap(&mut ml, Key::Enter);
    assert_eq!(
        fired(&ml, hud),
        Variant::Int(1),
        "Enter is bound to start_game"
    );

    tap(&mut ml, Key::Space);
    assert_eq!(fired(&ml, hud), Variant::Int(2), "so is Space");

    tap(&mut ml, Key::A);
    assert_eq!(
        fired(&ml, hud),
        Variant::Int(2),
        "an unbound key fires nothing"
    );
}

#[test]
fn a_hidden_button_does_not_answer_its_shortcut() {
    let (mut ml, hud, button) = boot();
    ml.tree_mut()
        .get_node_mut(button)
        .unwrap()
        .set_property("visible", Variant::Bool(false));
    tap(&mut ml, Key::Enter);
    assert_eq!(
        fired(&ml, hud),
        Variant::Int(0),
        "Godot's BaseButton ignores its shortcut unless visible in the tree"
    );
}
