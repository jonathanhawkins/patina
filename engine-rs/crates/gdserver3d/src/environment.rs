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

/// Where specular reflections come from (pat-h7sz7).
///
/// Maps to Godot's `Environment.ReflectionSource` enum
/// (`reflected_light_source`, default `BG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReflectionSource {
    /// The background: the sky's radiance map when the background is a sky,
    /// nothing otherwise.
    #[default]
    Background,
    /// No environment reflections.
    Disabled,
    /// The sky's radiance map, whatever the background.
    Sky,
}

impl ReflectionSource {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::Disabled,
            2 => Self::Sky,
            _ => Self::Background,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Background => 0,
            Self::Disabled => 1,
            Self::Sky => 2,
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
    /// AgX tone mapping (Godot 4.6's Blender-derived approximation; reads
    /// `tonemap_agx_white` / `tonemap_agx_contrast` instead of `tonemap_white`).
    Agx,
}

impl ToneMapper {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Linear,
            1 => Self::Reinhard,
            2 => Self::Filmic,
            3 => Self::Aces,
            4 => Self::Agx,
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
            Self::Agx => 4,
        }
    }
}

/// How the blurred glow buffer is composited back over the scene.
///
/// Maps to Godot's `Environment.GlowBlendMode` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GlowBlendMode {
    /// Adds glow on top of the scene (`color + glow`). Brightest.
    Additive,
    /// Screen blend — glow never brightens past white. The Godot default.
    #[default]
    Screen,
    /// Soft-light blend — subtle, anchored around mid-gray.
    Softlight,
    /// Replaces the scene with the glow buffer.
    Replace,
    /// Mix (lerp) between scene and glow by intensity.
    Mix,
}

