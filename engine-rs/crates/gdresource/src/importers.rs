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
///
/// Public since pat-s3rrv.47: the remap is not an image concern. Godot resolves
/// EVERY imported resource through its `.import` sidecar, and the audio bridge
/// had no remap lookup at all — so a project whose `music.ogg` had been
/// replaced by `.godot/imported/music-<md5>.oggvorbisstr` found nothing on disk
/// and played silence. Sharing this one implementation keeps the audio and
/// image paths from drifting apart.
pub fn resolve_import_artifact(source_path: &Path) -> EngineResult<Option<PathBuf>> {
    // The sidecar is "<full source filename>.import" (e.g. "icon.png.import").
    let mut sidecar_os = source_path.as_os_str().to_os_string();
    sidecar_os.push(".import");
    let sidecar = PathBuf::from(sidecar_os);
    if !sidecar.exists() {
        return Ok(None);
    }

    let import = load_import_file(&sidecar)?;
    let remap_path = match import.import_path().or_else(|| {
        // Per-platform remaps (`path.s3tc`, `path.etc2`, …) — take the first.
        import
            .remap
            .iter()
            .filter(|(k, _)| k.starts_with("path."))
            .map(|(_, v)| v.as_str())
            .next()
    }) {
        Some(p) => p,
        // A sidecar with no `[remap] path` names no artifact — Godot's `keep`
        // and `skip` importers, and any asset whose `.import` carries only
        // `[params]`, are exactly this case, and they load from the SOURCE.
        // This used to be a hard error, which made a params-only sidecar fail
        // the whole load instead of configuring it (pat-s3rrv.50).
        None => return Ok(None),
    };

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
    // A `.import` sidecar can remap a source image to ANY of the compiled
    // artifact types, not just a plain `.ctex` — a 2D-array or cubemap import
    // produces a `GSTL` file with a different extension. Dispatch on that
    // extension so following a remap lands in the right parser instead of
    // failing on the magic (pat-s3rrv.48).
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("ctexarray") => crate::ctex::load_ctexarray_file(path),
        Some("ccube") => crate::ctex::load_ccube_file(path),
        Some("ccubearray") => crate::ctex::load_ccubearray_file(path),
        Some("ctex3d") => crate::ctex::load_ctex3d_file(path),
        _ => {
            let data = std::fs::read(path).map_err(EngineError::Io)?;
            crate::ctex::load_ctex_bytes(&data, path)
        }
    }
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
        res.set_property("pixels", Variant::Array(colors.into()));
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
    res.set_property("pixels", Variant::Array(colors.into()));
    res.set_property("has_pixel_data", Variant::Bool(has_pixels));

    Ok(Arc::new(res))
}

/// Decodes a `.tga` into the same pixel-bearing `Texture2D` shape
/// [`import_image_bytes`] produces (pat-s3rrv.51).
///
/// `image_tga_bmp::decode_tga` has existed and compiled all along with **zero
/// consumers** — the decoder was never the gap, the registration was. Godot
/// accepts tga through `ResourceImporterTexture`
/// (editor/import/resource_importer_texture.cpp), so `load("res://x.tga")` has
/// to answer with the same contract every other image loader answers with, or
/// the caller has to special-case which extension it asked for.
pub fn import_tga(path: &Path) -> EngineResult<Arc<Resource>> {
    // The `.import` sidecar is checked FIRST, exactly as the png/webp paths do:
    // in a real imported project the source file may have been replaced by a
    // compiled `.ctex` artifact, and skipping this would find nothing on disk.
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    let decoded = crate::image_tga_bmp::decode_tga(&data)
        .ok_or_else(|| EngineError::Parse(format!("TGA decode failed: {}", path.display())))?;
    Ok(rgba8_texture_resource(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        path,
    ))
}

/// Decodes a `.bmp` into a `Texture2D`, as [`import_tga`] does for TGA
/// (pat-s3rrv.51).
pub fn import_bmp(path: &Path) -> EngineResult<Arc<Resource>> {
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    let decoded = crate::image_tga_bmp::decode_bmp(&data)
        .ok_or_else(|| EngineError::Parse(format!("BMP decode failed: {}", path.display())))?;
    Ok(rgba8_texture_resource(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        path,
    ))
}

/// Decodes a Radiance `.hdr` into a `Texture2D` (pat-s3rrv.51).
///
/// ⚠️ HDR is the one format here whose pixels are NOT sRGB 8-bit. `decode_hdr`
/// hands back tightly packed **linear** RGB floats, so the values go into the
/// `Color`s unchanged rather than through a `/ 255.0` — dividing would crush a
/// 4.0-nit highlight to 0.0157 and quietly destroy the dynamic range that is
/// the entire reason to load an HDR. Alpha is 1.0; Radiance carries none.
pub fn import_hdr(path: &Path) -> EngineResult<Arc<Resource>> {
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    let decoded = crate::radiance_hdr::decode_hdr(&data)
        .ok_or_else(|| EngineError::Parse(format!("HDR decode failed: {}", path.display())))?;

    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(decoded.width as i64));
    res.set_property("height", Variant::Int(decoded.height as i64));
    res.set_property("format", Variant::String("RGBF".into()));
    res.set_property("bit_depth", Variant::Int(32));
    let colors: Vec<Variant> = decoded
        .rgb
        .chunks_exact(3)
        .map(|px| Variant::Color(gdcore::math::Color::new(px[0], px[1], px[2], 1.0)))
        .collect();
    let has_pixels = !colors.is_empty();
    res.set_property("pixels", Variant::Array(colors.into()));
    res.set_property("has_pixel_data", Variant::Bool(has_pixels));
    Ok(Arc::new(res))
}

/// Builds the `Texture2D` shape every 8-bit image loader in this module
/// produces, from tightly packed RGBA8 (pat-s3rrv.51).
///
/// Factored out so the tga/bmp paths cannot drift from the png/webp contract —
/// a caller reads `width`/`height`/`pixels`/`has_pixel_data` without caring
/// which extension it asked for, and that only holds if one function writes it.
fn rgba8_texture_resource(width: usize, height: usize, rgba: &[u8], path: &Path) -> Arc<Resource> {
    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(width as i64));
    res.set_property("height", Variant::Int(height as i64));
    res.set_property("format", Variant::String("RGBA".into()));
    res.set_property("bit_depth", Variant::Int(8));
    let colors: Vec<Variant> = rgba
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
    res.set_property("pixels", Variant::Array(colors.into()));
    res.set_property("has_pixel_data", Variant::Bool(has_pixels));
    Arc::new(res)
}

