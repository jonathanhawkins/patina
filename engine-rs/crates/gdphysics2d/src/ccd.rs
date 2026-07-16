//! Continuous collision detection (swept time-of-impact) for fast 2D bodies
//! (pat-u3l8c.53).
//!
//! Discrete physics samples positions once per step, so a small fast body can
//! pass entirely through a thin obstacle between two frames ("tunneling").
//! These pure-`Vector2` swept queries return the earliest **time of impact**
//! `t ∈ [0, 1]` along a body's per-step displacement, letting the solver clamp
//! motion to the first contact instead.
//!
//! # Scope / remaining work (pat-u3l8c.53, a Physics2D pack)
//!
//! This delivers only the CCD/time-of-impact core (the "bullet does not tunnel"
//! acceptance point). The pack's other sub-features remain open: body
//! sleeping/islands, the `GrooveJoint` + two-anchor `PinJoint` fix,
//! `AnimatableBody2D`, render-time physics interpolation, and deterministic
//! iteration ordering.

use gdcore::math::Vector2;

const EPS: f32 = 1e-9;

/// Earliest `t ∈ [0, 1]` at which the point `origin + disp·t` first reaches
/// distance `radius` of `center`, or `None` if it never does within the step.
/// Returns `Some(0.0)` when the point starts inside the radius.
pub fn ray_circle_toi(origin: Vector2, disp: Vector2, center: Vector2, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let c_term = oc.dot(oc) - radius * radius;
    if c_term <= 0.0 {
        return Some(0.0); // already within the radius
    }
    let a = disp.dot(disp);
    if a <= EPS {
        return None; // not moving and outside
    }
    let b = 2.0 * oc.dot(disp);
    let disc = b * b - 4.0 * a * c_term;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a); // earliest root
    if (0.0..=1.0).contains(&t) {
        Some(t)
    } else {
        None
    }
}

/// Earliest time of impact of a circle (`center`, `radius`) swept by `vel`
/// against a static circle (`target`, `target_radius`), or `None`.
pub fn moving_circle_vs_circle_toi(
    center: Vector2,
    vel: Vector2,
    radius: f32,
    target: Vector2,
    target_radius: f32,
) -> Option<f32> {
    ray_circle_toi(center, vel, target, radius + target_radius)
}

/// Earliest time of impact of a circle (`center`, `radius`) swept by `vel`
/// against a static segment `a`–`b`, or `None`. Handles the two end-caps and
/// the flat side (the segment expanded by `radius` into a capsule).
pub fn moving_circle_vs_segment_toi(
    center: Vector2,
    vel: Vector2,
    radius: f32,
    a: Vector2,
    b: Vector2,
) -> Option<f32> {
    // Already touching the segment → immediate impact.
    if point_to_segment_distance_sq(center, a, b) <= radius * radius {
        return Some(0.0);
    }

    // End-caps.
    let mut best = min_opt(
        ray_circle_toi(center, vel, a, radius),
        ray_circle_toi(center, vel, b, radius),
    );

    // Flat side: the circle center crosses one of the two offset lines and the
    // contact foot lies on the segment body.
    let seg = b - a;
    let seg_len2 = seg.dot(seg);
    if seg_len2 > EPS {
        let normal = Vector2::new(-seg.y, seg.x);
        let nu = normal * (1.0 / normal.length());
        let d0 = (center - a).dot(nu); // signed distance from the line
        let dn = vel.dot(nu); // approach rate along the normal
        if dn.abs() > EPS {
            let target = if d0 >= 0.0 { radius } else { -radius };
            let t = (target - d0) / dn;
            if (0.0..=1.0).contains(&t) {
                let center_t = center + vel * t;
                let foot = center_t - nu * target; // point on the core line
                let proj = (foot - a).dot(seg) / seg_len2;
                if (0.0..=1.0).contains(&proj) {
                    best = min_opt(best, Some(t));
                }
            }
        }
    }

    best
}

