//! Raycast vehicle: a port of Godot 4.6's `scene/3d/physics/vehicle_body_3d.cpp`
//! (pat-qg3nu), driving a chassis body that lives in a [`PhysicsWorld3D`].
//!
//! Godot runs the whole vehicle inside `VehicleBody3D::_body_state_changed`,
//! the body's state-sync callback. With Jolt that callback fires from the
//! space's `flush_queries` at the START of each physics tick, i.e. right after
//! the previous step, and it changes the chassis velocity through
//! `PhysicsDirectBodyState3D::apply_impulse`. Those impulses are therefore
//! integrated by the NEXT step, and the `engine_force`/`steering` it reads are
//! the ones a script wrote during the previous tick. [`VehicleController3D::update`]
//! is that callback, and the server must call it right after `world.step`
//! (never before it), for the same reason `_integrate_forces` runs there.
//!
//! The order inside the callback is Godot's and is load-bearing:
//!
//! 1. `_update_wheel` for every wheel: the wheel's world transform from the
//!    CURRENT chassis transform and the PREVIOUS tick's suspension length.
//!    This is the transform written back onto the wheel node, so a wheel node
//!    always trails its ray cast by one tick (Godot's Wheel1 reads y 0.1152 on
//!    the first tick it is in contact, its authored height).
//! 2. `_ray_cast` for every wheel.
//! 3. `_update_suspension`, then one suspension impulse per wheel.
//! 4. `_update_friction`: side impulses (`_resolve_single_bilateral`) and
//!    forward impulses (engine, or rolling friction capped by `brake`), both
//!    clipped by the wheel's friction circle.
//! 5. Wheel spin: `m_deltaRotation` from the forward speed at the hard point.
//!
//! Each impulse acts on the chassis velocity immediately, as Jolt's
//! `AddImpulse` does, so later stages read the velocity earlier ones produced.
//!
//! ⚠️ Positive `engine_force` drives along the chassis' +Z axis, not Godot's
//! usual -Z "forward": `_update_friction` builds `forward = normal x axle`
//! (-Z on flat ground) and pushes along it with `-engine_force * step`.

use std::collections::HashSet;

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Quaternion, Transform3D};

use crate::body::{BodyId3D, PhysicsBody3D};
use crate::query::PhysicsRayQuery3D;
use crate::world::PhysicsWorld3D;

/// One wheel: Godot's `VehicleWheel3D` simulation fields.
#[derive(Debug, Clone)]
pub struct VehicleWheel3D {
    // -- geometry, cached once from the wheel node (Godot NOTIFICATION_ENTER_TREE) --
    /// `m_chassisConnectionPointCS`: the wheel node's authored local origin.
    pub chassis_connection_cs: Vector3,
    /// `m_wheelDirectionCS`: minus the authored local Y axis.
    pub direction_cs: Vector3,
    /// `m_wheelAxleCS`: the authored local X axis.
    pub axle_cs: Vector3,

    // -- tunables (VehicleWheel3D properties; defaults from vehicle_body_3d.h) --
    /// `wheel_radius`.
    pub radius: f32,
    /// `wheel_rest_length`.
    pub suspension_rest_length: f32,
    /// `suspension_travel`.
    pub max_suspension_travel: f32,
    /// `suspension_stiffness`.
    pub suspension_stiffness: f32,
    /// `suspension_max_force`.
    pub max_suspension_force: f32,
    /// `damping_compression`.
    pub damping_compression: f32,
    /// `damping_relaxation`.
    pub damping_relaxation: f32,
    /// `wheel_friction_slip`.
    pub friction_slip: f32,
    /// `wheel_roll_influence`.
    pub roll_influence: f32,
    /// `use_as_traction`.
    pub traction: bool,
    /// `use_as_steering`.
    pub steers: bool,
    /// `m_engineForce`.
    pub engine_force: f32,
    /// `m_brake`.
    pub brake: f32,
    /// `m_steering`.
    pub steering: f32,

