//! 2D physics joints that constrain pairs of bodies.
//!
//! Provides [`Joint2D`] variants that mirror Godot's joint types:
//! - [`PinJoint2D`]: constrains two bodies to share a point (position lock).
//! - [`DampedSpringJoint2D`]: spring connection with rest length, stiffness, and damping.
//! - [`GrooveJoint2D`]: constrains `body_b` to slide along a segment on `body_a`.

use gdcore::math::Vector2;

use crate::body::{BodyId, PhysicsBody2D};

/// Unique identifier for a joint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JointId(pub u64);

/// Rotates `v` by `angle` radians about the origin.
///
/// `Vector2` has no `rotated` of its own (only `Transform2D::rotated`), and the
/// joints need it per body every step to place a body-local anchor in world
/// space.
fn rotate(v: Vector2, angle: f32) -> Vector2 {
    let (s, c) = angle.sin_cos();
    Vector2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Base properties shared by all 2D joints.
#[derive(Debug, Clone)]
pub struct Joint2DBase {
    /// First body in the joint.
    pub body_a: BodyId,
    /// Second body in the joint.
    pub body_b: BodyId,
    /// Whether the joint is active.
    pub enabled: bool,
    /// Positional-correction bias, mirroring `Joint2D.bias`. `0.0` means "use
    /// the solver's own default" (Godot treats 0 the same way), otherwise it
    /// scales how much of the constraint error is resolved per step.
    pub bias: f32,
    /// Whether the two jointed bodies stop colliding with each other, mirroring
    /// `Joint2D.disable_collision` (Godot's default is `true`).
    pub disable_collision: bool,
}

impl Joint2DBase {
    /// Creates a new joint base connecting two bodies, with Godot's defaults
    /// (`bias = 0` meaning "solver default", collisions disabled between the
    /// pair).
    pub fn new(body_a: BodyId, body_b: BodyId) -> Self {
        Self {
            body_a,
            body_b,
            enabled: true,
            bias: 0.0,
            disable_collision: true,
        }
    }

    /// The fraction of the constraint error to resolve this step: the authored
    /// [`bias`](Self::bias) when set, else the solver default of full
    /// correction.
    pub fn correction_factor(&self) -> f32 {
        if self.bias > 0.0 {
            self.bias.min(1.0)
        } else {
            1.0
        }
    }
}

/// A pin joint constrains two bodies to share a single world-space point.
///
/// Each physics step, the joint moves both bodies toward their shared anchor
/// proportional to their inverse masses.
#[derive(Debug, Clone)]
pub struct PinJoint2D {
    /// Shared joint properties.
    pub base: Joint2DBase,
    /// The anchor point in world space. Defaults to the midpoint of both bodies.
    pub anchor: Vector2,
    /// Per-body anchor offsets in each body's own local frame, `(local_a,
    /// local_b)`, as captured by [`PinJoint2D::bind`].
    ///
    /// `None` keeps the simpler contract of [`PinJoint2D::new`]: both body
    /// *origins* are pulled onto [`anchor`](Self::anchor).
    ///
    /// `Some` is Godot's actual `PinJoint2D`: each body remembers where the
    /// anchor sits **within itself**, and the constraint only requires those two
    /// points to coincide. That is what lets a pendulum swing about its pin
    /// instead of being teleported onto it
    /// (upstream/godot/scene/2d/physics/joints/pin_joint_2d.cpp).
    pub local_anchors: Option<(Vector2, Vector2)>,
    /// `PinJoint2D.softness` — how much the constraint is allowed to give.
    /// `0.0` is rigid; larger values relax the per-step correction.
    pub softness: f32,
    /// Whether the relative rotation of the two bodies is clamped
    /// (`PinJoint2D.angular_limit_enabled`).
    pub angular_limit_enabled: bool,
    /// Lower bound of the relative rotation, in radians.
    pub angular_limit_lower: f32,
    /// Upper bound of the relative rotation, in radians.
    pub angular_limit_upper: f32,
    /// Whether the joint drives the relative rotation (`PinJoint2D.motor_enabled`).
    pub motor_enabled: bool,
    /// Target relative angular velocity in radians/second when the motor runs.
    pub motor_target_velocity: f32,
}

impl PinJoint2D {
    /// Creates a pin joint that pulls both body origins onto `anchor`.
    pub fn new(body_a: BodyId, body_b: BodyId, anchor: Vector2) -> Self {
        Self {
            base: Joint2DBase::new(body_a, body_b),
            anchor,
            local_anchors: None,
            softness: 0.0,
            angular_limit_enabled: false,
            angular_limit_lower: 0.0,
            angular_limit_upper: 0.0,
            motor_enabled: false,
            motor_target_velocity: 0.0,
        }
    }

    /// Creates a pin joint whose anchor is the midpoint of the two body positions.
    pub fn between(body_a: BodyId, pos_a: Vector2, body_b: BodyId, pos_b: Vector2) -> Self {
        let anchor = (pos_a + pos_b) * 0.5;
        Self::new(body_a, body_b, anchor)
    }

    /// Creates a pin joint the way Godot binds one: the bodies keep their
    /// current placement and each records where `anchor` falls inside its own
    /// local frame, so the pair is hinged at `anchor` rather than collapsed onto
    /// it.
    ///
    /// This is what a scene-authored `PinJoint2D` uses — the joint node's global
    /// position is the anchor and both bodies are already positioned around it.
    #[allow(clippy::too_many_arguments)]
    pub fn bind(
        body_a: BodyId,
        pos_a: Vector2,
        rot_a: f32,
        body_b: BodyId,
        pos_b: Vector2,
        rot_b: f32,
        anchor: Vector2,
    ) -> Self {
        let mut pin = Self::new(body_a, body_b, anchor);
        pin.local_anchors = Some((
            rotate(anchor - pos_a, -rot_a),
            rotate(anchor - pos_b, -rot_b),
        ));
        pin
    }
}

/// A damped spring joint connects two bodies with a spring that has rest length,
/// stiffness, and damping.
#[derive(Debug, Clone)]
pub struct DampedSpringJoint2D {
    /// Shared joint properties.
    pub base: Joint2DBase,
    /// Rest length of the spring.
    pub rest_length: f32,
    /// Spring stiffness (higher = stiffer). Units: force per unit displacement.
    pub stiffness: f32,
    /// Damping coefficient. Units: force per unit velocity.
    pub damping: f32,
    /// `DampedSpringJoint2D.length` — the spring's *maximum* extent. The bodies
    /// are pulled back onto it if the spring is stretched past this, so a soft
    /// spring still cannot be pulled apart indefinitely
    /// (upstream/godot/scene/2d/physics/joints/damped_spring_joint_2d.cpp).
    /// `0.0` disables the clamp.
    pub length: f32,
}

impl DampedSpringJoint2D {
    /// Creates a damped spring joint with the given parameters and no maximum
    /// length clamp.
    pub fn new(
        body_a: BodyId,
        body_b: BodyId,
        rest_length: f32,
        stiffness: f32,
        damping: f32,
    ) -> Self {
        Self {
            base: Joint2DBase::new(body_a, body_b),
            rest_length,
            stiffness,
            damping,
            length: 0.0,
        }
    }

    /// Sets the spring's maximum length (`0.0` disables the clamp).
    pub fn with_length(mut self, length: f32) -> Self {
        self.length = length;
        self
    }
}

/// A groove joint constrains `body_b`'s anchor to slide along a line segment
/// (the "groove") attached to `body_a`. Perpendicular displacement off the
/// groove is corrected each step; motion along the groove is free
/// (Godot `GrooveJoint2D`).
#[derive(Debug, Clone)]
pub struct GrooveJoint2D {
    /// Shared joint properties.
    pub base: Joint2DBase,
    /// First groove endpoint, as an offset from `body_a`'s position.
    pub groove_a1: Vector2,
    /// Second groove endpoint, as an offset from `body_a`'s position.
    pub groove_a2: Vector2,
    /// `body_b`'s constrained anchor, as an offset from `body_b`'s position.
    pub anchor_b: Vector2,
}

impl GrooveJoint2D {
    /// Creates a groove joint. `groove_a1`/`groove_a2` are the groove segment
    /// endpoints relative to `body_a`; `anchor_b` is the constrained point
    /// relative to `body_b`.
    pub fn new(
        body_a: BodyId,
        body_b: BodyId,
        groove_a1: Vector2,
        groove_a2: Vector2,
        anchor_b: Vector2,
    ) -> Self {
        Self {
            base: Joint2DBase::new(body_a, body_b),
            groove_a1,
            groove_a2,
            anchor_b,
        }
    }
}

/// A 2D joint that constrains two bodies.
#[derive(Debug, Clone)]
pub enum Joint2D {
    /// Pin joint: constrains bodies to a shared point.
    Pin(PinJoint2D),
    /// Damped spring: connects bodies with a spring.
    DampedSpring(DampedSpringJoint2D),
    /// Groove joint: constrains `body_b` to a segment on `body_a`.
    Groove(GrooveJoint2D),
}

impl Joint2D {
    /// Returns the base properties of this joint.
    pub fn base(&self) -> &Joint2DBase {
        match self {
            Joint2D::Pin(j) => &j.base,
            Joint2D::DampedSpring(j) => &j.base,
            Joint2D::Groove(j) => &j.base,
        }
    }

    /// Returns whether this joint is enabled.
    pub fn is_enabled(&self) -> bool {
        self.base().enabled
    }

    /// Returns a mutable reference to the base properties.
    pub fn base_mut(&mut self) -> &mut Joint2DBase {
        match self {
            Joint2D::Pin(j) => &mut j.base,
            Joint2D::DampedSpring(j) => &mut j.base,
            Joint2D::Groove(j) => &mut j.base,
        }
    }

    /// Enables or disables this joint.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.base_mut().enabled = enabled;
    }
}

