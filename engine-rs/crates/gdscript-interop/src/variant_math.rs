//! Script-facing surface for Godot's affine/math builtin Variant types
//! (pat-0q2kx.50).
//!
//! `Rect2`, `Transform2D`, `Transform3D`, `Quaternion`, `Basis`, `AABB` and
//! `Plane` all had complete, unit-tested implementations in `gdcore::math` /
//! `gdcore::math3d`, and all seven already had a `Variant` arm — but nothing in
//! the GDScript VM could reach them. There was no constructor, no member
//! get/set, no method dispatch and no `*` operator, so a script could not so
//! much as write `Transform2D(0.0, Vector2(10, 5))`.
//!
//! This module is that missing layer: pure value → value functions that the
//! interpreter calls from its five dispatch sites (constructor, static member,
//! member read, member write, method call, and `binary_mul`). Keeping it out of
//! `interpreter.rs` keeps the type-by-type tables readable and lets the
//! interpreter arms stay one-liners.
//!
//! Semantics are taken from the vendored Godot source (`upstream/godot/core/math/`)
//! rather than guessed: the `*` operator composes right-to-left (`a * b` applies
//! `b` first), `rotated`/`scaled`/`translated` are the *global* (left-multiply)
//! forms with `*_local` as the right-multiply counterparts, and `Rect2.has_point`
//! is inclusive at `position` and exclusive at `end`.

use gdcore::math::{
    Color, Rect2, Rect2i, Transform2D, Vector2, Vector2i, Vector3, Vector3i, Vector4, Vector4i,
};
use gdcore::math3d::{Aabb, Basis, EulerOrder, Plane, Projection, Quaternion, Transform3D};
use gdvariant::Variant;

use crate::interpreter::{RuntimeError, RuntimeErrorKind};

/// The builtin type names this module constructs.
pub(crate) const CONSTRUCTOR_NAMES: [&str; 7] = [
    "Rect2",
    "Transform2D",
    "Transform3D",
    "Quaternion",
    "Basis",
    "AABB",
    "Plane",
];

fn type_error(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::TypeError(msg))
}

fn undefined_method(type_name: &str, method: &str) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::UndefinedFunction(format!(
        "{type_name}.{method}"
    )))
}

/// Reads a numeric argument, accepting the int/float/bool coercions Godot
/// applies when a builtin expects a float.
fn num(v: &Variant, ctx: &str) -> Result<f32, RuntimeError> {
    match v {
        Variant::Float(f) => Ok(*f as f32),
        Variant::Int(i) => Ok(*i as f32),
        Variant::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => Err(type_error(format!(
            "{ctx} expects a number, got {}",
            other.variant_type()
        ))),
    }
}

// ---------------------------------------------------------------------------
// String static helpers (pat-mgixk.107), ported from core/string/ustring.cpp.
// ---------------------------------------------------------------------------

