//! pat-0q2kx.37 acceptance: a GDScript on a `RigidBody3D` can actually drive it.
//!
//! `gdphysics3d`'s body had the whole force/impulse/sleep API, but nothing
//! connected a scene node to it: the interpreter's `apply_*` arms coerce every
//! argument through `vec2_arg`, so a `Vector3` arrived as ZERO, and
//! `PhysicsServer3D` never re-read *any* rigid-body property after registration.
//! Every 3D script interaction was a silent no-op.
//!
//! These drive real `MainLoop::step` frames and assert on tree/script state. Each
//! test that claims "the script did it" carries a control run without the script,
//! so a pass cannot come from gravity alone.

use gdcore::math::Vector3;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance, LifecycleManager};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// `Root -> Ball (RigidBody3D, sphere r=0.5)` at `y`, with `script` attached to the
/// ball when given, and `props` applied to the ball node first.
fn ball_scene(y: f32, props: &[(&str, Variant)], script: Option<&str>) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let ball = tree
        .add_child(root, Node::new("Ball", "RigidBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, ball, Vector3::new(0.0, y, 0.0));
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(ball, shape).unwrap();

    for (key, value) in props {
        if let Some(node) = tree.get_node_mut(ball) {
            node.set_property(key, value.clone());
        }
    }
    if let Some(src) = script {
        tree.attach_script(
            ball,
            Box::new(GDScriptNodeInstance::from_source(src, ball).unwrap()),
        );
    }
    // `MainLoop::step` dispatches `_process` / `_physics_process` but **not**
    // `_ready` — entering the tree is what fires it (`LifecycleManager::enter_tree`
    // → `process_script_ready`), exactly as the engine does when a scene is added.
    // Without this the `_ready` bodies below would never run and the tests would
    // pass or fail for reasons unrelated to the forces they mean to check.
    LifecycleManager::enter_tree(&mut tree, root);
    (MainLoop::new(tree), ball)
}

fn run(ml: &mut MainLoop, ticks: usize) {
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
}

/// **The acceptance test.** `apply_central_impulse(Vector3(0, 10, 0))` from
/// `_ready` lifts the body instead of doing nothing.
#[test]
fn physics3d_rigidbody_script_forces() {
    let src = "\
extends RigidBody3D
func _ready():
    apply_central_impulse(Vector3(0, 10, 0))
";
    let (mut ml, ball) = ball_scene(5.0, &[], Some(src));
    run(&mut ml, 30);
    let lifted = node3d::get_position(ml.tree(), ball);

    // Control: identical scene, no script — gravity only.
    let (mut control_ml, control_ball) = ball_scene(5.0, &[], None);
    run(&mut control_ml, 30);
    let fell = node3d::get_position(control_ml.tree(), control_ball);

    assert!(
        fell.y < 5.0,
        "control: an unscripted body must fall, got {fell:?}",
    );
    assert!(
        lifted.y > 5.0,
        "apply_central_impulse(Vector3(0, 10, 0)) must lift the body above its \
         start height in 30 ticks: got {lifted:?} vs unscripted {fell:?}",
    );

    // The one-shot force must not keep pushing: `_pending_impulse` is drained and
    // cleared, so the body is in free fall again by the end.
    assert_eq!(
        ml.tree()
            .get_node(ball)
            .unwrap()
            .get_property("_pending_impulse"),
        Variant::Vector3(Vector3::ZERO),
        "the pending impulse must be cleared after the tick that drained it",
    );
}

/// Writing `linear_velocity` from a script moves the body along that axis. This
/// also covers the unauthored-property gate: nothing seeds `linear_velocity`, so
/// without a class default the write would land in a script local.
#[test]
fn script_written_linear_velocity_moves_the_body() {
    let src = "\
extends RigidBody3D
func _ready():
    linear_velocity = Vector3(3, 0, 0)
";
    let (mut ml, ball) = ball_scene(5.0, &[], Some(src));
    run(&mut ml, 30);
    let moved = node3d::get_position(ml.tree(), ball);

    assert!(
        moved.x > 0.5,
        "`linear_velocity = Vector3(3, 0, 0)` must move the body +X, got {moved:?}",
    );
    // Simulated velocity is published back, so a script can read what physics did.
    assert!(
        matches!(
            ml.tree().get_node(ball).unwrap().get_property("linear_velocity"),
            Variant::Vector3(v) if v.x > 0.5
        ),
        "the simulated linear_velocity must be published back onto the node",
    );
}

/// With `contact_monitor` on and a non-zero `max_contacts_reported`, landing on a
/// static floor fires `body_entered` exactly once — and resting on it does not
/// re-fire.
#[test]
fn contact_monitor_fires_body_entered_once_on_landing() {
    let src = "\
extends RigidBody3D
var hits = 0
func _on_body_entered(body):
    self.hits = self.hits + 1
";
    let (mut ml, ball) = ball_scene(
        0.0,
        &[
            ("contact_monitor", Variant::Bool(true)),
            ("max_contacts_reported", Variant::Int(4)),
        ],
        Some(src),
    );

    // A static floor whose top face is at y = -1.5, one unit below the ball.
    {
        let tree = ml.tree_mut();
        // The ball's parent, rather than a path string, so the fixture does not
        // depend on what the tree root is named.
        let root = tree.get_node(ball).unwrap().parent().unwrap();
        let floor = tree
            .add_child(root, Node::new("Floor", "StaticBody3D"))
            .unwrap();
        node3d::set_position(tree, floor, Vector3::new(0.0, -2.0, 0.0));
        let mut floor_shape = Node::new("CollisionShape", "CollisionShape3D");
        floor_shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
        floor_shape.set_property("size", Variant::Vector3(Vector3::new(10.0, 1.0, 10.0)));
        tree.add_child(floor, floor_shape).unwrap();
        // The monitoring body emits, and the connection dispatches to its own
        // script — the shape a `.tscn` `[connection … to="."]` produces.
        tree.connect_signal(
            ball,
            "body_entered",
            Connection::new(ball.object_id(), "_on_body_entered"),
        );
    }

    run(&mut ml, 60);
    let landed = script_var(&ml, ball, "hits");
    assert_eq!(
        landed,
        Some(Variant::Int(1)),
        "landing on the floor must fire body_entered exactly once, got {landed:?}",
    );

    // Resting on the floor for another second must not re-fire it.
    run(&mut ml, 60);
    assert_eq!(
        script_var(&ml, ball, "hits"),
        Some(Variant::Int(1)),
        "a body already resting on the floor must not re-enter it every tick",
    );
}

/// `sleeping = true` holds the body in place, and an impulse wakes it — which is
/// only true because the sleep flag is written *before* pending forces are
/// drained, so the impulse's `wake_up()` wins.
#[test]
fn a_sleeping_body_stays_put_until_an_impulse_wakes_it() {
    let src = "\
extends RigidBody3D
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 20:
        apply_central_impulse(Vector3(0, 6, 0))
";
    let (mut ml, ball) = ball_scene(5.0, &[("sleeping", Variant::Bool(true))], Some(src));

    // One tick: the authored flag must survive the frame that registers the body.
    // Registration seeds a body whose own `sleeping` defaults to false, and the end
    // of the same tick publishes the body's state onto the node — so unless the
    // node's value is adopted *during* registration, this write-back erases it and
    // the body is awake for good.
    run(&mut ml, 1);
    assert_eq!(
        ml.tree().get_node(ball).unwrap().get_property("sleeping"),
        Variant::Bool(true),
        "the scene's authored `sleeping` must survive the registration tick",
    );

    run(&mut ml, 14);
    let asleep = node3d::get_position(ml.tree(), ball);
    assert!(
        (asleep.y - 5.0).abs() < 1e-3,
        "a sleeping body must not fall, got {asleep:?}",
    );

    // Past tick 20 the script's impulse lands; the body wakes and rises.
    run(&mut ml, 25);
    let woken = node3d::get_position(ml.tree(), ball);
    assert!(
        woken.y > 5.0,
        "an impulse must wake a sleeping body and lift it, got {woken:?}",
    );
    assert_eq!(
        ml.tree().get_node(ball).unwrap().get_property("sleeping"),
        Variant::Bool(false),
        "the woken state must be published back to the node",
    );
}
