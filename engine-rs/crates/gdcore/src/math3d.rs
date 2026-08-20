//! 3D math types: Basis, Transform3D, Quaternion, AABB, Plane.
//!
//! These mirror Godot's built-in 3D math types and follow the same
//! conventions (Y-up, right-handed coordinate system).

use crate::math::{Vector2, Vector3, Vector4};
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

    /// Returns the pure-rotation part of this basis: orthonormalized, with a
    /// mirrored basis (negative determinant) flipped back to right-handed.
    ///
    /// Godot: `Basis::get_rotation_basis` (`core/math/basis.cpp`).
    pub fn get_rotation_basis(self) -> Self {
        let m = self.orthonormalized();
        if m.determinant() < 0.0 {
            Self {
                x: -m.x,
                y: -m.y,
                z: -m.z,
            }
        } else {
            m
        }
    }

    /// Converts an orthonormal basis to the equivalent unit quaternion.
    ///
    /// This is the inverse of [`Basis::from_quaternion`]. The classic
    /// trace-based extraction, with the three fallback branches that avoid the
    /// near-zero divisor when the trace is negative.
    ///
    /// Godot: `Basis::get_quaternion` (`core/math/basis.cpp`).
    pub fn to_quaternion(self) -> Quaternion {
        // `m[row][col]`, with our columns stored as `x`/`y`/`z`.
        let (m00, m11, m22) = (self.x.x, self.y.y, self.z.z);
        let trace = m00 + m11 + m22;
        if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Quaternion::new(
                (self.y.z - self.z.y) / s,
                (self.z.x - self.x.z) / s,
                (self.x.y - self.y.x) / s,
                0.25 * s,
            )
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Quaternion::new(
                0.25 * s,
                (self.y.x + self.x.y) / s,
                (self.z.x + self.x.z) / s,
                (self.y.z - self.z.y) / s,
            )
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Quaternion::new(
                (self.y.x + self.x.y) / s,
                0.25 * s,
                (self.z.y + self.y.z) / s,
                (self.z.x - self.x.z) / s,
            )
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Quaternion::new(
                (self.z.x + self.x.z) / s,
                (self.z.y + self.y.z) / s,
                0.25 * s,
                (self.x.y - self.y.x) / s,
            )
        }
    }

    /// Returns the rotation of this basis as a quaternion, discarding scale.
    ///
    /// Godot: `Basis::get_rotation_quaternion`.
    pub fn get_rotation_quaternion(self) -> Quaternion {
        self.get_rotation_basis().to_quaternion()
    }

    /// Builds a basis from a rotation quaternion and a per-axis scale
    /// (`R * diag(scale)`).
    ///
    /// Godot: `Basis::set_quaternion_scale`.
    pub fn from_quaternion_scale(q: Quaternion, scale: Vector3) -> Self {
        let r = Self::from_quaternion(q);
        Self {
            x: r.x * scale.x,
            y: r.y * scale.y,
            z: r.z * scale.z,
        }
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

impl Transform3D {
    /// Interpolates towards `to` by `weight`, decomposing each transform into
    /// rotation / scale / origin: the rotation slerps, the scale and origin
    /// lerp. A component-wise matrix lerp would shear the basis instead.
    ///
    /// Godot: `Transform3D::interpolate_with` (`core/math/transform_3d.cpp`).
    pub fn interpolate_with(self, to: Transform3D, weight: f32) -> Self {
        let src_rot = self.basis.get_rotation_quaternion();
        let dst_rot = to.basis.get_rotation_quaternion();
        Self {
            basis: Basis::from_quaternion_scale(
                src_rot.slerp(dst_rot, weight).normalized(),
                self.basis.get_scale().lerp(to.basis.get_scale(), weight),
            ),
            origin: self.origin.lerp(to.origin, weight),
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

    /// Returns the end corner (`position + size`).
    ///
    /// Godot: `AABB.end` / `get_end`.
    pub fn get_end(self) -> Vector3 {
        self.position + self.size
    }

    /// Returns a copy grown by `amount` on all six sides.
    ///
    /// Godot: `AABB.grow` (`core/math/aabb.h`).
    pub fn grow(self, amount: f32) -> Self {
        Self {
            position: Vector3::new(
                self.position.x - amount,
                self.position.y - amount,
                self.position.z - amount,
            ),
            size: Vector3::new(
                self.size.x + amount * 2.0,
                self.size.y + amount * 2.0,
                self.size.z + amount * 2.0,
            ),
        }
    }

    /// Returns an equivalent AABB with a non-negative size.
    ///
    /// Godot: `AABB.abs`.
    pub fn abs(self) -> Self {
        Self {
            position: Vector3::new(
                self.position.x + self.size.x.min(0.0),
                self.position.y + self.size.y.min(0.0),
                self.position.z + self.size.z.min(0.0),
            ),
            size: Vector3::new(self.size.x.abs(), self.size.y.abs(), self.size.z.abs()),
        }
    }

    /// Returns the overlapping box of two AABBs, or a zero-size box at the
    /// origin when they do not overlap.
    ///
    /// Godot: `AABB.intersection`.
    pub fn intersection(self, other: Self) -> Self {
        if !self.intersects(other) {
            return Self::new(Vector3::ZERO, Vector3::ZERO);
        }
        let a_end = self.get_end();
        let b_end = other.get_end();
        let position = Vector3::new(
            self.position.x.max(other.position.x),
            self.position.y.max(other.position.y),
            self.position.z.max(other.position.z),
        );
        let end = Vector3::new(
            a_end.x.min(b_end.x),
            a_end.y.min(b_end.y),
            a_end.z.min(b_end.z),
        );
        Self {
            position,
            size: end - position,
        }
    }

    /// Returns `true` if `other` lies entirely within this AABB.
    ///
    /// Godot: `AABB.encloses`.
    pub fn encloses(self, other: Self) -> bool {
        let a_end = self.get_end();
        let b_end = other.get_end();
        other.position.x >= self.position.x
            && other.position.y >= self.position.y
            && other.position.z >= self.position.z
            && b_end.x <= a_end.x
            && b_end.y <= a_end.y
            && b_end.z <= a_end.z
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

    /// Returns `point` projected onto this plane (the closest point on it).
    ///
    /// Godot: `Plane.project`.
    pub fn project(self, point: Vector3) -> Vector3 {
        point - self.normal * self.distance_to(point)
    }

    /// Returns the point on the plane closest to the origin
    /// (`normal * d`).
    ///
    /// Godot: `Plane.get_center`.
    pub fn get_center(self) -> Vector3 {
        self.normal * self.d
    }

    /// Returns `true` if `point` lies on the plane within `tolerance`.
    ///
    /// Godot: `Plane.has_point`.
    pub fn has_point(self, point: Vector3, tolerance: f32) -> bool {
        self.distance_to(point).abs() <= tolerance
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

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// A 4×4 projection matrix, matching Godot's `Projection`
/// (`core/math/projection.h`).
///
/// One of the Variant builtins Patina had no core type for (pat-s3rrv.59):
/// `Camera3D.get_camera_projection()`, every XR view matrix and any `.tscn`
/// property holding a `Projection(...)` literal are this type.
///
/// **Storage is column-major (`m[column][row]`)**, Godot's and OpenGL's
/// convention, and deliberately identical to the ad-hoc `Projection` in
/// `gdscene::camera3d` so consolidating the two is a move rather than a
/// rewrite. Getting this backwards transposes every matrix silently, so the
/// convention is stated at every constructor below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// Column-major data: `m[column][row]`.
    pub m: [[f32; 4]; 4],
}

impl Projection {
    /// The identity projection.
    pub const IDENTITY: Self = Self {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// The all-zero projection — Godot's `Projection.ZERO`.
    pub const ZERO: Self = Self { m: [[0.0; 4]; 4] };

    /// Builds a projection from its four columns, each a [`Vector4`].
    ///
    /// This is Godot's `Projection(Vector4, Vector4, Vector4, Vector4)`
    /// constructor, and the order a `.tscn` literal writes.
    pub const fn from_columns(x: Vector4, y: Vector4, z: Vector4, w: Vector4) -> Self {
        Self {
            m: [
                [x.x, x.y, x.z, x.w],
                [y.x, y.y, y.z, y.w],
                [z.x, z.y, z.z, z.w],
                [w.x, w.y, w.z, w.w],
            ],
        }
    }

    /// The `index`-th column as a [`Vector4`], the shape Godot's `[]` operator
    /// yields. Panics outside `0..4`, like an out-of-range index anywhere else.
    pub fn column(self, index: usize) -> Vector4 {
        let c = self.m[index];
        Vector4::new(c[0], c[1], c[2], c[3])
    }

    /// The 16 components in column-major order — the layout a GPU uniform
    /// upload wants, and what `PackedFloat32Array` round-trips.
    pub fn to_array(self) -> [f32; 16] {
        let mut out = [0.0; 16];
        for (col, chunk) in self.m.iter().enumerate() {
            out[col * 4..col * 4 + 4].copy_from_slice(chunk);
        }
        out
    }

    /// Rebuilds a projection from 16 column-major components.
    pub fn from_array(values: [f32; 16]) -> Self {
        let mut m = [[0.0f32; 4]; 4];
        for (col, chunk) in values.chunks_exact(4).enumerate() {
            m[col].copy_from_slice(chunk);
        }
        Self { m }
    }

    /// A perspective projection (Godot `Projection.create_perspective`).
    ///
    /// `fov_y` is in **degrees**, matching Godot's argument and `Camera3D.fov`.
    pub fn create_perspective(fov_y: f32, aspect: f32, z_near: f32, z_far: f32) -> Self {
        let half_fov = (fov_y * 0.5).to_radians();
        let top = z_near * half_fov.tan();
        let right = top * aspect;
        Self::create_frustum(-right, right, -top, top, z_near, z_far)
    }

    /// An off-centre perspective frustum (Godot `Projection.create_frustum`).
    pub fn create_frustum(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let mut p = Self::ZERO;
        p.m[0][0] = 2.0 * z_near / (right - left);
        p.m[1][1] = 2.0 * z_near / (top - bottom);
        p.m[2][0] = (right + left) / (right - left);
        p.m[2][1] = (top + bottom) / (top - bottom);
        p.m[2][2] = -(z_far + z_near) / (z_far - z_near);
        p.m[2][3] = -1.0;
        p.m[3][2] = -2.0 * z_far * z_near / (z_far - z_near);
        p
    }

    /// An orthographic projection (Godot `Projection.create_orthogonal`).
    pub fn create_orthogonal(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let mut p = Self::IDENTITY;
        p.m[0][0] = 2.0 / (right - left);
        p.m[1][1] = 2.0 / (top - bottom);
        p.m[2][2] = -2.0 / (z_far - z_near);
        p.m[3][0] = -(right + left) / (right - left);
        p.m[3][1] = -(top + bottom) / (top - bottom);
        p.m[3][2] = -(z_far + z_near) / (z_far - z_near);
        p
    }

    /// Transforms a homogeneous point (Godot's `Projection * Vector4`).
    pub fn xform(self, v: Vector4) -> Vector4 {
        Vector4::new(
            self.m[0][0] * v.x + self.m[1][0] * v.y + self.m[2][0] * v.z + self.m[3][0] * v.w,
            self.m[0][1] * v.x + self.m[1][1] * v.y + self.m[2][1] * v.z + self.m[3][1] * v.w,
            self.m[0][2] * v.x + self.m[1][2] * v.y + self.m[2][2] * v.z + self.m[3][2] * v.w,
            self.m[0][3] * v.x + self.m[1][3] * v.y + self.m[2][3] * v.z + self.m[3][3] * v.w,
        )
    }

    /// The transpose (Godot `Projection.transposed`).
    pub fn transposed(self) -> Self {
        let mut out = Self::ZERO;
        for col in 0..4 {
            for row in 0..4 {
                out.m[col][row] = self.m[row][col];
            }
        }
        out
    }

    /// A perspective frustum shifted off-axis by `offset`, in near-plane units.
    ///
    /// The form a stereo/HMD eye or a shifted-lens camera needs, and the one
    /// `gdscene::camera3d` already carries under this name — kept identical so
    /// consolidating the two `Projection` types is a move (pat-s3rrv.59).
    pub fn create_perspective_with_offset(
        fov_y: f32,
        aspect: f32,
        offset: Vector2,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let half_fov = (fov_y * 0.5).to_radians();
        let top = z_near * half_fov.tan();
        let right = top * aspect;
        Self::create_frustum(
            -right + offset.x,
            right + offset.x,
            -top + offset.y,
            top + offset.y,
            z_near,
            z_far,
        )
    }

    /// An orthographic projection sized by its vertical extent and aspect ratio
    /// — Godot's `Projection.create_orthogonal_aspect`, and the shape
    /// `Camera3D.size` describes.
    ///
    /// ⚠️ `gdscene::camera3d::Projection` — a field-for-field identical type —
    /// used to spell this `create_orthogonal` and the off-centre form
    /// `create_orthogonal_off_center`, i.e. Godot's names on the wrong
    /// overloads. **That is fixed (pat-s3rrv.59)**: both types now use Godot's
    /// spelling, so a call moved between them keeps its meaning. Consolidating
    /// the two types onto this one is now a re-export plus an import sweep,
    /// with no call-site rewriting left to do.
    pub fn create_orthogonal_aspect(size: f32, aspect: f32, z_near: f32, z_far: f32) -> Self {
        let half_h = size * 0.5;
        let half_w = half_h * aspect;
        Self::create_orthogonal(-half_w, half_w, -half_h, half_h, z_near, z_far)
    }

    /// The matrix determinant.
    ///
    /// Computed by the same Gauss elimination the inverse uses, so the two can
    /// never disagree about whether a matrix is singular.
    pub fn determinant(self) -> f32 {
        let mut a = self.m;
        let mut det = 1.0f32;
        for col in 0..4 {
            // Partial pivoting: swap in the row with the largest magnitude so a
            // zero (or tiny) pivot does not blow the elimination up.
            let mut pivot = col;
            for row in col + 1..4 {
                if a[row][col].abs() > a[pivot][col].abs() {
                    pivot = row;
                }
            }
            if a[pivot][col].abs() < 1e-12 {
                return 0.0;
            }
            if pivot != col {
                a.swap(pivot, col);
                det = -det;
            }
            det *= a[col][col];
            let inv_pivot = 1.0 / a[col][col];
            for row in col + 1..4 {
                let factor = a[row][col] * inv_pivot;
                if factor != 0.0 {
                    for k in col..4 {
                        a[row][k] -= factor * a[col][k];
                    }
                }
            }
        }
        det
    }

    /// The inverse projection — what unprojecting a screen point needs.
    ///
    /// Solved by Gauss-Jordan elimination with partial pivoting rather than a
    /// transcribed cofactor expansion: the elimination is short enough to read,
    /// and `p * p.inverse() == IDENTITY` is a property a test can check
    /// directly, which a hand-written cofactor table is not.
    ///
    /// A singular matrix returns [`IDENTITY`](Self::IDENTITY) rather than a
    /// matrix full of infinities, matching what `gdscene::camera3d` does — a
    /// degenerate camera should render wrongly, not poison every later
    /// computation with NaN.
    pub fn inverse(self) -> Self {
        // Row-major working copy: `a[row]` is a row of the matrix followed by a
        // row of the identity. `self.m` is COLUMN-major, so this transposes on
        // the way in and back on the way out.
        let mut a = [[0.0f32; 8]; 4];
        for row in 0..4 {
            for col in 0..4 {
                a[row][col] = self.m[col][row];
            }
            a[row][4 + row] = 1.0;
        }

        for col in 0..4 {
            let mut pivot = col;
            for row in col + 1..4 {
                if a[row][col].abs() > a[pivot][col].abs() {
                    pivot = row;
                }
            }
            if a[pivot][col].abs() < 1e-10 {
                return Self::IDENTITY;
            }
            a.swap(pivot, col);

            let inv_pivot = 1.0 / a[col][col];
            for k in 0..8 {
                a[col][k] *= inv_pivot;
            }
            for row in 0..4 {
                if row == col {
                    continue;
                }
                let factor = a[row][col];
                if factor != 0.0 {
                    for k in 0..8 {
                        a[row][k] -= factor * a[col][k];
                    }
                }
            }
        }

        let mut out = Self::ZERO;
        for row in 0..4 {
            for col in 0..4 {
                out.m[col][row] = a[row][4 + col];
            }
        }
        out
    }

    /// The six clipping planes, in the order `[left, right, bottom, top, near,
    /// far]`, with normals pointing INTO the frustum.
    ///
    /// Gribb–Hartmann extraction from the matrix rows. Gribb–Hartmann yields
    /// planes where `ax + by + cz + d >= 0` means "inside", while [`Plane`]'s
    /// `distance_to` computes `normal·p - d` — hence the negated `d`. Getting
    /// that sign wrong inverts every culling test, so it is spelled out here
    /// and asserted in the tests.
    pub fn get_frustum_planes(self) -> [Plane; 6] {
        let m = &self.m;
        // `m` is column-major, so a matrix ROW gathers one component across all
        // four columns.
        let row = |r: usize| -> [f32; 4] { [m[0][r], m[1][r], m[2][r], m[3][r]] };
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));

        let make_plane = |a: f32, b: f32, c: f32, d: f32| -> Plane {
            let len = (a * a + b * b + c * c).sqrt();
            if len < 1e-10 {
                return Plane::new(Vector3::new(0.0, 1.0, 0.0), 0.0);
            }
            let inv = 1.0 / len;
            Plane::new(Vector3::new(a * inv, b * inv, c * inv), -d * inv)
        };
        let combine = |sign: f32, r: [f32; 4]| -> Plane {
            make_plane(
                r3[0] + sign * r[0],
                r3[1] + sign * r[1],
                r3[2] + sign * r[2],
                r3[3] + sign * r[3],
            )
        };

        [
            combine(1.0, r0),  // left
            combine(-1.0, r0), // right
            combine(1.0, r1),  // bottom
            combine(-1.0, r1), // top
            combine(1.0, r2),  // near
            combine(-1.0, r2), // far
        ]
    }

    /// Whether the projection has no perspective divide — Godot
    /// `Projection.is_orthogonal`, which tests the last row of the matrix.
    pub fn is_orthogonal(self) -> bool {
        self.m[0][3] == 0.0 && self.m[1][3] == 0.0 && self.m[2][3] == 0.0 && self.m[3][3] == 1.0
    }
}

impl Default for Projection {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Projection {
    type Output = Self;

    /// Matrix product, in Godot's order: `a * b` applies `b` first.
    fn mul(self, rhs: Self) -> Self {
        let mut out = Self::ZERO;
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.m[k][row] * rhs.m[col][k];
                }
                out.m[col][row] = sum;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// pat-s3rrv.148 — Euler order, and the affine method gaps
// ---------------------------------------------------------------------------

/// Godot's global `EulerOrder` enum — the sequence the three axis rotations are
/// composed in.
///
/// 🔑 The default is **YXZ**, not XYZ. `Basis::get_euler()` and
/// `Node3D.rotation` both use it, so a port that defaulted to the more familiar
/// XYZ would disagree with the editor's own rotation fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EulerOrder {
    /// X, then Y, then Z.
    Xyz = 0,
    /// X, then Z, then Y.
    Xzy = 1,
    /// Y, then X, then Z — Godot's default.
    #[default]
    Yxz = 2,
    /// Y, then Z, then X.
    Yzx = 3,
    /// Z, then X, then Y.
    Zxy = 4,
    /// Z, then Y, then X.
    Zyx = 5,
}

impl EulerOrder {
    /// Maps the integer a script passes (`EULER_ORDER_*`) to an order.
    ///
    /// `None` for an out-of-range value, so a caller can reject it rather than
    /// silently substituting the default — Godot's own `set_euler` takes the
    /// `ERR_FAIL_MSG("Invalid Euler order parameter.")` path there.
    pub fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Xyz),
            1 => Some(Self::Xzy),
            2 => Some(Self::Yxz),
            3 => Some(Self::Yzx),
            4 => Some(Self::Zxy),
            5 => Some(Self::Zyx),
            _ => None,
        }
    }
}

impl Basis {
    /// A rotation about X by `angle`, as a basis.
    ///
    /// ⚠️ Godot's `Basis(1,0,0, 0,c,-s, 0,s,c)` constructor takes **rows**,
    /// while this type stores **columns**, so the sign of `s` moves between the
    /// two off-diagonal slots. Getting that backwards produces a rotation the
    /// right size in the wrong direction — which looks correct in any test that
    /// only checks a magnitude.
    fn rotation_x(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, c, s),
            z: Vector3::new(0.0, -s, c),
        }
    }

    /// A rotation about Y by `angle`, as a basis.
    fn rotation_y(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            x: Vector3::new(c, 0.0, -s),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(s, 0.0, c),
        }
    }

    /// A rotation about Z by `angle`, as a basis.
    fn rotation_z(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            x: Vector3::new(c, s, 0.0),
            y: Vector3::new(-s, c, 0.0),
            z: Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// Builds a basis from Euler angles composed in `order`
    /// (`Basis::set_euler`, `basis.cpp:656`).
    ///
    /// ⓘ Composed as a product of three axis rotations, exactly as Godot does,
    /// rather than as six hand-expanded closed forms. The product is verifiable
    /// term by term against the source; a transcribed closed form is six
    /// opportunities to swap a sign in a place nothing would catch.
    ///
    /// [`from_euler`](Self::from_euler) is this with [`EulerOrder::Yxz`].
    pub fn from_euler_order(euler: Vector3, order: EulerOrder) -> Self {
        let x = Self::rotation_x(euler.x);
        let y = Self::rotation_y(euler.y);
        let z = Self::rotation_z(euler.z);
        match order {
            EulerOrder::Xyz => x * (y * z),
            EulerOrder::Xzy => x * z * y,
            EulerOrder::Yxz => y * x * z,
            EulerOrder::Yzx => y * z * x,
            EulerOrder::Zxy => z * x * y,
            EulerOrder::Zyx => z * y * x,
        }
    }

    /// Extracts Euler angles in `order` (`Basis::get_euler`, `basis.cpp:455`).
    ///
    /// ⚠️ Only the inverse of [`from_euler_order`](Self::from_euler_order) with
    /// the SAME order. Reading angles back with a different order than they
    /// were written with yields a different rotation, silently — which is the
    /// bug this whole slice exists to remove from the script bindings.
    ///
    /// ⓘ Godot's element access is `rows[i][j]`; this type stores columns, so
    /// each `rows[i][j]` below reads as `<column j>.<component i>`. The mapping
    /// was checked against the pre-existing YXZ extraction, which is known
    /// good.
    pub fn to_euler_order(self, order: EulerOrder) -> Vector3 {
        // Godot's own epsilon: angles within ±0.04° of the singularity are
        // truncated to it (`basis.cpp:459`).
        const EPS: f32 = 0.000_000_25;
        // rows[i][j] == column j, component i.
        let r = |i: usize, j: usize| -> f32 {
            let col = match j {
                0 => self.x,
                1 => self.y,
                _ => self.z,
            };
            match i {
                0 => col.x,
                1 => col.y,
                _ => col.z,
            }
        };
        let half_pi = std::f32::consts::FRAC_PI_2;

        match order {
            EulerOrder::Xyz => {
                let sy = r(0, 2);
                if sy < 1.0 - EPS {
                    if sy > -(1.0 - EPS) {
                        Vector3::new(
                            (-r(1, 2)).atan2(r(2, 2)),
                            sy.asin(),
                            (-r(0, 1)).atan2(r(0, 0)),
                        )
                    } else {
                        Vector3::new(r(2, 1).atan2(r(1, 1)), -half_pi, 0.0)
                    }
                } else {
                    Vector3::new(r(2, 1).atan2(r(1, 1)), half_pi, 0.0)
                }
            }
            EulerOrder::Xzy => {
                let sz = r(0, 1);
                if sz < 1.0 - EPS {
                    if sz > -(1.0 - EPS) {
                        Vector3::new(r(2, 1).atan2(r(1, 1)), r(0, 2).atan2(r(0, 0)), (-sz).asin())
                    } else {
                        Vector3::new(-r(1, 2).atan2(r(2, 2)), 0.0, half_pi)
                    }
                } else {
                    Vector3::new(-r(1, 2).atan2(r(2, 2)), 0.0, -half_pi)
                }
            }
            EulerOrder::Yxz => {
                let m12 = r(1, 2);
                if m12 < 1.0 - EPS {
                    if m12 > -(1.0 - EPS) {
                        Vector3::new(
                            (-m12).asin(),
                            r(0, 2).atan2(r(2, 2)),
                            r(1, 0).atan2(r(1, 1)),
                        )
                    } else {
                        Vector3::new(half_pi, r(0, 1).atan2(r(0, 0)), 0.0)
                    }
                } else {
                    Vector3::new(-half_pi, -r(0, 1).atan2(r(0, 0)), 0.0)
                }
            }
            EulerOrder::Yzx => {
                let sz = r(1, 0);
                if sz < 1.0 - EPS {
                    if sz > -(1.0 - EPS) {
                        Vector3::new(
                            (-r(1, 2)).atan2(r(1, 1)),
                            (-r(2, 0)).atan2(r(0, 0)),
                            sz.asin(),
                        )
                    } else {
                        Vector3::new(r(2, 1).atan2(r(2, 2)), 0.0, -half_pi)
                    }
                } else {
                    Vector3::new(r(2, 1).atan2(r(2, 2)), 0.0, half_pi)
                }
            }
            EulerOrder::Zxy => {
                let sx = r(2, 1);
                if sx < 1.0 - EPS {
                    if sx > -(1.0 - EPS) {
                        Vector3::new(
                            sx.asin(),
                            (-r(2, 0)).atan2(r(2, 2)),
                            (-r(0, 1)).atan2(r(1, 1)),
                        )
                    } else {
                        Vector3::new(-half_pi, r(0, 2).atan2(r(0, 0)), 0.0)
                    }
                } else {
                    Vector3::new(half_pi, r(0, 2).atan2(r(0, 0)), 0.0)
                }
            }
            EulerOrder::Zyx => {
                let sy = r(2, 0);
                if sy < 1.0 - EPS {
                    if sy > -(1.0 - EPS) {
                        Vector3::new(r(2, 1).atan2(r(2, 2)), (-sy).asin(), r(1, 0).atan2(r(0, 0)))
                    } else {
                        Vector3::new(0.0, half_pi, -r(0, 1).atan2(r(1, 1)))
                    }
                } else {
                    Vector3::new(0.0, -half_pi, r(0, 1).atan2(r(1, 1)))
                }
            }
        }
    }

    /// A basis that scales by `scale` and rotates not at all
    /// (`Basis.from_scale`).
    pub fn from_scale(scale: Vector3) -> Self {
        Self {
            x: Vector3::new(scale.x, 0.0, 0.0),
            y: Vector3::new(0.0, scale.y, 0.0),
            z: Vector3::new(0.0, 0.0, scale.z),
        }
    }

    /// Scales in the basis's OWN space rather than the parent's
    /// (`Basis.scaled_local`).
    ///
    /// ⚠️ `self * from_scale(s)`, not `from_scale(s) * self`. The two differ
    /// for any basis that already carries a rotation, which is precisely when
    /// someone reaches for the `_local` variant.
    pub fn scaled_local(self, scale: Vector3) -> Self {
        self * Self::from_scale(scale)
    }

    /// Whether this basis is a rotation with uniform scale — angle-preserving
    /// (`Basis.is_conformal`).
    ///
    /// Ported from `basis.cpp`: the three columns must share one length and be
    /// mutually perpendicular.
    pub fn is_conformal(self) -> bool {
        let eps = 1e-5;
        let x = self.x;
        let y = self.y;
        let z = self.z;
        let x_len_sq = x.length_squared();
        (x_len_sq - y.length_squared()).abs() < eps
            && (x_len_sq - z.length_squared()).abs() < eps
            && x.dot(y).abs() < eps
            && x.dot(z).abs() < eps
            && y.dot(z).abs() < eps
    }

    /// Whether every component is finite (`Basis.is_finite`).
    pub fn is_finite(self) -> bool {
        vec3_is_finite(self.x) && vec3_is_finite(self.y) && vec3_is_finite(self.z)
    }

    /// Dot product of `v` with the basis's first ROW (`Basis.tdotx`).
    ///
    /// ⚠️ Row, not column. `tdotx` is the "transposed dot" — it is `x.x`,
    /// `y.x`, `z.x` across the three columns, i.e. the x component of each.
    /// Using the `x` column instead gives the untransposed product, which is
    /// equal for a symmetric basis and different for every other one.
    pub fn tdotx(self, v: Vector3) -> f32 {
        self.x.x * v.x + self.y.x * v.y + self.z.x * v.z
    }

    /// Dot product of `v` with the basis's second row (`Basis.tdoty`).
    pub fn tdoty(self, v: Vector3) -> f32 {
        self.x.y * v.x + self.y.y * v.y + self.z.y * v.z
    }

    /// Dot product of `v` with the basis's third row (`Basis.tdotz`).
    pub fn tdotz(self, v: Vector3) -> f32 {
        self.x.z * v.x + self.y.z * v.y + self.z.z * v.z
    }

    /// A basis whose -Z points at `target`, with `up` as the up hint
    /// (`Basis.looking_at`).
    ///
    /// 🔑 Godot's forward is **-Z**, so the Z column is `-target.normalized()`.
    /// Building it as `+target` yields a basis that faces exactly backwards —
    /// and still passes any test that only checks orthonormality.
    pub fn looking_at(target: Vector3, up: Vector3) -> Self {
        let v_z = -target.normalized();
        let mut v_x = up.cross(v_z);
        if v_x.length_squared() < 1e-12 {
            // ⚠️ Colinear target and up. Godot picks an arbitrary perpendicular
            // (`vector3.h:375`) rather than giving up, because returning the
            // identity here would leave the node UNROTATED — which reads as
            // "looking_at did nothing" instead of "the up hint was unusable".
            let a = if up.x.abs() <= up.y.abs() && up.x.abs() <= up.z.abs() {
                Vector3::new(1.0, 0.0, 0.0)
            } else {
                Vector3::new(0.0, 1.0, 0.0)
            };
            v_x = up.cross(a);
        }
        let v_x = v_x.normalized();
        let v_y = v_z.cross(v_x);
        Self {
            x: v_x,
            y: v_y,
            z: v_z,
        }
    }
}