/// Applies joint constraints to a pair of bodies for one physics step.
///
/// For **pin joints**, both bodies are moved toward the anchor point,
/// weighted by inverse mass.
///
/// For **damped spring joints**, a spring force (Hooke's law + damping)
/// is applied as impulses.
pub fn apply_joint_constraints(
    joint: &Joint2D,
    body_a: &mut PhysicsBody2D,
    body_b: &mut PhysicsBody2D,
    dt: f32,
) {
    if !joint.is_enabled() {
        return;
    }

    match joint {
        Joint2D::Pin(pin) => apply_pin_constraint(pin, body_a, body_b),
        Joint2D::DampedSpring(spring) => {
            apply_spring_constraint(spring, body_a, body_b, dt);
        }
        Joint2D::Groove(groove) => apply_groove_constraint(groove, body_a, body_b),
    }
}

/// Applies a pin joint constraint.
///
/// With [`PinJoint2D::local_anchors`] set (a scene-authored joint, bound via
/// [`PinJoint2D::bind`]) this is Godot's constraint: each body carries the
/// anchor in its own frame and only those two world points must coincide, so a
/// hanging body swings about the pin at a fixed radius. Without them, the
/// simpler original contract applies and both body origins are pulled onto
/// [`PinJoint2D::anchor`].
fn apply_pin_constraint(pin: &PinJoint2D, body_a: &mut PhysicsBody2D, body_b: &mut PhysicsBody2D) {
    let inv_a = body_a.inverse_mass();
    let inv_b = body_b.inverse_mass();
    let total_inv = inv_a + inv_b;
    if total_inv < 1e-10 {
        return; // Both bodies are immovable
    }

    match pin.local_anchors {
        Some((local_a, local_b)) => {
            // `softness` relaxes the constraint (Godot: 0 is rigid, larger is
            // looser); `bias` scales how much of the remaining drift is taken
            // per step.
            let softness = 1.0 / (1.0 + pin.softness.max(0.0));

            // Lever arms from each centre of mass out to the shared anchor.
            let r_a = rotate(local_a, body_a.rotation);
            let r_b = rotate(local_b, body_b.rotation);

            solve_pin_velocity(body_a, body_b, r_a, r_b, softness);

            // Positional projection for whatever drift the velocity solve left
            // (split-impulse style): the velocity solve is what makes the body
            // swing, this is what keeps the anchors coincident over long runs.
            let world_a = body_a.position + rotate(local_a, body_a.rotation);
            let world_b = body_b.position + rotate(local_b, body_b.rotation);
            let error = world_a - world_b;
            let strength = softness * pin.base.correction_factor();
            if inv_b > 0.0 {
                body_b.position = body_b.position + error * ((inv_b / total_inv) * strength);
            }
            if inv_a > 0.0 {
                body_a.position = body_a.position - error * ((inv_a / total_inv) * strength);
            }
        }
        None => {
            // Both bodies share the anchor point outright.
            if inv_a > 0.0 {
                body_a.position = pin.anchor;
            }
            if inv_b > 0.0 {
                body_b.position = pin.anchor;
            }
        }
    }

    apply_pin_angular(pin, body_a, body_b);
}

