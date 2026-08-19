//! pat-s3rrv.57 (merged finding 3): 22 members across five more classes.
//!
//! PhysicsBody3D and ProgressBar are Nodes and were cleared by the per-class
//! safety check (no runtime reader for any member). AnimationNodeTransition,
//! XRServer and ThemeDB are non-Nodes, which `create_node` never seeds, so they
//! are safe by construction.
//!
//! ⚠️ StyleBox is deliberately absent despite appearing on the same safe list:
//! it already carries all four content margins through the `content_margins`
//! closure at `class_db.rs:9158`. The census that built the list counted
//! `.property(` textually inside each registration block, so closure-applied
//! properties were invisible to it — registering StyleBox again would have
//! produced exactly the duplicated inspector rows that closure's comment warns
//! about.

use gdobject::class_db::{get_class_info, get_property_list, register_all_classes};
use gdvariant::Variant;

fn own(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

fn own_count(class: &str) -> usize {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .len()
}

#[test]
fn physics_body_3d_carries_its_six_axis_locks() {
    assert_eq!(own_count("PhysicsBody3D"), 6);
    for axis in [
        "linear_x",
        "linear_y",
        "linear_z",
        "angular_x",
        "angular_y",
        "angular_z",
    ] {
        assert_eq!(
            own("PhysicsBody3D", &format!("axis_lock_{axis}")),
            Variant::Bool(false),
            "an unconfigured body is free on every axis"
        );
    }
}

/// ⚠️ Three of Godot's four: `editor_preview_indeterminate` states no default in
/// the XML, so it is omitted rather than invented — the same rule that left ten
/// BaseMaterial3D members out.
#[test]
fn progress_bar_carries_its_stated_members() {
    assert_eq!(own_count("ProgressBar"), 3);
    // A false here would hide the percentage label on every bar in the project.
    assert_eq!(own("ProgressBar", "show_percentage"), Variant::Bool(true));
    assert_eq!(own("ProgressBar", "fill_mode"), Variant::Int(0));
    assert_eq!(own("ProgressBar", "indeterminate"), Variant::Bool(false));
}

#[test]
fn animation_node_transition_carries_its_four() {
    assert_eq!(own_count("AnimationNodeTransition"), 4);
    assert_eq!(
        own("AnimationNodeTransition", "xfade_time"),
        Variant::Float(0.0)
    );
    // `Curve` has no Patina type; recording the property as Nil says "declared,
    // not implemented" rather than pretending the curve works.
    assert_eq!(own("AnimationNodeTransition", "xfade_curve"), Variant::Nil);
}

#[test]
fn xr_server_and_theme_db_carry_their_scalar_defaults() {
    assert_eq!(own_count("XRServer"), 4);
    // ⚠️ A zero would collapse every XR world to a point.
    assert_eq!(own("XRServer", "world_scale"), Variant::Float(1.0));

    assert_eq!(own_count("ThemeDB"), 5);
    // ⚠️ Zeroes here would render every unstyled control invisible or collapsed.
    assert_eq!(own("ThemeDB", "fallback_font_size"), Variant::Int(16));
    assert_eq!(own("ThemeDB", "fallback_base_scale"), Variant::Float(1.0));
}

/// The margins StyleBox already had must still appear exactly once — the check
/// that would have caught a duplicate registration.
#[test]
fn style_box_margins_are_not_duplicated() {
    register_all_classes();
    let names: Vec<String> = get_property_list("StyleBoxFlat")
        .into_iter()
        .map(|p| p.name)
        .collect();

    for margin in [
        "content_margin_left",
        "content_margin_top",
        "content_margin_right",
        "content_margin_bottom",
    ] {
        assert_eq!(
            names.iter().filter(|n| *n == margin).count(),
            1,
            "`{margin}` must be inherited from StyleBox exactly once"
        );
    }
}
