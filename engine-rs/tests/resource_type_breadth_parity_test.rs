//! pat-zb5k: Parity tests for broadened resource type support.
//!
//! Validates that the loader and saver correctly handle all Variant types
//! that can appear in Godot `.tres` files, including 3D math types
//! (Quaternion, Basis, Transform3D, AABB, Plane), StringName, and the
//! roundtrip fidelity of the full saver output.

use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdresource::saver::TresSaver;
use gdvariant::Variant;
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

fn parse_tres(content: &str) -> Arc<Resource> {
    let loader = TresLoader::new();
    loader.parse_str(content, "test://inline").unwrap()
}

fn roundtrip(resource: &Resource) -> Arc<Resource> {
    let saver = TresSaver::new();
    let serialized = saver.save_to_string(resource).unwrap();
    let loader = TresLoader::new();
    loader.parse_str(&serialized, "test://roundtrip").unwrap()
}

// ===========================================================================
// Quaternion parsing
// ===========================================================================

#[test]
fn parse_quaternion_identity() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
rotation = Quaternion(0, 0, 0, 1)
"#,
    );
    match res.get_property("rotation") {
        Some(Variant::Quaternion(q)) => {
            assert_eq!(q.x, 0.0);
            assert_eq!(q.y, 0.0);
            assert_eq!(q.z, 0.0);
            assert_eq!(q.w, 1.0);
        }
        other => panic!("expected Quaternion, got {other:?}"),
    }
}

#[test]
fn parse_quaternion_arbitrary() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
rot = Quaternion(0.5, -0.5, 0.5, 0.5)
"#,
    );
    match res.get_property("rot") {
        Some(Variant::Quaternion(q)) => {
            assert!((q.x - 0.5).abs() < 1e-6);
            assert!((q.y - (-0.5)).abs() < 1e-6);
            assert!((q.z - 0.5).abs() < 1e-6);
            assert!((q.w - 0.5).abs() < 1e-6);
        }
        other => panic!("expected Quaternion, got {other:?}"),
    }
}

#[test]
fn parse_quaternion_invalid_arg_count() {
    let loader = TresLoader::new();
    let result = loader.parse_str(
        r#"[gd_resource type="Resource" format=3]

[resource]
rot = Quaternion(1, 2, 3)
"#,
        "test://bad",
    );
    assert!(result.is_err());
}

// ===========================================================================
// Basis parsing
// ===========================================================================

#[test]
fn parse_basis_identity() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
basis = Basis(1, 0, 0, 0, 1, 0, 0, 0, 1)
"#,
    );
    match res.get_property("basis") {
        Some(Variant::Basis(b)) => {
            assert_eq!(b.x.x, 1.0);
            assert_eq!(b.x.y, 0.0);
            assert_eq!(b.x.z, 0.0);
            assert_eq!(b.y.x, 0.0);
            assert_eq!(b.y.y, 1.0);
            assert_eq!(b.y.z, 0.0);
            assert_eq!(b.z.x, 0.0);
            assert_eq!(b.z.y, 0.0);
            assert_eq!(b.z.z, 1.0);
        }
        other => panic!("expected Basis, got {other:?}"),
    }
}

#[test]
fn parse_basis_invalid_arg_count() {
    let loader = TresLoader::new();
    let result = loader.parse_str(
        r#"[gd_resource type="Resource" format=3]

[resource]
b = Basis(1, 0, 0, 0, 1, 0)
"#,
        "test://bad",
    );
    assert!(result.is_err());
}

// ===========================================================================
// Transform3D parsing
// ===========================================================================

#[test]
fn parse_transform3d_identity() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
xform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
"#,
    );
    match res.get_property("xform") {
        Some(Variant::Transform3D(t)) => {
            assert_eq!(t.basis.x.x, 1.0);
            assert_eq!(t.basis.y.y, 1.0);
            assert_eq!(t.basis.z.z, 1.0);
            assert_eq!(t.origin.x, 0.0);
            assert_eq!(t.origin.y, 0.0);
            assert_eq!(t.origin.z, 0.0);
        }
        other => panic!("expected Transform3D, got {other:?}"),
    }
}

