//! `Gradient`'s method surface over a `Variant::Resource` property bag
//! (pat-nyn3w).
//!
//! truck_town's `speedometer.gd` tints its label every frame with
//! `tint_gradient.sample(remap(speed, 0.0, 30.0, 0.0, 1.0))`, an
//! `@export var tint_gradient: Gradient` the scene authors as an inline
//! `SubResource`. There was no `Gradient` method at all, so this raised every
//! frame once the resource was readable.
//!
//! A port of `scene/resources/gradient.h` `get_color_at_offset` (Godot 4.6.1),
//! in `f32` like Godot so the numbers match to the bit: points sorted by
//! offset, a binary search that answers an exact hit with that point's colour,
//! clamping outside the first/last point, then CONSTANT / LINEAR / CUBIC
//! interpolation in the sRGB, linear-sRGB or Oklab colour space.

use gdcore::math::Color;
use gdvariant::{ResourceRef, Variant};

/// `Gradient.InterpolationMode`.
const INTERPOLATE_CONSTANT: i64 = 1;
const INTERPOLATE_CUBIC: i64 = 2;
/// `Gradient.ColorSpace`.
const COLOR_SPACE_LINEAR_SRGB: i64 = 1;
const COLOR_SPACE_OKLAB: i64 = 2;

/// The gradient's points as Godot STORES them: `(offset, colour)` in the
/// order they were authored or added.
///
/// ⚠️ Unsorted, like Godot: `add_point` appends and `get_color(i)` indexes that
/// raw order; only sampling sorts (`_update_sorting`), and that sort persists —
/// see [`sorted`].
///
/// ⚠️ A bag with neither `offsets` nor `colors` is Godot's DEFAULT gradient —
/// black at 0, white at 1 (`Gradient::Gradient()`), which is what
/// `Gradient.new()` samples, not an empty one.
pub fn points(res: &ResourceRef) -> Vec<(f32, Color)> {
    let offsets: Option<Vec<f32>> = match res.properties.get("offsets") {
        Some(Variant::PackedFloat32Array(v)) => Some(v.clone()),
        Some(Variant::PackedFloat64Array(v)) => Some(v.iter().map(|f| *f as f32).collect()),
        Some(Variant::Array(a)) => Some(
            a.to_vec()
                .iter()
                .map(|v| match v {
                    Variant::Float(f) => *f as f32,
                    Variant::Int(i) => *i as f32,
                    _ => 0.0,
                })
                .collect(),
        ),
        _ => None,
    };
    let colors: Option<Vec<Color>> = match res.properties.get("colors") {
        Some(Variant::PackedColorArray(v)) => Some(v.clone()),
        Some(Variant::Array(a)) => Some(
            a.to_vec()
                .iter()
                .map(|v| match v {
                    Variant::Color(c) => *c,
                    _ => Color::new(0.0, 0.0, 0.0, 1.0),
                })
                .collect(),
        ),
        _ => None,
    };
    let (offsets, colors) = match (offsets, colors) {
        (None, None) => (
            vec![0.0, 1.0],
            vec![
                Color::new(0.0, 0.0, 0.0, 1.0),
                Color::new(1.0, 1.0, 1.0, 1.0),
            ],
        ),
        (o, c) => (o.unwrap_or_default(), c.unwrap_or_default()),
    };
    // Godot resizes `points` to whichever array was set last; a well-formed
    // resource has equal lengths, so the shorter one bounds the pairs here.
    offsets.into_iter().zip(colors).collect()
}

/// `points` sorted by offset, the order `get_color_at_offset` searches.
pub fn sorted(mut pts: Vec<(f32, Color)>) -> Vec<(f32, Color)> {
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    pts
}

fn is_sorted(pts: &[(f32, Color)]) -> bool {
    pts.windows(2).all(|w| w[0].0 <= w[1].0)
}

fn int_prop(res: &ResourceRef, key: &str) -> i64 {
    match res.properties.get(key) {
        Some(Variant::Int(i)) => *i,
        Some(Variant::Float(f)) => *f as i64,
        _ => 0,
    }
}

fn srgb_to_linear(c: Color) -> Color {
    let f = |v: f32| {
        if v < 0.04045 {
            v * (1.0 / 12.92)
        } else {
            (((v as f64 + 0.055) * (1.0 / (1.0 + 0.055))) as f32).powf(2.4)
        }
    };
    Color::new(f(c.r), f(c.g), f(c.b), c.a)
}

