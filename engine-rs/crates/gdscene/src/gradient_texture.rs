//! `GradientTexture2D` generated to pixels for 3D materials (pat-cezo4).
//!
//! A `.tscn` that gives a StandardMaterial3D an inline
//! `albedo_texture = SubResource("GradientTexture2D_x")` has no file to load:
//! Godot builds the image from the texture's `gradient`, `fill`,
//! `fill_from`/`fill_to`, `repeat` and size (`scene/resources/gradient_texture.cpp`,
//! `GradientTexture2D::_update`). The official 3D platformer's coin glow is
//! exactly that — a radial white-to-transparent gradient on an additive quad.
//!
//! By render time the `PackedScene` (and the texture's sub-resource body) is
//! gone, so the scene loader writes a self-contained DESCRIPTOR string onto the
//! node (`__mattex__<slot>`, see `packed_scene`), and
//! [`MaterialTextureCache`](crate::render_server_3d::MaterialTextureCache)
//! generates the pixels from it once.

use std::collections::HashMap;

use gdcore::math::{Color, Vector2};
use gdvariant::Variant;

/// Prefix of a gradient-texture descriptor.
pub const DESCRIPTOR_PREFIX: &str = "gradient2d:";

fn num(v: Option<&Variant>, default: f64) -> f64 {
    match v {
        Some(Variant::Float(f)) => *f,
        Some(Variant::Int(i)) => *i as f64,
        _ => default,
    }
}

fn vec2(v: Option<&Variant>, default: Vector2) -> Vector2 {
    match v {
        Some(Variant::Vector2(p)) => *p,
        _ => default,
    }
}

/// The descriptor for a `GradientTexture2D` with property bag `texture`, whose
/// `gradient` sub-resource has property bag `gradient` (`None`: Godot's default
/// gradient, black to white).
pub fn descriptor(
    texture: &HashMap<String, Variant>,
    gradient: Option<&HashMap<String, Variant>>,
) -> String {
    let width = num(texture.get("width"), 64.0) as u32;
    let height = num(texture.get("height"), 64.0) as u32;
    let fill = num(texture.get("fill"), 0.0) as i64;
    let repeat = num(texture.get("repeat"), 0.0) as i64;
    let from = vec2(texture.get("fill_from"), Vector2::new(0.0, 0.0));
    let to = vec2(texture.get("fill_to"), Vector2::new(1.0, 0.0));
    let (offsets, colors, interp) = match gradient {
        Some(g) => {
            let offsets: Vec<f32> = match g.get("offsets") {
                Some(Variant::PackedFloat32Array(o)) => o.to_vec(),
                _ => vec![0.0, 1.0],
            };
            let colors: Vec<f32> = match g.get("colors") {
                Some(Variant::PackedColorArray(c)) => {
                    c.iter().flat_map(|c| [c.r, c.g, c.b, c.a]).collect()
                }
                _ => vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            };
            (
                offsets,
                colors,
                num(g.get("interpolation_mode"), 0.0) as i64,
            )
        }
        None => (
            vec![0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            0,
        ),
    };
    let join = |v: &[f32]| {
        v.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{DESCRIPTOR_PREFIX}w={width};h={height};fill={fill};repeat={repeat};from={},{};to={},{};interp={interp};offsets={};colors={}",
        from.x,
        from.y,
        to.x,
        to.y,
        join(&offsets),
        join(&colors)
    )
}

/// Godot's `Math::cubic_interpolate` (Catmull-Rom).
fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}

/// `Gradient::get_color_at_offset` for sorted `offsets` / `colors`.
fn gradient_at(offsets: &[f32], colors: &[Color], interp: i64, t: f32) -> Color {
    if colors.is_empty() {
        return Color::new(0.0, 0.0, 0.0, 1.0);
    }
    if t <= offsets[0] || colors.len() == 1 {
        return colors[0];
    }
    let last = colors.len() - 1;
    if t >= offsets[last] {
        return colors[last];
    }
    let i = offsets
        .windows(2)
        .position(|w| t >= w[0] && t < w[1])
        .unwrap_or(0);
    let span = (offsets[i + 1] - offsets[i]).max(1e-6);
    let f = (t - offsets[i]) / span;
    match interp {
        // INTERPOLATION_CONSTANT
        1 => colors[i],
        // INTERPOLATION_CUBIC
        2 => {
            let c0 = colors[i.saturating_sub(1)];
            let (c1, c2) = (colors[i], colors[i + 1]);
            let c3 = colors[(i + 2).min(last)];
            Color::new(
                cubic(c0.r, c1.r, c2.r, c3.r, f),
                cubic(c0.g, c1.g, c2.g, c3.g, f),
                cubic(c0.b, c1.b, c2.b, c3.b, f),
                cubic(c0.a, c1.a, c2.a, c3.a, f),
            )
        }
        // INTERPOLATION_LINEAR
        _ => {
            let (a, b) = (colors[i], colors[i + 1]);
            Color::new(
                a.r + (b.r - a.r) * f,
                a.g + (b.g - a.g) * f,
                a.b + (b.b - a.b) * f,
                a.a + (b.a - a.a) * f,
            )
        }
    }
}