    // -- simulation state --
    /// `m_rotation`: accumulated spin about the axle, radians.
    pub rotation: f32,
    /// `m_deltaRotation`.
    pub delta_rotation: f32,
    /// `m_rpm`.
    pub rpm: f32,
    /// `m_skidInfo` (`get_skidinfo`).
    pub skid_info: f32,
    /// `m_wheelsSuspensionForce`.
    pub suspension_force: f32,
    /// `m_clippedInvContactDotSuspension`.
    pub clipped_inv_contact_dot_suspension: f32,
    /// `m_suspensionRelativeVelocity`.
    pub suspension_relative_velocity: f32,
    /// `m_raycastInfo.m_suspensionLength` (0 until the first ray cast, as in Godot).
    pub suspension_length: f32,
    /// `m_raycastInfo.m_hardPointWS`.
    pub hard_point_ws: Vector3,
    /// `m_raycastInfo.m_wheelDirectionWS`.
    pub direction_ws: Vector3,
    /// `m_raycastInfo.m_wheelAxleWS`.
    pub axle_ws: Vector3,
    /// `m_raycastInfo.m_contactPointWS` (`get_contact_point`).
    pub contact_point: Vector3,
    /// `m_raycastInfo.m_contactNormalWS` (`get_contact_normal`).
    pub contact_normal: Vector3,
    /// `m_raycastInfo.m_isInContact` (`is_in_contact`).
    pub in_contact: bool,
    /// `m_raycastInfo.m_groundObject` (`get_contact_body`).
    pub contact_body: Option<BodyId3D>,
    /// `m_worldTransform`: the pose written back onto the wheel node.
    pub world_transform: Transform3D,
}

impl VehicleWheel3D {
    /// A wheel whose node's authored local transform is `local`, with Godot's
    /// defaults for every tunable.
    pub fn new(local: Transform3D) -> Self {
        Self {
            chassis_connection_cs: local.origin,
            direction_cs: (local.basis.y * -1.0).normalized(),
            axle_cs: local.basis.x.normalized(),
            radius: 0.5,
            suspension_rest_length: 0.15,
            max_suspension_travel: 0.2,
            suspension_stiffness: 5.88,
            max_suspension_force: 6000.0,
            damping_compression: 0.83,
            damping_relaxation: 0.88,
            friction_slip: 10.5,
            roll_influence: 0.1,
            traction: false,
            steers: false,
            engine_force: 0.0,
            brake: 0.0,
            steering: 0.0,
            rotation: 0.0,
            delta_rotation: 0.0,
            rpm: 0.0,
            skid_info: 0.0,
            suspension_force: 0.0,
            clipped_inv_contact_dot_suspension: 1.0,
            suspension_relative_velocity: 0.0,
            suspension_length: 0.0,
            hard_point_ws: Vector3::ZERO,
            direction_ws: Vector3::ZERO,
            axle_ws: Vector3::ZERO,
            contact_point: Vector3::ZERO,
            contact_normal: Vector3::ZERO,
            in_contact: false,
            contact_body: None,
            world_transform: local,
        }
    }
}

/// What the friction stages need to know about the body a wheel stands on.
#[derive(Debug, Clone, Copy)]
struct Ground {
    origin: Vector3,
    linear_velocity: Vector3,
    angular_velocity: Vector3,
    inverse_mass: f32,
}

impl Ground {
    fn velocity_at(&self, point: Vector3) -> Vector3 {
        self.linear_velocity + self.angular_velocity.cross(point - self.origin)
    }
}

/// Godot's `Basis(axis, angle)`.
fn axis_angle(axis: Vector3, angle: f32) -> Basis {
    Basis::from_quaternion(Quaternion::from_axis_angle(axis, angle))
}

/// Drives one chassis body in a [`PhysicsWorld3D`] as a raycast vehicle.
#[derive(Debug, Clone)]
pub struct VehicleController3D {
    /// The chassis body.
    pub chassis: BodyId3D,
    /// The wheels, in scene order (Godot pushes them at ENTER_TREE).
    pub wheels: Vec<VehicleWheel3D>,
    /// `VehicleBody3D.mass` as Godot's `get_mass()` returns it.
    pub mass: f32,
    /// `VehicleBody3D.collision_mask`, used for the wheel rays.
    pub collision_mask: u32,
}

impl VehicleController3D {
    /// A controller for `chassis` with no wheels yet.
    pub fn new(chassis: BodyId3D) -> Self {
        Self {
            chassis,
            wheels: Vec::new(),
            mass: 40.0,
            collision_mask: 1,
        }
    }

    /// Adds a wheel and returns its index.
    pub fn add_wheel(&mut self, wheel: VehicleWheel3D) -> usize {
        self.wheels.push(wheel);
        self.wheels.len() - 1
    }

    /// `VehicleBody3D::set_engine_force`: every traction wheel takes it.
    pub fn set_engine_force(&mut self, force: f32) {
        for w in self.wheels.iter_mut().filter(|w| w.traction) {
            w.engine_force = force;
        }
    }

