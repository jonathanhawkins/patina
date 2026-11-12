//! pat-0q2kx.114 acceptance: `.tscn` joint nodes become world joints and
//! actually constrain motion.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/joints/joint_2d.cpp` —
//! a `Joint2D` references `node_a`/`node_b` as NodePaths **relative to the
//! joint**, anchors at the joint's own global position, and defaults
//! `disable_collision` to `true`.
//!
//! `gdphysics2d::joint` solved all three kinds and `world.add_joint` registered
//! them, but every caller was one of that crate's own `#[cfg(test)]` cases — no
//! scene path ever built a joint, so an authored `PinJoint2D` constrained
//! nothing at runtime.
//!
//! ⚠️ Nothing is injected: the joints, their paths and their parameters all
//! arrive as `.tscn` text through `PackedScene`.

use gdphysics2d::body::BodyId;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::physics_server::PhysicsServer;
use gdscene::SceneTree;

/// A static anchor and a free body, pinned together. Without the joint the free
/// body falls; with it, the pin holds it.
const PIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="Hanging" type="RigidBody2D" parent="."]
position = Vector2(0, 20)

[node name="HangingShape" type="CollisionShape2D" parent="Hanging"]
shape = SubResource("Circle_1")

[node name="Pin" type="PinJoint2D" parent="."]
position = Vector2(0, 10)
node_a = NodePath("../Anchor")
node_b = NodePath("../Hanging")
"#;

/// The same scene with the joint removed — the control that proves the pin is
/// what changes the motion.
const NO_JOINT_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="Hanging" type="RigidBody2D" parent="."]
position = Vector2(0, 20)

[node name="HangingShape" type="CollisionShape2D" parent="Hanging"]
shape = SubResource("Circle_1")
"#;

/// A damped spring, to prove the other joint kinds also reach the world.
const SPRING_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="Bob" type="RigidBody2D" parent="."]
position = Vector2(0, 40)

[node name="BobShape" type="CollisionShape2D" parent="Bob"]
shape = SubResource("Circle_1")

[node name="Spring" type="DampedSpringJoint2D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
length = 40.0
stiffness = 40.0
damping = 2.0
"#;

/// A spring with an explicit `rest_length` well below the bob's start distance,
/// and a `length` ceiling far enough out that the *rest* length is what shapes
/// the motion.
const SPRING_REST_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="Bob" type="RigidBody2D" parent="."]
position = Vector2(0, 80)

[node name="BobShape" type="CollisionShape2D" parent="Bob"]
shape = SubResource("Circle_1")

[node name="Spring" type="DampedSpringJoint2D" parent="."]
position = Vector2(0, 0)
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
length = 200.0
rest_length = 40.0
stiffness = 200.0
damping = 20.0
"#;

/// A spring far too weak to hold the bob against gravity — only the `length`
/// ceiling can stop it stretching away.
const SPRING_WEAK_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="Bob" type="RigidBody2D" parent="."]
position = Vector2(0, 40)

[node name="BobShape" type="CollisionShape2D" parent="Bob"]
shape = SubResource("Circle_1")

[node name="Spring" type="DampedSpringJoint2D" parent="."]
position = Vector2(0, 0)
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
length = 60.0
rest_length = 40.0
stiffness = 1.0
damping = 1.0
"#;

/// A pendulum: the arm starts horizontal so gravity has a lever to swing it
/// about the pin, rather than the degenerate straight-down hang that would sit
/// still whether or not the joint solves any rotation.
const PENDULUM_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="Bob" type="RigidBody2D" parent="."]
position = Vector2(40, 0)

[node name="BobShape" type="CollisionShape2D" parent="Bob"]
shape = SubResource("Circle_1")

[node name="Pin" type="PinJoint2D" parent="."]
position = Vector2(0, 0)
node_a = NodePath("../Anchor")
node_b = NodePath("../Bob")
"#;

/// Two overlapping bodies joined by a pin that explicitly re-enables collision.
/// Godot's `disable_collision` defaults to true, so `false` is the interesting
/// case: the pair must still collide.
const COLLIDING_PAIR_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Left" type="RigidBody2D" parent="."]
position = Vector2(-4, 0)

[node name="LeftShape" type="CollisionShape2D" parent="Left"]
shape = SubResource("Circle_1")

[node name="Right" type="RigidBody2D" parent="."]
position = Vector2(4, 0)

[node name="RightShape" type="CollisionShape2D" parent="Right"]
shape = SubResource("Circle_1")

[node name="Pin" type="PinJoint2D" parent="."]
position = Vector2(0, 0)
node_a = NodePath("../Left")
node_b = NodePath("../Right")
disable_collision = false
"#;

/// The same overlapping pair with Godot's default `disable_collision` (true).
const NONCOLLIDING_PAIR_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Left" type="RigidBody2D" parent="."]
position = Vector2(-4, 0)

