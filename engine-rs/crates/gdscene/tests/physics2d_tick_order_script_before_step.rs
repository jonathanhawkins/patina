//! pat-0q2kx.201 — `_physics_process` runs BEFORE `PhysicsServer2D::step`,
//! which is Godot's `Main::iteration` order.
//!
//! Godot's per-tick sequence (`main.cpp`) is: sync/flush the servers, run the
//! scene tree's physics process (every `_physics_process`, and so every
//! velocity write, impulse and `move_and_slide`), then `end_sync` and
//! `PhysicsServer2D::step`. Patina's `MainLoop` used to step the solver FIRST
//! and run the script phase at the end of the tick, so a scripted velocity sat
//! on the node until the NEXT tick — one tick of latency on every scripted
//! body, and traces that could not match Godot step for step.
//!
//! 🔑 The assertion that matters is "after exactly ONE tick". A test that steps
//! several times and then checks the body moved passes under the old order too
//! — it is merely one tick behind. Only the first tick distinguishes them:
//!
//!   * correct order → x = 100 * dt after one tick
//!   * old order     → x = 0 after one tick (and 100 * dt after two)
//!
//! ⚠️ Motion is asserted along +X. Gravity pulls along +Y, so no amount of
//! default integration moves the body sideways: a positive X displacement can
//! only come from the velocity the script wrote.
//!
//! ⚠️ `linear_velocity` is seeded on the node in the fixture. That is
//! load-bearing, not decoration: the interpreter only routes `self.x = ...` to
//! the *node* when the property already exists and is non-Nil (`interpreter.rs`
//! `is_node_prop`). Without the seed the script's assignment lands in a script
//! local and physics never sees it, and the test would fail for a reason that
//! has nothing to do with tick order.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
// `get_property` is a `ScriptInstance` trait method, so the trait has to be in
// scope to call it.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const PHYSICS_DT: f64 = 1.0 / 60.0;
const SPEED: f32 = 100.0;

/// Sets the body's velocity on its very first `_physics_process` tick, and only
/// then — a handler that rewrote the velocity every tick could not tell "the
/// write arrived in time for this tick's step" from "it arrived in time for the
/// next one".
const SCRIPT: &str = "\
extends RigidBody2D
var ticks = 0

func _physics_process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        self.linear_velocity = Vector2(100, 0)
";

/// A lone RigidBody2D at the origin with the script attached. No other body, so
/// nothing can collide and alter the horizontal motion.
fn scene() -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    // See the module note: the seed is what makes `self.linear_velocity = ...`
    // a NODE write rather than a script-local one.
    body.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    // Gravity is irrelevant to the +X assertion, but switching it off keeps the
    // body from drifting into anything a future fixture change might add.
    body.set_property("gravity_scale", Variant::Float(0.0));
    let id = tree.add_child(root, body).expect("the body attaches");

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(id, shape).expect("the collider attaches");

    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(SCRIPT, id).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    // `MainLoop::new` does not populate the physics world; the host registers
    // after loading a scene (`patina-runner` main.rs). Without this the body is
    // in no map and stays at its spawn point however the phases are ordered.
    ml.register_physics_bodies();
    (ml, id)
}

fn position(ml: &MainLoop, id: NodeId) -> Vector2 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("the body should carry a Vector2 position, got {other:?}"),
    }
}

fn ticks(ml: &MainLoop, id: NodeId) -> i64 {
    match ml
        .tree()
        .get_script(id)
        .and_then(|s| s.get_property("ticks"))
    {
        Some(Variant::Int(n)) => n,
        other => panic!("the script must expose a `ticks` counter, got {other:?}"),
    }
}

/// **The acceptance.** One tick, one step's worth of the scripted velocity.
#[test]
fn physics2d_tick_order_script_before_step() {
    let (mut ml, id) = scene();

    assert_eq!(
        position(&ml, id),
        Vector2::ZERO,
        "precondition: the body starts at the origin"
    );

    ml.step(PHYSICS_DT);

    // CONTROL: the handler ran, exactly once. Without this, a fixture that never
    // reached `_physics_process` would report x = 0 and read as an ordering bug
    // rather than as a scene that never stepped.
    assert_eq!(
        ticks(&ml, id),
        1,
        "`_physics_process` must have run exactly once for a single 1/60 step"
    );
    // pat-29zst: the step that ended tick 1 reaches the NODE at tick 2's
    // `flush_queries`, as in Godot 4.6.1 (a 60 px/s RigidBody2D still reads
    // x = 0 after one tick). So the ordering is read after the second tick,
    // whose flush publishes the step the tick-1 write drove.
    assert_eq!(
        position(&ml, id),
        Vector2::ZERO,
        "after one tick the node still holds its pre-step state, as in Godot"
    );
    ml.step(PHYSICS_DT);

    let expected = SPEED * PHYSICS_DT as f32;
    let x = position(&ml, id).x;

    // The ORDERING discriminator, stated as an absolute: under the old
    // step-before-scripts order this is exactly 0.0, because the only write
    // that could move the body sideways had not happened when the solver ran.
    // Kept separate from the magnitude check below so that damping, the
    // integration scheme or a future solver change can never turn an ordering
    // regression into a tolerance argument.
    assert!(
        x > 0.0,
        "the step ending the tick whose script wrote the velocity must have moved \
         the body along +X, got x = {x}. Exactly 0.0 is the bug \
         this bead is about: the solver stepped before the script phase, so the \
         write landed a tick late"
    );

    // ... and it is ONE step's worth, not two. 2% is far wider than any damping
    // (Godot's `linear_damp` default is 0, and even 0.1 moves this by 0.17%)
    // and still an order of magnitude tighter than the 2 * 100 * dt a
    // double-applied velocity would produce.
    assert!(
        (x - expected).abs() <= expected * 0.02,
        "the first step must advance the body exactly one step: expected \
         x = {expected}, got {x}"
    );
}

/// The second tick advances by one more step, not by two. Guards the opposite
/// failure: a fixture that somehow applied the velocity twice (for instance a
/// `sync_to_physics` called on both sides of the script phase) would satisfy the
/// acceptance above on tick 2 while being wrong on every tick.
#[test]
fn physics2d_tick_order_advances_one_step_per_tick() {
    let (mut ml, id) = scene();

    ml.step(PHYSICS_DT);
    let after_one = position(&ml, id).x;
    ml.step(PHYSICS_DT);
    let after_two = position(&ml, id).x;

    assert_eq!(
        ticks(&ml, id),
        2,
        "two 1/60 steps must run the handler twice"
    );

    let step = SPEED * PHYSICS_DT as f32;
    assert!(
        (after_two - after_one - step).abs() <= step * 0.02,
        "the second tick must add exactly one more step of travel: \
         {after_one} -> {after_two}, expected a delta of {step}"
    );
}