impl GlowBlendMode {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Additive,
            1 => Self::Screen,
            2 => Self::Softlight,
            3 => Self::Replace,
            4 => Self::Mix,
            _ => Self::Screen,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Additive => 0,
            Self::Screen => 1,
            Self::Softlight => 2,
            Self::Replace => 3,
            Self::Mix => 4,
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
    /// Specular reflection source (Godot `reflected_light_source`).
    pub reflection_source: ReflectionSource,
    /// Ambient light color.
    pub ambient_color: Color,
    /// Ambient light energy.
    pub ambient_energy: f32,
    /// Tone mapping mode.
    pub tone_mapper: ToneMapper,
    /// Exposure multiplier applied to the linear scene color before tone
    /// mapping (Godot's `tonemap_exposure`).
    pub tonemap_exposure: f32,
    /// The white reference point for tone mapping (Godot's `tonemap_white`).
    /// Scene values at this luminance map to display white.
    pub tonemap_white: f32,
    /// The white reference point when the tone mapper is [`ToneMapper::Agx`]
    /// (Godot's `tonemap_agx_white`, default 16.29 — Blender's AgX white).
    pub tonemap_agx_white: f32,
    /// AgX curve contrast (Godot's `tonemap_agx_contrast`, default 1.25 —
    /// approximately Blender's AgX contrast).
    pub tonemap_agx_contrast: f32,
    /// Whether the glow (bloom) post-process is enabled.
    pub glow_enabled: bool,
    /// Overall glow intensity — scales the gathered glow before blending
    /// (Godot's `glow_intensity`, default 0.3).
    pub glow_intensity: f32,
    /// Glow strength — scales each blur level's contribution during gather
    /// (Godot's `glow_strength`).
    pub glow_strength: f32,
    /// Additional bloom that leaks the whole image into glow regardless of
    /// the HDR threshold (Godot's `glow_bloom`).
    pub glow_bloom: f32,
    /// How the glow buffer is composited over the scene.
    pub glow_blend_mode: GlowBlendMode,
    /// Linear luminance above which pixels start bleeding into glow
    /// (Godot's `glow_hdr_bleed_threshold`, default 1.0).
    pub glow_hdr_bleed_threshold: f32,
    /// Width of the smoothstep bleed ramp above the threshold
    /// (Godot's `glow_hdr_bleed_scale`, default 2.0).
    pub glow_hdr_bleed_scale: f32,
    /// Maximum luminance a pixel can contribute to glow, clamping fireflies
    /// (Godot's `glow_hdr_luminance_cap`, default 12.0).
    pub glow_hdr_luminance_cap: f32,
    /// Per-level glow weights for the 7 blur mip levels (Godot's
    /// `glow_levels/1..7`, default `[0, 0.8, 0.4, 0.1, 0, 0, 0]`).
    pub glow_levels: [f32; 7],
    /// Whether fog is enabled.
    pub fog_enabled: bool,
    /// How fog density accumulates with distance (Godot's `fog_mode`).
    pub fog_mode: FogMode,
    /// Fog color (sRGB authoring).
    pub fog_light_color: Color,
    /// Fog color energy multiplier (Godot's `fog_light_energy`).
    pub fog_light_energy: f32,
    /// How strongly directional lights scatter into the fog toward the viewer
    /// (Godot's `fog_sun_scatter`, 0 = none).
    pub fog_sun_scatter: f32,
    /// Fog density for the exponential fog mode (Godot's `fog_density`).
    pub fog_density: f32,
    /// World-space height the height-fog band is anchored at (`fog_height`).
    pub fog_height: f32,
    /// Height-fog falloff; can be negative to invert (`fog_height_density`).
    /// 0 disables height fog.
    pub fog_height_density: f32,
    /// Blends the fog color toward the sky color in the view direction
    /// (Godot's `fog_aerial_perspective`, 0 = flat fog color).
    pub fog_aerial_perspective: f32,
    /// How much the fog covers the sky background (Godot's `fog_sky_affect`,
    /// 1 = fully, 0 = sky untouched).
    pub fog_sky_affect: f32,
    /// Depth-fog curve exponent (Godot's `fog_depth_curve`, DEPTH mode).
    pub fog_depth_curve: f32,
    /// Distance where depth fog begins (Godot's `fog_depth_begin`, DEPTH mode).
    pub fog_depth_begin: f32,
    /// Distance where depth fog reaches full density (`fog_depth_end`).
    pub fog_depth_end: f32,
    /// Gates the brightness/contrast/saturation adjustment stage
    /// (`adjustment_enabled`, default **false**). With it off the three knobs
    /// below are ignored, exactly as Godot's `FLAG_USE_BCS` gate works.
    pub adjustment_enabled: bool,
    /// Linear multiplier applied before the display encode
    /// (`adjustment_brightness`, default **1.0**).
    pub adjustment_brightness: f32,
    /// Contrast around a 0.5 pivot (`adjustment_contrast`, default **1.0**).
    ///
    /// ⚠️ Applied to the **sRGB-encoded** value, not the linear one — see
    /// [`crate::environment::Environment3D`] users in `postprocess`.
    pub adjustment_contrast: f32,
    /// Saturation (`adjustment_saturation`, default **1.0**). `0.0` is grey.
    ///
    /// ⚠️ Godot greys with an **even** 1/3 weight per channel, not a luminance
    /// weighting.
    pub adjustment_saturation: f32,
    /// Screen-space debanding dither after the sRGB encode. Patina extension:
    /// in Godot this is the `rendering/anti_aliasing/quality/use_debanding`
    /// viewport/project setting (default **false**), not an Environment
    /// property — it never appears in a Godot `.tscn`/`.tres`.
    pub use_debanding: bool,
    /// Screen-space ambient occlusion enable (Godot's `ssao_enabled`, default
    /// **false**). When on, the render path darkens creases using the depth
    /// buffer before tonemapping.
    pub ssao_enabled: bool,
    /// SSAO sampling radius in world units (Godot's `ssao_radius`, default
    /// **1.0**). Larger values widen the neighbourhood sampled for occlusion.
    pub ssao_radius: f32,
    /// SSAO strength multiplier (Godot's `ssao_intensity`, default **2.0**).
    /// 0 leaves the framebuffer untouched.
    pub ssao_intensity: f32,
}

