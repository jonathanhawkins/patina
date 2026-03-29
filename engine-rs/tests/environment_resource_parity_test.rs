//! pat-9vcaf: Environment resource with ambient light, fog, and tonemap settings.
//!
//! Validates that Environment3D types have correct Godot-matching defaults,
//! enum conversions roundtrip, from_properties/to_properties bridge works
//! correctly, and `.tres` files containing environment resources parse and
//! roundtrip without data loss.

use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdresource::saver::TresSaver;
use gdserver3d::environment::{AmbientSource, BackgroundMode, Environment3D, ToneMapper};
use gdserver3d::sky::{Sky, SkyMaterial};
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
// .tres parsing — Environment
// ===========================================================================

#[test]
fn parse_environment_tres_fixture() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_environment.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Environment");
    assert_eq!(res.get_property("background_mode"), Some(&Variant::Int(0)));
}

#[test]
fn parse_environment_with_ambient_light() {
    let res = parse_tres(
        r#"[gd_resource type="Environment" format=3]

[resource]
background_mode = 0
ambient_light_source = 2
ambient_light_color = Color(0.5, 0.5, 0.5, 1)
ambient_light_energy = 0.75
"#,
    );
    assert_eq!(res.class_name, "Environment");
    assert_eq!(
        res.get_property("ambient_light_source"),
        Some(&Variant::Int(2))
    );
    match res.get_property("ambient_light_color") {
        Some(Variant::Color(c)) => {
            assert!((c.r - 0.5).abs() < 1e-3);
            assert!((c.g - 0.5).abs() < 1e-3);
        }
        other => panic!("expected Color, got {other:?}"),
    }
    assert_eq!(
        res.get_property("ambient_light_energy"),
        Some(&Variant::Float(0.75))
    );
}

#[test]
fn parse_environment_with_fog() {
    let res = parse_tres(
        r#"[gd_resource type="Environment" format=3]

[resource]
fog_enabled = true
fog_light_color = Color(0.518, 0.553, 0.608, 1)
fog_density = 0.05
"#,
    );
    assert_eq!(res.get_property("fog_enabled"), Some(&Variant::Bool(true)));
    assert_eq!(res.get_property("fog_density"), Some(&Variant::Float(0.05)));
    match res.get_property("fog_light_color") {
        Some(Variant::Color(c)) => {
            assert!((c.r - 0.518).abs() < 1e-3);
        }
        other => panic!("expected Color, got {other:?}"),
    }
}

#[test]
fn parse_environment_with_tonemap() {
    let res = parse_tres(
        r#"[gd_resource type="Environment" format=3]

[resource]
tonemap_mode = 3
"#,
    );
    assert_eq!(res.get_property("tonemap_mode"), Some(&Variant::Int(3)));
}

#[test]
fn parse_environment_sky_background() {
    let res = parse_tres(
        r#"[gd_resource type="Environment" format=3]

[resource]
background_mode = 2
sky_custom_fov = 60.0
ambient_light_source = 3
tonemap_mode = 2
fog_enabled = true
fog_density = 0.02
"#,
    );
    assert_eq!(res.get_property("background_mode"), Some(&Variant::Int(2)));
    assert_eq!(
        res.get_property("sky_custom_fov"),
        Some(&Variant::Float(60.0))
    );
    assert_eq!(
        res.get_property("ambient_light_source"),
        Some(&Variant::Int(3))
    );
}

// ===========================================================================
// Roundtrip fidelity
// ===========================================================================

#[test]
fn roundtrip_environment_all_fields() {
    let mut r = Resource::new("Environment");
    r.set_property("background_mode", Variant::Int(2));
    r.set_property(
        "background_color",
        Variant::Color(gdcore::math::Color::new(0.1, 0.2, 0.3, 1.0)),
    );
    r.set_property("background_energy_multiplier", Variant::Float(1.5));
    r.set_property("sky_custom_fov", Variant::Float(45.0));
    r.set_property("ambient_light_source", Variant::Int(2));
    r.set_property(
        "ambient_light_color",
        Variant::Color(gdcore::math::Color::new(0.5, 0.5, 0.5, 1.0)),
    );
    r.set_property("ambient_light_energy", Variant::Float(0.8));
    r.set_property("tonemap_mode", Variant::Int(3));
    r.set_property("fog_enabled", Variant::Bool(true));
    r.set_property(
        "fog_light_color",
        Variant::Color(gdcore::math::Color::new(0.9, 0.9, 0.9, 1.0)),
    );
    r.set_property("fog_density", Variant::Float(0.03));

    let reloaded = roundtrip(&r);
    assert_eq!(reloaded.class_name, "Environment");
    assert_eq!(
        reloaded.get_property("background_mode"),
        Some(&Variant::Int(2))
    );
    assert_eq!(
        reloaded.get_property("tonemap_mode"),
        Some(&Variant::Int(3))
    );
    assert_eq!(
        reloaded.get_property("fog_enabled"),
        Some(&Variant::Bool(true))
    );
    assert_eq!(
        reloaded.get_property("ambient_light_source"),
        Some(&Variant::Int(2))
    );
}

