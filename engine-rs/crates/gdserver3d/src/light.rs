//! 3D light types for the rendering server.

use gdcore::math::Color;
use gdcore::math::Vector3;

/// Unique identifier for a 3D light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Light3DId(pub u64);

/// The type of 3D light source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    /// Infinite directional light (like the sun).
    Directional,
    /// Point light that emits in all directions from a position.
    Point,
    /// Spot light that emits in a cone from a position.
    Spot,
}

/// Godot-compatible shadow mode for OmniLight3D.
///
/// Determines how the shadow map is constructed for point lights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmniShadowMode {
    /// Render shadows using a dual-paraboloid map (Godot default).
    DualParaboloid,
    /// Render shadows using a 6-face cubemap (higher quality).
    Cube,
}

impl Default for OmniShadowMode {
    fn default() -> Self {
        Self::DualParaboloid
    }
}

/// Converts a colour temperature in Kelvin to an RGB tint, mirroring Godot's
/// `Light3D.light_temperature` (Tanner Helland's blackbody approximation, the
/// same curve Godot uses).
///
/// 6500K is daylight-neutral (very close to white); lower is warm (orange),
/// higher is cool (blue). Input is clamped to the 1000–40000K range the
/// approximation is defined over.
pub fn kelvin_to_rgb(kelvin: f32) -> Color {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;

    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_73 * (t - 60.0).powf(-0.133_204_76)
    };
    let g = if t <= 66.0 {
        99.470_802 * t.ln() - 161.119_57
    } else {
        288.122_17 * (t - 60.0).powf(-0.075_514_85)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };

    Color::new(
        (r / 255.0).clamp(0.0, 1.0),
        (g / 255.0).clamp(0.0, 1.0),
        (b / 255.0).clamp(0.0, 1.0),
        1.0,
    )
}

/// Soft-shadow filter quality, mirroring Godot's
/// `rendering/lights_and_shadows/soft_shadow_filter_quality` levels.
///
/// [`Hard`](Self::Hard) is the default and takes a single tap, giving the
/// binary lit/shadowed edge. The soft levels take multiple taps across the
/// light's [`shadow_blur`](Light3D::shadow_blur) radius and average the
/// results, so a texel partially occluded lands between 0 and 1 (a penumbra).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowFilterQuality {
    /// Single tap: fully lit or fully shadowed, no penumbra.
    Hard,
    /// 4 taps.
    Low,
    /// 8 taps.
    Medium,
    /// 16 taps.
    High,
}

impl Default for ShadowFilterQuality {
    fn default() -> Self {
        Self::Hard
    }
}

impl ShadowFilterQuality {
    /// Number of shadow taps this quality level takes.
    pub fn taps(self) -> u32 {
        match self {
            Self::Hard => 1,
            Self::Low => 4,
            Self::Medium => 8,
            Self::High => 16,
        }
    }

    /// Whether this level filters across the blur radius (more than one tap).
    pub fn is_soft(self) -> bool {
        self.taps() > 1
    }
}

/// A texture projected through a light, mirroring Godot's
/// `Light3D.light_projector` (a Texture2D shone through the light cone).
///
/// Stored as CPU texels because this crate renders on the CPU and carries no
/// GPU dependency. Texel `(0, 0)` is the top-left of the projected image.
#[derive(Debug, Clone, PartialEq)]
pub struct LightProjector {
    /// Texture width in texels.
    pub width: u32,
    /// Texture height in texels.
    pub height: u32,
    /// Row-major texel colors (`width * height` entries).
    pub texels: Vec<Color>,
}

impl LightProjector {
    /// Creates a projector from row-major `texels`.
    ///
    /// Returns `None` unless `texels.len() == width * height` and both
    /// dimensions are non-zero, so a malformed projector cannot be sampled.
    pub fn new(width: u32, height: u32, texels: Vec<Color>) -> Option<Self> {
        if width == 0 || height == 0 || texels.len() != (width as usize) * (height as usize) {
            return None;
        }
        Some(Self {
            width,
            height,
            texels,
        })
    }

    /// Creates a 1x1 projector of a single color (a plain light gel).
    pub fn solid(color: Color) -> Self {
        Self {
            width: 1,
            height: 1,
            texels: vec![color],
        }
    }

