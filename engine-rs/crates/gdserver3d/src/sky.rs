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
    /// Resource path to the sky-cover texture (Godot's `sky_cover`, e.g. clouds
    /// or stars), or empty. Sampled equirectangularly at `SKY_COORDS`.
    pub sky_cover_path: String,
    /// Optional decoded sky-cover texture (linear RGBA) for runtime sampling.
    /// When set, it is added over the sky hemisphere modulated by
    /// [`sky_cover_modulate`](Self::sky_cover_modulate).
    pub sky_cover: Option<PanoramaTexture>,
    /// Tint/opacity applied to the sky-cover texture (Godot's
    /// `sky_cover_modulate`, default opaque white).
    pub sky_cover_modulate: Color,
}

impl Default for ProceduralSkyMaterial {
    fn default() -> Self {
        Self {
            sky_top_color: Color::new(0.385, 0.454, 0.55, 1.0),
            // Godot's constructor uses the full-precision horizon color
            // (0.6463, 0.6558, 0.6708) — the truncated (0.646, 0.654, 0.67)
            // in its shader-default text is overridden by the setters.
            sky_horizon_color: Color::new(0.6463, 0.6558, 0.6708, 1.0),
            sky_curve: 0.15,
            sky_energy_multiplier: 1.0,
            ground_bottom_color: Color::new(0.2, 0.169, 0.133, 1.0),
            ground_horizon_color: Color::new(0.6463, 0.6558, 0.6708, 1.0),
            ground_curve: 0.02,
            ground_energy_multiplier: 1.0,
            sun_angle_max: 30.0,
            sun_curve: 0.15,
            sky_cover_path: String::new(),
            sky_cover: None,
            sky_cover_modulate: Color::new(1.0, 1.0, 1.0, 1.0),
        }
    }
}

/// Optional in-memory panorama pixels for equirect sampling (tests + loaders).
///
/// **Pixels MUST be linear.** All three consumers — `PanoramaSkyMaterial`,
/// `ProceduralSkyMaterial::sky_cover`, and `PhysicalSkyMaterial::night_sky` —
/// are Godot `source_color` samplers, so the sampler decodes sRGB→linear at
/// sample time. The renderer therefore uses these texels **raw** (already
/// linear). A loader that decodes one of these textures from an image file
/// must apply sRGB→linear itself (i.e. treat them as color, not data,
/// textures) before populating this buffer, or the cover/stars/panorama layer
/// will render too bright and over-saturated.
#[derive(Debug, Clone, PartialEq)]
pub struct PanoramaTexture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// **Linear** RGB(A) pixels, row-major, length `width * height` (see the
    /// type-level note — these are `source_color`, decode sRGB→linear on load).
    pub pixels: Vec<Color>,
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
    /// Optional decoded panorama for runtime equirect sampling.
    pub texture: Option<PanoramaTexture>,
}

