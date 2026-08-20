//! pat-s3rrv.192 — `Generic6DOFJoint3D` motors and springs actually drive the
//! solver, and a ragdoll bone's `joint_type` picks its constraint.
//!
//! Godot ground truth: `doc/classes/Generic6DOFJoint3D.xml` for the motor and
//! spring parameters, and `scene/3d/physics/physical_bone_3d.cpp` for
//! `joint_type` + `joint_constraints/*`.
//!
//! # What was wrong
//!
//! `gdphysics3d::joint` said so in its own words — "Motors and springs are a
//! later addition; this holds the limits" — so all 36 motor/spring properties
//! were authorable, round-tripped through a `.tscn`, and did nothing. Ragdolls
//! were worse than inert: every bone got a `PinJoint3D` no matter what
//! `joint_type` said, so a rig authored with hinge elbows flailed in every
//! direction while looking correctly configured.
//!
//! ⚠️ Everything here is driven by `MainLoop::step`, and every parameter arrives
//! as `.tscn` text or as an authored node property. Nothing is injected into the
//! physics world directly.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// (a) angular motor
// ---------------------------------------------------------------------------

/// A wheel on a 6DOF joint whose Y motor is told to spin it at 2 rad/s.
///
/// ⚠️ `angular_limit_y/enabled = false` is REQUIRED, not tidying. Godot's 6DOF
/// defaults every limit to `enabled = true` with `lower = upper = 0.0`
/// (`Generic6DOFJoint3D.xml`), so a joint that authors nothing locks all six
/// axes. Left at the default the limit constraint fights the motor every tick
/// and the wheel never reaches its target — the test would fail with the motor
/// working perfectly.
const MOTOR_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Base" type="StaticBody3D" parent="."]

[node name="Wheel" type="RigidBody3D" parent="."]
can_sleep = false
gravity_scale = 0.0

[node name="Dof" type="Generic6DOFJoint3D" parent="."]
node_a = NodePath("../Base")
node_b = NodePath("../Wheel")
angular_limit_y/enabled = false
angular_motor_y/enabled = true
angular_motor_y/target_velocity = 2.0
angular_motor_y/force_limit = 100.0
"#;

/// ⚠️ The control: byte-for-byte the same scene with the motor switched off.
/// Without it, "the wheel spins" would pass against a build that spun every
/// 6DOF body regardless of what was authored.
const MOTOR_OFF_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Base" type="StaticBody3D" parent="."]

[node name="Wheel" type="RigidBody3D" parent="."]
can_sleep = false
gravity_scale = 0.0

[node name="Dof" type="Generic6DOFJoint3D" parent="."]
node_a = NodePath("../Base")
node_b = NodePath("../Wheel")
angular_limit_y/enabled = false
angular_motor_y/enabled = false
angular_motor_y/target_velocity = 2.0
angular_motor_y/force_limit = 100.0
"#;

/// A body displaced 0.5 m above the anchor, with a stiff Y spring pulling it
/// back to the anchor plane.
///
/// `gravity_scale = 0` so the spring is the only thing acting on the body —
/// otherwise "it moved toward the anchor" could just be gravity.
///
/// ⚠️ `linear_limit_y/enabled = false` matters even MORE here than on the motor
/// scene. The default limit is `[0.0, 0.0]` enabled, which would drag the body
/// to y = 0 all by itself — so this test would have PASSED with springs
/// completely unimplemented. A false green is worse than a red, and
/// `a_disabled_spring_leaves_the_body_displaced` below is the control that
/// keeps it honest.
const SPRING_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]

[node name="Bob" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0.5, 0)
can_sleep = false
gravity_scale = 0.0

