//! Drawing primitives operating on a [`FrameBuffer`].
//!
//! All drawing functions clip to the framebuffer bounds automatically.

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdserver2d::canvas::NinePatchAxis;
use gdserver2d::material::{TextureFilter, TextureRepeat};
use gdserver2d::shader::{
    execute_fragment_with_env, FragmentContext, FragmentInstruction, Sampler, ShaderEnv,
};
use gdvariant::variant::Variant;

use crate::canvas_material;
use crate::renderer::FrameBuffer;
use crate::texture::Texture2D;

/// Screen-space AABB of `rect` transformed by `xform`, clamped to the
/// framebuffer, as inclusive-start/exclusive-end pixel bounds.
fn transformed_pixel_bounds(
    fb: &FrameBuffer,
    xform: Transform2D,
    rect: Rect2,
) -> (u32, u32, u32, u32) {
    let corners = [
        xform.xform(rect.position),
        xform.xform(Vector2::new(rect.position.x + rect.size.x, rect.position.y)),
        xform.xform(Vector2::new(rect.position.x, rect.position.y + rect.size.y)),
        xform.xform(Vector2::new(
            rect.position.x + rect.size.x,
            rect.position.y + rect.size.y,
        )),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for c in &corners {
        min_x = min_x.min(c.x);
        min_y = min_y.min(c.y);
        max_x = max_x.max(c.x);
        max_y = max_y.max(c.y);
    }
    (
        (min_x.floor() as i32).max(0) as u32,
        (min_y.floor() as i32).max(0) as u32,
        (max_x.ceil() as i32).max(0).min(fb.width as i32) as u32,
        (max_y.ceil() as i32).max(0).min(fb.height as i32) as u32,
    )
}

/// Fills `rect` under an arbitrary 2D transform (rotation/scale/skew) with
/// source-over alpha blending. Pixels are inverse-mapped from screen space
/// into the rect's local space (pat-u3l8c.24).
pub fn fill_rect_transformed(fb: &mut FrameBuffer, xform: Transform2D, rect: Rect2, color: Color) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 || color.a <= 0.001 {
        return;
    }
    let (x_start, y_start, x_end, y_end) = transformed_pixel_bounds(fb, xform, rect);
    let inv = xform.affine_inverse();
    for py in y_start..y_end {
        for px in x_start..x_end {
            let local = inv.xform(Vector2::new(px as f32 + 0.5, py as f32 + 0.5));
            if local.x >= rect.position.x
                && local.x <= rect.position.x + rect.size.x
                && local.y >= rect.position.y
                && local.y <= rect.position.y + rect.size.y
            {
                fb.blend_pixel(px, py, color);
            }
        }
    }
}

/// Draws `texture` mapped onto `rect` under an arbitrary 2D transform with
/// source-over alpha blending. When `source_rect` is `Some`, texels are
/// sampled from that sub-region (texture-atlas semantics); otherwise the
/// whole texture is mapped onto `rect` (pat-u3l8c.24).
pub fn draw_texture_rect_transformed(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    xform: Transform2D,
    rect: Rect2,
    source_rect: Option<Rect2>,
    modulate: Color,
) {
    // Today's semantics, unchanged: point sampling with texel coordinates
    // clamped to the texture. Every existing caller keeps its exact pixels.
    draw_texture_rect_transformed_filtered(
        fb,
        texture,
        xform,
        rect,
        source_rect,
        modulate,
        TextureFilter::Nearest,
        TextureRepeat::Disabled,
    );
}

/// Resolves a texel coordinate under a [`TextureRepeat`] mode (pat-s3rrv.28).
///
/// `Disabled` clamps to the edge texel, which is what the unfiltered blit has
/// always done — so routing the old path through here cannot move a pixel.
/// `ParentNode` is an *inheritance* marker, not a sampling mode: it must be
/// resolved against the ancestor chain before drawing, and reaching here means
/// nothing resolved it, so it degrades to Godot's default (`Disabled`) rather
/// than silently tiling.
fn wrap_texel(c: i64, size: u32, repeat: TextureRepeat) -> u32 {
    let size_i = size as i64;
    match repeat {
        TextureRepeat::Enabled => c.rem_euclid(size_i) as u32,
        // One period is TWO copies — forward then reversed — so the seam at each
        // boundary shares an edge texel instead of jumping across the texture.
        TextureRepeat::Mirror => {
            let period = size_i * 2;
            let m = c.rem_euclid(period);
            if m < size_i {
                m as u32
            } else {
                (period - 1 - m) as u32
            }
        }
        TextureRepeat::ParentNode | TextureRepeat::Disabled => c.clamp(0, size_i - 1) as u32,
    }
}

