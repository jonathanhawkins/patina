//! 3D rigid body and kinematic body types.

use gdcore::math::Vector3;
use gdcore::math3d::{Aabb, Quaternion};

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

/// Godot's default `physics/3d/time_before_sleep`, in seconds.
pub const DEFAULT_TIME_BEFORE_SLEEP: f32 = 0.5;

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
    /// Local-space offset of [`Self::shape`] from the body origin.
    ///
    /// Godot's `CollisionObject3D` owns a LIST of shapes, each with its own
    /// transform relative to the body (`shape_owner_set_transform`), and a
    /// `CollisionShape3D` contributes its own node transform — so authoring a
    /// character as a capsule raised by half its height is routine. Defaults to
    /// `ZERO`, which is exactly the old behaviour (pat-s3rrv.104).
    pub shape_offset: Vector3,
    /// Local-space rotation of [`Self::shape`] about [`Self::shape_offset`].
    ///
    /// 🔑 A `CollisionShape3D`'s FULL local transform belongs to the shape
    /// owner, rotation included (Godot's
    /// `CollisionObject3D::shape_owner_set_transform`). Only the translation
    /// was carried before, so a cylinder laid on its side or a wall box turned
    /// 90 degrees collided bolt upright — the scene looked right and the
    /// collision was at a right angle to it (pat-mgixk.39).
    ///
    /// `IDENTITY` by default, which is exactly the old behaviour.
    pub shape_rotation: Quaternion,
    /// Additional shapes beyond [`Self::shape`], each with its local offset
    /// and local rotation.
    ///
    /// ⚠️ Kept SEPARATE from `shape` rather than folding both into one
    /// `Vec<(Vector3, Shape3D)>`. `shape` has 33 readers across this crate
    /// (character controller, queries, areas, the 2D crate's 3D stack), and every
    /// body is built through [`Self::new`] — so an additive field changes one
    /// constructor and no call sites, while retyping `shape` would touch all of
    /// them at once. Empty for a single-shape body, which is the common case.
    ///
    /// ⚠️ The rotation lives IN the tuple rather than in a parallel
    /// `Vec<Quaternion>`: it is a per-shape value, and a second vector could
    /// desync from the shapes it describes — one `push` away from silently
    /// rotating the wrong collider.
    pub extra_shapes: Vec<(Vector3, Quaternion, Shape3D)>,
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
    /// When true, all rotation is locked (Godot `RigidBody3D.lock_rotation`):
    /// angular velocity is forced to zero and rotation is not integrated.
    pub lock_rotation: bool,
    /// Per-axis linear freeze, `[x, y, z]` (Godot's
    /// `PhysicsBody3D.axis_lock_linear_x/y/z`), default all false.
    ///
    /// ⚠️ A lock is applied to the VELOCITY, not to the position: zeroing the
    /// component after integration would leave the body having already moved
    /// that tick, so a "locked" axis would still creep under gravity
    /// (pat-s3rrv.105).
    pub axis_lock_linear: [bool; 3],
    /// Per-axis angular freeze, `[x, y, z]` (Godot's
    /// `axis_lock_angular_x/y/z`).
    ///
    /// ⚠️ Independent of [`lock_rotation`](Self::lock_rotation), which is the
    /// all-or-nothing switch. Godot exposes both, and a body may lock a single
    /// spin axis while still tumbling on the others.
    pub axis_lock_angular: [bool; 3],
    /// Constant force applied every physics frame (Godot
    /// `RigidBody3D.constant_force`), on top of one-shot accumulated forces.
    pub constant_force: Vector3,
    /// Constant torque applied every physics frame (Godot
    /// `RigidBody3D.constant_torque`).
    pub constant_torque: Vector3,
    /// Constant surface linear velocity this body imparts to contacts (Godot
    /// `StaticBody3D.constant_linear_velocity` — conveyor belts). Does not move
    /// the body itself; only drags bodies resting on it via friction.
    pub constant_linear_velocity: Vector3,
    /// Constant surface angular velocity this body imparts to contacts (Godot
    /// `StaticBody3D.constant_angular_velocity` — turntables).
    pub constant_angular_velocity: Vector3,
    /// Whether continuous collision detection is enabled.
    pub continuous_cd: bool,
    /// Whether the body can sleep when at rest.
    pub can_sleep: bool,
    /// Whether the body is currently sleeping.
    pub sleeping: bool,
    /// How long the body has been below the sleep thresholds (pat-l8jg8).
    sleep_timer: f32,
    /// Freeze mode (None = active, Static/Kinematic = frozen).
    pub freeze_mode: FreezeMode,
    /// Whether contact monitoring is enabled.
    pub contact_monitor: bool,
    /// Maximum number of contacts to report.
    pub max_contacts_reported: usize,
    /// Collision layer bitmask: which layers this body belongs to.
    pub collision_layer: u32,
    /// Collision mask bitmask: which layers this body scans for collisions.
    pub collision_mask: u32,
    /// Principal moments of inertia that replace the shape-derived ones
    /// (pat-qg3nu): a body's authored `inertia`, or the inertia about the
    /// body ORIGIN when its centre of mass is custom-placed there.
    pub inertia_override: Option<Vector3>,
    /// The centre of mass, in the body's LOCAL frame relative to its origin
    /// (pat-zljoy). The body rotates about this point, `linear_velocity` is
    /// this point's velocity, and contact/force lever arms are measured from
    /// it, as in Godot. `ZERO` (the default) is the body origin: Godot's
    /// `center_of_mass_mode` CUSTOM with the default `center_of_mass`. The
    /// server sets AUTO bodies to their shapes' centroid.
    pub center_of_mass: Vector3,
    /// Accumulated force for the current frame.
    accumulated_force: Vector3,
    /// Accumulated torque for the current frame.
    accumulated_torque: Vector3,
    /// Contact points from the last physics step.
    contacts: Vec<ContactPoint3D>,
}

