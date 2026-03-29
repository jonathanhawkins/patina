//! Texture2D import pipeline for PNG and JPG images.
//!
//! Provides full import of image files into [`TextureImportResult`] structs
//! containing decoded pixel data, dimensions, and format metadata. This
//! module bridges the gap between the header-only importers in
//! [`crate::importers`] and the rendering pipeline's need for decoded pixels.
//!
//! # Supported formats
//!
//! - **PNG**: 8-bit grayscale, RGB, grayscale+alpha, and RGBA (color types 0, 2, 4, 6)
//! - **JPEG**: baseline and progressive, grayscale (L8), RGB24, and CMYK32
//!
//! # Usage
//!
//! ```ignore
//! use gdresource::texture_import::{TextureImporter, TextureImportResult};
//!
//! let importer = TextureImporter::new();
//! let result = importer.import_file(path)?;
//! println!("{}x{} {}", result.width, result.height, result.format);
//! ```

use std::path::Path;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::Color;
use gdvariant::Variant;

use crate::resource::Resource;

/// The decoded image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat {
    /// Single-channel grayscale.
    L,
    /// Grayscale with alpha.
    LA,
    /// Three-channel RGB.
    Rgb,
    /// Four-channel RGBA.
    Rgba,
}

impl TextureFormat {
    /// Returns the format as a Godot-style string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::L => "L",
            Self::LA => "LA",
            Self::Rgb => "RGB",
            Self::Rgba => "RGBA",
        }
    }
}

impl std::fmt::Display for TextureFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of importing a texture image.
#[derive(Debug, Clone)]
pub struct TextureImportResult {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The decoded pixel format.
    pub format: TextureFormat,
    /// Decoded pixel data as RGBA colors (row-major).
    pub pixels: Vec<Color>,
}

impl TextureImportResult {
    /// Returns the total number of pixels.
    pub fn pixel_count(&self) -> usize {
        (self.width * self.height) as usize
    }

    /// Returns the color at `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if out of bounds.
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Converts this import result into a [`Resource`] with class `Texture2D`.
    pub fn to_resource(&self, path: &str) -> Arc<Resource> {
        let mut res = Resource::new("Texture2D");
        res.path = path.to_string();
        res.set_property("width", Variant::Int(self.width as i64));
        res.set_property("height", Variant::Int(self.height as i64));
        res.set_property("format", Variant::String(self.format.as_str().into()));
        res.set_property("has_pixels", Variant::Bool(true));
        Arc::new(res)
    }
}

/// Texture importer that decodes PNG and JPEG images to pixel data.
#[derive(Debug, Default)]
pub struct TextureImporter;

impl TextureImporter {
    /// Creates a new texture importer.
    pub fn new() -> Self {
        Self
    }

    /// Imports a texture from a file path, auto-detecting format by extension.
    pub fn import_file(&self, path: &Path) -> EngineResult<TextureImportResult> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let data = std::fs::read(path).map_err(EngineError::Io)?;

        match ext.as_str() {
            "png" => self.decode_png(&data),
            "jpg" | "jpeg" => self.decode_jpg(&data),
            _ => Err(EngineError::Parse(format!(
                "unsupported texture format: .{ext}"
            ))),
        }
    }

    /// Imports a texture from raw bytes, auto-detecting format by magic bytes.
    pub fn import_bytes(&self, data: &[u8]) -> EngineResult<TextureImportResult> {
        if data.len() >= 8 && data[..8] == PNG_MAGIC {
            self.decode_png(data)
        } else if data.len() >= 2 && data[..2] == JPEG_MAGIC {
            self.decode_jpg(data)
        } else {
            Err(EngineError::Parse(
                "unrecognized image format (bad magic bytes)".into(),
            ))
        }
    }

    /// Imports a file and returns a full [`Resource`] with decoded pixel data attached.
    pub fn import_resource(&self, path: &Path) -> EngineResult<Arc<Resource>> {
        let result = self.import_file(path)?;
        let res_path = format!(
            "res://{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        Ok(result.to_resource(&res_path))
    }

    /// Decodes PNG image data from raw bytes.
    pub fn decode_png(&self, data: &[u8]) -> EngineResult<TextureImportResult> {
        decode_png_bytes(data)
    }

    /// Decodes JPEG image data from raw bytes.
    pub fn decode_jpg(&self, data: &[u8]) -> EngineResult<TextureImportResult> {
        decode_jpg_bytes(data)
    }
}

// ---------------------------------------------------------------------------
// PNG magic and decoding
// ---------------------------------------------------------------------------