fn linear_to_srgb(c: Color) -> Color {
    let f = |v: f32| {
        if v < 0.0031308 {
            12.92 * v
        } else {
            ((1.0 + 0.055) * v.powf(1.0 / 2.4) as f64 - 0.055) as f32
        }
    };
    Color::new(f(c.r), f(c.g), f(c.b), c.a)
}

/// `ok_color::linear_srgb_to_oklab` (`thirdparty/misc/ok_color.h`).
fn linear_srgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    )
}

/// `ok_color::oklab_to_linear_srgb`.
fn oklab_to_linear_srgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    )
}

fn to_space(c: Color, space: i64) -> Color {
    match space {
        COLOR_SPACE_LINEAR_SRGB => srgb_to_linear(c),
        COLOR_SPACE_OKLAB => {
            let lin = srgb_to_linear(c);
            let (l, a, b) = linear_srgb_to_oklab(lin.r, lin.g, lin.b);
            Color::new(l, a, b, lin.a)
        }
        _ => c,
    }
}

fn from_space(c: Color, space: i64) -> Color {
    match space {
        COLOR_SPACE_LINEAR_SRGB => linear_to_srgb(c),
        COLOR_SPACE_OKLAB => {
            let (r, g, b) = oklab_to_linear_srgb(c.r, c.g, c.b);
            linear_to_srgb(Color::new(r, g, b, c.a))
        }
        _ => c,
    }
}

/// `Math::cubic_interpolate(from, to, pre, post, weight)`.
fn cubic(from: f32, to: f32, pre: f32, post: f32, w: f32) -> f32 {
    0.5 * ((from * 2.0)
        + (-pre + to) * w
        + (2.0 * pre - 5.0 * from + 4.0 * to - post) * (w * w)
        + (-pre + 3.0 * from - 3.0 * to + post) * (w * w * w))
}

/// `Gradient.sample(offset)` — `get_color_at_offset`.
pub fn sample(res: &ResourceRef, offset: f32) -> Color {
    let points = sorted(points(res));
    if points.is_empty() {
        return Color::new(0.0, 0.0, 0.0, 1.0);
    }
    let mut low: i64 = 0;
    let mut high: i64 = points.len() as i64 - 1;
    let mut middle: i64 = 0;
    while low <= high {
        middle = (low + high) / 2;
        let p = points[middle as usize];
        if p.0 > offset {
            high = middle - 1;
        } else if p.0 < offset {
            low = middle + 1;
        } else {
            return p.1;
        }
    }
    if points[middle as usize].0 > offset {
        middle -= 1;
    }
    let first = middle;
    let second = middle + 1;
    if second >= points.len() as i64 {
        return points[points.len() - 1].1;
    }
    if first < 0 {
        return points[0].1;
    }
    let (o1, c1) = points[first as usize];
    let (o2, c2) = points[second as usize];
    let weight = (offset - o1) / (o2 - o1);
    let space = int_prop(res, "interpolation_color_space");
    match int_prop(res, "interpolation_mode") {
        INTERPOLATE_CONSTANT => c1,
        INTERPOLATE_CUBIC => {
            let p0 = if first - 1 < 0 { first } else { first - 1 };
            let p3 = if second + 1 >= points.len() as i64 {
                second
            } else {
                second + 1
            };
            let c0 = to_space(points[p0 as usize].1, space);
            let c3 = to_space(points[p3 as usize].1, space);
            let (a, b) = (to_space(c1, space), to_space(c2, space));
            from_space(
                Color::new(
                    cubic(a.r, b.r, c0.r, c3.r, weight),
                    cubic(a.g, b.g, c0.g, c3.g, weight),
                    cubic(a.b, b.b, c0.b, c3.b, weight),
                    cubic(a.a, b.a, c0.a, c3.a, weight),
                ),
                space,
            )
        }
        _ => {
            let (a, b) = (to_space(c1, space), to_space(c2, space));
            // `Color::lerp`: `res + (to - res) * weight`, per channel.
            from_space(
                Color::new(
                    a.r + (b.r - a.r) * weight,
                    a.g + (b.g - a.g) * weight,
                    a.b + (b.b - a.b) * weight,
                    a.a + (b.a - a.a) * weight,
                ),
                space,
            )
        }
    }
}

