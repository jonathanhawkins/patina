//! CharacterBody3D — kinematic character controller for 3D.
//!
//! Provides [`CharacterBody3D`] with `move_and_slide` for 3D movement that
//! slides along surfaces and detects floor/wall/ceiling contacts.

use gdcore::math::Vector3;

use crate::body::PhysicsBody3D;
use crate::collision;
use crate::shape::Shape3D;

/// A kinematic character controller that slides along surfaces in 3D.
#[derive(Debug, Clone)]
pub struct CharacterBody3D {
    /// World-space position.
    pub position: Vector3,
    /// Collision shape in local space.
    pub shape: Shape3D,
    /// The up direction for floor/ceiling detection (default: positive Y).
    pub up_direction: Vector3,
    /// Maximum angle (in radians) from up_direction that still counts as floor.
    pub floor_max_angle: f32,
    /// Collision layer bitmask.
    pub collision_layer: u32,
    /// Collision mask bitmask.
    pub collision_mask: u32,

    on_floor: bool,
    on_wall: bool,
    on_ceiling: bool,
    floor_normal: Vector3,
    wall_normal: Vector3,
}

/// Maximum number of slide iterations per `move_and_slide` call.
const MAX_SLIDES: usize = 6;

/// Maximum sub-steps for large motions to prevent tunneling.
const MAX_SUBSTEPS: usize = 32;

/// Floor angle threshold (approximately 45 degrees).
const DEFAULT_FLOOR_MAX_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

impl CharacterBody3D {
    /// Creates a new character body at the given position.
    pub fn new(position: Vector3, shape: Shape3D) -> Self {
        Self {
            position,
            shape,
            up_direction: Vector3::new(0.0, 1.0, 0.0),
            floor_max_angle: DEFAULT_FLOOR_MAX_ANGLE,
            collision_layer: 1,
            collision_mask: 1,
            on_floor: false,
            on_wall: false,
            on_ceiling: false,
            floor_normal: Vector3::ZERO,
            wall_normal: Vector3::ZERO,
        }
    }

    /// Returns the minimum extent of the shape, used for sub-step sizing.
    fn safe_step_distance(&self) -> f32 {
        match &self.shape {
            Shape3D::Sphere { radius } => *radius,
            Shape3D::BoxShape { half_extents } => {
                half_extents.x.min(half_extents.y).min(half_extents.z)
            }
            Shape3D::CapsuleShape { radius, .. } => *radius,
            Shape3D::CylinderShape { radius, height } => radius.min(*height * 0.5),
            _ => 1.0,
        }
    }

    /// Moves the character by `velocity`, sliding along colliding surfaces.
    ///
    /// Returns the resulting velocity after sliding. Populates floor/wall/ceiling
    /// state for queries. Uses sub-stepping for large motions to prevent tunneling.
    pub fn move_and_slide(&mut self, velocity: Vector3, bodies: &[&PhysicsBody3D]) -> Vector3 {
        self.on_floor = false;
        self.on_wall = false;
        self.on_ceiling = false;
        self.floor_normal = Vector3::ZERO;
        self.wall_normal = Vector3::ZERO;

        let speed = velocity.length();
        if speed < 1e-8 {
            return velocity;
        }

        // Subdivide motion if it exceeds the shape's safe step distance.
        let safe_dist = self.safe_step_distance();
        let num_steps = ((speed / safe_dist).ceil() as usize).clamp(1, MAX_SUBSTEPS);
        let inv_steps = 1.0 / num_steps as f32;
        let _step_vel = velocity * inv_steps;

        let mut output_vel = velocity;

        for _ in 0..num_steps {
            // Use the current output_vel direction, scaled to one sub-step.
            let sub_vel = output_vel * inv_steps;
            if sub_vel.length_squared() < 1e-8 {
                break;
            }

            let sub_result = self.slide_step(sub_vel, bodies);

            // If a component was absorbed by collision, zero it in the output.
            if sub_vel.x.abs() > 1e-8 && sub_result.x.abs() < sub_vel.x.abs() * 0.1 {
                output_vel.x = 0.0;
            }
            if sub_vel.y.abs() > 1e-8 && sub_result.y.abs() < sub_vel.y.abs() * 0.1 {
                output_vel.y = 0.0;
            }
            if sub_vel.z.abs() > 1e-8 && sub_result.z.abs() < sub_vel.z.abs() * 0.1 {
                output_vel.z = 0.0;
            }
        }

        output_vel
    }

    /// Performs a single slide step with the given step velocity.
    fn slide_step(&mut self, step_vel: Vector3, bodies: &[&PhysicsBody3D]) -> Vector3 {
        let mut remaining = step_vel;

        for _ in 0..MAX_SLIDES {
            if remaining.length_squared() < 1e-8 {
                break;
            }

            let target = self.position + remaining;

            // Find the deepest collision against all bodies.
            let mut deepest: Option<collision::CollisionResult3D> = None;

            for body in bodies {
                // Layer/mask filtering.
                if (self.collision_mask & body.collision_layer) == 0 {
                    continue;
                }

                let result = collision::test_collision(
                    target,
                    &self.shape,
                    body.position,
                    &body.shape,
                );

                if result.colliding
                    && result.depth > 0.0
                    && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().depth)
                {
                    deepest = Some(result);
                }
            }

            let Some(result) = deepest else {
                // No collision — move freely.
                self.position = target;
                break;
            };

            // The collision normal points from A (character) toward B (body).
            // The surface normal points from the body toward the character.
            let surface_normal = -result.normal;
            self.classify_surface(surface_normal);

            // Separate from the surface.
            self.position = target + surface_normal * result.depth;

            // Slide: remove the velocity component along the surface normal.
            let vel_along_normal = remaining.dot(surface_normal);
            remaining = remaining - surface_normal * vel_along_normal;
        }

