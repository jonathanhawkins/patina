//! pat-0q2kx.202 acceptance: 3D viewport object picking.
//!
//! Godot ground truth: `upstream/godot/scene/main/viewport.cpp:750`
//! `_process_picking` — unproject the pointer through the current `Camera3D`,
//! raycast the 3D space, and dispatch `input_event` plus
//! `mouse_entered`/`mouse_exited` to the hit `CollisionObject3D` when its
//! `input_ray_pickable` is set.
//!
//! Every piece already existed and **none of them were joined**:
//! `camera3d::project_ray_origin` (`:854`) and `project_ray_normal` (`:885`) had
//! zero callers anywhere, `PhysicsRayQuery3D` was only reachable from `RayCast3D`
//! nodes, and `input_ray_pickable` appeared solely as an inspector schema string.
//! So a click never reached a 3D body however the scene was authored.
//!
//! Written as an integration test rather than in-module for the reason the 2D
//! twin (`pick2d_signals.rs`) documents: `gdscene`'s `#[cfg(test)]` build has been
//! intermittently broken by unrelated files, and every API used here is public.
//!
//! ⚠️ NOT delivered, and reported as such: `shape_idx` in the `input_event`
//! argument list, and picking against `Area3D` (the ray query walks
//! `world().bodies()`). See the completion report.
//!
//! NOTE ON THE FILE NAME: the acceptance names `physics3d_viewport_picking`;
//! that is the test **function**. The binary is `physics3d_picking` so
//! `-p gdscene --test physics3d_picking` stays short.

use gdcore::math::{Vector2, Vector3};
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdscript_interop::bindings::{MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::Variant;
use std::cell::RefCell;
use std::rc::Rc;

type CallLog = Rc<RefCell<Vec<(String, Vec<Variant>)>>>;

/// Records picking callbacks and the arguments they carried.
///
/// ⚠️ The `_on_` filter is load bearing. A listener is a real scripted node, so
/// the engine also dispatches `_process`, `_input`, `_physics_process` and
/// friends to it every frame; recording those makes a "this body must receive
/// nothing" assertion match lifecycle noise and fail on correct behaviour.
struct RecordingScript {
    who: String,
    calls: CallLog,
}

impl ScriptInstance for RecordingScript {
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        if name.starts_with("_on_") {
            self.calls
                .borrow_mut()
                .push((format!("{}:{}", self.who, name), args.to_vec()));
        }
        Ok(Variant::Nil)
    }
    fn get_property(&self, _name: &str) -> Option<Variant> {
        None
    }
    fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
        false
    }
    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        Vec::new()
    }
    fn list_methods(&self) -> Vec<MethodInfo> {
        Vec::new()
    }
    fn get_script_name(&self) -> &str {
        "RecordingScript"
    }
    // Must be overridden: the trait default answers from `list_methods`, which is
    // empty here, so every dispatch would be skipped and the negative assertions
    // would pass vacuously — the one failure mode a recorder must not have.
    fn has_method(&self, _name: &str) -> bool {
        true
    }
}

/// The viewport the pointer positions below are expressed in.
const VIEW_W: u32 = 200;
const VIEW_H: u32 = 200;

/// `Root -> { Camera (current, at +z looking down -z), Box (StaticBody3D, a
/// 2x2x2 box at the origin), Listener }`, with the box's three picking signals
/// connected to the listener's script.
fn picking_scene(log: &CallLog) -> (MainLoop, NodeId) {
    picking_scene_with(log, true)
}

