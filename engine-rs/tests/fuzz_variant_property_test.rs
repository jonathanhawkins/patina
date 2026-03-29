//! Fuzz and property tests for the Variant type system.
//!
//! Tests cover type coercion round-trips, arithmetic properties,
//! NaN/Inf edge cases, and CallableRef bind/unbind resolution.

use gdvariant::Variant;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies for generating arbitrary Variant values
// ---------------------------------------------------------------------------

fn arb_int() -> impl Strategy<Value = i64> {
    prop::num::i64::ANY
}

fn arb_float() -> impl Strategy<Value = f64> {
    prop_oneof![
        prop::num::f64::ANY,
        Just(f64::NAN),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(0.0),
        Just(-0.0),
    ]
}

fn arb_scalar_variant() -> impl Strategy<Value = Variant> {
    prop_oneof![
        Just(Variant::Nil),
        any::<bool>().prop_map(Variant::Bool),
        arb_int().prop_map(Variant::Int),
        arb_float().prop_map(Variant::Float),
        ".*".prop_map(|s| Variant::String(s)),
    ]
}

fn _arb_numeric_variant() -> impl Strategy<Value = Variant> {
    prop_oneof![
        arb_int().prop_map(Variant::Int),
        // Exclude NaN/Inf for arithmetic properties that require finite values
        prop::num::f64::NORMAL.prop_map(Variant::Float),
    ]
}

// ---------------------------------------------------------------------------
// Type coercion properties
// ---------------------------------------------------------------------------

proptest! {
    /// to_int on an Int variant is identity.
    #[test]
    fn int_coercion_identity(i in arb_int()) {
        let v = Variant::Int(i);
        prop_assert_eq!(v.to_int(), i);
    }

    /// to_float on a Float variant is identity (for finite values).
    #[test]
    fn float_coercion_identity(f in prop::num::f64::NORMAL) {
        let v = Variant::Float(f);
        prop_assert!((v.to_float() - f).abs() < f64::EPSILON || (f == 0.0 && v.to_float() == 0.0));
    }

    /// Bool → Int: false=0, true=1.
    #[test]
    fn bool_to_int(b in any::<bool>()) {
        let v = Variant::Bool(b);
        prop_assert_eq!(v.to_int(), if b { 1 } else { 0 });
    }

    /// Bool → Float: false=0.0, true=1.0.
    #[test]
    fn bool_to_float(b in any::<bool>()) {
        let v = Variant::Bool(b);
        let expected = if b { 1.0 } else { 0.0 };
        prop_assert_eq!(v.to_float(), expected);
    }

    /// to_bool matches is_truthy.
    #[test]
    fn to_bool_matches_is_truthy(v in arb_scalar_variant()) {
        prop_assert_eq!(v.to_bool(), v.is_truthy());
    }

    /// Nil, non-numeric types all coerce to 0 for to_int.
    #[test]
    fn nil_to_int_is_zero(_dummy in 0..1u8) {
        prop_assert_eq!(Variant::Nil.to_int(), 0);
    }

    /// Nil, non-numeric types all coerce to 0.0 for to_float.
    #[test]
    fn nil_to_float_is_zero(_dummy in 0..1u8) {
        prop_assert_eq!(Variant::Nil.to_float(), 0.0);
    }

    /// String containing valid integer parses correctly via to_int.
    #[test]
    fn string_to_int_valid(i in -1_000_000i64..1_000_000i64) {
        let v = Variant::String(i.to_string());
        prop_assert_eq!(v.to_int(), i);
    }

    /// String containing non-numeric text returns 0 for to_int.
    #[test]
    fn string_to_int_invalid(s in "[a-zA-Z]+") {
        let v = Variant::String(s);
        prop_assert_eq!(v.to_int(), 0);
    }

    /// Float → Int truncates (not rounds).
    #[test]
    fn float_to_int_truncates(f in -1e15f64..1e15f64) {
        let v = Variant::Float(f);
        prop_assert_eq!(v.to_int(), f as i64);
    }
}

// ---------------------------------------------------------------------------
// Arithmetic properties
// ---------------------------------------------------------------------------

