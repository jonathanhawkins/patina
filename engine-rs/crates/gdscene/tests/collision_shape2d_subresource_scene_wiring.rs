//! pat-0q2kx.28 — a `.tscn`-authored `Shape2D` sub-resource becomes the real
//! collision shape.
//!
//! Godot builds a body's shapes from `Shape2D` sub-resources whose typed properties
//! define the geometry (upstream/godot/scene/2d/physics/collision_shape_2d.cpp).
//! Patina's `shape_from_node` only understood a test-only convention — the shape's
//! *class name* in the `shape` property with dimensions on the node — so the real
//! parser output `shape = "SubResource:<id>"` matched no arm, returned `None`, and
//! every authored shape silently became a 16px circle. A 64×16 platform collided as
//! a small ball.
//!
//! These drive the real `PackedScene` → `PhysicsServer` path and assert the
//! resulting body: its extents by raycast, that `disabled` contributes nothing, and
//! that `one_way_collision` reaches the solver.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::physics_server::PhysicsServer;
use gdscene::SceneTree;

/// A static body whose shape is a genuine `RectangleShape2D` sub-resource of
/// 64×16, exactly as the Godot editor writes it.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_x"]
size = Vector2(64, 16)

[node name="Root" type="Node2D"]

[node name="Platform" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="Shape" type="CollisionShape2D" parent="Platform"]
shape = SubResource("RectangleShape2D_x")
"#;

/// The same scene with the shape disabled — Godot contributes no collision.
const SCENE_DISABLED: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_x"]
size = Vector2(64, 16)

[node name="Root" type="Node2D"]

[node name="Platform" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="Shape" type="CollisionShape2D" parent="Platform"]
shape = SubResource("RectangleShape2D_x")
disabled = true
"#;

/// A one-way platform: passable from below, solid from above.
const SCENE_ONE_WAY: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_x"]
size = Vector2(64, 16)

[node name="Root" type="Node2D"]

[node name="Platform" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="Shape" type="CollisionShape2D" parent="Platform"]
shape = SubResource("RectangleShape2D_x")
one_way_collision = true
"#;

/// Loads a scene and runs the physics sync that registers bodies.
fn world_from(scene_src: &str) -> (PhysicsServer, SceneTree, gdscene::NodeId) {
    let scene = PackedScene::from_tscn(scene_src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let platform = tree
        .get_node_or_null(scene_root, "Platform")
        .expect("Platform node");

    let mut server = PhysicsServer::new();
    server.sync_to_physics(&mut tree);
    (server, tree, platform)
}

#[test]
fn collision_shape2d_subresource_scene_wiring() {
    let (server, _tree, platform) = world_from(SCENE);

    // The sub-resource's geometry reached the body, rather than the 16px circle
    // fallback: the box spans x ∈ [-32, 32], y ∈ [-8, 8].
    let body_id = server
        .body_for_node(platform)
        .expect("the StaticBody2D registered a physics body");
    let body = server.world().get_body(body_id).expect("body exists");
    match body.shape {
        gdphysics2d::shape::Shape2D::Rectangle { half_extents } => {
            assert!(
                (half_extents.x - 32.0).abs() < 0.01 && (half_extents.y - 8.0).abs() < 0.01,
                "expected 64x16 (half extents 32x8) from the sub-resource, got {half_extents:?}"
            );
        }
        other => panic!("authored RectangleShape2D became {other:?} — sub-resource unresolved"),
    }

    // …and the extents are real to the solver: a ray at x=+30 is inside the box,
    // x=+40 is past its edge. Cast downward from above the platform.
    let hit_inside =
        server
            .world()
            .raycast(Vector2::new(30.0, -40.0), Vector2::new(0.0, 1.0), 80.0);
    let miss_outside =
        server
            .world()
            .raycast(Vector2::new(40.0, -40.0), Vector2::new(0.0, 1.0), 80.0);
    assert!(
        hit_inside.is_some(),
        "a ray at x=+30 must hit a 64-wide box centred on the origin"
    );
    assert!(
        miss_outside.is_none(),
        "a ray at x=+40 must miss it — the body is wider than authored"
    );
}

#[test]
fn disabled_shape_contributes_no_collision() {
    let (server, _tree, platform) = world_from(SCENE_DISABLED);

    // Godot skips a disabled shape entirely. The body may still exist, but nothing
    // may collide with it — and in particular it must NOT fall back to a default
    // circle, which is what silently happened before.
    // Cast off-axis (x=10, well inside the 64-wide box were it enabled) so the
    // result cannot hinge on a boundary case at the body's exact origin.
    let hit = server
        .world()
        .raycast(Vector2::new(10.0, -40.0), Vector2::new(0.0, 1.0), 80.0);
    if let Some(body_id) = server.body_for_node(platform) {
        if let Some(body) = server.world().get_body(body_id) {
            // The body may exist with degenerate geometry, but it must not have
            // taken the authored 64×16 extents nor the legacy default circle.
            match body.shape {
                gdphysics2d::shape::Shape2D::Rectangle { half_extents } => assert!(
                    half_extents.x.abs() < 0.01 && half_extents.y.abs() < 0.01,
                    "a disabled shape still supplied geometry: {half_extents:?}"
                ),
                ref other => panic!("disabled shape produced a collidable {other:?}"),
            }
        }
    }
    assert!(
        hit.is_none(),
        "a disabled CollisionShape2D must produce no collidable geometry"
    );
}

#[test]
fn one_way_collision_reaches_the_body() {
    let (server, _tree, platform) = world_from(SCENE_ONE_WAY);
    let body_id = server.body_for_node(platform).expect("body registered");
    let body = server.world().get_body(body_id).expect("body exists");

    assert!(
        body.one_way_collision,
        "one_way_collision on the shape node must reach the body the solver reads"
    );
    // The default direction already means "blocks from above", which is what an
    // unrotated one-way platform does in Godot.
    assert!(
        body.one_way_direction.y < 0.0,
        "one-way direction should face up (negative y), got {:?}",
        body.one_way_direction
    );
}
