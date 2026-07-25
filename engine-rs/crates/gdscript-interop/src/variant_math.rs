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

use gdcore::math::{Rect2, Transform2D, Vector2, Vector3};
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
        _ => None,
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
