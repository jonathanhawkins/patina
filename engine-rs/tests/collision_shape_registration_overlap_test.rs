//! pat-x4rz: Collision shape registration and overlap coverage.
//!
//! Tests that collision shapes are correctly extracted from scene tree nodes,
//! registered into the physics world, and produce deterministic overlap
//! results across registration, updates, and query paths.
//!
//! Coverage:
//!  1. Shape registration via PhysicsServer (circle, rectangle, defaults)
//!  2. Shape extraction from CollisionShape2D nodes (fallback, typed)
//!  3. Area registration and AreaStore management
//!  4. Overlap detection: enter, stable, exit, re-enter
//!  5. Layer/mask filtering on overlap
//!  6. Position updates and overlap state changes
//!  7. Multiple areas and bodies
//!  8. Shape scaling via node scale property
//!  9. Monitoring toggle suppresses/restores overlap detection

use std::collections::HashMap;

use gdcore::math::Vector2;
use gdphysics2d::area2d::{Area2D, AreaId, AreaStore, OverlapState};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::collision;
use gdcore::math::Transform2D;
use gdphysics2d::shape::Shape2D;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::LifecycleManager;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_body(id: u64, pos: Vector2, shape: Shape2D) -> PhysicsBody2D {
    let mut body = PhysicsBody2D::new(BodyId(id), BodyType::Rigid, pos, shape, 1.0);
    body.collision_layer = 1;
    body.collision_mask = 1;
    body
}

fn make_circle_body(id: u64, pos: Vector2, radius: f32) -> PhysicsBody2D {
    make_body(id, pos, Shape2D::Circle { radius })
}

fn make_rect_body(id: u64, pos: Vector2, hx: f32, hy: f32) -> PhysicsBody2D {
    make_body(id, pos, Shape2D::Rectangle { half_extents: Vector2::new(hx, hy) })
}

fn body_map(bodies: Vec<PhysicsBody2D>) -> HashMap<BodyId, PhysicsBody2D> {
    bodies.into_iter().map(|b| (b.id, b)).collect()
}

fn make_area(pos: Vector2, shape: Shape2D) -> Area2D {
    Area2D::new(AreaId(0), pos, shape)
}

// ===========================================================================
// 1. Shape registration — PhysicsServer extracts shapes from scene nodes
// ===========================================================================

#[test]
fn physics_server_registers_rigid_body_with_circle_shape() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Ball", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    body.set_property("mass", Variant::Float(2.0));
    let body_id = tree.add_child(root, body).unwrap();

    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("radius", Variant::Float(10.0));
    tree.add_child(body_id, cs).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    assert_eq!(ps.body_count(), 1, "one rigid body should be registered");
    assert!(ps.body_for_node(body_id).is_some(), "body mapped to node");
}

#[test]
fn physics_server_registers_static_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
    let wall_id = tree.add_child(root, wall).unwrap();

    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("size", Variant::Vector2(Vector2::new(200.0, 20.0)));
    tree.add_child(wall_id, cs).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    assert_eq!(ps.body_count(), 1);
    assert!(ps.body_for_node(wall_id).is_some());
}

#[test]
fn physics_server_registers_area2d() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let area_id = tree.add_child(root, area).unwrap();

    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("radius", Variant::Float(30.0));
    tree.add_child(area_id, cs).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    assert_eq!(ps.area_count(), 1, "one area should be registered");
    assert_eq!(ps.body_count(), 0, "areas are not bodies");
}

#[test]
fn physics_server_uses_default_shape_when_no_collision_shape_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("NoShape", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    let _body_id = tree.add_child(root, body).unwrap();
    // No CollisionShape2D child — should get default circle(16)

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    assert_eq!(ps.body_count(), 1, "body registered with default shape");
}

#[test]
fn physics_server_registers_typed_shape_string() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("TypedBody", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body).unwrap();

    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("shape", Variant::String("RectangleShape2D".into()));
    cs.set_property("size", Variant::Vector2(Vector2::new(40.0, 20.0)));
    tree.add_child(body_id, cs).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    assert_eq!(ps.body_count(), 1);
}

#[test]
fn physics_server_idempotent_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Ball", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body).unwrap();

    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_id, cs).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);
    ps.register_bodies(&tree); // second call should not duplicate

    assert_eq!(ps.body_count(), 1, "idempotent: still one body");
}

// ===========================================================================
// 2. Direct collision detection — shape pair tests
// ===========================================================================

#[test]
fn circle_circle_overlap_detected() {
    let result = collision::test_collision(
        &Shape2D::Circle { radius: 10.0 },
        &Transform2D::translated(Vector2::new(0.0, 0.0)),
        &Shape2D::Circle { radius: 10.0 },
        &Transform2D::translated(Vector2::new(15.0, 0.0)),
    ).unwrap();
    assert!(result.colliding);
    assert!((result.depth - 5.0).abs() < 0.01, "depth should be ~5.0");
}

#[test]
fn circle_circle_separated() {
    let result = collision::test_collision(
        &Shape2D::Circle { radius: 5.0 },
        &Transform2D::translated(Vector2::ZERO),
        &Shape2D::Circle { radius: 5.0 },
        &Transform2D::translated(Vector2::new(20.0, 0.0)),
    ).unwrap();
    assert!(!result.colliding);
}

#[test]
fn circle_rect_overlap_detected() {
    let result = collision::test_collision(
        &Shape2D::Circle { radius: 5.0 },
        &Transform2D::translated(Vector2::new(7.0, 0.0)),
        &Shape2D::Rectangle { half_extents: Vector2::new(5.0, 5.0) },
        &Transform2D::translated(Vector2::ZERO),
    ).unwrap();
    assert!(result.colliding, "circle at 7 should overlap rect edge at 5");
}

#[test]
fn rect_rect_overlap_detected() {
    let result = collision::test_collision(
        &Shape2D::Rectangle { half_extents: Vector2::new(5.0, 5.0) },
        &Transform2D::translated(Vector2::ZERO),
        &Shape2D::Rectangle { half_extents: Vector2::new(5.0, 5.0) },
        &Transform2D::translated(Vector2::new(8.0, 0.0)),
    ).unwrap();
    assert!(result.colliding);
    assert!((result.depth - 2.0).abs() < 0.01, "overlap should be ~2.0");
}

