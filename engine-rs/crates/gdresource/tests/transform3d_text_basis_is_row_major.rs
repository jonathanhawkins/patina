//! pat-2ifdx — a text `Transform3D(...)` / `Basis(...)` lists its nine basis
//! numbers ROW by row, as Godot writes and reads them.
//!
//! Godot oracle (4.6.1): `str_to_var("Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9,
//! 0, 0, 0)")` has `basis.x == (1, 4, 7)`, `basis.y == (2, 5, 8)`, `basis.z ==
//! (3, 6, 9)`. The loader took each triple as a COLUMN, transposing every
//! authored rotation in every `.tscn`/`.tres` — for a rotation, its inverse.
//! The official 3D platformer's player (`Transform3D(-4.37114e-08, 0, -1, 0,
//! 1, 0, 1, 0, -4.37114e-08, ...)`, Godot `basis.z == (-1, 0, 0)`) faced the
//! other way, its camera sat in front of it, and "forward" walked it backwards.

use gdcore::math::Vector3;
use gdresource::loader::parse_variant_value;
use gdresource::{Resource, TresLoader, TresSaver};
use gdvariant::Variant;

#[test]
fn transform3d_basis_numbers_are_rows() {
    let Variant::Transform3D(t) =
        parse_variant_value("Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)").unwrap()
    else {
        panic!("not a Transform3D");
    };
    assert_eq!(t.basis.x, Vector3::new(1.0, 4.0, 7.0));
    assert_eq!(t.basis.y, Vector3::new(2.0, 5.0, 8.0));
    assert_eq!(t.basis.z, Vector3::new(3.0, 6.0, 9.0));
    assert_eq!(t.origin, Vector3::new(10.0, 11.0, 12.0));
}

#[test]
fn basis_numbers_are_rows() {
    let Variant::Basis(b) = parse_variant_value("Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)").unwrap() else {
        panic!("not a Basis");
    };
    assert_eq!(b.x, Vector3::new(1.0, 4.0, 7.0));
    assert_eq!(b.y, Vector3::new(2.0, 5.0, 8.0));
    assert_eq!(b.z, Vector3::new(3.0, 6.0, 9.0));
}

/// The platformer's own player transform: Godot reports `basis.z == (-1, 0,
/// 0)`, so the camera parented at local +Z sits at world -X, behind it.
#[test]
fn the_platformer_players_rotation_faces_like_godot() {
    let Variant::Transform3D(t) = parse_variant_value(
        "Transform3D(-4.37114e-08, 0, -1, 0, 1, 0, 1, 0, -4.37114e-08, -9.49833, -3.84189, 3.9334)",
    )
    .unwrap() else {
        panic!("not a Transform3D");
    };
    assert!(
        (t.basis.z - Vector3::new(-1.0, 0.0, 0.0)).length() < 1e-6,
        "{:?}",
        t.basis
    );
    assert!(
        (t.basis.x - Vector3::new(0.0, 0.0, 1.0)).length() < 1e-6,
        "{:?}",
        t.basis
    );
}

/// Saving writes rows too, so a load → save → load round trip is lossless and
/// the saved text is what Godot itself would write.
#[test]
fn saved_transform3d_is_row_major_and_round_trips() {
    let mut res = Resource::new("Resource");
    res.set_property(
        "t",
        parse_variant_value("Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)").unwrap(),
    );
    res.set_property(
        "b",
        parse_variant_value("Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)").unwrap(),
    );
    let text = TresSaver::new().save_to_string(&res).unwrap();
    assert!(
        text.contains("t = Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)"),
        "{text}"
    );
    assert!(
        text.contains("b = Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)"),
        "{text}"
    );
    let back = TresLoader::new().parse_str(&text, "res://t.tres").unwrap();
    assert_eq!(back.get_property("t"), res.get_property("t"));
    assert_eq!(back.get_property("b"), res.get_property("b"));
}
