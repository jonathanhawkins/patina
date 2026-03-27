//! Bead pat-08kx: Close remaining core runtime subset gaps.
//!
//! Tests cover the gaps identified in the port plan (Bead Pack 03):
//! - Variant arithmetic operators (+, -, *, /, %, unary -)
//! - Variant comparison (PartialOrd)
//! - Variant convenience coercion methods (to_int, to_float, to_bool, to_string_lossy)
//! - ClassDB get_signal_list() and get_parent_class()
//!
//! Oracle: Godot 4.6.1-stable behavioral contracts.

use std::collections::HashMap;

use gdvariant::Variant;

// ===========================================================================
// 1. Variant arithmetic — Add
// ===========================================================================

#[test]
fn variant_add_int_int() {
    let result = Variant::Int(10) + Variant::Int(32);
    assert_eq!(result, Variant::Int(42));
}

#[test]
fn variant_add_float_float() {
    let result = Variant::Float(1.5) + Variant::Float(2.5);
    assert_eq!(result, Variant::Float(4.0));
}

#[test]
fn variant_add_int_float_promotes() {
    // Int + Float → Float (Godot type promotion)
    let result = Variant::Int(3) + Variant::Float(0.14);
    assert_eq!(result, Variant::Float(3.14));
}

#[test]
fn variant_add_float_int_promotes() {
    let result = Variant::Float(2.5) + Variant::Int(1);
    assert_eq!(result, Variant::Float(3.5));
}

#[test]
fn variant_add_string_concatenation() {
    let result = Variant::String("hello".into()) + Variant::String(" world".into());
    assert_eq!(result, Variant::String("hello world".into()));
}

#[test]
fn variant_add_vector2() {
    use gdcore::math::Vector2;
    let result = Variant::Vector2(Vector2::new(1.0, 2.0)) + Variant::Vector2(Vector2::new(3.0, 4.0));
    assert_eq!(result, Variant::Vector2(Vector2::new(4.0, 6.0)));
}

#[test]
fn variant_add_vector3() {
    use gdcore::math::Vector3;
    let result = Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)) + Variant::Vector3(Vector3::new(4.0, 5.0, 6.0));
    assert_eq!(result, Variant::Vector3(Vector3::new(5.0, 7.0, 9.0)));
}

#[test]
fn variant_add_incompatible_returns_nil() {
    let result = Variant::Int(1) + Variant::String("x".into());
    assert_eq!(result, Variant::Nil);
}

// ===========================================================================
// 2. Variant arithmetic — Sub
// ===========================================================================

#[test]
fn variant_sub_int_int() {
    assert_eq!(Variant::Int(10) - Variant::Int(3), Variant::Int(7));
}

#[test]
fn variant_sub_float_float() {
    assert_eq!(Variant::Float(5.5) - Variant::Float(2.5), Variant::Float(3.0));
}

#[test]
fn variant_sub_int_float_promotes() {
    assert_eq!(Variant::Int(5) - Variant::Float(1.5), Variant::Float(3.5));
}

// ===========================================================================
// 3. Variant arithmetic — Mul
// ===========================================================================

#[test]
fn variant_mul_int_int() {
    assert_eq!(Variant::Int(6) * Variant::Int(7), Variant::Int(42));
}

#[test]
fn variant_mul_float_float() {
    assert_eq!(Variant::Float(3.0) * Variant::Float(2.0), Variant::Float(6.0));
}

#[test]
fn variant_mul_int_float_promotes() {
    assert_eq!(Variant::Int(3) * Variant::Float(2.0), Variant::Float(6.0));
}

#[test]
fn variant_mul_vector2_scalar() {
    use gdcore::math::Vector2;
    let result = Variant::Vector2(Vector2::new(1.0, 2.0)) * Variant::Int(3);
    assert_eq!(result, Variant::Vector2(Vector2::new(3.0, 6.0)));
}

#[test]
fn variant_mul_scalar_vector3() {
    use gdcore::math::Vector3;
    let result = Variant::Float(2.0) * Variant::Vector3(Vector3::new(1.0, 2.0, 3.0));
    assert_eq!(result, Variant::Vector3(Vector3::new(2.0, 4.0, 6.0)));
}

// ===========================================================================
// 4. Variant arithmetic — Div
// ===========================================================================

#[test]
fn variant_div_int_int() {
    assert_eq!(Variant::Int(10) / Variant::Int(3), Variant::Int(3));
}

#[test]
fn variant_div_float_float() {
    assert_eq!(Variant::Float(10.0) / Variant::Float(4.0), Variant::Float(2.5));
}

