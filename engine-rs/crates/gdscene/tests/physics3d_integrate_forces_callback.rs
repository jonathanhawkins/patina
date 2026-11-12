//! pat-s3rrv.105 — the `_integrate_forces` acceptance for RigidBody3D.
//!
//! Covers the bead's clauses (a), (b) and (d):
//!
//! * **(a)** the callback runs exactly once per physics tick;
//! * **(b)** with `custom_integrator` on, a script zeroing `state.linear_velocity`
//!   stops the body falling entirely;
//! * **(d)** `add_collision_exception_with` makes two otherwise-colliding bodies
//!   pass through each other.
//!
//! Clause **(c)** — `get_contact_count()` and `get_colliding_bodies()` under
//! `contact_monitor` — is deliberately NOT re-tested here. It is already covered
//! by `rigidbody3d_contact_monitor_script_surface`
//! (`a_monitoring_rigidbody3d_reports_contacts_and_colliding_bodies`) and by
//! `rigidbody3d_integrate_forces_contacts`, both green. Restating it would
//! duplicate those rather than prove anything new.
//!
//! # What (d) needed
//!
//! `gdphysics3d` has had collision exceptions and its solver has always
//! consulted them — the gap was that the script arm resolved only 2D bodies, so
//! `add_collision_exception_with` on a 3D body silently did nothing.
//!
//! ⚠️ Script-added exceptions live in their own set, separate from the
//! joint-owned one. `remove_joint` drops a pair's key once no joint wants it and
//! has no idea a script also asked; sharing the set would mean deleting a joint
//! silently re-enables a collision the script disabled.

use gdcore::id::ObjectId;
use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Builds `Root -> {Floor(StaticBody3D), Faller(RigidBody3D)}` with `script` on
/// the faller. The floor's top face is at y = 0; the sphere starts well above it.
fn scene(script: &str, custom_integrator: bool) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // Box centred at -1 with half-height 1 => top face at y = 0.
    let mut floor = Node::new("Floor", "StaticBody3D");
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -1.0, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("FloorShape", "CollisionShape3D");
    fs.set_property("shape", Variant::String("BoxShape3D".to_string()));
    fs.set_property("size", Variant::Vector3(Vector3::new(100.0, 2.0, 100.0)));
    tree.add_child(floor_id, fs).unwrap();

    let mut body = Node::new("Faller", "RigidBody3D");
    body.set_property("position", Variant::Vector3(Vector3::new(0.0, 3.0, 0.0)));
    body.set_property("custom_integrator", Variant::Bool(custom_integrator));
    let body_id = tree.add_child(root, body).unwrap();
    let mut bs = Node::new("Shape", "CollisionShape3D");
    bs.set_property("shape", Variant::String("SphereShape3D".to_string()));
    bs.set_property("radius", Variant::Float(0.5));
    tree.add_child(body_id, bs).unwrap();

    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(script, body_id).unwrap()),
    );
    (MainLoop::new(tree), body_id, floor_id)
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("node exists")
        .get_property(name)
}

fn y_of(ml: &MainLoop, node: NodeId) -> f32 {
    match prop(ml, node, "position") {
        Variant::Vector3(v) => v.y,
        other => panic!("position must be a Vector3, got {other:?}"),
    }
}

// ===========================================================================
// (a) + (b)
// ===========================================================================

/// **The acceptance.** The callback fires once per physics tick, and a script
/// that zeroes the velocity stops the body dead.
#[test]
fn physics3d_integrate_forces_callback() {
    // `custom_integrator` on: the script owns integration, so zeroing the
    // velocity is the whole of the body's motion for that tick.
    let src = "\
extends RigidBody3D
var calls = 0
func _integrate_forces(state):
    self.calls = self.calls + 1
    state.linear_velocity = Vector3(0, 0, 0)
";
    let (mut ml, body, _floor) = scene(src, true);
    let start_y = y_of(&ml, body);

    // The ticks are counted from each frame's own report rather than assumed:
    // a frame runs zero or several physics ticks depending on the accumulator,
    // so asserting "30" would pin the frame rate rather than the callback.
    let mut ticks: i64 = 0;
    for _ in 0..30 {
        ticks += ml.step(1.0 / 60.0).physics_steps as i64;
    }

    // -- (a) exactly once per PHYSICS tick -----------------------------------
    assert!(
        ticks > 0,
        "the loop must have run physics ticks for this to mean anything"
    );
    match prop(&ml, body, "calls") {
        // pat-29zst: once per tick from the body's SECOND tick. Godot calls
        // `_integrate_forces` from the flush that follows a step, and a body
        // that entered with the scene has no step before its first tick.
        Variant::Int(n) => assert_eq!(
            n,
            ticks - 1,
            "_integrate_forces must run exactly once per physics tick after the \
             body's first — got {n} calls across {ticks} ticks. Fewer means it \
             is gated on something extra; more means it is being dispatched per \
             frame AND per tick, or before the body's first step"
        ),
        other => panic!("`calls` must be an int, got {other:?}"),
    }

    // -- (b) the body does not fall ------------------------------------------
    // A tolerance rather than exact equality: the body may be repositioned by
    // its own solve, but with the velocity zeroed every tick it cannot travel.
    let end_y = y_of(&ml, body);
    assert!(
        (end_y - start_y).abs() < 0.01,
        "with custom_integrator and `state.linear_velocity = Vector3.ZERO` the \
         body must not fall: started at {start_y}, ended at {end_y}"
    );
}

