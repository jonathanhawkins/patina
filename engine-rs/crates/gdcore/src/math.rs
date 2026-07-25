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
// Vector3i
// ---------------------------------------------------------------------------

/// A 3D vector with `i32` components, matching Godot's `Vector3i`.
///
/// The integer counterpart of [`Vector3`], for grid coordinates (`GridMap`
/// cells, voxel indices) where rounding through `f32` would lose the exact
/// cell. Added so a `.tscn`'s `Vector3i(...)` literal keeps its integer type
/// instead of being coerced to a float vector (pat-nzgdu.78).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vector3i {
    /// The X component.
    pub x: i32,
    /// The Y component.
    pub y: i32,
    /// The Z component.
    pub z: i32,
}

impl Vector3i {
    /// The zero vector `(0, 0, 0)`.
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };
    /// The one vector `(1, 1, 1)`.
    pub const ONE: Self = Self { x: 1, y: 1, z: 1 };
    /// The up direction `(0, 1, 0)` — Godot's 3D up, unlike the Y-down 2D
    /// convention used by [`Vector2i::UP`].
    pub const UP: Self = Self { x: 0, y: 1, z: 0 };
    /// The down direction `(0, -1, 0)`.
    pub const DOWN: Self = Self { x: 0, y: -1, z: 0 };
    /// The left direction `(-1, 0, 0)`.
    pub const LEFT: Self = Self { x: -1, y: 0, z: 0 };
    /// The right direction `(1, 0, 0)`.
    pub const RIGHT: Self = Self { x: 1, y: 0, z: 0 };
    /// The forward direction `(0, 0, -1)`.
    pub const FORWARD: Self = Self { x: 0, y: 0, z: -1 };
    /// The back direction `(0, 0, 1)`.
    pub const BACK: Self = Self { x: 0, y: 0, z: 1 };

    /// Creates a new integer vector.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

impl Add for Vector3i {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vector3i {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Neg for Vector3i {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

// ---------------------------------------------------------------------------
// Vector2
// ---------------------------------------------------------------------------

/// A 2D vector with `f32` components, matching Godot's `Vector2`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
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

    /// Returns a vector with each component replaced by its absolute value.
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
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

    /// Returns the center point (`position + size / 2`).
    ///
    /// Godot: `Rect2.get_center` (`core/math/rect2.h`).
    pub fn get_center(self) -> Vector2 {
        self.position + self.size * 0.5
    }

    /// Returns `true` if the rect has a strictly positive area.
    ///
    /// Godot: `Rect2.has_area`.
    pub fn has_area(self) -> bool {
        self.size.x > 0.0 && self.size.y > 0.0
    }

    /// Returns `true` if `other` lies entirely within this rect.
    ///
    /// Godot: `Rect2.encloses`.
    pub fn encloses(self, other: Rect2) -> bool {
        other.position.x >= self.position.x
            && other.position.y >= self.position.y
            && other.end().x <= self.end().x
            && other.end().y <= self.end().y
    }

    /// Returns a copy grown by `amount` on all four sides.
    ///
    /// Godot: `Rect2.grow` — the position moves out by `amount` and the size
    /// grows by `amount * 2` on each axis. A negative amount shrinks.
    pub fn grow(self, amount: f32) -> Self {
        self.grow_individual(amount, amount, amount, amount)
    }

    /// Returns a copy grown by a separate amount on each side.
    ///
    /// Godot: `Rect2.grow_individual`.
    pub fn grow_individual(self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            position: Vector2::new(self.position.x - left, self.position.y - top),
            size: Vector2::new(self.size.x + left + right, self.size.y + top + bottom),
        }
    }

    /// Returns a copy grown on a single side, identified by the `@GlobalScope`
    /// `Side` enum (`SIDE_LEFT` 0, `SIDE_TOP` 1, `SIDE_RIGHT` 2,
    /// `SIDE_BOTTOM` 3). An out-of-range side leaves the rect unchanged, which
    /// is what Godot's `grow_side` does for a side that matches nothing.
    ///
    /// Godot: `Rect2.grow_side`.
    pub fn grow_side(self, side: i64, amount: f32) -> Self {
        self.grow_individual(
            if side == 0 { amount } else { 0.0 },
            if side == 1 { amount } else { 0.0 },
            if side == 2 { amount } else { 0.0 },
            if side == 3 { amount } else { 0.0 },
        )
    }

