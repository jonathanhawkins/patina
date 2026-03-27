//! pat-z6dje: CharacterBody3D move_and_slide for 3D.
//!
//! Validates:
//! 1. move_and_slide with floor, wall, ceiling detection
//! 2. move_and_collide for single-collision stopping
//! 3. Surface normal classification (floor/wall/ceiling)
//! 4. Collision layer/mask filtering
//! 5. Sub-stepping prevents tunneling through thin surfaces
//! 6. Box-box collision detection
//! 7. Sphere-inside-box collision detection
//! 8. Zero velocity no-op

use gdcore::math::Vector3;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::character::CharacterBody3D;
use gdphysics3d::collision;
use gdphysics3d::shape::Shape3D;

// ── Helpers ─────────────────────────────────────────────────────────

fn make_floor(y: f32) -> PhysicsBody3D {
    let mut body = PhysicsBody3D::new(
        BodyId3D(100),
        BodyType3D::Static,
        Vector3::new(0.0, y, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(100.0, 1.0, 100.0),
        },
        1.0,
    );
    body.collision_layer = 1;
    body
}

fn make_wall(x: f32) -> PhysicsBody3D {
    let mut body = PhysicsBody3D::new(
        BodyId3D(200),
        BodyType3D::Static,
        Vector3::new(x, 0.0, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 100.0, 100.0),
        },
        1.0,
    );
    body.collision_layer = 1;
    body
}

fn make_ceiling(y: f32) -> PhysicsBody3D {
    let mut body = PhysicsBody3D::new(
        BodyId3D(300),
        BodyType3D::Static,
        Vector3::new(0.0, y, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(100.0, 1.0, 100.0),
        },
        1.0,
    );
    body.collision_layer = 1;
    body
}

// ── move_and_slide ──────────────────────────────────────────────────

#[test]
fn move_and_slide_free_movement() {
    let mut c = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
    let result = c.move_and_slide(Vector3::new(5.0, 0.0, 0.0), &[]);
    assert!((c.position.x - 5.0).abs() < 1e-3);
    assert!(!c.is_on_floor());
    assert!(!c.is_on_wall());
    assert!(!c.is_on_ceiling());
    assert!((result.x - 5.0).abs() < 1e-3);
}

#[test]
fn move_and_slide_lands_on_floor() {
    let mut c = CharacterBody3D::new(
        Vector3::new(0.0, 2.0, 0.0),
        Shape3D::Sphere { radius: 1.0 },
    );
    let floor = make_floor(-1.0);
    let result = c.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &[&floor]);
    assert!(c.is_on_floor(), "Should detect floor after landing");
    assert!(result.y.abs() < 1e-2, "Y velocity should be zeroed by floor");
}

#[test]
fn move_and_slide_slides_along_wall() {
    let mut c = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
    let wall = make_wall(5.0);
    let result = c.move_and_slide(Vector3::new(5.0, 0.0, 3.0), &[&wall]);
    assert!(c.is_on_wall(), "Should detect wall");
    assert!(result.x.abs() < 1e-2, "X velocity should be zeroed by wall");
    assert!((result.z - 3.0).abs() < 1e-2, "Z velocity should be preserved");
}

#[test]
fn move_and_slide_detects_ceiling() {
    let mut c = CharacterBody3D::new(
        Vector3::new(0.0, -2.0, 0.0),
        Shape3D::Sphere { radius: 1.0 },
    );
    let ceiling = make_ceiling(1.0);
    let _result = c.move_and_slide(Vector3::new(0.0, 2.5, 0.0), &[&ceiling]);
    assert!(c.is_on_ceiling(), "Should detect ceiling");
    assert!(!c.is_on_floor());
}

#[test]
fn move_and_slide_zero_velocity() {
    let pos = Vector3::new(1.0, 2.0, 3.0);
    let mut c = CharacterBody3D::new(pos, Shape3D::Sphere { radius: 1.0 });
    let result = c.move_and_slide(Vector3::ZERO, &[]);
    assert!((c.position.x - pos.x).abs() < 1e-6);
    assert!((c.position.y - pos.y).abs() < 1e-6);
    assert!((c.position.z - pos.z).abs() < 1e-6);
    assert!(result.length_squared() < 1e-6);
}

#[test]
fn move_and_slide_collision_mask_filtering() {
    let mut c = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
    c.collision_mask = 2; // Only collide with layer 2

    let mut wall = make_wall(3.0);
    wall.collision_layer = 1; // Wall on layer 1 — should be ignored

    let _result = c.move_and_slide(Vector3::new(10.0, 0.0, 0.0), &[&wall]);
    assert!(!c.is_on_wall(), "Wall on wrong layer should be ignored");
    assert!((c.position.x - 10.0).abs() < 1e-3);
}