/// Samples `texture` at continuous texel coordinates under a filter/repeat pair.
///
/// `Nearest` truncates, matching the historical `as u32` cast exactly. `Linear`
/// samples the 4 neighbours around the texel *centre* (hence the -0.5 bias);
/// each neighbour goes through [`wrap_texel`], so filtering and repeating
/// compose — a bilinear tap that crosses the right edge of a tiling texture
/// wraps to the left edge instead of smearing the border.
fn sample_texture(
    texture: &Texture2D,
    fx: f32,
    fy: f32,
    filter: TextureFilter,
    repeat: TextureRepeat,
) -> Color {
    let at = |x: i64, y: i64| -> Color {
        texture.get_pixel(
            wrap_texel(x, texture.width, repeat),
            wrap_texel(y, texture.height, repeat),
        )
    };
    match filter {
        TextureFilter::Linear => {
            let (gx, gy) = (fx - 0.5, fy - 0.5);
            let (x0, y0) = (gx.floor(), gy.floor());
            let (wx, wy) = (gx - x0, gy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let (c00, c10) = (at(x0, y0), at(x0 + 1, y0));
            let (c01, c11) = (at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let mix = |a: Color, b: Color, t: f32| {
                Color::new(
                    lerp(a.r, b.r, t),
                    lerp(a.g, b.g, t),
                    lerp(a.b, b.b, t),
                    lerp(a.a, b.a, t),
                )
            };
            mix(mix(c00, c10, wx), mix(c01, c11, wx), wy)
        }
        TextureFilter::Nearest | TextureFilter::ParentNode => at(fx as i64, fy as i64),
    }
}

/// [`draw_texture_rect_transformed`] with an explicit texture filter and repeat
/// mode (pat-s3rrv.28).
///
/// Godot resolves `CanvasItem.texture_filter` / `texture_repeat` down the parent
/// chain and applies them per item; the served Play blit had them hardcoded to
/// nearest + clamp, so `texture_filter = LINEAR` and `texture_repeat = ENABLED`
/// were inert in the running game however they were authored.
///
/// ⚠️ `repeat` only changes anything when the sampled coordinates leave the
/// texture — i.e. when `source_rect` is larger than the texture (the tiling
/// case). Inside the rect, `u`/`v` are normalised to 0..1 by construction, so a
/// test that does not oversize `source_rect` cannot tell the modes apart.
#[allow(clippy::too_many_arguments)]
pub fn draw_texture_rect_transformed_filtered(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    xform: Transform2D,
    rect: Rect2,
    source_rect: Option<Rect2>,
    modulate: Color,
    filter: TextureFilter,
    repeat: TextureRepeat,
) {
    if texture.width == 0 || texture.height == 0 {
        return;
    }
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    if let Some(sr) = source_rect {
        if sr.size.x <= 0.0 || sr.size.y <= 0.0 {
            return;
        }
    }

    let (x_start, y_start, x_end, y_end) = transformed_pixel_bounds(fb, xform, rect);
    let inv = xform.affine_inverse();
    for py in y_start..y_end {
        for px in x_start..x_end {
            let local = inv.xform(Vector2::new(px as f32 + 0.5, py as f32 + 0.5));
            let u = (local.x - rect.position.x) / rect.size.x;
            let v = (local.y - rect.position.y) / rect.size.y;
            // Outside the quad entirely (the transformed AABB overshoots on a
            // rotation) — not a repeat case, so it stays a skip for every mode.
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let (fx, fy) = match source_rect {
                Some(sr) => (sr.position.x + u * sr.size.x, sr.position.y + v * sr.size.y),
                None => (u * texture.width as f32, v * texture.height as f32),
            };
            let texel = sample_texture(texture, fx, fy, filter, repeat);
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

/// [`draw_texture_rect_transformed_filtered`] with a canvas_item `fragment()`
/// shader run per covered pixel (pat-s3rrv.28).
///
/// [`draw_texture_rect_shaded`] shades only an **axis-aligned** rect, which is
/// all the dead `scene_bridge` path ever needed. The shipped Play path draws
/// every sprite through its node's global [`Transform2D`], so a rotated or
/// scaled `Sprite2D` carrying a `.gdshader` had no shaded blit to route to at
/// all — the material was dropped and the plain texel drew instead.
///
/// `UV` spans 0..1 across `rect` (Godot's canvas_item UV for a textured quad)
/// and the incoming `COLOR` is the sampled texel × `modulate`, the value
/// Godot's default canvas_item fragment produces before any user code runs. An
/// empty instruction list is a pure passthrough, so this agrees pixel-for-pixel
/// with [`draw_texture_rect_transformed_filtered`] when the material carries no
/// fragment body.
#[allow(clippy::too_many_arguments)]
pub fn draw_texture_rect_transformed_shaded(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    xform: Transform2D,
    rect: Rect2,
    source_rect: Option<Rect2>,
    modulate: Color,
    filter: TextureFilter,
    repeat: TextureRepeat,
    instructions: &[FragmentInstruction],
    uniforms: &HashMap<String, Variant>,
    time: f32,
) {
    if texture.width == 0 || texture.height == 0 {
        return;
    }
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    if let Some(sr) = source_rect {
        if sr.size.x <= 0.0 || sr.size.y <= 0.0 {
            return;
        }
    }

    // Built ONCE per draw call, not per pixel. The closure owns a clone of the
    // texture because `Sampler` is `'static`; its taps go through
    // `canvas_material::sample_texture` so `texture(TEXTURE, UV)` honours the
    // same filter/repeat as the base sample below — sampling raw would make
    // TEXTURE disagree with COLOR.
    let shader_env = ShaderEnv {
        sampler: Some({
            let tex = texture.clone();
            Sampler::new(move |_name, uv| {
                let c = canvas_material::sample_texture(&tex, uv[0], uv[1], filter, repeat);
                [c.r, c.g, c.b, c.a]
            })
        }),
        texture_pixel_size: (
            1.0 / texture.width.max(1) as f32,
            1.0 / texture.height.max(1) as f32,
        ),
    };

    let (x_start, y_start, x_end, y_end) = transformed_pixel_bounds(fb, xform, rect);
    let inv = xform.affine_inverse();
    for py in y_start..y_end {
        for px in x_start..x_end {
            let local = inv.xform(Vector2::new(px as f32 + 0.5, py as f32 + 0.5));
            let u = (local.x - rect.position.x) / rect.size.x;
            let v = (local.y - rect.position.y) / rect.size.y;
            // Outside the quad entirely (the transformed AABB overshoots on a
            // rotation) — the shader must not run for a pixel the item does not
            // cover, or a full-screen `COLOR = tint` would paint the AABB.
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let (fx, fy) = match source_rect {
                Some(sr) => (sr.position.x + u * sr.size.x, sr.position.y + v * sr.size.y),
                None => (u * texture.width as f32, v * texture.height as f32),
            };
            let texel = sample_texture(texture, fx, fy, filter, repeat);
            let base = Color::new(
                texel.r * modulate.r,
                texel.g * modulate.g,
                texel.b * modulate.b,
                texel.a * modulate.a,
            );
            let ctx = FragmentContext {
                color: base,
                uv: (u, v),
                screen_uv: (u, v),
                time,
                // canvas_item shaders have no EYEDIR built-in.
                ..Default::default()
            };
            let out = execute_fragment_with_env(instructions, &ctx, uniforms, &shader_env);
            if out.a > 0.001 {
                fb.blend_pixel(px, py, out);
            }
        }
    }
}

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

/// Invokes `plot(x, y)` for every framebuffer pixel covered by a `width`-thick
/// segment from `from` to `to` — the set of pixels whose center lies within
/// `width / 2` of the segment (a capsule with rounded caps) — clipped to the
/// framebuffer. Backs the wide-line paths of [`draw_line`]/[`draw_line_blended`].
fn for_thick_line_pixels(
    fb_width: u32,
    fb_height: u32,
    from: Vector2,
    to: Vector2,
    width: f32,
    mut plot: impl FnMut(u32, u32),
) {
    let half = (width * 0.5).max(0.5);
    let half_sq = half * half;
    let min_x = (from.x.min(to.x) - half).floor().max(0.0) as u32;
    let min_y = (from.y.min(to.y) - half).floor().max(0.0) as u32;
    let max_x = ((from.x.max(to.x) + half).ceil() as i64).clamp(0, fb_width as i64) as u32;
    let max_y = ((from.y.max(to.y) + half).ceil() as i64).clamp(0, fb_height as i64) as u32;
    let seg = to - from;
    let seg_len_sq = seg.length_squared();
    for py in min_y..max_y {
        for px in min_x..max_x {
            let p = Vector2::new(px as f32 + 0.5, py as f32 + 0.5);
            // Parametric closest point on the (clamped) segment, then radial test.
            let t = if seg_len_sq < 1e-10 {
                0.0
            } else {
                ((p - from).dot(seg) / seg_len_sq).clamp(0.0, 1.0)
            };
            let closest = from + seg * t;
            if (p - closest).length_squared() <= half_sq {
                plot(px, py);
            }
        }
    }
}

/// Draws a line from `from` to `to`.
///
/// Widths `<= 1` use Bresenham's algorithm (a 1-pixel line); wider lines
/// rasterize a `width`-thick capsule so `draw_line`'s width is honored
/// (Godot draws the requested thickness).
pub fn draw_line(fb: &mut FrameBuffer, from: Vector2, to: Vector2, color: Color, width: f32) {
    if width > 1.0 {
        let (w, h) = (fb.width, fb.height);
        for_thick_line_pixels(w, h, from, to, width, |x, y| fb.set_pixel(x, y, color));
        return;
    }
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

    // Copy out before the loop: the sampling modes are read while `fb` is
    // borrowed mutably for the pixel writes.
    let tex_filter = fb.texture_filter;
    let tex_repeat = fb.texture_repeat;
    for py in y_min..y_max {
        for px in x_min..x_max {
            let u = (px as f32 - rect.position.x) / rect.size.x;
            let v = (py as f32 - rect.position.y) / rect.size.y;
            // pat-0q2kx.131: sample through `canvas_material` so the item's
            // resolved texture_filter/texture_repeat apply. This used to be a
            // hardcoded nearest-clamp, which is why LINEAR and ENABLED had no
            // observable effect no matter what a scene authored. The modes ride
            // on the framebuffer (like `scissor`/`blend_mode`) rather than
            // through every texture routine's signature.
            let texel = canvas_material::sample_texture(texture, u, v, tex_filter, tex_repeat);
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

/// [`draw_texture_region`] with alpha blending (source-over).
///
/// The plain region draw **overwrites** each covered pixel, which is right for
/// an opaque blit but wrong for a sprite: a transparent texel would punch a hole
/// in whatever is already there rather than letting it show through. This is the
/// variant a sprite needs once it samples a sub-rect — the region equivalent of
/// [`draw_texture_rect_blended`] (pat-0q2kx.107).
pub fn draw_texture_region_blended(
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
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
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
            if final_color.a > 0.001 {
                fb.blend_pixel(px, py, final_color);
            }
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
/// One axis's tiling plan: `(dst_offset, dst_extent, src_offset, src_extent)`
/// spans that together fill `dst_len` from a `src_len`-wide source region.
///
/// * `Stretch` → a single span scaling source to destination (Godot's default).
/// * `Tile` → whole tiles at the source's natural size, with the final partial
///   tile clipped on *both* sides so it shows a partial slice of the source
///   rather than a squashed whole one.
/// * `TileFit` → the tile size is adjusted to the nearest whole count so no
///   partial tile appears.
fn axis_spans(dst_len: f32, src_len: f32, mode: NinePatchAxis) -> Vec<(f32, f32, f32, f32)> {
    if dst_len <= 0.0 || src_len <= 0.0 {
        return Vec::new();
    }
    match mode {
        NinePatchAxis::Stretch => vec![(0.0, dst_len, 0.0, src_len)],
        NinePatchAxis::Tile | NinePatchAxis::TileFit => {
            let step = if mode == NinePatchAxis::TileFit {
                // Round to at least one whole tile so the span divides evenly.
                let count = (dst_len / src_len).round().max(1.0);
                dst_len / count
            } else {
                src_len
            };
            let mut spans = Vec::new();
            let mut at = 0.0f32;
            // Guard against a pathological step producing an unbounded loop.
            while at < dst_len - 1e-4 && spans.len() < 4096 {
                let remain = dst_len - at;
                let d = step.min(remain);
                // A clipped tile shows a proportional slice of the source.
                let s = src_len * (d / step);
                spans.push((at, d, 0.0, s));
                at += step;
            }
            spans
        }
    }
}

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
    draw_nine_patch_axes(
        fb,
        texture,
        rect,
        margin_left,
        margin_top,
        margin_right,
        margin_bottom,
        draw_center,
        NinePatchAxis::Stretch,
        NinePatchAxis::Stretch,
        modulate,
    );
}

/// [`draw_nine_patch`] honouring Godot's per-axis `AxisStretchMode`
/// (pat-0q2kx.148).
///
/// The modes were previously modelled on `StyleBoxTexture` and dropped before
/// reaching any rasterizer, so `axis_stretch_horizontal = Tile` stretched the
/// edge exactly like the default.
#[allow(clippy::too_many_arguments)]
/// Crops a texture to `region` for nine-patch sampling (pat-s3rrv.167).
///
/// `None` when the region is empty or degenerate, which means "use the whole
/// texture" — Godot's default, and the behaviour every existing scene relies on.
///
/// ⚠️ Implemented as a crop rather than by offsetting the nine-patch source
/// maths. Those coordinates are interdependent — `tw - src_right` is a source
/// WIDTH, not a position, so adding an origin to the obvious places silently
/// produces patches of the wrong size rather than a visible error. Cropping
/// leaves that arithmetic untouched and makes "the patches come from the
/// region" true by construction, at the cost of one copy on the non-default
/// path.
pub fn crop_to_region(texture: &Texture2D, region: Rect2) -> Option<Texture2D> {
    if region.size.x <= 0.0 || region.size.y <= 0.0 {
        return None;
    }
    // Clamp into the texture: a region reaching past the edge samples what is
    // there rather than reading out of bounds.
    let x0 = region.position.x.max(0.0).floor() as u32;
    let y0 = region.position.y.max(0.0).floor() as u32;
    let x1 = ((region.position.x + region.size.x).max(0.0).ceil() as u32).min(texture.width);
    let y1 = ((region.position.y + region.size.y).max(0.0).ceil() as u32).min(texture.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            pixels.push(texture.pixels[(y * texture.width + x) as usize]);
        }
    }
    Some(Texture2D {
        width: w,
        height: h,
        pixels,
    })
}

pub fn draw_nine_patch_axes(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    rect: Rect2,
    margin_left: f32,
    margin_top: f32,
    margin_right: f32,
    margin_bottom: f32,
    draw_center: bool,
    axis_h: NinePatchAxis,
    axis_v: NinePatchAxis,
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

    // Tiling-aware variant: splits the destination into spans per axis and
    // blits each, so `Tile`/`TileFit` repeat the source instead of scaling it
    // (pat-0q2kx.148). `Stretch` on both axes yields exactly one span and is
    // therefore identical to `draw_patch`.
    let mut draw_patch_tiled = |fb: &mut FrameBuffer,
                                dx: f32,
                                dy: f32,
                                dw: f32,
                                dh: f32,
                                sx: f32,
                                sy: f32,
                                sw: f32,
                                sh: f32,
                                mode_h: NinePatchAxis,
                                mode_v: NinePatchAxis| {
        if dw <= 0.0 || dh <= 0.0 || sw <= 0.0 || sh <= 0.0 {
            return;
        }
        for (ox, ew, osx, esw) in axis_spans(dw, sw, mode_h) {
            for (oy, eh, osy, esh) in axis_spans(dh, sh, mode_v) {
                let dst = Rect2::new(Vector2::new(dx + ox, dy + oy), Vector2::new(ew, eh));
                let src = Rect2::new(Vector2::new(sx + osx, sy + osy), Vector2::new(esw, esh));
                draw_texture_region(fb, texture, dst, src, modulate);
            }
        }
    };

    // Top-left corner
    draw_patch(fb, dst_x0, dst_y0, ml, mt, 0.0, 0.0, src_left, src_top);
    // Top edge
    draw_patch_tiled(
        fb,
        dst_x1,
        dst_y0,
        dst_x2 - dst_x1,
        mt,
        src_left,
        0.0,
        src_right - src_left,
        src_top,
        axis_h,
        NinePatchAxis::Stretch,
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
    draw_patch_tiled(
        fb,
        dst_x0,
        dst_y1,
        ml,
        dst_y2 - dst_y1,
        0.0,
        src_top,
        src_left,
        src_bottom - src_top,
        NinePatchAxis::Stretch,
        axis_v,
    );
    // Center
    if draw_center {
        draw_patch_tiled(
            fb,
            dst_x1,
            dst_y1,
            dst_x2 - dst_x1,
            dst_y2 - dst_y1,
            src_left,
            src_top,
            src_right - src_left,
            src_bottom - src_top,
            axis_h,
            axis_v,
        );
    }
    // Right edge
    draw_patch_tiled(
        fb,
        dst_x2,
        dst_y1,
        mr,
        dst_y2 - dst_y1,
        src_right,
        src_top,
        tw - src_right,
        src_bottom - src_top,
        NinePatchAxis::Stretch,
        axis_v,
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
    draw_patch_tiled(
        fb,
        dst_x1,
        dst_y2,
        dst_x2 - dst_x1,
        mb,
        src_left,
        src_bottom,
        src_right - src_left,
        th - src_bottom,
        axis_h,
        NinePatchAxis::Stretch,
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

/// Draws a line with alpha blending (source-over). Widths `> 1` rasterize a
/// `width`-thick capsule; `<= 1` uses a 1-pixel Bresenham line.
pub fn draw_line_blended(
    fb: &mut FrameBuffer,
    from: Vector2,
    to: Vector2,
    color: Color,
    width: f32,
) {
    if width > 1.0 {
        let (w, h) = (fb.width, fb.height);
        for_thick_line_pixels(w, h, from, to, width, |x, y| fb.blend_pixel(x, y, color));
        return;
    }
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

    // Copy out before the loop: the sampling modes are read while `fb` is
    // borrowed mutably for the pixel writes.
    let tex_filter = fb.texture_filter;
    let tex_repeat = fb.texture_repeat;
    for py in y_min..y_max {
        for px in x_min..x_max {
            let u = (px as f32 - rect.position.x) / rect.size.x;
            let v = (py as f32 - rect.position.y) / rect.size.y;
            // pat-0q2kx.131: sample through `canvas_material` so the item's
            // resolved texture_filter/texture_repeat apply. This used to be a
            // hardcoded nearest-clamp, which is why LINEAR and ENABLED had no
            // observable effect no matter what a scene authored. The modes ride
            // on the framebuffer (like `scissor`/`blend_mode`) rather than
            // through every texture routine's signature.
            let texel = canvas_material::sample_texture(texture, u, v, tex_filter, tex_repeat);
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

/// Draws an axis-aligned texture rect with a canvas_item `fragment()` shader
/// applied per pixel (pat-u3l8c.45).
///
/// Mirrors [`draw_texture_rect_blended`], but for each covered pixel it builds a
/// [`FragmentContext`] whose `COLOR` is the sampled texel × `modulate` (Godot's
/// default canvas_item fragment output before any user code), `UV` spans 0..1
/// across `rect`, and `TIME` is `time` seconds, then runs the parsed fragment
/// `instructions`. An empty instruction list is a pure passthrough, producing
/// the same result as [`draw_texture_rect_blended`].
#[allow(clippy::too_many_arguments)]
pub fn draw_texture_rect_shaded(
    fb: &mut FrameBuffer,
    texture: &Texture2D,
    rect: Rect2,
    modulate: Color,
    instructions: &[FragmentInstruction],
    uniforms: &HashMap<String, Variant>,
    time: f32,
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

    // Copy out before the loop: the sampling modes are read while `fb` is
    // borrowed mutably for the pixel writes.
    let tex_filter = fb.texture_filter;
    let tex_repeat = fb.texture_repeat;

    // pat-s3rrv.36: give the shader front end a way to run `texture(TEXTURE, UV)`
    // and read `TEXTURE_PIXEL_SIZE`. Built ONCE per draw call, not per pixel.
    //
    // The closure owns a clone of the texture because `Sampler` is `'static`:
    // capturing the `&Texture2D` borrow would not satisfy that bound. That costs
    // one pixel-buffer copy per draw call, which is why it is hoisted out of the
    // loop. It samples through `canvas_material::sample_texture` so a shader's
    // taps honour the same filter/repeat modes as the base sample below --
    // sampling raw would make TEXTURE disagree with COLOR.
    let shader_env = ShaderEnv {
        sampler: Some({
            let tex = texture.clone();
            Sampler::new(move |_name, uv| {
                let c = canvas_material::sample_texture(&tex, uv[0], uv[1], tex_filter, tex_repeat);
                [c.r, c.g, c.b, c.a]
            })
        }),
        texture_pixel_size: (
            1.0 / texture.width.max(1) as f32,
            1.0 / texture.height.max(1) as f32,
        ),
    };

    for py in y_min..y_max {
        for px in x_min..x_max {
            let u = (px as f32 - rect.position.x) / rect.size.x;
            let v = (py as f32 - rect.position.y) / rect.size.y;
            // pat-0q2kx.131: sample through `canvas_material` so the item's
            // resolved texture_filter/texture_repeat apply. This used to be a
            // hardcoded nearest-clamp, which is why LINEAR and ENABLED had no
            // observable effect no matter what a scene authored. The modes ride
            // on the framebuffer (like `scissor`/`blend_mode`) rather than
            // through every texture routine's signature.
            let texel = canvas_material::sample_texture(texture, u, v, tex_filter, tex_repeat);
            // The incoming COLOR is the sampled texel modulated by the item's
            // tint — the value Godot's default canvas_item fragment produces
            // via `COLOR = texture(TEXTURE, UV) * MODULATE`.
            let base = Color::new(
                texel.r * modulate.r,
                texel.g * modulate.g,
                texel.b * modulate.b,
                texel.a * modulate.a,
            );

            let ctx = FragmentContext {
                color: base,
                uv: (u, v),
                screen_uv: (u, v),
                time,
                // canvas_item shaders have no EYEDIR built-in.
                ..Default::default()
            };
            let out = execute_fragment_with_env(instructions, &ctx, uniforms, &shader_env);
            if out.a > 0.001 {
                fb.blend_pixel(px, py, out);
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

    // -- pat-s3rrv.28: texture filter + repeat in the blit -------------------

    /// A 2x1 texture: red at texel 0, blue at texel 1.
    fn red_blue_strip() -> Texture2D {
        Texture2D {
            width: 2,
            height: 1,
            pixels: vec![Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0)],
        }
    }

    /// Draws the strip into a 4px row through a `source_rect` spanning FOUR
    /// texels — two tiles.
    ///
    /// ⚠️ The oversize source rect is what makes the repeat modes
    /// distinguishable at all: inside the quad `u`/`v` are normalised to 0..1
    /// by construction, so with a 1:1 source rect every mode samples the same
    /// texels and the test would pass no matter what `repeat` did.
    fn strip_reds(filter: TextureFilter, repeat: TextureRepeat) -> Vec<bool> {
        let mut fb = FrameBuffer::new(4, 1, Color::BLACK);
        draw_texture_rect_transformed_filtered(
            &mut fb,
            &red_blue_strip(),
            Transform2D::IDENTITY,
            Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 1.0)),
            Some(Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 1.0))),
            Color::WHITE,
            filter,
            repeat,
        );
        (0..4).map(|x| fb.get_pixel(x, 0).r > 0.5).collect()
    }

    #[test]
    fn texture_repeat_enabled_tiles_where_disabled_smears_the_edge() {
        // DISABLED clamps: texels 2 and 3 fall off the right edge, so the second
        // tile repeats the last texel — red, blue, blue, blue.
        assert_eq!(
            strip_reds(TextureFilter::Nearest, TextureRepeat::Disabled),
            vec![true, false, false, false],
            "DISABLED must smear the edge texel across the second tile"
        );

        // ENABLED wraps, so the pattern repeats — red, blue, red, blue.
        assert_eq!(
            strip_reds(TextureFilter::Nearest, TextureRepeat::Enabled),
            vec![true, false, true, false],
            "ENABLED must tile the texture instead of clamping"
        );
    }

    #[test]
    fn texture_repeat_mirror_reverses_every_other_copy() {
        // red, blue | blue, red — the seam shares an edge texel.
        assert_eq!(
            strip_reds(TextureFilter::Nearest, TextureRepeat::Mirror),
            vec![true, false, false, true],
            "MIRROR must reverse the second copy"
        );
    }

    #[test]
    fn texture_filter_linear_blends_between_texels() {
        // Magnified 2x with NO source rect, so pixel 1 samples a quarter of the
        // way from red toward blue rather than landing on a texel centre.
        let draw = |filter: TextureFilter| {
            let mut fb = FrameBuffer::new(4, 1, Color::BLACK);
            draw_texture_rect_transformed_filtered(
                &mut fb,
                &red_blue_strip(),
                Transform2D::IDENTITY,
                Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 1.0)),
                None,
                Color::WHITE,
                filter,
                TextureRepeat::Disabled,
            );
            fb.get_pixel(1, 0)
        };

        let nearest = draw(TextureFilter::Nearest);
        assert!(
            nearest.r > 0.99 && nearest.b < 0.01,
            "NEAREST must pick a single texel outright, got r={} b={}",
            nearest.r,
            nearest.b
        );

        let linear = draw(TextureFilter::Linear);
        assert!(
            linear.r > 0.01 && linear.r < 0.99 && linear.b > 0.01,
            "LINEAR must blend the neighbouring texels, got r={} b={}",
            linear.r,
            linear.b
        );
    }

    /// The regression guard for the refactor itself: every existing caller goes
    /// through the delegating wrapper, so it must produce byte-identical pixels
    /// to the explicit nearest/clamp pair.
    #[test]
    fn the_unfiltered_entry_point_keeps_its_historical_pixels() {
        let tex = red_blue_strip();
        let rect = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 1.0));
        let src = Some(Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 1.0)));

        let mut old = FrameBuffer::new(4, 1, Color::BLACK);
        draw_texture_rect_transformed(
            &mut old,
            &tex,
            Transform2D::IDENTITY,
            rect,
            src,
            Color::WHITE,
        );

        let mut explicit = FrameBuffer::new(4, 1, Color::BLACK);
        draw_texture_rect_transformed_filtered(
            &mut explicit,
            &tex,
            Transform2D::IDENTITY,
            rect,
            src,
            Color::WHITE,
            TextureFilter::Nearest,
            TextureRepeat::Disabled,
        );

        for x in 0..4 {
            let (a, b) = (old.get_pixel(x, 0), explicit.get_pixel(x, 0));
            assert!(
                (a.r - b.r).abs() < 1e-6 && (a.g - b.g).abs() < 1e-6 && (a.b - b.b).abs() < 1e-6,
                "the delegating wrapper moved a pixel at x={x}"
            );
        }
    }

    // pat-nzgdu.71: draw_line / draw_line_blended must honor their width
    // argument (a 6px line paints a band, not a single Bresenham row).
    #[test]
    fn draw_line_honors_width() {
        let red = Color::rgb(1.0, 0.0, 0.0);

        // A horizontal 6px line at y=10, x∈[5,25] on a 40×40 black buffer:
        // sampling the middle column x=15 should hit ~6 painted rows.
        let mut fb = FrameBuffer::new(40, 40, Color::BLACK);
        draw_line(
            &mut fb,
            Vector2::new(5.0, 10.0),
            Vector2::new(25.0, 10.0),
            red,
            6.0,
        );
        let thick_rows = (0..40u32).filter(|&y| fb.get_pixel(15, y).r > 0.5).count();
        assert!(
            (5..=7).contains(&thick_rows),
            "a 6px line must paint ~6 rows, got {thick_rows}"
        );

        // A width-1 line stays a single row (thin Bresenham path unchanged).
        let mut fb_thin = FrameBuffer::new(40, 40, Color::BLACK);
        draw_line(
            &mut fb_thin,
            Vector2::new(5.0, 20.0),
            Vector2::new(25.0, 20.0),
            red,
            1.0,
        );
        let thin_rows = (0..40u32)
            .filter(|&y| fb_thin.get_pixel(15, y).r > 0.5)
            .count();
        assert_eq!(thin_rows, 1, "a 1px line must stay 1 row, got {thin_rows}");

        // The blended variant honors width too.
        let mut fb_b = FrameBuffer::new(40, 40, Color::BLACK);
        draw_line_blended(
            &mut fb_b,
            Vector2::new(5.0, 30.0),
            Vector2::new(25.0, 30.0),
            red,
            6.0,
        );
        let blend_rows = (0..40u32)
            .filter(|&y| fb_b.get_pixel(15, y).r > 0.5)
            .count();
        assert!(
            (5..=7).contains(&blend_rows),
            "a 6px blended line must paint ~6 rows, got {blend_rows}"
        );
    }

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

#[cfg(test)]
mod nine_patch_axis_tests {
    use super::*;

    /// `Stretch` is one span covering the whole destination — the pre-existing
    /// behaviour, which must be untouched.
    #[test]
    fn stretch_is_a_single_span() {
        let spans = axis_spans(100.0, 10.0, NinePatchAxis::Stretch);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0], (0.0, 100.0, 0.0, 10.0));
    }

    /// `Tile` repeats at the source's natural size. 100 / 10 = 10 whole tiles.
    #[test]
    fn tile_repeats_at_natural_size() {
        let spans = axis_spans(100.0, 10.0, NinePatchAxis::Tile);
        assert_eq!(spans.len(), 10, "ten whole tiles, got {spans:?}");
        for (i, (off, ext, _, sext)) in spans.iter().enumerate() {
            assert_eq!(*off, i as f32 * 10.0);
            assert_eq!(*ext, 10.0, "each tile keeps the source size");
            assert_eq!(*sext, 10.0, "and samples the whole source");
        }
    }

    /// A destination that is not a whole multiple clips the final tile on BOTH
    /// sides — a partial destination showing a *partial* source, not a squashed
    /// whole one. Getting this wrong is the classic nine-patch tiling artefact.
    #[test]
    fn tile_clips_the_final_partial_tile_proportionally() {
        let spans = axis_spans(25.0, 10.0, NinePatchAxis::Tile);
        assert_eq!(spans.len(), 3, "two whole tiles plus a half, got {spans:?}");
        assert_eq!(spans[2].1, 5.0, "final destination extent is the remainder");
        assert_eq!(
            spans[2].3, 5.0,
            "and it samples only half the source, so the tile is not squashed"
        );
        // The spans exactly tile the destination with no gap or overlap.
        let covered: f32 = spans.iter().map(|s| s.1).sum();
        assert_eq!(covered, 25.0);
    }

    /// `TileFit` adjusts the tile size so a whole number fits — no partial tile.
    #[test]
    fn tile_fit_divides_evenly_with_no_partial_tile() {
        // 25 / 10 rounds to 3 tiles ⇒ step 8.333…
        let spans = axis_spans(25.0, 10.0, NinePatchAxis::TileFit);
        assert_eq!(spans.len(), 3);
        for (_, ext, _, sext) in &spans {
            assert!(
                (*ext - 25.0 / 3.0).abs() < 1e-3,
                "every TileFit tile is the same adjusted size, got {ext}"
            );
            assert!(
                (*sext - 10.0).abs() < 1e-3,
                "and samples the FULL source — that is what makes it fit \
                 rather than clip, got {sext}"
            );
        }
        let covered: f32 = spans.iter().map(|s| s.1).sum();
        assert!((covered - 25.0).abs() < 1e-3);
    }

    /// Degenerate spans yield nothing rather than looping forever.
    #[test]
    fn degenerate_spans_are_empty() {
        assert!(axis_spans(0.0, 10.0, NinePatchAxis::Tile).is_empty());
        assert!(axis_spans(10.0, 0.0, NinePatchAxis::Tile).is_empty());
        assert!(axis_spans(-5.0, 10.0, NinePatchAxis::Tile).is_empty());
    }

    /// End to end: a tiled edge paints a repeating pattern where a stretched one
    /// paints a smooth ramp. Uses a 2-colour source so repetition is visible.
    #[test]
    fn tiled_edge_repeats_where_stretched_edge_does_not() {
        // 4x4 texture, 1px margins ⇒ the top edge source is 2px wide.
        // Left source pixel red, right source pixel blue.
        let mut pixels = vec![Color::new(0.0, 0.0, 0.0, 1.0); 16];
        for y in 0..4 {
            pixels[y * 4 + 1] = Color::new(1.0, 0.0, 0.0, 1.0);
            pixels[y * 4 + 2] = Color::new(0.0, 0.0, 1.0, 1.0);
        }
        let tex = Texture2D {
            width: 4,
            height: 4,
            pixels,
        };
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(20.0, 6.0));

        let render = |mode: NinePatchAxis| {
            let mut fb = FrameBuffer::new(20, 6, Color::new(0.0, 0.0, 0.0, 1.0));
            draw_nine_patch_axes(
                &mut fb,
                &tex,
                rect,
                1.0,
                1.0,
                1.0,
                1.0,
                true,
                mode,
                NinePatchAxis::Stretch,
                Color::WHITE,
            );
            // Count red→blue transitions along the top edge row.
            let mut flips = 0;
            for x in 2..17 {
                let a = fb.pixels[x];
                let b = fb.pixels[x + 1];
                if (a.r > 0.5) != (b.r > 0.5) {
                    flips += 1;
                }
            }
            flips
        };

        let stretched = render(NinePatchAxis::Stretch);
        let tiled = render(NinePatchAxis::Tile);
        assert!(
            tiled > stretched,
            "a tiled edge must alternate more often than a stretched one \
             (tiled {tiled} flips vs stretched {stretched})"
        );
    }
}