/// Loads a `.gltf`/`.glb` through the existing parser (pat-s3rrv.51).
///
/// `gltf_import::import_gltf` was reachable only by direct module call, so
/// `load("res://model.glb")` answered "unsupported extension" while the parser
/// sat one function away in the same crate.
pub fn import_gltf_asset(path: &Path) -> EngineResult<Arc<Resource>> {
    crate::gltf_import::import_gltf(path)
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
            .to_vec().iter()
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
// Texture import options (pat-s3rrv.50)
// ---------------------------------------------------------------------------

/// Godot's `process/channel_remap/*` enum
/// (`upstream/godot/editor/import/resource_importer_texture.h:52-63`):
/// `0..3` = R/G/B/A, `4..7` = the same inverted, `8` = Unused, `9` = Zero,
/// `10` = One. `Unused` is resolved to Zero for RGB and One for alpha, exactly
/// as `_remap_channels` does before its conversion loop (:487-491).
fn remap_channel(code: i64, src: gdcore::math::Color, is_alpha: bool) -> f32 {
    let code = if code == 8 {
        if is_alpha {
            10
        } else {
            9
        }
    } else {
        code
    };
    match code {
        0 => src.r,
        1 => src.g,
        2 => src.b,
        3 => src.a,
        4 => 1.0 - src.r,
        5 => 1.0 - src.g,
        6 => 1.0 - src.b,
        7 => 1.0 - src.a,
        9 => 0.0,
        10 => 1.0,
        // An out-of-range code keeps the channel — a scene from a newer Godot
        // must not silently zero a texture.
        _ => {
            if is_alpha {
                src.a
            } else {
                src.r
            }
        }
    }
}

/// Bilinear resample of a `Color` grid. Godot resizes with `INTERPOLATE_CUBIC`
/// (resource_importer_texture.cpp:832); bilinear is the simplification here —
/// the dimensions and the overall image match, the interpolation kernel does
/// not.
fn resize_pixels(
    src: &[gdcore::math::Color],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<gdcore::math::Color> {
    let mut out = Vec::with_capacity(dst_w * dst_h);
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return out;
    }
    let x_ratio = src_w as f32 / dst_w as f32;
    let y_ratio = src_h as f32 / dst_h as f32;
    for y in 0..dst_h {
        // Sample at pixel CENTRES, so a 2:1 downscale averages the pair it
        // covers instead of leaning on the left/top edge.
        let sy = ((y as f32 + 0.5) * y_ratio - 0.5).max(0.0);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(src_h - 1);
        let fy = sy - y0 as f32;
        for x in 0..dst_w {
            let sx = ((x as f32 + 0.5) * x_ratio - 0.5).max(0.0);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let fx = sx - x0 as f32;
            let p = |ix: usize, iy: usize| src[iy.min(src_h - 1) * src_w + ix.min(src_w - 1)];
            let (c00, c10, c01, c11) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let mix = |a: gdcore::math::Color, b: gdcore::math::Color, t: f32| {
                gdcore::math::Color::new(
                    lerp(a.r, b.r, t),
                    lerp(a.g, b.g, t),
                    lerp(a.b, b.b, t),
                    lerp(a.a, b.a, t),
                )
            };
            out.push(mix(mix(c00, c10, fx), mix(c01, c11, fx), fy));
        }
    }
    out
}

/// Applies the `ResourceImporterTexture` options this engine can express to an
/// already-decoded `Texture2D` resource (pat-s3rrv.50).
///
/// Before this, `[params]` was parsed into `ImportFile::other_sections` and had
/// **zero** readers, so every one of these was silently discarded.
///
/// Applied in Godot's own order — channel remap, then alpha processing, then
/// the size limit (resource_importer_texture.cpp:722-845). Options that need a
/// mip chain or a GPU format Patina does not model (`compress/*`,
/// `mipmaps/limit`, `roughness/*`, `hdr_*`) are recorded as properties instead
/// of being silently dropped, so a consumer can see what was asked for.
pub fn apply_texture_params(res: &Resource, params: &ImportParams) -> Resource {
    let mut out = res.clone();

    let mut width = match out.get_property("width") {
        Some(Variant::Int(w)) => *w as usize,
        _ => 0,
    };
    let mut height = match out.get_property("height") {
        Some(Variant::Int(h)) => *h as usize,
        _ => 0,
    };
    let mut pixels: Vec<gdcore::math::Color> = match out.get_property("pixels") {
        Some(Variant::Array(a)) => a
            .to_vec().iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(*c),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };

    if !pixels.is_empty() && pixels.len() == width * height {
        // -- channel remap --
        let remap = [
            params.int("process/channel_remap/red").unwrap_or(0),
            params.int("process/channel_remap/green").unwrap_or(1),
            params.int("process/channel_remap/blue").unwrap_or(2),
            params.int("process/channel_remap/alpha").unwrap_or(3),
        ];
        if remap != [0, 1, 2, 3] {
            for c in pixels.iter_mut() {
                let src = *c;
                *c = gdcore::math::Color::new(
                    remap_channel(remap[0], src, false),
                    remap_channel(remap[1], src, false),
                    remap_channel(remap[2], src, false),
                    remap_channel(remap[3], src, true),
                );
            }
        }

        // -- normal map Y inversion --
        if params.bool("process/normal_map_invert_y").unwrap_or(false) {
            for c in pixels.iter_mut() {
                c.g = 1.0 - c.g;
            }
        }

        // -- premultiplied alpha --
        if params.bool("process/premult_alpha").unwrap_or(false) {
            for c in pixels.iter_mut() {
                c.r *= c.a;
                c.g *= c.a;
                c.b *= c.a;
            }
        }

        // -- size limit --
        // Godot: only DOWNscales, and only when a side exceeds the limit; the
        // longer side becomes the limit and the other keeps the aspect ratio by
        // integer division (resource_importer_texture.cpp:824-843).
        let limit = params.int("process/size_limit").unwrap_or(0);
        if limit > 0 {
            let limit = limit as usize;
            if width > limit || height > limit {
                let (new_w, new_h) = if width >= height {
                    (limit, (height * limit / width).max(1))
                } else {
                    ((width * limit / height).max(1), limit)
                };
                pixels = resize_pixels(&pixels, width, height, new_w, new_h);
                width = new_w;
                height = new_h;
            }
        }

        out.set_property("width", Variant::Int(width as i64));
        out.set_property("height", Variant::Int(height as i64));
        out.set_property(
            "pixels",
            Variant::Array(pixels.into_iter().map(Variant::Color).collect()),
        );
    }

    // -- options recorded rather than applied --
    // Patina holds one mip level and one uncompressed pixel array, so these
    // cannot change the artifact here; recording them keeps the requested
    // setting readable instead of dropping it on the floor.
    if let Some(v) = params.bool("mipmaps/generate") {
        out.set_property("mipmaps", Variant::Bool(v));
    }
    if let Some(v) = params.int("compress/mode") {
        out.set_property("compress_mode", Variant::Int(v));
    }
    if let Some(v) = params.bool("process/fix_alpha_border") {
        out.set_property("fix_alpha_border", Variant::Bool(v));
    }
    out
}

/// [`import_image`] with the source's `.import` options applied.
///
/// ⚠️ A sidecar that names a compiled artifact (`[remap] path`) is loaded
/// as-is: Godot applied the options when it *produced* that `.ctex`, so
/// applying them again here would double them — a premultiplied-alpha texture
/// premultiplied twice, for instance. Patina has no import step, so the options
/// are applied when — and only when — the LOOSE SOURCE is what gets decoded.
pub fn import_image_with_params(path: &Path, params: &ImportParams) -> EngineResult<Arc<Resource>> {
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    let res = import_image_bytes(&data, path)?;
    if let Some(layered) = slice_layered_if_requested(&res, params, path)? {
        return Ok(layered);
    }
    Ok(Arc::new(apply_texture_params(&res, params)))
}

/// Turns a decoded source image into a layered texture when the sidecar asks
/// for one, or `None` for an ordinary 2D import (pat-s3rrv.138).
///
/// ⚠️ **Keyed on the PRESENCE of the slice options**, which is exactly how
/// upstream decides: `resource_importer_layered_texture.cpp:306-310` reads each
/// one with `p_options.has(...)`. No ordinary texture import writes a
/// `slices/*` key, so this branch cannot fire for the 2D path — its blast
/// radius on existing imports is zero.
///
/// ⚠️ **Defaults to a 2D array**, and that is a real limitation rather than a
/// choice. Which layered kind a source becomes is decided by the sidecar's
/// `[remap] importer=` (`2d_array_texture` / `cubemap_texture` /
/// `cubemap_array_texture` / `3d_texture`), and `load_source_params` reads that
/// field but does not carry it into [`ImportParams`] — so a cubemap source
/// currently slices as an array. Threading the importer name through is the
/// next step; it is deliberately not done here because
/// [`load_source_params`] is on the shared path every texture, audio and font
/// import takes.
fn slice_layered_if_requested(
    source: &Resource,
    params: &ImportParams,
    path: &Path,
) -> EngineResult<Option<Arc<Resource>>> {
    let requested = ["slices/horizontal", "slices/vertical", "slices/arrangement"]
        .iter()
        .any(|k| params.get(k).is_some());
    if !requested {
        return Ok(None);
    }

    let kind = params
        .string("importer")
        .and_then(crate::ctex::LayeredKind::from_importer_name)
        .unwrap_or(crate::ctex::LayeredKind::Texture2DArray);

    // ⚠️ Godot's defaults for these two are **8**, not 1 — an 8x8 atlas is what
    // the import dock offers first (`resource_importer_layered_texture.cpp`
    // `get_import_options`). Defaulting them to 1 would turn a sidecar that
    // sets only `slices/vertical` into a single-layer texture instead of an
    // eight-column strip.
    let hslices = params.int("slices/horizontal").unwrap_or(8).max(1) as u32;
    let vslices = params.int("slices/vertical").unwrap_or(8).max(1) as u32;
    let arrangement = params.int("slices/arrangement").unwrap_or(1);
    let layout = params.int("slices/layout").unwrap_or(1);
    let amount = params.int("slices/amount").unwrap_or(1).max(1) as u32;

    let (h, v) = kind.slice_grid(hslices, vslices, arrangement, layout, amount);
    Ok(Some(crate::ctex::slice_source_into_layers(
        source, kind, h, v, path,
    )?))
}

/// [`import_webp`] with the source's `.import` options applied. Same
/// artifact-wins rule as [`import_image_with_params`].
pub fn import_webp_with_params(path: &Path, params: &ImportParams) -> EngineResult<Arc<Resource>> {
    if let Some(artifact) = resolve_import_artifact(path)? {
        return load_ctex_file(&artifact);
    }
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    let res = import_webp_bytes(&data, path)?;
    Ok(Arc::new(apply_texture_params(&res, params)))
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

    // The `smpl` chunk's first loop, as Godot's ResourceImporterWAV reads it
    // (pat-s3rrv.47). Without this a looping sample imported from any tracker
    // or sampler played once and stopped: the loop points were in the file and
    // nothing ever read them.
    let (loop_mode, loop_begin, loop_end) = wav_smpl_loop(&data);
    res.set_property("loop_mode", Variant::Int(loop_mode));
    res.set_property("loop_begin", Variant::Int(loop_begin));
    res.set_property("loop_end", Variant::Int(loop_end));

    Ok(Arc::new(res))
}

/// [`import_wav`] with the source's `.import` options applied (pat-s3rrv.50).
///
/// Godot's `ResourceImporterWAV` rewrites the SAMPLES for most of these
/// (`force/mono` mixes down, `force/max_rate` resamples, `edit/trim` cuts
/// silence, `edit/normalize` rescales). Patina's WAV importer is header-only —
/// it decodes no sample data at all — so the options that describe the stream's
/// SHAPE are applied to the metadata that stands in for it, and the ones that
/// can only be expressed by rewriting samples are recorded as properties. Each
/// is marked below, so nothing here reads as more complete than it is.
///
/// `length_seconds` is deliberately left alone by the shape options: mixing to
/// mono or resampling changes the sample count, never the duration.
pub fn apply_wav_params(res: &Resource, params: &ImportParams) -> Resource {
    let mut out = res.clone();
    let int_of = |r: &Resource, k: &str| match r.get_property(k) {
        Some(Variant::Int(v)) => *v,
        _ => 0,
    };

    // -- applied to the stream's shape --
    if params.bool("force/mono").unwrap_or(false) {
        out.set_property("channels", Variant::Int(1));
    }
    if params.bool("force/8_bit").unwrap_or(false) {
        out.set_property("bit_depth", Variant::Int(8));
    }
    if params.bool("force/max_rate").unwrap_or(false) {
        // Godot only ever downsamples to the cap; a file already below it is
        // left where it is.
        if let Some(cap) = params.float("force/max_rate_hz") {
            let cap = cap.max(1.0) as i64;
            let current = int_of(&out, "sample_rate");
            if current > cap {
                out.set_property("sample_rate", Variant::Int(cap));
            }
        }
    }

    // -- loop points: the importer's option WINS over the file's `smpl` chunk --
    // Godot writes the editable loop back into the AudioStreamWAV, which is why
    // an artist can loop a sample the sampler never marked (pat-s3rrv.47 read
    // the chunk; this is the override on top of it).
    if let Some(mode) = params.int("edit/loop_mode") {
        out.set_property("loop_mode", Variant::Int(mode));
    }
    if let Some(begin) = params.int("edit/loop_begin") {
        out.set_property("loop_begin", Variant::Int(begin));
    }
    if let Some(end) = params.int("edit/loop_end") {
        out.set_property("loop_end", Variant::Int(end));
    }

    // -- recorded, not applied: these rewrite samples, which we do not hold --
    if let Some(v) = params.bool("edit/trim") {
        out.set_property("edit_trim", Variant::Bool(v));
    }
    if let Some(v) = params.bool("edit/normalize") {
        out.set_property("edit_normalize", Variant::Bool(v));
    }
    if let Some(v) = params.int("compress/mode") {
        out.set_property("compress_mode", Variant::Int(v));
    }
    out
}

/// [`import_wav`] with the source's `.import` options applied.
pub fn import_wav_with_params(path: &Path, params: &ImportParams) -> EngineResult<Arc<Resource>> {
    let res = import_wav(path)?;
    Ok(Arc::new(apply_wav_params(&res, params)))
}

/// Reads the first loop out of a WAV `smpl` chunk as Godot's
/// `AudioStreamWAV` loop fields (pat-s3rrv.47).
///
/// Returns `(loop_mode, loop_begin, loop_end)` with `loop_mode = 0`
/// (`LOOP_DISABLED`) when the file carries no `smpl` chunk or declares zero
/// loops — the overwhelmingly common case, and the behaviour every existing
/// caller already saw.
///
/// ⚠️ The WAV loop `type` field and Godot's `LoopMode` are NOT the same
/// numbering: WAV uses 0 = forward, 1 = alternating, 2 = backward, while Godot
/// uses 0 = disabled, 1 = forward, 2 = ping-pong, 3 = backward. Copying the raw
/// value across would turn every forward loop into a disabled one — silently,
/// because a disabled loop just sounds like the old unlooped behaviour.
///
/// Godot reads only the FIRST loop (`resource_importer_wav.cpp`); a multi-loop
/// sampler file is not an error, the extra loops are simply not represented.
fn wav_smpl_loop(data: &[u8]) -> (i64, i64, i64) {
    const DISABLED: (i64, i64, i64) = (0, 0, 0);
    let Some((offset, size)) = find_chunk(data, b"smpl") else {
        return DISABLED;
    };
    let chunk = &data[offset..];
    // 36 bytes of header before the first loop struct, then 24 per loop.
    if (size as usize) < 60 || chunk.len() < 60 {
        return DISABLED;
    }
    let le32 = |at: usize| -> u32 {
        u32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
    };
    if le32(28) == 0 {
        // A `smpl` chunk that declares no loops. Present in plenty of files
        // that carry only MIDI unity-note data, so this is not a malformed
        // file — it just has nothing to loop.
        return DISABLED;
    }
    let loop_type = le32(36 + 4);
    let begin = le32(36 + 8) as i64;
    let end = le32(36 + 12) as i64;
    let mode = match loop_type {
        0 => 1, // WAV forward   -> Godot LOOP_FORWARD
        1 => 2, // WAV alternate -> Godot LOOP_PINGPONG
        2 => 3, // WAV backward  -> Godot LOOP_BACKWARD
        // An unknown type still means "this file wants to loop"; forward is
        // the safe reading and matches Godot's own fallback.
        _ => 1,
    };
    (mode, begin, end)
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

    // pat-s3rrv.214: a WOFF file is an SFNT whose tables are individually
    // zlib-compressed. Reconstruct it here so everything downstream — the
    // `ttf_parser` metrics below, and the rasterizer that re-reads the payload —
    // sees an ordinary TTF/OTF and needs to know nothing about WOFF.
    //
    // ⚠️ Detection is by SIGNATURE, not by extension. A `.woff` served or saved
    // under a `.ttf` name is common, and the reverse costs nothing to tolerate;
    // sniffing also means a mislabelled file fails with a real message instead
    // of handing `ttf_parser` compressed bytes and reporting `parsed=false`.
    let is_woff = crate::woff::is_woff1(&bytes) || crate::woff::is_woff2(&bytes);
    let sfnt = if is_woff {
        crate::woff::woff_to_sfnt(&bytes)?
    } else {
        bytes.clone()
    };

    let mut res = Resource::new("FontFile");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("path", Variant::String(path.to_string_lossy().into()));
    res.set_property("size", Variant::Int(16));
    res.set_property("byte_length", Variant::Int(bytes.len() as i64));
    res.set_property("has_font_data", Variant::Bool(!bytes.is_empty()));
    // The on-disk container vs. what the rasterizer actually consumes. These
    // differ only for WOFF, and a consumer that wants the glyph bytes wants
    // `sfnt_byte_length`, not the compressed `byte_length`.
    res.set_property(
        "font_format",
        Variant::String(if is_woff { "woff" } else { "sfnt" }.to_string()),
    );
    res.set_property("sfnt_byte_length", Variant::Int(sfnt.len() as i64));

    // Parse the outline font to validate it and expose real metrics (pat-u3l8c.69).
    // Kept lenient: unparseable data still yields a resource with `parsed=false`
    // rather than an error, so bitmap/placeholder fonts continue to load.
    match ttf_parser::Face::parse(&sfnt, 0) {
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
    res.set_property("data_preview", Variant::Array(preview.into()));

    Ok(Arc::new(res))
}

/// Applies `ResourceImporterDynamicFont`'s options to an imported font
/// (pat-s3rrv.137).
///
/// Godot ground truth: `editor/import/resource_importer_dynamic_font.cpp`
/// `get_import_options`. The option names are **flat** — `hinting`, not
/// `rendering/hinting` — unlike the texture importer's `process/*` keys, so a
/// group prefix invented by analogy would silently match nothing.
///
/// ⚠️ **Four of these default to TRUE**: `disable_embedded_bitmaps`,
/// `allow_system_fallback`, `keep_rounding_remainders` and `compress`. Assuming
/// Godot's usual "bool options start false" flips all four at once, and the
/// result is a font that quietly ignores its embedded bitmap strikes. Two more
/// are non-zero ints: `hinting` and `antialiasing` default to `1`, and
/// `subpixel_positioning` to `4`.
///
/// ⚠️ **This records the options; it does not rasterise differently.** Patina's
/// `import_font` stores metrics and the raw payload — there is no atlas builder
/// to hand an MSDF flag to yet. Writing them onto the resource is what lets the
/// text layer read them later and what makes the sidecar round-trip, and is
/// exactly what `apply_wav_params` does for the sample-rewriting options it
/// likewise cannot perform.
pub fn apply_font_params(res: &Resource, params: &ImportParams) -> Resource {
    let mut out = res.clone();

    // -- Rendering --
    if let Some(v) = params.int("antialiasing") {
        out.set_property("antialiasing", Variant::Int(v));
    }
    if let Some(v) = params.bool("generate_mipmaps") {
        out.set_property("generate_mipmaps", Variant::Bool(v));
    }
    if let Some(v) = params.bool("disable_embedded_bitmaps") {
        out.set_property("disable_embedded_bitmaps", Variant::Bool(v));
    }
    if let Some(v) = params.bool("multichannel_signed_distance_field") {
        out.set_property("multichannel_signed_distance_field", Variant::Bool(v));
    }
    if let Some(v) = params.int("msdf_pixel_range") {
        out.set_property("msdf_pixel_range", Variant::Int(v));
    }
    if let Some(v) = params.int("msdf_size") {
        out.set_property("msdf_size", Variant::Int(v));
    }
    if let Some(v) = params.bool("allow_system_fallback") {
        out.set_property("allow_system_fallback", Variant::Bool(v));
    }
    if let Some(v) = params.bool("force_autohinter") {
        out.set_property("force_autohinter", Variant::Bool(v));
    }
    if let Some(v) = params.bool("modulate_color_glyphs") {
        out.set_property("modulate_color_glyphs", Variant::Bool(v));
    }
    if let Some(v) = params.int("hinting") {
        out.set_property("hinting", Variant::Int(v));
    }
    if let Some(v) = params.int("subpixel_positioning") {
        out.set_property("subpixel_positioning", Variant::Int(v));
    }
    if let Some(v) = params.bool("keep_rounding_remainders") {
        out.set_property("keep_rounding_remainders", Variant::Bool(v));
    }
    // ⚠️ `oversampling` is a FLOAT (`PROPERTY_HINT_RANGE, "0,10,0.1"`), and `0`
    // means "follow the viewport scale", not "no oversampling" — so it must be
    // written through even when zero rather than treated as unset.
    if let Some(v) = params.float("oversampling") {
        out.set_property("oversampling", Variant::Float(v));
    }

    // -- Fallbacks / compress / storage-only options --
    //
    // Passed through as whatever Variant the sidecar parsed them into: these
    // are Arrays and Dictionaries, and re-typing them here would be the only
    // place their shape is decided.
    if let Some(v) = params.bool("compress") {
        out.set_property("compress", Variant::Bool(v));
    }
    for key in [
        "fallbacks",
        "preload",
        "language_support",
        "script_support",
        "opentype_features",
    ] {
        if let Some(v) = params.get(key) {
            out.set_property(key, v.clone());
        }
    }

    out
}

/// [`import_font`] with the source's `.import` options applied
/// (pat-s3rrv.137). Mirrors [`import_wav_with_params`].
pub fn import_font_with_params(path: &Path, params: &ImportParams) -> EngineResult<Arc<Resource>> {
    let res = import_font(path)?;
    Ok(Arc::new(apply_font_params(&res, params)))
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

    /// The `[params]` section — the importer's editable options.
    ///
    /// Godot's ImportDock reads these to show what each option is *currently*
    /// set to and writes edits back here (pat-s3rrv.2). An asset that has
    /// never been imported has no section yet.
    pub fn params(&self) -> Option<&HashMap<String, String>> {
        self.other_sections.get("params")
    }

    /// A single `[params]` entry.
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params()?.get(key).map(|s| s.as_str())
    }

    /// Sets a `[params]` entry, creating the section when the asset has never
    /// been imported.
    pub fn set_param(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.other_sections
            .entry("params".to_string())
            .or_default()
            .insert(key.into(), value.into());
    }

    /// Sets a `[remap]` entry (`importer`, `type`, `uid`, `path`).
    pub fn set_remap(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.remap.insert(key.into(), value.into());
    }

    /// Sets a `[deps]` entry (`source_file`, `dest_files`).
    pub fn set_dep(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.deps.insert(key.into(), value.into());
    }
}

/// A `.import` file's `[params]` section, parsed into typed [`Variant`]s
/// (pat-s3rrv.50).
///
/// The raw section is `HashMap<String, String>`, so a reader could not tell
/// `true` from `"true"` from `1`. Every value here has been through
/// [`crate::loader::parse_variant_value`], the same parser `.tres`/`.tscn`
/// values go through, so `svg/scale=2.0` arrives as a `Float` and
/// `force/mono=true` as a `Bool`.
///
/// Project-wide defaults (`[importer_defaults] <importer>={...}` in
/// `project.godot`) fill in options the asset's own sidecar leaves out — the
/// same precedence Godot uses: per-asset value, else project default, else the
/// importer's built-in default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportParams {
    values: HashMap<String, Variant>,
}