    /// `VehicleBody3D::set_brake`: every wheel takes it.
    pub fn set_brake(&mut self, brake: f32) {
        for w in &mut self.wheels {
            w.brake = brake;
        }
    }

    /// `VehicleBody3D::set_steering`: every steering wheel takes it.
    pub fn set_steering(&mut self, steering: f32) {
        for w in self.wheels.iter_mut().filter(|w| w.steers) {
            w.steering = steering;
        }
    }

    /// `VehicleBody3D::_body_state_changed`, run right after a world step
    /// (see the module docs). `dt` is the step (`p_state->get_step()`).
    pub fn update(&mut self, world: &mut PhysicsWorld3D, dt: f32) {
        let Some(body) = world.get_body(self.chassis) else {
            return;
        };
        // A working copy of the chassis stands in for the direct body state:
        // the ray casts borrow the whole world, and Jolt applies each impulse
        // to the velocity at once, which the copy does too.
        let mut s: PhysicsBody3D = body.clone();
        let chassis = Transform3D {
            basis: Basis::from_quaternion(s.rotation),
            origin: s.position,
        };

        for w in &mut self.wheels {
            Self::update_wheel_transform(w, &chassis);
            Self::update_wheel(w);
        }

        let mut exclude = HashSet::new();
        exclude.insert(self.chassis);
        let mut grounds: Vec<Option<Ground>> = Vec::with_capacity(self.wheels.len());
        for w in &mut self.wheels {
            grounds.push(Self::ray_cast(w, &s, &chassis, world, &exclude, self.collision_mask));
        }

        self.update_suspension();
        for w in &self.wheels {
            let force = w.suspension_force.min(w.max_suspension_force);
            let impulse = w.contact_normal * (force * dt);
            s.apply_impulse_at_position(impulse, w.contact_point);
        }

        self.update_friction(&mut s, &chassis, &grounds, dt);

        for w in &mut self.wheels {
            let relpos = w.hard_point_ws - s.position;
            let vel = s.linear_velocity + s.angular_velocity.cross(relpos);
            if w.in_contact {
                // The chassis Z axis (Godot reads basis[i][2], the Z column).
                let mut fwd = chassis.basis.z;
                if w.steers {
                    fwd = axis_angle(Vector3::new(0.0, 1.0, 0.0), w.steering).xform(fwd);
                }
                let proj = fwd.dot(w.contact_normal);
                fwd = (fwd - w.contact_normal * proj).normalized();
                let proj2 = fwd.dot(vel);
                w.delta_rotation = (proj2 * dt) / w.radius;
            }
            w.rotation += w.delta_rotation;
            w.rpm = ((w.delta_rotation / dt) * 60.0) / std::f32::consts::TAU;
            // Damping of rotation when not in contact.
            w.delta_rotation *= 0.99;
        }

        if let Some(body) = world.get_body_mut(self.chassis) {
            body.linear_velocity = s.linear_velocity;
            body.angular_velocity = s.angular_velocity;
            if body.linear_velocity != Vector3::ZERO || body.angular_velocity != Vector3::ZERO {
                body.wake_up();
            }
        }
    }

    /// The wheel's pose relative to the chassis, as Godot's non-interpolated
    /// path writes it: `chassis.inverse() * m_worldTransform`.
    pub fn wheel_local_transform(&self, index: usize, chassis: &Transform3D) -> Option<Transform3D> {
        let w = self.wheels.get(index)?;
        Some(chassis.inverse() * w.world_transform)
    }

    fn update_wheel_transform(w: &mut VehicleWheel3D, chassis: &Transform3D) {
        w.in_contact = false;
        w.hard_point_ws = chassis.xform(w.chassis_connection_cs);
        w.direction_ws = chassis.basis.xform(w.direction_cs).normalized();
        w.axle_ws = chassis.basis.xform(w.axle_cs).normalized();
    }

    /// `_update_wheel` (after `_update_wheel_transform`).
    fn update_wheel(w: &mut VehicleWheel3D) {
        let up = w.direction_ws * -1.0;
        let right = w.axle_ws;
        let fwd = up.cross(right).normalized();
        let origin = w.hard_point_ws + w.direction_ws * w.suspension_length;
        let steering = axis_angle(up, w.steering);
        let rotating = axis_angle(right, w.rotation);
        // Basis(right[0], up[0], fwd[0], ...) is row-major: columns right, up, fwd.
        let basis2 = Basis { x: right, y: up, z: fwd };
        w.world_transform = Transform3D {
            basis: steering * rotating * basis2,
            origin,
        };
    }

