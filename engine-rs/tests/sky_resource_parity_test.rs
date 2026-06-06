//! pat-zciwl: Sky resource with panoramic and procedural sky.
//!
//! Validates that Sky, ProceduralSkyMaterial, PanoramicSkyMaterial, and
//! Environment3D types have correct defaults matching Godot, that enum
//! conversions roundtrip correctly, and that `.tres` files containing
//! sky resources parse and roundtrip without data loss.

use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdresource::saver::TresSaver;
use gdserver3d::environment::{AmbientSource, BackgroundMode, Environment3D, ToneMapper};
use gdserver3d::sky::{
    PanoramicSkyMaterial, PhysicalSkyMaterial, ProceduralSkyMaterial, Sky, SkyMaterial,
    SkyProcessMode,
};
use gdvariant::Variant;
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

fn parse_tres(content: &str) -> Arc<Resource> {
    let loader = TresLoader::new();
    loader.parse_str(content, "test://inline").unwrap()
}

fn roundtrip(resource: &Resource) -> Arc<Resource> {
    let saver = TresSaver::new();
    let serialized = saver.save_to_string(resource).unwrap();
    let loader = TresLoader::new();
    loader.parse_str(&serialized, "test://roundtrip").unwrap()
}

// ===========================================================================
// Sky resource .tres parsing
// ===========================================================================

#[test]
fn parse_sky_tres_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_sky.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Sky");
    assert_eq!(res.get_property("process_mode"), Some(&Variant::Int(0)));
    assert_eq!(res.get_property("radiance_size"), Some(&Variant::Int(3)));
}

#[test]
fn parse_inline_sky_resource() {
    let res = parse_tres(
        r#"[gd_resource type="Sky" format=3]

[resource]
process_mode = 2
radiance_size = 4
"#,
    );
    assert_eq!(res.class_name, "Sky");
    assert_eq!(res.get_property("process_mode"), Some(&Variant::Int(2)));
    assert_eq!(res.get_property("radiance_size"), Some(&Variant::Int(4)));
}

#[test]
fn parse_procedural_sky_material() {
    let res = parse_tres(
        r#"[gd_resource type="ProceduralSkyMaterial" format=3]

[resource]
sky_top_color = Color(0.385, 0.454, 0.55, 1)
sky_horizon_color = Color(0.646, 0.654, 0.67, 1)
sky_curve = 0.15
ground_bottom_color = Color(0.2, 0.169, 0.133, 1)
ground_horizon_color = Color(0.646, 0.654, 0.67, 1)
ground_curve = 0.02
sun_angle_max = 30.0
"#,
    );
    assert_eq!(res.class_name, "ProceduralSkyMaterial");
    match res.get_property("sky_top_color") {
        Some(Variant::Color(c)) => {
            assert!((c.r - 0.385).abs() < 1e-3);
            assert!((c.g - 0.454).abs() < 1e-3);
        }
        other => panic!("expected Color, got {other:?}"),
    }
    assert_eq!(res.get_property("sky_curve"), Some(&Variant::Float(0.15)));
}

#[test]
fn parse_panoramic_sky_material() {
    let res = parse_tres(
        r#"[gd_resource type="PanoramaSkyMaterial" format=3]

[resource]
filter = true
energy_multiplier = 1.5
"#,
    );
    assert_eq!(res.class_name, "PanoramaSkyMaterial");
    assert_eq!(
        res.get_property("energy_multiplier"),
        Some(&Variant::Float(1.5))
    );
}

#[test]
fn parse_environment_with_sky_background() {
    let res = parse_tres(
        r#"[gd_resource type="Environment" format=3]

[resource]
background_mode = 2
ambient_light_source = 3
tonemap_mode = 3
fog_enabled = true
fog_light_color = Color(0.518, 0.553, 0.608, 1)
fog_density = 0.01
"#,
    );
    assert_eq!(res.class_name, "Environment");
    assert_eq!(res.get_property("background_mode"), Some(&Variant::Int(2)));
    assert_eq!(
        res.get_property("ambient_light_source"),
        Some(&Variant::Int(3))
    );
    assert_eq!(res.get_property("tonemap_mode"), Some(&Variant::Int(3)));
    assert_eq!(res.get_property("fog_enabled"), Some(&Variant::Bool(true)));
}

