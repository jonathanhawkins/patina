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
///
/// `Default` is `(0, 0)`, which is what Godot's default-constructed `Vector2i`
/// is — and what lets [`Rect2i`] derive its own `Default` (pat-s3rrv.59).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
    /// The component-wise minimum — Godot's `Vector2i.MIN`.
    pub const MIN: Self = Self {
        x: i32::MIN,
        y: i32::MIN,
    };
    /// The component-wise maximum — Godot's `Vector2i.MAX`.
    pub const MAX: Self = Self {
        x: i32::MAX,
        y: i32::MAX,
    };

    /// Creates a new integer vector.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Component-wise minimum (Godot `Vector2i.min`).
    pub fn min_components(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// Component-wise maximum (Godot `Vector2i.max`).
    pub fn max_components(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
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
    /// The component-wise minimum — Godot's `Vector3i.MIN`.
    pub const MIN: Self = Self {
        x: i32::MIN,
        y: i32::MIN,
        z: i32::MIN,
    };
    /// The component-wise maximum — Godot's `Vector3i.MAX`.
    pub const MAX: Self = Self {
        x: i32::MAX,
        y: i32::MAX,
        z: i32::MAX,
    };

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
// Vector4i
// ---------------------------------------------------------------------------

/// A 4D vector with `i32` components, matching Godot's `Vector4i`
/// (`core/math/vector4i.h`).
///
/// One of the six Variant builtins Patina had no Rust type for at all
/// (pat-s3rrv.59). The integer counterpart of [`Vector4`], used wherever a
/// four-component index or extent must stay exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Vector4i {
    /// The X component.
    pub x: i32,
    /// The Y component.
    pub y: i32,
    /// The Z component.
    pub z: i32,
    /// The W component.
    pub w: i32,
}

impl Vector4i {
    /// The zero vector `(0, 0, 0, 0)`.
    pub const ZERO: Self = Self {
        x: 0,
        y: 0,
        z: 0,
        w: 0,
    };
    /// The one vector `(1, 1, 1, 1)`.
    pub const ONE: Self = Self {
        x: 1,
        y: 1,
        z: 1,
        w: 1,
    };
    /// The component-wise minimum — Godot's `Vector4i.MIN`.
    pub const MIN: Self = Self {
        x: i32::MIN,
        y: i32::MIN,
        z: i32::MIN,
        w: i32::MIN,
    };
    /// The component-wise maximum — Godot's `Vector4i.MAX`.
    pub const MAX: Self = Self {
        x: i32::MAX,
        y: i32::MAX,
        z: i32::MAX,
        w: i32::MAX,
    };

    /// Creates a new integer 4D vector.
    pub const fn new(x: i32, y: i32, z: i32, w: i32) -> Self {
        Self { x, y, z, w }
    }

    /// Squared length. Godot has no `length_squared` overflow guard either, so
    /// this is an `i64` to keep a large component from wrapping silently.
    pub fn length_squared(self) -> i64 {
        let (x, y, z, w) = (self.x as i64, self.y as i64, self.z as i64, self.w as i64);
        x * x + y * y + z * z + w * w
    }

    /// Euclidean length (Godot `Vector4i.length`, which returns a float).
    pub fn length(self) -> f32 {
        (self.length_squared() as f64).sqrt() as f32
    }

    /// Component-wise absolute value (Godot `Vector4i.abs`).
    pub fn abs(self) -> Self {
        Self::new(
            self.x.saturating_abs(),
            self.y.saturating_abs(),
            self.z.saturating_abs(),
            self.w.saturating_abs(),
        )
    }

    /// Component-wise sign (`-1`, `0` or `1`) — Godot `Vector4i.sign`.
    pub fn sign(self) -> Self {
        Self::new(
            self.x.signum(),
            self.y.signum(),
            self.z.signum(),
            self.w.signum(),
        )
    }

    /// Component-wise minimum (Godot `Vector4i.min`).
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
            self.w.min(other.w),
        )
    }

    /// Component-wise maximum (Godot `Vector4i.max`).
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
            self.w.max(other.w),
        )
    }

    /// Component-wise clamp between `min` and `max` (Godot `Vector4i.clamp`).
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }

    /// Euclidean distance to `to` (Godot `Vector4i.distance_to`).
    pub fn distance_to(self, to: Self) -> f32 {
        Self::new(to.x - self.x, to.y - self.y, to.z - self.z, to.w - self.w).length()
    }

    /// Squared distance to `to` (Godot `Vector4i.distance_squared_to`).
    ///
    /// ⚠️ Returns an integer, not a float — the squared length of an integer
    /// vector is exact, and Godot's own signature is `int64_t`
    /// (`core/math/vector4i.h:98`). Returning a float here would quietly lose
    /// exactness for the comparison this method exists to make cheap.
    pub fn distance_squared_to(self, to: Self) -> i64 {
        Self::new(to.x - self.x, to.y - self.y, to.z - self.z, to.w - self.w).length_squared()
    }

    /// Clamps every component between the same two scalars (Godot
    /// `Vector4i.clampi`).
    pub fn clampi(self, min: i32, max: i32) -> Self {
        Self::new(
            self.x.clamp(min, max),
            self.y.clamp(min, max),
            self.z.clamp(min, max),
            self.w.clamp(min, max),
        )
    }

    /// Component-wise minimum against a scalar (Godot `Vector4i.mini`).
    pub fn mini(self, scalar: i32) -> Self {
        Self::new(
            self.x.min(scalar),
            self.y.min(scalar),
            self.z.min(scalar),
            self.w.min(scalar),
        )
    }

    /// Component-wise maximum against a scalar (Godot `Vector4i.maxi`).
    pub fn maxi(self, scalar: i32) -> Self {
        Self::new(
            self.x.max(scalar),
            self.y.max(scalar),
            self.z.max(scalar),
            self.w.max(scalar),
        )
    }

    /// Snaps each component to the nearest multiple of the matching step
    /// (Godot `Vector4i.snapped`).
    pub fn snapped(self, step: Self) -> Self {
        Self::new(
            snapped_int(self.x, step.x),
            snapped_int(self.y, step.y),
            snapped_int(self.z, step.z),
            snapped_int(self.w, step.w),
        )
    }

    /// Snaps every component to the same step (Godot `Vector4i.snappedi`).
    pub fn snappedi(self, step: i32) -> Self {
        self.snapped(Self::new(step, step, step, step))
    }
}

/// `Math::snapped` for integer components (Godot `Vector4i.snappedi`).
///
/// ⚠️ Godot has only ONE `Math::snapped`, taking doubles
/// (`core/math/math_funcs.h:678`), so the integer path promotes, rounds and
/// truncates back. That is not the same as integer arithmetic would give:
/// `snapped(-5, 10)` is `floor(-0.5 + 0.5) * 10 = 0`, where a symmetric
/// round-half-away would say `-10`.
///
/// ⚠️ Promoted through `f64`, not `f32`: an `i32` past 2^24 is not exactly
/// representable in `f32`, so snapping a large coordinate would drift by more
/// than the step it was asked to snap to.
pub fn snapped_int(value: i32, step: i32) -> i32 {
    if step == 0 {
        return value;
    }
    let snapped = (f64::from(value) / f64::from(step) + 0.5).floor() * f64::from(step);
    snapped as i32
}

impl Add for Vector4i {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(
            self.x + rhs.x,
            self.y + rhs.y,
            self.z + rhs.z,
            self.w + rhs.w,
        )
    }
}

impl Sub for Vector4i {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(
            self.x - rhs.x,
            self.y - rhs.y,
            self.z - rhs.z,
            self.w - rhs.w,
        )
    }
}

impl Neg for Vector4i {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }
}

// ---------------------------------------------------------------------------
// Vector4
// ---------------------------------------------------------------------------

/// A 4D vector with `f32` components, matching Godot's `Vector4`
/// (`core/math/vector4.h`).
///
/// Godot's homogeneous coordinate type: the row of a [`crate::math3d::Projection`],
/// a shader `vec4`, and the element type of `PackedVector4Array`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector4 {
    /// The X component.
    pub x: f32,
    /// The Y component.
    pub y: f32,
    /// The Z component.
    pub z: f32,
    /// The W component.
    pub w: f32,
}

/// Godot `Math::cubic_interpolate` — the Catmull-Rom basis
/// (`core/math/math_funcs.h:343-349`).
pub(crate) fn cubic_interpolate_f(from: f32, to: f32, pre: f32, post: f32, w: f32) -> f32 {
    0.5 * ((from * 2.0)
        + (-pre + to) * w
        + (2.0 * pre - 5.0 * from + 4.0 * to - post) * (w * w)
        + (-pre + 3.0 * from - 3.0 * to + post) * (w * w * w))
}

/// Godot `Math::cubic_interpolate_in_time` — the Barry-Goldman method
/// (`core/math/math_funcs.h`).
///
/// ⚠️ THE FIVE ZERO-GUARDS ARE NOT INTERCHANGEABLE. Each degenerate time span
/// falls back to a DIFFERENT constant — `0.0`, `0.5`, `1.0`, `0.0`, `1.0` and
/// `0.5` in order — and a port that used one value for all of them, or that let
/// the division happen and produced NaN, would still look right for evenly
/// spaced keyframes and only misbehave on the degenerate ones animation
/// actually produces.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cubic_interpolate_in_time_f(
    from: f32,
    to: f32,
    pre: f32,
    post: f32,
    w: f32,
    to_t: f32,
    pre_t: f32,
    post_t: f32,
) -> f32 {
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let t = lerp(0.0, to_t, w);
    let a1 = lerp(
        pre,
        from,
        if pre_t == 0.0 {
            0.0
        } else {
            (t - pre_t) / -pre_t
        },
    );
    let a2 = lerp(from, to, if to_t == 0.0 { 0.5 } else { t / to_t });
    let a3 = lerp(
        to,
        post,
        if post_t - to_t == 0.0 {
            1.0
        } else {
            (t - to_t) / (post_t - to_t)
        },
    );
    let b1 = lerp(
        a1,
        a2,
        if to_t - pre_t == 0.0 {
            0.0
        } else {
            (t - pre_t) / (to_t - pre_t)
        },
    );
    let b2 = lerp(a2, a3, if post_t == 0.0 { 1.0 } else { t / post_t });
    lerp(b1, b2, if to_t == 0.0 { 0.5 } else { t / to_t })
}

impl Vector4 {
    /// The zero vector `(0, 0, 0, 0)`.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
    /// The one vector `(1, 1, 1, 1)`.
    pub const ONE: Self = Self {
        x: 1.0,
        y: 1.0,
        z: 1.0,
        w: 1.0,
    };
    /// All components positive infinity — Godot's `Vector4.INF`.
    pub const INF: Self = Self {
        x: f32::INFINITY,
        y: f32::INFINITY,
        z: f32::INFINITY,
        w: f32::INFINITY,
    };

