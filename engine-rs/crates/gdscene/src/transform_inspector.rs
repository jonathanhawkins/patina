//! Inspector transform decomposition (pat-ghfv0.37).
//!
//! Godot's inspector shows `position` / `rotation` / `scale` (+ `skew` for 2D)
//! DECOMPOSED from a node's transform. When a `.tscn` stores a full
//! `Transform2D` / `Transform3D` matrix (e.g. `transform = Transform3D(…)`)
//! instead of separate position/rotation/scale, Patina's node holds a
//! `transform` property but no `position`, so the served inspector would show
//! the schema default and mismatch Godot. This module derives the editor
//! components from the stored matrix so they match, exactly as Godot's
//! `Node2D`/`Node3D` expose them from their transform.

use gdcore::math::{Transform2D, Vector2};
use gdvariant::Variant;

/// Returns the decomposed inspector component (`"position"` / `"rotation"` /
/// `"scale"` / `"skew"`) of a stored `Transform2D`/`Transform3D` value, or
/// `None` if `transform` is not a transform variant or `component` is not
/// derivable from it (e.g. `"skew"` on a 3D transform).
///
/// Conventions match Godot's node property derivations:
///   * 3D `rotation` is Euler angles in **radians** (YXZ), `scale` is the
///     per-axis basis length.
///   * 2D `rotation` is the X-basis angle, `scale` is
///     `(len(x), sign(det)·len(y))`, `skew` is the deviation of the Y axis from
///     perpendicular to X.
pub fn transform_component(transform: &Variant, component: &str) -> Option<Variant> {
    match transform {
        Variant::Transform3D(t) => match component {
            "position" => Some(Variant::Vector3(t.origin)),
            "rotation" => Some(Variant::Vector3(t.basis.to_euler())),
            "scale" => Some(Variant::Vector3(t.basis.get_scale())),
            _ => None,
        },
        Variant::Transform2D(t) => match component {
            "position" => Some(Variant::Vector2(t.origin)),
            "rotation" => Some(Variant::Float(t.x.angle() as f64)),
            "scale" => Some(Variant::Vector2(transform2d_scale(t))),
            "skew" => Some(Variant::Float(transform2d_skew(t) as f64)),
            _ => None,
        },
        _ => None,
    }
}

/// `true` if `name` is a transform-derived editor component.
pub fn is_transform_component(name: &str) -> bool {
    matches!(name, "position" | "rotation" | "scale" | "skew")
}

fn basis_determinant_2d(t: &Transform2D) -> f32 {
    t.x.x * t.y.y - t.x.y * t.y.x
}

fn transform2d_scale(t: &Transform2D) -> Vector2 {
    let sign = if basis_determinant_2d(t) < 0.0 {
        -1.0
    } else {
        1.0
    };
    Vector2::new(t.x.length(), sign * t.y.length())
}

fn transform2d_skew(t: &Transform2D) -> f32 {
    if t.x.length_squared() == 0.0 || t.y.length_squared() == 0.0 {
        return 0.0;
    }
    let sign = if basis_determinant_2d(t) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let xn = t.x.normalized();
    let yn = t.y.normalized();
    let dot = xn.dot(Vector2::new(sign * yn.x, sign * yn.y));
    dot.clamp(-1.0, 1.0).acos() - std::f32::consts::FRAC_PI_2
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;
    use gdcore::math3d::{Basis, Transform3D};
    use gdcore::Vector3;

    #[test]
    fn transform3d_translation_decomposes_to_position() {
        // Matrix form `Transform3D(1,0,0, 0,1,0, 0,0,1, -3,0,0)` (identity basis).
        let t = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(-3.0, 0.0, 0.0),
        };
        let v = Variant::Transform3D(t);
        assert!(matches!(
            transform_component(&v, "position"),
            Some(Variant::Vector3(p)) if p.x == -3.0 && p.y == 0.0 && p.z == 0.0
        ));
        // Identity basis -> rotation 0, scale 1.
        assert!(matches!(
            transform_component(&v, "rotation"),
            Some(Variant::Vector3(r)) if r.x.abs() < 1e-6 && r.y.abs() < 1e-6 && r.z.abs() < 1e-6
        ));
        assert!(matches!(
            transform_component(&v, "scale"),
            Some(Variant::Vector3(s)) if (s.x - 1.0).abs() < 1e-6 && (s.y - 1.0).abs() < 1e-6 && (s.z - 1.0).abs() < 1e-6
        ));
        // `skew` is 2D-only.
        assert!(transform_component(&v, "skew").is_none());
    }

    #[test]
    fn transform3d_scaled_basis_decomposes_scale() {
        let t = Transform3D {
            basis: Basis {
                x: Vector3::new(2.0, 0.0, 0.0),
                y: Vector3::new(0.0, 3.0, 0.0),
                z: Vector3::new(0.0, 0.0, 4.0),
            },
            origin: Vector3::new(1.0, 2.0, 3.0),
        };
        let v = Variant::Transform3D(t);
        assert!(matches!(
            transform_component(&v, "scale"),
            Some(Variant::Vector3(s)) if (s.x - 2.0).abs() < 1e-6 && (s.y - 3.0).abs() < 1e-6 && (s.z - 4.0).abs() < 1e-6
        ));
    }

    #[test]
    fn transform2d_translation_decomposes() {
        let t = Transform2D {
            x: Vector2::new(1.0, 0.0),
            y: Vector2::new(0.0, 1.0),
            origin: Vector2::new(64.0, 32.0),
        };
        let v = Variant::Transform2D(t);
        assert!(matches!(
            transform_component(&v, "position"),
            Some(Variant::Vector2(p)) if p.x == 64.0 && p.y == 32.0
        ));
        assert!(matches!(
            transform_component(&v, "rotation"),
            Some(Variant::Float(r)) if r.abs() < 1e-6
        ));
        assert!(matches!(
            transform_component(&v, "scale"),
            Some(Variant::Vector2(s)) if (s.x - 1.0).abs() < 1e-6 && (s.y - 1.0).abs() < 1e-6
        ));
        assert!(matches!(
            transform_component(&v, "skew"),
            Some(Variant::Float(sk)) if sk.abs() < 1e-6
        ));
    }

    #[test]
    fn non_transform_and_unknown_component_return_none() {
        assert!(transform_component(&Variant::Int(5), "position").is_none());
        let t = Variant::Transform3D(Transform3D::IDENTITY);
        assert!(transform_component(&t, "not_a_component").is_none());
    }
}
