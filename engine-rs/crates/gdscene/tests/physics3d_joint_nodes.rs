//! pat-0q2kx.118 acceptance: `.tscn` `Joint3D` nodes become world joints and
//! actually constrain motion.
//!
//! Godot ground truth: `upstream/godot/scene/3d/physics/joints/*.cpp` — every
//! `Joint3D` references `node_a`/`node_b` as NodePaths **relative to the
//! joint**, takes its pivot and constraint axes from its own global transform,
//! and defaults `exclude_nodes_from_collision` to `true`. The per-type solvers
//! live in `modules/godot_physics_3d/joints/`, where the hinge runs along the
//! joint frame's Z column and the slider/cone-twist along X.
//!
//! `gdphysics3d::joint` carried the parameter structs for all five kinds and
//! `world.add_joint` registered them, but every caller was one of that crate's
//! own `#[cfg(test)]` cases: none of the classes was in ClassDB, no scene path
//! ever built a joint, and `solve_joints` only did a bias nudge for pins plus a
//! motor for hinges. An authored `PinJoint3D` constrained nothing.
//!
//! ⚠️ Nothing is injected: the joints, their `node_a`/`node_b` paths, their
//! pivots and every parameter arrive as `.tscn` text through `PackedScene`.

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::physics_server_3d::PhysicsServer3D;
use gdscene::SceneTree;

/// A static anchor at `(0, 2, 0)` and a bob 2 m out along +X, pinned at the
/// anchor. `can_sleep = false` because a pendulum is momentarily still at the
/// top of each swing and would otherwise be put to sleep there.
const PENDULUM_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)

[node name="Bob" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 0)
can_sleep = false

[node name="Pin" type="PinJoint3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
"#;

/// The same scene with the joint removed — the control that proves the pin is
/// what changes the motion.
const NO_JOINT_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)

[node name="Bob" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 0)
can_sleep = false
"#;

/// A hinge whose motor drives body B about the joint frame's Z axis.
const MOTOR_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Base" type="StaticBody3D" parent="."]

[node name="Wheel" type="RigidBody3D" parent="."]
can_sleep = false

[node name="Motor" type="HingeJoint3D" parent="."]
node_a = NodePath("../Base")
node_b = NodePath("../Wheel")
motor/enable = true
motor/target_velocity = 4.0
motor/max_impulse = 100.0
"#;

/// The same motor, now boxed in by a narrow angular limit.
const LIMITED_MOTOR_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Base" type="StaticBody3D" parent="."]

[node name="Wheel" type="RigidBody3D" parent="."]
can_sleep = false

[node name="Motor" type="HingeJoint3D" parent="."]
node_a = NodePath("../Base")
node_b = NodePath("../Wheel")
motor/enable = true
motor/target_velocity = 4.0
motor/max_impulse = 100.0
angular_limit/enable = true
angular_limit/lower = -0.2
angular_limit/upper = 0.2
"#;

/// A joint whose `node_b` names a node that is not a physics body at all. Godot
/// leaves such a joint inert; it must not be built from half a connection.
const DANGLING_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]

[node name="NotABody" type="Node3D" parent="."]

[node name="Pin" type="PinJoint3D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../NotABody")
"#;

fn world_from(tscn: &str) -> (PhysicsServer3D, SceneTree) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    (server, tree)
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

/// The simulated position of a body node, straight out of the physics world.
fn body_position(server: &PhysicsServer3D, id: NodeId) -> Vector3 {
    let body_id = server
        .body_for_node(id)
        .expect("node was registered as a physics body");
    server
        .world()
        .get_body(body_id)
        .expect("body is in the world")
        .position
}

/// The simulated angular velocity of a body node.
fn body_spin(server: &PhysicsServer3D, id: NodeId) -> Vector3 {
    let body_id = server
        .body_for_node(id)
        .expect("node was registered as a physics body");
    server
        .world()
        .get_body(body_id)
        .expect("body is in the world")
        .angular_velocity
}

/// One frame of the real 3D flow: push the tree in, step, pull the result back.
fn run(server: &mut PhysicsServer3D, tree: &mut SceneTree, ticks: usize) {
    for _ in 0..ticks {
        server.sync_to_physics(tree);
        server.step(1.0 / 60.0);
        server.sync_from_physics(tree);
    }
}

