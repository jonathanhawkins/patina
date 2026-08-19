//! 3D material types for the rendering server.
//!
//! Provides [`Material3D`] for basic PBR surfaces and [`StandardMaterial3D`]
//! for Godot-compatible materials with albedo, metallic, roughness, and
//! normal-map texture slots.

use gdcore::math::Color;
use gdvariant::Variant;
use std::sync::Arc;

/// An in-memory texture image sampled by the software and GPU render paths.
///
/// Pixels are stored row-major, length `width * height`, indexed `y * width + x`.
/// Color textures (albedo, emission) hold **sRGB-authored** texels; data
/// textures (normal, roughness, metallic / ORM) hold **raw linear** values.
/// Callers decode as appropriate (see the fragment-shader material sampling).
///
/// Wrapped in [`Arc`] on materials so per-draw material clones stay cheap.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureImage {
    /// Width in texels (must be >= 1).
    pub width: u32,
    /// Height in texels (must be >= 1).
    pub height: u32,
    /// Row-major pixels, length `width * height`.
    pub pixels: Vec<Color>,
}

impl TextureImage {
    /// Creates a texture from raw pixels. `pixels.len()` must equal
    /// `width * height`; otherwise sampling clamps out-of-range reads to black.
    pub fn new(width: u32, height: u32, pixels: Vec<Color>) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            pixels,
        }
    }

    /// Creates a solid 1x1 texture of the given color (handy for tests).
    pub fn solid(color: Color) -> Self {
        Self {
            width: 1,
            height: 1,
            pixels: vec![color],
        }
    }

    #[inline]
    fn texel(&self, x: u32, y: u32) -> Color {
        let idx = (y * self.width + x) as usize;
        self.pixels.get(idx).copied().unwrap_or(Color::BLACK)
    }

    /// Nearest-neighbour sample with **repeat** (tiling) wrap. `u`/`v` follow
    /// Godot/GL UV convention (origin top-left). Texel centres sit at
    /// `(i + 0.5) / size`.
    pub fn sample_nearest(&self, u: f32, v: f32) -> Color {
        let (w, h) = (self.width, self.height);
        let uf = u * w as f32 - 0.5;
        let vf = v * h as f32 - 0.5;
        let xi = wrap_index(uf.round() as i64, w);
        let yi = wrap_index(vf.round() as i64, h);
        self.texel(xi, yi)
    }

    /// Bilinear sample with **repeat** (tiling) wrap — matches Godot's default
    /// linear texture filter. Sampling exactly at a texel centre
    /// (`(i + 0.5) / size`) returns that texel unmodified.
    pub fn sample_bilinear(&self, u: f32, v: f32) -> Color {
        let (w, h) = (self.width, self.height);
        let uf = u * w as f32 - 0.5;
        let vf = v * h as f32 - 0.5;
        let x0f = uf.floor();
        let y0f = vf.floor();
        let tx = uf - x0f;
        let ty = vf - y0f;
        let x0 = wrap_index(x0f as i64, w);
        let x1 = wrap_index(x0f as i64 + 1, w);
        let y0 = wrap_index(y0f as i64, h);
        let y1 = wrap_index(y0f as i64 + 1, h);
        let c00 = self.texel(x0, y0);
        let c10 = self.texel(x1, y0);
        let c01 = self.texel(x0, y1);
        let c11 = self.texel(x1, y1);
        let top = lerp_color(c00, c10, tx);
        let bot = lerp_color(c01, c11, tx);
        lerp_color(top, bot, ty)
    }
}

/// Wraps a signed texel coordinate into `[0, size)` with repeat semantics.
#[inline]
fn wrap_index(i: i64, size: u32) -> u32 {
    let s = size as i64;
    (((i % s) + s) % s) as u32
}

#[inline]
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Shading mode for a 3D material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadingMode {
    /// Unlit — no lighting calculations.
    Unlit,
    /// Lambert diffuse shading.
    Lambert,
    /// Phong specular shading.
    Phong,
}

impl Default for ShadingMode {
    fn default() -> Self {
        Self::Lambert
    }
}

/// Godot `BaseMaterial3D.Transparency` (pat-s3rrv.30).
///
/// These three enums live here, beside `StandardMaterial3D`, rather than in
/// `gdrender3d::render_modes` where the logic was first written: `gdrender3d`
/// depends on `gdserver3d` and not the other way round, so a material could not
/// name a type defined over there. `render_modes` re-exports them, so there is
/// still exactly one definition and both renderers agree on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transparency {
    /// Fully opaque (Godot `TRANSPARENCY_DISABLED`, the default).
    #[default]
    Disabled,
    /// Alpha-blended; needs back-to-front sorting (`TRANSPARENCY_ALPHA`).
    Alpha,
    /// Alpha-tested: sub-threshold fragments are discarded, the rest opaque
    /// (`TRANSPARENCY_ALPHA_SCISSOR`).
    AlphaScissor,
    /// Stochastic alpha test (`TRANSPARENCY_ALPHA_HASH` = 3).
    ///
    /// ⚠️ A TEST, not blending: like [`AlphaScissor`](Self::AlphaScissor) it
    /// keeps or discards whole fragments and writes depth, so it belongs in the
    /// opaque pass and must NOT be sorted back-to-front. The threshold is a hash
    /// of the world position rather than a constant, which is what makes it look
    /// like blending from a distance while staying order-independent.
    AlphaHash,
}

