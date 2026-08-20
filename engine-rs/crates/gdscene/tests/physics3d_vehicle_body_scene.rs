//! pat-s3rrv.194 — a `.tscn` `VehicleBody3D` drives on real world geometry.
//!
//! Godot ground truth: `scene/3d/physics/vehicle_body_3d.cpp`,
//! `doc/classes/VehicleBody3D.xml`, `doc/classes/VehicleWheel3D.xml`.
//!
//! # What was wrong
//!
//! A complete raycast-vehicle solver existed at `gdphysics2d::vehicle3d` with
//! its own passing unit test, both classes were registered in ClassDB, and the
//! editor inspector listed their full property surface — and **nothing outside
//! that module ever constructed one**. `body_type_for_class_3d` did not know
//! `VehicleBody3D`, so a `.tscn` car registered as no body at all and its wheels
//! were inert. The solver was also built on a parallel 3D stack no runtime path
//! used, and rode a flat `ground_y: f32` rather than querying world bodies, so
//! it could not have touched a `StaticBody3D` floor even if something had called
//! it.
//!
//! ⚠️ Everything below arrives as `.tscn` text and is driven only by
//! `MainLoop::step`. Nothing is pushed into the physics world directly.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A car on four driven wheels, dropped just above a wide box floor.
///
/// ⚠️ `can_sleep = false` for the same reason the pin-joint pendulum test sets
/// it: the car spends ~3 simulated seconds settling at near-zero velocity
/// before the throttle is applied, which is exactly the state auto-sleep looks
/// for (`body.rs:303`, threshold 0.01). A sleeping body returns early from
/// `integrate`, so a car that dozed off would ignore `engine_force` and the
/// drive clause would fail with the drive model working perfectly.
///
/// ⚠️ `use_as_traction = true` is REQUIRED on every wheel, not decoration.
/// Godot defaults it to **false** (`VehicleWheel3D.xml`), and a car whose wheels
/// are all non-traction has nothing to deliver `engine_force` through — it would
/// sit on its suspension and never move, and the drive clause would fail with
/// the drive model working perfectly.
const CAR_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxShape3D" id="Floor_1"]
size = Vector3(200, 2, 200)

[sub_resource type="BoxShape3D" id="Chassis_1"]
size = Vector3(1.6, 0.8, 3.2)

[node name="Root" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]
position = Vector3(0, -1, 0)

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
shape = SubResource("Floor_1")

[node name="Car" type="VehicleBody3D" parent="."]
position = Vector3(0, 1.2, 0)
mass = 40.0
can_sleep = false

[node name="ChassisShape" type="CollisionShape3D" parent="Car"]
shape = SubResource("Chassis_1")

[node name="FL" type="VehicleWheel3D" parent="Car"]
position = Vector3(-0.7, -0.4, 1.4)
use_as_traction = true
use_as_steering = true

[node name="FR" type="VehicleWheel3D" parent="Car"]
position = Vector3(0.7, -0.4, 1.4)
use_as_traction = true
use_as_steering = true

[node name="RL" type="VehicleWheel3D" parent="Car"]
position = Vector3(-0.7, -0.4, -1.4)
use_as_traction = true

[node name="RR" type="VehicleWheel3D" parent="Car"]
position = Vector3(0.7, -0.4, -1.4)
use_as_traction = true
"#;

fn load(tscn: &str) -> MainLoop {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    MainLoop::new(tree)
}

