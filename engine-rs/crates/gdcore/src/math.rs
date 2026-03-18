//! Core math types and utilities.
//!
//! Provides the fundamental math primitives used throughout the engine:
//! vectors, transforms, rects, and colors. These mirror Godot's built-in
//! math types and follow the same conventions (Y-up, right-handed for 3D).

use std::ops::{Add, Mul, Neg, Sub};

// ---------------------------------------------------------------------------
// Vector2
// ---------------------------------------------------------------------------

/// A 2D vector with `f32` components, matching Godot's `Vector2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

impl Vector2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };
    pub const UP: Self = Self { x: 0.0, y: -1.0 };
    pub const DOWN: Self = Self { x: 0.0, y: 1.0 };
    pub const LEFT: Self = Self { x: -1.0, y: 0.0 };
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
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vector3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0, z: 1.0 };
    pub const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    pub const DOWN: Self = Self { x: 0.0, y: -1.0, z: 0.0 };
    pub const FORWARD: Self = Self { x: 0.0, y: 0.0, z: -1.0 };

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
    pub position: Vector2,
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
    pub x: Vector2,
    pub y: Vector2,
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

    /// Multiplies two transforms.
    pub fn mul(self, other: Self) -> Self {
        Self {
            x: Vector2::new(
                self.x.x * other.x.x + self.y.x * other.x.y,
                self.x.y * other.x.x + self.y.y * other.x.y,
            ),
            y: Vector2::new(
                self.x.x * other.y.x + self.y.x * other.y.y,
                self.x.y * other.y.x + self.y.y * other.y.y,
            ),
            origin: self.xform(other.origin),
        }
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
}

// ---------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------

/// An RGBA color with `f32` components in [0, 1], matching Godot's `Color`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

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
        let combined = t.mul(r);
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
}
