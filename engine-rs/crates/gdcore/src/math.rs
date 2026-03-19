//! Core math types and utilities.
//!
//! Provides the fundamental math primitives used throughout the engine:
//! vectors, transforms, rects, and colors. These mirror Godot's built-in
//! math types and follow the same conventions (Y-up, right-handed for 3D).

use std::ops::{Add, Div, Mul, Neg, Sub};

// ---------------------------------------------------------------------------
// Vector2i
// ---------------------------------------------------------------------------

/// A 2D vector with `i32` components, matching Godot's `Vector2i`.
///
/// Used for tile coordinates, pixel-perfect positions, and grid-based math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vector2i {
    /// The X component.
    pub x: i32,
    /// The Y component.
    pub y: i32,
}

impl Vector2i {
    /// The zero vector `(0, 0)`.
    pub const ZERO: Self = Self { x: 0, y: 0 };
    /// The one vector `(1, 1)`.
    pub const ONE: Self = Self { x: 1, y: 1 };
    /// The up direction `(0, -1)`.
    pub const UP: Self = Self { x: 0, y: -1 };
    /// The down direction `(0, 1)`.
    pub const DOWN: Self = Self { x: 0, y: 1 };
    /// The left direction `(-1, 0)`.
    pub const LEFT: Self = Self { x: -1, y: 0 };
    /// The right direction `(1, 0)`.
    pub const RIGHT: Self = Self { x: 1, y: 0 };

    /// Creates a new integer vector.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl Add for Vector2i {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vector2i {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Neg for Vector2i {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

// ---------------------------------------------------------------------------
// Vector2
// ---------------------------------------------------------------------------

/// A 2D vector with `f32` components, matching Godot's `Vector2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector2 {
    /// The X component.
    pub x: f32,
    /// The Y component.
    pub y: f32,
}

impl Vector2 {
    /// The zero vector `(0, 0)`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    /// The one vector `(1, 1)`.
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };
    /// The up direction `(0, -1)`.
    pub const UP: Self = Self { x: 0.0, y: -1.0 };
    /// The down direction `(0, 1)`.
    pub const DOWN: Self = Self { x: 0.0, y: 1.0 };
    /// The left direction `(-1, 0)`.
    pub const LEFT: Self = Self { x: -1.0, y: 0.0 };
    /// The right direction `(1, 0)`.
    pub const RIGHT: Self = Self { x: 1.0, y: 0.0 };

    /// Creates a new vector.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns the squared length (avoids a sqrt).
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Returns the length of the vector.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a normalized copy, or `ZERO` if the length is ~0.
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < 1e-10 {
            Self::ZERO
        } else {
            Self::new(self.x / len, self.y / len)
        }
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// 2D cross product (returns scalar z-component).
    pub fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    /// Linear interpolation.
    pub fn lerp(self, to: Self, t: f32) -> Self {
        Self::new(self.x + (to.x - self.x) * t, self.y + (to.y - self.y) * t)
    }

    /// Returns the angle of the vector (atan2(y, x)).
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    /// Returns the distance to another vector.
    pub fn distance_to(self, other: Self) -> f32 {
        (self - other).length()
    }
}

impl Add for Vector2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vector2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f32> for Vector2 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s)
    }
}

impl Div<f32> for Vector2 {
    type Output = Self;
    fn div(self, s: f32) -> Self {
        Self::new(self.x / s, self.y / s)
    }
}

impl Mul<Vector2> for f32 {
    type Output = Vector2;
    fn mul(self, v: Vector2) -> Vector2 {
        Vector2::new(self * v.x, self * v.y)
    }
}

impl Neg for Vector2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

// ---------------------------------------------------------------------------
// Vector3
// ---------------------------------------------------------------------------

/// A 3D vector with `f32` components, matching Godot's `Vector3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3 {
    /// The X component.
    pub x: f32,
    /// The Y component.
    pub y: f32,
    /// The Z component.
    pub z: f32,
}