    /// Samples the projector at normalized `(u, v)`, clamping to the edge.
    ///
    /// UVs outside `[0, 1]` clamp rather than wrap, matching how Godot's
    /// projector fades at the cone edge instead of tiling.
    pub fn sample_uv(&self, u: f32, v: f32) -> Color {
        if self.texels.is_empty() {
            return Color::WHITE;
        }
        let x = ((u.clamp(0.0, 1.0) * self.width as f32) as u32).min(self.width - 1);
        let y = ((v.clamp(0.0, 1.0) * self.height as f32) as u32).min(self.height - 1);
        self.texels[(y * self.width + x) as usize]
    }
}

/// Builds an orthonormal (right, up) basis perpendicular to `dir`.
///
/// Used to project a world point onto the plane facing a spot light so the
/// projector texture can be addressed in 2D.
fn perpendicular_basis(dir: Vector3) -> (Vector3, Vector3) {
    let d = dir.normalized();
    // Any reference axis not parallel to `d`; swap when `d` is near-vertical.
    let reference = if d.y.abs() > 0.99 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let right = reference.cross(d).normalized();
    let up = d.cross(right).normalized();
    (right, up)
}

/// Index of a cubemap face (+X, -X, +Y, -Y, +Z, -Z).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CubeFace {
    PositiveX = 0,
    NegativeX = 1,
    PositiveY = 2,
    NegativeY = 3,
    PositiveZ = 4,
    NegativeZ = 5,
}

impl CubeFace {
    /// All six faces in order.
    pub const ALL: [CubeFace; 6] = [
        Self::PositiveX,
        Self::NegativeX,
        Self::PositiveY,
        Self::NegativeY,
        Self::PositiveZ,
        Self::NegativeZ,
    ];

    /// Returns the forward direction vector for this cube face.
    pub fn forward(&self) -> Vector3 {
        match self {
            Self::PositiveX => Vector3::new(1.0, 0.0, 0.0),
            Self::NegativeX => Vector3::new(-1.0, 0.0, 0.0),
            Self::PositiveY => Vector3::new(0.0, 1.0, 0.0),
            Self::NegativeY => Vector3::new(0.0, -1.0, 0.0),
            Self::PositiveZ => Vector3::new(0.0, 0.0, 1.0),
            Self::NegativeZ => Vector3::new(0.0, 0.0, -1.0),
        }
    }

    /// Returns the up vector for this cube face.
    pub fn up(&self) -> Vector3 {
        match self {
            Self::PositiveX | Self::NegativeX => Vector3::new(0.0, -1.0, 0.0),
            Self::PositiveY => Vector3::new(0.0, 0.0, 1.0),
            Self::NegativeY => Vector3::new(0.0, 0.0, -1.0),
            Self::PositiveZ | Self::NegativeZ => Vector3::new(0.0, -1.0, 0.0),
        }
    }
}

/// A cubemap shadow map for an omnidirectional point light.
///
/// Stores depth values for 6 faces, each `resolution x resolution` pixels.
/// Depth values represent the linear distance from the light source.
#[derive(Debug, Clone)]
pub struct ShadowCubemap {
    /// Resolution of each face (width = height = resolution).
    pub resolution: u32,
    /// Depth data for all 6 faces. Each face is `resolution * resolution` f32 values.
    /// Face order matches [`CubeFace`] enum order: +X, -X, +Y, -Y, +Z, -Z.
    pub faces: [Vec<f32>; 6],
}

impl ShadowCubemap {
    /// Creates a new shadow cubemap with all faces cleared to max depth.
    pub fn new(resolution: u32) -> Self {
        let face_size = (resolution * resolution) as usize;
        Self {
            resolution,
            faces: std::array::from_fn(|_| vec![f32::MAX; face_size]),
        }
    }

    /// Clears all faces to max depth.
    pub fn clear(&mut self) {
        for face in &mut self.faces {
            face.fill(f32::MAX);
        }
    }

    /// Returns the depth at the given face, pixel coordinate.
    pub fn get_depth(&self, face: CubeFace, x: u32, y: u32) -> f32 {
        if x >= self.resolution || y >= self.resolution {
            return f32::MAX;
        }
        self.faces[face as usize][(y * self.resolution + x) as usize]
    }

    /// Sets the depth at the given face, pixel coordinate if closer than existing.
    /// Returns `true` if the depth was written (closer than existing).
    pub fn test_and_set(&mut self, face: CubeFace, x: u32, y: u32, depth: f32) -> bool {
        if x >= self.resolution || y >= self.resolution {
            return false;
        }
        let idx = (y * self.resolution + x) as usize;
        let slot = &mut self.faces[face as usize][idx];
        if depth < *slot {
            *slot = depth;
            true
        } else {
            false
        }
    }