impl ImportParams {
    /// Parses a raw `[params]` map. A value the Variant parser rejects is kept
    /// as a plain `String` rather than dropped, so an unrecognised option is
    /// still visible to whoever knows how to read it.
    pub fn from_raw(raw: &HashMap<String, String>) -> Self {
        let values = raw
            .iter()
            .map(|(k, v)| {
                let parsed = crate::loader::parse_variant_value(v)
                    .unwrap_or_else(|_| Variant::String(v.clone()));
                (k.clone(), parsed)
            })
            .collect();
        Self { values }
    }

    /// The typed `[params]` of an already-parsed sidecar.
    pub fn from_import_file(file: &ImportFile) -> Self {
        match file.params() {
            Some(raw) => Self::from_raw(raw),
            None => Self::default(),
        }
    }

    /// Fills in every key this set does not already carry. Used for the
    /// `importer_defaults` fallback, so a per-asset value always wins.
    pub fn fill_defaults(&mut self, defaults: &HashMap<String, Variant>) {
        for (k, v) in defaults {
            self.values.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }

    /// `true` when no option is set at all — the signal that a load can take
    /// the plain, param-free path.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Number of options carried.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// The raw [`Variant`] for an option.
    pub fn get(&self, key: &str) -> Option<&Variant> {
        self.values.get(key)
    }

    /// Sets an option (used by the editor's import dock and by tests).
    pub fn set(&mut self, key: impl Into<String>, value: Variant) {
        self.values.insert(key.into(), value);
    }

    /// An option as a bool. `Int`/`Float` count as booleans the way Godot's
    /// `Variant` conversion does, so `1` reads as `true`.
    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.values.get(key)? {
            Variant::Bool(b) => Some(*b),
            Variant::Int(i) => Some(*i != 0),
            Variant::Float(f) => Some(*f != 0.0),
            _ => None,
        }
    }

