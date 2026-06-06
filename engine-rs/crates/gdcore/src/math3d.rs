//! 3D math types: Basis, Transform3D, Quaternion, AABB, Plane.
//!
//! These mirror Godot's built-in 3D math types and follow the same
//! conventions (Y-up, right-handed coordinate system).

use crate::math::Vector3;
use std::ops::Mul;

// ---------------------------------------------------------------------------
// Quaternion
// ---------------------------------------------------------------------------

/// A unit quaternion for representing 3D rotations, matching Godot's `Quaternion`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternion {
    /// The X imaginary component.
    pub x: f32,
    /// The Y imaginary component.
    pub y: f32,
    /// The Z imaginary component.
    pub z: f32,
    /// The W real component.
    pub w: f32,
}

impl Quaternion {
    /// The identity quaternion (no rotation).
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Creates a new quaternion.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Creates a quaternion from Euler angles (YXZ convention, matching Godot).
    pub fn from_euler(euler: Vector3) -> Self {
        let (sy, cy) = (euler.y * 0.5).sin_cos();
        let (sx, cx) = (euler.x * 0.5).sin_cos();
        let (sz, cz) = (euler.z * 0.5).sin_cos();

        Self {
            x: cy * sx * cz + sy * cx * sz,
            y: sy * cx * cz - cy * sx * sz,
            z: -sy * sx * cz + cy * cx * sz,
            w: cy * cx * cz + sy * sx * sz,
        }
    }

    /// Converts this quaternion to Euler angles (YXZ convention).
    pub fn to_euler(self) -> Vector3 {
        let basis = Basis::from_quaternion(self);
        basis.to_euler()
    }

    /// Creates a quaternion from an axis and angle.
    pub fn from_axis_angle(axis: Vector3, angle: f32) -> Self {
        let half = angle * 0.5;
        let s = half.sin();
        let a = axis.normalized();
        Self {
            x: a.x * s,
            y: a.y * s,
            z: a.z * s,
            w: half.cos(),
        }
    }

    /// Returns the length (norm) of the quaternion.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    /// Normalizes this quaternion in place.
    pub fn normalize(&mut self) {
        let len = self.length();
        if len > 1e-10 {
            let inv = 1.0 / len;
            self.x *= inv;
            self.y *= inv;
            self.z *= inv;
            self.w *= inv;
        }
    }

    /// Returns a normalized copy.
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < 1e-10 {
            Self::IDENTITY
        } else {
            let inv = 1.0 / len;
            Self::new(self.x * inv, self.y * inv, self.z * inv, self.w * inv)
        }
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Spherical linear interpolation.
    pub fn slerp(self, to: Self, t: f32) -> Self {
        let mut dot = self.dot(to);
        let mut to = to;

        // If dot is negative, negate one to take the short path.
        if dot < 0.0 {
            to = Self::new(-to.x, -to.y, -to.z, -to.w);
            dot = -dot;
        }

        if dot > 0.9995 {
            // Very close — use linear interpolation.
            let result = Self::new(
                self.x + (to.x - self.x) * t,
                self.y + (to.y - self.y) * t,
                self.z + (to.z - self.z) * t,
                self.w + (to.w - self.w) * t,
            );
            return result.normalized();
        }

        let theta = dot.acos();
        let sin_theta = theta.sin();
        let a = ((1.0 - t) * theta).sin() / sin_theta;
        let b = (t * theta).sin() / sin_theta;

        Self::new(
            self.x * a + to.x * b,
            self.y * a + to.y * b,
            self.z * a + to.z * b,
            self.w * a + to.w * b,
        )
    }

