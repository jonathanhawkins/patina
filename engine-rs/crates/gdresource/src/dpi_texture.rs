//! `DPITexture` — an SVG-backed Texture2D that re-rasterizes per scale
//! (pat-0q2kx.260).
//!
//! Godot ground truth: `scene/resources/dpi_texture.{h,cpp}`,
//! `doc/classes/DPITexture.xml`, `scene/register_scene_types.cpp:1075`
//! (`GDREGISTER_CLASS` — concrete). Experimental in 4.6.
//!
//! The point of the class is that a theme icon stays crisp when the viewport
//! scale rises: instead of bilinear-upscaling a fixed bitmap, the SVG is
//! rasterized again at the higher density. Patina already had the rasterizer
//! ([`crate::image_svg::rasterize_svg_bytes`], resvg-backed with real
//! anti-aliasing) but nothing exposed it as a DPI-reactive resource.
//!
//! Semantics read from the vendored `dpi_texture.cpp`, not inferred:
//!   * `_get_scaled_rid` rasterizes at `p_scale * base_scale` — the requested
//!     scale is MULTIPLIED by `base_scale`, not substituted for it.
//!   * Rasters are cached per scale (upstream keys `texture_cache` by double).
//!   * The reported size is `raster_dimension / scale`, i.e. density-
//!     independent, with a non-zero `size_override` axis winning.
//!   * `saturation != 1.0` routes through `Image::adjust_bcs(1, 1, saturation)`,
//!     whose grey point is the UNWEIGHTED mean `(r+g+b)/3` — not a Rec.601/709
//!     luminance. See [`apply_saturation`].
//!
//! ⚠️ NOT DELIVERED: the automatic re-rasterization hook. Upstream re-renders
//! when the owning viewport's scale / font oversampling changes; patina has no
//! such runtime — `oversampling` exists only as two ClassDB property entries
//! with nothing driving them. So the scale here is caller-supplied. Wiring it
//! to a signal that does not exist would be inert code that looks finished.

use std::collections::HashMap;

use gdcore::error::EngineResult;
use gdcore::math::Color;

use crate::image_svg::rasterize_svg_bytes;

/// One rasterization of the source at a particular effective scale.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterizedSvg {
    /// Pixel width of the raster.
    pub width: u32,
    /// Pixel height of the raster.
    pub height: u32,
    /// Row-major RGBA with straight (non-premultiplied) alpha.
    pub pixels: Vec<Color>,
}

/// An SVG source that re-rasterizes to whatever density is asked for.
///
/// ⚠️ `Default` is written by hand, NOT derived. A derived `Default` would give
/// `base_scale: 0.0` and `saturation: 0.0` — a texture that rasterizes nothing
/// and renders every icon grey. Godot's documented defaults are 1.0 for both.
#[derive(Debug, Clone)]
pub struct DpiTexture {
    source: String,
    base_scale: f32,
    saturation: f32,
    /// Exact-match source→target colour remap.
    color_map: Vec<(Color, Color)>,
    /// Per-axis override of the REPORTED size; zero means "no override on this
    /// axis", matching upstream's `size_override.x != 0` test.
    size_override: (f32, f32),
    /// Rasters keyed by the effective scale's bit pattern — f32 is not `Eq`,
    /// and the key only ever has to match a previous request exactly.
    cache: HashMap<u32, RasterizedSvg>,
}

impl Default for DpiTexture {
    fn default() -> Self {
        Self::new()
    }
}

impl DpiTexture {
    /// An empty texture with Godot's documented defaults.
    pub fn new() -> Self {
        Self {
            source: String::new(),
            base_scale: 1.0,
            saturation: 1.0,
            color_map: Vec::new(),
            size_override: (0.0, 0.0),
            cache: HashMap::new(),
        }
    }

    /// Mirrors `DPITexture.create_from_string(source, scale, saturation, color_map)`.
    pub fn create_from_string(
        source: &str,
        scale: f32,
        saturation: f32,
        color_map: &[(Color, Color)],
    ) -> Self {
        let mut t = Self::new();
        t.set_base_scale(scale);
        t.set_saturation(saturation);
        t.color_map = color_map.to_vec();
        t.set_source(source);
        t
    }

    /// The SVG source text.
    pub fn get_source(&self) -> &str {
        &self.source
    }

    /// Replaces the SVG source. Drops every cached raster — they describe the
    /// old artwork, and serving one after a source change is the bug this
    /// class exists to avoid.
    pub fn set_source(&mut self, source: &str) {
        if self.source != source {
            self.source = source.to_string();
            self.cache.clear();
        }
    }

    /// The intrinsic scale multiplier applied on top of every request.
    pub fn base_scale(&self) -> f32 {
        self.base_scale
    }

    /// Sets `base_scale`, invalidating the cache when it actually changes.
    pub fn set_base_scale(&mut self, scale: f32) {
        if self.base_scale != scale {
            self.base_scale = scale;
            self.cache.clear();
        }
    }

