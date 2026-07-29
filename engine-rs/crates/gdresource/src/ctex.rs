//! CompressedTexture2D (`.ctex`) reader.
//!
//! Reads Godot's `.ctex` compiled-texture format
//! (`scene/resources/compressed_texture.cpp` `CompressedTexture2D::_load_data`
//! + `load_image_from_file`; writer in
//! `editor/import/resource_importer_texture.cpp`). Little-endian throughout.
//!
//! Exported/imported Godot projects reference `.godot/imported/*.ctex`
//! artifacts (via `.import` sidecars) rather than the source PNGs, so loading
//! them is required to open a real project. This reader produces the same
//! `Texture2D` [`Resource`] shape as the crate's PNG importer
//! ([`crate::importers::import_image`]) — width/height/format/pixels — so
//! downstream consumers are agnostic to whether a texture came from a source
//! image or a compiled `.ctex`.
//!
//! # Layout
//!
//! ```text
//! Outer header:
//!   magic        4 bytes  "GST2"
//!   version      u32      (<= FORMAT_VERSION = 1)
//!   width        u32      original texture width
//!   height       u32      original texture height
//!   df           u32      FORMAT_BIT_* flags (stream / mipmaps / detect)
//!   mipmap_limit u32
//!   reserved     u32 x 3
//! Inner image header:
//!   data_format  u32      DATA_FORMAT_IMAGE(0)/PNG(1)/WEBP(2)/BASIS(3)
//!   w            u16      image width
//!   h            u16      image height
//!   mipmaps      u32      extra mip levels
//!   format       u32      Image::Format enum
//! Data (level 0 read, remaining mip levels skipped):
//!   PNG/WEBP:    u32 size + `size` bytes (an embedded PNG/WebP file), per level
//!   IMAGE:       raw pixel bytes (w*h*bpp for level 0), per level
//! ```
//!
//! # Support
//!
//! - `DATA_FORMAT_PNG`: decoded by reusing the crate's PNG importer.
//! - `DATA_FORMAT_IMAGE`: raw uncompressed `L8`/`LA8`/`RGB8`/`RGBA8`.
//! - `DATA_FORMAT_WEBP` / `DATA_FORMAT_BASIS_UNIVERSAL` and other
//!   `Image::Format`s return a clear [`EngineError::Unsupported`] (this build
//!   has no WebP/Basis decoder). Only mip level 0 is read.

use std::path::Path;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::Color;
use gdvariant::Variant;

use crate::importers::{import_image_bytes, import_webp_bytes};
use crate::resource::Resource;

/// Magic bytes at the start of a `.ctex` CompressedTexture2D file.
pub const CTEX_MAGIC: &[u8; 4] = b"GST2";

/// Highest `.ctex` FORMAT_VERSION this reader accepts.
const CTEX_FORMAT_VERSION_MAX: u32 = 1;

// DataFormat enum (compressed_texture.h).
const CTEX_DATA_FORMAT_IMAGE: u32 = 0;
const CTEX_DATA_FORMAT_PNG: u32 = 1;
const CTEX_DATA_FORMAT_WEBP: u32 = 2;
const CTEX_DATA_FORMAT_BASIS_UNIVERSAL: u32 = 3;

// Image::Format enum values (core/io/image.h) supported for DATA_FORMAT_IMAGE.
const IMG_FORMAT_L8: u32 = 0;
const IMG_FORMAT_LA8: u32 = 1;
const IMG_FORMAT_RGB8: u32 = 4;
const IMG_FORMAT_RGBA8: u32 = 5;

