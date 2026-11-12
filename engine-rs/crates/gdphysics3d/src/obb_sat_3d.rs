//! Rotation-aware SAT narrowphase for oriented 3D boxes (pat-nzgdu.60).
//!
//! Patina's 3D solver only tests axis-aligned box pairs (`collision.rs`),
//! ignoring `body.rotation`. This module is the geometric core the
//! orientation-aware path needs: an oriented-box-vs-oriented-box overlap test
//! via the Separating Axis Theorem over all 15 candidate axes (3 face normals
//! per box + 9 edge-edge cross products), returning the minimum-translation
//! contact. It is a self-contained primitive over `gdcore::math::Vector3`; the
//! `collision.rs` dispatch feeds it each body's world axes (from its `Basis`).

use gdcore::math::Vector3;

/// An oriented box: world-space `center`, `half` extents along its own local
/// axes, and the three orthonormal world-space `axes` (columns of the body's
/// rotation basis).
#[derive(Debug, Clone, Copy)]
pub struct Obb3D {
    /// World-space center.
    pub center: Vector3,
    /// Half-extents along the local x/y/z axes.
    pub half: Vector3,
    /// The box's three orthonormal world-space axes (local x, y, z).
    pub axes: [Vector3; 3],
}

/// A narrowphase contact: unit `normal` (pointing from box A toward box B) and
/// penetration `depth` along it.
#[derive(Debug, Clone, Copy)]
pub struct ObbContact3D {
    /// Unit collision normal, from A toward B.
    pub normal: Vector3,
    /// Penetration depth along `normal` (positive on contact).
    pub depth: f32,
}

impl Obb3D {
    /// An axis-aligned box (identity orientation).
    pub fn aabb(center: Vector3, half: Vector3) -> Self {
        Self {
            center,
            half,
            axes: [
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ],
        }
    }

    /// Half-width of this box's projection onto the unit vector `axis`.
    fn projection_radius(&self, axis: Vector3) -> f32 {
        (self.half.x * self.axes[0].dot(axis)).abs()
            + (self.half.y * self.axes[1].dot(axis)).abs()
            + (self.half.z * self.axes[2].dot(axis)).abs()
    }
}

/// Tests two oriented boxes for overlap using SAT over the 15 candidate axes.
/// Returns the minimum-penetration [`ObbContact3D`] (normal from `a` toward `b`)
/// when they overlap, or `None` if any axis separates them. Near-parallel edge
/// pairs (degenerate cross products) are skipped.
pub fn obb_vs_obb_3d(a: &Obb3D, b: &Obb3D) -> Option<ObbContact3D> {
    let mut axes: Vec<Vector3> = Vec::with_capacity(15);
    axes.extend_from_slice(&a.axes);
    axes.extend_from_slice(&b.axes);
    for &ea in &a.axes {
        for &eb in &b.axes {
            let cross = ea.cross(eb);
            if cross.length_squared() > 1e-8 {
                axes.push(cross.normalized());
            }
        }
    }

    let d = b.center - a.center; // from A toward B
    let mut min_overlap = f32::INFINITY;
    let mut min_axis = Vector3::ZERO;
    for axis in axes {
        let ra = a.projection_radius(axis);
        let rb = b.projection_radius(axis);
        let center_dist = d.dot(axis).abs();
        let overlap = ra + rb - center_dist;
        if overlap <= 0.0 {
            return None; // separating axis found
        }
        if overlap < min_overlap {
            min_overlap = overlap;
            min_axis = if d.dot(axis) < 0.0 { -axis } else { axis };
        }
    }
    Some(ObbContact3D {
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

    // Axes for a box rotated by `angle` radians about the +Z axis.
    fn z_rot_axes(angle: f32) -> [Vector3; 3] {
        let (s, c) = angle.sin_cos();
        [
            Vector3::new(c, s, 0.0),
            Vector3::new(-s, c, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
    }

    #[test]
    fn axis_aligned_overlap_reports_normal_and_depth() {
        let a = Obb3D::aabb(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0));
        let b = Obb3D::aabb(Vector3::new(1.5, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        let c = obb_vs_obb_3d(&a, &b).expect("boxes overlap");
        assert!(approx(c.depth, 0.5), "depth {}", c.depth);
        assert!(
            approx(c.normal.x, 1.0) && approx(c.normal.y, 0.0) && approx(c.normal.z, 0.0),
            "normal {:?}",
            c.normal
        );
    }

    #[test]
    fn axis_aligned_separated_returns_none() {
        let a = Obb3D::aabb(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0));
        let b = Obb3D::aabb(Vector3::new(3.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        assert!(obb_vs_obb_3d(&a, &b).is_none());
    }

    #[test]
    fn rotation_about_z_detects_corner_overlap_an_aabb_would_miss() {
        // Box B is 2.2 to the right — axis-aligned its half-width (1) leaves a
        // 0.2 gap, but rotated 45deg about Z its corner reaches sqrt(2) along x.
        let a = Obb3D::aabb(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0));
        let b = Obb3D {
            center: Vector3::new(2.2, 0.0, 0.0),
            half: Vector3::new(1.0, 1.0, 1.0),
            axes: z_rot_axes(std::f32::consts::FRAC_PI_4),
        };
        let c = obb_vs_obb_3d(&a, &b).expect("rotated corner overlaps");
        assert!(
            approx(c.depth, 1.0 + std::f32::consts::SQRT_2 - 2.2),
            "depth {}",
            c.depth
        );
        assert!(approx(c.normal.x, 1.0), "normal {:?}", c.normal);
        // Same geometry unrotated must NOT collide.
        let b_flat = Obb3D::aabb(Vector3::new(2.2, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        assert!(obb_vs_obb_3d(&a, &b_flat).is_none());
    }

    #[test]
    fn separation_on_z_axis() {
        let a = Obb3D::aabb(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0));
        let b = Obb3D::aabb(Vector3::new(0.0, 0.0, 2.5), Vector3::new(1.0, 1.0, 1.0));
        assert!(obb_vs_obb_3d(&a, &b).is_none());
    }
}