/// Zeroes the components of `v` whose axis is locked (pat-s3rrv.105).
///
/// Indexed `[x, y, z]` to match Godot's `axis_lock_linear_x/y/z` naming, so the
/// bridge can read the three properties into an array without a mapping table.
fn apply_axis_lock(v: &mut Vector3, lock: [bool; 3]) {
    if lock[0] {
        v.x = 0.0;
    }
    if lock[1] {
        v.y = 0.0;
    }
    if lock[2] {
        v.z = 0.0;
    }
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
            shape_offset: Vector3::ZERO,
            shape_rotation: Quaternion::IDENTITY,
            extra_shapes: Vec::new(),
            mass,
            // Godot's body default with no `PhysicsMaterial`: friction 1.0,
            // bounce 0 (`RigidBody3D`/`StaticBody3D::_reload_physics_characteristics`
            // write `BODY_PARAM_FRICTION, 1`; Jolt's `JoltShapedObject3D`
            // starts at `mFriction = 1.0f`). 0.5 let a box slide 0.6 m down
            // truck_town's 6 degree road (pat-uqw01).
            friction: 1.0,
            bounce: 0.0,
            gravity_scale: 1.0,
            linear_damp: 0.0,
            angular_damp: 0.0,
            lock_rotation: false,
            axis_lock_linear: [false; 3],
            axis_lock_angular: [false; 3],
            constant_force: Vector3::ZERO,
            constant_torque: Vector3::ZERO,
            constant_linear_velocity: Vector3::ZERO,
            constant_angular_velocity: Vector3::ZERO,
            continuous_cd: false,
            can_sleep: true,
            sleeping: false,
            sleep_timer: 0.0,
            freeze_mode: FreezeMode::None,
            contact_monitor: false,
            max_contacts_reported: 0,
            collision_layer: 1,
            collision_mask: 1,
            inertia_override: None,
            center_of_mass: Vector3::ZERO,
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
            let offset = position - self.center_of_mass_world();
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
            let offset = position - self.center_of_mass_world();
            let torque_impulse = offset.cross(impulse);
            self.angular_velocity =
                self.angular_velocity + self.apply_inverse_inertia(torque_impulse);
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
            self.angular_velocity = self.angular_velocity + self.apply_inverse_inertia(impulse);
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
        let accel = scaled_gravity + (self.accumulated_force + self.constant_force) * inv_mass;
        self.linear_velocity = self.linear_velocity + accel * dt;

        // Apply linear damping
        if self.linear_damp > 0.0 {
            let damp = (1.0 - self.linear_damp * dt).max(0.0);
            self.linear_velocity = self.linear_velocity * damp;
        }

        // ⚠️ BEFORE the position update, not after. Zeroing a locked component
        // afterwards would leave the body having already travelled that tick,
        // so a "locked" axis would creep one step per frame under gravity —
        // slow enough to look like a solver tolerance rather than a bug
        // (pat-s3rrv.105).
        apply_axis_lock(&mut self.linear_velocity, self.axis_lock_linear);

        // `linear_velocity` moves the centre of mass; the rotation below then
        // turns the body about it, and the origin is put back relative to the
        // moved, turned centre (pat-zljoy).
        let com_after = self.center_of_mass_world() + self.linear_velocity * dt;
        self.position = self.position + self.linear_velocity * dt;

        // Angular integration — skipped entirely when rotation is locked
        // (Godot `lock_rotation`), which also zeroes any residual spin.
        if self.lock_rotation {
            self.angular_velocity = Vector3::ZERO;
        } else {
            // torque → angular acceleration via the inverse inertia tensor
            // (I⁻¹ · τ), including the per-frame constant torque.
            let angular_accel =
                self.apply_inverse_inertia(self.accumulated_torque + self.constant_torque);
            self.angular_velocity = self.angular_velocity + angular_accel * dt;

            // Apply angular damping
            if self.angular_damp > 0.0 {
                let damp = (1.0 - self.angular_damp * dt).max(0.0);
                self.angular_velocity = self.angular_velocity * damp;
            }

            // Per-axis angular freeze. Independent of `lock_rotation`, which is
            // handled by the branch above — this is the partial form, where a
            // body spins freely on the axes it did not lock.
            apply_axis_lock(&mut self.angular_velocity, self.axis_lock_angular);

            // Integrate rotation (simplified: small-angle approximation)
            if self.angular_velocity.length_squared() > 1e-12 {
                let half_dt = dt * 0.5;
                let dq = Quaternion::new(
                    self.angular_velocity.x * half_dt,
                    self.angular_velocity.y * half_dt,
                    self.angular_velocity.z * half_dt,
                    1.0,
                );
                // 🔑 `dq * rotation`, LEFT-multiplied (pat-qg3nu):
                // `angular_velocity` is WORLD-space, as in Godot, and
                // `rotation * dq` applies it in the body's LOCAL frame. The two
                // agree only while the body is unrotated, so the bug hid
                // until a body started turned: truck_town's car sits under an
                // `InstancePos` rotated 90 degrees about Y, so its pitch
                // integrated as a ROLL and the parked car rolled over.
                self.rotation = (dq * self.rotation).normalized();
                if self.center_of_mass != Vector3::ZERO {
                    self.position = com_after - self.rotation.xform(self.center_of_mass);
                }
            }
        }

        self.accumulated_force = Vector3::ZERO;
        self.accumulated_torque = Vector3::ZERO;

        // Auto-sleep check.
        //
        // ⚠️ Only SUSTAINED stillness may sleep a body: Godot (and Jolt) wait
        // `physics/3d/time_before_sleep` (0.5 s) below the thresholds, as the
        // 2D body already does. Sleeping on the first slow tick froze every
        // body released at rest in a 120 ticks/s project (truck_town): one
        // tick of gravity is 9.8/120 = 0.082 m/s, under the 0.1 m/s threshold,
        // so a dropped box hung in the air forever (pat-l8jg8).
        if self.can_sleep {
            let linear_threshold = 0.01;
            let angular_threshold = 0.01;
            if self.linear_velocity.length_squared() < linear_threshold
                && self.angular_velocity.length_squared() < angular_threshold
            {
                self.sleep_timer += dt;
                if self.sleep_timer >= DEFAULT_TIME_BEFORE_SLEEP {
                    self.sleeping = true;
                }
            } else {
                self.sleep_timer = 0.0;
            }
        } else {
            self.sleep_timer = 0.0;
        }
    }

    /// Velocity of this body's surface at `world_point` — the constant surface
    /// motion a `StaticBody3D`/`AnimatableBody3D` imparts to bodies in contact:
    /// `constant_linear_velocity + constant_angular_velocity × (point − pos)`.
    /// A conveyor drags contacts along its linear velocity; a turntable adds the
    /// tangential angular contribution. This is the value a friction/contact
    /// solver folds into the relative contact velocity so the surface carries
    /// resting bodies (the solver wiring is a separate step).
    pub fn surface_velocity_at(&self, world_point: Vector3) -> Vector3 {
        self.constant_linear_velocity
            + self
                .constant_angular_velocity
                .cross(world_point - self.position)
    }

    // -- Sleep API ---------------------------------------------------------

    /// Wakes the body from sleep.
    pub fn wake_up(&mut self) {
        self.sleeping = false;
        self.sleep_timer = 0.0;
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

    /// Returns the body's collision shape AABB translated into world space.
    ///
    /// Used by the broad-phase to prune candidate collision pairs. The shape's
    /// local bounding box is offset by the body's position; like the rest of
    /// the pipeline this treats shapes as axis-aligned, so for spheres it is
    /// exact and for other shapes it is a conservative bound.
    pub fn world_aabb(&self) -> Aabb {
        // ⚠️ Must cover EVERY shape, not just the primary one. The broad-phase
        // prunes on this box, so a body whose shapes sit at x = -2 and x = +2
        // would have both of them pruned away before the narrow-phase ever ran —
        // the feature would look implemented and collide with nothing.
        let mut out: Option<Aabb> = None;
        // Borrowed, not `shapes_world()`: that clones every shape (a convex
        // hull's whole point list) and the broadphase calls this for every body
        // every step — thousands of times a tick on a GridMap level.
        let primary = (
            self.position + self.rotation.xform(self.shape_offset),
            self.rotation * self.shape_rotation,
            &self.shape,
        );
        let extras = self.extra_shapes.iter().map(|(offset, rotation, shape)| {
            (
                self.position + self.rotation.xform(*offset),
                self.rotation * *rotation,
                shape,
            )
        });
        for (world_pos, world_rot, shape) in std::iter::once(primary).chain(extras) {
            let local = shape.bounding_aabb();
            // ⚠️ The box must follow the shape's ROTATION, or this silently
            // defeats the rotation support it sits next to. The broadphase
            // PRUNES pairs on this box, so a thin wall box turned 90 degrees
            // about Z — thin in Y locally, thin in X once placed — would be
            // tested against its unrotated extent, the falling body's pair
            // would be discarded before the narrowphase ever ran, and the body
            // would drop straight through a wall the narrowphase models
            // perfectly well (pat-mgixk.39).
            //
            // Conservative and exact enough: rotate all 8 corners of the local
            // box and bound them. An axis-aligned box of a rotated shape cannot
            // be tight, and over-estimating only costs a narrowphase test,
            // whereas under-estimating loses the collision entirely.
            let mut rotated = Aabb::new(
                world_pos + world_rot.xform(local.get_endpoint(0)),
                Vector3::ZERO,
            );
            for corner in 1..8u8 {
                rotated = rotated.expand(world_pos + world_rot.xform(local.get_endpoint(corner)));
            }
            let one = rotated;
            out = Some(match out {
                Some(acc) => acc.merge(one),
                None => one,
            });
        }
        // `shapes_world` always yields at least the primary shape, so the
        // fallback is unreachable; it keeps the signature total rather than
        // panicking on a future empty-shape body.
        out.unwrap_or_else(|| {
            let local = self.shape.bounding_aabb();
            Aabb::new(local.position + self.position, local.size)
        })
    }

    /// Every collision shape this body owns, paired with its WORLD-space centre.
    ///
    /// The primary shape comes first, then [`Self::extra_shapes`] in order. Each
    /// offset is rotated by the body's own rotation before being added to its
    /// position, so a tilted body carries its offset shapes around with it.
    ///
    /// Each shape's WORLD rotation is the body's rotation composed with the
    /// shape's own local one (pat-mgixk.39). ⚠️ Order matters and is not
    /// commutative: `body * local` applies the local rotation first and then
    /// the body's, which is what nesting a rotated `CollisionShape3D` under a
    /// rotated body means. Writing `local * body` gives a different orientation
    /// for any case where both are non-identity — and agrees with the correct
    /// answer whenever either one is identity, so an axis-aligned test cannot
    /// tell the two apart.
    ///
    /// Returns owned clones to match how the collision loop already consumes
    /// `shape` — it clones out of the borrow so the borrow ends before the
    /// mutable `bodies.get_mut` calls that follow.
    pub fn shapes_world(&self) -> Vec<(Vector3, Quaternion, Shape3D)> {
        let mut out = Vec::with_capacity(1 + self.extra_shapes.len());
        out.push((
            self.position + self.rotation.xform(self.shape_offset),
            self.rotation * self.shape_rotation,
            self.shape.clone(),
        ));
        for (offset, rotation, shape) in &self.extra_shapes {
            out.push((
                self.position + self.rotation.xform(*offset),
                self.rotation * *rotation,
                shape.clone(),
            ));
        }
        out
    }

    /// The centre of mass in world space: the point the body rotates about.
    pub fn center_of_mass_world(&self) -> Vector3 {
        self.position + self.rotation.xform(self.center_of_mass)
    }

    /// Returns `true` if this body is allowed to collide with `other` under
    /// Godot's layer/mask rules.
    ///
    /// Two bodies collide when either body's collision mask includes a layer
    /// the other body belongs to, i.e.
    /// `(a.mask & b.layer) != 0 || (b.mask & a.layer) != 0`, matching Godot's
    /// `CollisionObject3D::test_collision_mask`.
    pub fn should_collide_with(&self, other: &PhysicsBody3D) -> bool {
        (self.collision_mask & other.collision_layer) != 0
            || (other.collision_mask & self.collision_layer) != 0
    }

    /// Returns the inverse mass (0 for static/kinematic bodies).
    pub fn inverse_mass(&self) -> f32 {
        match self.body_type {
            BodyType3D::Rigid if self.mass > 0.0 => 1.0 / self.mass,
            _ => 0.0,
        }
    }

    // -- Inertia -----------------------------------------------------------

    /// Returns the local-space (principal-axis) moment-of-inertia diagonal
    /// `(Ixx, Iyy, Izz)` for this body's shape and mass.
    ///
    /// Collision shapes are authored on their principal axes (boxes aligned to
    /// the axes, capsules/cylinders along Y, spheres symmetric), so the inertia
    /// tensor is diagonal in local space and a single [`Vector3`] captures it.
    /// Convex/concave/plane shapes fall back to a solid sphere of the shape's
    /// bounding radius.
    pub fn local_inertia(&self) -> Vector3 {
        if let Some(i) = self.inertia_override {
            return i;
        }
        self.shape_inertia()
    }

    /// The primary shape's inertia about the body ORIGIN rather than the
    /// shape's own centre: [`Self::shape_inertia`] plus the parallel-axis term
    /// for [`Self::shape_offset`] (diagonal only; Godot's vehicle reads only the
    /// main diagonal too). This is Jolt's mass properties for a body whose
    /// `center_of_mass_mode` is CUSTOM at the origin (`OffsetCenterOfMassShape`
    /// translates the inner shape's inertia, `MassProperties::Translate`).
    ///
    /// truck_town's minivan: a 0.95 x 1 x 2.33 box 0.59 m above the origin,
    /// 40 kg, pitches about (35.5, 21.3, 20.2), not the box's own
    /// (21.4, 21.1, 6.4); with the latter it pitched 1.4x too fast onto a ramp.
    pub fn shape_inertia_about_origin(&self) -> Vector3 {
        let i = self.shape_inertia();
        let d = self.shape_offset;
        let m = self.mass.max(0.0);
        Vector3::new(
            i.x + m * (d.y * d.y + d.z * d.z),
            i.y + m * (d.x * d.x + d.z * d.z),
            i.z + m * (d.x * d.x + d.y * d.y),
        )
    }

    /// The primary shape's principal moments about its own centre.
    pub fn shape_inertia(&self) -> Vector3 {
        let m = self.mass.max(0.0);
        if m <= 0.0 {
            return Vector3::ZERO;
        }
        match &self.shape {
            Shape3D::Sphere { radius } => {
                let i = 0.4 * m * radius * radius;
                Vector3::new(i, i, i)
            }
            Shape3D::BoxShape { half_extents } => {
                let (hx, hy, hz) = (half_extents.x, half_extents.y, half_extents.z);
                Vector3::new(
                    (m / 3.0) * (hy * hy + hz * hz),
                    (m / 3.0) * (hx * hx + hz * hz),
                    (m / 3.0) * (hx * hx + hy * hy),
                )
            }
            Shape3D::CapsuleShape { radius, height }
            | Shape3D::CylinderShape { radius, height } => {
                // Solid cylinder about Y (spin axis) plus perpendicular axes.
                let (r, h) = (*radius, *height);
                let axis = 0.5 * m * r * r;
                let perp = (m / 12.0) * (3.0 * r * r + h * h);
                Vector3::new(perp, axis, perp)
            }
            _ => {
                let size = self.shape.bounding_aabb().size;
                let r = 0.5 * size.x.max(size.y).max(size.z).max(f32::EPSILON);
                let i = 0.4 * m * r * r;
                Vector3::new(i, i, i)
            }
        }
    }

    /// Returns the componentwise inverse of the local inertia diagonal.
    ///
    /// Returns [`Vector3::ZERO`] for static/kinematic bodies (they do not
    /// rotate in response to impulses).
    pub fn inverse_inertia_diagonal(&self) -> Vector3 {
        if self.body_type != BodyType3D::Rigid {
            return Vector3::ZERO;
        }
        let i = self.local_inertia();
        let inv = |x: f32| if x > 1e-9 { 1.0 / x } else { 0.0 };
        Vector3::new(inv(i.x), inv(i.y), inv(i.z))
    }

    /// Applies the world-space inverse inertia tensor to a world-space vector
    /// (a torque or an angular impulse), returning the resulting world-space
    /// angular quantity: `R · I_local⁻¹ · Rᵀ · v`.
    ///
    /// 🔑 A locked rotation axis has NO inverse inertia (pat-1unro), as in
    /// Godot, where a locked axis is infinitely stiff: `lock_rotation` zeroes
    /// the whole result and each `axis_lock_angular_*` zeroes its world axis.
    /// Otherwise the contact solver budgets part of every off-centre impulse
    /// for a rotation the lock then throws away — the impulse comes out too
    /// small, so a rotation-locked body resting on the floor (the platformer's
    /// enemies) kept most of its downward velocity every tick.
    pub fn apply_inverse_inertia(&self, world_vec: Vector3) -> Vector3 {
        if self.lock_rotation {
            return Vector3::ZERO;
        }
        let inv = self.inverse_inertia_diagonal();
        let local = self.rotation.inverse().xform(world_vec);
        let scaled = Vector3::new(local.x * inv.x, local.y * inv.y, local.z * inv.z);
        let mut out = self.rotation.xform(scaled);
        apply_axis_lock(&mut out, self.axis_lock_angular);
        out
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

    /// pat-qg3nu: angular velocity is WORLD-space. A body turned 90 degrees
    /// about Y spinning about world Z must end up rotated about world Z, not
    /// about its own local Z (which points along world -X).
    #[test]
    fn angular_velocity_integrates_in_world_space() {
        let mut b = make_rigid(Vector3::ZERO);
        let start =
            Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);
        b.rotation = start;
        b.can_sleep = false;
        b.angular_velocity = Vector3::new(0.0, 0.0, 1.0);
        for _ in 0..30 {
            b.integrate(1.0 / 120.0, Vector3::ZERO);
        }
        let want =
            (Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 0.25) * start).normalized();
        let probe = Vector3::new(0.3, 0.7, -0.2);
        let (got, exp) = (b.rotation.xform(probe), want.xform(probe));
        assert!(
            (got - exp).length() < 2e-3,
            "rotated {probe:?} to {got:?}, world-Z spin gives {exp:?}"
        );
    }

    /// pat-qg3nu: a box carried 0.59 m above a custom centre of mass at the
    /// origin turns about the origin (parallel-axis theorem), as Jolt's
    /// `OffsetCenterOfMassShape` does: truck_town's minivan box.
    #[test]
    fn shape_inertia_about_origin_adds_the_parallel_axis_term() {
        let mut b = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::BoxShape {
                half_extents: Vector3::new(0.477039, 0.5, 1.16331),
            },
            40.0,
        );
        b.shape_offset = Vector3::new(0.0, 0.588269, 0.0774262);
        let own = b.shape_inertia();
        let about = b.shape_inertia_about_origin();
        assert!(
            (own.x - 21.38).abs() < 0.05 && (own.z - 6.37).abs() < 0.05,
            "{own:?}"
        );
        assert!((about.x - 35.46).abs() < 0.05, "{about:?}");
        assert!((about.y - 21.31).abs() < 0.05, "{about:?}");
        assert!((about.z - 20.21).abs() < 0.05, "{about:?}");
        assert_eq!(
            b.local_inertia(),
            own,
            "no override: the shape's own inertia"
        );
        b.inertia_override = Some(about);
        assert_eq!(b.local_inertia(), about);
    }

    // pat-nzgdu.62: constant surface velocity (conveyor / turntable) a body
    // imparts to contacts, via `surface_velocity_at`.
    #[test]
    fn body_surface_velocity_conveyor_and_turntable() {
        // Conveyor: constant_linear_velocity drags every surface point equally.
        let mut belt = make_rigid(Vector3::ZERO);
        belt.constant_linear_velocity = Vector3::new(2.0, 0.0, 0.0);
        let v = belt.surface_velocity_at(Vector3::new(5.0, 0.0, 3.0));
        assert!(
            (v.x - 2.0).abs() < 1e-6 && v.y.abs() < 1e-6 && v.z.abs() < 1e-6,
            "conveyor surface velocity should be (2,0,0), got {v:?}"
        );

        // Turntable: ω=(0,1,0) about the center, point r=(1,0,0) -> ω×r=(0,0,-1).
        let mut table = make_rigid(Vector3::ZERO);
        table.constant_angular_velocity = Vector3::new(0.0, 1.0, 0.0);
        let t = table.surface_velocity_at(Vector3::new(1.0, 0.0, 0.0));
        assert!(
            t.x.abs() < 1e-6 && t.y.abs() < 1e-6 && (t.z + 1.0).abs() < 1e-6,
            "turntable tangential velocity should be (0,0,-1), got {t:?}"
        );

        // At the body's own center the angular term vanishes; linear remains.
        let mut both = make_rigid(Vector3::new(10.0, 0.0, 0.0));
        both.constant_linear_velocity = Vector3::new(0.0, 4.0, 0.0);
        both.constant_angular_velocity = Vector3::new(0.0, 0.0, 1.0);
        let at_center = both.surface_velocity_at(both.position);
        assert!(
            at_center.x.abs() < 1e-6 && (at_center.y - 4.0).abs() < 1e-6,
            "at-center velocity should be just the linear part (0,4,0), got {at_center:?}"
        );

        // Defaults are zero: a plain body imparts no surface motion.
        let still = make_rigid(Vector3::ZERO);
        let s = still.surface_velocity_at(Vector3::new(3.0, 3.0, 3.0));
        assert!(s.x.abs() < 1e-6 && s.y.abs() < 1e-6 && s.z.abs() < 1e-6);
    }

    // pat-nzgdu.61: RigidBody3D integrate-time properties — lock_rotation and
    // constant_force / constant_torque. (The scene-level material/bounce +
    // contact signals from the named acceptance test are a separate lane.)
    #[test]
    fn rigid_body_3d_lock_rotation_and_constant_force() {
        let dt = 1.0 / 60.0;

        // lock_rotation zeroes angular motion and freezes orientation even with
        // a pre-existing spin and an applied torque.
        let mut locked = make_rigid(Vector3::ZERO);
        locked.lock_rotation = true;
        locked.angular_velocity = Vector3::new(1.0, 2.0, 3.0);
        locked.apply_torque(Vector3::new(5.0, 0.0, 0.0));
        let rot_before = locked.rotation;
        locked.integrate(dt, Vector3::ZERO);
        assert_eq!(
            locked.angular_velocity,
            Vector3::ZERO,
            "lock_rotation must zero angular velocity"
        );
        assert_eq!(
            locked.rotation, rot_before,
            "lock_rotation must not integrate rotation"
        );

        // constant_force accelerates the body every frame (v = F/m · dt).
        let mut pushed = make_rigid(Vector3::ZERO);
        pushed.constant_force = Vector3::new(10.0, 0.0, 0.0);
        pushed.integrate(dt, Vector3::ZERO);
        let expected_vx = 10.0 / pushed.mass * dt;
        assert!(
            (pushed.linear_velocity.x - expected_vx).abs() < 1e-6,
            "constant_force must accelerate the body: got {}, want {expected_vx}",
            pushed.linear_velocity.x
        );

        // constant_torque spins up an unlocked body...
        let mut spun = make_rigid(Vector3::ZERO);
        spun.constant_torque = Vector3::new(0.0, 4.0, 0.0);
        spun.integrate(dt, Vector3::ZERO);
        assert!(
            spun.angular_velocity.y > 0.0,
            "constant_torque must spin the body up, got {:?}",
            spun.angular_velocity
        );

        // ...but lock_rotation overrides constant_torque entirely.
        let mut spun_locked = make_rigid(Vector3::ZERO);
        spun_locked.lock_rotation = true;
        spun_locked.constant_torque = Vector3::new(0.0, 4.0, 0.0);
        spun_locked.integrate(dt, Vector3::ZERO);
        assert_eq!(
            spun_locked.angular_velocity,
            Vector3::ZERO,
            "lock_rotation must override constant_torque"
        );
    }

    #[test]
    fn body_creation_defaults() {
        let body = make_rigid(Vector3::ZERO);
        assert_eq!(body.body_type, BodyType3D::Rigid);
        assert_eq!(body.linear_velocity, Vector3::ZERO);
        assert_eq!(body.rotation, Quaternion::IDENTITY);
        assert!((body.friction - 1.0).abs() < f32::EPSILON);
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

    /// pat-l8jg8: at 120 ticks/s one tick of gravity is 9.8/120 = 0.082 m/s,
    /// under the 0.1 m/s threshold, and the body used to sleep on that first
    /// tick — hanging in the air forever. Only sustained stillness
    /// (`time_before_sleep`, 0.5 s) may sleep a body.
    #[test]
    fn a_body_released_at_rest_falls_at_120_ticks() {
        let mut body = make_rigid(Vector3::new(0.0, 10.0, 0.0));
        for _ in 0..60 {
            body.integrate(1.0 / 120.0, Vector3::new(0.0, -9.8, 0.0));
        }
        assert!(!body.sleeping, "a falling body must not sleep");
        // Half a second of free fall: y = 10 - 0.5 * 9.8 * 0.5^2 (to within
        // the explicit integrator's one-step lead).
        assert!(body.position.y < 9.0, "body hung at y {}", body.position.y);
    }

    #[test]
    fn a_still_body_sleeps_only_after_time_before_sleep() {
        let mut body = make_rigid(Vector3::ZERO);
        let dt = 1.0 / 120.0;
        // 0.5 s is 60 ticks; one short of it the body is still awake.
        for _ in 0..59 {
            body.integrate(dt, Vector3::ZERO);
        }
        assert!(!body.sleeping, "asleep before time_before_sleep elapsed");
        for _ in 0..2 {
            body.integrate(dt, Vector3::ZERO);
        }
        assert!(body.sleeping, "a body still for 0.5 s should sleep");
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
    fn collision_layer_mask_defaults() {
        let body = make_rigid(Vector3::ZERO);
        assert_eq!(body.collision_layer, 1);
        assert_eq!(body.collision_mask, 1);
    }

    #[test]
    fn should_collide_with_matching_default_layers() {
        let a = make_rigid(Vector3::ZERO);
        let b = make_rigid(Vector3::ZERO);
        assert!(a.should_collide_with(&b));
    }

    #[test]
    fn should_not_collide_when_masks_disjoint() {
        let mut a = make_rigid(Vector3::ZERO);
        let mut b = make_rigid(Vector3::ZERO);
        // a is on layer 1, scans layer 1; b is on layer 2, scans layer 2.
        a.collision_layer = 0b0001;
        a.collision_mask = 0b0001;
        b.collision_layer = 0b0010;
        b.collision_mask = 0b0010;
        assert!(!a.should_collide_with(&b));
        assert!(!b.should_collide_with(&a));
    }

    #[test]
    fn should_collide_is_symmetric_and_one_directional() {
        // Godot semantics: collide if EITHER body's mask includes the other's
        // layer. Here only `a` scans `b`'s layer, `b` scans nothing of `a`.
        let mut a = make_rigid(Vector3::ZERO);
        let mut b = make_rigid(Vector3::ZERO);
        a.collision_layer = 0b0001;
        a.collision_mask = 0b0010; // a scans layer 2 (b's layer)
        b.collision_layer = 0b0010;
        b.collision_mask = 0b0100; // b scans layer 3 (nobody)
        assert!(a.should_collide_with(&b));
        assert!(b.should_collide_with(&a), "must be symmetric");
    }

    #[test]
    fn world_aabb_is_translated_by_position() {
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(5.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        let aabb = body.world_aabb();
        // Sphere local AABB is centred at origin with half-extent 1 → world box
        // spans x in [4, 6].
        assert_eq!(aabb.position, Vector3::new(4.0, -1.0, -1.0));
        assert_eq!(aabb.size, Vector3::new(2.0, 2.0, 2.0));
    }

    // -- Inertia tensor (pat-u3l8c.59) ------------------------------------

    #[test]
    fn sphere_local_inertia_is_isotropic() {
        // Solid sphere: I = 2/5 m r² on every axis.
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 2.0 },
            5.0,
        );
        let i = body.local_inertia();
        let expected = 0.4 * 5.0 * 2.0 * 2.0;
        assert!((i.x - expected).abs() < 1e-4);
        assert!((i.y - expected).abs() < 1e-4);
        assert!((i.z - expected).abs() < 1e-4);
    }

    #[test]
    fn box_local_inertia_is_anisotropic() {
        // Box half-extents (1,2,3), mass 12 →
        // Ixx = m/3 (hy²+hz²) = 4(4+9)=52, Iyy = 4(1+9)=40, Izz = 4(1+4)=20.
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::BoxShape {
                half_extents: Vector3::new(1.0, 2.0, 3.0),
            },
            12.0,
        );
        let i = body.local_inertia();
        assert!((i.x - 52.0).abs() < 1e-3, "Ixx={}", i.x);
        assert!((i.y - 40.0).abs() < 1e-3, "Iyy={}", i.y);
        assert!((i.z - 20.0).abs() < 1e-3, "Izz={}", i.z);
    }

    #[test]
    fn torque_impulse_scaled_by_inverse_inertia() {
        // A box spins less about its high-inertia axis than its low-inertia axis
        // under an equal torque impulse.
        let make = || {
            PhysicsBody3D::new(
                BodyId3D(0),
                BodyType3D::Rigid,
                Vector3::ZERO,
                Shape3D::BoxShape {
                    half_extents: Vector3::new(1.0, 2.0, 3.0),
                },
                12.0,
            )
        };
        // Ixx=52 (largest) → smallest spin; Izz=20 (smallest) → largest spin.
        let mut about_x = make();
        about_x.apply_torque_impulse(Vector3::new(10.0, 0.0, 0.0));
        let mut about_z = make();
        about_z.apply_torque_impulse(Vector3::new(0.0, 0.0, 10.0));
        assert!((about_x.angular_velocity.x - 10.0 / 52.0).abs() < 1e-4);
        assert!((about_z.angular_velocity.z - 10.0 / 20.0).abs() < 1e-4);
        assert!(
            about_z.angular_velocity.z > about_x.angular_velocity.x,
            "low-inertia axis must spin faster"
        );
    }

    #[test]
    fn static_body_has_zero_inverse_inertia() {
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        assert_eq!(body.inverse_inertia_diagonal(), Vector3::ZERO);
        assert_eq!(
            body.apply_inverse_inertia(Vector3::new(1.0, 2.0, 3.0)),
            Vector3::ZERO
        );
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