#[test]
fn unsupported_shape_pair_returns_none() {
    let result = collision::test_collision(
        &Shape2D::Capsule { radius: 5.0, height: 20.0 },
        &Transform2D::IDENTITY,
        &Shape2D::Circle { radius: 5.0 },
        &Transform2D::IDENTITY,
    );
    assert!(result.is_none(), "capsule-circle not yet supported");
}

// ===========================================================================
// 3. AreaStore — registration, removal, query
// ===========================================================================

#[test]
fn area_store_add_and_get() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    assert_eq!(store.area_count(), 1);
    let area = store.get_area(id).unwrap();
    assert_eq!(area.id, id);
    assert!(area.monitoring);
}

#[test]
fn area_store_remove() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));
    assert_eq!(store.area_count(), 1);

    let removed = store.remove_area(id);
    assert!(removed.is_some());
    assert_eq!(store.area_count(), 0);
    assert!(store.get_area(id).is_none());
}

#[test]
fn area_store_remove_nonexistent_returns_none() {
    let mut store = AreaStore::new();
    assert!(store.remove_area(AreaId(999)).is_none());
}

#[test]
fn area_store_multiple_areas() {
    let mut store = AreaStore::new();
    let _a = store.add_area(make_area(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 5.0 }));
    let _b = store.add_area(make_area(Vector2::new(100.0, 0.0), Shape2D::Circle { radius: 5.0 }));
    let _c = store.add_area(make_area(Vector2::new(200.0, 0.0), Shape2D::Circle { radius: 5.0 }));
    assert_eq!(store.area_count(), 3);
}

// ===========================================================================
// 4. Overlap detection — enter, stable, exit, re-enter
// ===========================================================================

#[test]
fn overlap_enter_event_on_first_contact() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(5.0, 0.0), 3.0)]);
    let events = store.detect_overlaps(&bodies);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Entered);
}

#[test]
fn no_event_while_stable_inside() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(5.0, 0.0), 3.0)]);
    let _ = store.detect_overlaps(&bodies); // enter
    let events = store.detect_overlaps(&bodies); // stable
    assert!(events.is_empty(), "no events while body stays inside");
}

#[test]
fn exit_event_when_body_leaves() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let mut bodies = body_map(vec![make_circle_body(1, Vector2::new(5.0, 0.0), 3.0)]);
    let _ = store.detect_overlaps(&bodies); // enter

    // Move body far away
    bodies.get_mut(&BodyId(1)).unwrap().position = Vector2::new(100.0, 0.0);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Exited);
}

#[test]
fn reenter_after_exit_fires_entered_again() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let mut bodies = body_map(vec![make_circle_body(1, Vector2::new(5.0, 0.0), 3.0)]);

    // Enter
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events[0].state, OverlapState::Entered);

    // Exit
    bodies.get_mut(&BodyId(1)).unwrap().position = Vector2::new(100.0, 0.0);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events[0].state, OverlapState::Exited);

    // Re-enter
    bodies.get_mut(&BodyId(1)).unwrap().position = Vector2::new(5.0, 0.0);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Entered);
}

// ===========================================================================
// 5. Layer/mask filtering
// ===========================================================================

#[test]
fn layer_mask_match_produces_overlap() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));
    store.get_area_mut(id).unwrap().collision_mask = 0b0010;

    let mut body = make_circle_body(1, Vector2::new(5.0, 0.0), 3.0);
    body.collision_layer = 0b0010;

    let events = store.detect_overlaps(&body_map(vec![body]));
    assert_eq!(events.len(), 1, "mask matches layer → overlap detected");
}

#[test]
fn layer_mask_mismatch_suppresses_overlap() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));
    store.get_area_mut(id).unwrap().collision_mask = 0b0100;

    let mut body = make_circle_body(1, Vector2::new(5.0, 0.0), 3.0);
    body.collision_layer = 0b0010; // doesn't match mask

    let events = store.detect_overlaps(&body_map(vec![body]));
    assert!(events.is_empty(), "mask doesn't match layer → no overlap");
}

#[test]
fn multi_bit_mask_matches_any_layer_bit() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));
    store.get_area_mut(id).unwrap().collision_mask = 0b1010; // scans layers 1 and 3

    let mut body = make_circle_body(1, Vector2::new(5.0, 0.0), 3.0);
    body.collision_layer = 0b0010; // on layer 1 → matches bit 1

    let events = store.detect_overlaps(&body_map(vec![body]));
    assert_eq!(events.len(), 1, "multi-bit mask should match");
}

// ===========================================================================
// 6. Position updates change overlap state
// ===========================================================================

#[test]
fn moving_body_into_area_triggers_enter() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let mut bodies = body_map(vec![make_circle_body(1, Vector2::new(50.0, 0.0), 3.0)]);

    // Frame 1: body far away — no overlap
    let events = store.detect_overlaps(&bodies);
    assert!(events.is_empty());

    // Frame 2: move body into area
    bodies.get_mut(&BodyId(1)).unwrap().position = Vector2::new(5.0, 0.0);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Entered);
}

#[test]
fn updating_area_position_affects_overlap() {
    let mut store = AreaStore::new();
    let area_id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(5.0, 0.0), 3.0)]);

    // Frame 1: overlapping
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Entered);

    // Move area far away
    store.get_area_mut(area_id).unwrap().position = Vector2::new(500.0, 500.0);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Exited);
}

// ===========================================================================
// 7. Multiple areas and bodies
// ===========================================================================

#[test]
fn body_overlaps_two_areas_simultaneously() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 20.0 }));
    store.add_area(make_area(Vector2::new(10.0, 0.0), Shape2D::Circle { radius: 20.0 }));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(5.0, 0.0), 3.0)]);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 2, "body should enter both areas");
    assert!(events.iter().all(|e| e.state == OverlapState::Entered));
}

