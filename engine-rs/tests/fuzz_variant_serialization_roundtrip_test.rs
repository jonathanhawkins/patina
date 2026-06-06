//! Property-based tests for Variant JSON serialization round-trips.
//!
//! Ensures that `to_json(v) |> from_json` produces the original `Variant` for
//! all serializable types, including edge cases (NaN, Infinity, empty
//! containers, deeply nested structures, Unicode strings).

use proptest::prelude::*;
use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Vector2, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use gdcore::{NodePath, StringName};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// proptest strategies for leaf Variant types
// ---------------------------------------------------------------------------

/// Finite f32 values (no NaN/Infinity — those break JSON).
fn finite_f32() -> impl Strategy<Value = f32> {
    prop_oneof![
        prop::num::f32::NORMAL,
        Just(0.0f32),
        Just(-0.0f32),
        Just(f32::MIN),
        Just(f32::MAX),
        Just(f32::EPSILON),
        Just(-f32::EPSILON),
    ]
}

/// Finite f64 values.
fn finite_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        prop::num::f64::NORMAL,
        Just(0.0f64),
        Just(-0.0f64),
        Just(1e30f64),
        Just(-1e30f64),
        Just(f64::EPSILON),
    ]
}

fn arb_vec2() -> impl Strategy<Value = Vector2> {
    (finite_f32(), finite_f32()).prop_map(|(x, y)| Vector2::new(x, y))
}

fn arb_vec3() -> impl Strategy<Value = Vector3> {
    (finite_f32(), finite_f32(), finite_f32()).prop_map(|(x, y, z)| Vector3::new(x, y, z))
}

fn arb_color() -> impl Strategy<Value = Color> {
    (finite_f32(), finite_f32(), finite_f32(), finite_f32())
        .prop_map(|(r, g, b, a)| Color::new(r, g, b, a))
}

fn arb_rect2() -> impl Strategy<Value = Rect2> {
    (arb_vec2(), arb_vec2()).prop_map(|(pos, size)| Rect2::new(pos, size))
}

fn arb_basis() -> impl Strategy<Value = Basis> {
    (arb_vec3(), arb_vec3(), arb_vec3()).prop_map(|(x, y, z)| Basis { x, y, z })
}

fn arb_quaternion() -> impl Strategy<Value = Quaternion> {
    (finite_f32(), finite_f32(), finite_f32(), finite_f32())
        .prop_map(|(x, y, z, w)| Quaternion::new(x, y, z, w))
}

fn arb_transform3d() -> impl Strategy<Value = Transform3D> {
    (arb_basis(), arb_vec3()).prop_map(|(basis, origin)| Transform3D { basis, origin })
}

fn arb_aabb() -> impl Strategy<Value = Aabb> {
    (arb_vec3(), arb_vec3()).prop_map(|(position, size)| Aabb::new(position, size))
}

fn arb_plane() -> impl Strategy<Value = Plane> {
    (arb_vec3(), finite_f32()).prop_map(|(normal, d)| Plane::new(normal, d))
}

/// A non-recursive leaf Variant (no Array/Dictionary).
fn arb_leaf_variant() -> impl Strategy<Value = Variant> {
    prop_oneof![
        Just(Variant::Nil),
        any::<bool>().prop_map(Variant::Bool),
        any::<i64>().prop_map(Variant::Int),
        finite_f64().prop_map(Variant::Float),
        ".*".prop_map(|s: String| Variant::String(s)),
        "[a-zA-Z_][a-zA-Z0-9_]{0,20}"
            .prop_map(|s: String| Variant::StringName(StringName::new(&s))),
        "/[a-zA-Z_/]{0,30}".prop_map(|s: String| Variant::NodePath(NodePath::new(&s))),
        arb_vec2().prop_map(Variant::Vector2),
        arb_vec3().prop_map(Variant::Vector3),
        arb_rect2().prop_map(Variant::Rect2),
        arb_color().prop_map(Variant::Color),
        arb_basis().prop_map(Variant::Basis),
        arb_transform3d().prop_map(Variant::Transform3D),
        arb_quaternion().prop_map(Variant::Quaternion),
        arb_aabb().prop_map(Variant::Aabb),
        arb_plane().prop_map(Variant::Plane),
        any::<u64>().prop_map(|raw| Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw))),
    ]
}

/// A Variant tree (leaves + Array/Dictionary up to depth 3).
fn arb_variant() -> impl Strategy<Value = Variant> {
    arb_leaf_variant().prop_recursive(
        3,  // max depth
        64, // max nodes
        8,  // items per collection
        |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..8).prop_map(Variant::Array),
                prop::collection::hash_map("[a-z]{1,8}", inner.clone(), 0..6)
                    .prop_map(Variant::Dictionary),
            ]
        },
    )
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Round-trip: serialize then deserialize. Returns None if from_json fails.
fn roundtrip(v: &Variant) -> Option<Variant> {
    let json = gdvariant::serialize::to_json(v);
    gdvariant::serialize::from_json(&json)
}

