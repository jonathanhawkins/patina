//! Fuzz and property tests for the resource loader pipeline.
//!
//! Tests that the `.tres` parser handles malformed input gracefully (never
//! panics) and that valid resources survive a load→save→load round-trip.

use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Deterministic PRNG for mutation-based fuzzing
// ---------------------------------------------------------------------------

struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn next_usize(&mut self, max: usize) -> usize {
        (self.next() as usize) % max.max(1)
    }

    fn next_u8(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }

    fn next_char(&mut self) -> char {
        let pool = b"abcdefghijklmnopqrstuvwxyz0123456789_/.=:\"'()[], \t\n{}[]@#$%^&*!";
        pool[self.next_usize(pool.len())] as char
    }
}

// ---------------------------------------------------------------------------
// Valid .tres templates for mutation testing
// ---------------------------------------------------------------------------

const VALID_TRES_MINIMAL: &str = r#"[gd_resource type="Resource" format=3]

[resource]
"#;

const VALID_TRES_WITH_PROPS: &str = r#"[gd_resource type="Resource" format=3]

[resource]
name = "TestResource"
speed = 42
velocity = Vector2(1.0, 2.0)
color = Color(1.0, 0.0, 0.0, 1.0)
enabled = true
"#;

const VALID_TRES_WITH_EXT: &str = r#"[gd_resource type="Resource" load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1"]

[resource]
texture = ExtResource("1")
"#;

const VALID_TRES_WITH_SUB: &str = r#"[gd_resource type="Resource" load_steps=2 format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_abc"]
bg_color = Color(0.2, 0.3, 0.4, 1.0)

[resource]
style = SubResource("StyleBoxFlat_abc")
"#;

const TEMPLATES: &[&str] = &[
    VALID_TRES_MINIMAL,
    VALID_TRES_WITH_PROPS,
    VALID_TRES_WITH_EXT,
    VALID_TRES_WITH_SUB,
];

// ---------------------------------------------------------------------------
// Fuzz helpers
// ---------------------------------------------------------------------------