    /// Returns the inverse (conjugate for unit quaternions).
    pub fn inverse(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Rotates a vector by this quaternion.
    pub fn xform(self, v: Vector3) -> Vector3 {
        let u = Vector3::new(self.x, self.y, self.z);
        let s = self.w;
        // q * v * q^-1 = v + 2s(u × v) + 2(u × (u × v))
        let uv = u.cross(v);
        let uuv = u.cross(uv);
        Vector3::new(
            v.x + (uv.x * s + uuv.x) * 2.0,
            v.y + (uv.y * s + uuv.y) * 2.0,
            v.z + (uv.z * s + uuv.z) * 2.0,
        )
    }
}

impl Mul for Quaternion {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        )
    }
}

// ---------------------------------------------------------------------------
// Basis
// ---------------------------------------------------------------------------

/// A 3×3 matrix stored as three column vectors, matching Godot's `Basis`.
///
/// Used to represent rotations and scales in 3D space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Basis {
    /// The X column (right).
    pub x: Vector3,
    /// The Y column (up).
    pub y: Vector3,
    /// The Z column (forward).
    pub z: Vector3,
}

impl Basis {
    /// The identity basis.
    pub const IDENTITY: Self = Self {
        x: Vector3::new(1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 1.0, 0.0),
        z: Vector3::new(0.0, 0.0, 1.0),
    };

    /// Returns the transpose of this basis.
    pub fn transposed(self) -> Self {
        Self {
            x: Vector3::new(self.x.x, self.y.x, self.z.x),
            y: Vector3::new(self.x.y, self.y.y, self.z.y),
            z: Vector3::new(self.x.z, self.y.z, self.z.z),
        }
    }

    /// Returns the determinant of this basis.
    pub fn determinant(self) -> f32 {
        self.x.x * (self.y.y * self.z.z - self.y.z * self.z.y)
            - self.x.y * (self.y.x * self.z.z - self.y.z * self.z.x)
            + self.x.z * (self.y.x * self.z.y - self.y.y * self.z.x)
    }

    /// Returns the inverse of this basis.
    ///
    /// Assumes the basis is invertible (non-zero determinant).
    pub fn inverse(self) -> Self {
        let det = self.determinant();
        let inv_det = 1.0 / det;
        Self {
            x: Vector3::new(
                (self.y.y * self.z.z - self.y.z * self.z.y) * inv_det,
                (self.x.z * self.z.y - self.x.y * self.z.z) * inv_det,
                (self.x.y * self.y.z - self.x.z * self.y.y) * inv_det,
            ),
            y: Vector3::new(
                (self.y.z * self.z.x - self.y.x * self.z.z) * inv_det,
                (self.x.x * self.z.z - self.x.z * self.z.x) * inv_det,
                (self.x.z * self.y.x - self.x.x * self.y.z) * inv_det,
            ),
            z: Vector3::new(
                (self.y.x * self.z.y - self.y.y * self.z.x) * inv_det,
                (self.x.y * self.z.x - self.x.x * self.z.y) * inv_det,
                (self.x.x * self.y.y - self.x.y * self.y.x) * inv_det,
            ),
        }
    }

    /// Creates a basis from Euler angles (YXZ convention, matching Godot).
    pub fn from_euler(euler: Vector3) -> Self {
        let (sy, cy) = euler.y.sin_cos();
        let (sx, cx) = euler.x.sin_cos();
        let (sz, cz) = euler.z.sin_cos();

        Self {
            x: Vector3::new(cy * cz + sy * sx * sz, cx * sz, -sy * cz + cy * sx * sz),
            y: Vector3::new(cy * -sz + sy * sx * cz, cx * cz, sy * sz + cy * sx * cz),
            z: Vector3::new(sy * cx, -sx, cy * cx),
        }
    }

    /// Converts this basis to Euler angles (YXZ convention).
    pub fn to_euler(self) -> Vector3 {
        // YXZ extraction from the rotation matrix.
        let sy = self.z.x;
        let cx = (1.0 - self.z.y * self.z.y).sqrt();

        if cx > 1e-6 {
            Vector3::new(
                (-self.z.y).asin(),
                sy.atan2(self.z.z),
                self.x.y.atan2(self.y.y),
            )
        } else {
            // Gimbal lock.
            Vector3::new(
                if self.z.y < 0.0 {
                    std::f32::consts::FRAC_PI_2
                } else {
                    -std::f32::consts::FRAC_PI_2
                },
                self.x.z.atan2(self.x.x),
                0.0,
            )
        }
    }