    /// Returns the smallest rect containing both this rect and `other`.
    ///
    /// Godot: `Rect2.merge`.
    pub fn merge(self, other: Rect2) -> Self {
        let position = Vector2::new(
            self.position.x.min(other.position.x),
            self.position.y.min(other.position.y),
        );
        let end = Vector2::new(
            self.end().x.max(other.end().x),
            self.end().y.max(other.end().y),
        );
        Self {
            position,
            size: end - position,
        }
    }

    /// Returns a copy of this rect expanded to include `point`.
    ///
    /// Godot: `Rect2.expand`.
    pub fn expand(self, point: Vector2) -> Self {
        self.merge(Rect2::new(point, Vector2::ZERO))
    }

    /// Returns the overlapping region of two rects, or a zero rect when they do
    /// not overlap.
    ///
    /// Godot: `Rect2.intersection`.
    pub fn intersection(self, other: Rect2) -> Self {
        if !self.intersects(other) {
            return Self::new(Vector2::ZERO, Vector2::ZERO);
        }
        let position = Vector2::new(
            self.position.x.max(other.position.x),
            self.position.y.max(other.position.y),
        );
        let end = Vector2::new(
            self.end().x.min(other.end().x),
            self.end().y.min(other.end().y),
        );
        Self {
            position,
            size: end - position,
        }
    }

    /// Returns an equivalent rect with a non-negative size.
    ///
    /// Godot: `Rect2.abs`.
    pub fn abs(self) -> Self {
        Self {
            position: Vector2::new(
                self.position.x + self.size.x.min(0.0),
                self.position.y + self.size.y.min(0.0),
            ),
            size: self.size.abs(),
        }
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

    /// Applies only the basis (rotation + scale) part of this transform to a vector.
    ///
    /// Unlike [`xform`](Self::xform), this does **not** add the translation origin.
    /// Use this for transforming directions, sizes, and offsets rather than positions.
    pub fn basis_xform(self, v: Vector2) -> Vector2 {
        Vector2::new(
            self.x.x * v.x + self.y.x * v.y,
            self.x.y * v.x + self.y.y * v.y,
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

    /// Builds a transform from a rotation (radians) and a translation.
    ///
    /// Godot: `Transform2D(rotation, position)`
    /// (`core/math/transform_2d.cpp:97`).
    pub fn from_rotation_origin(rotation: f32, origin: Vector2) -> Self {
        let (s, c) = rotation.sin_cos();
        Self {
            x: Vector2::new(c, s),
            y: Vector2::new(-s, c),
            origin,
        }
    }

    /// Builds a transform from rotation, scale, skew and translation.
    ///
    /// Godot: `Transform2D(rotation, scale, skew, position)`
    /// (`core/math/transform_2d.cpp:107`).
    pub fn from_rotation_scale_skew_origin(
        rotation: f32,
        scale: Vector2,
        skew: f32,
        origin: Vector2,
    ) -> Self {
        Self {
            x: Vector2::new(rotation.cos() * scale.x, rotation.sin() * scale.x),
            y: Vector2::new(
                -(rotation + skew).sin() * scale.y,
                (rotation + skew).cos() * scale.y,
            ),
            origin,
        }
    }

    /// Returns the determinant of the 2×2 basis.
    ///
    /// Godot: `Transform2D.determinant`.
    pub fn determinant(self) -> f32 {
        self.x.x * self.y.y - self.x.y * self.y.x
    }

    /// Returns the rotation of the basis, in radians.
    ///
    /// Godot: `Transform2D.get_rotation` (`atan2(columns[0].y, columns[0].x)`).
    pub fn get_rotation(self) -> f32 {
        self.x.y.atan2(self.x.x)
    }

    /// Returns the scale of the basis. The Y component carries the sign of the
    /// determinant, so a mirrored transform reports a negative Y scale.
    ///
    /// Godot: `Transform2D.get_scale` (`core/math/transform_2d.cpp:115`).
    pub fn get_scale(self) -> Vector2 {
        let det_sign = if self.determinant() < 0.0 { -1.0 } else { 1.0 };
        Vector2::new(self.x.length(), det_sign * self.y.length())
    }

    /// Returns the skew of the basis, in radians — the deviation of the Y axis
    /// from perpendicular to the X axis.
    ///
    /// Godot: `Transform2D.get_skew` (`core/math/transform_2d.cpp:72`).
    pub fn get_skew(self) -> f32 {
        let det_sign = if self.determinant() < 0.0 { -1.0 } else { 1.0 };
        let dot = self.x.normalized().dot(self.y.normalized() * det_sign);
        dot.clamp(-1.0, 1.0).acos() - std::f32::consts::FRAC_PI_2
    }

    /// Returns a copy whose basis vectors are unit-length and perpendicular
    /// (Gram-Schmidt), leaving the origin untouched.
    ///
    /// Godot: `Transform2D.orthonormalized`.
    pub fn orthonormalized(self) -> Self {
        let x = self.x.normalized();
        let y = (self.y - x * x.dot(self.y)).normalized();
        Self {
            x,
            y,
            origin: self.origin,
        }
    }

    /// Interpolates towards `to` by `weight`, decomposing both transforms into
    /// rotation / scale / skew / origin so the result stays a valid affine
    /// transform (a component-wise matrix lerp would not).
    ///
    /// Godot: `Transform2D.interpolate_with`
    /// (`core/math/transform_2d.cpp:262`).
    pub fn interpolate_with(self, to: Transform2D, weight: f32) -> Self {
        Self::from_rotation_scale_skew_origin(
            lerp_angle(self.get_rotation(), to.get_rotation(), weight),
            self.get_scale().lerp(to.get_scale(), weight),
            lerp_angle(self.get_skew(), to.get_skew(), weight),
            self.origin.lerp(to.origin, weight),
        )
    }
}

/// Composes two transforms: `a * b` applies `b` first, then `a`, matching
/// Godot's `Transform2D::operator*` (`core/math/transform_2d.cpp:198`) and the
/// convention already used by [`Transform3D`](crate::math3d::Transform3D).
impl Mul for Transform2D {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            x: self.basis_xform(rhs.x),
            y: self.basis_xform(rhs.y),
            origin: self.xform(rhs.origin),
        }
    }
}

/// Interpolates between two angles along the shortest arc, wrapping across
/// ±π rather than sweeping the long way round.
///
/// Godot: `Math::lerp_angle` (`core/math/math_funcs.h`).
pub fn lerp_angle(from: f32, to: f32, weight: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let difference = (to - from) % tau;
    let distance = (2.0 * difference) % tau - difference;
    from + distance * weight
}

// NOTE: a second `impl Mul for Transform2D` used to sit here, spelling the same
// composition out component-by-component. Two impls of one trait for one type is
// E0119 — it broke every crate that depends on `gdcore`, i.e. the whole
// workspace. The surviving impl above is that arithmetic expressed through
// `basis_xform`/`xform`: for each basis column `basis_xform(c)` expands to
// exactly `(x.x*c.x + y.x*c.y, x.y*c.x + y.y*c.y)`, and the origin term is
// identical, so nothing changed behaviourally when this copy went away.

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

