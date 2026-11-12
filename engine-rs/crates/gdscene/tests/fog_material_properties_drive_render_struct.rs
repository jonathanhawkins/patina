//! pat-nzgdu.208 — ClassDB's `FogMaterial` and the render struct must agree.
//!
//! Lives in gdscene because it needs both gdobject's ClassDB and gdserver3d's
//! `FogMaterial` — the two crates do not depend on each other (they meet at
//! `gdvariant::Variant`), so *nothing else forces them to stay in step*. This is
//! the same shape as `procedural_sky_material_classdb_registered.rs`
//! (pat-nzgdu.161).
//!
//! Without this test, ClassDB could say `density` defaults to 1.0 while the
//! struct drifted to something else, and every unit test on both sides would
//! still pass.

use gdcore::math::Color;
use gdobject::class_db;
use gdserver3d::fog_volume::{FogMaterial, FogVolume, FogVolumeShape};
use gdvariant::Variant;

fn classdb_default(class: &str, name: &str) -> Variant {
    class_db::register_3d_classes();
    class_db::get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// Every ClassDB default must equal the render struct's `default()`.
#[test]
fn fog_material_classdb_defaults_match_the_render_struct() {
    class_db::register_3d_classes();
    let def = FogMaterial::default();

    match classdb_default("FogMaterial", "density") {
        Variant::Float(v) => assert!(
            (v as f32 - def.density).abs() < 1e-4,
            "ClassDB density {v} != FogMaterial::default().density {}",
            def.density
        ),
        other => panic!("density must be a Float, got {other:?}"),
    }
    match classdb_default("FogMaterial", "albedo") {
        Variant::Color(c) => assert_eq!(c, def.albedo, "ClassDB albedo != struct default"),
        other => panic!("albedo must be a Color, got {other:?}"),
    }
    match classdb_default("FogMaterial", "emission") {
        Variant::Color(c) => assert_eq!(c, def.emission, "ClassDB emission != struct default"),
        other => panic!("emission must be a Color, got {other:?}"),
    }
    match classdb_default("FogMaterial", "height_falloff") {
        Variant::Float(v) => assert!((v as f32 - def.height_falloff).abs() < 1e-4),
        other => panic!("height_falloff must be a Float, got {other:?}"),
    }
    match classdb_default("FogMaterial", "edge_fade") {
        Variant::Float(v) => assert!(
            (v as f32 - def.edge_fade).abs() < 1e-4,
            "ClassDB edge_fade {v} != struct {}",
            def.edge_fade
        ),
        other => panic!("edge_fade must be a Float, got {other:?}"),
    }
}

/// The ClassDB property *names* must be exactly the keys `from_properties`
/// understands. A rename on either side silently stops the binding: the setter
/// would parse, match nothing, and hand back a default — fog that renders, but
/// ignores the scene.
#[test]
fn every_classdb_property_name_is_understood_by_the_binding() {
    class_db::register_3d_classes();

    let base: Vec<String> = class_db::get_property_list("Material")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let own: Vec<String> = class_db::get_property_list("FogMaterial")
        .into_iter()
        .map(|p| p.name)
        .filter(|p| !base.contains(p))
        .collect();
    assert!(!own.is_empty(), "FogMaterial declares properties");

    // A non-default value per property, keyed by the ClassDB name. If
    // from_properties ignores the key, the result stays at default and the
    // assertion below fires.
    let nondefault = |name: &str| -> Variant {
        match name {
            "density" => Variant::Float(7.0),
            "albedo" => Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
            "emission" => Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
            "height_falloff" => Variant::Float(3.0),
            "edge_fade" => Variant::Float(0.9),
            "density_texture" => Variant::String("res://fog_density.png".to_string()),
            other => panic!("unmapped ClassDB property `{other}` — the binding needs a case"),
        }
    };

    for name in &own {
        let value = nondefault(name);
        let mat = FogMaterial::from_properties([(name.as_str(), &value)].into_iter());
        assert_ne!(
            mat,
            FogMaterial::default(),
            "ClassDB exposes `{name}` but FogMaterial::from_properties ignores it — \
             the property would silently do nothing"
        );
    }
}

/// The end the bead cares about: a property bag (as a `.tscn` would supply)
/// drives the fog the renderer samples.
#[test]
fn density_from_a_property_bag_reaches_the_sampled_fog() {
    let density = Variant::Float(6.0);
    let material = FogMaterial::from_properties([("density", &density)].into_iter());

    let mut volume = FogVolume::box_shape(gdcore::math::Vector3::new(4.0, 4.0, 4.0));
    volume.material = material;
    volume.material.edge_fade = 0.0; // isolate density from the edge term

    let sampled = volume.sample_density(gdcore::math::Vector3::ZERO);
    assert!(
        (sampled - 6.0).abs() < 1e-4,
        "density set through the binding must reach sample_density, got {sampled}"
    );
}

/// ⚠️ The two sides must agree on the *node* default shape — BOX.
///
/// This is the bug this bead found. Godot's ctor sets `FOG_VOLUME_SHAPE_BOX`
/// (fog_volume.cpp:139; doc XML `shape` default `3`), but the backend took
/// `FogVolumeShape::default()` = Ellipsoid(0). Because a `.tscn` omits any
/// property equal to Godot's default, a stock FogVolume node carries no `shape`
/// key — so the editor viewport rendered every default fog volume as an
/// ellipsoid where Godot renders a box.
///
/// ⚠️ Two different questions live here, and conflating them caused the bug:
/// the ENUM's zero value is Ellipsoid (correct — `RS::FOG_VOLUME_SHAPE_ELLIPSOID
/// = 0`), while the NODE's default is Box. Both are true at once.
#[test]
fn the_node_default_shape_is_box_on_both_sides() {
    class_db::register_3d_classes();

    assert_eq!(
        classdb_default("FogVolume", "shape"),
        Variant::Int(3),
        "ClassDB follows Godot: FOG_VOLUME_SHAPE_BOX"
    );
    assert_eq!(
        FogVolume::default().shape,
        FogVolumeShape::Box,
        "the render struct must agree with ClassDB, or a stock .tscn fog volume \
         renders the wrong shape"
    );
    assert_eq!(
        FogVolume::default().shape.to_godot_int(),
        match classdb_default("FogVolume", "shape") {
            Variant::Int(v) => v,
            other => panic!("shape must be an Int, got {other:?}"),
        },
        "the two sides must agree as INTEGERS too — that is what a .tscn stores"
    );

    // The enum's zero value is a different question, and is still Ellipsoid.
    assert_eq!(FogVolumeShape::default(), FogVolumeShape::Ellipsoid);
    assert_eq!(FogVolumeShape::Ellipsoid.to_godot_int(), 0);
}
