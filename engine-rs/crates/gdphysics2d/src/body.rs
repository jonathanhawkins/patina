//! Rigid body and kinematic body types.
//!
//! Defines the physics body abstraction used by the physics world. Bodies have
//! a type (static, kinematic, or rigid), a shape, and physical properties
//! like mass, friction, and bounce. Rigid bodies support force/impulse
//! application and velocity integration.

use gdcore::math::{Transform2D, Vector2};

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

/// Godot's default `physics/2d/time_before_sleep`.
pub const DEFAULT_TIME_BEFORE_SLEEP: f32 = 0.5;
/// Godot's default `physics/2d/sleep_threshold_linear`, in pixels per second.
pub const DEFAULT_SLEEP_THRESHOLD_LINEAR: f32 = 2.0;
/// Godot's default `physics/2d/sleep_threshold_angular`: 8° per second in
/// radians (`servers/physics_2d/physics_server_2d.cpp:912`).
pub const DEFAULT_SLEEP_THRESHOLD_ANGULAR: f32 = 8.0 * std::f32::consts::PI / 180.0;

/// Godot's `CollisionShape2D.one_way_collision_margin` default, in pixels
/// (`collision_shape_2d.h`). See [`ShapeOwner2D::one_way_margin`].
pub const DEFAULT_ONE_WAY_MARGIN: f32 = 1.0;

/// One collision shape owned by a [`PhysicsBody2D`], with its transform and its
/// **own** one-way collision settings (pat-s3rrv.25).
///
/// Godot puts `one_way_collision` on the `CollisionShape2D` child, not on the
/// body, so a platformer character can carry a solid torso collider and a
/// one-way foot sensor at the same time. Patina lifted the flag to the body,
/// which made those two states mutually exclusive; this struct is where the
/// per-shape data lives instead.
///
/// ⚠️ [`transform`](Self::transform) means different things in the two places
/// this struct appears, exactly as the `(Shape2D, Transform2D)` tuple it
/// replaced did:
///
/// * in [`PhysicsBody2D::extra_shapes`] it is the collider's **local**
///   transform, relative to the body;
/// * in [`PhysicsBody2D::shape_owners`] it is the composed **world** transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeOwner2D {
    /// The collision geometry, already scaled at registration by `scale_shape`.
    pub shape: Shape2D,
    /// Local (in `extra_shapes`) or world (from `shape_owners`) transform.
    pub transform: Transform2D,
    /// Whether this shape blocks only contacts arriving against
    /// [`one_way_direction`](Self::one_way_direction).
    pub one_way_collision: bool,
    /// Outward normal of the blocking face. Godot's default is the shape's
    /// local "up", i.e. `(0, -1)`.
    pub one_way_direction: Vector2,
    /// Deepest penetration at which this one-way shape still BLOCKS
    /// (`CollisionShape2D.one_way_collision_margin`, Godot default `1.0` px).
    ///
    /// Godot rejects a one-way contact once it is deeper than
    /// `MAX(one_way_collision_margin, physics_margin)`
    /// (`godot_space_2d.cpp:949` sets `valid_depth`, `:424` discards the
    /// contact past it). A body already further inside than the margin has
    /// effectively passed through, so continuing to block it would pop it back
    /// out of a platform it is halfway through.
    ///
    /// ⚠️ Consequence, and the reason the property exists: a FAST body can
    /// cross more than the margin in a single step and so lands inside the
    /// platform and falls out of it. Raising the margin is Godot's fix, which
    /// is exactly what the `one_way_collision_margin` property is for.
    /// Ignored unless [`one_way_collision`](Self::one_way_collision) is set.
    pub one_way_margin: f32,
}

impl ShapeOwner2D {
    /// A solid (two-way) owner — the common case.
    pub fn solid(shape: Shape2D, transform: Transform2D) -> Self {
        Self {
            shape,
            transform,
            one_way_collision: false,
            one_way_direction: Vector2::new(0.0, -1.0),
            one_way_margin: DEFAULT_ONE_WAY_MARGIN,
        }
    }