#[test]
fn roundtrip_environment_minimal() {
    let mut r = Resource::new("Environment");
    r.set_property("background_mode", Variant::Int(0));
    let reloaded = roundtrip(&r);
    assert_eq!(reloaded.class_name, "Environment");
    assert_eq!(
        reloaded.get_property("background_mode"),
        Some(&Variant::Int(0))
    );
}

// ===========================================================================
// Environment3D typed struct — from_properties
// ===========================================================================

#[test]
fn from_properties_ambient_light_settings() {
    let cyan = gdcore::math::Color::new(0.0, 1.0, 1.0, 1.0);
    let props: Vec<(&str, Variant)> = vec![
        ("ambient_light_source", Variant::Int(2)),
        ("ambient_light_color", Variant::Color(cyan)),
        ("ambient_light_energy", Variant::Float(0.75)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.ambient_source, AmbientSource::Color);
    assert_eq!(env.ambient_color, cyan);
    assert!((env.ambient_energy - 0.75).abs() < 1e-5);
}

#[test]
fn from_properties_fog_settings() {
    let fog_color = gdcore::math::Color::new(0.8, 0.8, 0.9, 1.0);
    let props: Vec<(&str, Variant)> = vec![
        ("fog_enabled", Variant::Bool(true)),
        ("fog_light_color", Variant::Color(fog_color)),
        ("fog_density", Variant::Float(0.05)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert!(env.fog_enabled);
    assert_eq!(env.fog_light_color, fog_color);
    assert!((env.fog_density - 0.05).abs() < 1e-5);
}

#[test]
fn from_properties_tonemap_aces() {
    let props: Vec<(&str, Variant)> = vec![("tonemap_mode", Variant::Int(3))];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.tone_mapper, ToneMapper::Aces);
}

#[test]
fn from_properties_tonemap_filmic() {
    let props: Vec<(&str, Variant)> = vec![("tonemap_mode", Variant::Int(2))];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.tone_mapper, ToneMapper::Filmic);
}

#[test]
fn from_properties_tonemap_reinhard() {
    let props: Vec<(&str, Variant)> = vec![("tonemap_mode", Variant::Int(1))];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.tone_mapper, ToneMapper::Reinhard);
}

#[test]
fn from_properties_background_custom_color() {
    let bg = gdcore::math::Color::new(0.2, 0.3, 0.4, 1.0);
    let props: Vec<(&str, Variant)> = vec![
        ("background_mode", Variant::Int(1)),
        ("background_color", Variant::Color(bg)),
        ("background_energy_multiplier", Variant::Float(1.5)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.background_mode, BackgroundMode::CustomColor);
    assert_eq!(env.background_color, bg);
    assert!((env.background_energy_multiplier - 1.5).abs() < 1e-5);
}

#[test]
fn from_properties_sky_background() {
    let props: Vec<(&str, Variant)> = vec![
        ("background_mode", Variant::Int(2)),
        ("sky_custom_fov", Variant::Float(60.0)),
        ("ambient_light_source", Variant::Int(3)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.background_mode, BackgroundMode::Sky);
    assert!((env.sky_custom_fov - 60.0).abs() < 1e-5);
    assert_eq!(env.ambient_source, AmbientSource::Sky);
}

// ===========================================================================
// Environment3D — to_properties
// ===========================================================================

#[test]
fn to_properties_default_is_empty() {
    let env = Environment3D::default();
    let props = env.to_properties();
    assert!(
        props.is_empty(),
        "default environment should emit no properties"
    );
}

#[test]
fn to_properties_roundtrip_full() {
    let env = Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: gdcore::math::Color::new(0.1, 0.2, 0.3, 1.0),
        ambient_source: AmbientSource::Color,
        ambient_color: gdcore::math::Color::new(0.5, 0.5, 0.5, 1.0),
        ambient_energy: 0.8,
        tone_mapper: ToneMapper::Aces,
        fog_enabled: true,
        fog_light_color: gdcore::math::Color::new(0.9, 0.9, 0.9, 1.0),
        fog_density: 0.03,
        ..Default::default()
    };
    let props = env.to_properties();
    let reconstructed = Environment3D::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
    assert_eq!(reconstructed.background_mode, env.background_mode);
    assert_eq!(reconstructed.ambient_source, env.ambient_source);
    assert_eq!(reconstructed.ambient_color, env.ambient_color);
    assert!((reconstructed.ambient_energy - env.ambient_energy).abs() < 1e-5);
    assert_eq!(reconstructed.tone_mapper, env.tone_mapper);
    assert!(reconstructed.fog_enabled);
    assert_eq!(reconstructed.fog_light_color, env.fog_light_color);
    assert!((reconstructed.fog_density - env.fog_density).abs() < 1e-5);
}

#[test]
fn to_properties_only_emits_non_default() {
    let env = Environment3D {
        fog_enabled: true,
        ..Default::default()
    };
    let props = env.to_properties();
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].0, "fog_enabled");
    assert_eq!(props[0].1, Variant::Bool(true));
}