    /// `_ray_cast`.
    fn ray_cast(
        w: &mut VehicleWheel3D,
        s: &PhysicsBody3D,
        chassis: &Transform3D,
        world: &PhysicsWorld3D,
        exclude: &HashSet<BodyId3D>,
        mask: u32,
    ) -> Option<Ground> {
        Self::update_wheel_transform(w, chassis);
        let raylen = w.suspension_rest_length + w.radius;
        let target = w.hard_point_ws + w.direction_ws * raylen;
        w.contact_point = target;
        let source = w.hard_point_ws - w.direction_ws * w.radius;

        let mut query = PhysicsRayQuery3D::new(source, target);
        query.exclude = exclude.clone();
        query.collision_mask = mask;
        w.contact_body = None;

        let Some(hit) = query.intersect(world.bodies()) else {
            w.in_contact = false;
            w.suspension_length = w.suspension_rest_length;
            w.suspension_relative_velocity = 0.0;
            w.contact_normal = w.direction_ws * -1.0;
            w.clipped_inv_contact_dot_suspension = 1.0;
            return None;
        };

        let param = source.distance_to(hit.point) / source.distance_to(target);
        w.contact_normal = hit.normal;
        w.in_contact = true;
        w.contact_body = Some(hit.body_id);

        let hit_distance = param * raylen;
        w.suspension_length = hit_distance - w.radius;
        let min_len = w.suspension_rest_length - w.max_suspension_travel;
        let max_len = w.suspension_rest_length + w.max_suspension_travel;
        if w.suspension_length < min_len {
            w.suspension_length = min_len;
        }
        if w.suspension_length > max_len {
            w.suspension_length = max_len;
        }
        w.contact_point = hit.point;

        let denominator = w.contact_normal.dot(w.direction_ws);
        let chassis_velocity = s.linear_velocity + s.angular_velocity.cross(w.contact_point - s.position);
        let proj_vel = w.contact_normal.dot(chassis_velocity);
        if denominator >= -0.1 {
            w.suspension_relative_velocity = 0.0;
            w.clipped_inv_contact_dot_suspension = 1.0 / 0.1;
        } else {
            let inv = -1.0 / denominator;
            w.suspension_relative_velocity = proj_vel * inv;
            w.clipped_inv_contact_dot_suspension = inv;
        }

        world.get_body(hit.body_id).map(|g| Ground {
            origin: g.position,
            linear_velocity: g.linear_velocity,
            angular_velocity: g.angular_velocity,
            inverse_mass: g.inverse_mass(),
        })
    }

    /// `_update_suspension`.
    fn update_suspension(&mut self) {
        let mass = self.mass;
        for w in &mut self.wheels {
            if !w.in_contact {
                w.suspension_force = 0.0;
                continue;
            }
            let length_diff = w.suspension_rest_length - w.suspension_length;
            let mut force = w.suspension_stiffness * length_diff * w.clipped_inv_contact_dot_suspension;
            let rel_vel = w.suspension_relative_velocity;
            let damping = if rel_vel < 0.0 { w.damping_compression } else { w.damping_relaxation };
            force -= damping * rel_vel;
            w.suspension_force = (force * mass).max(0.0);
        }
    }

    /// `_resolve_single_bilateral` (Godot's ONLY_USE_LINEAR_MASS path).
    fn resolve_single_bilateral(
        &self,
        s: &PhysicsBody3D,
        pos: Vector3,
        ground: Option<Ground>,
        normal: Vector3,
        roll_influence: f32,
        dt: f32,
    ) -> f32 {
        if normal.length_squared() > 1.1 {
            return 0.0;
        }
        let vel1 = s.linear_velocity + s.angular_velocity.cross(pos - s.position);
        let vel2 = ground.map(|g| g.velocity_at(pos)).unwrap_or(Vector3::ZERO);
        let rel_vel = normal.dot(vel1 - vel2);
        let mut contact_damping = 0.2;
        if roll_influence > 0.0 {
            contact_damping = f32::min(contact_damping, dt / roll_influence);
        }
        let b2_inv_mass = ground.map(|g| g.inverse_mass).unwrap_or(0.0);
        let mass_term = 1.0 / ((1.0 / self.mass) + b2_inv_mass);
        -contact_damping * rel_vel * mass_term
    }

