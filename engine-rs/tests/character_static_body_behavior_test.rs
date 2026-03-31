//! pat-woja: CharacterBody2D and StaticBody2D behavior fixtures.
//!
//! Focused behavioral tests for CharacterBody2D movement, blocking, and
//! collision expectations — both programmatic and fixture-loaded from
//! `character_body_test.tscn`.
//!
//! Godot 4.x contract:
//! - CharacterBody2D is kinematic: moves only via `move_and_slide(velocity)`.
//! - StaticBody2D has infinite mass and never moves.
//! - `is_on_floor` / `is_on_wall` / `is_on_ceiling` are set after move_and_slide.
//! - Collision layer/mask filtering controls which bodies interact.
//!
//! Implementation note: Patina's process_character_movement reads velocity
//! from the scene node (in px/s), computes displacement = velocity * dt,
//! calls move_and_slide(displacement), and writes the *remaining displacement*
//! back as velocity. This means velocity is consumed after one frame. Tests
//! are designed to verify collision behavior within a single physics step.

use std::path::PathBuf;

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn vec2_prop(tree: &SceneTree, node_id: gdscene::node::NodeId, key: &str) -> Vector2 {
    match tree.get_node(node_id).unwrap().get_property(key) {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    }
}

fn bool_prop(tree: &SceneTree, node_id: gdscene::node::NodeId, key: &str) -> bool {
    match tree.get_node(node_id).unwrap().get_property(key) {
        Variant::Bool(b) => b,
        _ => false,
    }
}

// ===========================================================================
// Scene builders — characters placed close enough to collide in 1 frame
// ===========================================================================

/// CharacterBody2D just above a StaticBody2D platform.
/// Circle radius 12 at y=185, platform rect (200x20) at y=200 (top edge at 190).
/// Displacement at 600 px/s * 1/60 = 10 px puts center at 195 -> bottom at 207 > 190 = collision.
fn make_falling_character_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 175.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::new(0.0, 600.0)));
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(12.0));
    tree.add_child(player_id, s).unwrap();

    let mut platform = Node::new("Platform", "StaticBody2D");
    platform.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    platform.set_property("collision_layer", Variant::Int(1));
    platform.set_property("collision_mask", Variant::Int(0));
    let platform_id = tree.add_child(root, platform).unwrap();
    let mut ps = Node::new("Shape", "CollisionShape2D");
    ps.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(platform_id, ps).unwrap();

    (tree, player_id, platform_id)
}

/// CharacterBody2D moving rightward toward a nearby StaticBody2D wall.
/// Circle radius 10 at x=125, wall rect (20x200) at x=150 (left edge at 140).
/// Displacement at 1200 px/s * 1/60 = 20 px puts center at 145 -> right edge at 155 > 140 = collision.
fn make_wall_collision_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(125.0, 100.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::new(1200.0, 0.0)));
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(player_id, s).unwrap();

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(150.0, 100.0)));
    wall.set_property("collision_layer", Variant::Int(1));
    wall.set_property("collision_mask", Variant::Int(0));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut ws = Node::new("Shape", "CollisionShape2D");
    ws.set_property("size", Variant::Vector2(Vector2::new(20.0, 200.0)));
    tree.add_child(wall_id, ws).unwrap();

    (tree, player_id, wall_id)
}

/// CharacterBody2D moving upward toward a nearby StaticBody2D ceiling.
/// Circle radius 10 at y=45, ceiling rect (400x20) at y=20 (bottom edge at 30).
/// Displacement at -1200 px/s * 1/60 = -20 px puts center at 25 -> top edge at 15 < 30 = collision.
fn make_ceiling_collision_scene() -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 45.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::new(0.0, -1200.0)));
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(player_id, s).unwrap();

    let mut ceiling = Node::new("Ceiling", "StaticBody2D");
    ceiling.set_property("position", Variant::Vector2(Vector2::new(100.0, 20.0)));
    ceiling.set_property("collision_layer", Variant::Int(1));
    ceiling.set_property("collision_mask", Variant::Int(0));
    let _ceiling_id = tree.add_child(root, ceiling).unwrap();
    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(_ceiling_id, cs).unwrap();

    (tree, player_id)
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

