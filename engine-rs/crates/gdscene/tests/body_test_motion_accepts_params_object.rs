//! pat-s3rrv.96: `PhysicsServer2D.body_test_motion` accepts Godot's real arg 1,
//! a `PhysicsTestMotionParameters2D`, instead of only a bare `Vector2`.
//!
//! Godot's signature is
//! `body_test_motion(body, PhysicsTestMotionParameters2D, [PhysicsTestMotionResult2D])`
//! (`servers/physics_server_2d.h`), so real code builds a params object and sets
//! `motion` on it. Patina's arm took the vector directly.
//!
//! ⚠️ THE FAILURE THIS PREVENTS IS SILENT. `vec2_arg` answers `Vector2::ZERO`
//! for anything that is not a Vector2, so handing over a params object did not
//! raise — it swept a ZERO-length motion and reported "nothing in the way". A
//! script following Godot's documented API got a confident wrong answer, which
//! is worse than a refusal, and no test would have noticed because the call
//! still returned a well-formed result.
//!
//! 🔑 That is why the assertions below are DIFFERENTIAL. A params object whose
//! motion runs into the floor must produce the SAME answer as the bare vector,
//! and a materially different one from a zero motion. Asserting only "collided
//! is true" would pass against the old behaviour on a body already touching the
//! floor, which is exactly the fixture a careless test would use.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Builds a body just above a node floor, then sweeps it downward three ways:
/// through a params object, through the bare vector, and with no motion at all.
const DRIVER_GD: &str = "\
extends Node2D
var via_params = -1.0
var via_vector = -1.0
var via_zero = -1.0
var echo_y = -999.0
var stage = 0
var made = false

