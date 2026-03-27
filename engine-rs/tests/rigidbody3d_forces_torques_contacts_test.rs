//! pat-4gdj0: RigidBody3D full API with forces, torques, and contacts.
//!
//! Integration tests covering:
//! 1. ClassDB registration (properties, methods, inheritance)
//! 2. Scene tree integration (node creation, properties)
//! 3. Force API (apply_force, apply_central_force, apply_force_at_position)
//! 4. Impulse API (apply_impulse, apply_impulse_at_position)
//! 5. Torque API (apply_torque, apply_torque_impulse)
//! 6. Gravity scale
//! 7. Linear and angular damping
//! 8. Sleep/wake mechanics
//! 9. Freeze modes
//! 10. Contact monitoring and reporting
//! 11. Physics world integration
//! 12. Deterministic simulation

use gdcore::math::Vector3;
use gdphysics3d::body::*;
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn make_rigid(pos: Vector3) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        pos,
        Shape3D::Sphere { radius: 1.0 },
        1.0,
    )
}

fn make_rigid_mass(pos: Vector3, mass: f32) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        pos,
        Shape3D::Sphere { radius: 1.0 },
        mass,
    )
}

// ===========================================================================
// 1. ClassDB registration
// ===========================================================================

#[test]
fn classdb_rigidbody3d_exists() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("RigidBody3D"));
}

#[test]
fn classdb_rigidbody3d_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("RigidBody3D").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_rigidbody3d_has_physics_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("RigidBody3D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();

    let expected = [
        "mass", "gravity_scale", "linear_velocity", "angular_velocity",
        "linear_damp", "angular_damp", "friction", "bounce",
        "continuous_cd", "can_sleep", "sleeping", "freeze_mode",
        "contact_monitor", "max_contacts_reported",
    ];
    for prop in &expected {
        assert!(names.contains(prop), "Missing property: {prop}");
    }
}

#[test]
fn classdb_rigidbody3d_has_force_methods() {
    gdobject::class_db::register_3d_classes();
    let methods = [
        "apply_force", "apply_central_force", "apply_impulse",
        "apply_torque", "apply_torque_impulse", "get_contact_count",
    ];
    for method in &methods {
        assert!(
            gdobject::class_db::class_has_method("RigidBody3D", method),
            "Missing method: {method}"
        );
    }
}

#[test]
fn classdb_rigidbody3d_default_mass_is_1() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("RigidBody3D", false);
    let mass = props.iter().find(|p| p.name == "mass").unwrap();
    assert_eq!(mass.default_value, Variant::Float(1.0));
}

// ===========================================================================
// 2. Scene tree integration
// ===========================================================================

#[test]
fn rigidbody3d_node_creation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Ball", "RigidBody3D");
    let id = tree.add_child(root, node).unwrap();
    assert_eq!(tree.get_node(id).unwrap().class_name(), "RigidBody3D");
}

#[test]
fn rigidbody3d_set_get_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("R", "RigidBody3D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id).unwrap().set_property("mass", Variant::Float(5.0));
    assert_eq!(tree.get_node(id).unwrap().get_property("mass"), Variant::Float(5.0));

    tree.get_node_mut(id).unwrap().set_property("gravity_scale", Variant::Float(0.5));
    assert_eq!(tree.get_node(id).unwrap().get_property("gravity_scale"), Variant::Float(0.5));
}

// ===========================================================================
// 3. Force API
// ===========================================================================

