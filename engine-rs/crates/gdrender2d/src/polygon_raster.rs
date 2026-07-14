//! Polygon and thick-polyline rasterization (pat-u3l8c.27).
//!
//! `draw.rs` only had rect/circle/1px-line primitives; Godot's `Polygon2D` and
//! `Line2D` need filled polygons (with optional UV texturing) and thick
//! polylines with caps and joints. This module adds those as scanline
//! rasterizers operating directly on a [`FrameBuffer`].

use gdcore::math::{Color, Vector2};

use crate::renderer::FrameBuffer;
use crate::texture::Texture2D;

/// End-cap style for [`draw_polyline`] (mirrors Godot `Line2D.LineCapMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    /// Flat cap flush with the endpoint.
    None,
    /// Rounded cap extending a half-width past the endpoint.
    Round,
}

/// Joint style between polyline segments (mirrors Godot `Line2D.LineJointMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoint {
    /// Segments meet with no extra fill (may leave a notch on sharp turns).
    Sharp,
    /// A disc is drawn at each interior vertex for a smooth round joint.
    Round,
}

/// Scanline even-odd fill of a polygon with a solid `color`.
///
/// Points are in pixel space; the polygon is implicitly closed. Pixels are
/// sampled at their center (`y + 0.5`) so a rectangle on integer bounds fills
/// exactly its interior.
pub fn fill_polygon(fb: &mut FrameBuffer, points: &[Vector2], color: Color) {
    let n = points.len();
    if n < 3 {
        return;
    }
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for p in points {
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    let y_start = min_y.floor().max(0.0) as i32;
    let y_end = max_y.ceil().min((fb.height as f32) - 1.0) as i32;

    for y in y_start..=y_end {
        let yc = y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();
        for i in 0..n {
            let a = points[i];
            let b = points[(i + 1) % n];
            if (a.y <= yc && b.y > yc) || (b.y <= yc && a.y > yc) {
                let t = (yc - a.y) / (b.y - a.y);
                xs.push(a.x + t * (b.x - a.x));
            }
        }
        xs.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            let xl = xs[i].ceil().max(0.0) as i32;
            let xr = xs[i + 1].floor().min((fb.width as f32) - 1.0) as i32;
            for x in xl..=xr {
                fb.set_pixel(x as u32, y as u32, color);
            }
            i += 2;
        }
    }
}

/// Scanline fill of a polygon sampling `texture` through per-vertex UVs.
///
/// `uvs` (in `[0,1]`) must be parallel to `points`. UV is interpolated along
/// polygon edges to each scanline crossing, then linearly across each span, and
/// nearest-sampled from the texture. Silently no-ops on a malformed polygon.
pub fn fill_polygon_textured(
    fb: &mut FrameBuffer,
    points: &[Vector2],
    uvs: &[Vector2],
    texture: &Texture2D,
) {
    let n = points.len();
    if n < 3 || uvs.len() != n || texture.width == 0 || texture.height == 0 {
        return;
    }
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for p in points {
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    let y_start = min_y.floor().max(0.0) as i32;
    let y_end = max_y.ceil().min((fb.height as f32) - 1.0) as i32;

    for y in y_start..=y_end {
        let yc = y as f32 + 0.5;
        // (x, u, v) crossings.
        let mut xs: Vec<(f32, f32, f32)> = Vec::new();
        for i in 0..n {
            let a = points[i];
            let b = points[(i + 1) % n];
            if (a.y <= yc && b.y > yc) || (b.y <= yc && a.y > yc) {
                let t = (yc - a.y) / (b.y - a.y);
                let ua = uvs[i];
                let ub = uvs[(i + 1) % n];
                xs.push((
                    a.x + t * (b.x - a.x),
                    ua.x + t * (ub.x - ua.x),
                    ua.y + t * (ub.y - ua.y),
                ));
            }
        }
        xs.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            let (x0, u0, v0) = xs[i];
            let (x1, u1, v1) = xs[i + 1];
            let xl = x0.ceil().max(0.0) as i32;
            let xr = x1.floor().min((fb.width as f32) - 1.0) as i32;
            let dx = x1 - x0;
            for x in xl..=xr {
                let s = if dx.abs() > 1e-6 {
                    ((x as f32 + 0.5) - x0) / dx
                } else {
                    0.0
                };
                let u = (u0 + s * (u1 - u0)).clamp(0.0, 1.0);
                let v = (v0 + s * (v1 - v0)).clamp(0.0, 1.0);
                let tx = ((u * (texture.width as f32 - 1.0)).round() as u32).min(texture.width - 1);
                let ty =
                    ((v * (texture.height as f32 - 1.0)).round() as u32).min(texture.height - 1);
                fb.set_pixel(x as u32, y as u32, texture.get_pixel(tx, ty));
            }
            i += 2;
        }
    }
}

