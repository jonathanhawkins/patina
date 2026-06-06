//! Property-based / fuzz tests for Variant serialization roundtrips.
//!
//! Uses `proptest` to generate random Variant values and verify that
//! `to_json` -> `from_json` produces an equivalent value, and that
//! `variant_type()` is consistent before and after serialization.

#[cfg(test)]
mod tests {
    use crate::serialize::{from_json, to_json};
    use crate::variant::{Variant, VariantType};
    use gdcore::id::ObjectId;
    use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
    use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
    use gdcore::node_path::NodePath;
    use gdcore::string_name::StringName;
    use proptest::prelude::*;
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Strategy helpers
    // -----------------------------------------------------------------------

    /// Generate a finite f64 (no NaN, no infinity) that survives JSON roundtrip.
    fn finite_f64() -> impl Strategy<Value = f64> {
        prop_oneof![
            prop::num::f64::NORMAL,
            Just(0.0f64),
            Just(1.0f64),
            Just(-1.0f64),
        ]
    }

    /// Generate a finite f32 that survives f32 -> f64 -> f32 JSON roundtrip.
    fn finite_f32() -> impl Strategy<Value = f32> {
        prop_oneof![
            prop::num::f32::NORMAL,
            Just(0.0f32),
            Just(1.0f32),
            Just(-1.0f32),
        ]
    }

    fn arb_vector2() -> impl Strategy<Value = Vector2> {
        (finite_f32(), finite_f32()).prop_map(|(x, y)| Vector2::new(x, y))
    }

    fn arb_vector3() -> impl Strategy<Value = Vector3> {
        (finite_f32(), finite_f32(), finite_f32()).prop_map(|(x, y, z)| Vector3::new(x, y, z))
    }

    fn arb_color() -> impl Strategy<Value = Color> {
        (finite_f32(), finite_f32(), finite_f32(), finite_f32())
            .prop_map(|(r, g, b, a)| Color::new(r, g, b, a))
    }

    /// Generate a leaf Variant (no Array/Dictionary nesting).
    fn arb_leaf_variant() -> impl Strategy<Value = Variant> {
        prop_oneof![
            Just(Variant::Nil),
            any::<bool>().prop_map(Variant::Bool),
            any::<i64>().prop_map(Variant::Int),
            finite_f64().prop_map(Variant::Float),
            ".*".prop_map(|s: String| Variant::String(s)),
            arb_vector2().prop_map(Variant::Vector2),
            arb_vector3().prop_map(Variant::Vector3),
            arb_color().prop_map(Variant::Color),
            any::<u64>().prop_map(|id| Variant::ObjectId(ObjectId::from_raw(id))),
            ".*".prop_map(|s: String| Variant::StringName(StringName::new(&s))),
            ".*".prop_map(|s: String| Variant::NodePath(NodePath::new(&s))),
            (arb_vector2(), arb_vector2())
                .prop_map(|(pos, sz)| Variant::Rect2(Rect2::new(pos, sz))),
            (arb_vector2(), arb_vector2(), arb_vector2())
                .prop_map(|(x, y, o)| Variant::Transform2D(Transform2D { x, y, origin: o })),
            (arb_vector3(), arb_vector3(), arb_vector3())
                .prop_map(|(x, y, z)| Variant::Basis(Basis { x, y, z })),
            (finite_f32(), finite_f32(), finite_f32(), finite_f32())
                .prop_map(|(x, y, z, w)| Variant::Quaternion(Quaternion::new(x, y, z, w))),
            (arb_vector3(), arb_vector3()).prop_map(|(pos, sz)| Variant::Aabb(Aabb::new(pos, sz))),
            (arb_vector3(), finite_f32()).prop_map(|(n, d)| Variant::Plane(Plane::new(n, d))),
        ]
    }

    /// Generate a Variant that may include Arrays and Dictionaries (recursive).
    fn arb_variant() -> impl Strategy<Value = Variant> {
        arb_leaf_variant().prop_recursive(
            3,  // max depth
            64, // max nodes
            8,  // items per collection
            |inner| {
                prop_oneof![
                    prop::collection::vec(inner.clone(), 0..8).prop_map(Variant::Array),
                    prop::collection::hash_map("[a-z]{1,8}", inner, 0..6)
                        .prop_map(Variant::Dictionary),
                ]
            },
        )
    }

    // -----------------------------------------------------------------------
    // Roundtrip helper
    // -----------------------------------------------------------------------

