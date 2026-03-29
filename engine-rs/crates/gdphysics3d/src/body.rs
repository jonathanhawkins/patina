//! 3D rigid body and kinematic body types.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::shape::Shape3D;

/// Unique identifier for a 3D physics body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId3D(pub u64);

/// The type of a 3D physics body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyType3D {
    /// Does not move. Infinite mass for collision purposes.
    Static,
    /// Moved programmatically; not affected by forces.
    Kinematic,
    /// Fully simulated: affected by forces, impulses, and collisions.
    Rigid,
}

/// Freeze mode for a rigid body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FreezeMode {
    /// Body is fully simulated (default).
    #[default]
    None,
    /// Body is frozen as static (infinite mass, no movement).
    Static,
    /// Body is frozen as kinematic (can be moved programmatically).
    Kinematic,
}

/// A contact point between two physics bodies.
#[derive(Debug, Clone, Copy)]
pub struct ContactPoint3D {
    /// World-space position of the contact.
    pub position: Vector3,
    /// Contact normal (from this body toward the other).
    pub normal: Vector3,
    /// Penetration depth.
    pub depth: f32,
    /// ID of the other body involved in the contact.
    pub other_body: BodyId3D,
}

/// A 3D physics body with position, velocity, shape, and material properties.
#[derive(Debug, Clone)]
pub struct PhysicsBody3D {
    /// Unique identifier.
    pub id: BodyId3D,
    /// Body type.
    pub body_type: BodyType3D,
    /// World-space position.
    pub position: Vector3,
    /// Rotation as a quaternion.
    pub rotation: Quaternion,
    /// Linear velocity (units per second).
    pub linear_velocity: Vector3,
    /// Angular velocity (radians per second, axis-angle).
    pub angular_velocity: Vector3,
    /// Collision shape in local space.
    pub shape: Shape3D,
    /// Mass in arbitrary units.
    pub mass: f32,
    /// Friction coefficient in [0, 1].
    pub friction: f32,
    /// Coefficient of restitution (bounciness) in [0, 1].
    pub bounce: f32,
    /// Gravity scale multiplier (default 1.0).
    pub gravity_scale: f32,
    /// Linear damping factor.
    pub linear_damp: f32,
    /// Angular damping factor.
    pub angular_damp: f32,
    /// Whether continuous collision detection is enabled.
    pub continuous_cd: bool,
    /// Whether the body can sleep when at rest.
    pub can_sleep: bool,
    /// Whether the body is currently sleeping.
    pub sleeping: bool,
    /// Freeze mode (None = active, Static/Kinematic = frozen).
    pub freeze_mode: FreezeMode,
    /// Whether contact monitoring is enabled.
    pub contact_monitor: bool,
    /// Maximum number of contacts to report.
    pub max_contacts_reported: usize,
    /// Collision layer bitmask.
    pub collision_layer: u32,
    /// Accumulated force for the current frame.
    accumulated_force: Vector3,
    /// Accumulated torque for the current frame.
    accumulated_torque: Vector3,
    /// Contact points from the last physics step.
    contacts: Vec<ContactPoint3D>,
}

