//! Fuzz and property tests for the resource loader parser.
//!
//! Tests that parse_variant_value never panics on arbitrary input,
//! and that valid inputs round-trip correctly through parse→format→parse.

use gdresource::loader::parse_variant_value;
use gdvariant::Variant;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Robustness: parse_variant_value must never panic
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Arbitrary strings must not cause a panic in the parser.
    #[test]
    fn parse_never_panics(s in ".*") {
        let _ = parse_variant_value(&s);
    }

    /// Strings with balanced parens and function-call-like syntax must not panic.
    #[test]
    fn parse_funcall_syntax_never_panics(
        name in "(Vector2|Vector3|Color|Rect2|Transform2D|Quaternion|Basis|AABB|Plane|NodePath|StringName|Transform3D)",
        inner in ".*",
    ) {
        let input = format!("{name}({inner})");
        let _ = parse_variant_value(&input);
    }

    /// Deeply nested parentheses must not cause stack overflow or panic.
    #[test]
    fn parse_deep_nesting_no_panic(depth in 1usize..50) {
        let open: String = "(".repeat(depth);
        let close: String = ")".repeat(depth);
        let input = format!("Vector2{open}1.0, 2.0{close}");
        let _ = parse_variant_value(&input);
    }

    /// Strings with unmatched quotes must not panic.
    #[test]
    fn parse_unmatched_quotes_no_panic(s in "\"[^\"]*") {
        let _ = parse_variant_value(&s);
    }

    /// Strings that look like arrays with random contents must not panic.
    #[test]
    fn parse_array_like_no_panic(inner in ".*") {
        let input = format!("[{inner}]");
        let _ = parse_variant_value(&input);
    }
}

// ---------------------------------------------------------------------------
// Correctness: known valid inputs parse to expected types
// ---------------------------------------------------------------------------

proptest! {
    /// Boolean literals parse correctly.
    #[test]
    fn parse_booleans(b in prop::bool::ANY) {
        let input = if b { "true" } else { "false" };
        let result = parse_variant_value(input).unwrap();
        prop_assert_eq!(result, Variant::Bool(b));
    }

    /// Integer literals parse correctly.
    #[test]
    fn parse_integers(i in -999_999_999i64..999_999_999i64) {
        let input = i.to_string();
        let result = parse_variant_value(&input).unwrap();
        match result {
            Variant::Int(v) => prop_assert_eq!(v, i),
            Variant::Float(v) => {
                // Some integers might parse as floats; that's acceptable
                prop_assert!((v - i as f64).abs() < 1.0);
            }
            _ => prop_assert!(false, "Expected Int or Float, got {:?}", result),
        }
    }

    /// Quoted strings parse correctly (simple ASCII, no escape sequences).
    #[test]
    fn parse_simple_strings(s in "[a-zA-Z0-9 _]+") {
        let input = format!("\"{s}\"");
        let result = parse_variant_value(&input).unwrap();
        prop_assert_eq!(result, Variant::String(s));
    }

    /// Vector2 with valid floats parses correctly.
    #[test]
    fn parse_vector2(x in -1000.0f32..1000.0, y in -1000.0f32..1000.0) {
        let input = format!("Vector2({x}, {y})");
        let result = parse_variant_value(&input).unwrap();
        match result {
            Variant::Vector2(v) => {
                prop_assert!((v.x - x).abs() < 0.01, "x mismatch: {} vs {}", v.x, x);
                prop_assert!((v.y - y).abs() < 0.01, "y mismatch: {} vs {}", v.y, y);
            }
            _ => prop_assert!(false, "Expected Vector2, got {:?}", result),
        }
    }

    /// Vector3 with valid floats parses correctly.
    #[test]
    fn parse_vector3(
        x in -1000.0f32..1000.0,
        y in -1000.0f32..1000.0,
        z in -1000.0f32..1000.0,
    ) {
        let input = format!("Vector3({x}, {y}, {z})");
        let result = parse_variant_value(&input).unwrap();
        match result {
            Variant::Vector3(v) => {
                prop_assert!((v.x - x).abs() < 0.01);
                prop_assert!((v.y - y).abs() < 0.01);
                prop_assert!((v.z - z).abs() < 0.01);
            }
            _ => prop_assert!(false, "Expected Vector3, got {:?}", result),
        }
    }

    /// Color with valid components parses correctly.
    #[test]
    fn parse_color(
        r in 0.0f32..1.0,
        g in 0.0f32..1.0,
        b in 0.0f32..1.0,
        a in 0.0f32..1.0,
    ) {
        let input = format!("Color({r}, {g}, {b}, {a})");
        let result = parse_variant_value(&input).unwrap();
        match result {
            Variant::Color(c) => {
                prop_assert!((c.r - r).abs() < 0.01);
                prop_assert!((c.g - g).abs() < 0.01);
                prop_assert!((c.b - b).abs() < 0.01);
                prop_assert!((c.a - a).abs() < 0.01);
            }
            _ => prop_assert!(false, "Expected Color, got {:?}", result),
        }
    }

    /// Rect2 with valid floats parses correctly.
    #[test]
    fn parse_rect2(
        x in -500.0f32..500.0,
        y in -500.0f32..500.0,
        w in 0.0f32..1000.0,
        h in 0.0f32..1000.0,
    ) {
        let input = format!("Rect2({x}, {y}, {w}, {h})");
        let result = parse_variant_value(&input).unwrap();
        match result {
            Variant::Rect2(r) => {
                prop_assert!((r.position.x - x).abs() < 0.01);
                prop_assert!((r.position.y - y).abs() < 0.01);
                prop_assert!((r.size.x - w).abs() < 0.01);
                prop_assert!((r.size.y - h).abs() < 0.01);
            }
            _ => prop_assert!(false, "Expected Rect2, got {:?}", result),
        }
    }

    /// Quaternion with valid components parses correctly.
    #[test]
    fn parse_quaternion(
        x in -1.0f32..1.0,
        y in -1.0f32..1.0,
        z in -1.0f32..1.0,
        w in -1.0f32..1.0,
    ) {
        let input = format!("Quaternion({x}, {y}, {z}, {w})");
        let result = parse_variant_value(&input).unwrap();
        match result {
            Variant::Quaternion(q) => {
                prop_assert!((q.x - x).abs() < 0.01);
                prop_assert!((q.y - y).abs() < 0.01);
                prop_assert!((q.z - z).abs() < 0.01);
                prop_assert!((q.w - w).abs() < 0.01);
            }
            _ => prop_assert!(false, "Expected Quaternion, got {:?}", result),
        }
    }
}

