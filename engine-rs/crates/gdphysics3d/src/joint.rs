//! 3D physics joints that constrain pairs of bodies.
//!
//! Provides [`Joint3D`] types that mirror Godot's joint nodes:
//! - [`PinJoint3D`]: constrains two bodies to share a point (ball-and-socket).
//! - [`HingeJoint3D`]: constrains rotation to a single axis (door hinge).
//! - [`SliderJoint3D`]: constrains motion to a single axis (sliding rail).
//!
//! Each joint holds a [`Joint3DBase`] with shared properties (body references,
//! enabled flag, solver priority, etc.) plus type-specific parameters.

use gdcore::math::Vector3;

use crate::body::BodyId3D;

// ---------------------------------------------------------------------------
// Joint identifier
// ---------------------------------------------------------------------------

/// Unique identifier for a 3D physics joint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JointId3D(pub u64);

// ---------------------------------------------------------------------------
// Joint3DBase — shared properties
// ---------------------------------------------------------------------------

/// Base properties shared by all 3D joints.
#[derive(Debug, Clone)]
pub struct Joint3DBase {
    /// Unique identifier for this joint.
    pub id: JointId3D,
    /// First body (node A).
    pub body_a: BodyId3D,
    /// Second body (node B).
    pub body_b: BodyId3D,
    /// Whether the joint is active.
    pub enabled: bool,
    /// Whether connected bodies should collide with each other.
    /// Godot default: `true`.
    pub exclude_nodes_from_collision: bool,
    /// Solver priority — higher values are solved first.
    /// Godot default: `1`.
    pub solver_priority: i32,
}