/// **The acceptance test.** A `.tscn` `PinJoint3D` pendulum swings while holding
/// its bob at a fixed radius from the pivot, and a `.tscn` `HingeJoint3D` motor
/// spins its body at the authored rate.
#[test]
fn physics3d_joint_nodes() {
    // -- Pin: the bob keeps its radius for 300 ticks while swinging ---------
    let (mut server, mut tree) = world_from(PENDULUM_TSCN);
    assert_eq!(
        server.joint_count(),
        1,
        "the `.tscn` PinJoint3D must reach the world as a joint"
    );
    let bob = node(&tree, "/root/Main/Bob");
    let pivot = Vector3::new(0.0, 2.0, 0.0);
    let start = body_position(&server, bob);
    let rest_radius = (start - pivot).length();
    assert!(
        (rest_radius - 2.0).abs() < 1e-4,
        "the scene should start the bob 2 m from the pivot, got {rest_radius}"
    );

    let mut worst = 0.0_f32;
    let mut lowest = start.y;
    let mut widest_swing = 0.0_f32;
    for tick in 0..300 {
        run(&mut server, &mut tree, 1);
        let here = body_position(&server, bob);
        let radius = (here - pivot).length();
        let drift = (radius - rest_radius).abs();
        assert!(
            drift < 1e-2,
            "tick {tick}: the pin must hold the bob at {rest_radius} m from the \
             pivot, but it was {radius} m ({drift} off)"
        );
        worst = worst.max(drift);
        lowest = lowest.min(here.y);
        widest_swing = widest_swing.max((here.x - start.x).abs());
    }
    // A bob welded in place would also pass the radius check, so prove it moved.
    assert!(
        lowest < start.y - 0.5,
        "the bob must actually swing down, not hang rigid; lowest y was {lowest} \
         against a start of {}",
        start.y
    );
    assert!(
        widest_swing > 0.5,
        "the bob must sweep through the swing, but x moved only {widest_swing}"
    );

    // Control: with no joint the bob is in free fall and leaves the radius far
    // behind, so the check above cannot be passing on gravity alone.
    let (mut free_server, mut free_tree) = world_from(NO_JOINT_TSCN);
    assert_eq!(free_server.joint_count(), 0);
    let free_bob = node(&free_tree, "/root/Main/Bob");
    run(&mut free_server, &mut free_tree, 300);
    let free_radius = (body_position(&free_server, free_bob) - pivot).length();
    assert!(
        free_radius > 10.0,
        "without the pin the bob must fall away from the pivot, but it ended \
         {free_radius} m out (worst pinned drift was {worst})"
    );

    // -- Hinge: the motor reaches the authored target rate ------------------
    let (mut motor_server, mut motor_tree) = world_from(MOTOR_TSCN);
    assert_eq!(
        motor_server.joint_count(),
        1,
        "the `.tscn` HingeJoint3D must reach the world as a joint"
    );
    let wheel = node(&motor_tree, "/root/Main/Wheel");
    run(&mut motor_server, &mut motor_tree, 400);
    let spin = body_spin(&motor_server, wheel);
    assert!(
        (spin.z - 4.0).abs() <= 0.4,
        "the motor should hold body B at its 4.0 rad/s target within 10%, got \
         {} rad/s about Z",
        spin.z
    );
    // The hinge axis is the joint frame's Z, so nothing should spin off it.
    assert!(
        spin.x.abs() < 0.1 && spin.y.abs() < 0.1,
        "the hinge must confine rotation to its axis, but B also spun ({}, {}) \
         off it",
        spin.x,
        spin.y
    );
    // The point constraint holds the wheel on its pivot rather than letting
    // gravity drop it.
    let wheel_pos = body_position(&motor_server, wheel);
    assert!(
        wheel_pos.length() < 1e-2,
        "the hinge must hold the wheel at the joint, but it drifted to {wheel_pos:?}"
    );
}

/// With `angular_limit/enable` on, the same motor winds the body up to the limit
/// and no further.
#[test]
fn hinge_angular_limits_bound_the_motor() {
    // The control runs the identical motor with the limit lines deleted.
    let (mut free_server, mut free_tree) = world_from(MOTOR_TSCN);
    let free_wheel = node(&free_tree, "/root/Main/Wheel");
    run(&mut free_server, &mut free_tree, 300);
    let free_spin = body_spin(&free_server, free_wheel).z;

    let (mut server, mut tree) = world_from(LIMITED_MOTOR_TSCN);
    let wheel = node(&tree, "/root/Main/Wheel");
    run(&mut server, &mut tree, 300);
    let angle = gdscene::node3d::get_rotation(&tree, wheel).z;
    let spin = body_spin(&server, wheel);

    assert!(
        angle <= 0.3,
        "`angular_limit/upper = 0.2` must stop the motor, but the wheel reached \
         {angle} rad"
    );
    assert!(
        angle > 0.15,
        "the motor should still drive the wheel up *to* the limit, got {angle} rad"
    );
    assert!(
        spin.z.abs() < 0.5,
        "a wheel held against its limit should not still be spinning, got {} rad/s",
        spin.z
    );
    // Without the limit the very same motor is still turning at full speed, so
    // the stop above is the limit doing work and not the motor giving up.
    assert!(
        free_spin > 3.0,
        "the unlimited control should still be spinning near its 4.0 rad/s \
         target, got {free_spin}"
    );
}

