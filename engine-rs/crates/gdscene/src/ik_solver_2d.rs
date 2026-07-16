//! 2D inverse-kinematics solvers for the `Skeleton2D` modification stack
//! (`SkeletonModification2D` family).
//!
//! Self-contained, pure-`std` position solvers over [`gdcore::math::Vector2`].
//! This delivers the analytic **two-bone** solver that Godot's
//! `SkeletonModification2DTwoBoneIK` uses to bend a two-`Bone2D` chain toward a
//! target. Reading/writing the actual `Bone2D` transforms and executing the
//! modification stack are left to the caller (the node/stack layer).

use gdcore::math::Vector2;

const EPS: f32 = 1e-6;

/// The result of a two-bone 2D solve: the new middle joint and tip positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoBoneSolution2D {
    /// New middle-joint (the "elbow"/"knee") position.
    pub joint: Vector2,
    /// New end-effector (tip) position.
    pub tip: Vector2,
}

/// Solves a two-bone 2D chain rooted at `root` so its tip reaches `target` (or
/// the closest reachable point when out of range), using the law of cosines.
///
/// `l1` is the root→joint bone length and `l2` the joint→tip length. The target
/// distance is clamped to `[|l1 - l2|, l1 + l2]`, so an unreachable-far target
/// extends the chain straight and an unreachable-near one folds it. Two mirror
/// solutions exist; `bend_positive` selects the one whose joint sits on the
/// counter-clockwise side of the root→target line.
pub fn solve_two_bone_ik_2d(
    root: Vector2,
    target: Vector2,
    l1: f32,
    l2: f32,
    bend_positive: bool,
) -> TwoBoneSolution2D {
    let to_target = target - root;
    let dist = to_target.length();

    // Direction root→target, falling back to +X when the target coincides.
    let dir = if dist > EPS {
        to_target * (1.0 / dist)
    } else {
        Vector2::new(1.0, 0.0)
    };

    // Clamp the reach so the triangle is always solvable.
    let reach_min = (l1 - l2).abs();
    let reach_max = l1 + l2;
    let reach = dist.clamp(reach_min, reach_max);

    // Interior angle at the root between root→target and root→joint.
    let cos_a = if reach > EPS && l1 > EPS {
        ((l1 * l1 + reach * reach - l2 * l2) / (2.0 * l1 * reach)).clamp(-1.0, 1.0)
    } else {
        1.0
    };
    let a = cos_a.acos();

    let base_angle = dir.y.atan2(dir.x);
    let joint_angle = if bend_positive {
        base_angle + a
    } else {
        base_angle - a
    };
    let joint = root + Vector2::new(joint_angle.cos(), joint_angle.sin()) * l1;

    // Place the tip at l2 from the joint, toward the clamped target point.
    let clamped_target = root + dir * reach;
    let joint_to_target = clamped_target - joint;
    let jt_len = joint_to_target.length();
    let tip = if jt_len > EPS {
        joint + joint_to_target * (l2 / jt_len)
    } else {
        joint
    };

    TwoBoneSolution2D { joint, tip }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bone_2d_reaches_reachable_target() {
        let root = Vector2::ZERO;
        let target = Vector2::new(1.0, 1.0); // dist √2, within [0, 2]
        let s = solve_two_bone_ik_2d(root, target, 1.0, 1.0, true);
        // Tip lands on the target and both bone lengths are preserved.
        assert!((s.tip - target).length() < 1e-5, "tip {:?}", s.tip);
        assert!(((s.joint - root).length() - 1.0).abs() < 1e-5);
        assert!(((s.tip - s.joint).length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn two_bone_2d_unreachable_extends_straight() {
        let root = Vector2::ZERO;
        let target = Vector2::new(10.0, 0.0); // dist 10 > reach 2
        let s = solve_two_bone_ik_2d(root, target, 1.0, 1.0, true);
        assert!(
            (s.joint - Vector2::new(1.0, 0.0)).length() < 1e-5,
            "joint {:?}",
            s.joint
        );
        assert!(
            (s.tip - Vector2::new(2.0, 0.0)).length() < 1e-5,
            "tip {:?}",
            s.tip
        );
    }

    #[test]
    fn two_bone_2d_bend_direction_mirrors() {
        let root = Vector2::ZERO;
        let target = Vector2::new(1.4, 0.0);
        let up = solve_two_bone_ik_2d(root, target, 1.0, 1.0, true);
        let down = solve_two_bone_ik_2d(root, target, 1.0, 1.0, false);
        // Same tip; the joint mirrors across the root→target (X) axis.
        assert!((up.tip - down.tip).length() < 1e-5);
        assert!((up.joint.y - (-down.joint.y)).abs() < 1e-5);
        assert!(up.joint.y.abs() > 1e-3, "joint should be off-axis");
    }

    #[test]
    fn two_bone_2d_coincident_target_is_stable() {
        // Target at the root: no NaNs, chain stays at bone lengths.
        let root = Vector2::new(3.0, 4.0);
        let s = solve_two_bone_ik_2d(root, root, 1.0, 2.0, true);
        assert!(s.joint.x.is_finite() && s.joint.y.is_finite());
        assert!(s.tip.x.is_finite() && s.tip.y.is_finite());
        assert!(((s.joint - root).length() - 1.0).abs() < 1e-5);
    }
}