fn load_character_body_tscn() -> SceneTree {
    let path = fixtures_dir().join("scenes/character_body_test.tscn");
    let tscn = std::fs::read_to_string(&path).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::LifecycleManager::enter_tree(&mut tree, root);
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree
}

// ===========================================================================
// 1. CharacterBody2D lands on platform: is_on_floor true after 1 step
// ===========================================================================

#[test]
fn character_falls_onto_platform_is_on_floor() {
    let (tree, player_id, _) = make_falling_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // One step is enough: player is close to platform with high velocity.
    ml.step(1.0 / 60.0);

    assert!(
        bool_prop(ml.tree(), player_id, "is_on_floor"),
        "player should be on floor after landing on platform"
    );
}

// ===========================================================================
// 2. CharacterBody2D is blocked by platform (does not pass through)
// ===========================================================================

#[test]
fn character_blocked_by_platform_does_not_pass_through() {
    let (tree, player_id, _) = make_falling_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let player_pos = vec2_prop(ml.tree(), player_id, "position");

    // Platform top edge at y = 200 - 10 = 190. Player radius 12.
    // Player center should be at most 190 - 12 = 178.
    assert!(
        player_pos.y < 200.0,
        "player should not pass through platform: player_y={}, platform_y=200",
        player_pos.y
    );
}

// ===========================================================================
// 3. StaticBody2D platform does not move under collision
// ===========================================================================

#[test]
fn static_platform_immovable_under_character_collision() {
    let (tree, _, platform_id) = make_falling_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = vec2_prop(ml.tree(), platform_id, "position");

    ml.run_frames(10, 1.0 / 60.0);

    let pos_after = vec2_prop(ml.tree(), platform_id, "position");
    assert_eq!(
        pos_before, pos_after,
        "StaticBody2D platform must not move under collision"
    );
}

// ===========================================================================
// 4. CharacterBody2D hits wall: is_on_wall becomes true
// ===========================================================================

#[test]
fn character_hits_wall_is_on_wall() {
    let (tree, player_id, _) = make_wall_collision_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    assert!(
        bool_prop(ml.tree(), player_id, "is_on_wall"),
        "player should be on_wall after hitting wall"
    );
}

// ===========================================================================
// 5. CharacterBody2D blocked by wall (does not pass through)
// ===========================================================================

#[test]
fn character_blocked_by_wall_does_not_pass_through() {
    let (tree, player_id, _) = make_wall_collision_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let player_pos = vec2_prop(ml.tree(), player_id, "position");

    // Wall left edge at x = 150 - 10 = 140. Player radius 10.
    assert!(
        player_pos.x < 150.0,
        "player should not pass through wall: player_x={}, wall_x=150",
        player_pos.x
    );
}

// ===========================================================================
// 6. StaticBody2D wall does not move under collision
// ===========================================================================

#[test]
fn static_wall_immovable_under_character_collision() {
    let (tree, _, wall_id) = make_wall_collision_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = vec2_prop(ml.tree(), wall_id, "position");

    ml.run_frames(10, 1.0 / 60.0);

    let pos_after = vec2_prop(ml.tree(), wall_id, "position");
    assert_eq!(
        pos_before, pos_after,
        "StaticBody2D wall must not move under collision"
    );
}

// ===========================================================================
// 7. CharacterBody2D hits ceiling: is_on_ceiling becomes true
// ===========================================================================

#[test]
fn character_hits_ceiling_is_on_ceiling() {
    let (tree, player_id) = make_ceiling_collision_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    assert!(
        bool_prop(ml.tree(), player_id, "is_on_ceiling"),
        "player should be on_ceiling after hitting ceiling"
    );
}

// ===========================================================================
// 8. CharacterBody2D blocked by ceiling (does not pass through)
// ===========================================================================