/// Full-width float argument. `arg_num` narrows to `f32`, which these
/// formatters must not do.
fn string_static_f64(args: &[Variant], idx: usize, ctx: &str) -> Result<f64, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Float(f)) => Ok(*f),
        Some(Variant::Int(i)) => Ok(*i as f64),
        other => Err(type_error(format!(
            "{ctx} expects a number for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

/// Integer argument, or `None` when absent so a caller can apply Godot's own
/// default rather than a shared one.
fn string_static_i64(args: &[Variant], idx: usize) -> Option<i64> {
    match args.get(idx) {
        Some(Variant::Int(i)) => Some(*i),
        Some(Variant::Float(f)) => Some(*f as i64),
        _ => None,
    }
}

/// Godot `String::num_int64` / `num_uint64`: render in `base`, optionally with
/// uppercase digits above 9.
///
/// Base is clamped to 2..=36 — Godot asserts on anything else, and a base of 0
/// or 1 cannot terminate.
fn gd_num_in_base(value: i128, base: i64, capitalize: bool) -> String {
    let base = base.clamp(2, 36) as u32;
    if value == 0 {
        return "0".to_string();
    }
    let negative = value < 0;
    let mut n = value.unsigned_abs();
    let mut digits: Vec<char> = Vec::new();
    while n > 0 {
        let d = (n % base as u128) as u32;
        let ch = std::char::from_digit(d, base).unwrap_or('0');
        digits.push(if capitalize {
            ch.to_ascii_uppercase()
        } else {
            ch
        });
        n /= base as u128;
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

/// Godot `String::num_scientific`, which is C's `%lg`.
///
/// ⚠️ Rust has no `%g`, so the rule is implemented explicitly: with precision
/// `P = 6`, use `%e` when the decimal exponent is below -4 or at least `P`,
/// otherwise `%f` with `P - 1 - exponent` decimals; then strip trailing zeros
/// and a bare trailing point. Writing `{:e}` instead would print `3.14e0` where
/// C prints `3.14`.
fn gd_num_scientific(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    const P: i32 = 6;
    let exp = value.abs().log10().floor() as i32;
    let mut out = if exp < -4 || exp >= P {
        // `%e` carries P-1 fractional digits; normalise the exponent to C's
        // at-least-two-digit form.
        let mantissa_decimals = (P - 1) as usize;
        let formatted = format!("{value:.mantissa_decimals$e}");
        match formatted.split_once('e') {
            Some((mantissa, e)) => {
                let m = strip_trailing_zeros(mantissa);
                let sign = if e.starts_with('-') { '-' } else { '+' };
                let digits: String = e.trim_start_matches(['-', '+']).to_string();
                format!("{m}e{sign}{:0>2}", digits)
            }
            None => formatted,
        }
    } else {
        let decimals = (P - 1 - exp).max(0) as usize;
        strip_trailing_zeros(&format!("{value:.decimals$}"))
    };
    if out.is_empty() {
        out = "0".to_string();
    }
    out
}

/// Removes trailing zeros, and the decimal point if it is left bare — the same
/// trim `String::num` applies.
fn strip_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0');
    trimmed.trim_end_matches('.').to_string()
}

/// Godot `String::humanize_size` (`core/string/ustring.cpp`).
///
/// 🔑 The digit count comes from the SCALED value, not the byte count:
/// `_humanize_digits` gives 2 decimals below 100 and 1 below 1024, and the
/// result runs through `String::num`, whose trailing-zero trim is what turns
/// `1536` into `"1.5 KiB"` rather than `"1.50 KiB"`.
fn gd_humanize_size(size: u64) -> String {
    const PREFIXES: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut div: u64 = 1;
    let mut idx = 0usize;
    while idx < PREFIXES.len() - 1 && size > div.saturating_mul(1024) {
        div = div.saturating_mul(1024);
        idx += 1;
    }
    let scaled = size / div.max(1);
    let decimals = if idx == 0 {
        0
    } else if scaled < 100 {
        2
    } else if scaled < 1024 {
        1
    } else {
        0
    };
    let value = size as f64 / div.max(1) as f64;
    format!(
        "{} {}",
        gdvariant::num_with_decimals(value, decimals),
        PREFIXES[idx]
    )
}

fn arg_num(args: &[Variant], idx: usize, ctx: &str) -> Result<f32, RuntimeError> {
    match args.get(idx) {
        Some(v) => num(v, ctx),
        None => Err(type_error(format!("{ctx} is missing argument {idx}"))),
    }
}

fn arg_vec2(args: &[Variant], idx: usize, ctx: &str) -> Result<Vector2, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Vector2(v)) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector2 for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_vec3(args: &[Variant], idx: usize, ctx: &str) -> Result<Vector3, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Vector3(v)) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector3 for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_rect2(args: &[Variant], idx: usize, ctx: &str) -> Result<Rect2, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Rect2(r)) => Ok(*r),
        other => Err(type_error(format!(
            "{ctx} expects a Rect2 for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_aabb(args: &[Variant], idx: usize, ctx: &str) -> Result<Aabb, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Aabb(a)) => Ok(*a),
        other => Err(type_error(format!(
            "{ctx} expects an AABB for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_quat(args: &[Variant], idx: usize, ctx: &str) -> Result<Quaternion, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Quaternion(q)) => Ok(*q),
        other => Err(type_error(format!(
            "{ctx} expects a Quaternion for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_plane(args: &[Variant], idx: usize, ctx: &str) -> Result<Plane, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Plane(p)) => Ok(*p),
        other => Err(type_error(format!(
            "{ctx} expects a Plane for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

/// The optional `EulerOrder` argument of `get_euler` / `from_euler`
/// (pat-s3rrv.148).
///
/// 🔑 Absent means **YXZ**, Godot's default — not XYZ. This argument was being
/// discarded entirely, so a script asking for XYZ silently received YXZ angles:
/// a plausible rotation, of the wrong shape, with nothing to indicate it.
///
/// An out-of-range index is an error rather than a silent fallback to the
/// default, matching Godot's `ERR_FAIL_MSG("Invalid Euler order parameter.")`.
fn arg_euler_order(args: &[Variant], idx: usize, ctx: &str) -> Result<EulerOrder, RuntimeError> {
    let raw = match args.get(idx) {
        None | Some(Variant::Nil) => return Ok(EulerOrder::default()),
        Some(Variant::Int(v)) => *v,
        // A float is tolerated because arithmetic on an int constant yields one.
        Some(Variant::Float(f)) => *f as i64,
        other => {
            return Err(type_error(format!(
                "{ctx} expects an EulerOrder constant for argument {idx}, got {}",
                describe(other)
            )))
        }
    };
    EulerOrder::from_i64(raw).ok_or_else(|| {
        type_error(format!(
            "{ctx}: {raw} is not a valid EulerOrder (expected EULER_ORDER_XYZ..EULER_ORDER_ZYX)"
        ))
    })
}

fn arg_basis(args: &[Variant], idx: usize, ctx: &str) -> Result<Basis, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Basis(b)) => Ok(*b),
        other => Err(type_error(format!(
            "{ctx} expects a Basis for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_transform2d(args: &[Variant], idx: usize, ctx: &str) -> Result<Transform2D, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Transform2D(t)) => Ok(*t),
        other => Err(type_error(format!(
            "{ctx} expects a Transform2D for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_transform3d(args: &[Variant], idx: usize, ctx: &str) -> Result<Transform3D, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Transform3D(t)) => Ok(*t),
        other => Err(type_error(format!(
            "{ctx} expects a Transform3D for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn describe(v: Option<&Variant>) -> String {
    match v {
        Some(v) => format!("{}", v.variant_type()),
        None => "nothing".to_string(),
    }
}

/// Rejects a call whose argument count no overload accepts.
fn bad_arity(ctx: &str, got: usize) -> RuntimeError {
    type_error(format!("{ctx} does not take {got} argument(s)"))
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// Constructs one of the seven affine/math builtins. Mirrors the constructor
/// overloads Godot exposes in `extension_api.json`.
///
/// Returns `Err` for an argument list no overload accepts; the caller has
/// already matched `name`, so there is no "not mine" case.
pub(crate) fn construct(name: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match name {
        "Rect2" => match args.len() {
            0 => Ok(Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::ZERO))),
            1 => Ok(Variant::Rect2(arg_rect2(args, 0, "Rect2()")?)),
            2 => Ok(Variant::Rect2(Rect2::new(
                arg_vec2(args, 0, "Rect2()")?,
                arg_vec2(args, 1, "Rect2()")?,
            ))),
            4 => Ok(Variant::Rect2(Rect2::new(
                Vector2::new(arg_num(args, 0, "Rect2()")?, arg_num(args, 1, "Rect2()")?),
                Vector2::new(arg_num(args, 2, "Rect2()")?, arg_num(args, 3, "Rect2()")?),
            ))),
            n => Err(bad_arity("Rect2()", n)),
        },
        "Transform2D" => match args.len() {
            0 => Ok(Variant::Transform2D(Transform2D::IDENTITY)),
            1 => Ok(Variant::Transform2D(arg_transform2d(
                args,
                0,
                "Transform2D()",
            )?)),
            // Transform2D(rotation, position)
            2 => Ok(Variant::Transform2D(Transform2D::from_rotation_origin(
                arg_num(args, 0, "Transform2D()")?,
                arg_vec2(args, 1, "Transform2D()")?,
            ))),
            // Transform2D(x_axis, y_axis, origin)
            3 => Ok(Variant::Transform2D(Transform2D {
                x: arg_vec2(args, 0, "Transform2D()")?,
                y: arg_vec2(args, 1, "Transform2D()")?,
                origin: arg_vec2(args, 2, "Transform2D()")?,
            })),
            // Transform2D(rotation, scale, skew, position)
            4 => Ok(Variant::Transform2D(
                Transform2D::from_rotation_scale_skew_origin(
                    arg_num(args, 0, "Transform2D()")?,
                    arg_vec2(args, 1, "Transform2D()")?,
                    arg_num(args, 2, "Transform2D()")?,
                    arg_vec2(args, 3, "Transform2D()")?,
                ),
            )),
            // Transform2D(xx, xy, yx, yy, ox, oy)
            6 => Ok(Variant::Transform2D(Transform2D {
                x: Vector2::new(
                    arg_num(args, 0, "Transform2D()")?,
                    arg_num(args, 1, "Transform2D()")?,
                ),
                y: Vector2::new(
                    arg_num(args, 2, "Transform2D()")?,
                    arg_num(args, 3, "Transform2D()")?,
                ),
                origin: Vector2::new(
                    arg_num(args, 4, "Transform2D()")?,
                    arg_num(args, 5, "Transform2D()")?,
                ),
            })),
            n => Err(bad_arity("Transform2D()", n)),
        },
        "Transform3D" => match args.len() {
            0 => Ok(Variant::Transform3D(Transform3D::IDENTITY)),
            1 => Ok(Variant::Transform3D(arg_transform3d(
                args,
                0,
                "Transform3D()",
            )?)),
            // Transform3D(basis, origin)
            2 => Ok(Variant::Transform3D(Transform3D {
                basis: arg_basis(args, 0, "Transform3D()")?,
                origin: arg_vec3(args, 1, "Transform3D()")?,
            })),
            // Transform3D(x_axis, y_axis, z_axis, origin)
            4 => Ok(Variant::Transform3D(Transform3D {
                basis: Basis {
                    x: arg_vec3(args, 0, "Transform3D()")?,
                    y: arg_vec3(args, 1, "Transform3D()")?,
                    z: arg_vec3(args, 2, "Transform3D()")?,
                },
                origin: arg_vec3(args, 3, "Transform3D()")?,
            })),
            n => Err(bad_arity("Transform3D()", n)),
        },
        "Quaternion" => match args.len() {
            0 => Ok(Variant::Quaternion(Quaternion::IDENTITY)),
            1 => match &args[0] {
                Variant::Quaternion(q) => Ok(Variant::Quaternion(*q)),
                Variant::Basis(b) => Ok(Variant::Quaternion(b.get_rotation_quaternion())),
                other => Err(type_error(format!(
                    "Quaternion() cannot be built from {}",
                    other.variant_type()
                ))),
            },
            // Quaternion(axis, angle) and Quaternion(arc_from, arc_to) are told
            // apart by the second argument's type, exactly as Godot's overload
            // resolution does.
            2 => match (&args[0], &args[1]) {
                (Variant::Vector3(axis), Variant::Vector3(to)) => {
                    Ok(Variant::Quaternion(quaternion_from_arc(*axis, *to)))
                }
                (Variant::Vector3(axis), angle) => Ok(Variant::Quaternion(
                    Quaternion::from_axis_angle(*axis, num(angle, "Quaternion()")?),
                )),
                _ => Err(type_error(
                    "Quaternion(axis, angle) expects a Vector3 axis".into(),
                )),
            },
            4 => Ok(Variant::Quaternion(Quaternion::new(
                arg_num(args, 0, "Quaternion()")?,
                arg_num(args, 1, "Quaternion()")?,
                arg_num(args, 2, "Quaternion()")?,
                arg_num(args, 3, "Quaternion()")?,
            ))),
            n => Err(bad_arity("Quaternion()", n)),
        },
        "Basis" => match args.len() {
            0 => Ok(Variant::Basis(Basis::IDENTITY)),
            1 => match &args[0] {
                Variant::Basis(b) => Ok(Variant::Basis(*b)),
                Variant::Quaternion(q) => Ok(Variant::Basis(Basis::from_quaternion(*q))),
                other => Err(type_error(format!(
                    "Basis() cannot be built from {}",
                    other.variant_type()
                ))),
            },
            // Basis(axis, angle)
            2 => Ok(Variant::Basis(Basis::from_quaternion(
                Quaternion::from_axis_angle(
                    arg_vec3(args, 0, "Basis()")?,
                    arg_num(args, 1, "Basis()")?,
                ),
            ))),
            // Basis(x_axis, y_axis, z_axis)
            3 => Ok(Variant::Basis(Basis {
                x: arg_vec3(args, 0, "Basis()")?,
                y: arg_vec3(args, 1, "Basis()")?,
                z: arg_vec3(args, 2, "Basis()")?,
            })),
            n => Err(bad_arity("Basis()", n)),
        },
        "AABB" => match args.len() {
            0 => Ok(Variant::Aabb(Aabb::new(Vector3::ZERO, Vector3::ZERO))),
            1 => Ok(Variant::Aabb(arg_aabb(args, 0, "AABB()")?)),
            2 => Ok(Variant::Aabb(Aabb::new(
                arg_vec3(args, 0, "AABB()")?,
                arg_vec3(args, 1, "AABB()")?,
            ))),
            // AABB(px, py, pz, sx, sy, sz) — the SERIALIZED form, mirroring the
            // 6-float `Transform2D` arm above (pat-0q2kx.69).
            //
            // ⚠️ Godot's *GDScript* AABB takes only `()`, `(AABB)` or
            // `(Vector3, Vector3)` — there is no 6-float script constructor.
            // This arm exists because `var_to_str` emits `AABB(1, 2, 3, 4, 5, 6)`
            // (variant_parser.cpp writes all six components) and `str_to_var`
            // has to read that back through this same shared constructor. Without
            // it an AABB survives serialization but not deserialization, which is
            // a silent one-way trip.
            6 => Ok(Variant::Aabb(Aabb::new(
                Vector3::new(
                    arg_num(args, 0, "AABB()")?,
                    arg_num(args, 1, "AABB()")?,
                    arg_num(args, 2, "AABB()")?,
                ),
                Vector3::new(
                    arg_num(args, 3, "AABB()")?,
                    arg_num(args, 4, "AABB()")?,
                    arg_num(args, 5, "AABB()")?,
                ),
            ))),
            n => Err(bad_arity("AABB()", n)),
        },
        "Plane" => match args.len() {
            0 => Ok(Variant::Plane(Plane::new(Vector3::ZERO, 0.0))),
            1 => match &args[0] {
                Variant::Plane(p) => Ok(Variant::Plane(*p)),
                other => Err(type_error(format!(
                    "Plane() cannot be built from {}",
                    other.variant_type()
                ))),
            },
            // Plane(normal, d) and Plane(normal, point): a Vector3 second
            // argument is a point the plane passes through.
            2 => match (&args[0], &args[1]) {
                (Variant::Vector3(normal), Variant::Vector3(point)) => {
                    Ok(Variant::Plane(Plane::new(*normal, normal.dot(*point))))
                }
                (Variant::Vector3(normal), d) => {
                    Ok(Variant::Plane(Plane::new(*normal, num(d, "Plane()")?)))
                }
                _ => Err(type_error("Plane() expects a Vector3 normal".into())),
            },
            // Plane(point_a, point_b, point_c)
            3 => Ok(Variant::Plane(Plane::from_points(
                arg_vec3(args, 0, "Plane()")?,
                arg_vec3(args, 1, "Plane()")?,
                arg_vec3(args, 2, "Plane()")?,
            ))),
            // Plane(a, b, c, d) — normal components plus distance.
            4 => Ok(Variant::Plane(Plane::new(
                Vector3::new(
                    arg_num(args, 0, "Plane()")?,
                    arg_num(args, 1, "Plane()")?,
                    arg_num(args, 2, "Plane()")?,
                ),
                arg_num(args, 3, "Plane()")?,
            ))),
            n => Err(bad_arity("Plane()", n)),
        },
        other => Err(type_error(format!("{other}() is not a math builtin"))),
    }
}

/// The shortest-arc rotation taking `from` onto `to`
/// (Godot's `Quaternion(Vector3 arc_from, Vector3 arc_to)`).
/// Godot's `Math::is_equal_approx`: a RELATIVE tolerance that scales with the
/// magnitude, with an absolute floor so values near zero still compare equal.
/// A fixed epsilon would call two large transforms unequal over rounding that
/// does not matter, and two tiny ones equal when they differ meaningfully.
fn approx(a: f32, b: f32) -> bool {
    if a == b {
        return true;
    }
    let tolerance = (1e-6_f32 * a.abs()).max(1e-6);
    (a - b).abs() < tolerance
}

fn vec2_equal_approx(a: Vector2, b: Vector2) -> bool {
    approx(a.x, b.x) && approx(a.y, b.y)
}

fn vec3_equal_approx(a: Vector3, b: Vector3) -> bool {
    approx(a.x, b.x) && approx(a.y, b.y) && approx(a.z, b.z)
}

fn basis_equal_approx(a: Basis, b: Basis) -> bool {
    vec3_equal_approx(a.x, b.x) && vec3_equal_approx(a.y, b.y) && vec3_equal_approx(a.z, b.z)
}

fn quaternion_from_arc(from: Vector3, to: Vector3) -> Quaternion {
    let c = from.cross(to);
    let d = from.dot(to);
    if d < -1.0 + 1e-6 {
        // Opposite vectors: any perpendicular axis gives a 180° turn.
        Quaternion::new(0.0, 1.0, 0.0, 0.0)
    } else {
        let s = ((1.0 + d) * 2.0).sqrt();
        let rs = 1.0 / s;
        Quaternion::new(c.x * rs, c.y * rs, c.z * rs, s * 0.5).normalized()
    }
}

// ---------------------------------------------------------------------------
// Static members (`Transform2D.IDENTITY`, `Basis.IDENTITY`, ...)
// ---------------------------------------------------------------------------

/// Resolves `TypeName.CONSTANT` for the math builtins.
pub(crate) fn static_member(type_name: &str, member: &str) -> Option<Variant> {
    match (type_name, member) {
        ("Transform2D", "IDENTITY") => Some(Variant::Transform2D(Transform2D::IDENTITY)),
        ("Transform2D", "FLIP_X") => Some(Variant::Transform2D(Transform2D {
            x: Vector2::new(-1.0, 0.0),
            y: Vector2::new(0.0, 1.0),
            origin: Vector2::ZERO,
        })),
        ("Transform2D", "FLIP_Y") => Some(Variant::Transform2D(Transform2D {
            x: Vector2::new(1.0, 0.0),
            y: Vector2::new(0.0, -1.0),
            origin: Vector2::ZERO,
        })),
        ("Transform3D", "IDENTITY") => Some(Variant::Transform3D(Transform3D::IDENTITY)),
        ("Basis", "IDENTITY") => Some(Variant::Basis(Basis::IDENTITY)),
        ("Quaternion", "IDENTITY") => Some(Variant::Quaternion(Quaternion::IDENTITY)),
        ("Plane", "PLANE_YZ") => Some(Variant::Plane(Plane::new(Vector3::new(1.0, 0.0, 0.0), 0.0))),
        ("Plane", "PLANE_XZ") => Some(Variant::Plane(Plane::new(Vector3::new(0.0, 1.0, 0.0), 0.0))),
        ("Plane", "PLANE_XY") => Some(Variant::Plane(Plane::new(Vector3::new(0.0, 0.0, 1.0), 0.0))),
        // All 146 Godot colour constants, from the table `gdcore` owns
        // (pat-s3rrv.58). The interpreter used to hardcode SIX inline, so
        // `Color.YELLOW`, `Color.CYAN`, `Color.ORANGE` and 140 others resolved
        // to `None`.
        ("Color", name) => Color::named(name).map(Variant::Color),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Member access
// ---------------------------------------------------------------------------

/// Reads a member of a math builtin (`t.origin`, `q.w`, `r.size`, ...).
///
/// `None` means "not a math builtin, or no such member", which lets the
/// interpreter keep its own error message for the general case.
pub(crate) fn member_get(value: &Variant, member: &str) -> Option<Variant> {
    match value {
        Variant::Rect2(r) => match member {
            "position" => Some(Variant::Vector2(r.position)),
            "size" => Some(Variant::Vector2(r.size)),
            "end" => Some(Variant::Vector2(r.end())),
            _ => None,
        },
        // The pat-s3rrv.59 builtins. Components come back as `Int` for the
        // integer types and `Float` for the float ones — handing back a Float
        // for a `Vector4i.x` would make `v.x == 3` fail the way an int vector
        // silently widened to a float used to (pat-nzgdu.78).
        Variant::Rect2i(r) => match member {
            "position" => Some(Variant::Vector2i(r.position)),
            "size" => Some(Variant::Vector2i(r.size)),
            "end" => Some(Variant::Vector2i(r.end())),
            _ => None,
        },
        Variant::Vector4(v) => match member {
            "x" => Some(Variant::Float(v.x as f64)),
            "y" => Some(Variant::Float(v.y as f64)),
            "z" => Some(Variant::Float(v.z as f64)),
            "w" => Some(Variant::Float(v.w as f64)),
            _ => None,
        },
        Variant::Vector4i(v) => match member {
            "x" => Some(Variant::Int(v.x as i64)),
            "y" => Some(Variant::Int(v.y as i64)),
            "z" => Some(Variant::Int(v.z as i64)),
            "w" => Some(Variant::Int(v.w as i64)),
            _ => None,
        },
        // 🔑 `Vector2i` and `Vector3i` had NO member arm anywhere — not here and
        // not in the interpreter's inline dispatch, which covers only Vector2,
        // Vector3, Color and Dictionary before falling through to this function.
        // So `.x` on an int vector raised "cannot access member 'x' on
        // Vector2i" from GDScript, for every script, since always.
        //
        // ⚠️ This is far wider than the pat-s3rrv.59 builtins that surfaced it.
        // `Vector2i` is what `Rect2i.size`, `Image.get_size()`, `DisplayServer`
        // screen sizes and every tile coordinate return, so `img.get_size().x`
        // — the idiom the comment at `image_alpha_ops_and_size.rs:70` describes
        // in passing — could not run. It was invisible because the existing
        // tests read those components in RUST, off the returned Variant, rather
        // than from a script.
        //
        // Components come back as `Int`, per the note above: a Float here would
        // make `v.x == 3` fail (pat-nzgdu.78).
        Variant::Vector2i(v) => match member {
            "x" => Some(Variant::Int(v.x as i64)),
            "y" => Some(Variant::Int(v.y as i64)),
            _ => None,
        },
        Variant::Vector3i(v) => match member {
            "x" => Some(Variant::Int(v.x as i64)),
            "y" => Some(Variant::Int(v.y as i64)),
            "z" => Some(Variant::Int(v.z as i64)),
            _ => None,
        },
        // ⚠️ A Projection's `x`/`y`/`z`/`w` are its four COLUMNS, each a whole
        // Vector4 (doc/classes/Projection.xml:268-277) — NOT scalar components
        // the way `Vector4.x` is. Storage is column-major, so `m[0]` really is
        // the `x` column; reading rows here would transpose every matrix a
        // script inspects.
        Variant::Projection(p) => {
            let column = |i: usize| {
                Some(Variant::Vector4(gdcore::math::Vector4::new(
                    p.m[i][0], p.m[i][1], p.m[i][2], p.m[i][3],
                )))
            };
            match member {
                "x" => column(0),
                "y" => column(1),
                "z" => column(2),
                "w" => column(3),
                _ => None,
            }
        }
        // Godot's Color members beyond the four channels (pat-s3rrv.58): the
        // 8-bit views and the HSV decomposition. `r/g/b/a` are handled by the
        // interpreter's own fast path, and are repeated here so a caller
        // reaching `member_get` directly still gets them.
        //
        // ⚠️ `h`/`s`/`v` are 0..1, NOT degrees — a script treating `col.h` as a
        // 0..360 hue gets a wildly wrong colour that still looks like a colour.
        Variant::Color(c) => match member {
            "r" => Some(Variant::Float(c.r as f64)),
            "g" => Some(Variant::Float(c.g as f64)),
            "b" => Some(Variant::Float(c.b as f64)),
            "a" => Some(Variant::Float(c.a as f64)),
            "r8" => Some(Variant::Int(c.r8())),
            "g8" => Some(Variant::Int(c.g8())),
            "b8" => Some(Variant::Int(c.b8())),
            "a8" => Some(Variant::Int(c.a8())),
            "h" => Some(Variant::Float(c.h() as f64)),
            "s" => Some(Variant::Float(c.s() as f64)),
            "v" => Some(Variant::Float(c.v() as f64)),
            // The Oklab decomposition (pat-s3rrv.58). Perceptually uniform, so
            // `ok_hsl_l` is NOT a rename of `v`: pure red has v = 1.0 and
            // ok_hsl_l = 0.568.
            "ok_hsl_h" => Some(Variant::Float(c.ok_hsl_h() as f64)),
            "ok_hsl_s" => Some(Variant::Float(c.ok_hsl_s() as f64)),
            "ok_hsl_l" => Some(Variant::Float(c.ok_hsl_l() as f64)),
            _ => None,
        },
        Variant::Transform2D(t) => match member {
            "x" => Some(Variant::Vector2(t.x)),
            "y" => Some(Variant::Vector2(t.y)),
            "origin" => Some(Variant::Vector2(t.origin)),
            _ => None,
        },
        Variant::Transform3D(t) => match member {
            "basis" => Some(Variant::Basis(t.basis)),
            "origin" => Some(Variant::Vector3(t.origin)),
            _ => None,
        },
        Variant::Basis(b) => match member {
            "x" => Some(Variant::Vector3(b.x)),
            "y" => Some(Variant::Vector3(b.y)),
            "z" => Some(Variant::Vector3(b.z)),
            _ => None,
        },
        Variant::Quaternion(q) => match member {
            "x" => Some(Variant::Float(q.x as f64)),
            "y" => Some(Variant::Float(q.y as f64)),
            "z" => Some(Variant::Float(q.z as f64)),
            "w" => Some(Variant::Float(q.w as f64)),
            _ => None,
        },
        Variant::Aabb(a) => match member {
            "position" => Some(Variant::Vector3(a.position)),
            "size" => Some(Variant::Vector3(a.size)),
            "end" => Some(Variant::Vector3(a.get_end())),
            _ => None,
        },
        Variant::Plane(p) => match member {
            "normal" => Some(Variant::Vector3(p.normal)),
            "d" => Some(Variant::Float(p.d as f64)),
            "x" => Some(Variant::Float(p.normal.x as f64)),
            "y" => Some(Variant::Float(p.normal.y as f64)),
            "z" => Some(Variant::Float(p.normal.z as f64)),
            _ => None,
        },
        _ => None,
    }
}

/// Writes a member of a math builtin, returning the updated value.
///
/// Godot's math builtins are value types, so this returns a new `Variant`
/// rather than mutating in place — the caller writes it back to the place the
/// original came from. `None` means "not a math builtin, or no such member".
pub(crate) fn member_set(
    value: &Variant,
    member: &str,
    new_value: &Variant,
) -> Option<Result<Variant, RuntimeError>> {
    // The member must exist before we bother type-checking the assigned value.
    member_get(value, member)?;
    let ctx = format!("{}.{member}", value.variant_type());
    Some(member_set_inner(value, member, new_value, &ctx))
}

fn member_set_inner(
    value: &Variant,
    member: &str,
    new_value: &Variant,
    ctx: &str,
) -> Result<Variant, RuntimeError> {
    let want_vec2 = |v: &Variant| match v {
        Variant::Vector2(v) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector2, got {}",
            other.variant_type()
        ))),
    };
    let want_vec3 = |v: &Variant| match v {
        Variant::Vector3(v) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector3, got {}",
            other.variant_type()
        ))),
    };
    let want_vec2i = |v: &Variant| match v {
        Variant::Vector2i(v) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector2i, got {}",
            other.variant_type()
        ))),
    };
    let want_vec4 = |v: &Variant| match v {
        Variant::Vector4(v) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector4, got {}",
            other.variant_type()
        ))),
    };
    // A component assignment accepts an Int where a float is wanted, because
    // GDScript's `5` and `5.0` are both ordinary ways to write a coordinate.
    let want_f32 = |v: &Variant| match v {
        Variant::Float(f) => Ok(*f as f32),
        Variant::Int(i) => Ok(*i as f32),
        other => Err(type_error(format!(
            "{ctx} expects a float, got {}",
            other.variant_type()
        ))),
    };
    let want_i32 = |v: &Variant| match v {
        Variant::Int(i) => Ok(*i as i32),
        other => Err(type_error(format!(
            "{ctx} expects an int, got {}",
            other.variant_type()
        ))),
    };
    // ⚠️ Deliberately NOT `want_i32`. The 8-bit colour members clamp to
    // 0..255, and narrowing first would wrap an out-of-range value before the
    // clamp ever saw it: `col.r8 = 4294967296` narrows to 0 and writes BLACK,
    // where clamping the full value writes white.
    let want_i64 = |v: &Variant| match v {
        Variant::Int(i) => Ok(*i),
        other => Err(type_error(format!(
            "{ctx} expects an int, got {}",
            other.variant_type()
        ))),
    };
    match value {
        Variant::Rect2(r) => {
            let mut r = *r;
            match member {
                "position" => r.position = want_vec2(new_value)?,
                "size" => r.size = want_vec2(new_value)?,
                // Godot keeps `position` fixed and moves `end` by resizing.
                "end" => r.size = want_vec2(new_value)? - r.position,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Rect2(r))
        }
        Variant::Rect2i(r) => {
            let mut r = *r;
            match member {
                "position" => r.position = want_vec2i(new_value)?,
                "size" => r.size = want_vec2i(new_value)?,
                // Godot keeps `position` fixed and moves `end` by resizing.
                "end" => r.size = want_vec2i(new_value)? - r.position,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Rect2i(r))
        }
        Variant::Vector4(v) => {
            let mut v = *v;
            let f = want_f32(new_value)?;
            match member {
                "x" => v.x = f,
                "y" => v.y = f,
                "z" => v.z = f,
                "w" => v.w = f,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Vector4(v))
        }
        Variant::Vector4i(v) => {
            let mut v = *v;
            let i = want_i32(new_value)?;
            match member {
                "x" => v.x = i,
                "y" => v.y = i,
                "z" => v.z = i,
                "w" => v.w = i,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Vector4i(v))
        }
        Variant::Projection(p) => {
            let mut p = *p;
            let col = want_vec4(new_value)?;
            let index = match member {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                "w" => 3,
                _ => unreachable!("member_get gated the member name"),
            };
            // Column-major storage: writing `m[index]` replaces a whole column,
            // which is exactly what assigning `proj.x` means.
            p.m[index] = [col.x, col.y, col.z, col.w];
            Ok(Variant::Projection(p))
        }
        // Godot exposes every Color member as read-WRITE (`col.r8 = 128`,
        // `col.h = 0.5`), so the write half mirrors `member_get`'s read half
        // above (pat-s3rrv.58). Without this arm `member_set` answered `None`
        // for Color and the assignment silently did nothing.
        Variant::Color(c) => {
            let c = *c;
            Ok(Variant::Color(match member {
                "r" => gdcore::math::Color::new(want_f32(new_value)?, c.g, c.b, c.a),
                "g" => gdcore::math::Color::new(c.r, want_f32(new_value)?, c.b, c.a),
                "b" => gdcore::math::Color::new(c.r, c.g, want_f32(new_value)?, c.a),
                "a" => gdcore::math::Color::new(c.r, c.g, c.b, want_f32(new_value)?),
                "r8" => c.with_r8(want_i64(new_value)?),
                "g8" => c.with_g8(want_i64(new_value)?),
                "b8" => c.with_b8(want_i64(new_value)?),
                "a8" => c.with_a8(want_i64(new_value)?),
                // ⚠️ These go through an HSV round-trip, so writing one
                // re-derives the other two. Setting the hue of a GREY colour is
                // a no-op — saturation 0 has no hue to move — which is Godot's
                // behaviour, not a gap.
                "h" => c.with_h(want_f32(new_value)?),
                "s" => c.with_s(want_f32(new_value)?),
                "v" => c.with_v(want_f32(new_value)?),
                "ok_hsl_h" => c.with_ok_hsl_h(want_f32(new_value)?),
                "ok_hsl_s" => c.with_ok_hsl_s(want_f32(new_value)?),
                "ok_hsl_l" => c.with_ok_hsl_l(want_f32(new_value)?),
                _ => unreachable!("member_get gated the member name"),
            }))
        }
        Variant::Transform2D(t) => {
            let mut t = *t;
            match member {
                "x" => t.x = want_vec2(new_value)?,
                "y" => t.y = want_vec2(new_value)?,
                "origin" => t.origin = want_vec2(new_value)?,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Transform2D(t))
        }
        Variant::Transform3D(t) => {
            let mut t = *t;
            match member {
                "basis" => match new_value {
                    Variant::Basis(b) => t.basis = *b,
                    other => {
                        return Err(type_error(format!(
                            "{ctx} expects a Basis, got {}",
                            other.variant_type()
                        )))
                    }
                },
                "origin" => t.origin = want_vec3(new_value)?,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Transform3D(t))
        }
        Variant::Basis(b) => {
            let mut b = *b;
            match member {
                "x" => b.x = want_vec3(new_value)?,
                "y" => b.y = want_vec3(new_value)?,
                "z" => b.z = want_vec3(new_value)?,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Basis(b))
        }
        Variant::Quaternion(q) => {
            let mut q = *q;
            let f = num(new_value, ctx)?;
            match member {
                "x" => q.x = f,
                "y" => q.y = f,
                "z" => q.z = f,
                "w" => q.w = f,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Quaternion(q))
        }
        Variant::Aabb(a) => {
            let mut a = *a;
            match member {
                "position" => a.position = want_vec3(new_value)?,
                "size" => a.size = want_vec3(new_value)?,
                "end" => a.size = want_vec3(new_value)? - a.position,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Aabb(a))
        }
        Variant::Plane(p) => {
            let mut p = *p;
            match member {
                "normal" => p.normal = want_vec3(new_value)?,
                "d" => p.d = num(new_value, ctx)?,
                "x" => p.normal.x = num(new_value, ctx)?,
                "y" => p.normal.y = num(new_value, ctx)?,
                "z" => p.normal.z = num(new_value, ctx)?,
                _ => unreachable!("member_get gated the member name"),
            }
            Ok(Variant::Plane(p))
        }
        _ => unreachable!("member_get gated the receiver type"),
    }
}

// ---------------------------------------------------------------------------
// Method dispatch
// ---------------------------------------------------------------------------

/// Calls a method on a math builtin.
///
/// `None` means the receiver is not one of the seven types handled here, so the
/// interpreter falls through to its own dispatch. A receiver that *is* one of
/// them but does not have the method yields `Some(Err(UndefinedFunction))`.
pub(crate) fn call_method(
    value: &Variant,
    method: &str,
    args: &[Variant],
) -> Option<Result<Variant, RuntimeError>> {
    match value {
        Variant::Rect2(r) => Some(rect2_method(*r, method, args)),
        Variant::Transform2D(t) => Some(transform2d_method(*t, method, args)),
        Variant::Transform3D(t) => Some(transform3d_method(*t, method, args)),
        Variant::Basis(b) => Some(basis_method(*b, method, args)),
        Variant::Quaternion(q) => Some(quaternion_method(*q, method, args)),
        Variant::Aabb(a) => Some(aabb_method(*a, method, args)),
        Variant::Plane(p) => Some(plane_method(*p, method, args)),
        Variant::Color(c) => Some(color_method(*c, method, args)),
        Variant::Rect2i(r) => Some(rect2i_method(*r, method, args)),
        Variant::Projection(p) => Some(projection_method(*p, method, args)),
        Variant::Vector4(v) => Some(vector4_method(*v, method, args)),
        Variant::Vector4i(v) => Some(vector4i_method(*v, method, args)),
        _ => None,
    }
}

/// `Color`'s builtin methods (pat-s3rrv.58).
///
/// Every one forwards to the already-tested `gdcore::math::Color`
/// implementation rather than recomputing anything here, so the interpreter and
/// the engine cannot drift apart on what `darkened` means.
fn color_method(c: Color, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Color.{method}()");
    match method {
        // -- derived colours --
        "inverted" => Ok(Variant::Color(c.inverted())),
        "lightened" => Ok(Variant::Color(c.lightened(arg_num(args, 0, &ctx)?))),
        "darkened" => Ok(Variant::Color(c.darkened(arg_num(args, 0, &ctx)?))),
        "blend" => Ok(Variant::Color(c.blend(arg_color(args, 0, &ctx)?))),
        "lerp" => Ok(Variant::Color(
            c.lerp(arg_color(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        "clamp" => {
            // Godot's defaults are the 0..1 corners.
            let lo = args
                .first()
                .map(|_| arg_color(args, 0, &ctx))
                .transpose()?
                .unwrap_or(Color::new(0.0, 0.0, 0.0, 0.0));
            let hi = args
                .get(1)
                .map(|_| arg_color(args, 1, &ctx))
                .transpose()?
                .unwrap_or(Color::WHITE);
            Ok(Variant::Color(c.clamp_to(lo, hi)))
        }

        // -- scalars --
        "get_luminance" => Ok(Variant::Float(c.get_luminance() as f64)),
        "is_equal_approx" => Ok(Variant::Bool(c.is_equal_approx(arg_color(args, 0, &ctx)?))),

        // -- colour spaces --
        "srgb_to_linear" => Ok(Variant::Color(c.srgb_to_linear())),
        "linear_to_srgb" => Ok(Variant::Color(c.linear_to_srgb())),

        // -- serialisation --
        //
        // ⚠️ `to_html` defaults to WITH alpha in Godot, so a bare `to_html()`
        // yields 8 digits. Defaulting to false would quietly drop alpha from
        // every colour a script serialises.
        "to_html" => {
            let with_alpha = match args.first() {
                Some(Variant::Bool(b)) => *b,
                None => true,
                Some(other) => {
                    return Err(type_error(format!("{ctx} expects a bool, got {other:?}")))
                }
            };
            Ok(Variant::String(c.to_html(with_alpha)))
        }
        "to_argb32" => Ok(Variant::Int(c.to_argb32())),
        "to_abgr32" => Ok(Variant::Int(c.to_abgr32())),
        "to_rgba32" => Ok(Variant::Int(c.to_rgba32())),
        "to_argb64" => Ok(Variant::Int(c.to_argb64())),
        "to_abgr64" => Ok(Variant::Int(c.to_abgr64())),
        "to_rgba64" => Ok(Variant::Int(c.to_rgba64())),

        _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Color.{method}"),
        ))),
    }
}

/// A `Color` argument, accepting only a real colour — a script passing a number
/// where a colour belongs gets a named error rather than a silent black.
fn arg_color(args: &[Variant], idx: usize, ctx: &str) -> Result<Color, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Color(c)) => Ok(*c),
        Some(other) => Err(type_error(format!(
            "{ctx} expects a Color for argument {idx}, got {other:?}"
        ))),
        None => Err(type_error(format!("{ctx} is missing argument {idx}"))),
    }
}

fn rect2_method(r: Rect2, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Rect2.{method}()");
    match method {
        "get_center" => Ok(Variant::Vector2(r.get_center())),
        "get_area" => Ok(Variant::Float(r.area() as f64)),
        "has_area" => Ok(Variant::Bool(r.has_area())),
        "has_point" => Ok(Variant::Bool(r.contains_point(arg_vec2(args, 0, &ctx)?))),
        "encloses" => Ok(Variant::Bool(r.encloses(arg_rect2(args, 0, &ctx)?))),
        "intersects" => Ok(Variant::Bool(r.intersects(arg_rect2(args, 0, &ctx)?))),
        "intersection" => Ok(Variant::Rect2(r.intersection(arg_rect2(args, 0, &ctx)?))),
        "merge" => Ok(Variant::Rect2(r.merge(arg_rect2(args, 0, &ctx)?))),
        "expand" => Ok(Variant::Rect2(r.expand(arg_vec2(args, 0, &ctx)?))),
        "grow" => Ok(Variant::Rect2(r.grow(arg_num(args, 0, &ctx)?))),
        "grow_individual" => Ok(Variant::Rect2(r.grow_individual(
            arg_num(args, 0, &ctx)?,
            arg_num(args, 1, &ctx)?,
            arg_num(args, 2, &ctx)?,
            arg_num(args, 3, &ctx)?,
        ))),
        "grow_side" => {
            let side = match args.first() {
                Some(Variant::Int(i)) => *i,
                other => {
                    return Err(type_error(format!(
                        "{ctx} expects a Side constant, got {}",
                        describe(other)
                    )))
                }
            };
            Ok(Variant::Rect2(r.grow_side(side, arg_num(args, 1, &ctx)?)))
        }
        "abs" => Ok(Variant::Rect2(r.abs())),
        "get_support" => Ok(Variant::Vector2(r.get_support(arg_vec2(args, 0, &ctx)?))),
        "is_equal_approx" => Ok(Variant::Bool(r.is_equal_approx(arg_rect2(args, 0, &ctx)?))),
        "is_finite" => Ok(Variant::Bool(r.is_finite())),
        _ => Err(undefined_method("Rect2", method)),
    }
}

fn transform2d_method(
    t: Transform2D,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    let ctx = format!("Transform2D.{method}()");
    match method {
        // Godot's `inverse()` is only valid for an orthonormal transform;
        // `affine_inverse()` is the general form and agrees with it there, so
        // both names route to the general one.
        "inverse" | "affine_inverse" => Ok(Variant::Transform2D(t.affine_inverse())),
        "xform" => Ok(Variant::Vector2(t.xform(arg_vec2(args, 0, &ctx)?))),
        "basis_xform" => Ok(Variant::Vector2(t.basis_xform(arg_vec2(args, 0, &ctx)?))),
        "basis_xform_inv" => {
            let v = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Vector2(Vector2::new(t.x.dot(v), t.y.dot(v))))
        }
        // The bare forms are Godot's *global* (left-multiplied) variants; the
        // `_local` forms multiply on the right.
        "rotated" => Ok(Variant::Transform2D(
            Transform2D::rotated(arg_num(args, 0, &ctx)?) * t,
        )),
        "rotated_local" => Ok(Variant::Transform2D(
            t * Transform2D::rotated(arg_num(args, 0, &ctx)?),
        )),
        "scaled" => Ok(Variant::Transform2D(
            Transform2D::scaled(arg_vec2(args, 0, &ctx)?) * t,
        )),
        "scaled_local" => Ok(Variant::Transform2D(
            t * Transform2D::scaled(arg_vec2(args, 0, &ctx)?),
        )),
        "translated" => Ok(Variant::Transform2D(
            Transform2D::translated(arg_vec2(args, 0, &ctx)?) * t,
        )),
        "translated_local" => Ok(Variant::Transform2D(
            t * Transform2D::translated(arg_vec2(args, 0, &ctx)?),
        )),
        "get_origin" => Ok(Variant::Vector2(t.origin)),
        "get_rotation" => Ok(Variant::Float(t.get_rotation() as f64)),
        "get_scale" => Ok(Variant::Vector2(t.get_scale())),
        "get_skew" => Ok(Variant::Float(t.get_skew() as f64)),
        "determinant" => Ok(Variant::Float(t.determinant() as f64)),
        "orthonormalized" => Ok(Variant::Transform2D(t.orthonormalized())),
        "interpolate_with" => Ok(Variant::Transform2D(
            t.interpolate_with(arg_transform2d(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        // Godot compares every component with `Math::is_equal_approx`, i.e. a
        // RELATIVE epsilon — a plain `==` on floats would report two transforms
        // that differ only by accumulated rounding as unequal (pat-0q2kx.66).
        "is_equal_approx" => {
            let o = arg_transform2d(args, 0, &ctx)?;
            Ok(Variant::Bool(
                vec2_equal_approx(t.x, o.x)
                    && vec2_equal_approx(t.y, o.y)
                    && vec2_equal_approx(t.origin, o.origin),
            ))
        }
        // ⚠️ NOT the same predicate as `Basis.is_conformal` — see the note on
        // `Transform2D::is_conformal`.
        "is_conformal" => Ok(Variant::Bool(t.is_conformal())),
        "is_finite" => Ok(Variant::Bool(t.is_finite())),
        "looking_at" => Ok(Variant::Transform2D(t.looking_at(arg_vec2(args, 0, &ctx)?))),
        _ => Err(undefined_method("Transform2D", method)),
    }
}

fn transform3d_method(
    t: Transform3D,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    let ctx = format!("Transform3D.{method}()");
    let scale_basis = |s: Vector3| Transform3D {
        basis: Basis {
            x: Vector3::new(s.x, 0.0, 0.0),
            y: Vector3::new(0.0, s.y, 0.0),
            z: Vector3::new(0.0, 0.0, s.z),
        },
        origin: Vector3::ZERO,
    };
    match method {
        "inverse" | "affine_inverse" => Ok(Variant::Transform3D(t.inverse())),
        "xform" => Ok(Variant::Vector3(t.xform(arg_vec3(args, 0, &ctx)?))),
        "translated" => Ok(Variant::Transform3D(t.translated(arg_vec3(args, 0, &ctx)?))),
        "translated_local" => {
            let offset = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Transform3D(
                t * Transform3D {
                    basis: Basis::IDENTITY,
                    origin: offset,
                },
            ))
        }
        "rotated" => Ok(Variant::Transform3D(
            t.rotated(arg_vec3(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        "rotated_local" => {
            let rot = Basis::from_quaternion(Quaternion::from_axis_angle(
                arg_vec3(args, 0, &ctx)?,
                arg_num(args, 1, &ctx)?,
            ));
            Ok(Variant::Transform3D(
                t * Transform3D {
                    basis: rot,
                    origin: Vector3::ZERO,
                },
            ))
        }
        "scaled" => Ok(Variant::Transform3D(t.scaled(arg_vec3(args, 0, &ctx)?))),
        "scaled_local" => Ok(Variant::Transform3D(
            t * scale_basis(arg_vec3(args, 0, &ctx)?),
        )),
        "looking_at" => {
            let target = arg_vec3(args, 0, &ctx)?;
            let up = if args.len() > 1 {
                arg_vec3(args, 1, &ctx)?
            } else {
                Vector3::UP
            };
            Ok(Variant::Transform3D(t.looking_at(target, up)))
        }
        "orthonormalized" => Ok(Variant::Transform3D(Transform3D {
            basis: t.basis.orthonormalized(),
            origin: t.origin,
        })),
        "interpolate_with" => Ok(Variant::Transform3D(
            t.interpolate_with(arg_transform3d(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        "is_equal_approx" => {
            let o = arg_transform3d(args, 0, &ctx)?;
            Ok(Variant::Bool(
                basis_equal_approx(t.basis, o.basis) && vec3_equal_approx(t.origin, o.origin),
            ))
        }
        _ => Err(undefined_method("Transform3D", method)),
    }
}

fn basis_method(b: Basis, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Basis.{method}()");
    match method {
        "determinant" => Ok(Variant::Float(b.determinant() as f64)),
        "inverse" => Ok(Variant::Basis(b.inverse())),
        "transposed" => Ok(Variant::Basis(b.transposed())),
        "orthonormalized" => Ok(Variant::Basis(b.orthonormalized())),
        "get_scale" => Ok(Variant::Vector3(b.get_scale())),
        // 🔑 The order argument was DISCARDED here (pat-s3rrv.148). A script
        // asking for XYZ received YXZ angles — a plausible rotation of the
        // wrong shape, with nothing to indicate the argument had been ignored.
        "get_euler" => Ok(Variant::Vector3(
            b.to_euler_order(arg_euler_order(args, 0, &ctx)?),
        )),
        "scaled_local" => Ok(Variant::Basis(b.scaled_local(arg_vec3(args, 0, &ctx)?))),
        "is_conformal" => Ok(Variant::Bool(b.is_conformal())),
        "is_finite" => Ok(Variant::Bool(b.is_finite())),
        // ⚠️ The "transposed dot" family: these take the basis's ROWS, not its
        // columns. For a symmetric basis the two agree, which is why a wrong
        // port survives casual testing.
        "tdotx" => Ok(Variant::Float(b.tdotx(arg_vec3(args, 0, &ctx)?) as f64)),
        "tdoty" => Ok(Variant::Float(b.tdoty(arg_vec3(args, 0, &ctx)?) as f64)),
        "tdotz" => Ok(Variant::Float(b.tdotz(arg_vec3(args, 0, &ctx)?) as f64)),
        "get_rotation_quaternion" => Ok(Variant::Quaternion(b.get_rotation_quaternion())),
        "xform" => Ok(Variant::Vector3(b.xform(arg_vec3(args, 0, &ctx)?))),
        "xform_inv" => Ok(Variant::Vector3(b.xform_inv(arg_vec3(args, 0, &ctx)?))),
        "rotated" => Ok(Variant::Basis(
            b.rotated(arg_vec3(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        "scaled" => {
            let s = arg_vec3(args, 0, &ctx)?;
            // Godot scales each row, i.e. each column component-wise by the
            // matching axis factor.
            Ok(Variant::Basis(Basis {
                x: Vector3::new(b.x.x * s.x, b.x.y * s.y, b.x.z * s.z),
                y: Vector3::new(b.y.x * s.x, b.y.y * s.y, b.y.z * s.z),
                z: Vector3::new(b.z.x * s.x, b.z.y * s.y, b.z.z * s.z),
            }))
        }
        "is_orthonormal" => Ok(Variant::Bool(b.is_orthonormal(1e-4))),
        // Godot slerps the rotation and lerps the scale rather than lerping the
        // matrix entries, which would shear the basis mid-way.
        "slerp" => {
            let to = arg_basis(args, 0, &ctx)?;
            let weight = arg_num(args, 1, &ctx)?;
            Ok(Variant::Basis(Basis::from_quaternion_scale(
                b.get_rotation_quaternion()
                    .slerp(to.get_rotation_quaternion(), weight)
                    .normalized(),
                b.get_scale().lerp(to.get_scale(), weight),
            )))
        }
        "is_equal_approx" => Ok(Variant::Bool(basis_equal_approx(
            b,
            arg_basis(args, 0, &ctx)?,
        ))),
        _ => Err(undefined_method("Basis", method)),
    }
}

fn quaternion_method(
    q: Quaternion,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    let ctx = format!("Quaternion.{method}()");
    let length_squared = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    match method {
        "length" => Ok(Variant::Float(q.length() as f64)),
        "length_squared" => Ok(Variant::Float(length_squared as f64)),
        "normalized" => Ok(Variant::Quaternion(q.normalized())),
        "is_normalized" => Ok(Variant::Bool((length_squared - 1.0).abs() < 1e-5)),
        "inverse" => Ok(Variant::Quaternion(q.inverse())),
        "dot" => Ok(Variant::Float(q.dot(arg_quat(args, 0, &ctx)?) as f64)),
        "slerp" => Ok(Variant::Quaternion(
            q.slerp(arg_quat(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        // Same discarded-order bug as `Basis.get_euler` (pat-s3rrv.148).
        "get_euler" => Ok(Variant::Vector3(
            q.to_euler_order(arg_euler_order(args, 0, &ctx)?),
        )),
        "slerpni" => Ok(Variant::Quaternion(
            q.slerpni(arg_quat(args, 0, &ctx)?, arg_num(args, 1, &ctx)?),
        )),
        "spherical_cubic_interpolate" => Ok(Variant::Quaternion(q.spherical_cubic_interpolate(
            arg_quat(args, 0, &ctx)?,
            arg_quat(args, 1, &ctx)?,
            arg_quat(args, 2, &ctx)?,
            arg_num(args, 3, &ctx)?,
        ))),
        "spherical_cubic_interpolate_in_time" => {
            Ok(Variant::Quaternion(q.spherical_cubic_interpolate_in_time(
                arg_quat(args, 0, &ctx)?,
                arg_quat(args, 1, &ctx)?,
                arg_quat(args, 2, &ctx)?,
                arg_num(args, 3, &ctx)?,
                arg_num(args, 4, &ctx)?,
                arg_num(args, 5, &ctx)?,
                arg_num(args, 6, &ctx)?,
            )))
        }
        "exp" => Ok(Variant::Quaternion(q.exp())),
        "log" => Ok(Variant::Quaternion(q.log())),
        "is_finite" => Ok(Variant::Bool(q.is_finite())),
        "xform" => Ok(Variant::Vector3(q.xform(arg_vec3(args, 0, &ctx)?))),
        "get_angle" => Ok(Variant::Float((2.0 * q.w.clamp(-1.0, 1.0).acos()) as f64)),
        // ⚠️ The degenerate answer here USED to be `+Z`. Godot returns the raw
        // imaginary part (`quaternion.cpp`, `get_axis`), and `Quaternion.log()`
        // — added in pat-s3rrv.148 — is built on the axis, so a second,
        // disagreeing definition of the same method would have made `log` and
        // `get_axis` describe different rotations. Now both read gdcore's.
        "get_axis" => Ok(Variant::Vector3(q.get_axis())),
        "angle_to" => {
            let d = q.dot(arg_quat(args, 0, &ctx)?);
            Ok(Variant::Float(
                (2.0 * d * d - 1.0).clamp(-1.0, 1.0).acos() as f64
            ))
        }
        "is_equal_approx" => {
            let o = arg_quat(args, 0, &ctx)?;
            Ok(Variant::Bool(
                approx(q.x, o.x) && approx(q.y, o.y) && approx(q.z, o.z) && approx(q.w, o.w),
            ))
        }
        _ => Err(undefined_method("Quaternion", method)),
    }
}

fn aabb_method(a: Aabb, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("AABB.{method}()");
    match method {
        "get_center" => Ok(Variant::Vector3(a.get_center())),
        "get_volume" => Ok(Variant::Float(a.get_volume() as f64)),
        "has_volume" => Ok(Variant::Bool(a.has_volume())),
        "has_point" => Ok(Variant::Bool(a.contains_point(arg_vec3(args, 0, &ctx)?))),
        "intersects" => Ok(Variant::Bool(a.intersects(arg_aabb(args, 0, &ctx)?))),
        "intersection" => Ok(Variant::Aabb(a.intersection(arg_aabb(args, 0, &ctx)?))),
        "encloses" => Ok(Variant::Bool(a.encloses(arg_aabb(args, 0, &ctx)?))),
        "merge" => Ok(Variant::Aabb(a.merge(arg_aabb(args, 0, &ctx)?))),
        "expand" => Ok(Variant::Aabb(a.expand(arg_vec3(args, 0, &ctx)?))),
        "grow" => Ok(Variant::Aabb(a.grow(arg_num(args, 0, &ctx)?))),
        "abs" => Ok(Variant::Aabb(a.abs())),
        "get_end" => Ok(Variant::Vector3(a.get_end())),
        "get_position" => Ok(Variant::Vector3(a.position)),
        "get_size" => Ok(Variant::Vector3(a.size)),
        "get_endpoint" => {
            let idx = match args.first() {
                Some(Variant::Int(i)) if (0..8).contains(i) => *i as u8,
                other => {
                    return Err(type_error(format!(
                        "{ctx} expects a corner index 0-7, got {}",
                        describe(other)
                    )))
                }
            };
            Ok(Variant::Vector3(a.get_endpoint(idx)))
        }
        "get_longest_axis" => Ok(Variant::Vector3(a.get_longest_axis())),
        "get_longest_axis_index" => Ok(Variant::Int(a.get_longest_axis_index())),
        "get_longest_axis_size" => Ok(Variant::Float(a.get_longest_axis_size() as f64)),
        "get_shortest_axis" => Ok(Variant::Vector3(a.get_shortest_axis())),
        "get_shortest_axis_index" => Ok(Variant::Int(a.get_shortest_axis_index())),
        "get_shortest_axis_size" => Ok(Variant::Float(a.get_shortest_axis_size() as f64)),
        "get_support" => Ok(Variant::Vector3(a.get_support(arg_vec3(args, 0, &ctx)?))),
        // ⚠️ `||` over the extents, not `&&`: a flat box still has surface.
        // This is the whole difference from `has_volume`.
        "has_surface" => Ok(Variant::Bool(a.has_surface())),
        "intersects_plane" => Ok(Variant::Bool(a.intersects_plane(arg_plane(args, 0, &ctx)?))),
        // 🔑 These two return a Vector3 **or null**, not a bool — Godot binds
        // the `_bind` wrappers that hand back the hit POINT
        // (`aabb.cpp`, `intersects_ray_bind`). Returning a bool would compile,
        // read naturally, and silently drop the answer scripts actually pick
        // with.
        "intersects_ray" => {
            let from = arg_vec3(args, 0, &ctx)?;
            let dir = arg_vec3(args, 1, &ctx)?;
            Ok(a.intersects_ray(from, dir)
                .map_or(Variant::Nil, Variant::Vector3))
        }
        "intersects_segment" => {
            let from = arg_vec3(args, 0, &ctx)?;
            let to = arg_vec3(args, 1, &ctx)?;
            Ok(a.intersects_segment(from, to)
                .map_or(Variant::Nil, Variant::Vector3))
        }
        "is_equal_approx" => {
            let o = arg_aabb(args, 0, &ctx)?;
            Ok(Variant::Bool(
                vec3_equal_approx(a.position, o.position) && vec3_equal_approx(a.size, o.size),
            ))
        }
        "is_finite" => Ok(Variant::Bool(a.is_finite())),
        _ => Err(undefined_method("AABB", method)),
    }
}

fn plane_method(p: Plane, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Plane.{method}()");
    match method {
        "distance_to" => Ok(Variant::Float(
            p.distance_to(arg_vec3(args, 0, &ctx)?) as f64
        )),
        "is_point_over" => Ok(Variant::Bool(p.is_point_over(arg_vec3(args, 0, &ctx)?))),
        "has_point" => {
            let point = arg_vec3(args, 0, &ctx)?;
            let tolerance = if args.len() > 1 {
                arg_num(args, 1, &ctx)?
            } else {
                1e-5
            };
            Ok(Variant::Bool(p.has_point(point, tolerance)))
        }
        "normalized" => Ok(Variant::Plane(p.normalized())),
        "project" => Ok(Variant::Vector3(p.project(arg_vec3(args, 0, &ctx)?))),
        "get_center" => Ok(Variant::Vector3(p.get_center())),
        // Godot returns `null` when the ray or segment misses the plane.
        "intersects_ray" => Ok(p
            .intersects_ray(arg_vec3(args, 0, &ctx)?, arg_vec3(args, 1, &ctx)?)
            .map(Variant::Vector3)
            .unwrap_or(Variant::Nil)),
        "intersects_segment" => Ok(p
            .intersects_segment(arg_vec3(args, 0, &ctx)?, arg_vec3(args, 1, &ctx)?)
            .map(Variant::Vector3)
            .unwrap_or(Variant::Nil)),
        // Null when any two of the three are parallel, so a script cannot read
        // a garbage point as a real corner.
        "intersect_3" => {
            let b = arg_plane(args, 0, &ctx)?;
            let c = arg_plane(args, 1, &ctx)?;
            Ok(p.intersect_3(b, c).map_or(Variant::Nil, Variant::Vector3))
        }
        "is_equal_approx" => {
            let o = arg_plane(args, 0, &ctx)?;
            Ok(Variant::Bool(
                vec3_equal_approx(p.normal, o.normal) && (p.d - o.d).abs() < 1e-5,
            ))
        }
        "is_finite" => Ok(Variant::Bool(p.is_finite())),
        _ => Err(undefined_method("Plane", method)),
    }
}

// ---------------------------------------------------------------------------
// The `*` operator
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The Variant operator matrix (pat-mgixk.11)
// ---------------------------------------------------------------------------
//
// Godot registers +, -, *, /, % and unary - for every builtin pair in
// `core/variant/variant_op.cpp` `_register_variant_operators`. The interpreter
// only ever grew the handful of arms a given bead needed, so 186 of the 271
// non-equality rows raised `cannot multiply X and Y` at runtime — everyday
// idioms like `velocity * Vector2(1, 0)`, `cell * 16` and `modulate * 0.5`
// aborted the script.
//
// The table lives here rather than in `interpreter.rs` for two reasons: the
// arithmetic belongs next to the math types, and `interpreter.rs` is the single
// most contended file in the workspace — it gets only the fallback call sites.
//
// ⚠️ Every row below was transcribed from the upstream registration table, not
// inferred, because the result *type* is not guessable from the operands:
//
// * `Vector2i * int` -> **Vector2i** (stays a grid coordinate), but
//   `Vector2i * float` -> **Vector2**. Same for Vector3i/Vector4i. Getting this
//   backwards silently turns integer cell maths into floats, or truncates a
//   scaled position.
// * `Vector2 * Transform2D` is the INVERSE transform (`XFormInv`), while
//   `Transform2D * Vector2` is the forward one. The operand order is the whole
//   difference.
// * int-vector `/` and `%` are `DivNZ`/`ModNZ`: a zero divisor is a script
//   error, unlike float division which yields inf.
//
// Rows deliberately NOT implemented here, because Patina's math types have no
// helper with the verified convention yet: `Rect2 * Transform2D`,
// `Aabb * Transform3D`, `Plane * Transform3D`, `Transform3D * Plane`,
// `Vector4 * Projection`, `Projection * Vector4`. They keep the old TypeError.

/// Int-vector components are `i32` and Godot's are plain C `int32_t`, which
/// wraps rather than trapping. A debug Rust build would PANIC on the same
/// input, turning a wrapped coordinate into a crash, so the integer rows below
/// are all `wrapping_*`.
fn as_i32(scalar: i64) -> i32 {
    scalar as i32
}

/// `Color` has no arithmetic impls in `gdcore`, so every colour row is spelled
/// out componentwise here. (The bead claimed the type "already has the
/// arithmetic"; it does not — `grep "impl .* for Color"` finds no `Add`/`Mul`.)
fn color_zip(a: Color, b: Color, f: impl Fn(f32, f32) -> f32) -> Color {
    Color::new(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b), f(a.a, b.a))
}

fn color_map(c: Color, f: impl Fn(f32) -> f32) -> Color {
    Color::new(f(c.r), f(c.g), f(c.b), f(c.a))
}

fn quat_zip(a: Quaternion, b: Quaternion, f: impl Fn(f32, f32) -> f32) -> Quaternion {
    Quaternion::new(f(a.x, b.x), f(a.y, b.y), f(a.z, b.z), f(a.w, b.w))
}

/// `Basis * float` scales all three columns (`basis.h operator*(real_t)`).
fn scale_basis(b: Basis, s: f32) -> Basis {
    Basis {
        x: b.x * s,
        y: b.y * s,
        z: b.z * s,
    }
}

/// `Transform2D * float` scales the basis columns AND the origin
/// (`transform_2d.h operator*(real_t)`).
fn scale_transform2d(t: Transform2D, s: f32) -> Transform2D {
    Transform2D {
        x: t.x * s,
        y: t.y * s,
        origin: t.origin * s,
    }
}

/// `Transform3D * float`, likewise.
fn scale_transform3d(t: Transform3D, s: f32) -> Transform3D {
    Transform3D {
        basis: scale_basis(t.basis, s),
        origin: t.origin * s,
    }
}

/// Godot's `Transform2D::xform_inv`: `basis_xform_inv(v - origin)`, where
/// `basis_xform_inv` dots the *columns* with the vector
/// (`transform_2d.h:208`). Patina's `Transform2D` stores `x`/`y` as those same
/// columns, so this is a direct transcription rather than a transpose-by-index.
fn transform2d_xform_inv(t: Transform2D, v: Vector2) -> Vector2 {
    let d = v - t.origin;
    Vector2::new(t.x.dot(d), t.y.dot(d))
}

/// Godot's `Transform3D::xform_inv`: the basis' inverse applied to the
/// translated point. `Basis::xform_inv` is already the transpose-apply and
/// matches upstream for Patina's column storage, so it is reused rather than
/// re-derived.
fn transform3d_xform_inv(t: Transform3D, v: Vector3) -> Vector3 {
    t.basis.xform_inv(v - t.origin)
}

/// Applies `+` for the math builtins and the container concatenations.
///
/// `None` means the pair has no `+` row, so the interpreter keeps its own
/// "cannot add" error.
pub(crate) fn add(lhs: &Variant, rhs: &Variant) -> Option<Variant> {
    match (lhs, rhs) {
        (Variant::Vector4(a), Variant::Vector4(b)) => Some(Variant::Vector4(*a + *b)),
        (Variant::Vector4i(a), Variant::Vector4i(b)) => Some(Variant::Vector4i(*a + *b)),
        (Variant::Color(a), Variant::Color(b)) => {
            Some(Variant::Color(color_zip(*a, *b, |x, y| x + y)))
        }
        (Variant::Quaternion(a), Variant::Quaternion(b)) => {
            Some(Variant::Quaternion(quat_zip(*a, *b, |x, y| x + y)))
        }
        // `Array + Array` concatenates into a NEW, UNTYPED array
        // (`OperatorEvaluatorAddArray`): it must not alias either operand, or
        // `var c = a + b` would mutate `a` when `c` is appended to.
        (Variant::Array(a), Variant::Array(b)) => {
            let mut out = a.to_vec();
            out.extend(b.to_vec());
            Some(Variant::Array(gdvariant::SharedArray::from_vec(out)))
        }
        _ => packed_concat(lhs, rhs),
    }
}

/// `Packed*Array + Packed*Array` concatenation (`OperatorEvaluatorAppendArray`),
/// which is registered for all ten packed types and only between two arrays of
/// the SAME type.
fn packed_concat(lhs: &Variant, rhs: &Variant) -> Option<Variant> {
    macro_rules! cat {
        ($variant:ident, $a:expr, $b:expr) => {{
            let mut out = $a.clone();
            out.extend_from_slice($b);
            Some(Variant::$variant(out))
        }};
    }
    match (lhs, rhs) {
        (Variant::PackedByteArray(a), Variant::PackedByteArray(b)) => cat!(PackedByteArray, a, b),
        (Variant::PackedInt32Array(a), Variant::PackedInt32Array(b)) => {
            cat!(PackedInt32Array, a, b)
        }
        (Variant::PackedInt64Array(a), Variant::PackedInt64Array(b)) => {
            cat!(PackedInt64Array, a, b)
        }
        (Variant::PackedFloat32Array(a), Variant::PackedFloat32Array(b)) => {
            cat!(PackedFloat32Array, a, b)
        }
        (Variant::PackedFloat64Array(a), Variant::PackedFloat64Array(b)) => {
            cat!(PackedFloat64Array, a, b)
        }
        (Variant::PackedStringArray(a), Variant::PackedStringArray(b)) => {
            cat!(PackedStringArray, a, b)
        }
        (Variant::PackedVector2Array(a), Variant::PackedVector2Array(b)) => {
            cat!(PackedVector2Array, a, b)
        }
        (Variant::PackedVector3Array(a), Variant::PackedVector3Array(b)) => {
            cat!(PackedVector3Array, a, b)
        }
        (Variant::PackedVector4Array(a), Variant::PackedVector4Array(b)) => {
            cat!(PackedVector4Array, a, b)
        }
        (Variant::PackedColorArray(a), Variant::PackedColorArray(b)) => {
            cat!(PackedColorArray, a, b)
        }
        _ => None,
    }
}

/// Applies `-` for the math builtins. There is no `-` row for any container.
pub(crate) fn sub(lhs: &Variant, rhs: &Variant) -> Option<Variant> {
    match (lhs, rhs) {
        (Variant::Vector4(a), Variant::Vector4(b)) => Some(Variant::Vector4(*a - *b)),
        (Variant::Vector4i(a), Variant::Vector4i(b)) => Some(Variant::Vector4i(*a - *b)),
        (Variant::Color(a), Variant::Color(b)) => {
            Some(Variant::Color(color_zip(*a, *b, |x, y| x - y)))
        }
        (Variant::Quaternion(a), Variant::Quaternion(b)) => {
            Some(Variant::Quaternion(quat_zip(*a, *b, |x, y| x - y)))
        }
        _ => None,
    }
}

/// Applies `*` where at least one side is a math builtin: componentwise
/// products, scalar scaling, transform × point, transform × transform, and the
/// inverse-transform rows where the vector is on the LEFT.
///
/// `None` means this pair is not a math-builtin product, so the interpreter
/// keeps its own "cannot multiply" error.
pub(crate) fn mul(lhs: &Variant, rhs: &Variant) -> Option<Variant> {
    match (lhs, rhs) {
        // -- componentwise vector products --------------------------------
        (Variant::Vector2(a), Variant::Vector2(b)) => {
            Some(Variant::Vector2(Vector2::new(a.x * b.x, a.y * b.y)))
        }
        (Variant::Vector3(a), Variant::Vector3(b)) => Some(Variant::Vector3(Vector3::new(
            a.x * b.x,
            a.y * b.y,
            a.z * b.z,
        ))),
        (Variant::Vector4(a), Variant::Vector4(b)) => Some(Variant::Vector4(Vector4::new(
            a.x * b.x,
            a.y * b.y,
            a.z * b.z,
            a.w * b.w,
        ))),
        (Variant::Vector2i(a), Variant::Vector2i(b)) => Some(Variant::Vector2i(Vector2i::new(
            a.x.wrapping_mul(b.x),
            a.y.wrapping_mul(b.y),
        ))),
        (Variant::Vector3i(a), Variant::Vector3i(b)) => Some(Variant::Vector3i(Vector3i::new(
            a.x.wrapping_mul(b.x),
            a.y.wrapping_mul(b.y),
            a.z.wrapping_mul(b.z),
        ))),
        (Variant::Vector4i(a), Variant::Vector4i(b)) => Some(Variant::Vector4i(Vector4i::new(
            a.x.wrapping_mul(b.x),
            a.y.wrapping_mul(b.y),
            a.z.wrapping_mul(b.z),
            a.w.wrapping_mul(b.w),
        ))),

        // -- Vector4 × scalar (both orders) -------------------------------
        (Variant::Vector4(v), Variant::Float(s)) => Some(Variant::Vector4(*v * *s as f32)),
        (Variant::Vector4(v), Variant::Int(s)) => Some(Variant::Vector4(*v * *s as f32)),
        (Variant::Float(s), Variant::Vector4(v)) => Some(Variant::Vector4(*v * *s as f32)),
        (Variant::Int(s), Variant::Vector4(v)) => Some(Variant::Vector4(*v * *s as f32)),

        // -- int-vector × int stays INTEGRAL; × float widens to float -----
        //
        // ⚠️ These two result types differ, straight from the upstream table:
        // `VECTOR2I INT -> Vector2i` but `VECTOR2I FLOAT -> Vector2`.
        (Variant::Vector2i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(Variant::Vector2i(Vector2i::new(
                v.x.wrapping_mul(s),
                v.y.wrapping_mul(s),
            )))
        }
        (Variant::Int(s), Variant::Vector2i(v)) => {
            let s = as_i32(*s);
            Some(Variant::Vector2i(Vector2i::new(
                v.x.wrapping_mul(s),
                v.y.wrapping_mul(s),
            )))
        }
        (Variant::Vector2i(v), Variant::Float(s)) | (Variant::Float(s), Variant::Vector2i(v)) => {
            let s = *s as f32;
            Some(Variant::Vector2(Vector2::new(
                v.x as f32 * s,
                v.y as f32 * s,
            )))
        }
        (Variant::Vector3i(v), Variant::Int(s)) | (Variant::Int(s), Variant::Vector3i(v)) => {
            let s = as_i32(*s);
            Some(Variant::Vector3i(Vector3i::new(
                v.x.wrapping_mul(s),
                v.y.wrapping_mul(s),
                v.z.wrapping_mul(s),
            )))
        }
        (Variant::Vector3i(v), Variant::Float(s)) | (Variant::Float(s), Variant::Vector3i(v)) => {
            let s = *s as f32;
            Some(Variant::Vector3(Vector3::new(
                v.x as f32 * s,
                v.y as f32 * s,
                v.z as f32 * s,
            )))
        }
        (Variant::Vector4i(v), Variant::Int(s)) | (Variant::Int(s), Variant::Vector4i(v)) => {
            let s = as_i32(*s);
            Some(Variant::Vector4i(Vector4i::new(
                v.x.wrapping_mul(s),
                v.y.wrapping_mul(s),
                v.z.wrapping_mul(s),
                v.w.wrapping_mul(s),
            )))
        }
        (Variant::Vector4i(v), Variant::Float(s)) | (Variant::Float(s), Variant::Vector4i(v)) => {
            let s = *s as f32;
            Some(Variant::Vector4(Vector4::new(
                v.x as f32 * s,
                v.y as f32 * s,
                v.z as f32 * s,
                v.w as f32 * s,
            )))
        }

        // -- Color ---------------------------------------------------------
        (Variant::Color(a), Variant::Color(b)) => {
            Some(Variant::Color(color_zip(*a, *b, |x, y| x * y)))
        }
        (Variant::Color(c), Variant::Float(s)) | (Variant::Float(s), Variant::Color(c)) => {
            let s = *s as f32;
            Some(Variant::Color(color_map(*c, |x| x * s)))
        }
        (Variant::Color(c), Variant::Int(s)) | (Variant::Int(s), Variant::Color(c)) => {
            let s = *s as f32;
            Some(Variant::Color(color_map(*c, |x| x * s)))
        }

        // -- transforms × scalar -------------------------------------------
        (Variant::Basis(b), Variant::Float(s)) => Some(Variant::Basis(scale_basis(*b, *s as f32))),
        (Variant::Basis(b), Variant::Int(s)) => Some(Variant::Basis(scale_basis(*b, *s as f32))),
        (Variant::Transform2D(t), Variant::Float(s)) => {
            Some(Variant::Transform2D(scale_transform2d(*t, *s as f32)))
        }
        (Variant::Transform2D(t), Variant::Int(s)) => {
            Some(Variant::Transform2D(scale_transform2d(*t, *s as f32)))
        }
        (Variant::Transform3D(t), Variant::Float(s)) => {
            Some(Variant::Transform3D(scale_transform3d(*t, *s as f32)))
        }
        (Variant::Transform3D(t), Variant::Int(s)) => {
            Some(Variant::Transform3D(scale_transform3d(*t, *s as f32)))
        }

        // -- forward transforms (unchanged: pat-0q2kx.50) ------------------
        (Variant::Transform2D(t), Variant::Vector2(v)) => Some(Variant::Vector2(t.xform(*v))),
        (Variant::Transform2D(a), Variant::Transform2D(b)) => Some(Variant::Transform2D(*a * *b)),
        (Variant::Transform2D(t), Variant::Rect2(r)) => Some(Variant::Rect2(xform_rect2(*t, *r))),
        (Variant::Transform3D(t), Variant::Vector3(v)) => Some(Variant::Vector3(t.xform(*v))),
        (Variant::Transform3D(a), Variant::Transform3D(b)) => Some(Variant::Transform3D(*a * *b)),
        (Variant::Transform3D(t), Variant::Aabb(a)) => Some(Variant::Aabb(xform_aabb(*t, *a))),
        (Variant::Basis(b), Variant::Vector3(v)) => Some(Variant::Vector3(b.xform(*v))),
        (Variant::Basis(a), Variant::Basis(b)) => Some(Variant::Basis(*a * *b)),
        (Variant::Quaternion(q), Variant::Vector3(v)) => Some(Variant::Vector3(q.xform(*v))),
        (Variant::Quaternion(a), Variant::Quaternion(b)) => Some(Variant::Quaternion(*a * *b)),
        (Variant::Quaternion(q), Variant::Float(s)) => {
            Some(Variant::Quaternion(scale_quat(*q, *s as f32)))
        }
        (Variant::Quaternion(q), Variant::Int(s)) => {
            Some(Variant::Quaternion(scale_quat(*q, *s as f32)))
        }
        (Variant::Float(s), Variant::Quaternion(q)) => {
            Some(Variant::Quaternion(scale_quat(*q, *s as f32)))
        }
        (Variant::Int(s), Variant::Quaternion(q)) => {
            Some(Variant::Quaternion(scale_quat(*q, *s as f32)))
        }

        // -- INVERSE transforms: the vector is on the LEFT (XFormInv) ------
        //
        // ⚠️ Operand order is the entire difference from the forward rows
        // above. `t * v` transforms into the transform's space; `v * t`
        // transforms out of it.
        (Variant::Vector2(v), Variant::Transform2D(t)) => {
            Some(Variant::Vector2(transform2d_xform_inv(*t, *v)))
        }
        (Variant::Vector3(v), Variant::Basis(b)) => Some(Variant::Vector3(b.xform_inv(*v))),
        (Variant::Vector3(v), Variant::Transform3D(t)) => {
            Some(Variant::Vector3(transform3d_xform_inv(*t, *v)))
        }
        (Variant::Vector3(v), Variant::Quaternion(q)) => {
            Some(Variant::Vector3(q.inverse().xform(*v)))
        }

        _ => None,
    }
}

/// Applies `/` for the math builtins.
///
/// Three outcomes, which the nesting makes explicit: `None` = no such row (the
/// interpreter keeps its "cannot divide" error); `Some(Err)` = a zero divisor on
/// a `DivNZ`/`ModNZ` row, which Godot reports as a script error; `Some(Ok)` = a
/// value. Float rows are plain `Div` and yield inf rather than erroring, exactly
/// as upstream.
pub(crate) fn div(lhs: &Variant, rhs: &Variant) -> Option<Result<Variant, RuntimeError>> {
    match (lhs, rhs) {
        // -- float vectors: componentwise and by scalar, inf on zero -------
        (Variant::Vector2(a), Variant::Vector2(b)) => {
            Some(Ok(Variant::Vector2(Vector2::new(a.x / b.x, a.y / b.y))))
        }
        (Variant::Vector3(a), Variant::Vector3(b)) => Some(Ok(Variant::Vector3(Vector3::new(
            a.x / b.x,
            a.y / b.y,
            a.z / b.z,
        )))),
        (Variant::Vector4(a), Variant::Vector4(b)) => Some(Ok(Variant::Vector4(Vector4::new(
            a.x / b.x,
            a.y / b.y,
            a.z / b.z,
            a.w / b.w,
        )))),
        (Variant::Vector4(v), Variant::Float(s)) => Some(Ok(Variant::Vector4(*v / *s as f32))),
        (Variant::Vector4(v), Variant::Int(s)) => Some(Ok(Variant::Vector4(*v / *s as f32))),

        // -- int vectors: DivNZ, so a zero divisor is an error -------------
        (Variant::Vector2i(a), Variant::Vector2i(b)) => {
            Some(int_vec2(a.x.checked_div(b.x), a.y.checked_div(b.y)))
        }
        (Variant::Vector3i(a), Variant::Vector3i(b)) => Some(int_vec3(
            a.x.checked_div(b.x),
            a.y.checked_div(b.y),
            a.z.checked_div(b.z),
        )),
        (Variant::Vector4i(a), Variant::Vector4i(b)) => Some(int_vec4(
            a.x.checked_div(b.x),
            a.y.checked_div(b.y),
            a.z.checked_div(b.z),
            a.w.checked_div(b.w),
        )),
        (Variant::Vector2i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(int_vec2(v.x.checked_div(s), v.y.checked_div(s)))
        }
        (Variant::Vector3i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(int_vec3(
                v.x.checked_div(s),
                v.y.checked_div(s),
                v.z.checked_div(s),
            ))
        }
        (Variant::Vector4i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(int_vec4(
                v.x.checked_div(s),
                v.y.checked_div(s),
                v.z.checked_div(s),
                v.w.checked_div(s),
            ))
        }
        // ⚠️ An int vector divided by a FLOAT widens to the float vector, and
        // is still a `DivNZ` row upstream — so zero errors here too, unlike
        // `Vector2 / 0.0`.
        (Variant::Vector2i(v), Variant::Float(s)) => {
            if *s == 0.0 {
                return Some(Err(div_by_zero()));
            }
            let s = *s as f32;
            Some(Ok(Variant::Vector2(Vector2::new(
                v.x as f32 / s,
                v.y as f32 / s,
            ))))
        }
        (Variant::Vector3i(v), Variant::Float(s)) => {
            if *s == 0.0 {
                return Some(Err(div_by_zero()));
            }
            let s = *s as f32;
            Some(Ok(Variant::Vector3(Vector3::new(
                v.x as f32 / s,
                v.y as f32 / s,
                v.z as f32 / s,
            ))))
        }
        (Variant::Vector4i(v), Variant::Float(s)) => {
            if *s == 0.0 {
                return Some(Err(div_by_zero()));
            }
            let s = *s as f32;
            Some(Ok(Variant::Vector4(Vector4::new(
                v.x as f32 / s,
                v.y as f32 / s,
                v.z as f32 / s,
                v.w as f32 / s,
            ))))
        }

        // -- Color ---------------------------------------------------------
        (Variant::Color(a), Variant::Color(b)) => {
            Some(Ok(Variant::Color(color_zip(*a, *b, |x, y| x / y))))
        }
        (Variant::Color(c), Variant::Float(s)) => {
            let s = *s as f32;
            Some(Ok(Variant::Color(color_map(*c, |x| x / s))))
        }
        (Variant::Color(c), Variant::Int(s)) => {
            let s = *s as f32;
            Some(Ok(Variant::Color(color_map(*c, |x| x / s))))
        }

        // -- transforms / scalar -------------------------------------------
        (Variant::Basis(b), Variant::Float(s)) => {
            Some(Ok(Variant::Basis(scale_basis(*b, 1.0 / *s as f32))))
        }
        (Variant::Basis(b), Variant::Int(s)) => {
            Some(Ok(Variant::Basis(scale_basis(*b, 1.0 / *s as f32))))
        }
        (Variant::Transform2D(t), Variant::Float(s)) => Some(Ok(Variant::Transform2D(
            scale_transform2d(*t, 1.0 / *s as f32),
        ))),
        (Variant::Transform2D(t), Variant::Int(s)) => Some(Ok(Variant::Transform2D(
            scale_transform2d(*t, 1.0 / *s as f32),
        ))),
        (Variant::Transform3D(t), Variant::Float(s)) => Some(Ok(Variant::Transform3D(
            scale_transform3d(*t, 1.0 / *s as f32),
        ))),
        (Variant::Transform3D(t), Variant::Int(s)) => Some(Ok(Variant::Transform3D(
            scale_transform3d(*t, 1.0 / *s as f32),
        ))),
        (Variant::Quaternion(q), Variant::Float(s)) => {
            Some(Ok(Variant::Quaternion(scale_quat(*q, 1.0 / *s as f32))))
        }
        (Variant::Quaternion(q), Variant::Int(s)) => {
            Some(Ok(Variant::Quaternion(scale_quat(*q, 1.0 / *s as f32))))
        }
        _ => None,
    }
}

/// Applies `%` for the int vectors (`ModNZ`); no float type has a `%` row.
pub(crate) fn rem(lhs: &Variant, rhs: &Variant) -> Option<Result<Variant, RuntimeError>> {
    match (lhs, rhs) {
        (Variant::Vector2i(a), Variant::Vector2i(b)) => {
            Some(int_vec2(a.x.checked_rem(b.x), a.y.checked_rem(b.y)))
        }
        (Variant::Vector3i(a), Variant::Vector3i(b)) => Some(int_vec3(
            a.x.checked_rem(b.x),
            a.y.checked_rem(b.y),
            a.z.checked_rem(b.z),
        )),
        (Variant::Vector4i(a), Variant::Vector4i(b)) => Some(int_vec4(
            a.x.checked_rem(b.x),
            a.y.checked_rem(b.y),
            a.z.checked_rem(b.z),
            a.w.checked_rem(b.w),
        )),
        (Variant::Vector2i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(int_vec2(v.x.checked_rem(s), v.y.checked_rem(s)))
        }
        (Variant::Vector3i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(int_vec3(
                v.x.checked_rem(s),
                v.y.checked_rem(s),
                v.z.checked_rem(s),
            ))
        }
        (Variant::Vector4i(v), Variant::Int(s)) => {
            let s = as_i32(*s);
            Some(int_vec4(
                v.x.checked_rem(s),
                v.y.checked_rem(s),
                v.z.checked_rem(s),
                v.w.checked_rem(s),
            ))
        }
        _ => None,
    }
}

/// Applies unary `-` for the math builtins.
pub(crate) fn neg(value: &Variant) -> Option<Variant> {
    match value {
        Variant::Vector4(v) => Some(Variant::Vector4(-*v)),
        Variant::Vector4i(v) => Some(Variant::Vector4i(-*v)),
        Variant::Color(c) => Some(Variant::Color(color_map(*c, |x| -x))),
        Variant::Quaternion(q) => {
            Some(Variant::Quaternion(Quaternion::new(-q.x, -q.y, -q.z, -q.w)))
        }
        // Godot negates both the normal AND the distance (`plane.h operator-`),
        // which is the same plane facing the other way.
        Variant::Plane(p) => Some(Variant::Plane(Plane {
            normal: -p.normal,
            d: -p.d,
        })),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Ordering and membership (pat-mgixk.11 part 2)
// ---------------------------------------------------------------------------

/// How deep `cmp` will recurse into nested arrays.
///
/// ⚠️ A GDScript array can contain itself (`var a = []; a.append(a)`), and
/// Godot guards its recursive array walks with exactly this kind of cap
/// (`array.cpp` `MAX_RECURSION`). Without one, comparing two such arrays
/// recurses until the stack dies.
const MAX_CMP_DEPTH: u32 = 64;

/// Total order for the `<`, `<=`, `>`, `>=` rows Godot registers beyond the
/// numeric and string ones (`variant_op.cpp` OP_LESS: Vector2/2i/3/3i/4/4i,
/// ARRAY, RID, BOOL).
///
/// `None` means the pair has no ordering row, so the interpreter keeps its own
/// "cannot compare" error.
///
/// Returning an `Ordering` rather than four separate predicates is sound here
/// and worth checking, because Godot spells each operator out by hand: its
/// `Vector2::operator<=` is `x == p.x ? (y <= p.y) : (x < p.x)` while
/// `operator<` is `x == p.x ? (y < p.y) : (x < p.x)` (vector2.h:164-167). Those
/// differ only in the tie case, which is precisely `a < b || a == b` — i.e.
/// both are the lexicographic order, so one `Ordering` reproduces all four.
/// The same holds for `Array`, whose `<=` is defined as `!(>)`.
pub(crate) fn cmp(lhs: &Variant, rhs: &Variant) -> Option<std::cmp::Ordering> {
    cmp_depth(lhs, rhs, 0)
}

fn cmp_depth(lhs: &Variant, rhs: &Variant, depth: u32) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (lhs, rhs) {
        // ⚠️ The scalar and string rows are repeated here even though
        // `binary_cmp` already handles them at the top level, because the
        // `Array` arm below RECURSES through this function. Without them
        // `[1, 2] < [1, 3]` compared its elements, found no ordering row for
        // `(Int, Int)`, and failed the whole comparison — the Array arm was
        // wired but could not compare anything a real array contains.
        (Variant::Int(a), Variant::Int(b)) => Some(a.cmp(b)),
        (Variant::Float(a), Variant::Float(b)) => Some(num_cmp(*a, *b)),
        (Variant::Int(a), Variant::Float(b)) => Some(num_cmp(*a as f64, *b)),
        (Variant::Float(a), Variant::Int(b)) => Some(num_cmp(*a, *b as f64)),
        (Variant::String(a), Variant::String(b)) => Some(a.cmp(b)),
        (Variant::StringName(a), Variant::StringName(b)) => Some(a.cmp(b)),
        // Lexicographic, component by component (vector2.h:164, vector3.h:484).
        (Variant::Vector2(a), Variant::Vector2(b)) => lex(&[(a.x, b.x), (a.y, b.y)]),
        (Variant::Vector3(a), Variant::Vector3(b)) => lex(&[(a.x, b.x), (a.y, b.y), (a.z, b.z)]),
        (Variant::Vector4(a), Variant::Vector4(b)) => {
            lex(&[(a.x, b.x), (a.y, b.y), (a.z, b.z), (a.w, b.w)])
        }
        (Variant::Vector2i(a), Variant::Vector2i(b)) => Some((a.x, a.y).cmp(&(b.x, b.y))),
        (Variant::Vector3i(a), Variant::Vector3i(b)) => Some((a.x, a.y, a.z).cmp(&(b.x, b.y, b.z))),
        (Variant::Vector4i(a), Variant::Vector4i(b)) => {
            Some((a.x, a.y, a.z, a.w).cmp(&(b.x, b.y, b.z, b.w)))
        }
        // `false < true` (`OperatorEvaluatorLess<bool, bool>`).
        (Variant::Bool(a), Variant::Bool(b)) => Some(a.cmp(b)),
        // RIDs order by their handle (`rid.h operator<`).
        (Variant::Rid(a), Variant::Rid(b)) => Some(a.id().cmp(&b.id())),
        // Element-wise, with the SHORTER array sorting first on a shared
        // prefix (`array.cpp:164-179` — the `return a_len < b_len` tail). A
        // size-first comparison would be a different order and would make
        // `[1, 2] < [1, 3]` depend on length instead of content.
        (Variant::Array(a), Variant::Array(b)) => {
            if depth >= MAX_CMP_DEPTH {
                return None;
            }
            let (a, b) = (a.to_vec(), b.to_vec());
            for (x, y) in a.iter().zip(b.iter()) {
                match cmp_depth(x, y, depth + 1) {
                    Some(Ordering::Equal) => continue,
                    // An element pair with no ordering row makes the whole
                    // comparison an error, rather than silently treating the
                    // elements as equal and moving on.
                    None => return None,
                    other => return other,
                }
            }
            Some(a.len().cmp(&b.len()))
        }
        _ => None,
    }
}

/// Orders two floats, treating a NaN as equal to its counterpart.
///
/// Matches what the interpreter's own numeric arms do, and what Godot's
/// `==`-then-`<` chain produces for a NaN (both tests are false).
fn num_cmp(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// Lexicographic order over float component pairs.
///
/// A NaN component has no ordering; it is treated as equal to its counterpart
/// so the walk moves to the next component, which is the same thing Godot's
/// `==`-then-`<` chain does with a NaN (both tests are false).
fn lex(pairs: &[(f32, f32)]) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    for (a, b) in pairs {
        match a.partial_cmp(b) {
            Some(Ordering::Equal) | None => continue,
            other => return other,
        }
    }
    Some(Ordering::Equal)
}

/// `value in <Packed*Array>` (`variant_op.cpp` OP_IN rows for the ten packed
/// types).
///
/// `None` means `container` is not a packed array, so the interpreter falls
/// through to its other `in` cases. A packed array of the wrong element type
/// for `needle` answers `Some(false)` rather than `None`: Godot registers
/// `INT in PACKED_FLOAT32_ARRAY` and friends, so a numeric needle is compared
/// numerically across the integer and float arrays.
pub(crate) fn packed_contains(container: &Variant, needle: &Variant) -> Option<bool> {
    let number = match needle {
        Variant::Int(i) => Some(*i as f64),
        Variant::Float(f) => Some(*f),
        _ => None,
    };
    match container {
        Variant::PackedByteArray(items) => {
            Some(number.is_some_and(|n| items.iter().any(|v| f64::from(*v) == n)))
        }
        Variant::PackedInt32Array(items) => {
            Some(number.is_some_and(|n| items.iter().any(|v| f64::from(*v) == n)))
        }
        Variant::PackedInt64Array(items) => {
            Some(number.is_some_and(|n| items.iter().any(|v| *v as f64 == n)))
        }
        Variant::PackedFloat32Array(items) => {
            Some(number.is_some_and(|n| items.iter().any(|v| f64::from(*v) == n)))
        }
        Variant::PackedFloat64Array(items) => {
            Some(number.is_some_and(|n| items.iter().any(|v| *v == n)))
        }
        // String and StringName are both registered against
        // PackedStringArray, and compare by text.
        Variant::PackedStringArray(items) => {
            let text = match needle {
                Variant::String(s) => Some(s.clone()),
                Variant::StringName(s) => Some(s.to_string()),
                _ => None,
            };
            Some(text.is_some_and(|t| items.iter().any(|v| *v == t)))
        }
        Variant::PackedVector2Array(items) => Some(matches!(needle, Variant::Vector2(n)
            if items.iter().any(|v| v.x == n.x && v.y == n.y))),
        Variant::PackedVector3Array(items) => Some(matches!(needle, Variant::Vector3(n)
            if items.iter().any(|v| v.x == n.x && v.y == n.y && v.z == n.z))),
        Variant::PackedVector4Array(items) => Some(matches!(needle, Variant::Vector4(n)
            if items.iter().any(|v| v.x == n.x && v.y == n.y && v.z == n.z && v.w == n.w))),
        Variant::PackedColorArray(items) => Some(matches!(needle, Variant::Color(n)
            if items.iter().any(|v| v.r == n.r && v.g == n.g && v.b == n.b && v.a == n.a))),
        _ => None,
    }
}

/// `DivNZ`/`ModNZ` reports a zero divisor as a script error rather than
/// returning a sentinel, so `checked_div`/`checked_rem` returning `None` for any
/// component fails the whole operation.
fn div_by_zero() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::DivisionByZero)
}

fn int_vec2(x: Option<i32>, y: Option<i32>) -> Result<Variant, RuntimeError> {
    match (x, y) {
        (Some(x), Some(y)) => Ok(Variant::Vector2i(Vector2i::new(x, y))),
        _ => Err(div_by_zero()),
    }
}

fn int_vec3(x: Option<i32>, y: Option<i32>, z: Option<i32>) -> Result<Variant, RuntimeError> {
    match (x, y, z) {
        (Some(x), Some(y), Some(z)) => Ok(Variant::Vector3i(Vector3i::new(x, y, z))),
        _ => Err(div_by_zero()),
    }
}

fn int_vec4(
    x: Option<i32>,
    y: Option<i32>,
    z: Option<i32>,
    w: Option<i32>,
) -> Result<Variant, RuntimeError> {
    match (x, y, z, w) {
        (Some(x), Some(y), Some(z), Some(w)) => Ok(Variant::Vector4i(Vector4i::new(x, y, z, w))),
        _ => Err(div_by_zero()),
    }
}

fn scale_quat(q: Quaternion, s: f32) -> Quaternion {
    Quaternion::new(q.x * s, q.y * s, q.z * s, q.w * s)
}

/// Transforms a rect by taking the bounding rect of its four transformed
/// corners, matching Godot's `Transform2D::xform(Rect2)`.
fn xform_rect2(t: Transform2D, r: Rect2) -> Rect2 {
    let corners = [
        t.xform(r.position),
        t.xform(Vector2::new(r.position.x + r.size.x, r.position.y)),
        t.xform(Vector2::new(r.position.x, r.position.y + r.size.y)),
        t.xform(r.end()),
    ];
    let mut min = corners[0];
    let mut max = corners[0];
    for c in corners.iter().skip(1) {
        min = Vector2::new(min.x.min(c.x), min.y.min(c.y));
        max = Vector2::new(max.x.max(c.x), max.y.max(c.y));
    }
    Rect2::new(min, max - min)
}

/// Transforms an AABB by taking the bounding box of its eight transformed
/// corners, matching Godot's `Transform3D::xform(AABB)`.
fn xform_aabb(t: Transform3D, a: Aabb) -> Aabb {
    let mut min = t.xform(a.get_endpoint(0));
    let mut max = min;
    for i in 1..8u8 {
        let c = t.xform(a.get_endpoint(i));
        min = Vector3::new(min.x.min(c.x), min.y.min(c.y), min.z.min(c.z));
        max = Vector3::new(max.x.max(c.x), max.y.max(c.y), max.z.max(c.z));
    }
    Aabb::new(min, max - min)
}

// ---------------------------------------------------------------------------
// Vector2 / Vector3 / Vector2i / Vector3i method surface (pat-s3rrv.9)
// ---------------------------------------------------------------------------
//
// The interpreter's own tables implemented 8 Vector2 methods and 7 Vector3
// methods, and nothing at all for the int vectors — `Variant::Vector2i` had no
// arm in `call_method_on`. So `velocity.move_toward(...)`, `dir.rotated(...)`
// and `pos.snapped(...)` — what a real game calls every frame — raised
// `UndefinedFunction` and aborted the running script.
//
// Semantics come from `upstream/godot/core/math/vector2.h` / `vector3.h`, not
// from intuition, because several of these are easy to get subtly wrong:
// `reflect` is Godot's `2 * n * dot(n) - v` (a mirror about `n`, which is why
// `bounce` is exactly its negation), `angle_to_point` is `(to - self).angle()`
// as of Godot 4 (it was the opposite direction in 3.x), `snapped` rounds half
// away from zero via `floor(v / step + 0.5)`, and `sign` maps zero to zero
// where Rust's `f32::signum` answers ±1 for ±0.0.

/// Godot's `CMP_EPSILON` (`core/math/math_defs.h`).
const CMP_EPSILON: f32 = 0.00001;

/// Godot's `SIGN`: zero maps to zero, unlike `f32::signum`.
fn sign_f(v: f32) -> f32 {
    if v < 0.0 {
        -1.0
    } else if v > 0.0 {
        1.0
    } else {
        0.0
    }
}

fn sign_i(v: i32) -> i32 {
    match v.cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => 0,
    }
}

/// Godot's `Math::snapped`: `floor(value / step + 0.5) * step`, a no-op when
/// `step` is zero.
fn snap_f(value: f32, step: f32) -> f32 {
    if step != 0.0 {
        (value / step + 0.5).floor() * step
    } else {
        value
    }
}

fn snap_i(value: i32, step: i32) -> i32 {
    if step != 0 {
        let v = value as f64 / step as f64;
        ((v + 0.5).floor() as i32).saturating_mul(step)
    } else {
        value
    }
}

/// Godot's `Math::fposmod` — the result carries the sign of `y`, which
/// `f32::rem_euclid` does not do for a negative modulus.
fn fposmod(x: f32, y: f32) -> f32 {
    let mut value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value += y;
    }
    value
}

/// Godot's `Math::is_equal_approx` — a relative tolerance with an absolute
/// floor, so values near zero still compare sanely.
fn is_equal_approx_f(a: f32, b: f32) -> bool {
    if a == b {
        return true;
    }
    let tolerance = (CMP_EPSILON * a.abs()).max(CMP_EPSILON);
    (a - b).abs() < tolerance
}

fn arg_vec2i(args: &[Variant], idx: usize, ctx: &str) -> Result<Vector2i, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Vector2i(v)) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector2i for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_vec3i(args: &[Variant], idx: usize, ctx: &str) -> Result<Vector3i, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Vector3i(v)) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector3i for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

/// An optional trailing bool argument; absent means `false`.
///
/// Godot's `flip_fov` flags default to `false`, and a caller that omits them is
/// asking for the default rather than making an error.
fn arg_flag(args: &[Variant], idx: usize) -> bool {
    matches!(args.get(idx), Some(Variant::Bool(true)))
}

fn arg_rect2i(args: &[Variant], idx: usize, ctx: &str) -> Result<Rect2i, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Rect2i(r)) => Ok(*r),
        other => Err(type_error(format!(
            "{ctx} expects a Rect2i for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_vec4(args: &[Variant], idx: usize, ctx: &str) -> Result<Vector4, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Vector4(v)) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector4 for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_vec4i(args: &[Variant], idx: usize, ctx: &str) -> Result<Vector4i, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Vector4i(v)) => Ok(*v),
        other => Err(type_error(format!(
            "{ctx} expects a Vector4i for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

fn arg_int(args: &[Variant], idx: usize, ctx: &str) -> Result<i32, RuntimeError> {
    match args.get(idx) {
        Some(Variant::Int(i)) => Ok(*i as i32),
        Some(Variant::Float(f)) => Ok(*f as i32),
        Some(Variant::Bool(b)) => Ok(*b as i32),
        other => Err(type_error(format!(
            "{ctx} expects an int for argument {idx}, got {}",
            describe(other)
        ))),
    }
}

/// Dispatches a method call on any of the four vector Variants.
///
/// Returns `UndefinedFunction("<Type>.<method>")` for a name this table does not
/// implement, so the caller can surface exactly the error Godot would.
pub(crate) fn vector_method(
    value: &Variant,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    match value {
        Variant::Vector2(v) => vector2_method(*v, method, args),
        Variant::Vector3(v) => vector3_method(*v, method, args),
        Variant::Vector2i(v) => vector2i_method(*v, method, args),
        Variant::Vector3i(v) => vector3i_method(*v, method, args),
        other => Err(type_error(format!(
            "`{method}` is not a vector method (called on {})",
            describe(Some(other))
        ))),
    }
}

fn vector2_method(v: Vector2, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Vector2.{method}()");
    let vec = |x: f32, y: f32| -> Result<Variant, RuntimeError> {
        Ok(Variant::Vector2(Vector2::new(x, y)))
    };
    match method {
        // -- component-wise unary ------------------------------------------
        "abs" => vec(v.x.abs(), v.y.abs()),
        "sign" => vec(sign_f(v.x), sign_f(v.y)),
        "floor" => vec(v.x.floor(), v.y.floor()),
        "ceil" => vec(v.x.ceil(), v.y.ceil()),
        "round" => vec(v.x.round(), v.y.round()),
        "orthogonal" => vec(v.y, -v.x),

        // -- scalar queries -------------------------------------------------
        "aspect" => Ok(Variant::Float((v.x / v.y) as f64)),
        "distance_squared_to" => {
            let o = arg_vec2(args, 0, &ctx)?;
            let d = v - o;
            Ok(Variant::Float(d.length_squared() as f64))
        }
        "angle_to" => {
            let o = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Float(v.cross(o).atan2(v.dot(o)) as f64))
        }
        // Godot 4: the angle of the vector POINTING AT `to`, i.e. `(to - self)`.
        "angle_to_point" => {
            let o = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Float((o - v).angle() as f64))
        }
        "max_axis_index" => Ok(Variant::Int(if v.x < v.y { 1 } else { 0 })),
        "min_axis_index" => Ok(Variant::Int(if v.x < v.y { 0 } else { 1 })),
        "is_normalized" => Ok(Variant::Bool(is_equal_approx_f(v.length_squared(), 1.0))),
        "is_zero_approx" => Ok(Variant::Bool(
            v.x.abs() < CMP_EPSILON && v.y.abs() < CMP_EPSILON,
        )),
        "is_finite" => Ok(Variant::Bool(v.x.is_finite() && v.y.is_finite())),
        "is_equal_approx" => {
            let o = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Bool(
                is_equal_approx_f(v.x, o.x) && is_equal_approx_f(v.y, o.y),
            ))
        }

        // -- vector results -------------------------------------------------
        "direction_to" => {
            let o = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Vector2((o - v).normalized()))
        }
        "rotated" => {
            let a = arg_num(args, 0, &ctx)?;
            let (s, c) = (a.sin(), a.cos());
            vec(v.x * c - v.y * s, v.x * s + v.y * c)
        }
        "project" => {
            let o = arg_vec2(args, 0, &ctx)?;
            let denom = o.length_squared();
            if denom == 0.0 {
                return Ok(Variant::Vector2(Vector2::ZERO));
            }
            Ok(Variant::Vector2(o * (v.dot(o) / denom)))
        }
        // A mirror about `n`; `bounce` is its negation (Godot defines it that way).
        "reflect" => {
            let n = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Vector2(n * (2.0 * v.dot(n)) - v))
        }
        "bounce" => {
            let n = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Vector2(-(n * (2.0 * v.dot(n)) - v)))
        }
        "slide" => {
            let n = arg_vec2(args, 0, &ctx)?;
            Ok(Variant::Vector2(v - n * v.dot(n)))
        }
        "limit_length" => {
            let len = if args.is_empty() {
                1.0
            } else {
                arg_num(args, 0, &ctx)?
            };
            let l = v.length();
            if l > 0.0 && len < l {
                Ok(Variant::Vector2(v / l * len))
            } else {
                Ok(Variant::Vector2(v))
            }
        }
        "move_toward" => {
            let to = arg_vec2(args, 0, &ctx)?;
            let delta = arg_num(args, 1, &ctx)?;
            let vd = to - v;
            let len = vd.length();
            if len <= delta || len < CMP_EPSILON {
                Ok(Variant::Vector2(to))
            } else {
                Ok(Variant::Vector2(v + vd / len * delta))
            }
        }
        "snapped" => {
            let step = arg_vec2(args, 0, &ctx)?;
            vec(snap_f(v.x, step.x), snap_f(v.y, step.y))
        }
        "snappedf" => {
            let step = arg_num(args, 0, &ctx)?;
            vec(snap_f(v.x, step), snap_f(v.y, step))
        }
        "clamp" => {
            let lo = arg_vec2(args, 0, &ctx)?;
            let hi = arg_vec2(args, 1, &ctx)?;
            vec(v.x.clamp(lo.x, hi.x), v.y.clamp(lo.y, hi.y))
        }
        "clampf" => {
            let lo = arg_num(args, 0, &ctx)?;
            let hi = arg_num(args, 1, &ctx)?;
            vec(v.x.clamp(lo, hi), v.y.clamp(lo, hi))
        }
        "min" => {
            let o = arg_vec2(args, 0, &ctx)?;
            vec(v.x.min(o.x), v.y.min(o.y))
        }
        "max" => {
            let o = arg_vec2(args, 0, &ctx)?;
            vec(v.x.max(o.x), v.y.max(o.y))
        }
        "minf" => {
            let s = arg_num(args, 0, &ctx)?;
            vec(v.x.min(s), v.y.min(s))
        }
        "maxf" => {
            let s = arg_num(args, 0, &ctx)?;
            vec(v.x.max(s), v.y.max(s))
        }
        "posmod" => {
            let m = arg_num(args, 0, &ctx)?;
            vec(fposmod(v.x, m), fposmod(v.y, m))
        }
        "posmodv" => {
            let m = arg_vec2(args, 0, &ctx)?;
            vec(fposmod(v.x, m.x), fposmod(v.y, m.y))
        }
        "slerp" => {
            let to = arg_vec2(args, 0, &ctx)?;
            let w = arg_num(args, 1, &ctx)?;
            let (start_sq, end_sq) = (v.length_squared(), to.length_squared());
            if start_sq == 0.0 || end_sq == 0.0 {
                // A zero-length end point has no direction to rotate towards,
                // so Godot falls back to a plain lerp rather than dividing by
                // zero.
                return Ok(Variant::Vector2(v.lerp(to, w)));
            }
            let start_len = start_sq.sqrt();
            let result_len = start_len + (end_sq.sqrt() - start_len) * w;
            let angle = v.cross(to).atan2(v.dot(to)) * w;
            let (s, c) = (angle.sin(), angle.cos());
            let rotated = Vector2::new(v.x * c - v.y * s, v.x * s + v.y * c);
            Ok(Variant::Vector2(rotated * (result_len / start_len)))
        }
        _ => Err(undefined_method("Vector2", method)),
    }
}

/// `Rect2i`'s instance methods (pat-s3rrv.59).
///
/// Same dispatch gap the Vector4 pair had: the kind round-trips through a
/// `.tscn` and its `position`/`size`/`end` read, but nothing could be called on
/// it. Bodies delegate to `gdcore::math::Rect2i`.
///
/// ⚠️ Godot spells the area accessor `get_area`; gdcore's method is `area`.
/// The GDScript-facing name has to be Godot's.
fn rect2i_method(r: Rect2i, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Rect2i.{method}()");
    match method {
        "get_area" => Ok(Variant::Int(r.area())),
        "has_area" => Ok(Variant::Bool(r.has_area())),
        "get_center" => Ok(Variant::Vector2i(r.get_center())),
        "has_point" => Ok(Variant::Bool(r.has_point(arg_vec2i(args, 0, &ctx)?))),
        "intersects" => Ok(Variant::Bool(r.intersects(arg_rect2i(args, 0, &ctx)?))),
        "encloses" => Ok(Variant::Bool(r.encloses(arg_rect2i(args, 0, &ctx)?))),
        "intersection" => Ok(Variant::Rect2i(r.intersection(arg_rect2i(args, 0, &ctx)?))),
        "merge" => Ok(Variant::Rect2i(r.merge(arg_rect2i(args, 0, &ctx)?))),
        "expand" => Ok(Variant::Rect2i(r.expand(arg_vec2i(args, 0, &ctx)?))),
        "abs" => Ok(Variant::Rect2i(r.abs())),
        "grow" => Ok(Variant::Rect2i(r.grow(arg_int(args, 0, &ctx)?))),
        // pat-s3rrv.59: the float `Rect2` has had both of these since it was
        // written; `Rect2i` had only `grow`, so a script porting integer rect
        // maths hit UndefinedFunction on the two that take per-side amounts.
        "grow_individual" => Ok(Variant::Rect2i(r.grow_individual(
            arg_int(args, 0, &ctx)?,
            arg_int(args, 1, &ctx)?,
            arg_int(args, 2, &ctx)?,
            arg_int(args, 3, &ctx)?,
        ))),
        // ⚠️ The side is a `@GlobalScope` Side ordinal (LEFT 0, TOP 1, RIGHT 2,
        // BOTTOM 3), not a rect coordinate — read as an int and widened, the
        // same shape `Rect2.grow_side` takes.
        "grow_side" => Ok(Variant::Rect2i(
            r.grow_side(i64::from(arg_int(args, 0, &ctx)?), arg_int(args, 1, &ctx)?),
        )),
        _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Rect2i.{method}"),
        ))),
    }
}

/// `Projection`'s instance methods (pat-s3rrv.59).
///
/// ⚠️ Deliberately SHORT. Most of Godot's Projection surface is `create_*`
/// STATIC constructors, which belong in `vector_static_call` rather than here,
/// and the derived getters (`get_fov`, `get_z_near`, `get_aspect`, …) each read
/// a specific combination of matrix entries that has to be taken from
/// `core/math/projection.cpp` rather than guessed. Only the three gdcore
/// already implements and tests are exposed.
fn projection_method(
    p: gdcore::math3d::Projection,
    method: &str,
    _args: &[Variant],
) -> Result<Variant, RuntimeError> {
    match method {
        "determinant" => Ok(Variant::Float(p.determinant() as f64)),
        "inverse" => Ok(Variant::Projection(p.inverse())),
        "is_orthogonal" => Ok(Variant::Bool(p.is_orthogonal())),
        _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Projection.{method}"),
        ))),
    }
}

/// `Vector4`'s instance methods (pat-s3rrv.59).
///
/// The kind could be constructed and its members read, but `call_method` had no
/// arm for it, so every `v.length()` hit the "cannot call method on Vector4"
/// catch-all — the same dispatch-shaped gap `Color` had.
///
/// Bodies delegate to `gdcore::math::Vector4`, which is already tested, rather
/// than recomputing here.
fn vector4_method(v: Vector4, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Vector4.{method}()");
    let vec = |x: f32, y: f32, z: f32, w: f32| -> Result<Variant, RuntimeError> {
        Ok(Variant::Vector4(Vector4::new(x, y, z, w)))
    };
    match method {
        "length" => Ok(Variant::Float(v.length() as f64)),
        "length_squared" => Ok(Variant::Float(v.length_squared() as f64)),
        "normalized" => Ok(Variant::Vector4(v.normalized())),
        "is_normalized" => Ok(Variant::Bool(v.is_normalized())),
        "abs" => Ok(Variant::Vector4(v.abs())),
        "floor" => Ok(Variant::Vector4(v.floor())),
        "ceil" => Ok(Variant::Vector4(v.ceil())),
        "round" => Ok(Variant::Vector4(v.round())),
        "sign" => vec(sign_f(v.x), sign_f(v.y), sign_f(v.z), sign_f(v.w)),
        "dot" => Ok(Variant::Float(v.dot(arg_vec4(args, 0, &ctx)?) as f64)),
        "distance_to" => Ok(Variant::Float(
            v.distance_to(arg_vec4(args, 0, &ctx)?) as f64
        )),
        "distance_squared_to" => Ok(Variant::Float(
            v.distance_squared_to(arg_vec4(args, 0, &ctx)?) as f64,
        )),
        "direction_to" => {
            let o = arg_vec4(args, 0, &ctx)?;
            Ok(Variant::Vector4(
                Vector4::new(o.x - v.x, o.y - v.y, o.z - v.z, o.w - v.w).normalized(),
            ))
        }
        "lerp" => {
            let to = arg_vec4(args, 0, &ctx)?;
            let weight = arg_num(args, 1, &ctx)?;
            Ok(Variant::Vector4(v.lerp(to, weight)))
        }
        // pat-s3rrv.59. ⚠️ Godot's order is (b, pre_a, post_b, weight) — `b` is
        // the DESTINATION and `pre_a`/`post_b` the surrounding control points.
        // Reading them in the scalar's (from, to, pre, post) order instead would
        // still yield a smooth curve through plausible values.
        "cubic_interpolate" => {
            let b = arg_vec4(args, 0, &ctx)?;
            let pre_a = arg_vec4(args, 1, &ctx)?;
            let post_b = arg_vec4(args, 2, &ctx)?;
            let weight = arg_num(args, 3, &ctx)?;
            Ok(Variant::Vector4(
                v.cubic_interpolate(b, pre_a, post_b, weight),
            ))
        }
        "cubic_interpolate_in_time" => {
            let b = arg_vec4(args, 0, &ctx)?;
            let pre_a = arg_vec4(args, 1, &ctx)?;
            let post_b = arg_vec4(args, 2, &ctx)?;
            let weight = arg_num(args, 3, &ctx)?;
            let b_t = arg_num(args, 4, &ctx)?;
            let pre_a_t = arg_num(args, 5, &ctx)?;
            let post_b_t = arg_num(args, 6, &ctx)?;
            Ok(Variant::Vector4(v.cubic_interpolate_in_time(
                b, pre_a, post_b, weight, b_t, pre_a_t, post_b_t,
            )))
        }
        "min" => Ok(Variant::Vector4(v.min(arg_vec4(args, 0, &ctx)?))),
        "max" => Ok(Variant::Vector4(v.max(arg_vec4(args, 0, &ctx)?))),
        "minf" => Ok(Variant::Vector4(v.minf(arg_num(args, 0, &ctx)?))),
        "maxf" => Ok(Variant::Vector4(v.maxf(arg_num(args, 0, &ctx)?))),
        // ⚠️ `posmod` is Godot's `Math::fposmod`, whose result takes the sign of
        // the DIVISOR — `Vector4(5,5,5,5).posmod(-3)` is `-1` per component, not
        // `2`. Rust's `rem_euclid` is the always-positive spelling and disagrees
        // on exactly the negative-divisor case a wrapping calculation uses.
        "posmod" => Ok(Variant::Vector4(v.posmod(arg_num(args, 0, &ctx)?))),
        "posmodv" => Ok(Variant::Vector4(v.posmodv(arg_vec4(args, 0, &ctx)?))),
        // ⚠️ `snapped` rounds half UP (`floor(v/step + 0.5)`), not half away
        // from zero: snapping `-2.5` to a step of 1 gives `-2`, where `round()`
        // gives `-3`.
        "snapped" => Ok(Variant::Vector4(v.snapped(arg_vec4(args, 0, &ctx)?))),
        "snappedf" => Ok(Variant::Vector4(v.snappedf(arg_num(args, 0, &ctx)?))),
        // ⚠️ The component-wise RECIPROCAL, not a negation or a normalise.
        "inverse" => Ok(Variant::Vector4(v.inverse())),
        "clamp" => {
            let lo = arg_vec4(args, 0, &ctx)?;
            let hi = arg_vec4(args, 1, &ctx)?;
            Ok(Variant::Vector4(v.clamp(lo, hi)))
        }
        "clampf" => {
            let lo = arg_num(args, 0, &ctx)?;
            let hi = arg_num(args, 1, &ctx)?;
            Ok(Variant::Vector4(v.clampf(lo, hi)))
        }
        "is_finite" => Ok(Variant::Bool(
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && v.w.is_finite(),
        )),
        "is_zero_approx" => Ok(Variant::Bool(
            v.x.abs() < CMP_EPSILON
                && v.y.abs() < CMP_EPSILON
                && v.z.abs() < CMP_EPSILON
                && v.w.abs() < CMP_EPSILON,
        )),
        "is_equal_approx" => {
            let o = arg_vec4(args, 0, &ctx)?;
            Ok(Variant::Bool(
                is_equal_approx_f(v.x, o.x)
                    && is_equal_approx_f(v.y, o.y)
                    && is_equal_approx_f(v.z, o.z)
                    && is_equal_approx_f(v.w, o.w),
            ))
        }
        // ⚠️ Godot's two axis scans are DELIBERATELY asymmetric
        // (`vector4.cpp:37-59`): `min` compares with `<=` so a tie picks the
        // LAST axis, `max` with `>` so a tie picks the FIRST. Writing both the
        // same way is wrong on ties and invisible to any fixture whose
        // components are all distinct.
        "max_axis_index" => {
            let mut idx = 0;
            let mut best = v.x;
            for (i, c) in [v.y, v.z, v.w].iter().enumerate() {
                if *c > best {
                    best = *c;
                    idx = i + 1;
                }
            }
            Ok(Variant::Int(idx as i64))
        }
        "min_axis_index" => {
            let mut idx = 0;
            let mut best = v.x;
            for (i, c) in [v.y, v.z, v.w].iter().enumerate() {
                if *c <= best {
                    best = *c;
                    idx = i + 1;
                }
            }
            Ok(Variant::Int(idx as i64))
        }
        _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Vector4.{method}"),
        ))),
    }
}

/// `Vector4i`'s instance methods (pat-s3rrv.59).
///
/// ⚠️ `length` is a FLOAT while every component and `length_squared` are Ints —
/// Godot's integer vectors keep the exact squared magnitude and only the square
/// root widens.
fn vector4i_method(v: Vector4i, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Vector4i.{method}()");
    match method {
        "length" => Ok(Variant::Float(v.length() as f64)),
        "length_squared" => Ok(Variant::Int(v.length_squared())),
        "abs" => Ok(Variant::Vector4i(v.abs())),
        "sign" => Ok(Variant::Vector4i(v.sign())),
        "min" => Ok(Variant::Vector4i(v.min(arg_vec4i(args, 0, &ctx)?))),
        "max" => Ok(Variant::Vector4i(v.max(arg_vec4i(args, 0, &ctx)?))),
        "clamp" => {
            let lo = arg_vec4i(args, 0, &ctx)?;
            let hi = arg_vec4i(args, 1, &ctx)?;
            Ok(Variant::Vector4i(v.clamp(lo, hi)))
        }
        "clampi" => {
            let lo = arg_int(args, 0, &ctx)?;
            let hi = arg_int(args, 1, &ctx)?;
            Ok(Variant::Vector4i(v.clampi(lo, hi)))
        }
        "mini" => Ok(Variant::Vector4i(v.mini(arg_int(args, 0, &ctx)?))),
        "maxi" => Ok(Variant::Vector4i(v.maxi(arg_int(args, 0, &ctx)?))),
        "snapped" => Ok(Variant::Vector4i(v.snapped(arg_vec4i(args, 0, &ctx)?))),
        "snappedi" => Ok(Variant::Vector4i(v.snappedi(arg_int(args, 0, &ctx)?))),
        "distance_to" => Ok(Variant::Float(
            v.distance_to(arg_vec4i(args, 0, &ctx)?) as f64
        )),
        // ⚠️ An INT, matching Godot's `int64_t` signature: the squared length of
        // an integer vector is exact, and a float would lose that for the
        // comparison this method exists to make cheap.
        "distance_squared_to" => Ok(Variant::Int(
            v.distance_squared_to(arg_vec4i(args, 0, &ctx)?),
        )),
        // Same asymmetry as Vector4's; see the note there.
        "max_axis_index" => {
            let mut idx = 0;
            let mut best = v.x;
            for (i, c) in [v.y, v.z, v.w].iter().enumerate() {
                if *c > best {
                    best = *c;
                    idx = i + 1;
                }
            }
            Ok(Variant::Int(idx as i64))
        }
        "min_axis_index" => {
            let mut idx = 0;
            let mut best = v.x;
            for (i, c) in [v.y, v.z, v.w].iter().enumerate() {
                if *c <= best {
                    best = *c;
                    idx = i + 1;
                }
            }
            Ok(Variant::Int(idx as i64))
        }
        _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Vector4i.{method}"),
        ))),
    }
}

fn vector3_method(v: Vector3, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Vector3.{method}()");
    let vec = |x: f32, y: f32, z: f32| -> Result<Variant, RuntimeError> {
        Ok(Variant::Vector3(Vector3::new(x, y, z)))
    };
    match method {
        "abs" => vec(v.x.abs(), v.y.abs(), v.z.abs()),
        "sign" => vec(sign_f(v.x), sign_f(v.y), sign_f(v.z)),
        "floor" => vec(v.x.floor(), v.y.floor(), v.z.floor()),
        "ceil" => vec(v.x.ceil(), v.y.ceil(), v.z.ceil()),
        "round" => vec(v.x.round(), v.y.round(), v.z.round()),

        "distance_squared_to" => {
            let o = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Float((v - o).length_squared() as f64))
        }
        "angle_to" => {
            let o = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Float(v.cross(o).length().atan2(v.dot(o)) as f64))
        }
        // Godot returns the axis INDEX (0=X, 1=Y, 2=Z), not the component.
        "max_axis_index" => Ok(Variant::Int(if v.x < v.y {
            if v.y < v.z {
                2
            } else {
                1
            }
        } else if v.x < v.z {
            2
        } else {
            0
        })),
        "min_axis_index" => Ok(Variant::Int(if v.x < v.y {
            if v.x < v.z {
                0
            } else {
                2
            }
        } else if v.y < v.z {
            1
        } else {
            2
        })),
        "is_normalized" => Ok(Variant::Bool(is_equal_approx_f(v.length_squared(), 1.0))),
        "is_zero_approx" => Ok(Variant::Bool(
            v.x.abs() < CMP_EPSILON && v.y.abs() < CMP_EPSILON && v.z.abs() < CMP_EPSILON,
        )),
        "is_finite" => Ok(Variant::Bool(
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
        )),
        "is_equal_approx" => {
            let o = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Bool(
                is_equal_approx_f(v.x, o.x)
                    && is_equal_approx_f(v.y, o.y)
                    && is_equal_approx_f(v.z, o.z),
            ))
        }

        "direction_to" => {
            let o = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Vector3((o - v).normalized()))
        }
        // Godot builds a rotation basis from the axis and transforms the vector
        // (`Basis(p_axis, p_angle).xform(*this)`). `from_axis_angle` lives on
        // Quaternion, not Basis — Basis reaches it via `from_quaternion`.
        "rotated" => {
            let axis = arg_vec3(args, 0, &ctx)?;
            let angle = arg_num(args, 1, &ctx)?;
            Ok(Variant::Vector3(
                Basis::from_quaternion(Quaternion::from_axis_angle(axis, angle)).xform(v),
            ))
        }
        "project" => {
            let o = arg_vec3(args, 0, &ctx)?;
            let denom = o.length_squared();
            if denom == 0.0 {
                return Ok(Variant::Vector3(Vector3::ZERO));
            }
            Ok(Variant::Vector3(o * (v.dot(o) / denom)))
        }
        "reflect" => {
            let n = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Vector3(n * (2.0 * v.dot(n)) - v))
        }
        "bounce" => {
            let n = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Vector3(-(n * (2.0 * v.dot(n)) - v)))
        }
        "slide" => {
            let n = arg_vec3(args, 0, &ctx)?;
            Ok(Variant::Vector3(v - n * v.dot(n)))
        }
        "limit_length" => {
            let len = if args.is_empty() {
                1.0
            } else {
                arg_num(args, 0, &ctx)?
            };
            let l = v.length();
            if l > 0.0 && len < l {
                Ok(Variant::Vector3(v / l * len))
            } else {
                Ok(Variant::Vector3(v))
            }
        }
        "move_toward" => {
            let to = arg_vec3(args, 0, &ctx)?;
            let delta = arg_num(args, 1, &ctx)?;
            let vd = to - v;
            let len = vd.length();
            if len <= delta || len < CMP_EPSILON {
                Ok(Variant::Vector3(to))
            } else {
                Ok(Variant::Vector3(v + vd / len * delta))
            }
        }
        "snapped" => {
            let step = arg_vec3(args, 0, &ctx)?;
            vec(
                snap_f(v.x, step.x),
                snap_f(v.y, step.y),
                snap_f(v.z, step.z),
            )
        }
        "snappedf" => {
            let step = arg_num(args, 0, &ctx)?;
            vec(snap_f(v.x, step), snap_f(v.y, step), snap_f(v.z, step))
        }
        "clamp" => {
            let lo = arg_vec3(args, 0, &ctx)?;
            let hi = arg_vec3(args, 1, &ctx)?;
            vec(
                v.x.clamp(lo.x, hi.x),
                v.y.clamp(lo.y, hi.y),
                v.z.clamp(lo.z, hi.z),
            )
        }
        "clampf" => {
            let lo = arg_num(args, 0, &ctx)?;
            let hi = arg_num(args, 1, &ctx)?;
            vec(v.x.clamp(lo, hi), v.y.clamp(lo, hi), v.z.clamp(lo, hi))
        }
        "min" => {
            let o = arg_vec3(args, 0, &ctx)?;
            vec(v.x.min(o.x), v.y.min(o.y), v.z.min(o.z))
        }
        "max" => {
            let o = arg_vec3(args, 0, &ctx)?;
            vec(v.x.max(o.x), v.y.max(o.y), v.z.max(o.z))
        }
        "minf" => {
            let s = arg_num(args, 0, &ctx)?;
            vec(v.x.min(s), v.y.min(s), v.z.min(s))
        }
        "maxf" => {
            let s = arg_num(args, 0, &ctx)?;
            vec(v.x.max(s), v.y.max(s), v.z.max(s))
        }
        "posmod" => {
            let m = arg_num(args, 0, &ctx)?;
            vec(fposmod(v.x, m), fposmod(v.y, m), fposmod(v.z, m))
        }
        "posmodv" => {
            let m = arg_vec3(args, 0, &ctx)?;
            vec(fposmod(v.x, m.x), fposmod(v.y, m.y), fposmod(v.z, m.z))
        }
        _ => Err(undefined_method("Vector3", method)),
    }
}