#[test]
fn parse_transform3d_with_translation() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
xform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 20, 30)
"#,
    );
    match res.get_property("xform") {
        Some(Variant::Transform3D(t)) => {
            assert_eq!(t.origin.x, 10.0);
            assert_eq!(t.origin.y, 20.0);
            assert_eq!(t.origin.z, 30.0);
        }
        other => panic!("expected Transform3D, got {other:?}"),
    }
}

#[test]
fn parse_transform3d_invalid_arg_count() {
    let loader = TresLoader::new();
    let result = loader.parse_str(
        r#"[gd_resource type="Resource" format=3]

[resource]
xform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1)
"#,
        "test://bad",
    );
    assert!(result.is_err());
}

// ===========================================================================
// AABB parsing
// ===========================================================================

#[test]
fn parse_aabb() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
bounds = AABB(-1, -2, -3, 10, 20, 30)
"#,
    );
    match res.get_property("bounds") {
        Some(Variant::Aabb(a)) => {
            assert_eq!(a.position.x, -1.0);
            assert_eq!(a.position.y, -2.0);
            assert_eq!(a.position.z, -3.0);
            assert_eq!(a.size.x, 10.0);
            assert_eq!(a.size.y, 20.0);
            assert_eq!(a.size.z, 30.0);
        }
        other => panic!("expected Aabb, got {other:?}"),
    }
}

#[test]
fn parse_aabb_invalid_arg_count() {
    let loader = TresLoader::new();
    let result = loader.parse_str(
        r#"[gd_resource type="Resource" format=3]

[resource]
bounds = AABB(1, 2, 3)
"#,
        "test://bad",
    );
    assert!(result.is_err());
}

// ===========================================================================
// Plane parsing
// ===========================================================================

#[test]
fn parse_plane() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
ground = Plane(0, 1, 0, 5.5)
"#,
    );
    match res.get_property("ground") {
        Some(Variant::Plane(p)) => {
            assert_eq!(p.normal.x, 0.0);
            assert_eq!(p.normal.y, 1.0);
            assert_eq!(p.normal.z, 0.0);
            assert!((p.d - 5.5).abs() < 1e-6);
        }
        other => panic!("expected Plane, got {other:?}"),
    }
}

#[test]
fn parse_plane_invalid_arg_count() {
    let loader = TresLoader::new();
    let result = loader.parse_str(
        r#"[gd_resource type="Resource" format=3]

[resource]
p = Plane(0, 1, 0)
"#,
        "test://bad",
    );
    assert!(result.is_err());
}

// ===========================================================================
// StringName parsing
// ===========================================================================

#[test]
fn parse_stringname_quoted() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
action = StringName("jump")
"#,
    );
    match res.get_property("action") {
        Some(Variant::StringName(sn)) => {
            assert_eq!(sn.as_str(), "jump");
        }
        other => panic!("expected StringName, got {other:?}"),
    }
}

#[test]
fn parse_stringname_ampersand() {
    // Godot often writes StringName(&"name") in some contexts.
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
action = StringName(&"attack")
"#,
    );
    match res.get_property("action") {
        Some(Variant::StringName(sn)) => {
            assert_eq!(sn.as_str(), "attack");
        }
        other => panic!("expected StringName, got {other:?}"),
    }
}

#[test]
fn parse_stringname_invalid() {
    let loader = TresLoader::new();
    let result = loader.parse_str(
        r#"[gd_resource type="Resource" format=3]

[resource]
s = StringName(123)
"#,
        "test://bad",
    );
    assert!(result.is_err());
}

// ===========================================================================
// Saver roundtrip tests for new types
// ===========================================================================

