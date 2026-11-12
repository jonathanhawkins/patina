//! pat-s3rrv.199: TorusMesh, TubeTrailMesh and TextMesh are ClassDB-registered.
//!
//! ⚠️ Their absence was not merely cosmetic. An unregistered class has no
//! property list, so the inspector shows nothing to edit and
//! `create_node`-seeded defaults are empty — a `.tscn` could author
//! `inner_radius` and the editor could neither display nor round-trip it.
//!
//! Every default here is transcribed from `upstream/godot/doc/classes/*.xml`.
//! ⚠️ `justification_flags = 163` is the reason transcription matters: it is a
//! composed bitmask that no amount of reasoning about the class recovers.
//!
//! Sibling coverage: `primitive_mesh_family_metadata.rs` covers the classes
//! that were already registered, and `pat-s3rrv.198` covers RibbonTrailMesh /
//! PointMesh / QuadMesh.

use gdobject::class_db::{get_class_info, is_parent_class, register_all_classes};
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

fn property_names(class: &str) -> Vec<String> {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// All three exist and inherit PrimitiveMesh.
#[test]
fn the_three_subtypes_are_registered_under_primitive_mesh() {
    register_all_classes();
    for class in ["TorusMesh", "TubeTrailMesh", "TextMesh"] {
        assert!(
            get_class_info(class).is_some(),
            "{class} must be registered — an unregistered class has no \
             inspector properties and seeds no defaults"
        );
        assert!(
            is_parent_class(class, "PrimitiveMesh"),
            "{class} must inherit PrimitiveMesh"
        );
    }
}

/// TorusMesh, from `doc/classes/TorusMesh.xml`.
#[test]
fn torus_mesh_defaults_match_the_docs() {
    assert_eq!(own("TorusMesh", "inner_radius"), Variant::Float(0.5));
    assert_eq!(own("TorusMesh", "outer_radius"), Variant::Float(1.0));
    assert_eq!(own("TorusMesh", "rings"), Variant::Int(64));
    assert_eq!(own("TorusMesh", "ring_segments"), Variant::Int(32));
}

/// TubeTrailMesh, from `doc/classes/TubeTrailMesh.xml`.
#[test]
fn tube_trail_mesh_defaults_match_the_docs() {
    assert_eq!(own("TubeTrailMesh", "radius"), Variant::Float(0.5));
    assert_eq!(own("TubeTrailMesh", "radial_steps"), Variant::Int(8));
    assert_eq!(own("TubeTrailMesh", "sections"), Variant::Int(5));
    assert_eq!(own("TubeTrailMesh", "section_length"), Variant::Float(0.2));
    assert_eq!(own("TubeTrailMesh", "section_rings"), Variant::Int(3));
    assert_eq!(own("TubeTrailMesh", "cap_top"), Variant::Bool(true));
    assert_eq!(own("TubeTrailMesh", "cap_bottom"), Variant::Bool(true));
    // A `Curve` resource has no scalar default; Nil is the placeholder the
    // sibling RibbonTrailMesh registration uses for the same property.
    assert_eq!(own("TubeTrailMesh", "curve"), Variant::Nil);
}

/// TextMesh, from `doc/classes/TextMesh.xml`.
///
/// ⚠️ `justification_flags` is **163**, a composed bitmask. Deriving it — or
/// assuming a round number — gets it wrong, and nothing downstream would
/// complain until text justified differently from Godot.
#[test]
fn text_mesh_defaults_match_the_docs() {
    assert_eq!(own("TextMesh", "text"), Variant::String(String::new()));
    assert_eq!(own("TextMesh", "font_size"), Variant::Int(16));
    assert_eq!(own("TextMesh", "depth"), Variant::Float(0.05));
    assert_eq!(own("TextMesh", "pixel_size"), Variant::Float(0.01));
    assert_eq!(own("TextMesh", "curve_step"), Variant::Float(0.5));
    assert_eq!(own("TextMesh", "horizontal_alignment"), Variant::Int(1));
    assert_eq!(own("TextMesh", "vertical_alignment"), Variant::Int(1));
    assert_eq!(own("TextMesh", "uppercase"), Variant::Bool(false));
    assert_eq!(own("TextMesh", "line_spacing"), Variant::Float(0.0));
    assert_eq!(own("TextMesh", "autowrap_mode"), Variant::Int(0));
    assert_eq!(own("TextMesh", "justification_flags"), Variant::Int(163));
    assert_eq!(own("TextMesh", "width"), Variant::Float(500.0));
    assert_eq!(own("TextMesh", "text_direction"), Variant::Int(0));
    assert_eq!(
        own("TextMesh", "structured_text_bidi_override"),
        Variant::Int(0)
    );
    // A `Font` resource, like TubeTrailMesh's curve.
    assert_eq!(own("TextMesh", "font"), Variant::Nil);
}

/// ⚠️ A duplicate property SHADOWS silently — last one wins, no error.
///
/// `class_db.rs` is 17k lines with many near-identical registration blocks, so
/// a property added twice (or a class registered twice) is easy to introduce
/// and impossible to see. This asserts each name appears once per class.
#[test]
fn no_subtype_declares_a_property_twice() {
    for class in ["TorusMesh", "TubeTrailMesh", "TextMesh"] {
        let names = property_names(class);
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            names.len(),
            unique.len(),
            "{class} declares a duplicate property; the later one silently \
             shadows the earlier: {names:?}"
        );
    }
}

/// The geometry-driving properties the shared resolver reads must all exist.
///
/// ⚠️ This is the inverse audit: not "is what I registered correct" but "does
/// the resolver ask for anything ClassDB does not declare". A property the
/// renderer reads but ClassDB omits is invisible in the inspector and seeds no
/// default, so it silently falls back at every load.
#[test]
fn every_property_the_resolver_reads_is_declared() {
    for (class, keys) in [
        (
            "TorusMesh",
            &["inner_radius", "outer_radius", "rings", "ring_segments"][..],
        ),
        (
            "TubeTrailMesh",
            &[
                "radius",
                "radial_steps",
                "sections",
                "section_length",
                "section_rings",
                "cap_top",
                "cap_bottom",
            ][..],
        ),
        (
            "TextMesh",
            &[
                "text",
                "font",
                "font_size",
                "depth",
                "pixel_size",
                "curve_step",
            ][..],
        ),
    ] {
        let declared = property_names(class);
        for key in keys {
            assert!(
                declared.iter().any(|n| n == key),
                "{class} must declare `{key}` — the mesh resolver reads it"
            );
        }
    }
}