    /// Samples the cubemap given a direction vector from light to fragment.
    ///
    /// Returns the stored depth for the texel that the direction maps to.
    pub fn sample(&self, direction: Vector3) -> f32 {
        let (face, u, v) = Self::direction_to_face_uv(direction);
        let x = ((u * self.resolution as f32) as u32).min(self.resolution.saturating_sub(1));
        let y = ((v * self.resolution as f32) as u32).min(self.resolution.saturating_sub(1));
        self.get_depth(face, x, y)
    }

    /// Converts a direction vector to (face, u, v) coordinates.
    fn direction_to_face_uv(dir: Vector3) -> (CubeFace, f32, f32) {
        let ax = dir.x.abs();
        let ay = dir.y.abs();
        let az = dir.z.abs();

        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if dir.x > 0.0 {
                (CubeFace::PositiveX, -dir.z, -dir.y, ax)
            } else {
                (CubeFace::NegativeX, dir.z, -dir.y, ax)
            }
        } else if ay >= ax && ay >= az {
            if dir.y > 0.0 {
                (CubeFace::PositiveY, dir.x, dir.z, ay)
            } else {
                (CubeFace::NegativeY, dir.x, -dir.z, ay)
            }
        } else if dir.z > 0.0 {
            (CubeFace::PositiveZ, dir.x, -dir.y, az)
        } else {
            (CubeFace::NegativeZ, -dir.x, -dir.y, az)
        };

        // Avoid division by zero for zero-length directions.
        if ma < f32::EPSILON {
            return (face, 0.5, 0.5);
        }

        let u = (sc / ma * 0.5 + 0.5).clamp(0.0, 1.0);
        let v = (tc / ma * 0.5 + 0.5).clamp(0.0, 1.0);

        (face, u, v)
    }
}

/// A 3D light source.
#[derive(Debug, Clone, PartialEq)]
pub struct Light3D {
    /// Unique identifier.
    pub id: Light3DId,
    /// Type of light.
    pub light_type: LightType,
    /// Light color.
    pub color: Color,
    /// Light energy/intensity multiplier.
    pub energy: f32,
    /// Position in world space (ignored for directional lights).
    pub position: Vector3,
    /// Direction the light points (relevant for directional and spot lights).
    pub direction: Vector3,
    /// Spot angle in radians (only for spot lights, half-angle of the cone).
    pub spot_angle: f32,
    /// Maximum range (for point and spot lights, 0 = infinite).
    pub range: f32,
    /// Distance attenuation curve exponent (1.0 = linear, 2.0 = quadratic).
    pub attenuation: f32,
    /// Spot cone angle attenuation curve exponent.
    pub spot_angle_attenuation: f32,
    /// Whether the light casts shadows.
    pub shadow_enabled: bool,
    /// Shadow mode for OmniLight3D (ignored for other light types).
    pub omni_shadow_mode: OmniShadowMode,
    /// Angular size of the light in degrees (Godot's `light_angular_distance`).
    /// For directional lights this is the apparent diameter of the sun disc
    /// drawn by sky materials; 0 means a point-like sun (halo only).
    pub angular_distance: f32,
    /// Whether the light contributes to sky rendering (Godot's
    /// `DirectionalLight3D.sky_mode` LIGHT_AND_SKY vs LIGHT_ONLY). The editor
    /// uses this to keep its dim fill light out of the sky when the preview
    /// sun is toggled off.
    pub sky: bool,
    /// Whether the light shades scene geometry. Godot's
    /// `DirectionalLight3D.sky_mode = SKY_ONLY` sets this false: the light still
    /// draws its sky sun disc ([`sky`](Self::sky) stays true) but contributes
    /// nothing to surface lighting. The complement of [`sky`](Self::sky):
    /// `LIGHT_ONLY` clears `sky`, `SKY_ONLY` clears `affects_scene`, and the
    /// default `LIGHT_AND_SKY` leaves both set.
    pub affects_scene: bool,
    /// Texture projected through this light (Godot's `Light3D.light_projector`).
    /// `None` (the default) leaves the light's color unmodulated.
    pub projector: Option<LightProjector>,
    /// Radius the soft-shadow filter spreads its taps over, in world units
    /// (Godot's `Light3D.shadow_blur`). Only used when
    /// [`shadow_filter_quality`](Self::shadow_filter_quality) is a soft level.
    pub shadow_blur: f32,
    /// How many taps the shadow filter takes. Defaults to
    /// [`ShadowFilterQuality::Hard`] (a binary edge, matching the previous
    /// behavior); a soft level produces penumbrae.
    pub shadow_filter_quality: ShadowFilterQuality,
    /// Colour temperature in Kelvin (Godot's `Light3D.light_temperature`).
    /// 6500 is daylight-neutral. The tint is applied into
    /// [`color`](Self::color) when a scene sets the property; this field keeps
    /// the authored value for introspection and re-tinting.
    pub temperature: f32,
    /// Render layers this light affects (Godot's `Light3D.light_cull_mask`).
    /// A mesh is lit only when its layer bit is set here; defaults to all
    /// layers.
    pub cull_mask: u32,
    /// Multiplier for this light's contribution to indirect/baked lighting
    /// (Godot's `light_indirect_energy`).
    pub indirect_energy: f32,
    /// Multiplier for this light's contribution to volumetric fog
    /// (Godot's `light_volumetric_fog_energy`).
    pub volumetric_fog_energy: f32,
    /// Whether the light fades out with distance (`distance_fade_enabled`).
    pub distance_fade_enabled: bool,
    /// Distance at which the light begins to fade (`distance_fade_begin`).
    pub distance_fade_begin: f32,
    /// Distance at which the light's shadow fades (`distance_fade_shadow`).
    pub distance_fade_shadow: f32,
    /// Length of the distance fade transition (`distance_fade_length`).
    pub distance_fade_length: f32,
    /// Shadow darkness, 0..1 (Godot's `shadow_opacity`).
    pub shadow_opacity: f32,
    /// Layers whose meshes cast shadows from this light
    /// (Godot's `shadow_caster_mask`).
    pub shadow_caster_mask: u32,
    /// Whether the light exists only in the editor and is stripped at runtime
    /// (Godot's `editor_only`).
    pub editor_only: bool,
    /// DirectionalLight3D PSSM split fractions of the shadow max distance
    /// (`directional_shadow_split_1/2/3`). Ignored for other light types.
    pub shadow_split_1: f32,
    /// See [`shadow_split_1`](Self::shadow_split_1).
    pub shadow_split_2: f32,
    /// See [`shadow_split_1`](Self::shadow_split_1).
    pub shadow_split_3: f32,
    /// Whether PSSM splits blend across their boundaries
    /// (`directional_shadow_blend_splits`).
    pub blend_splits: bool,
}