/// Approximate f32 equality (to handle f64→f32→f64 precision loss).
fn f32_approx_eq(a: f32, b: f32) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    (a - b).abs() <= f32::EPSILON * 2.0 * a.abs().max(b.abs()).max(1.0)
}

/// Check whether two Variants are approximately equal (floats may lose
/// precision during f32→f64→f32 conversion through JSON).
fn variant_approx_eq(a: &Variant, b: &Variant) -> bool {
    match (a, b) {
        (Variant::Nil, Variant::Nil) => true,
        (Variant::Bool(a), Variant::Bool(b)) => a == b,
        (Variant::Int(a), Variant::Int(b)) => a == b,
        (Variant::Float(a), Variant::Float(b)) => {
            // f64 roundtrips exactly through JSON
            (a - b).abs() < f64::EPSILON * 2.0 * a.abs().max(b.abs()).max(1.0)
        }
        (Variant::String(a), Variant::String(b)) => a == b,
        (Variant::StringName(a), Variant::StringName(b)) => a.as_str() == b.as_str(),
        (Variant::NodePath(a), Variant::NodePath(b)) => a.to_string() == b.to_string(),
        (Variant::Vector2(a), Variant::Vector2(b)) => {
            f32_approx_eq(a.x, b.x) && f32_approx_eq(a.y, b.y)
        }
        (Variant::Vector3(a), Variant::Vector3(b)) => {
            f32_approx_eq(a.x, b.x) && f32_approx_eq(a.y, b.y) && f32_approx_eq(a.z, b.z)
        }
        (Variant::Rect2(a), Variant::Rect2(b)) => {
            f32_approx_eq(a.position.x, b.position.x)
                && f32_approx_eq(a.position.y, b.position.y)
                && f32_approx_eq(a.size.x, b.size.x)
                && f32_approx_eq(a.size.y, b.size.y)
        }
        (Variant::Color(a), Variant::Color(b)) => {
            f32_approx_eq(a.r, b.r)
                && f32_approx_eq(a.g, b.g)
                && f32_approx_eq(a.b, b.b)
                && f32_approx_eq(a.a, b.a)
        }
        (Variant::Basis(a), Variant::Basis(b)) => [a.x, a.y, a.z]
            .iter()
            .zip([b.x, b.y, b.z].iter())
            .all(|(va, vb)| {
                f32_approx_eq(va.x, vb.x) && f32_approx_eq(va.y, vb.y) && f32_approx_eq(va.z, vb.z)
            }),
        (Variant::Transform3D(a), Variant::Transform3D(b)) => {
            variant_approx_eq(&Variant::Basis(a.basis), &Variant::Basis(b.basis))
                && f32_approx_eq(a.origin.x, b.origin.x)
                && f32_approx_eq(a.origin.y, b.origin.y)
                && f32_approx_eq(a.origin.z, b.origin.z)
        }
        (Variant::Quaternion(a), Variant::Quaternion(b)) => {
            f32_approx_eq(a.x, b.x)
                && f32_approx_eq(a.y, b.y)
                && f32_approx_eq(a.z, b.z)
                && f32_approx_eq(a.w, b.w)
        }
        (Variant::Aabb(a), Variant::Aabb(b)) => {
            f32_approx_eq(a.position.x, b.position.x)
                && f32_approx_eq(a.position.y, b.position.y)
                && f32_approx_eq(a.position.z, b.position.z)
                && f32_approx_eq(a.size.x, b.size.x)
                && f32_approx_eq(a.size.y, b.size.y)
                && f32_approx_eq(a.size.z, b.size.z)
        }
        (Variant::Plane(a), Variant::Plane(b)) => {
            f32_approx_eq(a.normal.x, b.normal.x)
                && f32_approx_eq(a.normal.y, b.normal.y)
                && f32_approx_eq(a.normal.z, b.normal.z)
                && f32_approx_eq(a.d, b.d)
        }
        (Variant::ObjectId(a), Variant::ObjectId(b)) => a.raw() == b.raw(),
        (Variant::Array(a), Variant::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| variant_approx_eq(x, y))
        }
        (Variant::Dictionary(a), Variant::Dictionary(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).map(|bv| variant_approx_eq(v, bv)).unwrap_or(false))
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Every leaf Variant round-trips through JSON without loss.
    #[test]
    fn leaf_variant_roundtrip(v in arb_leaf_variant()) {
        let rt = roundtrip(&v).expect("from_json returned None");
        prop_assert!(
            variant_approx_eq(&v, &rt),
            "Roundtrip mismatch:\n  original: {:?}\n  got:      {:?}",
            v,
            rt,
        );
    }

    /// Recursive Variant trees (Array/Dictionary) round-trip.
    #[test]
    fn nested_variant_roundtrip(v in arb_variant()) {
        let rt = roundtrip(&v).expect("from_json returned None");
        prop_assert!(
            variant_approx_eq(&v, &rt),
            "Roundtrip mismatch for nested variant:\n  original: {:?}\n  got:      {:?}",
            v,
            rt,
        );
    }

    /// to_json always produces a JSON object with a "type" field.
    #[test]
    fn serialized_has_type_field(v in arb_variant()) {
        let json = gdvariant::serialize::to_json(&v);
        prop_assert!(json.is_object(), "Expected JSON object, got {:?}", json);
        let obj = json.as_object().unwrap();
        prop_assert!(
            obj.contains_key("type"),
            "Missing 'type' field in {:?}",
            json,
        );
        let ty = obj.get("type").unwrap();
        prop_assert!(ty.is_string(), "'type' field is not a string: {:?}", ty);
    }

    /// Double round-trip: serialize→deserialize→serialize→deserialize
    /// yields the same result as a single round-trip.
    #[test]
    fn double_roundtrip_stable(v in arb_leaf_variant()) {
        let rt1 = roundtrip(&v).expect("first roundtrip failed");
        let rt2 = roundtrip(&rt1).expect("second roundtrip failed");
        prop_assert!(
            variant_approx_eq(&rt1, &rt2),
            "Double roundtrip diverged:\n  rt1: {:?}\n  rt2: {:?}",
            rt1,
            rt2,
        );
    }

    /// Int values are preserved exactly (no float coercion).
    #[test]
    fn int_roundtrip_exact(i in any::<i64>()) {
        let v = Variant::Int(i);
        let rt = roundtrip(&v).expect("roundtrip failed");
        prop_assert_eq!(v, rt);
    }

    /// Bool values are preserved exactly.
    #[test]
    fn bool_roundtrip_exact(b in any::<bool>()) {
        let v = Variant::Bool(b);
        let rt = roundtrip(&v).expect("roundtrip failed");
        prop_assert_eq!(v, rt);
    }

    /// String values (including Unicode, empty, whitespace) are preserved.
    #[test]
    fn string_roundtrip_exact(s in ".*") {
        let v = Variant::String(s);
        let rt = roundtrip(&v).expect("roundtrip failed");
        prop_assert_eq!(v, rt);
    }

    /// Empty containers round-trip correctly.
    #[test]
    fn empty_containers_roundtrip(_dummy in Just(())) {
        let empty_arr = Variant::Array(vec![]);
        let rt_arr = roundtrip(&empty_arr).expect("empty array roundtrip failed");
        prop_assert_eq!(empty_arr, rt_arr);

        let empty_dict = Variant::Dictionary(HashMap::new());
        let rt_dict = roundtrip(&empty_dict).expect("empty dict roundtrip failed");
        prop_assert_eq!(empty_dict, rt_dict);
    }
}

