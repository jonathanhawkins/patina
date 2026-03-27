//! Integration tests for Sky resource with panoramic and procedural sky.
//!
//! Validates:
//! - ProceduralSkyMaterial from_properties/to_properties roundtrip
//! - PanoramicSkyMaterial from_properties/to_properties roundtrip
//! - PhysicalSkyMaterial from_properties/to_properties roundtrip
//! - Sky from_properties with material type selection
//! - Sky to_properties serialisation
//! - Environment3D integration with Sky
//! - SkyProcessMode enum roundtrip
//! - Default values match Godot

use gdcore::math::Color;
use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::sky::{
    PanoramicSkyMaterial, PhysicalSkyMaterial, ProceduralSkyMaterial, Sky, SkyMaterial,
    SkyProcessMode,
};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// ProceduralSkyMaterial
// ---------------------------------------------------------------------------

#[test]
fn procedural_sky_defaults_match_godot() {
    let mat = ProceduralSkyMaterial::default();
    // Godot 4.x defaults
    assert!((mat.sky_curve - 0.15).abs() < 1e-5);
    assert!((mat.sky_energy_multiplier - 1.0).abs() < 1e-5);
    assert!((mat.ground_curve - 0.02).abs() < 1e-5);
    assert!((mat.ground_energy_multiplier - 1.0).abs() < 1e-5);
    assert!((mat.sun_angle_max - 30.0).abs() < 1e-5);
    assert!((mat.sun_curve - 0.15).abs() < 1e-5);
}

#[test]
fn procedural_sky_from_properties_custom_colors() {
    let top = Color::new(0.0, 0.0, 1.0, 1.0);
    let horizon = Color::new(0.8, 0.8, 1.0, 1.0);
    let props: Vec<(&str, Variant)> = vec![
        ("sky_top_color", Variant::Color(top)),
        ("sky_horizon_color", Variant::Color(horizon)),
        ("sky_curve", Variant::Float(0.25)),
        ("sun_angle_max", Variant::Float(45.0)),
    ];
    let mat =
        ProceduralSkyMaterial::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(mat.sky_top_color, top);
    assert_eq!(mat.sky_horizon_color, horizon);
    assert!((mat.sky_curve - 0.25).abs() < 1e-5);
    assert!((mat.sun_angle_max - 45.0).abs() < 1e-5);
    // Unchanged properties remain default
    assert!((mat.sky_energy_multiplier - 1.0).abs() < 1e-5);
}

#[test]
fn procedural_sky_to_properties_default_is_empty() {
    let mat = ProceduralSkyMaterial::default();
    let props = mat.to_properties();
    assert!(
        props.is_empty(),
        "default procedural sky should emit no properties, got {:?}",
        props
    );
}

