//! pat-0q2kx.260 — `DPITexture` re-rasterizes SVG per scale.
//!
//! Godot ground truth: `scene/resources/dpi_texture.cpp`,
//! `doc/classes/DPITexture.xml`, `scene/register_scene_types.cpp:1075`.
//!
//! The class exists so a theme icon stays crisp as viewport scale rises. The
//! interesting claim is therefore NOT "a bigger raster comes out" — a bilinear
//! upscale also produces a bigger raster. It is that the result is genuinely
//! re-rasterized. `dpi_texture_svg_rescale` asserts that directly by building
//! the bilinear upscale it must beat, rather than testing against a magic
//! sharpness threshold.

use gdcore::math::Color;
use gdresource::dpi_texture::DpiTexture;

/// A circle: curved edges give the rasterizer real anti-aliasing to resolve,
/// which is what distinguishes a re-raster from an upscale. A rectangle on
/// integer boundaries would be pixel-exact at both scales and prove nothing.
// NOTE the `r##"…"##` delimiters: an SVG fill is written `fill="#000000"`, and
// the `"#` in that would close an ordinary `r#"…"#` raw string mid-literal.
const CIRCLE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><circle cx="16" cy="16" r="12" fill="#000000"/></svg>"##;

const RED_SQUARE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8"><rect x="0" y="0" width="8" height="8" fill="#ff0000"/></svg>"##;

/// Pixels that are neither fully transparent nor fully opaque — the soft edge
/// band. For a true rasterization this tracks the PERIMETER, so it shrinks as a
/// fraction of area when density doubles. An upscale cannot improve it: the
/// blur is already baked into the source pixels and interpolation only spreads
/// it further.
fn soft_fraction(pixels: &[Color]) -> f64 {
    let soft = pixels.iter().filter(|c| c.a > 0.02 && c.a < 0.98).count();
    soft as f64 / pixels.len() as f64
}

/// Bilinear 2x upscale of an alpha channel — the thing DPITexture is supposed
/// to be better than.
fn bilinear_2x(src: &[Color], w: u32, h: u32) -> Vec<Color> {
    let (dw, dh) = (w * 2, h * 2);
    let mut out = Vec::with_capacity((dw * dh) as usize);
    let at = |x: i64, y: i64| -> Color {
        let x = x.clamp(0, w as i64 - 1) as usize;
        let y = y.clamp(0, h as i64 - 1) as usize;
        src[y * w as usize + x]
    };
    for y in 0..dh {
        for x in 0..dw {
            // Centre-aligned sample position in source space.
            let sx = (x as f32 + 0.5) / 2.0 - 0.5;
            let sy = (y as f32 + 0.5) / 2.0 - 0.5;
            let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let a = lerp(
                lerp(at(x0, y0).a, at(x0 + 1, y0).a, fx),
                lerp(at(x0, y0 + 1).a, at(x0 + 1, y0 + 1).a, fx),
                fy,
            );
            out.push(Color::new(0.0, 0.0, 0.0, a));
        }
    }
    out
}

/// THE ACCEPTANCE. The class is registered under Texture2D, and a 2x request
/// yields a 2x-resolution raster that is sharper than a bilinear upscale of the
/// 1x bitmap.
#[test]
fn dpi_texture_svg_rescale() {
    gdobject::class_db::register_viewport_classes();
    assert!(
        gdobject::class_db::class_exists("DPITexture"),
        "DPITexture must be registered"
    );
    assert!(
        gdobject::class_db::is_parent_class("DPITexture", "Texture2D"),
        "DPITexture derives Texture2D in Godot"
    );

    let mut tex = DpiTexture::new();
    tex.set_source(CIRCLE_SVG);

    let (w1, h1, px1) = {
        let r = tex.rasterize(1.0).expect("1x must rasterize");
        (r.width, r.height, r.pixels.clone())
    };
    let (w2, h2, px2) = {
        let r = tex.rasterize(2.0).expect("2x must rasterize");
        (r.width, r.height, r.pixels.clone())
    };

    // Dimensions track the requested scale.
    assert_eq!((w1, h1), (32, 32), "the SVG is intrinsically 32x32");
    assert_eq!(
        (w2, h2),
        (64, 64),
        "a 2x request must produce a 2x raster, got {w2}x{h2}"
    );
    assert_eq!(
        px2.len(),
        px1.len() * 4,
        "2x on each axis is 4x the pixel data"
    );

    // Sharpness, measured against the upscale it must beat rather than a magic
    // constant. Both images are 64x64 of the same circle.
    let upscaled = bilinear_2x(&px1, w1, h1);
    assert_eq!(upscaled.len(), px2.len());

    let sharp = soft_fraction(&px2);
    let blurry = soft_fraction(&upscaled);
    assert!(
        sharp < blurry,
        "the re-rasterized 2x image must have a smaller soft-edge fraction \
         than a bilinear upscale of the 1x bitmap: re-raster {sharp:.4} vs \
         upscale {blurry:.4}. Equal-or-worse means the texture is stretching a \
         cached bitmap instead of re-rendering the SVG."
    );

    // Guard the guard: if neither image had soft pixels the comparison above
    // would be 0 < 0 and could not fail for the right reason.
    assert!(
        blurry > 0.0,
        "the upscale must have an anti-aliased band, or the metric is inert"
    );
}