// ---------------------------------------------------------------------------
// NaN / Infinity handling (these are special: JSON can't represent them)
// ---------------------------------------------------------------------------

#[test]
fn nan_float_serialization_does_not_panic() {
    let v = Variant::Float(f64::NAN);
    let json = gdvariant::serialize::to_json(&v);
    // NaN serializes to JSON null in serde_json — from_json may return None,
    // but it must not panic.
    let _ = gdvariant::serialize::from_json(&json);
}

#[test]
fn infinity_float_serialization_does_not_panic() {
    let v = Variant::Float(f64::INFINITY);
    let json = gdvariant::serialize::to_json(&v);
    let _ = gdvariant::serialize::from_json(&json);

    let v = Variant::Float(f64::NEG_INFINITY);
    let json = gdvariant::serialize::to_json(&v);
    let _ = gdvariant::serialize::from_json(&json);
}

#[test]
fn nan_vector_components_do_not_panic() {
    let v = Variant::Vector2(Vector2::new(f32::NAN, 0.0));
    let json = gdvariant::serialize::to_json(&v);
    let _ = gdvariant::serialize::from_json(&json);

    let v = Variant::Vector3(Vector3::new(0.0, f32::INFINITY, f32::NEG_INFINITY));
    let json = gdvariant::serialize::to_json(&v);
    let _ = gdvariant::serialize::from_json(&json);
}

#[test]
fn deeply_nested_array_roundtrip() {
    // Build a 10-deep nested array.
    let mut v = Variant::Int(42);
    for _ in 0..10 {
        v = Variant::Array(vec![v]);
    }
    let rt = roundtrip(&v).expect("deep roundtrip failed");
    assert_eq!(v, rt);
}

#[test]
fn deeply_nested_dictionary_roundtrip() {
    let mut v = Variant::String("leaf".into());
    for i in 0..10 {
        let mut d = HashMap::new();
        d.insert(format!("level_{i}"), v);
        v = Variant::Dictionary(d);
    }
    let rt = roundtrip(&v).expect("deep dict roundtrip failed");
    assert_eq!(v, rt);
}