/// Cancels the relative velocity of the two bodies *at the shared anchor point*,
/// the standard 2D point-constraint impulse.
///
/// This is the part that turns a pin into a hinge: the impulse acts at the
/// anchor, so its lever arm `r` also spins the body. Correcting position alone
/// would hold a hanging body rigidly in place (and let gravity pile up velocity
/// forever), which is why a "pendulum" never swung.
fn solve_pin_velocity(
    body_a: &mut PhysicsBody2D,
    body_b: &mut PhysicsBody2D,
    r_a: Vector2,
    r_b: Vector2,
    softness: f32,
) {
    let im_a = body_a.inverse_mass();
    let im_b = body_b.inverse_mass();
    let ii_a = body_a.inverse_inertia();
    let ii_b = body_b.inverse_inertia();

    // Velocity of the anchor point on each body: v + ω × r, which in 2D is
    // v + ω * perp(r) with perp(r) = (-r.y, r.x).
    let perp_a = Vector2::new(-r_a.y, r_a.x);
    let perp_b = Vector2::new(-r_b.y, r_b.x);
    let vel_a = body_a.linear_velocity + perp_a * body_a.angular_velocity;
    let vel_b = body_b.linear_velocity + perp_b * body_b.angular_velocity;
    let v_rel = vel_b - vel_a;

    // Effective-mass matrix K = (im_a + im_b)·I + Σ ii · [[r.y², -r.x·r.y],
    // [-r.x·r.y, r.x²]]. Solving K·P = -v_rel gives the impulse that zeroes the
    // relative anchor velocity.
    let m = im_a + im_b;
    let k11 = m + ii_a * r_a.y * r_a.y + ii_b * r_b.y * r_b.y;
    let k12 = -ii_a * r_a.x * r_a.y - ii_b * r_b.x * r_b.y;
    let k22 = m + ii_a * r_a.x * r_a.x + ii_b * r_b.x * r_b.x;

    let det = k11 * k22 - k12 * k12;
    if det.abs() < 1e-12 {
        return; // Degenerate — the positional projection still applies.
    }
    let inv_det = 1.0 / det;
    let px = (-v_rel.x * k22 + v_rel.y * k12) * inv_det;
    let py = (v_rel.x * k12 - v_rel.y * k11) * inv_det;
    let impulse = Vector2::new(px, py) * softness;

    body_a.linear_velocity = body_a.linear_velocity - impulse * im_a;
    body_a.angular_velocity -= ii_a * (r_a.x * impulse.y - r_a.y * impulse.x);
    body_b.linear_velocity = body_b.linear_velocity + impulse * im_b;
    body_b.angular_velocity += ii_b * (r_b.x * impulse.y - r_b.y * impulse.x);
}

