//! Rotation-aware SAT (Separating Axis Theorem) narrowphase for oriented 2D
//! boxes (pat-nzgdu.57).
//!
//! Patina's existing 2D collision only handles axis-aligned circle/rect pairs
//! and throws rotation away. This module provides the rotation-aware core the
//! narrowphase needs: an oriented-box-vs-oriented-box overlap test that returns
//! the minimum-translation contact. It is a self-contained primitive (pure
//! geometry over `gdcore::math::Vector2`) that the `collision` dispatch can call
//! once shapes carry a rotation. A rotated box whose corner pokes into another
//! is detected even when their axis-aligned bounds would report a gap.

use gdcore::math::Vector2;

/// An oriented bounding box: `center` in world space, `half` extents measured
/// along the box's own local axes, rotated by `rotation` radians.
#[derive(Debug, Clone, Copy)]
pub struct Obb2D {
    /// World-space center of the box.
    pub center: Vector2,
    /// Half-extents along the box's local x/y axes (before rotation).
    pub half: Vector2,
    /// Rotation of the box in radians (counter-clockwise).
    pub rotation: f32,
}

/// A narrowphase contact between two oriented boxes: the minimum-translation
/// `normal` (a unit vector pointing from box A toward box B) and the
/// penetration `depth` along it.
#[derive(Debug, Clone, Copy)]
pub struct ObbContact {
    /// Unit collision normal, pointing from A toward B.
    pub normal: Vector2,
    /// Penetration depth along `normal` (always positive on contact).
    pub depth: f32,
}

impl Obb2D {
    /// The box's world-space local axes `(u, v)` for its rotation. Both are
    /// unit vectors, so they double as candidate separating axes.
    fn axes(self) -> (Vector2, Vector2) {
        let (s, c) = self.rotation.sin_cos();
        (Vector2::new(c, s), Vector2::new(-s, c))
    }

    /// Half-width of the box's projection onto the unit vector `axis`, given the
    /// box's own axes `(u, v)`.
    fn projection_radius(self, u: Vector2, v: Vector2, axis: Vector2) -> f32 {
        (self.half.x * u.dot(axis)).abs() + (self.half.y * v.dot(axis)).abs()
    }
}

/// Tests two oriented boxes for overlap via the Separating Axis Theorem over
/// their four face normals. Returns the minimum-penetration [`ObbContact`]
/// (normal pointing from `a` toward `b`) when they overlap, or `None` if any
/// axis separates them.
pub fn obb_vs_obb(a: Obb2D, b: Obb2D) -> Option<ObbContact> {
    let (ua, va) = a.axes();
    let (ub, vb) = b.axes();
    let candidate_axes = [ua, va, ub, vb];
    let d = b.center - a.center; // from A's center toward B's center

    let mut min_overlap = f32::INFINITY;
    let mut min_axis = Vector2::ZERO;
    for axis in candidate_axes {
        let ra = a.projection_radius(ua, va, axis);
        let rb = b.projection_radius(ub, vb, axis);
        let center_dist = d.dot(axis).abs();
        let overlap = ra + rb - center_dist;
        if overlap <= 0.0 {
            return None; // separating axis found -> no collision
        }
        if overlap < min_overlap {
            min_overlap = overlap;
            // Orient the normal so it points from A toward B along this axis.
            min_axis = if d.dot(axis) < 0.0 { -axis } else { axis };
        }
    }
    Some(ObbContact {
        normal: min_axis,
        depth: min_overlap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn obb(cx: f32, cy: f32, hx: f32, hy: f32, rot: f32) -> Obb2D {
        Obb2D {
            center: Vector2::new(cx, cy),
            half: Vector2::new(hx, hy),
            rotation: rot,
        }
    }

    #[test]
    fn axis_aligned_overlap_reports_normal_and_depth() {
        // Unit boxes 1.5 apart on x overlap by 0.5 along +x.
        let c = obb_vs_obb(obb(0.0, 0.0, 1.0, 1.0, 0.0), obb(1.5, 0.0, 1.0, 1.0, 0.0))
            .expect("boxes overlap");
        assert!(approx(c.depth, 0.5), "depth {}", c.depth);
        assert!(
            approx(c.normal.x, 1.0) && approx(c.normal.y, 0.0),
            "normal {:?}",
            c.normal
        );
    }

    #[test]
    fn axis_aligned_separated_returns_none() {
        assert!(obb_vs_obb(obb(0.0, 0.0, 1.0, 1.0, 0.0), obb(3.0, 0.0, 1.0, 1.0, 0.0)).is_none());
    }

    #[test]
    fn rotation_detects_corner_overlap_an_aabb_would_miss() {
        // Box B sits 2.2 right of A. Axis-aligned, B's half-width (1) leaves a
        // 0.2 gap. Rotated 45°, B's corner reaches sqrt(2) ~= 1.414 along x and
        // pokes into A, so SAT reports a hit an AABB test would miss.
        let a = obb(0.0, 0.0, 1.0, 1.0, 0.0);
        let b = obb(2.2, 0.0, 1.0, 1.0, std::f32::consts::FRAC_PI_4);
        let c = obb_vs_obb(a, b).expect("rotated corner overlaps");
        // Min-penetration axis is A's x: overlap = 1 + sqrt(2) - 2.2 ~= 0.2142.
        assert!(
            approx(c.depth, 1.0 + std::f32::consts::SQRT_2 - 2.2),
            "depth {}",
            c.depth
        );
        assert!(
            approx(c.normal.x, 1.0) && approx(c.normal.y, 0.0),
            "normal {:?}",
            c.normal
        );
        // The same geometry unrotated must NOT collide — proving rotation is
        // what produced the contact.
        assert!(obb_vs_obb(a, obb(2.2, 0.0, 1.0, 1.0, 0.0)).is_none());
    }
}