proptest! {
    /// Int addition is wrapping (no panic on overflow).
    #[test]
    fn int_add_wrapping(a in arb_int(), b in arb_int()) {
        let result = Variant::Int(a) + Variant::Int(b);
        prop_assert_eq!(result, Variant::Int(a.wrapping_add(b)));
    }

    /// Int subtraction is wrapping.
    #[test]
    fn int_sub_wrapping(a in arb_int(), b in arb_int()) {
        let result = Variant::Int(a) - Variant::Int(b);
        prop_assert_eq!(result, Variant::Int(a.wrapping_sub(b)));
    }

    /// Int multiplication is wrapping.
    #[test]
    fn int_mul_wrapping(a in arb_int(), b in arb_int()) {
        let result = Variant::Int(a) * Variant::Int(b);
        prop_assert_eq!(result, Variant::Int(a.wrapping_mul(b)));
    }

    /// Int division by zero yields Nil (not panic).
    #[test]
    fn int_div_by_zero(a in arb_int()) {
        let result = Variant::Int(a) / Variant::Int(0);
        prop_assert_eq!(result, Variant::Nil);
    }

    /// Int modulo by zero yields Nil.
    #[test]
    fn int_rem_by_zero(a in arb_int()) {
        let result = Variant::Int(a) % Variant::Int(0);
        prop_assert_eq!(result, Variant::Nil);
    }

    /// Float division by zero yields Nil (not Inf/NaN).
    #[test]
    fn float_div_by_zero(a in prop::num::f64::NORMAL) {
        let result = Variant::Float(a) / Variant::Float(0.0);
        prop_assert_eq!(result, Variant::Nil);
    }

    /// Int + Float promotes to Float.
    #[test]
    fn int_float_promotion(i in -1_000_000i64..1_000_000i64, f in -1e6f64..1e6f64) {
        let result = Variant::Int(i) + Variant::Float(f);
        match result {
            Variant::Float(v) => {
                let expected = i as f64 + f;
                prop_assert!((v - expected).abs() < 1e-10,
                    "Expected {expected}, got {v}");
            }
            _ => prop_assert!(false, "Expected Float, got {:?}", result),
        }
    }

    /// Negation of Int is correct.
    #[test]
    fn int_negation(i in arb_int()) {
        let result = -Variant::Int(i);
        prop_assert_eq!(result, Variant::Int(-i));
    }

    /// Negation of Float is correct.
    #[test]
    fn float_negation(f in prop::num::f64::NORMAL) {
        let result = -Variant::Float(f);
        match result {
            Variant::Float(v) => prop_assert_eq!(v, -f),
            _ => prop_assert!(false, "Expected Float"),
        }
    }

    /// String concatenation via Add.
    #[test]
    fn string_concat(a in ".*", b in ".*") {
        let result = Variant::String(a.clone()) + Variant::String(b.clone());
        let expected = format!("{a}{b}");
        prop_assert_eq!(result, Variant::String(expected));
    }

    /// Unsupported type combinations yield Nil for all arithmetic ops.
    #[test]
    fn unsupported_types_yield_nil(s in ".*") {
        let sv = Variant::String(s);
        let iv = Variant::Int(42);
        // String + Int → Nil (not String, not Int)
        let add_result = sv.clone() + iv.clone();
        prop_assert_eq!(add_result, Variant::Nil);
        // String - Int → Nil
        let sub_result = sv.clone() - iv.clone();
        prop_assert_eq!(sub_result, Variant::Nil);
        // String * Int → Nil
        let mul_result = sv.clone() * iv.clone();
        prop_assert_eq!(mul_result, Variant::Nil);
    }
}

// ---------------------------------------------------------------------------
// Comparison / ordering properties
// ---------------------------------------------------------------------------

proptest! {
    /// Int comparison is reflexive.
    #[test]
    fn int_comparison_reflexive(i in arb_int()) {
        let v = Variant::Int(i);
        prop_assert!(v == v.clone());
        prop_assert!(v.partial_cmp(&v.clone()) == Some(std::cmp::Ordering::Equal));
    }

    /// Int comparison is transitive.
    #[test]
    fn int_comparison_transitive(a in -1000i64..1000, b in -1000i64..1000, c in -1000i64..1000) {
        let va = Variant::Int(a);
        let vb = Variant::Int(b);
        let vc = Variant::Int(c);
        if va <= vb && vb <= vc {
            prop_assert!(va <= vc);
        }
    }

    /// Int-Float cross-type comparison works.
    #[test]
    fn int_float_cross_comparison(i in -1_000_000i64..1_000_000i64) {
        let vi = Variant::Int(i);
        let vf = Variant::Float(i as f64);
        prop_assert!(vi.partial_cmp(&vf) == Some(std::cmp::Ordering::Equal),
            "Int({i}) should equal Float({}.0)", i);
    }

    /// Incomparable types return None for partial_cmp.
    #[test]
    fn incomparable_types_return_none(i in arb_int(), s in ".*") {
        let vi = Variant::Int(i);
        let vs = Variant::String(s);
        prop_assert_eq!(vi.partial_cmp(&vs), None);
    }
}

// ---------------------------------------------------------------------------
// NaN / special float handling
// ---------------------------------------------------------------------------

