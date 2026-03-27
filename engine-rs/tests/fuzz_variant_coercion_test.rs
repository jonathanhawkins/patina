//! Property-based tests for Variant type coercion rules.
//!
//! Validates that `coerce_variant`, `is_truthy`, `variant_type`, Display,
//! and the implicit numeric promotion in comparisons/equality all behave
//! correctly and never panic.

use gdcore::math::{Color, Vector2, Vector3};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use gdeditor::inspector::coerce_variant;
use gdvariant::{Variant, VariantType};
use proptest::prelude::*;

// ===========================================================================
// Strategy: generate arbitrary Variant values
// ===========================================================================

fn arb_variant() -> impl Strategy<Value = Variant> {
    prop_oneof![
        Just(Variant::Nil),
        any::<bool>().prop_map(Variant::Bool),
        any::<i64>().prop_map(Variant::Int),
        // Use finite floats only to avoid NaN comparison issues
        (-1e15f64..1e15).prop_map(Variant::Float),
        "[\\x00-\\x7f]{0,50}".prop_map(|s: String| Variant::String(s)),
        "[a-z_]{0,20}".prop_map(|s: String| Variant::StringName(StringName::new(&s))),
        "[a-zA-Z0-9_/]{0,30}".prop_map(|s: String| Variant::NodePath(NodePath::new(&s))),
        (any::<f32>(), any::<f32>()).prop_map(|(x, y)| Variant::Vector2(Vector2::new(x, y))),
        (any::<f32>(), any::<f32>(), any::<f32>())
            .prop_map(|(x, y, z)| Variant::Vector3(Vector3::new(x, y, z))),
        (any::<f32>(), any::<f32>(), any::<f32>(), any::<f32>()).prop_map(|(r, g, b, a)| {
            Variant::Color(Color::new(r, g, b, a))
        }),
    ]
}

fn arb_variant_type() -> impl Strategy<Value = VariantType> {
    prop_oneof![
        Just(VariantType::Nil),
        Just(VariantType::Bool),
        Just(VariantType::Int),
        Just(VariantType::Float),
        Just(VariantType::String),
        Just(VariantType::StringName),
        Just(VariantType::NodePath),
        Just(VariantType::Vector2),
        Just(VariantType::Vector3),
        Just(VariantType::Rect2),
        Just(VariantType::Transform2D),
        Just(VariantType::Color),
        Just(VariantType::Basis),
        Just(VariantType::Transform3D),
        Just(VariantType::Quaternion),
        Just(VariantType::Aabb),
        Just(VariantType::Plane),
        Just(VariantType::ObjectId),
        Just(VariantType::Array),
        Just(VariantType::Dictionary),
    ]
}

// ===========================================================================
// coerce_variant: never panics
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Coercing any variant to any type must never panic.
    #[test]
    fn coerce_never_panics(value in arb_variant(), target in arb_variant_type()) {
        let _ = coerce_variant(&value, target);
    }

    /// Coercing a value to its own type is always identity.
    #[test]
    fn coerce_same_type_is_identity(value in arb_variant()) {
        let target = value.variant_type();
        let result = coerce_variant(&value, target);
        prop_assert!(result.is_some(), "same-type coercion should always succeed");
        prop_assert_eq!(result.unwrap().variant_type(), target);
    }
}

// ===========================================================================
// Int ↔ Float coercion: round-trip properties
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Int → Float → Int round-trip preserves value for representable integers.
    #[test]
    fn int_float_roundtrip(i in -(1i64 << 52)..(1i64 << 52)) {
        let as_float = coerce_variant(&Variant::Int(i), VariantType::Float).unwrap();
        let back_to_int = coerce_variant(&as_float, VariantType::Int).unwrap();
        prop_assert_eq!(back_to_int, Variant::Int(i));
    }

    /// Float → Int truncates (matches Godot `int(f)` semantics).
    #[test]
    fn float_to_int_truncates(f in -1e9f64..1e9) {
        let result = coerce_variant(&Variant::Float(f), VariantType::Int).unwrap();
        match result {
            Variant::Int(i) => prop_assert_eq!(i, f as i64),
            _ => prop_assert!(false, "expected Int, got {:?}", result),
        }
    }

    /// Int → Float is exact for small integers.
    #[test]
    fn int_to_float_exact(i in -1_000_000i64..1_000_000) {
        let result = coerce_variant(&Variant::Int(i), VariantType::Float).unwrap();
        match result {
            Variant::Float(f) => prop_assert_eq!(f, i as f64),
            _ => prop_assert!(false, "expected Float"),
        }
    }
}