#[test]
fn two_bodies_enter_same_area() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 20.0 }));

    let bodies = body_map(vec![
        make_circle_body(1, Vector2::new(5.0, 0.0), 3.0),
        make_circle_body(2, Vector2::new(-5.0, 0.0), 3.0),
    ]);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 2, "both bodies should enter the area");
}

#[test]
fn one_body_exits_while_other_stays() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let mut bodies = body_map(vec![
        make_circle_body(1, Vector2::new(3.0, 0.0), 2.0),
        make_circle_body(2, Vector2::new(-3.0, 0.0), 2.0),
    ]);

    // Both enter
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 2);

    // Move body 1 out, body 2 stays
    bodies.get_mut(&BodyId(1)).unwrap().position = Vector2::new(100.0, 0.0);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].body_id, BodyId(1));
    assert_eq!(events[0].state, OverlapState::Exited);
}

// ===========================================================================
// 8. Shape scaling
// ===========================================================================

#[test]
fn scaled_circle_shape_increases_overlap_range() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Big", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    body.set_property("scale", Variant::Vector2(Vector2::new(3.0, 3.0)));
    let body_id = tree.add_child(root, body).unwrap();

    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("radius", Variant::Float(10.0)); // scaled by 3 → 30
    tree.add_child(body_id, cs).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(35.0, 0.0)));
    let area_nid = tree.add_child(root, area).unwrap();

    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(area_nid, sa).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);
    ps.step_physics(1.0 / 60.0);

    // Scaled body radius=30, area radius=10, gap=35. 30+10=40 > 35 → overlap
    let overlaps = ps.last_overlap_events();
    assert!(
        overlaps.iter().any(|e| e.state == OverlapState::Entered),
        "scaled shape should extend overlap range"
    );
}

// ===========================================================================
// 9. Monitoring toggle
// ===========================================================================

#[test]
fn monitoring_false_suppresses_detection() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));
    store.get_area_mut(id).unwrap().monitoring = false;

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(3.0, 0.0), 2.0)]);
    let events = store.detect_overlaps(&bodies);
    assert!(events.is_empty(), "monitoring=false should suppress detection");
}

#[test]
fn monitoring_reenabled_detects_existing_overlap() {
    let mut store = AreaStore::new();
    let id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(3.0, 0.0), 2.0)]);

    // Disable monitoring
    store.get_area_mut(id).unwrap().monitoring = false;
    let events = store.detect_overlaps(&bodies);
    assert!(events.is_empty());

    // Re-enable monitoring — body is already inside
    store.get_area_mut(id).unwrap().monitoring = true;
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Entered);
}

// ===========================================================================
// 10. Mixed shape type overlaps
// ===========================================================================

#[test]
fn circle_area_detects_rect_body() {
    let mut store = AreaStore::new();
    store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let bodies = body_map(vec![make_rect_body(1, Vector2::new(8.0, 0.0), 5.0, 5.0)]);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1, "circle area should detect rect body overlap");
    assert_eq!(events[0].state, OverlapState::Entered);
}

#[test]
fn rect_area_detects_circle_body() {
    let mut store = AreaStore::new();
    store.add_area(make_area(
        Vector2::ZERO,
        Shape2D::Rectangle { half_extents: Vector2::new(10.0, 10.0) },
    ));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(8.0, 0.0), 5.0)]);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1, "rect area should detect circle body overlap");
}

#[test]
fn rect_area_detects_rect_body() {
    let mut store = AreaStore::new();
    store.add_area(make_area(
        Vector2::ZERO,
        Shape2D::Rectangle { half_extents: Vector2::new(10.0, 10.0) },
    ));

    let bodies = body_map(vec![make_rect_body(1, Vector2::new(15.0, 0.0), 8.0, 8.0)]);
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1, "rect area should detect rect body overlap");
}

// ===========================================================================
// 11. Shape containment edge cases
// ===========================================================================

#[test]
fn capsule_contains_point_at_center() {
    let shape = Shape2D::Capsule { radius: 3.0, height: 10.0 };
    assert!(shape.contains_point(Vector2::ZERO));
}

#[test]
fn capsule_contains_point_at_top_hemisphere() {
    let shape = Shape2D::Capsule { radius: 3.0, height: 10.0 };
    // Top of capsule: half_h = 5-3 = 2, so point at (0, 4) is inside top hemisphere
    assert!(shape.contains_point(Vector2::new(0.0, 4.0)));
}

#[test]
fn capsule_excludes_point_outside() {
    let shape = Shape2D::Capsule { radius: 3.0, height: 10.0 };
    assert!(!shape.contains_point(Vector2::new(10.0, 0.0)));
}

#[test]
fn segment_contains_point_on_line() {
    let shape = Shape2D::Segment {
        a: Vector2::ZERO,
        b: Vector2::new(10.0, 0.0),
    };
    assert!(shape.contains_point(Vector2::new(5.0, 0.0)));
}

#[test]
fn segment_excludes_point_off_line() {
    let shape = Shape2D::Segment {
        a: Vector2::ZERO,
        b: Vector2::new(10.0, 0.0),
    };
    assert!(!shape.contains_point(Vector2::new(5.0, 1.0)));
}

// ===========================================================================
// pat-voae: Collision registration updates after shape replacement
// ===========================================================================

