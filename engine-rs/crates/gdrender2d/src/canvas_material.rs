//! CanvasItemMaterial blend modes + texture sampling filter/repeat (pat-u3l8c.30).
//!
//! Previously `CanvasItemMaterial` structs had no rasterizer consumers, sampling
//! was nearest-only, and there were no per-item filter/repeat options. This
//! module provides the pure compositing + sampling primitives a canvas
//! rasterizer needs: [`blend`] for the Godot `BlendMode`s and [`sample_texture`]
//! honoring `texture_filter` (nearest/bilinear) and `texture_repeat`.

use gdcore::math::Color;

use crate::texture::Texture2D;

// The blend/sampling modes are `gdserver2d`'s: a `CanvasItem` carries them, so
// defining a parallel set here would mean a conversion at every call site and
// two enums to keep in step. Re-exported so `canvas_material::BlendMode` still
// resolves for existing callers.
pub use gdserver2d::material::{BlendMode, TextureFilter, TextureRepeat};

/// Composites `src` over `dst` using `mode`.
///
/// `src.a` scales the source contribution (premultiplied on the fly), matching
/// Godot's canvas blend equations.
pub fn blend(src: Color, dst: Color, mode: BlendMode) -> Color {
    let a = src.a.clamp(0.0, 1.0);
    match mode {
        BlendMode::Mix => Color::new(
            src.r * a + dst.r * (1.0 - a),
            src.g * a + dst.g * (1.0 - a),
            src.b * a + dst.b * (1.0 - a),
            (a + dst.a * (1.0 - a)).min(1.0),
        ),
        BlendMode::Add => Color::new(
            (dst.r + src.r * a).min(1.0),
            (dst.g + src.g * a).min(1.0),
            (dst.b + src.b * a).min(1.0),
            dst.a.max(a).min(1.0),
        ),
        BlendMode::Sub => Color::new(
            (dst.r - src.r * a).max(0.0),
            (dst.g - src.g * a).max(0.0),
            (dst.b - src.b * a).max(0.0),
            dst.a,
        ),
        BlendMode::Mul => {
            // lerp(dst, dst*src, src.a)
            let product = Color::new(dst.r * src.r, dst.g * src.g, dst.b * src.b, dst.a);
            dst.lerp(product, a)
        }
        // The source colour is already multiplied by its own alpha, so it is
        // added straight on rather than scaled by `a` again — scaling twice is
        // what makes a premultiplied sprite look washed out.
        BlendMode::PremultAlpha => Color::new(
            (dst.r + src.r).min(1.0),
            (dst.g + src.g).min(1.0),
            (dst.b + src.b).min(1.0),
            (a + dst.a * (1.0 - a)).min(1.0),
        ),
    }
}

fn apply_repeat(coord: f32, repeat: TextureRepeat) -> f32 {
    match repeat {
        // `ParentNode` should have been resolved by the scene bridge before it
        // reaches the rasterizer; clamping is Godot's viewport default, so an
        // unresolved value degrades to the common case rather than tiling
        // unexpectedly.
        TextureRepeat::ParentNode | TextureRepeat::Disabled => coord.clamp(0.0, 1.0),
        TextureRepeat::Enabled => coord.rem_euclid(1.0),
        TextureRepeat::Mirror => {
            let t = coord.rem_euclid(2.0);
            if t > 1.0 {
                2.0 - t
            } else {
                t
            }
        }
    }
}