// ===========================================================================
// Environment3D defaults match Godot
// ===========================================================================

#[test]
fn default_environment_matches_godot() {
    let env = Environment3D::default();
    assert_eq!(env.background_mode, BackgroundMode::ClearColor);
    assert_eq!(env.background_color, gdcore::math::Color::BLACK);
    assert!((env.background_energy_multiplier - 1.0).abs() < 1e-5);
    assert!(env.sky.is_none());
    assert!((env.sky_custom_fov - 0.0).abs() < 1e-5);
    assert_eq!(env.ambient_source, AmbientSource::Background);
    assert_eq!(env.ambient_color, gdcore::math::Color::BLACK);
    assert!((env.ambient_energy - 1.0).abs() < 1e-5);
    assert_eq!(env.tone_mapper, ToneMapper::Linear);
    assert!(!env.fog_enabled);
    assert!((env.fog_density - 0.01).abs() < 1e-5);
}

// ===========================================================================
// Enum roundtrips
// ===========================================================================

#[test]
fn background_mode_all_variants_roundtrip() {
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
fn ambient_source_all_variants_roundtrip() {
    for (i, expected) in [
        AmbientSource::Background,
        AmbientSource::Disabled,
        AmbientSource::Color,
        AmbientSource::Sky,
    ]
    .iter()
    .enumerate()
    {
        let mode = AmbientSource::from_godot_int(i as i64);
        assert_eq!(&mode, expected);
        assert_eq!(mode.to_godot_int(), i as i64);
    }
}

#[test]
fn tone_mapper_all_variants_roundtrip() {
    for (i, expected) in [
        ToneMapper::Linear,
        ToneMapper::Reinhard,
        ToneMapper::Filmic,
        ToneMapper::Aces,
    ]
    .iter()
    .enumerate()
    {
        let mode = ToneMapper::from_godot_int(i as i64);
        assert_eq!(&mode, expected);
        assert_eq!(mode.to_godot_int(), i as i64);
    }
}

#[test]
fn unknown_enum_values_fallback() {
    assert_eq!(
        BackgroundMode::from_godot_int(99),
        BackgroundMode::ClearColor
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

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn from_properties_ignores_unknown_keys() {
    let props: Vec<(&str, Variant)> = vec![
        ("unknown_prop", Variant::Int(42)),
        ("fog_enabled", Variant::Bool(true)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert!(env.fog_enabled);
    assert_eq!(env.background_mode, BackgroundMode::ClearColor);
}

#[test]
fn from_properties_wrong_variant_type_ignored() {
    // Pass a string where int is expected — should be silently ignored
    let props: Vec<(&str, Variant)> = vec![
        ("background_mode", Variant::String("not_an_int".to_string())),
        ("fog_enabled", Variant::Bool(true)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
    assert_eq!(env.background_mode, BackgroundMode::ClearColor); // kept default
    assert!(env.fog_enabled); // correct type worked
}

#[test]
fn environment_with_sky_ref() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        ambient_source: AmbientSource::Sky,
        ..Default::default()
    };
    assert!(env.sky.is_some());
    let sky = env.sky.as_ref().unwrap();
    assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
}

#[test]
fn environment_clone_preserves_all_fields() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        fog_enabled: true,
        fog_density: 0.05,
        tone_mapper: ToneMapper::Aces,
        ambient_source: AmbientSource::Color,
        ambient_color: gdcore::math::Color::new(0.3, 0.3, 0.3, 1.0),
        ambient_energy: 0.5,
        ..Default::default()
    };
    let cloned = env.clone();
    assert_eq!(env, cloned);
}
