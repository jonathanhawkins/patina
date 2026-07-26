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
/// The uncompressed family of Godot's `Image.Format` (core/io/image.cpp): the
/// 8-bit-per-channel formats and the 32-bit float ones. Godot's half-float and
/// packed formats (`RGBAH`, `RGBE9995`) and the compressed families (BCn/ETC/
/// ASTC) are not in this enum — a compressed payload is decoded to
/// [`ImageFormat::Rgba8`] on load rather than stored compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// 8-bit luminance (grayscale), 1 byte per pixel.
    L8,
    /// 8-bit luminance + alpha, 2 bytes per pixel.
    La8,
    /// 8-bit red only, 1 byte per pixel.
    R8,
    /// 8-bit red + green, 2 bytes per pixel.
    Rg8,
    /// 8-bit RGB, 3 bytes per pixel.
    Rgb8,
    /// 8-bit RGBA, 4 bytes per pixel.
    Rgba8,
    /// 32-bit float red, 4 bytes per pixel.
    Rf,
    /// 32-bit float red + green, 8 bytes per pixel.
    Rgf,
    /// 32-bit float RGB, 12 bytes per pixel.
    Rgbf,
    /// 32-bit float RGBA, 16 bytes per pixel.
    Rgbaf,
}

impl ImageFormat {
    /// Number of bytes one pixel occupies in this format.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            ImageFormat::L8 | ImageFormat::R8 => 1,
            ImageFormat::La8 | ImageFormat::Rg8 => 2,
            ImageFormat::Rgb8 => 3,
            ImageFormat::Rgba8 | ImageFormat::Rf => 4,
            ImageFormat::Rgf => 8,
            ImageFormat::Rgbf => 12,
            ImageFormat::Rgbaf => 16,
        }
    }

    /// Whether the format stores a per-pixel alpha channel. Formats without one
    /// always read back `a = 1.0`, so `get_used_rect` treats them as fully used.
    pub fn has_alpha(self) -> bool {
        matches!(
            self,
            ImageFormat::La8 | ImageFormat::Rgba8 | ImageFormat::Rgbaf
        )
    }

    /// Whether channels are stored as 32-bit floats rather than bytes. Float
    /// formats are not clamped to `0.0..=1.0`, so they carry HDR values.
    pub fn is_float(self) -> bool {
        matches!(
            self,
            ImageFormat::Rf | ImageFormat::Rgf | ImageFormat::Rgbf | ImageFormat::Rgbaf
        )
    }
}

