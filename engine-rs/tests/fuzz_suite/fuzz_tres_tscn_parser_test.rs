//! Fuzz-style tests for .tres and .tscn resource parsers (pat-mykcg).
//!
//! Exercises the parsers with malformed, truncated, adversarial, and edge-case
//! inputs to verify they return errors gracefully without panicking or crashing.

use gdresource::TresLoader;
use gdscene::packed_scene::PackedScene;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a .tres string and assert it does NOT panic (may return Ok or Err).
fn tres_no_panic(input: &str) {
    let loader = TresLoader::new();
    let _ = loader.parse_str(input, "res://fuzz.tres");
}

/// Parse a .tscn string and assert it does NOT panic (may return Ok or Err).
fn tscn_no_panic(input: &str) {
    let _ = PackedScene::from_tscn(input);
}

// ---------------------------------------------------------------------------
// .tres parser fuzz tests
// ---------------------------------------------------------------------------

#[test]
fn tres_empty_input() {
    tres_no_panic("");
}

#[test]
fn tres_only_whitespace() {
    tres_no_panic("   \n\n\t\t\n   ");
}

#[test]
fn tres_only_comments() {
    tres_no_panic("; just a comment\n; another comment");
}

#[test]
fn tres_header_only() {
    tres_no_panic(r#"[gd_resource type="Resource" format=3]"#);
}

#[test]
fn tres_unclosed_section_bracket() {
    tres_no_panic("[gd_resource type=\"Resource\" format=3");
}

#[test]
fn tres_unopened_section_bracket() {
    tres_no_panic("gd_resource type=\"Resource\" format=3]");
}

#[test]
fn tres_nested_brackets() {
    tres_no_panic("[[gd_resource]]");
}

#[test]
fn tres_empty_brackets() {
    tres_no_panic("[]");
}

#[test]
fn tres_missing_type_attribute() {
    tres_no_panic("[gd_resource format=3]\n\n[resource]\nname = \"Test\"");
}

#[test]
fn tres_unknown_section() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[unknown_section id="5"]
foo = "bar"

[resource]
name = "Test"
"#,
    );
}

#[test]
fn tres_property_no_value() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
name =
"#,
    );
}

#[test]
fn tres_property_no_equals() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
name "Test"
"#,
    );
}

#[test]
fn tres_property_only_key() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
name
"#,
    );
}

#[test]
fn tres_duplicate_resource_section() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
a = 1

[resource]
b = 2
"#,
    );
}

#[test]
fn tres_very_long_string_value() {
    let long_val = "x".repeat(100_000);
    tres_no_panic(&format!(
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nname = \"{long_val}\"\n"
    ));
}

#[test]
fn tres_very_long_key() {
    let long_key = "k".repeat(10_000);
    tres_no_panic(&format!(
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\n{long_key} = \"value\"\n"
    ));
}

#[test]
fn tres_deeply_nested_vector_parens() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
val = Vector2(Vector2(1, 2), 3)
"#,
    );
}

#[test]
fn tres_unclosed_string() {
    tres_no_panic(
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nname = \"unclosed string\nnext = 42\n",
    );
}

#[test]
fn tres_binary_garbage() {
    let garbage: Vec<u8> = (0..=255).collect();
    let s = String::from_utf8_lossy(&garbage);
    tres_no_panic(&s);
}

#[test]
fn tres_null_bytes() {
    tres_no_panic("[gd_resource type=\"Resource\" format=3]\n\n[resource]\nname = \"has\0null\"\n");
}

#[test]
fn tres_many_sub_resources() {
    let mut input = String::from("[gd_resource type=\"Resource\" format=3]\n\n");
    for i in 0..200 {
        input.push_str(&format!(
            "[sub_resource type=\"Resource\" id=\"sub_{i}\"]\nval = {i}\n\n"
        ));
    }
    input.push_str("[resource]\nname = \"many\"\n");
    tres_no_panic(&input);
}

#[test]
fn tres_ext_resource_with_missing_id() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" path="res://icon.png"]

[resource]
texture = ExtResource("1")
"#,
    );
}

#[test]
fn tres_ext_resource_with_uid() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3 uid="uid://abc123"]

[ext_resource type="Texture2D" uid="uid://tex1" path="res://icon.png" id="1"]

[resource]
texture = ExtResource("1")
"#,
    );
}

#[test]
fn tres_vector2_malformed() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
pos = Vector2(abc, def)
"#,
    );
}