/// A bounds-checked little-endian cursor for `.ctex` parsing.
struct CtexCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> CtexCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> EngineResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| EngineError::Parse(".ctex length overflow".into()))?;
        if end > self.data.len() {
            return Err(EngineError::Parse(format!(
                ".ctex truncated: need {n} byte(s) at offset {}",
                self.pos
            )));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u16(&mut self) -> EngineResult<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> EngineResult<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Loads a `.ctex` CompressedTexture2D file into a `Texture2D` [`Resource`].
///
/// `path` supplies the resource's `res://` path (from its filename). All reads
/// are bounds-checked, so truncated or corrupt input yields an error rather
/// than a panic.
pub fn load_ctex_bytes(data: &[u8], path: &Path) -> EngineResult<Arc<Resource>> {
    let mut c = CtexCursor::new(data);

    if c.take(4)? != CTEX_MAGIC {
        return Err(EngineError::Parse(
            "not a .ctex file (expected GST2 magic)".into(),
        ));
    }
    let version = c.u32()?;
    if version > CTEX_FORMAT_VERSION_MAX {
        return Err(EngineError::Unsupported(format!(
            ".ctex format version {version} is newer than supported ({CTEX_FORMAT_VERSION_MAX})"
        )));
    }
    let _width = c.u32()?; // original texture width
    let _height = c.u32()?; // original texture height
    let _df = c.u32()?; // FORMAT_BIT_* flags
    let _mipmap_limit = c.u32()?;
    c.u32()?; // reserved
    c.u32()?;
    c.u32()?;

    // Inner image header.
    let data_format = c.u32()?;
    let w = c.u16()? as u32;
    let h = c.u16()? as u32;
    let _mipmaps = c.u32()?; // extra mip levels; we read level 0 only
    let img_format = c.u32()?;

    match data_format {
        CTEX_DATA_FORMAT_PNG => {
            // Level 0: u32 size + an embedded PNG file. Reuse the crate's PNG
            // importer so a `.ctex`-sourced texture matches a source-PNG one.
            let size = c.u32()? as usize;
            let png = c.take(size)?;
            import_image_bytes(png, path)
        }
        CTEX_DATA_FORMAT_WEBP => {
            // Level 0: u32 size + an embedded WebP file (same layout as PNG).
            // Godot's default 2D texture importer stores textures this way, so
            // this is the common path for any real imported project. (pat-nzgdu.8)
            let size = c.u32()? as usize;
            let webp = c.take(size)?;
            import_webp_bytes(webp, path)
        }
        CTEX_DATA_FORMAT_BASIS_UNIVERSAL => Err(EngineError::Unsupported(
            "Basis Universal .ctex is not supported".into(),
        )),
        CTEX_DATA_FORMAT_IMAGE => {
            let (bpp, format_str) = match img_format {
                IMG_FORMAT_L8 => (1usize, "L"),
                IMG_FORMAT_LA8 => (2, "LA"),
                IMG_FORMAT_RGB8 => (3, "RGB"),
                IMG_FORMAT_RGBA8 => (4, "RGBA"),
                other => {
                    return Err(EngineError::Unsupported(format!(
                        ".ctex uncompressed Image::Format {other} is not supported (only L8/LA8/RGB8/RGBA8)"
                    )))
                }
            };
            let level0 = (w as usize)
                .checked_mul(h as usize)
                .and_then(|n| n.checked_mul(bpp))
                .ok_or_else(|| EngineError::Parse(".ctex image size overflow".into()))?;
            let raw = c.take(level0)?;
            Ok(build_raw_texture(w, h, raw, bpp, format_str, path))
        }
        other => Err(EngineError::Parse(format!(
            "unknown .ctex data format {other}"
        ))),
    }
}

/// Builds a `Texture2D` [`Resource`] from tightly-packed 8-bit pixel data,
/// matching the shape [`crate::importers::import_image`] produces.
///
/// `raw` holds exactly `w * h * bpp` bytes (guaranteed by the caller).
fn build_raw_texture(
    w: u32,
    h: u32,
    raw: &[u8],
    bpp: usize,
    format_str: &str,
    path: &Path,
) -> Arc<Resource> {
    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(w as i64));
    res.set_property("height", Variant::Int(h as i64));
    res.set_property("format", Variant::String(format_str.into()));
    res.set_property("bit_depth", Variant::Int(8));

    let count = w as usize * h as usize;
    let mut colors = Vec::with_capacity(count);
    for i in 0..count {
        let idx = i * bpp;
        let color = match bpp {
            1 => {
                let g = raw[idx] as f32 / 255.0;
                Color::new(g, g, g, 1.0)
            }
            2 => {
                let g = raw[idx] as f32 / 255.0;
                let a = raw[idx + 1] as f32 / 255.0;
                Color::new(g, g, g, a)
            }
            3 => {
                let r = raw[idx] as f32 / 255.0;
                let g = raw[idx + 1] as f32 / 255.0;
                let b = raw[idx + 2] as f32 / 255.0;
                Color::new(r, g, b, 1.0)
            }
            _ => {
                let r = raw[idx] as f32 / 255.0;
                let g = raw[idx + 1] as f32 / 255.0;
                let b = raw[idx + 2] as f32 / 255.0;
                let a = raw[idx + 3] as f32 / 255.0;
                Color::new(r, g, b, a)
            }
        };
        colors.push(Variant::Color(color));
    }
    res.set_property("has_pixel_data", Variant::Bool(!colors.is_empty()));
    res.set_property("pixels", Variant::Array(colors));
    Arc::new(res)
}