impl Light3D {
    /// Creates a new directional light pointing downward.
    pub fn directional(id: Light3DId) -> Self {
        Self {
            id,
            light_type: LightType::Directional,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            position: Vector3::ZERO,
            direction: Vector3::new(0.0, -1.0, 0.0),
            spot_angle: 0.0,
            range: 0.0,
            attenuation: 1.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
            omni_shadow_mode: OmniShadowMode::default(),
            angular_distance: 0.0,
            sky: true,
            affects_scene: true,
            projector: None,
            shadow_blur: 0.0,
            shadow_filter_quality: ShadowFilterQuality::Hard,
            temperature: 6500.0,
            cull_mask: u32::MAX,
            indirect_energy: 1.0,
            volumetric_fog_energy: 1.0,
            distance_fade_enabled: false,
            distance_fade_begin: 40.0,
            distance_fade_shadow: 50.0,
            distance_fade_length: 10.0,
            shadow_opacity: 1.0,
            shadow_caster_mask: u32::MAX,
            editor_only: false,
            shadow_split_1: 0.1,
            shadow_split_2: 0.2,
            shadow_split_3: 0.5,
            blend_splits: false,
        }
    }

    /// Creates a new point light at the given position.
    pub fn point(id: Light3DId, position: Vector3) -> Self {
        Self {
            id,
            light_type: LightType::Point,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            position,
            direction: Vector3::ZERO,
            spot_angle: 0.0,
            range: 10.0,
            attenuation: 1.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
            omni_shadow_mode: OmniShadowMode::default(),
            angular_distance: 0.0,
            sky: true,
            affects_scene: true,
            projector: None,
            shadow_blur: 0.0,
            shadow_filter_quality: ShadowFilterQuality::Hard,
            temperature: 6500.0,
            cull_mask: u32::MAX,
            indirect_energy: 1.0,
            volumetric_fog_energy: 1.0,
            distance_fade_enabled: false,
            distance_fade_begin: 40.0,
            distance_fade_shadow: 50.0,
            distance_fade_length: 10.0,
            shadow_opacity: 1.0,
            shadow_caster_mask: u32::MAX,
            editor_only: false,
            shadow_split_1: 0.1,
            shadow_split_2: 0.2,
            shadow_split_3: 0.5,
            blend_splits: false,
        }
    }

