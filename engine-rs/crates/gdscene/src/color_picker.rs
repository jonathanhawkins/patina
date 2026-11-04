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

use gdcore::math::{Color, Rect2, Vector2};
use gdvariant::Variant;

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

// ---------------------------------------------------------------------------
// pat-s3rrv.172 — where the picker's parts sit inside its rect
// ---------------------------------------------------------------------------
//
// 🔑 Deliberately here and NOT in the draw arm, so the pixels drawn and the
// points hit-tested come from ONE rule. Godot has the same split — `uv_edit`
// and `w_edit` are real child controls whose rects both the draw and the
// `gui_input` handler read.

/// Width of the vertical hue bar, in pixels.
///
/// Godot's `h_width` theme constant, `round(30 * scale)`
/// (`scene/theme/default_theme.cpp:1079`). Absolute rather than proportional,
/// which is why a wide picker grows its SV square and not its hue strip.
pub const HUE_BAR_W: f32 = 30.0;

/// Gap between the SV square and the hue bar — the `HBoxContainer` separation
/// Godot lays them out with, 4px at scale 1.
pub const BAR_GAP: f32 = 4.0;

/// Height of the alpha bar shown when `edit_alpha` is set.
pub const ALPHA_BAR_H: f32 = 12.0;

/// The size a `ColorPickerButton` gives the picker it opens.
///
/// Godot's default theme sizes the SV square 256x256 (`default_theme.cpp:1077`),
/// so with the 4px gap and the 30px hue bar the control is `256 + 4 + 30` = 290
/// wide, and `256 + 4 + 12` = 272 tall once the alpha bar is counted. Written as
/// the sums rather than the totals so it stays consistent with
/// [`picker_layout`]: feeding this size back through it recovers exactly a
/// 256x256 SV square, which is the check that the two cannot drift apart.
pub const DEFAULT_PICKER_SIZE: Vector2 =
    Vector2::new(256.0 + BAR_GAP + HUE_BAR_W, 256.0 + BAR_GAP + ALPHA_BAR_H);

/// Where a `ColorPicker`'s parts sit inside its control rect.
///
/// ⚠️ All three are in the SAME space as the rect passed in (global viewport
/// coordinates during a draw or hit test), not picker-local — mixing the two is
/// how a picker draws correctly and then hit-tests as though it were at the
/// origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickerLayout {
    /// The saturation/value square.
    pub sv: Rect2,
    /// The vertical hue bar to its right.
    pub hue: Rect2,
    /// The horizontal alpha bar beneath, when `edit_alpha` is on.
    pub alpha: Option<Rect2>,
}

/// Splits `rect` into the picker's parts.
///
/// Godot's default geometry is a 256x256 SV square, a 4px gap and a 30px hue
/// bar (`default_theme.cpp:1077-1079`). Ported as "the hue bar and gap take
/// their fixed widths off the right; the SV square takes what is left", which
/// reproduces Godot exactly at the default size and degrades sensibly when a
/// scene gives the picker a different one.
///
/// ⚠️ Returns zero-width parts rather than negative ones for a control narrower
/// than the bar itself. A negative width would make `rect_has_point` accept
/// every point to its left, so a too-small picker would swallow clicks aimed at
/// whatever sits beside it.
pub fn picker_layout(rect: Rect2, edit_alpha: bool) -> PickerLayout {
    let alpha_h = if edit_alpha {
        ALPHA_BAR_H.min(rect.size.y)
    } else {
        0.0
    };
    let body_h = (rect.size.y - alpha_h - if edit_alpha { BAR_GAP } else { 0.0 }).max(0.0);
    let sv_w = (rect.size.x - HUE_BAR_W - BAR_GAP).max(0.0);

    let sv = Rect2::new(rect.position, Vector2::new(sv_w, body_h));
    let hue = Rect2::new(
        Vector2::new(rect.position.x + sv_w + BAR_GAP, rect.position.y),
        Vector2::new(HUE_BAR_W.min(rect.size.x), body_h),
    );
    // Anchored to the rect's BOTTOM edge. At any size with room this is the
    // same y as `body_h + BAR_GAP`; it differs only when the rect is shorter
    // than bar + gap, where the old `top + body_h + BAR_GAP` (body_h clamped
    // to 0) pushed the bar BAR_GAP px below the control — so a too-short
    // picker caught presses aimed at whatever sits under it.
    let alpha = if edit_alpha && alpha_h > 0.0 {
        Some(Rect2::new(
            Vector2::new(rect.position.x, rect.position.y + rect.size.y - alpha_h),
            Vector2::new(rect.size.x, alpha_h),
        ))
    } else {
        None
    };
    PickerLayout { sv, hue, alpha }
}