/// Encodes tightly-packed RGBA8 pixels as an uncompressed `.ctex`
/// (`DATA_FORMAT_IMAGE` / `IMG_FORMAT_RGBA8`) — the inverse of
/// [`load_ctex_bytes`]'s image arm (pat-s3rrv.2).
///
/// Nothing in the engine could *write* an import artifact before this: every
/// `.godot/imported/*.ctex` Patina resolved had to have been produced by a real
/// Godot editor, so a project authored in Patina had no artifacts at all. The
/// header is the same 48 bytes the reader consumes, so a round-trip through
/// `load_ctex_bytes` returns the pixels it was given.
///
/// Errors when `rgba` is not exactly `width * height * 4` bytes, rather than
/// writing a header whose declared extent the payload cannot satisfy.
pub fn save_ctex_image_rgba8(width: u32, height: u32, rgba: &[u8]) -> EngineResult<Vec<u8>> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| EngineError::Parse(".ctex image size overflow".into()))?;
    if rgba.len() != expected {
        return Err(EngineError::Parse(format!(
            ".ctex RGBA8 payload is {} bytes but {width}x{height} needs {expected}",
            rgba.len()
        )));
    }
    // `w`/`h` are u16 in the inner image header, so the format itself caps a
    // texture at 65535 on a side.
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(EngineError::Unsupported(format!(
            ".ctex dimensions {width}x{height} exceed the format's 65535 limit"
        )));
    }

    let mut d = Vec::with_capacity(48 + rgba.len());
    d.extend_from_slice(CTEX_MAGIC);
    d.extend_from_slice(&1u32.to_le_bytes()); // format version
    d.extend_from_slice(&width.to_le_bytes()); // original width
    d.extend_from_slice(&height.to_le_bytes()); // original height
    d.extend_from_slice(&0u32.to_le_bytes()); // FORMAT_BIT_* flags
    d.extend_from_slice(&0u32.to_le_bytes()); // mipmap limit
    d.extend_from_slice(&[0u8; 12]); // reserved (3 x u32)

    d.extend_from_slice(&CTEX_DATA_FORMAT_IMAGE.to_le_bytes());
    d.extend_from_slice(&(width as u16).to_le_bytes());
    d.extend_from_slice(&(height as u16).to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // no extra mip levels
    d.extend_from_slice(&IMG_FORMAT_RGBA8.to_le_bytes());
    d.extend_from_slice(rgba);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny valid 1x1 red PNG (crate test fixture).
    fn tiny_red_png() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/red1x1.png"))
            .expect("fixture red1x1.png")
    }

    /// Outer .ctex header (magic, version 1, width/height, flags, mipmap_limit, 3 reserved).
    fn ctex_header(width: u32, height: u32) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(CTEX_MAGIC);
        d.extend_from_slice(&1u32.to_le_bytes()); // version
        d.extend_from_slice(&width.to_le_bytes());
        d.extend_from_slice(&height.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // df flags
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmap_limit
        d.extend_from_slice(&[0u8; 12]); // 3 reserved u32
        d
    }

    /// Inner image header (data_format, w16, h16, mipmaps=0, image format).
    fn ctex_inner(d: &mut Vec<u8>, data_format: u32, w: u16, h: u16, img_format: u32) {
        d.extend_from_slice(&data_format.to_le_bytes());
        d.extend_from_slice(&w.to_le_bytes());
        d.extend_from_slice(&h.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmaps
        d.extend_from_slice(&img_format.to_le_bytes());
    }

    fn build_ctex_png(width: u32, height: u32, png: &[u8]) -> Vec<u8> {
        let mut d = ctex_header(width, height);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_PNG,
            width as u16,
            height as u16,
            IMG_FORMAT_RGBA8,
        );
        d.extend_from_slice(&(png.len() as u32).to_le_bytes()); // level-0 size
        d.extend_from_slice(png);
        d
    }

    fn build_ctex_image(width: u32, height: u32, img_format: u32, raw: &[u8]) -> Vec<u8> {
        let mut d = ctex_header(width, height);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_IMAGE,
            width as u16,
            height as u16,
            img_format,
        );
        d.extend_from_slice(raw);
        d
    }

    fn pixels_of(res: &Resource) -> &[Variant] {
        match res.get_property("pixels") {
            Some(Variant::Array(a)) => a,
            other => panic!("expected pixels array, got {other:?}"),
        }
    }

    #[test]
    fn ctex_png_fixture_decodes_to_texture2d() {
        // Embed the 1x1 red PNG fixture inside a .ctex (DATA_FORMAT_PNG).
        let png = tiny_red_png();
        let ctex = build_ctex_png(1, 1, &png);
        let res = load_ctex_bytes(&ctex, Path::new("icon.ctex")).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(1)));
        let px = pixels_of(&res);
        assert_eq!(px.len(), 1);
        match &px[0] {
            Variant::Color(c) => assert!(c.r > 0.9 && c.g < 0.1),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn ctex_raw_rgba8_decodes() {
        // 2x1: red pixel, green pixel.
        let raw = [255u8, 0, 0, 255, 0, 255, 0, 255];
        let ctex = build_ctex_image(2, 1, IMG_FORMAT_RGBA8, &raw);
        let res = load_ctex_bytes(&ctex, Path::new("art.ctex")).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(2)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );
        let px = pixels_of(&res);
        assert_eq!(px.len(), 2);
        match (&px[0], &px[1]) {
            (Variant::Color(a), Variant::Color(b)) => {
                assert!(a.r > 0.9 && a.g < 0.1);
                assert!(b.g > 0.9 && b.r < 0.1);
            }
            _ => panic!("expected colors"),
        }
    }

    #[test]
    fn ctex_raw_l8_decodes() {
        let ctex = build_ctex_image(1, 1, IMG_FORMAT_L8, &[128u8]);
        let res = load_ctex_bytes(&ctex, Path::new("g.ctex")).unwrap();
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("L".into()))
        );
        match &pixels_of(&res)[0] {
            Variant::Color(c) => {
                assert!((c.r - 128.0 / 255.0).abs() < 0.01);
                assert_eq!(c.r, c.g);
                assert!((c.a - 1.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn ctex_raw_la8_and_rgb8_decode() {
        let la = load_ctex_bytes(
            &build_ctex_image(1, 1, IMG_FORMAT_LA8, &[200, 100]),
            Path::new("la.ctex"),
        )
        .unwrap();
        assert_eq!(
            la.get_property("format"),
            Some(&Variant::String("LA".into()))
        );

        let rgb = load_ctex_bytes(
            &build_ctex_image(1, 1, IMG_FORMAT_RGB8, &[0, 255, 0]),
            Path::new("rgb.ctex"),
        )
        .unwrap();
        assert_eq!(
            rgb.get_property("format"),
            Some(&Variant::String("RGB".into()))
        );
        match &pixels_of(&rgb)[0] {
            Variant::Color(c) => assert!(c.g > 0.9 && c.r < 0.1 && c.b < 0.1),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn ctex_bad_magic_errors() {
        let mut d = build_ctex_image(1, 1, IMG_FORMAT_RGBA8, &[0, 0, 0, 0]);
        d[0] = b'X';
        assert!(load_ctex_bytes(&d, Path::new("x.ctex")).is_err());
    }

    #[test]
    fn ctex_newer_version_unsupported() {
        let mut d = build_ctex_image(1, 1, IMG_FORMAT_RGBA8, &[0, 0, 0, 0]);
        d[4..8].copy_from_slice(&99u32.to_le_bytes()); // version field
        let err = load_ctex_bytes(&d, Path::new("v.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("version"), "got: {err}");
    }

    #[test]
    fn ctex_webp_unsupported() {
        let mut d = ctex_header(1, 1);
        ctex_inner(&mut d, CTEX_DATA_FORMAT_WEBP, 1, 1, IMG_FORMAT_RGBA8);
        let err = load_ctex_bytes(&d, Path::new("w.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("WebP"), "got: {err}");
    }

    #[test]
    fn ctex_basis_unsupported() {
        let mut d = ctex_header(1, 1);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_BASIS_UNIVERSAL,
            1,
            1,
            IMG_FORMAT_RGBA8,
        );
        let err = load_ctex_bytes(&d, Path::new("b.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Basis"), "got: {err}");
    }

    #[test]
    fn ctex_unsupported_image_format_errors() {
        // FORMAT_RG8 (3) is a valid Godot format but unsupported here.
        let d = build_ctex_image(1, 1, 3, &[0, 0]);
        let err = load_ctex_bytes(&d, Path::new("rg.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not supported"), "got: {err}");
    }

    #[test]
    fn ctex_truncated_never_panics() {
        let full = build_ctex_png(1, 1, &tiny_red_png());
        assert!(load_ctex_bytes(&full, Path::new("f.ctex")).is_ok());
        for len in 0..full.len() {
            let _ = load_ctex_bytes(&full[..len], Path::new("t.ctex")); // must not panic
        }
        // Raw-image fixture too.
        let raw_full = build_ctex_image(2, 2, IMG_FORMAT_RGBA8, &[0u8; 16]);
        for len in 0..raw_full.len() {
            let _ = load_ctex_bytes(&raw_full[..len], Path::new("t.ctex"));
        }
    }

    // -- save_ctex_image_rgba8 (pat-s3rrv.2) ---------------------------------

    /// The writer's output must be readable by this module's own loader, with
    /// every pixel intact — that round-trip is the whole contract, since the
    /// artifacts it produces are what `import_image`'s remap path later reads.
    #[test]
    fn saved_ctex_round_trips_through_the_loader() {
        let raw: [u8; 16] = [
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 0, // transparent white
        ];
        let encoded = save_ctex_image_rgba8(2, 2, &raw).expect("2x2 RGBA8 encodes");
        assert_eq!(&encoded[..4], CTEX_MAGIC, "artifact must carry GST2 magic");

        let res = load_ctex_bytes(&encoded, Path::new("out.ctex")).expect("round-trips");
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(2)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(2)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );

        let pixels = match res.get_property("pixels") {
            Some(Variant::Array(px)) => px.clone(),
            other => panic!("expected a pixels Array, got {other:?}"),
        };
        assert_eq!(pixels.len(), 4);
        let at = |i: usize| match &pixels[i] {
            Variant::Color(c) => *c,
            other => panic!("pixel {i} is not a Color: {other:?}"),
        };
        // Channel-exact within one 8-bit step, in order, including alpha —
        // a writer that transposed or dropped a channel fails here.
        let near = |a: f32, b: f32| (a - b).abs() < 0.005;
        assert!(near(at(0).r, 1.0) && near(at(0).g, 0.0) && near(at(0).b, 0.0));
        assert!(near(at(1).r, 0.0) && near(at(1).g, 1.0) && near(at(1).b, 0.0));
        assert!(near(at(2).r, 0.0) && near(at(2).g, 0.0) && near(at(2).b, 1.0));
        assert!(
            near(at(3).a, 0.0),
            "alpha must survive the round-trip, got {}",
            at(3).a
        );
    }

    /// A payload that does not match the declared extent is rejected rather
    /// than written — a short artifact would fail far away, inside the loader.
    #[test]
    fn saved_ctex_rejects_a_mismatched_payload() {
        let err = save_ctex_image_rgba8(2, 2, &[0u8; 12])
            .unwrap_err()
            .to_string();
        assert!(err.contains("12 bytes"), "got: {err}");
        assert!(save_ctex_image_rgba8(1, 1, &[0u8; 4]).is_ok(), "1x1 is fine");
        // Zero-sized is consistent (0 bytes) and must encode rather than error.
        assert!(save_ctex_image_rgba8(0, 0, &[]).is_ok());
    }

    /// The inner header stores the extent as u16, so anything larger cannot be
    /// represented — say so instead of silently truncating the dimensions.
    #[test]
    fn saved_ctex_rejects_dimensions_beyond_u16() {
        // Correctly sized, so this reaches the dimension check rather than
        // tripping the payload-length guard first.
        let payload = vec![0u8; 70_000 * 4];
        let err = save_ctex_image_rgba8(70_000, 1, &payload)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("65535"),
            "oversized dimensions must be refused by the u16 guard, got: {err}"
        );
    }
}