impl Vector3 {
    /// The zero vector `(0, 0, 0)`.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// The one vector `(1, 1, 1)`.
    pub const ONE: Self = Self {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    /// The up direction `(0, 1, 0)`.
    pub const UP: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    /// The down direction `(0, -1, 0)`.
    pub const DOWN: Self = Self {
        x: 0.0,
        y: -1.0,
        z: 0.0,
    };
    /// The forward direction `(0, 0, -1)`.
    pub const FORWARD: Self = Self {
        x: 0.0,
        y: 0.0,
        z: -1.0,
    };

    /// Creates a new vector.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Returns the squared length.
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Returns the length of the vector.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a normalized copy, or `ZERO` if the length is ~0.
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < 1e-10 {
            Self::ZERO
        } else {
            Self::new(self.x / len, self.y / len, self.z / len)
        }
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product.
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Linear interpolation.
    pub fn lerp(self, to: Self, t: f32) -> Self {
        Self::new(
            self.x + (to.x - self.x) * t,
            self.y + (to.y - self.y) * t,
            self.z + (to.z - self.z) * t,
        )
    }

    /// Returns the distance to another vector.
    pub fn distance_to(self, other: Self) -> f32 {
        (self - other).length()
    }
}

impl Add for Vector3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vector3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f32> for Vector3 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Div<f32> for Vector3 {
    type Output = Self;
    fn div(self, s: f32) -> Self {
        Self::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Mul<Vector3> for f32 {
    type Output = Vector3;
    fn mul(self, v: Vector3) -> Vector3 {
        Vector3::new(self * v.x, self * v.y, self * v.z)
    }
}

impl Neg for Vector3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

// ---------------------------------------------------------------------------
// Rect2
// ---------------------------------------------------------------------------

/// An axis-aligned rectangle, matching Godot's `Rect2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect2 {
    /// The top-left corner position.
    pub position: Vector2,
    /// The width and height of the rectangle.
    pub size: Vector2,
}

impl Rect2 {
    /// Creates a new rectangle.
    pub const fn new(position: Vector2, size: Vector2) -> Self {
        Self { position, size }
    }

    /// Returns the end point (position + size).
    pub fn end(self) -> Vector2 {
        self.position + self.size
    }

    /// Returns `true` if `point` lies inside the rectangle.
    pub fn contains_point(self, point: Vector2) -> bool {
        let end = self.end();
        point.x >= self.position.x
            && point.y >= self.position.y
            && point.x < end.x
            && point.y < end.y
    }

    /// Returns the area.
    pub fn area(self) -> f32 {
        self.size.x * self.size.y
    }

    /// Returns `true` if this rect overlaps `other`.
    pub fn intersects(self, other: Rect2) -> bool {
        let a_end = self.end();
        let b_end = other.end();
        self.position.x < b_end.x
            && a_end.x > other.position.x
            && self.position.y < b_end.y
            && a_end.y > other.position.y
    }
}

// ---------------------------------------------------------------------------
// Transform2D
// ---------------------------------------------------------------------------

/// A 2D affine transform (2×3 matrix), matching Godot's `Transform2D`.
///
/// Stored as three column vectors: `x` (right), `y` (down), `origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    /// The X basis vector (right column).
    pub x: Vector2,
    /// The Y basis vector (down column).
    pub y: Vector2,
    /// The translation origin.
    pub origin: Vector2,
}

impl Transform2D {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        x: Vector2::new(1.0, 0.0),
        y: Vector2::new(0.0, 1.0),
        origin: Vector2::ZERO,
    };

    /// Applies this transform to a point.
    pub fn xform(self, p: Vector2) -> Vector2 {
        Vector2::new(
            self.x.x * p.x + self.y.x * p.y + self.origin.x,
            self.x.y * p.x + self.y.y * p.y + self.origin.y,
        )
    }

    /// Creates a translation-only transform.
    pub fn translated(offset: Vector2) -> Self {
        Self {
            origin: offset,
            ..Self::IDENTITY
        }
    }

