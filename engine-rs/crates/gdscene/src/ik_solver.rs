//! Inverse-kinematics solvers for the skeleton modifier stack (pat-u3l8c.103).
//!
//! Self-contained, pure-`std` position solvers over [`gdcore::math::Vector3`]:
//!
//! * [`solve_two_bone_ik`] — the analytic two-bone solver Godot's `SkeletonIK3D`
//!   / two-bone modifier uses for an arm/leg reaching a target, with a pole
//!   vector to disambiguate the bend plane.
//! * [`solve_fabrik`] — the iterative FABRIK (Forward And Backward Reaching
//!   Inverse Kinematics) solver for an N-bone chain.
//!
//! Both operate purely on world-space joint positions and preserve the input
//! bone lengths; converting the resulting positions back into per-bone rotations
//! (a `Basis`/`Quaternion` for each joint) is left to the caller.
//!
//! # Scope / remaining work (pat-u3l8c.103, a P2 pack)
//!
//! This delivers the IK **math core** (two-bone reach + FABRIK). The pack's
//! other pieces are NOT implemented here and remain open: the
//! `SkeletonModifier3D` process phase wired after pose update,
//! `LookAtModifier3D` (rotation output), CCD as an alternative solver, and
//! `SkeletonProfile` / `BoneMap` retargeting.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

const EPS: f32 = 1e-6;

/// The result of a two-bone solve: the new mid-joint (elbow/knee) position and
/// the end-effector (hand/foot) position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoBoneSolution {
    /// The mid joint (elbow/knee) world position.
    pub joint: Vector3,
    /// The end-effector (hand/foot) world position.
    pub end: Vector3,
}

/// Solves a two-bone IK chain analytically.
///
/// `root` is the fixed base (shoulder/hip), `target` the desired end-effector
/// position, and `pole` a hint position that the mid-joint bends toward.
/// `upper_len` is the root→joint bone length and `lower_len` the joint→end
/// length.
///
/// The returned joint is always exactly `upper_len` from `root` and `lower_len`
/// from the returned end. When the target is within reach the end equals the
/// (reach-clamped) target; when it is out of reach the chain extends straight
/// toward the target.
///
/// ```
/// use gdscene::ik_solver::solve_two_bone_ik;
/// use gdcore::math::Vector3;
///
/// let s = solve_two_bone_ik(
///     Vector3::new(0.0, 0.0, 0.0),
///     Vector3::new(1.0, 0.0, 0.0),
///     Vector3::new(0.0, 1.0, 0.0),
///     1.0,
///     1.0,
/// );
/// // Reachable: the end lands on the target.
/// assert!(s.end.distance_to(Vector3::new(1.0, 0.0, 0.0)) < 1e-4);
/// ```
pub fn solve_two_bone_ik(
    root: Vector3,
    target: Vector3,
    pole: Vector3,
    upper_len: f32,
    lower_len: f32,
) -> TwoBoneSolution {
    let to_target = target - root;
    let dist = to_target.length();
    let axis = if dist > EPS {
        to_target * (1.0 / dist)
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    };

    let reach = upper_len + lower_len;
    let min_reach = (upper_len - lower_len).abs();
    // Clamp the working distance into the annulus the chain can actually span,
    // keeping it strictly positive so the projection math is well-defined.
    let eff = dist.clamp(min_reach, reach).max(EPS);

    // Projection of the joint onto the root→target axis, and its perpendicular
    // offset (law of cosines / circle intersection).
    let x = (upper_len * upper_len - lower_len * lower_len + eff * eff) / (2.0 * eff);
    let h_sq = upper_len * upper_len - x * x;
    let h = if h_sq > 0.0 { h_sq.sqrt() } else { 0.0 };

    let perp = pole_direction(root, pole, axis);
    let joint = root + axis * x + perp * h;
    let end = root + axis * eff;

    TwoBoneSolution { joint, end }
}

/// Solves an N-joint chain toward `target` using FABRIK.
///
/// `joints` are the current world positions (at least two; `joints[0]` is the
/// fixed root). Bone lengths are taken from the input spacing and preserved.
/// Runs up to `iterations` passes, stopping early once the end is within
/// `tolerance` of the target. Returns the new joint positions.
pub fn solve_fabrik(
    joints: &[Vector3],
    target: Vector3,
    iterations: usize,
    tolerance: f32,
) -> Vec<Vector3> {
    let n = joints.len();
    if n < 2 {
        return joints.to_vec();
    }

    let lengths: Vec<f32> = (0..n - 1)
        .map(|i| joints[i].distance_to(joints[i + 1]))
        .collect();
    let total: f32 = lengths.iter().sum();
    let root = joints[0];
    let mut pts = joints.to_vec();

    if root.distance_to(target) >= total {
        // Unreachable: lay the chain out straight toward the target.
        let dir = unit_dir(root, target);
        for i in 1..n {
            pts[i] = pts[i - 1] + dir * lengths[i - 1];
        }
        return pts;
    }

    for _ in 0..iterations {
        // Backward pass: pin the end to the target, walk toward the root.
        pts[n - 1] = target;
        for i in (0..n - 1).rev() {
            let dir = unit_dir(pts[i + 1], pts[i]);
            pts[i] = pts[i + 1] + dir * lengths[i];
        }
        // Forward pass: pin the root back, walk toward the end.
        pts[0] = root;
        for i in 0..n - 1 {
            let dir = unit_dir(pts[i], pts[i + 1]);
            pts[i + 1] = pts[i] + dir * lengths[i];
        }
        if pts[n - 1].distance_to(target) < tolerance {
            break;
        }
    }

    pts
}