    /// `_calc_rolling_friction` with its `btVehicleWheelContactPoint`.
    fn calc_rolling_friction(
        s: &PhysicsBody3D,
        ground: Option<Ground>,
        pos: Vector3,
        dir: Vector3,
        max_impulse: f32,
    ) -> f32 {
        let r0 = pos - s.position;
        let c0 = r0.cross(dir);
        let vec = s.apply_inverse_inertia(c0).cross(r0);
        let denom0 = s.inverse_mass() + dir.dot(vec);
        let jac_diag_ab_inv = 1.0 / denom0;

        let vel1 = s.linear_velocity + s.angular_velocity.cross(r0);
        let vel2 = ground.map(|g| g.velocity_at(pos)).unwrap_or(Vector3::ZERO);
        let vrel = dir.dot(vel1 - vel2);
        let j1 = -vrel * jac_diag_ab_inv;
        j1.clamp(-max_impulse, max_impulse)
    }

    /// `_update_friction`.
    fn update_friction(&mut self, s: &mut PhysicsBody3D, chassis: &Transform3D, grounds: &[Option<Ground>], dt: f32) {
        let n = self.wheels.len();
        if n == 0 {
            return;
        }
        let mut forward_ws = vec![Vector3::ZERO; n];
        let mut axle = vec![Vector3::ZERO; n];
        let mut forward_impulse = vec![0.0f32; n];
        let mut side_impulse = vec![0.0f32; n];

        for i in 0..n {
            let w = &self.wheels[i];
            if !w.in_contact {
                continue;
            }
            let normal = w.contact_normal;
            let mut a = w.world_transform.basis.x;
            a = (a - normal * a.dot(normal)).normalized();
            axle[i] = a;
            forward_ws[i] = normal.cross(a).normalized();
            side_impulse[i] =
                self.resolve_single_bilateral(s, w.contact_point, grounds[i], a, w.roll_influence, dt);
        }

        let side_factor = 1.0f32;
        let fwd_factor = 0.5f32;
        let mut sliding = false;
        for i in 0..n {
            let mut rolling_friction = 0.0;
            {
                let w = &self.wheels[i];
                if w.in_contact {
                    if w.engine_force != 0.0 {
                        rolling_friction = -w.engine_force * dt;
                    } else {
                        let max_impulse = if w.brake != 0.0 { w.brake } else { 0.0 };
                        rolling_friction =
                            Self::calc_rolling_friction(s, grounds[i], w.contact_point, forward_ws[i], max_impulse);
                    }
                }
            }
            let w = &mut self.wheels[i];
            forward_impulse[i] = 0.0;
            w.skid_info = 1.0;
            if w.in_contact {
                let maximp = w.suspension_force * dt * w.friction_slip;
                let maximp_squared = maximp * maximp;
                forward_impulse[i] = rolling_friction;
                let x = forward_impulse[i] * fwd_factor;
                let y = side_impulse[i] * side_factor;
                let impulse_squared = x * x + y * y;
                if impulse_squared > maximp_squared {
                    sliding = true;
                    w.skid_info *= maximp / impulse_squared.sqrt();
                }
            }
        }

        if sliding {
            for i in 0..n {
                if side_impulse[i] != 0.0 && self.wheels[i].skid_info < 1.0 {
                    forward_impulse[i] *= self.wheels[i].skid_info;
                    side_impulse[i] *= self.wheels[i].skid_info;
                }
            }
        }

        // ROLLING_INFLUENCE_FIX is defined in vehicle_body_3d.cpp.
        let chassis_up = chassis.basis.y;
        for i in 0..n {
            let w = &self.wheels[i];
            let mut rel_pos = w.contact_point - s.position;
            if forward_impulse[i] != 0.0 {
                s.apply_impulse_at_position(forward_ws[i] * forward_impulse[i], s.position + rel_pos);
            }
            if side_impulse[i] != 0.0 {
                let side = axle[i] * side_impulse[i];
                rel_pos = rel_pos - chassis_up * (chassis_up.dot(rel_pos) * (1.0 - w.roll_influence));
                s.apply_impulse_at_position(side, s.position + rel_pos);
            }
        }
    }

