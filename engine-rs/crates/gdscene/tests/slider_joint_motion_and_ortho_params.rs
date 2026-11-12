//! pat-s3rrv.192 item (2) — `SliderJoint3D`'s `linear_motion/*` triple reaches
//! the solver.
//!
//! Godot gives a slider THREE parameter groups per axis pair, not one: `limit`
//! (while the axis is against a stop), `motion` (while it is travelling freely)
//! and `ortho` (the perpendicular directions). Only the `limit` group was ever
//! read, so a slider authored with damped travel behaved exactly like an
//! undamped one — the properties round-tripped through the `.tscn` and did
//! nothing.
//!
//! Ported from `upstream/godot/modules/godot_physics_3d/joints/godot_slider_joint_3d.cpp:148-152`,
//! whose impulse is `softness * (restitution * depth / step - damping * rel_vel)`
//! with the group chosen by `(i) ? Ortho : (solveLinLim ? Lim : Dir)`.
//!
//! ⚠️ Everything here is driven by `MainLoop::step` on `.tscn` text. The initial
//! velocity is an authored `linear_velocity`, not an injected body write, so the
//! test crosses the same load boundary the parameters do.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// A car sliding along +X with gravity off, so the ONLY thing that can slow it
/// is the joint.
///
/// ⚠️ The limits are widened to ±100 deliberately. Godot's defaults are ∓1.0
/// (`SliderJoint3D.xml`), and the bridge always enables them — left alone the
/// car would hit a stop within a quarter second and the `limit` triple, not the
/// `motion` triple, would be what this measured.
const FREE_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Rail" type="StaticBody3D" parent="."]

[node name="Car" type="RigidBody3D" parent="."]
can_sleep = false
gravity_scale = 0.0
linear_velocity = Vector3(4, 0, 0)

[node name="Slide" type="SliderJoint3D" parent="."]
node_a = NodePath("../Rail")
node_b = NodePath("../Car")
linear_limit/lower_distance = -100.0
linear_limit/upper_distance = 100.0
"#;

/// The same rail, with travel damping asked for.
const DAMPED_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Rail" type="StaticBody3D" parent="."]

[node name="Car" type="RigidBody3D" parent="."]
can_sleep = false
gravity_scale = 0.0
linear_velocity = Vector3(4, 0, 0)

[node name="Slide" type="SliderJoint3D" parent="."]
node_a = NodePath("../Rail")
node_b = NodePath("../Car")
linear_limit/lower_distance = -100.0
linear_limit/upper_distance = 100.0
linear_motion/damping = 1.0
"#;

/// A car with no authored motion params, hanging under gravity across the slide
/// axis — the off-axis lock this bead must not regress.
const LOAD_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Rail" type="StaticBody3D" parent="."]

[node name="Car" type="RigidBody3D" parent="."]
can_sleep = false

[node name="Slide" type="SliderJoint3D" parent="."]
node_a = NodePath("../Rail")
node_b = NodePath("../Car")
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

fn position(ml: &MainLoop, id: NodeId) -> Vector3 {
    let server = ml.physics_server_3d();
    let body = server.body_for_node(id).expect("node is a physics body");
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

/// Travel distance along the slide axis after one second.
fn travelled(tscn: &str) -> f32 {
    let mut ml = load(tscn);
    let car = node_of(&ml, "/root/Main/Car");
    run(&mut ml, 60);
    position(&ml, car).x
}

#[test]
fn linear_motion_damping_slows_travel_along_the_slide_axis() {
    let free = travelled(FREE_TSCN);
    let damped = travelled(DAMPED_TSCN);

    // 🔑 The control, and the reason the default matters. `linear_motion/damping`
    // defaults to 0.0 — alone among the four dampings, which are 1.0 — and that
    // zero is what makes a slider slide. If a future edit "tidies" it to 1.0 to
    // match its neighbours, this assertion is what catches it: an unauthored
    // slider would stop dead.
    assert!(
        free > 2.0,
        "an unauthored slider must travel freely — 4 m/s for one second should \
         cover most of 4m, got {free}"
    );

    assert!(
        damped < free * 0.5,
        "linear_motion/damping = 1.0 must visibly slow travel: damped {damped} \
         vs free {free}"
    );
}

/// The parameters are read from `.tscn` text, not merely defaulted.
///
/// ⚠️ Without this, the assertion above could pass on a build that ignored the
/// property and happened to be slow for some other reason. Two different
/// authored damping values must produce two different distances.
#[test]
fn the_authored_damping_value_is_what_varies_the_result() {
    let half = DAMPED_TSCN.replace(
        "linear_motion/damping = 1.0",
        "linear_motion/damping = 0.15",
    );
    assert!(
        half.contains("0.15"),
        "fixture edit must apply, or this test is vacuous"
    );

    let strong = travelled(DAMPED_TSCN);
    let weak = travelled(&half);
    assert!(
        weak > strong,
        "a weaker authored damping must let the car travel further: 0.15 gave \
         {weak}, 1.0 gave {strong}"
    );
}

/// The off-axis lock still holds — the regression guard for this change.
///
/// The ortho path was rewritten to scale by `softness * damping`; at Godot's
/// defaults that product is exactly 1.0, so the solve must stay bit-identical to
/// the unconditional one it replaced. A slider carrying a car against gravity is
/// the visible form of that.
#[test]
fn the_off_axis_lock_survives_the_ortho_rewrite() {
    let mut ml = load(LOAD_TSCN);
    let car = node_of(&ml, "/root/Main/Car");
    run(&mut ml, 120);
    let y = position(&ml, car).y;
    assert!(
        y.abs() < 0.25,
        "the slider must still carry the car against gravity — off-axis drop \
         must stay near zero, got {y}"
    );
}
