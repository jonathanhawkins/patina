//! Binary resource importers, import-file parsing, format registry, and path resolution.
//!
//! Provides importers for common asset types (PNG, WAV, fonts), a parser for
//! Godot `.import` sidecar files, a [`ResourceFormatLoader`] registry that
//! dispatches by extension, and `res://` path resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdvariant::Variant;

use crate::loader::TresLoader;
use crate::resource::Resource;

// ---------------------------------------------------------------------------
// res:// path resolution
// ---------------------------------------------------------------------------

/// Resolves a `res://` path to an absolute [`PathBuf`].
///
/// Strips the `res://` prefix and joins the remainder with `project_root`.
/// Returns an error if `res_path` does not start with `res://`.
pub fn resolve_res_path(project_root: &Path, res_path: &str) -> EngineResult<PathBuf> {
    let relative = res_path.strip_prefix("res://").ok_or_else(|| {
        EngineError::Parse(format!("path does not start with 'res://': {res_path}"))
    })?;
    Ok(project_root.join(relative))
}

// ---------------------------------------------------------------------------
// ImageImporter — PNG header metadata
// ---------------------------------------------------------------------------

/// PNG magic bytes.
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Reads a PNG file and creates a `Texture2D` resource **with pixel data**.
///
/// Decodes 8-bit grayscale / RGB / RGBA IDAT payloads (same path as the
/// renderer). Pixel colors are stored as a `Variant::Array` of `Color`
/// under the `"pixels"` property so the runtime can upload textures.
pub fn import_image(path: &Path) -> EngineResult<Arc<Resource>> {
    // If a `.import` sidecar redirects this source image to a compiled artifact
    // (e.g. a `.ctex` under `.godot/imported/`), load that artifact instead.
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    import_image_bytes(&data, path)
}

/// Resolves a source image's `.import` sidecar to its compiled artifact path.
///
/// Looks for `<source>.import` next to `source_path`. When present, reads the
/// `[remap] path` (or the first per-platform `path.*` key), resolves the
/// `res://` artifact via the project root (found by walking up for
/// `project.godot` / `.godot/`), and returns its absolute path. Returns
/// `Ok(None)` when no sidecar exists (direct source loading is the fallback);
/// returns an error when the sidecar names an artifact that cannot be found.
fn resolve_import_artifact(source_path: &Path) -> EngineResult<Option<PathBuf>> {
    // The sidecar is "<full source filename>.import" (e.g. "icon.png.import").
    let mut sidecar_os = source_path.as_os_str().to_os_string();
    sidecar_os.push(".import");
    let sidecar = PathBuf::from(sidecar_os);
    if !sidecar.exists() {
        return Ok(None);
    }

    let import = load_import_file(&sidecar)?;
    let remap_path = import
        .import_path()
        .or_else(|| {
            // Per-platform remaps (`path.s3tc`, `path.etc2`, …) — take the first.
            import
                .remap
                .iter()
                .filter(|(k, _)| k.starts_with("path."))
                .map(|(_, v)| v.as_str())
                .next()
        })
        .ok_or_else(|| EngineError::Parse(format!("{} has no [remap] path", sidecar.display())))?;

    let project_root = find_project_root(source_path).ok_or_else(|| {
        EngineError::NotFound(format!(
            "could not locate project root to resolve import artifact for {}",
            source_path.display()
        ))
    })?;
    let artifact = resolve_res_path(&project_root, remap_path)?;
    if !artifact.exists() {
        return Err(EngineError::NotFound(format!(
            "imported artifact not found: {} (referenced by {})",
            artifact.display(),
            sidecar.display()
        )));
    }
    Ok(Some(artifact))
}

/// Walks up from `start` to find the project root: the nearest ancestor
/// directory containing `project.godot` or a `.godot/` directory.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.parent();
    while let Some(dir) = cur {
        if dir.join("project.godot").exists() || dir.join(".godot").is_dir() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Loads a `.ctex` CompressedTexture2D file into a `Texture2D` [`Resource`].
fn load_ctex_file(path: &Path) -> EngineResult<Arc<Resource>> {
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    crate::ctex::load_ctex_bytes(&data, path)
}

/// Decode PNG bytes into a Texture2D resource (pixels included).
pub fn import_image_bytes(data: &[u8], path: &Path) -> EngineResult<Arc<Resource>> {
    if data.len() < 29 {
        return Err(EngineError::Parse("PNG file too short".into()));
    }
    if data[..8] != PNG_MAGIC {
        return Err(EngineError::Parse(
            "not a valid PNG file (bad magic)".into(),
        ));
    }

    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let bit_depth = data[24];
    let color_type = data[25];

    let format = match color_type {
        0 => "L",
        2 => "RGB",
        4 => "LA",
        6 => "RGBA",
        3 => "Indexed",
        _ => "Unknown",
    };

    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(width as i64));
    res.set_property("height", Variant::Int(height as i64));
    res.set_property("format", Variant::String(format.into()));
    res.set_property("bit_depth", Variant::Int(bit_depth as i64));

    // Full pixel decode (pat-z35r2.2 / pat-p0s82.19). The hand-rolled fast path
    // covers non-interlaced 8-bit; 16-bit / palette+tRNS / Adam7-interlaced PNGs
    // fall back to the `image` crate (pat-nzgdu.112).
    if let Some(pixels) = decode_png_pixels(data).or_else(|| decode_png_via_image(data)) {
        let colors: Vec<Variant> = pixels
            .into_iter()
            .map(|(r, g, b, a)| Variant::Color(gdcore::math::Color::new(r, g, b, a)))
            .collect();
        res.set_property("pixels", Variant::Array(colors));
        res.set_property("has_pixel_data", Variant::Bool(true));
    } else {
        res.set_property("has_pixel_data", Variant::Bool(false));
    }

    Ok(Arc::new(res))
}

/// Loads a source `.webp` image (pat-nzgdu.8).
///
/// Mirrors [`import_image`]: a `.import` sidecar redirecting to a compiled
/// `.ctex` artifact wins; otherwise the loose WebP file is decoded directly.
pub fn import_webp(path: &Path) -> EngineResult<Arc<Resource>> {
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    import_webp_bytes(&data, path)
}