    /// Converts this color from sRGB gamma space to linear space.
    ///
    /// Matches Godot's `Color::srgb_to_linear` (the standard sRGB EOTF): a
    /// linear segment below 0.04045 and a `pow(_, 2.4)` curve above. Rendering
    /// math (light transport, gradient mixing, tonemapping) is done in linear
    /// space; authoring colors (`source_color` in Godot) are sRGB and must be
    /// converted first. Alpha is left unchanged. The RGB channels are converted
    /// even outside `[0, 1]` (HDR authoring), so the transform is exact and
    /// invertible via [`linear_to_srgb`](Self::linear_to_srgb).
    pub fn srgb_to_linear(self) -> Self {
        Self::new(
            srgb_to_linear_channel(self.r),
            srgb_to_linear_channel(self.g),
            srgb_to_linear_channel(self.b),
            self.a,
        )
    }

    /// Converts this color from linear space back to sRGB gamma space.
    ///
    /// Matches Godot's `Color::linear_to_srgb` (the standard sRGB OETF), the
    /// inverse of [`srgb_to_linear`](Self::srgb_to_linear). This is the final
    /// step of the camera pipeline: after linear rendering + tonemapping the
    /// image is encoded to sRGB for display. Alpha is left unchanged.
    pub fn linear_to_srgb(self) -> Self {
        Self::new(
            linear_to_srgb_channel(self.r),
            linear_to_srgb_channel(self.g),
            linear_to_srgb_channel(self.b),
            self.a,
        )
    }
}

/// sRGB→linear transfer function for a single channel (Godot's exact curve).
#[inline]
pub fn srgb_to_linear_channel(c: f32) -> f32 {
    if c < 0.04045 {
        c * (1.0 / 12.92)
    } else {
        ((c + 0.055) * (1.0 / 1.055)).powf(2.4)
    }
}

/// linear→sRGB transfer function for a single channel (Godot's exact curve).
#[inline]
pub fn linear_to_srgb_channel(c: f32) -> f32 {
    if c < 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
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
    fn srgb_linear_endpoints_and_known_values() {
        // 0 and 1 are fixed points of both directions.
        assert!(approx_eq(srgb_to_linear_channel(0.0), 0.0));
        assert!(approx_eq(srgb_to_linear_channel(1.0), 1.0));
        assert!(approx_eq(linear_to_srgb_channel(0.0), 0.0));
        assert!(approx_eq(linear_to_srgb_channel(1.0), 1.0));

        // Godot's ProceduralSkyMaterial default sky_top_color 0.385 (sRGB) is
        // ~0.12260 linear (matches Godot's Color(0.385,...).srgb_to_linear()).
        assert!((srgb_to_linear_channel(0.385) - 0.122_60).abs() < 1e-4);
        // Mid-gray 0.5 sRGB → ~0.2140 linear.
        assert!((srgb_to_linear_channel(0.5) - 0.214_04).abs() < 1e-4);
        // The linear segment near black: 0.04 sRGB (< 0.04045) is a straight
        // 1/12.92 slope.
        assert!((srgb_to_linear_channel(0.04) - 0.04 / 12.92).abs() < 1e-7);
    }

    #[test]
    fn srgb_linear_round_trip_is_identity() {
        // The two transfer functions are exact inverses across the range,
        // including HDR values above 1.0 (used for bright authoring colors).
        for &c in &[0.0f32, 0.002, 0.05, 0.25, 0.385, 0.5, 0.75, 1.0, 2.5, 12.0] {
            let round = linear_to_srgb_channel(srgb_to_linear_channel(c));
            assert!(
                (round - c).abs() < 1e-5,
                "srgb→linear→srgb not identity for {c}: got {round}"
            );
        }
    }

    #[test]
    fn color_srgb_linear_preserves_alpha() {
        let c = Color::new(0.385, 0.5, 0.75, 0.42);
        let lin = c.srgb_to_linear();
        assert_eq!(lin.a, 0.42, "alpha must pass through unchanged");
        let back = lin.linear_to_srgb();
        assert!(approx_eq(back.r, 0.385) && approx_eq(back.g, 0.5) && approx_eq(back.b, 0.75));
        assert_eq!(back.a, 0.42);
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

    // -- Rect2 region algebra (pat-0q2kx.50) --------------------------------

    fn v2_approx_eq(a: Vector2, b: Vector2) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
    }

    /// Looser than [`approx_eq`] for values that survive a decomposition round
    /// trip (`atan2` of scaled sines, lengths of rotated columns), where f32
    /// error accumulates past 1e-6.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn rect2_grow_moves_every_side_out() {
        let r = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)).grow(2.0);
        assert_eq!(r.position, Vector2::new(-2.0, -2.0));
        assert_eq!(r.size, Vector2::new(14.0, 14.0));
        // A negative amount shrinks by the same rule.
        let shrunk = r.grow(-2.0);
        assert_eq!(shrunk.position, Vector2::new(0.0, 0.0));
        assert_eq!(shrunk.size, Vector2::new(10.0, 10.0));
    }

    #[test]
    fn rect2_grow_side_touches_only_that_side() {
        let r = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 4.0));
        // SIDE_LEFT = 0, SIDE_TOP = 1, SIDE_RIGHT = 2, SIDE_BOTTOM = 3.
        assert_eq!(r.grow_side(0, 1.0).position, Vector2::new(-1.0, 0.0));
        assert_eq!(r.grow_side(2, 1.0).position, r.position);
        assert_eq!(r.grow_side(2, 1.0).size, Vector2::new(5.0, 4.0));
        assert_eq!(r.grow_side(3, 1.0).size, Vector2::new(4.0, 5.0));
    }

    #[test]
    fn rect2_merge_intersection_and_encloses() {
        let a = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        let b = Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(10.0, 10.0));
        let merged = a.merge(b);
        assert_eq!(merged.position, Vector2::new(0.0, 0.0));
        assert_eq!(merged.size, Vector2::new(15.0, 15.0));
        assert!(merged.encloses(a) && merged.encloses(b));

        let overlap = a.intersection(b);
        assert_eq!(overlap.position, Vector2::new(5.0, 5.0));
        assert_eq!(overlap.size, Vector2::new(5.0, 5.0));

        // Disjoint rects intersect in nothing.
        let far = Rect2::new(Vector2::new(100.0, 100.0), Vector2::new(1.0, 1.0));
        assert_eq!(a.intersection(far).size, Vector2::ZERO);
        assert!(!a.encloses(far));
    }

    #[test]
    fn rect2_abs_and_expand() {
        let negative = Rect2::new(Vector2::new(10.0, 10.0), Vector2::new(-4.0, -4.0)).abs();
        assert_eq!(negative.position, Vector2::new(6.0, 6.0));
        assert_eq!(negative.size, Vector2::new(4.0, 4.0));

        let r = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(2.0, 2.0));
        let expanded = r.expand(Vector2::new(-3.0, 5.0));
        assert_eq!(expanded.position, Vector2::new(-3.0, 0.0));
        assert_eq!(expanded.end(), Vector2::new(2.0, 5.0));
    }

    #[test]
    fn rect2_center_and_area_flags() {
        let r = Rect2::new(Vector2::new(2.0, 4.0), Vector2::new(4.0, 8.0));
        assert_eq!(r.get_center(), Vector2::new(4.0, 8.0));
        assert!(r.has_area());
        assert!(!Rect2::new(Vector2::ZERO, Vector2::ZERO).has_area());
    }

    // -- Transform2D composition & decomposition (pat-0q2kx.50) -------------

    /// `a * b` must apply `b` first — the whole point of the operator, and what
    /// `rotated`/`scaled`/`translated` are expressed in terms of.
    #[test]
    fn transform2d_mul_applies_right_hand_side_first() {
        let translate = Transform2D::translated(Vector2::new(10.0, 0.0));
        let scale = Transform2D::scaled(Vector2::new(2.0, 2.0));
        let p = Vector2::new(1.0, 0.0);

        assert!(v2_approx_eq(
            (scale * translate).xform(p),
            scale.xform(translate.xform(p))
        ));
        // Scaling after translating reaches 22; translating after scaling, 12.
        assert!(v2_approx_eq(
            (scale * translate).xform(p),
            Vector2::new(22.0, 0.0)
        ));
        assert!(v2_approx_eq(
            (translate * scale).xform(p),
            Vector2::new(12.0, 0.0)
        ));
    }

    #[test]
    fn transform2d_identity_is_the_multiplicative_unit() {
        let t = Transform2D::from_rotation_origin(0.7, Vector2::new(3.0, -2.0));
        let left = Transform2D::IDENTITY * t;
        let right = t * Transform2D::IDENTITY;
        for got in [left, right] {
            assert!(v2_approx_eq(got.x, t.x));
            assert!(v2_approx_eq(got.y, t.y));
            assert!(v2_approx_eq(got.origin, t.origin));
        }
    }

    #[test]
    fn transform2d_decomposition_round_trips() {
        let origin = Vector2::new(4.0, -1.0);
        let t =
            Transform2D::from_rotation_scale_skew_origin(0.5, Vector2::new(2.0, 3.0), 0.0, origin);
        assert!(close(t.get_rotation(), 0.5));
        assert!(close(t.get_scale().x, 2.0) && close(t.get_scale().y, 3.0));
        assert!(t.get_skew().abs() < 1e-4);
        assert_eq!(t.origin, origin);
        // An unskewed, unscaled basis is already orthonormal.
        let unit = Transform2D::from_rotation_origin(0.5, origin);
        let unit_scale = unit.orthonormalized().get_scale();
        assert!(close(unit_scale.x, 1.0) && close(unit_scale.y, 1.0));
        assert!(close(t.determinant(), 6.0));
    }

    #[test]
    fn transform2d_interpolate_with_hits_both_ends() {
        let a = Transform2D::from_rotation_origin(0.0, Vector2::new(10.0, 5.0));
        let b = Transform2D::from_rotation_origin(1.0, Vector2::new(20.0, 5.0));
        assert!(v2_approx_eq(a.interpolate_with(b, 0.0).origin, a.origin));
        assert!(v2_approx_eq(a.interpolate_with(b, 1.0).origin, b.origin));
        assert!(close(a.interpolate_with(b, 1.0).get_rotation(), 1.0));
        // Halfway is halfway in both position and angle.
        let mid = a.interpolate_with(b, 0.5);
        assert!(v2_approx_eq(mid.origin, Vector2::new(15.0, 5.0)));
        assert!(close(mid.get_rotation(), 0.5));
    }

    #[test]
    fn lerp_angle_takes_the_short_way_round() {
        use std::f32::consts::PI;
        // From just under +pi to just over -pi: the short arc crosses the wrap.
        let from = PI - 0.1;
        let to = -PI + 0.1;
        let mid = lerp_angle(from, to, 0.5);
        // The midpoint sits at ±pi, not back at 0.
        assert!(
            mid.abs() > PI - 0.05,
            "expected a wrap-crossing lerp, got {mid}"
        );
        assert!(approx_eq(lerp_angle(0.2, 0.8, 0.5), 0.5));
    }
}