    /// Whether any wheel found ground on the last update.
    pub fn any_wheel_in_contact(&self) -> bool {
        self.wheels.iter().any(|w| w.in_contact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType3D;
    use crate::shape::Shape3D;

    fn wheel_at(x: f32, y: f32, z: f32) -> VehicleWheel3D {
        let mut w = VehicleWheel3D::new(Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(x, y, z),
        });
        w.radius = 0.25;
        w.suspension_stiffness = 40.0;
        w.max_suspension_travel = 2.0;
        w.traction = true;
        w
    }

    fn car_on_floor() -> (PhysicsWorld3D, VehicleController3D) {
        let mut world = PhysicsWorld3D::new();
        let floor = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(0.0, -1.0, 0.0),
            Shape3D::BoxShape { half_extents: Vector3::new(50.0, 1.0, 50.0) },
            0.0,
        );
        world.add_body(floor);
        let mut chassis = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(0.0, 0.5, 0.0),
            Shape3D::BoxShape { half_extents: Vector3::new(0.5, 0.2, 1.0) },
            40.0,
        );
        chassis.shape_offset = Vector3::new(0.0, 0.4, 0.0);
        chassis.can_sleep = false;
        let id = world.add_body(chassis);
        let mut c = VehicleController3D::new(id);
        for (x, z) in [(0.6, 1.0), (0.6, -0.8), (-0.6, 1.0), (-0.6, -0.8)] {
            c.add_wheel(wheel_at(x, 0.1, z));
        }
        (world, c)
    }

    /// Spring equilibrium: four wheels at stiffness 40 hold a mass-scaled load
    /// of g, so they compress by g / (4 * 40) = 0.06125 on AVERAGE from the
    /// rest length (the axles sit off-centre, so the front and rear differ).
    #[test]
    fn suspension_settles_at_the_spring_equilibrium() {
        let (mut world, mut c) = car_on_floor();
        for _ in 0..1200 {
            world.step(1.0 / 120.0);
            c.update(&mut world, 1.0 / 120.0);
        }
        assert!(c.wheels.iter().all(|w| w.in_contact));
        let mean = c
            .wheels
            .iter()
            .map(|w| w.suspension_rest_length - w.suspension_length)
            .sum::<f32>()
            / 4.0;
        assert!((mean - 9.8 / 160.0).abs() < 1e-3, "mean compression {mean}");
        let av = world.get_body(c.chassis).unwrap().angular_velocity;
        assert!(av.length() < 1e-4, "the chassis must come to rest, spinning at {av:?}");
    }

    /// Positive engine force drives along +Z (Godot's vehicle convention), and
    /// the wheels spin forwards.
    #[test]
    fn engine_force_drives_along_plus_z() {
        let (mut world, mut c) = car_on_floor();
        for _ in 0..240 {
            world.step(1.0 / 120.0);
            c.update(&mut world, 1.0 / 120.0);
        }
        let z0 = world.get_body(c.chassis).unwrap().position.z;
        c.set_engine_force(40.0);
        for _ in 0..240 {
            world.step(1.0 / 120.0);
            c.update(&mut world, 1.0 / 120.0);
        }
        let z1 = world.get_body(c.chassis).unwrap().position.z;
        assert!(z1 - z0 > 1.0, "moved {} along z", z1 - z0);
        assert!(c.wheels.iter().all(|w| w.rpm > 0.0 && w.rotation > 0.0));
    }

    /// An airborne wheel hangs at the rest length and reports no contact.
    #[test]
    fn airborne_wheel_hangs_at_rest_length() {
        let (mut world, mut c) = car_on_floor();
        if let Some(b) = world.get_body_mut(c.chassis) {
            b.position.y = 20.0;
        }
        c.update(&mut world, 1.0 / 120.0);
        c.update(&mut world, 1.0 / 120.0);
        for w in &c.wheels {
            assert!(!w.in_contact);
            assert_eq!(w.suspension_length, w.suspension_rest_length);
            // The pose trails the cast by one update: hard point + dir * rest.
            let y = w.world_transform.origin.y;
            assert!((y - (20.0 + 0.1 - 0.15)).abs() < 1e-4, "wheel y {y}");
        }
    }

    /// Steering turns the wheel pose about the suspension's up axis.
    #[test]
    fn steering_rotates_the_wheel_pose() {
        let (mut world, mut c) = car_on_floor();
        for w in &mut c.wheels {
            w.steers = true;
        }
        c.set_steering(0.4);
        c.update(&mut world, 1.0 / 120.0);
        let x = c.wheels[0].world_transform.basis.x;
        assert!((x.x - 0.4f32.cos()).abs() < 1e-4 && (x.z + 0.4f32.sin()).abs() < 1e-4, "axle {x:?}");
    }
}