/// Squared distance from `p` to segment `a`–`b`.
fn point_to_segment_distance_sq(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    let t = if l2 > EPS {
        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let foot = a + ab * t;
    let d = p - foot;
    d.dot(d)
}

/// The smaller of two optional times of impact.
fn min_opt(x: Option<f32>, y: Option<f32>) -> Option<f32> {
    match (x, y) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn circle_vs_circle_head_on() {
        // r=1 circle at (-3,0) moving +6x toward a r=1 circle at origin: contact
        // when centers are 2 apart, i.e. after 1 unit of travel → t = 1/6.
        let t = moving_circle_vs_circle_toi(
            Vector2::new(-3.0, 0.0),
            Vector2::new(6.0, 0.0),
            1.0,
            Vector2::new(0.0, 0.0),
            1.0,
        )
        .expect("should collide");
        assert!(approx(t, 1.0 / 6.0), "toi {t}");
    }

    #[test]
    fn circle_vs_circle_miss() {
        // Passing well above the target.
        assert!(moving_circle_vs_circle_toi(
            Vector2::new(-3.0, 5.0),
            Vector2::new(6.0, 0.0),
            0.5,
            Vector2::new(0.0, 0.0),
            0.5,
        )
        .is_none());
    }

    #[test]
    fn circle_vs_circle_already_overlapping_is_zero() {
        let t = moving_circle_vs_circle_toi(
            Vector2::new(0.5, 0.0),
            Vector2::new(1.0, 0.0),
            1.0,
            Vector2::new(0.0, 0.0),
            1.0,
        );
        assert_eq!(t, Some(0.0));
    }

    #[test]
    fn bullet_does_not_tunnel_through_thin_wall() {
        // Thin wall along the Y axis from (0,-5) to (0,5); a small fast bullet
        // starts left of it and ends right of it in one step. Discrete overlap
        // checks at t=0 and t=1 both miss — CCD must catch the crossing.
        let a = Vector2::new(0.0, -5.0);
        let b = Vector2::new(0.0, 5.0);
        let start = Vector2::new(-2.0, 0.0);
        let vel = Vector2::new(4.0, 0.0); // ends at (2,0)
        let r = 0.1;

        // Discrete checks would report no contact at either endpoint.
        assert!(point_to_segment_distance_sq(start, a, b) > r * r);
        assert!(point_to_segment_distance_sq(start + vel, a, b) > r * r);

        let toi = moving_circle_vs_segment_toi(start, vel, r, a, b).expect("must detect crossing");
        assert!(toi > 0.0 && toi < 1.0, "toi in (0,1): {toi}");
        // Center reaches x = -r (surface touches the wall at x=0) at t=0.475.
        assert!(approx(toi, 0.475), "toi {toi}");
    }

    #[test]
    fn bullet_above_wall_misses() {
        // Same motion but above the wall's top end.
        let a = Vector2::new(0.0, -5.0);
        let b = Vector2::new(0.0, 5.0);
        let toi = moving_circle_vs_segment_toi(
            Vector2::new(-2.0, 10.0),
            Vector2::new(4.0, 0.0),
            0.1,
            a,
            b,
        );
        assert!(toi.is_none(), "should miss above the wall: {toi:?}");
    }

    #[test]
    fn circle_hits_segment_endpoint_cap() {
        // Moving straight down onto the top endpoint (0,5) of the wall.
        let a = Vector2::new(0.0, -5.0);
        let b = Vector2::new(0.0, 5.0);
        let toi = moving_circle_vs_segment_toi(
            Vector2::new(0.0, 8.0),
            Vector2::new(0.0, -4.0), // ends at (0,4)
            1.0,
            a,
            b,
        )
        .expect("cap hit");
        // Contact when the center is 1 unit above (0,5) → at (0,6), t = 2/4 = 0.5.
        assert!(approx(toi, 0.5), "toi {toi}");
    }

    #[test]
    fn stationary_outside_has_no_impact() {
        assert!(moving_circle_vs_circle_toi(
            Vector2::new(5.0, 0.0),
            Vector2::new(0.0, 0.0),
            1.0,
            Vector2::new(0.0, 0.0),
            1.0,
        )
        .is_none());
    }

    #[test]
    fn slow_body_within_step_still_detected() {
        // A body that only just reaches the wall by t≈1 is still caught.
        let a = Vector2::new(0.0, -5.0);
        let b = Vector2::new(0.0, 5.0);
        let toi = moving_circle_vs_segment_toi(
            Vector2::new(-1.0, 0.0),
            Vector2::new(1.0, 0.0), // ends at (0,0), on the wall
            0.1,
            a,
            b,
        )
        .expect("should reach");
        assert!(toi > 0.0 && toi <= 1.0, "toi {toi}");
        assert!(approx(toi, 0.9), "center reaches x=-0.1 at t=0.9: {toi}");
    }
}