    /// Creates a basis from a quaternion.
    pub fn from_quaternion(q: Quaternion) -> Self {
        let x2 = q.x + q.x;
        let y2 = q.y + q.y;
        let z2 = q.z + q.z;
        let xx = q.x * x2;
        let xy = q.x * y2;
        let xz = q.x * z2;
        let yy = q.y * y2;
        let yz = q.y * z2;
        let zz = q.z * z2;
        let wx = q.w * x2;
        let wy = q.w * y2;
        let wz = q.w * z2;

        Self {
            x: Vector3::new(1.0 - (yy + zz), xy + wz, xz - wy),
            y: Vector3::new(xy - wz, 1.0 - (xx + zz), yz + wx),
            z: Vector3::new(xz + wy, yz - wx, 1.0 - (xx + yy)),
        }
    }

    /// Returns the scale of each basis axis.
    pub fn get_scale(self) -> Vector3 {
        Vector3::new(self.x.length(), self.y.length(), self.z.length())
    }

    /// Returns a basis rotated around `axis` by `angle` radians.
    pub fn rotated(self, axis: Vector3, angle: f32) -> Self {
        let rot = Basis::from_quaternion(Quaternion::from_axis_angle(axis, angle));
        rot * self
    }

    /// Transforms a vector by this basis (matrix-vector multiply).
    pub fn xform(self, v: Vector3) -> Vector3 {
        Vector3::new(
            self.x.x * v.x + self.y.x * v.y + self.z.x * v.z,
            self.x.y * v.x + self.y.y * v.y + self.z.y * v.z,
            self.x.z * v.x + self.y.z * v.y + self.z.z * v.z,
        )
    }

    /// Inverse-transforms a vector (multiply by transpose).
    pub fn xform_inv(self, v: Vector3) -> Vector3 {
        Vector3::new(self.x.dot(v), self.y.dot(v), self.z.dot(v))
    }

    /// Returns `true` if this basis is orthonormal within the given tolerance.
    ///
    /// An orthonormal basis has unit-length columns that are mutually
    /// perpendicular and a determinant of +1.
    pub fn is_orthonormal(self, epsilon: f32) -> bool {
        let x_unit = (self.x.length() - 1.0).abs() < epsilon;
        let y_unit = (self.y.length() - 1.0).abs() < epsilon;
        let z_unit = (self.z.length() - 1.0).abs() < epsilon;
        let xy_perp = self.x.dot(self.y).abs() < epsilon;
        let xz_perp = self.x.dot(self.z).abs() < epsilon;
        let yz_perp = self.y.dot(self.z).abs() < epsilon;
        let det_one = (self.determinant() - 1.0).abs() < epsilon;
        x_unit && y_unit && z_unit && xy_perp && xz_perp && yz_perp && det_one
    }

    /// Returns an orthonormalized copy of this basis using Gram-Schmidt.
    pub fn orthonormalized(self) -> Self {
        let x = self.x.normalized();
        let y = (self.y - x * x.dot(self.y)).normalized();
        let z = (self.z - x * x.dot(self.z) - y * y.dot(self.z)).normalized();
        Self { x, y, z }
    }
}

impl Mul for Basis {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            x: self.xform(rhs.x),
            y: self.xform(rhs.y),
            z: self.xform(rhs.z),
        }
    }
}

// ---------------------------------------------------------------------------
// Transform3D
// ---------------------------------------------------------------------------

/// A 3D affine transform (basis + origin), matching Godot's `Transform3D`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3D {
    /// The 3×3 rotation/scale basis.
    pub basis: Basis,
    /// The translation origin.
    pub origin: Vector3,
}