#[test]
fn character_blocked_by_ceiling_does_not_pass_through() {
    let (tree, player_id) = make_ceiling_collision_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let player_pos = vec2_prop(ml.tree(), player_id, "position");

    // Ceiling bottom edge at y = 20 + 10 = 30. Player radius 10.
    assert!(
        player_pos.y > 20.0,
        "player should not pass through ceiling: player_y={}, ceiling_y=20",
        player_pos.y
    );
}

// ===========================================================================
// 9. CharacterBody2D with zero velocity does not move
// ===========================================================================

#[test]
fn character_with_zero_velocity_stays_put() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::ZERO));
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(player_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = vec2_prop(ml.tree(), player_id, "position");
    ml.run_frames(30, 1.0 / 60.0);
    let pos_after = vec2_prop(ml.tree(), player_id, "position");

    assert!(
        approx_eq(pos_before.x, pos_after.x) && approx_eq(pos_before.y, pos_after.y),
        "zero-velocity character should not move: before={pos_before:?}, after={pos_after:?}"
    );
}

// ===========================================================================
// 10. Collision layer/mask: character passes through non-matching body
// ===========================================================================

#[test]
fn character_passes_through_non_matching_collision_layer() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Player on layer/mask 1, high velocity to cover distance in 1 frame
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(50.0, 100.0)));
    player.set_property(
        "velocity",
        Variant::Vector2(Vector2::new(12000.0, 0.0)), // 200 px in 1 frame
    );
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(player_id, s).unwrap();

    // Wall on layer 2 (not matching mask 1) — placed right in the path
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    wall.set_property("collision_layer", Variant::Int(2));
    wall.set_property("collision_mask", Variant::Int(2));
    let _wall_id = tree.add_child(root, wall).unwrap();
    let mut ws = Node::new("Shape", "CollisionShape2D");
    ws.set_property("size", Variant::Vector2(Vector2::new(20.0, 200.0)));
    tree.add_child(_wall_id, ws).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let player_pos = vec2_prop(ml.tree(), player_id, "position");

    // Player should have passed through the wall (different layer)
    assert!(
        player_pos.x > 100.0,
        "player should pass through wall on different layer: player_x={}",
        player_pos.x
    );
}

// ===========================================================================
// 11. Multiple StaticBody2D enclosure: character blocked from all sides
// ===========================================================================

#[test]
fn character_enclosed_by_static_bodies_cannot_escape() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Player at center of a large box, moderate velocity (displacement ~10 px/frame).
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(200.0, 200.0)));
    player.set_property(
        "velocity",
        Variant::Vector2(Vector2::new(600.0, 0.0)), // 10 px/frame rightward
    );
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(8.0));
    tree.add_child(player_id, s).unwrap();

    // Right wall only — simple blocking test
    let mut wall = Node::new("RightWall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(220.0, 200.0)));
    wall.set_property("collision_layer", Variant::Int(1));
    wall.set_property("collision_mask", Variant::Int(0));
    let wid = tree.add_child(root, wall).unwrap();
    let mut ws = Node::new("Shape", "CollisionShape2D");
    ws.set_property("size", Variant::Vector2(Vector2::new(20.0, 200.0)));
    tree.add_child(wid, ws).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let player_pos = vec2_prop(ml.tree(), player_id, "position");

    // Wall left edge at x = 220 - 10 = 210. Player radius 8.
    // Player center should be at most 210 - 8 = 202.
    assert!(
        player_pos.x < 220.0,
        "player should be blocked by right wall: x={}",
        player_pos.x
    );
}

// ===========================================================================
// 12. Fixture-loaded: bodies registered correctly
// ===========================================================================

#[test]
fn fixture_tscn_registers_all_physics_bodies() {
    let tree = load_character_body_tscn();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    assert_eq!(
        ml.physics_server().body_count(),
        3,
        "tscn should register Player + Platform + Wall"
    );
}

// ===========================================================================
// 13. Fixture-loaded: body types correct
// ===========================================================================