    /// Whether a body travelling `motion` passes through this shape instead of
    /// being blocked by it (pat-0q2kx.113, made per-shape by pat-s3rrv.25).
    ///
    /// `one_way_direction` is the outward normal of the blocking face, so the
    /// shape is solid when the mover approaches against that normal and
    /// permeable when it travels with it.
    ///
    /// ⚠️ Zero motion is never passthrough — otherwise a character resting on a
    /// one-way platform would drop out of it the moment it stopped.
    pub fn passthrough_for(&self, motion: Vector2) -> bool {
        if !self.one_way_collision {
            return false;
        }
        let normal = self.one_way_direction.normalized();
        if normal.length_squared() < 1e-8 || motion.length_squared() < 1e-8 {
            return false;
        }
        motion.dot(normal) > 0.0
    }
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
    /// The collider's own transform relative to the body — Godot's shape-owner
    /// transform (`CollisionObject2D::shape_owner_set_transform`, which
    /// `collision_shape_2d.cpp::_update_in_shape_owner` feeds from the
    /// `CollisionShape2D` child's own `position`/`rotation`).
    ///
    /// Identity for a collider sitting at the body's origin, which is why
    /// nothing noticed it was missing: the default scene layout is the one case
    /// where ignoring it happens to be right. Author a `CollisionShape2D` with a
    /// non-zero `position` — a character whose collider sits at the feet, a
    /// muzzle hitbox — and the body used to collide at the body's origin
    /// instead, silently (pat-s3rrv.25).
    ///
    /// ⚠️ Rotation and translation only. Collider *scale* keeps going through
    /// `scale_shape` at registration, which bakes it into the shape geometry.
    /// Putting scale here would break the rigidity that `raycast_shape` relies
    /// on to round-trip a hit distance through `affine_inverse` unchanged.
    pub shape_transform: Transform2D,
    /// Collider children beyond the first, as [`ShapeOwner2D`]s carrying each
    /// child's local transform and one-way flags (pat-s3rrv.25).
    ///
    /// Godot gives a `CollisionObject2D` a list of shape owners, and Patina now
    /// consults all of them: read these through
    /// [`shape_owners`](Self::shape_owners), which every collision, sweep and
    /// query path iterates. An L-shaped body is solid across both arms.
    ///
    /// Empty for every scene that authors a single collider, so single-collider
    /// behaviour is byte-identical to before the list existed.
    ///
    /// Each entry carries its own `one_way_collision` / `one_way_direction`,
    /// read from that collider child (pat-s3rrv.25). The body-level fields of
    /// the same name belong to the PRIMARY shape, i.e. owner `0`.
    pub extra_shapes: Vec<ShapeOwner2D>,
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
    /// If true, only collide when the other body approaches from the positive
    /// normal direction.
    ///
    /// This is the PRIMARY shape's flag — owner `0` of
    /// [`shape_owners`](Self::shape_owners). Extra colliders carry their own in
    /// [`extra_shapes`](Self::extra_shapes), so a body can be solid on one
    /// collider and one-way on another (pat-s3rrv.25).
    pub one_way_collision: bool,
    /// The direction for one-way collision (default: up, i.e. negative Y).
    ///
    /// Primary shape only; see [`one_way_collision`](Self::one_way_collision).
    pub one_way_direction: Vector2,
    /// Primary shape's [`ShapeOwner2D::one_way_margin`]; extras carry their own.
    pub one_way_margin: f32,
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
    /// When true, the body's simulation is halted (Godot `RigidBody2D.freeze`):
    /// [`integrate`](Self::integrate) is a no-op, so it holds its state.
    pub freeze: bool,
    /// When true, all rotation is locked (Godot `RigidBody2D.lock_rotation`):
    /// angular velocity is forced to zero and rotation is not integrated.
    pub lock_rotation: bool,
    /// Whether the body is asleep (Godot `sleeping`). A sleeping body does not
    /// integrate until woken; the sleep *detector* lives in the world step.
    pub sleeping: bool,
    /// Whether the body may auto-sleep when at rest (Godot `can_sleep`).
    pub can_sleep: bool,
    /// Seconds a body must stay below the sleep thresholds before it falls
    /// asleep (Godot's `ProjectSettings physics/2d/time_before_sleep`, default
    /// 0.5 s) — pat-0q2kx.116.
    pub time_before_sleep: f32,
    /// Linear speed under which the body counts as still, in units per second
    /// (`physics/2d/sleep_threshold_linear`, default 2.0). Per-body rather than
    /// a module constant because the space reads it from ProjectSettings and
    /// stamps it onto its bodies (`godot_space_2d.cpp:1220`).
    pub sleep_threshold_linear: f32,
    /// Angular speed under which the body counts as still, in radians per
    /// second (`physics/2d/sleep_threshold_angular`, default 8°/s).
    pub sleep_threshold_angular: f32,
    /// How long this body has been continuously below the sleep thresholds.
    /// Reset by any wake.
    sleep_timer: f32,
    /// Constant surface linear velocity this body imparts to contacts (Godot
    /// `StaticBody2D.constant_linear_velocity` — conveyor belts). Does not move
    /// the body itself; drags contacting bodies via friction.
    pub constant_linear_velocity: Vector2,
    /// Constant surface angular velocity this body imparts to contacts (Godot
    /// `StaticBody2D.constant_angular_velocity` — turntables), radians/second.
    pub constant_angular_velocity: f32,
    /// Bodies this one passes THROUGH while still colliding with everything
    /// else — Godot's `PhysicsBody2D.add_collision_exception_with`
    /// (pat-s3rrv.100). A bullet ignoring its shooter, a rider ignoring its
    /// vehicle.
    ///
    /// ⚠️ Godot's exception is **one-way in storage but two-way in effect**:
    /// `godot_body_2d.h` keeps a per-body set, and the broadphase skips a pair
    /// when EITHER side lists the other. Storing it on one body and testing
    /// only that body would make `a.add_collision_exception_with(b)` work or
    /// not depending on which of the two the pair loop happened to visit first.
    /// [`PhysicsBody2D::excludes`] is therefore always asked of both sides —
    /// see [`crate::world::PhysicsWorld2D`].
    ///
    /// A `Vec` rather than a `HashSet` on purpose: the set is tiny (a handful
    /// per body at most), and `get_collision_exceptions()` hands the list back
    /// to a script, where a nondeterministic iteration order would make the
    /// result — and any test asserting it — differ run to run.
    pub collision_exceptions: Vec<BodyId>,
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
            shape_transform: Transform2D::IDENTITY,
            extra_shapes: Vec::new(),
            mass,
            friction: 0.5,
            bounce: 0.0,
            collision_layer: 1,
            collision_mask: 1,
            one_way_collision: false,
            one_way_direction: Vector2::new(0.0, -1.0),
            one_way_margin: DEFAULT_ONE_WAY_MARGIN,
            gravity_scale: 1.0,
            linear_damp: 0.0,
            angular_damp: 0.0,
            accumulated_force: Vector2::ZERO,
            accumulated_torque: 0.0,
            center_of_mass: Vector2::ZERO,
            freeze: false,
            lock_rotation: false,
            sleeping: false,
            time_before_sleep: DEFAULT_TIME_BEFORE_SLEEP,
            sleep_threshold_linear: DEFAULT_SLEEP_THRESHOLD_LINEAR,
            sleep_threshold_angular: DEFAULT_SLEEP_THRESHOLD_ANGULAR,
            sleep_timer: 0.0,
            can_sleep: true,
            constant_linear_velocity: Vector2::ZERO,
            constant_angular_velocity: 0.0,
            collision_exceptions: Vec::new(),
        }
    }

    /// Godot's `add_collision_exception_with`. Idempotent: adding the same body
    /// twice leaves one entry, so a script calling it every frame in
    /// `_physics_process` — which is how these are usually written — does not
    /// grow the list without bound.
    pub fn add_collision_exception(&mut self, other: BodyId) {
        if !self.collision_exceptions.contains(&other) {
            self.collision_exceptions.push(other);
        }
    }

    /// Godot's `remove_collision_exception_with`. Removing one that was never
    /// added is a no-op rather than an error, matching Godot.
    pub fn remove_collision_exception(&mut self, other: BodyId) {
        self.collision_exceptions.retain(|&b| b != other);
    }

    /// Whether this body lists `other`. This is the ONE-WAY question; callers
    /// deciding whether a pair collides must use [`Self::excludes`] instead.
    pub fn lists_collision_exception(&self, other: BodyId) -> bool {
        self.collision_exceptions.contains(&other)
    }

    /// 🔑 **The question every collision site must ask.** True when `a` and `b`
    /// must not interact because EITHER lists the other.
    ///
    /// ⚠️ Symmetric on purpose. Godot stores the exception on the body the
    /// script called the method on, but its broadphase skips the pair
    /// regardless of direction, so `bullet.add_collision_exception_with(gun)`
    /// works exactly like the reverse. Testing only one side makes the feature
    /// depend on pair-iteration order — it would appear to work in a test whose
    /// bodies happen to be visited in the lucky order and fail in a scene where
    /// they are not, which is the worst possible way for this to break.
    pub fn excludes(a: &PhysicsBody2D, b: &PhysicsBody2D) -> bool {
        a.lists_collision_exception(b.id) || b.lists_collision_exception(a.id)
    }

    /// Velocity of this body's surface at `world_point` — the constant surface
    /// motion a `StaticBody2D`/`AnimatableBody2D` imparts to bodies in contact:
    /// `constant_linear_velocity + constant_angular_velocity × (point − pos)`.
    /// A conveyor drags contacts along its linear velocity; a turntable adds the
    /// tangential angular contribution. Does not move the body itself — the
    /// friction/contact solver folds this into the relative contact velocity so
    /// the surface carries resting bodies (the solver wiring is a separate step).
    pub fn surface_velocity_at(&self, world_point: Vector2) -> Vector2 {
        let r = world_point - self.position;
        // In 2D the angular velocity ω about the center gives a tangential
        // velocity ω × r = ω · (−r.y, r.x).
        self.constant_linear_velocity + Vector2::new(-r.y, r.x) * self.constant_angular_velocity
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
    /// The world transform of this body's *shape*: the body's own transform
    /// composed with the collider's local one (pat-s3rrv.25).
    ///
    /// Every place that used to build `Transform2D::from_rotation_origin(rotation,
    /// position)` to hand the narrow-phase a world transform must use this
    /// instead, or an authored collider offset is silently dropped. The
    /// composition order is Godot's: `a * b` applies `b` first, so the shape's
    /// local transform is applied before the body's.
    ///
    /// Rigid by construction — [`shape_transform`](Self::shape_transform) carries
    /// no scale — so `affine_inverse` round-trips a distance unchanged.
    pub fn shape_world_transform(&self) -> Transform2D {
        Transform2D::from_rotation_origin(self.rotation, self.position) * self.shape_transform
    }

    /// Every collision shape this body owns, each with its **world** transform:
    /// the primary shape first, then [`extra_shapes`](Self::extra_shapes).
    ///
    /// This is the seam the whole collision surface runs on (pat-s3rrv.25) —
    /// body-vs-body, area-vs-body, area-vs-area, the character sweeps, the
    /// point/ray queries and picking's `shape_idx` all iterate it.
    ///
    /// Yields exactly what `shape_world_transform` yields for its first item, so
    /// converting a call site cannot change single-collider behaviour.
    ///
    /// Owner `0` carries the body-level `one_way_collision` /
    /// `one_way_direction`; the extras carry their own. Every solver and sweep
    /// asks the OWNER, never the body, so one collider on a body may be solid
    /// while another lets a mover through (pat-s3rrv.25).
    pub fn shape_owners(&self) -> Vec<ShapeOwner2D> {
        let body_tf = Transform2D::from_rotation_origin(self.rotation, self.position);
        let mut out = Vec::with_capacity(1 + self.extra_shapes.len());
        out.push(ShapeOwner2D {
            shape: self.shape,
            transform: body_tf * self.shape_transform,
            one_way_collision: self.one_way_collision,
            one_way_direction: self.one_way_direction,
            one_way_margin: self.one_way_margin,
        });
        for extra in &self.extra_shapes {
            out.push(ShapeOwner2D {
                transform: body_tf * extra.transform,
                ..*extra
            });
        }
        out
    }

    /// Wakes the body and restarts its sleep countdown (Godot's implicit wake
    /// on any impulse, force, or contact) — pat-0q2kx.116.
    ///
    /// Every mutation that adds energy must call this. Before this existed
    /// nothing in the engine ever cleared `sleeping`, so a body that fell
    /// asleep could never be woken by gameplay again.
    pub fn wake(&mut self) {
        self.sleeping = false;
        self.sleep_timer = 0.0;
    }

    /// Advances the sleep countdown by `dt` and puts the body to sleep once it
    /// has been below both thresholds for `time_before_sleep`.
    ///
    /// ⚠️ The countdown is what makes this correct. Sleeping the instant the
    /// speed dips below the threshold freezes a ball at the apex of its arc —
    /// where velocity legitimately passes through zero — and it hangs in the
    /// air forever. Only *sustained* stillness may sleep a body.
    fn update_sleep(&mut self, dt: f32) {
        if !self.can_sleep {
            self.sleep_timer = 0.0;
            return;
        }
        let linear_limit = self.sleep_threshold_linear;
        let still = self.linear_velocity.length_squared() < linear_limit * linear_limit
            && self.angular_velocity.abs() < self.sleep_threshold_angular;
        if still {
            self.sleep_timer += dt;
            if self.sleep_timer >= self.time_before_sleep {
                self.sleeping = true;
                self.linear_velocity = Vector2::ZERO;
                self.angular_velocity = 0.0;
            }
        } else {
            self.sleep_timer = 0.0;
        }
    }

    pub fn apply_force(&mut self, force: Vector2) {
        self.wake();
        if self.body_type == BodyType::Rigid {
            self.accumulated_force = self.accumulated_force + force;
        }
    }

    /// Applies an instantaneous impulse (directly changes velocity).
    ///
    /// Has no effect on static or kinematic bodies.
    pub fn apply_impulse(&mut self, impulse: Vector2) {
        self.wake();
        if self.body_type == BodyType::Rigid && self.mass > 0.0 {
            self.linear_velocity = self.linear_velocity + impulse * (1.0 / self.mass);
        }
    }

    /// Godot's `RigidBody2D.apply_central_impulse` — an impulse through the
    /// centre of mass, imparting no spin.
    ///
    /// The same thing [`apply_impulse`](Self::apply_impulse) already did, under
    /// the name Godot (and [`crate::body3d`]'s 3D twin) uses. Scripts and tests
    /// reaching for the documented API name were hitting a missing method
    /// (pat-0q2kx.116); the off-centre form is
    /// [`apply_impulse_at_point`](Self::apply_impulse_at_point).
    pub fn apply_central_impulse(&mut self, impulse: Vector2) {
        self.apply_impulse(impulse);
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
        self.wake();
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
        self.integrate_forces(dt, gravity);
        self.integrate_velocities(dt);
    }

    /// Godot's `integrate_forces`: damping, gravity and the accumulated
    /// force/torque go into the **velocity**. The transform is not touched.
    ///
    /// Split out from [`integrate`](Self::integrate) so the world can run the
    /// contact solver *between* the two halves, exactly as Godot does
    /// (`godot_step_2d.cpp`: `integrate_forces` → `setup` → `solve` →
    /// `integrate_velocities`). Ordering is not cosmetic here: advancing the
    /// transform before the solver hands every resting body a full tick of
    /// gravity displacement that the solver can then only cancel *next* tick,
    /// which is precisely the sink a box stack shows (pat-0q2kx.116).
    pub fn integrate_forces(&mut self, dt: f32, gravity: Vector2) {
        if self.freeze || self.sleeping {
            return;
        }
        match self.body_type {
            BodyType::Static => {}
            BodyType::Kinematic => {
                if self.lock_rotation {
                    self.angular_velocity = 0.0;
                }
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
                // `lock_rotation` zeroes all angular motion (Godot
                // `RigidBody2D.lock_rotation`) before the rotation integrates.
                if self.lock_rotation {
                    self.angular_velocity = 0.0;
                }
                self.accumulated_force = Vector2::ZERO;
                self.accumulated_torque = 0.0;
            }
        }
    }

    /// Godot's `integrate_velocities`: advances the transform by the (already
    /// solved) velocity, plus any depenetration pseudo-velocity the contact
    /// solver produced, then runs the sleep countdown.
    ///
    /// `bias_linear`/`bias_angular` move the transform and are then discarded by
    /// the caller — that is what lets an overlap be pushed apart without the
    /// push showing up as kinetic energy next tick.
    pub fn integrate_velocities_biased(
        &mut self,
        dt: f32,
        bias_linear: Vector2,
        bias_angular: f32,
    ) {
        if self.freeze || self.sleeping {
            return;
        }
        match self.body_type {
            BodyType::Static => {}
            BodyType::Kinematic => {
                self.position = self.position + self.linear_velocity * dt;
                self.rotation += self.angular_velocity * dt;
            }
            BodyType::Rigid => {
                // `lock_rotation` is re-applied here, not only in
                // `integrate_forces`: the contact solver now runs *between* the
                // two halves and will happily hand a locked body angular
                // velocity, which the old single-pass `integrate` never had to
                // guard against (pat-0q2kx.116).
                let angular = if self.lock_rotation {
                    self.angular_velocity = 0.0;
                    0.0
                } else {
                    self.angular_velocity + bias_angular
                };
                self.position = self.position + (self.linear_velocity + bias_linear) * dt;
                self.rotation += angular * dt;
                // Only a DYNAMIC body sleeps: static bodies never move and a
                // kinematic one is driven by its owner, so neither has a
                // meaningful idle state (pat-0q2kx.116).
                self.update_sleep(dt);
            }
        }
    }

    /// [`integrate_velocities_biased`](Self::integrate_velocities_biased) with
    /// no depenetration term — the plain second half of [`integrate`].
    pub fn integrate_velocities(&mut self, dt: f32) {
        self.integrate_velocities_biased(dt, Vector2::ZERO, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // pat-nzgdu.123: freeze / sleeping halt integration; lock_rotation zeroes
    // angular motion (the sleep detector + CCD live in the world step).
    #[test]
    fn rigidbody2d_freeze_sleep_lock_states() {
        let make = || {
            PhysicsBody2D::new(
                BodyId(1),
                BodyType::Rigid,
                Vector2::ZERO,
                Shape2D::Circle { radius: 1.0 },
                1.0,
            )
        };
        let gravity = Vector2::new(0.0, -10.0);

        // freeze: a frozen body neither falls nor moves, and keeps its velocity.
        let mut frozen = make();
        frozen.freeze = true;
        frozen.linear_velocity = Vector2::new(5.0, 0.0);
        frozen.integrate(1.0, gravity);
        assert_eq!(frozen.position, Vector2::ZERO, "frozen body must not move");
        assert_eq!(frozen.linear_velocity, Vector2::new(5.0, 0.0));

        // sleeping: a sleeping body does not integrate either.
        let mut asleep = make();
        asleep.sleeping = true;
        asleep.integrate(1.0, gravity);
        assert_eq!(
            asleep.position,
            Vector2::ZERO,
            "sleeping body must not move"
        );

        // lock_rotation: angular velocity is zeroed and rotation stays fixed,
        // but linear integration still runs.
        let mut locked = make();
        locked.lock_rotation = true;
        locked.angular_velocity = 3.0;
        locked.linear_velocity = Vector2::new(2.0, 0.0);
        locked.integrate(1.0, Vector2::ZERO);
        assert_eq!(
            locked.angular_velocity, 0.0,
            "lock_rotation zeroes angular velocity"
        );
        assert_eq!(locked.rotation, 0.0, "lock_rotation keeps rotation fixed");
        assert_eq!(
            locked.position.x, 2.0,
            "linear motion still runs when rotation is locked"
        );

        // A normal awake, unlocked body still rotates and falls.
        let mut normal = make();
        normal.angular_velocity = 3.0;
        normal.integrate(1.0, gravity);
        assert!(normal.rotation > 0.0, "an unlocked body rotates");
        assert!(normal.position.y < 0.0, "an awake body falls under gravity");

        // Defaults.
        let d = make();
        assert!(!d.freeze && !d.sleeping && !d.lock_rotation && d.can_sleep);
    }

    // pat-nzgdu.125: constant surface velocity (conveyor / turntable) a body
    // imparts to contacts, via `surface_velocity_at`.
    #[test]
    fn body_surface_velocity_conveyor_and_turntable_2d() {
        let make = || {
            PhysicsBody2D::new(
                BodyId(1),
                BodyType::Static,
                Vector2::ZERO,
                Shape2D::Circle { radius: 1.0 },
                1.0,
            )
        };

        // Conveyor: constant_linear_velocity drags every surface point equally.
        let mut belt = make();
        belt.constant_linear_velocity = Vector2::new(100.0, 0.0);
        assert_eq!(
            belt.surface_velocity_at(Vector2::new(5.0, 3.0)),
            Vector2::new(100.0, 0.0)
        );

        // Turntable: ω=1 (ccw) about the center; a point at (1,0) moves +Y.
        let mut table = make();
        table.constant_angular_velocity = 1.0;
        assert_eq!(
            table.surface_velocity_at(Vector2::new(1.0, 0.0)),
            Vector2::new(0.0, 1.0)
        );

        // At the body's own position the angular term vanishes; linear remains.
        let mut both = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(10.0, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        both.constant_linear_velocity = Vector2::new(0.0, 4.0);
        both.constant_angular_velocity = 2.0;
        assert_eq!(
            both.surface_velocity_at(both.position),
            Vector2::new(0.0, 4.0)
        );

        // Defaults impart no surface motion.
        let still = make();
        assert_eq!(
            still.surface_velocity_at(Vector2::new(3.0, 3.0)),
            Vector2::ZERO
        );
    }

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
        assert!(
            body.position.y.abs() < 1e-6,
            "gravity_scale=0 must not fall"
        );
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
