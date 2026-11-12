//! pat-s3rrv.155 — `SubViewportContainer._propagate_input_event(event) -> bool`
//! decides whether an event reaches the hosted sub-scene.
//!
//! Godot ground truth: `SubViewportContainer._propagate_input_event`
//! (`doc/classes/SubViewportContainer.xml`, `virtual const`, returns bool). A
//! script overrides it to veto forwarding.
//!
//! ⚠️ **The seam is the point of this bead.** `hit_test_control` already
//! descends through a SubViewportContainer into its children — it has to, to
//! map the click into sub-viewport space — but it takes `&SceneTree`, and
//! calling a script needs `&mut`. So the veto is asked in
//! `SceneTree::gui_input_stage`, the first place after the hit that holds
//! `&mut self`.
//!
//! ⚠️ **Not overriding must change nothing.** `call_script_method` returns
//! `None` for a method the script does not define, so only an explicit `false`
//! vetoes — a container with no script, or a script without the override,
//! forwards exactly as before. Both are asserted.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Counts the `_gui_input` events the Button inside the sub-viewport receives.
const BUTTON_GD: &str = "\
extends Button
var hits = 0

func _gui_input(event):
    self.hits += 1
";

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// `SVC(stretch, shrink 2) -> SubViewport -> Btn`, with `svc_script` optionally
/// attached to the container. Mirrors the fixture in
/// `subviewport_container_composite_and_input.rs`, whose own test proves a click
/// at global (160,160) reaches the Button at sub-viewport (30,30).
fn scene(svc_script: Option<&str>) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut container = Node::new("SVC", "SubViewportContainer");
    container.set_property("stretch", Variant::Bool(true));
    container.set_property("stretch_shrink", Variant::Int(2));
    let svc = tree.add_child(root, container).unwrap();
    control::set_position(&mut tree, svc, Vector2::new(100.0, 100.0));
    control::set_size(&mut tree, svc, Vector2::new(200.0, 200.0));

    let sv = tree
        .add_child(svc, Node::new("SubViewport", "SubViewport"))
        .unwrap();
    let button = tree.add_child(sv, Node::new("Btn", "Button")).unwrap();
    control::set_position(&mut tree, button, Vector2::new(20.0, 20.0));
    control::set_size(&mut tree, button, Vector2::new(40.0, 40.0));

    tree.attach_script(
        button,
        Box::new(GDScriptNodeInstance::from_source(BUTTON_GD, button).unwrap()),
    );
    if let Some(src) = svc_script {
        tree.attach_script(
            svc,
            Box::new(GDScriptNodeInstance::from_source(src, svc).unwrap()),
        );
    }
    (tree, button)
}

fn hits(tree: &SceneTree, button: NodeId) -> i64 {
    match script_var(tree, button, "hits") {
        Some(Variant::Int(n)) => n,
        other => panic!("hits: {other:?}"),
    }
}

/// **The control.** With no script on the container the click gets through —
/// without this, the veto test below passes against an engine that forwards
/// nothing at all.
#[test]
fn an_unscripted_container_still_forwards_the_click() {
    let (mut tree, button) = scene(None);
    tree.push_input(&press(160.0, 160.0));
    assert_eq!(
        hits(&tree, button),
        1,
        "a click at global (160,160) maps to sub-viewport (30,30), the middle \
         of the Button — if this is 0 the fixture is broken and the veto test \
         below proves nothing"
    );
}

/// **The acceptance clause.** `_propagate_input_event` returning `false` stops
/// the click reaching the sub-viewport's Control.
#[test]
fn returning_false_stops_the_event_reaching_the_sub_scene() {
    let (mut tree, button) = scene(Some(
        "\
extends SubViewportContainer

func _propagate_input_event(event):
    return false
",
    ));
    tree.push_input(&press(160.0, 160.0));
    assert_eq!(
        hits(&tree, button),
        0,
        "the container vetoed forwarding, so the Button inside the sub-viewport \
         must never see the event"
    );
}

/// 🔑 Returning `true` — and defining nothing — both forward.
///
/// ⚠️ The `true` case is what separates "the veto is honoured" from "any script
/// on the container blocks input". An implementation that treated *any*
/// override as a veto, or that swallowed the event whenever the method existed,
/// passes the test above and fails this one.
#[test]
fn returning_true_or_not_overriding_forwards_as_before() {
    let (mut tree, button) = scene(Some(
        "\
extends SubViewportContainer

func _propagate_input_event(event):
    return true
",
    ));
    tree.push_input(&press(160.0, 160.0));
    assert_eq!(
        hits(&tree, button),
        1,
        "🔑 an explicit `true` must forward — only `false` vetoes"
    );

    // A script that does not override it at all is likewise untouched:
    // `call_script_method` returns None for an undefined method.
    let (mut tree, button) = scene(Some(
        "\
extends SubViewportContainer
var unused = 0
",
    ));
    tree.push_input(&press(160.0, 160.0));
    assert_eq!(
        hits(&tree, button),
        1,
        "🔑 a scripted container that does not override the hook must behave \
         exactly like an unscripted one"
    );
}