/// As above, but `with_camera` builds the scene without any `Camera3D`.
///
/// ⚠️ Clearing `current` on the only camera does NOT make the scene camera-less.
/// `current_camera3d` falls back to the first `Camera3D` in tree order, matching
/// Godot: a `.tscn` normally carries one camera with no explicit `current` and
/// still renders from it. Testing the no-camera path therefore means *omitting*
/// the node, not un-marking it.
fn picking_scene_with(log: &CallLog, with_camera: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // A default Camera3D looks down its own -Z, so placing it at +Z aims it at
    // the origin with no rotation to get wrong.
    if with_camera {
        let camera = tree
            .add_child(root, Node::new("Camera", "Camera3D"))
            .unwrap();
        node3d::set_position(&mut tree, camera, Vector3::new(0.0, 0.0, 10.0));
        tree.get_node_mut(camera)
            .unwrap()
            .set_property("current", Variant::Bool(true));
    }

    let body = tree
        .add_child(root, Node::new("Box", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, body, Vector3::ZERO);
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
    tree.add_child(body, shape).unwrap();

    // The listener owns no collider, so nothing it records can come from picking
    // dispatching to it directly — only from a connection actually firing.
    let listener = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    tree.attach_script(
        listener,
        Box::new(RecordingScript {
            who: "box".to_string(),
            calls: Rc::clone(log),
        }),
    );
    for (signal, method) in [
        ("input_event", "_on_input_event"),
        ("mouse_entered", "_on_mouse_entered"),
        ("mouse_exited", "_on_mouse_exited"),
    ] {
        tree.connect_signal(body, signal, Connection::new(listener.object_id(), method));
    }

    let mut ml = MainLoop::new(tree);
    ml.set_viewport_size(VIEW_W, VIEW_H);
    // ⚠️ One warm-up frame. Picking runs in the input stage, BEFORE physics, and
    // the 3D server registers bodies lazily inside `sync_to_physics` — so on a
    // fresh scene the world is still empty on frame 1 and a click would hit
    // nothing. Without this the whole file would pass vacuously in the negative
    // direction.
    ml.step(1.0 / 60.0);
    (ml, body)
}

fn click(position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position,
    }
}

fn motion(position: Vector2) -> InputEvent {
    InputEvent::MouseMotion {
        position,
        relative: Vector2::new(0.0, 0.0),
    }
}

fn names(log: &CallLog) -> Vec<String> {
    log.borrow().iter().map(|(n, _)| n.clone()).collect()
}

/// Screen centre — the ray through it hits the box dead on.
const CENTRE: Vector2 = Vector2 {
    x: VIEW_W as f32 / 2.0,
    y: VIEW_H as f32 / 2.0,
};
/// A corner, whose ray misses the 2x2x2 box at the origin entirely.
const OFF_BOX: Vector2 = Vector2 { x: 4.0, y: 4.0 };

/// **The acceptance.** Hovering fires `mouse_entered` once and `mouse_exited`
/// on leaving; a click fires `input_event` carrying a world position on the box
/// surface.
#[test]
fn physics3d_viewport_picking() {
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let (mut ml, _body) = picking_scene(&log);

    // -- hover on ------------------------------------------------------------
    ml.push_event(motion(CENTRE));
    ml.step(1.0 / 60.0);
    assert_eq!(
        names(&log)
            .iter()
            .filter(|n| *n == "box:_on_mouse_entered")
            .count(),
        1,
        "moving onto the box must fire `mouse_entered` exactly once, got {:?}",
        names(&log)
    );

    // Staying put must NOT re-fire it — hover is a transition, not a per-frame
    // state, and this is the half that needs stored state to get right.
    log.borrow_mut().clear();
    ml.push_event(motion(CENTRE));
    ml.step(1.0 / 60.0);
    assert!(
        !names(&log).iter().any(|n| n == "box:_on_mouse_entered"),
        "the pointer never left, so no second `mouse_entered` may fire, got {:?}",
        names(&log)
    );

    // -- the click, and its world position -----------------------------------
    log.borrow_mut().clear();
    ml.push_event(click(CENTRE));
    ml.step(1.0 / 60.0);

    let recorded = log.borrow().clone();
    let (_, args) = recorded
        .iter()
        .find(|(n, _)| n == "box:_on_input_event")
        .unwrap_or_else(|| {
            panic!(
                "a click on the box must fire `input_event`; before this bead the \
                 ray helpers had zero callers so nothing was ever dispatched. \
                 Recorded: {:?}",
                names(&log)
            )
        });

    // Godot's signature is (camera, event, event_position, normal, shape_idx).
    assert!(
        args.len() >= 4,
        "`input_event` must carry camera, event, position and normal, got {args:?}"
    );
    // Matched by reference: `Variant` is not `Copy`, so binding out of an index
    // expression would move out of the borrow.
    let Variant::Vector3(point) = &args[2] else {
        panic!(
            "argument 3 must be the world hit position, got {:?}",
            args[2]
        );
    };
    // The camera looks down -Z at a box spanning -1..1, so the ray strikes the
    // +Z face: z ≈ 1, and x/y ≈ 0 for a centre-screen ray. A position taken from
    // the wrong end of the ray (the camera, or the far plane) fails this.
    assert!(
        (point.z - 1.0).abs() < 1e-2,
        "the hit must be ON the box's near face (z = 1), got {point:?}"
    );
    assert!(
        point.x.abs() < 1e-2 && point.y.abs() < 1e-2,
        "a centre-screen ray must strike the middle of the face, got {point:?}"
    );

    // The surface normal points back at the camera.
    let Variant::Vector3(normal) = &args[3] else {
        panic!("argument 4 must be the surface normal, got {:?}", args[3]);
    };
    assert!(
        normal.z > 0.5,
        "the +Z face normal must point toward the camera, got {normal:?}"
    );

    // -- hover off -----------------------------------------------------------
    log.borrow_mut().clear();
    ml.push_event(motion(OFF_BOX));
    ml.step(1.0 / 60.0);
    assert!(
        names(&log).iter().any(|n| n == "box:_on_mouse_exited"),
        "moving off the box must fire `mouse_exited`, got {:?}",
        names(&log)
    );
    assert!(
        !names(&log).iter().any(|n| n == "box:_on_input_event"),
        "a motion that misses the box must not dispatch to it, got {:?}",
        names(&log)
    );
}

