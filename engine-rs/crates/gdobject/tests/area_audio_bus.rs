//! pat-0q2kx.189 clause 3 — `Area2D`/`Area3D` audio-bus override surface.
//!
//! `gdaudio`'s server already reroutes a playing voice when an area overrides
//! the bus (`gdaudio/src/server.rs:299,948`), and `gdeditor`'s generated
//! inspector schema already declares both properties with Godot's defaults.
//! **ClassDB was the only place that did not know them** — so a `.tscn` carrying
//! `audio_bus_override` silently dropped it, and the inspector had nothing to
//! show for a property the editor's own schema listed.
//!
//! ⚠️ **Surface only.** Registering these does not connect an `Area2D` to the
//! 2D mixer; `gdaudio`'s existing reroute path is written against `Area3D`.
//! These tests assert the schema and nothing about audio routing.

use gdcore::string_name::StringName;
use gdobject::class_db::{
    class_has_property, get_class_info, register_2d_classes, register_3d_classes,
};
use gdvariant::Variant;

fn default_of(class: &str, name: &str) -> Variant {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
        .clone()
}

/// Both areas declare the pair, with Godot's documented defaults.
#[test]
fn areas_declare_the_audio_bus_override_surface() {
    register_2d_classes();
    register_3d_classes();

    for class in ["Area2D", "Area3D"] {
        // `default="false"` in doc/classes/Area{2D,3D}.xml.
        assert_eq!(
            default_of(class, "audio_bus_override"),
            Variant::Bool(false),
            "{class}.audio_bus_override defaults to false"
        );
        // `default=&"Master"` — the bus every voice starts on. An empty string
        // would route to a bus that does not exist.
        assert_eq!(
            default_of(class, "audio_bus_name"),
            Variant::StringName(StringName::new("Master")),
            "{class}.audio_bus_name defaults to the Master bus, not an empty name"
        );
    }
}

/// The properties must resolve through the inheritance chain too, since scripts
/// and `.tscn` loading query by the concrete node class.
#[test]
fn audio_bus_properties_resolve_through_classdb_lookup() {
    register_2d_classes();
    register_3d_classes();

    for class in ["Area2D", "Area3D"] {
        assert!(class_has_property(class, "audio_bus_override"));
        assert!(class_has_property(class, "audio_bus_name"));
    }
}

/// The pre-existing monitoring/monitorable pair must survive alongside them —
/// this bead adds to `Area2D`'s registration rather than replacing it, and
/// `monitorable` in particular is load-bearing for area-vs-area detection.
#[test]
fn existing_area_properties_are_untouched() {
    register_2d_classes();
    register_3d_classes();

    for class in ["Area2D", "Area3D"] {
        assert_eq!(default_of(class, "monitoring"), Variant::Bool(true));
        assert_eq!(default_of(class, "monitorable"), Variant::Bool(true));
    }
    // Area2D's angular-damp override from pat-0q2kx.112 is still there.
    assert_eq!(
        default_of("Area2D", "angular_damp"),
        Variant::Float(1.0),
        "the angular-damp surface must not have been clobbered"
    );
}

/// pat-s3rrv.102: `Area3D`'s property table is COMPLETE against Godot.
///
/// The two stragglers were `reverb_bus_name` and `wind_source_path` — the last
/// members of `doc/classes/Area3D.xml` that ClassDB did not know, so a `.tscn`
/// authoring either one dropped it on load.
///
/// ⚠️ SURFACE ONLY, and the distinction matters: registering these does not
/// apply a wind force to a body or route a reverb bus. Nothing reads them yet.
/// They are here so authored values round-trip and the inspector can show them
/// — the same scope the audio-bus pair above shipped with.
#[test]
fn area3d_declares_the_wind_and_reverb_surface() {
    register_3d_classes();

    // `default=&"Master"` in Area3D.xml — NOT an empty name. An empty default
    // would route reverb to a bus that does not exist, which is the same trap
    // `audio_bus_name` documents above.
    assert_eq!(
        default_of("Area3D", "reverb_bus_name"),
        Variant::StringName(StringName::new("Master")),
        "Area3D.reverb_bus_name defaults to the Master bus"
    );

    // `default=NodePath("")` — an unset source, i.e. no wind.
    assert!(
        class_has_property("Area3D", "wind_source_path"),
        "Area3D must declare `wind_source_path`"
    );

    // The other wind/reverb members were already registered; assert the whole
    // set so a future edit cannot drop one silently.
    for name in [
        "wind_force_magnitude",
        "wind_attenuation_factor",
        "reverb_bus_enabled",
        "reverb_bus_amount",
        "reverb_bus_uniformity",
    ] {
        assert!(
            class_has_property("Area3D", name),
            "Area3D must declare `{name}`"
        );
    }
}