#[test]
fn roundtrip_quaternion() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "rot",
        Variant::Quaternion(gdcore::math3d::Quaternion::new(0.1, 0.2, 0.3, 0.9)),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("rot") {
        Some(Variant::Quaternion(q)) => {
            assert!((q.x - 0.1).abs() < 1e-5);
            assert!((q.y - 0.2).abs() < 1e-5);
            assert!((q.z - 0.3).abs() < 1e-5);
            assert!((q.w - 0.9).abs() < 1e-5);
        }
        other => panic!("expected Quaternion, got {other:?}"),
    }
}

#[test]
fn roundtrip_basis() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "basis",
        Variant::Basis(gdcore::math3d::Basis {
            x: gdcore::math::Vector3::new(1.0, 0.0, 0.0),
            y: gdcore::math::Vector3::new(0.0, 1.0, 0.0),
            z: gdcore::math::Vector3::new(0.0, 0.0, 1.0),
        }),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("basis") {
        Some(Variant::Basis(b)) => {
            assert_eq!(b.x.x, 1.0);
            assert_eq!(b.y.y, 1.0);
            assert_eq!(b.z.z, 1.0);
        }
        other => panic!("expected Basis, got {other:?}"),
    }
}

#[test]
fn roundtrip_transform3d() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "xform",
        Variant::Transform3D(gdcore::math3d::Transform3D {
            basis: gdcore::math3d::Basis {
                x: gdcore::math::Vector3::new(1.0, 0.0, 0.0),
                y: gdcore::math::Vector3::new(0.0, 1.0, 0.0),
                z: gdcore::math::Vector3::new(0.0, 0.0, 1.0),
            },
            origin: gdcore::math::Vector3::new(5.0, 10.0, 15.0),
        }),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("xform") {
        Some(Variant::Transform3D(t)) => {
            assert_eq!(t.origin.x, 5.0);
            assert_eq!(t.origin.y, 10.0);
            assert_eq!(t.origin.z, 15.0);
        }
        other => panic!("expected Transform3D, got {other:?}"),
    }
}

#[test]
fn roundtrip_aabb() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "bounds",
        Variant::Aabb(gdcore::math3d::Aabb::new(
            gdcore::math::Vector3::new(-1.0, -2.0, -3.0),
            gdcore::math::Vector3::new(10.0, 20.0, 30.0),
        )),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("bounds") {
        Some(Variant::Aabb(a)) => {
            assert_eq!(a.position.x, -1.0);
            assert_eq!(a.size.z, 30.0);
        }
        other => panic!("expected Aabb, got {other:?}"),
    }
}

#[test]
fn roundtrip_plane() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "ground",
        Variant::Plane(gdcore::math3d::Plane::new(
            gdcore::math::Vector3::new(0.0, 1.0, 0.0),
            5.0,
        )),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("ground") {
        Some(Variant::Plane(p)) => {
            assert_eq!(p.normal.y, 1.0);
            assert_eq!(p.d, 5.0);
        }
        other => panic!("expected Plane, got {other:?}"),
    }
}

#[test]
fn roundtrip_stringname() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "action",
        Variant::StringName(gdcore::string_name::StringName::new("jump")),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("action") {
        Some(Variant::StringName(sn)) => {
            assert_eq!(sn.as_str(), "jump");
        }
        other => panic!("expected StringName, got {other:?}"),
    }
}

#[test]
fn roundtrip_rect2() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "rect",
        Variant::Rect2(gdcore::math::Rect2::new(
            gdcore::math::Vector2::new(10.0, 20.0),
            gdcore::math::Vector2::new(100.0, 200.0),
        )),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("rect") {
        Some(Variant::Rect2(rect)) => {
            assert_eq!(rect.position.x, 10.0);
            assert_eq!(rect.size.y, 200.0);
        }
        other => panic!("expected Rect2, got {other:?}"),
    }
}

#[test]
fn roundtrip_transform2d() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "xform2d",
        Variant::Transform2D(gdcore::math::Transform2D {
            x: gdcore::math::Vector2::new(1.0, 0.0),
            y: gdcore::math::Vector2::new(0.0, 1.0),
            origin: gdcore::math::Vector2::new(50.0, 100.0),
        }),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("xform2d") {
        Some(Variant::Transform2D(t)) => {
            assert_eq!(t.origin.x, 50.0);
            assert_eq!(t.origin.y, 100.0);
        }
        other => panic!("expected Transform2D, got {other:?}"),
    }
}