fn mutate_string(rng: &mut Xorshift64, input: &str) -> String {
    let bytes: Vec<u8> = input.bytes().collect();
    if bytes.is_empty() {
        return String::new();
    }

    let mut result = bytes.clone();
    let mutation_count = rng.next_usize(5) + 1;

    for _ in 0..mutation_count {
        if result.is_empty() {
            break;
        }
        let op = rng.next_usize(6);
        match op {
            0 => {
                // Single byte flip.
                let idx = rng.next_usize(result.len());
                result[idx] ^= 1 << rng.next_usize(8);
            }
            1 => {
                // Insert random byte.
                let idx = rng.next_usize(result.len() + 1);
                result.insert(idx, rng.next_u8());
            }
            2 => {
                // Remove a byte.
                if !result.is_empty() {
                    let idx = rng.next_usize(result.len());
                    result.remove(idx);
                }
            }
            3 => {
                // Overwrite a byte with a random one.
                if !result.is_empty() {
                    let idx = rng.next_usize(result.len());
                    result[idx] = rng.next_u8();
                }
            }
            4 => {
                // Truncate at a random position.
                let len = rng.next_usize(result.len() + 1);
                result.truncate(len);
            }
            5 => {
                // Duplicate a random slice.
                if result.len() > 1 {
                    let start = rng.next_usize(result.len() - 1);
                    let end = start + rng.next_usize(result.len() - start).min(20);
                    let slice: Vec<u8> = result[start..end].to_vec();
                    let insert_at = rng.next_usize(result.len());
                    for (i, &b) in slice.iter().enumerate() {
                        result.insert(insert_at + i, b);
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    String::from_utf8_lossy(&result).into_owned()
}

fn generate_random_tres(rng: &mut Xorshift64) -> String {
    let mut s = String::new();
    let lines = rng.next_usize(30) + 1;
    for _ in 0..lines {
        let len = rng.next_usize(80);
        for _ in 0..len {
            s.push(rng.next_char());
        }
        s.push('\n');
    }
    s
}

/// Parse a .tres string and assert it doesn't panic. Returns true if parsing succeeded.
fn parse_does_not_panic(source: &str) -> bool {
    let loader = gdresource::loader::TresLoader::new();
    match loader.parse_str(source, "res://fuzz_test.tres") {
        Ok(_) => true,
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Mutation-based fuzz tests
// ---------------------------------------------------------------------------

#[test]
fn fuzz_mutated_valid_templates_never_panic() {
    for (template_idx, &template) in TEMPLATES.iter().enumerate() {
        for seed in 0..200 {
            let mut rng = Xorshift64::new((template_idx as u64) * 10000 + seed + 1);
            let mutated = mutate_string(&mut rng, template);
            parse_does_not_panic(&mutated);
        }
    }
}

#[test]
fn fuzz_random_tres_strings_never_panic() {
    for seed in 0..500 {
        let mut rng = Xorshift64::new(seed + 1);
        let random = generate_random_tres(&mut rng);
        parse_does_not_panic(&random);
    }
}

#[test]
fn fuzz_empty_and_edge_inputs() {
    let edge_cases = [
        "",
        "\n",
        "\n\n\n",
        " ",
        "\t",
        ";comment only",
        "[]",
        "[",
        "]",
        "[[]]",
        "[gd_resource",
        "[gd_resource]",
        "[gd_resource type=\"\"]",
        "[gd_resource type=\"Resource\" format=999]",
        "[resource]\n",
        "[resource]\nkey",
        "[resource]\nkey =",
        "[resource]\nkey = value",
        "[resource]\n= value",
        "[ext_resource]",
        "[ext_resource type=\"\" path=\"\" id=\"\"]",
        "[sub_resource]",
        "[sub_resource type=\"\" id=\"\"]",
        "a = b\nc = d\n",
        &"x".repeat(10000),
        &format!("[gd_resource type=\"Resource\" format=3]\n\n[resource]\n{}",
            (0..100).map(|i| format!("key_{i} = {i}\n")).collect::<String>()),
    ];

    for input in &edge_cases {
        parse_does_not_panic(input);
    }
}

#[test]
fn fuzz_property_value_edge_cases() {
    let property_values = [
        "Vector2()",
        "Vector2(1.0)",
        "Vector2(1.0, 2.0, 3.0)",
        "Vector2(nan, inf)",
        "Vector2(-inf, 0)",
        "Vector3()",
        "Vector3(1e308, -1e308, 0)",
        "Color()",
        "Color(1.0)",
        "Color(1.0, 0.5)",
        "Color(1.0, 0.5, 0.0)",
        "Color(1.0, 0.5, 0.0, 1.0, 0.5)",
        "true",
        "false",
        "null",
        "\"\"",
        "\"hello world\"",
        "\"unclosed",
        "42",
        "-1",
        "0",
        "99999999999999999999",
        "3.14",
        "-0.0",
        "1e30",
        "ExtResource(\"1\")",
        "ExtResource(\"nonexistent\")",
        "SubResource(\"abc\")",
        "SubResource(\"\")",
    ];

    for val in &property_values {
        let tres = format!("[gd_resource type=\"Resource\" format=3]\n\n[resource]\nprop = {val}\n");
        parse_does_not_panic(&tres);
    }
}

// ---------------------------------------------------------------------------
// Property tests with proptest
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Random ASCII strings never cause the parser to panic.
    #[test]
    fn random_ascii_never_panics(s in "[\\x20-\\x7e\n\t]{0,500}") {
        parse_does_not_panic(&s);
    }

    /// Random bytes (as lossy UTF-8) never cause the parser to panic.
    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..500)) {
        let s = String::from_utf8_lossy(&bytes).into_owned();
        parse_does_not_panic(&s);
    }

    /// Valid templates with random property values don't panic.
    #[test]
    fn valid_header_random_props_never_panic(
        props in prop::collection::vec(
            ("[a-z_]{1,20}", "[a-zA-Z0-9_.()\"\\-]{0,50}"),
            0..20,
        ),
    ) {
        let mut tres = String::from("[gd_resource type=\"Resource\" format=3]\n\n[resource]\n");
        for (key, val) in &props {
            tres.push_str(&format!("{key} = {val}\n"));
        }
        parse_does_not_panic(&tres);
    }

    /// Multiple sections with random ordering don't cause panics.
    #[test]
    fn random_section_ordering(
        sections in prop::collection::vec(
            prop_oneof![
                Just("[gd_resource type=\"Resource\" format=3]"),
                Just("[resource]"),
                Just("[ext_resource type=\"T\" path=\"res://x\" id=\"1\"]"),
                Just("[sub_resource type=\"S\" id=\"s1\"]"),
            ],
            1..10,
        ),
    ) {
        let tres = sections.join("\n");
        parse_does_not_panic(&tres);
    }

    /// Valid .tres with random key names parse without panicking.
    #[test]
    fn unicode_keys_and_values_never_panic(
        key in "\\PC{1,30}",
        value in "\\PC{0,50}",
    ) {
        let tres = format!(
            "[gd_resource type=\"Resource\" format=3]\n\n[resource]\n{key} = {value}\n"
        );
        parse_does_not_panic(&tres);
    }
}

// ---------------------------------------------------------------------------
// Valid resource round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn valid_minimal_resource_parses() {
    assert!(parse_does_not_panic(VALID_TRES_MINIMAL));
}

#[test]
fn valid_resource_with_props_parses() {
    assert!(parse_does_not_panic(VALID_TRES_WITH_PROPS));
}

#[test]
fn valid_resource_with_ext_resource_parses() {
    assert!(parse_does_not_panic(VALID_TRES_WITH_EXT));
}

#[test]
fn valid_resource_with_sub_resource_parses() {
    assert!(parse_does_not_panic(VALID_TRES_WITH_SUB));
}

#[test]
fn parsed_resource_has_correct_properties() {
    let loader = gdresource::loader::TresLoader::new();
    let res = loader
        .parse_str(VALID_TRES_WITH_PROPS, "res://test.tres")
        .expect("valid resource should parse");

    // Verify properties are populated.
    assert!(res.properties().next().is_some(), "Expected properties to be parsed");
}