func _physics_process(delta):
    if made == false:
        made = true
        var shape_rid = PhysicsServer2D.circle_shape_create()
        PhysicsServer2D.shape_set_data(shape_rid, 8.0)
        var body_rid = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_space(body_rid, 1)
        PhysicsServer2D.body_add_shape(body_rid, shape_rid)
        PhysicsServer2D.body_set_state(body_rid, 0, Vector2(0, 60))
        self.stage = 1

        var params = PhysicsTestMotionParameters2D.new()
        self.stage = 2
        params.motion = Vector2(0, 80)
        self.stage = 3
        var echo = params.motion
        self.stage = 4
        self.echo_y = echo.y
        self.stage = 5

        var a = PhysicsServer2D.body_test_motion(body_rid, params)
        var ta = a[\"travel\"]
        self.via_params = ta.y

        var b = PhysicsServer2D.body_test_motion(body_rid, Vector2(0, 80))
        var tb = b[\"travel\"]
        self.via_vector = tb.y

        var zero = PhysicsTestMotionParameters2D.new()
        zero.motion = Vector2(0, 0)
        var c = PhysicsServer2D.body_test_motion(body_rid, zero)
        var tc = c[\"travel\"]
        self.via_zero = tc.y
";

fn script_float(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    match ml
        .tree()
        .get_script(node)
        .and_then(|s| s.get_property(name))
    {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("`{name}` must be a number, got {other:?}"),
    }
}

#[test]
fn body_test_motion_reads_motion_off_a_params_object() {
    // ⚠️ REQUIRED. Neither `SceneTree` nor `MainLoop` populates ClassDB, and
    // `PhysicsTestMotionParameters2D.new()` resolves through it — without this
    // the constructor answers Nil, `params.motion = ..` goes nowhere, and the
    // sweep reads a zero motion. That is INDISTINGUISHABLE from the bug this
    // test exists to catch, which is exactly what the `params_is_object`
    // control below is for: it failed here first and named the cause.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // Floor spanning y ∈ [190, 210].
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let floor_id = tree.add_child(main, floor).unwrap();
    let mut floor_shape = Node::new("CollisionShape", "CollisionShape2D");
    floor_shape.set_property("size", Variant::Vector2(Vector2::new(1000.0, 20.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let driver = tree.add_child(main, Node::new("Driver", "Node2D")).unwrap();
    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(DRIVER_GD, driver).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // Only this populates `node_to_body`; without it the floor is not in the
    // world and every sweep below would report clear travel for that reason.
    ml.register_physics_bodies();
    // 🔑 THE DRIVER RUNS IN `_physics_process`, NOT `_ready`, and that is the
    // whole reason this fixture took three rounds.
    //
    // `MainLoop::step` drives `_process`/`_physics_process` with the script-
    // facing servers bound. It does NOT run `_ready`: that goes through
    // `LifecycleManager::enter_tree`, "driven by the host rather than by
    // Self::step" (see `MainLoop::enter_tree_with_scripts`). So:
    //   * `process_script_ready` runs `_ready` but leaves the physics server
    //     unbound, and `PhysicsServer2D.body_test_motion` answers Nil;
    //   * `step` binds the servers but never calls `_ready` at all.
    // Putting the work in `_physics_process` behind a run-once guard is what
    // the neighbouring `physics_server_2d_singleton_script_api` does, and it is
    // the only arrangement where both halves hold.
    ml.step(1.0 / 60.0);

    // FIXTURE CONTROL, instrumented. `stage` is a PROGRESS COUNTER: each step
    // of `_ready` bumps it, so if the body aborts the last value names the line
    // that raised. `echo_y` carries the round-tripped `motion.y`. Together one
    // run distinguishes every candidate — a raise inside `.new()` (stage 1), a
    // raise on the property write (2), on the read (3), on `.y` (4), or a
    // successful round trip with the wrong value (5 with echo_y != 80).
    let stage = ml
        .tree()
        .get_script(driver)
        .and_then(|s| s.get_property("stage"));
    let echo_y = ml
        .tree()
        .get_script(driver)
        .and_then(|s| s.get_property("echo_y"));
    assert_eq!(
        stage,
        Some(Variant::Int(5)),
        "fixture: `_ready` must reach the end. `stage` is where it stopped \
         (1=server calls done, 2=.new() returned, 3=motion written, 4=motion \
         read back, 5=`.y` taken) and `echo_y` is the round-tripped value \
         ({echo_y:?}). Stage 0 means the callback never ran at all — check it \
         is `_physics_process` and not `_ready`, since `step` drives only the \
         former. Anything between 1 and 4 means that line raised, NOT that the \
         implementation is wrong"
    );
    assert_eq!(
        echo_y,
        Some(Variant::Float(80.0)),
        "fixture: `params.motion` must round-trip Vector2(0, 80); got \
         {echo_y:?}. A zero here means the object swallowed the write, which \
         would read exactly like the bug under test"
    );

    let via_params = script_float(&ml, driver, "via_params");
    let via_vector = script_float(&ml, driver, "via_vector");
    let via_zero = script_float(&ml, driver, "via_zero");

    // CONTROL: the bare-vector form still works, so a failure below is about
    // the params object and not about the sweep or the floor.
    assert!(
        via_vector > 1.0,
        "fixture: the bare-vector sweep must travel toward the floor, got {via_vector}"
    );

    // A zero motion must NOT travel — this is the value the old code produced
    // for a params object, so it is the wrong answer to be distinguished from.
    assert!(
        via_zero.abs() < 1e-6,
        "a zero motion must produce zero travel, got {via_zero}"
    );

    // THE ASSERTION: the params object must be read, not silently zeroed.
    assert!(
        (via_params - via_vector).abs() < 1e-6,
        "`body_test_motion` must read `motion` off the PhysicsTestMotionParameters2D \
         and sweep exactly as the bare vector does: params travel {via_params} vs \
         vector travel {via_vector}. A params travel of {via_zero} (zero) means the \
         object fell through to `vec2_arg`, which answers Vector2::ZERO for a \
         Resource — the silent no-op this test exists to catch"
    );
}