impl Transparency {
    /// Maps Godot's integer enum, as a `.tscn` stores it.
    ///
    /// Unknown values fall back to `Disabled` rather than panicking: a scene
    /// authored in a newer Godot may carry a mode Patina has no rendering for,
    /// and drawing it opaque is a better failure than refusing to load.
    ///
    /// ⚠️ **3 and 4 were previously part of that fallback and should not have
    /// been** (pat-s3rrv.30). Both are documented Godot 4.6 modes and both are
    /// TRANSPARENT, so coercing them to `Disabled` rendered a hashed-alpha or
    /// depth-pre-pass material as a solid quad — the loudest possible wrong
    /// answer, and silent. The catch-all is right for genuinely unknown values;
    /// it was wrong for these two.
    ///
    /// ⚠️ `TRANSPARENCY_ALPHA_DEPTH_PRE_PASS` (4) maps to [`Alpha`](Self::Alpha).
    /// Godot draws it as ordinary alpha blending preceded by a depth-only pass,
    /// which reduces overdraw and stabilises sorting; Patina has no pre-pass, so
    /// this is blending WITHOUT that refinement. That is a close approximation
    /// of the same image, where `Disabled` was a different one.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            1 => Self::Alpha,
            2 => Self::AlphaScissor,
            3 => Self::AlphaHash,
            4 => Self::Alpha,
            _ => Self::Disabled,
        }
    }
}

/// Godot `BaseMaterial3D.CullMode` (pat-s3rrv.30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CullMode {
    /// Cull back faces — Godot's default (`CULL_BACK` = 0).
    #[default]
    Back,
    /// Cull front faces (`CULL_FRONT` = 1).
    Front,
    /// Draw both faces (`CULL_DISABLED` = 2).
    Disabled,
}

impl CullMode {
    /// Maps Godot's integer enum. Note the ordering is Godot's, NOT
    /// "disabled/back/front": 0 is Back, 1 is Front, 2 is Disabled.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            1 => Self::Front,
            2 => Self::Disabled,
            _ => Self::Back,
        }
    }
}

/// Godot `BaseMaterial3D.BlendMode` (pat-s3rrv.30).
///
/// The blend equation each mode selects is transcribed from
/// `upstream/godot/drivers/gles3/rasterizer_scene_gles3.cpp`, which is the only
/// place the factors are actually written down — the enum name alone does not
/// say, for instance, that `Sub` REVERSE-subtracts (dst − src) rather than
/// subtracting the destination, or that `Mul` and `Mix` leave the destination
/// alpha untouched against an opaque background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    /// Source-over (`BLEND_MODE_MIX` = 0, the default).
    #[default]
    Mix,
    /// Additive (`BLEND_MODE_ADD` = 1).
    Add,
    /// Reverse subtract (`BLEND_MODE_SUB` = 2).
    Sub,
    /// Modulate (`BLEND_MODE_MUL` = 3).
    Mul,
    /// Source already carries premultiplied alpha (`BLEND_MODE_PREMULT_ALPHA` = 4).
    PremultAlpha,
}

impl BlendMode {
    /// Maps Godot's integer enum (material.h:228-232).
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            1 => Self::Add,
            2 => Self::Sub,
            3 => Self::Mul,
            4 => Self::PremultAlpha,
            _ => Self::Mix,
        }
    }
}

/// Godot `BaseMaterial3D.DepthDrawMode` (pat-s3rrv.30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DepthDrawMode {
    /// Write depth only for opaque draws (`DEPTH_DRAW_OPAQUE_ONLY`, default).
    #[default]
    OpaqueOnly,
    /// Always write depth (`DEPTH_DRAW_ALWAYS`).
    Always,
    /// Never write depth (`DEPTH_DRAW_DISABLED`).
    Never,
}

impl DepthDrawMode {
    /// Maps Godot's integer enum.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            1 => Self::Always,
            2 => Self::Never,
            _ => Self::OpaqueOnly,
        }
    }
}