    /// Creates a new 4D vector.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Squared length (Godot `Vector4.length_squared`).
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Euclidean length (Godot `Vector4.length`).
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Unit vector in the same direction, or [`ZERO`](Self::ZERO) for a
    /// zero-length input — Godot returns a zero vector rather than NaN.
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len > 0.0 {
            Self::new(self.x / len, self.y / len, self.z / len, self.w / len)
        } else {
            Self::ZERO
        }
    }

    /// Whether the vector is (near enough) unit length — Godot
    /// `Vector4.is_normalized`, which compares the SQUARED length so no square
    /// root is taken.
    pub fn is_normalized(self) -> bool {
        (self.length_squared() - 1.0).abs() < 1e-5
    }

    /// Dot product (Godot `Vector4.dot`).
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Distance to `other` (Godot `Vector4.distance_to`).
    pub fn distance_to(self, other: Self) -> f32 {
        (other - self).length()
    }

    /// Squared distance to `other` (Godot `Vector4.distance_squared_to`).
    pub fn distance_squared_to(self, other: Self) -> f32 {
        (other - self).length_squared()
    }

    /// Catmull-Rom interpolation between `self` and `b`, with `pre_a` and
    /// `post_b` as the surrounding control points (Godot
    /// `Vector4.cubic_interpolate`), applied per component
    /// (`core/math/vector4.cpp:147-154`).
    ///
    /// ⚠️ ARGUMENT ORDER IS A TRAP. The method takes `(b, pre_a, post_b, w)`
    /// while the scalar it delegates to takes `(from, to, pre, post, w)` — so
    /// `b` is the SECOND scalar argument and `pre_a` the third. Transposing
    /// them still produces a smooth-looking curve through plausible values,
    /// which is why the tests pin the endpoints: at `w == 0` the result must be
    /// exactly `self` and at `w == 1` exactly `b`, and a swapped pair breaks
    /// both.
    pub fn cubic_interpolate(self, b: Self, pre_a: Self, post_b: Self, weight: f32) -> Self {
        Self::new(
            cubic_interpolate_f(self.x, b.x, pre_a.x, post_b.x, weight),
            cubic_interpolate_f(self.y, b.y, pre_a.y, post_b.y, weight),
            cubic_interpolate_f(self.z, b.z, pre_a.z, post_b.z, weight),
            cubic_interpolate_f(self.w, b.w, pre_a.w, post_b.w, weight),
        )
    }

    /// [`cubic_interpolate`](Self::cubic_interpolate) with explicit time values
    /// for each control point — Godot's Barry-Goldman variant
    /// (`Vector4.cubic_interpolate_in_time`, `core/math/vector4.cpp:156-163`).
    ///
    /// Used by animation when keyframes are not evenly spaced: with uniform
    /// times it agrees with the plain cubic, which the tests cross-check.
    #[allow(clippy::too_many_arguments)]
    pub fn cubic_interpolate_in_time(
        self,
        b: Self,
        pre_a: Self,
        post_b: Self,
        weight: f32,
        b_t: f32,
        pre_a_t: f32,
        post_b_t: f32,
    ) -> Self {
        let f = |from: f32, to: f32, pre: f32, post: f32| {
            cubic_interpolate_in_time_f(from, to, pre, post, weight, b_t, pre_a_t, post_b_t)
        };
        Self::new(
            f(self.x, b.x, pre_a.x, post_b.x),
            f(self.y, b.y, pre_a.y, post_b.y),
            f(self.z, b.z, pre_a.z, post_b.z),
            f(self.w, b.w, pre_a.w, post_b.w),
        )
    }

    /// Linear interpolation toward `other` by `weight` (Godot `Vector4.lerp`).
    pub fn lerp(self, other: Self, weight: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * weight,
            self.y + (other.y - self.y) * weight,
            self.z + (other.z - self.z) * weight,
            self.w + (other.w - self.w) * weight,
        )
    }

    /// Component-wise absolute value (Godot `Vector4.abs`).
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs(), self.z.abs(), self.w.abs())
    }

    /// Component-wise floor (Godot `Vector4.floor`).
    pub fn floor(self) -> Self {
        Self::new(
            self.x.floor(),
            self.y.floor(),
            self.z.floor(),
            self.w.floor(),
        )
    }

    /// Component-wise ceil (Godot `Vector4.ceil`).
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil(), self.z.ceil(), self.w.ceil())
    }

    /// Component-wise round (Godot `Vector4.round`).
    pub fn round(self) -> Self {
        Self::new(
            self.x.round(),
            self.y.round(),
            self.z.round(),
            self.w.round(),
        )
    }

    /// Component-wise minimum (Godot `Vector4.min`).
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
            self.w.min(other.w),
        )
    }

    /// Component-wise maximum (Godot `Vector4.max`).
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
            self.w.max(other.w),
        )
    }

    /// The integer vector with each component truncated toward zero, the
    /// conversion Godot's `Vector4i(Vector4)` constructor performs.
    pub fn to_vector4i(self) -> Vector4i {
        Vector4i::new(self.x as i32, self.y as i32, self.z as i32, self.w as i32)
    }

    /// Per-component `Math::fposmod` against a scalar (Godot `Vector4.posmod`).
    pub fn posmod(self, m: f32) -> Self {
        Self::new(
            fposmod(self.x, m),
            fposmod(self.y, m),
            fposmod(self.z, m),
            fposmod(self.w, m),
        )
    }

    /// Per-component `Math::fposmod` against a vector (Godot `Vector4.posmodv`).
    pub fn posmodv(self, m: Self) -> Self {
        Self::new(
            fposmod(self.x, m.x),
            fposmod(self.y, m.y),
            fposmod(self.z, m.z),
            fposmod(self.w, m.w),
        )
    }

    /// Snaps each component to the nearest multiple of the matching step
    /// (Godot `Vector4.snapped`).
    pub fn snapped(self, step: Self) -> Self {
        Self::new(
            snapped_scalar(self.x, step.x),
            snapped_scalar(self.y, step.y),
            snapped_scalar(self.z, step.z),
            snapped_scalar(self.w, step.w),
        )
    }

    /// Snaps every component to the same step (Godot `Vector4.snappedf`).
    pub fn snappedf(self, step: f32) -> Self {
        self.snapped(Self::new(step, step, step, step))
    }

    /// The component-wise reciprocal (Godot `Vector4.inverse`).
    ///
    /// ⚠️ Not the normalised or negated vector, despite the name — Godot's
    /// `Vector4::inverse` is literally `1/x` per component
    /// (`core/math/vector4.cpp:199`), so a zero component yields an infinity
    /// rather than an error.
    pub fn inverse(self) -> Self {
        Self::new(1.0 / self.x, 1.0 / self.y, 1.0 / self.z, 1.0 / self.w)
    }

    /// Component-wise clamp between two vectors (Godot `Vector4.clamp`).
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(
            self.x.clamp(min.x, max.x),
            self.y.clamp(min.y, max.y),
            self.z.clamp(min.z, max.z),
            self.w.clamp(min.w, max.w),
        )
    }

    /// Clamps every component between the same two scalars (Godot
    /// `Vector4.clampf`).
    pub fn clampf(self, min: f32, max: f32) -> Self {
        Self::new(
            self.x.clamp(min, max),
            self.y.clamp(min, max),
            self.z.clamp(min, max),
            self.w.clamp(min, max),
        )
    }

    /// Component-wise minimum against a scalar (Godot `Vector4.minf`).
    pub fn minf(self, scalar: f32) -> Self {
        Self::new(
            self.x.min(scalar),
            self.y.min(scalar),
            self.z.min(scalar),
            self.w.min(scalar),
        )
    }

    /// Component-wise maximum against a scalar (Godot `Vector4.maxf`).
    pub fn maxf(self, scalar: f32) -> Self {
        Self::new(
            self.x.max(scalar),
            self.y.max(scalar),
            self.z.max(scalar),
            self.w.max(scalar),
        )
    }
}

/// Godot's `Math::fposmod` (`core/math/math_funcs.h:279-286`).
///
/// ⚠️ **Not `rem_euclid`.** The result takes the sign of the DIVISOR, not an
/// unconditionally positive value: `fposmod(5, -3)` is `-1` in Godot, while
/// `5f32.rem_euclid(-3.0)` is `2`. Both are "a positive-ish modulo" by name and
/// they disagree on exactly the inputs a wrapping calculation cares about.
pub fn fposmod(x: f32, y: f32) -> f32 {
    let value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value + y
    } else {
        value
    }
}

/// Godot's `Math::snapped` (`core/math/math_funcs.cpp:121-126`).
///
/// ⚠️ `floor(value / step + 0.5)`, which is round-half-**UP**, not Rust's
/// `round()` (round-half-away-from-zero). They differ on every negative
/// half-step: snapping `-2.5` to a step of `1` gives `-2` here and `-3` through
/// `round()`. A step of `0` returns the value unchanged rather than dividing.
pub fn snapped_scalar(value: f32, step: f32) -> f32 {
    if step != 0.0 {
        (value / step + 0.5).floor() * step
    } else {
        value
    }
}

impl Add for Vector4 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(
            self.x + rhs.x,
            self.y + rhs.y,
            self.z + rhs.z,
            self.w + rhs.w,
        )
    }
}

impl Sub for Vector4 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(
            self.x - rhs.x,
            self.y - rhs.y,
            self.z - rhs.z,
            self.w - rhs.w,
        )
    }
}

impl Mul<f32> for Vector4 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs, self.w * rhs)
    }
}

impl Div<f32> for Vector4 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs, self.w / rhs)
    }
}

impl Neg for Vector4 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }
}

impl From<Vector4i> for Vector4 {
    fn from(v: Vector4i) -> Self {
        Self::new(v.x as f32, v.y as f32, v.z as f32, v.w as f32)
    }
}

// ---------------------------------------------------------------------------
// Rect2i
// ---------------------------------------------------------------------------

/// An axis-aligned rectangle with integer bounds, matching Godot's `Rect2i`
/// (`core/math/rect2i.h`).
///
/// The type a `TileSet` atlas region and every pixel-exact UI rect wants: going
/// through [`Rect2`]'s floats would round a region boundary and sample the
/// neighbouring tile (pat-s3rrv.59).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect2i {
    /// The top-left corner position.
    pub position: Vector2i,
    /// The width and height of the rectangle.
    pub size: Vector2i,
}

impl Rect2i {
    /// Creates a new integer rectangle.
    pub const fn new(position: Vector2i, size: Vector2i) -> Self {
        Self { position, size }
    }

