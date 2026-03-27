//! Variant serialization and deserialization support.
//!
//! Provides conversion between `Variant` and a JSON-compatible
//! representation. This is used for fixture I/O, debugging, and
//! the resource system. The wire format is a tagged JSON object:
//!
//! ```json
//! { "type": "Int", "value": 42 }
//! ```

use crate::variant::Variant;
use gdcore::math::{Color, Rect2, Vector2, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use serde_json::{json, Value};

/// Serializes a `Variant` to a `serde_json::Value`.
pub fn to_json(v: &Variant) -> Value {
    match v {
        Variant::Nil => json!({ "type": "Nil" }),
        Variant::Bool(b) => json!({ "type": "Bool", "value": b }),
        Variant::Int(i) => json!({ "type": "Int", "value": i }),
        Variant::Float(f) => json!({ "type": "Float", "value": f }),
        Variant::String(s) => json!({ "type": "String", "value": s }),
        Variant::StringName(sn) => json!({ "type": "StringName", "value": sn.as_str() }),
        Variant::NodePath(np) => json!({ "type": "NodePath", "value": np.to_string() }),
        Variant::Vector2(v) => json!({ "type": "Vector2", "value": [v.x, v.y] }),
        Variant::Vector3(v) => json!({ "type": "Vector3", "value": [v.x, v.y, v.z] }),
        Variant::Rect2(r) => json!({
            "type": "Rect2",
            "value": {
                "position": [r.position.x, r.position.y],
                "size": [r.size.x, r.size.y]
            }
        }),
        Variant::Transform2D(t) => json!({
            "type": "Transform2D",
            "value": {
                "x": [t.x.x, t.x.y],
                "y": [t.y.x, t.y.y],
                "origin": [t.origin.x, t.origin.y]
            }
        }),
        Variant::Color(c) => json!({ "type": "Color", "value": [c.r, c.g, c.b, c.a] }),
        Variant::Basis(b) => json!({
            "type": "Basis",
            "value": {
                "x": {"x": b.x.x, "y": b.x.y, "z": b.x.z},
                "y": {"x": b.y.x, "y": b.y.y, "z": b.y.z},
                "z": {"x": b.z.x, "y": b.z.y, "z": b.z.z}
            }
        }),
        Variant::Transform3D(t) => json!({
            "type": "Transform3D",
            "value": {
                "basis": {
                    "x": {"x": t.basis.x.x, "y": t.basis.x.y, "z": t.basis.x.z},
                    "y": {"x": t.basis.y.x, "y": t.basis.y.y, "z": t.basis.y.z},
                    "z": {"x": t.basis.z.x, "y": t.basis.z.y, "z": t.basis.z.z}
                },
                "origin": {"x": t.origin.x, "y": t.origin.y, "z": t.origin.z}
            }
        }),
        Variant::Quaternion(q) => json!({ "type": "Quaternion", "value": [q.x, q.y, q.z, q.w] }),
        Variant::Aabb(a) => json!({
            "type": "AABB",
            "value": {
                "position": [a.position.x, a.position.y, a.position.z],
                "size": [a.size.x, a.size.y, a.size.z]
            }
        }),
        Variant::Plane(p) => json!({
            "type": "Plane",
            "value": {
                "normal": [p.normal.x, p.normal.y, p.normal.z],
                "d": p.d
            }
        }),
        Variant::ObjectId(id) => json!({ "type": "ObjectId", "value": id.raw() }),
        Variant::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(to_json).collect();
            json!({ "type": "Array", "value": items })
        }
        Variant::Dictionary(dict) => {
            let entries: serde_json::Map<String, Value> =
                dict.iter().map(|(k, v)| (k.clone(), to_json(v))).collect();
            json!({ "type": "Dictionary", "value": entries })
        }
        Variant::Callable(c) => json!({ "type": "Callable", "value": format!("{c:?}") }),
        Variant::Resource(r) => json!({
            "type": "Resource",
            "value": { "class_name": r.class_name, "path": r.path }
        }),
    }
}

