//! 3D physics world state and simulation stepping.

use std::collections::{BTreeMap, HashMap, HashSet};

use gdcore::math::Vector3;
use gdcore::math3d::{Aabb, Quaternion};

use crate::area3d::{
    Area3D, AreaId3D, AreaOverlapEvent3D, AreaStore3D, OverlapEvent3D, SpaceOverride,
};
use crate::body::BodyType3D;
use crate::body::{BodyId3D, ContactPoint3D, PhysicsBody3D};
use crate::collision;
use crate::joint::{Joint3D, JointId3D};
use crate::shape::Shape3D;

/// Half the largest bounding-box extent of a shape — a conservative sphere
/// radius used by the continuous-collision sweep.
fn bounding_radius(shape: &Shape3D) -> f32 {
    let size = shape.bounding_aabb().size;
    0.5 * size.x.max(size.y).max(size.z)
}

/// Sign of `v` with a dead-band around zero (so a face-aligned direction keeps
/// the support point centred on that axis rather than biased to a corner).
fn axis_sign(v: f32) -> f32 {
    if v > 1e-6 {
        1.0
    } else if v < -1e-6 {
        -1.0
    } else {
        0.0
    }
}

/// Returns the farthest point of a shape (at `pos`/`rot`) in world direction
/// `dir` — its support point. Used to place the contact roughly on the touching
/// surface so an off-centre or tilted contact produces a torque, rather than
/// the centre-line midpoint (which is always parallel to the normal and can
/// never rotate a body).
fn support_point(shape: &Shape3D, pos: Vector3, rot: Quaternion, dir: Vector3) -> Vector3 {
    let d = dir.normalized();
    match shape {
        Shape3D::Sphere { radius } => pos + d * *radius,
        Shape3D::BoxShape { half_extents } => {
            let local_dir = rot.inverse().xform(d);
            let corner = Vector3::new(
                half_extents.x * axis_sign(local_dir.x),
                half_extents.y * axis_sign(local_dir.y),
                half_extents.z * axis_sign(local_dir.z),
            );
            pos + rot.xform(corner)
        }
        Shape3D::ConvexPolygonShape { points } if !points.is_empty() => {
            let local_dir = rot.inverse().xform(d);
            let mut best = points[0];
            let mut best_dot = best.dot(local_dir);
            for &p in &points[1..] {
                let dp = p.dot(local_dir);
                if dp > best_dot {
                    best_dot = dp;
                    best = p;
                }
            }
            pos + rot.xform(best)
        }
        // Capsule/cylinder/concave/plane: approximate with the bounding sphere.
        _ => pos + d * bounding_radius(shape),
    }
}

/// Result of a 3D raycast query.
#[derive(Debug, Clone, Copy)]
pub struct RaycastHit3D {
    /// The body that was hit.
    pub body_id: BodyId3D,
    /// The hit point in world space.
    pub point: Vector3,
    /// The surface normal at the hit point.
    pub normal: Vector3,
    /// Distance from the ray origin.
    pub distance: f32,
}

// ---------------------------------------------------------------------------
// Joint solving (pat-0q2kx.118)
// ---------------------------------------------------------------------------

/// Gauss-Seidel sweeps of the velocity-level joint constraints per step. Eight
/// is enough for a short chain to hold without visible stretch and cheap enough
/// that a scene full of joints still steps in well under a frame.
const JOINT_VELOCITY_ITERATIONS: usize = 8;

/// Baumgarte factor for the *limit* constraints — how much of a limit
/// overshoot is pushed back per step. The point constraints do not use it: they
/// are corrected by projection instead (see [`PhysicsWorld3D::solve_joints`]).
const JOINT_LIMIT_BIAS: f32 = 0.2;

/// Orders a body pair so one entry covers it regardless of which side asks.
/// Resolves one damping channel over the areas a body overlaps, following
/// Godot's space-override chain (pat-s3rrv.102).
///
/// `channel` picks the mode and value being resolved, so the linear and angular
/// halves share one implementation — Godot combines them identically and only
/// the field differs.
///
/// `areas` must be sorted by ASCENDING priority; this walks it in reverse so the
/// highest-priority area is consulted first, which is what lets a `Replace`
/// there stop everything below it.
///
/// Returns `None` when no overlapping area declares a mode for this channel —
/// the signal the caller uses to leave the body's own damping alone. That is
/// why `contributed` is tracked separately from the accumulator: a `Replace`
/// with a value of `0.0` is a real override meaning "no damping here", and
/// `Some(0.0)` must not collapse into `None`.
///
/// ⚠️ Mirrors `gdphysics2d::area2d::AreaStore::compute_body_damp`, minus its
/// `default_damp` baseline: the 3D caller applies area damping as an extra
/// decay on top of `integrate`, so folding a baseline in here would damp a body
/// twice.
fn resolve_damp(
    areas: &[&Area3D],
    channel: impl Fn(&Area3D) -> (SpaceOverride, f32),
) -> Option<f32> {
    let mut damp = 0.0_f32;
    let mut contributed = false;
    let mut stopped = false;
    for area in areas.iter().rev() {
        let (mode, value) = channel(area);
        match mode {
            SpaceOverride::Disabled => continue,
            SpaceOverride::Combine => damp += value,
            SpaceOverride::CombineReplace => {
                damp += value;
                stopped = true;
            }
            SpaceOverride::Replace => {
                damp = value;
                stopped = true;
            }
            SpaceOverride::ReplaceCombine => damp = value,
        }
        contributed = true;
        if stopped {
            break;
        }
    }
    contributed.then_some(damp)
}

fn exception_key(a: BodyId3D, b: BodyId3D) -> (BodyId3D, BodyId3D) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// The `i`-th local unit axis (0 = X, 1 = Y, anything else = Z).
fn local_unit_axis(i: usize) -> Vector3 {
    match i {
        0 => Vector3::new(1.0, 0.0, 0.0),
        1 => Vector3::new(0.0, 1.0, 0.0),
        _ => Vector3::new(0.0, 0.0, 1.0),
    }
}

/// Two unit vectors completing an orthonormal basis with `axis`.
fn perpendicular_basis(axis: Vector3) -> [Vector3; 2] {
    // Pick the seed that is *least* parallel to `axis`, or the cross product
    // collapses and the basis comes back as zeros.
    let seed = if axis.x.abs() < 0.9 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let u = axis.cross(seed).normalized();
    let v = axis.cross(u).normalized();
    [u, v]
}

/// Signed distance `value` lies outside `[lower, upper]`: negative below the
/// lower bound, positive above the upper, `0.0` when in range.
fn limit_violation(value: f32, lower: f32, upper: f32) -> f32 {
    if value > upper {
        value - upper
    } else if value < lower {
        value - lower
    } else {
        0.0
    }
}

/// Impulse bounds for a one-sided limit: a violation past the *upper* bound may
/// only be pushed back (negative impulse), and vice versa. Without this a limit
/// would also stop a body that is already returning into range.
/// The positional bias a spring of `stiffness` applies to remove `error` this
/// step (pat-s3rrv.192).
///
/// ⚠️ Saturating, not linear. A raw `stiffness * error * inv_dt` bias makes a
/// stiff spring overshoot and explode: the solver drives relative velocity to
/// exactly `-bias`, so a bias larger than the error needs in one step throws the
/// body past equilibrium harder than it arrived. `k / (1 + k)` with
/// `k = stiffness * dt` maps any stiffness into "remove at most all of the error
/// this step", which is stable for every value a scene can author.
fn spring_bias(stiffness: f32, error: f32, dt: f32, inv_dt: f32) -> f32 {
    let k = (stiffness * dt).max(0.0);
    let erp = k / (1.0 + k);
    erp * error * inv_dt
}

/// Bleeds relative linear velocity along `n` toward zero by `damping`.
///
/// ⚠️ Separate from the spring's positional bias because Godot's `damping` is a
/// distinct parameter: a stiff spring with no damping is *supposed* to oscillate.
/// Folding damping into the bias would make every spring critically damped and
/// the parameter would be stored and unread.
fn damp_linear_axis(
    a: &mut JointBody,
    b: &mut JointBody,
    ra: Vector3,
    rb: Vector3,
    n: Vector3,
    damping: f32,
) {
    if damping <= 0.0 {
        return;
    }
    let k = point_effective_mass(a, b, ra, rb, n);
    if k <= 1e-9 {
        return;
    }
    let relative = (b.velocity_at(rb) - a.velocity_at(ra)).dot(n);
    let lambda = (-relative / k) * damping.clamp(0.0, 1.0);
    if lambda == 0.0 {
        return;
    }
    apply_pair_impulse(a, b, ra, rb, n * lambda);
}

/// Angular twin of [`damp_linear_axis`].
fn damp_angular_axis(a: &mut JointBody, b: &mut JointBody, n: Vector3, damping: f32) {
    if damping <= 0.0 {
        return;
    }
    let k = angular_effective_mass(a, b, n);
    if k <= 1e-9 {
        return;
    }
    let relative = (b.angular_velocity - a.angular_velocity).dot(n);
    let lambda = (-relative / k) * damping.clamp(0.0, 1.0);
    if lambda == 0.0 {
        return;
    }
    let p = n * lambda;
    a.angular_velocity = a.angular_velocity - a.inv_inertia_mul(p);
    b.angular_velocity = b.angular_velocity + b.inv_inertia_mul(p);
}

fn one_sided_bounds(violation: f32) -> (f32, f32) {
    if violation > 0.0 {
        (f32::NEG_INFINITY, 0.0)
    } else {
        (0.0, f32::INFINITY)
    }
}

/// The rotation angle of `q` about `axis`, in radians on `(-π, π]`.
///
/// Swing-twist decomposition: project the quaternion's vector part onto the
/// axis, renormalise, and read the angle back off the resulting twist
/// quaternion. `q` and `axis` must be expressed in the same frame — the callers
/// pass `a⁻¹ · b` together with an axis in A's local frame.
fn twist_angle_about(q: Quaternion, axis: Vector3) -> f32 {
    let vector = Vector3::new(q.x, q.y, q.z);
    let projected = axis * vector.dot(axis);
    let (mut x, mut y, mut z, mut w) = (projected.x, projected.y, projected.z, q.w);
    let len = (x * x + y * y + z * z + w * w).sqrt();
    if len < 1e-9 {
        return 0.0;
    }
    x /= len;
    y /= len;
    z /= len;
    w /= len;
    // Both `q` and `-q` name the same rotation; pin the sign so the angle comes
    // back on (-π, π] instead of jumping by a full turn.
    if w < 0.0 {
        x = -x;
        y = -y;
        z = -z;
        w = -w;
    }
    let sine = Vector3::new(x, y, z).dot(axis);
    2.0 * sine.atan2(w)
}

/// A body's solver-relevant state, snapshotted so both ends of a joint can be
/// read and written without ever holding two `&mut` into the body map.
#[derive(Clone, Copy)]
struct JointBody {
    position: Vector3,
    rotation: Quaternion,
    linear_velocity: Vector3,
    angular_velocity: Vector3,
    inv_mass: f32,
    inv_inertia: Vector3,
}

impl JointBody {
    fn of(body: &PhysicsBody3D) -> Self {
        Self {
            position: body.position,
            rotation: body.rotation,
            linear_velocity: body.linear_velocity,
            angular_velocity: body.angular_velocity,
            inv_mass: body.inverse_mass(),
            inv_inertia: body.inverse_inertia_diagonal(),
        }
    }

