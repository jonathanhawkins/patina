//! VehicleBody3D / VehicleWheel3D arcade-to-sim vehicle physics.
//!
//! Implements Godot's VehicleBody3D model: a rigid-body chassis with
//! configurable VehicleWheel3D attachments providing suspension springs,
//! tire friction, steering, and engine/brake drive force.
//!
//! The vehicle is self-contained — it owns a [`PhysicsBody3D`] chassis and
//! steps it directly against a flat ground plane. Wheel raycasts hit this
//! plane, apply spring forces upward on the chassis, and drive/brake/lateral
//! friction forces act through the chassis center.
//!
//! Matches Godot's API shape:
//! - `engine_force`, `brake`, `steering` are user inputs
//! - Each wheel has suspension (rest length, travel, stiffness, damping),
//!   friction, and per-wheel `driven` / `steerable` flags
//! - Steering produces yaw via a bicycle-model turn rate

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
use crate::shape3d::Shape3D;

/// Unique identifier for a vehicle in a vehicle simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VehicleBodyId3D(pub u64);

/// Unique identifier for a wheel within a [`VehicleBody3D`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VehicleWheelId3D(pub u32);

/// A single VehicleWheel3D — suspension + tire attached to a chassis.
///
/// Mirrors Godot's `VehicleWheel3D`. Attachment point is in chassis local
/// space; the wheel casts a ray downward (chassis local `-Y`) and applies a
/// suspension spring force if it contacts ground within `suspension_travel`.
#[derive(Debug, Clone, Copy)]
pub struct VehicleWheel3D {
    /// Attachment point in chassis local space (where the suspension top mounts).
    pub attachment_local: Vector3,
    /// Wheel radius.
    pub radius: f32,
    /// Rest length of the suspension (uncompressed distance from attachment to wheel axle).
    pub suspension_rest_length: f32,
    /// Maximum suspension travel from rest.
    pub suspension_travel: f32,
    /// Suspension spring stiffness (force per unit compression).
    pub suspension_stiffness: f32,
    /// Suspension damping coefficient (force per unit vertical velocity).
    pub suspension_damping: f32,
    /// Tire friction coefficient used for lateral grip.
    pub friction: f32,
    /// If true, this wheel receives engine drive force.
    pub driven: bool,
    /// If true, this wheel contributes to steering yaw.
    pub steerable: bool,

    /// Current suspension compression in [0, 1]. 0 = fully extended, 1 = bottomed out.
    pub compression: f32,
    /// True if the wheel made ground contact on the most recent step.
    pub in_contact: bool,
    /// World-space ground contact point from the most recent step (valid when `in_contact`).
    pub contact_point: Vector3,
}

impl VehicleWheel3D {
    /// Creates a new wheel with sensible defaults.
    pub fn new(attachment_local: Vector3) -> Self {
        Self {
            attachment_local,
            radius: 0.3,
            suspension_rest_length: 0.4,
            suspension_travel: 0.3,
            suspension_stiffness: 5000.0,
            suspension_damping: 500.0,
            friction: 1.0,
            driven: true,
            steerable: false,
            compression: 0.0,
            in_contact: false,
            contact_point: Vector3::ZERO,
        }
    }
}

/// Arcade-to-sim vehicle body with suspension, drive, brake, and steering.
///
/// Models Godot's VehicleBody3D: a rigid chassis with a set of
/// [`VehicleWheel3D`] attachments. Step the vehicle with
/// [`VehicleBody3D::step`] to apply gravity, suspension, drive, brake, and
/// lateral friction forces, then integrate the chassis.
#[derive(Debug, Clone)]
pub struct VehicleBody3D {
    /// Unique identifier.
    pub id: VehicleBodyId3D,
    /// Chassis rigid body (position, velocity, rotation, mass).
    pub chassis: PhysicsBody3D,
    /// Attached wheels.
    pub wheels: Vec<VehicleWheel3D>,
    /// Engine drive force applied along the chassis forward axis.
    pub engine_force: f32,
    /// Brake force magnitude applied opposite to chassis forward velocity.
    pub brake: f32,
    /// Current steering angle (radians, clamped to `max_steer`).
    pub steering: f32,
    /// Maximum allowed steering angle (radians).
    pub max_steer: f32,
    /// Effective wheelbase used by the bicycle-model turn rate.
    pub wheelbase: f32,
}

