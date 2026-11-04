//! A string property whose value contains a newline must load. Godot writes the
//! newline LITERALLY, so the value spans lines — the official "Dodge the
//! Creeps" demo's `hud.tscn` has
//!
//! ```text
//! text = "Dodge the
//! Creeps"
//! ```
//!
//! The parser only continued a value across lines while BRACKETS were
//! unbalanced, so it read `"Dodge the` alone, failed to parse it, dropped the
//! property, and the game's title label came up empty.

use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

fn label_text(tscn: &str, path: &str) -> Variant {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let id = if path == "." {
        main
    } else {
        tree.get_node_relative(main, path).expect("node exists")
    };
    tree.get_node(id).unwrap().get_property("text")
}

#[test]
fn a_string_with_a_literal_newline_loads_whole() {
    let tscn = r#"[gd_scene format=3]

[node name="HUD" type="CanvasLayer"]

[node name="MessageLabel" type="Label" parent="."]
text = "Dodge the
Creeps"
horizontal_alignment = 1
"#;
    assert_eq!(
        label_text(tscn, "MessageLabel"),
        Variant::String("Dodge the\nCreeps".into())
    );
}

#[test]
fn the_line_after_a_multiline_string_still_parses() {
    let tscn = r#"[gd_scene format=3]

[node name="L" type="Label"]
text = "a
b"
horizontal_alignment = 1
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    assert_eq!(
        tree.get_node(id).unwrap().get_property("horizontal_alignment"),
        Variant::Int(1),
        "the property after the string must not be swallowed into it"
    );
}

#[test]
fn blank_lines_indentation_brackets_and_escaped_quotes_inside_the_string_survive() {
    // Every piece is something a line-trimming, bracket-counting joiner gets
    // wrong: a blank line (skipped as empty), leading spaces (trimmed), an
    // unbalanced `[` (counted as an open container), an escaped quote (read as
    // the end of the string).
    let tscn = "[gd_scene format=3]\n\n[node name=\"L\" type=\"Label\"]\ntext = \"line one [\n\n  indented \\\"quoted\\\"\nend\"\nvisible = false\n";
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let node = tree.get_node(id).unwrap();
    assert_eq!(
        node.get_property("text"),
        Variant::String("line one [\n\n  indented \"quoted\"\nend".into())
    );
    assert_eq!(node.get_property("visible"), Variant::Bool(false));
}

#[test]
fn a_multiline_string_inside_a_sub_resource_loads() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="Resource" id="1"]
resource_name = "two
lines"

[node name="N" type="Node"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let props = scene.sub_resource_props("1").expect("sub-resource parsed");
    assert_eq!(
        props.get("resource_name"),
        Some(&Variant::String("two\nlines".into()))
    );
}
