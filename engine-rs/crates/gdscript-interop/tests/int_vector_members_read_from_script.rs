//! pat-s3rrv.59 — `Vector2i` / `Vector3i` component access from GDScript.
//!
//! Godot ground truth: `doc/classes/Vector2i.xml` and `Vector3i.xml` — `x`/`y`
//! (and `z`) are **int** members.
//!
//! # 🔑 What was broken, and why nothing caught it
//!
//! Member access on a Variant is dispatched in two places, and **neither
//! handled the integer vectors**:
//!
//! * the interpreter's inline match covered only `Vector2`, `Vector3`, `Color`
//!   and `Dictionary`, falling through for everything else;
//! * `variant_math::member_get` covered `Rect2`, `Rect2i`, `Vector4`,
//!   `Vector4i`, `Projection`, `Color`, `Transform2D`, `Transform3D`, `Basis`,
//!   `Quaternion`, `Aabb` and `Plane`.
//!
//! `Vector2i` and `Vector3i` appear in neither list, so `v.x` on an int vector
//! raised `cannot access member 'x' on Vector2i` — for every script, always.
//!
//! ⚠️ It stayed invisible because the existing tests read those components in
//! **Rust**, off the returned `Variant`, rather than from a script.
//! `image_alpha_ops_and_size.rs:70` even describes the idiom
//! (`img.get_size().x`) in a comment while asserting `(v.x, v.y)` on the Rust
//! value — which passes whether or not a script could ever do it.
//!
//! This is much wider than the pat-s3rrv.59 builtins that surfaced it:
//! `Vector2i` is what `Rect2i.size`, `Image.get_size()`, `DisplayServer` screen
//! metrics and every tile coordinate return.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// 🔑 **THE REGRESSION.** `Vector2i(...).x` from a script.
///
/// ⚠️ Before the fix this did not return a wrong value — it **raised**, so the
/// script never reached its `return`.
#[test]
fn a_vector2i_components_are_readable() {
    assert_eq!(run_val("return Vector2i(3, 4).x\n"), Variant::Int(3));
    assert_eq!(run_val("return Vector2i(3, 4).y\n"), Variant::Int(4));
}

/// The same for `Vector3i`, which had the identical gap.
#[test]
fn b_vector3i_components_are_readable() {
    assert_eq!(run_val("return Vector3i(5, 6, 7).x\n"), Variant::Int(5));
    assert_eq!(run_val("return Vector3i(5, 6, 7).y\n"), Variant::Int(6));
    assert_eq!(run_val("return Vector3i(5, 6, 7).z\n"), Variant::Int(7));
}

/// 🔑 The exact chain that failed in the pat-s3rrv.59 acceptance test.
///
/// ⚠️ `Rect2i.size` always worked — it is `member_get`'s own arm. What failed
/// was reading `.x` off the `Vector2i` it hands back, one link further along.
/// The acceptance test's diagnostic legend blamed "Rect2i has no `size` member",
/// which is why this asserts the intermediate value too: `size` resolving is
/// what proves the failure was in the *next* step.
#[test]
fn c_rect2i_size_x_is_the_chain_that_failed() {
    assert_eq!(
        run_val("return Rect2i(0, 0, 16, 9).size\n"),
        Variant::Vector2i(gdcore::math::Vector2i::new(16, 9)),
        "Rect2i.size resolved even before the fix"
    );
    assert_eq!(
        run_val("return Rect2i(0, 0, 16, 9).size.x\n"),
        Variant::Int(16),
        "…and this is the link that raised"
    );
    assert_eq!(
        run_val("return Rect2i(0, 0, 16, 9).position.y\n"),
        Variant::Int(0)
    );
}

/// 🔑 Components come back as `Int`, not `Float`.
///
/// ⚠️ Handing back a Float would make `v.x == 3` **false** — an int silently
/// widened to a float is the pat-nzgdu.78 defect, and it is the failure mode a
/// value-only assertion cannot see, because `Float(3.0)` prints as `3`. So the
/// comparison is asserted from inside the script as well as on the Variant kind.
#[test]
fn d_components_are_int_not_float() {
    assert_eq!(run_val("return Vector2i(3, 4).x\n"), Variant::Int(3));
    assert_ne!(run_val("return Vector2i(3, 4).x\n"), Variant::Float(3.0));

    assert_eq!(
        run_val("return Vector2i(3, 4).x == 3\n"),
        Variant::Bool(true),
        "equality against an int literal must hold"
    );
    assert_eq!(
        run_val("return typeof(Vector2i(3, 4).x) == typeof(1)\n"),
        Variant::Bool(true),
        "typeof must agree with a plain int"
    );
}

/// Positive control: the FLOAT vectors were never broken.
///
/// ⚠️ Without this, tests a–d would also pass against a build that had simply
/// made every vector component an Int — including `Vector2.x`, which must stay
/// a Float.
#[test]
fn e_float_vectors_still_return_floats() {
    assert_eq!(run_val("return Vector2(3.5, 4.5).x\n"), Variant::Float(3.5));
    assert_eq!(
        run_val("return Vector3(1.5, 2.5, 3.5).z\n"),
        Variant::Float(3.5)
    );
    assert_eq!(
        run_val("return Vector4(1.0, 2.0, 3.0, 4.5).w\n"),
        Variant::Float(4.5),
        "Vector4 was already covered by member_get"
    );
    assert_eq!(
        run_val("return Vector4i(1, 2, 3, 4).w\n"),
        Variant::Int(4),
        "…and Vector4i already returned Int, which is the convention copied here"
    );
}