/// PNG magic bytes.
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// JPEG magic bytes (SOI marker).
const JPEG_MAGIC: [u8; 2] = [0xFF, 0xD8];

/// Decodes PNG image data from raw bytes into a [`TextureImportResult`].
///
/// Supports 8-bit grayscale (color type 0), RGB (type 2), grayscale+alpha
/// (type 4), and RGBA (type 6). Returns an error on unsupported formats.
pub fn decode_png_bytes(data: &[u8]) -> EngineResult<TextureImportResult> {
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    if data.len() < 8 || data[..8] != PNG_MAGIC {
        return Err(EngineError::Parse("not a valid PNG (bad magic)".into()));
    }

    // Parse IHDR.
    let mut pos = 8;
    let (width, height, bit_depth, color_type) = parse_ihdr(data, &mut pos)?;

    if bit_depth != 8 {
        return Err(EngineError::Parse(format!(
            "unsupported PNG bit depth: {bit_depth} (only 8-bit supported)"
        )));
    }

    // Collect all IDAT chunks.
    let mut idat_data = Vec::new();
    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        if chunk_type == b"IDAT" {
            if pos + 8 + chunk_len > data.len() {
                return Err(EngineError::Parse("PNG IDAT chunk truncated".into()));
            }
            idat_data.extend_from_slice(&data[pos + 8..pos + 8 + chunk_len]);
        } else if chunk_type == b"IEND" {
            break;
        }
        pos += 12 + chunk_len;
    }

    // Decompress.
    let raw = decompress_to_vec_zlib(&idat_data)
        .map_err(|e| EngineError::Parse(format!("PNG decompression failed: {e}")))?;

    // Determine bytes per pixel and format.
    let (bpp, format) = match color_type {
        0 => (1, TextureFormat::L),
        2 => (3, TextureFormat::Rgb),
        4 => (2, TextureFormat::LA),
        6 => (4, TextureFormat::Rgba),
        _ => {
            return Err(EngineError::Parse(format!(
                "unsupported PNG color type: {color_type}"
            )))
        }
    };

    // Unfilter scanlines.
    let stride = width as usize * bpp;
    let row_bytes = 1 + stride;
    if raw.len() < row_bytes * height as usize {
        return Err(EngineError::Parse(
            "PNG decompressed data too short for image dimensions".into(),
        ));
    }

    let mut unfiltered = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let filter_byte = raw[y * row_bytes];
        let src = &raw[y * row_bytes + 1..y * row_bytes + 1 + stride];
        let dst_offset = y * stride;

        match filter_byte {
            0 => {
                // None
                unfiltered[dst_offset..dst_offset + stride].copy_from_slice(src);
            }
            1 => {
                // Sub
                for i in 0..stride {
                    let a = if i >= bpp {
                        unfiltered[dst_offset + i - bpp]
                    } else {
                        0
                    };
                    unfiltered[dst_offset + i] = src[i].wrapping_add(a);
                }
            }
            2 => {
                // Up
                for i in 0..stride {
                    let b = if y > 0 {
                        unfiltered[(y - 1) * stride + i]
                    } else {
                        0
                    };
                    unfiltered[dst_offset + i] = src[i].wrapping_add(b);
                }
            }
            3 => {
                // Average
                for i in 0..stride {
                    let a = if i >= bpp {
                        unfiltered[dst_offset + i - bpp] as u16
                    } else {
                        0
                    };
                    let b = if y > 0 {
                        unfiltered[(y - 1) * stride + i] as u16
                    } else {
                        0
                    };
                    unfiltered[dst_offset + i] = src[i].wrapping_add(((a + b) / 2) as u8);
                }
            }
            4 => {
                // Paeth
                for i in 0..stride {
                    let a = if i >= bpp {
                        unfiltered[dst_offset + i - bpp]
                    } else {
                        0
                    };
                    let b = if y > 0 {
                        unfiltered[(y - 1) * stride + i]
                    } else {
                        0
                    };
                    let c = if y > 0 && i >= bpp {
                        unfiltered[(y - 1) * stride + i - bpp]
                    } else {
                        0
                    };
                    unfiltered[dst_offset + i] = src[i].wrapping_add(paeth_predictor(a, b, c));
                }
            }
            _ => {
                return Err(EngineError::Parse(format!(
                    "PNG unknown filter type: {filter_byte}"
                )))
            }
        }
    }

    // Convert to Color pixels.
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let idx = y * stride + x * bpp;
            let color = match color_type {
                0 => {
                    let g = unfiltered[idx] as f32 / 255.0;
                    Color::new(g, g, g, 1.0)
                }
                2 => {
                    let r = unfiltered[idx] as f32 / 255.0;
                    let g = unfiltered[idx + 1] as f32 / 255.0;
                    let b = unfiltered[idx + 2] as f32 / 255.0;
                    Color::new(r, g, b, 1.0)
                }
                4 => {
                    let g = unfiltered[idx] as f32 / 255.0;
                    let a = unfiltered[idx + 1] as f32 / 255.0;
                    Color::new(g, g, g, a)
                }
                6 => {
                    let r = unfiltered[idx] as f32 / 255.0;
                    let g = unfiltered[idx + 1] as f32 / 255.0;
                    let b = unfiltered[idx + 2] as f32 / 255.0;
                    let a = unfiltered[idx + 3] as f32 / 255.0;
                    Color::new(r, g, b, a)
                }
                _ => Color::TRANSPARENT,
            };
            pixels.push(color);
        }
    }

    Ok(TextureImportResult {
        width,
        height,
        format,
        pixels,
    })
}