    /// Creates a new spot light at the given position and direction.
    pub fn spot(id: Light3DId, position: Vector3, direction: Vector3) -> Self {
        Self {
            id,
            light_type: LightType::Spot,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            position,
            direction,
            spot_angle: std::f32::consts::FRAC_PI_4,
            range: 10.0,
            attenuation: 1.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
            omni_shadow_mode: OmniShadowMode::default(),
            angular_distance: 0.0,
            sky: true,
            affects_scene: true,
            projector: None,
            shadow_blur: 0.0,
            shadow_filter_quality: ShadowFilterQuality::Hard,
            temperature: 6500.0,
            cull_mask: u32::MAX,
            indirect_energy: 1.0,
            volumetric_fog_energy: 1.0,
            distance_fade_enabled: false,
            distance_fade_begin: 40.0,
            distance_fade_shadow: 50.0,
            distance_fade_length: 10.0,
            shadow_opacity: 1.0,
            shadow_caster_mask: u32::MAX,
            editor_only: false,
            shadow_split_1: 0.1,
            shadow_split_2: 0.2,
            shadow_split_3: 0.5,
            blend_splits: false,
        }
    }

    /// Whether this light affects a mesh on `layer` (a 1-based Godot render
    /// layer, as used by `VisualInstance3D.layers`).
    ///
    /// Mirrors Godot's `light_cull_mask` test: the light lights the mesh only
    /// when the mesh's layer bit is set in the mask. Layers outside 1..=32 are
    /// not addressable by the 32-bit mask and are treated as unlit.
    pub fn affects_layer(&self, layer: u32) -> bool {
        if !(1..=32).contains(&layer) {
            return false;
        }
        self.cull_mask & (1 << (layer - 1)) != 0
    }

    /// The RGB tint this light's colour temperature applies
    /// (Godot's `light_temperature`). 6500K is daylight-neutral.
    pub fn temperature_tint(&self) -> Color {
        kelvin_to_rgb(self.temperature)
    }