fn vector2i_method(v: Vector2i, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Vector2i.{method}()");
    let vec = |x: i32, y: i32| -> Result<Variant, RuntimeError> {
        Ok(Variant::Vector2i(Vector2i::new(x, y)))
    };
    // Godot computes the int vectors' lengths in double precision.
    let len_sq = (v.x as f64) * (v.x as f64) + (v.y as f64) * (v.y as f64);
    match method {
        "abs" => vec(v.x.abs(), v.y.abs()),
        "sign" => vec(sign_i(v.x), sign_i(v.y)),
        "length" => Ok(Variant::Float(len_sq.sqrt())),
        "length_squared" => Ok(Variant::Int(len_sq as i64)),
        "aspect" => Ok(Variant::Float(v.x as f64 / v.y as f64)),
        "max_axis_index" => Ok(Variant::Int(if v.x < v.y { 1 } else { 0 })),
        "min_axis_index" => Ok(Variant::Int(if v.x < v.y { 0 } else { 1 })),
        "distance_to" => {
            let o = arg_vec2i(args, 0, &ctx)?;
            let (dx, dy) = ((o.x - v.x) as f64, (o.y - v.y) as f64);
            Ok(Variant::Float((dx * dx + dy * dy).sqrt()))
        }
        "distance_squared_to" => {
            let o = arg_vec2i(args, 0, &ctx)?;
            let (dx, dy) = ((o.x - v.x) as i64, (o.y - v.y) as i64);
            Ok(Variant::Int(dx * dx + dy * dy))
        }
        "snapped" => {
            let step = arg_vec2i(args, 0, &ctx)?;
            vec(snap_i(v.x, step.x), snap_i(v.y, step.y))
        }
        "snappedi" => {
            let step = arg_int(args, 0, &ctx)?;
            vec(snap_i(v.x, step), snap_i(v.y, step))
        }
        "clamp" => {
            let lo = arg_vec2i(args, 0, &ctx)?;
            let hi = arg_vec2i(args, 1, &ctx)?;
            vec(v.x.clamp(lo.x, hi.x), v.y.clamp(lo.y, hi.y))
        }
        "clampi" => {
            let lo = arg_int(args, 0, &ctx)?;
            let hi = arg_int(args, 1, &ctx)?;
            vec(v.x.clamp(lo, hi), v.y.clamp(lo, hi))
        }
        "min" => {
            let o = arg_vec2i(args, 0, &ctx)?;
            vec(v.x.min(o.x), v.y.min(o.y))
        }
        "max" => {
            let o = arg_vec2i(args, 0, &ctx)?;
            vec(v.x.max(o.x), v.y.max(o.y))
        }
        "mini" => {
            let s = arg_int(args, 0, &ctx)?;
            vec(v.x.min(s), v.y.min(s))
        }
        "maxi" => {
            let s = arg_int(args, 0, &ctx)?;
            vec(v.x.max(s), v.y.max(s))
        }
        _ => Err(undefined_method("Vector2i", method)),
    }
}