/// Decodes WebP bytes into a `Texture2D` [`Resource`] with a populated
/// `pixels` array — the same contract [`import_image_bytes`] produces for PNG.
///
/// Used both for loose `.webp` sources and for the WebP payload embedded in a
/// `DATA_FORMAT_WEBP` `.ctex` (see [`crate::ctex`]). Godot's default texture
/// importer stores 2D textures this way, so this is the common path for any
/// real imported/exported project.
pub fn import_webp_bytes(data: &[u8], path: &Path) -> EngineResult<Arc<Resource>> {
    let rgba = ::image::load_from_memory_with_format(data, ::image::ImageFormat::WebP)
        .map_err(|e| EngineError::Parse(format!("WebP decode failed: {e}")))?
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    let raw = rgba.into_raw(); // tightly-packed RGBA8

    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(width as i64));
    res.set_property("height", Variant::Int(height as i64));
    res.set_property("format", Variant::String("RGBA".into()));
    res.set_property("bit_depth", Variant::Int(8));

    let colors: Vec<Variant> = raw
        .chunks_exact(4)
        .map(|px| {
            Variant::Color(gdcore::math::Color::new(
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
                px[3] as f32 / 255.0,
            ))
        })
        .collect();
    let has_pixels = !colors.is_empty();
    res.set_property("pixels", Variant::Array(colors));
    res.set_property("has_pixel_data", Variant::Bool(has_pixels));

    Ok(Arc::new(res))
}

/// Loads a `source_color` image (PNG / WebP / `.ctex`, honoring a `.import`
/// sidecar) and returns its pixels converted **sRGB→linear** — for use as a sky
/// panorama layer: [`ProceduralSkyMaterial::sky_cover`], the panorama of a
/// `PanoramaSkyMaterial`, and [`PhysicalSkyMaterial::night_sky`]. Returns
/// `(width, height, linear_pixels)` in row-major order.
///
/// Those three sky layers are Godot `source_color` samplers, so the sampler
/// decodes sRGB→linear at sample time and the renderer uses the texels raw (see
/// `gdserver3d`'s `PanoramaTexture` note). The on-disk decoders
/// ([`import_image`] / [`import_webp`]) yield raw *normalized sRGB* values, so
/// this applies [`gdcore::math::Color::srgb_to_linear`] per texel to satisfy
/// that linear-pixel contract.
///
/// HDR panorama formats (`.exr` / `.hdr`) are not yet decodable and return a
/// [`EngineError::Parse`] naming the format so a caller can log and skip rather
/// than render a broken layer. A decoded-but-empty texture (e.g. a GPU-compressed
/// `.ctex` with no CPU pixels) likewise errors instead of yielding garbage.
pub fn load_color_texture_linear(
    path: &Path,
) -> EngineResult<(u32, u32, Vec<gdcore::math::Color>)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let res = match ext.as_str() {
        "webp" => import_webp(path)?,
        "ctex" => load_ctex_file(path)?,
        "exr" | "hdr" => {
            return Err(EngineError::Parse(format!(
                "HDR sky texture format '.{ext}' is not yet supported: {}",
                path.display()
            )));
        }
        // PNG and unknown extensions: `import_image` honors a `.import` sidecar
        // (→ `.ctex`) and otherwise decodes a loose PNG by magic bytes.
        _ => import_image(path)?,
    };

    let width = match res.get_property("width") {
        Some(Variant::Int(w)) => (*w).max(0) as u32,
        _ => 0,
    };
    let height = match res.get_property("height") {
        Some(Variant::Int(h)) => (*h).max(0) as u32,
        _ => 0,
    };
    let pixels: Vec<gdcore::math::Color> = match res.get_property("pixels") {
        Some(Variant::Array(arr)) => arr
            .iter()
            .map(|v| match v {
                // Decoders store normalized sRGB; linearize for source_color use.
                Variant::Color(c) => c.srgb_to_linear(),
                _ => gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0),
            })
            .collect(),
        _ => Vec::new(),
    };

    if width == 0 || height == 0 || pixels.len() != (width as usize) * (height as usize) {
        return Err(EngineError::Parse(format!(
            "texture has no usable CPU pixel data: {} ({width}x{height}, {} texels)",
            path.display(),
            pixels.len()
        )));
    }

    Ok((width, height, pixels))
}