/// Filter used by [`Image::resize`], mirroring Godot's `Image.Interpolation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Point sampling — no blending.
    Nearest,
    /// Linear blend of the 4 neighbouring texels.
    Bilinear,
    /// Catmull-Rom cubic over the 4×4 neighbourhood.
    Cubic,
    /// Windowed-sinc (Lanczos, a = 3) over the 6×6 neighbourhood.
    Lanczos,
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
        let b = |i: usize| self.data[off + i] as f32 / 255.0;
        // Reads the 32-bit float channel `i` (little-endian, as stored).
        let f = |i: usize| {
            let s = off + i * 4;
            f32::from_le_bytes([
                self.data[s],
                self.data[s + 1],
                self.data[s + 2],
                self.data[s + 3],
            ])
        };
        match self.format {
            // Godot's L8 is a luminance channel: it reads back as grey, not red.
            ImageFormat::L8 => Color::new(b(0), b(0), b(0), 1.0),
            ImageFormat::La8 => Color::new(b(0), b(0), b(0), b(1)),
            // R8/RG8 are channel data, so the missing channels read 0 — unlike
            // L8, which broadcasts.
            ImageFormat::R8 => Color::new(b(0), 0.0, 0.0, 1.0),
            ImageFormat::Rg8 => Color::new(b(0), b(1), 0.0, 1.0),
            ImageFormat::Rgb8 => Color::new(b(0), b(1), b(2), 1.0),
            ImageFormat::Rgba8 => Color::new(b(0), b(1), b(2), b(3)),
            ImageFormat::Rf => Color::new(f(0), 0.0, 0.0, 1.0),
            ImageFormat::Rgf => Color::new(f(0), f(1), 0.0, 1.0),
            ImageFormat::Rgbf => Color::new(f(0), f(1), f(2), 1.0),
            ImageFormat::Rgbaf => Color::new(f(0), f(1), f(2), f(3)),
        }
    }

    /// Sets the color at `(x, y)` (`Image.set_pixel`); out-of-bounds writes are
    /// ignored. For [`ImageFormat::L8`] the color's Rec. 601 luma is stored.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        let Some(off) = self.pixel_offset(x, y) else {
            return;
        };
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let luma = color.r * 0.299 + color.g * 0.587 + color.b * 0.114;
        // Float formats are NOT clamped — that is the point of an HDR buffer.
        let put_f = |data: &mut Vec<u8>, i: usize, v: f32| {
            let s = off + i * 4;
            data[s..s + 4].copy_from_slice(&v.to_le_bytes());
        };
        match self.format {
            ImageFormat::L8 => self.data[off] = to_u8(luma),
            ImageFormat::La8 => {
                self.data[off] = to_u8(luma);
                self.data[off + 1] = to_u8(color.a);
            }
            ImageFormat::R8 => self.data[off] = to_u8(color.r),
            ImageFormat::Rg8 => {
                self.data[off] = to_u8(color.r);
                self.data[off + 1] = to_u8(color.g);
            }
            ImageFormat::Rgb8 => {
                self.data[off] = to_u8(color.r);
                self.data[off + 1] = to_u8(color.g);
                self.data[off + 2] = to_u8(color.b);
            }
            ImageFormat::Rgba8 => {
                self.data[off] = to_u8(color.r);
                self.data[off + 1] = to_u8(color.g);
                self.data[off + 2] = to_u8(color.b);
                self.data[off + 3] = to_u8(color.a);
            }
            ImageFormat::Rf => put_f(&mut self.data, 0, color.r),
            ImageFormat::Rgf => {
                put_f(&mut self.data, 0, color.r);
                put_f(&mut self.data, 1, color.g);
            }
            ImageFormat::Rgbf => {
                put_f(&mut self.data, 0, color.r);
                put_f(&mut self.data, 1, color.g);
                put_f(&mut self.data, 2, color.b);
            }
            ImageFormat::Rgbaf => {
                put_f(&mut self.data, 0, color.r);
                put_f(&mut self.data, 1, color.g);
                put_f(&mut self.data, 2, color.b);
                put_f(&mut self.data, 3, color.a);
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

    // -- Format conversion, sampling and region ops (pat-0q2kx.133) ---------
    //
    // Every operation below goes through `get_pixel`/`set_pixel` rather than
    // touching bytes directly, so each one works for EVERY format in
    // `ImageFormat` — including formats added later — instead of needing a
    // per-format branch that could silently miss one.

    /// Returns a copy of this image in `format` (`Image.convert`). Channels the
    /// target format cannot store are dropped, exactly as Godot's converter
    /// does (RGB8 loses alpha, L8 keeps luminance only).
    pub fn convert(&self, format: ImageFormat) -> Image {
        if format == self.format {
            return self.clone();
        }
        let mut out = Image::create(self.width, self.height, format);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set_pixel(x, y, self.get_pixel(x, y));
            }
        }
        out
    }

    /// Converts this image to `format` in place (`Image.convert`).
    pub fn convert_in_place(&mut self, format: ImageFormat) {
        *self = self.convert(format);
    }

    /// Samples with edge clamping, so a filter kernel that reaches past the
    /// border repeats the edge texel instead of reading black.
    fn sample_clamped(&self, x: i64, y: i64) -> Color {
        if self.is_empty() {
            return Color::new(0.0, 0.0, 0.0, 1.0);
        }
        let cx = x.clamp(0, self.width as i64 - 1) as u32;
        let cy = y.clamp(0, self.height as i64 - 1) as u32;
        self.get_pixel(cx, cy)
    }

    /// Catmull-Rom weight for offset `t` (a = -0.5, Godot's cubic).
    fn cubic_weight(t: f32) -> f32 {
        let a = -0.5f32;
        let t = t.abs();
        if t <= 1.0 {
            (a + 2.0) * t * t * t - (a + 3.0) * t * t + 1.0
        } else if t < 2.0 {
            a * t * t * t - 5.0 * a * t * t + 8.0 * a * t - 4.0 * a
        } else {
            0.0
        }
    }

    /// Lanczos-3 windowed-sinc weight for offset `t`.
    fn lanczos_weight(t: f32) -> f32 {
        const A: f32 = 3.0;
        let t = t.abs();
        if t < 1e-6 {
            return 1.0;
        }
        if t >= A {
            return 0.0;
        }
        let pt = std::f32::consts::PI * t;
        (pt.sin() / pt) * ((pt / A).sin() / (pt / A))
    }

    /// Separable weighted sample centred on the source position, with `radius`
    /// texels either side and `weight` supplying the kernel.
    fn sample_kernel(&self, sx: f32, sy: f32, radius: i64, weight: fn(f32) -> f32) -> Color {
        let bx = sx.floor() as i64;
        let by = sy.floor() as i64;
        let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut total = 0.0f32;
        for oy in (1 - radius)..=radius {
            let wy = weight(sy - (by + oy) as f32);
            if wy == 0.0 {
                continue;
            }
            for ox in (1 - radius)..=radius {
                let w = wy * weight(sx - (bx + ox) as f32);
                if w == 0.0 {
                    continue;
                }
                let c = self.sample_clamped(bx + ox, by + oy);
                r += c.r * w;
                g += c.g * w;
                b += c.b * w;
                a += c.a * w;
                total += w;
            }
        }
        if total.abs() < 1e-9 {
            return self.sample_clamped(bx, by);
        }
        // Normalising by the summed weight keeps brightness stable at the edges,
        // where clamping means the kernel is not symmetric.
        Color::new(r / total, g / total, b / total, a / total)
    }

    /// Returns this image resized to `width × height` (`Image.resize`).
    ///
    /// Source positions use the half-texel centre convention
    /// (`(dst + 0.5) * scale - 0.5`), so a resize to the same size is identity
    /// and an upscale does not shift the picture by half a pixel.
    pub fn resized(&self, width: u32, height: u32, interpolation: Interpolation) -> Image {
        let mut out = Image::create(width, height, self.format);
        if width == 0 || height == 0 || self.is_empty() {
            return out;
        }
        let sx_scale = self.width as f32 / width as f32;
        let sy_scale = self.height as f32 / height as f32;
        for y in 0..height {
            for x in 0..width {
                let sx = (x as f32 + 0.5) * sx_scale - 0.5;
                let sy = (y as f32 + 0.5) * sy_scale - 0.5;
                let c = match interpolation {
                    Interpolation::Nearest => {
                        self.sample_clamped(sx.round() as i64, sy.round() as i64)
                    }
                    Interpolation::Bilinear => {
                        let x0 = sx.floor();
                        let y0 = sy.floor();
                        let fx = sx - x0;
                        let fy = sy - y0;
                        let (x0, y0) = (x0 as i64, y0 as i64);
                        let c00 = self.sample_clamped(x0, y0);
                        let c10 = self.sample_clamped(x0 + 1, y0);
                        let c01 = self.sample_clamped(x0, y0 + 1);
                        let c11 = self.sample_clamped(x0 + 1, y0 + 1);
                        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
                        let mix = |a: &Color, b: &Color, t: f32| {
                            Color::new(
                                lerp(a.r, b.r, t),
                                lerp(a.g, b.g, t),
                                lerp(a.b, b.b, t),
                                lerp(a.a, b.a, t),
                            )
                        };
                        let top = mix(&c00, &c10, fx);
                        let bottom = mix(&c01, &c11, fx);
                        mix(&top, &bottom, fy)
                    }
                    Interpolation::Cubic => self.sample_kernel(sx, sy, 2, Image::cubic_weight),
                    Interpolation::Lanczos => self.sample_kernel(sx, sy, 3, Image::lanczos_weight),
                };
                out.set_pixel(x, y, c);
            }
        }
        out
    }

    /// Resizes this image in place (`Image.resize`).
    pub fn resize(&mut self, width: u32, height: u32, interpolation: Interpolation) {
        *self = self.resized(width, height, interpolation);
    }

    /// Returns the `width × height` region at `(x, y)` (`Image.get_region`).
    /// Pixels outside the source clamp to its edge.
    pub fn get_region(&self, x: i64, y: i64, width: u32, height: u32) -> Image {
        let mut out = Image::create(width, height, self.format);
        for oy in 0..height {
            for ox in 0..width {
                out.set_pixel(ox, oy, self.sample_clamped(x + ox as i64, y + oy as i64));
            }
        }
        out
    }

    /// Crops to `width × height` anchored at the top-left (`Image.crop`).
    /// Growing pads with transparent black, as Godot does.
    pub fn crop(&mut self, width: u32, height: u32) {
        let mut out = Image::create(width, height, self.format);
        for y in 0..height.min(self.height) {
            for x in 0..width.min(self.width) {
                out.set_pixel(x, y, self.get_pixel(x, y));
            }
        }
        *self = out;
    }

    /// Rotates 90° clockwise (`Image.rotate_90` with `CLOCKWISE`).
    pub fn rotate_90_cw(&mut self) {
        let mut out = Image::create(self.height, self.width, self.format);
        for y in 0..self.height {
            for x in 0..self.width {
                // (x, y) -> (H-1-y, x)
                out.set_pixel(self.height - 1 - y, x, self.get_pixel(x, y));
            }
        }
        *self = out;
    }

    /// Rotates 90° counter-clockwise (`Image.rotate_90` with `COUNTERCLOCKWISE`).
    pub fn rotate_90_ccw(&mut self) {
        let mut out = Image::create(self.height, self.width, self.format);
        for y in 0..self.height {
            for x in 0..self.width {
                // (x, y) -> (y, W-1-x)
                out.set_pixel(y, self.width - 1 - x, self.get_pixel(x, y));
            }
        }
        *self = out;
    }

    /// Rotates 180° in place (`Image.rotate_180`).
    pub fn rotate_180(&mut self) {
        self.flip_x();
        self.flip_y();
    }

    /// Fills the rectangle at `(x, y)` sized `width × height` with `color`
    /// (`Image.fill_rect`). The rect is clipped to the image.
    pub fn fill_rect(&mut self, x: i64, y: i64, width: u32, height: u32, color: Color) {
        for oy in 0..height {
            for ox in 0..width {
                let (px, py) = (x + ox as i64, y + oy as i64);
                if px < 0 || py < 0 {
                    continue;
                }
                self.set_pixel(px as u32, py as u32, color);
            }
        }
    }

    /// The smallest rect containing every pixel with non-zero alpha, as
    /// `(x, y, width, height)` (`Image.get_used_rect`). A format without alpha
    /// is fully used; a fully transparent image returns a zero-sized rect.
    pub fn get_used_rect(&self) -> (u32, u32, u32, u32) {
        if self.is_empty() {
            return (0, 0, 0, 0);
        }
        if !self.format.has_alpha() {
            return (0, 0, self.width, self.height);
        }
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        let (mut max_x, mut max_y) = (0u32, 0u32);
        let mut any = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get_pixel(x, y).a > 0.0 {
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if !any {
            return (0, 0, 0, 0);
        }
        (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
    }

    /// Copies a `src` region to `(dst_x, dst_y)`, overwriting (`Image.blit_rect`).
    pub fn blit_rect(
        &mut self,
        src: &Image,
        src_x: i64,
        src_y: i64,
        width: u32,
        height: u32,
        dst_x: i64,
        dst_y: i64,
    ) {
        self.copy_rect(src, src_x, src_y, width, height, dst_x, dst_y, None, false);
    }

    /// [`blit_rect`](Self::blit_rect) gated by `mask`: only source pixels whose
    /// matching mask pixel has non-zero alpha are copied (`Image.blit_rect_mask`).
    #[allow(clippy::too_many_arguments)]
    pub fn blit_rect_mask(
        &mut self,
        src: &Image,
        mask: &Image,
        src_x: i64,
        src_y: i64,
        width: u32,
        height: u32,
        dst_x: i64,
        dst_y: i64,
    ) {
        self.copy_rect(
            src,
            src_x,
            src_y,
            width,
            height,
            dst_x,
            dst_y,
            Some(mask),
            false,
        );
    }

    /// Alpha-blends a `src` region over `(dst_x, dst_y)` (`Image.blend_rect`).
    pub fn blend_rect(
        &mut self,
        src: &Image,
        src_x: i64,
        src_y: i64,
        width: u32,
        height: u32,
        dst_x: i64,
        dst_y: i64,
    ) {
        self.copy_rect(src, src_x, src_y, width, height, dst_x, dst_y, None, true);
    }

    /// [`blend_rect`](Self::blend_rect) gated by `mask` (`Image.blend_rect_mask`).
    #[allow(clippy::too_many_arguments)]
    pub fn blend_rect_mask(
        &mut self,
        src: &Image,
        mask: &Image,
        src_x: i64,
        src_y: i64,
        width: u32,
        height: u32,
        dst_x: i64,
        dst_y: i64,
    ) {
        self.copy_rect(
            src,
            src_x,
            src_y,
            width,
            height,
            dst_x,
            dst_y,
            Some(mask),
            true,
        );
    }

    /// Shared body of the four blit/blend entry points.
    #[allow(clippy::too_many_arguments)]
    fn copy_rect(
        &mut self,
        src: &Image,
        src_x: i64,
        src_y: i64,
        width: u32,
        height: u32,
        dst_x: i64,
        dst_y: i64,
        mask: Option<&Image>,
        blend: bool,
    ) {
        for oy in 0..height {
            for ox in 0..width {
                let (sx, sy) = (src_x + ox as i64, src_y + oy as i64);
                let (dx, dy) = (dst_x + ox as i64, dst_y + oy as i64);
                // Both ends are clipped: a source pixel outside `src` is not
                // invented, and a destination outside `self` is dropped.
                if sx < 0 || sy < 0 || sx >= src.width as i64 || sy >= src.height as i64 {
                    continue;
                }
                if dx < 0 || dy < 0 || dx >= self.width as i64 || dy >= self.height as i64 {
                    continue;
                }
                if let Some(mask) = mask {
                    // Godot indexes the mask by the SOURCE position.
                    if sx >= mask.width as i64 || sy >= mask.height as i64 {
                        continue;
                    }
                    if mask.get_pixel(sx as u32, sy as u32).a <= 0.0 {
                        continue;
                    }
                }
                let s = src.get_pixel(sx as u32, sy as u32);
                let out = if blend {
                    let d = self.get_pixel(dx as u32, dy as u32);
                    let sa = s.a.clamp(0.0, 1.0);
                    let inv = 1.0 - sa;
                    // Straight-alpha "over": the standard Porter-Duff source-over
                    // Godot's `blend_rect` performs.
                    let a = sa + d.a * inv;
                    if a <= 0.0 {
                        Color::new(0.0, 0.0, 0.0, 0.0)
                    } else {
                        Color::new(
                            (s.r * sa + d.r * d.a * inv) / a,
                            (s.g * sa + d.g * d.a * inv) / a,
                            (s.b * sa + d.b * d.a * inv) / a,
                            a,
                        )
                    }
                } else {
                    s
                };
                self.set_pixel(dx as u32, dy as u32, out);
            }
        }
    }

    /// Successive halvings of this image, largest first, down to 1×1
    /// (`Image.generate_mipmaps`). The full-size level is NOT included, matching
    /// Godot's mipmap chain which stores level 0 separately.
    pub fn generate_mipmaps(&self) -> Vec<Image> {
        let mut out = Vec::new();
        let (mut w, mut h) = (self.width, self.height);
        let mut cur = self.clone();
        while w > 1 || h > 1 {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            cur = cur.resized(w, h, Interpolation::Bilinear);
            out.push(cur.clone());
        }
        out
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