/// The control for (b): the same scene WITHOUT the velocity write falls.
///
/// Without this, a body that never moved for an unrelated reason — gravity not
/// applied, the tick never running, the scene not registered — would satisfy the
/// "does not fall" assertion above and prove nothing.
#[test]
fn physics3d_integrate_forces_callback_falls_when_the_script_does_not_intervene() {
    let src = "\
extends RigidBody3D
var calls = 0
func _integrate_forces(state):
    self.calls = self.calls + 1
";
    let (mut ml, body, _floor) = scene(src, false);
    let start_y = y_of(&ml, body);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let end_y = y_of(&ml, body);

    assert!(
        end_y < start_y - 0.05,
        "a body with no velocity override must fall under gravity: started at \
         {start_y}, ended at {end_y}. If it did not, the 'does not fall' \
         assertion in the acceptance is vacuous"
    );
}

// ===========================================================================
// (d) collision exceptions
// ===========================================================================

/// Script source for the exception tests. `arm()` only records the target; the
/// script itself makes the `add_collision_exception_with` call the acceptance
/// names, from inside `_physics_process`.
const EXCEPTER: &str = "\
extends RigidBody3D
var have_target = false
var target = 0
var armed = false
func arm(other):
    self.target = other
    self.have_target = true
func _physics_process(delta):
    if self.have_target and not self.armed:
        self.armed = true
        add_collision_exception_with(self.target)
";

/// Runs the faller for `steps` ticks, optionally excepting the floor first.
///
/// ⚠️ **The exception is applied from inside `_physics_process`, not by calling
/// the script from Rust.** Two separate reasons, and the second cost a failing
/// run to find:
///
/// 1. Both nodes must already be REGISTERED bodies — a call before the first
///    sync resolves no body id and is silently dropped. Hence the priming step.
/// 2. `PhysicsServer3D` is only bound to the tree for scripts *during* a frame
///    (`main_loop` sets and clears the address around its script phases). A
///    `call_script_method` from the test runs outside that bracket, so
///    `physics_3d_mut()` is `None` and the exception goes nowhere — which looks
///    exactly like an exception that reached the solver and did nothing.
///
/// So `arm()` only stores the target, and the script applies it on the next
/// physics frame, which is also how a real game would spell it.
fn run_with_exception(except: bool, steps: usize) -> (MainLoop, NodeId, f32) {
    let (mut ml, body, floor) = scene(EXCEPTER, false);
    ml.step(1.0 / 60.0);
    if except {
        let arg = Variant::ObjectId(ObjectId::from_raw(floor.raw()));
        ml.tree_mut().call_script_method(body, "arm", &[arg]);
    }
    for _ in 0..steps {
        ml.step(1.0 / 60.0);
    }
    if except {
        assert_eq!(
            prop(&ml, body, "armed"),
            Variant::Bool(true),
            "`_physics_process` must have run and applied the exception — if it \
             did not, the height assertion below is about nothing"
        );
    }
    let y = y_of(&ml, body);
    (ml, body, y)
}

/// **(d)** An excepted body passes through the floor; without the exception it
/// lands on it.
#[test]
fn physics3d_integrate_forces_callback_collision_exception_lets_bodies_pass() {
    // The control first, so "it fell through" is measured against a run that
    // demonstrably does NOT.
    let (_, _, landed_y) = run_with_exception(false, 120);
    assert!(
        landed_y > -0.5,
        "without an exception the body must be stopped by the floor (top face \
         at y = 0), but it reached {landed_y}"
    );

    let (_, _, passed_y) = run_with_exception(true, 120);
    assert!(
        passed_y < -1.0,
        "with `add_collision_exception_with(floor)` the body must fall THROUGH \
         the floor, but it stopped at {passed_y} (control landed at \
         {landed_y}). A 3D body whose exception never reached the solver stops \
         at the same height as the control"
    );
}

/// Removing the exception restores the collision.
///
/// The other direction, and the one that catches an `add` that writes somewhere
/// the `remove` cannot reach.
#[test]
fn physics3d_integrate_forces_callback_removing_the_exception_restores_collision() {
    // Adds the exception on the first physics frame and drops it on the second,
    // both from inside the callback for the reason documented on
    // `run_with_exception`.
    let src = "\
extends RigidBody3D
var have_target = false
var target = 0
var frames = 0
func arm(other):
    self.target = other
    self.have_target = true
func _physics_process(delta):
    if not self.have_target:
        return
    self.frames = self.frames + 1
    if self.frames == 1:
        add_collision_exception_with(self.target)
    if self.frames == 2:
        remove_collision_exception_with(self.target)
";
    let (mut ml, body, floor) = scene(src, false);
    ml.step(1.0 / 60.0);
    let arg = Variant::ObjectId(ObjectId::from_raw(floor.raw()));
    ml.tree_mut().call_script_method(body, "arm", &[arg]);

    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        matches!(prop(&ml, body, "frames"), Variant::Int(n) if n >= 2),
        "both the add and the remove must have run"
    );
    assert!(
        y_of(&ml, body) > -0.5,
        "after remove_collision_exception_with the floor must stop the body \
         again, but it reached {}",
        y_of(&ml, body)
    );
}