impl Transform3D {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        basis: Basis::IDENTITY,
        origin: Vector3::ZERO,
    };

    /// Transforms a point by this transform.
    pub fn xform(self, v: Vector3) -> Vector3 {
        self.basis.xform(v) + self.origin
    }

    /// Returns the inverse of this transform.
    pub fn inverse(self) -> Self {
        let inv_basis = self.basis.inverse();
        Self {
            basis: inv_basis,
            origin: inv_basis.xform(-self.origin),
        }
    }

    /// Creates a transform that looks at `target` from `self.origin` with the given `up` vector.
    pub fn looking_at(self, target: Vector3, up: Vector3) -> Self {
        let forward = (target - self.origin).normalized();
        let right = up.cross(forward).normalized();
        let new_up = forward.cross(right);

        Self {
            basis: Basis {
                x: right,
                y: new_up,
                z: forward,
            },
            origin: self.origin,
        }
    }

    /// Returns a copy translated by `offset`.
    pub fn translated(self, offset: Vector3) -> Self {
        Self {
            basis: self.basis,
            origin: self.origin + offset,
        }
    }

    /// Returns a copy rotated around `axis` by `angle`.
    pub fn rotated(self, axis: Vector3, angle: f32) -> Self {
        let rot = Basis::from_quaternion(Quaternion::from_axis_angle(axis, angle));
        Self {
            basis: rot * self.basis,
            origin: rot.xform(self.origin),
        }
    }

    /// Returns a copy scaled by `scale`.
    pub fn scaled(self, scale: Vector3) -> Self {
        Self {
            basis: Basis {
                x: self.basis.x * scale.x,
                y: self.basis.y * scale.y,
                z: self.basis.z * scale.z,
            },
            origin: Vector3::new(
                self.origin.x * scale.x,
                self.origin.y * scale.y,
                self.origin.z * scale.z,
            ),
        }
    }
}

impl Mul for Transform3D {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            basis: self.basis * rhs.basis,
            origin: self.xform(rhs.origin),
        }
    }
}

// ---------------------------------------------------------------------------
// AABB
// ---------------------------------------------------------------------------

/// An axis-aligned bounding box in 3D, matching Godot's `AABB`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// The minimum corner position.
    pub position: Vector3,
    /// The size (extents) of the box.
    pub size: Vector3,
}

impl Aabb {
    /// Creates a new AABB.
    pub const fn new(position: Vector3, size: Vector3) -> Self {
        Self { position, size }
    }

    /// Returns `true` if the point is inside this AABB.
    pub fn contains_point(self, point: Vector3) -> bool {
        let end = self.position + self.size;
        point.x >= self.position.x
            && point.y >= self.position.y
            && point.z >= self.position.z
            && point.x < end.x
            && point.y < end.y
            && point.z < end.z
    }

    /// Returns `true` if this AABB overlaps `other`.
    pub fn intersects(self, other: Self) -> bool {
        let a_end = self.position + self.size;
        let b_end = other.position + other.size;
        self.position.x < b_end.x
            && a_end.x > other.position.x
            && self.position.y < b_end.y
            && a_end.y > other.position.y
            && self.position.z < b_end.z
            && a_end.z > other.position.z
    }

    /// Returns an AABB that encloses both `self` and `other`.
    pub fn merge(self, other: Self) -> Self {
        let min = Vector3::new(
            self.position.x.min(other.position.x),
            self.position.y.min(other.position.y),
            self.position.z.min(other.position.z),
        );
        let a_end = self.position + self.size;
        let b_end = other.position + other.size;
        let max = Vector3::new(
            a_end.x.max(b_end.x),
            a_end.y.max(b_end.y),
            a_end.z.max(b_end.z),
        );
        Self {
            position: min,
            size: max - min,
        }
    }

    /// Returns the center point of this AABB.
    pub fn get_center(self) -> Vector3 {
        self.position + self.size * 0.5
    }