/// Decode PNG IDAT to linear RGBA floats in \[0,1\]. Returns None on failure.
/// Decodes the PNG variants the hand-rolled [`decode_png_pixels`] fast path
/// skips — 1/2/4/16-bit depths, `PLTE`+`tRNS` palettes, and Adam7 interlacing —
/// via the pure-Rust `image` crate (backed by the `png` crate), returning
/// straight-alpha RGBA as the same `(r, g, b, a)` f32 tuples (pat-nzgdu.112).
fn decode_png_via_image(data: &[u8]) -> Option<Vec<(f32, f32, f32, f32)>> {
    let rgba = ::image::load_from_memory_with_format(data, ::image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    Some(
        rgba.into_raw()
            .chunks_exact(4)
            .map(|px| {
                (
                    px[0] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[2] as f32 / 255.0,
                    px[3] as f32 / 255.0,
                )
            })
            .collect(),
    )
}

fn decode_png_pixels(data: &[u8]) -> Option<Vec<(f32, f32, f32, f32)>> {
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    if data.len() < 33 || data[0..8] != PNG_MAGIC {
        return None;
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as usize;
    let bit_depth = data[24];
    let color_type = data[25];
    // This hand-rolled fast path only decodes non-interlaced 8-bit truecolor/
    // grayscale. 1/2/4/16-bit, palette (`PLTE`+`tRNS`), and Adam7-interlaced
    // (`data[28] != 0`) PNGs fall through to the `image`-crate path in
    // `import_image_bytes` (pat-nzgdu.112).
    if bit_depth != 8 || width == 0 || height == 0 || data[28] != 0 {
        return None;
    }
    let bpp: usize = match color_type {
        0 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };

    let mut pos = 8usize;
    let mut idat = Vec::new();
    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        if chunk_type == b"IDAT" {
            if pos + 8 + chunk_len > data.len() {
                return None;
            }
            idat.extend_from_slice(&data[pos + 8..pos + 8 + chunk_len]);
        } else if chunk_type == b"IEND" {
            break;
        }
        pos += 12 + chunk_len;
    }
    let raw = decompress_to_vec_zlib(&idat).ok()?;
    let stride = width * bpp;
    let row_bytes = 1 + stride;
    if raw.len() < row_bytes * height {
        return None;
    }
    let mut unfiltered = vec![0u8; stride * height];
    for y in 0..height {
        let filter_byte = raw[y * row_bytes];
        let src = &raw[y * row_bytes + 1..y * row_bytes + 1 + stride];
        let dst_off = y * stride;
        match filter_byte {
            0 => unfiltered[dst_off..dst_off + stride].copy_from_slice(src),
            1 => {
                for i in 0..stride {
                    let a = if i >= bpp {
                        unfiltered[dst_off + i - bpp]
                    } else {
                        0
                    };
                    unfiltered[dst_off + i] = src[i].wrapping_add(a);
                }
            }
            2 => {
                for i in 0..stride {
                    let b = if y > 0 {
                        unfiltered[(y - 1) * stride + i]
                    } else {
                        0
                    };
                    unfiltered[dst_off + i] = src[i].wrapping_add(b);
                }
            }
            3 => {
                for i in 0..stride {
                    let a = if i >= bpp {
                        unfiltered[dst_off + i - bpp]
                    } else {
                        0
                    };
                    let b = if y > 0 {
                        unfiltered[(y - 1) * stride + i]
                    } else {
                        0
                    };
                    unfiltered[dst_off + i] =
                        src[i].wrapping_add(((a as u16 + b as u16) / 2) as u8);
                }
            }
            4 => {
                for i in 0..stride {
                    let a = if i >= bpp {
                        unfiltered[dst_off + i - bpp] as i32
                    } else {
                        0
                    };
                    let b = if y > 0 {
                        unfiltered[(y - 1) * stride + i] as i32
                    } else {
                        0
                    };
                    let c = if y > 0 && i >= bpp {
                        unfiltered[(y - 1) * stride + i - bpp] as i32
                    } else {
                        0
                    };
                    let p = a + b - c;
                    let pa = (p - a).abs();
                    let pb = (p - b).abs();
                    let pc = (p - c).abs();
                    let pr = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    unfiltered[dst_off + i] = src[i].wrapping_add(pr as u8);
                }
            }
            _ => return None,
        }
    }

    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let i = (y * stride) + x * bpp;
            let (r, g, b, a) = match color_type {
                0 => {
                    let v = unfiltered[i] as f32 / 255.0;
                    (v, v, v, 1.0)
                }
                2 => (
                    unfiltered[i] as f32 / 255.0,
                    unfiltered[i + 1] as f32 / 255.0,
                    unfiltered[i + 2] as f32 / 255.0,
                    1.0,
                ),
                4 => {
                    let v = unfiltered[i] as f32 / 255.0;
                    (v, v, v, unfiltered[i + 1] as f32 / 255.0)
                }
                6 => (
                    unfiltered[i] as f32 / 255.0,
                    unfiltered[i + 1] as f32 / 255.0,
                    unfiltered[i + 2] as f32 / 255.0,
                    unfiltered[i + 3] as f32 / 255.0,
                ),
                _ => return None,
            };
            out.push((r, g, b, a));
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// WavImporter — WAV header metadata
// ---------------------------------------------------------------------------

/// Reads a WAV file's RIFF/fmt chunk and creates an `AudioStreamWAV` resource.
///
/// Only the header is parsed — no sample data is decoded.
pub fn import_wav(path: &Path) -> EngineResult<Arc<Resource>> {
    let data = std::fs::read(path).map_err(EngineError::Io)?;

    if data.len() < 44 {
        return Err(EngineError::Parse("WAV file too short".into()));
    }
    if &data[0..4] != b"RIFF" {
        return Err(EngineError::Parse(
            "not a valid WAV file (missing RIFF)".into(),
        ));
    }
    if &data[8..12] != b"WAVE" {
        return Err(EngineError::Parse(
            "not a valid WAV file (missing WAVE)".into(),
        ));
    }

    // Find the fmt chunk — it usually starts at byte 12 but we'll search for it.
    let (fmt_offset, _fmt_size) = find_chunk(&data, b"fmt ")
        .ok_or_else(|| EngineError::Parse("WAV file missing fmt chunk".into()))?;

    let fmt = &data[fmt_offset..];
    if fmt.len() < 16 {
        return Err(EngineError::Parse("WAV fmt chunk too short".into()));
    }

    let audio_format = u16::from_le_bytes([fmt[0], fmt[1]]);
    let num_channels = u16::from_le_bytes([fmt[2], fmt[3]]);
    let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);

    // Find the data chunk to compute length.
    let data_size = find_chunk(&data, b"data")
        .map(|(_, size)| size)
        .unwrap_or(0);

    let bytes_per_sample = (bits_per_sample as u32).max(1) / 8;
    let frame_size = (num_channels as u32) * bytes_per_sample;
    let total_frames = if frame_size > 0 {
        data_size / frame_size
    } else {
        0
    };
    let length_seconds = if sample_rate > 0 {
        total_frames as f64 / sample_rate as f64
    } else {
        0.0
    };

    let mut res = Resource::new("AudioStreamWAV");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("sample_rate", Variant::Int(sample_rate as i64));
    res.set_property("channels", Variant::Int(num_channels as i64));
    res.set_property("bit_depth", Variant::Int(bits_per_sample as i64));
    res.set_property("audio_format", Variant::Int(audio_format as i64));
    res.set_property("length_seconds", Variant::Float(length_seconds));

    Ok(Arc::new(res))
}

/// Finds a RIFF chunk by its 4-byte ID. Returns `(data_offset, data_size)`.
fn find_chunk(data: &[u8], id: &[u8; 4]) -> Option<(usize, u32)> {
    let mut offset = 12; // skip RIFF header (4 id + 4 size + 4 format)
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        if chunk_id == id {
            return Some((offset + 8, chunk_size));
        }
        // Advance past chunk (size is padded to even).
        offset += 8 + ((chunk_size as usize + 1) & !1);
    }
    None
}

// ---------------------------------------------------------------------------
// FontImporter — stub
// ---------------------------------------------------------------------------