/// Draws a thick polyline through `points` with the given `width`.
///
/// Each segment is rasterized as a filled quad a half-width to either side of
/// the segment; `joint` controls interior-vertex fill and `cap` the endpoints.
/// When `closed` is true the last point connects back to the first (and caps
/// are not applied).
pub fn draw_polyline(
    fb: &mut FrameBuffer,
    points: &[Vector2],
    width: f32,
    color: Color,
    closed: bool,
    cap: LineCap,
    joint: LineJoint,
) {
    let n = points.len();
    if n < 2 || width <= 0.0 {
        return;
    }
    let hw = width * 0.5;
    let seg_count = if closed { n } else { n - 1 };
    for i in 0..seg_count {
        let a = points[i];
        let b = points[(i + 1) % n];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            continue;
        }
        // Perpendicular offset scaled to the half-width.
        let ox = -dy / len * hw;
        let oy = dx / len * hw;
        let quad = [
            Vector2::new(a.x + ox, a.y + oy),
            Vector2::new(b.x + ox, b.y + oy),
            Vector2::new(b.x - ox, b.y - oy),
            Vector2::new(a.x - ox, a.y - oy),
        ];
        fill_polygon(fb, &quad, color);
    }

    // Round joints: a disc at each interior vertex smooths sharp turns.
    if joint == LineJoint::Round {
        let (start, end) = if closed { (0, n) } else { (1, n - 1) };
        for i in start..end {
            crate::draw::fill_circle(fb, points[i % n], hw, color);
        }
    }

    // Round caps: a disc at each open endpoint.
    if !closed && cap == LineCap::Round {
        crate::draw::fill_circle(fb, points[0], hw, color);
        crate::draw::fill_circle(fb, points[n - 1], hw, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() -> Color {
        Color::new(0.0, 0.0, 0.0, 1.0)
    }
    fn red() -> Color {
        Color::new(1.0, 0.0, 0.0, 1.0)
    }
    fn approx(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-3
            && (a.g - b.g).abs() < 1e-3
            && (a.b - b.b).abs() < 1e-3
            && (a.a - b.a).abs() < 1e-3
    }

    #[test]
    fn fills_square_interior_not_exterior() {
        let mut fb = FrameBuffer::new(20, 20, clear());
        let square = [
            Vector2::new(5.0, 5.0),
            Vector2::new(15.0, 5.0),
            Vector2::new(15.0, 15.0),
            Vector2::new(5.0, 15.0),
        ];
        fill_polygon(&mut fb, &square, red());
        // Center is filled.
        assert!(approx(fb.get_pixel(10, 10), red()));
        // A corner well outside is untouched.
        assert!(approx(fb.get_pixel(1, 1), clear()));
        assert!(approx(fb.get_pixel(18, 18), clear()));
    }

    #[test]
    fn fills_triangle_interior() {
        let mut fb = FrameBuffer::new(20, 20, clear());
        let tri = [
            Vector2::new(10.0, 2.0),
            Vector2::new(2.0, 18.0),
            Vector2::new(18.0, 18.0),
        ];
        fill_polygon(&mut fb, &tri, red());
        // Interior point near the base center.
        assert!(approx(fb.get_pixel(10, 14), red()));
        // Outside the triangle (top corners).
        assert!(approx(fb.get_pixel(2, 3), clear()));
    }

    #[test]
    fn degenerate_polygon_is_noop() {
        let mut fb = FrameBuffer::new(8, 8, clear());
        fill_polygon(&mut fb, &[Vector2::new(1.0, 1.0), Vector2::new(4.0, 4.0)], red());
        assert!(approx(fb.get_pixel(2, 2), clear()));
    }

    #[test]
    fn textured_fill_samples_solid_texture() {
        let mut fb = FrameBuffer::new(16, 16, clear());
        let tex = Texture2D::solid(4, 4, Color::new(0.0, 1.0, 0.0, 1.0));
        let quad = [
            Vector2::new(2.0, 2.0),
            Vector2::new(14.0, 2.0),
            Vector2::new(14.0, 14.0),
            Vector2::new(2.0, 14.0),
        ];
        let uvs = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(0.0, 1.0),
        ];
        fill_polygon_textured(&mut fb, &quad, &uvs, &tex);
        // Interior takes the texture's color.
        assert!(approx(fb.get_pixel(8, 8), Color::new(0.0, 1.0, 0.0, 1.0)));
        // Outside the quad is untouched.
        assert!(approx(fb.get_pixel(0, 0), clear()));
    }

    #[test]
    fn thick_horizontal_line_covers_its_width() {
        let mut fb = FrameBuffer::new(40, 20, clear());
        let pts = [Vector2::new(5.0, 10.0), Vector2::new(35.0, 10.0)];
        draw_polyline(&mut fb, &pts, 6.0, red(), false, LineCap::None, LineJoint::Sharp);
        // Centerline and points within +/- (width/2) are filled.
        assert!(approx(fb.get_pixel(20, 10), red()));
        assert!(approx(fb.get_pixel(20, 8), red()));
        assert!(approx(fb.get_pixel(20, 12), red()));
        // Well outside the band is clear.
        assert!(approx(fb.get_pixel(20, 3), clear()));
        assert!(approx(fb.get_pixel(20, 17), clear()));
    }

    #[test]
    fn round_cap_extends_past_endpoint() {
        let mut fb = FrameBuffer::new(40, 20, clear());
        let pts = [Vector2::new(10.0, 10.0), Vector2::new(30.0, 10.0)];
        // Without a cap the endpoint column past x=30 is empty; with a round
        // cap the disc bulges past it.
        draw_polyline(&mut fb, &pts, 8.0, red(), false, LineCap::Round, LineJoint::Sharp);
        assert!(approx(fb.get_pixel(32, 10), red()), "round cap should extend past the endpoint");
    }

    #[test]
    fn thin_or_zero_width_line_is_noop() {
        let mut fb = FrameBuffer::new(10, 10, clear());
        let pts = [Vector2::new(1.0, 5.0), Vector2::new(9.0, 5.0)];
        draw_polyline(&mut fb, &pts, 0.0, red(), false, LineCap::None, LineJoint::Sharp);
        assert!(approx(fb.get_pixel(5, 5), clear()));
    }
}