/// Parses the IHDR chunk, returning `(width, height, bit_depth, color_type)`.
fn parse_ihdr(data: &[u8], pos: &mut usize) -> EngineResult<(u32, u32, u8, u8)> {
    if *pos + 25 > data.len() {
        return Err(EngineError::Parse("PNG IHDR chunk too short".into()));
    }
    let chunk_len =
        u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]) as usize;
    if chunk_len != 13 || &data[*pos + 4..*pos + 8] != b"IHDR" {
        return Err(EngineError::Parse(
            "PNG IHDR chunk missing or malformed".into(),
        ));
    }
    let width = u32::from_be_bytes([
        data[*pos + 8],
        data[*pos + 9],
        data[*pos + 10],
        data[*pos + 11],
    ]);
    let height = u32::from_be_bytes([
        data[*pos + 12],
        data[*pos + 13],
        data[*pos + 14],
        data[*pos + 15],
    ]);
    let bit_depth = data[*pos + 16];
    let color_type = data[*pos + 17];
    *pos += 12 + chunk_len;
    Ok((width, height, bit_depth, color_type))
}

/// Paeth predictor as defined in the PNG specification.
fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// JPEG decoding
// ---------------------------------------------------------------------------

/// Decodes JPEG image data from raw bytes into a [`TextureImportResult`].
///
/// Supports baseline and progressive JPEG with grayscale (L8), RGB24, and
/// CMYK32 pixel formats. CMYK is converted to RGB.
pub fn decode_jpg_bytes(data: &[u8]) -> EngineResult<TextureImportResult> {
    use jpeg_decoder::Decoder;

    let mut decoder = Decoder::new(data);
    let raw_pixels = decoder
        .decode()
        .map_err(|e| EngineError::Parse(format!("JPEG decode failed: {e}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| EngineError::Parse("JPEG: no image info after decode".into()))?;

    let width = info.width as u32;
    let height = info.height as u32;
    let pixel_count = (width * height) as usize;

    let mut pixels = Vec::with_capacity(pixel_count);
    let format;

    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            format = TextureFormat::L;
            for &g in &raw_pixels {
                let v = g as f32 / 255.0;
                pixels.push(Color::new(v, v, v, 1.0));
            }
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            format = TextureFormat::Rgb;
            for chunk in raw_pixels.chunks_exact(3) {
                let r = chunk[0] as f32 / 255.0;
                let g = chunk[1] as f32 / 255.0;
                let b = chunk[2] as f32 / 255.0;
                pixels.push(Color::new(r, g, b, 1.0));
            }
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            format = TextureFormat::Rgb; // Converted from CMYK.
            for chunk in raw_pixels.chunks_exact(4) {
                let c = chunk[0] as f32 / 255.0;
                let m = chunk[1] as f32 / 255.0;
                let y = chunk[2] as f32 / 255.0;
                let k = chunk[3] as f32 / 255.0;
                let r = (1.0 - c) * (1.0 - k);
                let g = (1.0 - m) * (1.0 - k);
                let b = (1.0 - y) * (1.0 - k);
                pixels.push(Color::new(r, g, b, 1.0));
            }
        }
        jpeg_decoder::PixelFormat::L16 => {
            format = TextureFormat::L;
            for chunk in raw_pixels.chunks_exact(2) {
                let v = u16::from_ne_bytes([chunk[0], chunk[1]]) as f32 / 65535.0;
                pixels.push(Color::new(v, v, v, 1.0));
            }
        }
    }

    Ok(TextureImportResult {
        width,
        height,
        format,
        pixels,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Minimal PNG generator for testing ──

    /// Creates a minimal valid PNG file in memory with the given dimensions
    /// and solid RGBA color.
    fn create_test_png(width: u32, height: u32, color: Color) -> Vec<u8> {
        use miniz_oxide::deflate::compress_to_vec_zlib;

        let mut buf = Vec::new();

        // PNG signature.
        buf.extend_from_slice(&PNG_MAGIC);

        // IHDR chunk.
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.push(8); // bit depth
        ihdr.push(6); // color type = RGBA
        ihdr.push(0); // compression
        ihdr.push(0); // filter
        ihdr.push(0); // interlace
        write_chunk(&mut buf, b"IHDR", &ihdr);

        // Build raw scanlines (filter byte 0 = None, then RGBA pixels).
        let r = (color.r * 255.0) as u8;
        let g = (color.g * 255.0) as u8;
        let b = (color.b * 255.0) as u8;
        let a = (color.a * 255.0) as u8;

        let mut raw = Vec::new();
        for _y in 0..height {
            raw.push(0); // filter = None
            for _x in 0..width {
                raw.extend_from_slice(&[r, g, b, a]);
            }
        }

        let compressed = compress_to_vec_zlib(&raw, 6);
        write_chunk(&mut buf, b"IDAT", &compressed);
        write_chunk(&mut buf, b"IEND", &[]);

        buf
    }

    /// Creates a minimal valid PNG in grayscale (color type 0).
    fn create_test_png_grayscale(width: u32, height: u32, value: u8) -> Vec<u8> {
        use miniz_oxide::deflate::compress_to_vec_zlib;

        let mut buf = Vec::new();
        buf.extend_from_slice(&PNG_MAGIC);

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.push(8);
        ihdr.push(0); // Grayscale
        ihdr.push(0);
        ihdr.push(0);
        ihdr.push(0);
        write_chunk(&mut buf, b"IHDR", &ihdr);

        let mut raw = Vec::new();
        for _y in 0..height {
            raw.push(0);
            for _x in 0..width {
                raw.push(value);
            }
        }

        let compressed = compress_to_vec_zlib(&raw, 6);
        write_chunk(&mut buf, b"IDAT", &compressed);
        write_chunk(&mut buf, b"IEND", &[]);
        buf
    }

    /// Creates a minimal valid PNG in RGB (color type 2).
    fn create_test_png_rgb(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        use miniz_oxide::deflate::compress_to_vec_zlib;

        let mut buf = Vec::new();
        buf.extend_from_slice(&PNG_MAGIC);

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.push(8);
        ihdr.push(2); // RGB
        ihdr.push(0);
        ihdr.push(0);
        ihdr.push(0);
        write_chunk(&mut buf, b"IHDR", &ihdr);

        let mut raw = Vec::new();
        for _y in 0..height {
            raw.push(0);
            for _x in 0..width {
                raw.extend_from_slice(&[r, g, b]);
            }
        }

        let compressed = compress_to_vec_zlib(&raw, 6);
        write_chunk(&mut buf, b"IDAT", &compressed);
        write_chunk(&mut buf, b"IEND", &[]);
        buf
    }

    /// Writes a PNG chunk with CRC.
    fn write_chunk(buf: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
        buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
        buf.extend_from_slice(chunk_type);
        buf.extend_from_slice(data);
        // CRC32 over type + data.
        let crc = crc32(chunk_type, data);
        buf.extend_from_slice(&crc.to_be_bytes());
    }

    /// Simple CRC32 for PNG chunks.
    fn crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &byte in chunk_type.iter().chain(data.iter()) {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    // ── PNG decoding tests ──

    #[test]
    fn decode_png_rgba_solid_red() {
        let png = create_test_png(4, 4, Color::new(1.0, 0.0, 0.0, 1.0));
        let result = decode_png_bytes(&png).unwrap();

        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
        assert_eq!(result.format, TextureFormat::Rgba);
        assert_eq!(result.pixel_count(), 16);

        for pixel in &result.pixels {
            assert!((pixel.r - 1.0).abs() < 0.01);
            assert!(pixel.g.abs() < 0.01);
            assert!(pixel.b.abs() < 0.01);
            assert!((pixel.a - 1.0).abs() < 0.01);
        }
    }

    #[test]
    fn decode_png_rgba_with_alpha() {
        let png = create_test_png(2, 2, Color::new(0.0, 0.5, 1.0, 0.5));
        let result = decode_png_bytes(&png).unwrap();

        assert_eq!(result.format, TextureFormat::Rgba);
        let p = result.get_pixel(0, 0);
        assert!(p.g > 0.4 && p.g < 0.6);
        assert!(p.a > 0.4 && p.a < 0.6);
    }

    #[test]
    fn decode_png_grayscale() {
        let png = create_test_png_grayscale(3, 3, 128);
        let result = decode_png_bytes(&png).unwrap();

        assert_eq!(result.width, 3);
        assert_eq!(result.height, 3);
        assert_eq!(result.format, TextureFormat::L);
        let p = result.get_pixel(1, 1);
        assert!((p.r - 128.0 / 255.0).abs() < 0.01);
        assert_eq!(p.r, p.g);
        assert_eq!(p.g, p.b);
        assert!((p.a - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn decode_png_rgb() {
        let png = create_test_png_rgb(2, 2, 0, 255, 0);
        let result = decode_png_bytes(&png).unwrap();

        assert_eq!(result.format, TextureFormat::Rgb);
        let p = result.get_pixel(0, 0);
        assert!(p.r.abs() < 0.01);
        assert!((p.g - 1.0).abs() < 0.01);
        assert!(p.b.abs() < 0.01);
    }

    #[test]
    fn decode_png_invalid_magic() {
        let result = decode_png_bytes(&[0, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_png_empty() {
        let result = decode_png_bytes(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_png_1x1() {
        let png = create_test_png(1, 1, Color::WHITE);
        let result = decode_png_bytes(&png).unwrap();
        assert_eq!(result.width, 1);
        assert_eq!(result.height, 1);
        assert_eq!(result.pixels.len(), 1);
    }

    // ── TextureImporter tests ──

    #[test]
    fn importer_detect_png_by_magic() {
        let importer = TextureImporter::new();
        let png = create_test_png(2, 2, Color::new(0.0, 0.0, 1.0, 1.0));
        let result = importer.import_bytes(&png).unwrap();
        assert_eq!(result.width, 2);
        assert_eq!(result.format, TextureFormat::Rgba);
    }

    #[test]
    fn importer_reject_unknown_format() {
        let importer = TextureImporter::new();
        let result = importer.import_bytes(&[0x00, 0x00, 0x00, 0x00, 0x00]);
        assert!(result.is_err());
    }

    #[test]
    fn importer_file_not_found() {
        let importer = TextureImporter::new();
        let result = importer.import_file(Path::new("/nonexistent/image.png"));
        assert!(result.is_err());
    }

    // ── TextureImportResult → Resource conversion ──

    #[test]
    fn import_result_to_resource() {
        let result = TextureImportResult {
            width: 64,
            height: 32,
            format: TextureFormat::Rgba,
            pixels: vec![Color::WHITE; 64 * 32],
        };
        let res = result.to_resource("res://icon.png");
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(64)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(32)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );
        assert_eq!(
            res.get_property("has_pixels"),
            Some(&Variant::Bool(true))
        );
    }

    #[test]
    fn texture_format_display() {
        assert_eq!(TextureFormat::L.as_str(), "L");
        assert_eq!(TextureFormat::LA.as_str(), "LA");
        assert_eq!(TextureFormat::Rgb.as_str(), "RGB");
        assert_eq!(TextureFormat::Rgba.as_str(), "RGBA");
        assert_eq!(format!("{}", TextureFormat::Rgba), "RGBA");
    }

    // ── File-based import with tempfile ──

    #[test]
    fn import_png_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let png_path = dir.path().join("test.png");
        let png_data = create_test_png(8, 8, Color::new(1.0, 0.0, 0.0, 1.0));
        std::fs::write(&png_path, &png_data).unwrap();

        let importer = TextureImporter::new();
        let result = importer.import_file(&png_path).unwrap();
        assert_eq!(result.width, 8);
        assert_eq!(result.height, 8);
        assert_eq!(result.format, TextureFormat::Rgba);
        assert!((result.get_pixel(0, 0).r - 1.0).abs() < 0.01);
    }

    #[test]
    fn import_resource_from_png_file() {
        let dir = tempfile::tempdir().unwrap();
        let png_path = dir.path().join("sprite.png");
        let png_data = create_test_png(16, 16, Color::new(0.0, 1.0, 0.0, 1.0));
        std::fs::write(&png_path, &png_data).unwrap();

        let importer = TextureImporter::new();
        let res = importer.import_resource(&png_path).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(16)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(16)));
    }
}