    /// Creates a rotation transform (angle in radians).
    pub fn rotated(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            x: Vector2::new(c, s),
            y: Vector2::new(-s, c),
            origin: Vector2::ZERO,
        }
    }

    /// Creates a scale transform.
    pub fn scaled(scale: Vector2) -> Self {
        Self {
            x: Vector2::new(scale.x, 0.0),
            y: Vector2::new(0.0, scale.y),
            origin: Vector2::ZERO,
        }
    }

    /// Returns the inverse of this affine transform.
    ///
    /// Assumes the transform is invertible (non-zero determinant).
    pub fn affine_inverse(self) -> Self {
        let det = self.x.x * self.y.y - self.x.y * self.y.x;
        // Invert the 2x2 basis.
        let inv_det = 1.0 / det;
        let ix = Vector2::new(self.y.y * inv_det, -self.x.y * inv_det);
        let iy = Vector2::new(-self.y.x * inv_det, self.x.x * inv_det);
        // Invert the origin: -inv_basis * origin
        let io = Vector2::new(
            -(ix.x * self.origin.x + iy.x * self.origin.y),
            -(ix.y * self.origin.x + iy.y * self.origin.y),
        );
        Self {
            x: ix,
            y: iy,
            origin: io,
        }
    }
}

impl Mul for Transform2D {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            x: Vector2::new(
                self.x.x * rhs.x.x + self.y.x * rhs.x.y,
                self.x.y * rhs.x.x + self.y.y * rhs.x.y,
            ),
            y: Vector2::new(
                self.x.x * rhs.y.x + self.y.x * rhs.y.y,
                self.x.y * rhs.y.x + self.y.y * rhs.y.y,
            ),
            origin: self.xform(rhs.origin),
        }
    }
}

// ---------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------

/// An RGBA color with `f32` components in [0, 1], matching Godot's `Color`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// The red channel.
    pub r: f32,
    /// The green channel.
    pub g: f32,
    /// The blue channel.
    pub b: f32,
    /// The alpha channel (opacity).
    pub a: f32,
}