/// Removing the joint node frees the constraint: the bob stops orbiting and
/// falls.
#[test]
fn removing_the_joint_node_frees_the_constraint() {
    let (mut server, mut tree) = world_from(PENDULUM_TSCN);
    let bob = node(&tree, "/root/Main/Bob");
    let pin = node(&tree, "/root/Main/Pin");
    let pivot = Vector3::new(0.0, 2.0, 0.0);

    run(&mut server, &mut tree, 30);
    let held = (body_position(&server, bob) - pivot).length();
    assert!(
        (held - 2.0).abs() < 1e-2,
        "the pin should still be holding at this point, radius {held}"
    );

    tree.remove_node(pin)
        .expect("the joint node can be removed");
    run(&mut server, &mut tree, 60);
    assert_eq!(
        server.joint_count(),
        0,
        "a joint whose node left the tree must be freed from the world"
    );
    assert_eq!(
        server.joint_for_node(pin),
        None,
        "the freed joint must not stay mapped to its dead node"
    );

    let released = (body_position(&server, bob) - pivot).length();
    assert!(
        released > 2.5,
        "with the joint gone the bob must fall away from the pivot, but it is \
         still {released} m out"
    );
}

/// A joint pointing at a non-body is skipped rather than half-built — a joint
/// with one real end would drag its body toward the origin.
#[test]
fn joint_with_an_unresolvable_end_is_skipped() {
    let (server, _tree) = world_from(DANGLING_TSCN);
    assert_eq!(
        server.joint_count(),
        0,
        "a joint whose node_b is not a physics body must not be built"
    );
}

/// `exclude_nodes_from_collision` defaults to `true`, so the two jointed bodies
/// stop colliding with each other — without it the contact solver fights the
/// constraint that holds them coincident.
#[test]
fn jointed_bodies_are_excluded_from_colliding() {
    let (server, tree) = world_from(MOTOR_TSCN);
    let base = server
        .body_for_node(node(&tree, "/root/Main/Base"))
        .expect("Base is a body");
    let wheel = server
        .body_for_node(node(&tree, "/root/Main/Wheel"))
        .expect("Wheel is a body");
    assert!(
        server.world().has_collision_exception(base, wheel),
        "a joint with exclude_nodes_from_collision set must take its pair out of \
         the narrow phase"
    );
}

// ---------------------------------------------------------------------------
// The other three joint kinds (pat-0q2kx.118)
//
// `solve_joint_velocity` carries arms for the slider, the cone-twist and the
// 6-DOF joint, and `build_joint` reads every one of their `.tscn` parameters —
// but nothing exercised any of it. Dormant solver code is unproven, not
// correct, so each kind gets a scene that can only behave that way if its arm
// actually runs.
//
// Every scene below leaves the joint's transform at identity, so the constraint
// axes are the world axes and no basis-serialization convention is assumed.
// ---------------------------------------------------------------------------

/// A slider along the world X axis. Gravity pulls along -Y, which is
/// *perpendicular* to the slide axis, so the joint has to carry the load: the
/// one free degree of freedom is the one direction gravity does not act in.
const SLIDER_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Rail" type="StaticBody3D" parent="."]

[node name="Car" type="RigidBody3D" parent="."]
can_sleep = false

[node name="Slide" type="SliderJoint3D" parent="."]
node_a = NodePath("../Rail")
node_b = NodePath("../Car")
"#;

/// The same car with no rail joint — the control that proves the slider, and
/// not something else, is what holds the car up.
const SLIDER_CONTROL_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Rail" type="StaticBody3D" parent="."]

[node name="Car" type="RigidBody3D" parent="."]
can_sleep = false
"#;

/// A cone-twist hung like the pendulum: the pivot constraint has to hold the
/// bob at a fixed radius while it swings.
const CONE_TWIST_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)

[node name="Bob" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 0)
can_sleep = false

[node name="Cone" type="ConeTwistJoint3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
"#;

/// An unconfigured `Generic6DOFJoint3D`. Godot enables every axis limit with a
/// zero-width range, so this is a weld and the body must not fall.
const DOF_WELD_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]

[node name="Load" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0)
can_sleep = false

[node name="Dof" type="Generic6DOFJoint3D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../Load")
"#;

