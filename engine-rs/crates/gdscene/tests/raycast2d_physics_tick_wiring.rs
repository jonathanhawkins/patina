//! pat-nzgdu.53 — RayCast2D is driven against the 2D physics world every
//! physics tick by the `MainLoop`, so a scene-loaded ray already reports its
//! collision state after one step with no manual `force_raycast_update` call.
//!
//! Godot ground truth: an enabled `RayCast2D` in a running scene updates itself
//! during the physics step. After a single `_physics_process` tick a ray aimed
//! at a body reports `is_colliding()` and its `get_collider()` resolves to the
//! hit `CollisionObject2D`; a ray aimed at empty space reports `false`.
//!
//! This test proves the wiring through a **scene-load-to-tick** path (not module
//! math): `.tscn` → `PackedScene::from_tscn` → `SceneTree` →
//! `MainLoop::register_physics_bodies` → `MainLoop::step` (which runs the physics
//! phase and then the RayCast2D query phase) → node properties. It mirrors the
//! inline `mainloop_raycast2d_detects_body_and_collider` test in `main_loop.rs`
//! but loads the scene from a `.tscn` string instead of building nodes in code.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Load a `.tscn` string into a fresh `SceneTree`, returning the tree. Mirrors
/// the `load` helper in `particle_2d_scene_renders_moving_particles.rs`.
fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// A scene-loaded RayCast2D aimed at a StaticBody2D reports the collision and
/// its collider after one `MainLoop::step`, while a second ray aimed at empty
/// space reports no collision — with no manual `force_raycast_update`.
#[test]
fn raycast2d_physics_tick_wiring() {
    // World layout (mirrors the inline test):
    //  * "Wall": StaticBody2D at (100, 0) carrying a 10x200 rectangle collision
    //    shape → left edge at x=95, spanning y ∈ [-100, 100].
    //  * "Ray": RayCast2D at the origin aimed +X (target (200, 0)), long enough
    //    to reach the wall → hits.
    //  * "RayMiss": RayCast2D at the origin aimed straight up (target (0, -200))
    //    at nothing → misses.
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Wall" type="StaticBody2D" parent="."]
position = Vector2(100, 0)

[node name="Shape" type="CollisionShape2D" parent="Wall"]
size = Vector2(10, 200)

[node name="Ray" type="RayCast2D" parent="."]
position = Vector2(0, 0)
enabled = true
target_position = Vector2(200, 0)

[node name="RayMiss" type="RayCast2D" parent="."]
position = Vector2(0, 0)
enabled = true
target_position = Vector2(0, -200)
"#;

    let tree = load(tscn);
    let wall_id = tree
        .get_node_by_path("/root/World/Wall")
        .expect("Wall StaticBody2D node");
    let ray_id = tree
        .get_node_by_path("/root/World/Ray")
        .expect("Ray RayCast2D node");
    let miss_id = tree
        .get_node_by_path("/root/World/RayMiss")
        .expect("RayMiss RayCast2D node");

    // Register the StaticBody2D into the physics world, then advance one frame.
    // `step` runs the physics phase (syncing the wall into the world) followed by
    // the RayCast2D query phase, which publishes `is_colliding` / `collider`.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    // 1. The aimed ray reports a collision after a single physics tick, with no
    //    manual force_raycast_update call.
    let colliding = ml
        .tree()
        .get_node(ray_id)
        .map(|n| n.get_property("is_colliding"));
    assert!(
        matches!(colliding, Some(Variant::Bool(true))),
        "RayCast2D aimed at the wall must report is_colliding=true after one step, got {:?}",
        colliding,
    );

    // 2. The ray's `collider` resolves to the Wall StaticBody2D node. The
    //    MainLoop stores it as the node's raw id (matching the inline test).
    match ml
        .tree()
        .get_node(ray_id)
        .map(|n| n.get_property("collider"))
    {
        Some(Variant::Int(raw)) => assert_eq!(
            raw as u64,
            wall_id.raw(),
            "RayCast2D collider must be the Wall StaticBody2D node",
        ),
        other => panic!("expected collider to be an Int node id, got {:?}", other),
    }

    // 3. The ray aimed at empty space reports no collision.
    let miss = ml
        .tree()
        .get_node(miss_id)
        .map(|n| n.get_property("is_colliding"));
    assert!(
        matches!(miss, Some(Variant::Bool(false))),
        "RayCast2D aimed at nothing must report is_colliding=false, got {:?}",
        miss,
    );

    // NOTE on `collision_point`: the RayCast2D query phase currently publishes
    // only `is_colliding` and `collider` (see `MainLoop::process_raycasts` in
    // `main_loop.rs`, which does not write a `collision_point` property). The
    // `gdphysics2d` `RaycastHit` does carry the hit `point`, so exposing
    // `collision_point` is a one-line follow-up in that update block — but it
    // lives in `main_loop.rs`, so it is intentionally left to a follow-up here.
}