/// Creates a stub `FontFile` resource for font files.
///
/// Does not parse the font — just records the path and a default size.
pub fn import_font(path: &Path) -> EngineResult<Arc<Resource>> {
    if !path.exists() {
        return Err(EngineError::NotFound(format!(
            "font file not found: {}",
            path.display()
        )));
    }

    // Load raw font bytes so the runtime can build an atlas / FreeType later
    // (pat-p0s82.19). We store length + a SHA-free presence flag; full TTF
    // parsing is optional beyond having the payload available.
    let bytes = std::fs::read(path).map_err(EngineError::Io)?;
    let mut res = Resource::new("FontFile");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("path", Variant::String(path.to_string_lossy().into()));
    res.set_property("size", Variant::Int(16));
    res.set_property("byte_length", Variant::Int(bytes.len() as i64));
    res.set_property("has_font_data", Variant::Bool(!bytes.is_empty()));

    // Parse the outline font to validate it and expose real metrics (pat-u3l8c.69).
    // Kept lenient: unparseable data still yields a resource with `parsed=false`
    // rather than an error, so bitmap/placeholder fonts continue to load.
    match ttf_parser::Face::parse(&bytes, 0) {
        Ok(face) => {
            res.set_property("parsed", Variant::Bool(true));
            res.set_property("units_per_em", Variant::Int(face.units_per_em() as i64));
            res.set_property("glyph_count", Variant::Int(face.number_of_glyphs() as i64));
            res.set_property("ascent", Variant::Int(face.ascender() as i64));
            res.set_property("descent", Variant::Int(face.descender() as i64));
            res.set_property("is_monospaced", Variant::Bool(face.is_monospaced()));
            if let Some(family) = face
                .names()
                .into_iter()
                .filter(|n| n.name_id == ttf_parser::name_id::FAMILY)
                .find_map(|n| n.to_string())
            {
                res.set_property("family_name", Variant::String(family));
            }
        }
        Err(_) => {
            res.set_property("parsed", Variant::Bool(false));
        }
    }
    // Store raw bytes as Array of Int for round-trip without binary Variant type.
    // Cap stored preview to first 64 bytes in properties for huge files; full
    // path remains for loaders that re-read the file.
    let preview: Vec<Variant> = bytes
        .iter()
        .take(64)
        .map(|b| Variant::Int(*b as i64))
        .collect();
    res.set_property("data_preview", Variant::Array(preview));

    Ok(Arc::new(res))
}

// ---------------------------------------------------------------------------
// ImportFileParser — .import sidecar files
// ---------------------------------------------------------------------------

/// Parsed contents of a Godot `.import` sidecar file.
#[derive(Debug, Clone, Default)]
pub struct ImportFile {
    /// `[remap]` section entries.
    pub remap: HashMap<String, String>,
    /// `[deps]` section entries.
    pub deps: HashMap<String, String>,
    /// Any other sections, keyed by section name.
    pub other_sections: HashMap<String, HashMap<String, String>>,
}

impl ImportFile {
    /// Convenience: the `importer` value from `[remap]`.
    pub fn importer(&self) -> Option<&str> {
        self.remap.get("importer").map(|s| s.as_str())
    }

    /// Convenience: the `type` value from `[remap]`.
    pub fn resource_type(&self) -> Option<&str> {
        self.remap.get("type").map(|s| s.as_str())
    }

    /// Convenience: the `source_file` value from `[deps]`.
    pub fn source_file(&self) -> Option<&str> {
        self.deps.get("source_file").map(|s| s.as_str())
    }

    /// Convenience: the `uid` value from `[remap]`.
    pub fn uid(&self) -> Option<&str> {
        self.remap.get("uid").map(|s| s.as_str())
    }

    /// Convenience: the `path` value from `[remap]`.
    pub fn import_path(&self) -> Option<&str> {
        self.remap.get("path").map(|s| s.as_str())
    }
}

/// Parses a `.import` sidecar file (INI-like format).
pub fn parse_import_file(contents: &str) -> EngineResult<ImportFile> {
    let mut result = ImportFile::default();
    let mut current_section = String::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        // Section header: [name]
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_string();
            continue;
        }

        // Key=value (or key = "value")
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let mut value = value.trim().to_string();

            // Strip surrounding quotes.
            if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                value = value[1..value.len() - 1].to_string();
            }

            match current_section.as_str() {
                "remap" => {
                    result.remap.insert(key, value);
                }
                "deps" => {
                    result.deps.insert(key, value);
                }
                section => {
                    result
                        .other_sections
                        .entry(section.to_string())
                        .or_default()
                        .insert(key, value);
                }
            }
        }
    }

    Ok(result)
}

/// Parses a `.import` sidecar file from disk.
pub fn load_import_file(path: &Path) -> EngineResult<ImportFile> {
    let contents = std::fs::read_to_string(path).map_err(EngineError::Io)?;
    parse_import_file(&contents)
}

// ---------------------------------------------------------------------------
// ResourceFormatLoader — extension-based registry
// ---------------------------------------------------------------------------

/// A loader function signature: takes a path, returns a resource.
pub type LoaderFn = fn(&Path) -> EngineResult<Arc<Resource>>;

/// Registry that maps file extensions to loader functions.
///
/// Call [`register`](ResourceFormatLoader::register) to add extension mappings,
/// then [`load_resource`](ResourceFormatLoader::load_resource) to dispatch.
#[derive(Default)]
pub struct ResourceFormatLoader {
    loaders: HashMap<String, LoaderFn>,
}

impl ResourceFormatLoader {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry pre-populated with the built-in loaders.
    pub fn with_defaults() -> Self {
        let mut rfl = Self::new();
        rfl.register(".png", import_image);
        rfl.register(".webp", import_webp);
        rfl.register(".svg", crate::image_svg::import_svg);
        rfl.register(".ctex", load_ctex_file);
        rfl.register(".wav", import_wav);
        rfl.register(".ttf", import_font);
        rfl.register(".otf", import_font);
        rfl.register(".tres", load_text_or_binary);
        rfl.register(".tscn", load_text_or_binary);
        rfl.register(".res", load_text_or_binary);
        rfl.register(".scn", load_text_or_binary);
        rfl.register(".vs", vs_loader);
        rfl.register(".gdshader", load_gdshader);
        rfl.register(".gdshaderinc", load_gdshader);
        rfl
    }

    /// Registers a loader for the given extension (including the dot, e.g. `".png"`).
    pub fn register(&mut self, extension: &str, loader: LoaderFn) {
        self.loaders.insert(extension.to_lowercase(), loader);
    }

    /// Returns the number of registered extensions.
    pub fn extension_count(&self) -> usize {
        self.loaders.len()
    }

    /// Returns `true` if a loader is registered for the extension.
    ///
    /// Accepts extensions with or without a leading dot (e.g. both `".vs"` and `"vs"`).
    pub fn can_load(&self, extension: &str) -> bool {
        let ext = extension.to_lowercase();
        if self.loaders.contains_key(&ext) {
            return true;
        }
        // Try with a leading dot if one wasn't provided
        if !ext.starts_with('.') {
            self.loaders.contains_key(&format!(".{ext}"))
        } else {
            false
        }
    }

