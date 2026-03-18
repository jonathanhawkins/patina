//! Rigid body and kinematic body types.
//!
//! Defines the physics body abstraction used by the physics world. Bodies have
//! a type (static, kinematic, or rigid), a shape, and physical properties
//! like mass, friction, and bounce. Rigid bodies support force/impulse
//! application and velocity integration.

use gdcore::math::Vector2;

use crate::shape::Shape2D;

/// Unique identifier for a physics body within a [`PhysicsWorld2D`](crate::world::PhysicsWorld2D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodyId(pub u64);

/// The type of a physics body, determining how it participates in simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyType {
    /// Does not move. Infinite mass for collision purposes.
    Static,
    /// Moved programmatically; not affected by forces but participates in collisions.
    Kinematic,
    /// Fully simulated: affected by forces, impulses, and collisions.
    Rigid,
}

/// A 2D physics body with position, velocity, shape, and material properties.
#[derive(Debug, Clone)]
pub struct PhysicsBody2D {
    /// Unique identifier.
    pub id: BodyId,
    /// Body type (static, kinematic, rigid).
    pub body_type: BodyType,
    /// World-space position.
    pub position: Vector2,
    /// Rotation in radians.
    pub rotation: f32,
    /// Linear velocity (units per second).
    pub linear_velocity: Vector2,
    /// Angular velocity (radians per second).
    pub angular_velocity: f32,
    /// Collision shape in local space.
    pub shape: Shape2D,
    /// Mass in arbitrary units. Must be > 0 for rigid bodies.
    pub mass: f32,
    /// Friction coefficient in [0, 1].
    pub friction: f32,
    /// Coefficient of restitution (bounciness) in [0, 1].
    pub bounce: f32,
    /// Collision layer bitmask — which layers this body occupies.
    pub collision_layer: u32,
    /// Collision mask bitmask — which layers this body scans for collisions.
    pub collision_mask: u32,
    /// If true, only collide when the other body approaches from the positive normal direction.
    pub one_way_collision: bool,
    /// The direction for one-way collision (default: up, i.e. negative Y).
    pub one_way_direction: Vector2,

    /// Accumulated force for the current frame (reset after integration).
    accumulated_force: Vector2,
}

impl PhysicsBody2D {
    /// Creates a new physics body with the given properties.
    pub fn new(
        id: BodyId,
        body_type: BodyType,
        position: Vector2,
        shape: Shape2D,
        mass: f32,
    ) -> Self {
        Self {
            id,
            body_type,
            position,
            rotation: 0.0,
            linear_velocity: Vector2::ZERO,
            angular_velocity: 0.0,
            shape,
            mass,
            friction: 0.5,
            bounce: 0.0,
            collision_layer: 1,
            collision_mask: 1,
            one_way_collision: false,
            one_way_direction: Vector2::new(0.0, -1.0),
            accumulated_force: Vector2::ZERO,
        }
    }

    /// Applies a continuous force (acceleration = force / mass) for the current step.
    ///
    /// Has no effect on static or kinematic bodies.
    pub fn apply_force(&mut self, force: Vector2) {
        if self.body_type == BodyType::Rigid {
            self.accumulated_force = self.accumulated_force + force;
        }
    }

    /// Applies an instantaneous impulse (directly changes velocity).
    ///
    /// Has no effect on static or kinematic bodies.
    pub fn apply_impulse(&mut self, impulse: Vector2) {
        if self.body_type == BodyType::Rigid && self.mass > 0.0 {
            self.linear_velocity = self.linear_velocity + impulse * (1.0 / self.mass);
        }
    }

    /// Returns the inverse mass (0 for static/kinematic bodies).
    pub fn inverse_mass(&self) -> f32 {
        match self.body_type {
            BodyType::Static | BodyType::Kinematic => 0.0,
            BodyType::Rigid => {
                if self.mass > 0.0 {
                    1.0 / self.mass
                } else {
                    0.0
                }
            }
        }
    }

    /// Integrates velocity and position using semi-implicit Euler.
    ///
    /// Static bodies are not integrated. Kinematic bodies only integrate
    /// position from their current velocity (forces are ignored).
    pub fn integrate(&mut self, dt: f32) {
        match self.body_type {
            BodyType::Static => {}
            BodyType::Kinematic => {
                self.position = self.position + self.linear_velocity * dt;
                self.rotation += self.angular_velocity * dt;
            }
            BodyType::Rigid => {
                if self.mass > 0.0 {
                    let acceleration = self.accumulated_force * (1.0 / self.mass);
                    self.linear_velocity = self.linear_velocity + acceleration * dt;
                }
                self.position = self.position + self.linear_velocity * dt;
                self.rotation += self.angular_velocity * dt;
                self.accumulated_force = Vector2::ZERO;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_integration_moves_position() {
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector2::new(10.0, 0.0);
        body.integrate(1.0);
        assert!((body.position.x - 10.0).abs() < 1e-5);
        assert!((body.position.y).abs() < 1e-5);
    }

    #[test]
    fn apply_impulse_changes_velocity() {
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            2.0, // mass = 2
        );
        body.apply_impulse(Vector2::new(10.0, 0.0));
        // impulse / mass = 10 / 2 = 5
        assert!((body.linear_velocity.x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn static_body_does_not_move() {
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(5.0, 5.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector2::new(100.0, 100.0);
        body.apply_force(Vector2::new(999.0, 999.0));
        body.apply_impulse(Vector2::new(999.0, 999.0));
        body.integrate(1.0);
        assert_eq!(body.position, Vector2::new(5.0, 5.0));
        // Velocity should be unchanged because impulse is ignored for static
        assert_eq!(body.linear_velocity, Vector2::new(100.0, 100.0));
    }

    #[test]
    fn force_accumulation_and_integration() {
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        // Apply gravity-like force for 1 second
        body.apply_force(Vector2::new(0.0, 9.8));
        body.integrate(1.0);
        // velocity should be 9.8 downward, position should be 9.8 downward
        assert!((body.linear_velocity.y - 9.8).abs() < 1e-4);
        assert!((body.position.y - 9.8).abs() < 1e-4);
    }

    #[test]
    fn zero_dt_integration() {
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::new(1.0, 2.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector2::new(100.0, 200.0);
        body.integrate(0.0);
        assert_eq!(body.position, Vector2::new(1.0, 2.0));
    }
}