#[test]
fn roundtrip_node_path() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "target",
        Variant::NodePath(gdcore::node_path::NodePath::new("Player/Sprite2D")),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("target") {
        Some(Variant::NodePath(np)) => {
            assert_eq!(format!("{np}"), "Player/Sprite2D");
        }
        other => panic!("expected NodePath, got {other:?}"),
    }
}

#[test]
fn roundtrip_array() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "items",
        Variant::Array(vec![
            Variant::Int(1),
            Variant::String("hello".into()),
            Variant::Bool(true),
        ]),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("items") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Variant::Int(1));
            assert_eq!(arr[1], Variant::String("hello".into()));
            assert_eq!(arr[2], Variant::Bool(true));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn roundtrip_dictionary() {
    let mut r = Resource::new("Resource");
    let mut dict = std::collections::HashMap::new();
    dict.insert("key1".to_string(), Variant::Int(42));
    dict.insert("key2".to_string(), Variant::String("val".into()));
    r.set_property("meta", Variant::Dictionary(dict));

    let reloaded = roundtrip(&r);
    match reloaded.get_property("meta") {
        Some(Variant::Dictionary(d)) => {
            assert_eq!(d.get("key1"), Some(&Variant::Int(42)));
            assert_eq!(d.get("key2"), Some(&Variant::String("val".into())));
        }
        other => panic!("expected Dictionary, got {other:?}"),
    }
}

// ===========================================================================
// Mixed-type .tres resource parsing
// ===========================================================================

#[test]
fn parse_3d_resource_with_multiple_types() {
    let res = parse_tres(
        r#"[gd_resource type="Environment3D" format=3]

[resource]
rotation = Quaternion(0, 0.707, 0, 0.707)
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 10, 15)
bounds = AABB(0, 0, 0, 100, 50, 200)
ground_plane = Plane(0, 1, 0, 0)
basis = Basis(1, 0, 0, 0, 1, 0, 0, 0, 1)
action_name = StringName("interact")
"#,
    );

    assert_eq!(res.class_name, "Environment3D");
    assert!(matches!(res.get_property("rotation"), Some(Variant::Quaternion(_))));
    assert!(matches!(res.get_property("transform"), Some(Variant::Transform3D(_))));
    assert!(matches!(res.get_property("bounds"), Some(Variant::Aabb(_))));
    assert!(matches!(res.get_property("ground_plane"), Some(Variant::Plane(_))));
    assert!(matches!(res.get_property("basis"), Some(Variant::Basis(_))));
    assert!(matches!(res.get_property("action_name"), Some(Variant::StringName(_))));
}

#[test]
fn parse_3d_resource_in_subresource() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[sub_resource type="CollisionShape3D" id="shape_1"]
half_extents = AABB(0, 0, 0, 5, 5, 5)

[resource]
shape = SubResource("shape_1")
"#,
    );

    let sub = res.subresources.get("shape_1").expect("should have sub-resource");
    assert_eq!(sub.class_name, "CollisionShape3D");
    match sub.get_property("half_extents") {
        Some(Variant::Aabb(a)) => {
            assert_eq!(a.size.x, 5.0);
        }
        other => panic!("expected Aabb in sub-resource, got {other:?}"),
    }
}

// ===========================================================================
// Saver output format verification
// ===========================================================================

#[test]
fn saver_formats_quaternion_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "q",
        Variant::Quaternion(gdcore::math3d::Quaternion::new(0.0, 0.0, 0.0, 1.0)),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(output.contains("q = Quaternion(0, 0, 0, 1)"), "got: {output}");
}