/// Whether all three components are finite.
fn vec3_is_finite(v: Vector3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

impl Quaternion {
    /// The rotation axis (`Quaternion.get_axis`).
    pub fn get_axis(self) -> Vector3 {
        if self.w.abs() > 1.0 - 1e-6 {
            // A near-identity rotation has no meaningful axis; Godot returns
            // the imaginary part unnormalized here.
            return Vector3::new(self.x, self.y, self.z);
        }
        let r = 1.0 / (1.0 - self.w * self.w).sqrt();
        Vector3::new(self.x * r, self.y * r, self.z * r)
    }

    /// The rotation angle in radians (`Quaternion.get_angle`).
    pub fn get_angle(self) -> f32 {
        2.0 * self.w.clamp(-1.0, 1.0).acos()
    }

    /// Builds a rotation from Euler angles composed in `order`
    /// (`Quaternion.from_euler`).
    pub fn from_euler_order(euler: Vector3, order: EulerOrder) -> Self {
        Basis::from_euler_order(euler, order).to_quaternion()
    }

    /// Extracts Euler angles in `order` (`Quaternion.get_euler`).
    pub fn to_euler_order(self, order: EulerOrder) -> Vector3 {
        Basis::from_quaternion(self).to_euler_order(order)
    }

    /// Whether every component is finite (`Quaternion.is_finite`).
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }

    /// Component-wise negation — the quaternion representing the SAME rotation
    /// by the other path around.
    fn negated(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: -self.w,
        }
    }

    /// `Quaternion.log()` — the axis scaled by the angle, as a pure quaternion.
    pub fn log(self) -> Self {
        let v = self.get_axis();
        let angle = self.get_angle();
        Self {
            x: v.x * angle,
            y: v.y * angle,
            z: v.z * angle,
            w: 0.0,
        }
    }

    /// `Quaternion.exp()` — the inverse of [`log`](Self::log).
    pub fn exp(self) -> Self {
        let v = Vector3::new(self.x, self.y, self.z);
        let theta = v.length();
        let v = v.normalized();
        if theta < 1e-5 || !v.is_normalized() {
            return Self::IDENTITY;
        }
        Self::from_axis_angle(v, theta)
    }

    /// `Quaternion.slerpni(to, weight)` — spherical interpolation that does
    /// **not** flip to the shorter arc.
    ///
    /// ⚠️ This is the whole difference from [`slerp`](Self::slerp): `slerp`
    /// negates `to` when the dot product is negative so the rotation takes the
    /// short way round; `slerpni` never does, so it can travel more than 180°.
    /// Implementing it as a call to `slerp` would make the two identical and
    /// the distinction invisible.
    pub fn slerpni(self, to: Self, weight: f32) -> Self {
        let dot = self.dot(to);
        if dot.abs() > 0.9999 {
            return self;
        }
        let theta = dot.acos();
        let sin_t = 1.0 / theta.sin();
        let new_factor = (weight * theta).sin() * sin_t;
        let inv_factor = ((1.0 - weight) * theta).sin() * sin_t;
        Self {
            x: inv_factor * self.x + new_factor * to.x,
            y: inv_factor * self.y + new_factor * to.y,
            z: inv_factor * self.z + new_factor * to.z,
            w: inv_factor * self.w + new_factor * to.w,
        }
    }

    /// `Quaternion.spherical_cubic_interpolate(b, pre_a, post_b, weight)`.
    ///
    /// Ported from `quaternion.cpp`: interpolate in the log map of both ends,
    /// then blend the two results to cancel the exp-map ambiguity.
    pub fn spherical_cubic_interpolate(
        self,
        b: Self,
        pre_a: Self,
        post_b: Self,
        weight: f32,
    ) -> Self {
        let (from_q, pre_q, to_q, post_q) = align_cubic_quaternions(self, b, pre_a, post_b);

        // In `from_q`'s tangent space.
        let ln_to = (from_q.inverse() * to_q).log();
        let ln_pre = (from_q.inverse() * pre_q).log();
        let ln_post = (from_q.inverse() * post_q).log();
        let ln1 = Vector3::new(
            crate::math::cubic_interpolate_f(0.0, ln_to.x, ln_pre.x, ln_post.x, weight),
            crate::math::cubic_interpolate_f(0.0, ln_to.y, ln_pre.y, ln_post.y, weight),
            crate::math::cubic_interpolate_f(0.0, ln_to.z, ln_pre.z, ln_post.z, weight),
        );
        let q1 = from_q * pure_quaternion(ln1).exp();

        // In `to_q`'s tangent space.
        let ln_from = (to_q.inverse() * from_q).log();
        let ln_pre = (to_q.inverse() * pre_q).log();
        let ln_post = (to_q.inverse() * post_q).log();
        let ln2 = Vector3::new(
            crate::math::cubic_interpolate_f(ln_from.x, 0.0, ln_pre.x, ln_post.x, weight),
            crate::math::cubic_interpolate_f(ln_from.y, 0.0, ln_pre.y, ln_post.y, weight),
            crate::math::cubic_interpolate_f(ln_from.z, 0.0, ln_pre.z, ln_post.z, weight),
        );
        let q2 = to_q * pure_quaternion(ln2).exp();

        q1.slerp(q2, weight)
    }

    /// `Quaternion.spherical_cubic_interpolate_in_time(...)` — the same with
    /// explicit, possibly uneven, key times.
    #[allow(clippy::too_many_arguments)]
    pub fn spherical_cubic_interpolate_in_time(
        self,
        b: Self,
        pre_a: Self,
        post_b: Self,
        weight: f32,
        b_t: f32,
        pre_a_t: f32,
        post_b_t: f32,
    ) -> Self {
        let (from_q, pre_q, to_q, post_q) = align_cubic_quaternions(self, b, pre_a, post_b);

        let ln_to = (from_q.inverse() * to_q).log();
        let ln_pre = (from_q.inverse() * pre_q).log();
        let ln_post = (from_q.inverse() * post_q).log();
        let t1 = |f: f32, t: f32, p: f32, o: f32| {
            crate::math::cubic_interpolate_in_time_f(f, t, p, o, weight, b_t, pre_a_t, post_b_t)
        };
        let ln1 = Vector3::new(
            t1(0.0, ln_to.x, ln_pre.x, ln_post.x),
            t1(0.0, ln_to.y, ln_pre.y, ln_post.y),
            t1(0.0, ln_to.z, ln_pre.z, ln_post.z),
        );
        let q1 = from_q * pure_quaternion(ln1).exp();

        let ln_from = (to_q.inverse() * from_q).log();
        let ln_pre = (to_q.inverse() * pre_q).log();
        let ln_post = (to_q.inverse() * post_q).log();
        let ln2 = Vector3::new(
            t1(ln_from.x, 0.0, ln_pre.x, ln_post.x),
            t1(ln_from.y, 0.0, ln_pre.y, ln_post.y),
            t1(ln_from.z, 0.0, ln_pre.z, ln_post.z),
        );
        let q2 = to_q * pure_quaternion(ln2).exp();

        q1.slerp(q2, weight)
    }
}