#[test]
fn variant_div_by_zero_int_returns_nil() {
    assert_eq!(Variant::Int(10) / Variant::Int(0), Variant::Nil);
}

#[test]
fn variant_div_by_zero_float_returns_nil() {
    assert_eq!(Variant::Float(10.0) / Variant::Float(0.0), Variant::Nil);
}

// ===========================================================================
// 5. Variant arithmetic — Rem (modulo)
// ===========================================================================

#[test]
fn variant_rem_int_int() {
    assert_eq!(Variant::Int(10) % Variant::Int(3), Variant::Int(1));
}

#[test]
fn variant_rem_float_float() {
    let result = Variant::Float(10.0) % Variant::Float(3.0);
    if let Variant::Float(f) = result {
        assert!((f - 1.0).abs() < 1e-10);
    } else {
        panic!("expected Float");
    }
}

#[test]
fn variant_rem_by_zero_returns_nil() {
    assert_eq!(Variant::Int(10) % Variant::Int(0), Variant::Nil);
}

// ===========================================================================
// 6. Variant unary negation
// ===========================================================================

#[test]
fn variant_neg_int() {
    assert_eq!(-Variant::Int(42), Variant::Int(-42));
}

#[test]
fn variant_neg_float() {
    assert_eq!(-Variant::Float(3.14), Variant::Float(-3.14));
}

#[test]
fn variant_neg_vector2() {
    use gdcore::math::Vector2;
    assert_eq!(-Variant::Vector2(Vector2::new(1.0, -2.0)), Variant::Vector2(Vector2::new(-1.0, 2.0)));
}

#[test]
fn variant_neg_incompatible_returns_nil() {
    assert_eq!(-Variant::String("x".into()), Variant::Nil);
}

// ===========================================================================
// 7. Variant comparison — PartialOrd
// ===========================================================================

#[test]
fn variant_cmp_int_int() {
    assert!(Variant::Int(1) < Variant::Int(2));
    assert!(Variant::Int(2) > Variant::Int(1));
    assert!(Variant::Int(3) >= Variant::Int(3));
    assert!(Variant::Int(3) <= Variant::Int(3));
}

#[test]
fn variant_cmp_float_float() {
    assert!(Variant::Float(1.0) < Variant::Float(2.0));
    assert!(Variant::Float(2.0) > Variant::Float(1.0));
}

#[test]
fn variant_cmp_int_float_cross_type() {
    // Godot allows comparing int and float
    assert!(Variant::Int(1) < Variant::Float(1.5));
    assert!(Variant::Float(0.5) < Variant::Int(1));
}

#[test]
fn variant_cmp_string_lexicographic() {
    assert!(Variant::String("apple".into()) < Variant::String("banana".into()));
    assert!(Variant::String("z".into()) > Variant::String("a".into()));
}

#[test]
fn variant_cmp_incompatible_is_none() {
    assert!(Variant::Int(1).partial_cmp(&Variant::String("x".into())).is_none());
    assert!(Variant::String("x".into()).partial_cmp(&Variant::Bool(true)).is_none());
}

// ===========================================================================
// 8. Variant coercion methods
// ===========================================================================

#[test]
fn variant_to_int_from_int() {
    assert_eq!(Variant::Int(42).to_int(), 42);
}

#[test]
fn variant_to_int_from_float() {
    assert_eq!(Variant::Float(3.9).to_int(), 3);
}

#[test]
fn variant_to_int_from_bool() {
    assert_eq!(Variant::Bool(true).to_int(), 1);
    assert_eq!(Variant::Bool(false).to_int(), 0);
}

#[test]
fn variant_to_int_from_string() {
    assert_eq!(Variant::String("123".into()).to_int(), 123);
    assert_eq!(Variant::String("not_a_number".into()).to_int(), 0);
}

#[test]
fn variant_to_int_from_nil() {
    assert_eq!(Variant::Nil.to_int(), 0);
}

#[test]
fn variant_to_float_from_float() {
    assert_eq!(Variant::Float(3.14).to_float(), 3.14);
}

#[test]
fn variant_to_float_from_int() {
    assert_eq!(Variant::Int(42).to_float(), 42.0);
}

#[test]
fn variant_to_float_from_bool() {
    assert_eq!(Variant::Bool(true).to_float(), 1.0);
    assert_eq!(Variant::Bool(false).to_float(), 0.0);
}

#[test]
fn variant_to_float_from_string() {
    assert_eq!(Variant::String("3.14".into()).to_float(), 3.14);
    assert_eq!(Variant::String("bad".into()).to_float(), 0.0);
}