/// Parse a Vector3 from either named-component format `{"x":1,"y":2,"z":3}`
/// or legacy array format `[1,2,3]`.
fn parse_vec3(val: &Value) -> Option<Vector3> {
    if let Some(obj) = val.as_object() {
        let x = obj.get("x")?.as_f64()? as f32;
        let y = obj.get("y")?.as_f64()? as f32;
        let z = obj.get("z")?.as_f64()? as f32;
        Some(Vector3::new(x, y, z))
    } else if let Some(arr) = val.as_array() {
        if arr.len() != 3 {
            return None;
        }
        Some(Vector3::new(
            arr[0].as_f64()? as f32,
            arr[1].as_f64()? as f32,
            arr[2].as_f64()? as f32,
        ))
    } else {
        None
    }
}

/// Deserializes a `Variant` from a `serde_json::Value` produced by [`to_json`].
///
/// Returns `None` if the JSON does not match the expected tagged format.
/// Accepts both named-component format (oracle) and legacy array format.
pub fn from_json(val: &Value) -> Option<Variant> {
    let obj = val.as_object()?;
    let ty = obj.get("type")?.as_str()?;

    match ty {
        "Nil" => Some(Variant::Nil),
        "Bool" => Some(Variant::Bool(obj.get("value")?.as_bool()?)),
        "Int" => Some(Variant::Int(obj.get("value")?.as_i64()?)),
        "Float" => Some(Variant::Float(obj.get("value")?.as_f64()?)),
        "String" => Some(Variant::String(obj.get("value")?.as_str()?.to_owned())),
        "StringName" => {
            let s = obj.get("value")?.as_str()?;
            Some(Variant::StringName(gdcore::StringName::new(s)))
        }
        "NodePath" => {
            let s = obj.get("value")?.as_str()?;
            Some(Variant::NodePath(gdcore::NodePath::new(s)))
        }
        "Vector2" => {
            let arr = obj.get("value")?.as_array()?;
            if arr.len() != 2 {
                return None;
            }
            Some(Variant::Vector2(Vector2::new(
                arr[0].as_f64()? as f32,
                arr[1].as_f64()? as f32,
            )))
        }
        "Vector3" => {
            let arr = obj.get("value")?.as_array()?;
            if arr.len() != 3 {
                return None;
            }
            Some(Variant::Vector3(Vector3::new(
                arr[0].as_f64()? as f32,
                arr[1].as_f64()? as f32,
                arr[2].as_f64()? as f32,
            )))
        }
        "Rect2" => {
            let v = obj.get("value")?.as_object()?;
            let pos = v.get("position")?.as_array()?;
            let sz = v.get("size")?.as_array()?;
            if pos.len() != 2 || sz.len() != 2 {
                return None;
            }
            Some(Variant::Rect2(Rect2::new(
                Vector2::new(pos[0].as_f64()? as f32, pos[1].as_f64()? as f32),
                Vector2::new(sz[0].as_f64()? as f32, sz[1].as_f64()? as f32),
            )))
        }
        "Color" => {
            let arr = obj.get("value")?.as_array()?;
            if arr.len() != 4 {
                return None;
            }
            Some(Variant::Color(Color::new(
                arr[0].as_f64()? as f32,
                arr[1].as_f64()? as f32,
                arr[2].as_f64()? as f32,
                arr[3].as_f64()? as f32,
            )))
        }
        "Basis" => {
            let v = obj.get("value")?.as_object()?;
            let x = parse_vec3(v.get("x")?)?;
            let y = parse_vec3(v.get("y")?)?;
            let z = parse_vec3(v.get("z")?)?;
            Some(Variant::Basis(Basis { x, y, z }))
        }
        "Transform3D" => {
            let v = obj.get("value")?.as_object()?;
            let b = v.get("basis")?.as_object()?;
            let bx = parse_vec3(b.get("x")?)?;
            let by = parse_vec3(b.get("y")?)?;
            let bz = parse_vec3(b.get("z")?)?;
            let origin = parse_vec3(v.get("origin")?)?;
            Some(Variant::Transform3D(Transform3D {
                basis: Basis { x: bx, y: by, z: bz },
                origin,
            }))
        }
        "Quaternion" => {
            let arr = obj.get("value")?.as_array()?;
            if arr.len() != 4 {
                return None;
            }
            Some(Variant::Quaternion(Quaternion::new(
                arr[0].as_f64()? as f32,
                arr[1].as_f64()? as f32,
                arr[2].as_f64()? as f32,
                arr[3].as_f64()? as f32,
            )))
        }
        "AABB" => {
            let v = obj.get("value")?.as_object()?;
            let pos = v.get("position")?.as_array()?;
            let sz = v.get("size")?.as_array()?;
            if pos.len() != 3 || sz.len() != 3 {
                return None;
            }
            Some(Variant::Aabb(Aabb::new(
                Vector3::new(
                    pos[0].as_f64()? as f32,
                    pos[1].as_f64()? as f32,
                    pos[2].as_f64()? as f32,
                ),
                Vector3::new(
                    sz[0].as_f64()? as f32,
                    sz[1].as_f64()? as f32,
                    sz[2].as_f64()? as f32,
                ),
            )))
        }
        "Plane" => {
            let v = obj.get("value")?.as_object()?;
            let n = v.get("normal")?.as_array()?;
            let d = v.get("d")?.as_f64()? as f32;
            if n.len() != 3 {
                return None;
            }
            Some(Variant::Plane(Plane::new(
                Vector3::new(
                    n[0].as_f64()? as f32,
                    n[1].as_f64()? as f32,
                    n[2].as_f64()? as f32,
                ),
                d,
            )))
        }
        "ObjectId" => {
            let raw = obj.get("value")?.as_u64()?;
            Some(Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw)))
        }
        "Array" => {
            let items = obj.get("value")?.as_array()?;
            let variants: Option<Vec<Variant>> = items.iter().map(from_json).collect();
            Some(Variant::Array(variants?))
        }
        "Dictionary" => {
            let entries = obj.get("value")?.as_object()?;
            let mut map = std::collections::HashMap::new();
            for (k, v) in entries {
                map.insert(k.clone(), from_json(v)?);
            }
            Some(Variant::Dictionary(map))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;

    fn roundtrip(v: Variant) -> Variant {
        let json = to_json(&v);
        from_json(&json).expect("roundtrip failed")
    }

    #[test]
    fn roundtrip_nil() {
        assert_eq!(roundtrip(Variant::Nil), Variant::Nil);
    }

    #[test]
    fn roundtrip_bool() {
        assert_eq!(roundtrip(Variant::Bool(true)), Variant::Bool(true));
    }

    #[test]
    fn roundtrip_int() {
        assert_eq!(roundtrip(Variant::Int(-99)), Variant::Int(-99));
    }

    #[test]
    fn roundtrip_float() {
        assert_eq!(roundtrip(Variant::Float(3.14)), Variant::Float(3.14));
    }

    #[test]
    fn roundtrip_string() {
        assert_eq!(
            roundtrip(Variant::String("hello".into())),
            Variant::String("hello".into()),
        );
    }

    #[test]
    fn roundtrip_vector2() {
        let v = Variant::Vector2(Vector2::new(1.0, 2.0));
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_color() {
        let v = Variant::Color(Color::new(1.0, 0.5, 0.25, 0.8));
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_array() {
        let v = Variant::Array(vec![
            Variant::Int(1),
            Variant::String("two".into()),
            Variant::Bool(false),
        ]);
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_dictionary() {
        let mut dict = std::collections::HashMap::new();
        dict.insert("name".into(), Variant::String("Patina".into()));
        dict.insert("version".into(), Variant::Int(1));
        let v = Variant::Dictionary(dict);
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_string_name() {
        let v = Variant::StringName(gdcore::StringName::new("player"));
        let rt = roundtrip(v.clone());
        assert_eq!(rt, v);
    }

    #[test]
    fn roundtrip_node_path() {
        let v = Variant::NodePath(gdcore::NodePath::new("/root/Player:position"));
        let rt = roundtrip(v.clone());
        assert_eq!(rt, v);
    }

    #[test]
    fn invalid_json_returns_none() {
        let bad = serde_json::json!({ "type": "Unknown" });
        assert!(from_json(&bad).is_none());

        let missing_type = serde_json::json!({ "value": 42 });
        assert!(from_json(&missing_type).is_none());
    }

    // -- Roundtrip remaining variant types ----------------------------------

    #[test]
    fn roundtrip_vector3() {
        let v = Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_rect2() {
        let v = Variant::Rect2(gdcore::math::Rect2::new(
            Vector2::new(10.0, 20.0),
            Vector2::new(100.0, 50.0),
        ));
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_transform2d() {
        let t = gdcore::math::Transform2D {
            x: Vector2::new(1.0, 0.0),
            y: Vector2::new(0.0, 1.0),
            origin: Vector2::new(50.0, 100.0),
        };
        // Transform2D doesn't have from_json support yet, test to_json is valid
        let json = to_json(&Variant::Transform2D(t));
        let obj = json.as_object().unwrap();
        assert_eq!(obj.get("type").unwrap().as_str().unwrap(), "Transform2D");
    }

    #[test]
    fn roundtrip_object_id() {
        let v = Variant::ObjectId(gdcore::id::ObjectId::from_raw(42));
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_nested_array() {
        let inner = Variant::Array(vec![Variant::Int(1), Variant::Int(2)]);
        let outer = Variant::Array(vec![inner, Variant::String("x".into())]);
        assert_eq!(roundtrip(outer.clone()), outer);
    }

    #[test]
    fn roundtrip_nested_dictionary() {
        let mut inner = std::collections::HashMap::new();
        inner.insert("nested".into(), Variant::Bool(true));
        let mut outer = std::collections::HashMap::new();
        outer.insert("child".into(), Variant::Dictionary(inner));
        let v = Variant::Dictionary(outer);
        assert_eq!(roundtrip(v.clone()), v);
    }

    // -- Malformed JSON inputs ----------------------------------------------

    #[test]
    fn malformed_not_an_object() {
        assert!(from_json(&serde_json::json!(42)).is_none());
        assert!(from_json(&serde_json::json!("string")).is_none());
        assert!(from_json(&serde_json::json!(null)).is_none());
    }

    #[test]
    fn malformed_type_not_string() {
        let j = serde_json::json!({ "type": 42 });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_bool_missing_value() {
        let j = serde_json::json!({ "type": "Bool" });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_int_wrong_value_type() {
        let j = serde_json::json!({ "type": "Int", "value": "not_a_number" });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_vector2_wrong_length() {
        let j = serde_json::json!({ "type": "Vector2", "value": [1.0] });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_vector3_wrong_length() {
        let j = serde_json::json!({ "type": "Vector3", "value": [1.0, 2.0] });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_color_wrong_length() {
        let j = serde_json::json!({ "type": "Color", "value": [1.0, 0.5] });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_array_with_bad_element() {
        let j = serde_json::json!({
            "type": "Array",
            "value": [{"type": "Unknown"}]
        });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_dictionary_with_bad_value() {
        let j = serde_json::json!({
            "type": "Dictionary",
            "value": {"key": {"type": "Unknown"}}
        });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_float_missing_value() {
        let j = serde_json::json!({ "type": "Float" });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_string_wrong_value_type() {
        let j = serde_json::json!({ "type": "String", "value": 42 });
        assert!(from_json(&j).is_none());
    }

    #[test]
    fn malformed_rect2_missing_size() {
        let j = serde_json::json!({
            "type": "Rect2",
            "value": { "position": [0.0, 0.0] }
        });
        assert!(from_json(&j).is_none());
    }

    // -- Basis / Transform3D roundtrip and format tests -----------------------

    #[test]
    fn roundtrip_basis() {
        let v = Variant::Basis(Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(0.0, 0.0, 1.0),
        });
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn roundtrip_transform3d() {
        let v = Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            },
            origin: Vector3::new(10.0, 20.0, 30.0),
        });
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn basis_serializes_named_component_format() {
        let v = Variant::Basis(Basis {
            x: Vector3::new(1.0, 2.0, 3.0),
            y: Vector3::new(4.0, 5.0, 6.0),
            z: Vector3::new(7.0, 8.0, 9.0),
        });
        let json = to_json(&v);
        let val = json.get("value").unwrap();
        // Must use named-component format {"x": ..., "y": ..., "z": ...}
        let x = val.get("x").unwrap().as_object().unwrap();
        assert_eq!(x.get("x").unwrap().as_f64().unwrap() as f32, 1.0);
        assert_eq!(x.get("y").unwrap().as_f64().unwrap() as f32, 2.0);
        assert_eq!(x.get("z").unwrap().as_f64().unwrap() as f32, 3.0);
    }

    #[test]
    fn transform3d_serializes_named_component_format() {
        let v = Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            },
            origin: Vector3::new(5.0, 10.0, 15.0),
        });
        let json = to_json(&v);
        let val = json.get("value").unwrap();
        // Origin must be named-component
        let origin = val.get("origin").unwrap().as_object().unwrap();
        assert_eq!(origin.get("x").unwrap().as_f64().unwrap() as f32, 5.0);
        assert_eq!(origin.get("y").unwrap().as_f64().unwrap() as f32, 10.0);
        assert_eq!(origin.get("z").unwrap().as_f64().unwrap() as f32, 15.0);
        // Basis rows must be named-component
        let bx = val.get("basis").unwrap().get("x").unwrap().as_object().unwrap();
        assert_eq!(bx.get("x").unwrap().as_f64().unwrap() as f32, 1.0);
    }

    #[test]
    fn basis_deserializes_legacy_array_format() {
        // Legacy format: rows as arrays [x, y, z]
        let j = serde_json::json!({
            "type": "Basis",
            "value": {
                "x": [1.0, 0.0, 0.0],
                "y": [0.0, 1.0, 0.0],
                "z": [0.0, 0.0, 1.0]
            }
        });
        let v = from_json(&j).unwrap();
        assert_eq!(v, Variant::Basis(Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(0.0, 0.0, 1.0),
        }));
    }

    #[test]
    fn transform3d_deserializes_legacy_array_format() {
        // Legacy format: arrays instead of named-component objects
        let j = serde_json::json!({
            "type": "Transform3D",
            "value": {
                "basis": {
                    "x": [1.0, 0.0, 0.0],
                    "y": [0.0, 1.0, 0.0],
                    "z": [0.0, 0.0, 1.0]
                },
                "origin": [10.0, 20.0, 30.0]
            }
        });
        let v = from_json(&j).unwrap();
        assert_eq!(v, Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            },
            origin: Vector3::new(10.0, 20.0, 30.0),
        }));
    }

    #[test]
    fn parse_vec3_named_format() {
        let j = serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0});
        let v = parse_vec3(&j).unwrap();
        assert_eq!(v, Vector3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn parse_vec3_array_format() {
        let j = serde_json::json!([4.0, 5.0, 6.0]);
        let v = parse_vec3(&j).unwrap();
        assert_eq!(v, Vector3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn parse_vec3_rejects_bad_array_length() {
        assert!(parse_vec3(&serde_json::json!([1.0, 2.0])).is_none());
        assert!(parse_vec3(&serde_json::json!([1.0, 2.0, 3.0, 4.0])).is_none());
    }

    #[test]
    fn parse_vec3_rejects_non_numeric() {
        assert!(parse_vec3(&serde_json::json!("not a vec")).is_none());
        assert!(parse_vec3(&serde_json::json!(42)).is_none());
    }
}