/// A pure (zero-real) quaternion carrying `v`.
fn pure_quaternion(v: Vector3) -> Quaternion {
    Quaternion {
        x: v.x,
        y: v.y,
        z: v.z,
        w: 0.0,
    }
}

/// Normalizes the four control rotations onto a consistent hemisphere.
///
/// ⚠️ Without this, a control quaternion on the opposite hemisphere — which
/// represents the SAME rotation — sends the interpolation the long way round
/// and the curve visibly snaps. `q` and `-q` being the same rotation is exactly
/// what makes this necessary and easy to omit.
fn align_cubic_quaternions(
    from: Quaternion,
    to: Quaternion,
    pre: Quaternion,
    post: Quaternion,
) -> (Quaternion, Quaternion, Quaternion, Quaternion) {
    let from_q = Basis::from_quaternion(from).get_rotation_quaternion();
    let mut pre_q = Basis::from_quaternion(pre).get_rotation_quaternion();
    let mut to_q = Basis::from_quaternion(to).get_rotation_quaternion();
    let mut post_q = Basis::from_quaternion(post).get_rotation_quaternion();

    let flip1 = from_q.dot(pre_q).is_sign_negative();
    if flip1 {
        pre_q = pre_q.negated();
    }
    let flip2 = from_q.dot(to_q).is_sign_negative();
    if flip2 {
        to_q = to_q.negated();
    }
    // ⚠️ Not symmetric with the other two: when `to` was flipped the test
    // becomes `<= 0` rather than a sign-bit check, so an exactly perpendicular
    // control flips here and not otherwise (`quaternion.cpp`).
    let flip3 = if flip2 {
        to_q.dot(post_q) <= 0.0
    } else {
        to_q.dot(post_q).is_sign_negative()
    };
    if flip3 {
        post_q = post_q.negated();
    }
    (from_q, pre_q, to_q, post_q)
}