[node name="Dof" type="Generic6DOFJoint3D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
linear_limit_y/enabled = false
linear_spring_y/enabled = true
linear_spring_y/stiffness = 100.0
linear_spring_y/damping = 1.0
linear_spring_y/equilibrium_point = 0.0
"#;

/// The spring scene with the spring switched off and the limit still freed —
/// nothing should move the body at all.
const NO_SPRING_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Anchor" type="StaticBody3D" parent="."]

[node name="Bob" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0.5, 0)
can_sleep = false
gravity_scale = 0.0

[node name="Dof" type="Generic6DOFJoint3D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
linear_limit_y/enabled = false
linear_spring_y/enabled = false
linear_spring_y/stiffness = 100.0
linear_spring_y/damping = 1.0
linear_spring_y/equilibrium_point = 0.0
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

fn spin(ml: &MainLoop, id: NodeId) -> Vector3 {
    let server = ml.physics_server_3d();
    let body = server.body_for_node(id).expect("node is a physics body");
    server
        .world()
        .get_body(body)
        .expect("body is in the world")
        .angular_velocity
}

fn position(ml: &MainLoop, id: NodeId) -> Vector3 {
    let server = ml.physics_server_3d();
    let body = server.body_for_node(id).expect("node is a physics body");
    server
        .world()
        .get_body(body)
        .expect("body is in the world")
        .position
}

/// The node's own authored transform, read WITHOUT going through the physics
/// server.
///
/// ⚠️ Needed because the 3D server registers bodies **lazily inside
/// `sync_to_physics`**, which only runs from `MainLoop::step` (main_loop.rs:3901).
/// Before the first tick `body_for_node` answers `None` for every node, so
/// [`position`] panics on its own `expect` — which is what the fixture
/// precondition below used to do (pat-s3rrv.192).
///
/// Stepping once first is NOT an alternative here: with stiffness 100 the spring
/// moves the bob ~14 mm in a single tick, which is fourteen times the 1e-3
/// tolerance the precondition asserts. The precondition is about what the SCENE
/// authored, so it has to be read from the scene.
fn authored_y(ml: &MainLoop, id: NodeId) -> f32 {
    node3d::get_global_transform(ml.tree(), id).origin.y
}

fn run(ml: &mut MainLoop, ticks: usize) {
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** An authored angular motor spins its body to the
/// target and holds it; an authored linear spring pulls a displaced body back
/// to equilibrium; and a ragdoll bone's `joint_type = HINGE` constrains its
/// relative rotation where the old unconditional pin did not.
#[test]
fn physics3d_joint_motors_and_springs() {
    // -- (a) the angular motor reaches and holds its target -----------------
    let mut ml = load(MOTOR_TSCN);
    run(&mut ml, 120);
    let wheel = node_of(&ml, "/root/Main/Wheel");
    let reached = spin(&ml, wheel).y;
    assert!(
        (reached - 2.0).abs() <= 0.2,
        "an enabled angular_motor_y with target_velocity 2.0 must drive the \
         body to within 10% of 2.0 rad/s, got {reached}"
    );

    // ...and HOLDS it, rather than overshooting once and drifting.
    run(&mut ml, 120);
    let held = spin(&ml, wheel).y;
    assert!(
        (held - 2.0).abs() <= 0.2,
        "the motor must hold the target, not overshoot and drift; got {held} \
         after another 2 seconds"
    );

    // -- the control: same scene, motor off ---------------------------------
    let mut off = load(MOTOR_OFF_TSCN);
    run(&mut off, 120);
    let idle = spin(&off, node_of(&off, "/root/Main/Wheel")).y;
    assert!(
        idle.abs() < 0.05,
        "with the motor disabled the body must stay still, got {idle} rad/s — \
         a build that spun it anyway would pass the assertion above"
    );

    // -- (b) the linear spring pulls the body back to equilibrium -----------
    let mut sp = load(SPRING_TSCN);
    let bob = node_of(&sp, "/root/Main/Bob");
    let start = authored_y(&sp, bob);
    assert!(
        (start - 0.5).abs() < 1e-3,
        "fixture: the bob must start displaced 0.5 m above the anchor, got \
         {start} — a bob already at equilibrium proves nothing"
    );

    run(&mut sp, 240);
    let settled = position(&sp, bob).y;
    assert!(
        settled.abs() <= 0.05,
        "a linear_spring_y with stiffness 100 and equilibrium_point 0 must pull \
         the body back to the anchor plane, got y = {settled}"
    );
}

// ---------------------------------------------------------------------------
// (c) ragdoll joint_type
// ---------------------------------------------------------------------------

const SEGMENT: f32 = 1.0;

/// ±0.2 rad, expressed in DEGREES.
///
/// ⚠️ `joint_constraints/angular_limit_lower` / `_upper` are authored in degrees
/// and converted with `deg_to_rad` on the way in (`physical_bone_3d.cpp:253`,
/// `:259`). Writing `0.2` here would ask for 0.2 DEGREES — a limit 57x tighter
/// than intended — and reading it as radians on our side would be the same
/// mistake in the other direction. Neither error crashes; the ragdoll just
/// behaves wrongly.
const LIMIT_DEGREES: f64 = 11.459_155;

/// A 2-bone chain under a simulator, each bone declaring `joint_type`.
fn build_rig(joint_type: i64, limited: bool) -> (MainLoop, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skeleton = tree
        .add_child(root, Node::new("Skel", "Skeleton3D"))
        .unwrap();

    let hip_idx = skeleton3d::add_bone(&mut tree, skeleton, "hip");
    let thigh_idx = skeleton3d::add_bone(&mut tree, skeleton, "thigh");
    skeleton3d::set_bone_parent(&mut tree, skeleton, thigh_idx, hip_idx as i64);
    // ⚠️ The chain reaches along +X, NOT straight down, and the bone bodies sit
    // BEYOND the joint rather than on it. Both are load-bearing (pat-s3rrv.192).
    //
    // The joint is placed at the child bone's head — `start_ragdoll_simulation`
    // uses `get_bone_global_pose(idx)` — and the pivot handed to the constraint
    // is `joint.origin - body_position`. The original rig put the ThighBone node
    // at exactly the thigh bone's head, so that pivot was the ZERO VECTOR: the
    // body was pinned through its own centre of mass. An impulse at r = 0
    // applies no torque, so the thigh could only ever translate, and
    // `widest_angle` read exactly 0.0 rad for every joint type — including the
    // pin, whose whole job in this test is to swing freely.
    //
    // A vertical chain would not have helped either: hanging straight down under
    // gravity is the equilibrium, so the torque is zero there too. The bone has
    // to stick out sideways for gravity to have a moment arm about the joint.
    let reach = Transform3D {
        basis: gdcore::math3d::Basis::IDENTITY,
        origin: Vector3::new(SEGMENT, 0.0, 0.0),
    };
    skeleton3d::set_bone_rest(&mut tree, skeleton, hip_idx, Transform3D::IDENTITY);
    skeleton3d::set_bone_rest(&mut tree, skeleton, thigh_idx, reach);

    let simulator = tree
        .add_child(skeleton, Node::new("Sim", "PhysicalBoneSimulator3D"))
        .unwrap();

    let add = |tree: &mut SceneTree, name: &str, bone: &str, pos: Vector3| -> NodeId {
        let id = tree
            .add_child(simulator, Node::new(name, "PhysicalBone3D"))
            .unwrap();
        {
            let n = tree.get_node_mut(id).unwrap();
            n.set_property("bone_name", Variant::String(bone.to_string()));
            n.set_property("joint_type", Variant::Int(joint_type));
            if limited {
                n.set_property(
                    "joint_constraints/angular_limit_enabled",
                    Variant::Bool(true),
                );
                n.set_property(
                    "joint_constraints/angular_limit_lower",
                    Variant::Float(-LIMIT_DEGREES),
                );
                n.set_property(
                    "joint_constraints/angular_limit_upper",
                    Variant::Float(LIMIT_DEGREES),
                );
            }
        }
        node3d::set_position(tree, id, pos);
        id
    };
    let hip = add(&mut tree, "HipBone", "hip", Vector3::ZERO);
    // Half a segment PAST the joint at (SEGMENT, 0, 0), so gravity acting at the
    // thigh's centre has a 0.5 m moment arm about the pivot.
    let thigh = add(
        &mut tree,
        "ThighBone",
        "thigh",
        Vector3::new(SEGMENT * 1.5, 0.0, 0.0),
    );

    (MainLoop::new(tree), simulator, hip, thigh)
}

/// The angle between two bodies' orientations, in radians.
fn relative_angle(ml: &MainLoop, a: NodeId, b: NodeId) -> f32 {
    let server = ml.physics_server_3d();
    let get = |id: NodeId| {
        let bid = server.body_for_node(id).expect("bone is a physics body");
        server
            .world()
            .get_body(bid)
            .expect("body is in the world")
            .rotation
    };
    (get(a).inverse() * get(b)).get_angle().abs()
}

/// Runs a rig for two simulated seconds and reports the widest relative angle
/// the chain reached.
fn widest_angle(joint_type: i64, limited: bool) -> f32 {
    let (mut ml, simulator, hip, thigh) = build_rig(joint_type, limited);
    // One tick registers the bodies before simulation is switched on.
    ml.step(1.0 / 60.0);
    // ⚠️ BONE NAMES, not node ids, and the call lives on `MainLoop` rather than
    // on the physics server. Only the thigh simulates, so the hip stays a
    // kinematic anchor for it to swing against.
    let simulated = ["thigh".to_string()];
    ml.physical_bones_start_simulation(simulator, &simulated);

    let mut widest = 0.0_f32;
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
        widest = widest.max(relative_angle(&ml, hip, thigh));
    }
    widest
}

/// A `joint_type = 3` (HINGE) bone stays inside its authored angular limit,
/// where the pin fallback does not constrain rotation at all.
///
/// ⚠️ Asserted BOTH ways. The absolute bound alone would pass on a rig that
/// never rotated for some unrelated reason, so the pin comparison proves the
/// chain really does swing when nothing stops it — and the pin bound proves the
/// hinge is what changed.
#[test]
fn a_hinge_bone_is_constrained_where_the_pin_fallback_is_not() {
    // JOINT_TYPE_HINGE = 3 (`PhysicalBone3D.xml`).
    let hinge = widest_angle(3, true);
    // JOINT_TYPE_PIN = 1 — the old unconditional behaviour.
    let pin = widest_angle(1, false);

    assert!(
        pin > 0.2,
        "fixture: a pinned chain must actually swing, or the hinge bound below \
         proves nothing; the pin chain only reached {pin} rad"
    );
    assert!(
        hinge <= 0.2 + 0.05,
        "a hinge bone with a ±0.2 rad limit must stay inside it, reached \
         {hinge} rad"
    );
    assert!(
        hinge < pin,
        "the hinge must constrain MORE than the pin it replaced: hinge \
         {hinge} rad vs pin {pin} rad"
    );
}

/// ⚠️ `joint_type = 0` (NONE) attaches the bone to nothing at all.
///
/// Godot's JointType enum starts at NONE, and a switch that treated 0 as "use
/// the default" would silently weld together bones the author deliberately
/// detached.
#[test]
fn joint_type_none_creates_no_joint() {
    let (mut ml, simulator, _hip, _thigh) = build_rig(0, false);
    ml.step(1.0 / 60.0);
    let simulated = ["thigh".to_string()];
    ml.physical_bones_start_simulation(simulator, &simulated);
    ml.step(1.0 / 60.0);

    assert_eq!(
        ml.physics_server_3d().joint_count(),
        0,
        "JOINT_TYPE_NONE must create no constraint between bone and parent"
    );
}

/// ⚠️ **The control that makes the spring assertion mean anything.** With the
/// spring disabled — and the default `linear_limit_y` still freed — nothing
/// pulls the body, so it must stay where it was put.
///
/// This exists because of a trap the fixture walked into first: Godot's 6DOF
/// joint defaults EVERY limit to `enabled = true` with `lower = upper = 0.0`
/// (`Generic6DOFJoint3D.xml`). A scene that simply authors a spring and nothing
/// else therefore has a `[0, 0]` linear limit dragging the body to the
/// equilibrium plane on its own — and the spring test would pass green with
/// springs entirely unimplemented. Freeing the limit is what makes the spring
/// the only candidate; this test is what proves the freeing worked.
#[test]
fn a_disabled_spring_leaves_the_body_displaced() {
    let mut ml = load(NO_SPRING_TSCN);
    let bob = node_of(&ml, "/root/Main/Bob");
    run(&mut ml, 240);
    let y = position(&ml, bob).y;
    assert!(
        (y - 0.5).abs() < 0.05,
        "with the spring off and the default limit freed, nothing may move the \
         body; it started at y = 0.5 and ended at {y}. If this drifted to 0 the \
         `linear_limit_y` default is still active and the spring test proves \
         nothing"
    );
}
