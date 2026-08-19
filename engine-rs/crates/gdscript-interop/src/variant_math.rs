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

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector2i, Vector3, Vector3i};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
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
        "get_euler" => Ok(Variant::Vector3(b.to_euler())),
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
        "get_euler" => Ok(Variant::Vector3(q.to_euler())),
        "xform" => Ok(Variant::Vector3(q.xform(arg_vec3(args, 0, &ctx)?))),
        "get_angle" => Ok(Variant::Float((2.0 * q.w.clamp(-1.0, 1.0).acos()) as f64)),
        "get_axis" => {
            // Near-identity has no meaningful axis; Godot returns +Z.
            if q.w.abs() > 1.0 - 1e-6 {
                Ok(Variant::Vector3(Vector3::new(0.0, 0.0, 1.0)))
            } else {
                let inv = 1.0 / (1.0 - q.w * q.w).sqrt();
                Ok(Variant::Vector3(Vector3::new(
                    q.x * inv,
                    q.y * inv,
                    q.z * inv,
                )))
            }
        }
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
        _ => Err(undefined_method("Plane", method)),
    }
}

// ---------------------------------------------------------------------------
// The `*` operator
// ---------------------------------------------------------------------------

/// Applies `*` where at least one side is a math builtin: transform × point,
/// transform × transform, and quaternion × scalar.
///
/// `None` means this pair is not a math-builtin product, so the interpreter
/// keeps its own "cannot multiply" error.
pub(crate) fn mul(lhs: &Variant, rhs: &Variant) -> Option<Variant> {
    match (lhs, rhs) {
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
        _ => None,
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
        "is_zero_approx" => {
            Ok(Variant::Bool(v.x.abs() < CMP_EPSILON && v.y.abs() < CMP_EPSILON))
        }
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
            Ok(Variant::Float(
                v.cross(o).length().atan2(v.dot(o)) as f64
            ))
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
            let (dx, dy, dz) = (
                (o.x - v.x) as f64,
                (o.y - v.y) as f64,
                (o.z - v.z) as f64,
            );
            Ok(Variant::Float((dx * dx + dy * dy + dz * dz).sqrt()))
        }
        "distance_squared_to" => {
            let o = arg_vec3i(args, 0, &ctx)?;
            let (dx, dy, dz) = (
                (o.x - v.x) as i64,
                (o.y - v.y) as i64,
                (o.z - v.z) as i64,
            );
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
}