/// Drives and clamps the pin's relative rotation: `PinJoint2D`'s motor spins
/// `body_b` relative to `body_a` at `motor_target_velocity`, and the angular
/// limit holds their relative angle inside `[lower, upper]`.
fn apply_pin_angular(pin: &PinJoint2D, body_a: &mut PhysicsBody2D, body_b: &mut PhysicsBody2D) {
    if !pin.motor_enabled && !pin.angular_limit_enabled {
        return;
    }

    let inv_a = body_a.inverse_inertia();
    let inv_b = body_b.inverse_inertia();
    let total_inv = inv_a + inv_b;
    if total_inv < 1e-10 {
        return; // Neither body can rotate.
    }

    if pin.motor_enabled {
        // Steer the *relative* angular velocity to the target, splitting the
        // change by inverse inertia so a static hinge stays put.
        let relative = body_b.angular_velocity - body_a.angular_velocity;
        let delta = pin.motor_target_velocity - relative;
        body_b.angular_velocity += delta * (inv_b / total_inv);
        body_a.angular_velocity -= delta * (inv_a / total_inv);
    }

    if pin.angular_limit_enabled {
        let (lower, upper) = if pin.angular_limit_lower <= pin.angular_limit_upper {
            (pin.angular_limit_lower, pin.angular_limit_upper)
        } else {
            (pin.angular_limit_upper, pin.angular_limit_lower)
        };
        let relative = body_b.rotation - body_a.rotation;
        let clamped = relative.clamp(lower, upper);
        let error = clamped - relative;
        if error != 0.0 {
            body_b.rotation += error * (inv_b / total_inv);
            body_a.rotation -= error * (inv_a / total_inv);
            // Stop pushing further past the limit.
            let relative_vel = body_b.angular_velocity - body_a.angular_velocity;
            if (error > 0.0 && relative_vel < 0.0) || (error < 0.0 && relative_vel > 0.0) {
                body_b.angular_velocity -= relative_vel * (inv_b / total_inv);
                body_a.angular_velocity += relative_vel * (inv_a / total_inv);
            }
        }
    }
}

