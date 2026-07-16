//! The core of Godot's `Image` data class: a CPU pixel buffer with a format
//! and a get/set-pixel API.
//!
//! This module provides the format-agnostic data type and the common pixel
//! operations (create, get/set pixel, fill, horizontal/vertical flip, raw-byte
//! access). Decoding (`load_png_from_buffer`), cross-format conversion,
//! mipmaps, resize, and blit/blend are layered on separately.

use gdcore::math::Color;

/// Pixel storage format for an [`Image`].
///
/// A focused subset of Godot's `Image.Format`; these two byte-addressable
/// formats cover the common grayscale and RGBA cases. More formats (the
/// float/RGBE families) can be added alongside these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// 8-bit luminance (grayscale), 1 byte per pixel.
    L8,
    /// 8-bit RGBA, 4 bytes per pixel.
    Rgba8,
}

impl ImageFormat {
    /// Number of bytes one pixel occupies in this format.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            ImageFormat::L8 => 1,
            ImageFormat::Rgba8 => 4,
        }
    }
}

/// A CPU image: `width × height` pixels stored as raw bytes in an
/// [`ImageFormat`], mirroring the core of Godot's `Image`.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    width: u32,
    height: u32,
    format: ImageFormat,
    data: Vec<u8>,
}

impl Image {
    /// Creates a `width × height` image zero-filled in `format`
    /// (`Image.create`).
    pub fn create(width: u32, height: u32, format: ImageFormat) -> Self {
        let len = width as usize * height as usize * format.bytes_per_pixel();
        Self {
            width,
            height,
            format,
            data: vec![0u8; len],
        }
    }

    /// Creates an image from existing raw bytes (`Image.create_from_data`); the
    /// buffer is padded or truncated to `width × height × bytes_per_pixel`.
    pub fn create_from_data(
        width: u32,
        height: u32,
        format: ImageFormat,
        mut data: Vec<u8>,
    ) -> Self {
        let len = width as usize * height as usize * format.bytes_per_pixel();
        data.resize(len, 0);
        Self {
            width,
            height,
            format,
            data,
        }
    }

    /// Image width in pixels (`Image.get_width`).
    pub fn get_width(&self) -> u32 {
        self.width
    }

    /// Image height in pixels (`Image.get_height`).
    pub fn get_height(&self) -> u32 {
        self.height
    }

    /// Pixel format (`Image.get_format`).
    pub fn get_format(&self) -> ImageFormat {
        self.format
    }

    /// Raw pixel bytes (`Image.get_data`).
    pub fn get_data(&self) -> &[u8] {
        &self.data
    }