fn vector3i_method(v: Vector3i, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let ctx = format!("Vector3i.{method}()");
    let vec = |x: i32, y: i32, z: i32| -> Result<Variant, RuntimeError> {
        Ok(Variant::Vector3i(Vector3i::new(x, y, z)))
    };
    let len_sq =
        (v.x as f64) * (v.x as f64) + (v.y as f64) * (v.y as f64) + (v.z as f64) * (v.z as f64);
    match method {
        "abs" => vec(v.x.abs(), v.y.abs(), v.z.abs()),
        "sign" => vec(sign_i(v.x), sign_i(v.y), sign_i(v.z)),
        "length" => Ok(Variant::Float(len_sq.sqrt())),
        "length_squared" => Ok(Variant::Int(len_sq as i64)),
        "max_axis_index" => Ok(Variant::Int(if v.x < v.y {
            if v.y < v.z {
                2
            } else {
                1
            }
        } else if v.x < v.z {
            2
        } else {
            0
        })),
        "min_axis_index" => Ok(Variant::Int(if v.x < v.y {
            if v.x < v.z {
                0
            } else {
                2
            }
        } else if v.y < v.z {
            1
        } else {
            2
        })),
        "distance_to" => {
            let o = arg_vec3i(args, 0, &ctx)?;
            let (dx, dy, dz) = ((o.x - v.x) as f64, (o.y - v.y) as f64, (o.z - v.z) as f64);
            Ok(Variant::Float((dx * dx + dy * dy + dz * dz).sqrt()))
        }
        "distance_squared_to" => {
            let o = arg_vec3i(args, 0, &ctx)?;
            let (dx, dy, dz) = ((o.x - v.x) as i64, (o.y - v.y) as i64, (o.z - v.z) as i64);
            Ok(Variant::Int(dx * dx + dy * dy + dz * dz))
        }
        "snapped" => {
            let step = arg_vec3i(args, 0, &ctx)?;
            vec(
                snap_i(v.x, step.x),
                snap_i(v.y, step.y),
                snap_i(v.z, step.z),
            )
        }
        "snappedi" => {
            let step = arg_int(args, 0, &ctx)?;
            vec(snap_i(v.x, step), snap_i(v.y, step), snap_i(v.z, step))
        }
        "clamp" => {
            let lo = arg_vec3i(args, 0, &ctx)?;
            let hi = arg_vec3i(args, 1, &ctx)?;
            vec(
                v.x.clamp(lo.x, hi.x),
                v.y.clamp(lo.y, hi.y),
                v.z.clamp(lo.z, hi.z),
            )
        }
        "clampi" => {
            let lo = arg_int(args, 0, &ctx)?;
            let hi = arg_int(args, 1, &ctx)?;
            vec(v.x.clamp(lo, hi), v.y.clamp(lo, hi), v.z.clamp(lo, hi))
        }
        "min" => {
            let o = arg_vec3i(args, 0, &ctx)?;
            vec(v.x.min(o.x), v.y.min(o.y), v.z.min(o.z))
        }
        "max" => {
            let o = arg_vec3i(args, 0, &ctx)?;
            vec(v.x.max(o.x), v.y.max(o.y), v.z.max(o.z))
        }
        "mini" => {
            let s = arg_int(args, 0, &ctx)?;
            vec(v.x.min(s), v.y.min(s), v.z.min(s))
        }
        "maxi" => {
            let s = arg_int(args, 0, &ctx)?;
            vec(v.x.max(s), v.y.max(s), v.z.max(s))
        }
        _ => Err(undefined_method("Vector3i", method)),
    }
}