impl VehicleBody3D {
    /// Creates a new vehicle body with a box chassis of the given half-extents
    /// at the given world position and mass. The chassis begins with no wheels.
    pub fn new(
        id: VehicleBodyId3D,
        position: Vector3,
        half_extents: Vector3,
        mass: f32,
    ) -> Self {
        let chassis = PhysicsBody3D::new(
            BodyId3D(id.0),
            BodyType3D::Rigid,
            position,
            Shape3D::BoxShape { half_extents },
            mass,
        );
        Self {
            id,
            chassis,
            wheels: Vec::new(),
            engine_force: 0.0,
            brake: 0.0,
            steering: 0.0,
            max_steer: 0.6,
            wheelbase: 2.0 * half_extents.z.max(0.5),
        }
    }

    /// Attaches a wheel and returns its index.
    pub fn add_wheel(&mut self, wheel: VehicleWheel3D) -> VehicleWheelId3D {
        let idx = self.wheels.len() as u32;
        self.wheels.push(wheel);
        VehicleWheelId3D(idx)
    }

    /// Returns the number of attached wheels.
    pub fn wheel_count(&self) -> usize {
        self.wheels.len()
    }

    /// Returns the chassis world-space forward direction (local `-Z`, matching
    /// Godot's Vector3::FORWARD convention).
    pub fn forward(&self) -> Vector3 {
        self.chassis.rotation.xform(Vector3::FORWARD)
    }

    /// Returns the chassis world-space right direction (local `+X`).
    pub fn right(&self) -> Vector3 {
        self.chassis.rotation.xform(Vector3::new(1.0, 0.0, 0.0))
    }

    /// Returns the chassis world-space up direction (local `+Y`).
    pub fn up(&self) -> Vector3 {
        self.chassis.rotation.xform(Vector3::UP)
    }

    /// Sets the steering angle, clamping to `[-max_steer, max_steer]`.
    pub fn set_steering(&mut self, angle: f32) {
        self.steering = angle.clamp(-self.max_steer, self.max_steer);
    }

    /// Steps the vehicle simulation by `dt` seconds against a flat ground at
    /// the given `ground_y`.
    ///
    /// Order of operations:
    /// 1. Gravity is accumulated on the chassis.
    /// 2. Each wheel performs a vertical raycast; on contact, a suspension
    ///    spring force is applied upward on the chassis.
    /// 3. Driven wheels in contact apply engine force along the forward axis.
    /// 4. Brake force opposes forward velocity, capped to avoid reversing.
    /// 5. Lateral tire friction opposes sideways velocity, capped to avoid
    ///    reversing.
    /// 6. Steering yaw is applied via the bicycle turn-rate formula
    ///    `omega = v_forward * tan(steer) / wheelbase`.
    /// 7. The chassis integrates; if it sinks below the ground plane, its
    ///    position and vertical velocity are clamped.
    pub fn step(&mut self, dt: f32, gravity: Vector3, ground_y: f32) {
        // (1) Gravity
        let mass = self.chassis.mass.max(1e-6);
        self.chassis.apply_force(gravity * mass);

        let forward = self.forward();
        let right = self.right();
        let up = self.up();

        // (2) Wheel raycasts and suspension
        let mut contact_count = 0usize;
        let mut driven_contact = 0usize;
        for wheel in &mut self.wheels {
            let attach = self.chassis.position
                + right * wheel.attachment_local.x
                + up * wheel.attachment_local.y
                + forward * wheel.attachment_local.z;
            let wheel_bottom_y = attach.y - wheel.suspension_rest_length - wheel.radius;

            if wheel_bottom_y <= ground_y {
                let penetration = (ground_y - wheel_bottom_y).min(wheel.suspension_travel);
                wheel.compression = (penetration / wheel.suspension_travel).clamp(0.0, 1.0);
                wheel.in_contact = true;
                wheel.contact_point = Vector3::new(attach.x, ground_y, attach.z);

                let vertical_velocity = self.chassis.linear_velocity.y;
                let spring_force =
                    wheel.suspension_stiffness * penetration - wheel.suspension_damping * vertical_velocity;
                let spring_force = spring_force.max(0.0);
                self.chassis
                    .apply_force(Vector3::new(0.0, spring_force, 0.0));

                contact_count += 1;
                if wheel.driven {
                    driven_contact += 1;
                }
            } else {
                wheel.compression = 0.0;
                wheel.in_contact = false;
            }
        }

        // (3) Engine drive force
        if driven_contact > 0 && self.engine_force != 0.0 {
            let per_wheel = self.engine_force / driven_contact as f32;
            for _ in 0..driven_contact {
                self.chassis.apply_force(forward * per_wheel);
            }
        }

        let forward_speed = self.chassis.linear_velocity.dot(forward);

        // (4) Brake opposes forward velocity, capped to not reverse direction
        if contact_count > 0 && self.brake > 0.0 && forward_speed.abs() > 1e-6 {
            let max_brake = forward_speed.abs() * mass / dt.max(1e-6);
            let brake_mag = self.brake.min(max_brake);
            let brake_dir = if forward_speed > 0.0 { -forward } else { forward };
            self.chassis.apply_force(brake_dir * brake_mag);
        }

        // (5) Lateral tire friction — cancel sideways velocity, capped
        if contact_count > 0 {
            let lateral_speed = self.chassis.linear_velocity.dot(right);
            if lateral_speed.abs() > 1e-6 {
                let avg_friction: f32 = self
                    .wheels
                    .iter()
                    .filter(|w| w.in_contact)
                    .map(|w| w.friction)
                    .sum::<f32>()
                    / contact_count as f32;
                let grip_force_cap = avg_friction * mass * gravity.length().max(1.0);
                let cancel_force = -lateral_speed * mass / dt.max(1e-6);
                let applied = cancel_force.clamp(-grip_force_cap, grip_force_cap);
                self.chassis.apply_force(right * applied);
            }
        }

        // (6) Steering yaw rate via bicycle model
        if contact_count > 0 && self.steering.abs() > 1e-6 && forward_speed.abs() > 0.01 {
            let wheelbase = self.wheelbase.max(0.5);
            let omega = forward_speed * self.steering.tan() / wheelbase;
            self.chassis.angular_velocity = Vector3::new(0.0, omega, 0.0);
        } else if contact_count > 0 && self.steering.abs() <= 1e-6 {
            self.chassis.angular_velocity = Vector3::ZERO;
        }

        // (7) Integrate
        self.chassis.integrate(dt);

        // Clamp to ground plane if the chassis has sunk through it.
        let half_height = match &self.chassis.shape {
            Shape3D::BoxShape { half_extents } => half_extents.y,
            Shape3D::Sphere { radius } => *radius,
            _ => 0.5,
        };
        let min_y = ground_y + half_height;
        if self.chassis.position.y < min_y {
            self.chassis.position.y = min_y;
            if self.chassis.linear_velocity.y < 0.0 {
                self.chassis.linear_velocity.y = 0.0;
            }
        }
    }