fn arg_f32(args: &[Variant], i: usize) -> Option<f32> {
    match args.get(i) {
        Some(Variant::Float(f)) => Some(*f as f32),
        Some(Variant::Int(n)) => Some(*n as f32),
        _ => None,
    }
}

fn arg_index(args: &[Variant], i: usize) -> Option<i64> {
    match args.get(i) {
        Some(Variant::Int(n)) => Some(*n),
        Some(Variant::Float(f)) => Some(*f as i64),
        _ => None,
    }
}

/// What a `Gradient` method call answers.
pub enum GradientCall {
    /// A read: the value to return.
    Value(Variant),
    /// A write: the receiver's new `offsets`/`colors`, to write back through
    /// the receiver (the method itself returns `Nil`).
    Write(Vec<f32>, Vec<Color>),
    /// A read that also persists the sort Godot performs on first sample.
    ValueAndWrite(Variant, Vec<f32>, Vec<Color>),
}

/// `Gradient`'s methods (`doc/classes/Gradient.xml`), or `None` for a name this
/// does not implement so the caller's loud `UndefinedFunction` still fires.
pub fn call(
    res: &ResourceRef,
    method: &str,
    args: &[Variant],
) -> Option<Result<GradientCall, String>> {
    let pts = points(res);
    let split = |pts: Vec<(f32, Color)>| -> GradientCall {
        let (o, c): (Vec<f32>, Vec<Color>) = pts.into_iter().unzip();
        GradientCall::Write(o, c)
    };
    let index = |i: Option<i64>| -> Result<usize, String> {
        match i {
            Some(i) if i >= 0 && (i as usize) < pts.len() => Ok(i as usize),
            Some(i) => Err(format!(
                "Gradient index {i} is out of bounds ({} points)",
                pts.len()
            )),
            None => Err(format!("Gradient.{method}() takes an int index")),
        }
    };
    Some(match method {
        "sample" => match arg_f32(args, 0) {
            Some(t) => {
                let value = Variant::Color(sample(res, t));
                if is_sorted(&pts) {
                    Ok(GradientCall::Value(value))
                } else {
                    let (o, c): (Vec<f32>, Vec<Color>) = sorted(pts.clone()).into_iter().unzip();
                    Ok(GradientCall::ValueAndWrite(value, o, c))
                }
            }
            None => Err("Gradient.sample() takes a float offset".into()),
        },
        "get_point_count" => Ok(GradientCall::Value(Variant::Int(pts.len() as i64))),
        "get_offset" => {
            index(arg_index(args, 0)).map(|i| GradientCall::Value(Variant::Float(pts[i].0 as f64)))
        }
        "get_color" => {
            index(arg_index(args, 0)).map(|i| GradientCall::Value(Variant::Color(pts[i].1)))
        }
        "set_offset" | "set_color" => {
            let i = match index(arg_index(args, 0)) {
                Ok(i) => i,
                Err(e) => return Some(Err(e)),
            };
            let mut pts = pts.clone();
            if method == "set_offset" {
                match arg_f32(args, 1) {
                    Some(o) => pts[i].0 = o,
                    None => return Some(Err("Gradient.set_offset() takes a float".into())),
                }
            } else {
                match args.get(1) {
                    Some(Variant::Color(c)) => pts[i].1 = *c,
                    _ => return Some(Err("Gradient.set_color() takes a Color".into())),
                }
            }
            Ok(split(pts))
        }
        "add_point" => match (arg_f32(args, 0), args.get(1)) {
            (Some(o), Some(Variant::Color(c))) => {
                // Appended, as Godot does; the next sample sorts.
                let mut pts = pts.clone();
                pts.push((o, *c));
                Ok(split(pts))
            }
            _ => Err("Gradient.add_point() takes (float offset, Color color)".into()),
        },
        "remove_point" => {
            let i = match index(arg_index(args, 0)) {
                Ok(i) => i,
                Err(e) => return Some(Err(e)),
            };
            // Godot refuses to drop the last point.
            if pts.len() <= 1 {
                return Some(Err("Gradient must keep at least one point".into()));
            }
            let mut pts = pts.clone();
            pts.remove(i);
            Ok(split(pts))
        }
        "reverse" => {
            // Godot flips every offset, then sorts immediately.
            let pts: Vec<(f32, Color)> = pts.iter().map(|(o, c)| (1.0 - o, *c)).collect();
            Ok(split(sorted(pts)))
        }
        _ => return None,
    })
}
