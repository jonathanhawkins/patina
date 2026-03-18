//! Drawing primitives operating on a [`FrameBuffer`].
//!
//! All drawing functions clip to the framebuffer bounds automatically.

use gdcore::math::{Color, Rect2, Vector2};

use crate::renderer::FrameBuffer;
use crate::texture::Texture2D;

/// Fills a rectangular region of the framebuffer with `color`, clipping to bounds.
pub fn fill_rect(fb: &mut FrameBuffer, rect: Rect2, color: Color) {
    let x_min = (rect.position.x as i32).max(0) as u32;
    let y_min = (rect.position.y as i32).max(0) as u32;
    let x_max = ((rect.position.x + rect.size.x) as i32).max(0).min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32).max(0).min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
            fb.set_pixel(px, py, color);
        }
    }
}

/// Fills a circle centered at `center` with the given `radius`.
pub fn fill_circle(fb: &mut FrameBuffer, center: Vector2, radius: f32, color: Color) {
    let r_sq = radius * radius;
    let x_min = ((center.x - radius) as i32).max(0) as u32;
    let y_min = ((center.y - radius) as i32).max(0) as u32;
    let x_max = ((center.x + radius).ceil() as i32).max(0).min(fb.width as i32) as u32;
    let y_max = ((center.y + radius).ceil() as i32).max(0).min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
            let dx = px as f32 + 0.5 - center.x;
            let dy = py as f32 + 0.5 - center.y;
            if dx * dx + dy * dy <= r_sq {
                fb.set_pixel(px, py, color);
            }
        }
    }
}

/// Draws a line from `from` to `to` using Bresenham's line algorithm.
///
/// The `_width` parameter is accepted for API compatibility but currently
/// draws a 1-pixel-wide line.
pub fn draw_line(fb: &mut FrameBuffer, from: Vector2, to: Vector2, color: Color, _width: f32) {
    let mut x0 = from.x as i32;
    let mut y0 = from.y as i32;
    let x1 = to.x as i32;
    let y1 = to.y as i32;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x0 >= 0 && y0 >= 0 && (x0 as u32) < fb.width && (y0 as u32) < fb.height {
            fb.set_pixel(x0 as u32, y0 as u32, color);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Draws a texture into `rect`, modulating each texel by `modulate`.
pub fn draw_texture_rect(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    rect: Rect2,
    modulate: Color,
) {
    if texture.width == 0 || texture.height == 0 {
        return;
    }

    let x_min = (rect.position.x as i32).max(0) as u32;
    let y_min = (rect.position.y as i32).max(0) as u32;
    let x_max = ((rect.position.x + rect.size.x) as i32).max(0).min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32).max(0).min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
            // Map pixel to texture coordinates.
            let u = (px as f32 - rect.position.x) / rect.size.x;
            let v = (py as f32 - rect.position.y) / rect.size.y;
            let tx = ((u * texture.width as f32) as u32).min(texture.width - 1);
            let ty = ((v * texture.height as f32) as u32).min(texture.height - 1);

            let texel = texture.get_pixel(tx, ty);
            let final_color = Color::new(
                texel.r * modulate.r,
                texel.g * modulate.g,
                texel.b * modulate.b,
                texel.a * modulate.a,
            );
            fb.set_pixel(px, py, final_color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_basic() {
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        let rect = Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(3.0, 3.0));
        let red = Color::rgb(1.0, 0.0, 0.0);
        fill_rect(&mut fb, rect, red);

        // Inside the rect.
        assert_eq!(fb.get_pixel(2, 2), red);
        assert_eq!(fb.get_pixel(4, 4), red);
        // Outside the rect.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
        assert_eq!(fb.get_pixel(5, 5), Color::BLACK);
    }

    #[test]
    fn fill_rect_clipping() {
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        // Rect that extends beyond the framebuffer.
        let rect = Rect2::new(Vector2::new(-2.0, -2.0), Vector2::new(5.0, 5.0));
        let green = Color::rgb(0.0, 1.0, 0.0);
        fill_rect(&mut fb, rect, green);

        // Inside clipped region.
        assert_eq!(fb.get_pixel(0, 0), green);
        assert_eq!(fb.get_pixel(2, 2), green);
        // Outside original rect area.
        assert_eq!(fb.get_pixel(5, 5), Color::BLACK);
    }

    #[test]
    fn fill_circle_basic() {
        let mut fb = FrameBuffer::new(20, 20, Color::BLACK);
        let blue = Color::rgb(0.0, 0.0, 1.0);
        fill_circle(&mut fb, Vector2::new(10.0, 10.0), 5.0, blue);

        // Center should be filled.
        assert_eq!(fb.get_pixel(10, 10), blue);
        // Corner should not be filled.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn draw_line_basic() {
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        let white = Color::WHITE;
        draw_line(
            &mut fb,
            Vector2::new(0.0, 0.0),
            Vector2::new(9.0, 0.0),
            white,
            1.0,
        );

        // Horizontal line along y=0.
        for x in 0..10 {
            assert_eq!(fb.get_pixel(x, 0), white);
        }
        // Row below should be black.
        assert_eq!(fb.get_pixel(0, 1), Color::BLACK);
    }

    #[test]
    fn draw_texture_rect_with_modulate() {
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        let tex = Texture2D::solid(2, 2, Color::WHITE);
        let rect = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 4.0));
        let half_red = Color::new(0.5, 0.0, 0.0, 1.0);
        draw_texture_rect(&mut fb, &tex, rect, half_red);

        let pixel = fb.get_pixel(1, 1);
        assert!((pixel.r - 0.5).abs() < 0.01);
        assert!(pixel.g.abs() < 0.01);
        assert!(pixel.b.abs() < 0.01);
    }
}