/// `Default` must agree with `new()`. A DERIVED Default would give
/// `base_scale: 0.0` (rasterizes nothing) and `saturation: 0.0` (every icon
/// grey) — both plausible-looking and both broken, and no other test here
/// would notice because they all construct via `new()`.
#[test]
fn default_matches_godots_documented_defaults() {
    let d = DpiTexture::default();
    assert_eq!(d.base_scale(), 1.0, "a zero base_scale rasterizes nothing");
    assert_eq!(d.saturation(), 1.0, "a zero saturation greys every icon");
    assert_eq!(d.get_source(), "");
}

/// `base_scale` MULTIPLIES the requested scale — upstream rasterizes at
/// `p_scale * base_scale`. Treating it as a substitute would make every
/// request render at the same density.
#[test]
fn base_scale_multiplies_the_requested_scale() {
    let mut tex = DpiTexture::new();
    tex.set_source(CIRCLE_SVG);
    tex.set_base_scale(2.0);

    assert_eq!(tex.request_scale(1.0), 2.0);
    assert_eq!(tex.request_scale(2.0), 4.0);

    let r = tex.rasterize(1.0).unwrap();
    assert_eq!(
        (r.width, r.height),
        (64, 64),
        "base_scale 2 with a 1x request must rasterize at 2x, not 1x"
    );
}

/// Rasters are cached per scale, and a repeat request reuses one.
#[test]
fn rasters_are_cached_per_scale() {
    let mut tex = DpiTexture::new();
    tex.set_source(CIRCLE_SVG);

    tex.rasterize(1.0).unwrap();
    assert_eq!(tex.cached_scale_count(), 1);

    tex.rasterize(1.0).unwrap();
    assert_eq!(
        tex.cached_scale_count(),
        1,
        "the same scale must reuse its cached raster"
    );

    tex.rasterize(2.0).unwrap();
    assert_eq!(
        tex.cached_scale_count(),
        2,
        "a different scale needs its own raster"
    );
}

/// Changing the source drops cached rasters. Serving a stale raster after a
/// source swap is exactly the staleness bug this class exists to avoid.
#[test]
fn changing_state_drops_cached_rasters() {
    let mut tex = DpiTexture::new();
    tex.set_source(CIRCLE_SVG);
    tex.rasterize(1.0).unwrap();
    assert_eq!(tex.cached_scale_count(), 1);

    tex.set_source(RED_SQUARE_SVG);
    assert_eq!(
        tex.cached_scale_count(),
        0,
        "a new source invalidates every cached raster"
    );

    // The new artwork's own intrinsic size is used, not the old one's.
    let r = tex.rasterize(1.0).unwrap();
    assert_eq!((r.width, r.height), (8, 8));

    // base_scale and saturation invalidate too — each changes the pixels.
    tex.set_base_scale(2.0);
    assert_eq!(tex.cached_scale_count(), 0);
    tex.rasterize(1.0).unwrap();
    tex.set_saturation(0.5);
    assert_eq!(tex.cached_scale_count(), 0);

    // Setting the SAME value must not thrash the cache.
    tex.rasterize(1.0).unwrap();
    tex.set_saturation(0.5);
    assert_eq!(
        tex.cached_scale_count(),
        1,
        "an idempotent set must not discard valid rasters"
    );
}

/// Saturation follows `Image::adjust_bcs`, whose grey point is the UNWEIGHTED
/// mean `(r+g+b)/3` — NOT a Rec.601/709 luminance, which is the natural wrong
/// guess and gives a different grey for a saturated colour.
#[test]
fn saturation_greys_toward_the_unweighted_mean() {
    let mut tex = DpiTexture::new();
    tex.set_source(RED_SQUARE_SVG);
    tex.set_saturation(0.0);

    let r = tex.rasterize(1.0).unwrap();
    // Middle of a solid 8x8 red square — well away from any edge pixel.
    let c = r.pixels[(4 * 8 + 4) as usize];

    let mean = 1.0 / 3.0;
    assert!(
        (c.r - mean).abs() < 1e-3 && (c.g - mean).abs() < 1e-3 && (c.b - mean).abs() < 1e-3,
        "pure red at saturation 0 must go to (r+g+b)/3 = 0.333 on every \
         channel, got {c:?}. Rec.601 luminance would give ~0.30 and Rec.709 \
         ~0.21 — both plausible and both wrong."
    );
    assert!(
        (c.a - 1.0).abs() < 1e-3,
        "adjust_bcs rewrites RGB only; alpha must survive"
    );
}

/// `set_size_override` changes the REPORTED size, not the raster. The pixels
/// staying at native density is the entire point — an override that resampled
/// them would reintroduce the blur.
#[test]
fn size_override_changes_reported_size_not_the_raster() {
    let mut tex = DpiTexture::new();
    tex.set_source(CIRCLE_SVG);

    assert_eq!(
        tex.size(1.0),
        Some((32.0, 32.0)),
        "logical size is raster/scale"
    );
    assert_eq!(
        tex.size(2.0),
        Some((32.0, 32.0)),
        "at 2x the raster is 64px but the LOGICAL size is unchanged — that is \
         what density-independence means"
    );

    // A zero axis means "no override", matching upstream's `!= 0` test.
    tex.set_size_override((100.0, 0.0));
    assert_eq!(
        tex.size(1.0),
        Some((100.0, 32.0)),
        "only the non-zero axis is overridden"
    );

    // The raster itself is untouched.
    let r = tex.rasterize(1.0).unwrap();
    assert_eq!(
        (r.width, r.height),
        (32, 32),
        "an override must not resample the pixels"
    );
}