/// Where in the SV square a point falls, as `(u, v)` in `0..=1`.
///
/// `None` when the point is outside. `u` is saturation left→right; `v` is
/// measured top→bottom and is handed to
/// [`ColorPickerModel::pick_sv_rect`](crate::color_picker::ColorPickerModel::pick_sv_rect),
/// which does the inversion — the TOP of the square is full value.
///
/// ⚠️ Clamped, not wrapped: a drag that leaves the square keeps painting its
/// edge value, which is what Godot does while the button stays held.
pub fn sv_uv_at(layout: &PickerLayout, point: Vector2) -> Option<(f32, f32)> {
    if layout.sv.size.x <= 0.0 || layout.sv.size.y <= 0.0 {
        return None;
    }
    let u = (point.x - layout.sv.position.x) / layout.sv.size.x;
    let v = (point.y - layout.sv.position.y) / layout.sv.size.y;
    Some((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
}

/// How far down the hue bar a point falls, in `0..=1`, or `None` when outside.
pub fn hue_t_at(layout: &PickerLayout, point: Vector2) -> Option<f32> {
    if layout.hue.size.y <= 0.0 {
        return None;
    }
    let t = (point.y - layout.hue.position.y) / layout.hue.size.y;
    Some(t.clamp(0.0, 1.0))
}

/// Which part of the picker a point is over.
///
/// 🔑 Resolved ONCE, here, rather than by the input branch asking each
/// sub-rect in turn: the gap between the SV square and the hue bar belongs to
/// neither, and a press landing in it must do nothing rather than snap to
/// whichever rect was tested first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerPart {
    /// The saturation/value square.
    Sv,
    /// The hue bar.
    Hue,
    /// The alpha bar.
    Alpha,
}

/// The part of the picker `point` is inside, if any.
pub fn part_at(layout: &PickerLayout, point: Vector2) -> Option<PickerPart> {
    let inside = |r: &Rect2| {
        point.x >= r.position.x
            && point.y >= r.position.y
            && point.x < r.position.x + r.size.x
            && point.y < r.position.y + r.size.y
    };
    if inside(&layout.sv) {
        return Some(PickerPart::Sv);
    }
    if inside(&layout.hue) {
        return Some(PickerPart::Hue);
    }
    match &layout.alpha {
        Some(a) if inside(a) => Some(PickerPart::Alpha),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// pat-s3rrv.172 — the script method surface
// ---------------------------------------------------------------------------

/// The node property the preset list is stored under.
///
/// 🔑 An ordinary (non-`__`-prefixed) property name on purpose. `scene_saver`
/// writes every non-internal property verbatim and `parse_variant_value` already
/// reads `PackedColorArray(...)` back, so storing the list here is what makes
/// presets survive a `.tscn` round-trip without a single line of serializer
/// work. A `__presets__` companion key would be skipped by
/// `scene_saver::is_internal_prop` and silently vanish on save.
pub const PRESETS_PROP: &str = "presets";

/// The picker's preset list, in order.
pub fn get_presets(
    tree: &crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
) -> Vec<Color> {
    match tree.get_node(node_id).map(|n| n.get_property(PRESETS_PROP)) {
        Some(Variant::PackedColorArray(v)) => v,
        // An `Array` of Colors is what a `.tscn` hand-written as `presets = [
        // Color(1, 0, 0, 1) ]` parses to, and a script may build one the same
        // way. Accepted rather than discarded, because dropping it would look
        // exactly like "the round-trip lost my presets".
        Some(Variant::Array(items)) => items
            .to_vec()
            .iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(*c),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn set_presets(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
    presets: Vec<Color>,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(PRESETS_PROP, Variant::PackedColorArray(presets));
    }
}

/// Answers `ColorPicker`'s script method surface, or `None` for any other node.
///
/// Runs ahead of the generic node-method table in `scripting::call_node_method`
/// and declines every non-ColorPicker node, the contract
/// `tilemap::call_tilemap_method` follows.
///
/// ⚠️ **`ColorPickerButton` is deliberately NOT accepted.** It looks like it
/// should forward these to the picker it hosts, but upstream gives it only
/// `get_picker` and `get_popup` (`doc/classes/ColorPickerButton.xml`) — the
/// preset methods live on `ColorPicker` alone. Accepting the button here would
/// invent API that a real Godot project cannot call.
pub fn call_color_picker_method(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if tree.get_node(node_id)?.class_name() != "ColorPicker" {
        return None;
    }
    let color_arg = || match args.first() {
        Some(Variant::Color(c)) => Some(*c),
        _ => None,
    };

    Some(match method {
        "add_preset" => {
            let color = color_arg()?;
            let mut presets = get_presets(tree, node_id);
            // 🔑 Adding a colour ALREADY in the list is not a no-op and does not
            // duplicate it — Godot moves the existing entry to the back
            // (`color_picker.cpp:1069`, `presets.move_to_back(e)`), so the list
            // is most-recently-added-last. Treating a repeat as "already there,
            // nothing to do" is the natural reading and it silently loses that
            // ordering, which is the only thing the list is for.
            presets.retain(|c| *c != color);
            presets.push(color);
            set_presets(tree, node_id, presets);
            // ⚠️ No `preset_added` here. Upstream emits it from
            // `_add_preset_pressed` (`:1522`) — the UI button — and NOT from
            // `add_preset` itself, so a script calling this must not see the
            // signal fire. Emitting from both would be a plausible-looking
            // parity bug that only a handler-counting test catches.
            Variant::Nil
        }
        "erase_preset" => {
            let color = color_arg()?;
            let mut presets = get_presets(tree, node_id);
            // Absent colour: upstream's `if (e)` guard makes the whole body a
            // no-op (`color_picker.cpp:1101`). `retain` matches that.
            presets.retain(|c| *c != color);
            set_presets(tree, node_id, presets);
            // Likewise emitted only from `_preset_input` (`:1482`, `:1492`).
            Variant::Nil
        }
        "get_presets" => Variant::PackedColorArray(get_presets(tree, node_id)),
        _ => return None,
    })
}
