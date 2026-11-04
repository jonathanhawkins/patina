//! A `.tres` string property containing a newline spans lines (Godot writes the
//! newline literally). The loader continued a value only while brackets were
//! unbalanced and skipped blank lines first, so such a value failed to parse.

use gdresource::loader::TresLoader;
use gdvariant::Variant;

#[test]
fn a_tres_string_with_literal_newlines_and_a_blank_line_loads_whole() {
    let tres = "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nresource_name = \"first\n\n  third [\"\nother = 5\n";
    let res = TresLoader::new()
        .parse_str(tres, "res://t.tres")
        .expect("tres parses");
    assert_eq!(
        res.get_property("resource_name"),
        Some(&Variant::String("first\n\n  third [".into()))
    );
    assert_eq!(
        res.get_property("other"),
        Some(&Variant::Int(5)),
        "the property after the string must not be swallowed into it"
    );
}