#[test]
fn procedural_sky_properties_roundtrip() {
    let top = Color::new(0.1, 0.2, 0.9, 1.0);
    let mat = ProceduralSkyMaterial {
        sky_top_color: top,
        sky_curve: 0.3,
        sun_angle_max: 60.0,
        ..Default::default()
    };
    let props = mat.to_properties();
    let restored =
        ProceduralSkyMaterial::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
    assert_eq!(restored.sky_top_color, top);
    assert!((restored.sky_curve - 0.3).abs() < 1e-5);
    assert!((restored.sun_angle_max - 60.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// PanoramicSkyMaterial
// ---------------------------------------------------------------------------

#[test]
fn panoramic_sky_defaults() {
    let mat = PanoramicSkyMaterial::default();
    assert!(mat.panorama_path.is_empty());
    assert!(mat.filter);
    assert!((mat.energy_multiplier - 1.0).abs() < 1e-5);
}

#[test]
fn panoramic_sky_from_properties() {
    let props: Vec<(&str, Variant)> = vec![
        ("panorama", Variant::String("res://sky.hdr".into())),
        ("filter", Variant::Bool(false)),
        ("energy_multiplier", Variant::Float(1.5)),
    ];
    let mat =
        PanoramicSkyMaterial::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(mat.panorama_path, "res://sky.hdr");
    assert!(!mat.filter);
    assert!((mat.energy_multiplier - 1.5).abs() < 1e-5);
}

#[test]
fn panoramic_sky_properties_roundtrip() {
    let mat = PanoramicSkyMaterial {
        panorama_path: "res://env/sunset.exr".into(),
        filter: false,
        energy_multiplier: 2.0,
    };
    let props = mat.to_properties();
    let restored =
        PanoramicSkyMaterial::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
    assert_eq!(restored.panorama_path, "res://env/sunset.exr");
    assert!(!restored.filter);
    assert!((restored.energy_multiplier - 2.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// PhysicalSkyMaterial
// ---------------------------------------------------------------------------

#[test]
fn physical_sky_defaults_match_godot() {
    let mat = PhysicalSkyMaterial::default();
    assert!((mat.rayleigh_coefficient - 2.0).abs() < 1e-5);
    assert!((mat.mie_coefficient - 0.005).abs() < 1e-5);
    assert!((mat.mie_eccentricity - 0.8).abs() < 1e-5);
    assert!((mat.turbidity - 10.0).abs() < 1e-5);
    assert!((mat.sun_disk_scale - 1.0).abs() < 1e-5);
    assert!((mat.energy_multiplier - 1.0).abs() < 1e-5);
}

#[test]
fn physical_sky_from_properties() {
    let props: Vec<(&str, Variant)> = vec![
        ("rayleigh_coefficient", Variant::Float(3.0)),
        ("turbidity", Variant::Float(5.0)),
        ("mie_eccentricity", Variant::Float(0.9)),
    ];
    let mat =
        PhysicalSkyMaterial::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert!((mat.rayleigh_coefficient - 3.0).abs() < 1e-5);
    assert!((mat.turbidity - 5.0).abs() < 1e-5);
    assert!((mat.mie_eccentricity - 0.9).abs() < 1e-5);
    // Unchanged
    assert!((mat.mie_coefficient - 0.005).abs() < 1e-5);
}

#[test]
fn physical_sky_properties_roundtrip() {
    let mat = PhysicalSkyMaterial {
        rayleigh_coefficient: 1.5,
        turbidity: 8.0,
        ..Default::default()
    };
    let props = mat.to_properties();
    let restored =
        PhysicalSkyMaterial::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
    assert!((restored.rayleigh_coefficient - 1.5).abs() < 1e-5);
    assert!((restored.turbidity - 8.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Sky from_properties / to_properties
// ---------------------------------------------------------------------------

#[test]
fn sky_from_properties_default_is_procedural() {
    let sky = Sky::from_properties(std::iter::empty());
    assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
    assert_eq!(sky.process_mode, SkyProcessMode::Automatic);
    assert_eq!(sky.radiance_size, 256);
}

#[test]
fn sky_from_properties_selects_panoramic() {
    let props: Vec<(&str, Variant)> = vec![
        (
            "sky_material_type",
            Variant::String("PanoramaSkyMaterial".into()),
        ),
        ("panorama", Variant::String("res://sky.hdr".into())),
        ("process_mode", Variant::Int(1)), // Quality
        ("radiance_size", Variant::Int(512)),
    ];
    let sky = Sky::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert!(matches!(sky.material, SkyMaterial::Panoramic(_)));
    if let SkyMaterial::Panoramic(mat) = &sky.material {
        assert_eq!(mat.panorama_path, "res://sky.hdr");
    }
    assert_eq!(sky.process_mode, SkyProcessMode::Quality);
    assert_eq!(sky.radiance_size, 512);
}

#[test]
fn sky_from_properties_selects_physical() {
    let props: Vec<(&str, Variant)> = vec![
        (
            "sky_material_type",
            Variant::String("PhysicalSkyMaterial".into()),
        ),
        ("turbidity", Variant::Float(5.0)),
    ];
    let sky = Sky::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert!(matches!(sky.material, SkyMaterial::Physical(_)));
    if let SkyMaterial::Physical(mat) = &sky.material {
        assert!((mat.turbidity - 5.0).abs() < 1e-5);
    }
}

#[test]
fn sky_to_properties_roundtrip_procedural() {
    let top = Color::new(0.1, 0.2, 0.9, 1.0);
    let sky = Sky {
        material: SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_top_color: top,
            ..Default::default()
        }),
        process_mode: SkyProcessMode::RealTime,
        radiance_size: 1024,
    };
    let props = sky.to_properties();
    let restored = Sky::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
    assert!(matches!(restored.material, SkyMaterial::Procedural(_)));
    if let SkyMaterial::Procedural(mat) = &restored.material {
        assert_eq!(mat.sky_top_color, top);
    }
    assert_eq!(restored.process_mode, SkyProcessMode::RealTime);
    assert_eq!(restored.radiance_size, 1024);
}

#[test]
fn sky_to_properties_roundtrip_panoramic() {
    let sky = Sky {
        material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
            panorama_path: "res://env/night.exr".into(),
            filter: false,
            energy_multiplier: 0.5,
        }),
        process_mode: SkyProcessMode::Quality,
        radiance_size: 512,
    };
    let props = sky.to_properties();
    let restored = Sky::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
    assert!(matches!(restored.material, SkyMaterial::Panoramic(_)));
    if let SkyMaterial::Panoramic(mat) = &restored.material {
        assert_eq!(mat.panorama_path, "res://env/night.exr");
        assert!(!mat.filter);
        assert!((mat.energy_multiplier - 0.5).abs() < 1e-5);
    }
    assert_eq!(restored.process_mode, SkyProcessMode::Quality);
}

// ---------------------------------------------------------------------------
// SkyProcessMode
// ---------------------------------------------------------------------------

#[test]
fn sky_process_mode_all_variants_roundtrip() {
    for (int_val, expected) in [
        (0, SkyProcessMode::Automatic),
        (1, SkyProcessMode::Quality),
        (2, SkyProcessMode::Incremental),
        (3, SkyProcessMode::RealTime),
    ] {
        let mode = SkyProcessMode::from_godot_int(int_val);
        assert_eq!(mode, expected);
        assert_eq!(mode.to_godot_int(), int_val);
    }
}

#[test]
fn sky_process_mode_unknown_defaults() {
    assert_eq!(SkyProcessMode::from_godot_int(99), SkyProcessMode::Automatic);
    assert_eq!(SkyProcessMode::from_godot_int(-1), SkyProcessMode::Automatic);
}

// ---------------------------------------------------------------------------
// Environment3D + Sky integration
// ---------------------------------------------------------------------------

#[test]
fn environment_with_procedural_sky() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        ..Default::default()
    };
    assert_eq!(env.background_mode, BackgroundMode::Sky);
    let sky = env.sky.as_ref().unwrap();
    assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
}

#[test]
fn environment_with_panoramic_sky() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                panorama_path: "res://sky.hdr".into(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(env.sky.is_some());
    if let Some(sky) = &env.sky {
        assert!(matches!(sky.material, SkyMaterial::Panoramic(_)));
        if let SkyMaterial::Panoramic(mat) = &sky.material {
            assert_eq!(mat.panorama_path, "res://sky.hdr");
        }
    }
}

#[test]
fn environment_sky_is_none_by_default() {
    let env = Environment3D::default();
    assert!(env.sky.is_none());
    assert_eq!(env.background_mode, BackgroundMode::ClearColor);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn sky_from_properties_ignores_unknown() {
    let props: Vec<(&str, Variant)> = vec![
        ("unknown_key", Variant::Int(42)),
        ("process_mode", Variant::Int(3)),
    ];
    let sky = Sky::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(sky.process_mode, SkyProcessMode::RealTime);
}

#[test]
fn procedural_sky_from_properties_empty_keeps_defaults() {
    let mat = ProceduralSkyMaterial::from_properties(std::iter::empty());
    assert_eq!(mat, ProceduralSkyMaterial::default());
}

#[test]
fn panoramic_sky_from_properties_empty_keeps_defaults() {
    let mat = PanoramicSkyMaterial::from_properties(std::iter::empty());
    assert_eq!(mat, PanoramicSkyMaterial::default());
}

#[test]
fn physical_sky_from_properties_empty_keeps_defaults() {
    let mat = PhysicalSkyMaterial::from_properties(std::iter::empty());
    assert_eq!(mat, PhysicalSkyMaterial::default());
}

#[test]
fn sky_clone_preserves_material() {
    let sky = Sky {
        material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
            panorama_path: "res://test.hdr".into(),
            ..Default::default()
        }),
        process_mode: SkyProcessMode::Quality,
        radiance_size: 128,
    };
    let cloned = sky.clone();
    assert_eq!(sky, cloned);
}

#[test]
fn all_three_sky_materials_constructible() {
    // Ensures all variants are constructible and distinguishable
    let proc_sky = SkyMaterial::Procedural(ProceduralSkyMaterial::default());
    let pano_sky = SkyMaterial::Panoramic(PanoramicSkyMaterial::default());
    let phys_sky = SkyMaterial::Physical(PhysicalSkyMaterial::default());

    assert!(matches!(proc_sky, SkyMaterial::Procedural(_)));
    assert!(matches!(pano_sky, SkyMaterial::Panoramic(_)));
    assert!(matches!(phys_sky, SkyMaterial::Physical(_)));
    assert_ne!(proc_sky, pano_sky);
    assert_ne!(pano_sky, phys_sky);
}