/// A 3D surface material.
#[derive(Debug, Clone, PartialEq)]
pub struct Material3D {
    /// Base albedo color.
    pub albedo: Color,
    /// Roughness factor (0.0 = mirror, 1.0 = fully rough).
    pub roughness: f32,
    /// Metallic factor (0.0 = dielectric, 1.0 = metallic).
    pub metallic: f32,
    /// Emission color (additive).
    pub emission: Color,
    /// Shading model.
    pub shading_mode: ShadingMode,
    /// Whether the material is double-sided.
    pub double_sided: bool,
    /// Godot `transparency` (pat-s3rrv.30). Carried through from
    /// `StandardMaterial3D` so the rasterizers can act on it — this is the
    /// render-facing material, and a mode that stops here changes no pixels.
    pub transparency: Transparency,
    /// Godot `alpha_scissor_threshold` (default 0.5).
    pub alpha_scissor_threshold: f32,
    /// Godot `cull_mode`.
    pub cull_mode: CullMode,
    /// Godot `depth_draw_mode`.
    pub depth_draw_mode: DepthDrawMode,
    /// Godot `blend_mode` (pat-s3rrv.30).
    pub blend_mode: BlendMode,
    /// Godot `no_depth_test` (pat-s3rrv.30).
    ///
    /// When set, the fragment's depth is never COMPARED — the surface draws
    /// over whatever is already there regardless of distance. Depth WRITING is
    /// a separate control (`depth_draw_mode`), which is why this is its own
    /// flag rather than a fifth draw mode.
    ///
    /// This is how HUD-ish 3D elements and x-ray highlights are authored.
    pub no_depth_test: bool,
    /// Optional albedo texture (sRGB); its decoded texel multiplies [`albedo`]
    /// per-fragment at the sampled UV. `None` = flat albedo color.
    ///
    /// [`albedo`]: Material3D::albedo
    pub albedo_texture: Option<Arc<TextureImage>>,
    /// Optional tangent-space normal map. Perturbs the interpolated normal
    /// before lighting when present.
    pub normal_texture: Option<Arc<TextureImage>>,
    /// Normal-map strength multiplier (1.0 = full strength).
    pub normal_scale: f32,
    /// Optional roughness texture; its **green** channel multiplies
    /// [`roughness`] per-fragment (Godot's default roughness channel).
    ///
    /// [`roughness`]: Material3D::roughness
    pub roughness_texture: Option<Arc<TextureImage>>,
    /// Optional metallic texture; its **blue** channel multiplies [`metallic`]
    /// per-fragment (Godot's default metallic channel).
    ///
    /// [`metallic`]: Material3D::metallic
    pub metallic_texture: Option<Arc<TextureImage>>,
    /// Optional emission texture (sRGB); its decoded texel multiplies
    /// [`emission`] per-fragment.
    ///
    /// [`emission`]: Material3D::emission
    pub emission_texture: Option<Arc<TextureImage>>,
}

impl Default for Material3D {
    fn default() -> Self {
        Self {
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            roughness: 0.5,
            metallic: 0.0,
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            shading_mode: ShadingMode::Lambert,
            double_sided: false,
            transparency: Transparency::Disabled,
            blend_mode: BlendMode::Mix,
            no_depth_test: false,
            alpha_scissor_threshold: 0.5,
            cull_mode: CullMode::Back,
            depth_draw_mode: DepthDrawMode::OpaqueOnly,
            albedo_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            roughness_texture: None,
            metallic_texture: None,
            emission_texture: None,
        }
    }
}

/// A texture channel reference — stores the resource path to a texture.
///
/// In Godot, texture properties are `Texture2D` sub-resources or external
/// resource references. Here we store the path string which can be resolved
/// through the resource cache at render time.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSlot {
    /// Resource path (e.g. `"res://textures/albedo.png"`).
    pub path: String,
}