    /// Returns the volume.
    pub fn get_volume(self) -> f32 {
        self.size.x * self.size.y * self.size.z
    }

    /// Returns an AABB expanded to include `point`.
    pub fn expand(self, point: Vector3) -> Self {
        let end = self.position + self.size;
        let new_pos = Vector3::new(
            self.position.x.min(point.x),
            self.position.y.min(point.y),
            self.position.z.min(point.z),
        );
        let new_end = Vector3::new(end.x.max(point.x), end.y.max(point.y), end.z.max(point.z));
        Self {
            position: new_pos,
            size: new_end - new_pos,
        }
    }

    /// Returns `true` if all size components are positive.
    pub fn has_volume(self) -> bool {
        self.size.x > 0.0 && self.size.y > 0.0 && self.size.z > 0.0
    }

    /// Returns one of the 8 corner points (index 0..7).
    pub fn get_endpoint(self, idx: u8) -> Vector3 {
        let end = self.position + self.size;
        Vector3::new(
            if idx & 1 == 0 { self.position.x } else { end.x },
            if idx & 2 == 0 { self.position.y } else { end.y },
            if idx & 4 == 0 { self.position.z } else { end.z },
        )
    }
}

// ---------------------------------------------------------------------------
// Plane
// ---------------------------------------------------------------------------

/// An infinite plane in 3D space, matching Godot's `Plane`.
///
/// Defined by a normal vector and distance `d` from the origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// The unit normal vector of the plane.
    pub normal: Vector3,
    /// The signed distance from the origin along the normal.
    pub d: f32,
}

impl Plane {
    /// Creates a new plane.
    pub const fn new(normal: Vector3, d: f32) -> Self {
        Self { normal, d }
    }

    /// Creates a plane from three points.
    pub fn from_points(a: Vector3, b: Vector3, c: Vector3) -> Self {
        let normal = (b - a).cross(c - a).normalized();
        let d = normal.dot(a);
        Self { normal, d }
    }

    /// Returns the signed distance from the plane to `point`.
    pub fn distance_to(self, point: Vector3) -> f32 {
        self.normal.dot(point) - self.d
    }

    /// Returns `true` if `point` is on the positive side of the plane.
    pub fn is_point_over(self, point: Vector3) -> bool {
        self.distance_to(point) > 0.0
    }

    /// Returns the intersection point of a ray with this plane, if any.
    pub fn intersects_ray(self, from: Vector3, dir: Vector3) -> Option<Vector3> {
        let denom = self.normal.dot(dir);
        if denom.abs() < 1e-10 {
            return None; // Parallel to plane.
        }
        let t = (self.d - self.normal.dot(from)) / denom;
        if t < 0.0 {
            return None; // Behind ray origin.
        }
        Some(from + dir * t)
    }

    /// Returns the intersection point of a line segment with this plane, if any.
    pub fn intersects_segment(self, a: Vector3, b: Vector3) -> Option<Vector3> {
        let dir = b - a;
        let denom = self.normal.dot(dir);
        if denom.abs() < 1e-10 {
            return None;
        }
        let t = (self.d - self.normal.dot(a)) / denom;
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        Some(a + dir * t)
    }