impl Joint3DBase {
    /// Creates a new joint base connecting two bodies.
    pub fn new(id: JointId3D, body_a: BodyId3D, body_b: BodyId3D) -> Self {
        Self {
            id,
            body_a,
            body_b,
            enabled: true,
            exclude_nodes_from_collision: true,
            solver_priority: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// PinJoint3D — ball-and-socket
// ---------------------------------------------------------------------------

/// Parameters for a PinJoint3D (ball-and-socket constraint).
///
/// Constrains two bodies so that a point on each body remains coincident
/// in world space.
#[derive(Debug, Clone)]
pub struct PinJoint3D {
    /// Shared joint properties.
    pub base: Joint3DBase,
    /// Local-space pivot point on body A.
    pub local_a: Vector3,
    /// Local-space pivot point on body B.
    pub local_b: Vector3,
    /// Damping of the pin constraint (Godot param `damping`).
    pub damping: f32,
    /// Impulse clamp — maximum impulse applied per step.
    pub impulse_clamp: f32,
    /// Bias factor for error correction.
    pub bias: f32,
}

impl PinJoint3D {
    /// Creates a pin joint with default parameters.
    pub fn new(id: JointId3D, body_a: BodyId3D, body_b: BodyId3D) -> Self {
        Self {
            base: Joint3DBase::new(id, body_a, body_b),
            local_a: Vector3::ZERO,
            local_b: Vector3::ZERO,
            damping: 1.0,
            impulse_clamp: 0.0,
            bias: 0.3,
        }
    }

    /// Sets the local-space pivot points on both bodies.
    pub fn with_pivots(mut self, local_a: Vector3, local_b: Vector3) -> Self {
        self.local_a = local_a;
        self.local_b = local_b;
        self
    }

    /// Returns the positional error between the two pivot points given
    /// their current world-space positions.
    pub fn compute_error(&self, world_a: Vector3, world_b: Vector3) -> Vector3 {
        Vector3::new(
            world_b.x - world_a.x,
            world_b.y - world_a.y,
            world_b.z - world_a.z,
        )
    }

    /// Returns the magnitude of the positional error.
    pub fn error_magnitude(&self, world_a: Vector3, world_b: Vector3) -> f32 {
        let e = self.compute_error(world_a, world_b);
        (e.x * e.x + e.y * e.y + e.z * e.z).sqrt()
    }
}

// ---------------------------------------------------------------------------
// HingeJoint3D — single-axis rotation
// ---------------------------------------------------------------------------

/// Parameters for a HingeJoint3D (hinge / revolute constraint).
///
/// Constrains two bodies so that they can only rotate around a shared axis.
#[derive(Debug, Clone)]
pub struct HingeJoint3D {
    /// Shared joint properties.
    pub base: Joint3DBase,
    /// The hinge axis in local space of body A.
    pub axis: Vector3,
    /// Local-space anchor point on body A.
    pub anchor_a: Vector3,
    /// Local-space anchor point on body B.
    pub anchor_b: Vector3,
    /// Whether angular limits are enabled.
    pub use_limit: bool,
    /// Lower angular limit (radians). Only used when `use_limit` is true.
    pub lower_limit: f32,
    /// Upper angular limit (radians). Only used when `use_limit` is true.
    pub upper_limit: f32,
    /// Bias factor for limit error correction.
    pub limit_bias: f32,
    /// Softness factor for limits (0 = rigid, 1 = fully soft).
    pub limit_softness: f32,
    /// Relaxation factor for limits.
    pub limit_relaxation: f32,
    /// Whether the hinge motor is enabled.
    pub motor_enabled: bool,
    /// Target angular velocity (radians/sec) for the motor.
    pub motor_target_velocity: f32,
    /// Maximum impulse the motor can apply per step.
    pub motor_max_impulse: f32,
}

impl HingeJoint3D {
    /// Creates a hinge joint with default parameters along the local Y axis.
    pub fn new(id: JointId3D, body_a: BodyId3D, body_b: BodyId3D) -> Self {
        Self {
            base: Joint3DBase::new(id, body_a, body_b),
            axis: Vector3::new(0.0, 1.0, 0.0),
            anchor_a: Vector3::ZERO,
            anchor_b: Vector3::ZERO,
            use_limit: false,
            lower_limit: 0.0,
            upper_limit: 0.0,
            limit_bias: 0.3,
            limit_softness: 0.9,
            limit_relaxation: 1.0,
            motor_enabled: false,
            motor_target_velocity: 0.0,
            motor_max_impulse: 1.0,
        }
    }

    /// Sets the hinge axis.
    pub fn with_axis(mut self, axis: Vector3) -> Self {
        let len = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt();
        if len > 1e-6 {
            self.axis = Vector3::new(axis.x / len, axis.y / len, axis.z / len);
        }
        self
    }

    /// Enables angular limits with the given range (radians).
    pub fn with_limits(mut self, lower: f32, upper: f32) -> Self {
        self.use_limit = true;
        self.lower_limit = lower;
        self.upper_limit = upper;
        self
    }

    /// Enables the motor with the given target velocity and max impulse.
    pub fn with_motor(mut self, target_velocity: f32, max_impulse: f32) -> Self {
        self.motor_enabled = true;
        self.motor_target_velocity = target_velocity;
        self.motor_max_impulse = max_impulse;
        self
    }

    /// Tests whether a given angle (radians) is within the configured limits.
    /// Returns `true` if limits are disabled or the angle is within bounds.
    pub fn angle_within_limits(&self, angle: f32) -> bool {
        if !self.use_limit {
            return true;
        }
        angle >= self.lower_limit && angle <= self.upper_limit
    }

    /// Clamps an angle to the configured limits. Returns the angle unchanged
    /// if limits are disabled.
    pub fn clamp_angle(&self, angle: f32) -> f32 {
        if !self.use_limit {
            return angle;
        }
        angle.clamp(self.lower_limit, self.upper_limit)
    }

    /// Computes the motor impulse for the current angular velocity.
    /// Returns `0.0` if the motor is disabled.
    pub fn compute_motor_impulse(&self, current_angular_velocity: f32, dt: f32) -> f32 {
        if !self.motor_enabled || dt <= 0.0 {
            return 0.0;
        }
        let velocity_error = self.motor_target_velocity - current_angular_velocity;
        let impulse = velocity_error * dt;
        impulse.clamp(-self.motor_max_impulse, self.motor_max_impulse)
    }
}

// ---------------------------------------------------------------------------
// SliderJoint3D — single-axis translation
// ---------------------------------------------------------------------------

/// Parameters for a SliderJoint3D (prismatic / slider constraint).
///
/// Constrains two bodies so that one can only slide along a single axis
/// relative to the other.
#[derive(Debug, Clone)]
pub struct SliderJoint3D {
    /// Shared joint properties.
    pub base: Joint3DBase,
    /// Sliding axis in local space of body A.
    pub axis: Vector3,
    /// Whether linear limits are enabled.
    pub use_linear_limit: bool,
    /// Lower linear limit (meters).
    pub linear_limit_lower: f32,
    /// Upper linear limit (meters).
    pub linear_limit_upper: f32,
    /// Softness for linear limits.
    pub linear_limit_softness: f32,
    /// Restitution for linear limit bounces.
    pub linear_limit_restitution: f32,
    /// Damping applied when within linear limits.
    pub linear_limit_damping: f32,
    /// Whether angular limits are enabled.
    pub use_angular_limit: bool,
    /// Lower angular limit (radians).
    pub angular_limit_lower: f32,
    /// Upper angular limit (radians).
    pub angular_limit_upper: f32,
    /// Softness for angular limits.
    pub angular_limit_softness: f32,
    /// Restitution for angular limit bounces.
    pub angular_limit_restitution: f32,
    /// Damping applied when within angular limits.
    pub angular_limit_damping: f32,
}

impl SliderJoint3D {
    /// Creates a slider joint along the local X axis with default parameters.
    pub fn new(id: JointId3D, body_a: BodyId3D, body_b: BodyId3D) -> Self {
        Self {
            base: Joint3DBase::new(id, body_a, body_b),
            axis: Vector3::new(1.0, 0.0, 0.0),
            use_linear_limit: false,
            linear_limit_lower: -1.0,
            linear_limit_upper: 1.0,
            linear_limit_softness: 1.0,
            linear_limit_restitution: 0.7,
            linear_limit_damping: 1.0,
            use_angular_limit: false,
            angular_limit_lower: 0.0,
            angular_limit_upper: 0.0,
            angular_limit_softness: 1.0,
            angular_limit_restitution: 0.7,
            angular_limit_damping: 1.0,
        }
    }

    /// Sets the sliding axis.
    pub fn with_axis(mut self, axis: Vector3) -> Self {
        let len = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt();
        if len > 1e-6 {
            self.axis = Vector3::new(axis.x / len, axis.y / len, axis.z / len);
        }
        self
    }

    /// Enables linear limits.
    pub fn with_linear_limits(mut self, lower: f32, upper: f32) -> Self {
        self.use_linear_limit = true;
        self.linear_limit_lower = lower;
        self.linear_limit_upper = upper;
        self
    }

    /// Enables angular limits.
    pub fn with_angular_limits(mut self, lower: f32, upper: f32) -> Self {
        self.use_angular_limit = true;
        self.angular_limit_lower = lower;
        self.angular_limit_upper = upper;
        self
    }

    /// Clamps a linear offset to the configured limits. Returns unchanged
    /// if limits are disabled.
    pub fn clamp_linear(&self, offset: f32) -> f32 {
        if !self.use_linear_limit {
            return offset;
        }
        offset.clamp(self.linear_limit_lower, self.linear_limit_upper)
    }

    /// Clamps an angular offset to the configured limits. Returns unchanged
    /// if limits are disabled.
    pub fn clamp_angular(&self, angle: f32) -> f32 {
        if !self.use_angular_limit {
            return angle;
        }
        angle.clamp(self.angular_limit_lower, self.angular_limit_upper)
    }

    /// Tests whether a linear offset is within the configured limits.
    pub fn linear_within_limits(&self, offset: f32) -> bool {
        if !self.use_linear_limit {
            return true;
        }
        offset >= self.linear_limit_lower && offset <= self.linear_limit_upper
    }

    /// Tests whether an angular offset is within the configured limits.
    pub fn angular_within_limits(&self, angle: f32) -> bool {
        if !self.use_angular_limit {
            return true;
        }
        angle >= self.angular_limit_lower && angle <= self.angular_limit_upper
    }
}

// ---------------------------------------------------------------------------
// Joint3D enum — unified type
// ---------------------------------------------------------------------------

/// A 3D physics joint, wrapping one of the concrete joint types.
#[derive(Debug, Clone)]
pub enum Joint3D {
    Pin(PinJoint3D),
    Hinge(HingeJoint3D),
    Slider(SliderJoint3D),
}

impl Joint3D {
    /// Returns a reference to the shared base properties.
    pub fn base(&self) -> &Joint3DBase {
        match self {
            Joint3D::Pin(j) => &j.base,
            Joint3D::Hinge(j) => &j.base,
            Joint3D::Slider(j) => &j.base,
        }
    }

    /// Returns a mutable reference to the shared base properties.
    pub fn base_mut(&mut self) -> &mut Joint3DBase {
        match self {
            Joint3D::Pin(j) => &mut j.base,
            Joint3D::Hinge(j) => &mut j.base,
            Joint3D::Slider(j) => &mut j.base,
        }
    }

    /// Returns the joint identifier.
    pub fn id(&self) -> JointId3D {
        self.base().id
    }

    /// Returns `true` if the joint is enabled.
    pub fn is_enabled(&self) -> bool {
        self.base().enabled
    }

    /// Returns the connected body IDs.
    pub fn bodies(&self) -> (BodyId3D, BodyId3D) {
        let b = self.base();
        (b.body_a, b.body_b)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn ids() -> (JointId3D, BodyId3D, BodyId3D) {
        (JointId3D(1), BodyId3D(10), BodyId3D(20))
    }

    // -- Joint3DBase --

    #[test]
    fn base_defaults() {
        let (jid, a, b) = ids();
        let base = Joint3DBase::new(jid, a, b);
        assert!(base.enabled);
        assert!(base.exclude_nodes_from_collision);
        assert_eq!(base.solver_priority, 1);
        assert_eq!(base.body_a, a);
        assert_eq!(base.body_b, b);
    }

    // -- PinJoint3D --

    #[test]
    fn pin_joint_defaults() {
        let (jid, a, b) = ids();
        let pin = PinJoint3D::new(jid, a, b);
        assert_eq!(pin.local_a, Vector3::ZERO);
        assert_eq!(pin.local_b, Vector3::ZERO);
        assert!((pin.damping - 1.0).abs() < f32::EPSILON);
        assert!((pin.impulse_clamp - 0.0).abs() < f32::EPSILON);
        assert!((pin.bias - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn pin_joint_with_pivots() {
        let (jid, a, b) = ids();
        let pin = PinJoint3D::new(jid, a, b)
            .with_pivots(Vector3::new(1.0, 0.0, 0.0), Vector3::new(-1.0, 0.0, 0.0));
        assert_eq!(pin.local_a.x, 1.0);
        assert_eq!(pin.local_b.x, -1.0);
    }

    #[test]
    fn pin_joint_error() {
        let (jid, a, b) = ids();
        let pin = PinJoint3D::new(jid, a, b);
        let err = pin.compute_error(Vector3::new(1.0, 0.0, 0.0), Vector3::new(3.0, 0.0, 0.0));
        assert!((err.x - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pin_joint_error_magnitude() {
        let (jid, a, b) = ids();
        let pin = PinJoint3D::new(jid, a, b);
        let mag = pin.error_magnitude(Vector3::ZERO, Vector3::new(3.0, 4.0, 0.0));
        assert!((mag - 5.0).abs() < 1e-5);
    }

    // -- HingeJoint3D --

    #[test]
    fn hinge_joint_defaults() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b);
        assert_eq!(hinge.axis, Vector3::new(0.0, 1.0, 0.0));
        assert!(!hinge.use_limit);
        assert!(!hinge.motor_enabled);
        assert!((hinge.limit_bias - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn hinge_joint_with_axis() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b).with_axis(Vector3::new(0.0, 0.0, 2.0));
        // Should be normalized
        assert!((hinge.axis.z - 1.0).abs() < 1e-5);
        assert!(hinge.axis.x.abs() < 1e-5);
    }

    #[test]
    fn hinge_joint_with_limits() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b).with_limits(-PI / 4.0, PI / 2.0);
        assert!(hinge.use_limit);
        assert!((hinge.lower_limit + PI / 4.0).abs() < 1e-5);
        assert!((hinge.upper_limit - PI / 2.0).abs() < 1e-5);
    }

    #[test]
    fn hinge_angle_within_limits() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b).with_limits(-1.0, 1.0);
        assert!(hinge.angle_within_limits(0.0));
        assert!(hinge.angle_within_limits(1.0));
        assert!(!hinge.angle_within_limits(1.5));
        assert!(!hinge.angle_within_limits(-1.5));
    }

    #[test]
    fn hinge_angle_no_limits_always_valid() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b);
        assert!(hinge.angle_within_limits(100.0));
    }

    #[test]
    fn hinge_clamp_angle() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b).with_limits(-1.0, 1.0);
        assert!((hinge.clamp_angle(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((hinge.clamp_angle(2.0) - 1.0).abs() < f32::EPSILON);
        assert!((hinge.clamp_angle(-2.0) + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn hinge_motor() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b).with_motor(10.0, 5.0);
        assert!(hinge.motor_enabled);
        let impulse = hinge.compute_motor_impulse(0.0, 1.0 / 60.0);
        assert!(impulse > 0.0);
        assert!(impulse <= hinge.motor_max_impulse);
    }

    #[test]
    fn hinge_motor_disabled_returns_zero() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b);
        assert!((hinge.compute_motor_impulse(5.0, 1.0 / 60.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn hinge_motor_clamped() {
        let (jid, a, b) = ids();
        let hinge = HingeJoint3D::new(jid, a, b).with_motor(1000.0, 0.5);
        let impulse = hinge.compute_motor_impulse(0.0, 1.0);
        assert!((impulse - 0.5).abs() < f32::EPSILON);
    }

    // -- SliderJoint3D --

    #[test]
    fn slider_joint_defaults() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b);
        assert_eq!(slider.axis, Vector3::new(1.0, 0.0, 0.0));
        assert!(!slider.use_linear_limit);
        assert!(!slider.use_angular_limit);
    }

    #[test]
    fn slider_with_axis() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b).with_axis(Vector3::new(0.0, 3.0, 0.0));
        assert!((slider.axis.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn slider_linear_limits() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b).with_linear_limits(-2.0, 5.0);
        assert!(slider.use_linear_limit);
        assert!(slider.linear_within_limits(0.0));
        assert!(slider.linear_within_limits(-2.0));
        assert!(slider.linear_within_limits(5.0));
        assert!(!slider.linear_within_limits(5.1));
        assert!(!slider.linear_within_limits(-2.1));
    }

    #[test]
    fn slider_linear_clamp() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b).with_linear_limits(-1.0, 1.0);
        assert!((slider.clamp_linear(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((slider.clamp_linear(3.0) - 1.0).abs() < f32::EPSILON);
        assert!((slider.clamp_linear(-3.0) + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn slider_angular_limits() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b).with_angular_limits(-PI, PI);
        assert!(slider.use_angular_limit);
        assert!(slider.angular_within_limits(0.0));
        assert!(!slider.angular_within_limits(PI + 0.1));
    }

    #[test]
    fn slider_angular_clamp() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b).with_angular_limits(-1.0, 1.0);
        assert!((slider.clamp_angular(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((slider.clamp_angular(5.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn slider_no_limits_always_valid() {
        let (jid, a, b) = ids();
        let slider = SliderJoint3D::new(jid, a, b);
        assert!(slider.linear_within_limits(999.0));
        assert!(slider.angular_within_limits(999.0));
    }

    // -- Joint3D enum --

    #[test]
    fn joint3d_pin_variant() {
        let (jid, a, b) = ids();
        let joint = Joint3D::Pin(PinJoint3D::new(jid, a, b));
        assert_eq!(joint.id(), jid);
        assert!(joint.is_enabled());
        assert_eq!(joint.bodies(), (a, b));
    }

    #[test]
    fn joint3d_hinge_variant() {
        let (jid, a, b) = ids();
        let joint = Joint3D::Hinge(HingeJoint3D::new(jid, a, b));
        assert_eq!(joint.id(), jid);
    }

    #[test]
    fn joint3d_slider_variant() {
        let (jid, a, b) = ids();
        let joint = Joint3D::Slider(SliderJoint3D::new(jid, a, b));
        assert_eq!(joint.id(), jid);
    }

    #[test]
    fn joint3d_base_mut() {
        let (jid, a, b) = ids();
        let mut joint = Joint3D::Pin(PinJoint3D::new(jid, a, b));
        joint.base_mut().enabled = false;
        assert!(!joint.is_enabled());
    }

    #[test]
    fn joint3d_disable_collision() {
        let (jid, a, b) = ids();
        let mut joint = Joint3D::Hinge(HingeJoint3D::new(jid, a, b));
        assert!(joint.base().exclude_nodes_from_collision);
        joint.base_mut().exclude_nodes_from_collision = false;
        assert!(!joint.base().exclude_nodes_from_collision);
    }

    #[test]
    fn joint_id_equality() {
        assert_eq!(JointId3D(1), JointId3D(1));
        assert_ne!(JointId3D(1), JointId3D(2));
    }
}
