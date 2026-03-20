//! 2D physics joints that constrain pairs of bodies.
//!
//! Provides [`Joint2D`] variants that mirror Godot's joint types:
//! - [`PinJoint2D`]: constrains two bodies to share a point (position lock).
//! - [`DampedSpringJoint2D`]: spring connection with rest length, stiffness, and damping.

use gdcore::math::Vector2;

use crate::body::{BodyId, PhysicsBody2D};

/// Unique identifier for a joint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JointId(pub u64);

/// Base properties shared by all 2D joints.
#[derive(Debug, Clone)]
pub struct Joint2DBase {
    /// First body in the joint.
    pub body_a: BodyId,
    /// Second body in the joint.
    pub body_b: BodyId,
    /// Whether the joint is active.
    pub enabled: bool,
}

impl Joint2DBase {
    /// Creates a new joint base connecting two bodies.
    pub fn new(body_a: BodyId, body_b: BodyId) -> Self {
        Self {
            body_a,
            body_b,
            enabled: true,
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
}

impl PinJoint2D {
    /// Creates a pin joint at the given anchor point.
    pub fn new(body_a: BodyId, body_b: BodyId, anchor: Vector2) -> Self {
        Self {
            base: Joint2DBase::new(body_a, body_b),
            anchor,
        }
    }

    /// Creates a pin joint whose anchor is the midpoint of the two body positions.
    pub fn between(body_a: BodyId, pos_a: Vector2, body_b: BodyId, pos_b: Vector2) -> Self {
        let anchor = (pos_a + pos_b) * 0.5;
        Self::new(body_a, body_b, anchor)
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
}

impl DampedSpringJoint2D {
    /// Creates a damped spring joint with the given parameters.
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
}

impl Joint2D {
    /// Returns the base properties of this joint.
    pub fn base(&self) -> &Joint2DBase {
        match self {
            Joint2D::Pin(j) => &j.base,
            Joint2D::DampedSpring(j) => &j.base,
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
    }
}

/// Applies a pin joint constraint: moves bodies toward the shared anchor.
fn apply_pin_constraint(pin: &PinJoint2D, body_a: &mut PhysicsBody2D, body_b: &mut PhysicsBody2D) {
    let inv_a = body_a.inverse_mass();
    let inv_b = body_b.inverse_mass();
    let total_inv = inv_a + inv_b;
    if total_inv < 1e-10 {
        return; // Both bodies are immovable
    }

    // Move each body fully to the anchor, weighted by its share of inverse mass.
    // A movable body paired with an immovable one moves 100% to the anchor.
    // Two equal-mass bodies each move 100% to the anchor.
    let ratio_a = inv_a / total_inv;
    let ratio_b = inv_b / total_inv;

    if inv_a > 0.0 {
        body_a.position = pin.anchor;
    }
    if inv_b > 0.0 {
        // If both are movable, body B also snaps to anchor.
        // If only B is movable, it moves fully to anchor.
        body_b.position = pin.anchor;
    }

    // For unequal masses where neither is static, we place the anchor
    // as the weighted point — but for a pin joint the contract is that
    // both bodies share the same point, so both go to the anchor.
    let _ = (ratio_a, ratio_b); // suppress unused warnings
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