#[test]
fn saver_formats_transform3d_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "t",
        Variant::Transform3D(gdcore::math3d::Transform3D {
            basis: gdcore::math3d::Basis {
                x: gdcore::math::Vector3::new(1.0, 0.0, 0.0),
                y: gdcore::math::Vector3::new(0.0, 1.0, 0.0),
                z: gdcore::math::Vector3::new(0.0, 0.0, 1.0),
            },
            origin: gdcore::math::Vector3::new(0.0, 0.0, 0.0),
        }),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(
        output.contains("t = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)"),
        "got: {output}"
    );
}

#[test]
fn saver_formats_aabb_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "a",
        Variant::Aabb(gdcore::math3d::Aabb::new(
            gdcore::math::Vector3::new(1.0, 2.0, 3.0),
            gdcore::math::Vector3::new(4.0, 5.0, 6.0),
        )),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(output.contains("a = AABB(1, 2, 3, 4, 5, 6)"), "got: {output}");
}

#[test]
fn saver_formats_plane_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "p",
        Variant::Plane(gdcore::math3d::Plane::new(
            gdcore::math::Vector3::new(0.0, 1.0, 0.0),
            0.0,
        )),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(output.contains("p = Plane(0, 1, 0, 0)"), "got: {output}");
}

#[test]
fn saver_formats_array_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "arr",
        Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)]),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(output.contains("arr = [1, 2, 3]"), "got: {output}");
}

#[test]
fn saver_formats_rect2_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "r",
        Variant::Rect2(gdcore::math::Rect2::new(
            gdcore::math::Vector2::new(0.0, 0.0),
            gdcore::math::Vector2::new(100.0, 200.0),
        )),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(output.contains("r = Rect2(0, 0, 100, 200)"), "got: {output}");
}

#[test]
fn saver_formats_transform2d_correctly() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "t2d",
        Variant::Transform2D(gdcore::math::Transform2D {
            x: gdcore::math::Vector2::new(1.0, 0.0),
            y: gdcore::math::Vector2::new(0.0, 1.0),
            origin: gdcore::math::Vector2::new(0.0, 0.0),
        }),
    );
    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();
    assert!(
        output.contains("t2d = Transform2D(1, 0, 0, 1, 0, 0)"),
        "got: {output}"
    );
}

// ===========================================================================
// pat-rju: Packed array parsing & roundtrip
// ===========================================================================

#[test]
fn parse_packed_int32_array() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
data = PackedInt32Array(1, 2, 3, 4, 5)
"#,
    );
    match res.get_property("data") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 5);
            assert_eq!(arr[0], Variant::Int(1));
            assert_eq!(arr[4], Variant::Int(5));
        }
        other => panic!("expected Array from PackedInt32Array, got {other:?}"),
    }
}

#[test]
fn parse_packed_float32_array() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
weights = PackedFloat32Array(0.1, 0.5, 1.0, 2.5)
"#,
    );
    match res.get_property("weights") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 4);
            match &arr[0] {
                Variant::Float(f) => assert!((*f - 0.1).abs() < 1e-5),
                other => panic!("expected Float, got {other:?}"),
            }
            match &arr[2] {
                Variant::Float(f) => assert!((*f - 1.0).abs() < 1e-5),
                other => panic!("expected Float, got {other:?}"),
            }
        }
        other => panic!("expected Array from PackedFloat32Array, got {other:?}"),
    }
}

#[test]
fn parse_packed_string_array() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
tags = PackedStringArray("alpha", "beta", "gamma")
"#,
    );
    match res.get_property("tags") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Variant::String("alpha".into()));
            assert_eq!(arr[2], Variant::String("gamma".into()));
        }
        other => panic!("expected Array from PackedStringArray, got {other:?}"),
    }
}

#[test]
fn parse_packed_byte_array_empty() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
raw = PackedByteArray()
"#,
    );
    match res.get_property("raw") {
        Some(Variant::Array(arr)) => {
            assert!(arr.is_empty(), "empty PackedByteArray should produce empty array");
        }
        other => panic!("expected empty Array, got {other:?}"),
    }
}