    /// `R · I_local⁻¹ · Rᵀ · v` — the world-space inverse inertia applied to an
    /// angular impulse. Mirrors [`PhysicsBody3D::apply_inverse_inertia`], on the
    /// snapshot rather than the live body.
    fn inv_inertia_mul(&self, world_vec: Vector3) -> Vector3 {
        let local = self.rotation.inverse().xform(world_vec);
        self.rotation.xform(Vector3::new(
            local.x * self.inv_inertia.x,
            local.y * self.inv_inertia.y,
            local.z * self.inv_inertia.z,
        ))
    }

    /// World velocity of the material point at body-relative offset `r`.
    fn velocity_at(&self, r: Vector3) -> Vector3 {
        self.linear_velocity + self.angular_velocity.cross(r)
    }
}

/// Effective mass of the pair along `n` for an impulse applied at the offsets
/// `ra`/`rb`: `1/mₐ + 1/m_b` plus each body's angular contribution.
fn point_effective_mass(a: &JointBody, b: &JointBody, ra: Vector3, rb: Vector3, n: Vector3) -> f32 {
    a.inv_mass
        + b.inv_mass
        + a.inv_inertia_mul(ra.cross(n)).cross(ra).dot(n)
        + b.inv_inertia_mul(rb.cross(n)).cross(rb).dot(n)
}

/// Applies `+p` to B and `-p` to A at their respective offsets.
fn apply_pair_impulse(a: &mut JointBody, b: &mut JointBody, ra: Vector3, rb: Vector3, p: Vector3) {
    a.linear_velocity = a.linear_velocity - p * a.inv_mass;
    a.angular_velocity = a.angular_velocity - a.inv_inertia_mul(ra.cross(p));
    b.linear_velocity = b.linear_velocity + p * b.inv_mass;
    b.angular_velocity = b.angular_velocity + b.inv_inertia_mul(rb.cross(p));
}

/// Drives the relative velocity of the two anchor points along `n` to zero,
/// optionally offset by `bias` and clamped to `[min, max]`.
///
/// Wide by design: the two bodies, their two anchor offsets, the axis and the
/// three scalars are all independent per call site, and bundling them into a
/// struct would only move the same eight values one line up.
#[allow(clippy::too_many_arguments)]
fn solve_linear_axis(
    a: &mut JointBody,
    b: &mut JointBody,
    ra: Vector3,
    rb: Vector3,
    n: Vector3,
    bias: f32,
    min: f32,
    max: f32,
) {
    let k = point_effective_mass(a, b, ra, rb, n);
    if k <= 1e-9 {
        return;
    }
    let relative = (b.velocity_at(rb) - a.velocity_at(ra)).dot(n);
    let lambda = ((-relative - bias) / k).clamp(min, max);
    if lambda == 0.0 {
        return;
    }
    apply_pair_impulse(a, b, ra, rb, n * lambda);
}

/// Cancels all relative velocity between the two anchor points — the shared
/// core of the pin, hinge and cone-twist constraints.
///
/// `max_impulse` mirrors Godot's `params/impulse_clamp`: `0.0` means unlimited.
fn solve_point_velocity(
    a: &mut JointBody,
    b: &mut JointBody,
    local_a: Vector3,
    local_b: Vector3,
    max_impulse: f32,
) {
    let ra = a.rotation.xform(local_a);
    let rb = b.rotation.xform(local_b);
    let (min, max) = if max_impulse > 0.0 {
        (-max_impulse, max_impulse)
    } else {
        (f32::NEG_INFINITY, f32::INFINITY)
    };
    // One world axis at a time, Gauss-Seidel: each solve sees the velocity the
    // previous one just produced, so three passes approximate the full 3×3
    // constraint without building and inverting the matrix.
    for n in [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        solve_linear_axis(a, b, ra, rb, n, 0.0, min, max);
    }
}

/// Angular effective mass of the pair about `n`.
fn angular_effective_mass(a: &JointBody, b: &JointBody, n: Vector3) -> f32 {
    n.dot(a.inv_inertia_mul(n)) + n.dot(b.inv_inertia_mul(n))
}

/// Drives the relative angular velocity about `n` toward `target`, offset by
/// `bias` and clamped to `[min, max]`.
fn solve_angular_axis(
    a: &mut JointBody,
    b: &mut JointBody,
    n: Vector3,
    target: f32,
    bias: f32,
    min: f32,
    max: f32,
) {
    let k = angular_effective_mass(a, b, n);
    if k <= 1e-9 {
        return;
    }
    let relative = (b.angular_velocity - a.angular_velocity).dot(n);
    let lambda = ((target - relative - bias) / k).clamp(min, max);
    if lambda == 0.0 {
        return;
    }
    let p = n * lambda;
    a.angular_velocity = a.angular_velocity - a.inv_inertia_mul(p);
    b.angular_velocity = b.angular_velocity + b.inv_inertia_mul(p);
}

/// Splits a relative angular-velocity step between the two bodies in proportion
/// to their inverse inertia about `axis`. A static anchor contributes nothing,
/// so the whole step lands on the moving body.
fn distribute_angular_velocity(a: &mut JointBody, b: &mut JointBody, axis: Vector3, delta: f32) {
    let ia = axis.dot(a.inv_inertia_mul(axis));
    let ib = axis.dot(b.inv_inertia_mul(axis));
    let total = ia + ib;
    if total <= 1e-9 {
        return;
    }
    a.angular_velocity = a.angular_velocity - axis * (delta * ia / total);
    b.angular_velocity = b.angular_velocity + axis * (delta * ib / total);
}

/// Moves the pair by `error` (pointing from A's anchor to B's anchor), split by
/// inverse mass so the heavier body moves less and a static body not at all.
fn distribute_position_correction(a: &mut JointBody, b: &mut JointBody, error: Vector3) {
    let total = a.inv_mass + b.inv_mass;
    if total <= 0.0 {
        return;
    }
    a.position = a.position + error * (a.inv_mass / total);
    b.position = b.position - error * (b.inv_mass / total);
}

/// Places the two anchor points exactly coincident.
///
/// Full correction, not a fraction of it: a pendulum's bob then sits at exactly
/// `|local_b|` from the pivot every step, which is what makes the radius hold
/// to a hair over hundreds of ticks instead of creeping.
fn project_point_constraint(
    a: &mut JointBody,
    b: &mut JointBody,
    local_a: Vector3,
    local_b: Vector3,
) {
    let world_a = a.position + a.rotation.xform(local_a);
    let world_b = b.position + b.rotation.xform(local_b);
    distribute_position_correction(a, b, world_b - world_a);
}

/// A 3D physics world that owns bodies and steps the simulation.
pub struct PhysicsWorld3D {
    bodies: BTreeMap<BodyId3D, PhysicsBody3D>,
    next_id: u64,
    /// Gravity acceleration applied to all rigid bodies each step.
    pub gravity: Vector3,
    /// Soft bodies stepped with gravity each frame (pat-p0s82.18).
    soft_bodies: Vec<crate::softbody3d::SoftBody3D>,
    /// Areas whose space overrides (gravity/damp) are applied during the step.
    area_store: AreaStore3D,
    /// Joints solved after integration each step (pat-u3l8c.57).
    joints: Vec<Joint3D>,
    /// Body pairs the narrow phase must ignore, keyed with the lower id first
    /// (pat-0q2kx.118). A joint whose `exclude_nodes_from_collision` is set —
    /// Godot's default — adds its pair here, because a pin that holds two
    /// bodies coincident would otherwise fight the contact solver forever.
    collision_exceptions: HashSet<(BodyId3D, BodyId3D)>,
    /// Pairs excluded by a script calling `add_collision_exception_with`
    /// (pat-s3rrv.105).
    ///
    /// ⚠️ **Deliberately a second set rather than sharing `collision_exceptions`
    /// above.** That one is owned by joints: `remove_joint` drops a pair's key
    /// once no remaining joint wants it, and it has no way to know a script also
    /// asked for the exception. Sharing the set would mean deleting a joint
    /// silently re-enables a collision the script explicitly disabled — a bug
    /// that surfaces frames later, somewhere else, as two bodies that suddenly
    /// notice each other.
    script_collision_exceptions: HashSet<(BodyId3D, BodyId3D)>,
}

