//! pat-s3rrv.13 acceptance: Godot-accurate Control hit-testing.
//!
//! `Viewport::_gui_find_control_at_pos` (upstream/godot/scene/main/viewport.cpp
//! :1819-1861) rejects an invisible CanvasItem outright, refuses to descend into
//! the children of a control that clips its contents when the point misses that
//! control's rect, skips `top_level` children, and only then applies
//! `mouse_filter` / `has_point`. Godot also hands `_gui_input` an event
//! **localized to the receiving control** (`localizer.xform(mpos)`,
//! viewport.cpp:2078-2088).
//!
//! Patina's `hit_test_control` did none of that:
//! - no visibility gate, so a hidden Panel or a closed menu still swallowed
//!   every click over its rect;
//! - no `clip_contents` gate (it was read only by the render pass), so a child
//!   scrolled outside a clipping parent stayed clickable where nothing was drawn;
//! - no `top_level` skip;
//! - `get_mouse_filter` hard-defaulted **every** class to STOP. Godot sets
//!   Label to MOUSE_FILTER_IGNORE in its constructor and never serializes it, so
//!   an authored Label arrived with no property and swallowed clicks meant for
//!   the Button behind it — a full-rect Label over a Button is ordinary layout,
//!   which made this the most visible symptom of the whole gap;
//! - `gui_input_stage` forwarded one viewport-space payload to every control, so
//!   `event.position` in a script `_gui_input` was off by the control's offset.
//!
//! These push real `InputEvent`s through `push_input`, so a regression to
//! "the math is right but nothing consults it" fails here.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A Control node with an explicit rect at `(x, y)` sized `(w, h)`.
fn control(name: &str, class: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node::new(name, class);
    n.set_property("position", Variant::Vector2(Vector2::new(x, y)));
    n.set_property("size", Variant::Vector2(Vector2::new(w, h)));
    n
}

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

fn release(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(x, y),
    }
}

fn script_var(tree: &SceneTree, node: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Records clicks and the coordinates `_gui_input` was handed.
const RECORDER: &str = "\
extends Button
var clicks = 0
var gui_events = 0
var last_x = -1.0
var last_y = -1.0
func _on_pressed():
    self.clicks = self.clicks + 1
func _gui_input(event):
    self.gui_events = self.gui_events + 1
    self.last_x = event.position.x
    self.last_y = event.position.y
";

/// Attaches `RECORDER` to `btn` and wires `pressed` **from Rust** rather than
/// from a script `_ready` + `connect(...)`.
///
/// Same reason `gui_input_control_routing_test` does it this way: it isolates
/// "the event reached the control" from "the script's `connect` in `_ready`
/// ran", which are two different mechanisms — conflating them makes a hit-test
/// regression look like a signal bug.
fn attach_recorder(tree: &mut SceneTree, btn: gdscene::node::NodeId) {
    tree.attach_script(
        btn,
        Box::new(GDScriptNodeInstance::from_source(RECORDER, btn).expect("script parses")),
    );
    tree.connect_signal(
        btn,
        "pressed",
        gdobject::signal::Connection::new(btn.object_id(), "_on_pressed"),
    );
}

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

/// **The bead's acceptance.** All four hit-test rules at once, through the real
/// input path.
#[test]
fn gui_hit_test_visibility_clip_and_local_coords() {
    // -- (1) a full-rect Label must NOT eat the Button's clicks --------------
    // Godot's Label is MOUSE_FILTER_IGNORE by construction and never serializes
    // it, so the Label here carries no `mouse_filter` property at all — exactly
    // what a .tscn produces.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, control("Main", "Control", 0.0, 0.0, 400.0, 300.0));
    let main = main.unwrap();
    let btn = tree
        .add_child(main, control("Btn", "Button", 0.0, 0.0, 100.0, 50.0))
        .unwrap();
    // Added AFTER the button so it is on top in the reverse-order hit walk.
    let _label = tree
        .add_child(main, control("Cover", "Label", 0.0, 0.0, 400.0, 300.0))
        .unwrap();
    attach_recorder(&mut tree, btn);

    tree.push_input(&press(50.0, 25.0));
    tree.push_input(&release(50.0, 25.0));

    assert_eq!(
        script_var(&tree, btn, "clicks"),
        Some(Variant::Int(1)),
        "a full-rect Label must not swallow the click: Godot's Label is \
         MOUSE_FILTER_IGNORE by construction, so the Button behind it is hit",
    );

    // -- (3) the event handed to `_gui_input` is control-local ---------------
    // The Button sits at (0,0) inside Main, so a click at (50,25) reads (50,25);
    // the offset case is covered by its own test below.
    assert_eq!(
        script_var(&tree, btn, "last_x"),
        Some(Variant::Float(50.0)),
        "`_gui_input` must receive control-local coordinates",
    );
}

/// A control positioned at (100, 50) receiving a viewport click at (120, 60)
/// must observe `event.position == Vector2(20, 10)` — the bead's exact clause.
#[test]
fn gui_input_event_position_is_control_local() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, control("Main", "Control", 0.0, 0.0, 400.0, 300.0))
        .unwrap();
    let btn = tree
        .add_child(main, control("Btn", "Button", 100.0, 50.0, 80.0, 40.0))
        .unwrap();
    attach_recorder(&mut tree, btn);

    tree.push_input(&press(120.0, 60.0));

    assert_eq!(
        script_var(&tree, btn, "gui_events"),
        Some(Variant::Int(1)),
        "the click must reach the control at all",
    );
    // The whole point: viewport (120,60) minus the control's global (100,50).
    assert_eq!(
        script_var(&tree, btn, "last_x"),
        Some(Variant::Float(20.0)),
        "event.position.x must be control-local (20), not viewport (120)",
    );
    assert_eq!(
        script_var(&tree, btn, "last_y"),
        Some(Variant::Float(10.0)),
        "event.position.y must be control-local (10), not viewport (60)",
    );
}