/// Generates the texture a descriptor describes: `(width, height, pixels)`,
/// texels as authored (sRGB-encoded), like every cached material texture. `None` for a string that is not a valid descriptor.
pub fn generate(descriptor: &str) -> Option<(u32, u32, Vec<Color>)> {
    let body = descriptor.strip_prefix(DESCRIPTOR_PREFIX)?;
    let fields: HashMap<&str, &str> = body.split(';').filter_map(|f| f.split_once('=')).collect();
    let nums = |k: &str| -> Vec<f32> {
        fields
            .get(k)
            .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
            .unwrap_or_default()
    };
    let one = |k: &str| nums(k).first().copied();
    let (w, h) = (one("w")? as u32, one("h")? as u32);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let (fill, repeat, interp) = (
        one("fill")? as i64,
        one("repeat")? as i64,
        one("interp")? as i64,
    );
    let (from, to) = (nums("from"), nums("to"));
    let (from, to) = (
        Vector2::new(*from.first()?, *from.get(1)?),
        Vector2::new(*to.first()?, *to.get(1)?),
    );
    let mut stops: Vec<(f32, Color)> = nums("offsets")
        .into_iter()
        .zip(
            nums("colors")
                .chunks_exact(4)
                .map(|c| Color::new(c[0], c[1], c[2], c[3]))
                .collect::<Vec<_>>(),
        )
        .collect();
    // Gradient keeps its points sorted by offset.
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let offsets: Vec<f32> = stops.iter().map(|s| s.0).collect();
    let colors: Vec<Color> = stops.iter().map(|s| s.1).collect();

    let axis = Vector2::new(to.x - from.x, to.y - from.y);
    let len_sq = (axis.x * axis.x + axis.y * axis.y).max(1e-12);
    let len = len_sq.sqrt();
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            // `GradientTexture2D::_get_gradient_offset_at`: the pixel's UV.
            let uv = Vector2::new(x as f32 / w as f32, y as f32 / h as f32);
            let d = Vector2::new(uv.x - from.x, uv.y - from.y);
            let mut t = match fill {
                // FILL_RADIAL
                1 => (d.x * d.x + d.y * d.y).sqrt() / len,
                // FILL_SQUARE
                2 => d.x.abs().max(d.y.abs()) / len,
                // FILL_LINEAR
                _ => (d.x * axis.x + d.y * axis.y) / len_sq,
            };
            t = match repeat {
                // REPEAT
                1 => t - t.floor(),
                // REPEAT_MIRROR
                2 => {
                    let m = t.abs() % 2.0;
                    if m > 1.0 {
                        2.0 - m
                    } else {
                        m
                    }
                }
                _ => t.clamp(0.0, 1.0),
            };
            let c = gradient_at(&offsets, &colors, interp, t);
            // Godot stores the gradient's colours as the texels, as authored;
            // the renderers decode colour maps at sample time.
            pixels.push(Color::new(c.r, c.g, c.b, c.a.clamp(0.0, 1.0)));
        }
    }
    Some((w, h, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The coin glow of the official 3D platformer (`coin/coin.tscn`): a
    /// radial fill from the centre, opaque at the centre and fully
    /// transparent at the rim and the corners.
    #[test]
    fn radial_gradient_is_opaque_at_the_centre_and_clear_at_the_rim() {
        let mut tex = HashMap::new();
        tex.insert("fill".into(), Variant::Int(1));
        tex.insert("fill_from".into(), Variant::Vector2(Vector2::new(0.5, 0.5)));
        tex.insert("fill_to".into(), Variant::Vector2(Vector2::new(0.5, 0.01)));
        let mut grad = HashMap::new();
        grad.insert("interpolation_mode".into(), Variant::Int(2));
        grad.insert(
            "offsets".into(),
            Variant::PackedFloat32Array(vec![0.0, 0.642276, 1.0].into()),
        );
        grad.insert(
            "colors".into(),
            Variant::PackedColorArray(
                vec![
                    Color::new(1.0, 1.0, 1.0, 1.0),
                    Color::new(1.0, 1.0, 1.0, 0.180392),
                    Color::new(1.0, 1.0, 1.0, 0.0),
                ]
                .into(),
            ),
        );
        let (w, h, px) = generate(&descriptor(&tex, Some(&grad))).expect("valid descriptor");
        assert_eq!((w, h), (64, 64));
        let at = |x: u32, y: u32| px[(y * w + x) as usize];
        assert!(at(32, 32).a > 0.98, "centre {:?}", at(32, 32));
        assert_eq!(at(0, 0).a, 0.0, "corner is beyond the radius");
        assert_eq!(at(32, 0).a.max(0.0) < 0.02, true, "rim {:?}", at(32, 0));
        // Monotone falloff along the radius.
        assert!(at(32, 20).a > at(32, 10).a);
        // White texels.
        assert!((at(32, 32).r - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_string_that_is_not_a_descriptor_generates_nothing() {
        assert!(generate("res://x.png").is_none());
        assert!(generate("gradient2d:w=0;h=4").is_none());
    }
}