#[test]
fn fixture_tscn_body_types_correct() {
    let tree = load_character_body_tscn();

    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let platform_id = tree.get_node_by_path("/root/World/Platform").unwrap();
    let wall_id = tree.get_node_by_path("/root/World/Wall").unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let player_body = ml.physics_server().body_for_node(player_id).unwrap();
    let platform_body = ml.physics_server().body_for_node(platform_id).unwrap();
    let wall_body = ml.physics_server().body_for_node(wall_id).unwrap();

    assert_eq!(
        ml.physics_server()
            .world()
            .get_body(player_body)
            .unwrap()
            .body_type,
        gdphysics2d::BodyType::Kinematic,
        "Player should be Kinematic"
    );
    assert_eq!(
        ml.physics_server()
            .world()
            .get_body(platform_body)
            .unwrap()
            .body_type,
        gdphysics2d::BodyType::Static,
        "Platform should be Static"
    );
    assert_eq!(
        ml.physics_server()
            .world()
            .get_body(wall_body)
            .unwrap()
            .body_type,
        gdphysics2d::BodyType::Static,
        "Wall should be Static"
    );
}

// ===========================================================================
// 14. Fixture-loaded: StaticBody2D unchanged after simulation
// ===========================================================================

#[test]
fn fixture_tscn_static_bodies_unchanged_after_simulation() {
    let tree = load_character_body_tscn();
    let platform_id = tree.get_node_by_path("/root/World/Platform").unwrap();
    let wall_id = tree.get_node_by_path("/root/World/Wall").unwrap();

    let platform_pos_before = vec2_prop(&tree, platform_id, "position");
    let wall_pos_before = vec2_prop(&tree, wall_id, "position");

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.run_frames(120, 1.0 / 60.0);

    let platform_pos_after = vec2_prop(ml.tree(), platform_id, "position");
    let wall_pos_after = vec2_prop(ml.tree(), wall_id, "position");

    assert_eq!(
        platform_pos_before, platform_pos_after,
        "Platform position must not change"
    );
    assert_eq!(
        wall_pos_before, wall_pos_after,
        "Wall position must not change"
    );
}

// ===========================================================================
// 15. CharacterBody2D movement in first frame: position advances by displacement
// ===========================================================================

#[test]
fn character_position_advances_in_first_frame() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    player.set_property(
        "velocity",
        Variant::Vector2(Vector2::new(600.0, 0.0)), // 10 px/frame at 60fps
    );
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(player_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = vec2_prop(ml.tree(), player_id, "position");
    ml.step(1.0 / 60.0);
    let pos_after = vec2_prop(ml.tree(), player_id, "position");

    let dx = pos_after.x - pos_before.x;
    assert!(
        dx > 5.0 && dx < 15.0,
        "character should advance ~10 px in first frame: dx={dx}"
    );
}

// ===========================================================================
// 16. CharacterBody2D corridor: slides along floor while moving horizontally
// ===========================================================================

#[test]
fn character_slides_along_floor_with_diagonal_velocity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Player close to a wide floor, moving diagonally (right + down).
    // Floor top edge at y = 200 - 10 = 190. Player radius 8.
    // Player at y=178, displacement (10, 10) → target y=188, bottom=196 > 190 = collision.
    // Should slide rightward after floor collision.
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(0.0, 178.0)));
    player.set_property(
        "velocity",
        Variant::Vector2(Vector2::new(600.0, 600.0)), // 10 px/frame each axis
    );
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(8.0));
    tree.add_child(player_id, s).unwrap();

    // Wide floor
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(200.0, 200.0)));
    floor.set_property("collision_layer", Variant::Int(1));
    let fid = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("Shape", "CollisionShape2D");
    fs.set_property("size", Variant::Vector2(Vector2::new(600.0, 20.0)));
    tree.add_child(fid, fs).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let player_pos = vec2_prop(ml.tree(), player_id, "position");

    // Player should have moved right (sliding component).
    assert!(
        player_pos.x > 2.0,
        "player should slide rightward along floor: x={}",
        player_pos.x
    );

    // Player should not have passed through the floor (floor top = 190).
    assert!(
        player_pos.y < 195.0,
        "player should be above floor: y={}",
        player_pos.y
    );

    // Should be on floor.
    assert!(
        bool_prop(ml.tree(), player_id, "is_on_floor"),
        "player should be on_floor after diagonal collision"
    );
}

