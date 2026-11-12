//! pat-s3rrv.104 — a 3D body owns a LIST of collision shapes, each at its own
//! offset, and that list is re-read every physics tick.
//!
//! Godot ground truth: `scene/3d/physics/collision_object_3d.cpp`
//! (`shape_owner_add_shape` / `shape_owner_set_transform` /
//! `shape_owner_set_disabled`) and `doc/classes/CollisionObject3D.xml`. A
//! `CollisionObject3D` owns many shape owners, and a `CollisionShape3D`
//! contributes its own node transform — so an L-shaped platform is two boxes and
//! a character is a capsule raised by half its height. Both are routine
//! authoring, not exotica.
//!
//! # What was wrong
//!
//! `physics_server_3d` picked the FIRST matching direct child
//! (`.find_map(|&child_id| shape_from_node_3d(tree, child_id))`) and discarded
//! every other shape, and `shape_from_node_3d` returned bare geometry with no
//! transform — so every shape collided at the body origin regardless of where it
//! was authored. The shape was also written only in the registration branch, so
//! unlike areas it was never refreshed: a shape moved or resized after the first
//! tick was ignored forever.
//!
//! ⚠️ Everything here is driven by `MainLoop::step` on a parsed `.tscn`. Nothing
//! is injected into the physics world directly, so a pass means the whole chain
//! — parse, instance, subtree walk, offset, broad phase, narrow phase — works.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// ⚠️ `f64`, because `MainLoop::step` takes `delta_secs: f64`. Declaring this
/// `f32` to match the `Vector3` maths around it is a type error, not a lint.
const STEP: f64 = 1.0 / 60.0;

fn load(tscn: &str) -> MainLoop {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    MainLoop::new(tree)
}

fn node_of(ml: &MainLoop, name: &str) -> NodeId {
    let root = ml.tree().root_id();
    ml.tree()
        .find_child(root, name, true)
        .unwrap_or_else(|| panic!("node {name} exists"))
}

/// The node's own world-space height.
///
/// ⚠️ Read from the NODE, not from the physics body. A 3D body registers lazily
/// inside `sync_to_physics`, which only runs from `MainLoop::step`, and reading
/// the body directly would also let a test pass on a value the fixture itself
/// wrote. This is the value a game script would see.
fn y_of(ml: &MainLoop, name: &str) -> f32 {
    node3d::get_global_transform(ml.tree(), node_of(ml, name))
        .origin
        .y
}

fn settle(ml: &mut MainLoop, ticks: usize) {
    for _ in 0..ticks {
        ml.step(STEP);
    }
}

// ---------------------------------------------------------------------------
// (a) two offset boxes on ONE static body
// ---------------------------------------------------------------------------

/// A platform whose collision is two pads at x = -2 and x = +2, with a real gap
/// between them, and a ball dropped above `drop_x`.
///
/// The pads are `2 x 1 x 2`, so each spans 1.0 either side of its centre: the
/// left covers x in [-3, -1], the right x in [1, 3], and nothing covers
/// x in (-1, 1). The ball is a 0.5 sphere, so at x = 0 it spans [-0.5, 0.5] and
/// fits through the gap cleanly rather than clipping a lip.
///
/// ⚠️ Both pads reference the SAME sub-resource. That is deliberate: it makes the
/// two shapes byte-identical, so the only thing distinguishing them is the
/// `position` on their `CollisionShape3D` — which is precisely the offset this
/// bead is about.
fn two_pad_scene(drop_x: f32) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxShape3D" id="Box_pad"]
size = Vector3(2, 1, 2)

[sub_resource type="SphereShape3D" id="Sphere_ball"]
radius = 0.5

[node name="Root" type="Node3D"]

[node name="Platform" type="StaticBody3D" parent="."]

[node name="Left" type="CollisionShape3D" parent="Platform"]
position = Vector3(-2, 0, 0)
shape = SubResource("Box_pad")

[node name="Right" type="CollisionShape3D" parent="Platform"]
position = Vector3(2, 0, 0)
shape = SubResource("Box_pad")

[node name="Ball" type="RigidBody3D" parent="."]
position = Vector3({drop_x}, 4, 0)
lock_rotation = true

[node name="BallShape" type="CollisionShape3D" parent="Ball"]
shape = SubResource("Sphere_ball")
"#
    )
}

fn ball_y_after_drop(drop_x: f32) -> f32 {
    let mut ml = load(&two_pad_scene(drop_x));
    settle(&mut ml, 180);
    y_of(&ml, "Ball")
}

/// **The acceptance, clause (a).** A body with two offset shapes collides with
/// BOTH of them, and with nothing in between.
#[test]
fn a_body_collides_with_every_offset_shape_it_owns() {
    let left = ball_y_after_drop(-2.0);
    let right = ball_y_after_drop(2.0);
    let gap = ball_y_after_drop(0.0);

    // Pad top is y = 0.5, ball radius 0.5, so a ball resting on a pad sits at
    // y ~= 1.0. Generous tolerance: this asserts "stopped on the pad", not the
    // exact penetration slop of the solver.
    assert!(
        (left - 1.0).abs() < 0.25,
        "a ball dropped over the LEFT pad (x = -2) must rest on it at y ~= 1.0, got {left}. \
         The left pad is the body's FIRST shape, so a build that keeps only one shape \
         still passes this clause — it is the right/gap pair below that separates them."
    );
    assert!(
        (right - 1.0).abs() < 0.25,
        "a ball dropped over the RIGHT pad (x = +2) must rest on it at y ~= 1.0, got {right}. \
         This is the clause that fails when only the first CollisionShape3D child is kept: \
         the second pad does not exist, so the ball falls past y = 0 forever."
    );
    assert!(
        gap < -2.0,
        "a ball dropped over the GAP (x = 0) must fall THROUGH, got y = {gap}. \
         This is the control: without it, a build that collided everything against one \
         huge shape at the body origin would satisfy both clauses above and still be wrong."
    );
}

