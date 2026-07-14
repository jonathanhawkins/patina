//! pat-z6dje: CharacterBody3D move_and_slide for 3D.
//!
//! Integration tests covering:
//! 1. ClassDB registration (properties, inheritance, methods)
//! 2. Scene tree integration (node creation, PhysicsServer3D mapping)
//! 3. move_and_slide — free movement, floor/wall/ceiling detection, sliding
//! 4. move_and_collide — stop-on-contact, no-collision pass-through
//! 5. Collision layer/mask filtering
//! 6. Sub-stepping for fast motion (tunneling prevention)
//! 7. Surface classification (floor_max_angle, up_direction)
//! 8. Multiple bodies and complex scenarios
//! 9. State reset between calls

use gdcore::math::Vector3;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::character::CharacterBody3D;
use gdphysics3d::shape::Shape3D;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn make_sphere_character(pos: Vector3) -> CharacterBody3D {
    CharacterBody3D::new(pos, Shape3D::Sphere { radius: 1.0 })
}

fn make_box_character(pos: Vector3) -> CharacterBody3D {
    CharacterBody3D::new(
        pos,
        Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 1.0, 0.5),
        },
    )
}

fn make_static_body(id: u64, pos: Vector3, shape: Shape3D) -> PhysicsBody3D {
    let mut body = PhysicsBody3D::new(BodyId3D(id), BodyType3D::Static, pos, shape, 1.0);
    body.collision_layer = 1;
    body
}

fn make_floor(y: f32) -> PhysicsBody3D {
    make_static_body(
        1,
        Vector3::new(0.0, y, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(100.0, 1.0, 100.0),
        },
    )
}

fn make_wall_x(x: f32, id: u64) -> PhysicsBody3D {
    make_static_body(
        id,
        Vector3::new(x, 0.0, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 100.0, 100.0),
        },
    )
}

fn make_wall_z(z: f32, id: u64) -> PhysicsBody3D {
    make_static_body(
        id,
        Vector3::new(0.0, 0.0, z),
        Shape3D::BoxShape {
            half_extents: Vector3::new(100.0, 100.0, 1.0),
        },
    )
}

fn make_ceiling(y: f32) -> PhysicsBody3D {
    make_static_body(
        3,
        Vector3::new(0.0, y, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(100.0, 1.0, 100.0),
        },
    )
}

// ===========================================================================
// 1. ClassDB registration
// ===========================================================================

#[test]
fn classdb_registers_characterbody3d() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("CharacterBody3D"));
}

#[test]
fn classdb_characterbody3d_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("CharacterBody3D").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_characterbody3d_has_velocity_property() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("CharacterBody3D");
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"velocity"));
    assert!(names.contains(&"up_direction"));
}

#[test]
fn classdb_characterbody3d_has_methods() {
    gdobject::class_db::register_3d_classes();
    let methods = gdobject::class_db::get_method_list("CharacterBody3D");
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"move_and_slide"), "Missing move_and_slide");
    assert!(names.contains(&"is_on_floor"), "Missing is_on_floor");
}

#[test]
fn classdb_characterbody3d_default_up_direction() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("CharacterBody3D");
    let up = props.iter().find(|p| p.name == "up_direction").unwrap();
    assert_eq!(
        up.default_value,
        Variant::Vector3(Vector3::new(0.0, 1.0, 0.0))
    );
}

// ===========================================================================
// 2. Scene tree integration
// ===========================================================================

#[test]
fn scene_tree_characterbody3d_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Player", "CharacterBody3D");
    let id = tree.add_child(root, node).unwrap();
    let n = tree.get_node(id).unwrap();
    assert_eq!(n.class_name(), "CharacterBody3D");
    assert_eq!(n.name(), "Player");
}

// ===========================================================================
// 3. move_and_slide — basic movement
// ===========================================================================

#[test]
fn move_and_slide_free_movement_no_obstacles() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    let bodies: Vec<&PhysicsBody3D> = vec![];
    let result = ch.move_and_slide(Vector3::new(5.0, 0.0, 3.0), &bodies);
    assert!(approx(ch.position.x, 5.0));
    assert!(approx(ch.position.z, 3.0));
    assert!(approx(result.x, 5.0));
    assert!(approx(result.z, 3.0));
    assert!(!ch.is_on_floor());
    assert!(!ch.is_on_wall());
    assert!(!ch.is_on_ceiling());
}

#[test]
fn move_and_slide_zero_velocity_noop() {
    let mut ch = make_sphere_character(Vector3::new(1.0, 2.0, 3.0));
    let bodies: Vec<&PhysicsBody3D> = vec![];
    let result = ch.move_and_slide(Vector3::ZERO, &bodies);
    assert!(approx(ch.position.x, 1.0));
    assert!(approx(ch.position.y, 2.0));
    assert!(approx(ch.position.z, 3.0));
    assert!(result.length() < EPSILON);
}

