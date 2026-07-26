//! pat-0q2kx.115 acceptance: a `.tscn` RigidBody2D's Godot property surface
//! reaches the physics body and changes how it simulates.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/rigid_body_2d.cpp` —
//! `freeze` (+`freeze_mode`), `lock_rotation`, `can_sleep`/`sleeping`, and
//! `physics_material_override`, a `PhysicsMaterial` resource carrying
//! `friction`/`bounce` (scene/resources/physics_material.cpp). A body has **no**
//! flat `bounce`/`friction` of its own.
//!
//! Patina modelled all of this on the body — `integrate` skips a frozen or
//! sleeping body and zeroes angular motion under `lock_rotation`
//! (gdphysics2d/src/body.rs:307-341) — but the bridge never read any of it from
//! the node: `register_bodies` took only mass/layers/velocities/damping, and
//! nothing resolved a `PhysicsMaterial` sub-resource at all. An authored
//! `freeze = true` simulated exactly like an unfrozen body.
//!
//! ⚠️ Every property here arrives as `.tscn` text through `PackedScene`. Nothing
//! is injected, because test-side injection is what would hide the missing read.

use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

/// A frozen body and an identical unfrozen twin, both under gravity.
const FREEZE_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Frozen" type="RigidBody2D" parent="."]
position = Vector2(0, 0)
freeze = true

[node name="FrozenShape" type="CollisionShape2D" parent="Frozen"]
shape = SubResource("Circle_1")

[node name="Falling" type="RigidBody2D" parent="."]
position = Vector2(100, 0)

[node name="FallingShape" type="CollisionShape2D" parent="Falling"]
shape = SubResource("Circle_1")
"#;

/// A body with `lock_rotation` spun by an authored angular velocity.
const LOCK_ROTATION_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[node name="Main" type="Node2D"]

[node name="Locked" type="RigidBody2D" parent="."]
lock_rotation = true
angular_velocity = 8.0

[node name="LockedShape" type="CollisionShape2D" parent="Locked"]
shape = SubResource("Circle_1")

[node name="Spinning" type="RigidBody2D" parent="."]
position = Vector2(100, 0)
angular_velocity = 8.0

[node name="SpinningShape" type="CollisionShape2D" parent="Spinning"]
shape = SubResource("Circle_1")
"#;

/// A bouncy `PhysicsMaterial` override next to a body with none.
const MATERIAL_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="CircleShape2D" id="Circle_1"]
radius = 8.0

[sub_resource type="PhysicsMaterial" id="Bouncy_1"]
bounce = 0.8
friction = 0.25

[node name="Main" type="Node2D"]

[node name="Bouncy" type="RigidBody2D" parent="."]
physics_material_override = SubResource("Bouncy_1")

[node name="BouncyShape" type="CollisionShape2D" parent="Bouncy"]
shape = SubResource("Circle_1")

[node name="Plain" type="RigidBody2D" parent="."]
position = Vector2(100, 0)

[node name="PlainShape" type="CollisionShape2D" parent="Plain"]
shape = SubResource("Circle_1")
"#;

fn tree_from(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

fn y_of(ml: &MainLoop, id: NodeId) -> f32 {
    gdscene::node2d::get_position(ml.tree(), id).y
}

fn rotation_of(ml: &MainLoop, id: NodeId) -> f32 {
    gdscene::node2d::get_rotation(ml.tree(), id)
}

/// Reads a registered body's field straight out of the physics world — the
/// value the solver actually uses, not the node property it came from.
fn body_bounce(ml: &MainLoop, id: NodeId) -> f32 {
    let server = ml.physics_server();
    let body_id = server
        .body_for_node(id)
        .expect("node should be registered as a physics body");
    server
        .world()
        .get_body(body_id)
        .expect("registered body should exist in the world")
        .bounce
}

/// **The acceptance test.** `freeze` holds a body still, `lock_rotation` pins
/// its rotation, and a `PhysicsMaterial` override supplies restitution — each
/// against an otherwise identical twin, so a scene-wide failure to simulate
/// cannot pass for success.
#[test]
fn rigidbody2d_full_property_sync() {
    // (a) freeze: the frozen body holds position while its twin falls.
    let tree = tree_from(FREEZE_TSCN);
    let frozen = node(&tree, "/root/Main/Frozen");
    let falling = node(&tree, "/root/Main/Falling");
    let mut ml = MainLoop::new(tree);
    // `MainLoop::step` does not register bodies for you — the scene has to be
    // handed to the physics server first, exactly as
    // `physics2d_tscn_shapes_drive_collision` does. Skipping it leaves the world
    // empty and every body silently motionless.
    ml.register_physics_bodies();
    let frozen_start = y_of(&ml, frozen);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let fell = y_of(&ml, falling);
    assert!(
        fell > 1.0,
        "the control body must fall under gravity, or this test proves nothing; \
         y went to {fell}"
    );
    assert!(
        (y_of(&ml, frozen) - frozen_start).abs() < 0.001,
        "`freeze = true` must hold the body still; it moved from {frozen_start} \
         to {}",
        y_of(&ml, frozen)
    );

    // (b) lock_rotation: angular velocity does not accumulate into rotation.
    let tree = tree_from(LOCK_ROTATION_TSCN);
    let locked = node(&tree, "/root/Main/Locked");
    let spinning = node(&tree, "/root/Main/Spinning");
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        rotation_of(&ml, spinning).abs() > 0.1,
        "the control body must spin, or this test proves nothing; rotation is {}",
        rotation_of(&ml, spinning)
    );
    assert!(
        rotation_of(&ml, locked).abs() < 0.001,
        "`lock_rotation = true` must pin rotation at 0, got {}",
        rotation_of(&ml, locked)
    );

    // (c) physics_material_override: the resource's bounce reaches the solver.
    let tree = tree_from(MATERIAL_TSCN);
    let bouncy = node(&tree, "/root/Main/Bouncy");
    let plain = node(&tree, "/root/Main/Plain");
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);
    assert!(
        (body_bounce(&ml, bouncy) - 0.8).abs() < 0.001,
        "the PhysicsMaterial's bounce must reach the body, got {}",
        body_bounce(&ml, bouncy)
    );
    assert!(
        body_bounce(&ml, plain).abs() < 0.001,
        "a body with no override keeps the default restitution, got {}",
        body_bounce(&ml, plain)
    );
}

/// The override is flattened onto the node by `PackedScene`, which is the half
/// the physics bridge cannot do for itself — asserted separately so a failure
/// says which side broke.
#[test]
fn physics_material_override_is_flattened_onto_the_node() {
    let tree = tree_from(MATERIAL_TSCN);
    let bouncy = node(&tree, "/root/Main/Bouncy");
    let props = tree.get_node(bouncy).expect("node exists");

    assert!(
        !props.get_property("__physics_material__bounce").is_nil(),
        "PackedScene must flatten the PhysicsMaterial sub-resource onto the body"
    );
    assert!(
        !props.get_property("__physics_material__friction").is_nil(),
        "friction travels with bounce"
    );

    let plain = node(&tree, "/root/Main/Plain");
    assert!(
        tree.get_node(plain)
            .expect("node exists")
            .get_property("__physics_material__bounce")
            .is_nil(),
        "a body with no override must not gain material keys"
    );
}