#[test]
fn parse_packed_vector2_array() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
path_points = PackedVector2Array(0, 0, 10, 20, 30, 40)
"#,
    );
    match res.get_property("path_points") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 3, "3 pairs of floats = 3 Vector2 values");
            match &arr[0] {
                Variant::Vector2(v) => {
                    assert_eq!(v.x, 0.0);
                    assert_eq!(v.y, 0.0);
                }
                other => panic!("expected Vector2, got {other:?}"),
            }
            match &arr[2] {
                Variant::Vector2(v) => {
                    assert_eq!(v.x, 30.0);
                    assert_eq!(v.y, 40.0);
                }
                other => panic!("expected Vector2, got {other:?}"),
            }
        }
        other => panic!("expected Array from PackedVector2Array, got {other:?}"),
    }
}

#[test]
fn parse_packed_vector3_array() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
vertices = PackedVector3Array(1, 2, 3, 4, 5, 6)
"#,
    );
    match res.get_property("vertices") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 2, "6 floats / 3 = 2 Vector3 values");
            match &arr[1] {
                Variant::Vector3(v) => {
                    assert_eq!(v.x, 4.0);
                    assert_eq!(v.y, 5.0);
                    assert_eq!(v.z, 6.0);
                }
                other => panic!("expected Vector3, got {other:?}"),
            }
        }
        other => panic!("expected Array from PackedVector3Array, got {other:?}"),
    }
}

#[test]
fn parse_packed_color_array() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
colors = PackedColorArray(1, 0, 0, 1, 0, 1, 0, 0.5)
"#,
    );
    match res.get_property("colors") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 2, "8 floats / 4 = 2 Color values");
            match &arr[0] {
                Variant::Color(c) => {
                    assert_eq!(c.r, 1.0);
                    assert_eq!(c.g, 0.0);
                    assert_eq!(c.b, 0.0);
                    assert_eq!(c.a, 1.0);
                }
                other => panic!("expected Color, got {other:?}"),
            }
        }
        other => panic!("expected Array from PackedColorArray, got {other:?}"),
    }
}

// ===========================================================================
// pat-rju: Dictionary parsing & roundtrip
// ===========================================================================

#[test]
fn parse_dictionary_with_mixed_values() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
metadata = {"name": "Player", "health": 100, "speed": 5.5, "alive": true}
"#,
    );
    match res.get_property("metadata") {
        Some(Variant::Dictionary(dict)) => {
            assert_eq!(dict.get("name"), Some(&Variant::String("Player".into())));
            assert_eq!(dict.get("health"), Some(&Variant::Int(100)));
            assert_eq!(dict.get("alive"), Some(&Variant::Bool(true)));
            match dict.get("speed") {
                Some(Variant::Float(f)) => assert!((*f - 5.5).abs() < 1e-6),
                other => panic!("expected Float for speed, got {other:?}"),
            }
        }
        other => panic!("expected Dictionary, got {other:?}"),
    }
}

#[test]
fn roundtrip_packed_int_array_via_array() {
    let mut r = Resource::new("Resource");
    r.set_property(
        "ids",
        Variant::Array(vec![Variant::Int(10), Variant::Int(20), Variant::Int(30)]),
    );
    let reloaded = roundtrip(&r);
    match reloaded.get_property("ids") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Variant::Int(10));
            assert_eq!(arr[2], Variant::Int(30));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn roundtrip_dictionary_mixed() {
    let mut r = Resource::new("Resource");
    let mut dict = std::collections::HashMap::new();
    dict.insert("score".to_string(), Variant::Int(9001));
    dict.insert("tag".to_string(), Variant::String("elite".into()));
    dict.insert("active".to_string(), Variant::Bool(true));
    r.set_property("stats", Variant::Dictionary(dict));

    let reloaded = roundtrip(&r);
    match reloaded.get_property("stats") {
        Some(Variant::Dictionary(d)) => {
            assert_eq!(d.get("score"), Some(&Variant::Int(9001)));
            assert_eq!(d.get("tag"), Some(&Variant::String("elite".into())));
            assert_eq!(d.get("active"), Some(&Variant::Bool(true)));
        }
        other => panic!("expected Dictionary, got {other:?}"),
    }
}

