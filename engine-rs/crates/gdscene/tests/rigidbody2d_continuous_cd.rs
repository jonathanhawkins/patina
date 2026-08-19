//! pat-s3rrv.100 — `RigidBody2D.continuous_cd` actually sweeps.
//!
//! ⚠️ This closes a FALSE GREEN, not just a missing feature. `continuous_cd` is
//! registered in ClassDB, so a `.tscn` could author it and the inspector would
//! display it — while `gdphysics2d::ccd` (a complete, unit-tested swept-TOI
//! module) had **zero callers** and nothing anywhere read the property. A fast
//! bullet went straight through a wall with the setting visibly enabled.
//!
//! The fixture is built so the bullet MUST tunnel without help: it travels
//! 50px per tick at a wall 4px thick, so the discrete step steps clean over it.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Wall centre; it is thin on purpose.
const WALL_X: f32 = 200.0;
const WALL_THICKNESS: f32 = 4.0;
/// 3000 px/s at 60Hz = 50px per tick, an order of magnitude past the wall.
const SPEED: f32 = 3000.0;

/// `Root -> {Wall, Bullet}` with the bullet fired along +x.
fn bullet_scene(continuous_cd: i64) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(WALL_X, 0.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut ws = Node::new("WallShape", "CollisionShape2D");
    ws.set_property(
        "size",
        Variant::Vector2(Vector2::new(WALL_THICKNESS, 400.0)),
    );
    tree.add_child(wall_id, ws).unwrap();

    let mut bullet = Node::new("Bullet", "RigidBody2D");
    bullet.set_property("position", Variant::Vector2(Vector2::ZERO));
    bullet.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(SPEED, 0.0)),
    );
    // Straight line, so the only thing that can stop it is the wall.
    bullet.set_property("gravity_scale", Variant::Float(0.0));
    // Godot's CCDMode: 0 = DISABLED, 1 = CAST_RAY, 2 = CAST_SHAPE.
    bullet.set_property("continuous_cd", Variant::Int(continuous_cd));
    let bullet_id = tree.add_child(root, bullet).unwrap();
    let mut bs = Node::new("Shape", "CollisionShape2D");
    bs.set_property("size", Variant::Vector2(Vector2::new(4.0, 4.0)));
    tree.add_child(bullet_id, bs).unwrap();

    (MainLoop::new(tree), bullet_id)
}

fn x_after(continuous_cd: i64, ticks: usize) -> f32 {
    let (mut ml, bullet) = bullet_scene(continuous_cd);
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
    match ml
        .tree()
        .get_node(bullet)
        .map(|n| n.get_property("position"))
    {
        Some(Variant::Vector2(v)) => v.x,
        other => panic!("bullet must carry a Vector2 position, got {other:?}"),
    }
}

/// ⚠️ The control, and the whole reason the fixture is shaped like this:
/// **without** CCD the bullet tunnels. If it did not, the test below would pass
/// against a solver that simply stops fast bodies for unrelated reasons.
#[test]
fn without_continuous_cd_a_fast_body_tunnels_through_the_wall() {
    let x = x_after(0, 20);
    assert!(
        x > WALL_X,
        "the fixture must actually tunnel with CCD disabled — the bullet ended \
         at {x}, short of the wall at {WALL_X}, so it is not moving fast enough \
         to skip over it and the CCD test proves nothing"
    );
}

/// **The acceptance.** With `continuous_cd` on, the sweep stops it at the wall.
#[test]
fn continuous_cd_stops_a_fast_body_at_the_wall() {
    let x = x_after(1, 20);
    assert!(
        x < WALL_X,
        "with continuous_cd enabled the bullet must be pulled back to its first \
         time of impact instead of passing through; it ended at {x}, past the \
         wall at {WALL_X}"
    );
    // And it must have actually travelled — a sweep that clamped everything to
    // the start position would also satisfy the bound above.
    assert!(
        x > WALL_X / 2.0,
        "the bullet must still fly most of the way to the wall, got {x}; a value \
         near zero means the clamp is firing on the first step, which is what \
         happens if a sweep that STARTS inside a radius is treated as a hit"
    );
}

/// `CAST_SHAPE` (2) enables the sweep just as `CAST_RAY` (1) does.
///
/// Godot distinguishes the two by how the sweep is performed, not by whether it
/// happens; Patina sweeps one way, so both non-zero modes must behave alike
/// rather than mode 2 silently falling back to no sweep at all.
#[test]
fn cast_shape_mode_also_enables_the_sweep() {
    let x = x_after(2, 20);
    assert!(
        x < WALL_X,
        "continuous_cd = 2 (CAST_SHAPE) must sweep too, but the bullet reached {x}"
    );
}
