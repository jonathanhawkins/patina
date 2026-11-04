//! Texture types for the software renderer.
//!
//! Provides a simple CPU-side texture representation used for
//! `DrawTextureRect` commands and testing, plus PNG decoding.

use gdcore::math::Color;

/// A CPU-side 2D texture stored as an array of RGBA pixels.
#[derive(Debug, Clone)]
pub struct Texture2D {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data in row-major order.
    pub pixels: Vec<Color>,
}

impl Texture2D {
    /// Creates a solid-color texture of the given dimensions.
    pub fn solid(width: u32, height: u32, color: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![color; (width * height) as usize],
        }
    }

    /// Returns the color at the given pixel coordinate.
    ///
    /// # Panics
    ///
    /// Panics if `(x, y)` is out of bounds.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Creates a horizontally flipped copy of this texture.
    pub fn flip_horizontal(&self) -> Texture2D {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for y in 0..self.height {
            for x in (0..self.width).rev() {
                pixels.push(self.get_pixel(x, y));
            }
        }
        Texture2D {
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    /// Creates a vertically flipped copy of this texture.
    pub fn flip_vertical(&self) -> Texture2D {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for y in (0..self.height).rev() {
            for x in 0..self.width {
                pixels.push(self.get_pixel(x, y));
            }
        }
        Texture2D {
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    /// Creates a resized copy of this texture using nearest-neighbor sampling.
    pub fn resize(&self, new_width: u32, new_height: u32) -> Texture2D {
        if new_width == 0 || new_height == 0 || self.width == 0 || self.height == 0 {
            return Texture2D::solid(new_width, new_height, Color::TRANSPARENT);
        }
        let mut pixels = Vec::with_capacity((new_width * new_height) as usize);
        for y in 0..new_height {
            for x in 0..new_width {
                let tx = ((x as f32 / new_width as f32) * self.width as f32) as u32;
                let ty = ((y as f32 / new_height as f32) * self.height as f32) as u32;
                pixels.push(self.get_pixel(tx.min(self.width - 1), ty.min(self.height - 1)));
            }
        }
        Texture2D {
            width: new_width,
            height: new_height,
            pixels,
        }
    }
}

// ---------------------------------------------------------------------------
// PNG decoding
// ---------------------------------------------------------------------------

/// Loads a PNG file from disk and returns a [`Texture2D`].
///
/// Handles RGB, RGBA, and grayscale PNG images. Returns `None` if the
/// file doesn't exist, can't be read, or has an unsupported format.
pub fn load_png(path: &str) -> Option<Texture2D> {
    let data = std::fs::read(path).ok()?;
    decode_png(&data)
}

/// Decodes PNG image data from raw bytes into a [`Texture2D`].
///
/// Supports every colour type and bit depth the PNG spec allows — grayscale
/// (1/2/4/8/16-bit), RGB (8/16), **palette** (1/2/4/8, with `tRNS` alpha),
/// grayscale+alpha and RGBA (8/16) — and Adam7 interlacing. Returns `None` on
/// malformed data rather than guessing.
///
/// 🔑 This used to accept 8-bit RGB/RGBA/gray only and returned `None` for a
/// palette image, so the sprite drew NOTHING: the official Godot demo "Dodge
/// the Creeps" ships its enemies as 8-bit palette PNGs (as do Kenney's packs
/// and anything run through pngquant/optipng), and every mob was invisible.
/// An interlaced image was worse — the interlace flag was never read, so it
/// decoded as scrambled rows.
pub fn decode_png(data: &[u8]) -> Option<Texture2D> {
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    // Verify PNG signature.
    if data.len() < 8 || data[0..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        return None;
    }

    // Parse IHDR chunk. The interlace method is its last byte (offset 12 of
    // the 13-byte body, which starts at file offset 16).
    let interlaced = *data.get(16 + 12)? == 1;
    let mut pos = 8;
    let (width, height, bit_depth, color_type) = parse_ihdr(data, &mut pos)?;
    if width == 0 || height == 0 {
        return None;
    }
    let channels: usize = match (color_type, bit_depth) {
        (0, 1 | 2 | 4 | 8 | 16) => 1,
        (3, 1 | 2 | 4 | 8) => 1,
        (2, 8 | 16) => 3,
        (4, 8 | 16) => 2,
        (6, 8 | 16) => 4,
        _ => return None,
    };

    // Collect IDAT, plus the palette and transparency chunks.
    let mut idat_data = Vec::new();
    let mut palette: &[u8] = &[];
    let mut trns: &[u8] = &[];
    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        if pos + 8 + chunk_len > data.len() {
            return None;
        }
        let body = &data[pos + 8..pos + 8 + chunk_len];
        match chunk_type {
            b"IDAT" => idat_data.extend_from_slice(body),
            b"PLTE" => palette = body,
            b"tRNS" => trns = body,
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + chunk_len; // 4 len + 4 type + data + 4 crc
    }
    if color_type == 3 && palette.is_empty() {
        return None;
    }

    // Decompress.
    let raw = decompress_to_vec_zlib(&idat_data).ok()?;

    let bits_per_pixel = channels * bit_depth as usize;
    // The filter's byte distance: one pixel, at least one byte.
    let filter_bpp = (bits_per_pixel / 8).max(1);
    let max_sample = ((1u32 << bit_depth) - 1) as f32;

    // Reads channel `c` of pixel `x` from an unfiltered scanline.
    let sample = |row: &[u8], x: usize, c: usize| -> u16 {
        match bit_depth {
            16 => {
                let i = (x * channels + c) * 2;
                u16::from_be_bytes([row[i], row[i + 1]])
            }
            8 => row[x * channels + c] as u16,
            depth => {
                // Sub-byte depths are single-channel (gray or palette index),
                // packed most-significant bit first.
                let depth = depth as usize;
                let bit = x * depth;
                let byte = row[bit / 8];
                let shift = 8 - depth - (bit % 8);
                ((byte >> shift) & ((1u8 << depth) - 1)) as u16
            }
        }
    };
    let trns_u16 = |i: usize| -> Option<u16> {
        Some(u16::from_be_bytes([
            *trns.get(i * 2)?,
            *trns.get(i * 2 + 1)?,
        ]))
    };
    let to_color = |row: &[u8], x: usize| -> Color {
        let unit = |v: u16| v as f32 / max_sample;
        match color_type {
            0 => {
                let v = sample(row, x, 0);
                let a = if trns_u16(0) == Some(v) { 0.0 } else { 1.0 };
                let g = unit(v);
                Color::new(g, g, g, a)
            }
            2 => {
                let (r, g, b) = (sample(row, x, 0), sample(row, x, 1), sample(row, x, 2));
                let keyed =
                    trns_u16(0) == Some(r) && trns_u16(1) == Some(g) && trns_u16(2) == Some(b);
                Color::new(unit(r), unit(g), unit(b), if keyed { 0.0 } else { 1.0 })
            }
            3 => {
                let i = sample(row, x, 0) as usize;
                let rgb = palette.get(i * 3..i * 3 + 3).unwrap_or(&[0, 0, 0]);
                // Entries past the end of `tRNS` are opaque (PNG spec 11.3.2.1).
                let a = trns.get(i).copied().unwrap_or(255);
                Color::new(
                    rgb[0] as f32 / 255.0,
                    rgb[1] as f32 / 255.0,
                    rgb[2] as f32 / 255.0,
                    a as f32 / 255.0,
                )
            }
            4 => {
                let g = unit(sample(row, x, 0));
                Color::new(g, g, g, unit(sample(row, x, 1)))
            }
            _ => Color::new(
                unit(sample(row, x, 0)),
                unit(sample(row, x, 1)),
                unit(sample(row, x, 2)),
                unit(sample(row, x, 3)),
            ),
        }
    };

    let mut pixels = vec![Color::TRANSPARENT; (width * height) as usize];
    // Adam7's seven passes as (x0, y0, dx, dy); a plain image is one pass.
    const ADAM7: [(usize, usize, usize, usize); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let passes: &[(usize, usize, usize, usize)] = if interlaced { &ADAM7 } else { &[(0, 0, 1, 1)] };
    let (w, h) = (width as usize, height as usize);
    let mut offset = 0usize;
    for &(x0, y0, dx, dy) in passes {
        if x0 >= w || y0 >= h {
            continue; // An empty pass contributes no bytes at all.
        }
        let pw = (w - x0).div_ceil(dx);
        let ph = (h - y0).div_ceil(dy);
        let stride = (pw * bits_per_pixel).div_ceil(8);
        let rows = unfilter(raw.get(offset..)?, stride, ph, filter_bpp)?;
        offset += (stride + 1) * ph;
        for py in 0..ph {
            let row = &rows[py * stride..(py + 1) * stride];
            for px in 0..pw {
                let (x, y) = (x0 + px * dx, y0 + py * dy);
                pixels[y * w + x] = to_color(row, px);
            }
        }
    }

    Some(Texture2D {
        width,
        height,
        pixels,
    })
}

/// Reverses PNG scanline filtering for `height` rows of `stride` bytes, each
/// preceded by its filter-type byte. `bpp` is the filter's byte distance.
fn unfilter(raw: &[u8], stride: usize, height: usize, bpp: usize) -> Option<Vec<u8>> {
    let row_bytes = 1 + stride;
    if raw.len() < row_bytes * height {
        return None;
    }
    let mut out = vec![0u8; stride * height];
    for y in 0..height {
        let filter = raw[y * row_bytes];
        let src = &raw[y * row_bytes + 1..y * row_bytes + 1 + stride];
        for i in 0..stride {
            let a = if i >= bpp {
                out[y * stride + i - bpp]
            } else {
                0
            };
            let b = if y > 0 { out[(y - 1) * stride + i] } else { 0 };
            let c = if y > 0 && i >= bpp {
                out[(y - 1) * stride + i - bpp]
            } else {
                0
            };
            let predicted = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => paeth_predictor(a, b, c),
                _ => return None,
            };
            out[y * stride + i] = src[i].wrapping_add(predicted);
        }
    }
    Some(out)
}

/// Parses the IHDR chunk, returning (width, height, bit_depth, color_type).
fn parse_ihdr(data: &[u8], pos: &mut usize) -> Option<(u32, u32, u8, u8)> {
    if *pos + 25 > data.len() {
        return None;
    }
    let chunk_len =
        u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]) as usize;
    if chunk_len != 13 || &data[*pos + 4..*pos + 8] != b"IHDR" {
        return None;
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
    *pos += 12 + chunk_len; // 4 len + 4 type + 13 data + 4 crc
    Some((width, height, bit_depth, color_type))
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

/// Resolves a `res://` path to a filesystem path relative to the given project root.
///
/// If `texture_path` starts with `res://`, strips that prefix and joins
/// with `project_root`. Otherwise returns the path as-is.
pub fn resolve_res_path(texture_path: &str, project_root: &str) -> String {
    let stripped = texture_path.strip_prefix("res://").unwrap_or(texture_path);
    if project_root.is_empty() {
        stripped.to_string()
    } else {
        format!("{}/{}", project_root.trim_end_matches('/'), stripped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_texture() {
        let red = Color::rgb(1.0, 0.0, 0.0);
        let tex = Texture2D::solid(4, 4, red);
        assert_eq!(tex.width, 4);
        assert_eq!(tex.height, 4);
        assert_eq!(tex.get_pixel(0, 0), red);
        assert_eq!(tex.get_pixel(3, 3), red);
    }

    #[test]
    fn flip_horizontal() {
        let mut pixels = vec![Color::BLACK; 4];
        pixels[0] = Color::rgb(1.0, 0.0, 0.0); // (0,0) = red
        pixels[1] = Color::rgb(0.0, 0.0, 1.0); // (1,0) = blue
        let tex = Texture2D {
            width: 2,
            height: 2,
            pixels,
        };
        let flipped = tex.flip_horizontal();
        assert_eq!(flipped.get_pixel(0, 0), Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(flipped.get_pixel(1, 0), Color::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn flip_vertical() {
        let mut pixels = vec![Color::BLACK; 4];
        pixels[0] = Color::rgb(1.0, 0.0, 0.0); // (0,0) = red, top row
        pixels[2] = Color::rgb(0.0, 0.0, 1.0); // (0,1) = blue, bottom row
        let tex = Texture2D {
            width: 2,
            height: 2,
            pixels,
        };
        let flipped = tex.flip_vertical();
        assert_eq!(flipped.get_pixel(0, 0), Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(flipped.get_pixel(0, 1), Color::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn resize_nearest_neighbor() {
        let tex = Texture2D::solid(2, 2, Color::rgb(1.0, 0.0, 0.0));
        let resized = tex.resize(4, 4);
        assert_eq!(resized.width, 4);
        assert_eq!(resized.height, 4);
        assert_eq!(resized.get_pixel(0, 0), Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(resized.get_pixel(3, 3), Color::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn resize_to_zero() {
        let tex = Texture2D::solid(2, 2, Color::WHITE);
        let resized = tex.resize(0, 0);
        assert_eq!(resized.width, 0);
        assert_eq!(resized.height, 0);
    }

    #[test]
    fn png_roundtrip() {
        // Create a 2x2 red framebuffer, encode to PNG, then decode back.
        let fb = crate::renderer::FrameBuffer::new(2, 2, Color::rgb(1.0, 0.0, 0.0));
        let png_data = crate::export::encode_png(&fb);
        let tex = decode_png(&png_data).expect("should decode our own PNG");
        assert_eq!(tex.width, 2);
        assert_eq!(tex.height, 2);
        let p = tex.get_pixel(0, 0);
        assert!(
            (p.r - 1.0).abs() < 0.01,
            "red channel should be ~1.0, got {}",
            p.r
        );
        assert!(p.g.abs() < 0.01);
        assert!(p.b.abs() < 0.01);
        assert!((p.a - 1.0).abs() < 0.01);
    }

    #[test]
    fn png_roundtrip_rgba_with_alpha() {
        use crate::renderer::FrameBuffer;
        let mut fb = FrameBuffer::new(1, 1, Color::TRANSPARENT);
        fb.set_pixel(0, 0, Color::new(0.5, 0.25, 0.75, 0.5));
        let png_data = crate::export::encode_png(&fb);
        let tex = decode_png(&png_data).expect("decode RGBA PNG");
        let p = tex.get_pixel(0, 0);
        // Allow tolerance due to 8-bit quantization.
        assert!((p.r - 0.5).abs() < 0.02);
        assert!((p.g - 0.25).abs() < 0.02);
        assert!((p.b - 0.75).abs() < 0.02);
        assert!((p.a - 0.5).abs() < 0.02);
    }

    #[test]
    fn decode_png_invalid_data_returns_none() {
        assert!(decode_png(&[]).is_none());
        assert!(decode_png(&[0, 1, 2, 3]).is_none());
        assert!(decode_png(b"not a png").is_none());
    }

    #[test]
    fn load_png_nonexistent_returns_none() {
        assert!(load_png("/nonexistent/path/to/texture.png").is_none());
    }

    #[test]
    fn resolve_res_path_strips_prefix() {
        assert_eq!(
            resolve_res_path("res://icon.png", "/project"),
            "/project/icon.png"
        );
        assert_eq!(
            resolve_res_path("res://assets/player.png", "/project"),
            "/project/assets/player.png"
        );
    }

    #[test]
    fn resolve_res_path_no_prefix() {
        assert_eq!(
            resolve_res_path("icon.png", "/project"),
            "/project/icon.png"
        );
    }

    #[test]
    fn resolve_res_path_empty_root() {
        assert_eq!(resolve_res_path("res://icon.png", ""), "icon.png");
    }

    #[test]
    fn resolve_res_path_trailing_slash() {
        assert_eq!(
            resolve_res_path("res://icon.png", "/project/"),
            "/project/icon.png"
        );
    }
}