#[test]
fn move_and_slide_lands_on_floor() {
    // Character sphere at y=2, radius=1. Floor box at y=-1, half_extents.y=1 (top at y=0).
    // Move down by 2.5 => center at y=-0.5 which is inside floor box => collision.
    let mut ch = make_sphere_character(Vector3::new(0.0, 2.0, 0.0));
    let floor = make_floor(-1.0);
    let bodies: Vec<&PhysicsBody3D> = vec![&floor];
    let result = ch.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
    assert!(ch.is_on_floor(), "Should detect floor");
    assert!(!ch.is_on_wall());
    assert!(!ch.is_on_ceiling());
    assert!(
        result.y.abs() < EPSILON,
        "Y velocity should be zeroed after floor contact"
    );
}

#[test]
fn move_and_slide_floor_normal_points_up() {
    let mut ch = make_sphere_character(Vector3::new(0.0, 2.0, 0.0));
    let floor = make_floor(-1.0);
    let bodies: Vec<&PhysicsBody3D> = vec![&floor];
    ch.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
    let normal = ch.get_floor_normal();
    assert!(
        normal.y > 0.5,
        "Floor normal should point up, got {:?}",
        normal
    );
}

#[test]
fn move_and_slide_slides_along_wall_preserves_parallel() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    let wall = make_wall_x(5.0, 2);
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];
    let result = ch.move_and_slide(Vector3::new(6.0, 0.0, 4.0), &bodies);
    assert!(ch.is_on_wall(), "Should detect wall");
    assert!(
        result.x.abs() < EPSILON,
        "X velocity should be zeroed by wall"
    );
    assert!(approx(result.z, 4.0), "Z velocity should be preserved");
}

#[test]
fn move_and_slide_detects_ceiling() {
    // Character at y=-2, radius=1. Ceiling box at y=1, half_extents.y=1 (bottom at y=0).
    // Move up by 2.5 => center at y=0.5 which is inside ceiling box => collision.
    let mut ch = make_sphere_character(Vector3::new(0.0, -2.0, 0.0));
    let ceiling = make_ceiling(1.0);
    let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];
    ch.move_and_slide(Vector3::new(0.0, 2.5, 0.0), &bodies);
    assert!(ch.is_on_ceiling(), "Should detect ceiling");
    assert!(!ch.is_on_floor());
}

// ===========================================================================
// 4. move_and_collide
// ===========================================================================

#[test]
fn move_and_collide_no_collision_returns_none() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    let bodies: Vec<&PhysicsBody3D> = vec![];
    let result = ch.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &bodies);
    assert!(result.is_none());
    assert!(approx(ch.position.x, 5.0));
}

#[test]
fn move_and_collide_returns_collision_result() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    let wall = make_wall_x(5.0, 2);
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];
    let result = ch.move_and_collide(Vector3::new(6.0, 0.0, 0.0), &bodies);
    assert!(result.is_some(), "Should report collision");
    let r = result.unwrap();
    assert!(r.colliding);
    assert!(r.depth > 0.0);
}

// ===========================================================================
// 5. Collision layer/mask filtering
// ===========================================================================

#[test]
fn move_and_slide_ignores_wrong_layer() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    ch.collision_mask = 2; // Only scan layer 2

    let mut wall = make_wall_x(3.0, 2);
    wall.collision_layer = 1; // Wall on layer 1
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];

    ch.move_and_slide(Vector3::new(10.0, 0.0, 0.0), &bodies);
    assert!(!ch.is_on_wall(), "Should not collide with layer 1 wall");
    assert!(approx(ch.position.x, 10.0));
}

#[test]
fn move_and_slide_collides_matching_layer() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    ch.collision_mask = 2;

    let mut wall = make_wall_x(5.0, 2);
    wall.collision_layer = 2; // Matching layer
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];

    ch.move_and_slide(Vector3::new(6.0, 0.0, 0.0), &bodies);
    assert!(ch.is_on_wall(), "Should collide with matching layer");
}

#[test]
fn move_and_collide_ignores_wrong_layer() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    ch.collision_mask = 4;

    let mut wall = make_wall_x(3.0, 2);
    wall.collision_layer = 1;
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];

    let result = ch.move_and_collide(Vector3::new(10.0, 0.0, 0.0), &bodies);
    assert!(result.is_none());
    assert!(approx(ch.position.x, 10.0));
}

// ===========================================================================
// 6. Sub-stepping prevents tunneling
// ===========================================================================

#[test]
fn move_and_slide_moderate_speed_hits_wall() {
    // Sphere radius = 1.0, wall at x=5 (half_extents.x=1, left edge at x=4).
    // Moving 6 units right puts sphere center at x=6, well inside the wall box.
    // Sub-stepping should detect collision.
    let mut ch = make_sphere_character(Vector3::ZERO);
    let wall = make_wall_x(5.0, 2);
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];
    ch.move_and_slide(Vector3::new(6.0, 0.0, 0.0), &bodies);
    assert!(
        ch.is_on_wall(),
        "Should detect wall with moderate speed motion"
    );
}