/// The same 6-DOF joint with the Y axis opened up. Only that axis is free, so
/// the body falls while X and Z stay welded — which is the whole point of a
/// per-axis limit table.
const DOF_FREE_Y_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]

[node name="Load" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0)
can_sleep = false

[node name="Dof" type="Generic6DOFJoint3D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../Load")
linear_limit_y/lower_distance = -100.0
linear_limit_y/upper_distance = 100.0
"#;

/// A `.tscn` `SliderJoint3D` locks every direction except its slide axis, so a
/// car on a horizontal rail does not fall.
#[test]
fn slider_joint_locks_the_off_axis_directions() {
    let (mut server, mut tree) = world_from(SLIDER_TSCN);
    assert_eq!(
        server.joint_count(),
        1,
        "the .tscn SliderJoint3D must reach the world"
    );
    let car = node(&tree, "/root/Main/Car");
    run(&mut server, &mut tree, 120);
    let held = body_position(&server, car);

    // The control falls freely over the same 120 ticks.
    let (mut control_server, mut control_tree) = world_from(SLIDER_CONTROL_TSCN);
    let free_car = node(&control_tree, "/root/Main/Car");
    run(&mut control_server, &mut control_tree, 120);
    let fell = body_position(&control_server, free_car);

    assert!(
        fell.y < -1.0,
        "the control body should fall freely, got y = {}",
        fell.y
    );
    assert!(
        held.y.abs() < 0.1,
        "the slider must carry the load perpendicular to its axis: y = {} \
         (unjointed control fell to {})",
        held.y,
        fell.y
    );
    assert!(
        held.z.abs() < 0.1,
        "the slider's third axis must be locked too, got z = {}",
        held.z
    );
}

/// A `.tscn` `ConeTwistJoint3D` holds its pivot, so the bob swings at a fixed
/// radius instead of falling away.
#[test]
fn cone_twist_joint_holds_its_pivot() {
    let (mut server, mut tree) = world_from(CONE_TWIST_TSCN);
    assert_eq!(
        server.joint_count(),
        1,
        "the .tscn ConeTwistJoint3D must reach the world"
    );
    let bob = node(&tree, "/root/Main/Bob");
    let pivot = Vector3::new(0.0, 2.0, 0.0);
    let radius = (body_position(&server, bob) - pivot).length();
    assert!((radius - 2.0).abs() < 1e-3, "authored radius is 2m");

    let mut swung = false;
    for _ in 0..300 {
        run(&mut server, &mut tree, 1);
        let p = body_position(&server, bob);
        let r = (p - pivot).length();
        assert!(
            (r - radius).abs() < 1e-2,
            "the cone-twist pivot must hold the bob at {radius}m, got {r}m at {p:?}"
        );
        if p.y < 1.9 {
            swung = true;
        }
    }
    assert!(
        swung,
        "the bob should actually swing under gravity, not sit still"
    );
}

/// An unconfigured `Generic6DOFJoint3D` welds its bodies: Godot's per-axis
/// limits all default to a zero-width range.
#[test]
fn generic_6dof_joint_with_default_limits_welds() {
    let (mut server, mut tree) = world_from(DOF_WELD_TSCN);
    assert_eq!(
        server.joint_count(),
        1,
        "the .tscn Generic6DOFJoint3D must reach the world"
    );
    let load = node(&tree, "/root/Main/Load");
    let start = body_position(&server, load);
    run(&mut server, &mut tree, 120);
    let end = body_position(&server, load);
    assert!(
        (end - start).length() < 0.1,
        "zero-width limits on every axis are a weld; the load moved from \
         {start:?} to {end:?}"
    );
}

/// Opening one axis of a `Generic6DOFJoint3D` frees exactly that axis — the
/// per-axis limit table is read from the `.tscn`, not defaulted wholesale.
#[test]
fn generic_6dof_joint_frees_only_the_widened_axis() {
    let (mut server, mut tree) = world_from(DOF_FREE_Y_TSCN);
    let load = node(&tree, "/root/Main/Load");
    let start = body_position(&server, load);
    run(&mut server, &mut tree, 120);
    let end = body_position(&server, load);

    assert!(
        end.y < start.y - 1.0,
        "the widened Y axis must let the load fall, got y = {} (started {})",
        end.y,
        start.y
    );
    assert!(
        (end.x - start.x).abs() < 0.1,
        "X was left at its zero-width default and must stay welded, got x = {} \
         (started {})",
        end.x,
        start.x
    );
    assert!(
        (end.z - start.z).abs() < 0.1,
        "Z was left at its zero-width default and must stay welded, got z = {}",
        end.z
    );
}