#[test]
fn move_and_slide_floor_normal_points_up() {
    let mut c = CharacterBody3D::new(
        Vector3::new(0.0, 2.0, 0.0),
        Shape3D::Sphere { radius: 1.0 },
    );
    let floor = make_floor(-1.0);
    c.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &[&floor]);
    let normal = c.get_floor_normal();
    assert!(
        normal.y > 0.5,
        "Floor normal should point up, got {:?}",
        normal
    );
}

// ── move_and_collide ────────────────────────────────────────────────

#[test]
fn move_and_collide_no_collision() {
    let mut c = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
    let result = c.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &[]);
    assert!(result.is_none());
    assert!((c.position.x - 5.0).abs() < 1e-4);
}

#[test]
fn move_and_collide_stops_at_wall() {
    let mut c = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
    let wall = make_wall(5.0);
    let result = c.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &[&wall]);
    assert!(result.is_some(), "Should collide with wall");
}

// ── Collision detection ─────────────────────────────────────────────

#[test]
fn sphere_box_collision_detects_overlap() {
    // Sphere partially inside box
    let result = collision::test_sphere_box(
        Vector3::new(0.0, 0.8, 0.0),
        1.0,
        Vector3::ZERO,
        Vector3::new(2.0, 1.0, 2.0),
    );
    assert!(result.colliding);
    assert!(result.depth > 0.0);
}

#[test]
fn sphere_inside_box_detects_collision() {
    let result = collision::test_sphere_box(
        Vector3::ZERO,
        0.5,
        Vector3::ZERO,
        Vector3::new(2.0, 2.0, 2.0),
    );
    assert!(result.colliding, "Sphere fully inside box should collide");
    assert!(result.depth > 0.0);
}

#[test]
fn box_box_overlap_detects() {
    let result = collision::test_box_box(
        Vector3::ZERO,
        Vector3::new(1.0, 1.0, 1.0),
        Vector3::new(1.5, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 1.0),
    );
    assert!(result.colliding);
    assert!((result.depth - 0.5).abs() < 1e-5);
}

#[test]
fn box_box_no_overlap() {
    let result = collision::test_box_box(
        Vector3::ZERO,
        Vector3::new(1.0, 1.0, 1.0),
        Vector3::new(3.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 1.0),
    );
    assert!(!result.colliding);
}

#[test]
fn collision_dispatch_box_box() {
    let a = Shape3D::BoxShape {
        half_extents: Vector3::new(1.0, 1.0, 1.0),
    };
    let b = Shape3D::BoxShape {
        half_extents: Vector3::new(1.0, 1.0, 1.0),
    };
    let result = collision::test_collision(
        Vector3::ZERO,
        &a,
        Vector3::new(1.0, 0.0, 0.0),
        &b,
    );
    assert!(result.colliding);
}

// ── Sub-stepping (tunneling prevention) ─────────────────────────────

#[test]
fn collision_detected_at_surface_boundary() {
    // Verify sphere-box collision at the surface boundary works
    let r = collision::test_sphere_box(
        Vector3::new(0.0, 0.0, 0.0),
        1.0,
        Vector3::new(0.0, -1.0, 0.0),
        Vector3::new(100.0, 1.0, 100.0),
    );
    assert!(r.colliding, "Sphere at y=0 with radius 1 should collide with box top at y=0");
    assert!(r.depth > 0.0);
}

#[test]
fn move_and_slide_moderate_velocity_hits_floor() {
    // Character at y=1.5 moves down by 2 — sphere enters floor box
    let mut c = CharacterBody3D::new(
        Vector3::new(0.0, 1.5, 0.0),
        Shape3D::Sphere { radius: 1.0 },
    );
    let floor = make_floor(-1.0);
    c.move_and_slide(Vector3::new(0.0, -2.0, 0.0), &[&floor]);
    assert!(c.is_on_floor(), "Should detect floor with moderate velocity");
}

#[test]
fn diagonal_large_velocity_hits_wall() {
    let mut c = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
    let wall = make_wall(5.0);
    let result = c.move_and_slide(Vector3::new(20.0, 0.0, 5.0), &[&wall]);
    assert!(c.is_on_wall(), "Should detect wall even with large velocity");
    assert!(result.x.abs() < 1e-2, "X should be zeroed by wall");
}
