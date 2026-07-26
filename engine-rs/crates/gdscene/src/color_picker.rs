//! `ColorPicker`'s HSV model — the maths behind the SV rectangle and hue bar
//! (pat-0q2kx.176).
//!
//! Godot ground truth: `scene/gui/color_picker.cpp` (default `SHAPE_HSV_RECT`:
//! a saturation/value rectangle beside a vertical hue bar) and
//! `core/math/color.cpp`'s `Color::set_hsv`.
//!
//! Patina had no colour picker at all and, more surprisingly, **no HSV
//! conversion anywhere** — `grep hsv` across `gdcore` matched nothing. So this
//! is the conversion plus the hit-mapping that turns a click position into a
//! colour; the drawing and popup layers build on top of it.

use gdcore::math::Color;

/// Godot's `Color::set_hsv` (`core/math/color.cpp:182`), ported exactly.
///
/// `h`, `s`, `v` are in `0..=1`. Hue wraps rather than clamping, matching
/// Godot's `fmod(h * 6, 6)`, so a hue of exactly 1.0 is the same red as 0.0
/// instead of falling off the end of the switch.
pub fn color_from_hsv(h: f32, s: f32, v: f32, a: f32) -> Color {
    // Achromatic: Godot returns grey without touching the hue sextant at all.
    if s == 0.0 {
        return Color::new(v, v, v, a);
    }

    let h6 = (h * 6.0).rem_euclid(6.0);
    let i = h6.floor();
    let f = h6 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));

    let (r, g, b) = match i as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        // Godot's `default:` arm — sextant 5, and anything the float maths
        // pushes past it.
        _ => (v, p, q),
    };
    Color::new(r, g, b, a)
}

/// The inverse: `(hue, saturation, value)`, each in `0..=1`.
///
/// Needed so a `color` assigned from a script or a `.tscn` places the picker's
/// cursors correctly, rather than the picker only ever knowing colours it
/// generated itself.
pub fn hsv_from_color(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let delta = max - min;

    let value = max;
    let saturation = if max == 0.0 { 0.0 } else { delta / max };

    // Grey has no meaningful hue; Godot reports 0 rather than NaN from the
    // divide-by-delta below.
    if delta == 0.0 {
        return (0.0, saturation, value);
    }

    let mut hue = if max == c.r {
        (c.g - c.b) / delta
    } else if max == c.g {
        2.0 + (c.b - c.r) / delta
    } else {
        4.0 + (c.r - c.g) / delta
    };
    hue /= 6.0;
    if hue < 0.0 {
        hue += 1.0;
    }
    (hue, saturation, value)
}

/// The picker's current HSV position, independent of how it is drawn.
///
/// Godot's picker keeps hue/saturation/value rather than a single `Color`
/// because the SV rectangle must stay put when only the hue changes — deriving
/// HSV back from RGB every time would make a fully black or fully desaturated
/// colour forget which hue the user had selected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorPickerModel {
    /// Hue in `0..=1`.
    pub hue: f32,
    /// Saturation in `0..=1`.
    pub saturation: f32,
    /// Value in `0..=1`.
    pub value: f32,
    /// Alpha in `0..=1`.
    pub alpha: f32,
}

impl Default for ColorPickerModel {
    /// Godot opens on opaque white.
    fn default() -> Self {
        Self {
            hue: 0.0,
            saturation: 0.0,
            value: 1.0,
            alpha: 1.0,
        }
    }
}

impl ColorPickerModel {
    /// A picker positioned to show `color`.
    pub fn from_color(color: Color) -> Self {
        let (hue, saturation, value) = hsv_from_color(color);
        Self {
            hue,
            saturation,
            value,
            alpha: color.a,
        }
    }

    /// The colour the picker currently represents.
    pub fn color(&self) -> Color {
        color_from_hsv(self.hue, self.saturation, self.value, self.alpha)
    }

    /// A click on the vertical hue bar, `t` being the fraction *down* the bar.
    ///
    /// Returns `true` when the colour actually changed, which is what gates
    /// `color_changed` — Godot does not re-emit for a click that lands on the
    /// value already selected.
    pub fn pick_hue_bar(&mut self, t: f32) -> bool {
        let before = self.color();
        self.hue = t.clamp(0.0, 1.0);
        self.color() != before
    }

    /// A click in the SV rectangle. `u` runs left→right as saturation; `v` runs
    /// top→bottom, and is inverted so the TOP of the rectangle is full value —
    /// which is how Godot draws it.
    pub fn pick_sv_rect(&mut self, u: f32, v: f32) -> bool {
        let before = self.color();
        self.saturation = u.clamp(0.0, 1.0);
        self.value = 1.0 - v.clamp(0.0, 1.0);
        self.color() != before
    }
}
