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
/// Supports 8-bit RGB (color type 2), RGBA (color type 6), and
/// grayscale (color type 0) images. Returns `None` on decode failure.
pub fn decode_png(data: &[u8]) -> Option<Texture2D> {
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    // Verify PNG signature.
    if data.len() < 8 || data[0..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        return None;
    }

    // Parse IHDR chunk.
    let mut pos = 8;
    let (width, height, bit_depth, color_type) = parse_ihdr(data, &mut pos)?;

    if bit_depth != 8 {
        return None; // Only 8-bit depth supported.
    }

    // Collect all IDAT chunks.
    let mut idat_data = Vec::new();
    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        if chunk_type == b"IDAT" {
            if pos + 8 + chunk_len > data.len() {
                return None;
            }
            idat_data.extend_from_slice(&data[pos + 8..pos + 8 + chunk_len]);
        } else if chunk_type == b"IEND" {
            break;
        }
        pos += 12 + chunk_len; // 4 len + 4 type + data + 4 crc
    }

    // Decompress.
    let raw = decompress_to_vec_zlib(&idat_data).ok()?;

    // Determine bytes per pixel.
    let bpp: usize = match color_type {
        0 => 1, // Grayscale
        2 => 3, // RGB
        4 => 2, // Grayscale + Alpha
        6 => 4, // RGBA
        _ => return None,
    };

    // Unfilter scanlines.
    let stride = width as usize * bpp;
    let row_bytes = 1 + stride; // filter byte + pixel data
    if raw.len() < row_bytes * height as usize {
        return None;
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
            _ => return None,
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

    Some(Texture2D {
        width,
        height,
        pixels,
    })
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
