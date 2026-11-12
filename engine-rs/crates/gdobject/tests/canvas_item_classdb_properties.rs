//! pat-0q2kx.39 — `CanvasItem` reports all 15 of its documented properties.
//!
//! The Play-path render walk (gdeditor/src/viewport_2d_render.rs, via
//! `gdscene::canvas_order`) honours `self_modulate`, `z_as_relative`,
//! `y_sort_enabled` and `clip_children`. ClassDB knew only three of the fifteen
//! — `visible`, `modulate`, `z_index` — so everything that answers from ClassDB
//! rather than from a node's own property map (the inspector's property list,
//! `property_can_revert`/`property_get_revert`, `.tscn` default elision)
//! reported that a canvas item has no such properties. Registering three of
//! fifteen is the failure mode this file exists to catch: the render semantics
//! were wired while the class surface they read from stayed empty.
//!
//! Names and defaults are the `<member>` entries of
//! `upstream/godot/doc/classes/CanvasItem.xml` — the same oracle every other
//! ClassDB parity test in this crate uses. `material` is the one member with no
//! `default=` attribute (an unset `Resource` reference), so it registers `Nil`.

use gdobject::class_db::{property_can_revert, property_get_revert};
use gdobject::{class_exists, get_property_list, is_parent_class, register_2d_classes};
use gdvariant::Variant;

/// Every documented `CanvasItem` member, with the default its doc XML declares.
fn documented() -> Vec<(&'static str, Variant)> {
    let white = Variant::Color(gdcore::math::Color::WHITE);
    vec![
        ("visible", Variant::Bool(true)),
        ("modulate", white.clone()),
        ("self_modulate", white),
        ("show_behind_parent", Variant::Bool(false)),
        ("top_level", Variant::Bool(false)),
        ("clip_children", Variant::Int(0)),
        ("light_mask", Variant::Int(1)),
        ("visibility_layer", Variant::Int(1)),
        ("z_index", Variant::Int(0)),
        ("z_as_relative", Variant::Bool(true)),
        ("y_sort_enabled", Variant::Bool(false)),
        ("texture_filter", Variant::Int(0)),
        ("texture_repeat", Variant::Int(0)),
        ("material", Variant::Nil),
        ("use_parent_material", Variant::Bool(false)),
    ]
}

/// **The acceptance test.** All fifteen documented members are registered on
/// `CanvasItem`, each exactly once and with its documented default.
#[test]
fn canvas_item_registers_all_documented_properties() {
    register_2d_classes();
    assert!(
        class_exists("CanvasItem"),
        "CanvasItem must be registered before its properties can mean anything"
    );

    let expected = documented();
    let props = get_property_list("CanvasItem");
    for (name, want) in &expected {
        let matches: Vec<_> = props.iter().filter(|p| p.name == *name).collect();
        assert_eq!(
            matches.len(),
            1,
            "CanvasItem must register `{name}` exactly once, found {}: \
             get_property_list concatenates the chain without deduping, so a \
             second registration would silently shadow this default",
            matches.len()
        );
        assert_eq!(
            &matches[0].default_value, want,
            "`{name}` must default to the `default=` of doc/classes/CanvasItem.xml"
        );
    }

    // The count is asserted too: a member registered under a misspelled name
    // would satisfy every check above by simply never being looked for.
    let found: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        props.len(),
        expected.len(),
        "CanvasItem must expose exactly the {} documented members, got {found:?}",
        expected.len()
    );
}

/// The properties have to reach the classes that actually render — a default
/// parked on a base nothing inherits from is still invisible to the inspector
/// and to `.tscn` default elision.
#[test]
fn canvas_item_properties_reach_the_2d_node_classes() {
    register_2d_classes();

    for class in ["Node2D", "Sprite2D", "Control"] {
        assert!(
            is_parent_class(class, "CanvasItem"),
            "{class} must resolve as a CanvasItem or it inherits none of these"
        );
        let names: Vec<String> = get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .collect();
        for (name, _) in documented() {
            assert!(
                names.iter().any(|n| n.as_str() == name),
                "{class} must inherit CanvasItem's `{name}`; it has {names:?}"
            );
            assert!(
                property_can_revert(class, name),
                "{class}.{name} must have a class default to revert to"
            );
        }
    }
}

/// `property_get_revert` is the path the inspector's revert arrow uses, and it
/// takes the LAST match in the merged chain. The inherited default has to be the
/// documented one, not whatever `Variant` happens to be zero — `z_as_relative`
/// is the trap: Godot defaults it to `true`, so a `Bool(false)` here would
/// invert relative-Z for every node that never serializes the property.
#[test]
fn inherited_canvas_item_defaults_survive_the_chain() {
    register_2d_classes();

    for (name, want) in documented() {
        // Node2D re-declares none of these, so the CanvasItem default is what a
        // derived class must revert to.
        assert_eq!(
            property_get_revert("Node2D", name),
            Some(want.clone()),
            "Node2D.{name} must revert to CanvasItem's documented default"
        );
    }

    assert_eq!(
        property_get_revert("Node2D", "z_as_relative"),
        Some(Variant::Bool(true)),
        "z_as_relative defaults to TRUE in Godot; false would silently disable \
         the parent-z accumulation the Play render walk depends on"
    );
}