    /// Size of the raw pixel buffer in bytes (`Image.get_data` length).
    pub fn get_data_size(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` when the image has no pixels (`Image.is_empty`).
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Byte offset of pixel `(x, y)`, or `None` when out of bounds.
    fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((y as usize * self.width as usize + x as usize) * self.format.bytes_per_pixel())
    }

    /// Returns the color at `(x, y)`, or opaque black when out of bounds
    /// (`Image.get_pixel`).
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        let Some(off) = self.pixel_offset(x, y) else {
            return Color::new(0.0, 0.0, 0.0, 1.0);
        };
        match self.format {
            ImageFormat::L8 => {
                let l = self.data[off] as f32 / 255.0;
                Color::new(l, l, l, 1.0)
            }
            ImageFormat::Rgba8 => Color::new(
                self.data[off] as f32 / 255.0,
                self.data[off + 1] as f32 / 255.0,
                self.data[off + 2] as f32 / 255.0,
                self.data[off + 3] as f32 / 255.0,
            ),
        }
    }

    /// Sets the color at `(x, y)` (`Image.set_pixel`); out-of-bounds writes are
    /// ignored. For [`ImageFormat::L8`] the color's Rec. 601 luma is stored.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        let Some(off) = self.pixel_offset(x, y) else {
            return;
        };
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        match self.format {
            ImageFormat::L8 => {
                let luma = color.r * 0.299 + color.g * 0.587 + color.b * 0.114;
                self.data[off] = to_u8(luma);
            }
            ImageFormat::Rgba8 => {
                self.data[off] = to_u8(color.r);
                self.data[off + 1] = to_u8(color.g);
                self.data[off + 2] = to_u8(color.b);
                self.data[off + 3] = to_u8(color.a);
            }
        }
    }

    /// Fills every pixel with `color` (`Image.fill`).
    pub fn fill(&mut self, color: Color) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// Mirrors the image horizontally in place (`Image.flip_x`).
    pub fn flip_x(&mut self) {
        let bpp = self.format.bytes_per_pixel();
        let row = self.width as usize * bpp;
        for y in 0..self.height as usize {
            for x in 0..(self.width as usize / 2) {
                let left = y * row + x * bpp;
                let right = y * row + (self.width as usize - 1 - x) * bpp;
                for b in 0..bpp {
                    self.data.swap(left + b, right + b);
                }
            }
        }
    }

    /// Mirrors the image vertically in place (`Image.flip_y`).
    pub fn flip_y(&mut self) {
        let bpp = self.format.bytes_per_pixel();
        let row = self.width as usize * bpp;
        for y in 0..(self.height as usize / 2) {
            let top = y * row;
            let bottom = (self.height as usize - 1 - y) * row;
            for b in 0..row {
                self.data.swap(top + b, bottom + b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // pat-nzgdu.81: core Image pixel API + fill + flips + raw-byte access.
    #[test]
    fn image_class_pixels_to_texture() {
        let mut img = Image::create(3, 2, ImageFormat::Rgba8);
        assert_eq!(img.get_width(), 3);
        assert_eq!(img.get_height(), 2);
        assert_eq!(img.get_data_size(), 3 * 2 * 4);
        assert!(!img.is_empty());
        // Freshly created RGBA8 -> all zero (transparent black).
        assert_eq!(img.get_pixel(0, 0), Color::new(0.0, 0.0, 0.0, 0.0));

        // set/get round-trips an 8-bit-exact color (channels in {0,1}).
        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        img.set_pixel(2, 1, red);
        assert_eq!(img.get_pixel(2, 1), red);

        // Out-of-bounds set is a no-op; out-of-bounds get is opaque black.
        img.set_pixel(99, 99, red);
        assert_eq!(img.get_pixel(99, 99), Color::new(0.0, 0.0, 0.0, 1.0));

        // fill paints every pixel.
        let blue = Color::new(0.0, 0.0, 1.0, 1.0);
        img.fill(blue);
        assert_eq!(img.get_pixel(0, 0), blue);
        assert_eq!(img.get_pixel(2, 1), blue);

        // flip_x moves column 0 to column width-1.
        let green = Color::new(0.0, 1.0, 0.0, 1.0);
        img.set_pixel(0, 0, green);
        img.flip_x();
        assert_eq!(img.get_pixel(2, 0), green);
        assert_eq!(img.get_pixel(0, 0), blue);

        // flip_y moves row 0 to row height-1.
        img.set_pixel(1, 0, red);
        img.flip_y();
        assert_eq!(img.get_pixel(1, 1), red);
    }

    #[test]
    fn image_l8_stores_luma() {
        let mut gray = Image::create(1, 1, ImageFormat::L8);
        assert_eq!(gray.get_data_size(), 1);
        gray.set_pixel(0, 0, Color::new(1.0, 1.0, 1.0, 1.0));
        let px = gray.get_pixel(0, 0);
        assert!((px.r - 1.0).abs() < 0.01);
        assert_eq!(px.a, 1.0);

        // create_from_data pads/truncates to the format size.
        let img = Image::create_from_data(2, 1, ImageFormat::L8, vec![200]);
        assert_eq!(img.get_data_size(), 2);
        assert_eq!(img.get_data()[0], 200);
        assert_eq!(img.get_data()[1], 0);
    }
}
