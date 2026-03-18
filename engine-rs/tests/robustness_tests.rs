//! Robustness (fuzz-like) tests for the Patina Engine.
//!
//! Feeds garbage, edge-case, and adversarial inputs to parsers and
//! deserializers to verify they return errors gracefully and never panic.

use gdresource::TresLoader;
use gdscene::PackedScene;
use gdvariant::serialize::from_json;

/// Collect a set of adversarial strings for testing parsers.
fn garbage_strings() -> Vec<String> {
    vec![
        "".into(),
        " ".into(),
        "\0".into(),
        "\0\0\0".into(),
        "\n\n\n".into(),
        "null".into(),
        "nil".into(),
        "true".into(),
        "false".into(),
        "42".into(),
        "-1".into(),
        "3.14".into(),
        "NaN".into(),
        "Infinity".into(),
        "-Infinity".into(),
        "\"\"".into(),
        "\"hello\"".into(),
        "Vector2(".into(),
        "Vector2(,)".into(),
        "Vector2(1)".into(),
        "Vector2(1,)".into(),
        "Vector2(1, 2, 3)".into(),
        "Color()".into(),
        "Color(a, b, c, d)".into(),
        "Rect2()".into(),
        "NodePath(".into(),
        "[".into(),
        "]".into(),
        "[]".into(),
        "{".into(),
        "}".into(),
        "{}".into(),
        "{key}".into(),
        "{key:}".into(),
        "{:value}".into(),
        "[[[[[[[[[[".into(),
        "]]]]]]]]]]".into(),
        "{{{{{{{{{{".into(),
        "}}}}}}}}}}".into(),
        "[gd_scene".into(),
        "[gd_scene format=99]".into(),
        "[node name=\"\" type=\"\"]".into(),
        "[resource]".into(),
        "= value".into(),
        "key =".into(),
        "\t\t\t".into(),
        "\r\n\r\n".into(),
        "🎮🎲🎯".into(),
        "日本語テスト".into(),
        "مرحبا".into(),
        "\u{FEFF}".into(), // BOM
        "\u{200B}".into(), // zero-width space
        "a\nb\nc".into(),
        "A".repeat(100),
    ]
}

/// A 100KB string of repeated 'x'.
fn long_string() -> String {
    "x".repeat(100_000)
}

/// String with embedded null bytes throughout.
fn null_byte_string() -> String {
    let mut s = String::with_capacity(1000);
    for i in 0..100 {
        s.push('\0');
        s.push((b'a' + (i % 26)) as char);
    }
    s
}

/// Random-looking byte sequence as a UTF-8 string.
fn pseudo_random_unicode() -> String {
    // A mix of valid unicode codepoints that look like garbage.
    (0..500)
        .map(|i| char::from_u32(((i * 137 + 42) % 0x10000) as u32).unwrap_or('?'))
        .collect()
}

// ---------------------------------------------------------------------------
// parse_variant_value: must never panic
// ---------------------------------------------------------------------------

#[test]
fn robustness_parse_variant_value_garbage_strings() {
    for input in &garbage_strings() {
        // We don't care about the result, only that it doesn't panic.
        let _ = gdresource::parse_variant_value(input);
    }
}

#[test]
fn robustness_parse_variant_value_long_string() {
    let long = long_string();
    let _ = gdresource::parse_variant_value(&long);
}

#[test]
fn robustness_parse_variant_value_null_bytes() {
    let s = null_byte_string();
    let _ = gdresource::parse_variant_value(&s);
}

#[test]
fn robustness_parse_variant_value_unicode_garbage() {
    let s = pseudo_random_unicode();
    let _ = gdresource::parse_variant_value(&s);
}

// ---------------------------------------------------------------------------
// PackedScene::from_tscn: must return Err, not panic
// ---------------------------------------------------------------------------

#[test]
fn robustness_packed_scene_garbage_strings() {
    for input in &garbage_strings() {
        let result = PackedScene::from_tscn(input);
        // Should be Err (or trivially Ok for empty scenes), but never panic.
        let _ = result;
    }
}

#[test]
fn robustness_packed_scene_long_string() {
    let long = long_string();
    let _ = PackedScene::from_tscn(&long);
}

