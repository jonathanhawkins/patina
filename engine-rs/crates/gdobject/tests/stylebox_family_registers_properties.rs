//! pat-0q2kx.148 — the `StyleBox` family exposes its real Godot schema.
//!
//! The four subtypes were registered as bare class names with no properties, so
//! the inspector had nothing to show for a StyleBox and a `.tscn` round-trip had
//! no defaults to compare against. `content_margin_*` in particular lives on the
//! **base** class in Godot (`scene/resources/style_box.cpp`), which is what makes
//! `StyleBoxEmpty` the standard pad-without-painting resource.

use gdobject::{
    class_exists, class_has_enum, get_property_list, is_parent_class, register_2d_classes,
};
use gdvariant::Variant;

const SUBTYPES: &[&str] = &[
    "StyleBoxFlat",
    "StyleBoxTexture",
    "StyleBoxLine",
    "StyleBoxEmpty",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn assert_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

/// **The registration clause.** Every subtype carries the base class's
/// `content_margin_*`, because Godot declares it on `StyleBox` itself.
#[test]
fn stylebox_family_registers_properties() {
    register_2d_classes();

    assert!(class_exists("StyleBox"));
    for class in SUBTYPES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "StyleBox"),
            "{class} derives StyleBox"
        );
        // Must reach the root — an unregistered intermediate severs the chain
        // and `class_exists` alone would not notice.
        assert!(
            is_parent_class(class, "Resource"),
            "{class} reaches Resource"
        );

        assert_props(
            class,
            &[
                "content_margin_left",
                "content_margin_top",
                "content_margin_right",
                "content_margin_bottom",
            ],
        );
        // Godot's sentinel is -1 ("unset, derive"), not 0 — a 0 default would
        // silently mean "no padding" and lose the distinction.
        assert_eq!(
            default_of(class, "content_margin_left"),
            Variant::Float(-1.0),
            "{class}: unset content margin must be -1, Godot's sentinel"
        );
    }

    // `StyleBoxEmpty` adds nothing of its own — the base margins are its whole
    // purpose, so it must expose them and nothing more exotic.
    // Minus Resource's own three (`resource_local_to_scene`, `resource_name`,
    // `resource_path`), which every StyleBox inherits now that Resource is
    // registered from one canonical definition.
    let resource = props("Resource");
    let empty: Vec<_> = props("StyleBoxEmpty")
        .into_iter()
        .filter(|p| !resource.contains(p))
        .collect();
    assert_eq!(
        empty.len(),
        4,
        "StyleBoxEmpty is exactly the four base margins, got {empty:?}"
    );
}

/// `StyleBoxFlat`'s own schema — the fields a `.tscn` carries and that would be
/// silently dropped if the property were missing.
#[test]
fn stylebox_flat_exposes_its_full_schema() {
    register_2d_classes();

    assert_props(
        "StyleBoxFlat",
        &[
            "bg_color",
            "draw_center",
            "skew",
            "border_width_left",
            "border_width_bottom",
            "border_color",
            "border_blend",
            "corner_radius_top_left",
            "corner_radius_bottom_right",
            "corner_detail",
            "expand_margin_left",
            "shadow_color",
            "shadow_size",
            "shadow_offset",
            "anti_aliasing",
            "anti_aliasing_size",
        ],
    );

    // The four corner radii are independent properties, not one shared value —
    // that independence is the point of this bead.
    for corner in [
        "corner_radius_top_left",
        "corner_radius_top_right",
        "corner_radius_bottom_right",
        "corner_radius_bottom_left",
    ] {
        assert_eq!(default_of("StyleBoxFlat", corner), Variant::Int(0));
    }

    // Godot's documented defaults for the flags.
    assert_eq!(
        default_of("StyleBoxFlat", "draw_center"),
        Variant::Bool(true)
    );
    assert_eq!(
        default_of("StyleBoxFlat", "anti_aliasing"),
        Variant::Bool(true)
    );
    assert_eq!(default_of("StyleBoxFlat", "corner_detail"), Variant::Int(8));
}

/// `StyleBoxTexture`'s schema plus its `AxisStretchMode` enum — the tiling modes
/// an authored theme selects.
#[test]
fn stylebox_texture_exposes_axis_stretch_and_region() {
    register_2d_classes();

    assert_props(
        "StyleBoxTexture",
        &[
            "texture",
            "texture_margin_left",
            "texture_margin_bottom",
            "expand_margin_left",
            "axis_stretch_horizontal",
            "axis_stretch_vertical",
            "region_rect",
            "modulate_color",
            "draw_center",
        ],
    );
    // STRETCH = 0 is Godot's default for both axes.
    assert_eq!(
        default_of("StyleBoxTexture", "axis_stretch_horizontal"),
        Variant::Int(0)
    );
    assert!(
        class_has_enum("StyleBoxTexture", "AxisStretchMode"),
        "the tiling modes must be a real enum, not bare ints"
    );
}

/// `StyleBoxLine`'s schema.
#[test]
fn stylebox_line_exposes_its_schema() {
    register_2d_classes();

    assert_props(
        "StyleBoxLine",
        &["color", "grow_begin", "grow_end", "thickness", "vertical"],
    );
    assert_eq!(default_of("StyleBoxLine", "thickness"), Variant::Int(1));
    assert_eq!(default_of("StyleBoxLine", "vertical"), Variant::Bool(false));
}