    /// Creates a rectangle from loose components, the order Godot's
    /// `Rect2i(x, y, w, h)` constructor takes.
    pub const fn from_components(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            position: Vector2i::new(x, y),
            size: Vector2i::new(width, height),
        }
    }

    /// The end point (`position + size`) — Godot's `Rect2i.end`.
    pub fn end(self) -> Vector2i {
        self.position + self.size
    }

    /// Whether `point` lies inside the rectangle.
    ///
    /// ⚠️ Godot's `has_point` is half-open — the low edge is inside and the
    /// high edge is not — so tiling rectangles never both claim a pixel.
    pub fn has_point(self, point: Vector2i) -> bool {
        let end = self.end();
        point.x >= self.position.x
            && point.y >= self.position.y
            && point.x < end.x
            && point.y < end.y
    }

    /// The enclosed area (Godot `Rect2i.get_area`).
    pub fn area(self) -> i64 {
        self.size.x as i64 * self.size.y as i64
    }

    /// Whether the rect has a strictly positive area (Godot `Rect2i.has_area`).
    pub fn has_area(self) -> bool {
        self.size.x > 0 && self.size.y > 0
    }

    /// The centre point.
    ///
    /// ⚠️ Godot computes this with INTEGER division (`position + (size / 2)`),
    /// so an odd extent biases toward the low corner — matching it matters for
    /// anything that pairs a centre with `has_point`.
    pub fn get_center(self) -> Vector2i {
        Vector2i::new(
            self.position.x + self.size.x / 2,
            self.position.y + self.size.y / 2,
        )
    }

    /// Whether this rect overlaps `other` (Godot `Rect2i.intersects`, which is
    /// half-open on both axes like `has_point`).
    pub fn intersects(self, other: Self) -> bool {
        let a_end = self.end();
        let b_end = other.end();
        self.position.x < b_end.x
            && a_end.x > other.position.x
            && self.position.y < b_end.y
            && a_end.y > other.position.y
    }

    /// Whether `other` lies entirely within this rect (Godot `Rect2i.encloses`).
    pub fn encloses(self, other: Self) -> bool {
        other.position.x >= self.position.x
            && other.position.y >= self.position.y
            && other.end().x <= self.end().x
            && other.end().y <= self.end().y
    }

    /// The overlapping region, or a zero-size rect when they do not overlap
    /// (Godot `Rect2i.intersection`).
    pub fn intersection(self, other: Self) -> Self {
        if !self.intersects(other) {
            return Self::default();
        }
        let position = self.position.max_components(other.position);
        let end = self.end().min_components(other.end());
        Self::new(position, end - position)
    }

    /// The smallest rect containing both (Godot `Rect2i.merge`).
    pub fn merge(self, other: Self) -> Self {
        let position = self.position.min_components(other.position);
        let end = self.end().max_components(other.end());
        Self::new(position, end - position)
    }

    /// The same rect with a non-negative size, moving `position` to the true
    /// top-left (Godot `Rect2i.abs`).
    pub fn abs(self) -> Self {
        Self::new(
            Vector2i::new(
                self.position.x + self.size.x.min(0),
                self.position.y + self.size.y.min(0),
            ),
            Vector2i::new(self.size.x.abs(), self.size.y.abs()),
        )
    }

    /// The rect grown by `amount` on every side (Godot `Rect2i.grow`).
    pub fn grow(self, amount: i32) -> Self {
        Self::new(
            Vector2i::new(self.position.x - amount, self.position.y - amount),
            Vector2i::new(self.size.x + amount * 2, self.size.y + amount * 2),
        )
    }

    /// The rect grown by a separate amount on each side (Godot
    /// `Rect2i.grow_individual`), transcribed from `core/math/rect2i.h:172-180`.
    ///
    /// ⚠️ The position moves by LEFT/TOP only, while the size grows by
    /// `left + right` and `top + bottom` — growing the right side must not move
    /// the origin. A negative amount shrinks, and Godot does not clamp, so a
    /// rect can be shrunk to a negative size exactly as upstream allows.
    pub fn grow_individual(self, left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self::new(
            Vector2i::new(self.position.x - left, self.position.y - top),
            Vector2i::new(self.size.x + left + right, self.size.y + top + bottom),
        )
    }

    /// The rect grown on a single side, identified by the `@GlobalScope` `Side`
    /// enum — `SIDE_LEFT` 0, `SIDE_TOP` 1, `SIDE_RIGHT` 2, `SIDE_BOTTOM` 3
    /// (`core/math/math_defs.h:116-119`).
    ///
    /// An out-of-range side leaves the rect unchanged: Godot's `grow_side`
    /// passes `0` for every side that does not match, so an unmatched value
    /// grows nothing. Mirrors [`Rect2::grow_side`].
    ///
    /// Godot: `Rect2i.grow_side` (`core/math/rect2i.h:159-166`).
    pub fn grow_side(self, side: i64, amount: i32) -> Self {
        self.grow_individual(
            if side == 0 { amount } else { 0 },
            if side == 1 { amount } else { 0 },
            if side == 2 { amount } else { 0 },
            if side == 3 { amount } else { 0 },
        )
    }

    /// The smallest rect containing this one and `point` (Godot `Rect2i.expand`).
    pub fn expand(self, point: Vector2i) -> Self {
        self.merge(Self::new(point, Vector2i::ZERO))
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
    /// The infinity vector — Godot's `Vector2.INF`, used as a "no result"
    /// sentinel by the geometry helpers.
    pub const INF: Self = Self {
        x: f32::INFINITY,
        y: f32::INFINITY,
    };

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
    /// The back direction `(0, 0, 1)`.
    pub const BACK: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    /// The left direction `(-1, 0, 0)`.
    pub const LEFT: Self = Self {
        x: -1.0,
        y: 0.0,
        z: 0.0,
    };
    /// The right direction `(1, 0, 0)`.
    pub const RIGHT: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    /// The infinity vector — Godot's `Vector3.INF`, used as a "no hit" sentinel
    /// by the raycast/geometry helpers.
    pub const INF: Self = Self {
        x: f32::INFINITY,
        y: f32::INFINITY,
        z: f32::INFINITY,
    };

    // Model-space direction constants (`upstream/godot/core/math/vector3.h`).
    // These describe the axes of an imported *model*, which faces +Z, and so
    // are deliberately NOT the same as the world-space FORWARD/BACK above.
    /// Model-space left `(1, 0, 0)`.
    pub const MODEL_LEFT: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    /// Model-space right `(-1, 0, 0)`.
    pub const MODEL_RIGHT: Self = Self {
        x: -1.0,
        y: 0.0,
        z: 0.0,
    };
    /// Model-space top `(0, 1, 0)`.
    pub const MODEL_TOP: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    /// Model-space bottom `(0, -1, 0)`.
    pub const MODEL_BOTTOM: Self = Self {
        x: 0.0,
        y: -1.0,
        z: 0.0,
    };
    /// Model-space front `(0, 0, 1)`.
    pub const MODEL_FRONT: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    /// Model-space rear `(0, 0, -1)`.
    pub const MODEL_REAR: Self = Self {
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

    /// Whether the vector is (near enough) unit length — Godot
    /// `Vector3.is_normalized`, which compares the SQUARED length so no square
    /// root is taken. Mirrors `Vector4::is_normalized`, epsilon included.
    pub fn is_normalized(self) -> bool {
        (self.length_squared() - 1.0).abs() < 1e-5
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

    /// Catmull-Rom interpolation between `self` and `b`, with `pre_a` and
    /// `post_b` as the surrounding control points (Godot
    /// `Vector3.cubic_interpolate`, `core/math/vector3.cpp`).
    ///
    /// ⚠️ ARGUMENT ORDER IS A TRAP, exactly as on [`Vector4::cubic_interpolate`]:
    /// the method takes `(b, pre_a, post_b, w)` while the scalar it delegates to
    /// takes `(from, to, pre, post, w)`, so `b` is the SECOND scalar argument
    /// and `pre_a` the third. A transposition still yields a smooth curve
    /// through plausible values, which is why the tests pin the endpoints:
    /// `w == 0` must be exactly `self` and `w == 1` exactly `b`.
    ///
    /// Added for `Curve3D`'s cubic baked sampling (pat-s3rrv.91); Godot has had
    /// this on Vector3 all along and only the Vector4 form existed here.
    pub fn cubic_interpolate(self, b: Self, pre_a: Self, post_b: Self, weight: f32) -> Self {
        Self::new(
            cubic_interpolate_f(self.x, b.x, pre_a.x, post_b.x, weight),
            cubic_interpolate_f(self.y, b.y, pre_a.y, post_b.y, weight),
            cubic_interpolate_f(self.z, b.z, pre_a.z, post_b.z, weight),
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

    // -- 8-bit members (pat-s3rrv.58) ---------------------------------------
    //
    // Godot exposes `r8`/`g8`/`b8`/`a8` as read/write members in 0..255. They
    // ROUND rather than truncate (`core/math/color.h`), so 0.5 is 128, not 127
    // — truncating would drift every colour one step dark and only show up as a
    // slightly-off screenshot diff.

    /// Red in 0..255 (Godot `Color.r8`).
    pub fn r8(self) -> i64 {
        channel_to_8bit(self.r)
    }

    /// Green in 0..255 (Godot `Color.g8`).
    pub fn g8(self) -> i64 {
        channel_to_8bit(self.g)
    }

    /// Blue in 0..255 (Godot `Color.b8`).
    pub fn b8(self) -> i64 {
        channel_to_8bit(self.b)
    }

    /// Alpha in 0..255 (Godot `Color.a8`).
    pub fn a8(self) -> i64 {
        channel_to_8bit(self.a)
    }

    /// Builds a colour from a packed RGBE9995 value (Godot
    /// `Color.from_rgbe9995`).
    ///
    /// Nine mantissa bits per channel and a shared five-bit exponent, the
    /// layout Godot uses for HDR lightmaps and radiance data
    /// (`core/math/color.cpp:464`).
    ///
    /// ⚠️ Deliberately does NOT clamp. This is an HDR format: a channel of
    /// 511.0 is a legitimate value, not an overflow to correct. Clamping here
    /// would silently flatten every highlight the format exists to carry.
    pub fn from_rgbe9995(rgbe: u32) -> Self {
        let r = (rgbe & 0x1ff) as f32;
        let g = ((rgbe >> 9) & 0x1ff) as f32;
        let b = ((rgbe >> 18) & 0x1ff) as f32;
        // The exponent is the top five bits, so the shift alone isolates it —
        // upstream masks nothing here either.
        let e = (rgbe >> 27) as f32;
        let m = 2.0f32.powf(e - 15.0 - 9.0);

        Self::new(r * m, g * m, b * m, 1.0)
    }

    /// Builds a colour from 0..255 components (Godot `Color8`).
    pub fn from_rgba8(r: i64, g: i64, b: i64, a: i64) -> Self {
        Self::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    // -- HSV members --------------------------------------------------------

    /// Hue in 0..1 (Godot `Color.h`).
    ///
    /// ⚠️ 0..1, NOT 0..360. Godot normalises hue everywhere in the Color API,
    /// so a caller passing degrees gets a wildly wrong colour that still looks
    /// like a colour.
    pub fn h(self) -> f32 {
        let (h, _, _) = self.to_hsv();
        h
    }

    /// Saturation in 0..1 (Godot `Color.s`).
    pub fn s(self) -> f32 {
        let (_, s, _) = self.to_hsv();
        s
    }

    /// Value in 0..1 (Godot `Color.v`).
    pub fn v(self) -> f32 {
        let (_, _, v) = self.to_hsv();
        v
    }

    // -- Member SETTERS (Godot `Color::set_r8` … `set_v`) --------------------
    //
    // Godot exposes all fourteen Color members as read-WRITE: `col.r8 = 128`
    // and `col.h = 0.5` are ordinary assignments. These are the write halves,
    // named `with_*` because `Color` is `Copy` here and the callers build a new
    // value rather than mutating in place.

    /// This colour with its red channel set from a 0..255 value.
    ///
    /// ⚠️ CLAMPS, and that is a real divergence from [`Color::from_rgba8`],
    /// which does not. Godot's `set_r8` is `CLAMP(r8, 0, 255) / 255` while
    /// `Color::from_rgba8` is a bare `p_r8 / 255` (`core/math/color.h:227` vs
    /// `color.cpp:478`). Copying one into the other is the easy mistake: an
    /// unclamped `col.r8 = 300` would store 1.18 and read back as 255, so the
    /// value written and the value read would disagree.
    pub fn with_r8(self, r8: i64) -> Self {
        Self::new(channel_from_8bit(r8), self.g, self.b, self.a)
    }

    /// This colour with its green channel set from a 0..255 value.
    pub fn with_g8(self, g8: i64) -> Self {
        Self::new(self.r, channel_from_8bit(g8), self.b, self.a)
    }

    /// This colour with its blue channel set from a 0..255 value.
    pub fn with_b8(self, b8: i64) -> Self {
        Self::new(self.r, self.g, channel_from_8bit(b8), self.a)
    }

    /// This colour with its alpha set from a 0..255 value.
    pub fn with_a8(self, a8: i64) -> Self {
        Self::new(self.r, self.g, self.b, channel_from_8bit(a8))
    }

    /// This colour with its hue replaced, saturation and value preserved.
    ///
    /// Godot's `set_h` is `set_hsv(p_h, get_s(), get_v(), a)` — it round-trips
    /// through HSV, so the other two components are re-derived rather than
    /// carried. Alpha is preserved.
    ///
    /// ⚠️ A greyscale colour has saturation 0, so setting its hue is a no-op —
    /// `Color(0.5, 0.5, 0.5).h = 0.3` stays grey, exactly as in Godot. That is
    /// a property of the HSV round-trip, not a bug to "fix" by special-casing.
    pub fn with_h(self, h: f32) -> Self {
        let (_, s, v) = self.to_hsv();
        Self::from_hsv(h, s, v, self.a)
    }

    /// This colour with its saturation replaced, hue and value preserved.
    pub fn with_s(self, s: f32) -> Self {
        let (h, _, v) = self.to_hsv();
        Self::from_hsv(h, s, v, self.a)
    }

    /// This colour with its value (brightness) replaced, hue and saturation
    /// preserved.
    pub fn with_v(self, v: f32) -> Self {
        let (h, s, _) = self.to_hsv();
        Self::from_hsv(h, s, v, self.a)
    }

    /// This colour as `(hue, saturation, value)`, each in 0..1.
    ///
    /// Matches `Color::get_h/get_s/get_v` (`core/math/color.cpp`). A greyscale
    /// colour has no meaningful hue; Godot reports 0 for it rather than NaN.
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let delta = max - min;
        let h = if delta == 0.0 {
            0.0
        } else if max == self.r {
            ((self.g - self.b) / delta) % 6.0
        } else if max == self.g {
            (self.b - self.r) / delta + 2.0
        } else {
            (self.r - self.g) / delta + 4.0
        };
        // Godot's hue is 0..1, so the 0..6 sector value is scaled and wrapped.
        let h = (h / 6.0).rem_euclid(1.0);
        let s = if max == 0.0 { 0.0 } else { delta / max };
        (h, s, max)
    }

    // -- OKHSL members (Godot `Color.ok_hsl_h/s/l`, `from_ok_hsl`) ----------
    //
    // Oklab-based HSL: perceptually uniform, so equal steps in `ok_hsl_l` look
    // like equal steps in brightness where plain HSV's `v` does not. The maths
    // lives in [`crate::ok_color`], a transcription of Godot's vendored
    // `thirdparty/misc/ok_color.h` (pat-s3rrv.58).

    /// Builds a colour from Oklab HSL, each component in 0..1 (Godot
    /// `Color.from_ok_hsl`).
    ///
    /// Mirrors `Color::set_ok_hsl` (`core/math/color.cpp:236`), including its
    /// final `clamp()`: the OKHSL gamut is larger than sRGB, so a saturated
    /// request can land outside 0..1 and Godot clips rather than wrapping.
    pub fn from_ok_hsl(h: f32, s: f32, l: f32, a: f32) -> Self {
        let (r, g, b) = crate::ok_color::okhsl_to_srgb(h, s, l);
        Self::new(r, g, b, a).clamp_to(Self::new(0.0, 0.0, 0.0, 0.0), Self::new(1.0, 1.0, 1.0, 1.0))
    }

    /// This colour as `(ok_hsl_h, ok_hsl_s, ok_hsl_l)`, each in 0..1.
    ///
    /// ⚠️ A pure grey has no hue, and the underlying transform divides by a
    /// chroma of zero — so upstream really does return NaN here. Godot maps
    /// each NaN to 0 before clamping (`color.cpp:504`), and so does this; a
    /// caller that skipped the check would propagate NaN into a colour and
    /// paint nothing.
    pub fn to_ok_hsl(self) -> (f32, f32, f32) {
        let (h, s, l) = crate::ok_color::srgb_to_okhsl(self.r, self.g, self.b);
        let fix = |x: f32| if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) };
        (fix(h), fix(s), fix(l))
    }

    /// Oklab hue in 0..1 (Godot `Color.ok_hsl_h`).
    pub fn ok_hsl_h(self) -> f32 {
        self.to_ok_hsl().0
    }

    /// Oklab saturation in 0..1 (Godot `Color.ok_hsl_s`).
    pub fn ok_hsl_s(self) -> f32 {
        self.to_ok_hsl().1
    }

    /// Oklab lightness in 0..1 (Godot `Color.ok_hsl_l`).
    pub fn ok_hsl_l(self) -> f32 {
        self.to_ok_hsl().2
    }

    /// This colour with its Oklab hue replaced.
    pub fn with_ok_hsl_h(self, h: f32) -> Self {
        let (_, s, l) = self.to_ok_hsl();
        Self::from_ok_hsl(h, s, l, self.a)
    }

    /// This colour with its Oklab saturation replaced.
    pub fn with_ok_hsl_s(self, s: f32) -> Self {
        let (h, _, l) = self.to_ok_hsl();
        Self::from_ok_hsl(h, s, l, self.a)
    }

    /// This colour with its Oklab lightness replaced.
    pub fn with_ok_hsl_l(self, l: f32) -> Self {
        let (h, s, _) = self.to_ok_hsl();
        Self::from_ok_hsl(h, s, l, self.a)
    }

    /// Builds a colour from hue/saturation/value, each in 0..1 (Godot
    /// `Color.from_hsv`).
    pub fn from_hsv(h: f32, s: f32, v: f32, a: f32) -> Self {
        let h = h.rem_euclid(1.0) * 6.0;
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let i = h.floor();
        let f = h - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i as i32 % 6 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        Self::new(r, g, b, a)
    }

    // -- Derived colours ----------------------------------------------------

    /// The colour with RGB inverted, alpha untouched (Godot `inverted`).
    pub fn inverted(self) -> Self {
        Self::new(1.0 - self.r, 1.0 - self.g, 1.0 - self.b, self.a)
    }

    /// Interpolated `amount` of the way towards white (Godot `lightened`).
    ///
    /// ⚠️ NOT symmetric with [`darkened`](Self::darkened): lightening moves
    /// TOWARDS white (`c + (1 - c) * amount`) while darkening SCALES towards
    /// black (`c * (1 - amount)`). Implementing either as the other's mirror
    /// gives plausible-looking but wrong values.
    pub fn lightened(self, amount: f32) -> Self {
        Self::new(
            self.r + (1.0 - self.r) * amount,
            self.g + (1.0 - self.g) * amount,
            self.b + (1.0 - self.b) * amount,
            self.a,
        )
    }

    /// The colour scaled `amount` of the way towards black (Godot `darkened`).
    pub fn darkened(self, amount: f32) -> Self {
        let k = 1.0 - amount;
        Self::new(self.r * k, self.g * k, self.b * k, self.a)
    }

    /// Alpha-blends `over` on top of this colour (Godot `blend`).
    ///
    /// A fully transparent result is returned as transparent BLACK rather than
    /// dividing by a zero alpha, which is what Godot does and what keeps the
    /// channels finite.
    pub fn blend(self, over: Self) -> Self {
        let sa = 1.0 - over.a;
        let a = self.a * sa + over.a;
        if a <= 0.0 {
            return Self::new(0.0, 0.0, 0.0, 0.0);
        }
        Self::new(
            (self.r * self.a * sa + over.r * over.a) / a,
            (self.g * self.a * sa + over.g * over.a) / a,
            (self.b * self.a * sa + over.b * over.a) / a,
            a,
        )
    }

    /// Perceived brightness (Godot `get_luminance`), Rec. 709 weights.
    pub fn get_luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// Every component clamped into `[min, max]` (Godot `clamp`).
    pub fn clamp_to(self, min: Self, max: Self) -> Self {
        Self::new(
            self.r.clamp(min.r, max.r),
            self.g.clamp(min.g, max.g),
            self.b.clamp(min.b, max.b),
            self.a.clamp(min.a, max.a),
        )
    }

    /// Whether every component is approximately equal (Godot
    /// `is_equal_approx`).
    pub fn is_equal_approx(self, other: Self) -> bool {
        (self.r - other.r).abs() < 1e-5
            && (self.g - other.g).abs() < 1e-5
            && (self.b - other.b).abs() < 1e-5
            && (self.a - other.a).abs() < 1e-5
    }

    // -- HTML / integer forms ----------------------------------------------

    /// The colour as a hex string, WITHOUT a leading `#` (Godot `to_html`).
    ///
    /// ⚠️ Godot orders the alpha LAST here (`rrggbbaa`), unlike the packed
    /// integer forms below which are ARGB. The two conventions sit next to each
    /// other in the same API and are easy to swap.
    pub fn to_html(self, with_alpha: bool) -> String {
        let mut out = format!("{:02x}{:02x}{:02x}", self.r8(), self.g8(), self.b8());
        if with_alpha {
            out.push_str(&format!("{:02x}", self.a8()));
        }
        out
    }

    /// Parses a Godot HTML colour: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`,
    /// with or without the leading `#` (Godot `Color.html`).
    ///
    /// Returns `None` for anything else, which is what
    /// [`html_is_valid`](Self::html_is_valid) reports.
    pub fn from_html(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#').unwrap_or(text);
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        // The 3- and 4-digit forms repeat each digit: `#f80` is `#ff8800`.
        let expand = |c: char| -> String { format!("{c}{c}") };
        let full: String = match hex.len() {
            3 | 4 => hex.chars().map(expand).collect(),
            6 | 8 => hex.to_string(),
            _ => return None,
        };
        let byte = |i: usize| -> f32 {
            u8::from_str_radix(&full[i * 2..i * 2 + 2], 16).unwrap_or(0) as f32 / 255.0
        };
        let a = if full.len() == 8 { byte(3) } else { 1.0 };
        Some(Self::new(byte(0), byte(1), byte(2), a))
    }

    /// Whether `text` parses as a Godot HTML colour (Godot `html_is_valid`).
    pub fn html_is_valid(text: &str) -> bool {
        Self::from_html(text).is_some()
    }

    /// Packed `0xAARRGGBB` (Godot `to_argb32`).
    pub fn to_argb32(self) -> i64 {
        (self.a8() << 24) | (self.r8() << 16) | (self.g8() << 8) | self.b8()
    }

    /// Packed `0xAABBGGRR` (Godot `to_abgr32`).
    pub fn to_abgr32(self) -> i64 {
        (self.a8() << 24) | (self.b8() << 16) | (self.g8() << 8) | self.r8()
    }

    /// Packed `0xRRGGBBAA` (Godot `to_rgba32`).
    pub fn to_rgba32(self) -> i64 {
        (self.r8() << 24) | (self.g8() << 16) | (self.b8() << 8) | self.a8()
    }

    // -- 64-bit and packed-integer constructors (pat-s3rrv.58) -------------

    /// A 0..1 channel as a 0..65535 half-word.
    ///
    /// Rounds, for the same reason [`channel_to_8bit`] does.
    #[inline]
    fn channel_to_16bit(c: f32) -> i64 {
        (c.clamp(0.0, 1.0) * 65535.0).round() as i64
    }

    /// Packs four 16-bit channels into the low-to-high slots of one word.
    ///
    /// ⚠️ Assembled in `u64` and cast at the end, NOT shifted in `i64`. A full
    /// channel in the top slot is `65535 << 48`, which exceeds `i64::MAX` — in
    /// `i64` that is an overflow panic in debug builds, and the panic would only
    /// fire for colours with a high alpha, i.e. almost every opaque one. Godot
    /// returns the same bit pattern from a signed `int64_t`, so the cast is the
    /// faithful part, not a workaround.
    #[inline]
    fn pack64(hi: i64, a: i64, b: i64, lo: i64) -> i64 {
        (((hi as u64) << 48) | ((a as u64) << 32) | ((b as u64) << 16) | (lo as u64)) as i64
    }

    /// Packed `0xAAAARRRRGGGGBBBB` (Godot `to_argb64`).
    pub fn to_argb64(self) -> i64 {
        Self::pack64(
            Self::channel_to_16bit(self.a),
            Self::channel_to_16bit(self.r),
            Self::channel_to_16bit(self.g),
            Self::channel_to_16bit(self.b),
        )
    }

    /// Packed `0xAAAABBBBGGGGRRRR` (Godot `to_abgr64`).
    pub fn to_abgr64(self) -> i64 {
        Self::pack64(
            Self::channel_to_16bit(self.a),
            Self::channel_to_16bit(self.b),
            Self::channel_to_16bit(self.g),
            Self::channel_to_16bit(self.r),
        )
    }

    /// Packed `0xRRRRGGGGBBBBAAAA` (Godot `to_rgba64`).
    pub fn to_rgba64(self) -> i64 {
        Self::pack64(
            Self::channel_to_16bit(self.r),
            Self::channel_to_16bit(self.g),
            Self::channel_to_16bit(self.b),
            Self::channel_to_16bit(self.a),
        )
    }

    /// Builds a colour from a packed 32-bit `0xRRGGBBAA` (Godot `Color.hex`).
    ///
    /// ⚠️ RGBA order, matching `to_rgba32` — NOT the ARGB of `to_argb32`, which
    /// sits two methods away in the same API.
    pub fn from_hex32(hex: u32) -> Self {
        Self::new(
            ((hex >> 24) & 0xff) as f32 / 255.0,
            ((hex >> 16) & 0xff) as f32 / 255.0,
            ((hex >> 8) & 0xff) as f32 / 255.0,
            (hex & 0xff) as f32 / 255.0,
        )
    }

    /// Builds a colour from a packed 64-bit `0xRRRRGGGGBBBBAAAA` (Godot
    /// `Color.hex64`).
    pub fn from_hex64(hex: u64) -> Self {
        Self::new(
            ((hex >> 48) & 0xffff) as f32 / 65535.0,
            ((hex >> 32) & 0xffff) as f32 / 65535.0,
            ((hex >> 16) & 0xffff) as f32 / 65535.0,
            (hex & 0xffff) as f32 / 65535.0,
        )
    }

    /// A Godot named colour constant, or `None` if the name is not one.
    ///
    /// Godot exposes 146 of these on `Color` (`core/math/color_names.inc`) —
    /// the X11/CSS palette plus `TRANSPARENT`. The GDScript interpreter used to
    /// hardcode SIX of them inline, so `Color.YELLOW`, `Color.CYAN`,
    /// `Color.ORANGE` and 140 others silently resolved to `None`
    /// (pat-s3rrv.58).
    ///
    /// The table lives here rather than in the interpreter because `Color` owns
    /// its own constants, and because it keeps the interpreter's lookup to one
    /// line instead of 146 — a real consideration in a 28k-line file several
    /// agents share.
    ///
    /// ⚠️ Values are Godot's, which are the X11 ones, and X11 disagrees with
    /// CSS on several names: `GRAY` is `bebebe` (not CSS `808080` — that is
    /// `WEB_GRAY`), `GREEN` is pure `00ff00` (not CSS `008000` — that is
    /// `WEB_GREEN`), and `MAROON`/`PURPLE` likewise have `WEB_` variants.
    /// Taking the CSS value for those four is the easy mistake and produces a
    /// colour that looks reasonable and is wrong.
    pub fn named(name: &str) -> Option<Self> {
        let hex = match name {
            "ALICE_BLUE" => "f0f8ff",
            "ANTIQUE_WHITE" => "faebd7",
            "AQUA" => "00ffff",
            "AQUAMARINE" => "7fffd4",
            "AZURE" => "f0ffff",
            "BEIGE" => "f5f5dc",
            "BISQUE" => "ffe4c4",
            "BLACK" => "000000",
            "BLANCHED_ALMOND" => "ffebcd",
            "BLUE" => "0000ff",
            "BLUE_VIOLET" => "8a2be2",
            "BROWN" => "a52a2a",
            "BURLYWOOD" => "deb887",
            "CADET_BLUE" => "5f9ea0",
            "CHARTREUSE" => "7fff00",
            "CHOCOLATE" => "d2691e",
            "CORAL" => "ff7f50",
            "CORNFLOWER_BLUE" => "6495ed",
            "CORNSILK" => "fff8dc",
            "CRIMSON" => "dc143c",
            "CYAN" => "00ffff",
            "DARK_BLUE" => "00008b",
            "DARK_CYAN" => "008b8b",
            "DARK_GOLDENROD" => "b8860b",
            "DARK_GRAY" => "a9a9a9",
            "DARK_GREEN" => "006400",
            "DARK_KHAKI" => "bdb76b",
            "DARK_MAGENTA" => "8b008b",
            "DARK_OLIVE_GREEN" => "556b2f",
            "DARK_ORANGE" => "ff8c00",
            "DARK_ORCHID" => "9932cc",
            "DARK_RED" => "8b0000",
            "DARK_SALMON" => "e9967a",
            "DARK_SEA_GREEN" => "8fbc8f",
            "DARK_SLATE_BLUE" => "483d8b",
            "DARK_SLATE_GRAY" => "2f4f4f",
            "DARK_TURQUOISE" => "00ced1",
            "DARK_VIOLET" => "9400d3",
            "DEEP_PINK" => "ff1493",
            "DEEP_SKY_BLUE" => "00bfff",
            "DIM_GRAY" => "696969",
            "DODGER_BLUE" => "1e90ff",
            "FIREBRICK" => "b22222",
            "FLORAL_WHITE" => "fffaf0",
            "FOREST_GREEN" => "228b22",
            "FUCHSIA" => "ff00ff",
            "GAINSBORO" => "dcdcdc",
            "GHOST_WHITE" => "f8f8ff",
            "GOLD" => "ffd700",
            "GOLDENROD" => "daa520",
            "GRAY" => "bebebe",
            "GREEN" => "00ff00",
            "GREEN_YELLOW" => "adff2f",
            "HONEYDEW" => "f0fff0",
            "HOT_PINK" => "ff69b4",
            "INDIAN_RED" => "cd5c5c",
            "INDIGO" => "4b0082",
            "IVORY" => "fffff0",
            "KHAKI" => "f0e68c",
            "LAVENDER" => "e6e6fa",
            "LAVENDER_BLUSH" => "fff0f5",
            "LAWN_GREEN" => "7cfc00",
            "LEMON_CHIFFON" => "fffacd",
            "LIGHT_BLUE" => "add8e6",
            "LIGHT_CORAL" => "f08080",
            "LIGHT_CYAN" => "e0ffff",
            "LIGHT_GOLDENROD" => "fafad2",
            "LIGHT_GRAY" => "d3d3d3",
            "LIGHT_GREEN" => "90ee90",
            "LIGHT_PINK" => "ffb6c1",
            "LIGHT_SALMON" => "ffa07a",
            "LIGHT_SEA_GREEN" => "20b2aa",
            "LIGHT_SKY_BLUE" => "87cefa",
            "LIGHT_SLATE_GRAY" => "778899",
            "LIGHT_STEEL_BLUE" => "b0c4de",
            "LIGHT_YELLOW" => "ffffe0",
            "LIME" => "00ff00",
            "LIME_GREEN" => "32cd32",
            "LINEN" => "faf0e6",
            "MAGENTA" => "ff00ff",
            "MAROON" => "b03060",
            "MEDIUM_AQUAMARINE" => "66cdaa",
            "MEDIUM_BLUE" => "0000cd",
            "MEDIUM_ORCHID" => "ba55d3",
            "MEDIUM_PURPLE" => "9370db",
            "MEDIUM_SEA_GREEN" => "3cb371",
            "MEDIUM_SLATE_BLUE" => "7b68ee",
            "MEDIUM_SPRING_GREEN" => "00fa9a",
            "MEDIUM_TURQUOISE" => "48d1cc",
            "MEDIUM_VIOLET_RED" => "c71585",
            "MIDNIGHT_BLUE" => "191970",
            "MINT_CREAM" => "f5fffa",
            "MISTY_ROSE" => "ffe4e1",
            "MOCCASIN" => "ffe4b5",
            "NAVAJO_WHITE" => "ffdead",
            "NAVY_BLUE" => "000080",
            "OLD_LACE" => "fdf5e6",
            "OLIVE" => "808000",
            "OLIVE_DRAB" => "6b8e23",
            "ORANGE" => "ffa500",
            "ORANGE_RED" => "ff4500",
            "ORCHID" => "da70d6",
            "PALE_GOLDENROD" => "eee8aa",
            "PALE_GREEN" => "98fb98",
            "PALE_TURQUOISE" => "afeeee",
            "PALE_VIOLET_RED" => "db7093",
            "PAPAYA_WHIP" => "ffefd5",
            "PEACH_PUFF" => "ffdab9",
            "PERU" => "cd853f",
            "PINK" => "ffc0cb",
            "PLUM" => "dda0dd",
            "POWDER_BLUE" => "b0e0e6",
            "PURPLE" => "a020f0",
            "REBECCA_PURPLE" => "663399",
            "RED" => "ff0000",
            "ROSY_BROWN" => "bc8f8f",
            "ROYAL_BLUE" => "4169e1",
            "SADDLE_BROWN" => "8b4513",
            "SALMON" => "fa8072",
            "SANDY_BROWN" => "f4a460",
            "SEA_GREEN" => "2e8b57",
            "SEASHELL" => "fff5ee",
            "SIENNA" => "a0522d",
            "SILVER" => "c0c0c0",
            "SKY_BLUE" => "87ceeb",
            "SLATE_BLUE" => "6a5acd",
            "SLATE_GRAY" => "708090",
            "SNOW" => "fffafa",
            "SPRING_GREEN" => "00ff7f",
            "STEEL_BLUE" => "4682b4",
            "TAN" => "d2b48c",
            "TEAL" => "008080",
            "THISTLE" => "d8bfd8",
            "TOMATO" => "ff6347",
            "TRANSPARENT" => "ffffff00",
            "TURQUOISE" => "40e0d0",
            "VIOLET" => "ee82ee",
            "WEB_GRAY" => "808080",
            "WEB_GREEN" => "008000",
            "WEB_MAROON" => "800000",
            "WEB_PURPLE" => "800080",
            "WHEAT" => "f5deb3",
            "WHITE" => "ffffff",
            "WHITE_SMOKE" => "f5f5f5",
            "YELLOW" => "ffff00",
            "YELLOW_GREEN" => "9acd32",
            _ => return None,
        };
        Self::from_html(hex)
    }

    /// Every Godot named colour, as `(name, colour)` pairs.
    ///
    /// Exposed so a consumer (the inspector's colour picker, a test sweeping
    /// the table) can enumerate them without duplicating the list.
    pub fn named_constants() -> &'static [&'static str] {
        &[
            "ALICE_BLUE",
            "ANTIQUE_WHITE",
            "AQUA",
            "AQUAMARINE",
            "AZURE",
            "BEIGE",
            "BISQUE",
            "BLACK",
            "BLANCHED_ALMOND",
            "BLUE",
            "BLUE_VIOLET",
            "BROWN",
            "BURLYWOOD",
            "CADET_BLUE",
            "CHARTREUSE",
            "CHOCOLATE",
            "CORAL",
            "CORNFLOWER_BLUE",
            "CORNSILK",
            "CRIMSON",
            "CYAN",
            "DARK_BLUE",
            "DARK_CYAN",
            "DARK_GOLDENROD",
            "DARK_GRAY",
            "DARK_GREEN",
            "DARK_KHAKI",
            "DARK_MAGENTA",
            "DARK_OLIVE_GREEN",
            "DARK_ORANGE",
            "DARK_ORCHID",
            "DARK_RED",
            "DARK_SALMON",
            "DARK_SEA_GREEN",
            "DARK_SLATE_BLUE",
            "DARK_SLATE_GRAY",
            "DARK_TURQUOISE",
            "DARK_VIOLET",
            "DEEP_PINK",
            "DEEP_SKY_BLUE",
            "DIM_GRAY",
            "DODGER_BLUE",
            "FIREBRICK",
            "FLORAL_WHITE",
            "FOREST_GREEN",
            "FUCHSIA",
            "GAINSBORO",
            "GHOST_WHITE",
            "GOLD",
            "GOLDENROD",
            "GRAY",
            "GREEN",
            "GREEN_YELLOW",
            "HONEYDEW",
            "HOT_PINK",
            "INDIAN_RED",
            "INDIGO",
            "IVORY",
            "KHAKI",
            "LAVENDER",
            "LAVENDER_BLUSH",
            "LAWN_GREEN",
            "LEMON_CHIFFON",
            "LIGHT_BLUE",
            "LIGHT_CORAL",
            "LIGHT_CYAN",
            "LIGHT_GOLDENROD",
            "LIGHT_GRAY",
            "LIGHT_GREEN",
            "LIGHT_PINK",
            "LIGHT_SALMON",
            "LIGHT_SEA_GREEN",
            "LIGHT_SKY_BLUE",
            "LIGHT_SLATE_GRAY",
            "LIGHT_STEEL_BLUE",
            "LIGHT_YELLOW",
            "LIME",
            "LIME_GREEN",
            "LINEN",
            "MAGENTA",
            "MAROON",
            "MEDIUM_AQUAMARINE",
            "MEDIUM_BLUE",
            "MEDIUM_ORCHID",
            "MEDIUM_PURPLE",
            "MEDIUM_SEA_GREEN",
            "MEDIUM_SLATE_BLUE",
            "MEDIUM_SPRING_GREEN",
            "MEDIUM_TURQUOISE",
            "MEDIUM_VIOLET_RED",
            "MIDNIGHT_BLUE",
            "MINT_CREAM",
            "MISTY_ROSE",
            "MOCCASIN",
            "NAVAJO_WHITE",
            "NAVY_BLUE",
            "OLD_LACE",
            "OLIVE",
            "OLIVE_DRAB",
            "ORANGE",
            "ORANGE_RED",
            "ORCHID",
            "PALE_GOLDENROD",
            "PALE_GREEN",
            "PALE_TURQUOISE",
            "PALE_VIOLET_RED",
            "PAPAYA_WHIP",
            "PEACH_PUFF",
            "PERU",
            "PINK",
            "PLUM",
            "POWDER_BLUE",
            "PURPLE",
            "REBECCA_PURPLE",
            "RED",
            "ROSY_BROWN",
            "ROYAL_BLUE",
            "SADDLE_BROWN",
            "SALMON",
            "SANDY_BROWN",
            "SEA_GREEN",
            "SEASHELL",
            "SIENNA",
            "SILVER",
            "SKY_BLUE",
            "SLATE_BLUE",
            "SLATE_GRAY",
            "SNOW",
            "SPRING_GREEN",
            "STEEL_BLUE",
            "TAN",
            "TEAL",
            "THISTLE",
            "TOMATO",
            "TRANSPARENT",
            "TURQUOISE",
            "VIOLET",
            "WEB_GRAY",
            "WEB_GREEN",
            "WEB_MAROON",
            "WEB_PURPLE",
            "WHEAT",
            "WHITE",
            "WHITE_SMOKE",
            "YELLOW",
            "YELLOW_GREEN",
        ]
    }
}

/// A 0..1 channel as Godot's 0..255 byte.
///
/// ⚠️ Rounds rather than truncates, matching `core/math/color.h`. Truncation
/// would map 0.5 to 127 instead of 128 and pull every converted colour one step
/// dark.
#[inline]
fn channel_to_8bit(c: f32) -> i64 {
    (c.clamp(0.0, 1.0) * 255.0).round() as i64
}

/// Godot's 0..255 byte as a 0..1 channel — the inverse of [`channel_to_8bit`].
///
/// ⚠️ Clamps the BYTE before dividing, matching `Color::set_r8`
/// (`core/math/color.h:227`). Dividing first and clamping after would be the
/// same for in-range input and differ for out-of-range: `300 / 255` is 1.18,
/// which reads back as 255 and makes the written and read values disagree.
#[inline]
fn channel_from_8bit(v: i64) -> f32 {
    v.clamp(0, 255) as f32 / 255.0
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
// pat-s3rrv.148 — the Rect2 / Transform2D method gaps
// ---------------------------------------------------------------------------

impl Rect2 {
    /// The corner furthest along `direction` (`Rect2.get_support`).
    pub fn get_support(self, direction: Vector2) -> Vector2 {
        let mut support = self.position;
        if direction.x > 0.0 {
            support.x += self.size.x;
        }
        if direction.y > 0.0 {
            support.y += self.size.y;
        }
        support
    }

    /// Whether both corners are approximately equal (`Rect2.is_equal_approx`).
    pub fn is_equal_approx(self, other: Rect2) -> bool {
        (self.position.x - other.position.x).abs() < 1e-5
            && (self.position.y - other.position.y).abs() < 1e-5
            && (self.size.x - other.size.x).abs() < 1e-5
            && (self.size.y - other.size.y).abs() < 1e-5
    }

    /// Whether every component is finite (`Rect2.is_finite`).
    pub fn is_finite(self) -> bool {
        self.position.x.is_finite()
            && self.position.y.is_finite()
            && self.size.x.is_finite()
            && self.size.y.is_finite()
    }
}

impl Transform2D {
    /// Whether this transform preserves angles (`Transform2D.is_conformal`).
    ///
    /// 🔑 **Not the same test as `Basis.is_conformal`.** Godot checks two
    /// specific 2×2 shapes — the rotation form (`a == d`, `b == -c`) and the
    /// FLIPPED form (`a == -d`, `b == c`) — rather than "equal column lengths
    /// and perpendicular" (`transform_2d.cpp`). Generalizing from the 3D
    /// version looks equivalent and accepts matrices Godot rejects.
    pub fn is_conformal(self) -> bool {
        let eq = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // Non-flipped (a rotation with uniform scale).
        if eq(self.x.x, self.y.y) && eq(self.x.y, -self.y.x) {
            return true;
        }
        // Flipped (a rotation with uniform scale and a mirror).
        if eq(self.x.x, -self.y.y) && eq(self.x.y, self.y.x) {
            return true;
        }
        false
    }

    /// Whether every component is finite (`Transform2D.is_finite`).
    pub fn is_finite(self) -> bool {
        self.x.x.is_finite()
            && self.x.y.is_finite()
            && self.y.x.is_finite()
            && self.y.y.is_finite()
            && self.origin.x.is_finite()
            && self.origin.y.is_finite()
    }

    /// This transform rotated to face `target` (`Transform2D.looking_at`).
    ///
    /// ⚠️ The target is a GLOBAL point, brought into local space by the affine
    /// inverse before its angle is taken — passing it straight to `angle()`
    /// would aim at a point measured from the origin of the world rather than
    /// from this transform.
    ///
    /// ⓘ Scale is applied to the local target before the angle, so a
    /// non-uniformly scaled transform aims correctly in the space the user
    /// sees. Rotation replaces, rather than adds to, whatever `get_rotation`
    /// already reported — the sum below is Godot's, and it cancels because
    /// `target_position` was measured after the existing rotation.
    pub fn looking_at(self, target: Vector2) -> Self {
        let local = self.affine_inverse().xform(target);
        let scale = self.get_scale();
        let scaled = Vector2::new(local.x * scale.x, local.y * scale.y);
        Self::from_rotation_origin(self.get_rotation() + scaled.angle(), self.origin)
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

    /// `Vector3::is_normalized` existed for Vector4 but not Vector3, so a
    /// caller in math3d.rs failed to compile with E0599 and took the whole
    /// gdcore lib — and therefore every dependent crate — down with it.
    #[test]
    fn vector3_is_normalized() {
        assert!(Vector3::new(1.0, 0.0, 0.0).is_normalized());
        assert!(Vector3::new(1.0, 2.0, -3.0).normalized().is_normalized());
        // The two ways it must answer false: too long, and the ZERO that
        // `normalized()` returns for a degenerate vector — the caller relies on
        // that second case to bail out to IDENTITY.
        assert!(!Vector3::new(1.0, 1.0, 0.0).is_normalized());
        assert!(!Vector3::ZERO.is_normalized());
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

    // -- Color builtin surface (pat-s3rrv.58) -------------------------------

    /// Godot's own value for every case here, so a plausible-but-wrong formula
    /// fails rather than passing on self-consistency.
    #[test]
    fn color_8bit_members_round_rather_than_truncate() {
        // ⚠️ 0.5 is 128, not 127. Truncating pulls every converted colour one
        // step dark — invisible in isolation, visible as a screenshot diff.
        assert_eq!(Color::new(0.5, 0.0, 1.0, 1.0).r8(), 128);
        assert_eq!(Color::new(0.5, 0.0, 1.0, 1.0).g8(), 0);
        assert_eq!(Color::new(0.5, 0.0, 1.0, 1.0).b8(), 255);
        assert_eq!(Color::new(0.0, 0.0, 0.0, 0.5).a8(), 128);
        // Out-of-range components clamp rather than wrapping.
        assert_eq!(Color::new(2.0, -1.0, 0.0, 1.0).r8(), 255);
        assert_eq!(Color::new(2.0, -1.0, 0.0, 1.0).g8(), 0);
    }

    #[test]
    fn color_to_html_matches_godot() {
        let c = Color::from_html("#ff8800").expect("valid html");
        assert_eq!(c.to_html(false), "ff8800");
        // ⚠️ Alpha goes LAST in to_html (rgba), unlike the packed ARGB ints.
        assert_eq!(c.to_html(true), "ff8800ff");
    }

    #[test]
    fn color_from_html_accepts_every_godot_form() {
        // The 3- and 4-digit forms repeat each digit.
        assert!(Color::from_html("#f80")
            .unwrap()
            .is_equal_approx(Color::from_html("#ff8800").unwrap()));
        assert!(Color::from_html("f80f")
            .unwrap()
            .is_equal_approx(Color::from_html("#ff8800ff").unwrap()));
        // …and the leading `#` is optional.
        assert!(Color::from_html("ff8800").is_some());

        assert!(!Color::html_is_valid("#12345"), "5 digits is not a form");
        assert!(!Color::html_is_valid("#gggggg"), "non-hex digits");
        assert!(!Color::html_is_valid(""), "empty");
    }

    #[test]
    fn color_lightened_and_darkened_are_not_mirrors() {
        let yellow = Color::new(1.0, 1.0, 0.0, 1.0);
        // The acceptance's case: Color.YELLOW.darkened(0.2) -> (0.8, 0.8, 0).
        let dark = yellow.darkened(0.2);
        assert!(
            dark.is_equal_approx(Color::new(0.8, 0.8, 0.0, 1.0)),
            "{dark:?}"
        );

        // ⚠️ Lightening moves TOWARDS white, it does not scale: 0.5 lightened by
        // 0.2 is 0.6, whereas the mirror of `darkened` would give 0.4.
        let grey = Color::new(0.5, 0.5, 0.5, 1.0);
        assert!(
            grey.lightened(0.2)
                .is_equal_approx(Color::new(0.6, 0.6, 0.6, 1.0)),
            "{:?}",
            grey.lightened(0.2)
        );
        assert!(grey
            .darkened(0.2)
            .is_equal_approx(Color::new(0.4, 0.4, 0.4, 1.0)));
    }

    #[test]
    fn color_inverted_leaves_alpha_alone() {
        let c = Color::new(1.0, 1.0, 0.0, 0.25).inverted();
        assert!(c.is_equal_approx(Color::new(0.0, 0.0, 1.0, 0.25)), "{c:?}");
    }

    #[test]
    fn color_luminance_uses_rec709_weights() {
        assert!((Color::WHITE.get_luminance() - 1.0).abs() < 1e-5);
        assert!((Color::BLACK.get_luminance()).abs() < 1e-5);
        // Yellow: 0.2126 + 0.7152.
        let y = Color::new(1.0, 1.0, 0.0, 1.0).get_luminance();
        assert!((y - 0.9278).abs() < 1e-4, "{y}");
        // Green alone must outweigh red alone — the ordering a flat 1/3 average
        // would lose.
        assert!(
            Color::new(0.0, 1.0, 0.0, 1.0).get_luminance()
                > Color::new(1.0, 0.0, 0.0, 1.0).get_luminance()
        );
    }

    #[test]
    fn color_hsv_round_trips() {
        // from_hsv(0.5, 1, 1) is cyan.
        let cyan = Color::from_hsv(0.5, 1.0, 1.0, 1.0);
        assert!(
            cyan.is_equal_approx(Color::new(0.0, 1.0, 1.0, 1.0)),
            "{cyan:?}"
        );

        for c in [
            Color::new(1.0, 0.0, 0.0, 1.0),
            Color::new(0.0, 1.0, 0.0, 1.0),
            Color::new(0.25, 0.5, 0.75, 1.0),
        ] {
            let (h, s, v) = c.to_hsv();
            let back = Color::from_hsv(h, s, v, 1.0);
            assert!(
                back.is_equal_approx(c),
                "{c:?} -> ({h},{s},{v}) -> {back:?}"
            );
        }

        // ⚠️ Hue is 0..1, NOT degrees. A greyscale colour reports hue 0 rather
        // than NaN.
        assert!(Color::new(1.0, 0.0, 0.0, 1.0).h() < 1e-5);
        assert!((Color::new(0.0, 1.0, 1.0, 1.0).h() - 0.5).abs() < 1e-5);
        assert_eq!(Color::new(0.3, 0.3, 0.3, 1.0).h(), 0.0);
        assert_eq!(Color::new(0.3, 0.3, 0.3, 1.0).s(), 0.0);
        assert!((Color::new(0.3, 0.3, 0.3, 1.0).v() - 0.3).abs() < 1e-5);
    }

    #[test]
    fn color_blend_composites_over() {
        // Opaque over anything is that colour.
        let base = Color::new(1.0, 0.0, 0.0, 1.0);
        let over = Color::new(0.0, 0.0, 1.0, 1.0);
        assert!(base.blend(over).is_equal_approx(over));

        // Fully transparent over leaves the base untouched.
        assert!(base
            .blend(Color::new(0.0, 1.0, 0.0, 0.0))
            .is_equal_approx(base));

        // Half-alpha blue over opaque red meets in the middle.
        let half = base.blend(Color::new(0.0, 0.0, 1.0, 0.5));
        assert!(
            half.is_equal_approx(Color::new(0.5, 0.0, 0.5, 1.0)),
            "{half:?}"
        );

        // Transparent over transparent stays finite rather than dividing by 0.
        let none = Color::new(1.0, 1.0, 1.0, 0.0).blend(Color::new(1.0, 1.0, 1.0, 0.0));
        assert!(
            none.is_equal_approx(Color::new(0.0, 0.0, 0.0, 0.0)),
            "{none:?}"
        );
    }

    #[test]
    fn color_packed_integer_forms_use_their_own_orders() {
        // ⚠️ to_html is RGBA but the packed ints are ARGB/ABGR/RGBA — three
        // orders in one API, so each is pinned separately.
        let c = Color::new(1.0, 0.5, 0.0, 1.0);
        assert_eq!(c.to_argb32(), 0xff_ff_80_00);
        assert_eq!(c.to_abgr32(), 0xff_00_80_ff);
        assert_eq!(c.to_rgba32(), 0xff_80_00_ff);
    }

    #[test]
    fn color_from_rgba8_and_clamp() {
        let c = Color::from_rgba8(255, 128, 0, 255);
        assert_eq!(c.r8(), 255);
        assert_eq!(c.g8(), 128);
        assert_eq!(c.b8(), 0);

        let clamped =
            Color::new(2.0, -1.0, 0.5, 1.0).clamp_to(Color::new(0.0, 0.0, 0.0, 0.0), Color::WHITE);
        assert!(
            clamped.is_equal_approx(Color::new(1.0, 0.0, 0.5, 1.0)),
            "{clamped:?}"
        );
    }

    #[test]
    fn color_named_covers_godots_whole_table() {
        assert_eq!(
            Color::named_constants().len(),
            146,
            "Godot exposes 146 named colours (core/math/color_names.inc)"
        );
        // Every advertised name must actually resolve — a name in the list that
        // `named()` does not know would be a silent hole.
        for name in Color::named_constants() {
            assert!(
                Color::named(name).is_some(),
                "{name} is advertised but does not resolve"
            );
        }
        assert!(Color::named("NOT_A_COLOUR").is_none());
        assert!(Color::named("yellow").is_none(), "names are SCREAMING_CASE");
    }

    /// ⚠️ Godot uses the X11 palette, which disagrees with CSS on four names.
    /// Taking the CSS value gives a plausible colour that is wrong, and the
    /// `WEB_` variants exist precisely to carry the CSS ones.
    #[test]
    fn color_named_uses_x11_values_not_css() {
        assert!(Color::named("GRAY")
            .unwrap()
            .is_equal_approx(Color::from_html("bebebe").unwrap()));
        assert!(Color::named("WEB_GRAY")
            .unwrap()
            .is_equal_approx(Color::from_html("808080").unwrap()));

        // X11 GREEN is pure green; CSS `green` is the darker WEB_GREEN.
        assert!(Color::named("GREEN")
            .unwrap()
            .is_equal_approx(Color::new(0.0, 1.0, 0.0, 1.0)));
        assert!(Color::named("WEB_GREEN")
            .unwrap()
            .is_equal_approx(Color::from_html("008000").unwrap()));

        assert!(Color::named("MAROON")
            .unwrap()
            .is_equal_approx(Color::from_html("b03060").unwrap()));
        assert!(Color::named("PURPLE")
            .unwrap()
            .is_equal_approx(Color::from_html("a020f0").unwrap()));
    }

    #[test]
    fn color_named_matches_the_existing_constants() {
        // The six the interpreter already hardcoded must not change value.
        assert!(Color::named("WHITE").unwrap().is_equal_approx(Color::WHITE));
        assert!(Color::named("BLACK").unwrap().is_equal_approx(Color::BLACK));
        assert!(Color::named("RED")
            .unwrap()
            .is_equal_approx(Color::new(1.0, 0.0, 0.0, 1.0)));
        assert!(Color::named("BLUE")
            .unwrap()
            .is_equal_approx(Color::new(0.0, 0.0, 1.0, 1.0)));

        // The acceptance's colour, and the aliases Godot keeps.
        assert!(Color::named("YELLOW")
            .unwrap()
            .is_equal_approx(Color::new(1.0, 1.0, 0.0, 1.0)));
        assert!(Color::named("AQUA")
            .unwrap()
            .is_equal_approx(Color::named("CYAN").unwrap()));
        assert!(Color::named("FUCHSIA")
            .unwrap()
            .is_equal_approx(Color::named("MAGENTA").unwrap()));
    }

    /// pat-s3rrv.58: the member SETTERS. Godot exposes all fourteen Color
    /// members as read-write; only the read half existed.
    #[test]
    fn color_member_setters_write_the_8bit_views() {
        let c = Color::new(0.0, 0.0, 0.0, 1.0);

        // Round-trip through the pair that must agree: 128 -> 0.50196 -> 128.
        assert_eq!(c.with_r8(128).r8(), 128);
        assert_eq!(c.with_g8(64).g8(), 64);
        assert_eq!(c.with_b8(255).b8(), 255);
        assert_eq!(c.with_a8(0).a8(), 0);

        // Each setter touches ONE channel. A copy-paste slip in `with_g8` that
        // wrote `r` would pass a single-channel test but fail this one.
        let c = Color::new(0.1, 0.2, 0.3, 0.4).with_g8(200);
        assert_eq!(c.g8(), 200);
        assert!((c.r - 0.1).abs() < 1e-6, "red must not move, got {}", c.r);
        assert!((c.b - 0.3).abs() < 1e-6, "blue must not move, got {}", c.b);
        assert!((c.a - 0.4).abs() < 1e-6, "alpha must not move, got {}", c.a);
    }

    /// 🔑 `set_r8` CLAMPS while `from_rgba8` does not — the two really do
    /// differ in Godot (`core/math/color.h:227` vs `color.cpp:478`), so this
    /// pins the distinction rather than assuming one rule for both.
    #[test]
    fn color_member_setters_clamp_but_from_rgba8_does_not() {
        assert_eq!(Color::BLACK.with_r8(300).r8(), 255, "set_r8 clamps high");
        assert_eq!(Color::WHITE.with_r8(-5).r8(), 0, "…and low");

        // The written value and the value read back must agree. Dividing
        // before clamping would store 300/255 = 1.18 here, which still reads
        // back as 255 — the same answer for the WRONG reason — so the check
        // that actually separates them is on the stored channel.
        assert!(
            (Color::BLACK.with_r8(300).r - 1.0).abs() < 1e-6,
            "the stored channel must be clamped to 1.0, not 1.18"
        );

        // ⚠️ The unclamped sibling, asserted so nobody "fixes" one to match
        // the other: Godot's `from_rgba8` is a bare divide.
        assert!(
            Color::from_rgba8(300, 0, 0, 255).r > 1.0,
            "from_rgba8 must stay unclamped — it is a different Godot function"
        );
    }

    /// Writing one HSV component re-derives the colour and leaves the other
    /// two — and alpha — alone.
    #[test]
    fn color_member_setters_round_trip_through_hsv() {
        let c = Color::new(1.0, 0.0, 0.0, 0.25);

        let shifted = c.with_h(0.5);
        assert!(
            (shifted.h() - 0.5).abs() < 1e-4,
            "hue must move, got {}",
            shifted.h()
        );
        assert!((shifted.s() - c.s()).abs() < 1e-4, "saturation must not");
        assert!((shifted.v() - c.v()).abs() < 1e-4, "value must not");
        assert!(
            (shifted.a - 0.25).abs() < 1e-6,
            "alpha must survive the HSV round-trip"
        );

        let desaturated = c.with_s(0.0);
        assert!((desaturated.r - desaturated.g).abs() < 1e-6);
        assert!((desaturated.g - desaturated.b).abs() < 1e-6);

        let dimmed = c.with_v(0.5);
        assert!((dimmed.v() - 0.5).abs() < 1e-4);
    }

    /// ⚠️ Setting the hue of a GREY colour is a no-op, because grey has no
    /// hue to move. That is Godot's behaviour (`set_h` round-trips through
    /// `set_hsv`, and saturation 0 short-circuits to `r = g = b = v`), not a
    /// gap — asserted so it is not "fixed" into a divergence.
    #[test]
    fn color_setting_the_hue_of_grey_is_a_no_op() {
        let grey = Color::new(0.5, 0.5, 0.5, 1.0);
        let after = grey.with_h(0.3);
        assert!(
            (after.r - 0.5).abs() < 1e-6
                && (after.g - 0.5).abs() < 1e-6
                && (after.b - 0.5).abs() < 1e-6,
            "grey has saturation 0, so it has no hue to move; got {after:?}"
        );
    }

    /// pat-s3rrv.58: the packed HDR constructor.
    #[test]
    fn color_from_rgbe9995_unpacks_mantissas_and_exponent() {
        // e = 24 makes the multiplier 2^(24-24) = 1, so each 9-bit mantissa is
        // its own value — which is what separates the three channels. A port
        // that shifted by the wrong amount would still pass a grey case.
        let c = Color::from_rgbe9995((24 << 27) | (3 << 18) | (2 << 9) | 1);
        assert_eq!((c.r, c.g, c.b, c.a), (1.0, 2.0, 3.0, 1.0));

        assert_eq!(
            Color::from_rgbe9995(0),
            Color::new(0.0, 0.0, 0.0, 1.0),
            "an all-zero word is opaque black, not transparent"
        );

        // e = 15 gives 2^-9, so a mantissa of 256 is exactly a half.
        let half = Color::from_rgbe9995((15 << 27) | (256 << 18) | (256 << 9) | 256);
        assert!(
            half.is_equal_approx(Color::new(0.5, 0.5, 0.5, 1.0)),
            "{half:?}"
        );
    }

    /// 🔑 RGBE9995 is an HDR format, so values above 1.0 are the POINT of it.
    ///
    /// Clamping here would flatten exactly the highlights the format exists to
    /// carry — and it would look correct in any test that only used in-range
    /// values. Godot does not clamp (`color.cpp:475`).
    #[test]
    fn color_from_rgbe9995_does_not_clamp_hdr_values() {
        let hdr = Color::from_rgbe9995((24 << 27) | 511);
        assert_eq!(hdr.r, 511.0, "a 511 mantissa at e=24 is 511.0, not 1.0");
        assert_eq!((hdr.g, hdr.b), (0.0, 0.0));
    }

    /// pat-s3rrv.58: the OKHSL members. Expected values come from an
    /// independent Python transcription of the vendored `ok_color.h`, and the
    /// hue is additionally pinned to the published OkLCh value for sRGB red
    /// (29.23°) — an oracle outside this codebase entirely.
    #[test]
    fn color_ok_hsl_members_match_godot() {
        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        assert!(
            (red.ok_hsl_h() - 0.081205).abs() < 2e-4,
            "sRGB red's Oklab hue is 29.23° = 0.081205, got {}",
            red.ok_hsl_h()
        );
        assert!((red.ok_hsl_s() - 1.0).abs() < 2e-4, "{}", red.ok_hsl_s());
        assert!(
            (red.ok_hsl_l() - 0.568085).abs() < 2e-4,
            "{}",
            red.ok_hsl_l()
        );

        // ⚠️ NOT the same as plain HSV lightness — that is the entire point of
        // the Oklab variant. `Color(1,0,0).v` is 1.0 while its `ok_hsl_l` is
        // 0.568, because red is perceptually far darker than white.
        assert!(
            (red.v() - red.ok_hsl_l()).abs() > 0.4,
            "ok_hsl_l must not be a rename of v"
        );
    }

    /// `from_ok_hsl` is the inverse, and it preserves alpha.
    #[test]
    fn color_from_ok_hsl_round_trips() {
        let red = Color::new(1.0, 0.0, 0.0, 0.25);
        let (h, s, l) = red.to_ok_hsl();
        let back = Color::from_ok_hsl(h, s, l, red.a);
        // ⚠️ 2e-4, not `is_equal_approx`'s 1e-5. The chain runs a cube root and
        // two Halley iterations in f32; the round-trip error is ~4e-7 in f64
        // but has real headroom to grow in f32, and a gate that is tight enough
        // to flake is worse than one that is merely tight enough to catch a
        // structural error (which moves these by >= 1e-2).
        for (got, want, ch) in [(back.r, 1.0, "r"), (back.g, 0.0, "g"), (back.b, 0.0, "b")] {
            assert!(
                (got - want).abs() < 2e-4,
                "from_ok_hsl(to_ok_hsl(red)).{ch} must be {want}, got {got}"
            );
        }
        assert!((back.a - 0.25).abs() < 1e-6, "alpha must survive");
    }

    /// 🔑 The clamp in `from_ok_hsl` is load-bearing, not defensive.
    ///
    /// The OKHSL gamut is larger than sRGB, so a fully saturated request can
    /// land OUTSIDE 0..1: at h=0.075, s=1.0, l=0.55 the green channel comes
    /// back as −0.0018. Godot's `set_ok_hsl` ends in `.clamp()`
    /// (`color.cpp:242`) and so does this. A negative channel would survive
    /// into blending and shaders, where it is not merely "dark".
    #[test]
    fn color_from_ok_hsl_clamps_out_of_gamut_requests() {
        let c = Color::from_ok_hsl(0.075, 1.0, 0.55, 1.0);
        assert_eq!(
            c.g, 0.0,
            "the out-of-gamut green must clamp to 0, got {}",
            c.g
        );
        assert!(
            c.r >= 0.0 && c.r <= 1.0 && c.b >= 0.0 && c.b <= 1.0,
            "{c:?}"
        );
        assert!((c.r - 0.965267).abs() < 2e-4, "{}", c.r);
        assert!((c.b - 0.109343).abs() < 2e-4, "{}", c.b);
    }

    /// A grey has no hue axis to divide by; nothing may leak a NaN into a
    /// colour a caller would then paint with.
    #[test]
    fn color_ok_hsl_of_grey_is_finite() {
        let (h, s, l) = Color::new(0.5, 0.5, 0.5, 1.0).to_ok_hsl();
        assert!(
            h.is_finite() && s.is_finite() && l.is_finite(),
            "{h} {s} {l}"
        );
        assert!(s < 1e-3, "grey must be unsaturated, got {s}");
    }

    /// 🔴 `Color.TRANSPARENT` IS TRANSPARENT WHITE, and it deliberately does
    /// NOT equal this crate's `Color::TRANSPARENT`.
    ///
    /// Godot documents `Color.TRANSPARENT` as `Color(1, 1, 1, 0)`
    /// (`color_names.inc`), so that is what the GDScript-facing table returns.
    /// gdcore's own `Color::TRANSPARENT` const is `(0, 0, 0, 0)` and is used
    /// across the engine as a ZERO SENTINEL — `MultiMesh::instance_custom_data`
    /// defaults to it, and that data is fed to shaders where an alpha of 0 does
    /// not hide the RGB channels. Changing the const to match Godot would
    /// silently rewrite those defaults from zeros to ones.
    ///
    /// So the two intentionally differ. If a future change "fixes" the const to
    /// match Godot, sweep the 19 `Color::TRANSPARENT` uses first — several want
    /// the zero, not the colour.
    #[test]
    fn color_transparent_is_white_with_zero_alpha() {
        let t = Color::named("TRANSPARENT").unwrap();
        assert_eq!(t.a8(), 0);
        assert_eq!(t.r8(), 255);
        assert_eq!(t.g8(), 255);
        assert_eq!(t.b8(), 255);

        // The divergence, asserted rather than left to be discovered.
        assert_ne!(
            t.r8(),
            Color::TRANSPARENT.r8(),
            "gdcore's TRANSPARENT const is the zero sentinel, not Godot's colour"
        );
    }

    #[test]
    fn color_64bit_forms_and_hex_constructors() {
        let c = Color::new(1.0, 0.5, 0.0, 1.0);
        // 0.5 -> 32768 (rounded), 1.0 -> 65535, 0.0 -> 0.
        assert_eq!(c.to_rgba64(), 0xffff_8000_0000_ffffu64 as i64);
        assert_eq!(c.to_argb64(), 0xffff_ffff_8000_0000u64 as i64);
        assert_eq!(c.to_abgr64(), 0xffff_0000_8000_ffffu64 as i64);

        // ⚠️ `hex` is RGBA, matching to_rgba32 — not the ARGB of to_argb32.
        let round = Color::from_hex32(0xff8000ff);
        assert!(round.is_equal_approx(c), "{round:?}");
        assert_eq!(
            Color::from_hex32(c.to_rgba32() as u32).to_rgba32(),
            c.to_rgba32()
        );

        let wide = Color::from_hex64(0xffff_8000_0000_ffff);
        assert!(wide.is_equal_approx(c), "{wide:?}");
    }

    /// ⚠️ The 64-bit packers must not overflow on a full top channel.
    ///
    /// `65535 << 48` exceeds `i64::MAX`, so shifting in `i64` panics in debug
    /// builds — and it would fire for almost every OPAQUE colour, since alpha
    /// occupies the top slot in two of the three orders. Opaque white is the
    /// case that catches it.
    #[test]
    fn color_64bit_packing_survives_a_full_top_channel() {
        let w = Color::WHITE;
        assert_eq!(w.to_argb64(), -1, "all-ones is -1 as a signed word");
        assert_eq!(w.to_rgba64(), -1);
        assert_eq!(w.to_abgr64(), -1);
        // …and round-trips through the unsigned view.
        assert_eq!(w.to_rgba64() as u64, u64::MAX);
        assert!(Color::from_hex64(w.to_rgba64() as u64).is_equal_approx(w));
    }
}