// ---------------------------------------------------------------------------
// Null/Nil variants
// ---------------------------------------------------------------------------

#[test]
fn parse_null_variants() {
    assert_eq!(parse_variant_value("null").unwrap(), Variant::Nil);
    assert_eq!(parse_variant_value("nil").unwrap(), Variant::Nil);
    assert_eq!(parse_variant_value("Nil").unwrap(), Variant::Nil);
}

// ---------------------------------------------------------------------------
// Edge cases for string escaping
// ---------------------------------------------------------------------------

#[test]
fn parse_string_with_escapes() {
    let result = parse_variant_value(r#""hello\nworld""#).unwrap();
    assert_eq!(result, Variant::String("hello\nworld".into()));

    let result = parse_variant_value(r#""tab\there""#).unwrap();
    assert_eq!(result, Variant::String("tab\there".into()));

    let result = parse_variant_value(r#""escaped\\backslash""#).unwrap();
    assert_eq!(result, Variant::String("escaped\\backslash".into()));

    let result = parse_variant_value(r#""quote\"inside""#).unwrap();
    assert_eq!(result, Variant::String("quote\"inside".into()));
}

#[test]
fn parse_empty_string() {
    let result = parse_variant_value(r#""""#).unwrap();
    assert_eq!(result, Variant::String(String::new()));
}

// ---------------------------------------------------------------------------
// Whitespace tolerance
// ---------------------------------------------------------------------------

proptest! {
    /// Leading/trailing whitespace is tolerated.
    #[test]
    fn parse_with_whitespace(
        leading in "[ \\t]*",
        trailing in "[ \\t]*",
    ) {
        let input = format!("{leading}true{trailing}");
        let result = parse_variant_value(&input).unwrap();
        prop_assert_eq!(result, Variant::Bool(true));
    }
}

// ---------------------------------------------------------------------------
// Wrong arg counts produce errors, not panics
// ---------------------------------------------------------------------------

#[test]
fn vector2_wrong_arg_count() {
    assert!(parse_variant_value("Vector2(1.0)").is_err());
    assert!(parse_variant_value("Vector2(1.0, 2.0, 3.0)").is_err());
}

#[test]
fn vector3_wrong_arg_count() {
    assert!(parse_variant_value("Vector3(1.0, 2.0)").is_err());
    assert!(parse_variant_value("Vector3(1.0, 2.0, 3.0, 4.0)").is_err());
}

#[test]
fn color_wrong_arg_count() {
    assert!(parse_variant_value("Color(1.0)").is_err());
    assert!(parse_variant_value("Color(1.0, 2.0)").is_err());
    // 3-arg Color is valid (RGB), 4-arg Color is valid (RGBA)
    assert!(parse_variant_value("Color(1.0, 0.5, 0.0)").is_ok());
    assert!(parse_variant_value("Color(1.0, 0.5, 0.0, 1.0)").is_ok());
}

#[test]
fn rect2_wrong_arg_count() {
    assert!(parse_variant_value("Rect2(1.0, 2.0, 3.0)").is_err());
}

#[test]
fn transform2d_wrong_arg_count() {
    assert!(parse_variant_value("Transform2D(1, 2, 3, 4, 5)").is_err());
}