impl Aabb {
    /// The corner furthest along `direction` (`AABB.get_support`).
    pub fn get_support(self, direction: Vector3) -> Vector3 {
        let mut support = self.position;
        if direction.x > 0.0 {
            support.x += self.size.x;
        }
        if direction.y > 0.0 {
            support.y += self.size.y;
        }
        if direction.z > 0.0 {
            support.z += self.size.z;
        }
        support
    }

    /// Whether the box has any surface area at all (`AABB.has_surface`).
    ///
    /// ⚠️ `||`, not `&&`: a flat box with one zero extent still has surface.
    /// This is why it is distinct from `has_volume`.
    pub fn has_surface(self) -> bool {
        self.size.x > 0.0 || self.size.y > 0.0 || self.size.z > 0.0
    }

    /// The unit vector along the longest extent (`AABB.get_longest_axis`).
    pub fn get_longest_axis(self) -> Vector3 {
        match self.get_longest_axis_index() {
            0 => Vector3::new(1.0, 0.0, 0.0),
            1 => Vector3::new(0.0, 1.0, 0.0),
            _ => Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// The index (0=X, 1=Y, 2=Z) of the longest extent.
    ///
    /// ⓘ Ties resolve to the EARLIER axis: the comparisons are strict `>`, so a
    /// cube reports X.
    pub fn get_longest_axis_index(self) -> i64 {
        let mut axis = 0;
        let mut max_size = self.size.x;
        if self.size.y > max_size {
            axis = 1;
            max_size = self.size.y;
        }
        if self.size.z > max_size {
            axis = 2;
        }
        axis
    }

    /// The length of the longest extent.
    pub fn get_longest_axis_size(self) -> f32 {
        self.size.x.max(self.size.y).max(self.size.z)
    }

    /// The unit vector along the shortest extent (`AABB.get_shortest_axis`).
    pub fn get_shortest_axis(self) -> Vector3 {
        match self.get_shortest_axis_index() {
            0 => Vector3::new(1.0, 0.0, 0.0),
            1 => Vector3::new(0.0, 1.0, 0.0),
            _ => Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// The index (0=X, 1=Y, 2=Z) of the shortest extent.
    pub fn get_shortest_axis_index(self) -> i64 {
        let mut axis = 0;
        let mut min_size = self.size.x;
        if self.size.y < min_size {
            axis = 1;
            min_size = self.size.y;
        }
        if self.size.z < min_size {
            axis = 2;
        }
        axis
    }

    /// The length of the shortest extent.
    pub fn get_shortest_axis_size(self) -> f32 {
        self.size.x.min(self.size.y).min(self.size.z)
    }

    /// Whether the plane cuts through the box (`AABB.intersects_plane`).
    ///
    /// ⚠️ True only when corners fall on BOTH sides. A box merely touching the
    /// plane, or entirely on one side, does not intersect it.
    pub fn intersects_plane(self, plane: Plane) -> bool {
        let mut over = false;
        let mut under = false;
        for i in 0..8 {
            if plane.distance_to(self.get_endpoint(i as u8)) > 0.0 {
                over = true;
            } else {
                under = true;
            }
        }
        under && over
    }

    /// Where a ray from `from` along `dir` first meets the box, if it does
    /// (`AABB.intersects_ray`).
    ///
    /// 🔑 A ray that STARTS INSIDE returns `from` itself. The geometric entry
    /// point is behind the origin in that case, and handing it back would put
    /// the hit behind the shooter — Godot's `intersects_ray_bind` special-cases
    /// exactly this.
    pub fn intersects_ray(self, from: Vector3, dir: Vector3) -> Option<Vector3> {
        let end = self.position + self.size;
        let mut tmin = -1e20f32;
        let mut tmax = 1e20f32;
        let mut axis = 0usize;

        let comp = |v: Vector3, i: usize| match i {
            0 => v.x,
            1 => v.y,
            _ => v.z,
        };

        for i in 0..3 {
            let d = comp(dir, i);
            if d == 0.0 {
                // Parallel to this slab: a miss unless already between them.
                if comp(from, i) < comp(self.position, i) || comp(from, i) > comp(end, i) {
                    return None;
                }
            } else {
                let mut t1 = (comp(self.position, i) - comp(from, i)) / d;
                let mut t2 = (comp(end, i) - comp(from, i)) / d;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                if t1 >= tmin {
                    tmin = t1;
                    axis = i;
                }
                if t2 < tmax {
                    if t2 < 0.0 {
                        return None;
                    }
                    tmax = t2;
                }
                if tmin > tmax {
                    return None;
                }
            }
        }

        if tmin < 0.0 {
            // Started inside.
            return Some(from);
        }

        let mut point = from + dir * tmin;
        // Snap onto the slab plane so float error cannot leave the reported hit
        // fractionally outside the box it just hit.
        let snapped = if comp(dir, axis) >= 0.0 {
            comp(self.position, axis)
        } else {
            comp(end, axis)
        };
        match axis {
            0 => point.x = snapped,
            1 => point.y = snapped,
            _ => point.z = snapped,
        }
        Some(point)
    }

    /// Where the segment `from`..`to` first meets the box, if it does
    /// (`AABB.intersects_segment`).
    pub fn intersects_segment(self, from: Vector3, to: Vector3) -> Option<Vector3> {
        let mut min = 0.0f32;
        let mut max = 1.0f32;

        let comp = |v: Vector3, i: usize| match i {
            0 => v.x,
            1 => v.y,
            _ => v.z,
        };

        for i in 0..3 {
            let seg_from = comp(from, i);
            let seg_to = comp(to, i);
            let box_begin = comp(self.position, i);
            let box_end = box_begin + comp(self.size, i);
            let (cmin, cmax);

            if seg_from < seg_to {
                if seg_from > box_end || seg_to < box_begin {
                    return None;
                }
                let length = seg_to - seg_from;
                cmin = if seg_from < box_begin {
                    (box_begin - seg_from) / length
                } else {
                    0.0
                };
                cmax = if seg_to > box_end {
                    (box_end - seg_from) / length
                } else {
                    1.0
                };
            } else {
                if seg_to > box_end || seg_from < box_begin {
                    return None;
                }
                let length = seg_to - seg_from;
                cmin = if seg_from > box_end {
                    (box_end - seg_from) / length
                } else {
                    0.0
                };
                cmax = if seg_to < box_begin {
                    (box_begin - seg_from) / length
                } else {
                    1.0
                };
            }

            if cmin > min {
                min = cmin;
            }
            if cmax < max {
                max = cmax;
            }
            if max < min {
                return None;
            }
        }

        Some(from + (to - from) * min)
    }

    /// Whether every component is finite (`AABB.is_finite`).
    pub fn is_finite(self) -> bool {
        vec3_is_finite(self.position) && vec3_is_finite(self.size)
    }
}

impl Plane {
    /// The point where three planes meet, if they do (`Plane.intersect_3`).
    ///
    /// `None` when any two are parallel, which is when the triple product of
    /// the normals vanishes.
    pub fn intersect_3(self, b: Plane, c: Plane) -> Option<Vector3> {
        let n0 = self.normal;
        let n1 = b.normal;
        let n2 = c.normal;
        let denom = n0.cross(n1).dot(n2);
        if denom.abs() < 1e-6 {
            return None;
        }
        Some((n1.cross(n2) * self.d + n2.cross(n0) * b.d + n0.cross(n1) * c.d) * (1.0 / denom))
    }

    /// Whether every component is finite (`Plane.is_finite`).
    pub fn is_finite(self) -> bool {
        vec3_is_finite(self.normal) && self.d.is_finite()
    }
}

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

    // -- Basis <-> Quaternion, interpolation, AABB/Plane extras
    //    (pat-0q2kx.50) -----------------------------------------------------

    /// `to_quaternion` must invert `from_quaternion`. Compared by the rotation
    /// they apply rather than component-wise, since `q` and `-q` name the same
    /// rotation.
    #[test]
    fn basis_quaternion_round_trips() {
        let probe = Vector3::new(1.0, 2.0, 3.0);
        for q in [
            Quaternion::IDENTITY,
            Quaternion::from_axis_angle(Vector3::UP, FRAC_PI_2),
            Quaternion::from_axis_angle(Vector3::new(1.0, 0.0, 0.0), PI),
            Quaternion::from_axis_angle(Vector3::new(1.0, 1.0, 1.0), FRAC_PI_4),
        ] {
            let back = Basis::from_quaternion(q).to_quaternion();
            assert!(
                v3_approx_eq(back.xform(probe), q.xform(probe)),
                "round trip changed the rotation for {q:?}"
            );
        }
    }

    /// A scaled basis reports its rotation without the scale, and rebuilding
    /// from that rotation plus the scale reproduces the basis.
    #[test]
    fn basis_rotation_quaternion_ignores_scale() {
        let rot = Quaternion::from_axis_angle(Vector3::UP, FRAC_PI_2);
        let scale = Vector3::new(2.0, 3.0, 4.0);
        let b = Basis::from_quaternion_scale(rot, scale);
        assert!(v3_approx_eq(b.get_scale(), scale));

        let probe = Vector3::new(1.0, 0.0, 0.0);
        assert!(v3_approx_eq(
            b.get_rotation_quaternion().xform(probe),
            rot.xform(probe)
        ));
    }

    #[test]
    fn transform3d_interpolate_with_hits_both_ends() {
        let a = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(1.0, 2.0, 3.0),
        };
        let b = Transform3D {
            basis: Basis::from_quaternion(Quaternion::from_axis_angle(Vector3::UP, FRAC_PI_2)),
            origin: Vector3::new(3.0, 2.0, 3.0),
        };
        assert!(v3_approx_eq(a.interpolate_with(b, 0.0).origin, a.origin));
        assert!(v3_approx_eq(a.interpolate_with(b, 1.0).origin, b.origin));
        // The midpoint sits halfway along the path, and stays a rigid motion.
        let mid = a.interpolate_with(b, 0.5);
        assert!(v3_approx_eq(mid.origin, Vector3::new(2.0, 2.0, 3.0)));
        assert!(v3_approx_eq(mid.basis.get_scale(), Vector3::ONE));
    }

    #[test]
    fn aabb_grow_abs_and_region_algebra() {
        let a = Aabb::new(Vector3::ZERO, Vector3::new(2.0, 2.0, 2.0));
        let grown = a.grow(1.0);
        assert!(v3_approx_eq(grown.position, Vector3::new(-1.0, -1.0, -1.0)));
        assert!(v3_approx_eq(grown.size, Vector3::new(4.0, 4.0, 4.0)));
        assert!(grown.encloses(a));
        assert!(!a.encloses(grown));
        assert!(v3_approx_eq(a.get_end(), Vector3::new(2.0, 2.0, 2.0)));

        let b = Aabb::new(Vector3::new(1.0, 1.0, 1.0), Vector3::new(2.0, 2.0, 2.0));
        let overlap = a.intersection(b);
        assert!(v3_approx_eq(overlap.position, Vector3::new(1.0, 1.0, 1.0)));
        assert!(v3_approx_eq(overlap.size, Vector3::ONE));

        // Disjoint boxes intersect in nothing.
        let far = Aabb::new(Vector3::new(50.0, 50.0, 50.0), Vector3::ONE);
        assert!(v3_approx_eq(a.intersection(far).size, Vector3::ZERO));

        // A negative-size box normalizes to the same region.
        let negative = Aabb::new(Vector3::new(2.0, 2.0, 2.0), Vector3::new(-2.0, -2.0, -2.0));
        assert!(v3_approx_eq(negative.abs().position, Vector3::ZERO));
        assert!(v3_approx_eq(
            negative.abs().size,
            Vector3::new(2.0, 2.0, 2.0)
        ));
    }

    #[test]
    fn plane_project_and_center() {
        let p = Plane::new(Vector3::UP, 1.0);
        // Projection drops the point onto the plane and is idempotent.
        let projected = p.project(Vector3::new(0.0, 4.0, 0.0));
        assert!(v3_approx_eq(projected, Vector3::new(0.0, 1.0, 0.0)));
        assert!(v3_approx_eq(p.project(projected), projected));
        assert!(approx_eq(p.distance_to(projected), 0.0));
        assert!(p.has_point(projected, 1e-4));
        assert!(!p.has_point(Vector3::ZERO, 1e-4));
        assert!(v3_approx_eq(p.get_center(), Vector3::new(0.0, 1.0, 0.0)));
    }
}
