//! Raycast vehicle driving a body in the REAL 3D world (pat-s3rrv.194).
//!
//! Godot ground truth: `scene/3d/physics/vehicle_body_3d.cpp` — each
//! `VehicleWheel3D` casts down `wheel_rest_length + wheel_radius` every physics
//! tick, the suspension spring pushes the chassis up, `engine_force`/`brake`
//! drive the contact patch, and the tire model cancels lateral slip.
//!
//! # Why this is a port and not a move
//!
//! A complete vehicle solver already existed at `gdphysics2d::vehicle3d`, with
//! its own unit test, and **nothing outside that module ever constructed one**.
//! It could not be wired up as it stood for two reasons:
//!
//! 1. it OWNED its chassis (`chassis: PhysicsBody3D`) instead of driving a body
//!    that lives in a [`PhysicsWorld3D`], so it could never interact with the
//!    bodies a scene actually contains; and
//! 2. it took a flat `ground_y: f32` instead of casting a ray, so wheels could
//!    only ride an infinite horizontal plane — not a `StaticBody3D` floor, a
//!    ramp, or a trimesh.
//!
//! The force model below is carried over as-is; what changed is where the
//! chassis lives and how the ground is found.
//!
//! ⚠️ The ground query is [`PhysicsRayQuery3D`], **not** `PhysicsWorld3D::raycast`.
//! That method is documented "sphere bodies only" and would silently miss a box
//! floor — every wheel would report no contact and the chassis would sink
//! through it. `PhysicsRayQuery3D` handles the full `Shape3D` set, each arm
//! mirroring the matching Godot `intersect_segment`.

use std::collections::HashSet;

use gdcore::math::Vector3;

use crate::body::BodyId3D;

/// Standard gravity, used only to convert Godot's dimensionless suspension
/// coefficients into a force. The world's own gravity still integrates the
/// chassis; this is a scale factor, not a second gravity.
const GRAVITY_MAGNITUDE: f32 = 9.8;
use crate::query::PhysicsRayQuery3D;
use crate::world::PhysicsWorld3D;

/// One wheel of a [`VehicleController3D`].
#[derive(Debug, Clone)]
pub struct VehicleWheel3D {
    /// Where the wheel hangs, in chassis-local space.
    pub attachment_local: Vector3,
    /// Wheel radius (Godot `wheel_radius`).
    pub radius: f32,
    /// Rest length of the suspension (Godot `wheel_rest_length`).
    pub suspension_rest_length: f32,
    /// How far the suspension may compress (Godot `suspension_travel`).
    pub suspension_travel: f32,
    /// Spring rate (Godot `suspension_stiffness`).
    pub suspension_stiffness: f32,
    /// Damper rate, standing in for Godot's compression/relaxation pair.
    pub suspension_damping: f32,
    /// Tire grip coefficient (Godot `wheel_friction_slip`).
    pub friction: f32,
    /// Receives engine force (Godot `use_as_traction`).
    pub driven: bool,
    /// Turns with `steering` (Godot `use_as_steering`).
    pub steerable: bool,
    /// 0..1 suspension compression, published for `get_skidinfo`-style reads.
    pub compression: f32,
    /// Whether the last step found ground (Godot `is_in_contact`).
    pub in_contact: bool,
    /// World-space contact point from the last step.
    pub contact_point: Vector3,
    /// The body the wheel is resting on (Godot `get_contact_body`).
    pub contact_body: Option<BodyId3D>,
}

impl VehicleWheel3D {
    /// A wheel hanging at `attachment_local` with Godot's documented defaults.
    pub fn new(attachment_local: Vector3) -> Self {
        Self {
            attachment_local,
            radius: 0.5,
            suspension_rest_length: 0.15,
            suspension_travel: 0.2,
            suspension_stiffness: 5.88,
            suspension_damping: 0.5,
            friction: 10.5,
            driven: false,
            steerable: false,
            compression: 0.0,
            in_contact: false,
            contact_point: Vector3::ZERO,
            contact_body: None,
        }
    }

    /// How far down the wheel probes: the suspension at full extension plus the
    /// tire radius, which is the length Godot casts.
    fn probe_length(&self) -> f32 {
        self.suspension_rest_length + self.radius
    }
}

/// Drives one chassis body in a [`PhysicsWorld3D`] as a raycast vehicle.
#[derive(Debug, Clone)]
pub struct VehicleController3D {
    /// The chassis body this controller pushes. It is an ordinary rigid body in
    /// the world — the controller only applies forces to it.
    pub chassis: BodyId3D,
    /// The wheels, in scene order.
    pub wheels: Vec<VehicleWheel3D>,
    /// Godot `engine_force`, split across the driven wheels in contact.
    pub engine_force: f32,
    /// Godot `brake`.
    pub brake: f32,
    /// Godot `steering`, in radians.
    pub steering: f32,
}

impl VehicleController3D {
    /// A controller for `chassis` with no wheels yet.
    pub fn new(chassis: BodyId3D) -> Self {
        Self {
            chassis,
            wheels: Vec::new(),
            engine_force: 0.0,
            brake: 0.0,
            steering: 0.0,
        }
    }

    /// Adds a wheel and returns its index.
    pub fn add_wheel(&mut self, wheel: VehicleWheel3D) -> usize {
        self.wheels.push(wheel);
        self.wheels.len() - 1
    }