#[test]
fn tres_vector2_too_many_args() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
pos = Vector2(1, 2, 3, 4, 5)
"#,
    );
}

#[test]
fn tres_vector2_empty_parens() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
pos = Vector2()
"#,
    );
}

#[test]
fn tres_color_malformed() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
c = Color(not, a, color, value)
"#,
    );
}

#[test]
fn tres_negative_numbers() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
x = -999
y = -0.5
"#,
    );
}

#[test]
fn tres_very_large_numbers() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
big_int = 99999999999999999999999999999999
big_float = 1e308
neg_big = -1e308
"#,
    );
}

#[test]
fn tres_equals_in_value() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[resource]
expr = "a = b"
"#,
    );
}

#[test]
fn tres_truncated_mid_section() {
    tres_no_panic("[gd_resource type=\"Resource\" format=3]\n\n[sub_resource type=\"Res");
}

#[test]
fn tres_only_newlines() {
    tres_no_panic(&"\n".repeat(10_000));
}

#[test]
fn tres_sub_resource_duplicate_id() {
    tres_no_panic(
        r#"[gd_resource type="Resource" format=3]

[sub_resource type="Resource" id="dup"]
a = 1

[sub_resource type="Resource" id="dup"]
b = 2

[resource]
name = "dup_ids"
"#,
    );
}

// ---------------------------------------------------------------------------
// .tscn parser fuzz tests
// ---------------------------------------------------------------------------

#[test]
fn tscn_empty_input() {
    tscn_no_panic("");
}

#[test]
fn tscn_only_whitespace() {
    tscn_no_panic("   \n\n\t\t\n   ");
}

#[test]
fn tscn_only_comments() {
    tscn_no_panic("; just a comment\n; another comment");
}

#[test]
fn tscn_header_only_no_nodes() {
    tscn_no_panic(r#"[gd_scene format=3]"#);
}

#[test]
fn tscn_unclosed_bracket() {
    tscn_no_panic("[gd_scene format=3\n[node name=\"Root\" type=\"Node2D\"]\n");
}

#[test]
fn tscn_node_missing_type() {
    tscn_no_panic("[gd_scene format=3]\n\n[node name=\"Root\"]\n");
}

#[test]
fn tscn_node_missing_name() {
    tscn_no_panic("[gd_scene format=3]\n\n[node type=\"Node2D\"]\n");
}

#[test]
fn tscn_node_with_parent_but_no_root() {
    tscn_no_panic("[gd_scene format=3]\n\n[node name=\"Child\" type=\"Node2D\" parent=\".\"]\n");
}

#[test]
fn tscn_connection_missing_fields() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[connection signal=\"pressed\" from=\".\" to=\".\"]\n",
    );
}

#[test]
fn tscn_connection_all_fields() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[connection signal=\"pressed\" from=\".\" to=\".\" method=\"_on_pressed\" flags=3]\n",
    );
}

#[test]
fn tscn_duplicate_root_nodes() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root1\" type=\"Node2D\"]\n[node name=\"Root2\" type=\"Node2D\"]\n",
    );
}

#[test]
fn tscn_binary_garbage() {
    let garbage: Vec<u8> = (0..=255).collect();
    let s = String::from_utf8_lossy(&garbage);
    tscn_no_panic(&s);
}

#[test]
fn tscn_null_bytes() {
    tscn_no_panic("[gd_scene format=3]\n\n[node name=\"Root\0\" type=\"Node2D\"]\n");
}

#[test]
fn tscn_very_deep_hierarchy() {
    let mut input = String::from("[gd_scene format=3]\n\n");
    input.push_str("[node name=\"Root\" type=\"Node2D\"]\n\n");
    let mut parent_path = String::from(".");
    for i in 0..100 {
        let name = format!("Child{i}");
        input.push_str(&format!(
            "[node name=\"{name}\" type=\"Node2D\" parent=\"{parent_path}\"]\n\n"
        ));
        if i == 0 {
            parent_path = name;
        } else {
            parent_path = format!("{parent_path}/{name}");
        }
    }
    tscn_no_panic(&input);
}

#[test]
fn tscn_many_ext_resources() {
    let mut input = String::from("[gd_scene format=3]\n\n");
    for i in 0..100 {
        input.push_str(&format!(
            "[ext_resource type=\"Texture2D\" path=\"res://tex{i}.png\" id=\"{i}\"]\n"
        ));
    }
    input.push_str("\n[node name=\"Root\" type=\"Node2D\"]\n");
    tscn_no_panic(&input);
}

