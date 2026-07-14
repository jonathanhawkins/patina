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
    /// Multiplies the world gravity applied to this body (Godot `gravity_scale`).
    ///
    /// Defaults to `1.0`. A value of `0.0` makes the body ignore gravity; `2.0`
    /// makes it fall twice as fast. Only affects [`BodyType::Rigid`] bodies.
    pub gravity_scale: f32,
    /// Linear damping coefficient (Godot `linear_damp`). Defaults to `0.0`.
    ///
    /// Each step the linear velocity is multiplied by `max(1 - dt * linear_damp, 0)`,
    /// matching Godot's `GodotBody2D::integrate_forces`.
    pub linear_damp: f32,
    /// Angular damping coefficient (Godot `angular_damp`). Defaults to `0.0`.
    ///
    /// Each step the angular velocity is multiplied by `max(1 - dt * angular_damp, 0)`.
    pub angular_damp: f32,

    /// Accumulated force for the current frame (reset after integration).
    accumulated_force: Vector2,
    /// Accumulated torque for the current frame (reset after integration).
    accumulated_torque: f32,
    /// Local-space center of mass offset (Godot `center_of_mass`).
    ///
    /// Positioned impulses/forces produce torque about this point. Defaults to
    /// the body origin `(0, 0)`.
    pub center_of_mass: Vector2,
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
            gravity_scale: 1.0,
            linear_damp: 0.0,
            angular_damp: 0.0,
            accumulated_force: Vector2::ZERO,
            accumulated_torque: 0.0,
            center_of_mass: Vector2::ZERO,
        }
    }

    /// Sets the gravity scale multiplier (Godot `gravity_scale`) for this body.
    pub fn set_gravity_scale(&mut self, scale: f32) {
        self.gravity_scale = scale;
    }

    /// Sets the linear damping coefficient (Godot `linear_damp`) for this body.
    pub fn set_linear_damp(&mut self, damp: f32) {
        self.linear_damp = damp;
    }

    /// Sets the angular damping coefficient (Godot `angular_damp`) for this body.
    pub fn set_angular_damp(&mut self, damp: f32) {
        self.angular_damp = damp;
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

    /// Returns the body's moment of inertia about its center of mass, derived
    /// from its shape and mass (matching Godot's automatic inertia computation
    /// for a `RigidBody2D` that does not override `inertia`).
    ///
    /// Clamped to a small positive value so [`inverse_inertia`] never diverges
    /// for degenerate shapes.
    ///
    /// [`inverse_inertia`]: Self::inverse_inertia
    pub fn moment_of_inertia(&self) -> f32 {
        let m = self.mass.max(0.0);
        let i = match &self.shape {
            Shape2D::Circle { radius } => 0.5 * m * radius * radius,
            Shape2D::Rectangle { half_extents } => {
                // I = m (w^2 + h^2) / 12 with w = 2*hx, h = 2*hy.
                m * (half_extents.x * half_extents.x + half_extents.y * half_extents.y) / 3.0
            }
            Shape2D::Capsule { radius, height } => {
                // Central rod (length `height`) plus the end-cap disc.
                m * height * height / 12.0 + 0.5 * m * radius * radius
            }
            Shape2D::Segment { a, b } => {
                let len = (*b - *a).length();
                m * len * len / 12.0
            }
            _ => {
                // Polygon / world-boundary / separation-ray: approximate with a
                // disc of the shape's bounding radius (0 for unbounded shapes).
                let br2 = self
                    .shape
                    .polygon_points()
                    .iter()
                    .map(|p| p.x * p.x + p.y * p.y)
                    .fold(0.0_f32, f32::max);
                0.5 * m * br2
            }
        };
        i.max(1e-4)
    }

    /// Returns the inverse moment of inertia (0 for static/kinematic bodies).
    pub fn inverse_inertia(&self) -> f32 {
        match self.body_type {
            BodyType::Static | BodyType::Kinematic => 0.0,
            BodyType::Rigid => {
                let i = self.moment_of_inertia();
                if i > 0.0 {
                    1.0 / i
                } else {
                    0.0
                }
            }
        }
    }

    /// Applies a continuous torque (angular acceleration = torque / inertia) for
    /// the current step. Has no effect on non-rigid bodies.
    pub fn apply_torque(&mut self, torque: f32) {
        if self.body_type == BodyType::Rigid {
            self.accumulated_torque += torque;
        }
    }

    /// Applies an instantaneous angular impulse: directly changes the angular
    /// velocity by `torque_impulse * inverse_inertia`.
    pub fn apply_torque_impulse(&mut self, torque_impulse: f32) {
        if self.body_type == BodyType::Rigid {
            let inv_i = self.inverse_inertia();
            self.angular_velocity += torque_impulse * inv_i;
        }
    }

    /// Applies an instantaneous impulse at a point offset from the body origin,
    /// producing both a linear velocity change and an angular velocity change
    /// about the center of mass (Godot `apply_impulse(impulse, position)`).
    ///
    /// `point` is relative to the body's position (its origin).
    pub fn apply_impulse_at_point(&mut self, impulse: Vector2, point: Vector2) {
        if self.body_type != BodyType::Rigid {
            return;
        }
        self.apply_impulse(impulse);
        let r = point - self.center_of_mass;
        let torque_impulse = r.x * impulse.y - r.y * impulse.x;
        let inv_i = self.inverse_inertia();
        self.angular_velocity += torque_impulse * inv_i;
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
    /// `gravity` is the world gravity vector in px/s². It is applied to dynamic
    /// ([`BodyType::Rigid`]) bodies only, scaled by this body's [`gravity_scale`].
    ///
    /// The order matches Godot's `GodotBody2D::integrate_forces`: linear and
    /// angular damping are applied to the velocity *first*, then gravity plus the
    /// accumulated force are added as acceleration. Because gravity contributes
    /// `inv_mass * (gravity * mass) == gravity`, it acts as a pure acceleration
    /// independent of mass (as in Godot).
    ///
    /// Static bodies are not integrated. Kinematic bodies only integrate
    /// position from their current velocity (forces, gravity, and damping are
    /// ignored).
    ///
    /// [`gravity_scale`]: Self::gravity_scale
    pub fn integrate(&mut self, dt: f32, gravity: Vector2) {
        match self.body_type {
            BodyType::Static => {}
            BodyType::Kinematic => {
                self.position = self.position + self.linear_velocity * dt;
                self.rotation += self.angular_velocity * dt;
            }
            BodyType::Rigid => {
                // Apply damping to the existing velocity before integrating
                // forces (Godot: `linear_velocity *= max(1 - dt*damp, 0)`).
                let linear_damp_factor = (1.0 - dt * self.linear_damp).max(0.0);
                let angular_damp_factor = (1.0 - dt * self.angular_damp).max(0.0);
                self.linear_velocity = self.linear_velocity * linear_damp_factor;
                self.angular_velocity *= angular_damp_factor;

                if self.mass > 0.0 {
                    // gravity acts as a pure acceleration (scaled per body); the
                    // accumulated force is divided by mass.
                    let acceleration =
                        gravity * self.gravity_scale + self.accumulated_force * (1.0 / self.mass);
                    self.linear_velocity = self.linear_velocity + acceleration * dt;
                    // Angular acceleration from accumulated torque (τ / I).
                    let inv_i = self.inverse_inertia();
                    self.angular_velocity += self.accumulated_torque * inv_i * dt;
                }
                self.position = self.position + self.linear_velocity * dt;
                self.rotation += self.angular_velocity * dt;
                self.accumulated_force = Vector2::ZERO;
                self.accumulated_torque = 0.0;
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
        body.integrate(1.0, Vector2::ZERO);
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
        // Non-zero gravity must be ignored by static bodies.
        body.integrate(1.0, Vector2::new(0.0, 980.0));
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
        body.integrate(1.0, Vector2::ZERO);
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
        body.integrate(0.0, Vector2::ZERO);
        assert_eq!(body.position, Vector2::new(1.0, 2.0));
    }

    #[test]
    fn gravity_scale_and_damp_defaults() {
        let body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        assert_eq!(body.gravity_scale, 1.0);
        assert_eq!(body.linear_damp, 0.0);
        assert_eq!(body.angular_damp, 0.0);
    }

    #[test]
    fn gravity_applied_as_acceleration_independent_of_mass() {
        // Two rigid bodies with different masses fall the same distance under
        // the same gravity, because gravity is a pure acceleration.
        let g = Vector2::new(0.0, 980.0);
        let mut light = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        let mut heavy = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            10.0,
        );
        for _ in 0..10 {
            light.integrate(1.0 / 60.0, g);
            heavy.integrate(1.0 / 60.0, g);
        }
        assert!((light.position.y - heavy.position.y).abs() < 1e-4);
        assert!(light.position.y > 0.0, "body should fall in +Y");
    }

    #[test]
    fn gravity_scale_zero_ignores_gravity() {
        let g = Vector2::new(0.0, 980.0);
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        body.set_gravity_scale(0.0);
        for _ in 0..10 {
            body.integrate(1.0 / 60.0, g);
        }
        assert!(body.position.y.abs() < 1e-6, "gravity_scale=0 must not fall");
        assert!(body.linear_velocity.y.abs() < 1e-6);
    }

    #[test]
    fn linear_damp_decays_velocity_each_step() {
        // With zero gravity and damping d, velocity is multiplied by
        // (1 - dt*d) each step.
        let dt = 1.0 / 60.0;
        let d = 2.0_f32;
        let mut body = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        body.linear_velocity = Vector2::new(100.0, 0.0);
        body.set_linear_damp(d);
        let steps = 60;
        for _ in 0..steps {
            body.integrate(dt, Vector2::ZERO);
        }
        let expected = 100.0 * (1.0 - dt * d).powi(steps);
        assert!(
            (body.linear_velocity.x - expected).abs() < 1e-2,
            "damped velocity {} != expected {}",
            body.linear_velocity.x,
            expected
        );
        assert!(body.linear_velocity.x < 100.0, "velocity must decay");
    }

    fn rigid_disc(mass: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 2.0 },
            mass,
        )
    }

    #[test]
    fn moment_of_inertia_positive_for_rigid_shapes() {
        assert!(rigid_disc(1.0).moment_of_inertia() > 0.0);
        assert!(rigid_disc(1.0).inverse_inertia() > 0.0);
        // Static/kinematic bodies have zero inverse inertia (no rotation from torque).
        let mut stat = rigid_disc(1.0);
        stat.body_type = BodyType::Static;
        assert_eq!(stat.inverse_inertia(), 0.0);
    }

    #[test]
    fn apply_torque_spins_body_through_integration() {
        let mut body = rigid_disc(1.0);
        body.apply_torque(50.0);
        body.integrate(1.0 / 60.0, Vector2::ZERO);
        assert!(
            body.angular_velocity > 0.0,
            "positive torque must produce positive angular velocity, got {}",
            body.angular_velocity
        );
        assert!(body.rotation > 0.0, "body should have rotated");
        // Torque is cleared after integration (like force).
        let omega = body.angular_velocity;
        body.integrate(1.0 / 60.0, Vector2::ZERO);
        assert!(
            (body.angular_velocity - omega).abs() < 1e-6,
            "no residual torque should remain after integration"
        );
    }

    #[test]
    fn apply_torque_impulse_changes_angular_velocity_immediately() {
        let mut body = rigid_disc(1.0);
        body.apply_torque_impulse(10.0);
        assert!(
            body.angular_velocity > 0.0,
            "torque impulse must change angular velocity without integration"
        );
    }

    #[test]
    fn off_center_impulse_imparts_spin_centered_does_not() {
        // Impulse applied off-center produces angular velocity.
        let mut spun = rigid_disc(1.0);
        spun.apply_impulse_at_point(Vector2::new(10.0, 0.0), Vector2::new(0.0, 2.0));
        assert!(
            spun.angular_velocity.abs() > 1e-4,
            "off-center impulse must impart spin, got {}",
            spun.angular_velocity
        );
        assert!(
            spun.linear_velocity.x > 0.0,
            "off-center impulse still changes linear velocity"
        );

        // Impulse through the center of mass produces no spin.
        let mut straight = rigid_disc(1.0);
        straight.apply_impulse_at_point(Vector2::new(10.0, 0.0), Vector2::ZERO);
        assert!(
            straight.angular_velocity.abs() < 1e-6,
            "central impulse must not spin the body, got {}",
            straight.angular_velocity
        );
    }
}