/// Applies a groove joint constraint: pulls `body_b`'s anchor perpendicular
/// onto the groove segment attached to `body_a`, weighted by inverse mass.
/// Motion *along* the groove is unconstrained — the anchor may slide anywhere
/// between the endpoints (and is clamped to them beyond the ends).
fn apply_groove_constraint(
    groove: &GrooveJoint2D,
    body_a: &mut PhysicsBody2D,
    body_b: &mut PhysicsBody2D,
) {
    let inv_a = body_a.inverse_mass();
    let inv_b = body_b.inverse_mass();
    let total_inv = inv_a + inv_b;
    if total_inv < 1e-10 {
        return; // Both bodies immovable.
    }

    // Groove segment endpoints and body_b's anchor, in world space.
    let p1 = body_a.position + groove.groove_a1;
    let p2 = body_a.position + groove.groove_a2;
    let anchor = body_b.position + groove.anchor_b;

    // Closest point on the (clamped) groove segment to the anchor.
    let seg = p2 - p1;
    let len_sq = seg.length_squared();
    let closest = if len_sq < 1e-10 {
        p1
    } else {
        let t = ((anchor - p1).dot(seg) / len_sq).clamp(0.0, 1.0);
        p1 + seg * t
    };

    // Perpendicular correction bringing the anchor onto the groove, split by
    // inverse mass: a static `body_a` holds the groove fixed while `body_b`
    // slides onto it.
    let error = closest - anchor;
    body_b.position = body_b.position + error * (inv_b / total_inv);
    body_a.position = body_a.position - error * (inv_a / total_inv);
}