// ===========================================================================
// Roundtrip fidelity
// ===========================================================================

#[test]
fn roundtrip_sky_resource() {
    let mut r = Resource::new("Sky");
    r.set_property("process_mode", Variant::Int(1));
    r.set_property("radiance_size", Variant::Int(4));
    let reloaded = roundtrip(&r);
    assert_eq!(reloaded.class_name, "Sky");
    assert_eq!(
        reloaded.get_property("process_mode"),
        Some(&Variant::Int(1))
    );
    assert_eq!(
        reloaded.get_property("radiance_size"),
        Some(&Variant::Int(4))
    );
}

#[test]
fn roundtrip_procedural_sky_material() {
    let mut r = Resource::new("ProceduralSkyMaterial");
    r.set_property("sky_curve", Variant::Float(0.25));
    r.set_property("sun_angle_max", Variant::Float(45.0));
    let reloaded = roundtrip(&r);
    assert_eq!(reloaded.class_name, "ProceduralSkyMaterial");
    assert_eq!(
        reloaded.get_property("sky_curve"),
        Some(&Variant::Float(0.25))
    );
    assert_eq!(
        reloaded.get_property("sun_angle_max"),
        Some(&Variant::Float(45.0))
    );
}

#[test]
fn roundtrip_panoramic_sky_material() {
    let mut r = Resource::new("PanoramaSkyMaterial");
    r.set_property("filter", Variant::Bool(false));
    r.set_property("energy_multiplier", Variant::Float(2.0));
    let reloaded = roundtrip(&r);
    assert_eq!(reloaded.class_name, "PanoramaSkyMaterial");
    assert_eq!(reloaded.get_property("filter"), Some(&Variant::Bool(false)));
}

#[test]
fn roundtrip_environment_with_sky_mode() {
    let mut r = Resource::new("Environment");
    r.set_property("background_mode", Variant::Int(2));
    r.set_property("ambient_light_source", Variant::Int(3));
    r.set_property("fog_enabled", Variant::Bool(true));
    r.set_property("fog_density", Variant::Float(0.05));
    let reloaded = roundtrip(&r);
    assert_eq!(
        reloaded.get_property("background_mode"),
        Some(&Variant::Int(2))
    );
    assert_eq!(
        reloaded.get_property("fog_enabled"),
        Some(&Variant::Bool(true))
    );
}

// ===========================================================================
// Sky type unit tests
// ===========================================================================

#[test]
fn sky_default_matches_godot() {
    let sky = Sky::default();
    assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
    assert_eq!(sky.process_mode, SkyProcessMode::Automatic);
    assert_eq!(sky.radiance_size, 256);
}

#[test]
fn procedural_sky_material_defaults() {
    let mat = ProceduralSkyMaterial::default();
    // Godot defaults
    assert!((mat.sky_curve - 0.15).abs() < 1e-5);
    assert!((mat.ground_curve - 0.02).abs() < 1e-5);
    assert!((mat.sun_angle_max - 30.0).abs() < 1e-5);
    assert!((mat.sky_energy_multiplier - 1.0).abs() < 1e-5);
    assert!((mat.ground_energy_multiplier - 1.0).abs() < 1e-5);
}

#[test]
fn panoramic_sky_material_defaults() {
    let mat = PanoramicSkyMaterial::default();
    assert!(mat.panorama_path.is_empty());
    assert!(mat.filter);
    assert!((mat.energy_multiplier - 1.0).abs() < 1e-5);
}

