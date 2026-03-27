//! Environment resource for 3D scenes.
//!
//! Implements Godot's `Environment` resource which controls background
//! rendering (sky, color, or custom), ambient lighting, fog, and
//! tone-mapping for a 3D scene.

use gdcore::math::Color;
use gdvariant::Variant;

use crate::sky::Sky;

/// Background rendering mode.
///
/// Maps to Godot's `Environment.BGMode` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BackgroundMode {
    /// Clear to the clear color (default).
    #[default]
    ClearColor,
    /// Display a custom color.
    CustomColor,
    /// Render a sky.
    Sky,
    /// Use the canvas background.
    Canvas,
    /// Keep the previous frame (no clear).
    Keep,
    /// Use the camera feed.
    CameraFeed,
}

impl BackgroundMode {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::ClearColor,
            1 => Self::CustomColor,
            2 => Self::Sky,
            3 => Self::Canvas,
            4 => Self::Keep,
            5 => Self::CameraFeed,
            _ => Self::ClearColor,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::ClearColor => 0,
            Self::CustomColor => 1,
            Self::Sky => 2,
            Self::Canvas => 3,
            Self::Keep => 4,
            Self::CameraFeed => 5,
        }
    }
}

/// Ambient light source for the environment.
///
/// Maps to Godot's `Environment.AmbientSource` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AmbientSource {
    /// Use the background for ambient light.
    #[default]
    Background,
    /// Disabled.
    Disabled,
    /// Use a flat ambient color.
    Color,
    /// Derive from the sky.
    Sky,
}

impl AmbientSource {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Background,
            1 => Self::Disabled,
            2 => Self::Color,
            3 => Self::Sky,
            _ => Self::Background,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Background => 0,
            Self::Disabled => 1,
            Self::Color => 2,
            Self::Sky => 3,
        }
    }
}

/// Tone-mapping mode.
///
/// Maps to Godot's `Environment.ToneMapper` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToneMapper {
    /// Linear (no tone mapping).
    #[default]
    Linear,
    /// Reinhard tone mapping.
    Reinhard,
    /// Filmic tone mapping.
    Filmic,
    /// ACES filmic tone mapping.
    Aces,
}

impl ToneMapper {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Linear,
            1 => Self::Reinhard,
            2 => Self::Filmic,
            3 => Self::Aces,
            _ => Self::Linear,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Linear => 0,
            Self::Reinhard => 1,
            Self::Filmic => 2,
            Self::Aces => 3,
        }
    }
}

/// A 3D environment resource controlling background, lighting, and
/// post-processing.
///
/// Maps to Godot's `Environment` resource.
#[derive(Debug, Clone, PartialEq)]
pub struct Environment3D {
    /// Background rendering mode.
    pub background_mode: BackgroundMode,
    /// Background color (used when `background_mode` is `CustomColor`).
    pub background_color: Color,
    /// Background energy multiplier.
    pub background_energy_multiplier: f32,
    /// Sky resource (used when `background_mode` is `Sky`).
    pub sky: Option<Sky>,
    /// Custom field-of-view override for the sky (0 = use camera FOV).
    pub sky_custom_fov: f32,
    /// Ambient light source.
    pub ambient_source: AmbientSource,
    /// Ambient light color.
    pub ambient_color: Color,
    /// Ambient light energy.
    pub ambient_energy: f32,
    /// Tone mapping mode.
    pub tone_mapper: ToneMapper,
    /// Whether fog is enabled.
    pub fog_enabled: bool,
    /// Fog color.
    pub fog_light_color: Color,
    /// Fog density.
    pub fog_density: f32,
}

impl Default for Environment3D {
    fn default() -> Self {
        Self {
            background_mode: BackgroundMode::ClearColor,
            background_color: Color::BLACK,
            background_energy_multiplier: 1.0,
            sky: None,
            sky_custom_fov: 0.0,
            ambient_source: AmbientSource::Background,
            ambient_color: Color::BLACK,
            ambient_energy: 1.0,
            tone_mapper: ToneMapper::Linear,
            fog_enabled: false,
            fog_light_color: Color::new(0.518, 0.553, 0.608, 1.0),
            fog_density: 0.01,
        }
    }
}