#[test]
fn nan_is_truthy() {
    // NaN is nonzero, so it's truthy in Godot
    assert!(Variant::Float(f64::NAN).is_truthy());
}

#[test]
fn nan_to_int_is_zero() {
    // NaN as i64 is 0 in Rust
    assert_eq!(Variant::Float(f64::NAN).to_int(), 0);
}

#[test]
fn inf_is_truthy() {
    assert!(Variant::Float(f64::INFINITY).is_truthy());
    assert!(Variant::Float(f64::NEG_INFINITY).is_truthy());
}

#[test]
fn nan_not_equal_to_self() {
    // Variant::Float uses f64 PartialEq, so NaN != NaN
    let v = Variant::Float(f64::NAN);
    assert_ne!(v, v.clone());
}

#[test]
fn nan_comparison_is_none() {
    let v = Variant::Float(f64::NAN);
    assert_eq!(v.partial_cmp(&v), None);
}

// ---------------------------------------------------------------------------
// Variant type tag stability
// ---------------------------------------------------------------------------

proptest! {
    /// variant_type always returns the correct tag.
    #[test]
    fn variant_type_matches(v in arb_scalar_variant()) {
        let tag = v.variant_type();
        match v {
            Variant::Nil => prop_assert_eq!(tag, gdvariant::VariantType::Nil),
            Variant::Bool(_) => prop_assert_eq!(tag, gdvariant::VariantType::Bool),
            Variant::Int(_) => prop_assert_eq!(tag, gdvariant::VariantType::Int),
            Variant::Float(_) => prop_assert_eq!(tag, gdvariant::VariantType::Float),
            Variant::String(_) => prop_assert_eq!(tag, gdvariant::VariantType::String),
            _ => {} // covered above
        }
    }
}

// ---------------------------------------------------------------------------
// CallableRef bind/unbind property tests
// ---------------------------------------------------------------------------

proptest! {
    /// Bind appends arguments after call-site args.
    #[test]
    fn callable_bind_appends(
        call_args_len in 0usize..5,
        bound_args_len in 0usize..5,
    ) {
        use gdvariant::CallableRef;

        let call_args: Vec<Variant> = (0..call_args_len as i64)
            .map(|i| Variant::Int(i))
            .collect();
        let bound_args: Vec<Variant> = (100..100 + bound_args_len as i64)
            .map(|i| Variant::Int(i))
            .collect();

        let callable = CallableRef::Bound {
            inner: Box::new(CallableRef::Method {
                target_id: 1,
                method: "test".into(),
            }),
            bound_args: bound_args.clone(),
        };

        let resolved = callable.resolve_args(&call_args);
        prop_assert_eq!(resolved.len(), call_args_len + bound_args_len);
        // First args match call args
        for (i, arg) in call_args.iter().enumerate() {
            prop_assert_eq!(&resolved[i], arg);
        }
        // Trailing args match bound args
        for (i, arg) in bound_args.iter().enumerate() {
            prop_assert_eq!(&resolved[call_args_len + i], arg);
        }
    }

    /// Unbind drops trailing arguments.
    #[test]
    fn callable_unbind_drops_trailing(
        call_args_len in 0usize..10,
        unbind_count in 0usize..10,
    ) {
        use gdvariant::CallableRef;

        let call_args: Vec<Variant> = (0..call_args_len as i64)
            .map(|i| Variant::Int(i))
            .collect();

        let callable = CallableRef::Unbound {
            inner: Box::new(CallableRef::Method {
                target_id: 1,
                method: "test".into(),
            }),
            unbind_count,
        };

        let resolved = callable.resolve_args(&call_args);
        let expected_len = call_args_len.saturating_sub(unbind_count);
        prop_assert_eq!(resolved.len(), expected_len);
        // Remaining args are the first N of call_args
        for (i, arg) in resolved.iter().enumerate() {
            prop_assert_eq!(arg, &call_args[i]);
        }
    }

    /// inner_callable always resolves to a Method or Lambda.
    #[test]
    fn inner_callable_unwraps(depth in 1usize..6) {
        use gdvariant::CallableRef;

        let base = CallableRef::Method {
            target_id: 42,
            method: "target_method".into(),
        };
        let mut wrapped: CallableRef = base;
        for _ in 0..depth {
            wrapped = CallableRef::Bound {
                inner: Box::new(wrapped),
                bound_args: vec![Variant::Int(1)],
            };
        }

        let inner = wrapped.inner_callable();
        match inner {
            CallableRef::Method { target_id, method } => {
                prop_assert_eq!(*target_id, 42);
                prop_assert_eq!(method, "target_method");
            }
            _ => prop_assert!(false, "Expected Method, got {:?}", inner),
        }
    }
}