#[test]
fn physical_sky_material_defaults() {
    let mat = PhysicalSkyMaterial::default();
    assert!((mat.rayleigh_coefficient - 2.0).abs() < 1e-5);
    assert!((mat.mie_eccentricity - 0.8).abs() < 1e-5);
    assert!((mat.turbidity - 10.0).abs() < 1e-5);
}

#[test]
fn environment_default_matches_godot() {
    let env = Environment3D::default();
    assert_eq!(env.background_mode, BackgroundMode::ClearColor);
    assert!(env.sky.is_none());
    assert_eq!(env.ambient_source, AmbientSource::Background);
    assert_eq!(env.tone_mapper, ToneMapper::Linear);
    assert!(!env.fog_enabled);
    assert!((env.fog_density - 0.01).abs() < 1e-5);
}

#[test]
fn sky_process_mode_all_variants() {
    for (i, expected) in [
        SkyProcessMode::Automatic,
        SkyProcessMode::Quality,
        SkyProcessMode::Incremental,
        SkyProcessMode::RealTime,
    ]
    .iter()
    .enumerate()
    {
        let mode = SkyProcessMode::from_godot_int(i as i64);
        assert_eq!(&mode, expected);
        assert_eq!(mode.to_godot_int(), i as i64);
    }
}

#[test]
fn background_mode_all_variants() {
    for (i, expected) in [
        BackgroundMode::ClearColor,
        BackgroundMode::CustomColor,
        BackgroundMode::Sky,
        BackgroundMode::Canvas,
        BackgroundMode::Keep,
        BackgroundMode::CameraFeed,
    ]
    .iter()
    .enumerate()
    {
        let mode = BackgroundMode::from_godot_int(i as i64);
        assert_eq!(&mode, expected);
        assert_eq!(mode.to_godot_int(), i as i64);
    }
}

#[test]
fn environment_sky_composition() {
    let sky = Sky {
        material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
            panorama_path: "res://sky.hdr".to_string(),
            filter: true,
            energy_multiplier: 1.2,
        }),
        process_mode: SkyProcessMode::Quality,
        radiance_size: 512,
    };
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(sky),
        sky_custom_fov: 60.0,
        ambient_source: AmbientSource::Sky,
        tone_mapper: ToneMapper::Aces,
        ..Default::default()
    };
    assert_eq!(env.background_mode, BackgroundMode::Sky);
    assert_eq!(env.tone_mapper, ToneMapper::Aces);
    let sky = env.sky.as_ref().unwrap();
    assert!(matches!(sky.material, SkyMaterial::Panoramic(_)));
    if let SkyMaterial::Panoramic(ref pan) = sky.material {
        assert_eq!(pan.panorama_path, "res://sky.hdr");
    }
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn unknown_enum_values_fall_back_to_defaults() {
    assert_eq!(
        SkyProcessMode::from_godot_int(99),
        SkyProcessMode::Automatic
    );
    assert_eq!(
        BackgroundMode::from_godot_int(-1),
        BackgroundMode::ClearColor
    );
    assert_eq!(
        AmbientSource::from_godot_int(100),
        AmbientSource::Background
    );
    assert_eq!(ToneMapper::from_godot_int(42), ToneMapper::Linear);
}

#[test]
fn sky_with_physical_material() {
    let sky = Sky {
        material: SkyMaterial::Physical(PhysicalSkyMaterial {
            turbidity: 5.0,
            sun_disk_scale: 2.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    if let SkyMaterial::Physical(ref phys) = sky.material {
        assert!((phys.turbidity - 5.0).abs() < 1e-5);
        assert!((phys.sun_disk_scale - 2.0).abs() < 1e-5);
    } else {
        panic!("expected Physical sky material");
    }
}

#[test]
fn parse_sky_with_subresource() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_sky.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    // The sub-resource should be stored on the resource
    assert!(!res.subresources.is_empty() || res.get_property("sky_material").is_some());
}