/// How fog density accumulates with distance.
///
/// Maps to Godot's `Environment.FogMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FogMode {
    /// `1 - exp(-distance * density)` — the Godot default.
    #[default]
    Exponential,
    /// `pow(smoothstep(begin, end, distance), curve) * density`.
    Depth,
}

impl FogMode {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::Depth,
            _ => Self::Exponential,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Exponential => 0,
            Self::Depth => 1,
        }
    }
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
            reflection_source: ReflectionSource::Background,
            ambient_color: Color::BLACK,
            ambient_energy: 1.0,
            tone_mapper: ToneMapper::Linear,
            tonemap_exposure: 1.0,
            tonemap_white: 1.0,
            // Godot 4.6 defaults: Blender's AgX white / approximate contrast.
            tonemap_agx_white: 16.29,
            tonemap_agx_contrast: 1.25,
            glow_enabled: false,
            glow_intensity: 0.3,
            glow_strength: 1.0,
            glow_bloom: 0.0,
            glow_blend_mode: GlowBlendMode::Screen,
            glow_hdr_bleed_threshold: 1.0,
            glow_hdr_bleed_scale: 2.0,
            glow_hdr_luminance_cap: 12.0,
            glow_levels: [0.0, 0.8, 0.4, 0.1, 0.0, 0.0, 0.0],
            fog_enabled: false,
            fog_mode: FogMode::Exponential,
            fog_light_color: Color::new(0.518, 0.553, 0.608, 1.0),
            fog_light_energy: 1.0,
            fog_sun_scatter: 0.0,
            fog_density: 0.01,
            fog_height: 0.0,
            fog_height_density: 0.0,
            fog_aerial_perspective: 0.0,
            fog_sky_affect: 1.0,
            fog_depth_curve: 1.0,
            fog_depth_begin: 10.0,
            fog_depth_end: 100.0,
            // Off by default so deterministic framebuffer goldens stay stable;
            // enable for editor/sky paths that want Godot-like gradient dither.
            adjustment_enabled: false,
            adjustment_brightness: 1.0,
            adjustment_contrast: 1.0,
            adjustment_saturation: 1.0,
            use_debanding: false,
            // Godot 4.6 SSAO defaults (Environment).
            ssao_enabled: false,
            ssao_radius: 1.0,
            ssao_intensity: 2.0,
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
                // The BCS adjustment stage (pat-s3rrv.113). These were
                // registered in ClassDB, so the inspector round-tripped them
                // into the .tscn, and nothing here read them back.
                "adjustment_enabled" => {
                    if let Variant::Bool(b) = value {
                        env.adjustment_enabled = *b;
                    }
                }
                "adjustment_brightness" => {
                    if let Variant::Float(f) = value {
                        env.adjustment_brightness = *f as f32;
                    }
                }
                "adjustment_contrast" => {
                    if let Variant::Float(f) = value {
                        env.adjustment_contrast = *f as f32;
                    }
                }
                "adjustment_saturation" => {
                    if let Variant::Float(f) = value {
                        env.adjustment_saturation = *f as f32;
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
                "reflected_light_source" => {
                    if let Variant::Int(v) = value {
                        env.reflection_source = ReflectionSource::from_godot_int(*v);
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
                "tonemap_exposure" => {
                    if let Variant::Float(f) = value {
                        env.tonemap_exposure = *f as f32;
                    }
                }
                "tonemap_white" => {
                    if let Variant::Float(f) = value {
                        env.tonemap_white = *f as f32;
                    }
                }
                "tonemap_agx_white" => {
                    if let Variant::Float(f) = value {
                        env.tonemap_agx_white = *f as f32;
                    }
                }
                "tonemap_agx_contrast" => {
                    if let Variant::Float(f) = value {
                        // Godot's property hint is `1.0,2.0,0.01,or_greater` —
                        // enforce the lower bound (contrast ≤ 0 would NaN the
                        // allenwp shoulder: slope and w collapse to 0).
                        env.tonemap_agx_contrast = (*f as f32).max(1.0);
                    }
                }
                "glow_enabled" => {
                    if let Variant::Bool(b) = value {
                        env.glow_enabled = *b;
                    }
                }
                "glow_intensity" => {
                    if let Variant::Float(f) = value {
                        env.glow_intensity = *f as f32;
                    }
                }
                "glow_strength" => {
                    if let Variant::Float(f) = value {
                        env.glow_strength = *f as f32;
                    }
                }
                "glow_bloom" => {
                    if let Variant::Float(f) = value {
                        env.glow_bloom = *f as f32;
                    }
                }
                "glow_blend_mode" => {
                    if let Variant::Int(v) = value {
                        env.glow_blend_mode = GlowBlendMode::from_godot_int(*v);
                    }
                }
                "glow_hdr_bleed_threshold" => {
                    if let Variant::Float(f) = value {
                        env.glow_hdr_bleed_threshold = *f as f32;
                    }
                }
                "glow_hdr_scale" => {
                    if let Variant::Float(f) = value {
                        env.glow_hdr_bleed_scale = *f as f32;
                    }
                }
                "glow_hdr_luminance_cap" => {
                    if let Variant::Float(f) = value {
                        env.glow_hdr_luminance_cap = *f as f32;
                    }
                }
                // Godot exposes the 7 glow levels as `glow_levels/1`..`/7`.
                _ if key.starts_with("glow_levels/") => {
                    if let (Some(idx), Variant::Float(f)) =
                        (key["glow_levels/".len()..].parse::<usize>().ok(), value)
                    {
                        if (1..=7).contains(&idx) {
                            env.glow_levels[idx - 1] = *f as f32;
                        }
                    }
                }
                "fog_enabled" => {
                    if let Variant::Bool(b) = value {
                        env.fog_enabled = *b;
                    }
                }
                "fog_mode" => {
                    if let Variant::Int(v) = value {
                        env.fog_mode = FogMode::from_godot_int(*v);
                    }
                }
                "fog_light_color" => {
                    if let Variant::Color(c) = value {
                        env.fog_light_color = *c;
                    }
                }
                "fog_light_energy" => {
                    if let Variant::Float(f) = value {
                        env.fog_light_energy = *f as f32;
                    }
                }
                "fog_sun_scatter" => {
                    if let Variant::Float(f) = value {
                        env.fog_sun_scatter = *f as f32;
                    }
                }
                "fog_density" => {
                    if let Variant::Float(f) = value {
                        env.fog_density = *f as f32;
                    }
                }
                "fog_height" => {
                    if let Variant::Float(f) = value {
                        env.fog_height = *f as f32;
                    }
                }
                "fog_height_density" => {
                    if let Variant::Float(f) = value {
                        env.fog_height_density = *f as f32;
                    }
                }
                "fog_aerial_perspective" => {
                    if let Variant::Float(f) = value {
                        env.fog_aerial_perspective = *f as f32;
                    }
                }
                "fog_sky_affect" => {
                    if let Variant::Float(f) = value {
                        env.fog_sky_affect = *f as f32;
                    }
                }
                "fog_depth_curve" => {
                    if let Variant::Float(f) = value {
                        env.fog_depth_curve = *f as f32;
                    }
                }
                "fog_depth_begin" => {
                    if let Variant::Float(f) = value {
                        env.fog_depth_begin = *f as f32;
                    }
                }
                "fog_depth_end" => {
                    if let Variant::Float(f) = value {
                        env.fog_depth_end = *f as f32;
                    }
                }
                "ssao_enabled" => {
                    if let Variant::Bool(b) = value {
                        env.ssao_enabled = *b;
                    }
                }
                "ssao_radius" => {
                    if let Variant::Float(f) = value {
                        env.ssao_radius = *f as f32;
                    }
                }
                "ssao_intensity" => {
                    if let Variant::Float(f) = value {
                        env.ssao_intensity = *f as f32;
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
            props.push((
                "background_color".into(),
                Variant::Color(self.background_color),
            ));
        }
        if (self.background_energy_multiplier - def.background_energy_multiplier).abs()
            > f32::EPSILON
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
        if self.reflection_source != def.reflection_source {
            props.push((
                "reflected_light_source".into(),
                Variant::Int(self.reflection_source.to_godot_int()),
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
        if (self.tonemap_exposure - def.tonemap_exposure).abs() > f32::EPSILON {
            props.push((
                "tonemap_exposure".into(),
                Variant::Float(self.tonemap_exposure as f64),
            ));
        }
        if (self.tonemap_white - def.tonemap_white).abs() > f32::EPSILON {
            props.push((
                "tonemap_white".into(),
                Variant::Float(self.tonemap_white as f64),
            ));
        }
        if (self.tonemap_agx_white - def.tonemap_agx_white).abs() > f32::EPSILON {
            props.push((
                "tonemap_agx_white".into(),
                Variant::Float(self.tonemap_agx_white as f64),
            ));
        }
        if (self.tonemap_agx_contrast - def.tonemap_agx_contrast).abs() > f32::EPSILON {
            props.push((
                "tonemap_agx_contrast".into(),
                Variant::Float(self.tonemap_agx_contrast as f64),
            ));
        }
        if self.glow_enabled != def.glow_enabled {
            props.push(("glow_enabled".into(), Variant::Bool(self.glow_enabled)));
        }
        if (self.glow_intensity - def.glow_intensity).abs() > f32::EPSILON {
            props.push((
                "glow_intensity".into(),
                Variant::Float(self.glow_intensity as f64),
            ));
        }
        if (self.glow_strength - def.glow_strength).abs() > f32::EPSILON {
            props.push((
                "glow_strength".into(),
                Variant::Float(self.glow_strength as f64),
            ));
        }
        if (self.glow_bloom - def.glow_bloom).abs() > f32::EPSILON {
            props.push(("glow_bloom".into(), Variant::Float(self.glow_bloom as f64)));
        }
        if self.glow_blend_mode != def.glow_blend_mode {
            props.push((
                "glow_blend_mode".into(),
                Variant::Int(self.glow_blend_mode.to_godot_int()),
            ));
        }
        if (self.glow_hdr_bleed_threshold - def.glow_hdr_bleed_threshold).abs() > f32::EPSILON {
            props.push((
                "glow_hdr_bleed_threshold".into(),
                Variant::Float(self.glow_hdr_bleed_threshold as f64),
            ));
        }
        if (self.glow_hdr_bleed_scale - def.glow_hdr_bleed_scale).abs() > f32::EPSILON {
            props.push((
                "glow_hdr_scale".into(),
                Variant::Float(self.glow_hdr_bleed_scale as f64),
            ));
        }
        if (self.glow_hdr_luminance_cap - def.glow_hdr_luminance_cap).abs() > f32::EPSILON {
            props.push((
                "glow_hdr_luminance_cap".into(),
                Variant::Float(self.glow_hdr_luminance_cap as f64),
            ));
        }
        for (i, (&lvl, &def_lvl)) in self
            .glow_levels
            .iter()
            .zip(def.glow_levels.iter())
            .enumerate()
        {
            if (lvl - def_lvl).abs() > f32::EPSILON {
                props.push((format!("glow_levels/{}", i + 1), Variant::Float(lvl as f64)));
            }
        }
        if self.fog_enabled != def.fog_enabled {
            props.push(("fog_enabled".into(), Variant::Bool(self.fog_enabled)));
        }
        if self.fog_mode != def.fog_mode {
            props.push((
                "fog_mode".into(),
                Variant::Int(self.fog_mode.to_godot_int()),
            ));
        }
        if self.fog_light_color != def.fog_light_color {
            props.push((
                "fog_light_color".into(),
                Variant::Color(self.fog_light_color),
            ));
        }
        if (self.fog_light_energy - def.fog_light_energy).abs() > f32::EPSILON {
            props.push((
                "fog_light_energy".into(),
                Variant::Float(self.fog_light_energy as f64),
            ));
        }
        if (self.fog_sun_scatter - def.fog_sun_scatter).abs() > f32::EPSILON {
            props.push((
                "fog_sun_scatter".into(),
                Variant::Float(self.fog_sun_scatter as f64),
            ));
        }
        if (self.fog_density - def.fog_density).abs() > f32::EPSILON {
            props.push((
                "fog_density".into(),
                Variant::Float(self.fog_density as f64),
            ));
        }
        if (self.fog_height - def.fog_height).abs() > f32::EPSILON {
            props.push(("fog_height".into(), Variant::Float(self.fog_height as f64)));
        }
        if (self.fog_height_density - def.fog_height_density).abs() > f32::EPSILON {
            props.push((
                "fog_height_density".into(),
                Variant::Float(self.fog_height_density as f64),
            ));
        }
        if (self.fog_aerial_perspective - def.fog_aerial_perspective).abs() > f32::EPSILON {
            props.push((
                "fog_aerial_perspective".into(),
                Variant::Float(self.fog_aerial_perspective as f64),
            ));
        }
        if (self.fog_sky_affect - def.fog_sky_affect).abs() > f32::EPSILON {
            props.push((
                "fog_sky_affect".into(),
                Variant::Float(self.fog_sky_affect as f64),
            ));
        }
        if (self.fog_depth_curve - def.fog_depth_curve).abs() > f32::EPSILON {
            props.push((
                "fog_depth_curve".into(),
                Variant::Float(self.fog_depth_curve as f64),
            ));
        }
        if (self.fog_depth_begin - def.fog_depth_begin).abs() > f32::EPSILON {
            props.push((
                "fog_depth_begin".into(),
                Variant::Float(self.fog_depth_begin as f64),
            ));
        }
        if (self.fog_depth_end - def.fog_depth_end).abs() > f32::EPSILON {
            props.push((
                "fog_depth_end".into(),
                Variant::Float(self.fog_depth_end as f64),
            ));
        }
        if self.ssao_enabled != def.ssao_enabled {
            props.push(("ssao_enabled".into(), Variant::Bool(self.ssao_enabled)));
        }
        if (self.ssao_radius - def.ssao_radius).abs() > f32::EPSILON {
            props.push((
                "ssao_radius".into(),
                Variant::Float(self.ssao_radius as f64),
            ));
        }
        if (self.ssao_intensity - def.ssao_intensity).abs() > f32::EPSILON {
            props.push((
                "ssao_intensity".into(),
                Variant::Float(self.ssao_intensity as f64),
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
    fn glow_and_tonemap_defaults_match_godot() {
        let env = Environment3D::default();
        // Godot Environment defaults (scene/resources/environment.h/.cpp).
        assert!(!env.glow_enabled);
        assert_eq!(env.glow_blend_mode, GlowBlendMode::Screen);
        assert!((env.glow_intensity - 0.3).abs() < 1e-6);
        assert!((env.glow_strength - 1.0).abs() < 1e-6);
        assert!((env.glow_hdr_bleed_threshold - 1.0).abs() < 1e-6);
        assert!((env.glow_hdr_bleed_scale - 2.0).abs() < 1e-6);
        assert!((env.glow_hdr_luminance_cap - 12.0).abs() < 1e-6);
        assert_eq!(env.glow_levels, [0.0, 0.8, 0.4, 0.1, 0.0, 0.0, 0.0]);
        assert!((env.tonemap_exposure - 1.0).abs() < 1e-6);
        assert!((env.tonemap_white - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fog_defaults_match_godot() {
        let env = Environment3D::default();
        // scene/resources/environment.h defaults.
        assert!(!env.fog_enabled);
        assert_eq!(env.fog_mode, FogMode::Exponential);
        assert_eq!(env.fog_light_color, Color::new(0.518, 0.553, 0.608, 1.0));
        assert!((env.fog_light_energy - 1.0).abs() < 1e-6);
        assert!((env.fog_sun_scatter - 0.0).abs() < 1e-6);
        assert!((env.fog_density - 0.01).abs() < 1e-6);
        assert!((env.fog_height - 0.0).abs() < 1e-6);
        assert!((env.fog_height_density - 0.0).abs() < 1e-6);
        assert!((env.fog_aerial_perspective - 0.0).abs() < 1e-6);
        assert!((env.fog_sky_affect - 1.0).abs() < 1e-6);
        assert!((env.fog_depth_begin - 10.0).abs() < 1e-6);
        assert!((env.fog_depth_end - 100.0).abs() < 1e-6);
    }

    #[test]
    fn fog_properties_roundtrip() {
        let mut env = Environment3D::default();
        env.fog_enabled = true;
        env.fog_mode = FogMode::Depth;
        env.fog_sun_scatter = 0.4;
        env.fog_height = -2.0;
        env.fog_height_density = 0.7;
        env.fog_aerial_perspective = 0.5;
        env.fog_sky_affect = 0.25;
        env.fog_depth_begin = 5.0;

        let props = env.to_properties();
        let restored = Environment3D::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
        assert!(restored.fog_enabled);
        assert_eq!(restored.fog_mode, FogMode::Depth);
        assert!((restored.fog_sun_scatter - 0.4).abs() < 1e-6);
        assert!((restored.fog_height + 2.0).abs() < 1e-6);
        assert!((restored.fog_height_density - 0.7).abs() < 1e-6);
        assert!((restored.fog_aerial_perspective - 0.5).abs() < 1e-6);
        assert!((restored.fog_sky_affect - 0.25).abs() < 1e-6);
        assert!((restored.fog_depth_begin - 5.0).abs() < 1e-6);
    }

    #[test]
    fn glow_blend_mode_godot_int_roundtrip() {
        for (i, m) in [
            (0, GlowBlendMode::Additive),
            (1, GlowBlendMode::Screen),
            (2, GlowBlendMode::Softlight),
            (3, GlowBlendMode::Replace),
            (4, GlowBlendMode::Mix),
        ] {
            assert_eq!(GlowBlendMode::from_godot_int(i), m);
            assert_eq!(m.to_godot_int(), i);
        }
        assert_eq!(GlowBlendMode::from_godot_int(99), GlowBlendMode::Screen);
    }

    #[test]
    fn glow_and_tonemap_properties_roundtrip() {
        let mut env = Environment3D::default();
        env.glow_enabled = true;
        env.glow_intensity = 0.55;
        env.glow_blend_mode = GlowBlendMode::Softlight;
        env.glow_hdr_bleed_threshold = 0.8;
        env.glow_levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7];
        env.tone_mapper = ToneMapper::Filmic;
        env.tonemap_exposure = 1.4;
        env.tonemap_white = 6.0;

        let props = env.to_properties();
        // `glow_levels/1`..`/7` all differ from defaults here.
        let level_keys = props
            .iter()
            .filter(|(k, _)| k.starts_with("glow_levels/"))
            .count();
        assert_eq!(level_keys, 7);

        let restored = Environment3D::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
        assert_eq!(restored.glow_enabled, true);
        assert!((restored.glow_intensity - 0.55).abs() < 1e-6);
        assert_eq!(restored.glow_blend_mode, GlowBlendMode::Softlight);
        assert!((restored.glow_hdr_bleed_threshold - 0.8).abs() < 1e-6);
        assert_eq!(restored.glow_levels, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]);
        assert_eq!(restored.tone_mapper, ToneMapper::Filmic);
        assert!((restored.tonemap_exposure - 1.4).abs() < 1e-6);
        assert!((restored.tonemap_white - 6.0).abs() < 1e-6);
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
        assert_eq!(
            env.sky.as_ref().unwrap().process_mode,
            SkyProcessMode::RealTime
        );
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
        assert_eq!(
            BackgroundMode::from_godot_int(99),
            BackgroundMode::ClearColor
        );
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
        let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
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
        let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert!(env.fog_enabled);
        assert_eq!(env.fog_light_color, fog_color);
        assert!((env.fog_density - 0.05).abs() < 1e-5);
    }

    #[test]
    fn from_properties_tonemap() {
        let props: Vec<(&str, Variant)> = vec![("tonemap_mode", Variant::Int(3))];
        let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert_eq!(env.tone_mapper, ToneMapper::Aces);
    }

    /// AgX (Godot ToneMapper int 4) round-trips with its dedicated white /
    /// contrast properties; defaults are Blender's (16.29 / 1.25).
    #[test]
    fn from_properties_tonemap_agx() {
        assert!((Environment3D::default().tonemap_agx_white - 16.29).abs() < 1e-5);
        assert!((Environment3D::default().tonemap_agx_contrast - 1.25).abs() < 1e-5);

        let props: Vec<(&str, Variant)> = vec![
            ("tonemap_mode", Variant::Int(4)),
            ("tonemap_agx_white", Variant::Float(8.0)),
            ("tonemap_agx_contrast", Variant::Float(1.4)),
        ];
        let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert_eq!(env.tone_mapper, ToneMapper::Agx);
        assert!((env.tonemap_agx_white - 8.0).abs() < 1e-5);
        assert!((env.tonemap_agx_contrast - 1.4).abs() < 1e-5);

        // Out-of-range contrast clamps to Godot's property floor (1.0) —
        // contrast ≤ 0 would NaN the allenwp shoulder. (Named `clamped`, not
        // `env`: the serialization checks below still use the AgX env.)
        let zero: Vec<(&str, Variant)> = vec![("tonemap_agx_contrast", Variant::Float(0.0))];
        let clamped = Environment3D::from_properties(zero.iter().map(|(k, v)| (*k, v)));
        assert!(
            (clamped.tonemap_agx_contrast - 1.0).abs() < 1e-5,
            "contrast must clamp to 1.0"
        );

        // And they serialize back (non-default values only).
        let out = env.to_properties();
        assert!(out.iter().any(|(k, _)| k == "tonemap_agx_white"));
        assert!(out.iter().any(|(k, _)| k == "tonemap_agx_contrast"));
    }

    #[test]
    fn from_properties_background() {
        let bg = Color::new(0.2, 0.3, 0.4, 1.0);
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
    fn to_properties_default_is_empty() {
        let env = Environment3D::default();
        let props = env.to_properties();
        assert!(
            props.is_empty(),
            "default environment should emit no properties"
        );
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
        let reconstructed =
            Environment3D::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
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
    fn ssao_defaults_match_godot() {
        let env = Environment3D::default();
        assert!(!env.ssao_enabled);
        assert!((env.ssao_radius - 1.0).abs() < f32::EPSILON);
        assert!((env.ssao_intensity - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ssao_from_properties_parses_tscn_fields() {
        let props: Vec<(&str, Variant)> = vec![
            ("ssao_enabled", Variant::Bool(true)),
            ("ssao_radius", Variant::Float(4.5)),
            ("ssao_intensity", Variant::Float(3.0)),
        ];
        let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert!(env.ssao_enabled);
        assert!((env.ssao_radius - 4.5).abs() < 1e-5);
        assert!((env.ssao_intensity - 3.0).abs() < 1e-5);
    }

    #[test]
    fn ssao_to_properties_roundtrip() {
        let env = Environment3D {
            ssao_enabled: true,
            ssao_radius: 2.5,
            ssao_intensity: 1.5,
            ..Default::default()
        };
        let props = env.to_properties();
        // Only the three SSAO overrides should be emitted (all else default).
        assert!(props.iter().any(|(k, _)| k == "ssao_enabled"));
        let reconstructed =
            Environment3D::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
        assert!(reconstructed.ssao_enabled);
        assert!((reconstructed.ssao_radius - 2.5).abs() < 1e-5);
        assert!((reconstructed.ssao_intensity - 1.5).abs() < 1e-5);
    }

    #[test]
    fn from_properties_ignores_unknown() {
        let props: Vec<(&str, Variant)> = vec![
            ("unknown_prop", Variant::Int(42)),
            ("fog_enabled", Variant::Bool(true)),
        ];
        let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));
        assert!(env.fog_enabled);
        // everything else stays default
        assert_eq!(env.background_mode, BackgroundMode::ClearColor);
    }
}
