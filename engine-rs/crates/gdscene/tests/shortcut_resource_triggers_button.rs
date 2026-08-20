//! pat-s3rrv.175 acceptance: a `.tscn` Button whose `shortcut` is a `Shortcut`
//! resource fires `pressed` when its key combination is pushed as input.
//!
//! Godot ground truth: `Shortcut` is a Resource holding an `Array[InputEvent]`;
//! `BaseButton::shortcut_input` (upstream/godot/scene/gui/base_button.cpp)
//! fires the button when one of those events matches during the
//! `_shortcut_input` stage, and `Viewport::_push_unhandled_input_internal`
//! admits Key/Shortcut/JoypadButton to that stage.
//!
//! Patina had neither class registered and nothing to match against: the
//! `_shortcut_input` stage ran (scene_tree.rs) over a tree in which no node
//! carried an accelerator, so a Ctrl+S in a scene authored with a Shortcut did
//! nothing at all. The two halves are separately breakable, so both are
//! asserted here: the scene loader must flatten the Shortcut's nested
//! sub-resources onto the node, and the input stage must match them.
//!
//! The negative cases are what give this test teeth. A matcher that ignored the
//! modifier mask, or tested it for containment instead of equality, would still
//! pass the Ctrl+S assertion — it fails only on plain `S` and on Ctrl+Shift+S.

use gdobject::class_db::{class_exists, register_all_classes};
use gdplatform::input::{InputEvent, Key};
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Counts `pressed` emissions so the assertions can distinguish "fired once"
/// from "fired on every key" — an exact count is what separates a real match
/// from a matcher that fires on any key event.
const LISTENER_GD: &str = "\
extends Node
var fired = 0
func _on_pressed():
    self.fired = self.fired + 1
";

/// A scene whose Button carries a `Shortcut` holding one `InputEventKey`.
///
/// The Shortcut's `events` is an Array of *nested* `SubResource` references, so
/// loading has to resolve two levels to reach the keycode — the level a naive
/// implementation stops short of, leaving the array full of strings.
fn scene_source(keycode: i64, modifier_lines: &str) -> String {
    format!(
        "[gd_scene load_steps=3 format=3]\n\
         \n\
         [sub_resource type=\"InputEventKey\" id=\"key_1\"]\n\
         keycode = {keycode}\n\
         {modifier_lines}\n\
         [sub_resource type=\"Shortcut\" id=\"sc_1\"]\n\
         events = [SubResource(\"key_1\")]\n\
         \n\
         [node name=\"Root\" type=\"Node\"]\n\
         \n\
         [node name=\"Button\" type=\"Button\" parent=\".\"]\n\
         shortcut = SubResource(\"sc_1\")\n\
         \n\
         [connection signal=\"pressed\" from=\"Button\" to=\".\" method=\"_on_pressed\"]\n"
    )
}

/// Loads the scene and attaches the counting script to its root.
fn load(source: &str) -> (MainLoop, NodeId) {
    let scene = PackedScene::from_tscn(source).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("scene instantiates");
    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(LISTENER_GD, root).unwrap()),
    );
    (MainLoop::new(tree), root)
}

fn fired(ml: &MainLoop, root: NodeId) -> i64 {
    match ml
        .tree()
        .get_script(root)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == "fired")
                .map(|p| p.default_value)
        })
        .unwrap_or(Variant::Nil)
    {
        Variant::Int(n) => n,
        other => panic!("`fired` should be an int, got {other:?}"),
    }
}

fn key(key: Key, ctrl: bool, shift: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: true,
        shift,
        ctrl,
        alt: false,
    }
}

/// Pushes `event` and runs one frame, as the editor's Play input path does.
fn push_and_step(ml: &mut MainLoop, event: &InputEvent) {
    ml.tree_mut().push_input(event);
    ml.step(1.0 / 60.0);
}

#[test]
fn ctrl_s_shortcut_fires_the_button_exactly_once() {
    let (mut ml, root) = load(&scene_source(83, "ctrl_pressed = true\n"));
    assert_eq!(fired(&ml, root), 0, "nothing pressed before any input");

    push_and_step(&mut ml, &key(Key::S, true, false));

    assert_eq!(
        fired(&ml, root),
        1,
        "Ctrl+S should emit `pressed` on the Button once"
    );
}

#[test]
fn plain_s_does_not_fire_a_ctrl_s_shortcut() {
    let (mut ml, root) = load(&scene_source(83, "ctrl_pressed = true\n"));

    push_and_step(&mut ml, &key(Key::S, false, false));

    assert_eq!(
        fired(&ml, root),
        0,
        "the bare key must not trigger a modified shortcut"
    );

    // Positive control: the same scene DOES fire for the right combination, so
    // the zero above cannot be explained by the shortcut never loading.
    push_and_step(&mut ml, &key(Key::S, true, false));
    assert_eq!(fired(&ml, root), 1, "Ctrl+S still fires in this same scene");
}

#[test]
fn an_extra_modifier_does_not_match() {
    let (mut ml, root) = load(&scene_source(83, "ctrl_pressed = true\n"));

    push_and_step(&mut ml, &key(Key::S, true, true));

    assert_eq!(
        fired(&ml, root),
        0,
        "Ctrl+Shift+S must not match a Ctrl+S shortcut — Godot matches exactly"
    );
}

#[test]
fn a_different_key_does_not_fire() {
    let (mut ml, root) = load(&scene_source(83, "ctrl_pressed = true\n"));

    push_and_step(&mut ml, &key(Key::A, true, false));

    assert_eq!(fired(&ml, root), 0, "Ctrl+A is a different accelerator");
}

#[test]
fn an_unmodified_shortcut_fires_on_the_bare_key() {
    // No modifier lines at all: proves the mask is read from the event bodies
    // rather than defaulted to something that only ever matches Ctrl.
    let (mut ml, root) = load(&scene_source(83, ""));

    push_and_step(&mut ml, &key(Key::S, false, false));
    assert_eq!(fired(&ml, root), 1, "a bare-S shortcut fires on bare S");

    push_and_step(&mut ml, &key(Key::S, true, false));
    assert_eq!(
        fired(&ml, root),
        1,
        "and Ctrl+S does not also fire it — equality, not containment"
    );
}

#[test]
fn classdb_reports_shortcut_and_input_event_shortcut() {
    register_all_classes();

    assert!(class_exists("Shortcut"), "Shortcut must be registered");
    assert!(
        class_exists("InputEventShortcut"),
        "InputEventShortcut must be registered"
    );
}