    /// Advances the vehicle one tick against `world`.
    ///
    /// ⚠️ Forces are accumulated into locals and applied to the chassis at the
    /// END. The ray query borrows every body in the world immutably, so
    /// mutating the chassis inside the wheel loop would not borrow-check — and
    /// applying forces mid-loop would also let wheel 2 raycast against a chassis
    /// wheel 1 had already moved, making the result depend on wheel order.
    pub fn step(&mut self, world: &mut PhysicsWorld3D, dt: f32) {
        let Some(chassis) = world.get_body(self.chassis) else {
            return;
        };
        let mass = chassis.mass.max(1e-6);
        let origin = chassis.position;
        let rotation = chassis.rotation;
        let linear_velocity = chassis.linear_velocity;

        let forward = rotation.xform(Vector3::FORWARD);
        let right = rotation.xform(Vector3::new(1.0, 0.0, 0.0));
        let up = rotation.xform(Vector3::UP);

        // The chassis must never be its own ground.
        let mut exclude: HashSet<BodyId3D> = HashSet::new();
        exclude.insert(self.chassis);

        let mut force = Vector3::ZERO;
        let mut contact_count = 0usize;
        let mut driven_contact = 0usize;

        for wheel in &mut self.wheels {
            let attach = origin
                + right * wheel.attachment_local.x
                + up * wheel.attachment_local.y
                + forward * wheel.attachment_local.z;
            let probe = wheel.probe_length() + wheel.suspension_travel;
            let mut query = PhysicsRayQuery3D::new(attach, attach - up * probe);
            query.exclude = exclude.clone();

            match query.intersect(world.bodies()) {
                Some(hit) if hit.distance <= probe => {
                    // How far the suspension is compressed from full extension.
                    let penetration =
                        (wheel.probe_length() - hit.distance).clamp(0.0, wheel.suspension_travel);
                    wheel.compression = if wheel.suspension_travel > 0.0 {
                        (penetration / wheel.suspension_travel).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    wheel.in_contact = true;
                    wheel.contact_point = hit.point;
                    wheel.contact_body = Some(hit.body_id);

                    // Spring pushes along the chassis up axis, damped by the
                    // vertical component of the chassis velocity.
                    //
                    // ⚠️ Godot's `suspension_stiffness` (default 5.88) and
                    // `damping_compression` (0.83) are Bullet-style
                    // COEFFICIENTS, not a spring rate in N/m and a damper in
                    // N·s/m. Feeding 5.88 straight into `rate * penetration`
                    // would need ~66 m of travel to hold a 40 kg car up, so the
                    // chassis would sink through the floor while every property
                    // read looked correct.
                    //
                    // Bullet scales the suspension force by the chassis mass, so
                    // the rate is `stiffness * weight`, and the damper is taken
                    // at the critical-damping form `2 * sqrt(rate * mass)`.
                    // These are corroborated rather than invented: with Godot's
                    // own defaults they give ~2305 N/m and ~504 N·s/m, and the
                    // hand-tuned model this was ported from used 5000 and
                    // **500** — the damper lands within 1% of a value nobody
                    // derived from these numbers.
                    let rate = wheel.suspension_stiffness * mass * GRAVITY_MAGNITUDE;
                    let damp = wheel.suspension_damping * 2.0 * (rate * mass).sqrt();
                    let vertical_velocity = linear_velocity.dot(up);
                    let spring = (rate * penetration - damp * vertical_velocity).max(0.0);
                    force = force + up * spring;

                    contact_count += 1;
                    if wheel.driven {
                        driven_contact += 1;
                    }
                }
                _ => {
                    wheel.compression = 0.0;
                    wheel.in_contact = false;
                    wheel.contact_body = None;
                }
            }
        }

        // Engine drive, split across the driven wheels that found ground. A
        // wheel in the air spins freely and delivers nothing.
        if driven_contact > 0 && self.engine_force != 0.0 {
            force = force + forward * self.engine_force;
        }

        let forward_speed = linear_velocity.dot(forward);

        // Brake opposes forward motion, capped so it cannot reverse the car.
        if contact_count > 0 && self.brake > 0.0 && forward_speed.abs() > 1e-6 {
            let max_brake = forward_speed.abs() * mass / dt.max(1e-6);
            let magnitude = self.brake.min(max_brake);
            let direction = if forward_speed > 0.0 {
                -forward
            } else {
                forward
            };
            force = force + direction * magnitude;
        }

        // Lateral tire friction: cancel sideways velocity, capped by grip.
        if contact_count > 0 {
            let lateral_speed = linear_velocity.dot(right);
            if lateral_speed.abs() > 1e-6 {
                let grip: f32 = self
                    .wheels
                    .iter()
                    .filter(|w| w.in_contact)
                    .map(|w| w.friction)
                    .sum::<f32>()
                    / contact_count as f32;
                let cap = grip * mass * GRAVITY_MAGNITUDE;
                let cancel = -lateral_speed * mass / dt.max(1e-6);
                force = force + right * cancel.clamp(-cap, cap);
            }
        }

        if let Some(body) = world.get_body_mut(self.chassis) {
            body.apply_force(force);
        }
    }

    /// Whether any wheel found ground on the last step.
    pub fn any_wheel_in_contact(&self) -> bool {
        self.wheels.iter().any(|w| w.in_contact)
    }
}