impl Default for PanoramicSkyMaterial {
    fn default() -> Self {
        Self {
            panorama_path: String::new(),
            filter: true,
            energy_multiplier: 1.0,
            texture: None,
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
    /// Resource path to the night-sky texture (Godot's `night_sky`), or empty.
    /// Sampled equirectangularly at `SKY_COORDS`.
    pub night_sky_path: String,
    /// Optional decoded night-sky texture (linear RGB). Shown where the
    /// atmosphere is thin (added to `L0` × extinction) and as the whole sky
    /// when there is no sun.
    pub night_sky: Option<PanoramaTexture>,
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
            night_sky_path: String::new(),
            night_sky: None,
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

/// Populates a sky material's optional panorama texture(s) from its stored
/// resource path(s), using a caller-supplied `load` closure that maps a
/// resource reference to decoded **linear** pixels.
///
/// This is the seam between scene-loading (which only records a texture's
/// resource path — e.g. the normalized `"ExtResource:<id>"` form the `.tscn`
/// parser stores in [`ProceduralSkyMaterial::sky_cover_path`]) and the renderer
/// (which needs a [`PanoramaTexture`] of linear texels). It is deliberately
/// decode-free so it lives in `gdserver3d` (no image-codec dependency): the host
/// — the editor or runtime — provides `load`, which resolves the reference to a
/// file and decodes+linearizes it (see `gdresource::load_color_texture_linear`).
///
/// `load` returning `None` (unresolved reference, unsupported/HDR format, decode
/// failure) leaves the texture unset, so the material renders without that layer
/// rather than failing — matching Godot, where a missing sky cover is simply not
/// drawn. Materials whose texture is already populated (e.g. constructed in a
/// test) or whose path is empty are left untouched, so this is idempotent.
///
/// Covers all three texture-bearing sky materials: `ProceduralSkyMaterial`'s
/// `sky_cover`, `PhysicalSkyMaterial`'s `night_sky`, and the panorama of a
/// `PanoramaSkyMaterial`.
pub fn resolve_sky_material_textures(
    material: &mut SkyMaterial,
    mut load: impl FnMut(&str) -> Option<PanoramaTexture>,
) {
    match material {
        SkyMaterial::Procedural(m) => {
            if m.sky_cover.is_none() && !m.sky_cover_path.is_empty() {
                m.sky_cover = load(&m.sky_cover_path);
            }
        }
        SkyMaterial::Physical(m) => {
            if m.night_sky.is_none() && !m.night_sky_path.is_empty() {
                m.night_sky = load(&m.night_sky_path);
            }
        }
        SkyMaterial::Panoramic(m) => {
            if m.texture.is_none() && !m.panorama_path.is_empty() {
                m.texture = load(&m.panorama_path);
            }
        }
    }
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
                // The texture itself is resolved by a loader; here we keep the
                // resource-ref string (Godot serialises `sky_cover` as a
                // SubResource/ExtResource) and the modulate tint.
                "sky_cover" => {
                    if let Variant::String(s) = value {
                        mat.sky_cover_path = s.clone();
                    }
                }
                "sky_cover_modulate" => {
                    if let Variant::Color(c) = value {
                        mat.sky_cover_modulate = *c;
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
            props.push((
                "sky_horizon_color".into(),
                Variant::Color(self.sky_horizon_color),
            ));
        }
        if (self.sky_curve - def.sky_curve).abs() > f32::EPSILON {
            props.push(("sky_curve".into(), Variant::Float(self.sky_curve as f64)));
        }
        if (self.sky_energy_multiplier - def.sky_energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "sky_energy_multiplier".into(),
                Variant::Float(self.sky_energy_multiplier as f64),
            ));
        }
        if self.ground_bottom_color != def.ground_bottom_color {
            props.push((
                "ground_bottom_color".into(),
                Variant::Color(self.ground_bottom_color),
            ));
        }
        if self.ground_horizon_color != def.ground_horizon_color {
            props.push((
                "ground_horizon_color".into(),
                Variant::Color(self.ground_horizon_color),
            ));
        }
        if (self.ground_curve - def.ground_curve).abs() > f32::EPSILON {
            props.push((
                "ground_curve".into(),
                Variant::Float(self.ground_curve as f64),
            ));
        }
        if (self.ground_energy_multiplier - def.ground_energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "ground_energy_multiplier".into(),
                Variant::Float(self.ground_energy_multiplier as f64),
            ));
        }
        if (self.sun_angle_max - def.sun_angle_max).abs() > f32::EPSILON {
            props.push((
                "sun_angle_max".into(),
                Variant::Float(self.sun_angle_max as f64),
            ));
        }
        if (self.sun_curve - def.sun_curve).abs() > f32::EPSILON {
            props.push(("sun_curve".into(), Variant::Float(self.sun_curve as f64)));
        }
        if self.sky_cover_path != def.sky_cover_path {
            props.push((
                "sky_cover".into(),
                Variant::String(self.sky_cover_path.clone()),
            ));
        }
        if self.sky_cover_modulate != def.sky_cover_modulate {
            props.push((
                "sky_cover_modulate".into(),
                Variant::Color(self.sky_cover_modulate),
            ));
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
            props.push((
                "panorama".into(),
                Variant::String(self.panorama_path.clone()),
            ));
        }
        if self.filter != def.filter {
            props.push(("filter".into(), Variant::Bool(self.filter)));
        }
        if (self.energy_multiplier - def.energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "energy_multiplier".into(),
                Variant::Float(self.energy_multiplier as f64),
            ));
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
                // The texture is resolved by a loader; keep the resource-ref
                // string (Godot serialises `night_sky` as a Sub/ExtResource).
                "night_sky" => {
                    if let Variant::String(s) = value {
                        mat.night_sky_path = s.clone();
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
            props.push((
                "rayleigh_coefficient".into(),
                Variant::Float(self.rayleigh_coefficient as f64),
            ));
        }
        if self.rayleigh_color != def.rayleigh_color {
            props.push(("rayleigh_color".into(), Variant::Color(self.rayleigh_color)));
        }
        if (self.mie_coefficient - def.mie_coefficient).abs() > f32::EPSILON {
            props.push((
                "mie_coefficient".into(),
                Variant::Float(self.mie_coefficient as f64),
            ));
        }
        if (self.mie_eccentricity - def.mie_eccentricity).abs() > f32::EPSILON {
            props.push((
                "mie_eccentricity".into(),
                Variant::Float(self.mie_eccentricity as f64),
            ));
        }
        if self.mie_color != def.mie_color {
            props.push(("mie_color".into(), Variant::Color(self.mie_color)));
        }
        if (self.turbidity - def.turbidity).abs() > f32::EPSILON {
            props.push(("turbidity".into(), Variant::Float(self.turbidity as f64)));
        }
        if (self.sun_disk_scale - def.sun_disk_scale).abs() > f32::EPSILON {
            props.push((
                "sun_disk_scale".into(),
                Variant::Float(self.sun_disk_scale as f64),
            ));
        }
        if self.ground_color != def.ground_color {
            props.push(("ground_color".into(), Variant::Color(self.ground_color)));
        }
        if (self.energy_multiplier - def.energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "energy_multiplier".into(),
                Variant::Float(self.energy_multiplier as f64),
            ));
        }
        if self.night_sky_path != def.night_sky_path {
            props.push((
                "night_sky".into(),
                Variant::String(self.night_sky_path.clone()),
            ));
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
            "PanoramaSkyMaterial" | "PanoramicSkyMaterial" => SkyMaterial::Panoramic(
                PanoramicSkyMaterial::from_properties(material_props.into_iter()),
            ),
            "PhysicalSkyMaterial" => SkyMaterial::Physical(PhysicalSkyMaterial::from_properties(
                material_props.into_iter(),
            )),
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
                        texture: None,
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
        assert_eq!(
            SkyProcessMode::from_godot_int(99),
            SkyProcessMode::Automatic
        );
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

    // -- resolve_sky_material_textures -----------------------------------------

    fn one_pixel(c: Color) -> PanoramaTexture {
        PanoramaTexture { width: 1, height: 1, pixels: vec![c] }
    }

    #[test]
    fn resolve_loads_procedural_sky_cover_from_path() {
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_cover_path: "ExtResource:1_clouds".into(),
            ..Default::default()
        });
        let mut seen = Vec::new();
        resolve_sky_material_textures(&mut sky, |p| {
            seen.push(p.to_string());
            Some(one_pixel(Color::new(0.2, 0.4, 0.6, 1.0)))
        });
        // The loader is handed the raw stored reference (the parser's normalized
        // `ExtResource:<id>` form), which the host resolves to a file.
        assert_eq!(seen, vec!["ExtResource:1_clouds".to_string()]);
        match sky {
            SkyMaterial::Procedural(m) => {
                let tex = m.sky_cover.expect("sky_cover populated");
                assert_eq!(tex.pixels[0], Color::new(0.2, 0.4, 0.6, 1.0));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn resolve_loads_physical_night_sky_and_panorama() {
        let mut phys = SkyMaterial::Physical(PhysicalSkyMaterial {
            night_sky_path: "res://stars.png".into(),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut phys, |_| Some(one_pixel(Color::new(1.0, 1.0, 1.0, 1.0))));
        assert!(matches!(&phys, SkyMaterial::Physical(m) if m.night_sky.is_some()));

        let mut pano = SkyMaterial::Panoramic(PanoramicSkyMaterial {
            panorama_path: "res://sky.png".into(),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut pano, |_| Some(one_pixel(Color::new(0.5, 0.5, 0.5, 1.0))));
        assert!(matches!(&pano, SkyMaterial::Panoramic(m) if m.texture.is_some()));
    }

    #[test]
    fn resolve_skips_empty_path_without_calling_loader() {
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial::default());
        let mut called = false;
        resolve_sky_material_textures(&mut sky, |_| {
            called = true;
            Some(one_pixel(Color::new(1.0, 0.0, 0.0, 1.0)))
        });
        assert!(!called, "loader must not run for an empty texture path");
        assert!(matches!(&sky, SkyMaterial::Procedural(m) if m.sky_cover.is_none()));
    }

    #[test]
    fn resolve_is_idempotent_when_already_loaded() {
        let existing = one_pixel(Color::new(0.1, 0.1, 0.1, 1.0));
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_cover_path: "res://clouds.png".into(),
            sky_cover: Some(existing.clone()),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut sky, |_| {
            panic!("loader must not run when the texture is already populated")
        });
        assert!(matches!(&sky, SkyMaterial::Procedural(m) if m.sky_cover == Some(existing.clone())));
    }

    #[test]
    fn resolve_leaves_texture_unset_when_loader_returns_none() {
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_cover_path: "res://missing.exr".into(),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut sky, |_| None);
        assert!(
            matches!(&sky, SkyMaterial::Procedural(m) if m.sky_cover.is_none()),
            "a failed load must leave the layer unset, not crash"
        );
    }
}