    /// Saturation multiplier; 1.0 leaves colour untouched.
    pub fn saturation(&self) -> f32 {
        self.saturation
    }

    /// Sets `saturation`, invalidating the cache when it actually changes.
    pub fn set_saturation(&mut self, saturation: f32) {
        if self.saturation != saturation {
            self.saturation = saturation;
            self.cache.clear();
        }
    }

    /// Overrides the REPORTED size. A zero axis means "no override", matching
    /// upstream. This does not change the raster: the whole point is that the
    /// pixels stay at native density while the logical size is what layout
    /// sees.
    pub fn set_size_override(&mut self, size: (f32, f32)) {
        self.size_override = size;
    }

    /// The logical size at `scale`: `raster / scale` per axis, with a non-zero
    /// override winning. `None` when the source does not rasterize.
    pub fn size(&mut self, scale: f32) -> Option<(f32, f32)> {
        // Scoped so the raster borrow ends before `size_override` is read —
        // `rasterize` takes `&mut self`, so holding its result across the
        // field reads below would not borrow-check.
        let (w, h) = {
            let raster = self.rasterize(scale).ok()?;
            (raster.width as f32, raster.height as f32)
        };
        // Divide by the REQUESTED scale, not the effective one: base_scale is
        // part of the artwork's intrinsic size, so it must survive into the
        // logical size rather than cancelling out.
        let denom = scale.max(f32::MIN_POSITIVE);
        let mut out = (w / denom, h / denom);
        if self.size_override.0 != 0.0 {
            out.0 = self.size_override.0;
        }
        if self.size_override.1 != 0.0 {
            out.1 = self.size_override.1;
        }
        Some(out)
    }

    /// The effective rasterization scale for a request: `scale * base_scale`.
    pub fn request_scale(&self, scale: f32) -> f32 {
        scale * self.base_scale
    }

    /// Rasterizes at `scale * base_scale`, reusing a cached raster when one
    /// exists for exactly that effective scale.
    pub fn rasterize(&mut self, scale: f32) -> EngineResult<&RasterizedSvg> {
        let effective = self.request_scale(scale);
        let key = effective.to_bits();
        if !self.cache.contains_key(&key) {
            let (width, height, mut pixels) =
                rasterize_svg_bytes(self.source.as_bytes(), effective)?;
            if !self.color_map.is_empty() {
                remap_colors(&mut pixels, &self.color_map);
            }
            if self.saturation != 1.0 {
                apply_saturation(&mut pixels, self.saturation);
            }
            self.cache.insert(
                key,
                RasterizedSvg {
                    width,
                    height,
                    pixels,
                },
            );
        }
        // The insert above guarantees presence; `expect` documents that rather
        // than hiding it behind an unwrap.
        Ok(self.cache.get(&key).expect("just inserted"))
    }

    /// How many distinct scales are currently cached. Exposed so a test can
    /// prove the cache is actually reused rather than silently re-rasterizing.
    pub fn cached_scale_count(&self) -> usize {
        self.cache.len()
    }
}

/// Godot's `Image::adjust_bcs(1, 1, saturation)` restricted to the saturation
/// term.
///
/// ⚠️ The grey point is the UNWEIGHTED mean `(r + g + b) / 3`, read from
/// `core/io/image.cpp`. A Rec.601/709 luminance is the natural guess and gives
/// visibly different greys for saturated colours.
///
/// Results are clamped to `0..=1`: upstream writes back through an RGBA8 buffer,
/// which clamps, so a `saturation > 1` cannot push a channel out of range there.
fn apply_saturation(pixels: &mut [Color], saturation: f32) {
    for c in pixels.iter_mut() {
        let center = (c.r + c.g + c.b) / 3.0;
        c.r = (center + saturation * (c.r - center)).clamp(0.0, 1.0);
        c.g = (center + saturation * (c.g - center)).clamp(0.0, 1.0);
        c.b = (center + saturation * (c.b - center)).clamp(0.0, 1.0);
        // Alpha is untouched — adjust_bcs only rewrites RGB.
    }
}

/// Applies the colour map by exact RGB match.
///
/// ⚠️ DEVIATION, stated rather than hidden: upstream hands `color_map` to
/// `ImageLoaderSVG` so the remap happens DURING rasterization, which also
/// recolours anti-aliased edge pixels. Doing it afterwards can only catch
/// pixels that came out exactly on a mapped colour, so edges keep the original
/// hue. Matching upstream needs the remap threaded into the resvg call.
fn remap_colors(pixels: &mut [Color], map: &[(Color, Color)]) {
    for c in pixels.iter_mut() {
        for (from, to) in map {
            if c.r == from.r && c.g == from.g && c.b == from.b {
                c.r = to.r;
                c.g = to.g;
                c.b = to.b;
                break;
            }
        }
    }
}