#[test]
fn variant_to_bool_matches_truthiness() {
    assert!(Variant::Int(1).to_bool());
    assert!(!Variant::Int(0).to_bool());
    assert!(Variant::String("hello".into()).to_bool());
    assert!(!Variant::String(String::new()).to_bool());
    assert!(!Variant::Nil.to_bool());
}

#[test]
fn variant_to_string_lossy() {
    assert_eq!(Variant::Int(42).to_string_lossy(), "42");
    assert_eq!(Variant::Float(3.14).to_string_lossy(), "3.14");
    assert_eq!(Variant::Bool(true).to_string_lossy(), "true");
    assert_eq!(Variant::Nil.to_string_lossy(), "<null>");
}

// ===========================================================================
// 9. ClassDB — get_signal_list
// ===========================================================================

#[test]
fn classdb_get_signal_list_own() {
    use gdobject::{
        clear_for_testing, register_class, get_signal_list,
        ClassRegistration, SignalInfo,
    };

    clear_for_testing();

    register_class(
        ClassRegistration::new("BaseNode")
            .signal(SignalInfo::new("base_signal", 0)),
    );
    register_class(
        ClassRegistration::new("DerivedNode")
            .parent("BaseNode")
            .signal(SignalInfo::new("derived_signal", 1)),
    );

    // Own only: should only return derived's signal
    let own = get_signal_list("DerivedNode", true);
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].name, "derived_signal");

    // Full chain: should include base + derived signals
    let all = get_signal_list("DerivedNode", false);
    assert_eq!(all.len(), 2);
    let names: Vec<_> = all.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"base_signal"));
    assert!(names.contains(&"derived_signal"));

    clear_for_testing();
}

#[test]
fn classdb_get_signal_list_nonexistent_class() {
    use gdobject::{clear_for_testing, get_signal_list};

    clear_for_testing();
    let signals = get_signal_list("NonExistent", false);
    assert!(signals.is_empty());
    clear_for_testing();
}

// ===========================================================================
// 10. ClassDB — get_parent_class
// ===========================================================================

#[test]
fn classdb_get_parent_class() {
    use gdobject::{
        clear_for_testing, register_class, get_parent_class,
        ClassRegistration,
    };

    clear_for_testing();

    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_class(ClassRegistration::new("Node2D").parent("Node"));

    assert_eq!(get_parent_class("Node2D"), Some("Node".into()));
    assert_eq!(get_parent_class("Node"), Some("Object".into()));
    // Root class has no parent
    assert_eq!(get_parent_class("Object"), None);
    // Non-existent class
    assert_eq!(get_parent_class("Ghost"), None);

    clear_for_testing();
}

// ===========================================================================
// 11. Extended readiness: combined operator + coercion workflow
// ===========================================================================

#[test]
fn variant_expression_workflow() {
    // Simulates: var result = (10 + 5) * 2 - 3
    let a = Variant::Int(10) + Variant::Int(5);  // 15
    let b = a * Variant::Int(2);                  // 30
    let result = b - Variant::Int(3);             // 27
    assert_eq!(result, Variant::Int(27));
    assert_eq!(result.to_float(), 27.0);
    assert!(result.to_bool());
}

#[test]
fn variant_mixed_type_expression() {
    // Simulates: var result = 10 + 2.5 (should promote to float)
    let result = Variant::Int(10) + Variant::Float(2.5);
    assert_eq!(result, Variant::Float(12.5));
    assert_eq!(result.to_int(), 12); // truncates
}

// ===========================================================================
// 12. Edge cases
// ===========================================================================

#[test]
fn variant_int_overflow_wraps() {
    // Godot wraps on overflow; Rust wrapping_add matches
    let result = Variant::Int(i64::MAX) + Variant::Int(1);
    assert_eq!(result, Variant::Int(i64::MIN));
}

#[test]
fn variant_empty_string_concat() {
    let result = Variant::String(String::new()) + Variant::String(String::new());
    assert_eq!(result, Variant::String(String::new()));
}

#[test]
fn variant_nil_arithmetic_returns_nil() {
    assert_eq!(Variant::Nil + Variant::Int(1), Variant::Nil);
    assert_eq!(Variant::Nil - Variant::Float(1.0), Variant::Nil);
    assert_eq!(Variant::Nil * Variant::Int(2), Variant::Nil);
}

#[test]
fn variant_dict_not_comparable() {
    let a = Variant::Dictionary(HashMap::new());
    let b = Variant::Dictionary(HashMap::new());
    assert!(a.partial_cmp(&b).is_none());
}