        remaining
    }

    /// Moves the character by `motion`, stopping at the first collision.
    ///
    /// Returns `Some(CollisionResult3D)` if a collision occurred, or `None`
    /// if the full motion completed without hitting anything.
    pub fn move_and_collide(
        &mut self,
        motion: Vector3,
        bodies: &[&PhysicsBody3D],
    ) -> Option<collision::CollisionResult3D> {
        let target = self.position + motion;

        let mut deepest: Option<collision::CollisionResult3D> = None;
        for body in bodies {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let result = collision::test_collision(
                target,
                &self.shape,
                body.position,
                &body.shape,
            );
            if result.colliding
                && result.depth > 0.0
                && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().depth)
            {
                deepest = Some(result);
            }
        }

        match deepest {
            None => {
                self.position = target;
                None
            }
            Some(result) => {
                let surface_normal = -result.normal;
                self.position = target + surface_normal * result.depth;
                Some(collision::CollisionResult3D {
                    colliding: true,
                    normal: surface_normal,
                    depth: result.depth,
                })
            }
        }
    }

    /// Classifies a collision normal as floor, wall, or ceiling.
    fn classify_surface(&mut self, normal: Vector3) {
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
    pub fn get_floor_normal(&self) -> Vector3 {
        self.floor_normal
    }

    /// Returns the wall normal from the last `move_and_slide` call.
    pub fn get_wall_normal(&self) -> Vector3 {
        self.wall_normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{BodyId3D, BodyType3D};

    fn make_static_floor(y: f32) -> PhysicsBody3D {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
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

    fn make_static_wall(x: f32) -> PhysicsBody3D {
        let mut body = PhysicsBody3D::new(
            BodyId3D(2),
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

    fn make_static_ceiling(y: f32) -> PhysicsBody3D {
        let mut body = PhysicsBody3D::new(
            BodyId3D(3),
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

    #[test]
    fn move_and_slide_no_collision() {
        let mut character =
            CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let result = character.move_and_slide(Vector3::new(10.0, 0.0, 0.0), &bodies);
        assert!((character.position.x - 10.0).abs() < 1e-4);
        assert!(!character.is_on_floor());
        assert!(!character.is_on_wall());
        assert!((result.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_slide_hits_floor() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        // Floor box centered at y=-1 with half_extent.y=1 => top edge at y=0
        // Character at y=2, moves down by 2.5 => center at y=-0.5
        // Sphere center inside box => collision detected
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];

        let result = character.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(character.is_on_floor(), "Should detect floor");
        assert!(!character.is_on_wall());
        assert!(result.y.abs() < 1e-3, "Y velocity should be zeroed");
    }

    #[test]
    fn move_and_slide_slides_along_wall() {
        let mut character =
            CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        // Wall box centered at x=5, half_extents (1, 100, 100) => left edge at x=4
        // Character moves right by 5 => center at x=5 (inside wall box)
        let wall = make_static_wall(5.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        let result = character.move_and_slide(Vector3::new(5.0, 0.0, 3.0), &bodies);
        assert!(character.is_on_wall(), "Should detect wall");
        assert!(result.x.abs() < 1e-3, "X velocity should be zeroed by wall");
        assert!((result.z - 3.0).abs() < 1e-3, "Z velocity should remain");
    }

    #[test]
    fn move_and_slide_detects_ceiling() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, -2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        // Ceiling box centered at y=1, half_extents (100, 1, 100) => bottom edge at y=0
        // Character at y=-2, moves up by 2.5 => center at y=0.5 (inside ceiling box)
        let ceiling = make_static_ceiling(1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];

        let _result = character.move_and_slide(Vector3::new(0.0, 2.5, 0.0), &bodies);
        assert!(character.is_on_ceiling(), "Should detect ceiling");
        assert!(!character.is_on_floor());
    }

    #[test]
    fn move_and_slide_respects_collision_mask() {
        let mut character =
            CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        character.collision_mask = 2; // Only scan layer 2

        let mut wall = make_static_wall(3.0);
        wall.collision_layer = 1; // Wall is on layer 1 — shouldn't collide
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        let _result = character.move_and_slide(Vector3::new(10.0, 0.0, 0.0), &bodies);
        assert!(!character.is_on_wall());
        assert!((character.position.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_collide_no_collision() {
        let mut character =
            CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let result = character.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &bodies);
        assert!(result.is_none());
        assert!((character.position.x - 5.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_collide_stops_at_wall() {
        let mut character =
            CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        // Wall at x=5 with half_extent 1 => left edge at x=4
        // Move right by 5 => center at x=5, inside the wall box
        let wall = make_static_wall(5.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        let result = character.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &bodies);
        assert!(result.is_some());
    }

    #[test]
    fn get_floor_normal_after_landing() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];

        character.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        let normal = character.get_floor_normal();
        assert!(
            normal.y > 0.5,
            "Floor normal should point up (positive Y), got {:?}",
            normal
        );
    }

    #[test]
    fn zero_velocity_does_not_move() {
        let mut character =
            CharacterBody3D::new(Vector3::new(1.0, 2.0, 3.0), Shape3D::Sphere { radius: 1.0 });
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let result = character.move_and_slide(Vector3::ZERO, &bodies);
        assert!((character.position.x - 1.0).abs() < 1e-6);
        assert!((character.position.y - 2.0).abs() < 1e-6);
        assert!((character.position.z - 3.0).abs() < 1e-6);
        assert!(result.length_squared() < 1e-8);
    }
}