    /// Loads a resource by dispatching to the registered loader for its extension.
    pub fn load_resource(&self, path: &Path) -> EngineResult<Arc<Resource>> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        let loader = self.loaders.get(&ext).ok_or_else(|| {
            EngineError::NotFound(format!("no loader registered for extension '{ext}'"))
        })?;

        loader(path)
    }
}

/// The engine-wide resource load entry point: `res://` resolution plus format
/// dispatch through [`ResourceFormatLoader::with_defaults`] (pat-0q2kx.45).
///
/// The format loaders and the `.import` remap resolution were fully implemented
/// and tested, but **nothing at runtime called them** — the interpreter's `load()`
/// guessed from the extension and returned empty placeholders, and the editor's
/// texture cache called a PNG-only decoder. Anything that needs a resource should
/// come through here so all four consumers agree on what a path means.
///
/// Remap handling is inherited from the individual importers: `import_image` and
/// `import_webp` each check for a `<source>.import` sidecar and load the compiled
/// `.godot/imported/*.ctex` artifact instead, so a source file that has been
/// removed after import still resolves.
#[derive(Debug)]
pub struct UnifiedLoader {
    loader: ResourceFormatLoader,
    project_root: PathBuf,
}

impl UnifiedLoader {
    /// Creates a loader rooted at `project_root`, the directory holding
    /// `project.godot`, used to resolve `res://` paths.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            loader: ResourceFormatLoader::with_defaults(),
            project_root: project_root.into(),
        }
    }

    /// Whether some registered format loader claims this path's extension.
    pub fn can_load_path(&self, path: &str) -> bool {
        match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some(ext) => self.loader.can_load(ext),
            None => false,
        }
    }

    /// Loads a `res://`-style (or plain filesystem) path.
    ///
    /// `res://` prefixes resolve against the project root; anything else is taken
    /// as a filesystem path so callers holding an already-resolved path — the
    /// editor's texture cache, for one — need no special case.
    pub fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        self.loader.load_resource(&self.resolve(path))
    }

    /// Maps a `res://` path onto the filesystem. Non-`res://` paths pass through.
    pub fn resolve(&self, path: &str) -> PathBuf {
        match path.strip_prefix("res://") {
            Some(rel) => self.project_root.join(rel),
            None => PathBuf::from(path),
        }
    }
}