// ===========================================================================
// 7. Custom up_direction and floor_max_angle
// ===========================================================================

#[test]
fn custom_up_direction() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    // Set up direction to negative X (sideways gravity scenario).
    ch.up_direction = Vector3::new(-1.0, 0.0, 0.0);
    assert!(approx(ch.up_direction.x, -1.0));
    assert!(approx(ch.up_direction.y, 0.0));
}

#[test]
fn floor_max_angle_default() {
    let ch = make_sphere_character(Vector3::ZERO);
    assert!(
        approx(ch.floor_max_angle, std::f32::consts::FRAC_PI_4),
        "Default floor_max_angle should be pi/4"
    );
}

// ===========================================================================
// 8. Multiple bodies
// ===========================================================================

#[test]
fn move_and_slide_corner_two_walls() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    let wall_x = make_wall_x(5.0, 2);
    let wall_z = make_wall_z(5.0, 3);
    let bodies: Vec<&PhysicsBody3D> = vec![&wall_x, &wall_z];

    let result = ch.move_and_slide(Vector3::new(6.0, 0.0, 6.0), &bodies);
    // Both X and Z should be blocked.
    assert!(
        result.x.abs() < 1.0,
        "X should be mostly zeroed near corner"
    );
    assert!(
        result.z.abs() < 1.0,
        "Z should be mostly zeroed near corner"
    );
}

#[test]
fn move_and_slide_floor_and_wall_simultaneous() {
    let mut ch = make_sphere_character(Vector3::new(0.0, 2.0, 0.0));
    let floor = make_floor(-1.0);
    let wall = make_wall_x(5.0, 2);
    let bodies: Vec<&PhysicsBody3D> = vec![&floor, &wall];

    ch.move_and_slide(Vector3::new(6.0, -2.5, 0.0), &bodies);
    assert!(ch.is_on_floor(), "Should detect floor");
}

// ===========================================================================
// 9. State resets between calls
// ===========================================================================

#[test]
fn state_resets_between_move_and_slide_calls() {
    let mut ch = make_sphere_character(Vector3::new(0.0, 2.0, 0.0));
    let floor = make_floor(-1.0);
    let bodies: Vec<&PhysicsBody3D> = vec![&floor];

    // First call: hit the floor (center at y=-0.5 inside floor box)
    ch.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
    assert!(ch.is_on_floor());

    // Second call: move away from floor (no collision)
    let empty: Vec<&PhysicsBody3D> = vec![];
    ch.move_and_slide(Vector3::new(0.0, 5.0, 0.0), &empty);
    assert!(!ch.is_on_floor(), "Floor state should reset between calls");
    assert!(!ch.is_on_wall());
    assert!(!ch.is_on_ceiling());
}

#[test]
fn wall_normal_available_after_collision() {
    let mut ch = make_sphere_character(Vector3::ZERO);
    let wall = make_wall_x(5.0, 2);
    let bodies: Vec<&PhysicsBody3D> = vec![&wall];

    ch.move_and_slide(Vector3::new(6.0, 0.0, 0.0), &bodies);
    assert!(ch.is_on_wall());
    let wn = ch.get_wall_normal();
    assert!(
        wn.x.abs() > 0.5,
        "Wall normal should have significant X, got {:?}",
        wn
    );
}

// ===========================================================================
// 10. Box-shaped character body
// ===========================================================================

#[test]
fn box_character_slides_on_floor() {
    // Box character half_extents.y=1.0, at y=2. Floor at y=-1 (top at y=0).
    // Move down 2.5 => center at y=-0.5 inside floor box => collision.
    let mut ch = make_box_character(Vector3::new(0.0, 2.0, 0.0));
    let floor = make_floor(-1.0);
    let bodies: Vec<&PhysicsBody3D> = vec![&floor];

    ch.move_and_slide(Vector3::new(2.0, -2.5, 0.0), &bodies);
    assert!(ch.is_on_floor(), "Box character should detect floor");
}

#[test]
fn capsule_character_free_movement() {
    let mut ch = CharacterBody3D::new(
        Vector3::ZERO,
        Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        },
    );
    let bodies: Vec<&PhysicsBody3D> = vec![];
    ch.move_and_slide(Vector3::new(3.0, 0.0, 4.0), &bodies);
    assert!(approx(ch.position.x, 3.0));
    assert!(approx(ch.position.z, 4.0));
}

// ===========================================================================
// 11. Godot API surface: defaults
// ===========================================================================

#[test]
fn characterbody3d_default_collision_layer_mask() {
    let ch = make_sphere_character(Vector3::ZERO);
    assert_eq!(ch.collision_layer, 1);
    assert_eq!(ch.collision_mask, 1);
}

#[test]
fn characterbody3d_up_direction_default_positive_y() {
    let ch = make_sphere_character(Vector3::ZERO);
    assert!(approx(ch.up_direction.x, 0.0));
    assert!(approx(ch.up_direction.y, 1.0));
    assert!(approx(ch.up_direction.z, 0.0));
}