impl TextureSlot {
    /// Creates a new texture slot with the given path.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// A Godot-compatible `StandardMaterial3D` with PBR texture slots.
///
/// Maps to Godot's `StandardMaterial3D` resource type with support for
/// albedo, metallic, roughness, and normal-map channels. Each channel has
/// a scalar/color value and an optional texture slot.
#[derive(Debug, Clone, PartialEq)]
pub struct StandardMaterial3D {
    /// Base albedo color.
    pub albedo_color: Color,
    /// Optional albedo texture.
    pub albedo_texture: Option<TextureSlot>,
    /// Metallic factor (0.0 = dielectric, 1.0 = metallic).
    pub metallic: f32,
    /// Optional metallic texture (sampled from the blue channel in Godot).
    pub metallic_texture: Option<TextureSlot>,
    /// Roughness factor (0.0 = mirror, 1.0 = fully rough).
    pub roughness: f32,
    /// Optional roughness texture (sampled from the green channel in Godot).
    pub roughness_texture: Option<TextureSlot>,
    /// Whether the normal map is enabled.
    pub normal_enabled: bool,
    /// Optional normal-map texture.
    pub normal_texture: Option<TextureSlot>,
    /// Normal map strength multiplier.
    pub normal_scale: f32,
    /// Emission color (additive).
    pub emission: Color,
    /// Shading model.
    pub shading_mode: ShadingMode,
    /// Whether the material is double-sided.
    ///
    /// Retained for existing callers; [`cull_mode`](Self::cull_mode) is the
    /// fuller expression of the same idea. `double_sided` can say "cull
    /// nothing" but cannot say "cull the FRONT faces", which is why an authored
    /// `cull_mode = 1` had nowhere to land before pat-s3rrv.30.
    pub double_sided: bool,
    /// Godot `transparency` (pat-s3rrv.30). Never parsed from `.tscn` before,
    /// so an authored alpha material rendered fully opaque in both backends.
    pub transparency: Transparency,
    /// Godot `alpha_scissor_threshold`; meaningful only with
    /// [`Transparency::AlphaScissor`]. Godot's default is 0.5.
    pub alpha_scissor_threshold: f32,
    /// Godot `cull_mode`.
    pub cull_mode: CullMode,
    /// Godot `depth_draw_mode`.
    pub depth_draw_mode: DepthDrawMode,
    /// Godot `blend_mode` (pat-s3rrv.30). Like `transparency` before it, this
    /// key was never read from `.tscn`, so an additive or multiply material —
    /// how essentially every glow, flame and shadow-decal effect is authored —
    /// composited as a plain source-over blend.
    pub blend_mode: BlendMode,
    /// Godot `no_depth_test` — a FLAG in Godot (`set_flag`/`get_flag`), not a
    /// mode, so its `.tscn` key is a plain bool (pat-s3rrv.30).
    pub no_depth_test: bool,
}

impl Default for StandardMaterial3D {
    fn default() -> Self {
        Self {
            albedo_color: Color::new(1.0, 1.0, 1.0, 1.0),
            albedo_texture: None,
            metallic: 0.0,
            metallic_texture: None,
            roughness: 1.0,
            roughness_texture: None,
            normal_enabled: false,
            normal_texture: None,
            normal_scale: 1.0,
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            shading_mode: ShadingMode::Lambert,
            double_sided: false,
            transparency: Transparency::Disabled,
            blend_mode: BlendMode::Mix,
            no_depth_test: false,
            // Godot's own default; a scene that enables AlphaScissor without
            // naming a threshold expects 0.5, not 0.0 (which would discard
            // nothing and look like the feature is inert).
            alpha_scissor_threshold: 0.5,
            cull_mode: CullMode::Back,
            depth_draw_mode: DepthDrawMode::OpaqueOnly,
        }
    }
}

impl StandardMaterial3D {
    /// Converts this standard material to a basic [`Material3D`] by
    /// discarding texture references.
    pub fn to_material3d(&self) -> Material3D {
        Material3D {
            albedo: self.albedo_color,
            roughness: self.roughness,
            metallic: self.metallic,
            emission: self.emission,
            shading_mode: self.shading_mode,
            double_sided: self.double_sided,
            // pat-s3rrv.30: mapped EXPLICITLY. The `..default()` below would
            // otherwise silently reset each of these to its default, so an
            // authored `transparency = Alpha` would reach this conversion and
            // die here — the hardest kind of gap to spot, because both the
            // parser and the renderer would look correct in isolation.
            transparency: self.transparency,
            alpha_scissor_threshold: self.alpha_scissor_threshold,
            cull_mode: self.cull_mode,
            depth_draw_mode: self.depth_draw_mode,
            blend_mode: self.blend_mode,
            no_depth_test: self.no_depth_test,
            ..Material3D::default()
        }
    }