/// `input_ray_pickable = false` opts a body out entirely — the gate Godot
/// applies, and the reason the test above cannot pass merely by dispatching to
/// everything.
#[test]
fn an_unpickable_body_receives_nothing() {
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let (mut ml, body) = picking_scene(&log);
    ml.tree_mut()
        .get_node_mut(body)
        .unwrap()
        .set_property("input_ray_pickable", Variant::Bool(false));

    ml.push_event(motion(CENTRE));
    ml.step(1.0 / 60.0);
    ml.push_event(click(CENTRE));
    ml.step(1.0 / 60.0);

    assert!(
        names(&log).is_empty(),
        "an `input_ray_pickable = false` body must receive no picking traffic at \
         all — not the click, and not the hover either, got {:?}",
        names(&log)
    );
}

/// A body that never mentions `input_ray_pickable` still picks: Godot's default
/// is `true`, and a gate written as "the property must be present and true"
/// would silently make every ordinary scene unpickable.
#[test]
fn the_default_is_pickable() {
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let (mut ml, body) = picking_scene(&log);
    assert_eq!(
        ml.tree()
            .get_node(body)
            .unwrap()
            .get_property("input_ray_pickable"),
        Variant::Nil,
        "this test is only meaningful while the property is genuinely absent"
    );

    ml.push_event(click(CENTRE));
    ml.step(1.0 / 60.0);
    assert!(
        names(&log).iter().any(|n| n == "box:_on_input_event"),
        "a body with no `input_ray_pickable` property must still pick, got {:?}",
        names(&log)
    );
}

/// A scene with no `Camera3D` has no frustum to unproject through, so picking
/// is skipped rather than guessing a view.
#[test]
fn no_camera_means_no_picking() {
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let (mut ml, _body) = picking_scene_with(&log, false);
    assert!(
        ml.tree().all_nodes_in_tree_order().into_iter().all(|id| !ml
            .tree()
            .get_node(id)
            .is_some_and(|n| n.class_name() == "Camera3D")),
        "this test is only meaningful while the scene genuinely has no camera"
    );

    ml.push_event(motion(CENTRE));
    ml.step(1.0 / 60.0);
    ml.push_event(click(CENTRE));
    ml.step(1.0 / 60.0);
    assert!(
        names(&log).is_empty(),
        "a camera-less scene has no pick ray, got {:?}",
        names(&log)
    );
}