impl Color {
    /// Opaque white `(1, 1, 1, 1)`.
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    /// Opaque black `(0, 0, 0, 1)`.
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    /// Fully transparent black `(0, 0, 0, 0)`.
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// Creates a new color.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates an opaque color (alpha = 1).
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Linear interpolation between colors.
    pub fn lerp(self, to: Self, t: f32) -> Self {
        Self::new(
            self.r + (to.r - self.r) * t,
            self.g + (to.g - self.g) * t,
            self.b + (to.b - self.b) * t,
            self.a + (to.a - self.a) * t,
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn vector2_basics() {
        let v = Vector2::new(3.0, 4.0);
        assert!(approx_eq(v.length(), 5.0));

        let n = v.normalized();
        assert!(approx_eq(n.length(), 1.0));
    }

    #[test]
    fn vector2_arithmetic() {
        let a = Vector2::new(1.0, 2.0);
        let b = Vector2::new(3.0, 4.0);
        assert_eq!(a + b, Vector2::new(4.0, 6.0));
        assert_eq!(b - a, Vector2::new(2.0, 2.0));
        assert_eq!(a * 2.0, Vector2::new(2.0, 4.0));
    }

    #[test]
    fn vector2_dot_cross() {
        let a = Vector2::new(1.0, 0.0);
        let b = Vector2::new(0.0, 1.0);
        assert!(approx_eq(a.dot(b), 0.0));
        assert!(approx_eq(a.cross(b), 1.0));
    }

    #[test]
    fn vector3_cross_product() {
        let x = Vector3::new(1.0, 0.0, 0.0);
        let y = Vector3::new(0.0, 1.0, 0.0);
        let z = x.cross(y);
        assert!(approx_eq(z.x, 0.0));
        assert!(approx_eq(z.y, 0.0));
        assert!(approx_eq(z.z, 1.0));
    }

    #[test]
    fn rect2_contains() {
        let r = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        assert!(r.contains_point(Vector2::new(5.0, 5.0)));
        assert!(!r.contains_point(Vector2::new(10.0, 10.0)));
        assert!(!r.contains_point(Vector2::new(-1.0, 5.0)));
    }

    #[test]
    fn rect2_intersects() {
        let a = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        let b = Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(10.0, 10.0));
        let c = Rect2::new(Vector2::new(20.0, 20.0), Vector2::new(5.0, 5.0));
        assert!(a.intersects(b));
        assert!(!a.intersects(c));
    }

    #[test]
    fn transform2d_identity() {
        let p = Vector2::new(3.0, 7.0);
        assert_eq!(Transform2D::IDENTITY.xform(p), p);
    }

    #[test]
    fn transform2d_translation() {
        let t = Transform2D::translated(Vector2::new(10.0, 20.0));
        let p = t.xform(Vector2::new(1.0, 2.0));
        assert!(approx_eq(p.x, 11.0));
        assert!(approx_eq(p.y, 22.0));
    }

    #[test]
    fn transform2d_rotation_90() {
        let t = Transform2D::rotated(std::f32::consts::FRAC_PI_2);
        let p = t.xform(Vector2::new(1.0, 0.0));
        assert!(approx_eq(p.x, 0.0));
        assert!(approx_eq(p.y, 1.0));
    }

    #[test]
    fn transform2d_compose() {
        let t = Transform2D::translated(Vector2::new(5.0, 0.0));
        let r = Transform2D::rotated(std::f32::consts::FRAC_PI_2);
        let combined = t * r;
        let p = combined.xform(Vector2::ZERO);
        // Origin of r is (0,0), translated by t → (5,0)
        assert!(approx_eq(p.x, 5.0));
        assert!(approx_eq(p.y, 0.0));
    }

    #[test]
    fn color_lerp() {
        let c = Color::BLACK.lerp(Color::WHITE, 0.5);
        assert!(approx_eq(c.r, 0.5));
        assert!(approx_eq(c.g, 0.5));
        assert!(approx_eq(c.b, 0.5));
        assert!(approx_eq(c.a, 1.0));
    }

    // -- Vector2 edge cases -------------------------------------------------

    #[test]
    fn vector2_normalize_zero_returns_zero() {
        let v = Vector2::ZERO.normalized();
        assert_eq!(v, Vector2::ZERO);
    }

    #[test]
    fn vector2_normalize_very_small_returns_zero() {
        let v = Vector2::new(1e-20, 1e-20).normalized();
        assert_eq!(v, Vector2::ZERO);
    }

    #[test]
    fn vector2_lerp_at_zero_returns_self() {
        let a = Vector2::new(1.0, 2.0);
        let b = Vector2::new(10.0, 20.0);
        let r = a.lerp(b, 0.0);
        assert!(approx_eq(r.x, a.x));
        assert!(approx_eq(r.y, a.y));
    }

    #[test]
    fn vector2_lerp_at_one_returns_target() {
        let a = Vector2::new(1.0, 2.0);
        let b = Vector2::new(10.0, 20.0);
        let r = a.lerp(b, 1.0);
        assert!(approx_eq(r.x, b.x));
        assert!(approx_eq(r.y, b.y));
    }

    #[test]
    fn vector2_lerp_at_half() {
        let a = Vector2::new(0.0, 0.0);
        let b = Vector2::new(10.0, 20.0);
        let r = a.lerp(b, 0.5);
        assert!(approx_eq(r.x, 5.0));
        assert!(approx_eq(r.y, 10.0));
    }

    #[test]
    fn vector2_negation() {
        let v = Vector2::new(3.0, -4.0);
        let neg = -v;
        assert!(approx_eq(neg.x, -3.0));
        assert!(approx_eq(neg.y, 4.0));
    }

    #[test]
    fn vector2_length_squared() {
        let v = Vector2::new(3.0, 4.0);
        assert!(approx_eq(v.length_squared(), 25.0));
    }

    // -- Vector3 edge cases -------------------------------------------------

    #[test]
    fn vector3_normalize_zero_returns_zero() {
        let v = Vector3::ZERO.normalized();
        assert_eq!(v, Vector3::ZERO);
    }

    #[test]
    fn vector3_normalize_very_small_returns_zero() {
        let v = Vector3::new(1e-20, 1e-20, 1e-20).normalized();
        assert_eq!(v, Vector3::ZERO);
    }

    #[test]
    fn vector3_lerp_at_zero_returns_self() {
        let a = Vector3::new(1.0, 2.0, 3.0);
        let b = Vector3::new(10.0, 20.0, 30.0);
        let r = a.lerp(b, 0.0);
        assert!(approx_eq(r.x, a.x));
        assert!(approx_eq(r.y, a.y));
        assert!(approx_eq(r.z, a.z));
    }

    #[test]
    fn vector3_lerp_at_one_returns_target() {
        let a = Vector3::new(1.0, 2.0, 3.0);
        let b = Vector3::new(10.0, 20.0, 30.0);
        let r = a.lerp(b, 1.0);
        assert!(approx_eq(r.x, b.x));
        assert!(approx_eq(r.y, b.y));
        assert!(approx_eq(r.z, b.z));
    }

    #[test]
    fn vector3_basics() {
        let v = Vector3::new(1.0, 2.0, 2.0);
        assert!(approx_eq(v.length(), 3.0));
        let n = v.normalized();
        assert!(approx_eq(n.length(), 1.0));
    }

    #[test]
    fn vector3_arithmetic() {
        let a = Vector3::new(1.0, 2.0, 3.0);
        let b = Vector3::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vector3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vector3::new(3.0, 3.0, 3.0));
        assert_eq!(a * 2.0, Vector3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn vector3_negation() {
        let v = Vector3::new(1.0, -2.0, 3.0);
        let neg = -v;
        assert!(approx_eq(neg.x, -1.0));
        assert!(approx_eq(neg.y, 2.0));
        assert!(approx_eq(neg.z, -3.0));
    }

    #[test]
    fn vector3_dot_product() {
        let a = Vector3::new(1.0, 0.0, 0.0);
        let b = Vector3::new(0.0, 1.0, 0.0);
        assert!(approx_eq(a.dot(b), 0.0));
        assert!(approx_eq(a.dot(a), 1.0));
    }

    // -- Rect2 edge cases ---------------------------------------------------

    #[test]
    fn rect2_zero_size_contains_nothing() {
        let r = Rect2::new(Vector2::new(5.0, 5.0), Vector2::ZERO);
        assert!(!r.contains_point(Vector2::new(5.0, 5.0)));
        assert!(approx_eq(r.area(), 0.0));
    }

    #[test]
    fn rect2_negative_size_contains_nothing() {
        let r = Rect2::new(Vector2::new(10.0, 10.0), Vector2::new(-5.0, -5.0));
        assert!(!r.contains_point(Vector2::new(7.0, 7.0)));
        assert!(!r.contains_point(Vector2::new(10.0, 10.0)));
    }

    #[test]
    fn rect2_negative_size_area() {
        let r = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(-5.0, -5.0));
        assert!(approx_eq(r.area(), 25.0)); // -5 * -5 = 25
    }

    #[test]
    fn rect2_end_point() {
        let r = Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
        let end = r.end();
        assert!(approx_eq(end.x, 4.0));
        assert!(approx_eq(end.y, 6.0));
    }

    #[test]
    fn rect2_touching_rects_do_not_intersect() {
        let a = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        let b = Rect2::new(Vector2::new(10.0, 0.0), Vector2::new(10.0, 10.0));
        assert!(!a.intersects(b));
    }

    #[test]
    fn rect2_contains_point_boundary() {
        let r = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        assert!(r.contains_point(Vector2::new(0.0, 0.0)));
        assert!(!r.contains_point(Vector2::new(10.0, 0.0)));
        assert!(!r.contains_point(Vector2::new(0.0, 10.0)));
    }

    // -- Transform2D edge cases ---------------------------------------------

    #[test]
    fn transform2d_scale() {
        let t = Transform2D::scaled(Vector2::new(2.0, 3.0));
        let p = t.xform(Vector2::new(5.0, 10.0));
        assert!(approx_eq(p.x, 10.0));
        assert!(approx_eq(p.y, 30.0));
    }

    #[test]
    fn transform2d_identity_mul_identity() {
        let result = Transform2D::IDENTITY * Transform2D::IDENTITY;
        assert_eq!(result, Transform2D::IDENTITY);
    }

    #[test]
    fn transform2d_rotation_360_returns_near_identity() {
        let t = Transform2D::rotated(2.0 * std::f32::consts::PI);
        let p = t.xform(Vector2::new(1.0, 0.0));
        assert!(approx_eq(p.x, 1.0));
        assert!(approx_eq(p.y, 0.0));
    }

    #[test]
    fn transform2d_translate_then_scale() {
        let t = Transform2D::translated(Vector2::new(10.0, 0.0));
        let s = Transform2D::scaled(Vector2::new(2.0, 2.0));
        let combined = t * s;
        let p = combined.xform(Vector2::new(1.0, 0.0));
        assert!(approx_eq(p.x, 12.0)); // 1*2 + 10
        assert!(approx_eq(p.y, 0.0));
    }

    // -- Color edge cases ---------------------------------------------------

    #[test]
    fn color_lerp_at_zero_returns_self() {
        let a = Color::new(0.1, 0.2, 0.3, 0.4);
        let b = Color::new(0.9, 0.8, 0.7, 0.6);
        let c = a.lerp(b, 0.0);
        assert!(approx_eq(c.r, a.r));
        assert!(approx_eq(c.g, a.g));
        assert!(approx_eq(c.b, a.b));
        assert!(approx_eq(c.a, a.a));
    }

    #[test]
    fn color_lerp_at_one_returns_target() {
        let a = Color::new(0.1, 0.2, 0.3, 0.4);
        let b = Color::new(0.9, 0.8, 0.7, 0.6);
        let c = a.lerp(b, 1.0);
        assert!(approx_eq(c.r, b.r));
        assert!(approx_eq(c.g, b.g));
        assert!(approx_eq(c.b, b.b));
        assert!(approx_eq(c.a, b.a));
    }

    #[test]
    fn color_rgb_has_alpha_one() {
        let c = Color::rgb(0.5, 0.6, 0.7);
        assert!(approx_eq(c.a, 1.0));
    }

    #[test]
    fn color_constants() {
        assert_eq!(Color::WHITE, Color::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(Color::BLACK, Color::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(Color::TRANSPARENT, Color::new(0.0, 0.0, 0.0, 0.0));
    }

    // -- Vector2i -----------------------------------------------------------

    #[test]
    fn vector2i_add() {
        let a = Vector2i::new(1, 2);
        let b = Vector2i::new(3, 4);
        assert_eq!(a + b, Vector2i::new(4, 6));
    }

    #[test]
    fn vector2i_sub() {
        let a = Vector2i::new(5, 7);
        let b = Vector2i::new(2, 3);
        assert_eq!(a - b, Vector2i::new(3, 4));
    }

    #[test]
    fn vector2i_neg() {
        let v = Vector2i::new(3, -4);
        assert_eq!(-v, Vector2i::new(-3, 4));
    }

    #[test]
    fn vector2i_eq_and_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Vector2i::new(1, 2));
        set.insert(Vector2i::new(1, 2));
        set.insert(Vector2i::new(3, 4));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn vector2i_constants() {
        assert_eq!(Vector2i::ZERO, Vector2i::new(0, 0));
        assert_eq!(Vector2i::ONE, Vector2i::new(1, 1));
        assert_eq!(Vector2i::UP, Vector2i::new(0, -1));
        assert_eq!(Vector2i::DOWN, Vector2i::new(0, 1));
    }
}