    /// Returns the chassis world-space position.
    pub fn position(&self) -> Vector3 {
        self.chassis.position
    }

    /// Returns the chassis world-space linear velocity.
    pub fn linear_velocity(&self) -> Vector3 {
        self.chassis.linear_velocity
    }

    /// Returns the chassis rotation.
    pub fn rotation(&self) -> Quaternion {
        self.chassis.rotation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_vehicle() -> VehicleBody3D {
        let mut v = VehicleBody3D::new(
            VehicleBodyId3D(1),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.9, 0.4, 1.8),
            800.0,
        );
        // Four wheels at corners, slightly inboard
        let positions = [
            Vector3::new(-0.8, -0.3, -1.5),
            Vector3::new(0.8, -0.3, -1.5),
            Vector3::new(-0.8, -0.3, 1.5),
            Vector3::new(0.8, -0.3, 1.5),
        ];
        for p in positions {
            v.add_wheel(VehicleWheel3D::new(p));
        }
        v
    }

    #[test]
    fn wheel_defaults_are_reasonable() {
        let w = VehicleWheel3D::new(Vector3::ZERO);
        assert!(w.radius > 0.0);
        assert!(w.suspension_rest_length > 0.0);
        assert!(w.suspension_travel > 0.0);
        assert!(w.suspension_stiffness > 0.0);
        assert!(w.friction > 0.0);
        assert!(w.driven);
        assert!(!w.in_contact);
        assert_eq!(w.compression, 0.0);
    }

    #[test]
    fn vehicle_default_has_no_wheels() {
        let v = VehicleBody3D::new(
            VehicleBodyId3D(0),
            Vector3::ZERO,
            Vector3::new(1.0, 0.5, 2.0),
            1000.0,
        );
        assert_eq!(v.wheel_count(), 0);
        assert_eq!(v.engine_force, 0.0);
        assert_eq!(v.brake, 0.0);
        assert_eq!(v.steering, 0.0);
    }

    #[test]
    fn add_wheel_increases_count() {
        let v = make_test_vehicle();
        assert_eq!(v.wheel_count(), 4);
    }

    #[test]
    fn steering_is_clamped_to_max() {
        let mut v = make_test_vehicle();
        v.set_steering(10.0);
        assert!((v.steering - v.max_steer).abs() < 1e-5);
        v.set_steering(-10.0);
        assert!((v.steering + v.max_steer).abs() < 1e-5);
    }
}