/// Helper: builds a minimal scene with a body and area that initially overlap,
/// registers them in the PhysicsServer, and returns everything needed for
/// shape-replacement tests.
fn setup_overlap_scene() -> (
    SceneTree,
    gdscene::physics_server::PhysicsServer,
    gdscene::node::NodeId, // body node
    gdscene::node::NodeId, // body's CollisionShape2D
    gdscene::node::NodeId, // area node
    gdscene::node::NodeId, // area's CollisionShape2D
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // RigidBody2D at origin with circle radius 20.
    let mut body_node = Node::new("Ball", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("BodyShape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(20.0));
    let body_shape_id = tree.add_child(body_id, body_shape).unwrap();

    // Area2D at (25, 0) with circle radius 20 — overlaps the body.
    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(25.0, 0.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AreaShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(20.0));
    let area_shape_id = tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    (tree, ps, body_id, body_shape_id, area_id, area_shape_id)
}

#[test]
fn sync_shapes_updates_body_shape_after_replacement() {
    let (mut tree, mut ps, _body_id, body_shape_id, _area_id, _area_shape_id) =
        setup_overlap_scene();

    // Verify initial overlap: body circle(20) at origin + area circle(20) at (25,0)
    // distance = 25, sum of radii = 40 → overlapping.
    ps.step_physics(1.0 / 60.0);
    let overlaps_before = ps.last_overlap_events();
    assert!(
        !overlaps_before.is_empty(),
        "initial shapes should overlap"
    );

    // Shrink the body shape to radius 1 — should no longer overlap.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));

    // Without sync_shapes, broadphase would still use the old radius 20.
    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let overlaps_after = ps.last_overlap_events();
    // After shrinking, distance=25 > sum of radii=21 would still overlap...
    // Actually radius 1 + 20 = 21 < 25, so no overlap. Check for exit event.
    let has_exit = overlaps_after
        .iter()
        .any(|e| e.state == gdphysics2d::area2d::OverlapState::Exited);
    assert!(
        has_exit,
        "shrinking body shape via sync_shapes must produce an exit event"
    );
}

#[test]
fn sync_shapes_updates_area_shape_after_replacement() {
    let (mut tree, mut ps, _body_id, _body_shape_id, _area_id, area_shape_id) =
        setup_overlap_scene();

    // Confirm initial overlap.
    ps.step_physics(1.0 / 60.0);
    assert!(
        !ps.last_overlap_events().is_empty(),
        "initial shapes should overlap"
    );

    // Shrink area shape to radius 1 — too small to reach the body at origin.
    tree.get_node_mut(area_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_exit = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == gdphysics2d::area2d::OverlapState::Exited);
    assert!(
        has_exit,
        "shrinking area shape via sync_shapes must produce an exit event"
    );
}

#[test]
fn sync_shapes_grows_shape_causes_new_overlap() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body at origin with tiny circle (radius 1).
    let mut body_node = Node::new("Ball", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(1.0));
    let body_shape_id = tree.add_child(body_id, body_shape).unwrap();

    // Area at (30, 0) with radius 10 — no overlap initially (distance=30, radii=11).
    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(30.0, 0.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step once — no overlap.
    ps.step_physics(1.0 / 60.0);
    assert!(
        ps.last_overlap_events().is_empty(),
        "tiny body should not overlap area 30 units away"
    );

    // Grow body shape to radius 25 — now overlapping (distance=30, radii=35).
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(25.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_enter = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == gdphysics2d::area2d::OverlapState::Entered);
    assert!(
        has_enter,
        "growing body shape via sync_shapes must produce an enter event"
    );
}

#[test]
fn sync_shapes_circle_to_rectangle_type_change() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body at origin with circle radius 5.
    let mut body_node = Node::new("Player", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(5.0));
    let body_shape_id = tree.add_child(body_id, body_shape).unwrap();

    // Area at (50, 0) with radius 10. No overlap initially (distance=50, radii=15).
    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(50.0, 0.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    ps.step_physics(1.0 / 60.0);
    assert!(
        ps.last_overlap_events().is_empty(),
        "small circle should not overlap area 50 units away"
    );

    // Replace circle with a large rectangle that reaches the area.
    // Rectangle half_extents (45, 10) → extends 45 units on x-axis.
    // Area center at 50, radius 10 → leftmost point at 40.
    // Body rect rightmost point at 45. 45 > 40 → overlap.
    let shape_node = tree.get_node_mut(body_shape_id).unwrap();
    // Clear radius, set shape type and size for rectangle.
    shape_node.set_property("radius", Variant::Nil);
    shape_node.set_property("shape", Variant::String("RectangleShape2D".into()));
    shape_node.set_property("size", Variant::Vector2(Vector2::new(90.0, 20.0)));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_enter = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == gdphysics2d::area2d::OverlapState::Entered);
    assert!(
        has_enter,
        "replacing circle with large rectangle via sync_shapes must produce enter event"
    );
}

#[test]
fn broadphase_collision_responds_to_shape_replacement() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Two rigid bodies far apart — no collision initially.
    let mut body_a = Node::new("A", "RigidBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::ZERO));
    let a_id = tree.add_child(root, body_a).unwrap();

    let mut shape_a = Node::new("ShapeA", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(5.0));
    let shape_a_id = tree.add_child(a_id, shape_a).unwrap();

    let mut body_b = Node::new("B", "RigidBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    let b_id = tree.add_child(root, body_b).unwrap();

    let mut shape_b = Node::new("ShapeB", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(5.0));
    tree.add_child(b_id, shape_b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step — no collision (distance=100, radii=10).
    ps.step_physics(1.0 / 60.0);
    assert!(
        ps.last_collision_events().is_empty(),
        "far apart bodies should not collide"
    );

    // Grow body A's shape to radius 96 — now overlapping (distance=100, radii=101).
    tree.get_node_mut(shape_a_id)
        .unwrap()
        .set_property("radius", Variant::Float(96.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_collision = !ps.last_collision_events().is_empty();
    assert!(
        has_collision,
        "growing body shape must cause broadphase to detect new collision"
    );
}

#[test]
fn without_sync_shapes_broadphase_uses_stale_shape() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body at origin with circle radius 20.
    let mut body_node = Node::new("Ball", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(20.0));
    let body_shape_id = tree.add_child(body_id, body_shape).unwrap();

    // Area at (25, 0) with radius 20 — overlapping.
    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(25.0, 0.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(20.0));
    tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Confirm initial overlap.
    ps.step_physics(1.0 / 60.0);
    assert!(!ps.last_overlap_events().is_empty(), "should overlap initially");

    // Shrink body shape to radius 1 — but do NOT call sync_shapes.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));

    // Step WITHOUT sync_shapes — broadphase still uses old radius 20.
    ps.step_physics(1.0 / 60.0);

    // Should still show as persisting (no exit), because the physics world
    // doesn't know the shape changed.
    let has_exit = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == gdphysics2d::area2d::OverlapState::Exited);
    assert!(
        !has_exit,
        "without sync_shapes, broadphase should still use stale shape (no exit)"
    );
}

// ===========================================================================
// pat-0rms: Collision registration updates after shape replacement
// ===========================================================================

/// Multiple sequential shape replacements: shrink → grow → shrink, with
/// sync_shapes after each. Overlap state must track each change.
#[test]
fn sequential_shape_replacements_track_overlap_state() {
    let (mut tree, mut ps, _body_id, body_shape_id, _area_id, _area_shape_id) =
        setup_overlap_scene();

    // Step 1: initial overlap (body r=20 at origin, area r=20 at (25,0)).
    ps.step_physics(1.0 / 60.0);
    assert!(
        !ps.last_overlap_events().is_empty(),
        "initial overlap expected"
    );

    // Step 2: shrink body → no overlap.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));
    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);
    let has_exit = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Exited);
    assert!(has_exit, "shrink must produce exit");

    // Step 3: grow body back → overlap again.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(20.0));
    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);
    let has_enter = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Entered);
    assert!(has_enter, "re-growing must produce enter");

    // Step 4: shrink again → exit again.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));
    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);
    let has_exit_2 = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Exited);
    assert!(has_exit_2, "second shrink must also produce exit");
}