/// Static constructors on the vector types: `Vector2.from_angle(a)`.
///
/// Godot exposes these as static methods on the builtin, so they arrive as a
/// call on a *type name* rather than on a value.
pub(crate) fn vector_static_call(
    type_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Result<Variant, RuntimeError>> {
    match (type_name, method) {
        // String's six statics (pat-mgixk.107). They land here because this
        // hook is consulted for any `Ident.method(..)`, and routing them
        // through it means the direct `String.num(..)` spelling and
        // `ClassDB.class_call_static("String", "num", ..)` cannot disagree.
        //
        // ⚠️ These deliberately do NOT use `arg_num`: it returns **f32**, and
        // Godot's `String.num` takes a double. Narrowing first would print the
        // widening error for anything needing more than ~7 digits, which is the
        // opposite of what these formatters are for.
        ("String", "num") => {
            let ctx = "String.num()";
            Some((|| {
                let n = string_static_f64(args, 0, ctx)?;
                // Godot's `decimals` defaults to -1, which means "use num_real".
                let decimals = string_static_i64(args, 1).unwrap_or(-1);
                Ok(Variant::String(if decimals < 0 {
                    gdvariant::num_real_f64(n, true)
                } else {
                    gdvariant::num_with_decimals(n, decimals.clamp(0, 32) as i32)
                }))
            })())
        }
        ("String", "num_int64") => {
            let ctx = "String.num_int64()";
            Some((|| {
                let n = string_static_i64(args, 0)
                    .ok_or_else(|| type_error(format!("{ctx} expects an int")))?;
                let base = string_static_i64(args, 1).unwrap_or(10);
                let capitalize = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                Ok(Variant::String(gd_num_in_base(n as i128, base, capitalize)))
            })())
        }
        ("String", "num_uint64") => {
            let ctx = "String.num_uint64()";
            Some((|| {
                let n = string_static_i64(args, 0)
                    .ok_or_else(|| type_error(format!("{ctx} expects an int")))?;
                let base = string_static_i64(args, 1).unwrap_or(10);
                let capitalize = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                // Reinterpreted as unsigned, which is the whole difference from
                // `num_int64`: -1 prints as 18446744073709551615, not "-1".
                Ok(Variant::String(gd_num_in_base(
                    (n as u64) as i128,
                    base,
                    capitalize,
                )))
            })())
        }
        ("String", "num_scientific") => {
            let ctx = "String.num_scientific()";
            Some(string_static_f64(args, 0, ctx).map(|n| Variant::String(gd_num_scientific(n))))
        }
        ("String", "chr") => {
            let ctx = "String.chr()";
            Some((|| {
                let code = string_static_i64(args, 0)
                    .ok_or_else(|| type_error(format!("{ctx} expects a code point")))?;
                // An unpaired surrogate or out-of-range value is not a scalar
                // value; Godot yields nothing usable there either, so an empty
                // string is the honest answer rather than a replacement char
                // that looks like successful conversion.
                let out = u32::try_from(code)
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from)
                    .unwrap_or_default();
                Ok(Variant::String(out))
            })())
        }
        ("String", "humanize_size") => {
            let ctx = "String.humanize_size()";
            Some((|| {
                let bytes = string_static_i64(args, 0)
                    .ok_or_else(|| type_error(format!("{ctx} expects a byte count")))?;
                Ok(Variant::String(gd_humanize_size(bytes.max(0) as u64)))
            })())
        }

        ("Vector2", "from_angle") => {
            let ctx = "Vector2.from_angle()";
            Some(arg_num(args, 0, ctx).map(|a| Variant::Vector2(Vector2::new(a.cos(), a.sin()))))
        }

        // Color's static constructors (pat-s3rrv.58). These arrive here rather
        // than at `call_method_on` because they are calls on a TYPE NAME, and
        // this hook is consulted for any `Ident.method(...)` — not only the
        // vector types its name suggests.
        ("Color", "from_hsv") => {
            let ctx = "Color.from_hsv()";
            Some((|| {
                let h = arg_num(args, 0, ctx)?;
                let s = arg_num(args, 1, ctx)?;
                let v = arg_num(args, 2, ctx)?;
                // Godot's alpha argument is optional and defaults to opaque.
                let a = match args.get(3) {
                    Some(_) => arg_num(args, 3, ctx)?,
                    None => 1.0,
                };
                Ok(Variant::Color(Color::from_hsv(h, s, v, a)))
            })())
        }
        // The Oklab variant. Same shape as `from_hsv`, and deliberately right
        // next to it: the two differ only in colour space, and a caller
        // reaching for one should see the other.
        ("Color", "from_ok_hsl") => {
            let ctx = "Color.from_ok_hsl()";
            Some((|| {
                let h = arg_num(args, 0, ctx)?;
                let s = arg_num(args, 1, ctx)?;
                let l = arg_num(args, 2, ctx)?;
                let a = match args.get(3) {
                    Some(_) => arg_num(args, 3, ctx)?,
                    None => 1.0,
                };
                Ok(Variant::Color(Color::from_ok_hsl(h, s, l, a)))
            })())
        }
        // ⚠️ Reads the raw i64 rather than going through `arg_int`, which
        // narrows to i32. An RGBE9995 word is a FULL u32 and has its top bit
        // set whenever the exponent is 16 or more — a perfectly ordinary
        // value like 3221488129 (opaque white) narrows to a negative i32 and
        // would unpack to something else entirely.
        // Projection's static constructors (pat-s3rrv.59). Without these the
        // only Projection a script could make was the identity, so
        // `is_orthogonal` had nothing to answer `false` about.
        ("Projection", "create_perspective") => {
            let ctx = "Projection.create_perspective()";
            Some((|| {
                let fov = arg_num(args, 0, ctx)?;
                let aspect = arg_num(args, 1, ctx)?;
                let z_near = arg_num(args, 2, ctx)?;
                let z_far = arg_num(args, 3, ctx)?;
                // ⚠️ `flip_fov` reinterprets the angle as HORIZONTAL, and the
                // conversion is Godot's own `get_fovy(fovx, 1/aspect)`
                // (`projection.h:103`), not a swap of the aspect. gdcore's
                // constructor only takes a vertical fov, so the conversion
                // happens here.
                let fov = if arg_flag(args, 4) {
                    let half = (fov.to_radians() * 0.5).tan();
                    (((1.0 / aspect) * half).atan() * 2.0).to_degrees()
                } else {
                    fov
                };
                Ok(Variant::Projection(Projection::create_perspective(
                    fov, aspect, z_near, z_far,
                )))
            })())
        }
        ("Projection", "create_orthogonal") => {
            let ctx = "Projection.create_orthogonal()";
            Some((|| {
                Ok(Variant::Projection(Projection::create_orthogonal(
                    arg_num(args, 0, ctx)?,
                    arg_num(args, 1, ctx)?,
                    arg_num(args, 2, ctx)?,
                    arg_num(args, 3, ctx)?,
                    arg_num(args, 4, ctx)?,
                    arg_num(args, 5, ctx)?,
                )))
            })())
        }
        ("Projection", "create_frustum") => {
            let ctx = "Projection.create_frustum()";
            Some((|| {
                Ok(Variant::Projection(Projection::create_frustum(
                    arg_num(args, 0, ctx)?,
                    arg_num(args, 1, ctx)?,
                    arg_num(args, 2, ctx)?,
                    arg_num(args, 3, ctx)?,
                    arg_num(args, 4, ctx)?,
                    arg_num(args, 5, ctx)?,
                )))
            })())
        }
        ("Projection", "create_orthogonal_aspect") => {
            let ctx = "Projection.create_orthogonal_aspect()";
            Some((|| {
                let size = arg_num(args, 0, ctx)?;
                let aspect = arg_num(args, 1, ctx)?;
                let z_near = arg_num(args, 2, ctx)?;
                let z_far = arg_num(args, 3, ctx)?;
                // 🔑 Godot's flag reads INVERTED here: `set_orthogonal` does
                // `if (!p_flip_fov) { p_size *= p_aspect; }`
                // (`projection.cpp:356-362`) — the widening happens when the
                // flag is FALSE. gdcore's `create_orthogonal_aspect` already
                // encodes that default case, so only the flipped one needs
                // spelling out.
                if arg_flag(args, 4) {
                    let half_w = size * 0.5;
                    let half_h = size / aspect * 0.5;
                    Ok(Variant::Projection(Projection::create_orthogonal(
                        -half_w, half_w, -half_h, half_h, z_near, z_far,
                    )))
                } else {
                    Ok(Variant::Projection(Projection::create_orthogonal_aspect(
                        size, aspect, z_near, z_far,
                    )))
                }
            })())
        }
        ("Color", "from_rgbe9995") => {
            let ctx = "Color.from_rgbe9995()";
            Some((|| match args.first() {
                Some(Variant::Int(v)) if (0..=u32::MAX as i64).contains(v) => {
                    Ok(Variant::Color(Color::from_rgbe9995(*v as u32)))
                }
                Some(Variant::Int(v)) => Err(type_error(format!(
                    "{ctx} expects a 32-bit unsigned value, got {v}"
                ))),
                other => Err(type_error(format!(
                    "{ctx} expects an int, got {}",
                    describe(other)
                ))),
            })())
        }
        ("Color", "from_rgba8") => {
            let ctx = "Color.from_rgba8()";
            Some((|| {
                let r = arg_int(args, 0, ctx)? as i64;
                let g = arg_int(args, 1, ctx)? as i64;
                let b = arg_int(args, 2, ctx)? as i64;
                let a = match args.get(3) {
                    Some(_) => arg_int(args, 3, ctx)? as i64,
                    None => 255,
                };
                Ok(Variant::Color(Color::from_rgba8(r, g, b, a)))
            })())
        }
        // `Color.html("#ff8800")`. An unparseable string is an ERROR rather
        // than a silent black — Godot's own `html` reports the bad value, and a
        // silent default is the kind of failure that surfaces as a wrong-looking
        // scene hours later.
        ("Color", "html") => {
            let ctx = "Color.html()";
            Some(match args.first() {
                Some(Variant::String(text)) => Color::from_html(text)
                    .map(Variant::Color)
                    .ok_or_else(|| type_error(format!("{ctx}: invalid html colour {text:?}"))),
                Some(other) => Err(type_error(format!("{ctx} expects a String, got {other:?}"))),
                None => Err(type_error(format!("{ctx} is missing argument 0"))),
            })
        }
        // `Color.from_string(str, default)` — the ONE static this family was
        // missing (pat-s3rrv.58).
        //
        // ⚠️ Unlike its neighbours it must NOT error on a bad string: Godot's
        // `Color::from_string` exists precisely to be total, falling back to the
        // caller's default. `html()` above is the strict spelling and errors;
        // conflating the two would make `from_string` useless for the case it
        // was added for — parsing user or project text where a bad value should
        // degrade to a chosen colour rather than abort the script.
        //
        // Order is Godot's own (core/math/color.cpp): html FIRST, then the named
        // palette, then the default. Trying the palette first would let a name
        // that also parses as hex resolve to the wrong one.
        ("Color", "from_string") => {
            let ctx = "Color.from_string()";
            Some(match (args.first(), args.get(1)) {
                (Some(Variant::String(text)), default) => {
                    let fallback = match default {
                        Some(Variant::Color(c)) => *c,
                        // Godot's signature requires the default; a missing or
                        // non-Color one degrades to transparent black rather
                        // than erroring, keeping the method total.
                        _ => Color::new(0.0, 0.0, 0.0, 0.0),
                    };
                    Ok(Variant::Color(
                        Color::from_html(text)
                            .or_else(|| Color::named(text))
                            .unwrap_or(fallback),
                    ))
                }
                (Some(other), _) => {
                    Err(type_error(format!("{ctx} expects a String, got {other:?}")))
                }
                (None, _) => Err(type_error(format!("{ctx} is missing argument 0"))),
            })
        }
        ("Color", "html_is_valid") => {
            let ctx = "Color.html_is_valid()";
            Some(match args.first() {
                Some(Variant::String(text)) => Ok(Variant::Bool(Color::html_is_valid(text))),
                Some(other) => Err(type_error(format!("{ctx} expects a String, got {other:?}"))),
                None => Err(type_error(format!("{ctx} is missing argument 0"))),
            })
        }
        ("Color", "hex") => {
            let ctx = "Color.hex()";
            Some(arg_int(args, 0, ctx).map(|h| Variant::Color(Color::from_hex32(h as u32))))
        }
        ("Color", "hex64") => {
            let ctx = "Color.hex64()";
            Some(match args.first() {
                Some(Variant::Int(h)) => Ok(Variant::Color(Color::from_hex64(*h as u64))),
                Some(other) => Err(type_error(format!("{ctx} expects an int, got {other:?}"))),
                None => Err(type_error(format!("{ctx} is missing argument 0"))),
            })
        }

        // The affine static constructors (pat-s3rrv.148). `Basis.from_euler`
        // is the documented inverse of `Basis.get_euler`, and without it a
        // script could read a rotation back in a chosen order but never build
        // one — so the order argument had no round trip to be checked against.
        ("Basis", "from_euler") => {
            let ctx = "Basis.from_euler()";
            Some((|| {
                let euler = arg_vec3(args, 0, ctx)?;
                let order = arg_euler_order(args, 1, ctx)?;
                Ok(Variant::Basis(Basis::from_euler_order(euler, order)))
            })())
        }
        ("Basis", "from_scale") => {
            let ctx = "Basis.from_scale()";
            Some(arg_vec3(args, 0, ctx).map(|s| Variant::Basis(Basis::from_scale(s))))
        }
        ("Basis", "looking_at") => {
            let ctx = "Basis.looking_at()";
            Some((|| {
                let target = arg_vec3(args, 0, ctx)?;
                // Godot's `up` defaults to +Y.
                let up = match args.get(1) {
                    None | Some(Variant::Nil) => Vector3::new(0.0, 1.0, 0.0),
                    _ => arg_vec3(args, 1, ctx)?,
                };
                Ok(Variant::Basis(Basis::looking_at(target, up)))
            })())
        }
        ("Quaternion", "from_euler") => {
            let ctx = "Quaternion.from_euler()";
            Some((|| {
                let euler = arg_vec3(args, 0, ctx)?;
                // ⓘ Godot's `Quaternion.from_euler` takes NO order argument —
                // it is YXZ only. Accepting one here would invent an API.
                Ok(Variant::Quaternion(Quaternion::from_euler_order(
                    euler,
                    EulerOrder::Yxz,
                )))
            })())
        }
        _ => None,
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    // pat-s3rrv.58 — the Color dispatch tables.
    //
    // These call `variant_math` directly rather than through the interpreter,
    // so they verify the half of the wiring that lives here independently of
    // the `call_method_on` delegation line in `interpreter.rs`.

    fn col(r: f32, g: f32, b: f32, a: f32) -> Variant {
        Variant::Color(Color::new(r, g, b, a))
    }

    fn unwrap_color(v: Variant) -> Color {
        match v {
            Variant::Color(c) => c,
            other => panic!("expected a Color, got {other:?}"),
        }
    }

    fn call(c: Variant, method: &str, args: &[Variant]) -> Variant {
        call_method(&c, method, args)
            .unwrap_or_else(|| panic!("Color.{method} is not dispatched at all"))
            .unwrap_or_else(|e| panic!("Color.{method} errored: {e:?}"))
    }

    /// The acceptance's own case: `Color.YELLOW.darkened(0.2)`.
    #[test]
    fn color_instance_methods_dispatch() {
        let yellow = Variant::Color(Color::named("YELLOW").expect("YELLOW"));
        let dark = unwrap_color(call(yellow.clone(), "darkened", &[Variant::Float(0.2)]));
        assert!(
            dark.is_equal_approx(Color::new(0.8, 0.8, 0.0, 1.0)),
            "{dark:?}"
        );

        let inv = unwrap_color(call(col(1.0, 1.0, 0.0, 1.0), "inverted", &[]));
        assert!(
            inv.is_equal_approx(Color::new(0.0, 0.0, 1.0, 1.0)),
            "{inv:?}"
        );

        match call(col(1.0, 1.0, 0.0, 1.0), "get_luminance", &[]) {
            Variant::Float(l) => assert!((l - 0.9278).abs() < 1e-3, "{l}"),
            other => panic!("expected Float, got {other:?}"),
        }

        let blended = unwrap_color(call(
            col(1.0, 0.0, 0.0, 1.0),
            "blend",
            &[col(0.0, 0.0, 1.0, 0.5)],
        ));
        assert!(
            blended.is_equal_approx(Color::new(0.5, 0.0, 0.5, 1.0)),
            "{blended:?}"
        );
    }

    /// ⚠️ Godot's `to_html` defaults to WITH alpha. Defaulting to `false` would
    /// silently drop alpha from every colour a script serialises.
    #[test]
    fn to_html_defaults_to_including_alpha() {
        let c = Variant::Color(Color::from_html("#ff8800").unwrap());
        assert_eq!(
            call(c.clone(), "to_html", &[]),
            Variant::String("ff8800ff".into()),
            "a bare to_html() must include alpha"
        );
        // …and the acceptance's explicit `to_html(false)`.
        assert_eq!(
            call(c, "to_html", &[Variant::Bool(false)]),
            Variant::String("ff8800".into())
        );
    }

    #[test]
    fn color_constants_resolve_through_static_member() {
        let yellow = static_member("Color", "YELLOW").expect("YELLOW resolves");
        assert!(unwrap_color(yellow).is_equal_approx(Color::new(1.0, 1.0, 0.0, 1.0)));
        // One that the old six-arm table could not answer.
        assert!(static_member("Color", "REBECCA_PURPLE").is_some());
        assert!(static_member("Color", "NOT_A_COLOUR").is_none());
    }

    #[test]
    fn color_members_read_through_member_get() {
        let c = Color::new(0.5, 0.0, 1.0, 1.0);
        let v = Variant::Color(c);
        assert_eq!(member_get(&v, "r8"), Some(Variant::Int(128)));
        assert_eq!(member_get(&v, "b8"), Some(Variant::Int(255)));
        match member_get(&v, "h") {
            // ⚠️ 0..1, not degrees.
            Some(Variant::Float(h)) => assert!((0.0..=1.0).contains(&h), "hue {h} out of 0..1"),
            other => panic!("expected Float hue, got {other:?}"),
        }
        assert_eq!(member_get(&v, "not_a_member"), None);
    }

    /// `Color.from_hsv(...)` and `Color.html(...)` are calls on a TYPE NAME, so
    /// they route through `vector_static_call` — which, despite its name, is
    /// consulted for any `Ident.method(...)`.
    #[test]
    fn color_static_constructors_dispatch() {
        let cyan = vector_static_call(
            "Color",
            "from_hsv",
            &[
                Variant::Float(0.5),
                Variant::Float(1.0),
                Variant::Float(1.0),
            ],
        )
        .expect("from_hsv is dispatched")
        .expect("from_hsv succeeds");
        assert!(unwrap_color(cyan).is_equal_approx(Color::new(0.0, 1.0, 1.0, 1.0)));

        let parsed = vector_static_call("Color", "html", &[Variant::String("#ff8800".into())])
            .expect("html is dispatched")
            .expect("html succeeds");
        assert!(unwrap_color(parsed).is_equal_approx(Color::from_html("ff8800").unwrap()));

        assert_eq!(
            vector_static_call("Color", "html_is_valid", &[Variant::String("#zzz".into())])
                .expect("dispatched")
                .expect("succeeds"),
            Variant::Bool(false)
        );
    }

    /// A bad argument is a NAMED error, not a silent default — a silent black
    /// surfaces as a wrong-looking scene long after the mistake.
    #[test]
    fn bad_arguments_report_rather_than_defaulting() {
        assert!(
            call_method(&col(1.0, 0.0, 0.0, 1.0), "blend", &[Variant::Int(3)])
                .expect("dispatched")
                .is_err(),
            "blend() with a non-Color must error"
        );
        assert!(
            vector_static_call("Color", "html", &[Variant::String("nonsense".into())])
                .expect("dispatched")
                .is_err(),
            "an unparseable html colour must error rather than yielding black"
        );
        assert!(
            call_method(&col(1.0, 0.0, 0.0, 1.0), "no_such_method", &[])
                .expect("Color is dispatched")
                .is_err(),
            "an unknown method must name itself"
        );
    }

    /// pat-s3rrv.58: the member WRITE half. `member_get` answered for all
    /// eleven Color members but `member_set` had no Color arm at all, so
    /// `col.r8 = 128` resolved to `None` and the assignment silently did
    /// nothing — the value read back unchanged, with no error to point at.
    #[test]
    fn color_members_are_writable() {
        let set = |c: Variant, member: &str, v: Variant| {
            unwrap_color(
                member_set(&c, member, &v)
                    .unwrap_or_else(|| panic!("Color.{member} is not settable at all"))
                    .unwrap_or_else(|e| panic!("Color.{member} = … errored: {e:?}")),
            )
        };

        assert_eq!(
            set(col(0.0, 0.0, 0.0, 1.0), "r8", Variant::Int(128)).r8(),
            128
        );
        assert_eq!(
            set(col(0.0, 0.0, 0.0, 1.0), "a8", Variant::Int(64)).a8(),
            64
        );

        // The plain channels stay float-typed, and accept an Int the way
        // GDScript's `1` and `1.0` are both ordinary ways to write one.
        assert!((set(col(0.0, 0.0, 0.0, 1.0), "g", Variant::Float(0.5)).g - 0.5).abs() < 1e-6);
        assert!((set(col(0.0, 0.0, 0.0, 1.0), "b", Variant::Int(1)).b - 1.0).abs() < 1e-6);

        // Writing hue re-derives through HSV and preserves alpha.
        let shifted = set(col(1.0, 0.0, 0.0, 0.25), "h", Variant::Float(0.5));
        assert!((shifted.h() - 0.5).abs() < 1e-4, "{shifted:?}");
        assert!((shifted.a - 0.25).abs() < 1e-6, "alpha must survive");

        // 🔑 An out-of-range byte CLAMPS rather than wrapping. This goes
        // through `want_i64`, not `want_i32`: narrowing first would turn
        // 4294967296 into 0 and write BLACK where Godot writes white.
        assert_eq!(
            set(col(0.0, 0.0, 0.0, 1.0), "r8", Variant::Int(4_294_967_296)).r8(),
            255,
            "a huge byte must clamp to 255, not wrap to 0"
        );

        // An unknown member is still not settable, and a wrong-typed value is
        // an error rather than a silent no-op.
        assert!(
            member_set(&col(0.0, 0.0, 0.0, 1.0), "no_such_member", &Variant::Int(1)).is_none(),
            "member_set must gate on member_get"
        );
        assert!(
            member_set(
                &col(0.0, 0.0, 0.0, 1.0),
                "r8",
                &Variant::String("128".into())
            )
            .expect("r8 is a real member")
            .is_err(),
            "a String assigned to r8 must error, not coerce"
        );
    }

    /// pat-s3rrv.58: `Color.from_rgbe9995` reaches GDScript.
    ///
    /// 🔑 The test value 3221488129 is above `i32::MAX` on purpose. Every
    /// RGBE9995 word with an exponent of 16 or more has its top bit set, so
    /// routing this through the crate's i32 argument helper would narrow it to
    /// a negative and unpack a different colour entirely. That is the bug this
    /// case exists to catch, and in-range values never expose it.
    #[test]
    fn color_from_rgbe9995_reaches_scripts_without_narrowing() {
        let white = vector_static_call("Color", "from_rgbe9995", &[Variant::Int(3_221_488_129)])
            .expect("from_rgbe9995 is dispatched")
            .expect("from_rgbe9995 succeeds");
        assert!(
            unwrap_color(white).is_equal_approx(Color::new(1.0, 1.0, 1.0, 1.0)),
            "3221488129 is opaque white; a narrowed i32 would not be"
        );

        // HDR values pass through unclamped — that is the point of the format.
        let hdr = vector_static_call("Color", "from_rgbe9995", &[Variant::Int(3_221_225_983)])
            .expect("dispatched")
            .expect("succeeds");
        assert_eq!(unwrap_color(hdr).r, 511.0);

        // Out of range and wrong-typed inputs are errors, not silent wraps.
        assert!(
            vector_static_call("Color", "from_rgbe9995", &[Variant::Int(-1)])
                .expect("dispatched")
                .is_err(),
            "a negative word must error rather than wrap to a huge u32"
        );
        assert!(
            vector_static_call("Color", "from_rgbe9995", &[Variant::Int(4_294_967_296)])
                .expect("dispatched")
                .is_err(),
            "a value past u32::MAX must error"
        );
    }

    /// pat-s3rrv.58: the OKHSL surface reaches GDScript — the members read and
    /// write, and `Color.from_ok_hsl` dispatches as a static.
    ///
    /// Values come from gdcore's port of the vendored `ok_color.h`, whose hue
    /// is pinned against the published OkLCh figure for sRGB red (29.23°).
    #[test]
    fn color_ok_hsl_members_and_constructor_reach_scripts() {
        let red = col(1.0, 0.0, 0.0, 1.0);

        match member_get(&red, "ok_hsl_h").expect("ok_hsl_h is a member") {
            Variant::Float(h) => assert!((h - 0.081205).abs() < 2e-4, "hue {h}"),
            other => panic!("expected Float, got {other:?}"),
        }
        match member_get(&red, "ok_hsl_l").expect("ok_hsl_l is a member") {
            Variant::Float(l) => assert!((l - 0.568085).abs() < 2e-4, "lightness {l}"),
            other => panic!("expected Float, got {other:?}"),
        }

        // 🔑 `ok_hsl_l` is not a second name for `v`. Red is perceptually much
        // darker than white, so the two disagree by more than 0.4 — a wiring
        // slip that pointed one at the other would pass every "is it a float"
        // check.
        match (
            member_get(&red, "v").expect("v"),
            member_get(&red, "ok_hsl_l").expect("ok_hsl_l"),
        ) {
            (Variant::Float(v), Variant::Float(l)) => {
                assert!((v - l).abs() > 0.4, "v={v} ok_hsl_l={l}")
            }
            other => panic!("expected two Floats, got {other:?}"),
        }

        // The members are writable too.
        let lightened = member_set(&red, "ok_hsl_l", &Variant::Float(0.8))
            .expect("ok_hsl_l is settable")
            .expect("assignment succeeds");
        match member_get(&lightened, "ok_hsl_l").expect("ok_hsl_l") {
            Variant::Float(l) => assert!((l - 0.8).abs() < 2e-3, "written lightness {l}"),
            other => panic!("expected Float, got {other:?}"),
        }

        // And the static constructor round-trips red back.
        let rebuilt = vector_static_call(
            "Color",
            "from_ok_hsl",
            &[
                Variant::Float(0.081205),
                Variant::Float(1.0),
                Variant::Float(0.568085),
            ],
        )
        .expect("from_ok_hsl is dispatched")
        .expect("from_ok_hsl succeeds");
        let c = unwrap_color(rebuilt);
        assert!(
            (c.r - 1.0).abs() < 2e-3 && c.g < 2e-3 && c.b < 2e-3,
            "Color.from_ok_hsl(red's own okhsl) must rebuild red, got {c:?}"
        );
    }

    /// `Color.from_string` is the TOTAL parser (pat-s3rrv.58).
    ///
    /// ⚠️ The third case is the one that matters and the only one that
    /// distinguishes this from `Color.html`: an unparseable string must return
    /// the caller's default, NOT an error. `html()` is deliberately strict and
    /// errors; `from_string` exists for user- and project-supplied text where a
    /// bad value should degrade to a chosen colour instead of aborting the
    /// script. An implementation that simply forwarded to `html` would satisfy
    /// the first case here and fail this one.
    #[test]
    fn color_from_string_falls_back_to_its_default() {
        let default = Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0));

        // An html string parses.
        let html = unwrap_color(
            vector_static_call(
                "Color",
                "from_string",
                &[Variant::String("#ff8800".into()), default.clone()],
            )
            .expect("from_string is dispatched")
            .expect("a valid html string succeeds"),
        );
        let expect = Color::from_html("ff8800").expect("fixture parses");
        assert!(
            html.is_equal_approx(expect),
            "from_string(\"#ff8800\") must parse as html, got {html:?}"
        );

        // A PALETTE NAME parses too — Godot tries html first, then `named`.
        let named = unwrap_color(
            vector_static_call(
                "Color",
                "from_string",
                &[Variant::String("YELLOW".into()), default.clone()],
            )
            .expect("from_string is dispatched")
            .expect("a palette name succeeds"),
        );
        assert!(
            named.is_equal_approx(Color::new(1.0, 1.0, 0.0, 1.0)),
            "from_string(\"YELLOW\") must resolve the named palette, got {named:?}"
        );

        // The point of the method: garbage yields the DEFAULT, not an error.
        let fallback = unwrap_color(
            vector_static_call(
                "Color",
                "from_string",
                &[Variant::String("not_a_colour".into()), default],
            )
            .expect("from_string is dispatched")
            .expect("an invalid string must NOT be an error"),
        );
        assert!(
            fallback.is_equal_approx(Color::new(1.0, 0.0, 0.0, 1.0)),
            "an unparseable string must return the caller's default, got {fallback:?}"
        );
    }
}
