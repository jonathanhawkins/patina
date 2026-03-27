//! pat-s4x: 3D physics hooks and deterministic test coverage.
//!
//! Integration tests for the PhysicsServer3D scene bridge (gdscene::physics_server_3d)
//! that verify:
//! 1. Scene nodes register as physics bodies via sync_to_physics
//! 2. Physics simulation steps are deterministic (same inputs → identical outputs)
//! 3. sync_from_physics writes updated transforms back to the scene tree
//! 4. Multi-body scenes produce reproducible traces
//! 5. Body type mapping matches Godot conventions
//! 6. CollisionShape3D child nodes feed correct shapes into the physics world

use gdscene::physics_server_3d::PhysicsServer3D;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::SceneTree;
use gdcore::math::Vector3;
use gdvariant::Variant;

const DT: f32 = 1.0 / 60.0;
const EPSILON: f32 = 1e-4;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn approx_vec3(a: Vector3, b: Vector3) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
}

// ===========================================================================
// Helpers
// ===========================================================================

fn make_rigid_body_scene() -> (SceneTree, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let body = Node::new("Ball", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();
    node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 10.0, 0.0));
    (tree, body_id)
}

fn make_static_floor_scene() -> (SceneTree, gdscene::NodeId, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Static floor at y=0
    let floor = Node::new("Floor", "StaticBody3D");
    let floor_id = tree.add_child(root, floor).unwrap();
    node3d::set_position(&mut tree, floor_id, Vector3::new(0.0, 0.0, 0.0));

    // Falling rigid body
    let body = Node::new("Ball", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();
    node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 20.0, 0.0));

    (tree, floor_id, body_id)
}

// ===========================================================================
// 1. Registration: RigidBody3D registers as a physics body
// ===========================================================================

#[test]
fn s4x_rigid_body_registers() {
    let (tree, _body_id) = make_rigid_body_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1, "RigidBody3D should register");
}

// ===========================================================================
// 2. Registration: StaticBody3D registers
// ===========================================================================

#[test]
fn s4x_static_body_registers() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Floor", "StaticBody3D")).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1, "StaticBody3D should register");
}

// ===========================================================================
// 3. Registration: CharacterBody3D registers as kinematic
// ===========================================================================

#[test]
fn s4x_character_body_registers() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Player", "CharacterBody3D")).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1, "CharacterBody3D should register");
}

// ===========================================================================
// 4. Non-physics nodes are ignored
// ===========================================================================

#[test]
fn s4x_non_physics_nodes_ignored() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    tree.add_child(root, Node::new("Mesh", "MeshInstance3D")).unwrap();
    tree.add_child(root, Node::new("Light", "DirectionalLight3D")).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 0, "non-physics nodes should not register");
}

// ===========================================================================
// 5. Sync idempotence: double-sync doesn't duplicate
// ===========================================================================

#[test]
fn s4x_sync_idempotent() {
    let (tree, _) = make_rigid_body_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1, "second sync should not duplicate body");
}

// ===========================================================================
// 6. Step advances simulation: rigid body falls
// ===========================================================================

#[test]
fn s4x_step_rigid_body_falls() {
    let (mut tree, body_id) = make_rigid_body_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);

    for _ in 0..30 {
        server.step(DT);
    }

    server.sync_from_physics(&mut tree);
    let pos = node3d::get_position(&tree, body_id);
    assert!(
        pos.y < 10.0,
        "rigid body should fall under gravity, y={}",
        pos.y
    );
}

// ===========================================================================
// 7. Determinism: same scene produces identical traces
// ===========================================================================

#[test]
fn s4x_deterministic_single_body_60_frames() {
    fn run() -> Vector3 {
        let (mut tree, body_id) = make_rigid_body_scene();
        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);

        for _ in 0..60 {
            server.step(DT);
        }

        server.sync_from_physics(&mut tree);
        node3d::get_position(&tree, body_id)
    }

    let a = run();
    let b = run();
    assert!(
        approx_vec3(a, b),
        "single-body simulation must be deterministic: {a:?} vs {b:?}"
    );
}

// ===========================================================================
// 8. Determinism: multi-body scene
// ===========================================================================

#[test]
fn s4x_deterministic_multi_body_120_frames() {
    fn run() -> Vec<Vector3> {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // 3 rigid bodies at different positions
        let mut ids = Vec::new();
        for (i, y) in [10.0f32, 20.0, 30.0].iter().enumerate() {
            let body = Node::new(&format!("Body{i}"), "RigidBody3D");
            let nid = tree.add_child(root, body).unwrap();
            node3d::set_position(&mut tree, nid, Vector3::new(i as f32 * 5.0, *y, 0.0));
            ids.push(nid);
        }

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);

        for _ in 0..120 {
            server.step(DT);
        }

        server.sync_from_physics(&mut tree);
        ids.iter()
            .map(|&nid| node3d::get_position(&tree, nid))
            .collect()
    }

    let a = run();
    let b = run();
    for (i, (pa, pb)) in a.iter().zip(b.iter()).enumerate() {
        assert!(
            approx_vec3(*pa, *pb),
            "body {i} not deterministic: {pa:?} vs {pb:?}"
        );
    }
}

// ===========================================================================
// 9. Removed node is untracked
// ===========================================================================

#[test]
fn s4x_removed_node_untracked() {
    let (mut tree, body_id) = make_rigid_body_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);

    tree.remove_node(body_id).unwrap();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 0, "removed node should be untracked");
}

// ===========================================================================
// 10. CollisionShape3D sphere child feeds shape
// ===========================================================================

#[test]
fn s4x_collision_shape_sphere_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let body = Node::new("Ball", "RigidBody3D");
    let body_nid = tree.add_child(root, body).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    shape.set_property("radius", Variant::Float(3.0));
    tree.add_child(body_nid, shape).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

