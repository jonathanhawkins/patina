//! Sky resource types for 3D environment rendering.
//!
//! Implements Godot's Sky, ProceduralSkyMaterial, and PanoramicSkyMaterial
//! resource types. A [`Sky`] holds a [`SkyMaterial`] that defines how the
//! sky dome is rendered, and is consumed by an [`Environment3D`](super::environment::Environment3D).

use gdcore::math::Color;
use gdvariant::Variant;

/// The processing mode for sky rendering.
///
/// Maps to Godot's `Sky.ProcessMode` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SkyProcessMode {
    /// Automatically choose quality based on usage.
    #[default]
    Automatic,
    /// High quality — update every frame.
    Quality,
    /// Low quality — update incrementally.
    Incremental,
    /// Real-time — always re-render.
    RealTime,
}

impl SkyProcessMode {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Automatic,
            1 => Self::Quality,
            2 => Self::Incremental,
            3 => Self::RealTime,
            _ => Self::Automatic,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Automatic => 0,
            Self::Quality => 1,
            Self::Incremental => 2,
            Self::RealTime => 3,
        }
    }
}

/// A procedural sky material that generates a sky from color gradients
/// and sun parameters.
///
/// Maps to Godot's `ProceduralSkyMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralSkyMaterial {
    /// Color at the top of the sky dome.
    pub sky_top_color: Color,
    /// Color at the horizon.
    pub sky_horizon_color: Color,
    /// Curve controlling the sky gradient falloff (higher = sharper).
    pub sky_curve: f32,
    /// Energy multiplier for the sky.
    pub sky_energy_multiplier: f32,
    /// Color at the bottom of the sky dome (ground reflection).
    pub ground_bottom_color: Color,
    /// Color at the ground horizon.
    pub ground_horizon_color: Color,
    /// Curve controlling the ground gradient falloff.
    pub ground_curve: f32,
    /// Energy multiplier for the ground.
    pub ground_energy_multiplier: f32,
    /// Angular size of the sun disc in degrees.
    pub sun_angle_max: f32,
    /// Curve controlling sun disc falloff.
    pub sun_curve: f32,
}

impl Default for ProceduralSkyMaterial {
    fn default() -> Self {
        Self {
            sky_top_color: Color::new(0.385, 0.454, 0.55, 1.0),
            sky_horizon_color: Color::new(0.646, 0.654, 0.67, 1.0),
            sky_curve: 0.15,
            sky_energy_multiplier: 1.0,
            ground_bottom_color: Color::new(0.2, 0.169, 0.133, 1.0),
            ground_horizon_color: Color::new(0.646, 0.654, 0.67, 1.0),
            ground_curve: 0.02,
            ground_energy_multiplier: 1.0,
            sun_angle_max: 30.0,
            sun_curve: 0.15,
        }
    }
}

/// A panoramic sky material that maps an HDR panorama texture onto the
/// sky dome.
///
/// Maps to Godot's `PanoramaSkyMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct PanoramicSkyMaterial {
    /// Resource path to the panoramic texture (e.g. `"res://sky.hdr"`).
    pub panorama_path: String,
    /// Whether to filter the panorama texture.
    pub filter: bool,
    /// Energy multiplier.
    pub energy_multiplier: f32,
}

impl Default for PanoramicSkyMaterial {
    fn default() -> Self {
        Self {
            panorama_path: String::new(),
            filter: true,
            energy_multiplier: 1.0,
        }
    }
}

/// A physical sky material based on atmospheric scattering.
///
/// Maps to Godot's `PhysicalSkyMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalSkyMaterial {
    /// Rayleigh scattering coefficient.
    pub rayleigh_coefficient: f32,
    /// Rayleigh scattering color.
    pub rayleigh_color: Color,
    /// Mie scattering coefficient.
    pub mie_coefficient: f32,
    /// Mie eccentricity (directionality).
    pub mie_eccentricity: f32,
    /// Mie scattering color.
    pub mie_color: Color,
    /// Turbidity factor (atmospheric haze).
    pub turbidity: f32,
    /// Sun disc radius in degrees.
    pub sun_disk_scale: f32,
    /// Ground color for the lower hemisphere.
    pub ground_color: Color,
    /// Energy multiplier.
    pub energy_multiplier: f32,
}

impl Default for PhysicalSkyMaterial {
    fn default() -> Self {
        Self {
            rayleigh_coefficient: 2.0,
            rayleigh_color: Color::new(0.3, 0.405, 0.6, 1.0),
            mie_coefficient: 0.005,
            mie_eccentricity: 0.8,
            mie_color: Color::new(0.69, 0.729, 0.812, 1.0),
            turbidity: 10.0,
            sun_disk_scale: 1.0,
            ground_color: Color::new(0.1, 0.07, 0.034, 1.0),
            energy_multiplier: 1.0,
        }
    }
}