#[test]
fn tscn_node_property_malformed_vector() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\nposition = Vector2(abc, def)\n",
    );
}

#[test]
fn tscn_node_property_no_equals() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\nposition Vector2(10, 20)\n",
    );
}

#[test]
fn tscn_truncated_mid_node() {
    tscn_no_panic("[gd_scene format=3]\n\n[node name=\"Root\" type=\"No");
}

#[test]
fn tscn_empty_brackets() {
    tscn_no_panic("[]");
}

#[test]
fn tscn_nested_brackets() {
    tscn_no_panic("[[gd_scene format=3]]");
}

#[test]
fn tscn_unknown_section_type() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[banana type=\"Fruit\"]\n\n[node name=\"Root\" type=\"Node2D\"]\n",
    );
}

#[test]
fn tscn_node_with_groups() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\" groups=[\"enemies\", \"all\"]]\n",
    );
}

#[test]
fn tscn_node_with_malformed_groups() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\" groups=[\"unclosed]\n",
    );
}

#[test]
fn tscn_instance_without_ext_resource() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"Child\" parent=\".\" instance=ExtResource(\"99\")]\n",
    );
}

#[test]
fn tscn_very_long_node_name() {
    let long_name = "N".repeat(10_000);
    tscn_no_panic(&format!(
        "[gd_scene format=3]\n\n[node name=\"{long_name}\" type=\"Node2D\"]\n"
    ));
}

#[test]
fn tscn_very_long_property_value() {
    let long_val = "x".repeat(100_000);
    tscn_no_panic(&format!(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\ndata = \"{long_val}\"\n"
    ));
}

#[test]
fn tscn_only_newlines() {
    tscn_no_panic(&"\n".repeat(10_000));
}

#[test]
fn tscn_unique_name_prefix() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"%UniqueChild\" type=\"Sprite2D\" parent=\".\"]\n",
    );
}

#[test]
fn tscn_connection_invalid_flags() {
    tscn_no_panic(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[connection signal=\"sig\" from=\".\" to=\".\" method=\"m\" flags=abc]\n",
    );
}

// ---------------------------------------------------------------------------
// Mutation-based fuzz: take valid inputs, corrupt them byte-by-byte
// ---------------------------------------------------------------------------

#[test]
fn tres_byte_flip_no_panic() {
    let valid = r#"[gd_resource type="Resource" format=3]

[sub_resource type="StyleBox" id="sb1"]
margin_left = 5.0

[resource]
name = "Test"
value = 42
position = Vector2(10, 20)
"#;
    let bytes = valid.as_bytes().to_vec();
    for i in (0..bytes.len()).step_by(7) {
        let mut mutated = bytes.clone();
        mutated[i] = mutated[i].wrapping_add(1);
        if let Ok(s) = String::from_utf8(mutated) {
            tres_no_panic(&s);
        }
    }
}

#[test]
fn tscn_byte_flip_no_panic() {
    let valid = r#"[gd_scene format=3 uid="uid://abc"]

[ext_resource type="Texture2D" path="res://icon.png" id="1"]

[node name="Root" type="Node2D"]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="."]
texture = ExtResource("1")

[connection signal="ready" from="." to="." method="_on_ready"]
"#;
    let bytes = valid.as_bytes().to_vec();
    for i in (0..bytes.len()).step_by(5) {
        let mut mutated = bytes.clone();
        mutated[i] = mutated[i].wrapping_add(1);
        if let Ok(s) = String::from_utf8(mutated) {
            tscn_no_panic(&s);
        }
    }
}

// ---------------------------------------------------------------------------
// Truncation fuzz: parse all prefixes of a valid file
// ---------------------------------------------------------------------------

#[test]
fn tres_progressive_truncation() {
    let valid = r#"[gd_resource type="Resource" format=3]

[sub_resource type="Resource" id="sub1"]
x = 10

[resource]
name = "Truncate"
val = Vector2(5, 10)
"#;
    for len in (0..valid.len()).step_by(10) {
        tres_no_panic(&valid[..len]);
    }
}

#[test]
fn tscn_progressive_truncation() {
    let valid = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="1"]

[node name="Root" type="Node2D"]

[node name="Child" type="Sprite2D" parent="."]
position = Vector2(50, 50)

[connection signal="ready" from="." to="." method="_ready"]
"#;
    for len in (0..valid.len()).step_by(10) {
        tscn_no_panic(&valid[..len]);
    }
}
