//! Texture types for the software renderer.
//!
//! Provides a simple CPU-side texture representation used for
//! `DrawTextureRect` commands and testing.

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
}