impl Environment3D {
    /// Constructs an `Environment3D` from an iterator of property name/value pairs.
    ///
    /// Unrecognised properties are silently ignored. Missing properties keep
    /// their default values.  This is the primary bridge between the generic
    /// [`Resource`](gdresource) property bag and the typed environment struct.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut env = Self::default();
        for (key, value) in props {
            match key {
                "background_mode" => {
                    if let Variant::Int(v) = value {
                        env.background_mode = BackgroundMode::from_godot_int(*v);
                    }
                }
                "background_color" => {
                    if let Variant::Color(c) = value {
                        env.background_color = *c;
                    }
                }
                "background_energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        env.background_energy_multiplier = *f as f32;
                    }
                }
                "sky_custom_fov" => {
                    if let Variant::Float(f) = value {
                        env.sky_custom_fov = *f as f32;
                    }
                }
                "ambient_light_source" => {
                    if let Variant::Int(v) = value {
                        env.ambient_source = AmbientSource::from_godot_int(*v);
                    }
                }
                "ambient_light_color" => {
                    if let Variant::Color(c) = value {
                        env.ambient_color = *c;
                    }
                }
                "ambient_light_energy" => {
                    if let Variant::Float(f) = value {
                        env.ambient_energy = *f as f32;
                    }
                }
                "tonemap_mode" => {
                    if let Variant::Int(v) = value {
                        env.tone_mapper = ToneMapper::from_godot_int(*v);
                    }
                }
                "fog_enabled" => {
                    if let Variant::Bool(b) = value {
                        env.fog_enabled = *b;
                    }
                }
                "fog_light_color" => {
                    if let Variant::Color(c) = value {
                        env.fog_light_color = *c;
                    }
                }
                "fog_density" => {
                    if let Variant::Float(f) = value {
                        env.fog_density = *f as f32;
                    }
                }
                _ => {} // ignore unknown properties
            }
        }
        env
    }

    /// Serialises the environment back into a list of `(property_name, Variant)`
    /// pairs, suitable for storing in a generic [`Resource`](gdresource).
    ///
    /// Only non-default values are emitted to match Godot's behaviour of
    /// omitting properties that equal their default.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();

        if self.background_mode != def.background_mode {
            props.push((
                "background_mode".into(),
                Variant::Int(self.background_mode.to_godot_int()),
            ));
        }
        if self.background_color != def.background_color {
            props.push(("background_color".into(), Variant::Color(self.background_color)));
        }
        if (self.background_energy_multiplier - def.background_energy_multiplier).abs() > f32::EPSILON
        {
            props.push((
                "background_energy_multiplier".into(),
                Variant::Float(self.background_energy_multiplier as f64),
            ));
        }
        if self.sky_custom_fov != def.sky_custom_fov {
            props.push((
                "sky_custom_fov".into(),
                Variant::Float(self.sky_custom_fov as f64),
            ));
        }
        if self.ambient_source != def.ambient_source {
            props.push((
                "ambient_light_source".into(),
                Variant::Int(self.ambient_source.to_godot_int()),
            ));
        }
        if self.ambient_color != def.ambient_color {
            props.push((
                "ambient_light_color".into(),
                Variant::Color(self.ambient_color),
            ));
        }
        if (self.ambient_energy - def.ambient_energy).abs() > f32::EPSILON {
            props.push((
                "ambient_light_energy".into(),
                Variant::Float(self.ambient_energy as f64),
            ));
        }
        if self.tone_mapper != def.tone_mapper {
            props.push((
                "tonemap_mode".into(),
                Variant::Int(self.tone_mapper.to_godot_int()),
            ));
        }
        if self.fog_enabled != def.fog_enabled {
            props.push(("fog_enabled".into(), Variant::Bool(self.fog_enabled)));
        }
        if self.fog_light_color != def.fog_light_color {
            props.push(("fog_light_color".into(), Variant::Color(self.fog_light_color)));
        }
        if (self.fog_density - def.fog_density).abs() > f32::EPSILON {
            props.push((
                "fog_density".into(),
                Variant::Float(self.fog_density as f64),
            ));
        }
        props
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::{ProceduralSkyMaterial, SkyMaterial, SkyProcessMode};

    #[test]
    fn default_environment() {
        let env = Environment3D::default();
        assert_eq!(env.background_mode, BackgroundMode::ClearColor);
        assert!(env.sky.is_none());
        assert!(!env.fog_enabled);
        assert_eq!(env.tone_mapper, ToneMapper::Linear);
    }

    #[test]
    fn environment_with_sky() {
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            ..Default::default()
        };
        assert_eq!(env.background_mode, BackgroundMode::Sky);
        assert!(env.sky.is_some());
        let sky = env.sky.unwrap();
        assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
    }

    #[test]
    fn environment_with_procedural_sky_custom() {
        let sky = Sky {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial {
                sky_top_color: Color::new(0.0, 0.2, 0.8, 1.0),
                sky_horizon_color: Color::new(0.6, 0.7, 0.9, 1.0),
                ground_bottom_color: Color::new(0.05, 0.05, 0.05, 1.0),
                ..Default::default()
            }),
            process_mode: SkyProcessMode::RealTime,
            radiance_size: 512,
        };
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(sky),
            ambient_source: AmbientSource::Sky,
            ..Default::default()
        };
        assert_eq!(env.ambient_source, AmbientSource::Sky);
        assert_eq!(env.sky.as_ref().unwrap().process_mode, SkyProcessMode::RealTime);
    }

    #[test]
    fn background_mode_roundtrip() {
        for (int_val, expected) in [
            (0, BackgroundMode::ClearColor),
            (1, BackgroundMode::CustomColor),
            (2, BackgroundMode::Sky),
            (3, BackgroundMode::Canvas),
            (4, BackgroundMode::Keep),
            (5, BackgroundMode::CameraFeed),
        ] {
            let mode = BackgroundMode::from_godot_int(int_val);
            assert_eq!(mode, expected);
            assert_eq!(mode.to_godot_int(), int_val);
        }
    }

    #[test]
    fn ambient_source_roundtrip() {
        for (int_val, expected) in [
            (0, AmbientSource::Background),
            (1, AmbientSource::Disabled),
            (2, AmbientSource::Color),
            (3, AmbientSource::Sky),
        ] {
            let mode = AmbientSource::from_godot_int(int_val);
            assert_eq!(mode, expected);
            assert_eq!(mode.to_godot_int(), int_val);
        }
    }

    #[test]
    fn tone_mapper_roundtrip() {
        for (int_val, expected) in [
            (0, ToneMapper::Linear),
            (1, ToneMapper::Reinhard),
            (2, ToneMapper::Filmic),
            (3, ToneMapper::Aces),
        ] {
            let mode = ToneMapper::from_godot_int(int_val);
            assert_eq!(mode, expected);
            assert_eq!(mode.to_godot_int(), int_val);
        }
    }

    #[test]
    fn environment_with_fog() {
        let env = Environment3D {
            fog_enabled: true,
            fog_light_color: Color::new(0.8, 0.8, 0.9, 1.0),
            fog_density: 0.05,
            ..Default::default()
        };
        assert!(env.fog_enabled);
        assert!((env.fog_density - 0.05).abs() < 1e-5);
    }

    #[test]
    fn environment_clone_eq() {
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            fog_enabled: true,
            ..Default::default()
        };
        let cloned = env.clone();
        assert_eq!(env, cloned);
    }

    #[test]
    fn unknown_enum_values_default() {
        assert_eq!(BackgroundMode::from_godot_int(99), BackgroundMode::ClearColor);
        assert_eq!(AmbientSource::from_godot_int(-1), AmbientSource::Background);
        assert_eq!(ToneMapper::from_godot_int(100), ToneMapper::Linear);
    }

    #[test]
    fn from_properties_empty_gives_default() {
        let env = Environment3D::from_properties(std::iter::empty());
        assert_eq!(env, Environment3D::default());
    }

    #[test]
    fn from_properties_ambient_light() {
        let cyan = Color::new(0.0, 1.0, 1.0, 1.0);
        let props: Vec<(&str, Variant)> = vec![
            ("ambient_light_source", Variant::Int(2)),
            ("ambient_light_color", Variant::Color(cyan)),
            ("ambient_light_energy", Variant::Float(0.75)),
        ];
        let env =
            Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert_eq!(env.ambient_source, AmbientSource::Color);
        assert_eq!(env.ambient_color, cyan);
        assert!((env.ambient_energy - 0.75).abs() < 1e-5);
    }

    #[test]
    fn from_properties_fog() {
        let fog_color = Color::new(0.8, 0.8, 0.9, 1.0);
        let props: Vec<(&str, Variant)> = vec![
            ("fog_enabled", Variant::Bool(true)),
            ("fog_light_color", Variant::Color(fog_color)),
            ("fog_density", Variant::Float(0.05)),
        ];
        let env =
            Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert!(env.fog_enabled);
        assert_eq!(env.fog_light_color, fog_color);
        assert!((env.fog_density - 0.05).abs() < 1e-5);
    }

    #[test]
    fn from_properties_tonemap() {
        let props: Vec<(&str, Variant)> = vec![("tonemap_mode", Variant::Int(3))];
        let env =
            Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert_eq!(env.tone_mapper, ToneMapper::Aces);
    }

    #[test]
    fn from_properties_background() {
        let bg = Color::new(0.2, 0.3, 0.4, 1.0);
        let props: Vec<(&str, Variant)> = vec![
            ("background_mode", Variant::Int(1)),
            ("background_color", Variant::Color(bg)),
            ("background_energy_multiplier", Variant::Float(1.5)),
        ];
        let env =
            Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert_eq!(env.background_mode, BackgroundMode::CustomColor);
        assert_eq!(env.background_color, bg);
        assert!((env.background_energy_multiplier - 1.5).abs() < 1e-5);
    }

    #[test]
    fn to_properties_default_is_empty() {
        let env = Environment3D::default();
        let props = env.to_properties();
        assert!(props.is_empty(), "default environment should emit no properties");
    }

    #[test]
    fn to_properties_roundtrip() {
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.1, 0.2, 0.3, 1.0),
            ambient_source: AmbientSource::Color,
            ambient_color: Color::new(0.5, 0.5, 0.5, 1.0),
            ambient_energy: 0.8,
            tone_mapper: ToneMapper::Aces,
            fog_enabled: true,
            fog_light_color: Color::new(0.9, 0.9, 0.9, 1.0),
            fog_density: 0.03,
            ..Default::default()
        };
        let props = env.to_properties();
        let reconstructed = Environment3D::from_properties(
            props.iter().map(|(k, v)| (k.as_str(), v)),
        );
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
    fn from_properties_ignores_unknown() {
        let props: Vec<(&str, Variant)> = vec![
            ("unknown_prop", Variant::Int(42)),
            ("fog_enabled", Variant::Bool(true)),
        ];
        let env =
            Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert!(env.fog_enabled);
        // everything else stays default
        assert_eq!(env.background_mode, BackgroundMode::ClearColor);
    }
}
