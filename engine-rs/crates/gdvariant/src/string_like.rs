//! Godot's `StringLikeVariantComparator` — the key/element equality rule that
//! `Dictionary` and `Array.find`/`rfind`/`count`/`has` use, which is **not** the
//! `==` operator (pat-mgixk.12).
//!
//! Two rules separate it from `==` (`Variant::evaluate(OP_EQUAL)`), and Patina
//! previously had both of them inverted:
//!
//! 1. **Exact type match, so `int` and `float` keys are distinct.**
//!    `Variant::hash_compare` (`core/variant/variant.cpp:3172-3175`) opens with
//!    `if (type != p_variant.type) { return false; }`. There is no numeric
//!    promotion, so `{1: "x"}.has(1.0)` and `[1].has(1.0)` are **false** in
//!    Godot even though `1 == 1.0` is true.
//! 2. **`String` and `StringName` compare equal by text.**
//!    `StringLikeVariantComparator::compare` (`variant.cpp:3396-3407`) falls
//!    back to a text comparison across the two kinds, so `["a"].has(&"a")` and
//!    `{"a": 1}.has(&"a")` are **true**. This is the rule that matters in
//!    practice: a `.tscn` writes `&"walk"` and the loader hands the script a
//!    `Variant::StringName`, so without it every dictionary and array lookup
//!    against a scene-authored name silently missed.
//!
//! Godot wires the comparator in at `dictionary.cpp:50` (the key comparator of
//! the backing `HashMap`) and `array.cpp:383/437/489` (`find`, `rfind`,
//! `count`; `has` is `find() != -1`).
//!
//! ⚠️ `Array.erase` deliberately does **not** use it: `array.cpp:347` goes
//! through `Vector::erase`, which compares with `Variant::operator==` — the
//! `==` operator, numeric promotion included. Keep `erase` on the `==` path.

use crate::variant::Variant;

/// Godot's `Variant::hash_compare` — structural equality with **no** cross-type
/// promotion (`variant.cpp:3172`).
///
/// `Variant`'s derived `PartialEq` is already structural and answers `false`
/// for two different kinds, which *is* `hash_compare`'s opening
/// `type != p_variant.type` test. The one place it diverges is `NaN`: Godot's
/// `hash_compare_scalar_base` (`variant.cpp:3134-3135`) treats `NaN == NaN` as
/// true so that a `NaN` key can be looked up again, while Rust's `f64: PartialEq`
/// does not.
pub fn hash_compare(a: &Variant, b: &Variant) -> bool {
    match (a, b) {
        (Variant::Float(x), Variant::Float(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

/// Godot's `StringLikeVariantComparator::compare` (`variant.cpp:3396`).
///
/// [`hash_compare`] first, then the `String` ⇄ `StringName` text fallback.
pub fn string_like_eq(a: &Variant, b: &Variant) -> bool {
    if hash_compare(a, b) {
        return true;
    }
    match (a, b) {
        (Variant::String(s), Variant::StringName(sn))
        | (Variant::StringName(sn), Variant::String(s)) => s.as_str() == sn.as_str(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::string_name::StringName;

    fn sn(s: &str) -> Variant {
        Variant::StringName(StringName::new(s))
    }

    fn st(s: &str) -> Variant {
        Variant::String(s.to_string())
    }

    #[test]
    fn string_and_string_name_compare_equal_both_ways() {
        assert!(string_like_eq(&st("walk"), &sn("walk")));
        assert!(string_like_eq(&sn("walk"), &st("walk")));
        assert!(!string_like_eq(&st("walk"), &sn("run")));
        assert!(!string_like_eq(&sn("walk"), &st("run")));
    }

    #[test]
    fn int_and_float_are_not_interchangeable() {
        // Godot's hash_compare bails on `type != p_variant.type`, so these are
        // distinct even though `1 == 1.0` under the `==` operator.
        assert!(!string_like_eq(&Variant::Int(1), &Variant::Float(1.0)));
        assert!(!string_like_eq(&Variant::Float(1.0), &Variant::Int(1)));
        assert!(string_like_eq(&Variant::Int(1), &Variant::Int(1)));
        assert!(string_like_eq(&Variant::Float(1.0), &Variant::Float(1.0)));
    }

    #[test]
    fn nan_equals_itself_so_a_nan_key_stays_findable() {
        let nan = Variant::Float(f64::NAN);
        assert!(hash_compare(&nan, &Variant::Float(f64::NAN)));
        assert!(string_like_eq(&nan, &Variant::Float(f64::NAN)));
        // Still not equal to a number.
        assert!(!string_like_eq(&nan, &Variant::Float(0.0)));
    }

    #[test]
    fn unrelated_kinds_stay_unequal() {
        assert!(!string_like_eq(&st("1"), &Variant::Int(1)));
        assert!(!string_like_eq(&Variant::Nil, &Variant::Bool(false)));
        assert!(!string_like_eq(&sn("1"), &Variant::Int(1)));
    }

    #[test]
    fn structural_kinds_still_compare() {
        use gdcore::math::Vector2;
        assert!(string_like_eq(
            &Variant::Vector2(Vector2::new(1.0, 2.0)),
            &Variant::Vector2(Vector2::new(1.0, 2.0))
        ));
        assert!(!string_like_eq(
            &Variant::Vector2(Vector2::new(1.0, 2.0)),
            &Variant::Vector2(Vector2::new(2.0, 1.0))
        ));
    }
}