    /// Constructs a `StandardMaterial3D` from a [`gdresource::Resource`]
    /// property bag.
    ///
    /// Reads Godot-standard property names (`albedo_color`, `metallic`,
    /// `roughness`, `normal_enabled`, `normal_scale`, etc.) and maps
    /// texture references (stored as `"ExtResource:<id>"` or
    /// `"SubResource:<id>"` strings) into [`TextureSlot`] values.
    pub fn from_properties<'a>(
        properties: impl Iterator<Item = (&'a String, &'a Variant)>,
    ) -> Self {
        let mut mat = Self::default();
        for (key, value) in properties {
            match key.as_str() {
                "albedo_color" => {
                    if let Variant::Color(c) = value {
                        mat.albedo_color = *c;
                    }
                }
                "albedo_texture" => {
                    if let Variant::String(s) = value {
                        mat.albedo_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "metallic" => {
                    if let Variant::Float(f) = value {
                        mat.metallic = *f as f32;
                    }
                }
                "metallic_texture" => {
                    if let Variant::String(s) = value {
                        mat.metallic_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "roughness" => {
                    if let Variant::Float(f) = value {
                        mat.roughness = *f as f32;
                    }
                }
                "roughness_texture" => {
                    if let Variant::String(s) = value {
                        mat.roughness_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "normal_enabled" => {
                    if let Variant::Bool(b) = value {
                        mat.normal_enabled = *b;
                    }
                }
                "normal_texture" => {
                    if let Variant::String(s) = value {
                        mat.normal_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "normal_scale" => {
                    if let Variant::Float(f) = value {
                        mat.normal_scale = *f as f32;
                    }
                }
                // pat-s3rrv.30: the render modes. This is the SECOND producer
                // of a `StandardMaterial3D` (the `.tres`/resource path) beside
                // `standard_material_from_props` for `.tscn` sub-resources —
                // wiring only one would leave the modes working for scenes and
                // silently inert for standalone material resources.
                "transparency" => {
                    if let Variant::Int(v) = value {
                        mat.transparency = Transparency::from_godot_int(*v);
                    }
                }
                "alpha_scissor_threshold" => match value {
                    Variant::Float(f) => mat.alpha_scissor_threshold = *f as f32,
                    Variant::Int(i) => mat.alpha_scissor_threshold = *i as f32,
                    _ => {}
                },
                "cull_mode" => {
                    if let Variant::Int(v) = value {
                        mat.cull_mode = CullMode::from_godot_int(*v);
                        // Keep the legacy flag in step: existing consumers read
                        // `double_sided`, and Godot's CULL_DISABLED is exactly
                        // what it means.
                        mat.double_sided = mat.cull_mode == CullMode::Disabled;
                    }
                }
                "depth_draw_mode" => {
                    if let Variant::Int(v) = value {
                        mat.depth_draw_mode = DepthDrawMode::from_godot_int(*v);
                    }
                }
                // 🔑 `blend_mode` was wired into `standard_material_from_props`
                // (the `.tscn` path) but NOT here — which is exactly the split
                // the comment above warns about. A `.tres` material authored
                // additive composited as plain source-over, while the same
                // material inline in a scene blended correctly.
                "blend_mode" => {
                    if let Variant::Int(v) = value {
                        mat.blend_mode = BlendMode::from_godot_int(*v);
                    }
                }
                // A FLAG in Godot (`set_flag`/`get_flag`), so the value is a
                // plain bool rather than an enum ordinal.
                "no_depth_test" => {
                    if let Variant::Bool(b) = value {
                        mat.no_depth_test = *b;
                    }
                }
                _ => {} // Ignore unknown properties.
            }
        }
        mat
    }

    /// Returns `true` if any texture slot is populated.
    pub fn has_textures(&self) -> bool {
        self.albedo_texture.is_some()
            || self.metallic_texture.is_some()
            || self.roughness_texture.is_some()
            || self.normal_texture.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🔑 Every documented Godot transparency mode maps to something that is
    /// actually transparent.
    ///
    /// ⚠️ Modes 3 and 4 previously fell into the unknown-value catch-all and
    /// came back `Disabled`, so a hashed-alpha or depth-pre-pass material
    /// rendered as a SOLID quad. The assertion that matters is that neither is
    /// `Disabled` — mapping them to *some* transparent mode is the fix; which
    /// one is the refinement.
    #[test]
    fn every_documented_transparency_mode_maps_to_a_transparent_result() {
        assert_eq!(Transparency::from_godot_int(0), Transparency::Disabled);
        assert_eq!(Transparency::from_godot_int(1), Transparency::Alpha);
        assert_eq!(Transparency::from_godot_int(2), Transparency::AlphaScissor);
        assert_eq!(
            Transparency::from_godot_int(3),
            Transparency::AlphaHash,
            "TRANSPARENCY_ALPHA_HASH must not collapse to Disabled — that draws \
             a stochastically-cut-out material as a solid quad",
        );
        assert_eq!(
            Transparency::from_godot_int(4),
            Transparency::Alpha,
            "TRANSPARENCY_ALPHA_DEPTH_PRE_PASS is alpha blending with a depth \
             pre-pass; Patina has no pre-pass, so plain Alpha is the close \
             approximation and Disabled was a different image entirely",
        );
        for mode in [3, 4] {
            assert_ne!(
                Transparency::from_godot_int(mode),
                Transparency::Disabled,
                "mode {mode} is a TRANSPARENT mode in Godot 4.6",
            );
        }
    }

    /// A genuinely unknown value still falls back to `Disabled`.
    ///
    /// ⚠️ The catch-all is correct for values Patina has never heard of — a
    /// scene from a newer Godot should load and draw, not refuse. Fixing 3 and 4
    /// must not turn that fallback into a panic or into a blanket "assume
    /// transparent".
    #[test]
    fn unknown_transparency_values_still_fall_back_to_disabled() {
        for mode in [5, 6, 99, -1] {
            assert_eq!(
                Transparency::from_godot_int(mode),
                Transparency::Disabled,
                "mode {mode} is not a Godot 4.6 transparency mode",
            );
        }
    }

    // ── TextureImage sampling ──

    fn checker_2x2() -> TextureImage {
        // (0,0)=red (1,0)=green / (0,1)=blue (1,1)=white, row-major.
        TextureImage::new(
            2,
            2,
            vec![
                Color::new(1.0, 0.0, 0.0, 1.0),
                Color::new(0.0, 1.0, 0.0, 1.0),
                Color::new(0.0, 0.0, 1.0, 1.0),
                Color::new(1.0, 1.0, 1.0, 1.0),
            ],
        )
    }

    #[test]
    fn bilinear_at_texel_centers_returns_exact_texel() {
        let t = checker_2x2();
        // Texel centres sit at (i + 0.5) / size = 0.25 and 0.75.
        assert_eq!(
            t.sample_bilinear(0.25, 0.25),
            Color::new(1.0, 0.0, 0.0, 1.0)
        );
        assert_eq!(
            t.sample_bilinear(0.75, 0.25),
            Color::new(0.0, 1.0, 0.0, 1.0)
        );
        assert_eq!(
            t.sample_bilinear(0.25, 0.75),
            Color::new(0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(
            t.sample_bilinear(0.75, 0.75),
            Color::new(1.0, 1.0, 1.0, 1.0)
        );
    }

    #[test]
    fn nearest_at_texel_centers_returns_exact_texel() {
        let t = checker_2x2();
        assert_eq!(t.sample_nearest(0.1, 0.1), Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(t.sample_nearest(0.9, 0.1), Color::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(t.sample_nearest(0.1, 0.9), Color::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(t.sample_nearest(0.9, 0.9), Color::new(1.0, 1.0, 1.0, 1.0));
    }

    #[test]
    fn bilinear_midpoint_blends_neighbours() {
        // Two horizontal texels: black then white. Midpoint u=0.5 (between the
        // two texel centres 0.25 and 0.75) blends to 50% grey.
        let t = TextureImage::new(2, 1, vec![Color::BLACK, Color::WHITE]);
        let mid = t.sample_bilinear(0.5, 0.5);
        assert!((mid.r - 0.5).abs() < 1e-5, "got {mid:?}");
        assert!((mid.g - 0.5).abs() < 1e-5);
        assert!((mid.b - 0.5).abs() < 1e-5);
    }

    #[test]
    fn nearest_repeat_wraps_out_of_range_uv() {
        let t = TextureImage::new(2, 1, vec![Color::BLACK, Color::WHITE]);
        // u=1.1 wraps to 0.1 → texel 0 (black); u=1.9 wraps to 0.9 → texel 1.
        assert_eq!(t.sample_nearest(1.1, 0.0), Color::BLACK);
        assert_eq!(t.sample_nearest(1.9, 0.0), Color::WHITE);
        // Negative UVs wrap too. ⚠️ Both directions are asserted because the
        // arithmetic is easy to get backwards: sampling uses texel CENTRES
        // (`u * w - 0.5`), so -0.1 lands one texel BEFORE index 0 and wraps to
        // the LAST texel, while -0.9 lands two before and wraps back to the
        // first. This case previously expected white for -0.9, which
        // contradicted both the implementation and the comment's own "wraps to
        // 0.1" reasoning — 0.1 of a 2-texel row is texel 0, and that is black.
        assert_eq!(t.sample_nearest(-0.1, 0.0), Color::WHITE);
        assert_eq!(t.sample_nearest(-0.9, 0.0), Color::BLACK);
    }

    #[test]
    fn solid_texture_samples_uniformly() {
        let c = Color::new(0.2, 0.4, 0.6, 1.0);
        let t = TextureImage::solid(c);
        assert_eq!(t.sample_bilinear(0.0, 0.0), c);
        assert_eq!(t.sample_bilinear(0.37, 0.91), c);
        assert_eq!(t.sample_nearest(2.5, -3.3), c);
    }

    #[test]
    fn out_of_bounds_pixel_index_is_black_not_panic() {
        // width*height claims 4 texels but only 1 provided; missing reads → black.
        let t = TextureImage::new(2, 2, vec![Color::WHITE]);
        assert_eq!(t.sample_nearest(0.9, 0.9), Color::BLACK);
    }

    #[test]
    fn material_texture_slots_default_to_none() {
        let mat = Material3D::default();
        assert!(mat.albedo_texture.is_none());
        assert!(mat.normal_texture.is_none());
        assert!(mat.roughness_texture.is_none());
        assert!(mat.metallic_texture.is_none());
        assert!(mat.emission_texture.is_none());
        assert!((mat.normal_scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn default_material() {
        let mat = Material3D::default();
        assert_eq!(mat.albedo, Color::new(1.0, 1.0, 1.0, 1.0));
        assert!((mat.roughness - 0.5).abs() < f32::EPSILON);
        assert!(mat.metallic.abs() < f32::EPSILON);
        assert!(!mat.double_sided);
        assert_eq!(mat.shading_mode, ShadingMode::Lambert);
    }

    #[test]
    fn custom_material() {
        let mat = Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            roughness: 0.8,
            metallic: 1.0,
            emission: Color::new(0.5, 0.5, 0.0, 1.0),
            shading_mode: ShadingMode::Phong,
            double_sided: true,
            ..Material3D::default()
        };
        assert_eq!(mat.shading_mode, ShadingMode::Phong);
        assert!(mat.double_sided);
        assert!((mat.metallic - 1.0).abs() < f32::EPSILON);
    }

    // ── TextureSlot ──

    #[test]
    fn texture_slot_creation() {
        let slot = TextureSlot::new("res://textures/albedo.png");
        assert_eq!(slot.path, "res://textures/albedo.png");
    }

    // ── StandardMaterial3D ──

    /// pat-s3rrv.30: the `.tres` / resource path parses the render modes too.
    ///
    /// 🔑 `blend_mode` was wired into `standard_material_from_props` (the
    /// `.tscn` path) and NOT into `from_properties`, so a material authored as
    /// a standalone `.tres` composited as plain source-over while the identical
    /// material inline in a scene blended correctly. The comment in
    /// `from_properties` warns about exactly this split; the warning was there
    /// and the arm was not.
    #[test]
    fn the_resource_path_parses_blend_mode_and_no_depth_test() {
        use std::collections::HashMap;

        let props: HashMap<String, Variant> = [
            ("blend_mode".to_string(), Variant::Int(1)),
            ("no_depth_test".to_string(), Variant::Bool(true)),
        ]
        .into_iter()
        .collect();
        let mat = StandardMaterial3D::from_properties(props.iter());

        assert_eq!(
            mat.blend_mode,
            BlendMode::Add,
            "`blend_mode = 1` in a .tres must parse to Add, as it already did \
             for a .tscn",
        );
        assert!(mat.no_depth_test, "`no_depth_test = true` must parse");

        // ⚠️ The conversion is where both would silently die: `to_material3d`
        // ends in `..Material3D::default()`, so anything not mapped explicitly
        // resets — and the parser and the renderer both still look correct.
        let render = mat.to_material3d();
        assert_eq!(render.blend_mode, BlendMode::Add);
        assert!(
            render.no_depth_test,
            "no_depth_test must survive to_material3d's ..default() tail",
        );
    }

    /// An unspecified material keeps Godot's defaults for both.
    #[test]
    fn the_render_modes_default_to_godots_values() {
        let mat = StandardMaterial3D::default();
        assert_eq!(mat.blend_mode, BlendMode::Mix);
        assert!(
            !mat.no_depth_test,
            "depth testing is ON by default; `no_depth_test` is opt-in",
        );
        assert!(!mat.to_material3d().no_depth_test);
    }

    #[test]
    fn standard_material_defaults() {
        let mat = StandardMaterial3D::default();
        assert_eq!(mat.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
        assert!(mat.albedo_texture.is_none());
        assert!(mat.metallic.abs() < f32::EPSILON);
        assert!(mat.metallic_texture.is_none());
        assert!((mat.roughness - 1.0).abs() < f32::EPSILON);
        assert!(mat.roughness_texture.is_none());
        assert!(!mat.normal_enabled);
        assert!(mat.normal_texture.is_none());
        assert!((mat.normal_scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(mat.shading_mode, ShadingMode::Lambert);
        assert!(!mat.double_sided);
        assert!(!mat.has_textures());
    }

    #[test]
    fn standard_material_with_all_textures() {
        let mat = StandardMaterial3D {
            albedo_color: Color::new(0.8, 0.2, 0.1, 1.0),
            albedo_texture: Some(TextureSlot::new("res://albedo.png")),
            metallic: 0.9,
            metallic_texture: Some(TextureSlot::new("res://metallic.png")),
            roughness: 0.3,
            roughness_texture: Some(TextureSlot::new("res://roughness.png")),
            normal_enabled: true,
            normal_texture: Some(TextureSlot::new("res://normal.png")),
            normal_scale: 1.5,
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            shading_mode: ShadingMode::Phong,
            double_sided: false,
            // The render-mode fields are irrelevant to this texture assertion
            // (pat-s3rrv.30); take the defaults rather than restating them.
            ..Default::default()
        };
        assert!(mat.has_textures());
        assert_eq!(
            mat.albedo_texture.as_ref().unwrap().path,
            "res://albedo.png"
        );
        assert_eq!(
            mat.metallic_texture.as_ref().unwrap().path,
            "res://metallic.png"
        );
        assert_eq!(
            mat.roughness_texture.as_ref().unwrap().path,
            "res://roughness.png"
        );
        assert_eq!(
            mat.normal_texture.as_ref().unwrap().path,
            "res://normal.png"
        );
        assert!(mat.normal_enabled);
        assert!((mat.normal_scale - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn standard_material_to_material3d() {
        let std_mat = StandardMaterial3D {
            albedo_color: Color::new(1.0, 0.0, 0.0, 1.0),
            metallic: 0.8,
            roughness: 0.2,
            emission: Color::new(0.1, 0.1, 0.0, 0.0),
            shading_mode: ShadingMode::Phong,
            double_sided: true,
            ..Default::default()
        };
        let basic = std_mat.to_material3d();
        assert_eq!(basic.albedo, Color::new(1.0, 0.0, 0.0, 1.0));
        assert!((basic.metallic - 0.8).abs() < f32::EPSILON);
        assert!((basic.roughness - 0.2).abs() < f32::EPSILON);
        assert_eq!(basic.emission, Color::new(0.1, 0.1, 0.0, 0.0));
        assert_eq!(basic.shading_mode, ShadingMode::Phong);
        assert!(basic.double_sided);
    }

    #[test]
    fn standard_material_from_properties_full() {
        let mut props = std::collections::HashMap::new();
        props.insert(
            "albedo_color".to_string(),
            Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)),
        );
        props.insert(
            "albedo_texture".to_string(),
            Variant::String("ExtResource:1".to_string()),
        );
        props.insert("metallic".to_string(), Variant::Float(0.7));
        props.insert(
            "metallic_texture".to_string(),
            Variant::String("ExtResource:2".to_string()),
        );
        props.insert("roughness".to_string(), Variant::Float(0.4));
        props.insert(
            "roughness_texture".to_string(),
            Variant::String("SubResource:3".to_string()),
        );
        props.insert("normal_enabled".to_string(), Variant::Bool(true));
        props.insert(
            "normal_texture".to_string(),
            Variant::String("ExtResource:4".to_string()),
        );
        props.insert("normal_scale".to_string(), Variant::Float(2.0));

        let mat = StandardMaterial3D::from_properties(props.iter());
        assert_eq!(mat.albedo_color, Color::new(0.5, 0.5, 0.5, 1.0));
        assert_eq!(mat.albedo_texture.as_ref().unwrap().path, "ExtResource:1");
        assert!((mat.metallic - 0.7).abs() < f32::EPSILON);
        assert_eq!(mat.metallic_texture.as_ref().unwrap().path, "ExtResource:2");
        assert!((mat.roughness - 0.4).abs() < f32::EPSILON);
        assert_eq!(
            mat.roughness_texture.as_ref().unwrap().path,
            "SubResource:3"
        );
        assert!(mat.normal_enabled);
        assert_eq!(mat.normal_texture.as_ref().unwrap().path, "ExtResource:4");
        assert!((mat.normal_scale - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn standard_material_from_properties_partial() {
        let mut props = std::collections::HashMap::new();
        props.insert("metallic".to_string(), Variant::Float(0.5));
        props.insert("normal_enabled".to_string(), Variant::Bool(true));

        let mat = StandardMaterial3D::from_properties(props.iter());
        // Explicitly set.
        assert!((mat.metallic - 0.5).abs() < f32::EPSILON);
        assert!(mat.normal_enabled);
        // Defaults for everything else.
        assert_eq!(mat.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
        assert!(mat.albedo_texture.is_none());
        assert!((mat.roughness - 1.0).abs() < f32::EPSILON);
        assert!((mat.normal_scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn standard_material_from_properties_ignores_unknown() {
        let mut props = std::collections::HashMap::new();
        props.insert("albedo_color".to_string(), Variant::Color(Color::WHITE));
        props.insert("unknown_field".to_string(), Variant::Int(42));
        props.insert("another_thing".to_string(), Variant::Bool(true));

        let mat = StandardMaterial3D::from_properties(props.iter());
        assert_eq!(mat.albedo_color, Color::WHITE);
        // Should not panic or change defaults for unknown keys.
        assert!(!mat.normal_enabled);
    }

    #[test]
    fn standard_material_from_properties_wrong_types_ignored() {
        let mut props = std::collections::HashMap::new();
        // Pass wrong types — should be silently ignored.
        props.insert("metallic".to_string(), Variant::String("bad".into()));
        props.insert("normal_enabled".to_string(), Variant::Int(1));
        props.insert("albedo_color".to_string(), Variant::Float(0.5));

        let mat = StandardMaterial3D::from_properties(props.iter());
        // All should remain at defaults since types didn't match.
        assert!(mat.metallic.abs() < f32::EPSILON);
        assert!(!mat.normal_enabled);
        assert_eq!(mat.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
    }

    #[test]
    fn standard_material_has_textures_single() {
        let mut mat = StandardMaterial3D::default();
        assert!(!mat.has_textures());
        mat.normal_texture = Some(TextureSlot::new("res://n.png"));
        assert!(mat.has_textures());
    }

    #[test]
    fn standard_material_clone_eq() {
        let mat = StandardMaterial3D {
            albedo_color: Color::new(1.0, 0.0, 0.0, 1.0),
            albedo_texture: Some(TextureSlot::new("res://a.png")),
            normal_enabled: true,
            normal_scale: 2.0,
            ..Default::default()
        };
        let cloned = mat.clone();
        assert_eq!(mat, cloned);
    }
}