/// The material that defines how a sky dome is rendered.
#[derive(Debug, Clone, PartialEq)]
pub enum SkyMaterial {
    /// Procedurally generated sky from gradients and sun parameters.
    Procedural(ProceduralSkyMaterial),
    /// Sky from a panoramic HDR texture.
    Panoramic(PanoramicSkyMaterial),
    /// Physically-based atmospheric scattering sky.
    Physical(PhysicalSkyMaterial),
}

/// A Sky resource that holds a material and rendering parameters.
///
/// Maps to Godot's `Sky` resource. Used as the sky source for
/// [`Environment3D`](super::environment::Environment3D).
#[derive(Debug, Clone, PartialEq)]
pub struct Sky {
    /// The sky material defining the visual appearance.
    pub material: SkyMaterial,
    /// Processing mode for sky updates.
    pub process_mode: SkyProcessMode,
    /// Radiance size for reflections (in pixels per face).
    pub radiance_size: u32,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial::default()),
            process_mode: SkyProcessMode::Automatic,
            radiance_size: 256,
        }
    }
}

impl ProceduralSkyMaterial {
    /// Constructs a `ProceduralSkyMaterial` from property name/value pairs.
    ///
    /// Unrecognised properties are ignored; missing ones keep defaults.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "sky_top_color" => {
                    if let Variant::Color(c) = value {
                        mat.sky_top_color = *c;
                    }
                }
                "sky_horizon_color" => {
                    if let Variant::Color(c) = value {
                        mat.sky_horizon_color = *c;
                    }
                }
                "sky_curve" => {
                    if let Variant::Float(f) = value {
                        mat.sky_curve = *f as f32;
                    }
                }
                "sky_energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.sky_energy_multiplier = *f as f32;
                    }
                }
                "ground_bottom_color" => {
                    if let Variant::Color(c) = value {
                        mat.ground_bottom_color = *c;
                    }
                }
                "ground_horizon_color" => {
                    if let Variant::Color(c) = value {
                        mat.ground_horizon_color = *c;
                    }
                }
                "ground_curve" => {
                    if let Variant::Float(f) = value {
                        mat.ground_curve = *f as f32;
                    }
                }
                "ground_energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.ground_energy_multiplier = *f as f32;
                    }
                }
                "sun_angle_max" => {
                    if let Variant::Float(f) = value {
                        mat.sun_angle_max = *f as f32;
                    }
                }
                "sun_curve" => {
                    if let Variant::Float(f) = value {
                        mat.sun_curve = *f as f32;
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if self.sky_top_color != def.sky_top_color {
            props.push(("sky_top_color".into(), Variant::Color(self.sky_top_color)));
        }
        if self.sky_horizon_color != def.sky_horizon_color {
            props.push(("sky_horizon_color".into(), Variant::Color(self.sky_horizon_color)));
        }
        if (self.sky_curve - def.sky_curve).abs() > f32::EPSILON {
            props.push(("sky_curve".into(), Variant::Float(self.sky_curve as f64)));
        }
        if (self.sky_energy_multiplier - def.sky_energy_multiplier).abs() > f32::EPSILON {
            props.push(("sky_energy_multiplier".into(), Variant::Float(self.sky_energy_multiplier as f64)));
        }
        if self.ground_bottom_color != def.ground_bottom_color {
            props.push(("ground_bottom_color".into(), Variant::Color(self.ground_bottom_color)));
        }
        if self.ground_horizon_color != def.ground_horizon_color {
            props.push(("ground_horizon_color".into(), Variant::Color(self.ground_horizon_color)));
        }
        if (self.ground_curve - def.ground_curve).abs() > f32::EPSILON {
            props.push(("ground_curve".into(), Variant::Float(self.ground_curve as f64)));
        }
        if (self.ground_energy_multiplier - def.ground_energy_multiplier).abs() > f32::EPSILON {
            props.push(("ground_energy_multiplier".into(), Variant::Float(self.ground_energy_multiplier as f64)));
        }
        if (self.sun_angle_max - def.sun_angle_max).abs() > f32::EPSILON {
            props.push(("sun_angle_max".into(), Variant::Float(self.sun_angle_max as f64)));
        }
        if (self.sun_curve - def.sun_curve).abs() > f32::EPSILON {
            props.push(("sun_curve".into(), Variant::Float(self.sun_curve as f64)));
        }
        props
    }
}

