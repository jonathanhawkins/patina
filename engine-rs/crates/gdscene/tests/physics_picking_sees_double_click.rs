//! pat-s3rrv.76 — a collider's `_input_event` receives the real `double_click`
//! flag, not a hardcoded `false`.
//!
//! Godot delivers a double click to `CollisionObject2D._input_event` exactly as
//! it does to `_input`; `InputEventMouseButton.double_click` is a property of
//! the event, and picking is just another dispatch stage for that same event
//! (`viewport.cpp:3464-3580`, where picking is the LAST input stage).
//!
//! ⚠️ Patina computed the flag correctly in `SceneTree::push_input` and then
//! passed a literal `false` to both physics-picking payloads
//! (`main_loop.rs`). So `_input` saw double clicks and a collider never did —
//! the failure only shows up in the one stage nobody had a test for, and it
//! looked deliberate because a comment justified the `false`.
//!
//! **Why reading the mirrored flag is ordering-safe** — this is the question the
//! fix turns on. `MainLoop::step` runs, per event, in one loop iteration:
//!
//! ```text
//! self.tree.push_input(&event);        // computes double_click for THIS event
//! self.physics_pick_input(&event);     // 2D picking  — reads it
//! self.physics_pick_input_3d(&event);  // 3D picking  — reads it
//! ```
//!
//! so the value picking reads was computed from the very event it is handling,
//! microseconds earlier — never a stale read from the previous press. Were the
//! order ever reversed, picking would silently report the PREVIOUS event's flag,
//! which is worse than always-false because it is wrong only sometimes. The
//! second test below is what would catch that.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton, DOUBLE_TAP_FRAMES};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Records the flag from the event object itself, exactly as a project would.
///
/// `last_double` is stored rather than counted so each click can be asserted in
/// sequence — "some call saw a double" would not distinguish the flag arriving
/// on the right press from it arriving on the wrong one.
const PICKABLE: &str = "extends Area2D\n\
                        var calls = 0\n\
                        var last_double = false\n\
                        func _input_event(viewport, event, shape_idx):\n\
                        \tcalls = calls + 1\n\
                        \tlast_double = event.double_click\n";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .unwrap_or_else(|| panic!("node has no script attached"))
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no variable named `{name}`"))
        .default_value
}

fn script_int(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

fn click(position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position,
    }
}

/// An `Area2D` with a 40x40 shape centred at (50,50), scripted with Godot's
/// documented 2D picking signature.
fn pickable() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let area = tree.add_child(root, Node::new("Coin", "Area2D")).unwrap();
    tree.get_node_mut(area)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();
    tree.attach_script(
        area,
        Box::new(GDScriptNodeInstance::from_source(PICKABLE, area).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // Picking runs in the input stage, which precedes physics, so the collider
    // must be in the world before the first step — otherwise the click hits
    // nothing and every assertion below passes vacuously in the negative.
    ml.register_physics_bodies();
    (ml, area)
}

fn click_and_step(ml: &mut MainLoop) {
    ml.push_event(click(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);
}

// ---------------------------------------------------------------------------

/// 🔑 The acceptance. Two clicks a frame apart, and the collider sees the second
/// as a double.
#[test]
fn a_second_click_inside_the_window_reaches_the_collider_as_a_double() {
    let (mut ml, area) = pickable();

    click_and_step(&mut ml);
    assert_eq!(
        script_int(&ml, area, "calls"),
        1,
        "the handler must run for a click inside the collider — a 0 here means \
         picking never dispatched and the real assertion below is meaningless",
    );
    assert_eq!(
        script_var(&ml, area, "last_double"),
        Variant::Bool(false),
        "the FIRST click of a pair is not a double click",
    );

    // One frame apart, well inside the 15-frame window.
    click_and_step(&mut ml);
    assert_eq!(script_int(&ml, area, "calls"), 2, "the handler ran twice");
    assert_eq!(
        script_var(&ml, area, "last_double"),
        Variant::Bool(true),
        "⚠️ THE BEAD. The second click must arrive at the collider with \
         `double_click == true`. This is the assertion that fails against the \
         old tree, where both picking sites passed a literal `false` and a \
         collider could never observe a double click at all",
    );
}

/// ⚠️ The guard that makes the assertion above mean something.
///
/// An implementation that hardcoded `true`, or one that read a stale flag left
/// over from an earlier press, passes the acceptance and fails here.
#[test]
fn a_click_outside_the_window_is_not_reported_as_a_double() {
    let (mut ml, area) = pickable();

    click_and_step(&mut ml);
    assert_eq!(
        script_var(&ml, area, "last_double"),
        Variant::Bool(false),
        "a lone click is not a double",
    );

    // Step past the window. The counter advances once per `step`, so this puts
    // the next click DOUBLE_TAP_FRAMES + 1 frames after the first — derived from
    // the constant rather than hardcoded, so the test follows if it is retuned.
    for _ in 0..=DOUBLE_TAP_FRAMES {
        ml.step(1.0 / 60.0);
    }

    click_and_step(&mut ml);
    assert_eq!(script_int(&ml, area, "calls"), 2, "the handler ran twice");
    assert_eq!(
        script_var(&ml, area, "last_double"),
        Variant::Bool(false),
        "a click {} frames after the previous one is outside the double-click \
         window and must NOT be reported as a double",
        DOUBLE_TAP_FRAMES + 1,
    );
}