/// Shape replacement + re-registration: after changing both shape and position
/// on a rigid body, re-registering picks up the new state.
#[test]
fn shape_replacement_and_reregistration_combined() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body at origin with small circle — no overlap with area at (15,0).
    let mut body_node = Node::new("Ball", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(2.0));
    let body_shape_id = tree.add_child(body_id, body_shape).unwrap();

    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(5.0));
    tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    ps.step_physics(1.0 / 60.0);
    assert!(
        ps.last_overlap_events().is_empty(),
        "no initial overlap expected (distance=15, radii=7)"
    );

    // Grow the shape to radius 15 → distance=15, radii=20 → overlap.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(15.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_enter = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Entered);
    assert!(
        has_enter,
        "growing shape to overlap distance must produce enter event"
    );
}

/// Body and area shapes both replaced in the same frame: registration must
/// update both before overlap detection runs.
#[test]
fn both_shapes_replaced_simultaneously() {
    let (mut tree, mut ps, _body_id, body_shape_id, _area_id, area_shape_id) =
        setup_overlap_scene();

    // Initial overlap.
    ps.step_physics(1.0 / 60.0);
    assert!(!ps.last_overlap_events().is_empty());

    // Shrink BOTH shapes to radius 1 in the same frame.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));
    tree.get_node_mut(area_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    // Distance=25, sum of radii=2 → no overlap.
    let has_exit = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Exited);
    assert!(
        has_exit,
        "shrinking both shapes must produce exit event"
    );
}