// ===========================================================================
// pat-rju: Real Godot resource fixture loading
// ===========================================================================

#[test]
fn load_style_box_flat_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_style_box.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "StyleBoxFlat");
    // bg_color = Color(0.2, 0.3, 0.4, 1)
    match res.get_property("bg_color") {
        Some(Variant::Color(c)) => {
            assert!((c.r - 0.2).abs() < 1e-5);
            assert!((c.g - 0.3).abs() < 1e-5);
            assert!((c.b - 0.4).abs() < 1e-5);
            assert_eq!(c.a, 1.0);
        }
        other => panic!("expected Color for bg_color, got {other:?}"),
    }
    // border_width_left = 2
    assert_eq!(res.get_property("border_width_left"), Some(&Variant::Int(2)));
    // corner_radius_top_left = 4
    assert_eq!(
        res.get_property("corner_radius_top_left"),
        Some(&Variant::Int(4))
    );
}

#[test]
fn load_environment_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_environment.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Environment");
    assert_eq!(
        res.get_property("background_mode"),
        Some(&Variant::Int(0))
    );
}

#[test]
fn load_rectangle_shape_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_rect_shape.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "RectangleShape2D");
    match res.get_property("size") {
        Some(Variant::Vector2(v)) => {
            assert_eq!(v.x, 20.0);
            assert_eq!(v.y, 20.0);
        }
        other => panic!("expected Vector2 for size, got {other:?}"),
    }
}

#[test]
fn load_animation_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_animation.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Animation");
    assert_eq!(
        res.get_property("resource_name"),
        Some(&Variant::String("test_walk".into()))
    );
    match res.get_property("length") {
        Some(Variant::Float(f)) => assert!((*f - 1.0).abs() < 1e-6),
        other => panic!("expected Float for length, got {other:?}"),
    }
    assert_eq!(res.get_property("loop_mode"), Some(&Variant::Int(1)));
    match res.get_property("step") {
        Some(Variant::Float(f)) => assert!((*f - 0.05).abs() < 1e-6),
        other => panic!("expected Float for step, got {other:?}"),
    }
}

#[test]
fn load_theme_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_theme.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Theme");
    assert_eq!(
        res.get_property("default_font_size"),
        Some(&Variant::Int(16))
    );
}

// ===========================================================================
// pat-rju: Resources with sub-resources and ext-resources
// ===========================================================================

#[test]
fn load_theme_with_subresources() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/resources/theme.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Theme");
    // Should have 2 sub-resources: panel_style and button_style
    assert!(
        res.subresources.contains_key("panel_style"),
        "should have panel_style sub-resource"
    );
    assert!(
        res.subresources.contains_key("button_style"),
        "should have button_style sub-resource"
    );
    let panel = res.subresources.get("panel_style").unwrap();
    assert_eq!(panel.class_name, "StyleBoxFlat");
    match panel.get_property("bg_color") {
        Some(Variant::Color(_)) => {} // Color property parsed successfully
        other => panic!("expected Color for panel bg_color, got {other:?}"),
    }
}

#[test]
fn load_resource_with_ext_refs() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/resources/with_ext_refs.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "PackedScene");
    // Should have 3 ext_resources
    assert_eq!(
        res.ext_resources.len(),
        3,
        "should have 3 ext_resources, got {}",
        res.ext_resources.len()
    );
    // First ext_resource should be Texture2D
    let tex = res.ext_resources.get("1").expect("should have ext_resource id=1");
    assert_eq!(tex.resource_type, "Texture2D");
    assert_eq!(tex.path, "res://icon.png");
    // Should have inline sub-resource
    assert!(
        res.subresources.contains_key("inline_style"),
        "should have inline_style sub-resource"
    );
}

// ===========================================================================
// pat-rju: Diverse resource class names (engine-agnostic parsing)
// ===========================================================================