/// Unit direction `from` → `to`, or `+X` if the two points coincide.
fn unit_dir(from: Vector3, to: Vector3) -> Vector3 {
    let d = to - from;
    if d.length() > EPS {
        d.normalized()
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    }
}

/// A unit vector perpendicular to `axis` pointing toward `pole` (relative to
/// `root`). Falls back to an arbitrary perpendicular when the pole is colinear.
fn pole_direction(root: Vector3, pole: Vector3, axis: Vector3) -> Vector3 {
    let pole_vec = pole - root;
    let perp = pole_vec - axis * pole_vec.dot(axis);
    let len = perp.length();
    if len > EPS {
        perp * (1.0 / len)
    } else {
        any_perpendicular(axis)
    }
}

/// Any unit vector perpendicular to (approximately unit) `axis`.
fn any_perpendicular(axis: Vector3) -> Vector3 {
    let seed = if axis.x.abs() < 0.9 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let perp = seed - axis * seed.dot(axis);
    let len = perp.length();
    if len > EPS {
        perp * (1.0 / len)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    }
}

/// The shortest-arc rotation turning direction `from` onto direction `to` (both
/// normalized internally) — the rotation core of Godot's `AimModifier3D`, which
/// turns a bone's forward axis to point at a target.
///
/// Returns the identity for a zero-length or already-aligned input, and a 180°
/// turn about an arbitrary perpendicular axis for anti-parallel inputs.
pub fn aim_rotation(from: Vector3, to: Vector3) -> Quaternion {
    if from.length() < EPS || to.length() < EPS {
        return Quaternion::IDENTITY;
    }
    let a = from.normalized();
    let b = to.normalized();
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 1.0 - EPS {
        // Already aligned.
        return Quaternion::IDENTITY;
    }
    if d < -1.0 + EPS {
        // Anti-parallel: rotate 180° about any axis perpendicular to `a`.
        return Quaternion::from_axis_angle(any_perpendicular(a), std::f32::consts::PI);
    }
    Quaternion::from_axis_angle(a.cross(b).normalized(), d.acos())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn two_bone_reaches_reachable_target() {
        let root = Vector3::new(0.0, 0.0, 0.0);
        let target = Vector3::new(1.0, 0.0, 0.0);
        let pole = Vector3::new(0.0, 1.0, 0.0);
        let s = solve_two_bone_ik(root, target, pole, 1.0, 1.0);

        // End reaches the target; bone lengths are exact.
        assert!(s.end.distance_to(target) < 1e-4, "end {:?}", s.end);
        assert!(
            approx(root.distance_to(s.joint), 1.0),
            "upper {}",
            root.distance_to(s.joint)
        );
        assert!(
            approx(s.joint.distance_to(s.end), 1.0),
            "lower {}",
            s.joint.distance_to(s.end)
        );
        // Bends toward the pole (+Y).
        assert!(
            s.joint.y > 0.0,
            "joint should bend toward pole: {:?}",
            s.joint
        );
    }

    #[test]
    fn two_bone_known_elbow_position() {
        // Symmetric unit bones reaching (1,0,0) put the elbow at (0.5, √0.75, 0).
        let s = solve_two_bone_ik(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            1.0,
            1.0,
        );
        assert!(approx(s.joint.x, 0.5), "x {}", s.joint.x);
        assert!(approx(s.joint.y, 0.75_f32.sqrt()), "y {}", s.joint.y);
        assert!(approx(s.joint.z, 0.0), "z {}", s.joint.z);
    }

    #[test]
    fn two_bone_out_of_reach_extends_straight() {
        let root = Vector3::new(0.0, 0.0, 0.0);
        let target = Vector3::new(5.0, 0.0, 0.0);
        let pole = Vector3::new(0.0, 1.0, 0.0);
        let s = solve_two_bone_ik(root, target, pole, 1.0, 1.0);

        // Fully extended toward the target: end at reach distance (2,0,0).
        assert!(
            s.end.distance_to(Vector3::new(2.0, 0.0, 0.0)) < 1e-4,
            "end {:?}",
            s.end
        );
        assert!(approx(root.distance_to(s.joint), 1.0));
        assert!(approx(s.joint.distance_to(s.end), 1.0));
        // Straight line: joint on the root→target axis.
        assert!(
            approx(s.joint.y, 0.0) && approx(s.joint.z, 0.0),
            "joint {:?}",
            s.joint
        );
    }

    #[test]
    fn two_bone_preserves_lengths_for_asymmetric_bones() {
        let root = Vector3::new(1.0, 2.0, -1.0);
        let target = Vector3::new(3.0, 2.5, 0.5);
        let pole = Vector3::new(1.0, 5.0, -1.0);
        let s = solve_two_bone_ik(root, target, pole, 2.0, 1.5);
        assert!(
            approx(root.distance_to(s.joint), 2.0),
            "upper {}",
            root.distance_to(s.joint)
        );
        assert!(
            approx(s.joint.distance_to(s.end), 1.5),
            "lower {}",
            s.joint.distance_to(s.end)
        );
    }

    #[test]
    fn fabrik_reaches_reachable_target() {
        let joints = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
        ];
        let target = Vector3::new(1.0, 1.0, 0.0); // dist √2 < reach 2
        let result = solve_fabrik(&joints, target, 32, 1e-5);

        assert_eq!(result.len(), 3);
        // Root pinned; end reaches target; bone lengths preserved.
        assert!(approx(result[0].distance_to(joints[0]), 0.0), "root moved");
        assert!(result[2].distance_to(target) < 1e-3, "end {:?}", result[2]);
        assert!(approx(result[0].distance_to(result[1]), 1.0));
        assert!(approx(result[1].distance_to(result[2]), 1.0));
    }

    #[test]
    fn fabrik_unreachable_extends_straight_and_keeps_lengths() {
        let joints = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
        ];
        let target = Vector3::new(10.0, 0.0, 0.0);
        let result = solve_fabrik(&joints, target, 16, 1e-5);
        // Straight toward target, end at total reach (2,0,0).
        assert!(
            result[2].distance_to(Vector3::new(2.0, 0.0, 0.0)) < 1e-4,
            "end {:?}",
            result[2]
        );
        assert!(approx(result[0].distance_to(result[1]), 1.0));
        assert!(approx(result[1].distance_to(result[2]), 1.0));
    }

    #[test]
    fn fabrik_three_bone_chain_converges() {
        let joints = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ];
        let target = Vector3::new(1.5, 1.5, 0.5);
        let result = solve_fabrik(&joints, target, 64, 1e-5);
        assert!(result[3].distance_to(target) < 1e-3, "end {:?}", result[3]);
        for i in 0..3 {
            assert!(
                approx(result[i].distance_to(result[i + 1]), 1.0),
                "bone {i}"
            );
        }
    }

    #[test]
    fn fabrik_short_chain_is_returned_unchanged() {
        let single = [Vector3::new(1.0, 2.0, 3.0)];
        let out = solve_fabrik(&single, Vector3::new(9.0, 9.0, 9.0), 10, 1e-5);
        assert_eq!(out, single.to_vec());
    }

    #[test]
    fn aim_rotation_aligns_forward_to_target() {
        // Aim +X at +Y: applying the rotation to +X yields ~+Y.
        let q = aim_rotation(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
        let rotated = q.xform(Vector3::new(1.0, 0.0, 0.0));
        assert!(
            (rotated - Vector3::new(0.0, 1.0, 0.0)).length() < 1e-5,
            "rotated {rotated:?}"
        );
    }

    #[test]
    fn aim_rotation_already_aligned_is_identity() {
        // Same direction (any magnitude) -> identity -> leaves vectors unchanged.
        let q = aim_rotation(Vector3::new(0.0, 0.0, 2.0), Vector3::new(0.0, 0.0, 5.0));
        let v = Vector3::new(1.0, 2.0, 3.0);
        assert!((q.xform(v) - v).length() < 1e-5);
    }

    #[test]
    fn aim_rotation_antiparallel_flips_180() {
        let from = Vector3::new(1.0, 0.0, 0.0);
        let q = aim_rotation(from, Vector3::new(-1.0, 0.0, 0.0));
        let rotated = q.xform(from);
        assert!(
            (rotated - Vector3::new(-1.0, 0.0, 0.0)).length() < 1e-4,
            "rotated {rotated:?}"
        );
    }

    #[test]
    fn aim_rotation_zero_input_is_identity() {
        let q = aim_rotation(Vector3::ZERO, Vector3::new(0.0, 1.0, 0.0));
        let v = Vector3::new(1.0, 0.0, 0.0);
        assert!((q.xform(v) - v).length() < 1e-6);
    }
}