// ===========================================================================
// 17. Multiple StaticBody2D: all remain stationary after extended simulation
// ===========================================================================

#[test]
fn multiple_static_bodies_all_remain_stationary() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ids_and_positions = Vec::new();
    let positions = [
        Vector2::new(0.0, 200.0),
        Vector2::new(100.0, 200.0),
        Vector2::new(200.0, 200.0),
        Vector2::new(0.0, 0.0),
        Vector2::new(200.0, 0.0),
    ];

    for (i, &pos) in positions.iter().enumerate() {
        let mut body = Node::new(&format!("Static{i}"), "StaticBody2D");
        body.set_property("position", Variant::Vector2(pos));
        body.set_property("collision_layer", Variant::Int(1));
        let bid = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("size", Variant::Vector2(Vector2::new(50.0, 20.0)));
        tree.add_child(bid, s).unwrap();
        ids_and_positions.push((bid, pos));
    }

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.run_frames(120, 1.0 / 60.0);

    for (id, expected_pos) in &ids_and_positions {
        let actual = vec2_prop(ml.tree(), *id, "position");
        assert_eq!(
            actual, *expected_pos,
            "StaticBody2D should not move: expected {expected_pos:?}, got {actual:?}"
        );
    }
}

// ===========================================================================
// 18. CharacterBody2D velocity reduced on collision axis after floor landing
// ===========================================================================

#[test]
fn character_velocity_reduced_on_floor_collision() {
    let (tree, player_id, _) = make_falling_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let velocity = vec2_prop(ml.tree(), player_id, "velocity");

    // After landing on floor, downward velocity component should be near zero
    // (the collision removes the component along the floor normal).
    assert!(
        velocity.y.abs() < 50.0,
        "downward velocity should be reduced after floor collision: vy={}",
        velocity.y
    );
}

// ===========================================================================
// 19. Fixture-loaded: initial positions match tscn values
// ===========================================================================

#[test]
fn fixture_tscn_initial_positions_correct() {
    let tree = load_character_body_tscn();

    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let platform_id = tree.get_node_by_path("/root/World/Platform").unwrap();
    let wall_id = tree.get_node_by_path("/root/World/Wall").unwrap();

    let player_pos = vec2_prop(&tree, player_id, "position");
    let platform_pos = vec2_prop(&tree, platform_id, "position");
    let wall_pos = vec2_prop(&tree, wall_id, "position");

    assert!(
        approx_eq(player_pos.x, 100.0) && approx_eq(player_pos.y, 100.0),
        "Player position should be (100, 100): {player_pos:?}"
    );
    assert!(
        approx_eq(platform_pos.x, 100.0) && approx_eq(platform_pos.y, 200.0),
        "Platform position should be (100, 200): {platform_pos:?}"
    );
    assert!(
        approx_eq(wall_pos.x, 300.0) && approx_eq(wall_pos.y, 150.0),
        "Wall position should be (300, 150): {wall_pos:?}"
    );
}

// ===========================================================================
// 20. CharacterBody2D deterministic: same setup produces same result
// ===========================================================================

#[test]
fn character_movement_deterministic_across_runs() {
    fn run_once() -> Vector2 {
        let (tree, player_id, _) = make_falling_character_scene();
        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);
        vec2_prop(ml.tree(), player_id, "position")
    }

    let pos1 = run_once();
    let pos2 = run_once();

    assert_eq!(
        pos1, pos2,
        "same setup should produce identical results: run1={pos1:?}, run2={pos2:?}"
    );
}
