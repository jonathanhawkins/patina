//! 3D rigid body and kinematic body types.
//!
//! Defines the 3D physics body abstraction used by the physics world. Bodies have
//! a type (static, kinematic, or rigid), a shape, and physical properties
//! like mass, friction, and bounce. Rigid bodies support force/impulse
//! application and velocity integration.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::shape3d::Shape3D;

/// Unique identifier for a 3D physics body within a [`PhysicsWorld3D`](crate::world3d::PhysicsWorld3D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodyId3D(pub u64);

/// The type of a 3D physics body, determining how it participates in simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyType3D {
    /// Does not move. Infinite mass for collision purposes.
    Static,
    /// Moved programmatically; not affected by forces but participates in collisions.
    Kinematic,
    /// Fully simulated: affected by forces, impulses, and collisions.
    Rigid,
}

/// A 3D physics body with position, velocity, shape, and material properties.
#[derive(Debug, Clone)]
pub struct PhysicsBody3D {
    /// Unique identifier.
    pub id: BodyId3D,
    /// Body type (static, kinematic, rigid).
    pub body_type: BodyType3D,
    /// World-space position.
    pub position: Vector3,
    /// Rotation as a quaternion.
    pub rotation: Quaternion,
    /// Linear velocity (units per second).
    pub linear_velocity: Vector3,
    /// Angular velocity (radians per second, axis-angle representation).
    pub angular_velocity: Vector3,
    /// Collision shape in local space.
    pub shape: Shape3D,
    /// Mass in arbitrary units. Must be > 0 for rigid bodies.
    pub mass: f32,
    /// Friction coefficient in [0, 1].
    pub friction: f32,
    /// Coefficient of restitution (bounciness) in [0, 1].
    pub bounce: f32,

    /// Accumulated force for the current frame (reset after integration).
    accumulated_force: Vector3,
}

impl PhysicsBody3D {
    /// Creates a new 3D physics body with the given properties.
    pub fn new(
        id: BodyId3D,
        body_type: BodyType3D,
        position: Vector3,
        shape: Shape3D,
        mass: f32,
    ) -> Self {
        Self {
            id,
            body_type,
            position,
            rotation: Quaternion::IDENTITY,
            linear_velocity: Vector3::ZERO,
            angular_velocity: Vector3::ZERO,
            shape,
            mass,
            friction: 0.5,
            bounce: 0.0,
            accumulated_force: Vector3::ZERO,
        }
    }

    /// Applies a continuous force (acceleration = force / mass) for the current step.
    ///
    /// Has no effect on static or kinematic bodies.
    pub fn apply_force(&mut self, force: Vector3) {
        if self.body_type == BodyType3D::Rigid {
            self.accumulated_force = self.accumulated_force + force;
        }
    }

    /// Applies an instantaneous impulse (directly changes velocity).
    ///
    /// Has no effect on static or kinematic bodies.
    pub fn apply_impulse(&mut self, impulse: Vector3) {
        if self.body_type == BodyType3D::Rigid && self.mass > 0.0 {
            self.linear_velocity = self.linear_velocity + impulse * (1.0 / self.mass);
        }
    }

    /// Returns the inverse mass (0 for static/kinematic bodies).
    pub fn inverse_mass(&self) -> f32 {
        match self.body_type {
            BodyType3D::Static | BodyType3D::Kinematic => 0.0,
            BodyType3D::Rigid => {
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
            BodyType3D::Static => {}
            BodyType3D::Kinematic => {
                self.position = self.position + self.linear_velocity * dt;
                self.integrate_rotation(dt);
            }
            BodyType3D::Rigid => {
                if self.mass > 0.0 {
                    let acceleration = self.accumulated_force * (1.0 / self.mass);
                    self.linear_velocity = self.linear_velocity + acceleration * dt;
                }
                self.position = self.position + self.linear_velocity * dt;
                self.integrate_rotation(dt);
                self.accumulated_force = Vector3::ZERO;
            }
        }
    }

    /// Integrates angular velocity into the rotation quaternion.
    fn integrate_rotation(&mut self, dt: f32) {
        let av = self.angular_velocity;
        let speed = av.length();
        if speed > 1e-10 {
            let axis = av * (1.0 / speed);
            let angle = speed * dt;
            let dq = Quaternion::from_axis_angle(axis, angle);
            self.rotation = dq * self.rotation;
            self.rotation.normalize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn body3d_integration_moves_position() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        body.integrate(1.0);
        assert!(approx_eq(body.position.x, 10.0));
        assert!(approx_eq(body.position.y, 0.0));
        assert!(approx_eq(body.position.z, 0.0));
    }

    #[test]
    fn apply_impulse_changes_velocity() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            2.0,
        );
        body.apply_impulse(Vector3::new(10.0, 0.0, 0.0));
        assert!(approx_eq(body.linear_velocity.x, 5.0));
    }

    #[test]
    fn static_body_does_not_move() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Static,
            Vector3::new(5.0, 5.0, 5.0),
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector3::new(100.0, 100.0, 100.0);
        body.apply_force(Vector3::new(999.0, 999.0, 999.0));
        body.apply_impulse(Vector3::new(999.0, 999.0, 999.0));
        body.integrate(1.0);
        assert_eq!(body.position, Vector3::new(5.0, 5.0, 5.0));
    }

    #[test]
    fn force_accumulation_and_integration() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        body.apply_force(Vector3::new(0.0, -9.8, 0.0));
        body.integrate(1.0);
        assert!(approx_eq(body.linear_velocity.y, -9.8));
        assert!(approx_eq(body.position.y, -9.8));
    }

    #[test]
    fn zero_dt_integration() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Rigid,
            Vector3::new(1.0, 2.0, 3.0),
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector3::new(100.0, 200.0, 300.0);
        body.integrate(0.0);
        assert_eq!(body.position, Vector3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn angular_velocity_rotates_body() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        body.angular_velocity = Vector3::new(0.0, 1.0, 0.0); // 1 rad/s around Y
        body.integrate(1.0);
        // Rotation should no longer be identity
        assert!(body.rotation != Quaternion::IDENTITY);
        assert!(approx_eq(body.rotation.length(), 1.0));
    }
}