fn node_of(ml: &MainLoop, path: &str) -> NodeId {
    ml.tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

/// The chassis position straight out of the physics world.
fn chassis_position(ml: &MainLoop, car: NodeId) -> Vector3 {
    let server = ml.physics_server_3d();
    let body = server
        .body_for_node(car)
        .expect("the VehicleBody3D must register as a physics body");
    server
        .world()
        .get_body(body)
        .expect("body is in the world")
        .position
}

fn run(ml: &mut MainLoop, ticks: usize) {
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** The car settles on its suspension above the floor
/// instead of sinking through it, drives forward under `engine_force`, and
/// reports wheel contact to GDScript.
#[test]
fn physics3d_vehicle_body_scene() {
    let mut ml = load(CAR_TSCN);
    let car = node_of(&ml, "/root/Root/Car");

    // ⚠️ One tick FIRST. Bodies are registered in `sync_to_physics`, which only
    // runs inside `MainLoop::step` — before the first step the world is empty
    // and `body_for_node` returns `None`, so reading the chassis here without
    // stepping fails with "must register as a physics body" no matter how
    // correctly the class is wired.
    ml.step(1.0 / 60.0);

    // The class must reach the world at all — without the `body_type_for_class_3d`
    // arm this panics, and every assertion below would be about nothing.
    let start = chassis_position(&ml, car);
    assert!(
        start.y > 1.1 && start.y <= 1.2,
        "fixture: the car starts at y = 1.2 and has had one tick of gravity, \
         got {start:?}"
    );

    // -- (a) settles on the suspension, does not sink -----------------------
    run(&mut ml, 180);
    let resting = chassis_position(&ml, car).y;

    // The floor's top surface is at y = 0 (a 2 m box centred at y = -1).
    assert!(
        resting > 0.2,
        "the chassis must rest ON its suspension above the floor (top surface \
         y = 0), got y = {resting}. A value at or below 0 means the wheels \
         found no ground and the car fell through — which is what happens if \
         the wheel ray is cast with `PhysicsWorld3D::raycast` (sphere bodies \
         only) instead of a shape-aware query"
    );
    assert!(
        resting < 1.3,
        "...and it must actually SETTLE rather than be flung upward by an \
         over-stiff spring, got y = {resting}"
    );

    // Still settled a second later: a car oscillating or drifting upward would
    // pass a single sample.
    run(&mut ml, 60);
    let still = chassis_position(&ml, car).y;
    assert!(
        (still - resting).abs() < 0.1,
        "the ride height must be stable, moved from {resting} to {still}"
    );

    // -- (b) engine_force drives it forward ---------------------------------
    if let Some(node) = ml.tree_mut().get_node_mut(car) {
        node.set_property("engine_force", Variant::Float(200.0));
    }
    let before = chassis_position(&ml, car);
    run(&mut ml, 120);
    let after = chassis_position(&ml, car);

    let travelled = (after - before).length();
    assert!(
        travelled > 1.0,
        "engine_force = 200 for 2 s must move the car more than 1 m, went \
         {travelled} m (from {before:?} to {after:?})"
    );
    // Godot's forward is -Z, so a car with no steering must travel that way.
    assert!(
        after.z < before.z,
        "the car must drive along its forward axis (-Z), z went {} -> {}",
        before.z,
        after.z
    );
}

/// ⚠️ The control for the drive clause: the same car with no `engine_force`
/// must stay put.
///
/// Without this, "it moved" would pass against a build that let the car slide,
/// roll or be pushed by the suspension itself.
#[test]
fn a_car_with_no_engine_force_stays_put() {
    let mut ml = load(CAR_TSCN);
    let car = node_of(&ml, "/root/Root/Car");
    run(&mut ml, 180);
    let settled = chassis_position(&ml, car);
    run(&mut ml, 120);
    let later = chassis_position(&ml, car);

    let drift = Vector3::new(later.x - settled.x, 0.0, later.z - settled.z).length();
    assert!(
        drift < 0.2,
        "a car with engine_force = 0 must not drive itself, drifted {drift} m"
    );
}

/// 🔑 Acceptance clause (c): `$Wheel.is_in_contact()` reads true from GDScript
/// while the car is grounded.
///
/// ⚠️ Uses the `$Wheel.method()` spelling the bead asks for. That routes through
/// the interpreter's `call_method_on`, which falls back to
/// `SceneAccess::call_node_method` for names it does not know — so the single
/// registration there serves this spelling and the bare one a script on the
/// wheel itself would use.
#[test]
fn is_in_contact_reads_true_from_gdscript_while_grounded() {
    let mut ml = load(CAR_TSCN);
    let car = node_of(&ml, "/root/Root/Car");

    // Let it settle so the wheels are actually on the ground.
    run(&mut ml, 180);
    assert!(
        chassis_position(&ml, car).y > 0.2,
        "fixture: the car must be resting on the floor before asking its wheels \
         whether they are touching it"
    );

    // ⚠️ The script goes on the CAR, so `$FL` is a plain child lookup — the
    // `$Name.method()` spelling the acceptance names and the one every other
    // test in this suite uses (`$RayCast3D.is_colliding()`). A quoted
    // `$"../Car/FL"` path would additionally be testing node-path parsing,
    // which is not what this clause is about.
    let src = "\
extends VehicleBody3D
var grounded = false
func _ready():
    self.grounded = $FL.is_in_contact()
";
    let script = GDScriptNodeInstance::from_source(src, car).expect("script parses");
    ml.tree_mut().attach_script(car, Box::new(script));
    ml.tree_mut().process_script_ready(car);

    let grounded = ml
        .tree()
        .get_script(car)
        .expect("the car has a script")
        .get_property("grounded");
    assert_eq!(
        grounded,
        Some(Variant::Bool(true)),
        "a grounded VehicleWheel3D must report is_in_contact() == true to \
         GDScript; `Some(Bool(false))` means the wheel found no ground and \
         `None` means the method never dispatched at all"
    );
}