/// Applies a damped spring constraint using Hooke's law with damping.
fn apply_spring_constraint(
    spring: &DampedSpringJoint2D,
    body_a: &mut PhysicsBody2D,
    body_b: &mut PhysicsBody2D,
    dt: f32,
) {
    let inv_a = body_a.inverse_mass();
    let inv_b = body_b.inverse_mass();
    let total_inv = inv_a + inv_b;
    if total_inv < 1e-10 {
        return;
    }

    let diff = body_b.position - body_a.position;
    let distance = diff.length();
    if distance < 1e-10 {
        return; // Bodies are at the same position, no direction to apply force
    }

    let direction = diff * (1.0 / distance);
    let displacement = distance - spring.rest_length;

    // Hooke's law: F = -k * x
    let spring_force = spring.stiffness * displacement;

    // Damping: F_d = -c * v_relative_along_spring
    let relative_vel = body_b.linear_velocity - body_a.linear_velocity;
    let relative_speed_along = relative_vel.dot(direction);
    let damping_force = spring.damping * relative_speed_along;

    let total_force = spring_force + damping_force;

    // Apply as impulses (force * dt / mass = delta velocity)
    let impulse = direction * total_force * dt;

    if inv_a > 0.0 {
        body_a.linear_velocity = body_a.linear_velocity + impulse * inv_a;
    }
    if inv_b > 0.0 {
        body_b.linear_velocity = body_b.linear_velocity - impulse * inv_b;
    }

    // `length` is the spring's hard maximum extent: a soft spring may stretch,
    // but never past this. Pull the overshoot straight back out, split by
    // inverse mass.
    if spring.length > 0.0 && distance > spring.length {
        let overshoot = distance - spring.length;
        let correction = direction * (overshoot * spring.base.correction_factor());
        if inv_b > 0.0 {
            body_b.position = body_b.position - correction * (inv_b / total_inv);
        }
        if inv_a > 0.0 {
            body_a.position = body_a.position + correction * (inv_a / total_inv);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType;
    use crate::shape::Shape2D;

    const EPSILON: f32 = 1e-3;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn make_rigid(pos: Vector2) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(0),
            BodyType::Rigid,
            pos,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        )
    }

    fn make_static(pos: Vector2) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            pos,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        )
    }

    // ---- PinJoint2D tests ----

    #[test]
    fn pin_joint_holds_bodies_at_anchor() {
        let mut a = make_rigid(Vector2::new(-5.0, 0.0));
        let mut b = make_rigid(Vector2::new(5.0, 0.0));
        let pin = PinJoint2D::new(BodyId(1), BodyId(2), Vector2::ZERO);
        let joint = Joint2D::Pin(pin);

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        // Both bodies should move toward the anchor (origin)
        assert!(
            a.position.x > -5.0,
            "Body A should move toward anchor: {}",
            a.position.x
        );
        assert!(
            b.position.x < 5.0,
            "Body B should move toward anchor: {}",
            b.position.x
        );
    }

    #[test]
    fn pin_joint_equal_mass_bodies_meet_at_anchor() {
        let mut a = make_rigid(Vector2::new(-2.0, 0.0));
        let mut b = make_rigid(Vector2::new(2.0, 0.0));
        let pin = PinJoint2D::new(BodyId(1), BodyId(2), Vector2::ZERO);
        let joint = Joint2D::Pin(pin);

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        // Equal mass: both move equally toward anchor
        assert!(
            approx_eq(a.position.x, 0.0),
            "Body A at anchor: {}",
            a.position.x
        );
        assert!(
            approx_eq(b.position.x, 0.0),
            "Body B at anchor: {}",
            b.position.x
        );
    }

    #[test]
    fn pin_joint_static_body_does_not_move() {
        let mut a = make_static(Vector2::new(-3.0, 0.0));
        let mut b = make_rigid(Vector2::new(3.0, 0.0));
        let pin = PinJoint2D::new(BodyId(1), BodyId(2), Vector2::ZERO);
        let joint = Joint2D::Pin(pin);

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        assert_eq!(
            a.position,
            Vector2::new(-3.0, 0.0),
            "Static body must not move"
        );
        assert!(
            approx_eq(b.position.x, 0.0),
            "Rigid body moves fully to anchor: {}",
            b.position.x
        );
    }

    #[test]
    fn pin_joint_between_creates_midpoint_anchor() {
        let pos_a = Vector2::new(0.0, 0.0);
        let pos_b = Vector2::new(10.0, 0.0);
        let pin = PinJoint2D::between(BodyId(1), pos_a, BodyId(2), pos_b);

        assert!(approx_eq(pin.anchor.x, 5.0));
        assert!(approx_eq(pin.anchor.y, 0.0));
    }

    #[test]
    fn pin_joint_disabled_does_nothing() {
        let mut a = make_rigid(Vector2::new(-5.0, 0.0));
        let mut b = make_rigid(Vector2::new(5.0, 0.0));
        let mut pin = PinJoint2D::new(BodyId(1), BodyId(2), Vector2::ZERO);
        pin.base.enabled = false;
        let joint = Joint2D::Pin(pin);

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        assert_eq!(a.position, Vector2::new(-5.0, 0.0));
        assert_eq!(b.position, Vector2::new(5.0, 0.0));
    }

    // ---- GrooveJoint2D tests ----

    // pat-nzgdu.59: a GrooveJoint2D pulls body_b perpendicular onto the groove
    // segment on body_a while leaving motion along the groove free.
    #[test]
    fn groove_joint_constrains_body_to_segment() {
        // Horizontal groove on a STATIC body_a spanning x ∈ [-5, 5] at y = 0;
        // body_b starts 3 units above the line at x = 2.
        let mut a = make_static(Vector2::ZERO);
        let mut b = make_rigid(Vector2::new(2.0, 3.0));
        let groove = GrooveJoint2D::new(
            BodyId(1),
            BodyId(2),
            Vector2::new(-5.0, 0.0),
            Vector2::new(5.0, 0.0),
            Vector2::ZERO,
        );
        let joint = Joint2D::Groove(groove);

        // Exhaustive-match wiring reaches the Groove variant.
        assert!(joint.is_enabled());
        assert_eq!(joint.base().body_b, BodyId(2));

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        // body_b snapped onto the groove line (y ≈ 0), keeping its slide
        // position along the groove (x ≈ 2).
        assert!(
            approx_eq(b.position.y, 0.0),
            "body_b must sit on the groove line, got y={}",
            b.position.y
        );
        assert!(
            approx_eq(b.position.x, 2.0),
            "body_b must keep its position along the groove, got x={}",
            b.position.x
        );
        // The static body_a holding the groove does not move.
        assert_eq!(a.position, Vector2::ZERO);
    }

    // ---- DampedSpringJoint2D tests ----

    #[test]
    fn spring_pulls_bodies_toward_rest_length() {
        // Bodies at distance 10, rest_length 5 — spring should pull them together
        let mut a = make_rigid(Vector2::new(0.0, 0.0));
        let mut b = make_rigid(Vector2::new(10.0, 0.0));
        let spring = DampedSpringJoint2D::new(BodyId(1), BodyId(2), 5.0, 100.0, 0.0);
        let joint = Joint2D::DampedSpring(spring);

        let dt = 1.0 / 60.0;
        apply_joint_constraints(&joint, &mut a, &mut b, dt);

        // Body A should gain positive x velocity (toward B)
        assert!(
            a.linear_velocity.x > 0.0,
            "Body A should accelerate toward B: {}",
            a.linear_velocity.x
        );
        // Body B should gain negative x velocity (toward A)
        assert!(
            b.linear_velocity.x < 0.0,
            "Body B should accelerate toward A: {}",
            b.linear_velocity.x
        );
    }

    #[test]
    fn spring_pushes_bodies_apart_when_compressed() {
        // Bodies at distance 2, rest_length 10 — spring should push apart
        let mut a = make_rigid(Vector2::new(0.0, 0.0));
        let mut b = make_rigid(Vector2::new(2.0, 0.0));
        let spring = DampedSpringJoint2D::new(BodyId(1), BodyId(2), 10.0, 100.0, 0.0);
        let joint = Joint2D::DampedSpring(spring);

        let dt = 1.0 / 60.0;
        apply_joint_constraints(&joint, &mut a, &mut b, dt);

        // Body A should gain negative x velocity (away from B)
        assert!(
            a.linear_velocity.x < 0.0,
            "Body A should push away: {}",
            a.linear_velocity.x
        );
        // Body B should gain positive x velocity (away from A)
        assert!(
            b.linear_velocity.x > 0.0,
            "Body B should push away: {}",
            b.linear_velocity.x
        );
    }

    #[test]
    fn spring_at_rest_length_no_force() {
        let mut a = make_rigid(Vector2::new(0.0, 0.0));
        let mut b = make_rigid(Vector2::new(5.0, 0.0));
        let spring = DampedSpringJoint2D::new(BodyId(1), BodyId(2), 5.0, 100.0, 0.0);
        let joint = Joint2D::DampedSpring(spring);

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        assert!(
            approx_eq(a.linear_velocity.x, 0.0),
            "No force at rest length: {}",
            a.linear_velocity.x
        );
        assert!(
            approx_eq(b.linear_velocity.x, 0.0),
            "No force at rest length: {}",
            b.linear_velocity.x
        );
    }

    #[test]
    fn spring_oscillates_over_multiple_steps() {
        let mut a = make_rigid(Vector2::new(0.0, 0.0));
        a.id = BodyId(1);
        let mut b = make_rigid(Vector2::new(10.0, 0.0));
        b.id = BodyId(2);

        let spring = DampedSpringJoint2D::new(BodyId(1), BodyId(2), 5.0, 50.0, 1.0);
        let joint = Joint2D::DampedSpring(spring);

        let dt = 1.0 / 60.0;
        let mut distances = Vec::new();

        for _ in 0..120 {
            apply_joint_constraints(&joint, &mut a, &mut b, dt);
            // Integrate positions manually
            a.position = a.position + a.linear_velocity * dt;
            b.position = b.position + b.linear_velocity * dt;
            distances.push((b.position - a.position).length());
        }

        // After oscillation with damping, distance should approach rest length (5.0)
        let final_distance = *distances.last().unwrap();
        assert!(
            (final_distance - 5.0).abs() < 2.0,
            "Spring should converge toward rest length 5.0, got {final_distance}"
        );

        // Verify oscillation happened: distance should have been both > and < rest length
        let had_stretch = distances.iter().any(|&d| d > 5.5);
        let had_compress = distances.iter().any(|&d| d < 4.5);
        assert!(
            had_stretch || had_compress,
            "Spring should oscillate around rest length"
        );
    }

    #[test]
    fn spring_damping_reduces_oscillation() {
        // Compare undamped vs heavily damped spring
        let run = |damping: f32| -> f32 {
            let mut a = make_rigid(Vector2::new(0.0, 0.0));
            let mut b = make_rigid(Vector2::new(10.0, 0.0));
            let spring = DampedSpringJoint2D::new(BodyId(1), BodyId(2), 5.0, 50.0, damping);
            let joint = Joint2D::DampedSpring(spring);
            let dt = 1.0 / 60.0;

            for _ in 0..60 {
                apply_joint_constraints(&joint, &mut a, &mut b, dt);
                a.position = a.position + a.linear_velocity * dt;
                b.position = b.position + b.linear_velocity * dt;
            }

            // Return total kinetic energy as a measure of remaining oscillation
            let ke_a = a.linear_velocity.length_squared();
            let ke_b = b.linear_velocity.length_squared();
            ke_a + ke_b
        };

        let energy_low_damp = run(0.5);
        let energy_high_damp = run(20.0);

        assert!(
            energy_high_damp < energy_low_damp,
            "Higher damping should reduce oscillation energy: low={energy_low_damp}, high={energy_high_damp}"
        );
    }

    #[test]
    fn spring_disabled_does_nothing() {
        let mut a = make_rigid(Vector2::new(0.0, 0.0));
        let mut b = make_rigid(Vector2::new(10.0, 0.0));
        let mut spring = DampedSpringJoint2D::new(BodyId(1), BodyId(2), 5.0, 100.0, 0.0);
        spring.base.enabled = false;
        let joint = Joint2D::DampedSpring(spring);

        apply_joint_constraints(&joint, &mut a, &mut b, 1.0 / 60.0);

        assert_eq!(a.linear_velocity, Vector2::ZERO);
        assert_eq!(b.linear_velocity, Vector2::ZERO);
    }

    #[test]
    fn joint_base_accessors() {
        let pin = Joint2D::Pin(PinJoint2D::new(BodyId(1), BodyId(2), Vector2::ZERO));
        assert_eq!(pin.base().body_a, BodyId(1));
        assert_eq!(pin.base().body_b, BodyId(2));
        assert!(pin.is_enabled());

        let spring = Joint2D::DampedSpring(DampedSpringJoint2D::new(
            BodyId(3),
            BodyId(4),
            5.0,
            100.0,
            10.0,
        ));
        assert_eq!(spring.base().body_a, BodyId(3));
        assert!(spring.is_enabled());
    }
}