[node name="LeftShape" type="CollisionShape2D" parent="Left"]
shape = SubResource("Circle_1")

[node name="Right" type="RigidBody2D" parent="."]
position = Vector2(4, 0)

[node name="RightShape" type="CollisionShape2D" parent="Right"]
shape = SubResource("Circle_1")

[node name="Pin" type="PinJoint2D" parent="."]
position = Vector2(0, 0)
node_a = NodePath("../Left")
node_b = NodePath("../Right")
"#;

/// A joint whose `node_b` names a node that is not a physics body at all. Godot
/// leaves such a joint inert; it must not be built from half a connection.
const DANGLING_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Anchor" type="StaticBody2D" parent="."]

[node name="AnchorShape" type="CollisionShape2D" parent="Anchor"]
shape = SubResource("Circle_1")

[node name="NotABody" type="Node2D" parent="."]

[node name="Pin" type="PinJoint2D" parent="."]
node_a = NodePath("../Anchor")
node_b = NodePath("../NotABody")
"#;

fn world_from(tscn: &str) -> (PhysicsServer, SceneTree) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);
    (server, tree)
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

/// One physics frame: the server's own sync-step-sync flow.
fn step(server: &mut PhysicsServer, tree: &mut SceneTree) {
    server.sync_to_physics(tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(tree);
}

/// Steps the world and returns the body's final y.
fn settle(server: &mut PhysicsServer, tree: &mut SceneTree, id: NodeId, steps: usize) -> f32 {
    for _ in 0..steps {
        step(server, tree);
    }
    gdscene::node2d::get_position(tree, id).y
}

/// How many joints the scene actually contributed to the world.
fn joint_count(server: &PhysicsServer) -> usize {
    server.world().joint_count()
}

/// **The acceptance test.** A pinned body is held against gravity; the same
/// scene without the joint falls.
#[test]
fn joint2d_scene_nodes_constrain_bodies() {
    // Control first: with no joint the body falls freely.
    let (mut server, mut tree) = world_from(NO_JOINT_TSCN);
    let free_id = node(&tree, "/root/Main/Hanging");
    let free_y = settle(&mut server, &mut tree, free_id, 60);
    assert!(
        free_y > 21.0,
        "without a joint the body must fall under gravity, or this test proves \
         nothing; y went from 20 to {free_y}"
    );

    // With the pin, the same body is constrained near its anchor.
    let (mut server, mut tree) = world_from(PIN_TSCN);
    assert_eq!(
        joint_count(&server),
        1,
        "the `.tscn` PinJoint2D must reach the world as a joint"
    );
    let pinned_id = node(&tree, "/root/Main/Hanging");
    let pinned_y = settle(&mut server, &mut tree, pinned_id, 60);
    assert!(
        pinned_y < free_y - 1.0,
        "the pin must hold the body up: pinned y {pinned_y} should be well above \
         the unjointed {free_y}"
    );
}

/// **Acceptance clause 1** — the pendulum swings about its pin and the joint
/// constraint holds to well under a pixel across 300 ticks.
///
/// The pin is bound the way Godot binds one: each body records where the anchor
/// falls inside itself, so the constraint only asks those two points to
/// coincide. The bob therefore stays exactly one arm-length from the anchor
/// while it rotates around it. The previous solver teleported *both* bodies onto
/// the anchor point, so there was no arm and nothing to swing.
#[test]
fn pin_joint_pendulum_swings_at_constant_anchor_distance() {
    let (mut server, mut tree) = world_from(PENDULUM_TSCN);
    let bob = node(&tree, "/root/Main/Bob");

    // The arm starts horizontal: 40px along +X from the anchor at the origin.
    const ARM: f32 = 40.0;
    let start = gdscene::node2d::get_position(&tree, bob);
    assert!(
        (start.x - ARM).abs() < 1e-3 && start.y.abs() < 1e-3,
        "the bob starts on the +X arm, got {start:?}",
    );

    let mut worst_error: f32 = 0.0;
    let mut swept_down: f32 = 0.0;
    let mut min_x: f32 = start.x;
    for _ in 0..300 {
        step(&mut server, &mut tree);
        let p = gdscene::node2d::get_position(&tree, bob);
        // The constraint pins the bob's own anchor point onto the world anchor
        // at the origin, so its distance from the origin must stay one arm.
        worst_error = worst_error.max((p.length() - ARM).abs());
        swept_down = swept_down.max(p.y);
        min_x = min_x.min(p.x);
    }

    assert!(
        worst_error < 1.0,
        "the pin constraint must hold to under a pixel over 300 ticks, worst \
         deviation from the {ARM}px arm was {worst_error}",
    );
    // Gravity is +Y in Godot 2D, so a swinging arm must travel downward and
    // away from its starting +X position — otherwise the "pendulum" is just
    // frozen in place and the error bound above proves nothing.
    assert!(
        swept_down > 10.0,
        "the bob must actually swing down about the pin, greatest y reached was {swept_down}",
    );
    assert!(
        min_x < ARM - 10.0,
        "the bob must swing away from its starting arm angle, least x was {min_x}",
    );
}

/// **Acceptance clause 2** — a damped spring resizes toward `rest_length`.
///
/// Godot: "the joint always tries to resize towards this length"
/// (`DampedSpringJoint2D.rest_length`).
#[test]
fn damped_spring_joint_settles_toward_rest_length() {
    let (mut server, mut tree) = world_from(SPRING_REST_TSCN);
    let bob = node(&tree, "/root/Main/Bob");

    let start = gdscene::node2d::get_position(&tree, bob).length();
    assert!(
        (start - 80.0).abs() < 1e-3,
        "the bob starts 80px out, twice its 40px rest length; got {start}",
    );

    for _ in 0..300 {
        step(&mut server, &mut tree);
    }
    let settled = gdscene::node2d::get_position(&tree, bob).length();

    assert!(
        settled < 50.0 && settled > 35.0,
        "the spring must pull the bob from 80px back toward its 40px rest \
         length; it settled at {settled}",
    );
}

/// `DampedSpringJoint2D.length` is a hard ceiling: "the two attached bodies
/// cannot stretch it past this value" (Godot's own docs). A spring far too weak
/// to carry the bob against gravity must still not exceed it.
#[test]
fn damped_spring_joint_cannot_stretch_past_its_length() {
    let (mut server, mut tree) = world_from(SPRING_WEAK_TSCN);
    let bob = node(&tree, "/root/Main/Bob");

    for _ in 0..300 {
        step(&mut server, &mut tree);
    }
    let stretched = gdscene::node2d::get_position(&tree, bob).length();

    // Stiffness 1 against 980px/s² gravity would settle far past 60px without
    // the clamp — the un-clamped equilibrium is over 130px.
    assert!(
        stretched <= 60.5,
        "`length = 60` is the spring's maximum extent, but it stretched to {stretched}",
    );
}

/// **Acceptance clause 3** — `disable_collision = false` lets the jointed pair
/// collide.
///
/// Every jointed pair used to be excluded from collision unconditionally, so the
/// property was inert: authoring `false` changed nothing.
#[test]
fn disable_collision_false_lets_the_jointed_pair_collide() {
    // Default (`disable_collision` absent, i.e. true): the overlapping pair
    // reports no contact with each other.
    let (mut server, mut tree) = world_from(NONCOLLIDING_PAIR_TSCN);
    let pair = jointed_pair(&server, &tree);
    step(&mut server, &mut tree);
    assert!(
        !reports_contact(&server, pair),
        "Godot's default `disable_collision = true` must keep the jointed pair \
         from colliding",
    );

    // Explicit `disable_collision = false`: the same overlapping pair collides.
    let (mut server, mut tree) = world_from(COLLIDING_PAIR_TSCN);
    let pair = jointed_pair(&server, &tree);
    step(&mut server, &mut tree);
    assert!(
        reports_contact(&server, pair),
        "`disable_collision = false` must let the jointed pair collide",
    );
}

/// Resolves the two jointed body ids of the overlapping-pair scenes.
fn jointed_pair(server: &PhysicsServer, tree: &SceneTree) -> (BodyId, BodyId) {
    let left = server
        .body_for_node(node(tree, "/root/Main/Left"))
        .expect("Left registered as a body");
    let right = server
        .body_for_node(node(tree, "/root/Main/Right"))
        .expect("Right registered as a body");
    (left, right)
}

/// Whether the last step reported a contact between exactly this pair.
fn reports_contact(server: &PhysicsServer, (a, b): (BodyId, BodyId)) -> bool {
    server
        .last_collision_events()
        .iter()
        .any(|e| (e.body_a == a && e.body_b == b) || (e.body_a == b && e.body_b == a))
}

/// The spring kind reaches the world too, so the bridge is not pin-only.
#[test]
fn damped_spring_joint_reaches_the_world() {
    let (server, _tree) = world_from(SPRING_TSCN);
    assert_eq!(
        joint_count(&server),
        1,
        "a `.tscn` DampedSpringJoint2D must reach the world as a joint"
    );
}

/// A joint pointing at a non-body is skipped rather than half-built — a joint
/// with one real end would drag its body toward the origin.
#[test]
fn joint_with_an_unresolvable_end_is_skipped() {
    let (server, _tree) = world_from(DANGLING_TSCN);
    assert_eq!(
        joint_count(&server),
        0,
        "a joint whose node_b is not a physics body must not be built"
    );
}