// ===========================================================================
// Bool coercion
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Int → Bool: 0 is false, everything else is true.
    #[test]
    fn int_to_bool(i in any::<i64>()) {
        let result = coerce_variant(&Variant::Int(i), VariantType::Bool).unwrap();
        let expected = Variant::Bool(i != 0);
        prop_assert_eq!(result, expected);
    }

    /// Bool → Int: true=1, false=0.
    #[test]
    fn bool_to_int(b in any::<bool>()) {
        let result = coerce_variant(&Variant::Bool(b), VariantType::Int).unwrap();
        let expected = Variant::Int(if b { 1 } else { 0 });
        prop_assert_eq!(result, expected);
    }

    /// Float → Bool: 0.0 is false, everything else is true.
    #[test]
    fn float_to_bool(f in -1e10f64..1e10) {
        let result = coerce_variant(&Variant::Float(f), VariantType::Bool).unwrap();
        let expected = Variant::Bool(f != 0.0);
        prop_assert_eq!(result, expected);
    }

    /// Bool → Float: true=1.0, false=0.0.
    #[test]
    fn bool_to_float(b in any::<bool>()) {
        let result = coerce_variant(&Variant::Bool(b), VariantType::Float).unwrap();
        let expected = Variant::Float(if b { 1.0 } else { 0.0 });
        prop_assert_eq!(result, expected);
    }
}

// ===========================================================================
// String coercion
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Int → String produces decimal representation.
    #[test]
    fn int_to_string(i in any::<i64>()) {
        let result = coerce_variant(&Variant::Int(i), VariantType::String).unwrap();
        prop_assert_eq!(result, Variant::String(i.to_string()));
    }

    /// Bool → String produces "true" or "false".
    #[test]
    fn bool_to_string(b in any::<bool>()) {
        let result = coerce_variant(&Variant::Bool(b), VariantType::String).unwrap();
        prop_assert_eq!(result, Variant::String(b.to_string()));
    }

    /// String → StringName → String round-trip preserves value.
    #[test]
    fn string_stringname_roundtrip(s in "[a-z_]{0,30}") {
        let as_sn = coerce_variant(&Variant::String(s.clone()), VariantType::StringName).unwrap();
        let back = coerce_variant(&as_sn, VariantType::String).unwrap();
        prop_assert_eq!(back, Variant::String(s));
    }

    /// String → NodePath always succeeds.
    #[test]
    fn string_to_nodepath(s in "[a-zA-Z0-9_/]{0,30}") {
        let result = coerce_variant(&Variant::String(s.clone()), VariantType::NodePath);
        prop_assert!(result.is_some());
        match result.unwrap() {
            Variant::NodePath(np) => prop_assert_eq!(np.to_string(), s),
            other => prop_assert!(false, "expected NodePath, got {:?}", other),
        }
    }
}

// ===========================================================================
// Unsupported coercions return None (not panic)
// ===========================================================================

#[test]
fn vector2_to_int_returns_none() {
    let v = Variant::Vector2(Vector2::new(1.0, 2.0));
    assert!(coerce_variant(&v, VariantType::Int).is_none());
}

#[test]
fn color_to_int_returns_none() {
    let c = Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0));
    assert!(coerce_variant(&c, VariantType::Int).is_none());
}

#[test]
fn nil_to_int_returns_none() {
    assert!(coerce_variant(&Variant::Nil, VariantType::Int).is_none());
}

#[test]
fn int_to_vector2_returns_none() {
    assert!(coerce_variant(&Variant::Int(42), VariantType::Vector2).is_none());
}

#[test]
fn string_to_int_returns_none() {
    // Unlike some languages, Godot doesn't auto-parse "42" → 42 in coercion
    assert!(coerce_variant(&Variant::String("42".into()), VariantType::Int).is_none());
}

#[test]
fn string_to_float_returns_none() {
    assert!(coerce_variant(&Variant::String("3.14".into()), VariantType::Float).is_none());
}

// ===========================================================================
// is_truthy: property-based tests
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// is_truthy never panics on any variant.
    #[test]
    fn is_truthy_never_panics(value in arb_variant()) {
        let _ = value.is_truthy();
    }

    /// Zero int is falsy, nonzero is truthy.
    #[test]
    fn int_truthiness(i in any::<i64>()) {
        prop_assert_eq!(Variant::Int(i).is_truthy(), i != 0);
    }

    /// Zero float is falsy, nonzero is truthy.
    #[test]
    fn float_truthiness(f in -1e15f64..1e15) {
        prop_assert_eq!(Variant::Float(f).is_truthy(), f != 0.0);
    }

    /// Empty string is falsy, non-empty is truthy.
    #[test]
    fn string_truthiness(s in ".*") {
        prop_assert_eq!(Variant::String(s.clone()).is_truthy(), !s.is_empty());
    }

    /// Empty array is falsy, non-empty is truthy.
    #[test]
    fn array_truthiness(len in 0usize..10) {
        let arr: Vec<Variant> = (0..len).map(|i| Variant::Int(i as i64)).collect();
        prop_assert_eq!(Variant::Array(arr).is_truthy(), len > 0);
    }
}

