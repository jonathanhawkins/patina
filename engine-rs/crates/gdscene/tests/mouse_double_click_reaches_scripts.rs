//! pat-s3rrv.76 — `InputEventMouseButton.double_click` is computed.
//!
//! `input_event_to_variant` hardcoded `double_click: false` for every mouse
//! button event, and nothing anywhere tracked a click window for the mouse. The
//! touch path had `would_be_double_tap` in `InputState`; the mouse had no
//! equivalent, so a script could never see a double click and every
//! double-click-to-activate interaction was dead.
//!
//! The flag is computed in `SceneTree::push_input` rather than carried on the
//! event: `InputEvent::MouseButton` has 71 construction sites across six crates,
//! so widening it would be a far larger change than this flag warrants.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton, DOUBLE_TAP_FRAMES};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const WATCHER: &str = "\
extends Node
var clicks = 0
var doubles = 0
func _input(event):
    if event is InputEventMouseButton and event.pressed:
        self.clicks = self.clicks + 1
        if event.double_click:
            self.doubles = self.doubles + 1
";

fn watcher() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Watcher", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(WATCHER, node).expect("script parses")),
    );
    (MainLoop::new(tree), node)
}

fn var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn press(ml: &mut MainLoop, button: MouseButton) {
    ml.push_event(InputEvent::MouseButton {
        button,
        pressed: true,
        position: Vector2::new(4.0, 4.0),
    });
    ml.step(1.0 / 60.0);
}

#[test]
fn a_second_click_inside_the_window_is_a_double_click() {
    let (mut ml, node) = watcher();
    press(&mut ml, MouseButton::Left);
    assert_eq!(
        var(&ml, node, "clicks"),
        Variant::Int(1),
        "the first press must reach `_input`, or nothing below is measured"
    );
    assert_eq!(
        var(&ml, node, "doubles"),
        Variant::Int(0),
        "a FIRST press is never a double click"
    );

    press(&mut ml, MouseButton::Left);
    assert_eq!(var(&ml, node, "clicks"), Variant::Int(2));
    assert_eq!(
        var(&ml, node, "doubles"),
        Variant::Int(1),
        "the second press one frame later must carry double_click = true. A 0 \
         here is the old behaviour: the field was hardcoded false"
    );
}

/// ⚠️ The window must actually bound it. Without this, an implementation that
/// returns `true` for any repeat press — however far apart — passes the test
/// above and is wrong for every ordinary pair of separate clicks.
#[test]
fn a_click_past_the_window_is_not_a_double_click() {
    let (mut ml, node) = watcher();
    press(&mut ml, MouseButton::Left);
    for _ in 0..(DOUBLE_TAP_FRAMES + 4) {
        ml.step(1.0 / 60.0);
    }
    press(&mut ml, MouseButton::Left);

    assert_eq!(
        var(&ml, node, "clicks"),
        Variant::Int(2),
        "both presses land"
    );
    assert_eq!(
        var(&ml, node, "doubles"),
        Variant::Int(0),
        "past DOUBLE_TAP_FRAMES the second press is a fresh click"
    );
}

/// ⚠️ Godot measures the window against the previous press of the SAME button,
/// so chording left-then-right is not a double click. An implementation that
/// tracked one global "last press frame" would report `true` here.
#[test]
fn two_different_buttons_are_not_a_double_click() {
    let (mut ml, node) = watcher();
    press(&mut ml, MouseButton::Left);
    press(&mut ml, MouseButton::Right);

    assert_eq!(
        var(&ml, node, "clicks"),
        Variant::Int(2),
        "both presses land"
    );
    assert_eq!(
        var(&ml, node, "doubles"),
        Variant::Int(0),
        "left then right is two separate clicks, not a double click"
    );
}