    /// Returns a copy with a normalized normal vector.
    pub fn normalized(self) -> Self {
        let len = self.normal.length();
        if len < 1e-10 {
            return self;
        }
        let inv = 1.0 / len;
        Self {
            normal: self.normal * inv,
            d: self.d * inv,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn v3_approx_eq(a: Vector3, b: Vector3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    // -- Quaternion ---------------------------------------------------------

    #[test]
    fn quaternion_identity_length() {
        assert!(approx_eq(Quaternion::IDENTITY.length(), 1.0));
    }

    #[test]
    fn quaternion_normalize() {
        let q = Quaternion::new(1.0, 2.0, 3.0, 4.0).normalized();
        assert!(approx_eq(q.length(), 1.0));
    }

    #[test]
    fn quaternion_inverse() {
        let q = Quaternion::from_axis_angle(Vector3::UP, FRAC_PI_4);
        let inv = q.inverse();
        let result = q * inv;
        assert!(approx_eq(result.x, 0.0));
        assert!(approx_eq(result.y, 0.0));
        assert!(approx_eq(result.z, 0.0));
        assert!(approx_eq(result.w, 1.0));
    }

    #[test]
    fn quaternion_from_axis_angle_90_y() {
        let q = Quaternion::from_axis_angle(Vector3::UP, FRAC_PI_2);
        let v = q.xform(Vector3::new(1.0, 0.0, 0.0));
        assert!(v3_approx_eq(v, Vector3::new(0.0, 0.0, -1.0)));
    }

    #[test]
    fn quaternion_slerp_endpoints() {
        let a = Quaternion::IDENTITY;
        let b = Quaternion::from_axis_angle(Vector3::UP, PI);
        let at_zero = a.slerp(b, 0.0);
        assert!(approx_eq(at_zero.dot(a).abs(), 1.0));
        let at_one = a.slerp(b, 1.0);
        assert!(approx_eq(at_one.dot(b).abs(), 1.0));
    }

    #[test]
    fn quaternion_dot_self() {
        let q = Quaternion::IDENTITY;
        assert!(approx_eq(q.dot(q), 1.0));
    }

    #[test]
    fn quaternion_mul_identity() {
        let q = Quaternion::from_axis_angle(Vector3::UP, 0.5);
        let r = q * Quaternion::IDENTITY;
        assert!(approx_eq(r.x, q.x));
        assert!(approx_eq(r.y, q.y));
        assert!(approx_eq(r.z, q.z));
        assert!(approx_eq(r.w, q.w));
    }

    #[test]
    fn quaternion_xform_preserves_length() {
        let q = Quaternion::from_axis_angle(Vector3::new(1.0, 1.0, 0.0).normalized(), 1.0);
        let v = Vector3::new(3.0, 4.0, 5.0);
        let rotated = q.xform(v);
        assert!(approx_eq(v.length(), rotated.length()));
    }

    // -- Basis --------------------------------------------------------------

    #[test]
    fn basis_identity_xform() {
        let v = Vector3::new(3.0, 7.0, -2.0);
        assert_eq!(Basis::IDENTITY.xform(v), v);
    }

    #[test]
    fn basis_identity_determinant() {
        assert!(approx_eq(Basis::IDENTITY.determinant(), 1.0));
    }

    #[test]
    fn basis_transpose_identity() {
        assert_eq!(Basis::IDENTITY.transposed(), Basis::IDENTITY);
    }

    #[test]
    fn basis_inverse_identity() {
        let inv = Basis::IDENTITY.inverse();
        assert!(v3_approx_eq(inv.x, Basis::IDENTITY.x));
        assert!(v3_approx_eq(inv.y, Basis::IDENTITY.y));
        assert!(v3_approx_eq(inv.z, Basis::IDENTITY.z));
    }

    #[test]
    fn basis_mul_identity() {
        let b = Basis::from_euler(Vector3::new(0.3, 0.5, 0.7));
        let r = b * Basis::IDENTITY;
        assert!(v3_approx_eq(r.x, b.x));
        assert!(v3_approx_eq(r.y, b.y));
        assert!(v3_approx_eq(r.z, b.z));
    }

    #[test]
    fn basis_from_quaternion_identity() {
        let b = Basis::from_quaternion(Quaternion::IDENTITY);
        assert!(v3_approx_eq(b.x, Basis::IDENTITY.x));
        assert!(v3_approx_eq(b.y, Basis::IDENTITY.y));
        assert!(v3_approx_eq(b.z, Basis::IDENTITY.z));
    }

    #[test]
    fn basis_get_scale_identity() {
        let s = Basis::IDENTITY.get_scale();
        assert!(v3_approx_eq(s, Vector3::ONE));
    }

    #[test]
    fn basis_xform_inv_roundtrip() {
        let b = Basis::from_euler(Vector3::new(0.5, 1.0, 0.2));
        let v = Vector3::new(1.0, 2.0, 3.0);
        let transformed = b.xform(v);
        let back = b.xform_inv(transformed);
        // For orthogonal basis, xform_inv is the inverse operation.
        assert!(v3_approx_eq(back, v));
    }

    #[test]
    fn basis_rotated() {
        let b = Basis::IDENTITY.rotated(Vector3::UP, FRAC_PI_2);
        let v = b.xform(Vector3::new(1.0, 0.0, 0.0));
        assert!(v3_approx_eq(v, Vector3::new(0.0, 0.0, -1.0)));
    }

    // -- Transform3D --------------------------------------------------------

    #[test]
    fn transform3d_identity_xform() {
        let v = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(Transform3D::IDENTITY.xform(v), v);
    }

    #[test]
    fn transform3d_translated() {
        let t = Transform3D::IDENTITY.translated(Vector3::new(10.0, 0.0, 0.0));
        let p = t.xform(Vector3::ZERO);
        assert!(v3_approx_eq(p, Vector3::new(10.0, 0.0, 0.0)));
    }

    #[test]
    fn transform3d_mul_identity() {
        let t = Transform3D::IDENTITY;
        let r = t * t;
        assert!(v3_approx_eq(r.origin, Vector3::ZERO));
    }

    #[test]
    fn transform3d_inverse_roundtrip() {
        let t = Transform3D {
            basis: Basis::from_euler(Vector3::new(0.3, 0.5, 0.0)),
            origin: Vector3::new(10.0, 20.0, 30.0),
        };
        let inv = t.inverse();
        let v = Vector3::new(1.0, 2.0, 3.0);
        let roundtrip = inv.xform(t.xform(v));
        assert!(v3_approx_eq(roundtrip, v));
    }

    #[test]
    fn transform3d_scaled() {
        let t = Transform3D::IDENTITY.scaled(Vector3::new(2.0, 3.0, 4.0));
        let p = t.xform(Vector3::new(1.0, 1.0, 1.0));
        assert!(v3_approx_eq(p, Vector3::new(2.0, 3.0, 4.0)));
    }

    #[test]
    fn transform3d_rotated() {
        let t = Transform3D::IDENTITY.rotated(Vector3::UP, FRAC_PI_2);
        let p = t.xform(Vector3::new(1.0, 0.0, 0.0));
        assert!(v3_approx_eq(p, Vector3::new(0.0, 0.0, -1.0)));
    }

    // -- AABB ---------------------------------------------------------------

    #[test]
    fn aabb_contains_point() {
        let a = Aabb::new(Vector3::ZERO, Vector3::ONE);
        assert!(a.contains_point(Vector3::new(0.5, 0.5, 0.5)));
        assert!(!a.contains_point(Vector3::new(1.0, 0.5, 0.5)));
        assert!(!a.contains_point(Vector3::new(-0.1, 0.5, 0.5)));
    }

    #[test]
    fn aabb_intersects() {
        let a = Aabb::new(Vector3::ZERO, Vector3::ONE);
        let b = Aabb::new(Vector3::new(0.5, 0.5, 0.5), Vector3::ONE);
        let c = Aabb::new(Vector3::new(2.0, 2.0, 2.0), Vector3::ONE);
        assert!(a.intersects(b));
        assert!(!a.intersects(c));
    }

    #[test]
    fn aabb_merge() {
        let a = Aabb::new(Vector3::ZERO, Vector3::ONE);
        let b = Aabb::new(Vector3::new(2.0, 0.0, 0.0), Vector3::ONE);
        let merged = a.merge(b);
        assert!(v3_approx_eq(merged.position, Vector3::ZERO));
        assert!(v3_approx_eq(merged.size, Vector3::new(3.0, 1.0, 1.0)));
    }

    #[test]
    fn aabb_center_and_volume() {
        let a = Aabb::new(Vector3::ZERO, Vector3::new(2.0, 4.0, 6.0));
        assert!(v3_approx_eq(a.get_center(), Vector3::new(1.0, 2.0, 3.0)));
        assert!(approx_eq(a.get_volume(), 48.0));
    }

    #[test]
    fn aabb_has_volume() {
        assert!(Aabb::new(Vector3::ZERO, Vector3::ONE).has_volume());
        assert!(!Aabb::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 1.0)).has_volume());
    }

    #[test]
    fn aabb_expand() {
        let a = Aabb::new(Vector3::ZERO, Vector3::ONE);
        let expanded = a.expand(Vector3::new(3.0, 0.5, 0.5));
        assert!(v3_approx_eq(expanded.position, Vector3::ZERO));
        assert!(v3_approx_eq(expanded.size, Vector3::new(3.0, 1.0, 1.0)));
    }

    #[test]
    fn aabb_get_endpoint() {
        let a = Aabb::new(Vector3::ZERO, Vector3::ONE);
        assert!(v3_approx_eq(a.get_endpoint(0), Vector3::ZERO));
        assert!(v3_approx_eq(a.get_endpoint(7), Vector3::ONE));
    }

    // -- Plane --------------------------------------------------------------

    #[test]
    fn plane_from_points() {
        let p = Plane::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        // Normal should point in -Y direction (right-hand rule: X cross Z = -Y).
        // Actually (B-A) cross (C-A) = (1,0,0) cross (0,0,1) = (0,-1,0)
        assert!(v3_approx_eq(p.normal, Vector3::new(0.0, -1.0, 0.0)));
        assert!(approx_eq(p.d, 0.0));
    }

    #[test]
    fn plane_distance_to() {
        let p = Plane::new(Vector3::UP, 5.0);
        assert!(approx_eq(p.distance_to(Vector3::new(0.0, 10.0, 0.0)), 5.0));
        assert!(approx_eq(p.distance_to(Vector3::new(0.0, 5.0, 0.0)), 0.0));
    }

    #[test]
    fn plane_is_point_over() {
        let p = Plane::new(Vector3::UP, 0.0);
        assert!(p.is_point_over(Vector3::new(0.0, 1.0, 0.0)));
        assert!(!p.is_point_over(Vector3::new(0.0, -1.0, 0.0)));
    }

    #[test]
    fn plane_intersects_ray() {
        let p = Plane::new(Vector3::UP, 0.0);
        let hit = p.intersects_ray(Vector3::new(0.0, 5.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
        assert!(hit.is_some());
        assert!(v3_approx_eq(hit.unwrap(), Vector3::ZERO));
    }

    #[test]
    fn plane_intersects_ray_miss() {
        let p = Plane::new(Vector3::UP, 0.0);
        // Ray pointing away from plane.
        let hit = p.intersects_ray(Vector3::new(0.0, 5.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
        assert!(hit.is_none());
    }

    #[test]
    fn plane_intersects_segment() {
        let p = Plane::new(Vector3::UP, 0.0);
        let hit = p.intersects_segment(Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
        assert!(hit.is_some());
        assert!(v3_approx_eq(hit.unwrap(), Vector3::ZERO));
    }

    #[test]
    fn plane_intersects_segment_miss() {
        let p = Plane::new(Vector3::UP, 0.0);
        // Segment entirely above plane.
        let hit = p.intersects_segment(Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 2.0, 0.0));
        assert!(hit.is_none());
    }

    #[test]
    fn plane_normalized() {
        let p = Plane::new(Vector3::new(0.0, 2.0, 0.0), 4.0).normalized();
        assert!(v3_approx_eq(p.normal, Vector3::UP));
        assert!(approx_eq(p.d, 2.0));
    }
}
