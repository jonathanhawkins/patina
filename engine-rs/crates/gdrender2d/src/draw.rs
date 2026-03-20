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
    let x_max = ((rect.position.x + rect.size.x) as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32)
        .max(0)
        .min(fb.height as i32) as u32;

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
    let x_max = ((center.x + radius).ceil() as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((center.y + radius).ceil() as i32)
        .max(0)
        .min(fb.height as i32) as u32;

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
pub fn draw_texture_rect(fb: &mut FrameBuffer, texture: &Texture2D, rect: Rect2, modulate: Color) {
    if texture.width == 0 || texture.height == 0 {
        return;
    }

    let x_min = (rect.position.x as i32).max(0) as u32;
    let y_min = (rect.position.y as i32).max(0) as u32;
    let x_max = ((rect.position.x + rect.size.x) as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32)
        .max(0)
        .min(fb.height as i32) as u32;

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

/// Draws a sub-region of a texture into `rect`, modulating each texel by `modulate`.
///
/// `source_rect` specifies the pixel region within the texture to sample from.
pub fn draw_texture_region(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    rect: Rect2,
    source_rect: Rect2,
    modulate: Color,
) {
    if texture.width == 0 || texture.height == 0 {
        return;
    }
    if source_rect.size.x <= 0.0 || source_rect.size.y <= 0.0 {
        return;
    }

    let x_min = (rect.position.x as i32).max(0) as u32;
    let y_min = (rect.position.y as i32).max(0) as u32;
    let x_max = ((rect.position.x + rect.size.x) as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32)
        .max(0)
        .min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
            let u = (px as f32 - rect.position.x) / rect.size.x;
            let v = (py as f32 - rect.position.y) / rect.size.y;
            let tx =
                ((source_rect.position.x + u * source_rect.size.x) as u32).min(texture.width - 1);
            let ty =
                ((source_rect.position.y + v * source_rect.size.y) as u32).min(texture.height - 1);

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

/// Fills a rectangular region with alpha blending (source-over).
pub fn fill_rect_blended(fb: &mut FrameBuffer, rect: Rect2, color: Color) {
    let x_min = (rect.position.x as i32).max(0) as u32;
    let y_min = (rect.position.y as i32).max(0) as u32;
    let x_max = ((rect.position.x + rect.size.x) as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32)
        .max(0)
        .min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
            fb.blend_pixel(px, py, color);
        }
    }
}

/// Draws a nine-patch (9-slice) texture into `rect`.
///
/// The source texture is divided into 9 regions by the four margin values.
/// Corners are drawn at their natural size, edges are stretched along one
/// axis, and the center region is stretched in both axes (if `draw_center`
/// is true). Each texel is tinted by `modulate`.
pub fn draw_nine_patch(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    rect: Rect2,
    margin_left: f32,
    margin_top: f32,
    margin_right: f32,
    margin_bottom: f32,
    draw_center: bool,
    modulate: Color,
) {
    if texture.width == 0 || texture.height == 0 {
        return;
    }

    let tw = texture.width as f32;
    let th = texture.height as f32;

    // Clamp margins to texture size.
    let ml = margin_left.min(tw).max(0.0);
    let mt = margin_top.min(th).max(0.0);
    let mr = margin_right.min(tw - ml).max(0.0);
    let mb = margin_bottom.min(th - mt).max(0.0);

    // Source regions (in texture pixel coords).
    let src_left = ml;
    let src_top = mt;
    let src_right = tw - mr;
    let src_bottom = th - mb;

    // Destination regions.
    let dst_x0 = rect.position.x;
    let dst_y0 = rect.position.y;
    let dst_x3 = rect.position.x + rect.size.x;
    let dst_y3 = rect.position.y + rect.size.y;
    let dst_x1 = dst_x0 + ml;
    let dst_y1 = dst_y0 + mt;
    // Ensure middle region doesn't collapse to negative.
    let dst_x2 = (dst_x3 - mr).max(dst_x1);
    let dst_y2 = (dst_y3 - mb).max(dst_y1);

    // Helper closure: draw a texture sub-region into a destination rect.
    let mut draw_patch = |fb: &mut FrameBuffer,
                          dx: f32,
                          dy: f32,
                          dw: f32,
                          dh: f32,
                          sx: f32,
                          sy: f32,
                          sw: f32,
                          sh: f32| {
        if dw <= 0.0 || dh <= 0.0 || sw <= 0.0 || sh <= 0.0 {
            return;
        }
        let dst = Rect2::new(Vector2::new(dx, dy), Vector2::new(dw, dh));
        let src = Rect2::new(Vector2::new(sx, sy), Vector2::new(sw, sh));
        draw_texture_region(fb, texture, dst, src, modulate);
    };

    // Top-left corner
    draw_patch(fb, dst_x0, dst_y0, ml, mt, 0.0, 0.0, src_left, src_top);
    // Top edge
    draw_patch(
        fb,
        dst_x1,
        dst_y0,
        dst_x2 - dst_x1,
        mt,
        src_left,
        0.0,
        src_right - src_left,
        src_top,
    );
    // Top-right corner
    draw_patch(
        fb,
        dst_x2,
        dst_y0,
        mr,
        mt,
        src_right,
        0.0,
        tw - src_right,
        src_top,
    );

    // Left edge
    draw_patch(
        fb,
        dst_x0,
        dst_y1,
        ml,
        dst_y2 - dst_y1,
        0.0,
        src_top,
        src_left,
        src_bottom - src_top,
    );
    // Center
    if draw_center {
        draw_patch(
            fb,
            dst_x1,
            dst_y1,
            dst_x2 - dst_x1,
            dst_y2 - dst_y1,
            src_left,
            src_top,
            src_right - src_left,
            src_bottom - src_top,
        );
    }
    // Right edge
    draw_patch(
        fb,
        dst_x2,
        dst_y1,
        mr,
        dst_y2 - dst_y1,
        src_right,
        src_top,
        tw - src_right,
        src_bottom - src_top,
    );

    // Bottom-left corner
    draw_patch(
        fb,
        dst_x0,
        dst_y2,
        ml,
        mb,
        0.0,
        src_bottom,
        src_left,
        th - src_bottom,
    );
    // Bottom edge
    draw_patch(
        fb,
        dst_x1,
        dst_y2,
        dst_x2 - dst_x1,
        mb,
        src_left,
        src_bottom,
        src_right - src_left,
        th - src_bottom,
    );
    // Bottom-right corner
    draw_patch(
        fb,
        dst_x2,
        dst_y2,
        mr,
        mb,
        src_right,
        src_bottom,
        tw - src_right,
        th - src_bottom,
    );
}

/// Draws a line with alpha blending (source-over).
pub fn draw_line_blended(
    fb: &mut FrameBuffer,
    from: Vector2,
    to: Vector2,
    color: Color,
    _width: f32,
) {
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
            fb.blend_pixel(x0 as u32, y0 as u32, color);
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

/// Fills a circle with alpha blending (source-over).
pub fn fill_circle_blended(fb: &mut FrameBuffer, center: Vector2, radius: f32, color: Color) {
    let r_sq = radius * radius;
    let x_min = ((center.x - radius) as i32).max(0) as u32;
    let y_min = ((center.y - radius) as i32).max(0) as u32;
    let x_max = ((center.x + radius).ceil() as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((center.y + radius).ceil() as i32)
        .max(0)
        .min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
            let dx = px as f32 + 0.5 - center.x;
            let dy = py as f32 + 0.5 - center.y;
            if dx * dx + dy * dy <= r_sq {
                fb.blend_pixel(px, py, color);
            }
        }
    }
}

/// Draws a texture into `rect` with alpha blending, modulating each texel by `modulate`.
///
/// Unlike [`draw_texture_rect`], this version uses source-over alpha compositing
/// so that transparent texels do not overwrite the background.
pub fn draw_texture_rect_blended(
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
    let x_max = ((rect.position.x + rect.size.x) as i32)
        .max(0)
        .min(fb.width as i32) as u32;
    let y_max = ((rect.position.y + rect.size.y) as i32)
        .max(0)
        .min(fb.height as i32) as u32;

    for py in y_min..y_max {
        for px in x_min..x_max {
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
            if final_color.a > 0.001 {
                fb.blend_pixel(px, py, final_color);
            }
        }
    }
}

/// Draws a rectangle outline with alpha blending.
pub fn draw_rect_outline_blended(fb: &mut FrameBuffer, rect: Rect2, color: Color, _width: f32) {
    let tl = rect.position;
    let tr = Vector2::new(rect.position.x + rect.size.x, rect.position.y);
    let br = Vector2::new(rect.position.x + rect.size.x, rect.position.y + rect.size.y);
    let bl = Vector2::new(rect.position.x, rect.position.y + rect.size.y);
    draw_line_blended(fb, tl, tr, color, 1.0);
    draw_line_blended(fb, tr, br, color, 1.0);
    draw_line_blended(fb, br, bl, color, 1.0);
    draw_line_blended(fb, bl, tl, color, 1.0);
}

/// Fills a rotated rectangle on the framebuffer.
///
/// Uses a 2D rotation matrix to test whether each pixel in the bounding box
/// lies inside the rotated rectangle defined by `center`, `half_extents`, and `angle`.
pub fn fill_rotated_rect(
    fb: &mut FrameBuffer,
    center: Vector2,
    half_extents: Vector2,
    angle: f32,
    color: Color,
) {
    let (sin_a, cos_a) = angle.sin_cos();

    // Compute bounding box of the rotated rect.
    let corners = [
        Vector2::new(-half_extents.x, -half_extents.y),
        Vector2::new(half_extents.x, -half_extents.y),
        Vector2::new(half_extents.x, half_extents.y),
        Vector2::new(-half_extents.x, half_extents.y),
    ];

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for c in &corners {
        let rx = c.x * cos_a - c.y * sin_a + center.x;
        let ry = c.x * sin_a + c.y * cos_a + center.y;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }

    let x_start = (min_x.floor() as i32).max(0) as u32;
    let y_start = (min_y.floor() as i32).max(0) as u32;
    let x_end = (max_x.ceil() as i32).max(0).min(fb.width as i32) as u32;
    let y_end = (max_y.ceil() as i32).max(0).min(fb.height as i32) as u32;

    for py in y_start..y_end {
        for px in x_start..x_end {
            // Transform pixel back to local rect space (inverse rotation).
            let dx = px as f32 + 0.5 - center.x;
            let dy = py as f32 + 0.5 - center.y;
            let local_x = dx * cos_a + dy * sin_a;
            let local_y = -dx * sin_a + dy * cos_a;

            if local_x.abs() <= half_extents.x && local_y.abs() <= half_extents.y {
                fb.set_pixel(px, py, color);
            }
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

    #[test]
    fn draw_texture_region_samples_subregion() {
        // Create a 4x4 texture: left half red, right half blue.
        let mut pixels = vec![Color::BLACK; 16];
        for y in 0..4 {
            for x in 0..2 {
                pixels[y * 4 + x] = Color::rgb(1.0, 0.0, 0.0);
            }
            for x in 2..4 {
                pixels[y * 4 + x] = Color::rgb(0.0, 0.0, 1.0);
            }
        }
        let tex = Texture2D {
            width: 4,
            height: 4,
            pixels,
        };

        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        // Draw only the right half (blue region).
        let dst = Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0));
        let src = Rect2::new(Vector2::new(2.0, 0.0), Vector2::new(2.0, 4.0));
        draw_texture_region(&mut fb, &tex, dst, src, Color::WHITE);

        // Should be blue.
        let pixel = fb.get_pixel(1, 1);
        assert!((pixel.b - 1.0).abs() < 0.01);
        assert!(pixel.r.abs() < 0.01);
    }

    #[test]
    fn draw_texture_region_with_modulate() {
        let tex = Texture2D::solid(4, 4, Color::WHITE);
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        let dst = Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0));
        let src = Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0));
        let green = Color::rgb(0.0, 1.0, 0.0);
        draw_texture_region(&mut fb, &tex, dst, src, green);

        let pixel = fb.get_pixel(1, 1);
        assert!((pixel.g - 1.0).abs() < 0.01);
        assert!(pixel.r.abs() < 0.01);
    }

    #[test]
    fn nine_patch_draws_corners_and_center() {
        // 6x6 texture: top-left 2x2 = red, rest = blue
        let mut pixels = vec![Color::rgb(0.0, 0.0, 1.0); 36];
        for y in 0..2 {
            for x in 0..2 {
                pixels[y * 6 + x] = Color::rgb(1.0, 0.0, 0.0);
            }
        }
        let tex = Texture2D {
            width: 6,
            height: 6,
            pixels,
        };

        let mut fb = FrameBuffer::new(12, 12, Color::BLACK);
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(12.0, 12.0));
        draw_nine_patch(&mut fb, &tex, rect, 2.0, 2.0, 2.0, 2.0, true, Color::WHITE);

        // Top-left corner pixel should be red (from the 2x2 red corner of texture)
        assert_eq!(fb.get_pixel(0, 0), Color::rgb(1.0, 0.0, 0.0));
        // Center should be blue (center region stretched)
        assert_eq!(fb.get_pixel(6, 6), Color::rgb(0.0, 0.0, 1.0));
    }

    #[test]
    fn nine_patch_no_center() {
        let tex = Texture2D::solid(6, 6, Color::WHITE);
        let mut fb = FrameBuffer::new(12, 12, Color::BLACK);
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(12.0, 12.0));
        draw_nine_patch(&mut fb, &tex, rect, 2.0, 2.0, 2.0, 2.0, false, Color::WHITE);

        // Corners should be drawn (white)
        assert_eq!(fb.get_pixel(0, 0), Color::WHITE);
        assert_eq!(fb.get_pixel(11, 11), Color::WHITE);
        // Center should remain black (draw_center=false)
        assert_eq!(fb.get_pixel(6, 6), Color::BLACK);
    }

    #[test]
    fn nine_patch_with_modulate() {
        let tex = Texture2D::solid(6, 6, Color::WHITE);
        let mut fb = FrameBuffer::new(12, 12, Color::BLACK);
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(12.0, 12.0));
        let half_green = Color::rgb(0.0, 0.5, 0.0);
        draw_nine_patch(&mut fb, &tex, rect, 2.0, 2.0, 2.0, 2.0, true, half_green);

        let pixel = fb.get_pixel(0, 0);
        assert!((pixel.g - 0.5).abs() < 0.01);
        assert!(pixel.r.abs() < 0.01);
    }

    #[test]
    fn nine_patch_empty_texture() {
        let tex = Texture2D {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0));
        // Should not panic
        draw_nine_patch(&mut fb, &tex, rect, 2.0, 2.0, 2.0, 2.0, true, Color::WHITE);
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn nine_patch_edges_stretch() {
        // 6x6 solid green texture
        let tex = Texture2D::solid(6, 6, Color::rgb(0.0, 1.0, 0.0));
        let mut fb = FrameBuffer::new(20, 20, Color::BLACK);
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0));
        draw_nine_patch(&mut fb, &tex, rect, 2.0, 2.0, 2.0, 2.0, true, Color::WHITE);

        // Top edge (stretched horizontally) should be green
        assert_eq!(fb.get_pixel(10, 0), Color::rgb(0.0, 1.0, 0.0));
        // Left edge (stretched vertically) should be green
        assert_eq!(fb.get_pixel(0, 10), Color::rgb(0.0, 1.0, 0.0));
    }

    #[test]
    fn fill_rotated_rect_zero_angle() {
        let mut fb = FrameBuffer::new(20, 20, Color::BLACK);
        let red = Color::rgb(1.0, 0.0, 0.0);
        // Center at (10,10), half_extents (3,3), no rotation → 6×6 rect.
        fill_rotated_rect(
            &mut fb,
            Vector2::new(10.0, 10.0),
            Vector2::new(3.0, 3.0),
            0.0,
            red,
        );

        // Center pixel should be filled.
        assert_eq!(fb.get_pixel(10, 10), red);
        // Far corner should be black.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn fill_rotated_rect_90_degrees() {
        let mut fb = FrameBuffer::new(20, 20, Color::BLACK);
        let blue = Color::rgb(0.0, 0.0, 1.0);
        // Tall rect (2 wide, 6 tall) rotated 90° → becomes 6 wide, 2 tall.
        fill_rotated_rect(
            &mut fb,
            Vector2::new(10.0, 10.0),
            Vector2::new(1.0, 3.0),
            std::f32::consts::FRAC_PI_2,
            blue,
        );

        // Center should be filled.
        assert_eq!(fb.get_pixel(10, 10), blue);
        // After 90° rotation, the tall axis becomes horizontal.
        // Point 2 units to the right of center should be filled.
        assert_eq!(fb.get_pixel(12, 10), blue);
        // Point 2 units above center should NOT be filled (now only 1 unit thick).
        assert_eq!(fb.get_pixel(10, 8), Color::BLACK);
    }

    #[test]
    fn fill_rotated_rect_45_degrees() {
        let mut fb = FrameBuffer::new(30, 30, Color::BLACK);
        let green = Color::rgb(0.0, 1.0, 0.0);
        fill_rotated_rect(
            &mut fb,
            Vector2::new(15.0, 15.0),
            Vector2::new(4.0, 4.0),
            std::f32::consts::FRAC_PI_4,
            green,
        );

        // Center should be filled.
        assert_eq!(fb.get_pixel(15, 15), green);
        // Corners of the original square far from center should be black.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }
}