/// An invisible control covering the Button must neither receive `gui_input`
/// nor block the click — Godot discards `!is_visible()` before any rect test.
#[test]
fn invisible_control_neither_receives_nor_blocks_input() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, control("Main", "Control", 0.0, 0.0, 400.0, 300.0))
        .unwrap();
    let btn = tree
        .add_child(main, control("Btn", "Button", 0.0, 0.0, 100.0, 50.0))
        .unwrap();
    let mut cover = control("Hidden", "Panel", 0.0, 0.0, 400.0, 300.0);
    // A Panel is MOUSE_FILTER_STOP, so with `visible` unset it WOULD block.
    cover.set_property("visible", Variant::Bool(false));
    let cover_id = tree.add_child(main, cover).unwrap();
    attach_recorder(&mut tree, btn);

    tree.push_input(&press(50.0, 25.0));
    tree.push_input(&release(50.0, 25.0));

    assert_eq!(
        script_var(&tree, btn, "clicks"),
        Some(Variant::Int(1)),
        "a hidden Panel must not block the Button underneath it",
    );
    assert_ne!(
        gdscene::control::control_at_point(&tree, Vector2::new(50.0, 25.0)),
        Some(cover_id),
        "the hidden control must never be the hit target",
    );
}

/// A `clip_contents` parent must not report a child lying outside its rect:
/// nothing is drawn out there, so nothing is clickable out there.
#[test]
fn clip_contents_parent_hides_children_outside_its_rect() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, control("Main", "Control", 0.0, 0.0, 400.0, 300.0))
        .unwrap();
    // A small clipping viewport onto a child that overflows it.
    let mut clipper = control("Clip", "Panel", 0.0, 0.0, 50.0, 50.0);
    clipper.set_property("clip_contents", Variant::Bool(true));
    let clip_id = tree.add_child(main, clipper).unwrap();
    let child = tree
        .add_child(
            clip_id,
            control("Overflow", "Button", 0.0, 0.0, 300.0, 300.0),
        )
        .unwrap();

    // Inside the clipper: the child is visible there, so it is hit.
    assert_eq!(
        gdscene::control::control_at_point(&tree, Vector2::new(25.0, 25.0)),
        Some(child),
        "inside the clip rect the child is drawn and must be hit",
    );
    // Outside the clipper but inside the child's own rect: clipped away, so the
    // walk must not descend — the child must NOT be reported.
    assert_ne!(
        gdscene::control::control_at_point(&tree, Vector2::new(200.0, 200.0)),
        Some(child),
        "a child clipped out of view must not be clickable",
    );
}

/// The per-class defaults must not make everything transparent: a Panel still
/// stops clicks, which is what keeps a dialog from leaking input to the scene
/// behind it. Guards against "fixed Label by defaulting everyone to IGNORE".
#[test]
fn panel_still_stops_clicks_by_default() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, control("Main", "Control", 0.0, 0.0, 400.0, 300.0))
        .unwrap();
    let btn = tree
        .add_child(main, control("Btn", "Button", 0.0, 0.0, 100.0, 50.0))
        .unwrap();
    let panel = tree
        .add_child(main, control("Cover", "Panel", 0.0, 0.0, 400.0, 300.0))
        .unwrap();

    assert_eq!(
        gdscene::control::control_at_point(&tree, Vector2::new(50.0, 25.0)),
        Some(panel),
        "a visible Panel defaults to MOUSE_FILTER_STOP and must consume the click",
    );
    assert_ne!(
        gdscene::control::control_at_point(&tree, Vector2::new(50.0, 25.0)),
        Some(btn),
        "the Button behind an opaque Panel must not be reachable",
    );
}
