//! pat-s3rrv.103 — `CharacterBody3D.apply_floor_snap()` as a script method.
//!
//! Godot exposes the floor snap as its own callable, not only as an internal
//! phase of `move_and_slide` (`CharacterBody3D.xml`). A script uses it after
//! teleporting, or to stay glued to a slope while standing still.
//!
//! # Why the standing case is the whole test
//!
//! The solver already snaps inside `move_and_slide` — `character.rs` calls
//! `apply_floor_snap` in its `grounded_mode` branch — so a body that is
//! *moving* was already being snapped and would prove nothing here. The
//! standalone method only adds anything when the body is **idle**, which is
//! exactly the case the character pass used to skip: it `continue`d on
//! near-zero velocity before ever building the solver state.
//!
//! ⚠️ That skip is why the request has to defeat the idle check rather than
//! ride along with it. Gating the snap on velocity would leave the method a
//! no-op in the only situation it exists for — and it would look implemented,
//! because the moving case keeps working.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Sphere radius, and therefore the distance from the body's centre to the
/// floor when it is resting on it.
const RADIUS: f32 = 0.5;
/// Floor top face. The box is centred half its height below this.
const FLOOR_TOP: f32 = 0.0;
/// Where a body resting on the floor sits.
const RESTING_Y: f32 = FLOOR_TOP + RADIUS;
/// Start height: a gap of 0.2 above resting, well inside the authored snap
/// length below and well outside the default 0.1 (so a body using the default
/// would NOT reach the floor — the fixture has to author it).
const START_Y: f32 = RESTING_Y + 0.2;
const SNAP_LENGTH: f32 = 0.5;

/// A `CharacterBody3D` hovering `0.2` above a floor, with `script` attached.
fn hovering_scene(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // Box centred at -0.5 with height 1 => top face at y = 0.
    let mut floor = Node::new("Floor", "StaticBody3D");
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -0.5, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut floor_shape = Node::new("FloorShape", "CollisionShape3D");
    floor_shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    floor_shape.set_property("size", Variant::Vector3(Vector3::new(200.0, 1.0, 200.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let player = tree
        .add_child(root, Node::new("Player", "CharacterBody3D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(RADIUS as f64));
    tree.add_child(player, shape).unwrap();
    if let Some(n) = tree.get_node_mut(player) {
        // ⚠️ Authored, not defaulted. `DEFAULT_FLOOR_SNAP_LENGTH` is 0.1 and the
        // gap here is 0.2, so a fixture that forgot this would see no snap and
        // read as "the method does nothing".
        n.set_property("floor_snap_length", Variant::Float(SNAP_LENGTH as f64));
    }
    node3d::set_position(&mut tree, player, Vector3::new(0.0, START_Y, 0.0));
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(script, player).unwrap()),
    );
    (MainLoop::new(tree), player)
}

/// Calls `apply_floor_snap()` on the first physics frame and never moves.
///
/// `velocity` is left at zero throughout, so any downward motion observed can
/// only have come from the snap.
const SNAPPER: &str = "\
extends CharacterBody3D
var calls = 0
var returned = 0
func _physics_process(delta):
    if self.calls == 0:
        self.calls = 1
        apply_floor_snap()
        self.returned = 1
";

/// The control: identical, but never asks.
const IDLER: &str = "\
extends CharacterBody3D
var calls = 0
func _physics_process(delta):
    self.calls = self.calls + 1
";

fn y_of(ml: &MainLoop, node: NodeId) -> f32 {
    node3d::get_position(ml.tree(), node).y
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("player node")
        .get_property(name)
}

// ===========================================================================

/// **The acceptance.** A standing body asked to snap lands on the floor.
#[test]
fn character_body_3d_apply_floor_snap() {
    let (mut ml, player) = hovering_scene(SNAPPER);
    assert!(
        (y_of(&ml, player) - START_Y).abs() < 1e-4,
        "fixture must start hovering at {START_Y}"
    );

    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }

    // The script really ran — otherwise "it did not move" below would be about
    // a script that never executed rather than about the snap.
    assert_eq!(
        prop(&ml, player, "calls"),
        Variant::Int(1),
        "the script must have called apply_floor_snap() exactly once"
    );
    // ⚠️ Set AFTER the call. If `apply_floor_snap()` raised — because the name
    // never reached `call_node_method` — the script aborts mid-body and this
    // stays 0, while `calls` above is already 1. That distinguishes "the method
    // is not wired" from "it is wired and did nothing", which otherwise look
    // identical from the body's position alone.
    assert_eq!(
        prop(&ml, player, "returned"),
        Variant::Int(1),
        "apply_floor_snap() must return normally — 0 here means the call \
         raised and the script never got past it"
    );

    let end = y_of(&ml, player);
    assert!(
        (end - RESTING_Y).abs() < 0.05,
        "apply_floor_snap() must pull the body down onto the floor: expected \
         ~{RESTING_Y}, got {end} (started at {START_Y})"
    );
    assert_eq!(
        prop(&ml, player, "is_on_floor"),
        Variant::Bool(true),
        "and the body must report itself on the floor afterwards"
    );
}

/// The control: without the call, an idle body stays exactly where it is.
///
/// This is what makes the acceptance meaningful. A `CharacterBody3D` is never
/// auto-moved — Godot only moves it when the script asks — so if the body fell
/// on its own, the assertion above would pass without `apply_floor_snap()`
/// doing anything at all.
#[test]
fn character_body_3d_apply_floor_snap_idle_body_does_not_move() {
    let (mut ml, player) = hovering_scene(IDLER);

    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        matches!(prop(&ml, player, "calls"), Variant::Int(n) if n > 0),
        "the control's script must have run too"
    );
    let end = y_of(&ml, player);
    assert!(
        (end - START_Y).abs() < 1e-4,
        "an idle CharacterBody3D that never asks to snap must not move: \
         started at {START_Y}, ended at {end}"
    );
    // ⚠️ `assert_ne!` against `true`, not `assert_eq!` against `false`. An idle
    // body never enters the character pass at all, so nothing ever publishes
    // `is_on_floor` and the property reads `Nil` rather than `Bool(false)` —
    // which is a correct absence, not a wrong answer.
    assert_ne!(
        prop(&ml, player, "is_on_floor"),
        Variant::Bool(true),
        "a body that never reached the floor must not claim to be on it"
    );
}

/// The request is one-shot: it snaps once, not every frame.
///
/// Mirrors `_move_and_slide_requested`. Without the clear, a single call would
/// leave the body permanently glued, which diverges the moment a script wants
/// to jump.
#[test]
fn character_body_3d_apply_floor_snap_is_one_shot() {
    let (mut ml, player) = hovering_scene(SNAPPER);
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        (y_of(&ml, player) - RESTING_Y).abs() < 0.05,
        "the body snapped down on the first frame"
    );

    // Lift it back up. If the request were still latched, the next frames would
    // drag it down again.
    node3d::set_position(ml.tree_mut(), player, Vector3::new(0.0, START_Y, 0.0));
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }

    let end = y_of(&ml, player);
    assert!(
        (end - START_Y).abs() < 1e-4,
        "the snap request must be consumed by the frame that served it — the \
         body was lifted back to {START_Y} and must stay there, but it is at \
         {end}"
    );
}