impl std::fmt::Debug for ResourceFormatLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceFormatLoader")
            .field("extensions", &self.loaders.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Loader for `.tres`/`.tscn`/`.res`/`.scn` files.
///
/// Sniffs the file's leading magic: binary resources (`RSRC`/`RSCC`) route to
/// [`load_res_binary`](crate::res_loader::load_res_binary); everything else is
/// parsed as Godot text (`.tres`/`.tscn`). This lets a binary-saved `.tres` and
/// a text-saved `.res` both load by content rather than extension.
fn load_text_or_binary(path: &Path) -> EngineResult<Arc<Resource>> {
    let path_str = path.to_str().unwrap_or("");
    let bytes = std::fs::read(path).map_err(EngineError::Io)?;
    if crate::res_loader::is_res_binary(&bytes) {
        crate::res_loader::load_res_binary(&bytes, path_str)
    } else {
        let text = std::str::from_utf8(&bytes)
            .map_err(|e| EngineError::Parse(format!("invalid UTF-8 in {path_str}: {e}")))?;
        TresLoader::new().parse_str(text, path_str)
    }
}

/// Loader for `.gdshader` / `.gdshaderinc` files.
///
/// Godot stores shader source in plain-text `.gdshader` files (and reusable
/// include fragments in `.gdshaderinc`). This produces a `Shader` [`Resource`]
/// whose `code` property holds the verbatim source, `shader_type` holds the
/// declared `shader_type` mode (e.g. `"spatial"`, `"canvas_item"`, `"sky"`),
/// and `uniforms` holds a comma-joined list of the declared uniform names so
/// callers (the inspector, ShaderMaterial parameter resolution) can enumerate
/// parameters without re-tokenizing. Include files carry no `shader_type`
/// declaration, so that property is simply omitted for them.
pub fn load_gdshader(path: &Path) -> EngineResult<Arc<Resource>> {
    let source = std::fs::read_to_string(path).map_err(EngineError::Io)?;
    Ok(Arc::new(shader_from_source(
        &source,
        path.to_str().unwrap_or(""),
    )))
}

/// Builds a `Shader` [`Resource`] from raw shader source (no filesystem access).
///
/// Shared by [`load_gdshader`] and the tscn/tres loaders, which encounter inline
/// `[sub_resource type="Shader"]` sections with a `code = "..."` property.
pub fn shader_from_source(source: &str, res_path: &str) -> Resource {
    use crate::shader_tokenizer::{
        extract_shader_type, extract_uniforms, tokenize_shader_no_comments,
    };

    let mut res = Resource::new("Shader");
    if !res_path.is_empty() {
        res.path = res_path.to_string();
    }
    res.set_property("code", Variant::String(source.to_string()));

    // Tokenize once to derive shader_type + uniform names. A lex error (bad
    // source) is non-fatal: the raw code still loads so the editor can show and
    // repair it, matching Godot's tolerance of shaders that fail to compile.
    if let Ok(tokens) = tokenize_shader_no_comments(source) {
        if let Some(ty) = extract_shader_type(&tokens) {
            res.set_property("shader_type", Variant::String(ty));
        }
        let names: Vec<String> = extract_uniforms(&tokens)
            .into_iter()
            .map(|u| u.name)
            .collect();
        if !names.is_empty() {
            res.set_property("uniforms", Variant::String(names.join(",")));
        }
    }
    res
}

/// Loader function for `.vs` (VisualScript) files.
///
/// VisualScript was deprecated in Godot 4. This loader creates a stub
/// resource so that scenes referencing `.vs` files can load without errors.
fn vs_loader(path: &Path) -> EngineResult<Arc<Resource>> {
    if !path.exists() {
        return Err(EngineError::NotFound(format!(
            "VisualScript file not found: {}",
            path.display()
        )));
    }
    let mut res = Resource::new("VisualScript");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("_deprecated", Variant::Bool(true));
    Ok(Arc::new(res))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    // -- Minimal valid PNG ---------------------------------------------------

    /// Creates a minimal valid PNG with the given dimensions.
    fn make_minimal_png(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        // PNG magic
        data.extend_from_slice(&PNG_MAGIC);
        // IHDR chunk: length=13, "IHDR", 4 bytes width, 4 bytes height,
        // 1 byte bit_depth, 1 byte color_type, 3 bytes (compression, filter, interlace)
        // then 4 bytes CRC (we use zeros — not validated here).
        data.extend_from_slice(&13u32.to_be_bytes()); // chunk length
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.push(8); // bit depth
        data.push(6); // color type: RGBA
        data.push(0); // compression
        data.push(0); // filter
        data.push(0); // interlace
        data.extend_from_slice(&[0u8; 4]); // CRC placeholder
                                           // IEND chunk
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(b"IEND");
        data.extend_from_slice(&[0u8; 4]); // CRC placeholder
        data
    }

    // -- Minimal valid WAV ---------------------------------------------------

    /// Creates a minimal valid WAV with the given params.
    fn make_minimal_wav(
        sample_rate: u32,
        num_channels: u16,
        bits_per_sample: u16,
        num_samples: u32,
    ) -> Vec<u8> {
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = num_samples * num_channels as u32 * bits_per_sample as u32 / 8;
        let file_size = 36 + data_size;

        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        // fmt chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        // data chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        // Silence samples
        buf.resize(buf.len() + data_size as usize, 0);
        buf
    }

    // -- res:// path resolution -----------------------------------------------

    #[test]
    fn resolve_res_path_basic() {
        let root = PathBuf::from("/project");
        let resolved = resolve_res_path(&root, "res://icon.png").unwrap();
        assert_eq!(resolved, PathBuf::from("/project/icon.png"));
    }

    #[test]
    fn resolve_res_path_nested() {
        let root = PathBuf::from("/project");
        let resolved = resolve_res_path(&root, "res://assets/sprites/hero.png").unwrap();
        assert_eq!(resolved, PathBuf::from("/project/assets/sprites/hero.png"));
    }

    #[test]
    fn resolve_res_path_no_prefix_fails() {
        let root = PathBuf::from("/project");
        let result = resolve_res_path(&root, "/absolute/path.png");
        assert!(result.is_err());
    }

    // -- ImageImporter --------------------------------------------------------

    /// Minimal 1×1 red RGB PNG (8-bit) with valid IHDR+IDAT+IEND.
    fn tiny_red_png() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/red1x1.png"))
            .expect("fixture red1x1.png")
    }

    #[test]
    fn import_png_decodes_pixel_data() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("red.png");
        std::fs::write(&path, tiny_red_png()).unwrap();
        let res = import_image(&path).unwrap();
        assert_eq!(
            res.get_property("has_pixel_data"),
            Some(&Variant::Bool(true))
        );
        match res.get_property("pixels") {
            Some(Variant::Array(a)) => {
                assert_eq!(a.len(), 1, "1x1 image");
                match &a[0] {
                    Variant::Color(c) => {
                        assert!(c.r > 0.9, "red channel, got {}", c.r);
                        assert!(c.g < 0.1, "green should be low");
                    }
                    other => panic!("expected Color, got {other:?}"),
                }
            }
            other => panic!("pixels missing: {other:?}"),
        }
    }

    /// Builds a real 1×1 RGBA PNG (valid deflate IDAT) filled with one color.
    /// CRCs are zero — [`decode_png_pixels`] walks chunks by length and does not
    /// validate them, so this is sufficient for round-trip decode tests.
    fn solid_rgba_png(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        use miniz_oxide::deflate::compress_to_vec_zlib;
        let mut data = Vec::new();
        data.extend_from_slice(&PNG_MAGIC);
        // IHDR: 1×1, 8-bit, color type 6 (RGBA).
        data.extend_from_slice(&13u32.to_be_bytes());
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.push(8);
        data.push(6);
        data.extend_from_slice(&[0u8, 0u8, 0u8]); // compression, filter, interlace
        data.extend_from_slice(&[0u8; 4]); // CRC placeholder
                                           // IDAT: one scanline = filter byte 0 followed by RGBA.
        let idat = compress_to_vec_zlib(&[0u8, r, g, b, a], 6);
        data.extend_from_slice(&(idat.len() as u32).to_be_bytes());
        data.extend_from_slice(b"IDAT");
        data.extend_from_slice(&idat);
        data.extend_from_slice(&[0u8; 4]); // CRC placeholder
                                           // IEND
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(b"IEND");
        data.extend_from_slice(&[0u8; 4]);
        data
    }

    #[test]
    fn load_color_texture_linear_applies_srgb_to_linear() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("gray.png");
        // sRGB mid-gray: byte 128 ≈ 0.502 normalized.
        std::fs::write(&path, solid_rgba_png(128, 128, 128, 255)).unwrap();

        let (w, h, pixels) = load_color_texture_linear(&path).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(pixels.len(), 1);
        let c = pixels[0];
        // srgb_to_linear(128/255) ≈ 0.216 — far below the ~0.502 sRGB value, so
        // this proves the loader linearized rather than passing texels through raw.
        let expected = gdcore::math::srgb_to_linear_channel(128.0 / 255.0);
        assert!(
            (c.r - expected).abs() < 1e-4,
            "r={} expected≈{expected}",
            c.r
        );
        assert!(
            c.r < 0.30,
            "linearized mid-gray must drop well below sRGB 0.5, got {}",
            c.r
        );
        assert!(
            (c.a - 1.0).abs() < 1e-6,
            "alpha must be preserved (not linearized), got {}",
            c.a
        );
    }

    #[test]
    fn load_color_texture_linear_rejects_hdr_formats() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sky.exr");
        std::fs::write(&path, b"not really exr data").unwrap();
        let err = load_color_texture_linear(&path).unwrap_err();
        assert!(
            format!("{err}").contains(".exr"),
            "error should name the unsupported format: {err}"
        );
    }

    #[test]
    fn import_png_reads_dimensions() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.png");
        std::fs::write(&path, make_minimal_png(128, 64)).unwrap();

        let res = import_image(&path).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(128)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(64)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );
        assert_eq!(res.get_property("bit_depth"), Some(&Variant::Int(8)));
    }

    #[test]
    fn import_png_rgb_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rgb.png");
        let mut data = make_minimal_png(32, 32);
        data[25] = 2; // color_type = RGB
        std::fs::write(&path, &data).unwrap();

        let res = import_image(&path).unwrap();
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGB".into()))
        );
    }

    #[test]
    fn import_png_grayscale_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("gray.png");
        let mut data = make_minimal_png(16, 16);
        data[25] = 0; // color_type = Grayscale
        std::fs::write(&path, &data).unwrap();

        let res = import_image(&path).unwrap();
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("L".into()))
        );
    }

    #[test]
    fn import_png_too_short_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tiny.png");
        std::fs::write(&path, &[0x89, 0x50, 0x4E, 0x47]).unwrap();

        assert!(import_image(&path).is_err());
    }

    #[test]
    fn import_png_bad_magic_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.png");
        let mut data = make_minimal_png(1, 1);
        data[0] = 0x00; // corrupt magic
        std::fs::write(&path, &data).unwrap();

        assert!(import_image(&path).is_err());
    }

    // -- WavImporter ----------------------------------------------------------

    #[test]
    fn import_wav_reads_metadata() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wav");
        std::fs::write(&path, make_minimal_wav(44100, 2, 16, 44100)).unwrap();

        let res = import_wav(&path).unwrap();
        assert_eq!(res.class_name, "AudioStreamWAV");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(44100)));
        assert_eq!(res.get_property("channels"), Some(&Variant::Int(2)));
        assert_eq!(res.get_property("bit_depth"), Some(&Variant::Int(16)));
        assert_eq!(res.get_property("audio_format"), Some(&Variant::Int(1)));
    }

    #[test]
    fn import_wav_length_seconds() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("onesec.wav");
        // 44100 samples at 44100 Hz = 1 second, mono 16-bit
        std::fs::write(&path, make_minimal_wav(44100, 1, 16, 44100)).unwrap();

        let res = import_wav(&path).unwrap();
        let length = match res.get_property("length_seconds") {
            Some(Variant::Float(f)) => *f,
            other => panic!("expected Float, got {other:?}"),
        };
        assert!((length - 1.0).abs() < 0.001);
    }

    #[test]
    fn import_wav_too_short_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tiny.wav");
        std::fs::write(&path, b"RIFF").unwrap();

        assert!(import_wav(&path).is_err());
    }

    #[test]
    fn import_wav_bad_riff_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.wav");
        let mut data = make_minimal_wav(44100, 1, 16, 100);
        data[0..4].copy_from_slice(b"NOPE");
        std::fs::write(&path, &data).unwrap();

        assert!(import_wav(&path).is_err());
    }

    // -- FontImporter ---------------------------------------------------------

    #[test]
    fn import_font_creates_resource() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.ttf");
        std::fs::write(&path, b"fake font data").unwrap();

        let res = import_font(&path).unwrap();
        assert_eq!(res.class_name, "FontFile");
        assert_eq!(res.get_property("size"), Some(&Variant::Int(16)));
        assert!(res.get_property("path").is_some());
    }

    #[test]
    fn import_font_nonexistent_fails() {
        assert!(import_font(Path::new("/nonexistent/font.ttf")).is_err());
    }

    #[test]
    fn import_font_parses_real_ttf() {
        const FONT: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_assets/Cantarell-Regular.ttf"
        ));
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("Cantarell-Regular.ttf");
        std::fs::write(&path, FONT).unwrap();

        let res = import_font(&path).unwrap();
        assert_eq!(res.get_property("parsed"), Some(&Variant::Bool(true)));
        match res.get_property("units_per_em") {
            Some(Variant::Int(upm)) => assert!(*upm > 0),
            other => panic!("units_per_em: {other:?}"),
        }
        match res.get_property("glyph_count") {
            Some(Variant::Int(gc)) => assert!(*gc > 10),
            other => panic!("glyph_count: {other:?}"),
        }
        assert!(res.get_property("family_name").is_some());
    }

    #[test]
    fn import_font_marks_invalid_data_unparsed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.ttf");
        std::fs::write(&path, b"not a real font").unwrap();
        let res = import_font(&path).unwrap();
        assert_eq!(res.get_property("parsed"), Some(&Variant::Bool(false)));
    }

    // -- ImportFileParser -----------------------------------------------------

    #[test]
    fn parse_import_file_basic() {
        let contents = r#"[remap]

importer="texture"
type="CompressedTexture2D"
uid="uid://abc"
path="res://.godot/imported/icon.ctex"

[deps]

source_file="res://icon.png"
"#;
        let import = parse_import_file(contents).unwrap();
        assert_eq!(import.importer(), Some("texture"));
        assert_eq!(import.resource_type(), Some("CompressedTexture2D"));
        assert_eq!(import.uid(), Some("uid://abc"));
        assert_eq!(
            import.import_path(),
            Some("res://.godot/imported/icon.ctex")
        );
        assert_eq!(import.source_file(), Some("res://icon.png"));
    }

    #[test]
    fn parse_import_file_with_comments() {
        let contents = r#"; Godot import file
# Another comment

[remap]
importer="wav"
type="AudioStreamWAV"
"#;
        let import = parse_import_file(contents).unwrap();
        assert_eq!(import.importer(), Some("wav"));
    }

    #[test]
    fn parse_import_file_extra_sections() {
        let contents = r#"[remap]
importer="font"

[params]
antialiased=true
"#;
        let import = parse_import_file(contents).unwrap();
        assert_eq!(import.importer(), Some("font"));
        assert_eq!(
            import
                .other_sections
                .get("params")
                .and_then(|s| s.get("antialiased")),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn parse_import_file_empty() {
        let import = parse_import_file("").unwrap();
        assert!(import.remap.is_empty());
        assert!(import.deps.is_empty());
    }

    // -- ResourceFormatLoader -------------------------------------------------

    #[test]
    fn format_loader_register_and_dispatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("icon.png");
        std::fs::write(&path, make_minimal_png(64, 64)).unwrap();

        let mut rfl = ResourceFormatLoader::new();
        rfl.register(".png", import_image);
        assert!(rfl.can_load(".png"));
        assert!(!rfl.can_load(".jpg"));

        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "Texture2D");
    }

    #[test]
    fn format_loader_with_defaults() {
        let rfl = ResourceFormatLoader::with_defaults();
        assert!(rfl.can_load(".png"));
        assert!(rfl.can_load(".wav"));
        assert!(rfl.can_load(".ttf"));
        assert!(rfl.can_load(".otf"));
        assert!(rfl.can_load(".tres"));
        assert!(rfl.can_load(".tscn"));
        assert!(rfl.can_load(".res"));
        assert!(rfl.can_load(".scn"));
        assert!(rfl.can_load(".ctex"));
        assert!(rfl.can_load(".gdshader"));
        assert!(rfl.can_load(".gdshaderinc"));
        assert_eq!(rfl.extension_count(), 12);
    }

    #[test]
    fn format_loader_unknown_extension_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file.xyz");
        std::fs::write(&path, b"data").unwrap();

        let rfl = ResourceFormatLoader::new();
        assert!(rfl.load_resource(&path).is_err());
    }

    #[test]
    fn format_loader_loads_wav() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sound.wav");
        std::fs::write(&path, make_minimal_wav(22050, 1, 8, 1000)).unwrap();

        let rfl = ResourceFormatLoader::with_defaults();
        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "AudioStreamWAV");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(22050)));
    }

    #[test]
    fn format_loader_loads_tres() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.tres");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[gd_resource type=\"Theme\" format=3]").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[resource]").unwrap();
        writeln!(f, "name = \"TestTheme\"").unwrap();

        let rfl = ResourceFormatLoader::with_defaults();
        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "Theme");
    }

    #[test]
    fn format_loader_case_insensitive() {
        let mut rfl = ResourceFormatLoader::new();
        rfl.register(".png", import_image);
        assert!(rfl.can_load(".PNG"));
        assert!(rfl.can_load(".Png"));
    }

    // ── .import remap → .ctex redirection ──

    /// Builds a minimal 1x1 RGBA8 `.ctex` (DATA_FORMAT_IMAGE=0, FORMAT_RGBA8=5).
    fn build_ctex_rgba8_1x1(rgba: [u8; 4]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(b"GST2"); // magic
        d.extend_from_slice(&1u32.to_le_bytes()); // version
        d.extend_from_slice(&1u32.to_le_bytes()); // width
        d.extend_from_slice(&1u32.to_le_bytes()); // height
        d.extend_from_slice(&0u32.to_le_bytes()); // df flags
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmap_limit
        d.extend_from_slice(&[0u8; 12]); // 3 reserved u32
        d.extend_from_slice(&0u32.to_le_bytes()); // DATA_FORMAT_IMAGE
        d.extend_from_slice(&1u16.to_le_bytes()); // w
        d.extend_from_slice(&1u16.to_le_bytes()); // h
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmaps
        d.extend_from_slice(&5u32.to_le_bytes()); // Image::FORMAT_RGBA8
        d.extend_from_slice(&rgba); // 1x1 pixel
        d
    }

    #[test]
    fn import_remap_redirects_png_to_ctex() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".godot/imported")).unwrap();

        // A deliberately-invalid source PNG: if the loader decoded it directly
        // (no redirect) the test would fail — proving the redirect happened.
        let png = root.join("icon.png");
        std::fs::write(&png, b"NOT A PNG").unwrap();

        std::fs::write(
            root.join("icon.png.import"),
            "[remap]\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\npath=\"res://.godot/imported/icon.ctex\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".godot/imported/icon.ctex"),
            build_ctex_rgba8_1x1([255, 0, 0, 255]),
        )
        .unwrap();

        let res = import_image(&png).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(1)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(1)));
        match res.get_property("pixels") {
            Some(Variant::Array(a)) => {
                assert_eq!(a.len(), 1);
                match &a[0] {
                    Variant::Color(c) => assert!(c.r > 0.9 && c.g < 0.1),
                    other => panic!("expected Color, got {other:?}"),
                }
            }
            other => panic!("expected pixels array, got {other:?}"),
        }
    }

    #[test]
    fn import_remap_missing_artifact_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".godot")).unwrap();
        let png = root.join("icon.png");
        std::fs::write(&png, b"src").unwrap();
        std::fs::write(
            root.join("icon.png.import"),
            "[remap]\npath=\"res://.godot/imported/missing.ctex\"\n",
        )
        .unwrap();

        let err = import_image(&png).unwrap_err().to_string();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn no_import_sidecar_loads_source_directly() {
        // Without a `.import`, the source PNG is loaded as before.
        let dir = tempfile::TempDir::new().unwrap();
        let png = dir.path().join("plain.png");
        std::fs::write(&png, tiny_red_png()).unwrap();
        let res = import_image(&png).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(
            res.get_property("has_pixel_data"),
            Some(&Variant::Bool(true))
        );
    }

    #[test]
    fn ctex_loader_via_registry() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("art.ctex");
        std::fs::write(&path, build_ctex_rgba8_1x1([0, 255, 0, 255])).unwrap();
        let rfl = ResourceFormatLoader::with_defaults();
        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );
    }

    // -- .gdshader loader (pat-u3l8c.43) -------------------------------------

    #[test]
    fn gdshader_loads_as_shader_resource() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("water.gdshader");
        let src = "shader_type spatial;\n\
                   uniform float wave_height = 0.5;\n\
                   uniform vec3 tint;\n\
                   void fragment() { ALBEDO = tint; }\n";
        std::fs::write(&path, src).unwrap();

        let res = load_gdshader(&path).unwrap();
        assert_eq!(res.class_name, "Shader");
        // Full source preserved verbatim under `code`.
        assert_eq!(res.get_property("code"), Some(&Variant::String(src.into())));
        // shader_type declaration extracted.
        assert_eq!(
            res.get_property("shader_type"),
            Some(&Variant::String("spatial".into()))
        );
        // Declared uniforms enumerated.
        match res.get_property("uniforms") {
            Some(Variant::String(u)) => {
                assert!(u.contains("wave_height"), "uniforms: {u}");
                assert!(u.contains("tint"), "uniforms: {u}");
            }
            other => panic!("expected uniforms string, got {other:?}"),
        }
    }

    #[test]
    fn gdshader_loads_via_registry() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("outline.gdshader");
        std::fs::write(&path, "shader_type canvas_item;\nvoid fragment() {}\n").unwrap();
        let rfl = ResourceFormatLoader::with_defaults();
        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "Shader");
        assert_eq!(
            res.get_property("shader_type"),
            Some(&Variant::String("canvas_item".into()))
        );
    }

    #[test]
    fn gdshaderinc_has_no_shader_type() {
        // Include fragments have no `shader_type` declaration.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("common.gdshaderinc");
        std::fs::write(&path, "float noise(vec2 p) { return 0.0; }\n").unwrap();
        let res = load_gdshader(&path).unwrap();
        assert_eq!(res.class_name, "Shader");
        assert!(res.get_property("shader_type").is_none());
    }

    #[test]
    fn shader_from_source_preserves_code_and_type() {
        let s = shader_from_source("shader_type sky;\nvoid sky() {}\n", "res://s.gdshader");
        assert_eq!(s.class_name, "Shader");
        assert_eq!(s.path, "res://s.gdshader");
        assert_eq!(
            s.get_property("shader_type"),
            Some(&Variant::String("sky".into()))
        );
    }
}
