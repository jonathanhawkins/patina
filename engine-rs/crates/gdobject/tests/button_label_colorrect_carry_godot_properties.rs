//! pat-s3rrv.57 (merged finding 3): the commonest UI classes must carry their
//! real properties.
//!
//! Button, Label and ColorRect were registered name-only — `ClassRegistration::new("Button").parent("BaseButton")`
//! and nothing else — so `get_property_list("Button")` reported none of the 13
//! members Godot gives it, the inspector had no schema for the commonest widget
//! in any UI, and a node created through `create_node` was seeded with nothing.
//! They are three of the 86 registered classes the audit found carrying zero
//! properties.
//!
//! Defaults come from `upstream/godot/doc/classes/*.xml` (`<member default>`),
//! which is the only place they are stated exactly.

use gdobject::class_db::{get_class_info, get_property_list, instantiate, register_all_classes};
// ⚠️ `get_property` is a `GodotObject` trait method, not an inherent one: the
// trait must be in scope or the call fails to resolve (E0599).
use gdobject::GodotObject;
use gdvariant::Variant;

fn own_property(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

fn own_property_count(class: &str) -> usize {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .len()
}

#[test]
fn button_carries_its_thirteen_members() {
    assert_eq!(
        own_property_count("Button"),
        13,
        "Button.xml declares exactly 13 members"
    );

    assert_eq!(
        own_property("Button", "text"),
        Variant::String(String::new())
    );
    assert_eq!(own_property("Button", "flat"), Variant::Bool(false));
    assert_eq!(own_property("Button", "expand_icon"), Variant::Bool(false));

    // ⚠️ The enum-zero trap. Both alignment members default to 1 (CENTER); a
    // registration that reached for the usual 0 would left-align every button
    // in the engine.
    assert_eq!(own_property("Button", "alignment"), Variant::Int(1));
    assert_eq!(
        own_property("Button", "vertical_icon_alignment"),
        Variant::Int(1)
    );
    assert_eq!(own_property("Button", "icon_alignment"), Variant::Int(0));
}

#[test]
fn label_carries_its_twenty_two_members() {
    assert_eq!(
        own_property_count("Label"),
        22,
        "Label.xml lists 24, two of which (`mouse_filter`, `size_flags_vertical`) \
         are Control members Label merely re-defaults"
    );

    assert_eq!(
        own_property("Label", "text"),
        Variant::String(String::new())
    );
    assert_eq!(own_property("Label", "uppercase"), Variant::Bool(false));

    // ⚠️ Four defaults where zero is a real, different value.
    assert_eq!(
        own_property("Label", "visible_characters"),
        Variant::Int(-1),
        "-1 means `show all`; 0 would render an empty label"
    );
    assert_eq!(own_property("Label", "max_lines_visible"), Variant::Int(-1));
    assert_eq!(own_property("Label", "visible_ratio"), Variant::Float(1.0));
    assert_eq!(
        own_property("Label", "autowrap_trim_flags"),
        Variant::Int(192)
    );
    assert_eq!(
        own_property("Label", "justification_flags"),
        Variant::Int(163)
    );

    assert_eq!(
        own_property("Label", "ellipsis_char"),
        Variant::String("\u{2026}".to_string()),
        "the ellipsis is one character, not three dots"
    );
    assert_eq!(
        own_property("Label", "paragraph_separator"),
        Variant::String("\n".to_string())
    );
}

/// ⚠️ `get_property_list` concatenates the inheritance chain WITHOUT deduping,
/// so re-declaring an inherited member shows it twice — in the inspector and in
/// script reflection alike. Godot's XML lists `mouse_filter` and
/// `size_flags_vertical` under Label because it re-defaults them; copying that
/// list wholesale is the trap.
#[test]
fn no_member_is_declared_twice_along_the_chain() {
    register_all_classes();

    for class in ["Button", "Label", "ColorRect"] {
        let names: Vec<String> = get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .collect();
        for name in &names {
            assert_eq!(
                names.iter().filter(|n| *n == name).count(),
                1,
                "`{name}` appears more than once in {class}'s property list"
            );
        }
    }
}

/// ColorRect's `color` reaches an instantiated object, which is the path
/// `SceneAccess::create_node` uses to seed a live node's property bag.
#[test]
fn colorrect_color_defaults_to_white_on_a_real_instance() {
    register_all_classes();

    assert_eq!(
        own_property("ColorRect", "color"),
        Variant::Color(gdcore::math::Color::WHITE),
        "ColorRect.xml: Color(1, 1, 1, 1) — the same fallback control_render.rs \
         already applies, so ClassDB now agrees with the renderer"
    );

    let rect = instantiate("ColorRect").expect("ColorRect is instantiable");
    assert_eq!(
        rect.get_property("color"),
        Variant::Color(gdcore::math::Color::WHITE),
        "a registered default must reach the instance, not just the schema"
    );
}
