//! SVG rasterization → `Texture2D` (pat-nzgdu.44).
//!
//! Godot's `Image.load_svg_from_buffer(buffer, scale)` /
//! `load_svg_from_string(str, scale)` (doc/classes/Image.xml) rasterize an
//! uncompressed SVG at a scale factor into an RGBA8 image; the whole editor
//! renders its icons this way and projects use it for vector assets. Patina had
//! no SVG rasterizer anywhere — `.svg` was only ever classified as a `Texture2D`
//! by file extension, with no pixel data.
//!
//! This module adds the missing pipeline using the pure-Rust, wasm-friendly
//! `resvg` stack (which re-exports `usvg` and `tiny_skia`): it parses the SVG,
//! rasterizes it at `scale` — rounding the output to
//! `ceil(width*scale) x ceil(height*scale)` like Godot — and produces the
//! standard pixel-bearing `Texture2D` [`Resource`] on the same load path PNG and
//! WebP use ([`crate::importers::import_image`] /
//! [`crate::importers::import_webp`]). `.svg` is registered in
//! [`ResourceFormatLoader::with_defaults`](crate::importers::ResourceFormatLoader::with_defaults)
//! so a loose `.svg` file loads to a `Texture2D` with decoded pixels.

use std::path::Path;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::Color;
use gdvariant::Variant;

use crate::resource::Resource;

use resvg::{tiny_skia, usvg};

/// Rasterizes uncompressed SVG bytes at `scale`, returning
/// `(width, height, pixels)` where `pixels` is a row-major RGBA color buffer of
/// length `width * height` with **straight** (non-premultiplied) alpha, matching
/// the PNG/WebP importers.
///
/// The output dimensions follow Godot: `ceil(intrinsic * scale)` on each axis
/// (so `scale = 2.0` yields four times the pixel count of `scale = 1.0`). A
/// non-finite or non-positive `scale` is treated as `1.0`, and the dimensions
/// are clamped to a minimum of 1px so a degenerate SVG still allocates.
pub fn rasterize_svg_bytes(bytes: &[u8], scale: f32) -> EngineResult<(u32, u32, Vec<Color>)> {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };

    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(bytes, &options)
        .map_err(|e| EngineError::Parse(format!("SVG parse failed: {e}")))?;

    let size = tree.size();
    let width = ((size.width() * scale).ceil() as u32).max(1);
    let height = ((size.height() * scale).ceil() as u32).max(1);

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        EngineError::Parse(format!("SVG pixmap allocation failed ({width}x{height})"))
    })?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // `pixmap.data()` is tightly-packed, premultiplied RGBA8. Godot's Image
    // stores straight alpha, so un-premultiply each pixel.
    let colors: Vec<Color> = pixmap
        .data()
        .chunks_exact(4)
        .map(|px| {
            let a = px[3] as f32 / 255.0;
            if a > 0.0 {
                Color::new(
                    ((px[0] as f32 / 255.0) / a).min(1.0),
                    ((px[1] as f32 / 255.0) / a).min(1.0),
                    ((px[2] as f32 / 255.0) / a).min(1.0),
                    a,
                )
            } else {
                Color::new(0.0, 0.0, 0.0, 0.0)
            }
        })
        .collect();

    Ok((width, height, colors))
}

/// Builds a `Texture2D` [`Resource`] from SVG bytes rasterized at `scale`.
///
/// Mirrors the property contract of [`crate::importers::import_image`]:
/// `width`/`height`/`format`/`bit_depth` metadata plus a `pixels`
/// `Variant::Array` of `Color` and `has_pixel_data = true`.
pub fn import_svg_bytes(bytes: &[u8], path: &Path, scale: f32) -> EngineResult<Arc<Resource>> {
    let (width, height, colors) = rasterize_svg_bytes(bytes, scale)?;

    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(width as i64));
    res.set_property("height", Variant::Int(height as i64));
    res.set_property("format", Variant::String("RGBA".into()));
    res.set_property("bit_depth", Variant::Int(8));

    let pixels: Vec<Variant> = colors.into_iter().map(Variant::Color).collect();
    res.set_property("pixels", Variant::Array(pixels));
    res.set_property("has_pixel_data", Variant::Bool(true));

    Ok(Arc::new(res))
}