    /// An option as an integer, accepting a `Float` written without a fraction.
    pub fn int(&self, key: &str) -> Option<i64> {
        match self.values.get(key)? {
            Variant::Int(i) => Some(*i),
            Variant::Float(f) => Some(*f as i64),
            Variant::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }

    /// An option as a float, accepting an `Int` (`svg/scale=2` is legal INI).
    pub fn float(&self, key: &str) -> Option<f64> {
        match self.values.get(key)? {
            Variant::Float(f) => Some(*f),
            Variant::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// An option as a string.
    pub fn string(&self, key: &str) -> Option<&str> {
        match self.values.get(key)? {
            Variant::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The first of `keys` that is set, as a float. Godot spells the SVG scale
    /// `svg/scale` on the texture importer and `base_scale` on the SVG one; a
    /// caller should not have to care which importer wrote the sidecar.
    pub fn float_any(&self, keys: &[&str]) -> Option<f64> {
        keys.iter().find_map(|k| self.float(k))
    }
}

/// Reads `[importer_defaults] <importer>={ "option": value, ... }` out of a
/// parsed `project.godot` (pat-s3rrv.50).
///
/// Godot stores project-wide importer defaults as one Dictionary per importer
/// name. Returns an empty map when the section, the importer key, or the
/// dictionary shape is absent — a project without the section is the norm.
pub fn project_importer_defaults(
    project: &crate::project::GodotProjectFile,
    importer: &str,
) -> HashMap<String, Variant> {
    let mut out = HashMap::new();
    let Some(raw) = project
        .sections
        .get("importer_defaults")
        .and_then(|s| s.get(importer))
    else {
        return out;
    };
    if let Ok(Variant::Dictionary(dict)) = crate::loader::parse_variant_value(raw) {
        for (key, value) in dict.to_dict().iter() {
            if let Variant::String(k) = key {
                out.insert(k.clone(), value.clone());
            }
        }
    }
    out
}

/// The effective import options for a source file: its `<source>.import`
/// `[params]`, with `[importer_defaults]` from the enclosing project filling
/// the gaps.
///
/// `None` when there is no sidecar and no project default — the signal to load
/// the file the plain way.
pub fn load_source_params(source_path: &Path) -> Option<ImportParams> {
    let mut sidecar_os = source_path.as_os_str().to_os_string();
    sidecar_os.push(".import");
    let sidecar = PathBuf::from(sidecar_os);
    if !sidecar.exists() {
        return None;
    }
    let file = load_import_file(&sidecar).ok()?;
    let mut params = ImportParams::from_import_file(&file);
    if let Some(importer) = file.importer() {
        if let Some(root) = find_project_root(source_path) {
            if let Ok(text) = std::fs::read_to_string(root.join("project.godot")) {
                if let Ok(project) = crate::project::GodotProjectFile::parse(&text) {
                    params.fill_defaults(&project_importer_defaults(&project, importer));
                }
            }
        }
    }
    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

/// Formats a parsed value back into an INI literal.
///
/// [`parse_import_file`] strips the quotes off string values, so the writer has
/// to decide which ones Godot would have written bare. Anything that reads as a
/// Godot literal — a bool, `null`, a number, an array/dictionary, a `&`-prefixed
/// StringName, or a constructor call like `Vector2(1, 2)` — is emitted bare;
/// everything else is quoted. `res://` and `uid://` values are strings in
/// Godot's own output, so they take the quoted branch.
fn format_ini_value(value: &str) -> String {
    let v = value.trim();
    let looks_like_ctor = match v.split_once('(') {
        Some((head, _)) => {
            v.ends_with(')')
                && !head.is_empty()
                && head
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    };
    let bare = matches!(v, "true" | "false" | "null")
        || v.parse::<f64>().is_ok()
        || v.starts_with('[')
        || v.starts_with('{')
        || v.starts_with('&')
        || looks_like_ctor;
    if bare {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Orders a section's keys: the `lead` names first (when present), then the
/// rest alphabetically.
///
/// `ImportFile` stores sections in a `HashMap`, whose iteration order varies
/// per process. Without this a rewrite would shuffle every line and each save
/// would show up as a whole-file diff.
fn ordered_keys<'a>(map: &'a HashMap<String, String>, lead: &[&str]) -> Vec<&'a String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut out: Vec<&String> = Vec::with_capacity(keys.len());
    for name in lead {
        if let Some(k) = keys.iter().find(|k| k.as_str() == *name) {
            out.push(k);
        }
    }
    for k in keys {
        if !lead.contains(&k.as_str()) {
            out.push(k);
        }
    }
    out
}

/// Re-emits an [`ImportFile`] as a Godot `.import` sidecar.
///
/// Round-trips through [`parse_import_file`]: `[remap]` leads with its identity
/// keys, then `[deps]`, then the remaining sections alphabetically (so
/// `[params]` lands last, as Godot writes it).
pub fn serialize_import_file(file: &ImportFile) -> String {
    let mut out = String::from("[remap]\n\n");
    for key in ordered_keys(&file.remap, &["importer", "type", "uid", "path"]) {
        let value = format_ini_value(&file.remap[key]);
        out.push_str(&format!("{key}={value}\n"));
    }

    if !file.deps.is_empty() {
        out.push_str("\n[deps]\n\n");
        for key in ordered_keys(&file.deps, &["source_file", "dest_files"]) {
            let value = format_ini_value(&file.deps[key]);
            out.push_str(&format!("{key}={value}\n"));
        }
    }

    let mut sections: Vec<&String> = file.other_sections.keys().collect();
    sections.sort();
    for section in sections {
        let entries = &file.other_sections[section];
        if entries.is_empty() {
            continue;
        }
        out.push_str(&format!("\n[{section}]\n\n"));
        for key in ordered_keys(entries, &[]) {
            let value = format_ini_value(&entries[key]);
            out.push_str(&format!("{key}={value}\n"));
        }
    }
    out
}

/// Writes an `.import` sidecar to disk, creating parent directories.
pub fn save_import_file(path: &Path, file: &ImportFile) -> EngineResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(EngineError::Io)?;
        }
    }
    std::fs::write(path, serialize_import_file(file)).map_err(EngineError::Io)
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

/// A loader that also receives the source's `.import` options (pat-s3rrv.50).
///
/// Kept separate from [`LoaderFn`] rather than widening it: every existing
/// `register` caller and every plain loader (`.tres`, `.gdshader`, …) has no
/// use for import options, and changing the public signature would break them
/// all for the four formats that do.
pub type LoaderWithParamsFn = fn(&Path, &ImportParams) -> EngineResult<Arc<Resource>>;

/// Registry that maps file extensions to loader functions.
///
/// Call [`register`](ResourceFormatLoader::register) to add extension mappings,
/// then [`load_resource`](ResourceFormatLoader::load_resource) to dispatch.
#[derive(Default)]
pub struct ResourceFormatLoader {
    loaders: HashMap<String, LoaderFn>,
    /// Loaders that honour the source's `.import` `[params]`. Consulted first
    /// when a sidecar carries options; `loaders` is the fallback and stays the
    /// only path for formats that have no import options.
    params_loaders: HashMap<String, LoaderWithParamsFn>,
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
        // ⚠️ Registered BOTH ways on purpose. `register_with_params` handles a
        // font that has a `.import` sidecar; the plain `register` is still the
        // path for a bare `.ttf` with no sidecar, which is the common case
        // (pat-s3rrv.137).
        rfl.register(".ttf", import_font);
        rfl.register(".otf", import_font);
        // pat-s3rrv.214. `.woff2` is registered DELIBERATELY even though this
        // build cannot decode it: the format needs Brotli and no decoder exists
        // here, so `woff_to_sfnt` returns a specific "convert the font" error.
        // Leaving the extension unregistered instead would surface as a generic
        // "no importer", and the Label would silently draw the built-in fallback
        // — a wrong font on screen and no clue why.
        rfl.register(".woff", import_font);
        rfl.register(".woff2", import_font);
        rfl.register_with_params(".ttf", import_font_with_params);
        rfl.register_with_params(".otf", import_font_with_params);
        rfl.register(".tres", load_text_or_binary);
        rfl.register(".tscn", load_text_or_binary);
        rfl.register(".res", load_text_or_binary);
        rfl.register(".scn", load_text_or_binary);
        rfl.register(".vs", vs_loader);
        rfl.register(".gdshader", load_gdshader);
        rfl.register(".gdshaderinc", load_gdshader);
        // pat-s3rrv.51 — source-asset extensions whose decoders already existed
        // and compiled with ZERO consumers. Godot resolves every one of these
        // through a registered loader (core/io/image_loader.cpp,
        // modules/gltf/), so `load()`, the editor and the renderer agree.
        rfl.register(".tga", import_tga);
        rfl.register(".bmp", import_bmp);
        rfl.register(".hdr", import_hdr);
        rfl.register(".gltf", import_gltf_asset);
        rfl.register(".glb", import_gltf_asset);
        // The formats Godot gives import options to (pat-s3rrv.50). Each still
        // has a plain loader above, used when the asset has no sidecar.
        rfl.register_with_params(".png", import_image_with_params);
        rfl.register_with_params(".webp", import_webp_with_params);
        rfl.register_with_params(".svg", crate::image_svg::import_svg_with_params);
        rfl.register_with_params(".wav", import_wav_with_params);
        // Layered and volume artifacts (pat-s3rrv.48). Godot's layered texture
        // importer writes one of these per mode; all four are `GSTL` files that
        // only their extension tells apart.
        rfl.register(".ctexarray", crate::ctex::load_ctexarray_file);
        rfl.register(".ccube", crate::ctex::load_ccube_file);
        rfl.register(".ccubearray", crate::ctex::load_ccubearray_file);
        rfl.register(".ctex3d", crate::ctex::load_ctex3d_file);
        rfl
    }

    /// Registers a loader for the given extension (including the dot, e.g. `".png"`).
    pub fn register(&mut self, extension: &str, loader: LoaderFn) {
        self.loaders.insert(extension.to_lowercase(), loader);
    }

    /// Registers a loader that honours `.import` options for the extension.
    pub fn register_with_params(&mut self, extension: &str, loader: LoaderWithParamsFn) {
        self.params_loaders.insert(extension.to_lowercase(), loader);
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

    /// Loads a resource by dispatching to the registered loader for its
    /// extension, applying the source's `.import` options when it has any.
    ///
    /// Godot persists each asset's importer settings in its `.import`
    /// `[params]` section and the importer applies them when producing the
    /// artifact. Patina parsed that section and then dropped it — nothing read
    /// it, so `svg/scale`, `force/mono`, `process/size_limit` and the rest were
    /// inert (pat-s3rrv.50). Looking the sidecar up here means every consumer
    /// of the registry gets the configured asset, not just the ones that
    /// remember to ask.
    pub fn load_resource(&self, path: &Path) -> EngineResult<Arc<Resource>> {
        match load_source_params(path) {
            Some(params) => self.load_resource_with_params(path, &params),
            None => self.load_plain(path),
        }
    }

    /// Loads with an explicit option set, bypassing the sidecar lookup — the
    /// entry point for a caller that already holds the params (the editor's
    /// import dock previewing an unsaved change, for one).
    pub fn load_resource_with_params(
        &self,
        path: &Path,
        params: &ImportParams,
    ) -> EngineResult<Arc<Resource>> {
        let ext = Self::extension_of(path);
        match self.params_loaders.get(&ext) {
            Some(loader) => loader(path, params),
            // A format with options but no params-aware loader still loads;
            // the options simply do not apply to it.
            None => self.load_plain(path),
        }
    }

    /// The option-free load path.
    fn load_plain(&self, path: &Path) -> EngineResult<Arc<Resource>> {
        let ext = Self::extension_of(path);
        let loader = self.loaders.get(&ext).ok_or_else(|| {
            EngineError::NotFound(format!("no loader registered for extension '{ext}'"))
        })?;
        loader(path)
    }

    /// `".png"`-style lowercase extension, empty when the path has none.
    fn extension_of(path: &Path) -> String {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default()
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
                match &a.to_vec()[0] {
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
        assert!(rfl.can_load(".tga"));
        assert!(rfl.can_load(".bmp"));
        assert!(rfl.can_load(".hdr"));
        assert!(rfl.can_load(".gltf"));
        assert!(rfl.can_load(".glb"));

        // ⚠️ This bare count has now gone stale three times (12 while 14 were
        // registered, then 19 while 25 are), and each time it sat red at HEAD
        // and failed an unrelated bead's verify — `--lib loader` matches this
        // test's *name*, so anyone touching `loader.rs` inherits the failure.
        //
        // A count alone cannot say WHICH extension went missing, and the
        // `format_loader_covers_source_asset_extensions` the old note pointed
        // at was never written. So the list is asserted here instead: a
        // registration added or dropped now names itself in the diff.
        let mut extensions: Vec<&str> = EXPECTED_DEFAULT_EXTENSIONS.to_vec();
        extensions.sort_unstable();
        for ext in &extensions {
            assert!(rfl.can_load(ext), "{ext} must have a default loader");
        }
        assert_eq!(
            rfl.extension_count(),
            extensions.len(),
            "`with_defaults` registers {} loaders but EXPECTED_DEFAULT_EXTENSIONS lists {} — \
             the two have to move together",
            rfl.extension_count(),
            extensions.len()
        );
    }

    /// Every extension `ResourceFormatLoader::with_defaults` registers.
    ///
    /// Kept beside the test rather than derived from the loader, so that
    /// losing a registration is a failure instead of a silently smaller list.
    const EXPECTED_DEFAULT_EXTENSIONS: &[&str] = &[
        ".bmp",
        ".ccube",
        ".ccubearray",
        ".ctex",
        ".ctex3d",
        ".ctexarray",
        ".gdshader",
        ".gdshaderinc",
        ".glb",
        ".gltf",
        ".hdr",
        ".otf",
        ".png",
        ".res",
        ".scn",
        ".svg",
        ".tga",
        ".tres",
        ".tscn",
        ".ttf",
        ".vs",
        ".wav",
        ".webp",
        ".woff",
        ".woff2",
    ];

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
                match &a.to_vec()[0] {
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

    // -- .import writing (pat-s3rrv.2) ---------------------------------------

    /// A real Godot-authored sidecar must survive parse -> serialize -> parse
    /// with every section intact. Nothing could write one of these before, so
    /// the round-trip is what proves the editor won't corrupt a project it
    /// didn't create.
    #[test]
    fn import_file_round_trips_through_the_writer() {
        let original = "\
[remap]

importer=\"texture\"
type=\"CompressedTexture2D\"
uid=\"uid://bx7q\"
path=\"res://.godot/imported/hero.png-9f2.ctex\"

[deps]

source_file=\"res://hero.png\"
dest_files=[\"res://.godot/imported/hero.png-9f2.ctex\"]

[params]

compress/mode=0
mipmaps/generate=true
process/hdr_as_srgb=false
svg/scale=1.0
";
        let parsed = parse_import_file(original).expect("fixture parses");
        let round = parse_import_file(&serialize_import_file(&parsed)).expect("re-parses");

        assert_eq!(round.importer(), Some("texture"));
        assert_eq!(round.resource_type(), Some("CompressedTexture2D"));
        assert_eq!(round.uid(), Some("uid://bx7q"));
        assert_eq!(
            round.import_path(),
            Some("res://.godot/imported/hero.png-9f2.ctex")
        );
        assert_eq!(round.source_file(), Some("res://hero.png"));
        assert_eq!(round.param("compress/mode"), Some("0"));
        assert_eq!(round.param("mipmaps/generate"), Some("true"));
        assert_eq!(round.param("svg/scale"), Some("1.0"));
    }

    /// Godot writes paths and uids quoted but bools/numbers/arrays bare. Emitting
    /// a bool as `"false"` would make Godot read the option as a *string*, which
    /// is how a sidecar silently stops matching its own importer.
    #[test]
    fn writer_quotes_strings_and_leaves_literals_bare() {
        let mut f = ImportFile::default();
        f.set_remap("importer", "texture");
        f.set_remap("path", "res://.godot/imported/a.ctex");
        f.set_param("mipmaps", "false");
        f.set_param("max_size", "0");
        f.set_param("lossy_quality", "0.7");
        f.set_param("dest", "[\"res://a.ctex\"]");
        f.set_param("label", "My Texture");

        let out = serialize_import_file(&f);
        assert!(
            out.contains("importer=\"texture\""),
            "strings quoted:\n{out}"
        );
        assert!(
            out.contains("path=\"res://.godot/imported/a.ctex\""),
            "res:// paths stay quoted:\n{out}"
        );
        assert!(out.contains("mipmaps=false"), "bools bare:\n{out}");
        assert!(out.contains("max_size=0"), "ints bare:\n{out}");
        assert!(out.contains("lossy_quality=0.7"), "floats bare:\n{out}");
        assert!(
            out.contains("dest=[\"res://a.ctex\"]"),
            "arrays bare:\n{out}"
        );
        assert!(
            out.contains("label=\"My Texture\""),
            "free text quoted:\n{out}"
        );
    }

    /// `[params]` is created on demand: an asset the dock touches before it has
    /// ever been imported still gets a well-formed sidecar.
    #[test]
    fn set_param_creates_the_section_and_writes_to_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sidecar = dir.path().join("hero.png.import");

        let mut f = ImportFile::default();
        assert!(f.params().is_none(), "no [params] before the first edit");
        f.set_remap("importer", "texture");
        f.set_param("mipmaps", "false");
        save_import_file(&sidecar, &f).expect("writes");

        let text = std::fs::read_to_string(&sidecar).expect("sidecar on disk");
        assert!(text.contains("[params]"), "section emitted:\n{text}");
        assert_eq!(
            load_import_file(&sidecar)
                .expect("reloads")
                .param("mipmaps"),
            Some("false"),
        );
    }
}