// ---------------------------------------------------------------------------
// (b) a single shape, authored at an offset
// ---------------------------------------------------------------------------

/// A ball whose collision sphere sits `shape_y` above its node origin, dropped
/// onto a wide floor.
fn offset_shape_scene(shape_y: f32) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxShape3D" id="Box_floor"]
size = Vector3(40, 1, 40)

[sub_resource type="SphereShape3D" id="Sphere_ball"]
radius = 0.5

[node name="Root" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
shape = SubResource("Box_floor")

[node name="Ball" type="RigidBody3D" parent="."]
position = Vector3(0, 5, 0)
lock_rotation = true

[node name="BallShape" type="CollisionShape3D" parent="Ball"]
position = Vector3(0, {shape_y}, 0)
shape = SubResource("Sphere_ball")
"#
    )
}

/// **The acceptance, clause (b).** A shape offset upward by 1.0 leaves the body's
/// ORIGIN resting 1.0 lower, because it is the shape that lands, not the origin.
#[test]
fn an_offset_shape_lowers_the_bodys_resting_origin_by_the_offset() {
    let mut centred = load(&offset_shape_scene(0.0));
    settle(&mut centred, 240);
    let centred_y = y_of(&centred, "Ball");

    let mut raised = load(&offset_shape_scene(1.0));
    settle(&mut raised, 240);
    let raised_y = y_of(&raised, "Ball");

    // Sanity: the centred control must actually have landed, not still be falling
    // or have tunnelled. Floor top is y = 0.5, ball radius 0.5 => origin ~= 1.0.
    assert!(
        (centred_y - 1.0).abs() < 0.25,
        "the CONTROL (centred shape) must come to rest at y ~= 1.0, got {centred_y}. \
         Every reading below is meaningless if the control never landed."
    );

    let drop = centred_y - raised_y;
    assert!(
        (drop - 1.0).abs() < 0.2,
        "a CollisionShape3D offset +1.0 in y must leave the node origin 1.0 LOWER \
         (centred {centred_y}, raised {raised_y}, difference {drop}). \
         A difference of ~0.0 means the offset was dropped and both shapes collided \
         at the body origin, which is exactly the old behaviour."
    );
}

// ---------------------------------------------------------------------------
// (c) the shape list is re-read every tick
// ---------------------------------------------------------------------------

/// **The acceptance, clause (c).** Resizing a `CollisionShape3D` AFTER the body
/// has registered changes the simulation on a later tick.
///
/// ⚠️ The resize is written to `__shape__size`, which is where the `.tscn`
/// instancer copies a `BoxShape3D` sub-resource's dimensions and the key
/// `shape_from_node_3d` prefers when reading them back. Writing plain `size`
/// would be shadowed by the namespaced value still on the node and the test
/// would report "no change" for the wrong reason.
#[test]
fn resizing_a_shape_after_the_first_tick_changes_the_simulation() {
    let scene = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxShape3D" id="Box_floor"]
size = Vector3(40, 1, 40)

[sub_resource type="BoxShape3D" id="Box_block"]
size = Vector3(1, 1, 1)

[node name="Root" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
shape = SubResource("Box_floor")

[node name="Block" type="RigidBody3D" parent="."]
position = Vector3(0, 4, 0)
lock_rotation = true

[node name="BlockShape" type="CollisionShape3D" parent="Block"]
shape = SubResource("Box_block")
"#;

    let mut ml = load(scene);
    settle(&mut ml, 240);
    let small_rest = y_of(&ml, "Block");

    // Floor top 0.5 + half of a 1-unit cube (0.5) => origin ~= 1.0.
    assert!(
        (small_rest - 1.0).abs() < 0.25,
        "the 1x1x1 block must first come to rest at y ~= 1.0, got {small_rest}"
    );

    // Now grow the shape to 3 units tall, long after registration.
    let shape_id = node_of(&ml, "BlockShape");
    ml.tree_mut()
        .get_node_mut(shape_id)
        .expect("shape node")
        .set_property(
            "__shape__size",
            Variant::Vector3(Vector3::new(1.0, 3.0, 1.0)),
        );

    settle(&mut ml, 240);
    let tall_rest = y_of(&ml, "Block");

    // Half of 3 is 1.5, so the origin should now settle at 0.5 + 1.5 = 2.0.
    assert!(
        tall_rest > small_rest + 0.7,
        "growing the CollisionShape3D from 1 to 3 units tall must push the body UP \
         (was {small_rest}, now {tall_rest}). An unchanged value means the shape list \
         is still written only at registration and never re-read, so a shape resized \
         at runtime is ignored forever."
    );
}