/// Loads a loose `.svg` file into a `Texture2D` [`Resource`] at scale `1.0`.
///
/// This is the [`LoaderFn`](crate::importers::LoaderFn) registered for `.svg` in
/// [`ResourceFormatLoader::with_defaults`](crate::importers::ResourceFormatLoader::with_defaults).
pub fn import_svg(path: &Path) -> EngineResult<Arc<Resource>> {
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    import_svg_bytes(&data, path, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::ResourceFormatLoader;

    /// 32×32 SVG: a red rect with a blue circle. Simple shapes only (no text),
    /// so no font database is needed.
    const TEST_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><rect x="4" y="4" width="24" height="24" fill="#ff0000"/><circle cx="16" cy="16" r="6" fill="#0000ff"/></svg>"##;

    fn int_prop(res: &Resource, key: &str) -> i64 {
        match res.get_property(key) {
            Some(Variant::Int(v)) => *v,
            other => panic!("expected Int for '{key}', got {other:?}"),
        }
    }

    fn pixel_count(res: &Resource) -> usize {
        match res.get_property("pixels") {
            Some(Variant::Array(a)) => a.len(),
            other => panic!("expected pixels Array, got {other:?}"),
        }
    }

    /// End-to-end acceptance (pat-nzgdu.44): a `.svg` file loaded through the
    /// default `ResourceFormatLoader` yields a `Texture2D` whose `pixels` are
    /// populated and whose `width`/`height` reflect the SVG viewBox × scale;
    /// rasterizing the same SVG at scale 2.0 yields 4× the pixel count of
    /// scale 1.0. Proves the wiring from an `.svg` file to decoded pixels on the
    /// same load path PNG uses — not an extension-only classification.
    #[test]
    fn svg_import_wiring() {
        // (1) `.svg` is registered in with_defaults() and loads to a Texture2D
        // with real pixels reachable by the same load path PNG uses.
        let dir = tempfile::tempdir().expect("tempdir");
        let svg_path = dir.path().join("icon.svg");
        std::fs::write(&svg_path, TEST_SVG).expect("write svg");

        let rfl = ResourceFormatLoader::with_defaults();
        assert!(rfl.can_load(".svg"), ".svg must be a registered loader");

        let res = rfl
            .load_resource(&svg_path)
            .expect("load .svg via registry");
        assert_eq!(res.class_name, "Texture2D", "svg imports as a Texture2D");
        assert_eq!(int_prop(&res, "width"), 32, "viewBox width at scale 1.0");
        assert_eq!(int_prop(&res, "height"), 32, "viewBox height at scale 1.0");

        let n1 = pixel_count(&res);
        assert_eq!(n1, 32 * 32, "pixels array is fully populated (w*h)");

        // The rasterization actually ran (not a blank/hardcoded buffer): the
        // blue circle sits at the SVG center, so the center pixel is blue-ish.
        let center = match res.get_property("pixels") {
            Some(Variant::Array(a)) => match &a[16 * 32 + 16] {
                Variant::Color(c) => *c,
                other => panic!("expected Color, got {other:?}"),
            },
            other => panic!("expected pixels Array, got {other:?}"),
        };
        assert!(
            center.b > 0.5 && center.a > 0.5 && center.r < 0.5,
            "center pixel should be the rasterized blue circle, got {center:?}"
        );

        // (2) scale = 2.0 → ceil(32*2) = 64 on each axis → 4× the pixels.
        let (w2, h2, colors2) = rasterize_svg_bytes(TEST_SVG.as_bytes(), 2.0).expect("raster 2x");
        assert_eq!((w2, h2), (64, 64), "scale 2.0 doubles each dimension");
        assert_eq!(colors2.len(), 4 * n1, "scale 2.0 yields 4× the pixel count");
    }

    /// Malformed SVG bytes surface a parse error rather than panicking.
    #[test]
    fn svg_rasterize_rejects_garbage() {
        let err = rasterize_svg_bytes(b"not an svg at all", 1.0);
        assert!(err.is_err(), "garbage input must be a parse error");
    }
}