impl PanoramicSkyMaterial {
    /// Constructs a `PanoramicSkyMaterial` from property name/value pairs.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "panorama" => {
                    if let Variant::String(s) = value {
                        mat.panorama_path = s.clone();
                    }
                }
                "filter" => {
                    if let Variant::Bool(b) = value {
                        mat.filter = *b;
                    }
                }
                "energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.energy_multiplier = *f as f32;
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if self.panorama_path != def.panorama_path {
            props.push(("panorama".into(), Variant::String(self.panorama_path.clone())));
        }
        if self.filter != def.filter {
            props.push(("filter".into(), Variant::Bool(self.filter)));
        }
        if (self.energy_multiplier - def.energy_multiplier).abs() > f32::EPSILON {
            props.push(("energy_multiplier".into(), Variant::Float(self.energy_multiplier as f64)));
        }
        props
    }
}

impl PhysicalSkyMaterial {
    /// Constructs a `PhysicalSkyMaterial` from property name/value pairs.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "rayleigh_coefficient" => {
                    if let Variant::Float(f) = value {
                        mat.rayleigh_coefficient = *f as f32;
                    }
                }
                "rayleigh_color" => {
                    if let Variant::Color(c) = value {
                        mat.rayleigh_color = *c;
                    }
                }
                "mie_coefficient" => {
                    if let Variant::Float(f) = value {
                        mat.mie_coefficient = *f as f32;
                    }
                }
                "mie_eccentricity" => {
                    if let Variant::Float(f) = value {
                        mat.mie_eccentricity = *f as f32;
                    }
                }
                "mie_color" => {
                    if let Variant::Color(c) = value {
                        mat.mie_color = *c;
                    }
                }
                "turbidity" => {
                    if let Variant::Float(f) = value {
                        mat.turbidity = *f as f32;
                    }
                }
                "sun_disk_scale" => {
                    if let Variant::Float(f) = value {
                        mat.sun_disk_scale = *f as f32;
                    }
                }
                "ground_color" => {
                    if let Variant::Color(c) = value {
                        mat.ground_color = *c;
                    }
                }
                "energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.energy_multiplier = *f as f32;
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if (self.rayleigh_coefficient - def.rayleigh_coefficient).abs() > f32::EPSILON {
            props.push(("rayleigh_coefficient".into(), Variant::Float(self.rayleigh_coefficient as f64)));
        }
        if self.rayleigh_color != def.rayleigh_color {
            props.push(("rayleigh_color".into(), Variant::Color(self.rayleigh_color)));
        }
        if (self.mie_coefficient - def.mie_coefficient).abs() > f32::EPSILON {
            props.push(("mie_coefficient".into(), Variant::Float(self.mie_coefficient as f64)));
        }
        if (self.mie_eccentricity - def.mie_eccentricity).abs() > f32::EPSILON {
            props.push(("mie_eccentricity".into(), Variant::Float(self.mie_eccentricity as f64)));
        }
        if self.mie_color != def.mie_color {
            props.push(("mie_color".into(), Variant::Color(self.mie_color)));
        }
        if (self.turbidity - def.turbidity).abs() > f32::EPSILON {
            props.push(("turbidity".into(), Variant::Float(self.turbidity as f64)));
        }
        if (self.sun_disk_scale - def.sun_disk_scale).abs() > f32::EPSILON {
            props.push(("sun_disk_scale".into(), Variant::Float(self.sun_disk_scale as f64)));
        }
        if self.ground_color != def.ground_color {
            props.push(("ground_color".into(), Variant::Color(self.ground_color)));
        }
        if (self.energy_multiplier - def.energy_multiplier).abs() > f32::EPSILON {
            props.push(("energy_multiplier".into(), Variant::Float(self.energy_multiplier as f64)));
        }
        props
    }
}

impl Sky {
    /// Constructs a `Sky` from property name/value pairs.
    ///
    /// The `"sky_material_type"` property selects the material variant:
    /// `"ProceduralSkyMaterial"` (default), `"PanoramaSkyMaterial"`, or
    /// `"PhysicalSkyMaterial"`. Other material-specific properties are
    /// forwarded to the corresponding material's `from_properties`.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut sky = Self::default();
        let mut material_type = String::new();
        let mut material_props: Vec<(&str, &Variant)> = Vec::new();

        for (key, value) in props {
            match key {
                "sky_material_type" | "sky_material" => {
                    if let Variant::String(s) = value {
                        material_type = s.clone();
                    }
                }
                "process_mode" => {
                    if let Variant::Int(v) = value {
                        sky.process_mode = SkyProcessMode::from_godot_int(*v);
                    }
                }
                "radiance_size" => {
                    if let Variant::Int(v) = value {
                        sky.radiance_size = *v as u32;
                    }
                }
                _ => {
                    material_props.push((key, value));
                }
            }
        }