#[test]
fn parse_arbitrary_resource_class_names() {
    // The loader must accept ANY class name — it's class-agnostic.
    for class in &[
        "StyleBoxFlat",
        "GradientTexture2D",
        "AudioStreamMP3",
        "ShaderMaterial",
        "CurveTexture",
        "SpriteFrames",
        "TileSet",
        "PhysicsMaterial",
    ] {
        let tres = format!(
            r#"[gd_resource type="{class}" format=3]

[resource]
test_prop = 42
"#
        );
        let res = parse_tres(&tres);
        assert_eq!(res.class_name, *class, "class name should be preserved");
        assert_eq!(
            res.get_property("test_prop"),
            Some(&Variant::Int(42)),
            "property should parse for class {class}"
        );
    }
}

#[test]
fn roundtrip_preserves_class_name() {
    for class in &[
        "StyleBoxFlat",
        "AnimationLibrary",
        "Environment",
        "WorldBoundaryShape2D",
    ] {
        let mut r = Resource::new(*class);
        r.set_property("value", Variant::Int(1));
        let reloaded = roundtrip(&r);
        assert_eq!(
            reloaded.class_name, *class,
            "roundtrip must preserve class name {class}"
        );
    }
}

// ===========================================================================
// pat-rju: Nested array and dictionary in resource
// ===========================================================================

#[test]
fn parse_nested_array_in_resource() {
    let res = parse_tres(
        r#"[gd_resource type="Resource" format=3]

[resource]
items = [1, "two", Vector2(3, 4), true, null]
"#,
    );
    match res.get_property("items") {
        Some(Variant::Array(arr)) => {
            assert_eq!(arr.len(), 5);
            assert_eq!(arr[0], Variant::Int(1));
            assert_eq!(arr[1], Variant::String("two".into()));
            assert!(matches!(&arr[2], Variant::Vector2(v) if v.x == 3.0 && v.y == 4.0));
            assert_eq!(arr[3], Variant::Bool(true));
            assert_eq!(arr[4], Variant::Nil);
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn roundtrip_resource_with_multiple_variant_types() {
    // A resource with many different variant types — comprehensive roundtrip.
    let mut r = Resource::new("CustomResource");
    r.set_property("flag", Variant::Bool(false));
    r.set_property("count", Variant::Int(42));
    r.set_property("ratio", Variant::Float(3.14));
    r.set_property("label", Variant::String("hello".into()));
    r.set_property(
        "pos",
        Variant::Vector2(gdcore::math::Vector2::new(1.0, 2.0)),
    );
    r.set_property(
        "pos3d",
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
    );
    r.set_property(
        "tint",
        Variant::Color(gdcore::math::Color::new(0.5, 0.5, 0.5, 1.0)),
    );
    r.set_property(
        "target",
        Variant::NodePath(gdcore::node_path::NodePath::new("../Sprite2D")),
    );
    r.set_property(
        "action",
        Variant::StringName(gdcore::string_name::StringName::new("fire")),
    );

    let reloaded = roundtrip(&r);
    assert_eq!(reloaded.class_name, "CustomResource");
    assert_eq!(reloaded.get_property("flag"), Some(&Variant::Bool(false)));
    assert_eq!(reloaded.get_property("count"), Some(&Variant::Int(42)));
    assert_eq!(
        reloaded.get_property("label"),
        Some(&Variant::String("hello".into()))
    );
    assert!(matches!(
        reloaded.get_property("pos"),
        Some(Variant::Vector2(v)) if v.x == 1.0 && v.y == 2.0
    ));
    assert!(matches!(
        reloaded.get_property("pos3d"),
        Some(Variant::Vector3(v)) if v.x == 1.0 && v.y == 2.0 && v.z == 3.0
    ));
    assert!(matches!(
        reloaded.get_property("tint"),
        Some(Variant::Color(c)) if (c.r - 0.5).abs() < 1e-5
    ));
    match reloaded.get_property("ratio") {
        Some(Variant::Float(f)) => assert!((*f - 3.14).abs() < 1e-4),
        other => panic!("expected Float for ratio, got {other:?}"),
    }
}