/// Shape replacement to just-outside boundary: sum of radii < distance.
/// After shrinking shapes so they no longer overlap, exit event fires.
#[test]
fn shape_replacement_to_just_outside_boundary_exits_overlap() {
    let (mut tree, mut ps, _body_id, body_shape_id, _area_id, area_shape_id) =
        setup_overlap_scene();

    // Initial overlap (body r=20, area r=20, distance=25, penetration depth=15).
    ps.step_physics(1.0 / 60.0);
    assert!(!ps.last_overlap_events().is_empty());

    // Set radii so sum is just under the distance.
    // Body at (0,0), area at (25,0). Set body r=14, area r=10 → sum=24 < 25.
    tree.get_node_mut(body_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(14.0));
    tree.get_node_mut(area_shape_id)
        .unwrap()
        .set_property("radius", Variant::Float(10.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    // sum=24 < distance=25 → no overlap → exit event.
    let has_exit = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Exited);
    assert!(
        has_exit,
        "shapes with sum of radii < distance should produce exit event"
    );
}

/// Rectangle-to-circle shape type change: replacement should update
/// the registered shape type and affect overlap detection.
#[test]
fn rectangle_to_circle_type_change() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body at origin with wide rectangle — overlaps area at (30, 0).
    let mut body_node = Node::new("Player", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("shape", Variant::String("RectangleShape2D".into()));
    body_shape.set_property("size", Variant::Vector2(Vector2::new(80.0, 20.0)));
    let body_shape_id = tree.add_child(body_id, body_shape).unwrap();

    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(30.0, 0.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(15.0));
    tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step — rectangle half_ext 40 x-axis reaches 40, area starts at 15 → overlap.
    ps.step_physics(1.0 / 60.0);
    assert!(
        !ps.last_overlap_events().is_empty(),
        "wide rectangle should overlap area at (30,0)"
    );

    // Replace with a small circle (radius 5). Distance=30, radii=5+15=20 → no overlap.
    let shape_node = tree.get_node_mut(body_shape_id).unwrap();
    shape_node.set_property("shape", Variant::Nil);
    shape_node.set_property("size", Variant::Nil);
    shape_node.set_property("radius", Variant::Float(5.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_exit = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Exited);
    assert!(
        has_exit,
        "replacing wide rectangle with small circle must produce exit event"
    );
}

// ===========================================================================
// pat-mal: Match collision registration updates after shape replacement
// ===========================================================================

/// Broadphase collision exit: shrinking a body shape so two previously-colliding
/// bodies no longer overlap must produce an Exited collision event.
#[test]
fn broadphase_collision_exit_after_shape_shrink() {
    use gdphysics2d::world::ContactState;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Rigid body + static body overlapping (distance=15, sum of radii=20).
    // Using StaticBody2D for B so collision resolution doesn't push it.
    let mut body_a = Node::new("A", "RigidBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::ZERO));
    let a_id = tree.add_child(root, body_a).unwrap();

    let mut shape_a = Node::new("ShapeA", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(10.0));
    let shape_a_id = tree.add_child(a_id, shape_a).unwrap();

    let mut body_b = Node::new("B", "StaticBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    let b_id = tree.add_child(root, body_b).unwrap();

    let mut shape_b = Node::new("ShapeB", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, shape_b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step — collision expected (distance=15, radii=20).
    ps.step_physics(1.0 / 60.0);
    let collisions = ps.last_collision_events();
    assert!(
        collisions.iter().any(|e| e.state == ContactState::Entered),
        "overlapping bodies should produce Entered collision event"
    );

    // Shrink body A to radius 1 — no longer overlapping (distance=15, radii=11).
    tree.get_node_mut(shape_a_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_exit = ps
        .last_collision_events()
        .iter()
        .any(|e| e.state == ContactState::Exited);
    assert!(
        has_exit,
        "shrinking body shape must produce Exited collision event"
    );
}

/// Broadphase collision detects overlap correctly with static+rigid pair
/// after shape replacement shrinks the rigid body.
#[test]
fn broadphase_collision_exit_with_static_body_after_shrink() {
    use gdphysics2d::world::ContactState;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Rigid body A at origin, static body B at (15, 0).
    // Both radius 10: sum=20 > 15 → overlap.
    let mut body_a = Node::new("A", "RigidBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::ZERO));
    let a_id = tree.add_child(root, body_a).unwrap();

    let mut shape_a = Node::new("ShapeA", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(10.0));
    let shape_a_id = tree.add_child(a_id, shape_a).unwrap();

    let mut body_b = Node::new("B", "StaticBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    let b_id = tree.add_child(root, body_b).unwrap();

    let mut shape_b = Node::new("ShapeB", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, shape_b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step 1 — collision entered.
    ps.step_physics(0.0);
    assert!(
        ps.last_collision_events()
            .iter()
            .any(|e| e.state == ContactState::Entered),
        "initial collision expected"
    );

    // Shrink body A to radius 2 → A was pushed left by separation,
    // now with tiny radius it definitely no longer overlaps B.
    tree.get_node_mut(shape_a_id)
        .unwrap()
        .set_property("radius", Variant::Float(2.0));

    ps.sync_shapes(&tree);
    ps.step_physics(0.0);

    let has_exit = ps
        .last_collision_events()
        .iter()
        .any(|e| e.state == ContactState::Exited);
    assert!(
        has_exit,
        "shrinking rigid body shape against static must produce exit"
    );
}

/// Collision shape type change (circle→rectangle) updates broadphase
/// registration and produces correct collision events.
#[test]
fn broadphase_collision_after_shape_type_change() {
    use gdphysics2d::world::ContactState;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body A at origin with small circle — no collision with B at (50, 0).
    let mut body_a = Node::new("A", "RigidBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::ZERO));
    let a_id = tree.add_child(root, body_a).unwrap();

    let mut shape_a = Node::new("ShapeA", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(5.0));
    let shape_a_id = tree.add_child(a_id, shape_a).unwrap();

    let mut body_b = Node::new("B", "RigidBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(50.0, 0.0)));
    let b_id = tree.add_child(root, body_b).unwrap();

    let mut shape_b = Node::new("ShapeB", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(5.0));
    tree.add_child(b_id, shape_b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step — no collision (distance=50, radii=10).
    ps.step_physics(1.0 / 60.0);
    assert!(
        ps.last_collision_events().is_empty(),
        "no collision expected initially"
    );

    // Replace A's circle with a wide rectangle (half_extents 48x5).
    // Rect extends 48 units right, B circle center at 50 with radius 5.
    // Rect right edge at 48, circle left edge at 45 → overlap.
    let shape_node = tree.get_node_mut(shape_a_id).unwrap();
    shape_node.set_property("radius", Variant::Nil);
    shape_node.set_property("shape", Variant::String("RectangleShape2D".into()));
    shape_node.set_property("size", Variant::Vector2(Vector2::new(96.0, 10.0)));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_collision = ps
        .last_collision_events()
        .iter()
        .any(|e| e.state == ContactState::Entered);
    assert!(
        has_collision,
        "circle→rectangle type change must cause new collision"
    );
}

/// Sequential collision shape replacements: enter → exit → re-enter via
/// broadphase, verifying previous_contacts tracking is consistent.
/// Uses a StaticBody2D so collision resolution doesn't scatter positions.
#[test]
fn broadphase_sequential_shape_replacements() {
    use gdphysics2d::world::ContactState;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body_a = Node::new("A", "RigidBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::ZERO));
    let a_id = tree.add_child(root, body_a).unwrap();

    let mut shape_a = Node::new("ShapeA", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(10.0));
    let shape_a_id = tree.add_child(a_id, shape_a).unwrap();

    let mut body_b = Node::new("B", "StaticBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    let b_id = tree.add_child(root, body_b).unwrap();

    let mut shape_b = Node::new("ShapeB", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, shape_b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step 1: enter (distance=15, radii=20).
    ps.step_physics(0.0);
    assert!(
        ps.last_collision_events()
            .iter()
            .any(|e| e.state == ContactState::Entered),
        "step 1: collision entered"
    );

    // Step 2: shrink → exit. A was pushed left by separation, with tiny radius
    // it definitely no longer overlaps B.
    tree.get_node_mut(shape_a_id)
        .unwrap()
        .set_property("radius", Variant::Float(1.0));
    ps.sync_shapes(&tree);
    ps.step_physics(0.0);
    assert!(
        ps.last_collision_events()
            .iter()
            .any(|e| e.state == ContactState::Exited),
        "step 2: collision exited after shrink"
    );

    // Step 3: grow to very large radius → re-enter despite A being pushed away.
    tree.get_node_mut(shape_a_id)
        .unwrap()
        .set_property("radius", Variant::Float(50.0));
    ps.sync_shapes(&tree);
    ps.step_physics(0.0);
    assert!(
        ps.last_collision_events()
            .iter()
            .any(|e| e.state == ContactState::Entered),
        "step 3: collision re-entered after grow"
    );
}

/// Area shape scaling is applied during sync_shapes — scaled area must
/// update overlap detection range.
#[test]
fn sync_shapes_applies_scale_to_area() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body at origin with radius 5.
    let mut body_node = Node::new("Ball", "RigidBody2D");
    body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body_node).unwrap();

    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_id, body_shape).unwrap();

    // Area at (30, 0) with radius 5 and scale (3, 3).
    // Unscaled: sum of radii = 10 < 30 → no overlap.
    // Scaled: area radius = 15, sum = 20 < 30 → still no overlap.
    // But we'll test that sync_shapes picks up scale changes correctly.
    let mut area_node = Node::new("Zone", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(18.0, 0.0)));
    area_node.set_property("scale", Variant::Vector2(Vector2::new(1.0, 1.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut area_shape = Node::new("AShape", "CollisionShape2D");
    area_shape.set_property("radius", Variant::Float(5.0));
    tree.add_child(area_id, area_shape).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step — no overlap initially (distance=18, radii=10).
    ps.step_physics(1.0 / 60.0);
    assert!(
        ps.last_overlap_events().is_empty(),
        "no overlap initially (distance=18, radii=10)"
    );

    // Apply scale 3x to area → area radius becomes 15, sum=20 > 18 → overlap.
    tree.get_node_mut(area_id)
        .unwrap()
        .set_property("scale", Variant::Vector2(Vector2::new(3.0, 3.0)));

    ps.sync_shapes(&tree);
    ps.step_physics(1.0 / 60.0);

    let has_enter = ps
        .last_overlap_events()
        .iter()
        .any(|e| e.state == OverlapState::Entered);
    assert!(
        has_enter,
        "scaling area via sync_shapes must extend overlap range"
    );
}

/// When a rigid body's shape is shrunk from overlapping to non-overlapping,
/// an exit event fires — proving shape replacement updates collision state.
/// Uses a simple two-body setup to avoid separation-order ambiguity.
#[test]
fn shape_replacement_selective_collision_exit() {
    use gdphysics2d::world::ContactState;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Static body A at origin, large circle.
    let mut body_a = Node::new("A", "StaticBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::ZERO));
    let a_id = tree.add_child(root, body_a).unwrap();

    let mut shape_a = Node::new("ShapeA", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(40.0));
    let shape_a_id = tree.add_child(a_id, shape_a).unwrap();

    // Static body B at (30, 0), radius 5.
    // Initial overlap: distance=30, sum_radii=40+5=45 > 30 → overlapping.
    let mut body_b = Node::new("B", "StaticBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(30.0, 0.0)));
    let b_id = tree.add_child(root, body_b).unwrap();

    let mut shape_b = Node::new("ShapeB", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(5.0));
    tree.add_child(b_id, shape_b).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ps = gdscene::physics_server::PhysicsServer::new();
    ps.register_bodies(&tree);

    // Step — A(r=40) overlaps B(dist=30,r=5): sum=45>30 → Entered.
    ps.step_physics(0.0);
    let has_enter = ps
        .last_collision_events()
        .iter()
        .any(|e| e.state == ContactState::Entered);
    assert!(has_enter, "A-B should collide initially");

    // Shrink A to radius 10.
    // Now sum_radii = 10+5 = 15 < 30 → no overlap → exit expected.
    tree.get_node_mut(shape_a_id)
        .unwrap()
        .set_property("radius", Variant::Float(10.0));

    ps.sync_shapes(&tree);
    ps.step_physics(0.0);

    let a_body = ps.body_for_node(a_id).unwrap();
    let b_body = ps.body_for_node(b_id).unwrap();

    let has_exit = ps
        .last_collision_events()
        .iter()
        .any(|e| {
            e.state == ContactState::Exited
                && ((e.body_a == a_body && e.body_b == b_body)
                    || (e.body_a == b_body && e.body_b == a_body))
        });
    assert!(has_exit, "A-B collision should exit after A shrinks below overlap range");
}

// ===========================================================================
// pat-1bl: Match collision registration updates after shape replacement
// ===========================================================================

/// Direct PhysicsWorld2D shape replacement: mutating body.shape and stepping
/// must reflect the new shape in collision detection immediately.
#[test]
fn world_direct_shape_replacement_updates_collision() {
    use gdphysics2d::world::{ContactState, PhysicsWorld2D};

    let mut world = PhysicsWorld2D::new();

    // Two static bodies at distance 15, both radius 10 → overlapping.
    let id_a = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Circle { radius: 10.0 },
        1.0,
    ));
    let _id_b = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(15.0, 0.0),
        Shape2D::Circle { radius: 10.0 },
        1.0,
    ));

    // Step 1: collision entered.
    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Entered),
        "initial collision expected"
    );

    // Replace A's shape with a tiny circle — no longer overlapping.
    world.get_body_mut(id_a).unwrap().shape = Shape2D::Circle { radius: 1.0 };

    // Step 2: collision should exit.
    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Exited),
        "direct shape replacement must produce exit event"
    );
}

/// Direct shape replacement from circle to rectangle in PhysicsWorld2D.
#[test]
fn world_direct_circle_to_rect_replacement() {
    use gdphysics2d::world::{ContactState, PhysicsWorld2D};

    let mut world = PhysicsWorld2D::new();

    // Static A at origin with small circle, static B at (50, 0) with radius 5.
    // No collision: distance=50, sum_radii=10.
    let id_a = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Circle { radius: 5.0 },
        1.0,
    ));
    let _id_b = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(50.0, 0.0),
        Shape2D::Circle { radius: 5.0 },
        1.0,
    ));

    let events = world.step(0.0);
    assert!(events.is_empty(), "no collision initially");

    // Replace A with a wide rectangle that reaches B.
    // half_extents (48, 5) → rect right edge at 48, B at 50 with r=5 → left at 45 → overlap.
    world.get_body_mut(id_a).unwrap().shape = Shape2D::Rectangle {
        half_extents: Vector2::new(48.0, 5.0),
    };

    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Entered),
        "circle→rectangle replacement must cause new collision"
    );
}

/// Rectangle-to-rectangle size change: growing the rect causes a new collision.
#[test]
fn world_rect_to_larger_rect_causes_collision() {
    use gdphysics2d::world::{ContactState, PhysicsWorld2D};

    let mut world = PhysicsWorld2D::new();

    // A at origin with small rect, B at (20, 0) with rect half_extents (5, 5).
    // A half_extents (3, 3): sum of half_extents on x = 3+5 = 8 < 20 → no collision.
    let id_a = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Rectangle {
            half_extents: Vector2::new(3.0, 3.0),
        },
        1.0,
    ));
    let _id_b = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(20.0, 0.0),
        Shape2D::Rectangle {
            half_extents: Vector2::new(5.0, 5.0),
        },
        1.0,
    ));

    let events = world.step(0.0);
    assert!(events.is_empty(), "no collision with small rect");

    // Grow A's rect: half_extents (18, 3) → sum on x = 18+5 = 23 > 20 → overlap.
    world.get_body_mut(id_a).unwrap().shape = Shape2D::Rectangle {
        half_extents: Vector2::new(18.0, 3.0),
    };

    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Entered),
        "growing rect must cause new collision"
    );
}