impl PhysicsWorld3D {
    /// Creates an empty 3D physics world with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            bodies: BTreeMap::new(),
            next_id: 1,
            gravity: Vector3::new(0.0, -9.8, 0.0),
            soft_bodies: Vec::new(),
            area_store: AreaStore3D::new(),
            joints: Vec::new(),
            collision_exceptions: HashSet::new(),
            script_collision_exceptions: HashSet::new(),
        }
    }

    // -- Joint management (pat-u3l8c.57) -----------------------------------

    /// Adds a joint to be solved each step. Returns its [`JointId3D`] (the id
    /// the joint was created with).
    ///
    /// A joint carrying `exclude_nodes_from_collision` (Godot's default) also
    /// registers a collision exception for its pair, so the two jointed bodies
    /// stop colliding with each other (pat-0q2kx.118).
    pub fn add_joint(&mut self, joint: Joint3D) -> JointId3D {
        let id = joint.id();
        let base = joint.base();
        if base.exclude_nodes_from_collision {
            self.collision_exceptions
                .insert(exception_key(base.body_a, base.body_b));
        }
        self.joints.push(joint);
        id
    }

    /// Removes a joint by id, releasing any collision exception it added.
    ///
    /// Returns the joint when one was registered. Freeing the constraint is
    /// what lets a bob fall once its joint node leaves the tree
    /// (pat-0q2kx.118).
    pub fn remove_joint(&mut self, id: JointId3D) -> Option<Joint3D> {
        let index = self.joints.iter().position(|j| j.id() == id)?;
        let joint = self.joints.remove(index);
        let (a, b) = joint.bodies();
        let key = exception_key(a, b);
        // Only drop the exception when no *other* joint still wants it.
        let still_wanted = self.joints.iter().any(|j| {
            let base = j.base();
            base.exclude_nodes_from_collision && exception_key(base.body_a, base.body_b) == key
        });
        if !still_wanted {
            self.collision_exceptions.remove(&key);
        }
        Some(joint)
    }

    /// Number of joints registered in the world.
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Returns a reference to a joint by id.
    pub fn get_joint(&self, id: JointId3D) -> Option<&Joint3D> {
        self.joints.iter().find(|j| j.id() == id)
    }

    /// Returns a mutable reference to a joint by id, so an authored parameter
    /// change (a motor speed, a limit) reaches the solver without rebuilding
    /// the joint.
    pub fn get_joint_mut(&mut self, id: JointId3D) -> Option<&mut Joint3D> {
        self.joints.iter_mut().find(|j| j.id() == id)
    }

    /// Godot's `PhysicsBody3D.add_collision_exception_with` (pat-s3rrv.105).
    ///
    /// Idempotent, and symmetric: the pair is keyed by `exception_key`, so
    /// `a.add_collision_exception_with(b)` also stops `b` colliding with `a`.
    /// Godot stores the exception on one body and consults it from both sides,
    /// which is the same observable behaviour.
    pub fn add_collision_exception(&mut self, a: BodyId3D, b: BodyId3D) {
        self.script_collision_exceptions.insert(exception_key(a, b));
    }

    /// Godot's `PhysicsBody3D.remove_collision_exception_with`.
    ///
    /// Removing a pair that was never excluded is a no-op, matching Godot.
    ///
    /// ⚠️ Only clears a SCRIPT-added exception. A pair held apart by a joint's
    /// `exclude_nodes_from_collision` stays held apart — that exclusion belongs
    /// to the joint, and letting a script delete it would leave a pin fighting
    /// the contact solver.
    pub fn remove_collision_exception(&mut self, a: BodyId3D, b: BodyId3D) {
        self.script_collision_exceptions
            .remove(&exception_key(a, b));
    }

    /// Returns `true` when the two bodies are excluded from colliding with each
    /// other — by a joint, or by a script.
    ///
    /// The solver consults this one predicate, so a script exception and a joint
    /// exclusion are indistinguishable at the point where it matters.
    pub fn has_collision_exception(&self, a: BodyId3D, b: BodyId3D) -> bool {
        let key = exception_key(a, b);
        self.collision_exceptions.contains(&key) || self.script_collision_exceptions.contains(&key)
    }

    // -- Area management ---------------------------------------------------

    /// Adds an [`Area3D`] whose space overrides affect overlapping rigid
    /// bodies during the step, returning its ID.
    pub fn add_area(&mut self, area: Area3D) -> AreaId3D {
        self.area_store.add_area(area)
    }

    /// Removes an area by ID.
    pub fn remove_area(&mut self, id: AreaId3D) -> Option<Area3D> {
        self.area_store.remove_area(id)
    }

    /// Returns a reference to an area by ID.
    pub fn get_area(&self, id: AreaId3D) -> Option<&Area3D> {
        self.area_store.get_area(id)
    }

    /// Every area currently overlapping `id`. Delegates to
    /// [`AreaStore3D::overlapping_areas`], the same way every other area
    /// accessor on this world does — the store is private, so the scene bridge
    /// has no other route to the overlap set.
    pub fn overlapping_areas(&self, id: AreaId3D) -> Vec<AreaId3D> {
        self.area_store.overlapping_areas(id)
    }

    /// Returns a mutable reference to an area by ID.
    pub fn get_area_mut(&mut self, id: AreaId3D) -> Option<&mut Area3D> {
        self.area_store.get_area_mut(id)
    }

    /// Returns the number of areas in the world.
    pub fn area_count(&self) -> usize {
        self.area_store.area_count()
    }

    /// Runs the area store's overlap detection against the world's current
    /// bodies, returning this step's `(body enter/exit, area enter/exit)` events.
    ///
    /// The area store is private to the world, so this is how a scene-level
    /// server reaches enter/exit detection to turn into `body_entered` /
    /// `area_entered` signals (pat-0q2kx.35). Call it after [`step`](Self::step);
    /// events are relative to the previous call, so calling it twice per step
    /// reports the second pair of frames as "no change".
    pub fn detect_overlaps(&mut self) -> (Vec<OverlapEvent3D>, Vec<AreaOverlapEvent3D>) {
        // `detect_body_overlaps` wants a map and only reads its values; the
        // bodies live in a `BTreeMap`, so clone into the expected shape — the
        // same per-step clone the 2D server does.
        let body_map: HashMap<BodyId3D, PhysicsBody3D> = self
            .bodies
            .iter()
            .map(|(&id, body)| (id, body.clone()))
            .collect();
        let body_events = self.area_store.detect_body_overlaps(&body_map);
        let area_events = self.area_store.detect_area_overlaps();
        (body_events, area_events)
    }

    /// Register a soft body for simulation.
    pub fn add_soft_body(&mut self, body: crate::softbody3d::SoftBody3D) {
        self.soft_bodies.push(body);
    }

    /// Soft body count.
    pub fn soft_body_count(&self) -> usize {
        self.soft_bodies.len()
    }

    /// Mutable access to soft bodies.
    pub fn soft_bodies_mut(&mut self) -> &mut [crate::softbody3d::SoftBody3D] {
        &mut self.soft_bodies
    }

    /// Adds a body to the world and returns its unique ID.
    pub fn add_body(&mut self, mut body: PhysicsBody3D) -> BodyId3D {
        let id = BodyId3D(self.next_id);
        self.next_id += 1;
        body.id = id;
        self.bodies.insert(id, body);
        id
    }

    /// Removes a body from the world by ID.
    pub fn remove_body(&mut self, id: BodyId3D) -> Option<PhysicsBody3D> {
        self.bodies.remove(&id)
    }

    /// Returns a reference to a body by ID.
    pub fn get_body(&self, id: BodyId3D) -> Option<&PhysicsBody3D> {
        self.bodies.get(&id)
    }

    /// Returns a mutable reference to a body by ID.
    pub fn get_body_mut(&mut self, id: BodyId3D) -> Option<&mut PhysicsBody3D> {
        self.bodies.get_mut(&id)
    }

    /// Returns the number of bodies in the world.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Returns an iterator over all bodies in the world.
    pub fn bodies(&self) -> impl Iterator<Item = &PhysicsBody3D> {
        self.bodies.values()
    }

    /// Steps the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let gravity = self.gravity;

        // Soft bodies integrate with the same gravity (pat-p0s82.18).
        for sb in &mut self.soft_bodies {
            sb.step(dt, gravity);
        }

        // 0. Clear contacts from previous step.
        for body in self.bodies.values_mut() {
            body.clear_contacts();
        }

        // Record pre-integration positions of continuous-CD bodies so we can
        // sweep their motion for tunnelling after integration.
        let ccd_prev: Vec<(BodyId3D, Vector3)> = self
            .bodies
            .values()
            .filter(|b| b.continuous_cd && b.body_type == BodyType3D::Rigid && !b.sleeping)
            .map(|b| (b.id, b.position))
            .collect();

        // 1. Integrate all rigid bodies, applying Area3D gravity/damp overrides
        //    for any body overlapping an area with an active space override.
        let ids: Vec<BodyId3D> = self.bodies.keys().copied().collect();
        for id in &ids {
            let (eff_gravity, lin_damp, ang_damp) = self.effective_space_params(*id);
            if let Some(body) = self.bodies.get_mut(id) {
                body.integrate(dt, eff_gravity);
                if let Some(d) = lin_damp {
                    let factor = (1.0 - d * dt).max(0.0);
                    body.linear_velocity = body.linear_velocity * factor;
                }
                if let Some(d) = ang_damp {
                    let factor = (1.0 - d * dt).max(0.0);
                    body.angular_velocity = body.angular_velocity * factor;
                }
            }
        }

        // 1b. Continuous collision detection: clamp fast movers that stepped
        //     clean through a static/kinematic body so they cannot tunnel.
        for (id, prev) in ccd_prev {
            self.ccd_clamp(id, prev);
        }

        // 2. Broad-phase: prune candidate pairs whose world-space AABBs overlap
        //    via sweep-and-prune (O(n log n + k) instead of O(n^2) all-pairs).
        //    Each candidate is then filtered by collision layer/mask before the
        //    narrow-phase test and impulse resolution.
        let pairs = self.broadphase_pairs();
        for (id_a, id_b) in pairs {
            let (should_collide, pos_a, rot_a, shape_a, pos_b, rot_b, shape_b) = {
                let a = &self.bodies[&id_a];
                let b = &self.bodies[&id_b];
                (
                    a.should_collide_with(b),
                    a.position,
                    a.rotation,
                    a.shape.clone(),
                    b.position,
                    b.rotation,
                    b.shape.clone(),
                )
            };

            // Collision layer/mask filter (Godot semantics): skip the pair
            // unless either body's mask includes a layer the other belongs to.
            if !should_collide {
                continue;
            }

            // A joint with `exclude_nodes_from_collision` set takes the pair out
            // of the narrow phase entirely (pat-0q2kx.118) — a pin holds the two
            // anchors coincident, which the contact solver would otherwise read
            // as a permanent deep overlap and fight every step.
            if self.has_collision_exception(id_a, id_b) {
                continue;
            }

            let result =
                collision::test_collision_rot(pos_a, rot_a, &shape_a, pos_b, rot_b, &shape_b);
            if result.colliding {
                // Contact point on the touching surfaces: midpoint of each
                // body's support point toward the other. This is offset from a
                // body's centre of mass for tilted/edge contacts, so the normal
                // impulse can induce rotation (a tumbling box), unlike a plain
                // centre-line midpoint.
                let support_a = support_point(&shape_a, pos_a, rot_a, result.normal);
                let support_b = support_point(&shape_b, pos_b, rot_b, result.normal * -1.0);
                let contact_pos = (support_a + support_b) * 0.5;
                if let Some(a) = self.bodies.get_mut(&id_a) {
                    a.add_contact(ContactPoint3D {
                        position: contact_pos,
                        normal: result.normal,
                        depth: result.depth,
                        other_body: id_b,
                    });
                }
                if let Some(b) = self.bodies.get_mut(&id_b) {
                    b.add_contact(ContactPoint3D {
                        position: contact_pos,
                        normal: result.normal * -1.0,
                        depth: result.depth,
                        other_body: id_a,
                    });
                }

                let a_inv = self.bodies[&id_a].inverse_mass();
                let b_inv = self.bodies[&id_b].inverse_mass();
                let total_inv = a_inv + b_inv;

                if total_inv > 0.0 {
                    // Position correction.
                    let correction = result.normal * result.depth;
                    if let Some(a) = self.bodies.get_mut(&id_a) {
                        a.position = a.position - correction * (a_inv / total_inv);
                    }
                    if let Some(b) = self.bodies.get_mut(&id_b) {
                        b.position = b.position + correction * (b_inv / total_inv);
                    }

                    // Impulse-based velocity response with restitution and
                    // friction. Contact impulses are applied at `contact_pos`
                    // (offset from each body's centre of mass), so an off-centre
                    // contact induces angular velocity through the inverse
                    // inertia tensor — a box resting past a ledge tumbles.
                    let n = result.normal;
                    let ra = contact_pos - pos_a;
                    let rb = contact_pos - pos_b;
                    let vel_a = self.bodies[&id_a].linear_velocity;
                    let vel_b = self.bodies[&id_b].linear_velocity;
                    let ang_a = self.bodies[&id_a].angular_velocity;
                    let ang_b = self.bodies[&id_b].angular_velocity;
                    // Relative velocity at the contact point (linear + rotational).
                    let vp_a = vel_a + ang_a.cross(ra);
                    let vp_b = vel_b + ang_b.cross(rb);
                    let relative_vel = vp_a - vp_b;
                    let vel_along_normal = relative_vel.dot(n);

                    // Only resolve if bodies are approaching at the contact.
                    if vel_along_normal > 0.0 {
                        // Godot: `CLAMP(A.bounce + B.bounce, 0, 1)`
                        // (`godot_body_pair_3d.cpp:254`) — the SAME formula the
                        // 2D pair uses, and not a minimum. A minimum makes the
                        // dullest surface win, so a bouncy ball on a bouncy
                        // floor barely rebounds (pat-s3rrv.99).
                        let restitution =
                            (self.bodies[&id_a].bounce + self.bodies[&id_b].bounce).clamp(0.0, 1.0);

                        // Effective mass along the normal including the angular
                        // contribution of each body: 1/m_a + 1/m_b +
                        // n·((I_a⁻¹(r_a×n))×r_a) + n·((I_b⁻¹(r_b×n))×r_b).
                        let raxn = ra.cross(n);
                        let rbxn = rb.cross(n);
                        let ang_term_a = self.bodies[&id_a]
                            .apply_inverse_inertia(raxn)
                            .cross(ra)
                            .dot(n);
                        let ang_term_b = self.bodies[&id_b]
                            .apply_inverse_inertia(rbxn)
                            .cross(rb)
                            .dot(n);
                        let k = total_inv + ang_term_a + ang_term_b;
                        if k <= 0.0 {
                            continue;
                        }

                        let j = -(1.0 + restitution) * vel_along_normal / k;
                        let impulse = n * j;

                        // Apply the normal impulse to linear and angular state.
                        if let Some(a) = self.bodies.get_mut(&id_a) {
                            a.linear_velocity = a.linear_velocity + impulse * a_inv;
                            let dw = a.apply_inverse_inertia(ra.cross(impulse));
                            a.angular_velocity = a.angular_velocity + dw;
                        }
                        if let Some(b) = self.bodies.get_mut(&id_b) {
                            b.linear_velocity = b.linear_velocity - impulse * b_inv;
                            let dw = b.apply_inverse_inertia(rb.cross(impulse));
                            b.angular_velocity = b.angular_velocity - dw;
                        }

                        // Friction impulse (tangential), applied to linear and
                        // angular state as well.
                        let tangent = relative_vel - n * vel_along_normal;
                        let tangent_len = tangent.length();
                        if tangent_len > 1e-8 {
                            let tangent_dir = tangent * (1.0 / tangent_len);
                            // Godot: `abs(MIN(A.friction, B.friction))`
                            // (`godot_body_pair_3d.cpp:258`), not a geometric
                            // mean. The two agree only when the coefficients
                            // are equal, which is what a same-material fixture
                            // uses — so the mean looks right until two
                            // different materials meet.
                            let friction_coeff = self.bodies[&id_a]
                                .friction
                                .min(self.bodies[&id_b].friction)
                                .abs();
                            // Effective mass along the tangent direction.
                            let raxt = ra.cross(tangent_dir);
                            let rbxt = rb.cross(tangent_dir);
                            let kt = total_inv
                                + self.bodies[&id_a]
                                    .apply_inverse_inertia(raxt)
                                    .cross(ra)
                                    .dot(tangent_dir)
                                + self.bodies[&id_b]
                                    .apply_inverse_inertia(rbxt)
                                    .cross(rb)
                                    .dot(tangent_dir);
                            let jt = if kt > 0.0 { -tangent_len / kt } else { 0.0 };
                            // Coulomb friction clamp.
                            let friction_impulse = if jt.abs() < j.abs() * friction_coeff {
                                tangent_dir * jt
                            } else {
                                tangent_dir * (-j.abs() * friction_coeff)
                            };

                            if let Some(a) = self.bodies.get_mut(&id_a) {
                                a.linear_velocity = a.linear_velocity + friction_impulse * a_inv;
                                let dw = a.apply_inverse_inertia(ra.cross(friction_impulse));
                                a.angular_velocity = a.angular_velocity + dw;
                            }
                            if let Some(b) = self.bodies.get_mut(&id_b) {
                                b.linear_velocity = b.linear_velocity - friction_impulse * b_inv;
                                let dw = b.apply_inverse_inertia(rb.cross(friction_impulse));
                                b.angular_velocity = b.angular_velocity - dw;
                            }
                        }
                    }
                }
            }
        }

        // Solve joint constraints (pin position correction + hinge motor)
        // after integration and contact resolution (pat-u3l8c.57).
        self.solve_joints(dt);
    }

    /// Applies the registered joints to their bodies for this step
    /// (pat-0q2kx.118).
    ///
    /// Three passes, in Godot's order: motors first (they set the velocity the
    /// constraints then have to respect), then `JOINT_VELOCITY_ITERATIONS`
    /// Gauss-Seidel sweeps of the velocity-level constraints, then one
    /// positional projection sweep.
    ///
    /// Splitting velocity from position ("split impulse") is what keeps a
    /// pendulum tight: the velocity pass never sees the accumulated drift, so it
    /// injects no energy, and the projection pass then places the anchors
    /// exactly coincident — leaving the bob at a fixed radius from the pivot
    /// instead of the slow spiral a bias-only correction gives.
    ///
    /// The joint list is moved out for the duration so each pass can take `&mut
    /// self` to reach the bodies; it is put back before returning.
    fn solve_joints(&mut self, dt: f32) {
        if self.joints.is_empty() {
            return;
        }
        let joints = std::mem::take(&mut self.joints);

        for j in &joints {
            if j.is_enabled() {
                self.apply_joint_motor(j, dt);
            }
        }
        for _ in 0..JOINT_VELOCITY_ITERATIONS {
            for j in &joints {
                if j.is_enabled() {
                    self.solve_joint_velocity(j, dt);
                }
            }
        }
        for j in &joints {
            if j.is_enabled() {
                self.project_joint_position(j);
            }
        }

        self.joints = joints;
    }

    /// Snapshots both of a joint's bodies, or `None` when either is missing or
    /// the joint names the same body twice (which would double-apply every
    /// write-back).
    fn joint_bodies(&self, joint: &Joint3D) -> Option<(BodyId3D, JointBody, BodyId3D, JointBody)> {
        let (id_a, id_b) = joint.bodies();
        if id_a == id_b {
            return None;
        }
        match (self.bodies.get(&id_a), self.bodies.get(&id_b)) {
            (Some(a), Some(b)) => Some((id_a, JointBody::of(a), id_b, JointBody::of(b))),
            _ => None,
        }
    }

    /// Writes a solved snapshot's velocities back. Static and kinematic bodies
    /// are scene-driven, so a joint may never push them around.
    fn write_joint_velocity(&mut self, id: BodyId3D, state: &JointBody) {
        if let Some(body) = self.bodies.get_mut(&id) {
            if body.body_type == BodyType3D::Rigid {
                body.linear_velocity = state.linear_velocity;
                body.angular_velocity = state.angular_velocity;
            }
        }
    }

    /// Writes a projected snapshot's position back, under the same rule.
    fn write_joint_position(&mut self, id: BodyId3D, state: &JointBody) {
        if let Some(body) = self.bodies.get_mut(&id) {
            if body.body_type == BodyType3D::Rigid {
                body.position = state.position;
            }
        }
    }

    /// Runs a hinge motor for one step: drives the *relative* angular velocity
    /// about the hinge axis toward the target, splitting the step between the
    /// two bodies by their inverse inertia. Against a static anchor the whole
    /// step lands on body B, which is the case a scene actually authors.
    ///
    /// Once per step rather than once per iteration — `compute_motor_impulse`
    /// already scales by `dt`, so running it inside the Gauss-Seidel loop would
    /// multiply the motor's authority by the iteration count.
    fn apply_joint_motor(&mut self, joint: &Joint3D, dt: f32) {
        let Joint3D::Hinge(h) = joint else {
            return;
        };
        if !h.motor_enabled {
            return;
        }
        let Some((id_a, mut a, id_b, mut b)) = self.joint_bodies(joint) else {
            return;
        };
        let axis = a.rotation.xform(h.axis).normalized();
        if axis.length_squared() < 0.5 {
            return; // degenerate axis
        }
        let relative = (b.angular_velocity - a.angular_velocity).dot(axis);
        let delta = h.compute_motor_impulse(relative, dt);
        distribute_angular_velocity(&mut a, &mut b, axis, delta);
        self.write_joint_velocity(id_a, &a);
        self.write_joint_velocity(id_b, &b);
    }

    /// One Gauss-Seidel sweep of a joint's velocity-level constraints.
    fn solve_joint_velocity(&mut self, joint: &Joint3D, dt: f32) {
        let Some((id_a, mut a, id_b, mut b)) = self.joint_bodies(joint) else {
            return;
        };
        let inv_dt = if dt > 1e-6 { 1.0 / dt } else { 0.0 };

        match joint {
            Joint3D::Pin(p) => {
                solve_point_velocity(&mut a, &mut b, p.local_a, p.local_b, p.impulse_clamp);
            }
            Joint3D::Hinge(h) => {
                solve_point_velocity(&mut a, &mut b, h.anchor_a, h.anchor_b, 0.0);
                // Axis alignment: the hinge axis as carried by A and as carried
                // by B must stay parallel, so only rotation *about* it survives.
                let axis_a = a.rotation.xform(h.axis).normalized();
                let axis_b = b.rotation.xform(h.axis).normalized();
                let misalignment = axis_a.cross(axis_b);
                for n in perpendicular_basis(axis_a) {
                    let bias = JOINT_LIMIT_BIAS * misalignment.dot(n) * inv_dt;
                    solve_angular_axis(
                        &mut a,
                        &mut b,
                        n,
                        0.0,
                        bias,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    );
                }
                if h.use_limit {
                    let angle =
                        twist_angle_about(a.rotation.inverse() * b.rotation, h.axis.normalized());
                    let violation = limit_violation(angle, h.lower_limit, h.upper_limit);
                    if violation != 0.0 {
                        let (min, max) = one_sided_bounds(violation);
                        let bias = h.limit_bias * violation * inv_dt;
                        solve_angular_axis(&mut a, &mut b, axis_a, 0.0, bias, min, max);
                    }
                }
            }
            Joint3D::Slider(s) => {
                let axis = a.rotation.xform(s.axis).normalized();
                let ra = a.rotation.xform(s.local_a);
                let rb = b.rotation.xform(s.local_b);
                let offset = ((b.position + rb) - (a.position + ra)).dot(axis);
                // Off-axis translation is governed by the `linear_ortho`
                // triple; travel along the axis is the one free linear degree of
                // freedom, governed by `linear_motion` (pat-s3rrv.192).
                //
                // 🔑 `solve_linear_axis` drives the relative velocity to
                // `-bias`, so passing `-(1 - f) * rel` leaves exactly the
                // fraction `f` of it removed. That reproduces the reference's
                // `damping * rel_vel` impulse term
                // (`godot_slider_joint_3d.cpp:152`) without a second solver
                // path — and at Godot's defaults (softness 1.0, damping 1.0)
                // `f == 1`, so `bias == 0` and this is bit-identical to the
                // unconditional solve it replaces.
                let ortho_lin = (s.linear_ortho_softness * s.linear_ortho_damping).clamp(0.0, 1.0);
                for n in perpendicular_basis(axis) {
                    let rel = (b.velocity_at(rb) - a.velocity_at(ra)).dot(n);
                    solve_linear_axis(
                        &mut a,
                        &mut b,
                        ra,
                        rb,
                        n,
                        -(1.0 - ortho_lin) * rel,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    );
                }
                // ⚠️ The reference picks the limit triple ONLY while the axis is
                // actually against a limit, and the motion triple otherwise —
                // `(i) ? Ortho : (solveLinLim ? Lim : Dir)`. Applying both, or
                // applying motion damping during a limit violation, would fight
                // the limit impulse.
                let lin_violation = if s.use_linear_limit {
                    limit_violation(offset, s.linear_limit_lower, s.linear_limit_upper)
                } else {
                    0.0
                };
                if lin_violation != 0.0 {
                    let (min, max) = one_sided_bounds(lin_violation);
                    let bias = JOINT_LIMIT_BIAS * lin_violation * inv_dt;
                    solve_linear_axis(&mut a, &mut b, ra, rb, axis, bias, min, max);
                } else {
                    // ⚠️ `linear_motion/damping` defaults to 0.0, so this arm is
                    // inert unless the author asks for it. That zero is what
                    // makes a slider slide.
                    let f = (s.linear_motion_softness * s.linear_motion_damping).clamp(0.0, 1.0);
                    if f > 0.0 {
                        let rel = (b.velocity_at(rb) - a.velocity_at(ra)).dot(axis);
                        solve_linear_axis(
                            &mut a,
                            &mut b,
                            ra,
                            rb,
                            axis,
                            -(1.0 - f) * rel,
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                        );
                    }
                }
                // Rotation off the slide axis is constrained the same way the
                // hinge does it; rotation about the axis is limited, not locked.
                let axis_b = b.rotation.xform(s.axis).normalized();
                let misalignment = axis.cross(axis_b);
                // The `angular_ortho` triple scales how hard the off-axis
                // misalignment is removed; the reference folds the same product
                // into `velrelOrthog` at `godot_slider_joint_3d.cpp:207`. The
                // positional `bias` stays untouched — that is the restitution
                // term, not the damping one.
                let ortho_ang =
                    (s.angular_ortho_softness * s.angular_ortho_damping).clamp(0.0, 1.0);
                for n in perpendicular_basis(axis) {
                    let bias = JOINT_LIMIT_BIAS * misalignment.dot(n) * inv_dt;
                    let rel = (b.angular_velocity - a.angular_velocity).dot(n);
                    solve_angular_axis(
                        &mut a,
                        &mut b,
                        n,
                        (1.0 - ortho_ang) * rel,
                        bias,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    );
                }
                let ang_violation = if s.use_angular_limit {
                    let angle =
                        twist_angle_about(a.rotation.inverse() * b.rotation, s.axis.normalized());
                    limit_violation(angle, s.angular_limit_lower, s.angular_limit_upper)
                } else {
                    0.0
                };
                if ang_violation != 0.0 {
                    let (min, max) = one_sided_bounds(ang_violation);
                    let bias = JOINT_LIMIT_BIAS * ang_violation * inv_dt;
                    solve_angular_axis(&mut a, &mut b, axis, 0.0, bias, min, max);
                } else {
                    // Free rotation about the axis: the `angular_motion` triple,
                    // mirroring the reference's `m_solveAngLim ? Lim : Dir`
                    // selection at :227-231.
                    let f = (s.angular_motion_softness * s.angular_motion_damping).clamp(0.0, 1.0);
                    if f > 0.0 {
                        let rel = (b.angular_velocity - a.angular_velocity).dot(axis);
                        solve_angular_axis(
                            &mut a,
                            &mut b,
                            axis,
                            (1.0 - f) * rel,
                            0.0,
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                        );
                    }
                }
            }
            Joint3D::ConeTwist(c) => {
                solve_point_velocity(&mut a, &mut b, c.local_a, c.local_b, 0.0);
                let axis_a = a.rotation.xform(c.twist_axis).normalized();
                let axis_b = b.rotation.xform(c.twist_axis).normalized();
                // Swing: the angle between the two twist axes, clamped to the
                // cone half-angle.
                let swing = axis_a.dot(axis_b).clamp(-1.0, 1.0).acos();
                let excess = c.swing_violation(swing);
                if excess > 0.0 {
                    let swing_axis = axis_a.cross(axis_b).normalized();
                    if swing_axis.length_squared() > 0.5 {
                        let bias = c.bias * excess * inv_dt;
                        solve_angular_axis(
                            &mut a,
                            &mut b,
                            swing_axis,
                            0.0,
                            bias,
                            f32::NEG_INFINITY,
                            0.0,
                        );
                    }
                }
                let twist =
                    twist_angle_about(a.rotation.inverse() * b.rotation, c.twist_axis.normalized());
                let violation = limit_violation(twist, -c.twist_span, c.twist_span);
                if violation != 0.0 {
                    let (min, max) = one_sided_bounds(violation);
                    let bias = c.bias * violation * inv_dt;
                    solve_angular_axis(&mut a, &mut b, axis_a, 0.0, bias, min, max);
                }
            }
            Joint3D::Generic6Dof(g) => {
                let ra = a.rotation.xform(g.local_a);
                let rb = b.rotation.xform(g.local_b);
                let offset = (b.position + rb) - (a.position + ra);
                let relative = a.rotation.inverse() * b.rotation;
                for axis in 0..3 {
                    let local = local_unit_axis(axis);
                    let n = a.rotation.xform(local).normalized();

                    let violation = g.linear_violation(axis, offset.dot(n));
                    if violation != 0.0 {
                        let (min, max) = one_sided_bounds(violation);
                        let bias = JOINT_LIMIT_BIAS * violation * inv_dt;
                        solve_linear_axis(&mut a, &mut b, ra, rb, n, bias, min, max);
                    }

                    let angle = twist_angle_about(relative, local);
                    let violation = g.angular_violation(axis, angle);
                    if violation != 0.0 {
                        let (min, max) = one_sided_bounds(violation);
                        let bias = JOINT_LIMIT_BIAS * violation * inv_dt;
                        solve_angular_axis(&mut a, &mut b, n, 0.0, bias, min, max);
                    }

                    // -- motors (pat-s3rrv.192) ------------------------------
                    //
                    // A motor drives the RELATIVE velocity toward its target,
                    // with the impulse capped by `force_limit`. Force is a rate,
                    // so the per-step impulse cap is `force_limit * dt` — using
                    // the force directly would make the cap depend on the tick
                    // rate, and a 120 Hz world twice as strong as a 60 Hz one.
                    let m = g.linear_motors[axis];
                    if m.enabled && m.force_limit > 0.0 {
                        let cap = m.force_limit * dt;
                        // `solve_linear_axis` computes `(-relative - bias) / k`,
                        // so a bias of `-target` drives toward `target` exactly
                        // as the angular solver's own `target` argument does.
                        solve_linear_axis(&mut a, &mut b, ra, rb, n, -m.target_velocity, -cap, cap);
                    }
                    let m = g.angular_motors[axis];
                    if m.enabled && m.force_limit > 0.0 {
                        let cap = m.force_limit * dt;
                        solve_angular_axis(&mut a, &mut b, n, m.target_velocity, 0.0, -cap, cap);
                    }

                    // -- springs ---------------------------------------------
                    //
                    // A spring pulls the axis back toward its equilibrium rather
                    // than merely stopping it at a limit, so the bias comes from
                    // the POSITION error and the bounds are two-sided — a
                    // one-sided clamp like the limits use could only ever push
                    // one way and the spring would not return.
                    let sp = g.linear_springs[axis];
                    if sp.enabled && sp.stiffness > 0.0 {
                        // ⚠️ Measured from body B's ORIGIN to body A's anchor, NOT
                        // between the two anchor frames (pat-s3rrv.192).
                        //
                        // `offset` above is frame-to-frame, and both frames derive
                        // from the same joint node — so it is EXACTLY zero at joint
                        // creation, for every rig, by construction. A spring fed
                        // that error can never move anything: it reads the authored
                        // pose as its own equilibrium and sits there. The symptom is
                        // a body that neither drifts nor sags but stays at its start
                        // value to the last digit.
                        //
                        // Subtracting `rb` removes B's own anchor arm, leaving where
                        // B's origin sits relative to A's anchor along this axis —
                        // which is what the acceptance means by pulling a displaced
                        // body back to the ANCHOR PLANE, and what `equilibrium_point`
                        // is measured against.
                        //
                        // ⚠️ Deliberately NOT applied to the limits above or to the
                        // angular spring below. A limit is a frame-to-frame
                        // constraint and is correct as it stands, and the angular
                        // error already comes from the bodies' RELATIVE rotation, so
                        // it never cancels the way the linear one does.
                        let spring_offset = offset - rb;
                        let error = spring_offset.dot(n) - sp.equilibrium_point;
                        let bias = spring_bias(sp.stiffness, error, dt, inv_dt);
                        solve_linear_axis(
                            &mut a,
                            &mut b,
                            ra,
                            rb,
                            n,
                            bias,
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                        );
                        damp_linear_axis(&mut a, &mut b, ra, rb, n, sp.damping);
                    }
                    let sp = g.angular_springs[axis];
                    if sp.enabled && sp.stiffness > 0.0 {
                        let error = angle - sp.equilibrium_point;
                        let bias = spring_bias(sp.stiffness, error, dt, inv_dt);
                        solve_angular_axis(
                            &mut a,
                            &mut b,
                            n,
                            0.0,
                            bias,
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                        );
                        damp_angular_axis(&mut a, &mut b, n, sp.damping);
                    }
                }
            }
        }

        self.write_joint_velocity(id_a, &a);
        self.write_joint_velocity(id_b, &b);
    }

    /// Places a joint's anchors back where the constraint says they belong,
    /// after the velocity pass has run.
    fn project_joint_position(&mut self, joint: &Joint3D) {
        let Some((id_a, mut a, id_b, mut b)) = self.joint_bodies(joint) else {
            return;
        };
        match joint {
            Joint3D::Pin(p) => project_point_constraint(&mut a, &mut b, p.local_a, p.local_b),
            Joint3D::Hinge(h) => project_point_constraint(&mut a, &mut b, h.anchor_a, h.anchor_b),
            Joint3D::ConeTwist(c) => project_point_constraint(&mut a, &mut b, c.local_a, c.local_b),
            Joint3D::Slider(s) => {
                // Only the off-axis drift is projected out — the whole point of a
                // slider is that motion along the axis is free.
                let axis = a.rotation.xform(s.axis).normalized();
                let ra = a.rotation.xform(s.local_a);
                let rb = b.rotation.xform(s.local_b);
                let error = (b.position + rb) - (a.position + ra);
                let perpendicular = error - axis * error.dot(axis);
                distribute_position_correction(&mut a, &mut b, perpendicular);
            }
            // 6-DOF limits are corrected by their velocity bias alone: with six
            // independent one-sided limits there is no single point to project
            // onto, and projecting each axis in turn would fight the others.
            Joint3D::Generic6Dof(_) => return,
        }
        self.write_joint_position(id_a, &a);
        self.write_joint_position(id_b, &b);
    }

    /// Computes the effective space parameters for a body this step by folding
    /// in every [`Area3D`] it overlaps.
    ///
    /// Returns the effective *space* gravity (before the body's own
    /// `gravity_scale`) plus optional linear/angular damping overrides.
    ///
    /// ⚠️ Godot treats gravity, linear damp and angular damp as THREE
    /// independent channels, each with its own five-way `SpaceOverride` mode
    /// resolved by `priority`. This used to fold damping in unconditionally for
    /// any area whose *gravity* override happened to be active, which is two
    /// bugs at once: an area could damp without asking to, and a damp-only area
    /// was filtered out before its mode was read (pat-s3rrv.102). The damp
    /// channels now go through [`resolve_damp`]; gravity keeps its own arm
    /// below, where `Replace` discards the accumulated value and `Combine` adds
    /// to it.
    fn effective_space_params(&self, id: BodyId3D) -> (Vector3, Option<f32>, Option<f32>) {
        let body = match self.bodies.get(&id) {
            Some(b) => b,
            None => return (self.gravity, None, None),
        };
        if body.body_type != BodyType3D::Rigid {
            return (self.gravity, None, None);
        }
        if self.area_store.area_count() == 0 {
            return (self.gravity, None, None);
        }

        // ⚠️ NOT filtered on `gravity_space_override` (pat-s3rrv.102). Godot
        // resolves gravity, linear damp and angular damp as three INDEPENDENT
        // channels, so an area that overrides only damping has
        // `gravity_space_override == Disabled` and was previously dropped here
        // — before its damp mode was ever consulted. Each channel applies its
        // own `Disabled` check below instead; the gravity arm already no-ops on
        // `Disabled`, so widening this filter leaves gravity unchanged.
        let mut overlapping: Vec<&Area3D> = self
            .area_store
            .areas()
            .filter(|a| (a.collision_mask & body.collision_layer) != 0)
            .filter(|a| {
                collision::test_collision(a.position, &a.shape, body.position, &body.shape)
                    .colliding
            })
            .collect();
        if overlapping.is_empty() {
            return (self.gravity, None, None);
        }
        // Highest priority applied last so it wins a `Replace`.
        overlapping.sort_by(|x, y| {
            x.priority
                .partial_cmp(&y.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut gravity = self.gravity;
        for area in &overlapping {
            let g = area.gravity_vector(body.position);
            match area.gravity_space_override {
                SpaceOverride::Replace | SpaceOverride::ReplaceCombine => gravity = g,
                SpaceOverride::Combine | SpaceOverride::CombineReplace => gravity = gravity + g,
                SpaceOverride::Disabled => {}
            }
        }

        let lin_damp = resolve_damp(&overlapping, |a| {
            (a.linear_damp_space_override, a.linear_damp)
        });
        let ang_damp = resolve_damp(&overlapping, |a| {
            (a.angular_damp_space_override, a.angular_damp)
        });
        (gravity, lin_damp, ang_damp)
    }

    /// Continuous collision detection for a body that moved from `prev` to its
    /// current position this step. If the swept motion (approximated as a
    /// sphere sweep) crosses a static or kinematic body it did not start inside,
    /// the body is clamped to the entry point so it cannot tunnel through thin
    /// geometry in a single large step.
    fn ccd_clamp(&mut self, id: BodyId3D, prev: Vector3) {
        let (new_pos, r_self, self_layer, self_mask) = match self.bodies.get(&id) {
            Some(b) => (
                b.position,
                bounding_radius(&b.shape),
                b.collision_layer,
                b.collision_mask,
            ),
            None => return,
        };
        let motion = new_pos - prev;
        let dist = motion.length();
        if dist < 1e-5 {
            return;
        }
        let dir = motion * (1.0 / dist);

        let mut earliest = dist;
        let mut hit = false;
        for other in self.bodies.values() {
            if other.id == id || other.body_type == BodyType3D::Rigid {
                continue;
            }
            // Godot layer/mask semantics (symmetric).
            if (self_mask & other.collision_layer) == 0 && (other.collision_mask & self_layer) == 0
            {
                continue;
            }
            let r = r_self + bounding_radius(&other.shape);
            let oc = prev - other.position;
            let c = oc.dot(oc) - r * r;
            if c <= 0.0 {
                // Already overlapping at the start of the step — the regular
                // narrow-phase/resolution pass handles this, not CCD.
                continue;
            }
            let b = oc.dot(dir);
            if b > 0.0 {
                continue; // moving away from this body
            }
            let disc = b * b - c;
            if disc < 0.0 {
                continue; // sweep misses the expanded sphere
            }
            let t = -b - disc.sqrt();
            if t > 1e-4 && t < earliest {
                earliest = t;
                hit = true;
            }
        }

        if hit {
            if let Some(body) = self.bodies.get_mut(&id) {
                body.position = prev + dir * earliest;
            }
        }
    }

    /// Broad-phase sweep-and-prune: returns candidate body pairs whose
    /// world-space AABBs overlap, sorted by ascending id so downstream
    /// resolution is deterministic.
    ///
    /// Bodies are sorted along the X axis; each body is only compared against
    /// neighbours that still overlap it on X, so a scene of well-separated
    /// bodies costs O(n log n + k) (k = overlapping pairs) rather than the
    /// O(n^2) all-pairs scan. A real collision always implies AABB overlap, so
    /// pruning never drops a colliding pair.
    fn broadphase_pairs(&self) -> Vec<(BodyId3D, BodyId3D)> {
        let mut entries: Vec<(BodyId3D, Aabb)> = self
            .bodies
            .values()
            .map(|b| (b.id, b.world_aabb()))
            .collect();
        entries.sort_by(|a, b| {
            a.1.position
                .x
                .partial_cmp(&b.1.position.x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut pairs: Vec<(BodyId3D, BodyId3D)> = Vec::new();
        for (i, &(id_i, aabb_i)) in entries.iter().enumerate() {
            let i_max_x = aabb_i.position.x + aabb_i.size.x;
            for &(id_j, aabb_j) in entries.iter().skip(i + 1) {
                // Sorted on X: once a neighbour begins beyond i's max X, no
                // later neighbour can overlap on X either — stop scanning.
                if aabb_j.position.x > i_max_x {
                    break;
                }
                if aabb_i.intersects(aabb_j) {
                    if id_i.0 <= id_j.0 {
                        pairs.push((id_i, id_j));
                    } else {
                        pairs.push((id_j, id_i));
                    }
                }
            }
        }

        // Deterministic ascending-id order, matching the prior all-pairs scan.
        pairs.sort_by(|x, y| (x.0 .0, x.1 .0).cmp(&(y.0 .0, y.1 .0)));
        pairs
    }

    /// Casts a ray and returns the closest hit (sphere bodies only).
    pub fn raycast(&self, origin: Vector3, direction: Vector3) -> Option<RaycastHit3D> {
        let dir = direction.normalized();
        let mut closest: Option<RaycastHit3D> = None;

        for body in self.bodies.values() {
            if let Shape3D::Sphere { radius } = body.shape {
                let oc = origin - body.position;
                let a = dir.dot(dir);
                let b = 2.0 * oc.dot(dir);
                let c = oc.dot(oc) - radius * radius;
                let discriminant = b * b - 4.0 * a * c;

                if discriminant >= 0.0 {
                    let t = (-b - discriminant.sqrt()) / (2.0 * a);
                    if t > 0.0 {
                        let point = origin + dir * t;
                        let normal = (point - body.position).normalized();
                        let hit = RaycastHit3D {
                            body_id: body.id,
                            point,
                            normal,
                            distance: t,
                        };
                        if closest.as_ref().map_or(true, |c| t < c.distance) {
                            closest = Some(hit);
                        }
                    }
                }
            }
        }

        closest
    }
}

impl Default for PhysicsWorld3D {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere_body(pos: Vector3) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        )
    }

    #[test]
    fn add_and_get_body() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::ZERO));
        assert!(world.get_body(id).is_some());
        assert_eq!(world.body_count(), 1);
    }

    #[test]
    fn remove_body() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::ZERO));
        let removed = world.remove_body(id);
        assert!(removed.is_some());
        assert_eq!(world.body_count(), 0);
    }

    // -- Area3D damp space overrides (pat-s3rrv.102) -------------------------

    fn damp_area(mode: SpaceOverride, damp: f32, priority: f32) -> Area3D {
        let mut a = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 5.0 });
        a.linear_damp_space_override = mode;
        a.linear_damp = damp;
        a.priority = priority;
        a
    }

    /// 🔑 `Disabled` MEANS NO DAMPING, NOT "DAMP WITH `linear_damp`".
    ///
    /// This is the whole defect: the resolver used to `max()` every overlapping
    /// area's `linear_damp` together and never look at the mode, so an area
    /// authored `linear_damp_space_override = 0` damped anyway — and
    /// `linear_damp` defaults to 0.1, so EVERY area silently damped.
    #[test]
    fn area_damp_disabled_contributes_nothing() {
        let area = damp_area(SpaceOverride::Disabled, 5.0, 0.0);
        let areas = [&area];
        assert_eq!(
            resolve_damp(&areas, |a| (a.linear_damp_space_override, a.linear_damp)),
            None,
            "a Disabled channel must not damp at all"
        );
    }

    /// 🔑 `Combine` ADDS, AND STACKS ACROSS AREAS.
    #[test]
    fn area_damp_combine_adds_together() {
        let low = damp_area(SpaceOverride::Combine, 2.0, 0.0);
        let high = damp_area(SpaceOverride::Combine, 3.0, 1.0);
        // Ascending priority, as `effective_space_params` sorts them.
        let areas = [&low, &high];
        assert_eq!(
            resolve_damp(&areas, |a| (a.linear_damp_space_override, a.linear_damp)),
            Some(5.0),
            "two Combine areas must sum"
        );
    }

    /// 🔑 `Replace` ON THE HIGHEST-PRIORITY AREA STOPS THE CHAIN.
    ///
    /// ⚠️ The priority ORDER is what this really tests. `resolve_damp` walks the
    /// ascending-priority slice in reverse, so the 5.0 area is consulted first
    /// and its `Replace` discards the lower one. Sorting the wrong way round
    /// would yield 2.0 here and still look like a working override.
    #[test]
    fn area_damp_replace_takes_highest_priority_and_stops() {
        let low = damp_area(SpaceOverride::Combine, 2.0, 0.0);
        let high = damp_area(SpaceOverride::Replace, 7.0, 1.0);
        let areas = [&low, &high];
        assert_eq!(
            resolve_damp(&areas, |a| (a.linear_damp_space_override, a.linear_damp)),
            Some(7.0),
            "Replace must win outright, not sum to 9.0"
        );
    }

    /// 🔑 `ReplaceCombine` REPLACES BUT KEEPS WALKING.
    ///
    /// The distinction from `Replace` is only observable with a lower area
    /// present: 7.0 replaces the accumulator, then the 2.0 `Combine` below adds
    /// to it. A resolver that treated the two Replace modes alike returns 7.0.
    #[test]
    fn area_damp_replace_combine_keeps_walking() {
        let low = damp_area(SpaceOverride::Combine, 2.0, 0.0);
        let high = damp_area(SpaceOverride::ReplaceCombine, 7.0, 1.0);
        let areas = [&low, &high];
        assert_eq!(
            resolve_damp(&areas, |a| (a.linear_damp_space_override, a.linear_damp)),
            Some(9.0),
            "ReplaceCombine must replace then keep combining"
        );
    }

    /// 🔑 A `Replace` OF ZERO IS AN OVERRIDE, NOT AN ABSENCE.
    ///
    /// ⚠️ `Some(0.0)` and `None` mean different things to the caller — "this
    /// zone explicitly cancels damping" versus "no zone had an opinion". A
    /// resolver that returned `None` for a zero accumulator would collapse them,
    /// which is why `contributed` is tracked separately from the value.
    #[test]
    fn area_damp_zero_replace_is_some_not_none() {
        let area = damp_area(SpaceOverride::Replace, 0.0, 0.0);
        let areas = [&area];
        assert_eq!(
            resolve_damp(&areas, |a| (a.linear_damp_space_override, a.linear_damp)),
            Some(0.0),
            "an explicit zero override must not read as 'no override'"
        );
    }

    /// 🔑 THE ACCEPTANCE, AT THE WORLD LEVEL: DISABLED leaves a body's speed
    /// alone while COMBINE slows it.
    ///
    /// ⚠️ Gravity is zeroed so the ONLY thing that can change the speed is
    /// damping. With gravity on, a falling body speeds up regardless and the
    /// comparison would measure the wrong thing.
    #[test]
    fn area_damp_slows_a_body_only_when_the_mode_is_active() {
        fn speed_after(mode: SpaceOverride) -> f32 {
            let mut world = PhysicsWorld3D::new();
            // `gravity` is a public field, not a setter — there is no
            // `PhysicsWorld3D::set_gravity`, and calling one made this whole
            // lib test target fail to compile (E0599).
            world.gravity = Vector3::ZERO;
            let mut area = damp_area(mode, 5.0, 0.0);
            area.gravity_space_override = SpaceOverride::Disabled;
            world.add_area(area);

            let mut body = sphere_body(Vector3::ZERO);
            body.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
            let id = world.add_body(body);

            world.step(1.0 / 60.0);
            world.get_body(id).unwrap().linear_velocity.length()
        }

        let disabled = speed_after(SpaceOverride::Disabled);
        assert!(
            (disabled - 10.0).abs() < 1e-4,
            "a DISABLED damp override must leave the speed untouched, got {disabled}"
        );

        let combined = speed_after(SpaceOverride::Combine);
        assert!(
            combined < disabled - 0.1,
            "a COMBINE damp override must slow the body: {combined} vs {disabled}"
        );
    }

    // -- Joint constraint solving (pat-u3l8c.57) ----------------------------

    fn small_sphere(pos: Vector3, ty: BodyType3D, mass: f32) -> PhysicsBody3D {
        PhysicsBody3D::new(BodyId3D(0), ty, pos, Shape3D::Sphere { radius: 0.1 }, mass)
    }

    #[test]
    fn add_joint_registers_and_is_retrievable() {
        let mut world = PhysicsWorld3D::new();
        let a = world.add_body(sphere_body(Vector3::ZERO));
        let b = world.add_body(sphere_body(Vector3::new(0.0, -3.0, 0.0)));
        let jid = JointId3D(7);
        world.add_joint(Joint3D::Pin(crate::joint::PinJoint3D::new(jid, a, b)));
        assert_eq!(world.joint_count(), 1);
        assert!(world.get_joint(jid).is_some());
    }

    #[test]
    fn pin_joint_pulls_body_toward_shared_pivot() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO; // isolate the constraint from gravity
        let a = world.add_body(small_sphere(Vector3::ZERO, BodyType3D::Static, 0.0));
        let b = world.add_body(small_sphere(
            Vector3::new(2.0, 0.0, 0.0),
            BodyType3D::Rigid,
            1.0,
        ));
        // Default pin: pivots at both body centres — the constraint wants the
        // two centres coincident, so the rigid B is pulled toward the static A.
        world.add_joint(Joint3D::Pin(crate::joint::PinJoint3D::new(
            JointId3D(1),
            a,
            b,
        )));

        let x0 = world.get_body(b).unwrap().position.x; // 2.0
        for _ in 0..40 {
            world.step(1.0 / 60.0);
        }
        let x1 = world.get_body(b).unwrap().position.x;
        assert!(x1 < x0, "pin should pull B toward A (x {x1} < {x0})");
        assert!(
            x1 < 1.0,
            "pin should substantially close the 2.0 gap, got {x1}"
        );
    }

    #[test]
    fn pin_joint_holds_a_swinging_bob_at_a_fixed_radius() {
        // The pin's real job is not "pull them together" but "keep this offset
        // point coincident while everything else moves" — a bias-only nudge
        // satisfies the first and fails the second (pat-0q2kx.118).
        let mut world = PhysicsWorld3D::new();
        let anchor = world.add_body(small_sphere(Vector3::ZERO, BodyType3D::Static, 0.0));
        let bob = world.add_body(small_sphere(
            Vector3::new(2.0, 0.0, 0.0),
            BodyType3D::Rigid,
            1.0,
        ));
        if let Some(b) = world.get_body_mut(bob) {
            b.can_sleep = false;
        }
        // Pivot at the anchor: 2 m out along the bob's local -X.
        let pin = crate::joint::PinJoint3D::new(JointId3D(1), anchor, bob)
            .with_pivots(Vector3::ZERO, Vector3::new(-2.0, 0.0, 0.0));
        world.add_joint(Joint3D::Pin(pin));

        let mut swung = false;
        for tick in 0..300 {
            world.step(1.0 / 60.0);
            let pos = world.get_body(bob).unwrap().position;
            let radius = pos.length();
            assert!(
                (radius - 2.0).abs() < 1e-2,
                "tick {tick}: radius drifted to {radius}"
            );
            if pos.y < -0.5 {
                swung = true;
            }
        }
        assert!(swung, "the bob must swing down, not hang rigid");
    }

    #[test]
    fn joint_excludes_its_pair_from_collision_and_releases_it_on_removal() {
        let mut world = PhysicsWorld3D::new();
        let a = world.add_body(sphere_body(Vector3::ZERO));
        let b = world.add_body(sphere_body(Vector3::ZERO));
        let jid = JointId3D(3);
        world.add_joint(Joint3D::Pin(crate::joint::PinJoint3D::new(jid, a, b)));
        assert!(
            world.has_collision_exception(a, b),
            "exclude_nodes_from_collision defaults to true, so the pair must be \
             excluded"
        );
        // The key is order-independent.
        assert!(world.has_collision_exception(b, a));

        assert!(world.remove_joint(jid).is_some());
        assert_eq!(world.joint_count(), 0);
        assert!(
            !world.has_collision_exception(a, b),
            "removing the joint must release its exception"
        );
        assert!(world.remove_joint(jid).is_none(), "removal is idempotent");
    }

    #[test]
    fn hinge_motor_drives_angular_velocity_toward_target() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        // A far away so it never collides with B; the motor only drives B.
        let a = world.add_body(sphere_body(Vector3::new(100.0, 0.0, 0.0)));
        let b = world.add_body(sphere_body(Vector3::ZERO));
        let hinge = crate::joint::HingeJoint3D::new(JointId3D(1), a, b)
            .with_axis(Vector3::new(0.0, 1.0, 0.0))
            .with_motor(5.0, 100.0);
        world.add_joint(Joint3D::Hinge(hinge));

        for _ in 0..180 {
            world.step(1.0 / 60.0);
        }
        let wy = world.get_body(b).unwrap().angular_velocity.y;
        assert!(wy > 1.0, "motor should spin B up about +Y, got {wy}");
        assert!(
            wy <= 5.05,
            "motor should not overshoot its target, got {wy}"
        );
    }

    #[test]
    fn step_applies_gravity() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::new(0.0, 10.0, 0.0)));
        world.step(1.0 / 60.0);
        let body = world.get_body(id).unwrap();
        assert!(body.position.y < 10.0, "body should fall");
    }

    #[test]
    fn collision_separates_overlapping_spheres() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let id_a = world.add_body(sphere_body(Vector3::ZERO));
        let id_b = world.add_body(sphere_body(Vector3::new(1.0, 0.0, 0.0)));
        world.step(1.0 / 60.0);
        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        let dist = (b.position - a.position).length();
        assert!(
            dist >= 1.9,
            "spheres should be separated after collision, dist={dist}"
        );
    }

    #[test]
    fn raycast_hits_sphere() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        world.add_body(sphere_body(Vector3::new(0.0, 0.0, 10.0)));
        let hit = world.raycast(Vector3::ZERO, Vector3::new(0.0, 0.0, 1.0));
        assert!(hit.is_some());
        let h = hit.unwrap();
        assert!((h.distance - 9.0).abs() < 1e-3);
    }

    #[test]
    fn raycast_misses() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        world.add_body(sphere_body(Vector3::new(10.0, 0.0, 0.0)));
        let hit = world.raycast(Vector3::ZERO, Vector3::new(0.0, 0.0, 1.0));
        assert!(hit.is_none());
    }

    #[test]
    fn deterministic_stepping() {
        let run = || {
            let mut world = PhysicsWorld3D::new();
            let id = world.add_body(sphere_body(Vector3::new(0.0, 100.0, 0.0)));
            for _ in 0..60 {
                world.step(1.0 / 60.0);
            }
            world.get_body(id).unwrap().position
        };
        let a = run();
        let b = run();
        assert_eq!(a, b, "simulation must be deterministic");
    }

    #[test]
    fn bounce_reverses_velocity() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        // Rigid ball moving right toward a static wall.
        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.bounce = 1.0; // perfect bounce
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let ball_id = world.add_body(ball);

        // Static wall at x=1.5 (overlap with sphere at x=0, radius 1 + radius 1 = 2 > 1.5 → colliding)
        let mut wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        wall.bounce = 1.0; // match ball's bounce for perfect reflection
        world.add_body(wall);

        world.step(1.0 / 60.0);

        let body = world.get_body(ball_id).unwrap();
        // Ball should have reversed its X velocity (or at least be moving left after bounce).
        assert!(
            body.linear_velocity.x < 0.0,
            "ball should bounce back, got vx={}",
            body.linear_velocity.x
        );
    }

    #[test]
    fn zero_bounce_absorbs_velocity() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.bounce = 0.0;
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let ball_id = world.add_body(ball);

        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        world.add_body(wall);

        world.step(1.0 / 60.0);

        let body = world.get_body(ball_id).unwrap();
        // With zero bounce, normal velocity should be absorbed (near zero or slightly negative from separation).
        assert!(
            body.linear_velocity.x.abs() < 1.0,
            "zero-bounce should absorb velocity, got vx={}",
            body.linear_velocity.x
        );
    }

    #[test]
    fn contact_recording_after_collision() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.contact_monitor = true;
        ball.max_contacts_reported = 4;
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let ball_id = world.add_body(ball);

        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        let wall_id = world.add_body(wall);

        world.step(1.0 / 60.0);

        let body = world.get_body(ball_id).unwrap();
        assert!(body.get_contact_count() > 0, "should record contact");
        let colliders = body.get_colliding_bodies();
        assert!(colliders.contains(&wall_id));
    }

    #[test]
    fn static_body_not_moved_by_collision() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        world.add_body(ball);

        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        let wall_id = world.add_body(wall);

        world.step(1.0 / 60.0);

        let wall_body = world.get_body(wall_id).unwrap();
        assert!(
            (wall_body.position.x - 1.5).abs() < 1e-3,
            "static body should not move"
        );
    }

    #[test]
    fn get_body_mut_modifies() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::ZERO));
        if let Some(body) = world.get_body_mut(id) {
            body.linear_velocity = Vector3::new(5.0, 0.0, 0.0);
        }
        assert!((world.get_body(id).unwrap().linear_velocity.x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn default_gravity() {
        let world = PhysicsWorld3D::new();
        assert!((world.gravity.y - (-9.8)).abs() < 1e-5);
    }

    // -- Area space overrides (pat-u3l8c.59) ------------------------------

    #[test]
    fn area_management_add_get_remove() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_area(Area3D::new(
            AreaId3D(0),
            Vector3::ZERO,
            Shape3D::Sphere { radius: 5.0 },
        ));
        assert_eq!(world.area_count(), 1);
        assert!(world.get_area(id).is_some());
        world.get_area_mut(id).unwrap().gravity = 3.0;
        assert!((world.get_area(id).unwrap().gravity - 3.0).abs() < 1e-6);
        assert!(world.remove_area(id).is_some());
        assert_eq!(world.area_count(), 0);
    }

    #[test]
    fn area_gravity_override_redirects_body() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::new(0.0, -9.8, 0.0);
        // A body that would normally fall.
        let id = world.add_body(sphere_body(Vector3::new(0.0, 1.0, 0.0)));
        // An area that replaces gravity with a strong upward field.
        let mut area = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 20.0 });
        area.gravity_space_override = SpaceOverride::Replace;
        area.gravity = 30.0;
        area.gravity_direction = Vector3::new(0.0, 1.0, 0.0);
        world.add_area(area);

        world.step(1.0 / 60.0);
        let b = world.get_body(id).unwrap();
        assert!(
            b.linear_velocity.y > 0.0,
            "upward area gravity should redirect the body upward, vy={}",
            b.linear_velocity.y
        );
    }

    #[test]
    fn area_without_override_leaves_gravity_alone() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::new(0.0, -9.8, 0.0);
        let id = world.add_body(sphere_body(Vector3::new(0.0, 1.0, 0.0)));
        // Disabled override → default gravity still applies (body falls).
        let mut area = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 20.0 });
        area.gravity_space_override = SpaceOverride::Disabled;
        area.gravity = 30.0;
        area.gravity_direction = Vector3::new(0.0, 1.0, 0.0);
        world.add_area(area);

        world.step(1.0 / 60.0);
        assert!(world.get_body(id).unwrap().linear_velocity.y < 0.0);
    }

    #[test]
    fn area_damp_override_slows_body() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut body = sphere_body(Vector3::new(0.0, 1.0, 0.0));
        body.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        body.can_sleep = false;
        let id = world.add_body(body);
        // ⚠️ UPDATED by pat-s3rrv.102. This used to set only
        // `gravity_space_override` and still expect damping, which is the exact
        // conflation that bead fixed: Godot resolves gravity and linear damp as
        // INDEPENDENT channels, and `linear_damp_space_override` defaults to
        // DISABLED. The test's intent — "an active area damp override slows a
        // body" — is unchanged; only its premise is corrected, because the
        // damp channel now has to be switched on to damp.
        let mut area = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 20.0 });
        area.gravity_space_override = SpaceOverride::Combine;
        area.gravity = 0.0;
        area.linear_damp_space_override = SpaceOverride::Combine;
        area.linear_damp = 5.0;
        world.add_area(area);

        world.step(1.0 / 60.0);
        let vx = world.get_body(id).unwrap().linear_velocity.x;
        assert!(
            vx < 10.0 && vx > 0.0,
            "area damp should slow but not reverse, vx={vx}"
        );
    }

    // -- Continuous collision detection (pat-u3l8c.59) --------------------

    #[test]
    fn ccd_prevents_tunneling_through_static_wall() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        // A fast bullet that would step from x=-5 to x=+5 in one frame, clean
        // through a thin static wall at the origin.
        let mut bullet = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(-5.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 0.1 },
            1.0,
        );
        bullet.linear_velocity = Vector3::new(600.0, 0.0, 0.0); // 10 units / (1/60)s
        bullet.continuous_cd = true;
        bullet.can_sleep = false;
        let id = world.add_body(bullet);
        world.add_body(PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 0.2 },
            0.0,
        ));

        world.step(1.0 / 60.0);
        let x = world.get_body(id).unwrap().position.x;
        assert!(
            x < 0.5,
            "CCD must stop the bullet at the wall (x={x}), not tunnel to +5"
        );
    }

    #[test]
    fn without_ccd_fast_body_tunnels() {
        // Control: the same setup without CCD tunnels straight through.
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut bullet = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(-5.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 0.1 },
            1.0,
        );
        bullet.linear_velocity = Vector3::new(600.0, 0.0, 0.0);
        bullet.continuous_cd = false;
        bullet.can_sleep = false;
        let id = world.add_body(bullet);
        world.add_body(PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 0.2 },
            0.0,
        ));

        world.step(1.0 / 60.0);
        let x = world.get_body(id).unwrap().position.x;
        assert!(x > 4.0, "without CCD the fast body tunnels through (x={x})");
    }

    // -- Contact torque / angular response (pat-u3l8c.59) -----------------

    #[test]
    fn tilted_box_gains_angular_velocity_from_contact() {
        // A tilted box driven into a wide static floor gains angular velocity:
        // the contact/impulse is offset from its centre of mass so the normal
        // impulse produces a torque (the box tumbles toward flat).
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut box_body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(0.0, 0.9, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(0.5, 0.5, 0.5),
            },
            1.0,
        );
        box_body.rotation = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 0.5);
        box_body.linear_velocity = Vector3::new(0.0, -3.0, 0.0);
        box_body.can_sleep = false;
        let id = world.add_body(box_body);
        world.add_body(PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::BoxShape {
                half_extents: Vector3::new(5.0, 0.5, 5.0),
            },
            0.0,
        ));

        world.step(1.0 / 60.0);
        let av = world.get_body(id).unwrap().angular_velocity;
        assert!(
            av.length() > 1e-3,
            "offset contact should induce spin, angular_velocity={av:?}"
        );
    }

    #[test]
    fn head_on_sphere_collision_stays_linear() {
        // Regression: a centre-line sphere/sphere collision must NOT spuriously
        // introduce angular velocity (contact is on the line of centres).
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut ball = sphere_body(Vector3::ZERO);
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let id = world.add_body(ball);
        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        world.add_body(wall);

        world.step(1.0 / 60.0);
        let av = world.get_body(id).unwrap().angular_velocity;
        assert!(
            av.length() < 1e-5,
            "head-on contact must stay linear, av={av:?}"
        );
    }

    // -- Collision layer/mask filtering -----------------------------------

    #[test]
    fn collision_mask_skips_non_matching_pair() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        // Two overlapping spheres that should ignore each other: a is only on
        // and scanning layer 1, b is only on and scanning layer 2.
        let mut a = sphere_body(Vector3::ZERO);
        a.collision_layer = 0b0001;
        a.collision_mask = 0b0001;
        let mut b = sphere_body(Vector3::new(1.0, 0.0, 0.0));
        b.collision_layer = 0b0010;
        b.collision_mask = 0b0010;
        let id_a = world.add_body(a);
        let id_b = world.add_body(b);

        world.step(1.0 / 60.0);

        // No separation and no recorded contacts because masks are disjoint.
        let pa = world.get_body(id_a).unwrap().position;
        let pb = world.get_body(id_b).unwrap().position;
        let dist = (pb - pa).length();
        assert!(
            (dist - 1.0).abs() < 1e-5,
            "masked-out spheres must not be pushed apart, dist={dist}"
        );
    }

    #[test]
    fn collision_mask_resolves_matching_pair() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        // a belongs to layer 1; b scans layer 1 → they collide (one-directional
        // match is enough under Godot semantics).
        let mut a = sphere_body(Vector3::ZERO);
        a.collision_layer = 0b0001;
        a.collision_mask = 0b0000;
        let mut b = sphere_body(Vector3::new(1.0, 0.0, 0.0));
        b.collision_layer = 0b0010;
        b.collision_mask = 0b0001;
        let id_a = world.add_body(a);
        let id_b = world.add_body(b);

        world.step(1.0 / 60.0);

        let pa = world.get_body(id_a).unwrap().position;
        let pb = world.get_body(id_b).unwrap().position;
        let dist = (pb - pa).length();
        assert!(
            dist >= 1.9,
            "matching-mask spheres should separate, dist={dist}"
        );
    }

    // -- Broad-phase sweep-and-prune --------------------------------------

    /// Reference O(n^2) broad-phase: every pair whose world AABBs overlap,
    /// normalised to ascending-id order.
    fn bruteforce_pairs(world: &PhysicsWorld3D) -> Vec<(BodyId3D, BodyId3D)> {
        let bodies: Vec<&PhysicsBody3D> = world.bodies().collect();
        let mut pairs = Vec::new();
        for i in 0..bodies.len() {
            for j in (i + 1)..bodies.len() {
                if bodies[i].world_aabb().intersects(bodies[j].world_aabb()) {
                    let (a, b) = (bodies[i].id, bodies[j].id);
                    if a.0 <= b.0 {
                        pairs.push((a, b));
                    } else {
                        pairs.push((b, a));
                    }
                }
            }
        }
        pairs.sort_by(|x, y| (x.0 .0, x.1 .0).cmp(&(y.0 .0, y.1 .0)));
        pairs
    }

    #[test]
    fn broadphase_matches_bruteforce() {
        let mut world = PhysicsWorld3D::new();
        // Deterministic pseudo-random spread with several overlaps and gaps.
        let coords = [
            (0.0, 0.0, 0.0),
            (0.5, 0.0, 0.0),   // overlaps 0
            (1.4, 0.0, 0.0),   // overlaps 1
            (10.0, 0.0, 0.0),  // isolated
            (10.3, 0.4, 0.0),  // overlaps 3
            (-5.0, 2.0, 1.0),  // isolated
            (-4.2, 2.0, 1.0),  // overlaps 5
            (100.0, 0.0, 0.0), // isolated
        ];
        for (x, y, z) in coords {
            world.add_body(sphere_body(Vector3::new(x, y, z)));
        }
        assert_eq!(
            world.broadphase_pairs(),
            bruteforce_pairs(&world),
            "sweep-and-prune must produce the same overlapping pairs as O(n^2)"
        );
    }

    #[test]
    fn broadphase_prunes_separated_bodies() {
        let mut world = PhysicsWorld3D::new();
        // 1000 spheres (radius 1) on a grid with 4-unit spacing → none overlap.
        for i in 0..1000u64 {
            let x = (i % 50) as f32 * 4.0;
            let z = (i / 50) as f32 * 4.0;
            world.add_body(sphere_body(Vector3::new(x, 0.0, z)));
        }
        let pairs = world.broadphase_pairs();
        assert!(
            pairs.is_empty(),
            "well-separated bodies must yield zero candidate pairs, got {}",
            pairs.len()
        );
    }

    #[test]
    fn thousand_body_scene_steps_within_budget() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        // Sparse grid: 1000 bodies, spacing 4 → broad-phase prunes to near-zero
        // candidate pairs, so the O(n^2) all-pairs cost is avoided.
        for i in 0..1000u64 {
            let x = (i % 50) as f32 * 4.0;
            let z = (i / 50) as f32 * 4.0;
            world.add_body(sphere_body(Vector3::new(x, 0.0, z)));
        }

        // Assert the "budget" deterministically via the pruned candidate-pair
        // count rather than wall-clock time (which is flaky under CPU-contended
        // CI): a naive O(n^2) scan would consider ~500k pairs, sweep-and-prune
        // keeps it at zero for this well-separated grid.
        let pairs = world.broadphase_pairs();
        assert!(
            pairs.is_empty(),
            "sparse grid must prune to zero candidate pairs, got {}",
            pairs.len()
        );

        // The simulation still steps to completion over many frames.
        for _ in 0..60 {
            world.step(1.0 / 60.0);
        }
        assert_eq!(world.body_count(), 1000);
    }
}

#[cfg(test)]
mod softbody_world_tests {
    use super::*;
    use gdcore::math::Vector3;
    use crate::softbody3d::{SoftBody3D, SoftBodyId3D};

    #[test]
    fn soft_body_steps_with_world() {
        let mut world = PhysicsWorld3D::new();
        let positions = [
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(-1.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, -1.0),
        ];
        let edges = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
        let sb = SoftBody3D::new(SoftBodyId3D(1), &positions, &edges);
        let y0 = sb.vertices[0].position.y;
        world.add_soft_body(sb);
        world.step(1.0 / 60.0);
        assert_eq!(world.soft_body_count(), 1);
        let y1 = world.soft_bodies_mut()[0].vertices[0].position.y;
        assert!(y1 < y0, "unpinned soft body falls under gravity");
    }
}