    fn roundtrip(v: &Variant) -> Variant {
        let json = to_json(v);
        from_json(&json).unwrap_or_else(|| {
            panic!(
                "from_json returned None for variant {:?}\nJSON: {}",
                v,
                serde_json::to_string_pretty(&json).unwrap()
            )
        })
    }

    fn assert_roundtrip_eq(original: &Variant, deserialized: &Variant) {
        assert_eq!(
            original, deserialized,
            "roundtrip mismatch:\n  original:     {:?}\n  deserialized: {:?}",
            original, deserialized
        );
    }

    // -----------------------------------------------------------------------
    // 1. Leaf type roundtrips via proptest
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn fuzz_nil_roundtrip(_ in 0..1u8) {
            assert_roundtrip_eq(&Variant::Nil, &roundtrip(&Variant::Nil));
        }

        #[test]
        fn fuzz_bool_roundtrip(b in any::<bool>()) {
            let v = Variant::Bool(b);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_int_roundtrip(i in any::<i64>()) {
            let v = Variant::Int(i);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_float_roundtrip(f in finite_f64()) {
            let v = Variant::Float(f);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_string_roundtrip(s in ".*") {
            let v = Variant::String(s);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_vector2_roundtrip(vec in arb_vector2()) {
            let v = Variant::Vector2(vec);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_vector3_roundtrip(vec in arb_vector3()) {
            let v = Variant::Vector3(vec);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_color_roundtrip(c in arb_color()) {
            let v = Variant::Color(c);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_string_name_roundtrip(s in ".*") {
            let v = Variant::StringName(StringName::new(&s));
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_node_path_roundtrip(s in "(/[a-zA-Z_][a-zA-Z0-9_]*){0,5}") {
            let v = Variant::NodePath(NodePath::new(&s));
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_rect2_roundtrip((pos, sz) in (arb_vector2(), arb_vector2())) {
            let v = Variant::Rect2(Rect2::new(pos, sz));
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_quaternion_roundtrip((x, y, z, w) in (finite_f32(), finite_f32(), finite_f32(), finite_f32())) {
            let v = Variant::Quaternion(Quaternion::new(x, y, z, w));
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_object_id_roundtrip(id in any::<u64>()) {
            let v = Variant::ObjectId(ObjectId::from_raw(id));
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }
    }

    // -----------------------------------------------------------------------
    // 2. Collection roundtrips via proptest
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn fuzz_array_roundtrip(items in prop::collection::vec(arb_leaf_variant(), 0..10)) {
            let v = Variant::Array(items);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_dictionary_roundtrip(entries in prop::collection::hash_map("[a-z]{1,8}", arb_leaf_variant(), 0..8)) {
            let v = Variant::Dictionary(entries);
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }

        #[test]
        fn fuzz_nested_variant_roundtrip(v in arb_variant()) {
            assert_roundtrip_eq(&v, &roundtrip(&v));
        }
    }

    // -----------------------------------------------------------------------
    // 3. variant_type() consistency via proptest
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn fuzz_variant_type_preserved(v in arb_variant()) {
            let ty_before = v.variant_type();
            let rt = roundtrip(&v);
            let ty_after = rt.variant_type();
            prop_assert_eq!(ty_before, ty_after,
                "variant_type changed: {:?} -> {:?}", ty_before, ty_after);
        }

        #[test]
        fn fuzz_variant_type_matches_discriminant(v in arb_leaf_variant()) {
            let ty = v.variant_type();
            match &v {
                Variant::Nil => prop_assert_eq!(ty, VariantType::Nil),
                Variant::Bool(_) => prop_assert_eq!(ty, VariantType::Bool),
                Variant::Int(_) => prop_assert_eq!(ty, VariantType::Int),
                Variant::Float(_) => prop_assert_eq!(ty, VariantType::Float),
                Variant::String(_) => prop_assert_eq!(ty, VariantType::String),
                Variant::StringName(_) => prop_assert_eq!(ty, VariantType::StringName),
                Variant::NodePath(_) => prop_assert_eq!(ty, VariantType::NodePath),
                Variant::Vector2(_) => prop_assert_eq!(ty, VariantType::Vector2),
                Variant::Vector3(_) => prop_assert_eq!(ty, VariantType::Vector3),
                Variant::Rect2(_) => prop_assert_eq!(ty, VariantType::Rect2),
                Variant::Transform2D(_) => prop_assert_eq!(ty, VariantType::Transform2D),
                Variant::Color(_) => prop_assert_eq!(ty, VariantType::Color),
                Variant::Basis(_) => prop_assert_eq!(ty, VariantType::Basis),
                Variant::Quaternion(_) => prop_assert_eq!(ty, VariantType::Quaternion),
                Variant::Aabb(_) => prop_assert_eq!(ty, VariantType::Aabb),
                Variant::Plane(_) => prop_assert_eq!(ty, VariantType::Plane),
                Variant::ObjectId(_) => prop_assert_eq!(ty, VariantType::ObjectId),
                Variant::Array(_) => prop_assert_eq!(ty, VariantType::Array),
                Variant::Dictionary(_) => prop_assert_eq!(ty, VariantType::Dictionary),
                _ => {} // Callable, Resource not generated by arb_leaf_variant
            }
        }
    }

    // -----------------------------------------------------------------------
    // 4. Deterministic edge-case tests
    // -----------------------------------------------------------------------

    #[test]
    fn edge_empty_string_roundtrip() {
        let v = Variant::String(String::new());
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_very_large_int_roundtrip() {
        let v = Variant::Int(i64::MAX);
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_very_small_int_roundtrip() {
        let v = Variant::Int(i64::MIN);
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_zero_int_roundtrip() {
        let v = Variant::Int(0);
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_nan_float_serializes_but_does_not_roundtrip() {
        // NaN cannot survive JSON roundtrip (serde_json serializes NaN as null).
        let v = Variant::Float(f64::NAN);
        let json = to_json(&v);
        let result = from_json(&json);
        assert!(
            result.is_none(),
            "NaN should not roundtrip through JSON; got {:?}",
            result
        );
    }

    #[test]
    fn edge_infinity_float_does_not_roundtrip() {
        let v = Variant::Float(f64::INFINITY);
        let json = to_json(&v);
        let result = from_json(&json);
        assert!(
            result.is_none(),
            "Infinity should not roundtrip through JSON; got {:?}",
            result
        );
    }

    #[test]
    fn edge_neg_infinity_float_does_not_roundtrip() {
        let v = Variant::Float(f64::NEG_INFINITY);
        let json = to_json(&v);
        let result = from_json(&json);
        assert!(
            result.is_none(),
            "Negative infinity should not roundtrip through JSON; got {:?}",
            result
        );
    }

    #[test]
    fn edge_deeply_nested_arrays() {
        let mut v = Variant::Int(42);
        for _ in 0..10 {
            v = Variant::Array(vec![v]);
        }
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_deeply_nested_dictionaries() {
        let mut v = Variant::String("leaf".into());
        for i in 0..10 {
            let mut map = HashMap::new();
            map.insert(format!("level_{i}"), v);
            v = Variant::Dictionary(map);
        }
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_empty_array_roundtrip() {
        let v = Variant::Array(vec![]);
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_empty_dictionary_roundtrip() {
        let v = Variant::Dictionary(HashMap::new());
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_mixed_array_all_leaf_types() {
        let items = vec![
            Variant::Nil,
            Variant::Bool(true),
            Variant::Int(42),
            Variant::Float(3.14),
            Variant::String("hello".into()),
            Variant::Vector2(Vector2::new(1.0, 2.0)),
            Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        ];
        let v = Variant::Array(items);
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_unicode_string_roundtrip() {
        let v = Variant::String("Hello \u{1F600} world \u{00E9}\u{00F1}\u{00FC}".into());
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_string_with_json_special_chars() {
        let v = Variant::String(r#"quote: " backslash: \ newline: \n tab: \t"#.into());
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_negative_zero_float() {
        let v = Variant::Float(-0.0);
        let rt = roundtrip(&v);
        // -0.0 == 0.0 in f64, both are valid roundtrip results
        assert_eq!(rt, Variant::Float(0.0));
    }

    #[test]
    fn edge_basis_identity_roundtrip() {
        let v = Variant::Basis(Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(0.0, 0.0, 1.0),
        });
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_transform3d_identity_roundtrip() {
        let v = Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            },
            origin: Vector3::new(0.0, 0.0, 0.0),
        });
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_plane_zero_d_roundtrip() {
        let v = Variant::Plane(Plane::new(Vector3::new(0.0, 1.0, 0.0), 0.0));
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_aabb_zero_size_roundtrip() {
        let v = Variant::Aabb(Aabb::new(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
        ));
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }

    #[test]
    fn edge_deeply_nested_mixed_array_dict() {
        // Alternate array and dictionary nesting 10 levels deep
        let mut v = Variant::Int(99);
        for i in 0..10 {
            if i % 2 == 0 {
                v = Variant::Array(vec![v]);
            } else {
                let mut map = HashMap::new();
                map.insert(format!("k{i}"), v);
                v = Variant::Dictionary(map);
            }
        }
        assert_roundtrip_eq(&v, &roundtrip(&v));
    }
}
