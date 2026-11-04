//! pat-0q2kx.69 (print half): `Variant`'s `Display` matches Godot's
//! `operator String`.
//!
//! Godot defines exact print output per type (`core/variant/variant.cpp`):
//! containers print their ELEMENTS, matrix types label their columns, and a
//! Rect2/AABB uses `[P: …, S: …]`. Patina printed debug placeholders —
//! `[Array; len=2]`, `<Transform2D>`, `{Dict; len=1}` — so every `print()` of a
//! container or transform diverged from Godot, which is exactly the output a
//! developer compares when porting a project.
//!
//! These pin the shapes that were wrong. Vector2/Vector3 were already correct
//! and are included so a future "simplification" cannot quietly reformat them.

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use gdvariant::{Dictionary, Variant};

#[test]
fn containers_print_their_elements() {
    let a = Variant::Array(vec![Variant::Int(1), Variant::Int(2)].into());
    assert_eq!(a.to_string(), "[1, 2]", "an Array prints its elements");

    // A string INSIDE a container keeps its quotes, while a bare string prints
    // unquoted — Godot's `print("hi")` is `hi` but `print(["hi"])` is `["hi"]`.
    assert_eq!(Variant::String("hi".into()).to_string(), "hi");
    let quoted = Variant::Array(vec![Variant::String("hi".into())].into());
    assert_eq!(quoted.to_string(), "[\"hi\"]");

    let mut d = Dictionary::new();
    d.insert("a".to_string(), Variant::Vector2(Vector2::new(1.0, 2.0)));
    assert_eq!(
        Variant::Dictionary(d.into()).to_string(),
        "{ \"a\": (1.0, 2.0) }",
        "a Dictionary pads its braces and quotes string keys"
    );

    // The bead's headline example: nested container inside an array.
    let mut inner = Dictionary::new();
    inner.insert("a".to_string(), Variant::Vector2(Vector2::new(1.0, 2.0)));
    let nested = Variant::Array(vec![Variant::Int(1), Variant::Dictionary(inner.into())].into());
    assert_eq!(nested.to_string(), "[1, { \"a\": (1.0, 2.0) }]");

    // Packed arrays print elements too, not a length placeholder.
    assert_eq!(
        Variant::PackedInt32Array(vec![1, 2, 3]).to_string(),
        "[1, 2, 3]"
    );
    assert_eq!(
        Variant::PackedVector2Array(vec![Vector2::new(1.0, 2.0)]).to_string(),
        "[(1.0, 2.0)]"
    );
}

// ⚠️ Float components print WITH `.0`: Godot's `Vector2/3::operator String`
// use `String::num_real(x, true)` (core/math/vector2.cpp:210), whose `true`
// keeps a trailing `.0` on whole numbers. This file asserted `(1, 2)` — the
// Godot 3 form. Exceptions, also upstream's: Plane's `D` uses
// `num_real(d, false)` and Color uses `String::num(r, 4)` (no `.0`).
#[test]
fn matrix_types_label_their_columns() {
    assert_eq!(
        Variant::Transform2D(Transform2D::IDENTITY).to_string(),
        "[X: (1.0, 0.0), Y: (0.0, 1.0), O: (0.0, 0.0)]"
    );
    assert_eq!(
        Variant::Basis(Basis::IDENTITY).to_string(),
        "[X: (1.0, 0.0, 0.0), Y: (0.0, 1.0, 0.0), Z: (0.0, 0.0, 1.0)]"
    );
    assert_eq!(
        Variant::Transform3D(Transform3D::IDENTITY).to_string(),
        "[X: (1.0, 0.0, 0.0), Y: (0.0, 1.0, 0.0), Z: (0.0, 0.0, 1.0), O: (0.0, 0.0, 0.0)]"
    );
}

#[test]
fn rect_and_shape_types_use_p_s_labels() {
    let r = Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
    assert_eq!(Variant::Rect2(r).to_string(), "[P: (1.0, 2.0), S: (3.0, 4.0)]");

    let aabb = Aabb::new(Vector3::new(1.0, 2.0, 3.0), Vector3::new(4.0, 5.0, 6.0));
    assert_eq!(
        Variant::Aabb(aabb).to_string(),
        "[P: (1.0, 2.0, 3.0), S: (4.0, 5.0, 6.0)]"
    );

    let plane = Plane {
        normal: Vector3::new(0.0, 1.0, 0.0),
        d: 5.0,
    };
    assert_eq!(Variant::Plane(plane).to_string(), "[N: (0.0, 1.0, 0.0), D: 5]");
}

#[test]
fn scalar_and_vector_shapes_are_bare_tuples() {
    // Godot prints these WITHOUT a type-name prefix — `(1, 2)`, not
    // `Vector2(1, 2)`; the constructor text belongs to `var_to_str`, not print.
    assert_eq!(
        Variant::Vector2(Vector2::new(1.0, 2.0)).to_string(),
        "(1.0, 2.0)"
    );
    assert_eq!(
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)).to_string(),
        "(1.0, 2.0, 3.0)"
    );
    assert_eq!(
        Variant::Color(Color::new(1.0, 0.5, 0.0, 1.0)).to_string(),
        "(1, 0.5, 0, 1)"
    );
    assert_eq!(
        Variant::Quaternion(Quaternion::IDENTITY).to_string(),
        "(0, 0, 0, 1)"
    );
    assert_eq!(Variant::Nil.to_string(), "<null>");
}