/// Samples `tex` at normalized `(u, v)` honoring `filter` and `repeat`.
///
/// Returns transparent black for an empty texture.
pub fn sample_texture(
    tex: &Texture2D,
    u: f32,
    v: f32,
    filter: TextureFilter,
    repeat: TextureRepeat,
) -> Color {
    if tex.width == 0 || tex.height == 0 {
        return Color::new(0.0, 0.0, 0.0, 0.0);
    }
    let u = apply_repeat(u, repeat);
    let v = apply_repeat(v, repeat);
    match filter {
        // As above: unresolved `ParentNode` falls back to the viewport default,
        // which Godot ships as LINEAR — but Patina's project default is NEAREST
        // (`default_texture_filter = 1`), so it groups with Nearest here.
        TextureFilter::ParentNode | TextureFilter::Nearest => {
            let x = ((u * tex.width as f32) as u32).min(tex.width - 1);
            let y = ((v * tex.height as f32) as u32).min(tex.height - 1);
            tex.get_pixel(x, y)
        }
        TextureFilter::Linear => {
            // Sample at texel centers.
            let fx = u * tex.width as f32 - 0.5;
            let fy = v * tex.height as f32 - 0.5;
            let x0 = fx.floor();
            let y0 = fy.floor();
            let tx = fx - x0;
            let ty = fy - y0;
            let cx = |x: f32| (x.max(0.0) as u32).min(tex.width - 1);
            let cy = |y: f32| (y.max(0.0) as u32).min(tex.height - 1);
            let (x0i, x1i) = (cx(x0), cx(x0 + 1.0));
            let (y0i, y1i) = (cy(y0), cy(y0 + 1.0));
            let top = tex.get_pixel(x0i, y0i).lerp(tex.get_pixel(x1i, y0i), tx);
            let bot = tex.get_pixel(x0i, y1i).lerp(tex.get_pixel(x1i, y1i), tx);
            top.lerp(bot, ty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn mix_blend_is_source_over() {
        let out = blend(
            Color::new(1.0, 0.0, 0.0, 1.0), // opaque red
            Color::new(0.0, 0.0, 1.0, 1.0), // blue
            BlendMode::Mix,
        );
        assert!(approx(out.r, 1.0) && approx(out.g, 0.0) && approx(out.b, 0.0));
    }

    #[test]
    fn mix_blend_half_alpha_averages() {
        let out = blend(
            Color::new(1.0, 1.0, 1.0, 0.5),
            Color::new(0.0, 0.0, 0.0, 1.0),
            BlendMode::Mix,
        );
        assert!(approx(out.r, 0.5), "got {}", out.r);
    }

    #[test]
    fn additive_blend_brightens() {
        let out = blend(
            Color::new(0.5, 0.5, 0.5, 1.0),
            Color::new(0.5, 0.5, 0.5, 1.0),
            BlendMode::Add,
        );
        assert!(
            approx(out.r, 1.0),
            "additive should reach 1.0, got {}",
            out.r
        );
    }

    #[test]
    fn subtractive_blend_darkens() {
        let out = blend(
            Color::new(0.3, 0.3, 0.3, 1.0),
            Color::new(0.5, 0.5, 0.5, 1.0),
            BlendMode::Sub,
        );
        assert!(approx(out.r, 0.2), "got {}", out.r);
    }

    #[test]
    fn multiply_blend_scales() {
        let out = blend(
            Color::new(0.5, 0.5, 0.5, 1.0),
            Color::new(1.0, 1.0, 1.0, 1.0),
            BlendMode::Mul,
        );
        assert!(approx(out.r, 0.5), "got {}", out.r);
    }

    fn tex2x2() -> Texture2D {
        Texture2D {
            width: 2,
            height: 2,
            pixels: vec![
                Color::new(1.0, 0.0, 0.0, 1.0), // (0,0) red
                Color::new(0.0, 1.0, 0.0, 1.0), // (1,0) green
                Color::new(0.0, 0.0, 1.0, 1.0), // (0,1) blue
                Color::new(1.0, 1.0, 1.0, 1.0), // (1,1) white
            ],
        }
    }

    #[test]
    fn nearest_sampling_picks_texel() {
        let tex = tex2x2();
        let tl = sample_texture(
            &tex,
            0.25,
            0.25,
            TextureFilter::Nearest,
            TextureRepeat::Disabled,
        );
        assert!(
            approx(tl.r, 1.0) && approx(tl.g, 0.0),
            "expected red, got {:?}",
            tl
        );
        let br = sample_texture(
            &tex,
            0.75,
            0.75,
            TextureFilter::Nearest,
            TextureRepeat::Disabled,
        );
        assert!(
            approx(br.r, 1.0) && approx(br.g, 1.0) && approx(br.b, 1.0),
            "expected white"
        );
    }

    #[test]
    fn bilinear_interpolates_between_texels() {
        // 2x1: black then white; sampling the midpoint yields ~grey.
        let tex = Texture2D {
            width: 2,
            height: 1,
            pixels: vec![
                Color::new(0.0, 0.0, 0.0, 1.0),
                Color::new(1.0, 1.0, 1.0, 1.0),
            ],
        };
        let mid = sample_texture(
            &tex,
            0.5,
            0.5,
            TextureFilter::Linear,
            TextureRepeat::Disabled,
        );
        assert!(approx(mid.r, 0.5), "midpoint should be grey, got {}", mid.r);
    }

    #[test]
    fn repeat_enabled_wraps_uv() {
        let tex = Texture2D {
            width: 2,
            height: 1,
            pixels: vec![
                Color::new(0.0, 0.0, 0.0, 1.0),
                Color::new(1.0, 1.0, 1.0, 1.0),
            ],
        };
        let a = sample_texture(
            &tex,
            0.25,
            0.0,
            TextureFilter::Nearest,
            TextureRepeat::Enabled,
        );
        let b = sample_texture(
            &tex,
            1.25,
            0.0,
            TextureFilter::Nearest,
            TextureRepeat::Enabled,
        );
        assert!(
            approx(a.r, b.r) && approx(a.g, b.g) && approx(a.b, b.b),
            "wrap should match"
        );
    }

    #[test]
    fn repeat_disabled_clamps_uv() {
        let tex = Texture2D {
            width: 2,
            height: 1,
            pixels: vec![
                Color::new(0.0, 0.0, 0.0, 1.0),
                Color::new(1.0, 1.0, 1.0, 1.0),
            ],
        };
        // u=2.0 clamps to the last texel (white).
        let c = sample_texture(
            &tex,
            2.0,
            0.0,
            TextureFilter::Nearest,
            TextureRepeat::Disabled,
        );
        assert!(
            approx(c.r, 1.0) && approx(c.g, 1.0) && approx(c.b, 1.0),
            "clamp to last texel"
        );
    }

    #[test]
    fn empty_texture_is_transparent() {
        let tex = Texture2D {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let c = sample_texture(
            &tex,
            0.5,
            0.5,
            TextureFilter::Linear,
            TextureRepeat::Enabled,
        );
        assert!(approx(c.a, 0.0));
    }
}
