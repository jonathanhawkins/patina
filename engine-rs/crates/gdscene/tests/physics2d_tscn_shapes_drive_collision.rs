//! pat-0q2kx.34 — a real Godot-format `.tscn` drives 2D collision with the
//! shapes it authored, end to end: parse → instance → `register_physics_bodies`
//! → `MainLoop::step` → the solver uses the authored geometry.
//!
//! Godot ground truth: `scene/2d/physics/collision_shape_2d.cpp` and
//! `collision_polygon_2d.cpp` — a CollisionShape2D resolves its `shape`
//! sub-resource, a CollisionPolygon2D builds geometry from its inline `polygon`,
//! and a `disabled` collider contributes nothing.
//!
//! Two distinct silent failures are covered here, both of which produce a body
//! that still collides — just with the wrong geometry, so nothing errors and the
//! scene merely behaves subtly wrong:
//!
//!   * a `shape = SubResource("…")` that never resolves leaves the body with the
//!     legacy default 16px circle, so a ball authored at radius 8 rests 8px too
//!     high;
//!   * a `CollisionPolygon2D` was rejected outright by `shape_from_node` (it
//!     matched only `CollisionShape2D`), so an authored ramp or wall became that
//!     same default circle — a 200px-wide barrier shrank to a 16px ball and
//!     things walked straight past it.
//!
//! NOTE ON SCOPE: the bead's scene also places the ball's shape at offset
//! (0,10). `PhysicsBody2D` has no shape-offset field — a shape is always centred
//! on the body origin — so that case is deliberately NOT asserted here; it needs
//! an offset added to the body model and threaded through the narrow-phase,
//! raycast and overlap paths in gdphysics2d. Asserting it now would encode a
//! resting height the engine cannot produce. Everything below is offset-free and
//! genuinely reachable today.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::physics_server::PhysicsServer;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Floor top surface is y = 90 (centre 100, half-height 10). A ball of radius 8
/// rests centred at y = 82; the 16px default fallback would rest at y = 74.
const SUBRESOURCE_SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="RectangleShape2D" id="Rect_floor"]
size = Vector2(400, 20)

[sub_resource type="CircleShape2D" id="Circle_ball"]
radius = 8.0

[node name="World" type="Node2D"]

[node name="Floor" type="StaticBody2D" parent="."]
position = Vector2(0, 100)

[node name="FloorShape" type="CollisionShape2D" parent="Floor"]
shape = SubResource("Rect_floor")

[node name="Ball" type="RigidBody2D" parent="."]
position = Vector2(0, 0)

[node name="BallShape" type="CollisionShape2D" parent="Ball"]
shape = SubResource("Circle_ball")
"#;

/// A `CollisionPolygon2D` barrier spanning x ∈ [95,300], y ∈ [-100,100], with a
/// CharacterBody2D walking into it from the left at 2px/tick.
///
/// The barrier is deliberately DEEP and the walker deliberately slow. A thin
/// 10px barrier with a 10px-per-tick walker lets the character step clean over
/// the far face in a single tick and settle on the wrong side — which reads as
/// "the polygon never became a shape" while actually proving the opposite.
const POLYGON_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Barrier" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="Poly" type="CollisionPolygon2D" parent="Barrier"]
polygon = PackedVector2Array(95, -100, 300, -100, 300, 100, 95, 100)

[node name="Walker" type="CharacterBody2D" parent="."]
position = Vector2(0, 0)
velocity = Vector2(120, 0)

[node name="WalkerShape" type="CollisionShape2D" parent="Walker"]
radius = 4.0
"#;

/// The same barrier, disabled. Godot contributes no collision at all — and in
/// particular must NOT fall back to a default shape.
const DISABLED_POLYGON_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Barrier" type="StaticBody2D" parent="."]
position = Vector2(0, 0)

[node name="Poly" type="CollisionPolygon2D" parent="Barrier"]
polygon = PackedVector2Array(95, -100, 300, -100, 300, 100, 95, 100)
disabled = true

[node name="Walker" type="CharacterBody2D" parent="."]
position = Vector2(0, 0)
velocity = Vector2(120, 0)

[node name="WalkerShape" type="CollisionShape2D" parent="Walker"]
radius = 4.0
"#;

/// Returns the tree and the instanced scene's root, so nodes are looked up
/// RELATIVE to it — the `/root/...` absolute form is a runner-side convention
/// that a bare `SceneTree` does not provide.
fn load(tscn: &str) -> (SceneTree, gdscene::NodeId) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (tree, scene_root)
}

fn node_pos(tree: &SceneTree, scene_root: gdscene::NodeId, name: &str) -> Vector2 {
    let id = tree
        .get_node_or_null(scene_root, name)
        .unwrap_or_else(|| panic!("node {name} exists"));
    match tree.get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("expected a Vector2 position, got {other:?}"),
    }
}

/// **The acceptance test.** An authored `.tscn` sub-resource shape — not the
/// default fallback — decides where the ball comes to rest.
#[test]
fn physics2d_tscn_subresource_shapes_drive_collision() {
    let (tree, scene_root) = load(SUBRESOURCE_SCENE);
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // One second: free-fall from y=0 to the floor is ~82px, reached in ~0.41s.
    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }

    let y = node_pos(ml.tree(), scene_root, "Ball").y;

    // ⚠️ radius 8 rests at y=82; the unresolved-sub-resource fallback (a 16px
    // circle) rests at y=74. The 8px gap is the whole point of the test, so the
    // tolerance has to stay well under it.
    assert!(
        (y - 82.0).abs() < 2.0,
        "the ball must rest on the floor using its AUTHORED radius-8 shape \
         (y≈82); y={y} — y≈74 means the `shape = SubResource(...)` never \
         resolved and the body fell back to the default 16px circle"
    );
}

/// A `CollisionPolygon2D` is real collision geometry: the barrier stops the
/// walker. Before this bead the polygon resolved to nothing and the body became
/// a 16px circle at the barrier's ORIGIN (0,0) — which is where the walker
/// already stands, so it never blocked anything at x≈95.
#[test]
fn tscn_collision_polygon2d_blocks_movement() {
    let (mut tree, scene_root) = load(POLYGON_SCENE);
    let mut server = PhysicsServer::new();
    server.set_gravity(Vector2::ZERO);
    server.register_bodies(&tree);

    // 2px per tick; the near face is 95px away, so ~48 ticks to reach it.
    for _ in 0..60 {
        server.process_character_movement(1.0 / 60.0, &mut tree);
    }

    let x = node_pos(&tree, scene_root, "Walker").x;
    assert!(
        x < 95.0,
        "the walker must be stopped by the authored polygon barrier at x=95, \
         got x={x} — the polygon never became a collision shape"
    );
    assert!(
        x > 80.0,
        "the walker must actually reach the barrier rather than being blocked \
         early by a stray default shape near the origin, got x={x}"
    );
}

/// A disabled polygon contributes nothing, and must not fall back to a default
/// shape that blocks the walker anyway.
#[test]
fn tscn_disabled_collision_polygon2d_is_ignored() {
    let (mut tree, scene_root) = load(DISABLED_POLYGON_SCENE);
    let mut server = PhysicsServer::new();
    server.set_gravity(Vector2::ZERO);
    server.register_bodies(&tree);

    for _ in 0..60 {
        server.process_character_movement(1.0 / 60.0, &mut tree);
    }

    let x = node_pos(&tree, scene_root, "Walker").x;
    assert!(
        x > 95.0,
        "a disabled polygon must contribute no collision — the walker should \
         pass the barrier line, got x={x}"
    );
}