#[test]
fn robustness_packed_scene_null_bytes() {
    let s = null_byte_string();
    let _ = PackedScene::from_tscn(&s);
}

#[test]
fn robustness_packed_scene_unicode_garbage() {
    let s = pseudo_random_unicode();
    let _ = PackedScene::from_tscn(&s);
}

// ---------------------------------------------------------------------------
// TresLoader: must return Err, not panic
// ---------------------------------------------------------------------------

#[test]
fn robustness_tres_loader_garbage_strings() {
    let loader = TresLoader::new();
    for input in &garbage_strings() {
        let result = loader.parse_str(input, "garbage://test.tres");
        let _ = result;
    }
}

#[test]
fn robustness_tres_loader_long_string() {
    let loader = TresLoader::new();
    let long = long_string();
    let _ = loader.parse_str(&long, "garbage://long.tres");
}

#[test]
fn robustness_tres_loader_null_bytes() {
    let loader = TresLoader::new();
    let s = null_byte_string();
    let _ = loader.parse_str(&s, "garbage://null.tres");
}

#[test]
fn robustness_tres_loader_unicode_garbage() {
    let loader = TresLoader::new();
    let s = pseudo_random_unicode();
    let _ = loader.parse_str(&s, "garbage://unicode.tres");
}

// ---------------------------------------------------------------------------
// from_json: must return None, not panic
// ---------------------------------------------------------------------------

#[test]
fn robustness_from_json_garbage_json_values() {
    let garbage_values = vec![
        serde_json::json!(null),
        serde_json::json!(42),
        serde_json::json!(3.14),
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!("hello"),
        serde_json::json!([]),
        serde_json::json!([1, 2, 3]),
        serde_json::json!({}),
        serde_json::json!({"type": null}),
        serde_json::json!({"type": 42}),
        serde_json::json!({"type": true}),
        serde_json::json!({"type": "Unknown"}),
        serde_json::json!({"type": "Int"}), // missing value
        serde_json::json!({"type": "Int", "value": "not_a_number"}),
        serde_json::json!({"type": "Bool", "value": 42}),
        serde_json::json!({"type": "Vector2", "value": []}),
        serde_json::json!({"type": "Vector2", "value": [1]}),
        serde_json::json!({"type": "Vector2", "value": [1, 2, 3]}),
        serde_json::json!({"type": "Color", "value": [1]}),
        serde_json::json!({"type": "Array", "value": "not_array"}),
        serde_json::json!({"type": "Dictionary", "value": "not_dict"}),
        serde_json::json!({"type": "Rect2", "value": "bad"}),
        serde_json::json!({"type": "Basis", "value": {}}),
        serde_json::json!({"type": "AABB", "value": {}}),
    ];

    for val in &garbage_values {
        // Must return None, not panic.
        let result = from_json(val);
        assert!(result.is_none(), "Expected None for garbage input: {val}");
    }
}

#[test]
fn robustness_from_json_random_byte_strings() {
    // Parse random-looking strings as JSON first, then feed to from_json.
    let test_strings = vec![
        "{}",
        "{\"type\": \"\"}",
        "{\"type\": \"Int\", \"value\": null}",
        "{\"type\": \"Float\", \"value\": \"nan\"}",
        "{\"type\": \"String\", \"value\": null}",
        "[]",
        "null",
        "\"just a string\"",
        "0",
        "-0",
    ];

    for s in test_strings {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            let _ = from_json(&val);
        }
    }
}

// ---------------------------------------------------------------------------
// Edge cases: empty and whitespace-only inputs
// ---------------------------------------------------------------------------

#[test]
fn robustness_empty_inputs() {
    let _ = gdresource::parse_variant_value("");
    let _ = PackedScene::from_tscn("");

    let loader = TresLoader::new();
    let _ = loader.parse_str("", "empty://test.tres");

    assert!(from_json(&serde_json::json!(null)).is_none());
}

#[test]
fn robustness_whitespace_only() {
    let whitespace = "   \t\n\r  ";
    let _ = gdresource::parse_variant_value(whitespace);
    let _ = PackedScene::from_tscn(whitespace);

    let loader = TresLoader::new();
    let _ = loader.parse_str(whitespace, "ws://test.tres");
}
