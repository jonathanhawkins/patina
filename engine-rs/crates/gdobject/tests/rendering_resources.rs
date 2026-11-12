//! pat-0q2kx.214 — the rendering resource classes are registered in ClassDB.
//!
//! `gdserver3d` implements the render side of all of these — `environment.rs`
//! (`Environment3D`, with `from_properties`/`to_properties`), `sky.rs` (the
//! three sky materials), `compositor.rs` — and `gdrender3d` consumes them. But
//! ClassDB knew about exactly one of them (`ProceduralSkyMaterial`), so
//! `ClassDB.instantiate("Environment")` failed, introspection returned nothing,
//! and scripted environment setup was impossible.
//!
//! Property names, types and defaults are transcribed from the vendored
//! `upstream/godot/doc/classes/*.xml`, which is the oracle rather than
//! recollection — `glow_intensity` is `0.3`, not the `0.8` that "sounds right".

use gdobject::class_db::{can_instantiate, get_property_list};
// `set_property` / `get_property` reach `GenericObject` through the
// `GodotObject` **trait** (object.rs:289/293 delegate to `self.base`); the
// inherent `pub fn`s at object.rs:117/128 are on `ObjectBase`, not on
// `GenericObject`. Without this import the calls are E0599.
use gdobject::GodotObject;
use gdobject::{instantiate, is_parent_class, register_3d_classes};
use gdvariant::Variant;

/// The ten classes the bead names.
const CLASSES: [&str; 10] = [
    "Environment",
    "Sky",
    "PanoramaSkyMaterial",
    "PhysicalSkyMaterial",
    "World3D",
    "CameraAttributes",
    "CameraAttributesPractical",
    "CameraAttributesPhysical",
    "Compositor",
    "CompositorEffect",
];

/// The registered default for `class.property`, or `None` if absent.
fn default_of(class: &str, property: &str) -> Option<Variant> {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** All ten instantiable, Environment round-trips a
/// property, and spot-checked defaults match the Godot XML.
#[test]
fn rendering_resource_classes_registered() {
    register_3d_classes();

    // -- (a) every class is instantiable ------------------------------------
    for class in CLASSES {
        assert!(
            can_instantiate(class),
            "ClassDB.can_instantiate({class}) must be true",
        );
        assert!(
            instantiate(class).is_some(),
            "{class} must actually instantiate",
        );
    }

    // -- (b) Environment round-trips a property -----------------------------
    //
    // The default is asserted first on purpose: if `background_color` were
    // unregistered, both the write and the read would be Nil and a bare
    // round-trip assertion would pass while proving nothing.
    assert_eq!(
        default_of("Environment", "background_color"),
        Some(Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0))),
        "background_color must be registered with Godot's black default",
    );

    let mut env = instantiate("Environment").expect("Environment instantiates");
    let wanted = gdcore::math::Color::new(0.25, 0.5, 0.75, 1.0);
    env.set_property("background_color", Variant::Color(wanted));
    assert_eq!(
        env.get_property("background_color"),
        Variant::Color(wanted),
        "background_color must round-trip through an instantiated Environment",
    );

    // -- (c) spot-checked defaults match upstream/godot/doc/classes/*.xml ----
    let spot: [(&str, &str, Variant); 12] = [
        ("Environment", "background_mode", Variant::Int(0)),
        (
            "Environment",
            "background_energy_multiplier",
            Variant::Float(1.0),
        ),
        ("Environment", "glow_enabled", Variant::Bool(false)),
        ("Environment", "glow_intensity", Variant::Float(0.3)),
        ("Environment", "fog_density", Variant::Float(0.01)),
        ("Environment", "adjustment_brightness", Variant::Float(1.0)),
        ("Sky", "radiance_size", Variant::Int(3)),
        ("PanoramaSkyMaterial", "filter", Variant::Bool(true)),
        ("PhysicalSkyMaterial", "turbidity", Variant::Float(10.0)),
        (
            "PhysicalSkyMaterial",
            "mie_eccentricity",
            Variant::Float(0.8),
        ),
        (
            "CameraAttributes",
            "exposure_sensitivity",
            Variant::Float(100.0),
        ),
        (
            "CameraAttributesPhysical",
            "frustum_focal_length",
            Variant::Float(35.0),
        ),
    ];
    for (class, prop, expected) in spot {
        assert_eq!(
            default_of(class, prop).as_ref(),
            Some(&expected),
            "{class}.{prop} default must match the Godot XML",
        );
    }
}

// ===========================================================================
// Inheritance and surface
// ===========================================================================

/// The inheritance chains are what make these usable as resources — a Sky
/// material that is not a `Material` cannot be assigned where one is expected.
#[test]
fn rendering_resources_inherit_correctly() {
    register_3d_classes();

    for class in [
        "Environment",
        "Sky",
        "World3D",
        "CameraAttributes",
        "Compositor",
    ] {
        assert!(
            is_parent_class(class, "Resource"),
            "{class} must derive from Resource",
        );
    }
    for material in ["PanoramaSkyMaterial", "PhysicalSkyMaterial"] {
        assert!(
            is_parent_class(material, "Material"),
            "{material} must derive from Material",
        );
    }
    for sub in ["CameraAttributesPractical", "CameraAttributesPhysical"] {
        assert!(
            is_parent_class(sub, "CameraAttributes"),
            "{sub} must derive from CameraAttributes so it inherits exposure",
        );
        // Inherited, not re-declared: exposure lives on the base.
        assert!(
            is_parent_class(sub, "Resource"),
            "{sub} must still resolve up to Resource",
        );
    }
}

/// Environment carries its whole authored surface, not just the handful the
/// renderer happens to read today — the inspector and `.tscn` round-tripping
/// both depend on the full list.
#[test]
fn environment_carries_its_full_property_surface() {
    register_3d_classes();
    let props = get_property_list("Environment");
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();

    // One from each authored group in Environment.xml.
    for expected in [
        "background_mode",
        "ambient_light_source",
        "reflected_light_source",
        "tonemap_mode",
        "ssr_enabled",
        "ssao_enabled",
        "ssil_enabled",
        "sdfgi_enabled",
        "glow_enabled",
        "fog_enabled",
        "volumetric_fog_enabled",
        "adjustment_enabled",
    ] {
        assert!(
            names.contains(&expected),
            "Environment must register `{expected}`; it has {} properties",
            names.len(),
        );
    }
    assert!(
        props.len() >= 90,
        "Environment's surface is ~100 properties in Godot, got {}",
        props.len(),
    );
}

/// Object-valued members register as `Nil` rather than being dropped — a
/// `.tscn` assigning `sky_material = SubResource(...)` needs somewhere to land.
#[test]
fn object_valued_members_are_registered_as_nil() {
    register_3d_classes();
    for (class, prop) in [
        ("Sky", "sky_material"),
        ("World3D", "environment"),
        ("World3D", "camera_attributes"),
        ("PanoramaSkyMaterial", "panorama"),
        ("Environment", "sky"),
    ] {
        assert_eq!(
            default_of(class, prop),
            Some(Variant::Nil),
            "{class}.{prop} must be registered (defaulting to Nil)",
        );
    }
}

/// `Compositor.compositor_effects` is an array, not a single object — getting
/// this wrong would silently accept one effect and drop the rest.
#[test]
fn compositor_effects_is_an_array() {
    register_3d_classes();
    assert_eq!(
        default_of("Compositor", "compositor_effects"),
        Some(Variant::Array(Default::default())),
        "Compositor.compositor_effects must default to an empty array",
    );
}