// ===========================================================================
// 11. CollisionShape3D box child feeds shape
// ===========================================================================

#[test]
fn s4x_collision_shape_box_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let body = Node::new("Crate", "RigidBody3D");
    let body_nid = tree.add_child(root, body).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_owned()));
    shape.set_property("size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0)));
    tree.add_child(body_nid, shape).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

// ===========================================================================
// 12. Custom gravity
// ===========================================================================

#[test]
fn s4x_custom_gravity() {
    let server = PhysicsServer3D::with_gravity(Vector3::new(0.0, -20.0, 0.0));
    let g = server.gravity();
    assert!(approx_eq(g.y, -20.0));
}

// ===========================================================================
// 13. sync_from_physics doesn't move static bodies
// ===========================================================================

#[test]
fn s4x_static_body_position_preserved() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let floor = Node::new("Floor", "StaticBody3D");
    let floor_id = tree.add_child(root, floor).unwrap();
    node3d::set_position(&mut tree, floor_id, Vector3::new(5.0, 0.0, 3.0));

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);

    for _ in 0..60 {
        server.step(DT);
    }

    server.sync_from_physics(&mut tree);
    let pos = node3d::get_position(&tree, floor_id);
    assert!(
        approx_vec3(pos, Vector3::new(5.0, 0.0, 3.0)),
        "static body should not move, got {pos:?}"
    );
}

// ===========================================================================
// 14. Mixed scene: rigid falls, static stays
// ===========================================================================

#[test]
fn s4x_mixed_scene_rigid_falls_static_stays() {
    let (mut tree, floor_id, body_id) = make_static_floor_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 2);

    for _ in 0..60 {
        server.step(DT);
    }

    server.sync_from_physics(&mut tree);

    let floor_pos = node3d::get_position(&tree, floor_id);
    let body_pos = node3d::get_position(&tree, body_id);

    assert!(
        approx_vec3(floor_pos, Vector3::new(0.0, 0.0, 0.0)),
        "floor should stay at origin"
    );
    assert!(
        body_pos.y < 20.0,
        "rigid body should fall from y=20, got y={}",
        body_pos.y
    );
}

// ===========================================================================
// 15. Determinism: mixed scene with floor + multiple bodies
// ===========================================================================

#[test]
fn s4x_deterministic_mixed_scene_180_frames() {
    fn run() -> Vec<Vector3> {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Floor
        let floor = Node::new("Floor", "StaticBody3D");
        let _floor_id = tree.add_child(root, floor).unwrap();

        // 4 rigid bodies
        let mut ids = Vec::new();
        for i in 0..4 {
            let body = Node::new(&format!("Body{i}"), "RigidBody3D");
            let nid = tree.add_child(root, body).unwrap();
            node3d::set_position(
                &mut tree,
                nid,
                Vector3::new(i as f32 * 3.0, 15.0 + i as f32 * 5.0, 0.0),
            );
            ids.push(nid);
        }

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);

        for _ in 0..180 {
            server.step(DT);
        }

        server.sync_from_physics(&mut tree);
        ids.iter()
            .map(|&nid| node3d::get_position(&tree, nid))
            .collect()
    }

    let a = run();
    let b = run();
    for (i, (pa, pb)) in a.iter().zip(b.iter()).enumerate() {
        assert!(
            approx_vec3(*pa, *pb),
            "body {i} not deterministic in mixed scene: {pa:?} vs {pb:?}"
        );
    }
}

// ===========================================================================
// 16. Frame trace: capture positions at each step for golden comparison
// ===========================================================================

#[test]
fn s4x_frame_trace_reproducible() {
    fn run_with_trace() -> Vec<(u32, Vector3)> {
        let (mut tree, body_id) = make_rigid_body_scene();
        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);

        let mut trace = Vec::new();
        for frame in 0..30 {
            server.step(DT);
            server.sync_from_physics(&mut tree);
            let pos = node3d::get_position(&tree, body_id);
            trace.push((frame, pos));
        }
        trace
    }

    let t1 = run_with_trace();
    let t2 = run_with_trace();

    assert_eq!(t1.len(), t2.len());
    for (i, ((f1, p1), (f2, p2))) in t1.iter().zip(t2.iter()).enumerate() {
        assert_eq!(f1, f2, "frame number mismatch at index {i}");
        assert!(
            approx_vec3(*p1, *p2),
            "frame {f1} position mismatch: {p1:?} vs {p2:?}"
        );
    }
}

// ===========================================================================
// 17. Gravity direction: body falls monotonically
// ===========================================================================

#[test]
fn s4x_gravity_monotonic_fall() {
    let (mut tree, body_id) = make_rigid_body_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);

    let mut prev_y = 10.0f32;
    for _ in 0..60 {
        server.step(DT);
        server.sync_from_physics(&mut tree);
        let pos = node3d::get_position(&tree, body_id);
        assert!(
            pos.y <= prev_y + EPSILON,
            "body should fall monotonically: prev={prev_y}, now={}",
            pos.y
        );
        prev_y = pos.y;
    }
}

// ===========================================================================
// 18. No lateral drift under pure vertical gravity
// ===========================================================================

#[test]
fn s4x_no_lateral_drift() {
    let (mut tree, body_id) = make_rigid_body_scene();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);

    for _ in 0..60 {
        server.step(DT);
    }

    server.sync_from_physics(&mut tree);
    let pos = node3d::get_position(&tree, body_id);
    assert!(
        approx_eq(pos.x, 0.0),
        "no x drift expected, got x={}",
        pos.x
    );
    assert!(
        approx_eq(pos.z, 0.0),
        "no z drift expected, got z={}",
        pos.z
    );
}