    /// Returns the projector tint this light casts onto `world_pos`, or
    /// [`Color::WHITE`] when the light has no projector (i.e. no modulation).
    ///
    /// The point is projected onto the plane facing the light and addressed in
    /// the cone's cross-section, so the texture is shone through the light the
    /// way Godot's `light_projector` is. Points behind a spot light, and light
    /// types without a projection frame, return `WHITE`.
    ///
    /// Only spot lights project today; omni (cubemap-addressed) and directional
    /// projectors are not implemented and return `WHITE`.
    pub fn projector_modulate(&self, world_pos: Vector3) -> Color {
        let projector = match self.projector.as_ref() {
            Some(p) => p,
            None => return Color::WHITE,
        };
        match self.light_type {
            LightType::Spot => {
                let to_point = world_pos - self.position;
                let axis = self.direction.normalized();
                let distance_along = to_point.dot(axis);
                // Behind (or exactly at) the light: nothing is projected.
                if distance_along <= 0.0 {
                    return Color::WHITE;
                }
                // Half-width of the cone's cross-section at this distance; the
                // texture spans the cone, so this maps the point into [0, 1].
                let half_extent = (self.spot_angle.tan() * distance_along).max(1e-6);
                let (right, up) = perpendicular_basis(axis);
                let u = (to_point.dot(right) / half_extent) * 0.5 + 0.5;
                let v = (to_point.dot(up) / half_extent) * 0.5 + 0.5;
                projector.sample_uv(u, v)
            }
            LightType::Directional | LightType::Point => Color::WHITE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- pat-nzgdu.121: temperature / cull mask / PSSM ------------------------

    /// 6500K is daylight-neutral, so a light left at the default temperature
    /// must not tint. This is what keeps temperature opt-in: a scene that never
    /// sets `light_temperature` renders exactly as before.
    #[test]
    fn kelvin_6500_is_neutral() {
        let c = kelvin_to_rgb(6500.0);
        assert!(
            (c.r - c.g).abs() < 0.05 && (c.r - c.b).abs() < 0.05,
            "6500K must be ~neutral, got {c:?}",
        );
    }

    /// Below 6500K is warm (orange), above is cool (blue) — the direction of
    /// Godot's blackbody curve.
    #[test]
    fn kelvin_below_neutral_is_warm_above_is_cool() {
        let warm = kelvin_to_rgb(3000.0);
        assert!(warm.r > warm.g && warm.g > warm.b, "3000K must be warm, got {warm:?}");

        let cool = kelvin_to_rgb(20000.0);
        assert!(cool.b > cool.r, "20000K must be cool, got {cool:?}");

        // A warm light is measurably less blue than the neutral reference —
        // this is the "tinted vs a 6500K light" comparison.
        assert!(
            warm.b < kelvin_to_rgb(6500.0).b,
            "3000K must be less blue than 6500K",
        );
    }

    /// The approximation is only defined over 1000..=40000K; outside that it
    /// clamps rather than producing NaNs or out-of-range channels.
    #[test]
    fn kelvin_clamps_out_of_range_and_stays_in_gamut() {
        assert_eq!(kelvin_to_rgb(500.0), kelvin_to_rgb(1000.0));
        assert_eq!(kelvin_to_rgb(99999.0), kelvin_to_rgb(40000.0));
        for k in [1000.0, 3000.0, 6500.0, 40000.0] {
            let c = kelvin_to_rgb(k);
            for ch in [c.r, c.g, c.b] {
                assert!((0.0..=1.0).contains(&ch), "{k}K channel {ch} out of gamut");
            }
        }
    }

    /// A light's temperature tint is reachable from the record.
    #[test]
    fn light_temperature_defaults_to_neutral_6500() {
        let light = Light3D::directional(Light3DId(1));
        assert_eq!(light.temperature, 6500.0);
        let tint = light.temperature_tint();
        assert!((tint.r - tint.g).abs() < 0.05 && (tint.r - tint.b).abs() < 0.05);
    }

    /// `light_cull_mask` decides which render layers a light touches. The
    /// default lights every layer, so existing scenes are unaffected.
    #[test]
    fn cull_mask_defaults_to_all_layers() {
        let light = Light3D::directional(Light3DId(1));
        assert_eq!(light.cull_mask, u32::MAX);
        for layer in [1u32, 2, 16, 32] {
            assert!(light.affects_layer(layer), "layer {layer} must be lit by default");
        }
    }

    /// A mask excluding a mesh's layer leaves that mesh unlit — the bead's
    /// cull-mask acceptance, at the light record.
    #[test]
    fn cull_mask_excludes_unset_layers() {
        let mut light = Light3D::directional(Light3DId(1));
        // Only layer 1 (bit 0).
        light.cull_mask = 0b1;
        assert!(light.affects_layer(1), "layer 1 is in the mask");
        assert!(!light.affects_layer(2), "layer 2 must be excluded");
        assert!(!light.affects_layer(32), "layer 32 must be excluded");

        // Layers outside 1..=32 aren't addressable by a 32-bit mask.
        assert!(!light.affects_layer(0), "layer 0 is not a Godot layer");
        assert!(!light.affects_layer(33), "layer 33 exceeds the mask");
    }

    /// PSSM splits and the rest of the new property surface carry Godot's
    /// defaults on the record.
    #[test]
    fn light_property_surface_matches_godot_defaults() {
        let l = Light3D::directional(Light3DId(1));
        assert_eq!(l.shadow_split_1, 0.1);
        assert_eq!(l.shadow_split_2, 0.2);
        assert_eq!(l.shadow_split_3, 0.5);
        assert!(!l.blend_splits);
        assert_eq!(l.indirect_energy, 1.0);
        assert_eq!(l.volumetric_fog_energy, 1.0);
        assert_eq!(l.shadow_opacity, 1.0);
        assert_eq!(l.shadow_caster_mask, u32::MAX);
        assert!(!l.editor_only);
        assert!(!l.distance_fade_enabled);
        assert_eq!(l.distance_fade_begin, 40.0);
        assert_eq!(l.distance_fade_shadow, 50.0);
        assert_eq!(l.distance_fade_length, 10.0);
    }

    // -- pat-p0s82.25: light projector ---------------------------------------

    const RED: Color = Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    const BLUE: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    /// A light with no projector modulates nothing — the default must leave
    /// existing lighting untouched.
    #[test]
    fn light_without_projector_modulates_white() {
        let light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        assert!(light.projector.is_none());
        assert_eq!(light.projector_modulate(Vector3::ZERO), Color::WHITE);
    }

    /// The projector is addressed by where the point falls in the light's cone,
    /// not returned wholesale: a 2x1 texture shows its left texel on one side of
    /// the axis and its right texel on the other.
    #[test]
    fn spot_projector_addresses_texture_by_cone_position() {
        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.projector = LightProjector::new(2, 1, vec![RED, BLUE]);
        assert!(light.projector.is_some(), "2x1 projector must be accepted");

        // The cone's horizontal axis runs along -Z, so opposite z lands on
        // opposite texels of the projected image.
        assert_eq!(light.projector_modulate(Vector3::new(0.0, 0.0, 2.0)), RED);
        assert_eq!(light.projector_modulate(Vector3::new(0.0, 0.0, -2.0)), BLUE);
    }

    /// Points behind a spot light are not in its projection frame.
    #[test]
    fn spot_projector_ignores_points_behind_the_light() {
        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.projector = Some(LightProjector::solid(RED));
        // Above the light, i.e. opposite the direction it points.
        assert_eq!(
            light.projector_modulate(Vector3::new(0.0, 9.0, 0.0)),
            Color::WHITE,
        );
    }

    /// Omni/directional projectors are not implemented; they must degrade to no
    /// modulation rather than mis-projecting the spot math.
    #[test]
    fn non_spot_projector_is_unmodulated() {
        let mut light = Light3D::point(Light3DId(1), Vector3::new(0.0, 5.0, 0.0));
        light.projector = Some(LightProjector::solid(RED));
        assert_eq!(light.projector_modulate(Vector3::ZERO), Color::WHITE);
    }

    /// A malformed projector (texel count not matching the dimensions) is
    /// rejected at construction rather than panicking at sample time.
    #[test]
    fn projector_rejects_mismatched_texel_count() {
        assert!(LightProjector::new(2, 2, vec![RED]).is_none());
        assert!(LightProjector::new(0, 0, vec![]).is_none());
        assert!(LightProjector::new(1, 1, vec![RED]).is_some());
    }

    /// UVs outside the cone clamp to the edge instead of wrapping or panicking.
    #[test]
    fn projector_sample_uv_clamps_to_edge() {
        let p = LightProjector::new(2, 1, vec![RED, BLUE]).unwrap();
        assert_eq!(p.sample_uv(-5.0, 0.0), RED);
        assert_eq!(p.sample_uv(5.0, 5.0), BLUE);
        // u = 1.0 must land on the last texel, not out of bounds.
        assert_eq!(p.sample_uv(1.0, 1.0), BLUE);
    }

    #[test]
    fn directional_light_defaults() {
        let light = Light3D::directional(Light3DId(1));
        assert_eq!(light.light_type, LightType::Directional);
        assert!((light.energy - 1.0).abs() < f32::EPSILON);
        assert!(!light.shadow_enabled);
    }

    #[test]
    fn point_light_at_position() {
        let light = Light3D::point(Light3DId(2), Vector3::new(5.0, 10.0, 0.0));
        assert_eq!(light.light_type, LightType::Point);
        assert_eq!(light.position, Vector3::new(5.0, 10.0, 0.0));
        assert!((light.range - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spot_light_with_direction() {
        let light = Light3D::spot(
            Light3DId(3),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        assert_eq!(light.light_type, LightType::Spot);
        assert!((light.spot_angle - std::f32::consts::FRAC_PI_4).abs() < f32::EPSILON);
        assert!((light.attenuation - 1.0).abs() < f32::EPSILON);
        assert!((light.spot_angle_attenuation - 1.0).abs() < f32::EPSILON);
        assert!((light.range - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spot_light_custom_attenuation() {
        let mut light = Light3D::spot(
            Light3DId(4),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.attenuation = 2.0;
        light.spot_angle_attenuation = 0.5;
        light.spot_angle = std::f32::consts::FRAC_PI_6; // 30 degrees
        light.range = 20.0;
        light.shadow_enabled = true;

        assert!((light.attenuation - 2.0).abs() < f32::EPSILON);
        assert!((light.spot_angle_attenuation - 0.5).abs() < f32::EPSILON);
        assert!((light.spot_angle - std::f32::consts::FRAC_PI_6).abs() < f32::EPSILON);
        assert!((light.range - 20.0).abs() < f32::EPSILON);
        assert!(light.shadow_enabled);
    }

    #[test]
    fn point_light_default_attenuation() {
        let light = Light3D::point(Light3DId(5), Vector3::ZERO);
        assert!((light.attenuation - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn light_id_equality() {
        assert_eq!(Light3DId(10), Light3DId(10));
        assert_ne!(Light3DId(10), Light3DId(20));
    }

    #[test]
    fn omni_shadow_mode_default_is_dual_paraboloid() {
        assert_eq!(OmniShadowMode::default(), OmniShadowMode::DualParaboloid);
    }

    #[test]
    fn point_light_default_omni_shadow_mode() {
        let light = Light3D::point(Light3DId(10), Vector3::ZERO);
        assert_eq!(light.omni_shadow_mode, OmniShadowMode::DualParaboloid);
    }

    // ── CubeFace tests ──

    #[test]
    fn cube_face_all_has_six_faces() {
        assert_eq!(CubeFace::ALL.len(), 6);
    }

    #[test]
    fn cube_face_forward_vectors_are_unit_length() {
        for face in CubeFace::ALL {
            let fwd = face.forward();
            let len = (fwd.x * fwd.x + fwd.y * fwd.y + fwd.z * fwd.z).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-5,
                "face {:?} forward not unit: {}",
                face,
                len
            );
        }
    }

    #[test]
    fn cube_face_up_vectors_are_unit_length() {
        for face in CubeFace::ALL {
            let up = face.up();
            let len = (up.x * up.x + up.y * up.y + up.z * up.z).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-5,
                "face {:?} up not unit: {}",
                face,
                len
            );
        }
    }

    #[test]
    fn cube_face_forward_and_up_are_orthogonal() {
        for face in CubeFace::ALL {
            let dot = face.forward().dot(face.up());
            assert!(dot.abs() < 1e-5, "face {:?} forward·up = {}", face, dot);
        }
    }

    // ── ShadowCubemap tests ──

    #[test]
    fn shadow_cubemap_new_initialized_to_max() {
        let cm = ShadowCubemap::new(4);
        assert_eq!(cm.resolution, 4);
        for face in CubeFace::ALL {
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(cm.get_depth(face, x, y), f32::MAX);
                }
            }
        }
    }

    #[test]
    fn shadow_cubemap_test_and_set() {
        let mut cm = ShadowCubemap::new(4);
        assert!(cm.test_and_set(CubeFace::PositiveX, 1, 2, 5.0));
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 1, 2), 5.0);
        // Closer depth wins.
        assert!(cm.test_and_set(CubeFace::PositiveX, 1, 2, 3.0));
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 1, 2), 3.0);
        // Farther depth is rejected.
        assert!(!cm.test_and_set(CubeFace::PositiveX, 1, 2, 4.0));
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 1, 2), 3.0);
    }

    #[test]
    fn shadow_cubemap_out_of_bounds() {
        let mut cm = ShadowCubemap::new(4);
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 10, 10), f32::MAX);
        assert!(!cm.test_and_set(CubeFace::PositiveX, 10, 10, 1.0));
    }

    #[test]
    fn shadow_cubemap_clear() {
        let mut cm = ShadowCubemap::new(4);
        cm.test_and_set(CubeFace::PositiveZ, 0, 0, 1.0);
        cm.clear();
        assert_eq!(cm.get_depth(CubeFace::PositiveZ, 0, 0), f32::MAX);
    }

    #[test]
    fn shadow_cubemap_sample_axis_aligned() {
        let mut cm = ShadowCubemap::new(8);
        // Write a depth to the center of the +X face.
        cm.test_and_set(CubeFace::PositiveX, 4, 4, 7.5);
        // Sampling in the +X direction should hit that face.
        let depth = cm.sample(Vector3::new(1.0, 0.0, 0.0));
        assert!(
            depth < f32::MAX,
            "expected written depth on +X face, got MAX"
        );
    }

    #[test]
    fn shadow_cubemap_sample_negative_z() {
        let mut cm = ShadowCubemap::new(8);
        // Fill the entire -Z face with depth 3.0.
        for y in 0..8 {
            for x in 0..8 {
                cm.test_and_set(CubeFace::NegativeZ, x, y, 3.0);
            }
        }
        let depth = cm.sample(Vector3::new(0.0, 0.0, -1.0));
        assert!((depth - 3.0).abs() < 1e-5);
    }

    #[test]
    fn shadow_cubemap_faces_are_independent() {
        let mut cm = ShadowCubemap::new(4);
        cm.test_and_set(CubeFace::PositiveX, 0, 0, 2.0);
        // Other faces should remain at MAX.
        assert_eq!(cm.get_depth(CubeFace::NegativeX, 0, 0), f32::MAX);
        assert_eq!(cm.get_depth(CubeFace::PositiveY, 0, 0), f32::MAX);
    }

    #[test]
    fn shadow_cubemap_zero_direction_does_not_panic() {
        let cm = ShadowCubemap::new(4);
        // Zero vector should not panic, just return some depth.
        let _depth = cm.sample(Vector3::ZERO);
    }
}