impl PhysicsBody3D {
    /// Creates a new 3D physics body.
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
            gravity_scale: 1.0,
            linear_damp: 0.0,
            angular_damp: 0.0,
            continuous_cd: false,
            can_sleep: true,
            sleeping: false,
            freeze_mode: FreezeMode::None,
            contact_monitor: false,
            max_contacts_reported: 0,
            collision_layer: 1,
            accumulated_force: Vector3::ZERO,
            accumulated_torque: Vector3::ZERO,
            contacts: Vec::new(),
        }
    }

    // -- Force API ---------------------------------------------------------

    /// Applies a central force to a rigid body (accumulated until integration).
    pub fn apply_force(&mut self, force: Vector3) {
        if self.is_active_rigid() {
            self.accumulated_force = self.accumulated_force + force;
            self.wake_up();
        }
    }

    /// Alias for `apply_force` matching Godot's `apply_central_force`.
    pub fn apply_central_force(&mut self, force: Vector3) {
        self.apply_force(force);
    }

    /// Applies a force at a world-space position, generating both linear
    /// force and torque.
    pub fn apply_force_at_position(&mut self, force: Vector3, position: Vector3) {
        if self.is_active_rigid() {
            self.accumulated_force = self.accumulated_force + force;
            let offset = position - self.position;
            self.accumulated_torque = self.accumulated_torque + offset.cross(force);
            self.wake_up();
        }
    }

    /// Applies an instantaneous central impulse to a rigid body.
    pub fn apply_impulse(&mut self, impulse: Vector3) {
        if self.is_active_rigid() && self.mass > 0.0 {
            self.linear_velocity = self.linear_velocity + impulse * (1.0 / self.mass);
            self.wake_up();
        }
    }

    /// Alias for `apply_impulse` matching Godot's `apply_central_impulse`.
    pub fn apply_central_impulse(&mut self, impulse: Vector3) {
        self.apply_impulse(impulse);
    }

    /// Sets velocity along a single axis, preserving other axes.
    ///
    /// Matches Godot's `set_axis_velocity`. The axis is determined by the
    /// direction of `axis_velocity`; velocity along that axis is replaced.
    pub fn set_axis_velocity(&mut self, axis_velocity: Vector3) {
        let axis = axis_velocity.normalized();
        let len_sq = axis.length_squared();
        if len_sq < 1e-8 {
            return;
        }
        // Remove existing velocity component along this axis, then add the new one.
        let existing = self.linear_velocity.dot(axis);
        self.linear_velocity = self.linear_velocity - axis * existing + axis_velocity;
    }

    /// Applies an impulse at a world-space position, affecting both linear
    /// and angular velocity.
    pub fn apply_impulse_at_position(&mut self, impulse: Vector3, position: Vector3) {
        if self.is_active_rigid() && self.mass > 0.0 {
            self.linear_velocity = self.linear_velocity + impulse * (1.0 / self.mass);
            let offset = position - self.position;
            let torque_impulse = offset.cross(impulse);
            // Simplified: assume unit inertia tensor for now
            self.angular_velocity = self.angular_velocity + torque_impulse * (1.0 / self.mass);
            self.wake_up();
        }
    }

    // -- Torque API --------------------------------------------------------

    /// Applies a torque (accumulated until integration).
    pub fn apply_torque(&mut self, torque: Vector3) {
        if self.is_active_rigid() {
            self.accumulated_torque = self.accumulated_torque + torque;
            self.wake_up();
        }
    }

    /// Applies an instantaneous torque impulse.
    pub fn apply_torque_impulse(&mut self, impulse: Vector3) {
        if self.is_active_rigid() && self.mass > 0.0 {
            // Simplified: assume unit inertia tensor
            self.angular_velocity = self.angular_velocity + impulse * (1.0 / self.mass);
            self.wake_up();
        }
    }

    // -- Integration -------------------------------------------------------

    /// Integrates velocity and position for one time step.
    pub fn integrate(&mut self, dt: f32, gravity: Vector3) {
        if self.body_type != BodyType3D::Rigid || self.freeze_mode != FreezeMode::None {
            return;
        }
        if self.sleeping {
            return;
        }

        // Apply gravity (scaled) and accumulated forces.
        let scaled_gravity = gravity * self.gravity_scale;
        let inv_mass = 1.0 / self.mass.max(f32::EPSILON);
        let accel = scaled_gravity + self.accumulated_force * inv_mass;
        self.linear_velocity = self.linear_velocity + accel * dt;

        // Apply linear damping
        if self.linear_damp > 0.0 {
            let damp = (1.0 - self.linear_damp * dt).max(0.0);
            self.linear_velocity = self.linear_velocity * damp;
        }

        self.position = self.position + self.linear_velocity * dt;

        // Angular integration
        let angular_accel = self.accumulated_torque * inv_mass;
        self.angular_velocity = self.angular_velocity + angular_accel * dt;

        // Apply angular damping
        if self.angular_damp > 0.0 {
            let damp = (1.0 - self.angular_damp * dt).max(0.0);
            self.angular_velocity = self.angular_velocity * damp;
        }

        // Integrate rotation (simplified: small-angle approximation)
        if self.angular_velocity.length_squared() > 1e-12 {
            let half_dt = dt * 0.5;
            let dq = Quaternion::new(
                self.angular_velocity.x * half_dt,
                self.angular_velocity.y * half_dt,
                self.angular_velocity.z * half_dt,
                1.0,
            );
            self.rotation = (self.rotation * dq).normalized();
        }

        self.accumulated_force = Vector3::ZERO;
        self.accumulated_torque = Vector3::ZERO;

        // Auto-sleep check
        if self.can_sleep {
            let linear_threshold = 0.01;
            let angular_threshold = 0.01;
            if self.linear_velocity.length_squared() < linear_threshold
                && self.angular_velocity.length_squared() < angular_threshold
            {
                self.sleeping = true;
            }
        }
    }

    // -- Sleep API ---------------------------------------------------------

    /// Wakes the body from sleep.
    pub fn wake_up(&mut self) {
        self.sleeping = false;
    }

    /// Puts the body to sleep.
    pub fn put_to_sleep(&mut self) {
        if self.can_sleep {
            self.sleeping = true;
            self.linear_velocity = Vector3::ZERO;
            self.angular_velocity = Vector3::ZERO;
        }
    }

    // -- Contact API -------------------------------------------------------

    /// Records a contact point (called by the physics world during stepping).
    pub fn add_contact(&mut self, contact: ContactPoint3D) {
        if self.contact_monitor && self.contacts.len() < self.max_contacts_reported {
            self.contacts.push(contact);
        }
    }

    /// Returns the contacts from the last physics step.
    pub fn get_contacts(&self) -> &[ContactPoint3D] {
        &self.contacts
    }

    /// Returns the number of contacts from the last physics step.
    pub fn get_contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Returns the unique body IDs of all bodies currently in contact.
    pub fn get_colliding_bodies(&self) -> Vec<BodyId3D> {
        let mut ids: Vec<BodyId3D> = self.contacts.iter().map(|c| c.other_body).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Clears recorded contacts (called at the start of each step).
    pub fn clear_contacts(&mut self) {
        self.contacts.clear();
    }

    /// Sets the linear velocity directly.
    pub fn set_linear_velocity(&mut self, velocity: Vector3) {
        self.linear_velocity = velocity;
        if velocity.length_squared() > 1e-8 {
            self.wake_up();
        }
    }

    /// Sets the angular velocity directly.
    pub fn set_angular_velocity(&mut self, velocity: Vector3) {
        self.angular_velocity = velocity;
        if velocity.length_squared() > 1e-8 {
            self.wake_up();
        }
    }

    // -- Query API ---------------------------------------------------------

    /// Returns the inverse mass (0 for static/kinematic bodies).
    pub fn inverse_mass(&self) -> f32 {
        match self.body_type {
            BodyType3D::Rigid if self.mass > 0.0 => 1.0 / self.mass,
            _ => 0.0,
        }
    }

    /// Returns `true` if this is a rigid body that is not frozen.
    fn is_active_rigid(&self) -> bool {
        self.body_type == BodyType3D::Rigid && self.freeze_mode == FreezeMode::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rigid(pos: Vector3) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        )
    }

    #[test]
    fn body_creation_defaults() {
        let body = make_rigid(Vector3::ZERO);
        assert_eq!(body.body_type, BodyType3D::Rigid);
        assert_eq!(body.linear_velocity, Vector3::ZERO);
        assert_eq!(body.rotation, Quaternion::IDENTITY);
        assert!((body.friction - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_impulse_changes_velocity() {
        let mut body = make_rigid(Vector3::ZERO);
        body.apply_impulse(Vector3::new(10.0, 0.0, 0.0));
        assert!((body.linear_velocity.x - 10.0).abs() < 1e-5);
    }

    #[test]
    fn static_body_ignores_force() {
        let mut body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        body.apply_force(Vector3::new(100.0, 0.0, 0.0));
        body.integrate(1.0 / 60.0, Vector3::ZERO);
        assert_eq!(body.position, Vector3::ZERO);
    }

    #[test]
    fn gravity_integration() {
        let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
        let gravity = Vector3::new(0.0, -9.8, 0.0);
        body.integrate(1.0, gravity);
        assert!(body.position.y < 10.0, "body should fall under gravity");
        assert!(body.linear_velocity.y < 0.0, "velocity should be negative");
    }

    #[test]
    fn inverse_mass_static_is_zero() {
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        assert!(body.inverse_mass().abs() < f32::EPSILON);
    }

    #[test]
    fn inverse_mass_rigid() {
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            2.0,
        );
        assert!((body.inverse_mass() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_central_impulse_alias() {
        let mut body = make_rigid(Vector3::ZERO);
        body.apply_central_impulse(Vector3::new(5.0, 0.0, 0.0));
        assert!((body.linear_velocity.x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn set_axis_velocity_replaces_component() {
        let mut body = make_rigid(Vector3::ZERO);
        body.linear_velocity = Vector3::new(3.0, 4.0, 0.0);
        // Replace Y component only
        body.set_axis_velocity(Vector3::new(0.0, -10.0, 0.0));
        assert!((body.linear_velocity.x - 3.0).abs() < 1e-5);
        assert!((body.linear_velocity.y - (-10.0)).abs() < 1e-5);
    }

    #[test]
    fn set_axis_velocity_zero_is_noop() {
        let mut body = make_rigid(Vector3::ZERO);
        body.linear_velocity = Vector3::new(1.0, 2.0, 3.0);
        body.set_axis_velocity(Vector3::ZERO);
        assert!((body.linear_velocity.x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn set_linear_velocity_wakes_body() {
        let mut body = make_rigid(Vector3::ZERO);
        body.put_to_sleep();
        assert!(body.sleeping);
        body.set_linear_velocity(Vector3::new(1.0, 0.0, 0.0));
        assert!(!body.sleeping);
        assert!((body.linear_velocity.x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn set_angular_velocity_wakes_body() {
        let mut body = make_rigid(Vector3::ZERO);
        body.put_to_sleep();
        assert!(body.sleeping);
        body.set_angular_velocity(Vector3::new(0.0, 1.0, 0.0));
        assert!(!body.sleeping);
        assert!((body.angular_velocity.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn get_colliding_bodies_empty() {
        let body = make_rigid(Vector3::ZERO);
        assert!(body.get_colliding_bodies().is_empty());
    }

    #[test]
    fn get_colliding_bodies_deduplicates() {
        let mut body = make_rigid(Vector3::ZERO);
        body.contact_monitor = true;
        body.max_contacts_reported = 10;
        body.add_contact(ContactPoint3D {
            position: Vector3::ZERO,
            normal: Vector3::new(0.0, 1.0, 0.0),
            depth: 0.1,
            other_body: BodyId3D(5),
        });
        body.add_contact(ContactPoint3D {
            position: Vector3::new(1.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            depth: 0.2,
            other_body: BodyId3D(5),
        });
        body.add_contact(ContactPoint3D {
            position: Vector3::ZERO,
            normal: Vector3::new(1.0, 0.0, 0.0),
            depth: 0.1,
            other_body: BodyId3D(7),
        });
        let colliders = body.get_colliding_bodies();
        assert_eq!(colliders.len(), 2);
        assert_eq!(colliders[0], BodyId3D(5));
        assert_eq!(colliders[1], BodyId3D(7));
    }

    #[test]
    fn apply_force_at_position_generates_torque() {
        let mut body = make_rigid(Vector3::ZERO);
        // Force at offset should generate torque via cross product.
        body.apply_force_at_position(Vector3::new(0.0, 10.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        body.integrate(1.0, Vector3::ZERO);
        // Cross(1,0,0) x (0,10,0) = (0,0,10) => angular_velocity.z should be nonzero
        assert!(body.angular_velocity.z.abs() > 0.1);
    }

    #[test]
    fn frozen_body_does_not_integrate() {
        let mut body = make_rigid(Vector3::ZERO);
        body.freeze_mode = FreezeMode::Static;
        body.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        body.integrate(1.0, Vector3::ZERO);
        assert_eq!(body.position, Vector3::ZERO);
    }

    #[test]
    fn linear_damping() {
        let mut body = make_rigid(Vector3::ZERO);
        body.linear_damp = 0.5;
        body.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        body.integrate(1.0, Vector3::ZERO);
        // After 1s with 0.5 damp: vel *= (1 - 0.5*1) = 0.5, so vel.x ~ 5
        assert!(body.linear_velocity.x < 10.0);
        assert!(body.linear_velocity.x > 0.0);
    }

    #[test]
    fn bounce_property_default() {
        let body = make_rigid(Vector3::ZERO);
        assert!((body.bounce - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn contact_monitor_respects_max() {
        let mut body = make_rigid(Vector3::ZERO);
        body.contact_monitor = true;
        body.max_contacts_reported = 2;
        for i in 0..5 {
            body.add_contact(ContactPoint3D {
                position: Vector3::ZERO,
                normal: Vector3::new(0.0, 1.0, 0.0),
                depth: 0.1,
                other_body: BodyId3D(i),
            });
        }
        assert_eq!(body.get_contact_count(), 2);
    }

    #[test]
    fn sleeping_body_does_not_integrate() {
        let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
        body.put_to_sleep();
        body.integrate(1.0, Vector3::new(0.0, -9.8, 0.0));
        assert!((body.position.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn apply_impulse_at_position_changes_angular() {
        let mut body = make_rigid(Vector3::ZERO);
        body.apply_impulse_at_position(Vector3::new(0.0, 0.0, 10.0), Vector3::new(1.0, 0.0, 0.0));
        // Cross (1,0,0) x (0,0,10) = (0,-10,0)? No: offset.cross(impulse)
        // (1,0,0).cross(0,0,10) = (0*10-0*0, 0*0-1*10, 1*0-0*0) = (0,-10,0)
        assert!(body.angular_velocity.y.abs() > 1.0);
    }

    #[test]
    fn gravity_scale() {
        let mut body_normal = make_rigid(Vector3::new(0.0, 10.0, 0.0));
        let mut body_scaled = make_rigid(Vector3::new(0.0, 10.0, 0.0));
        body_scaled.gravity_scale = 2.0;
        let gravity = Vector3::new(0.0, -9.8, 0.0);
        body_normal.integrate(1.0, gravity);
        body_scaled.integrate(1.0, gravity);
        assert!(body_scaled.position.y < body_normal.position.y);
    }
}