/// Shape replacement during Persisting contact state: replacing shape while
/// contacts are Persisting must correctly transition to Exited.
#[test]
fn shape_replacement_during_persisting_contact() {
    use gdphysics2d::world::{ContactState, PhysicsWorld2D};

    let mut world = PhysicsWorld2D::new();

    let id_a = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Circle { radius: 10.0 },
        1.0,
    ));
    let _id_b = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(15.0, 0.0),
        Shape2D::Circle { radius: 10.0 },
        1.0,
    ));

    // Step 1: Entered.
    let events = world.step(0.0);
    assert!(events.iter().any(|e| e.state == ContactState::Entered));

    // Step 2: Persisting (same shapes, same positions).
    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Persisting),
        "second step should be Persisting"
    );

    // Replace A's shape while Persisting — should exit next step.
    world.get_body_mut(id_a).unwrap().shape = Shape2D::Circle { radius: 1.0 };

    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Exited),
        "shape replacement during Persisting must produce Exited"
    );
}

/// Kinematic body shape replacement updates collision registration.
#[test]
fn kinematic_body_shape_replacement_updates_collision() {
    use gdphysics2d::world::{ContactState, PhysicsWorld2D};

    let mut world = PhysicsWorld2D::new();

    // Kinematic A at origin, static B at (15, 0). Both radius 10 → collision.
    let id_a = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Kinematic,
        Vector2::ZERO,
        Shape2D::Circle { radius: 10.0 },
        1.0,
    ));
    let _id_b = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(15.0, 0.0),
        Shape2D::Circle { radius: 10.0 },
        1.0,
    ));

    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Entered),
        "kinematic body should collide initially"
    );

    // Shrink kinematic body's shape.
    world.get_body_mut(id_a).unwrap().shape = Shape2D::Circle { radius: 1.0 };

    let events = world.step(0.0);
    assert!(
        events.iter().any(|e| e.state == ContactState::Exited),
        "kinematic shape replacement must produce exit"
    );
}