        sky.material = match material_type.as_str() {
            "PanoramaSkyMaterial" | "PanoramicSkyMaterial" => {
                SkyMaterial::Panoramic(PanoramicSkyMaterial::from_properties(
                    material_props.into_iter(),
                ))
            }
            "PhysicalSkyMaterial" => {
                SkyMaterial::Physical(PhysicalSkyMaterial::from_properties(
                    material_props.into_iter(),
                ))
            }
            _ => {
                // Default to procedural
                SkyMaterial::Procedural(ProceduralSkyMaterial::from_properties(
                    material_props.into_iter(),
                ))
            }
        };

        sky
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();

        // Emit material type and material-specific properties.
        match &self.material {
            SkyMaterial::Procedural(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("ProceduralSkyMaterial".into()),
                ));
                props.extend(mat.to_properties());
            }
            SkyMaterial::Panoramic(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("PanoramaSkyMaterial".into()),
                ));
                props.extend(mat.to_properties());
            }
            SkyMaterial::Physical(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("PhysicalSkyMaterial".into()),
                ));
                props.extend(mat.to_properties());
            }
        }

        if self.process_mode != def.process_mode {
            props.push((
                "process_mode".into(),
                Variant::Int(self.process_mode.to_godot_int()),
            ));
        }
        if self.radiance_size != def.radiance_size {
            props.push((
                "radiance_size".into(),
                Variant::Int(self.radiance_size as i64),
            ));
        }

        props
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sky_is_procedural() {
        let sky = Sky::default();
        assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
        assert_eq!(sky.process_mode, SkyProcessMode::Automatic);
        assert_eq!(sky.radiance_size, 256);
    }

    #[test]
    fn procedural_sky_defaults_match_godot() {
        let mat = ProceduralSkyMaterial::default();
        assert!((mat.sky_curve - 0.15).abs() < 1e-5);
        assert!((mat.sky_energy_multiplier - 1.0).abs() < 1e-5);
        assert!((mat.ground_curve - 0.02).abs() < 1e-5);
        assert!((mat.sun_angle_max - 30.0).abs() < 1e-5);
    }

    #[test]
    fn panoramic_sky_defaults() {
        let mat = PanoramicSkyMaterial::default();
        assert!(mat.panorama_path.is_empty());
        assert!(mat.filter);
        assert!((mat.energy_multiplier - 1.0).abs() < 1e-5);
    }

    #[test]
    fn physical_sky_defaults() {
        let mat = PhysicalSkyMaterial::default();
        assert!((mat.rayleigh_coefficient - 2.0).abs() < 1e-5);
        assert!((mat.mie_eccentricity - 0.8).abs() < 1e-5);
        assert!((mat.turbidity - 10.0).abs() < 1e-5);
    }

    #[test]
    fn sky_with_panoramic_material() {
        let sky = Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                panorama_path: "res://sky_hdr.exr".to_string(),
                filter: true,
                energy_multiplier: 1.5,
            }),
            process_mode: SkyProcessMode::Quality,
            radiance_size: 512,
        };
        assert!(matches!(sky.material, SkyMaterial::Panoramic(_)));
        assert_eq!(sky.process_mode, SkyProcessMode::Quality);
    }

    #[test]
    fn sky_process_mode_roundtrip() {
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
    fn sky_process_mode_unknown_defaults_to_automatic() {
        assert_eq!(SkyProcessMode::from_godot_int(99), SkyProcessMode::Automatic);
    }

    #[test]
    fn procedural_sky_custom_colors() {
        let mat = ProceduralSkyMaterial {
            sky_top_color: Color::new(0.0, 0.0, 1.0, 1.0),
            sky_horizon_color: Color::new(0.8, 0.8, 1.0, 1.0),
            ground_bottom_color: Color::new(0.1, 0.1, 0.1, 1.0),
            ground_horizon_color: Color::new(0.5, 0.5, 0.5, 1.0),
            sun_angle_max: 45.0,
            ..Default::default()
        };
        assert_eq!(mat.sky_top_color, Color::new(0.0, 0.0, 1.0, 1.0));
        assert!((mat.sun_angle_max - 45.0).abs() < 1e-5);
    }

    #[test]
    fn sky_clone_eq() {
        let sky = Sky::default();
        let cloned = sky.clone();
        assert_eq!(sky, cloned);
    }
}