#[test]
fn apply_force_accumulates() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_force(Vector3::new(10.0, 0.0, 0.0));
    body.apply_force(Vector3::new(5.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    // 15N on 1kg for 1s → v = 15 m/s
    assert!(approx(body.linear_velocity.x, 15.0), "v.x={}", body.linear_velocity.x);
}

#[test]
fn apply_central_force_same_as_apply_force() {
    let mut a = make_rigid(Vector3::ZERO);
    let mut b = make_rigid(Vector3::ZERO);
    a.apply_force(Vector3::new(10.0, 0.0, 0.0));
    b.apply_central_force(Vector3::new(10.0, 0.0, 0.0));
    a.integrate(1.0, Vector3::ZERO);
    b.integrate(1.0, Vector3::ZERO);
    assert_eq!(a.linear_velocity, b.linear_velocity);
}

#[test]
fn apply_force_at_position_generates_torque() {
    let mut body = make_rigid(Vector3::ZERO);
    // Force at offset → should generate torque
    body.apply_force_at_position(
        Vector3::new(0.0, 10.0, 0.0), // force up
        Vector3::new(1.0, 0.0, 0.0),  // at +x offset
    );
    body.integrate(1.0, Vector3::ZERO);
    // Torque = offset × force = (1,0,0) × (0,10,0) = (0,0,10)
    assert!(body.angular_velocity.length() > 0.1, "Should have angular velocity");
}

#[test]
fn static_body_ignores_forces() {
    let mut body = PhysicsBody3D::new(
        BodyId3D(0), BodyType3D::Static, Vector3::ZERO,
        Shape3D::Sphere { radius: 1.0 }, 1.0,
    );
    body.apply_force(Vector3::new(100.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    assert_eq!(body.position, Vector3::ZERO);
    assert_eq!(body.linear_velocity, Vector3::ZERO);
}

#[test]
fn kinematic_body_ignores_forces() {
    let mut body = PhysicsBody3D::new(
        BodyId3D(0), BodyType3D::Kinematic, Vector3::ZERO,
        Shape3D::Sphere { radius: 1.0 }, 1.0,
    );
    body.apply_force(Vector3::new(100.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    assert_eq!(body.position, Vector3::ZERO);
}

// ===========================================================================
// 4. Impulse API
// ===========================================================================

#[test]
fn apply_impulse_instant_velocity_change() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_impulse(Vector3::new(5.0, 0.0, 0.0));
    // Impulse on 1kg body → v = 5 m/s immediately
    assert!(approx(body.linear_velocity.x, 5.0));
}

#[test]
fn apply_impulse_mass_dependent() {
    let mut body = make_rigid_mass(Vector3::ZERO, 2.0);
    body.apply_impulse(Vector3::new(10.0, 0.0, 0.0));
    // 10 Ns on 2kg → v = 5 m/s
    assert!(approx(body.linear_velocity.x, 5.0));
}

#[test]
fn apply_impulse_at_position_affects_angular() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_impulse_at_position(
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert!(body.linear_velocity.y > 0.0, "Should have linear velocity");
    assert!(body.angular_velocity.length() > 0.1, "Should have angular velocity");
}

// ===========================================================================
// 5. Torque API
// ===========================================================================

#[test]
fn apply_torque_changes_angular_velocity() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_torque(Vector3::new(0.0, 5.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    assert!(body.angular_velocity.y > 0.0, "Torque should create angular velocity");
}

#[test]
fn apply_torque_impulse_instant() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_torque_impulse(Vector3::new(0.0, 0.0, 3.0));
    assert!(body.angular_velocity.z > 0.0, "Torque impulse should be instant");
}

#[test]
fn torque_accumulates() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_torque(Vector3::new(1.0, 0.0, 0.0));
    body.apply_torque(Vector3::new(1.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    // 2 Nm on 1kg body for 1s → ω ≈ 2 rad/s (simplified inertia)
    assert!(body.angular_velocity.x > 1.5, "Torque should accumulate: ω.x={}", body.angular_velocity.x);
}

#[test]
fn static_body_ignores_torque() {
    let mut body = PhysicsBody3D::new(
        BodyId3D(0), BodyType3D::Static, Vector3::ZERO,
        Shape3D::Sphere { radius: 1.0 }, 1.0,
    );
    body.apply_torque(Vector3::new(100.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    assert_eq!(body.angular_velocity, Vector3::ZERO);
}

// ===========================================================================
// 6. Gravity scale
// ===========================================================================

#[test]
fn gravity_scale_default_is_one() {
    let body = make_rigid(Vector3::ZERO);
    assert!(approx(body.gravity_scale, 1.0));
}

#[test]
fn gravity_scale_zero_no_gravity() {
    let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
    body.gravity_scale = 0.0;
    body.integrate(1.0, Vector3::new(0.0, -9.8, 0.0));
    assert!(approx(body.position.y, 10.0), "Zero gravity scale should not move: y={}", body.position.y);
}

#[test]
fn gravity_scale_double() {
    let mut a = make_rigid(Vector3::new(0.0, 100.0, 0.0));
    let mut b = make_rigid(Vector3::new(0.0, 100.0, 0.0));
    b.gravity_scale = 2.0;
    let g = Vector3::new(0.0, -9.8, 0.0);
    a.integrate(1.0, g);
    b.integrate(1.0, g);
    assert!(b.position.y < a.position.y, "2x gravity should fall faster");
}

#[test]
fn gravity_scale_negative_floats_up() {
    let mut body = make_rigid(Vector3::new(0.0, 0.0, 0.0));
    body.gravity_scale = -1.0;
    body.integrate(1.0, Vector3::new(0.0, -9.8, 0.0));
    assert!(body.position.y > 0.0, "Negative gravity scale should go up: y={}", body.position.y);
}

// ===========================================================================
// 7. Damping
// ===========================================================================

#[test]
fn linear_damp_reduces_velocity() {
    let mut body = make_rigid(Vector3::ZERO);
    body.linear_damp = 0.5;
    body.apply_impulse(Vector3::new(10.0, 0.0, 0.0));
    let v0 = body.linear_velocity.x;
    body.integrate(1.0, Vector3::ZERO);
    assert!(body.linear_velocity.x < v0, "Linear damp should reduce velocity");
}

#[test]
fn angular_damp_reduces_angular_velocity() {
    let mut body = make_rigid(Vector3::ZERO);
    body.angular_damp = 0.5;
    body.apply_torque_impulse(Vector3::new(0.0, 10.0, 0.0));
    let w0 = body.angular_velocity.y;
    body.integrate(1.0, Vector3::ZERO);
    assert!(body.angular_velocity.y < w0, "Angular damp should reduce angular velocity");
}

#[test]
fn zero_damp_preserves_velocity() {
    let mut body = make_rigid(Vector3::ZERO);
    body.linear_damp = 0.0;
    body.angular_damp = 0.0;
    body.apply_impulse(Vector3::new(10.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    // With no gravity and no damp, velocity should be preserved
    assert!(approx(body.linear_velocity.x, 10.0), "v.x={}", body.linear_velocity.x);
}

// ===========================================================================
// 8. Sleep/wake
// ===========================================================================

#[test]
fn body_starts_awake() {
    let body = make_rigid(Vector3::ZERO);
    assert!(!body.sleeping);
}

#[test]
fn body_can_sleep_default_true() {
    let body = make_rigid(Vector3::ZERO);
    assert!(body.can_sleep);
}

#[test]
fn put_to_sleep_zeroes_velocity() {
    let mut body = make_rigid(Vector3::ZERO);
    body.apply_impulse(Vector3::new(10.0, 0.0, 0.0));
    body.put_to_sleep();
    assert!(body.sleeping);
    assert_eq!(body.linear_velocity, Vector3::ZERO);
    assert_eq!(body.angular_velocity, Vector3::ZERO);
}

#[test]
fn sleeping_body_does_not_integrate() {
    let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
    body.put_to_sleep();
    let pos_before = body.position;
    body.integrate(1.0, Vector3::new(0.0, -9.8, 0.0));
    assert_eq!(body.position, pos_before, "Sleeping body should not move");
}

#[test]
fn force_wakes_sleeping_body() {
    let mut body = make_rigid(Vector3::ZERO);
    body.put_to_sleep();
    assert!(body.sleeping);
    body.apply_force(Vector3::new(1.0, 0.0, 0.0));
    assert!(!body.sleeping, "Force should wake body");
}

#[test]
fn impulse_wakes_sleeping_body() {
    let mut body = make_rigid(Vector3::ZERO);
    body.put_to_sleep();
    body.apply_impulse(Vector3::new(1.0, 0.0, 0.0));
    assert!(!body.sleeping);
}

#[test]
fn torque_wakes_sleeping_body() {
    let mut body = make_rigid(Vector3::ZERO);
    body.put_to_sleep();
    body.apply_torque(Vector3::new(1.0, 0.0, 0.0));
    assert!(!body.sleeping);
}

// ===========================================================================
// 9. Freeze modes
// ===========================================================================

#[test]
fn freeze_mode_default_none() {
    let body = make_rigid(Vector3::ZERO);
    assert_eq!(body.freeze_mode, FreezeMode::None);
}

#[test]
fn frozen_static_ignores_integration() {
    let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
    body.freeze_mode = FreezeMode::Static;
    body.integrate(1.0, Vector3::new(0.0, -9.8, 0.0));
    assert!(approx(body.position.y, 10.0), "Frozen body should not move");
}

#[test]
fn frozen_kinematic_ignores_integration() {
    let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
    body.freeze_mode = FreezeMode::Kinematic;
    body.integrate(1.0, Vector3::new(0.0, -9.8, 0.0));
    assert!(approx(body.position.y, 10.0));
}

#[test]
fn frozen_body_ignores_forces() {
    let mut body = make_rigid(Vector3::ZERO);
    body.freeze_mode = FreezeMode::Static;
    body.apply_force(Vector3::new(100.0, 0.0, 0.0));
    body.integrate(1.0, Vector3::ZERO);
    assert_eq!(body.linear_velocity, Vector3::ZERO);
}

// ===========================================================================
// 10. Contact monitoring
// ===========================================================================

#[test]
fn contact_monitor_default_off() {
    let body = make_rigid(Vector3::ZERO);
    assert!(!body.contact_monitor);
    assert_eq!(body.max_contacts_reported, 0);
}

#[test]
fn contact_added_when_monitoring() {
    let mut body = make_rigid(Vector3::ZERO);
    body.contact_monitor = true;
    body.max_contacts_reported = 10;
    body.add_contact(ContactPoint3D {
        position: Vector3::new(1.0, 0.0, 0.0),
        normal: Vector3::new(1.0, 0.0, 0.0),
        depth: 0.1,
        other_body: BodyId3D(2),
    });
    assert_eq!(body.get_contact_count(), 1);
    assert_eq!(body.get_contacts()[0].other_body, BodyId3D(2));
}

#[test]
fn contact_not_added_when_monitoring_off() {
    let mut body = make_rigid(Vector3::ZERO);
    body.contact_monitor = false;
    body.add_contact(ContactPoint3D {
        position: Vector3::ZERO,
        normal: Vector3::ZERO,
        depth: 0.0,
        other_body: BodyId3D(2),
    });
    assert_eq!(body.get_contact_count(), 0);
}

#[test]
fn contacts_limited_by_max() {
    let mut body = make_rigid(Vector3::ZERO);
    body.contact_monitor = true;
    body.max_contacts_reported = 2;
    for i in 0..5 {
        body.add_contact(ContactPoint3D {
            position: Vector3::ZERO,
            normal: Vector3::ZERO,
            depth: 0.0,
            other_body: BodyId3D(i),
        });
    }
    assert_eq!(body.get_contact_count(), 2);
}

#[test]
fn clear_contacts_empties_list() {
    let mut body = make_rigid(Vector3::ZERO);
    body.contact_monitor = true;
    body.max_contacts_reported = 10;
    body.add_contact(ContactPoint3D {
        position: Vector3::ZERO,
        normal: Vector3::ZERO,
        depth: 0.0,
        other_body: BodyId3D(1),
    });
    body.clear_contacts();
    assert_eq!(body.get_contact_count(), 0);
}

// ===========================================================================
// 11. Physics world integration
// ===========================================================================

#[test]
fn world_collision_records_contacts() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let mut a = make_rigid(Vector3::ZERO);
    a.contact_monitor = true;
    a.max_contacts_reported = 10;
    let id_a = world.add_body(a);

    let mut b = make_rigid(Vector3::new(1.0, 0.0, 0.0));
    b.contact_monitor = true;
    b.max_contacts_reported = 10;
    let id_b = world.add_body(b);

    world.step(1.0 / 60.0);

    let body_a = world.get_body(id_a).unwrap();
    let body_b = world.get_body(id_b).unwrap();
    assert!(body_a.get_contact_count() > 0, "Body A should have contacts");
    assert!(body_b.get_contact_count() > 0, "Body B should have contacts");
    assert_eq!(body_a.get_contacts()[0].other_body, id_b);
    assert_eq!(body_b.get_contacts()[0].other_body, id_a);
}

#[test]
fn world_contacts_cleared_each_step() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let mut a = make_rigid(Vector3::ZERO);
    a.contact_monitor = true;
    a.max_contacts_reported = 10;
    let id_a = world.add_body(a);

    let b = make_rigid(Vector3::new(1.0, 0.0, 0.0));
    world.add_body(b);

    world.step(1.0 / 60.0); // collision + contacts
    world.step(1.0 / 60.0); // bodies separated, contacts cleared

    let body_a = world.get_body(id_a).unwrap();
    // After separation, contacts should be from this step only (may be 0)
    // The key test is that contacts don't accumulate across steps
    assert!(body_a.get_contact_count() <= 1);
}

#[test]
fn world_gravity_with_gravity_scale() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::new(0.0, -10.0, 0.0);

    let mut body = make_rigid(Vector3::new(0.0, 100.0, 0.0));
    body.gravity_scale = 0.5;
    body.can_sleep = false;
    let id = world.add_body(body);

    world.step(1.0);
    let pos = world.get_body(id).unwrap().position.y;
    // With 0.5 gravity scale, should fall less than full gravity
    assert!(pos > 85.0 && pos < 100.0, "Half gravity: y={pos}");
}

#[test]
fn world_force_then_step() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let id = world.add_body(make_rigid(Vector3::ZERO));
    world.get_body_mut(id).unwrap().apply_force(Vector3::new(10.0, 0.0, 0.0));
    world.step(1.0);

    let body = world.get_body(id).unwrap();
    assert!(body.position.x > 0.0, "Force should move body");
}

// ===========================================================================
// 12. Deterministic simulation
// ===========================================================================

#[test]
fn deterministic_with_forces_and_torques() {
    let run = || {
        let mut world = PhysicsWorld3D::new();
        let mut body = make_rigid(Vector3::new(0.0, 50.0, 0.0));
        body.can_sleep = false;
        let id = world.add_body(body);
        world.get_body_mut(id).unwrap().apply_force(Vector3::new(5.0, 0.0, 0.0));
        world.get_body_mut(id).unwrap().apply_torque(Vector3::new(0.0, 1.0, 0.0));
        for _ in 0..60 {
            world.step(1.0 / 60.0);
        }
        let b = world.get_body(id).unwrap();
        (b.position, b.linear_velocity, b.angular_velocity)
    };
    let (pos_a, vel_a, ang_a) = run();
    let (pos_b, vel_b, ang_b) = run();
    assert_eq!(pos_a, pos_b);
    assert_eq!(vel_a, vel_b);
    assert_eq!(ang_a, ang_b);
}

// ===========================================================================
// 13. Inverse mass
// ===========================================================================

#[test]
fn inverse_mass_rigid() {
    let body = make_rigid_mass(Vector3::ZERO, 4.0);
    assert!(approx(body.inverse_mass(), 0.25));
}

#[test]
fn inverse_mass_static_zero() {
    let body = PhysicsBody3D::new(
        BodyId3D(0), BodyType3D::Static, Vector3::ZERO,
        Shape3D::Sphere { radius: 1.0 }, 10.0,
    );
    assert!(approx(body.inverse_mass(), 0.0));
}

// ===========================================================================
// 14. Rotation integration
// ===========================================================================

#[test]
fn angular_velocity_changes_rotation() {
    let mut body = make_rigid(Vector3::ZERO);
    body.can_sleep = false;
    body.apply_torque_impulse(Vector3::new(0.0, 5.0, 0.0));
    let rot_before = body.rotation;
    body.integrate(0.1, Vector3::ZERO);
    assert_ne!(body.rotation, rot_before, "Rotation should change with angular velocity");
}

// ===========================================================================
// 15. Multiple body interaction
// ===========================================================================

#[test]
fn two_bodies_different_masses() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let mut light = make_rigid_mass(Vector3::ZERO, 1.0);
    light.can_sleep = false;
    let id_light = world.add_body(light);

    let mut heavy = make_rigid_mass(Vector3::new(1.0, 0.0, 0.0), 10.0);
    heavy.can_sleep = false;
    let id_heavy = world.add_body(heavy);

    world.step(1.0 / 60.0);

    let light_pos = world.get_body(id_light).unwrap().position;
    let heavy_pos = world.get_body(id_heavy).unwrap().position;
    // Light body should be pushed more than heavy body
    assert!(
        light_pos.x.abs() > (heavy_pos.x - 1.0).abs() * 0.5,
        "Light body should move more: light.x={}, heavy.x={}",
        light_pos.x, heavy_pos.x
    );
}
