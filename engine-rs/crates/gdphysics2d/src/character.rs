//! CharacterBody2D — kinematic character controller.
//!
//! Provides [`CharacterBody2D`] with `move_and_slide` for platformer-style
//! movement that slides along surfaces and detects floor/wall/ceiling contacts.

use gdcore::math::{Transform2D, Vector2};

use crate::body::PhysicsBody2D;
#[cfg(test)]
use crate::body::{BodyId, BodyType};
use crate::collision;
use crate::shape::Shape2D;

/// A kinematic character controller that slides along surfaces.
#[derive(Debug, Clone)]
pub struct CharacterBody2D {
    /// World-space position.
    pub position: Vector2,
    /// Collision shape in local space.
    pub shape: Shape2D,
    /// The up direction for floor/ceiling detection (default: negative Y).
    pub up_direction: Vector2,
    /// Maximum angle (in radians) from up_direction that still counts as floor.
    pub floor_max_angle: f32,
    /// Collision layer bitmask.
    pub collision_layer: u32,
    /// Collision mask bitmask.
    pub collision_mask: u32,

    on_floor: bool,
    on_wall: bool,
    on_ceiling: bool,
    floor_normal: Vector2,
    wall_normal: Vector2,
}

/// Maximum number of slide iterations per `move_and_slide` call.
const MAX_SLIDES: usize = 4;

/// Floor angle threshold (approximately 45 degrees).
const DEFAULT_FLOOR_MAX_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

impl CharacterBody2D {
    /// Creates a new character body at the given position.
    pub fn new(position: Vector2, shape: Shape2D) -> Self {
        Self {
            position,
            shape,
            up_direction: Vector2::new(0.0, -1.0),
            floor_max_angle: DEFAULT_FLOOR_MAX_ANGLE,
            collision_layer: 1,
            collision_mask: 1,
            on_floor: false,
            on_wall: false,
            on_ceiling: false,
            floor_normal: Vector2::ZERO,
            wall_normal: Vector2::ZERO,
        }
    }

    /// Moves the character by `velocity * dt`, sliding along colliding surfaces.
    ///
    /// Returns the resulting velocity after sliding. Populates floor/wall/ceiling
    /// state for queries.
    pub fn move_and_slide(&mut self, velocity: Vector2, bodies: &[&PhysicsBody2D]) -> Vector2 {
        self.on_floor = false;
        self.on_wall = false;
        self.on_ceiling = false;
        self.floor_normal = Vector2::ZERO;
        self.wall_normal = Vector2::ZERO;

        let mut remaining_vel = velocity;

        for _ in 0..MAX_SLIDES {
            if remaining_vel.length_squared() < 1e-8 {
                break;
            }

            // Try to move the full remaining velocity
            let target = self.position + remaining_vel;

            // Check collision against all bodies
            let tf_target = Transform2D::translated(target);
            let mut deepest: Option<(collision::CollisionResult, usize)> = None;

            for (idx, body) in bodies.iter().enumerate() {
                // Layer/mask filtering
                if (self.collision_mask & body.collision_layer) == 0 {
                    continue;
                }

                let tf_body = Transform2D::translated(body.position);
                if let Some(result) =
                    collision::test_collision(&self.shape, &tf_target, &body.shape, &tf_body)
                {
                    if result.colliding
                        && result.depth > 0.0
                        && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().0.depth)
                    {
                        deepest = Some((result, idx));
                    }
                }
            }

            let Some((result, _body_idx)) = deepest else {
                // No collision — move freely
                self.position = target;
                break;
            };

            // The collision normal points from A (character) toward B (body).
            // The surface normal points from the body toward the character (opposite).
            let surface_normal = -result.normal;
            self.classify_surface(surface_normal);

            // Separate from the surface (push character away from body)
            self.position = target + surface_normal * result.depth;

            // Slide: remove the velocity component along the surface normal
            let vel_along_normal = remaining_vel.dot(surface_normal);
            remaining_vel = remaining_vel - surface_normal * vel_along_normal;
        }