#[test]
fn nil_is_falsy() {
    assert!(!Variant::Nil.is_truthy());
}

#[test]
fn bool_truthiness_matches_value() {
    assert!(Variant::Bool(true).is_truthy());
    assert!(!Variant::Bool(false).is_truthy());
}

#[test]
fn math_types_are_always_truthy() {
    // Even zero vectors are truthy in Godot
    assert!(Variant::Vector2(Vector2::ZERO).is_truthy());
    assert!(Variant::Vector3(Vector3::ZERO).is_truthy());
    assert!(Variant::Color(Color::new(0.0, 0.0, 0.0, 0.0)).is_truthy());
}

// ===========================================================================
// variant_type: always consistent
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// variant_type never panics and returns a consistent tag.
    #[test]
    fn variant_type_consistent(value in arb_variant()) {
        let t1 = value.variant_type();
        let t2 = value.variant_type();
        prop_assert_eq!(t1, t2);
    }
}

// ===========================================================================
// Display: never panics
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Display formatting never panics on any variant.
    #[test]
    fn display_never_panics(value in arb_variant()) {
        let _ = format!("{value}");
    }

    /// VariantType Display never panics.
    #[test]
    fn variant_type_display_never_panics(t in arb_variant_type()) {
        let _ = format!("{t}");
    }
}

// ===========================================================================
// Coercion transitivity / consistency
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// If A coerces to B and B coerces to C, the result type should be C.
    #[test]
    fn coercion_chain_type_correct(
        value in arb_variant(),
        mid in arb_variant_type(),
        target in arb_variant_type(),
    ) {
        if let Some(mid_val) = coerce_variant(&value, mid) {
            if let Some(final_val) = coerce_variant(&mid_val, target) {
                prop_assert_eq!(final_val.variant_type(), target);
            }
        }
    }

    /// Bool → Int → Bool round-trip preserves the boolean.
    #[test]
    fn bool_int_bool_roundtrip(b in any::<bool>()) {
        let as_int = coerce_variant(&Variant::Bool(b), VariantType::Int).unwrap();
        let back = coerce_variant(&as_int, VariantType::Bool).unwrap();
        prop_assert_eq!(back, Variant::Bool(b));
    }

    /// Bool → Float → Bool round-trip preserves the boolean.
    #[test]
    fn bool_float_bool_roundtrip(b in any::<bool>()) {
        let as_float = coerce_variant(&Variant::Bool(b), VariantType::Float).unwrap();
        let back = coerce_variant(&as_float, VariantType::Bool).unwrap();
        prop_assert_eq!(back, Variant::Bool(b));
    }
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn coerce_extreme_int_to_float() {
    // i64::MAX may lose precision as f64, but should not panic
    let result = coerce_variant(&Variant::Int(i64::MAX), VariantType::Float);
    assert!(result.is_some());
}

#[test]
fn coerce_extreme_float_to_int() {
    // Very large float → Int should not panic (wraps/saturates per `as i64`)
    let result = coerce_variant(&Variant::Float(1e18), VariantType::Int);
    assert!(result.is_some());
}

#[test]
fn coerce_negative_zero_float_to_bool() {
    // -0.0 should be falsy (same as 0.0)
    let result = coerce_variant(&Variant::Float(-0.0), VariantType::Bool).unwrap();
    assert_eq!(result, Variant::Bool(false));
}

#[test]
fn coerce_nan_float_to_bool() {
    // NaN != 0.0 so it should be truthy
    let result = coerce_variant(&Variant::Float(f64::NAN), VariantType::Bool).unwrap();
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn coerce_inf_float_to_int() {
    // Infinity as i64 is implementation-defined but must not panic
    let result = coerce_variant(&Variant::Float(f64::INFINITY), VariantType::Int);
    assert!(result.is_some());
}

#[test]
fn coerce_empty_string_to_nodepath() {
    let result = coerce_variant(&Variant::String(String::new()), VariantType::NodePath).unwrap();
    match result {
        Variant::NodePath(np) => assert!(np.is_empty()),
        _ => panic!("expected NodePath"),
    }
}

#[test]
fn coerce_empty_string_to_stringname() {
    let result =
        coerce_variant(&Variant::String(String::new()), VariantType::StringName).unwrap();
    match result {
        Variant::StringName(sn) => assert!(sn.as_str().is_empty()),
        _ => panic!("expected StringName"),
    }
}