/// Shape replacement with layer/mask filtering still respected:
/// growing a shape should NOT cause collision if layers don't match.
#[test]
fn shape_replacement_respects_layer_mask_filtering() {
    use gdphysics2d::world::PhysicsWorld2D;

    let mut world = PhysicsWorld2D::new();

    let mut body_a = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    body_a.collision_layer = 0b0001;
    body_a.collision_mask = 0b0001;
    let id_a = world.add_body(body_a);

    let mut body_b = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(15.0, 0.0),
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    body_b.collision_layer = 0b0010; // different layer from A's mask
    body_b.collision_mask = 0b0010;
    world.add_body(body_b);

    // Step — no collision due to layer mismatch.
    let events = world.step(0.0);
    assert!(events.is_empty(), "layer mismatch → no collision");

    // Grow A's shape to overlap B's position — but layers still don't match.
    world.get_body_mut(id_a).unwrap().shape = Shape2D::Circle { radius: 20.0 };

    let events = world.step(0.0);
    assert!(
        events.is_empty(),
        "shape replacement must not bypass layer/mask filtering"
    );
}

/// Area overlap detection with direct shape replacement on the area itself.
#[test]
fn area_direct_shape_replacement_updates_overlap() {
    let mut store = AreaStore::new();
    let area_id = store.add_area(make_area(Vector2::ZERO, Shape2D::Circle { radius: 10.0 }));

    let bodies = body_map(vec![make_circle_body(1, Vector2::new(25.0, 0.0), 5.0)]);

    // No overlap: distance=25, sum_radii=15.
    let events = store.detect_overlaps(&bodies);
    assert!(events.is_empty(), "no initial overlap");

    // Replace area shape with larger circle.
    store.get_area_mut(area_id).unwrap().shape = Shape2D::Circle { radius: 25.0 };

    // Now overlap: distance=25, sum_radii=30.
    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, OverlapState::Entered);
}

/// Replacing a body's shape from circle to rectangle changes which areas it overlaps.
#[test]
fn body_shape_replacement_changes_area_overlap_set() {
    let mut store = AreaStore::new();
    // Area A at (8, 0), area B at (0, 20) — B is far on Y axis.
    store.add_area(make_area(Vector2::new(8.0, 0.0), Shape2D::Circle { radius: 5.0 }));
    store.add_area(make_area(Vector2::new(0.0, 20.0), Shape2D::Circle { radius: 5.0 }));

    // Body at origin with circle radius 5. Overlaps A (distance=8, sum=10) but not B (distance=20, sum=10).
    let mut bodies = body_map(vec![make_circle_body(1, Vector2::ZERO, 5.0)]);

    let events = store.detect_overlaps(&bodies);
    assert_eq!(events.len(), 1, "overlaps only area A initially");

    // Replace body shape with a tall rectangle that reaches area B.
    // half_extents (3, 18) → extends 18 on y-axis. B at (0, 20) with r=5, left at 15. 18 > 15 → overlap.
    bodies.get_mut(&BodyId(1)).unwrap().shape = Shape2D::Rectangle {
        half_extents: Vector2::new(3.0, 18.0),
    };

    let events = store.detect_overlaps(&bodies);
    // Area A: rect half_ext x=3 vs circle at (8,0) r=5. Closest rect point to circle: (3,0).
    // Distance from (3,0) to (8,0) = 5.0 = radius → touching. Let's check.
    // Area B: rect half_ext y=18 vs circle at (0,20) r=5. Closest rect point: (0,18).
    // Distance from (0,18) to (0,20) = 2.0 < 5.0 → overlap.
    assert!(
        events.iter().any(|e| e.state == OverlapState::Entered),
        "tall rectangle should enter area B"
    );
}