        remaining_vel
    }

    /// Classifies a collision normal as floor, wall, or ceiling.
    fn classify_surface(&mut self, normal: Vector2) {
        let dot = normal.dot(self.up_direction);
        if dot > self.floor_max_angle.cos() {
            self.on_floor = true;
            self.floor_normal = normal;
        } else if dot < -(self.floor_max_angle.cos()) {
            self.on_ceiling = true;
        } else {
            self.on_wall = true;
            self.wall_normal = normal;
        }
    }

    /// Returns `true` if the character is touching a floor surface.
    pub fn is_on_floor(&self) -> bool {
        self.on_floor
    }

    /// Returns `true` if the character is touching a wall surface.
    pub fn is_on_wall(&self) -> bool {
        self.on_wall
    }

    /// Returns `true` if the character is touching a ceiling surface.
    pub fn is_on_ceiling(&self) -> bool {
        self.on_ceiling
    }

    /// Returns the floor normal from the last `move_and_slide` call.
    pub fn get_floor_normal(&self) -> Vector2 {
        self.floor_normal
    }

    /// Returns the wall normal from the last `move_and_slide` call.
    pub fn get_wall_normal(&self) -> Vector2 {
        self.wall_normal
    }

    /// Moves the character by `motion` and stops at the first collision.
    ///
    /// Unlike `move_and_slide`, this does NOT slide along surfaces. Returns
    /// collision info if a collision occurred, or `None` if the full motion
    /// completed without hitting anything.
    pub fn move_and_collide(
        &mut self,
        motion: Vector2,
        bodies: &[&PhysicsBody2D],
    ) -> Option<KinematicCollision2D> {
        let target = self.position + motion;
        let tf_target = Transform2D::translated(target);

        let mut deepest: Option<(collision::CollisionResult, usize)> = None;

        for (idx, body) in bodies.iter().enumerate() {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }

            let tf_body = Transform2D::translated(body.position);
            if let Some(result) =
                collision::test_collision(&self.shape, &tf_target, &body.shape, &tf_body)
            {
                if result.colliding
                    && result.depth > 0.0
                    && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().0.depth)
                {
                    deepest = Some((result, idx));
                }
            }
        }

        match deepest {
            None => {
                // No collision — move full distance.
                self.position = target;
                None
            }
            Some((result, _body_idx)) => {
                let surface_normal = -result.normal;
                // Push out of the collision.
                self.position = target + surface_normal * result.depth;
                self.classify_surface(surface_normal);
                Some(KinematicCollision2D {
                    normal: surface_normal,
                    depth: result.depth,
                    position: self.position,
                    remainder: Vector2::ZERO, // no sliding
                })
            }
        }
    }
}

/// Collision information returned by [`CharacterBody2D::move_and_collide`].
#[derive(Debug, Clone)]
pub struct KinematicCollision2D {
    /// The surface normal at the collision point (pointing away from the body).
    pub normal: Vector2,
    /// Penetration depth.
    pub depth: f32,
    /// Position of the character after collision resolution.
    pub position: Vector2,
    /// Remaining motion not applied (always zero for move_and_collide).
    pub remainder: Vector2,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_static_floor(y: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(0.0, y),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 1.0),
            },
            1.0,
        )
    }

    fn make_static_wall(x: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(x, 0.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(1.0, 100.0),
            },
            1.0,
        )
    }

    fn make_static_ceiling(y: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(3),
            BodyType::Static,
            Vector2::new(0.0, y),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 1.0),
            },
            1.0,
        )
    }

    #[test]
    fn move_and_slide_no_collision() {
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        let bodies: Vec<&PhysicsBody2D> = vec![];
        let result = character.move_and_slide(Vector2::new(10.0, 0.0), &bodies);
        assert!((character.position.x - 10.0).abs() < 1e-4);
        assert!(!character.is_on_floor());
        assert!(!character.is_on_wall());
        assert!((result.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_slide_hits_floor() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Floor rect centered at y=10.5 with half_extent 1 => top edge at y=9.5
        // Character circle radius 1 moves to y=10 => bottom at y=9 => overlap 0.5
        let floor = make_static_floor(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor];

        let result = character.move_and_slide(Vector2::new(0.0, 10.0), &bodies);
        assert!(character.is_on_floor(), "Should detect floor");
        assert!(!character.is_on_wall());
        assert!(result.y.abs() < 1e-3, "Y velocity should be zeroed");
    }

    #[test]
    fn move_and_slide_slides_along_wall() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Wall at x=10.5 (left edge at x=9.5). Character moves to x=10, overlap 0.5
        let wall = make_static_wall(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&wall];

        let result = character.move_and_slide(Vector2::new(10.0, 5.0), &bodies);
        assert!(character.is_on_wall(), "Should detect wall");
        assert!(result.x.abs() < 1e-3, "X velocity should be zeroed by wall");
        assert!((result.y - 5.0).abs() < 1e-3, "Y velocity should remain");
    }

    #[test]
    fn move_and_slide_detects_ceiling() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Ceiling at y=-10.5 (bottom edge at y=-9.5). Character moves to y=-10, overlap 0.5
        let ceiling = make_static_ceiling(-10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&ceiling];

        let _result = character.move_and_slide(Vector2::new(0.0, -10.0), &bodies);
        assert!(character.is_on_ceiling(), "Should detect ceiling");
        assert!(!character.is_on_floor());
    }

    #[test]
    fn move_and_slide_respects_collision_mask() {
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        character.collision_mask = 2; // Only scan layer 2

        let mut wall = make_static_wall(3.0);
        wall.collision_layer = 1; // Wall is on layer 1 — shouldn't collide
        let bodies: Vec<&PhysicsBody2D> = vec![&wall];

        let _result = character.move_and_slide(Vector2::new(10.0, 0.0), &bodies);
        assert!(!character.is_on_wall());
        assert!((character.position.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn get_floor_normal_returns_correct_value() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Floor at y=10.5, character moves to y=10
        let floor = make_static_floor(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor];

        character.move_and_slide(Vector2::new(0.0, 10.0), &bodies);
        let normal = character.get_floor_normal();
        assert!(
            normal.y < -0.5,
            "Floor normal should point up (negative Y), got {:?}",
            normal
        );
    }
}
