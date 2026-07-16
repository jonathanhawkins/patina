//! Box-projected cubemap reflection correction for `ReflectionProbe`
//! (pat-u3l8c.40).
//!
//! A plain (infinite) cubemap lookup uses only the reflection direction, which
//! makes a reflected environment appear infinitely far away and slide
//! incorrectly as the viewer moves. **Box projection** ("parallax-corrected
//! cubemap") intersects the reflection ray with the probe's finite bounding box
//! and re-aims the lookup at that hit point, so flat walls/floors reflect
//! correctly inside the probe volume.
//!
//! This is the pure-`Vector3` math [`ReflectionProbe`](crate::reflection_probe)
//! needs once `box_projection` is enabled.
//!
//! # Scope / remaining work (pat-u3l8c.40)
//!
//! This delivers only the box-projection direction correction. The rest of the
//! bead — actually **capturing** the surrounding scene into the probe's cubemap
//! and sampling that specular cubemap on the GPU — is not implemented here.

use gdcore::math::Vector3;

const EPS: f32 = 1e-9;

/// Corrects a reflection direction for box projection.
///
/// Given a surface `world_pos`, a (not necessarily unit) `reflection`
/// direction, the probe's world-space box (`box_min`..`box_max`), and the
/// cubemap capture `origin`, returns the unit direction from `origin` toward the
/// point where the reflection ray leaves the box. When the ray direction is
/// degenerate the original reflection direction (normalized) is returned.
pub fn box_project_reflection(
    world_pos: Vector3,
    reflection: Vector3,
    box_min: Vector3,
    box_max: Vector3,
    origin: Vector3,
) -> Vector3 {
    if reflection.length() < EPS {
        return reflection;
    }
    // Distance along the ray to leave the box on each axis (slab method); axes
    // parallel to the ray impose no exit constraint.
    let ex = axis_exit(world_pos.x, reflection.x, box_min.x, box_max.x);
    let ey = axis_exit(world_pos.y, reflection.y, box_min.y, box_max.y);
    let ez = axis_exit(world_pos.z, reflection.z, box_min.z, box_max.z);
    let dist = ex.min(ey).min(ez);

    if !dist.is_finite() || dist <= 0.0 {
        // No forward exit (e.g. ray points out of an unbounded axis set): fall
        // back to the plain reflection direction.
        return reflection.normalized();
    }

    let hit = world_pos + reflection * dist;
    (hit - origin).normalized()
}

/// The distance `t >= ...` at which `p + r·t` exits the `[lo, hi]` slab on one
/// axis (the further of the two crossings), or `+∞` when the ray is parallel to
/// the slab.
fn axis_exit(p: f32, r: f32, lo: f32, hi: f32) -> f32 {
    if r.abs() < EPS {
        return f32::INFINITY;
    }
    let t_hi = (hi - p) / r;
    let t_lo = (lo - p) / r;
    t_hi.max(t_lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vector3, b: Vector3) -> bool {
        a.distance_to(b) < 1e-4
    }

    fn unit_box() -> (Vector3, Vector3) {
        (Vector3::new(-1.0, -1.0, -1.0), Vector3::new(1.0, 1.0, 1.0))
    }

    #[test]
    fn from_center_axis_reflection_is_unchanged() {
        // At the probe center, a +X reflection exits at (1,0,0); corrected dir
        // from the (0,0,0) origin is still +X.
        let (lo, hi) = unit_box();
        let d = box_project_reflection(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            lo,
            hi,
            Vector3::new(0.0, 0.0, 0.0),
        );
        assert!(approx(d, Vector3::new(1.0, 0.0, 0.0)), "d {d:?}");
    }

    #[test]
    fn offset_position_reprojects_direction() {
        // From (-0.5,0,0) a +X ray still exits the box at (1,0,0); measured from
        // the origin that is +X, but the exit distance differs (parallax).
        let (lo, hi) = unit_box();
        let origin = Vector3::new(0.0, 0.0, 0.0);
        let d = box_project_reflection(
            Vector3::new(-0.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            lo,
            hi,
            origin,
        );
        assert!(approx(d, Vector3::new(1.0, 0.0, 0.0)), "d {d:?}");

        // From an off-center point looking +X, an origin offset in Y tilts the
        // corrected direction downward.
        let d2 = box_project_reflection(
            Vector3::new(0.0, 0.5, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            lo,
            hi,
            Vector3::new(0.0, 0.0, 0.0),
        );
        // Ray exits at (1, 0.5, 0); from origin that is (1,0.5,0) normalized.
        assert!(
            approx(d2, Vector3::new(1.0, 0.5, 0.0).normalized()),
            "d2 {d2:?}"
        );
    }

    #[test]
    fn diagonal_reflection_hits_corner_edge() {
        let (lo, hi) = unit_box();
        let d = box_project_reflection(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0), // non-unit diagonal
            lo,
            hi,
            Vector3::new(0.0, 0.0, 0.0),
        );
        // Exits at (1,1,0); normalized corrected dir is the 45° diagonal.
        assert!(
            approx(d, Vector3::new(1.0, 1.0, 0.0).normalized()),
            "d {d:?}"
        );
    }

    #[test]
    fn result_is_unit_length() {
        let (lo, hi) = unit_box();
        let d = box_project_reflection(
            Vector3::new(0.3, -0.2, 0.6),
            Vector3::new(0.4, 0.7, -0.2),
            lo,
            hi,
            Vector3::new(0.1, 0.1, 0.1),
        );
        assert!((d.length() - 1.0).abs() < 1e-4, "not unit: {}", d.length());
    }

    #[test]
    fn non_cubic_box_exit_picks_nearest_wall() {
        // A wide, shallow box: a +Z reflection exits the near Z wall first.
        let lo = Vector3::new(-10.0, -10.0, -1.0);
        let hi = Vector3::new(10.0, 10.0, 1.0);
        let d = box_project_reflection(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            lo,
            hi,
            Vector3::new(0.0, 0.0, 0.0),
        );
        assert!(approx(d, Vector3::new(0.0, 0.0, 1.0)), "d {d:?}");
    }

    #[test]
    fn degenerate_reflection_returns_input() {
        let (lo, hi) = unit_box();
        let d = box_project_reflection(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            lo,
            hi,
            Vector3::new(0.0, 0.0, 0.0),
        );
        assert!(approx(d, Vector3::new(0.0, 0.0, 0.0)));
    }
}
