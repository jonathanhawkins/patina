//! StyleBox resource family (pat-u3l8c.70).
//!
//! Godot's StyleBox resources describe how a Control's background/frame is
//! drawn. This module provides the three core kinds — [`StyleBoxFlat`]
//! (flat fill with borders, rounded corners and drop shadow), [`StyleBoxTexture`]
//! (nine-patch texture with per-axis stretch modes) and [`StyleBoxLine`] —
//! each able to emit [`DrawCommand`]s that fill a rect, plus a `.tscn`/`.tres`
//! parser that builds StyleBoxes from `[sub_resource type="StyleBox*"]` sections
//! (for theme integration).
//!
//! Rounded corners are approximated with the renderer's filled-rect and
//! filled-circle primitives (two cross bars + four corner discs), which the 2D
//! software renderer rasterizes directly.

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Vector2};
use gdserver2d::canvas::DrawCommand;
use gdvariant::Variant;

/// Per-axis stretch mode for [`StyleBoxTexture`] edges/center (matches Godot's
/// `AxisStretchMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AxisStretchMode {
    /// Stretch the region to fill (default).
    #[default]
    Stretch,
    /// Tile the region at its native size.
    Tile,
    /// Tile, scaling to fit a whole number of tiles.
    TileFit,
}

impl AxisStretchMode {
    /// Maps Godot's integer enum value to a mode.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Tile,
            2 => Self::TileFit,
            _ => Self::Stretch,
        }
    }
}

/// A flat box with optional borders, rounded corners and drop shadow
/// (`StyleBoxFlat`).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleBoxFlat {
    /// Background fill color.
    pub bg_color: Color,
    /// Border widths `[left, top, right, bottom]`.
    pub border_width: [f32; 4],
    /// Border color.
    pub border_color: Color,
    /// Corner radii `[top_left, top_right, bottom_right, bottom_left]`.
    pub corner_radius: [f32; 4],
    /// Drop-shadow color.
    pub shadow_color: Color,
    /// Drop-shadow size (outward expansion in px).
    pub shadow_size: f32,
    /// Drop-shadow offset.
    pub shadow_offset: Vector2,
    /// Content margins `[l, t, r, b]`; negative means "derive from border width".
    pub content_margin: [f32; 4],
}

impl Default for StyleBoxFlat {
    fn default() -> Self {
        Self {
            bg_color: Color::new(0.15, 0.15, 0.15, 1.0),
            border_width: [0.0; 4],
            border_color: Color::new(0.0, 0.0, 0.0, 1.0),
            corner_radius: [0.0; 4],
            shadow_color: Color::new(0.0, 0.0, 0.0, 0.0),
            shadow_size: 0.0,
            shadow_offset: Vector2::ZERO,
            content_margin: [-1.0; 4],
        }
    }
}

impl StyleBoxFlat {
    /// Effective uniform radius used for rendering (max of the four corners).
    fn radius(&self) -> f32 {
        self.corner_radius.iter().copied().fold(0.0, f32::max)
    }

    fn draw(&self, rect: Rect2) -> Vec<DrawCommand> {
        let mut out = Vec::new();
        let r = self.radius();

        // Drop shadow, drawn first so it sits behind the box.
        if self.shadow_size > 0.0 && self.shadow_color.a > 0.0 {
            let s = self.shadow_size;
            let sr = Rect2::new(
                Vector2::new(
                    rect.position.x - s + self.shadow_offset.x,
                    rect.position.y - s + self.shadow_offset.y,
                ),
                Vector2::new(rect.size.x + 2.0 * s, rect.size.y + 2.0 * s),
            );
            push_rounded(&mut out, sr, r + s, self.shadow_color);
        }

        let has_border =
            self.border_width.iter().any(|w| *w > 0.0) && self.border_color.a > 0.0;
        if has_border {
            // Border layer, then the background inset by the border widths.
            push_rounded(&mut out, rect, r, self.border_color);
            let inner = Rect2::new(
                Vector2::new(
                    rect.position.x + self.border_width[0],
                    rect.position.y + self.border_width[1],
                ),
                Vector2::new(
                    (rect.size.x - self.border_width[0] - self.border_width[2]).max(0.0),
                    (rect.size.y - self.border_width[1] - self.border_width[3]).max(0.0),
                ),
            );
            let inner_r = (r - self.border_width[0].max(self.border_width[1])).max(0.0);
            push_rounded(&mut out, inner, inner_r, self.bg_color);
        } else {
            push_rounded(&mut out, rect, r, self.bg_color);
        }
        out
    }
}

/// A nine-patch textured box (`StyleBoxTexture`).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleBoxTexture {
    /// Source texture path.
    pub texture_path: String,
    /// Texture margins `[l, t, r, b]` defining the nine-patch regions.
    pub texture_margin: [f32; 4],
    /// Extra outward draw expansion `[l, t, r, b]`.
    pub expand_margin: [f32; 4],
    /// Horizontal edge/center stretch mode.
    pub axis_h: AxisStretchMode,
    /// Vertical edge/center stretch mode.
    pub axis_v: AxisStretchMode,
    /// Tint applied to the whole texture.
    pub modulate: Color,
    /// Whether to draw the center region.
    pub draw_center: bool,
}

impl Default for StyleBoxTexture {
    fn default() -> Self {
        Self {
            texture_path: String::new(),
            texture_margin: [0.0; 4],
            expand_margin: [0.0; 4],
            axis_h: AxisStretchMode::Stretch,
            axis_v: AxisStretchMode::Stretch,
            modulate: Color::WHITE,
            draw_center: true,
        }
    }
}

impl StyleBoxTexture {
    fn draw(&self, rect: Rect2) -> Vec<DrawCommand> {
        if self.texture_path.is_empty() {
            return Vec::new();
        }
        let er = Rect2::new(
            Vector2::new(
                rect.position.x - self.expand_margin[0],
                rect.position.y - self.expand_margin[1],
            ),
            Vector2::new(
                rect.size.x + self.expand_margin[0] + self.expand_margin[2],
                rect.size.y + self.expand_margin[1] + self.expand_margin[3],
            ),
        );
        // The software renderer's DrawNinePatch stretches edges/center; the
        // per-axis Tile/TileFit modes are modeled on the resource for parity and
        // future renderer support.
        vec![DrawCommand::DrawNinePatch {
            texture_path: self.texture_path.clone(),
            rect: er,
            margin_left: self.texture_margin[0],
            margin_top: self.texture_margin[1],
            margin_right: self.texture_margin[2],
            margin_bottom: self.texture_margin[3],
            draw_center: self.draw_center,
            modulate: self.modulate,
        }]
    }
}

/// A single horizontal or vertical line box (`StyleBoxLine`).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleBoxLine {
    /// Line color.
    pub color: Color,
    /// Line thickness in px.
    pub thickness: f32,
    /// Extension before the start.
    pub grow_begin: f32,
    /// Extension past the end.
    pub grow_end: f32,
    /// Vertical (`true`) or horizontal (`false`) orientation.
    pub vertical: bool,
}

impl Default for StyleBoxLine {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            thickness: 1.0,
            grow_begin: 1.0,
            grow_end: 1.0,
            vertical: false,
        }
    }
}

impl StyleBoxLine {
    fn draw(&self, rect: Rect2) -> Vec<DrawCommand> {
        let t = self.thickness.max(1.0);
        let r = if self.vertical {
            let cx = rect.position.x + rect.size.x * 0.5 - t * 0.5;
            Rect2::new(
                Vector2::new(cx, rect.position.y - self.grow_begin),
                Vector2::new(t, rect.size.y + self.grow_begin + self.grow_end),
            )
        } else {
            let cy = rect.position.y + rect.size.y * 0.5 - t * 0.5;
            Rect2::new(
                Vector2::new(rect.position.x - self.grow_begin, cy),
                Vector2::new(rect.size.x + self.grow_begin + self.grow_end, t),
            )
        };
        vec![DrawCommand::DrawRect {
            rect: r,
            color: self.color,
            filled: true,
        }]
    }
}

/// A StyleBox resource: the drawable background/frame of a Control.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleBox {
    /// Flat fill with borders/corners/shadow.
    Flat(StyleBoxFlat),
    /// Nine-patch texture.
    Texture(StyleBoxTexture),
    /// A single line.
    Line(StyleBoxLine),
    /// An empty (invisible) stylebox.
    Empty,
}

impl StyleBox {
    /// Emits draw commands to render this stylebox filling `rect`.
    pub fn draw(&self, rect: Rect2) -> Vec<DrawCommand> {
        match self {
            StyleBox::Flat(f) => f.draw(rect),
            StyleBox::Texture(t) => t.draw(rect),
            StyleBox::Line(l) => l.draw(rect),
            StyleBox::Empty => Vec::new(),
        }
    }

    /// Content margins `[l, t, r, b]` (padding) this stylebox reserves.
    pub fn content_margins(&self) -> [f32; 4] {
        match self {
            StyleBox::Flat(f) => {
                let mut m = [0.0; 4];
                for i in 0..4 {
                    m[i] = if f.content_margin[i] >= 0.0 {
                        f.content_margin[i]
                    } else {
                        f.border_width[i]
                    };
                }
                m
            }
            StyleBox::Texture(t) => t.texture_margin,
            StyleBox::Line(_) | StyleBox::Empty => [0.0; 4],
        }
    }

    /// The Godot class name of this stylebox kind.
    pub fn class_name(&self) -> &'static str {
        match self {
            StyleBox::Flat(_) => "StyleBoxFlat",
            StyleBox::Texture(_) => "StyleBoxTexture",
            StyleBox::Line(_) => "StyleBoxLine",
            StyleBox::Empty => "StyleBoxEmpty",
        }
    }
}

/// Appends draw commands filling `rect` with a rounded rectangle of `color`.
/// A radius of 0 emits a single filled rect; otherwise two cross bars plus four
/// corner discs approximate the rounded shape.
fn push_rounded(out: &mut Vec<DrawCommand>, rect: Rect2, radius: f32, color: Color) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 || color.a <= 0.0 {
        return;
    }
    let r = radius.min(rect.size.x * 0.5).min(rect.size.y * 0.5).max(0.0);
    if r <= 0.0 {
        out.push(DrawCommand::DrawRect {
            rect,
            color,
            filled: true,
        });
        return;
    }
    let (x, y, w, h) = (rect.position.x, rect.position.y, rect.size.x, rect.size.y);
    // Horizontal bar (full width, inset top/bottom by r).
    out.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y + r), Vector2::new(w, h - 2.0 * r)),
        color,
        filled: true,
    });
    // Vertical bar (full height, inset left/right by r).
    out.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x + r, y), Vector2::new(w - 2.0 * r, h)),
        color,
        filled: true,
    });
    // Four corner discs.
    for (cx, cy) in [
        (x + r, y + r),
        (x + w - r, y + r),
        (x + w - r, y + h - r),
        (x + r, y + h - r),
    ] {
        out.push(DrawCommand::DrawCircle {
            center: Vector2::new(cx, cy),
            radius: r,
            color,
        });
    }
}

// ===========================================================================
// tscn / tres parsing
// ===========================================================================

/// Parses StyleBox sub-resources from a Godot `.tscn`/`.tres` string, returning
/// a map from sub-resource id to [`StyleBox`].
pub fn parse_styleboxes_from_tscn(text: &str) -> HashMap<String, StyleBox> {
    let mut out = HashMap::new();
    let mut cur_id: Option<String> = None;
    let mut cur_class: Option<String> = None;
    let mut props: HashMap<String, Variant> = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("[sub_resource") {
            flush_stylebox(&cur_id, &cur_class, &props, &mut out);
            props.clear();
            cur_class = extract_attr(line, "type");
            cur_id = extract_attr(line, "id");
            if cur_class
                .as_deref()
                .map_or(true, |c| !c.starts_with("StyleBox"))
            {
                cur_id = None;
                cur_class = None;
            }
        } else if line.starts_with('[') {
            // A different section begins — flush and stop tracking.
            flush_stylebox(&cur_id, &cur_class, &props, &mut out);
            props.clear();
            cur_id = None;
            cur_class = None;
        } else if cur_id.is_some() {
            if let Some((k, v)) = line.split_once('=') {
                if let Some(val) = parse_tscn_value(v.trim()) {
                    props.insert(k.trim().to_string(), val);
                }
            }
        }
    }
    flush_stylebox(&cur_id, &cur_class, &props, &mut out);
    out
}

fn flush_stylebox(
    id: &Option<String>,
    class: &Option<String>,
    props: &HashMap<String, Variant>,
    out: &mut HashMap<String, StyleBox>,
) {
    if let (Some(id), Some(class)) = (id, class) {
        if let Some(sb) = stylebox_from_props(class, props) {
            out.insert(id.clone(), sb);
        }
    }
}

/// Builds a [`StyleBox`] from a class name and a parsed property map.
pub fn stylebox_from_props(class: &str, props: &HashMap<String, Variant>) -> Option<StyleBox> {
    let f = |k: &str, d: f32| props.get(k).and_then(variant_f32).unwrap_or(d);
    let col = |k: &str, d: Color| props.get(k).and_then(variant_color).unwrap_or(d);
    match class {
        "StyleBoxFlat" => {
            let def = StyleBoxFlat::default();
            Some(StyleBox::Flat(StyleBoxFlat {
                bg_color: col("bg_color", def.bg_color),
                border_width: [
                    f("border_width_left", 0.0),
                    f("border_width_top", 0.0),
                    f("border_width_right", 0.0),
                    f("border_width_bottom", 0.0),
                ],
                border_color: col("border_color", def.border_color),
                corner_radius: [
                    f("corner_radius_top_left", 0.0),
                    f("corner_radius_top_right", 0.0),
                    f("corner_radius_bottom_right", 0.0),
                    f("corner_radius_bottom_left", 0.0),
                ],
                shadow_color: col("shadow_color", def.shadow_color),
                shadow_size: f("shadow_size", 0.0),
                shadow_offset: props
                    .get("shadow_offset")
                    .and_then(variant_vec2)
                    .unwrap_or(Vector2::ZERO),
                content_margin: [
                    f("content_margin_left", -1.0),
                    f("content_margin_top", -1.0),
                    f("content_margin_right", -1.0),
                    f("content_margin_bottom", -1.0),
                ],
            }))
        }
        "StyleBoxTexture" => {
            let def = StyleBoxTexture::default();
            Some(StyleBox::Texture(StyleBoxTexture {
                texture_path: props
                    .get("texture")
                    .and_then(variant_string)
                    .unwrap_or_default(),
                texture_margin: [
                    f("texture_margin_left", 0.0),
                    f("texture_margin_top", 0.0),
                    f("texture_margin_right", 0.0),
                    f("texture_margin_bottom", 0.0),
                ],
                expand_margin: [
                    f("expand_margin_left", 0.0),
                    f("expand_margin_top", 0.0),
                    f("expand_margin_right", 0.0),
                    f("expand_margin_bottom", 0.0),
                ],
                axis_h: AxisStretchMode::from_i64(
                    props
                        .get("axis_stretch_horizontal")
                        .and_then(variant_i64)
                        .unwrap_or(0),
                ),
                axis_v: AxisStretchMode::from_i64(
                    props
                        .get("axis_stretch_vertical")
                        .and_then(variant_i64)
                        .unwrap_or(0),
                ),
                modulate: col("modulate_color", def.modulate),
                draw_center: props
                    .get("draw_center")
                    .and_then(variant_bool)
                    .unwrap_or(true),
            }))
        }
        "StyleBoxLine" => {
            let def = StyleBoxLine::default();
            Some(StyleBox::Line(StyleBoxLine {
                color: col("color", def.color),
                thickness: f("thickness", 1.0),
                grow_begin: f("grow_begin", 1.0),
                grow_end: f("grow_end", 1.0),
                vertical: props
                    .get("vertical")
                    .and_then(variant_bool)
                    .unwrap_or(false),
            }))
        }
        "StyleBoxEmpty" => Some(StyleBox::Empty),
        _ => None,
    }
}

fn extract_attr(line: &str, name: &str) -> Option<String> {
    let pat = format!("{name}=\"");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_tscn_value(s: &str) -> Option<Variant> {
    if let Some(inner) = s.strip_prefix("Color(").and_then(|x| x.strip_suffix(')')) {
        let n: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        return match n.len() {
            4 => Some(Variant::Color(Color::new(n[0], n[1], n[2], n[3]))),
            3 => Some(Variant::Color(Color::new(n[0], n[1], n[2], 1.0))),
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("Vector2(").and_then(|x| x.strip_suffix(')')) {
        let n: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        return if n.len() == 2 {
            Some(Variant::Vector2(Vector2::new(n[0], n[1])))
        } else {
            None
        };
    }
    if s == "true" {
        return Some(Variant::Bool(true));
    }
    if s == "false" {
        return Some(Variant::Bool(false));
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(Variant::String(inner.to_string()));
    }
    if let Ok(i) = s.parse::<i64>() {
        return Some(Variant::Int(i));
    }
    if let Ok(fl) = s.parse::<f64>() {
        return Some(Variant::Float(fl));
    }
    None
}

fn variant_f32(v: &Variant) -> Option<f32> {
    match v {
        Variant::Float(f) => Some(*f as f32),
        Variant::Int(i) => Some(*i as f32),
        _ => None,
    }
}

fn variant_i64(v: &Variant) -> Option<i64> {
    match v {
        Variant::Int(i) => Some(*i),
        Variant::Float(f) => Some(*f as i64),
        _ => None,
    }
}

fn variant_color(v: &Variant) -> Option<Color> {
    match v {
        Variant::Color(c) => Some(*c),
        _ => None,
    }
}

fn variant_vec2(v: &Variant) -> Option<Vector2> {
    match v {
        Variant::Vector2(x) => Some(*x),
        _ => None,
    }
}

fn variant_bool(v: &Variant) -> Option<bool> {
    match v {
        Variant::Bool(b) => Some(*b),
        _ => None,
    }
}

fn variant_string(v: &Variant) -> Option<String> {
    match v {
        Variant::String(s) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_plain_emits_single_rect() {
        let sb = StyleBox::Flat(StyleBoxFlat {
            bg_color: Color::new(0.2, 0.2, 0.2, 1.0),
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::ZERO, Vector2::new(50.0, 40.0)));
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], DrawCommand::DrawRect { filled: true, .. }));
    }

    #[test]
    fn flat_rounded_bordered_emits_bars_and_discs() {
        let sb = StyleBox::Flat(StyleBoxFlat {
            bg_color: Color::new(0.2, 0.2, 0.2, 1.0),
            border_width: [2.0; 4],
            border_color: Color::WHITE,
            corner_radius: [8.0; 4],
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::new(10.0, 10.0), Vector2::new(80.0, 60.0)));
        // border layer (2 bars + 4 discs) + bg layer (2 bars + 4 discs) = 12.
        assert_eq!(cmds.len(), 12);
        let circles = cmds
            .iter()
            .filter(|c| matches!(c, DrawCommand::DrawCircle { .. }))
            .count();
        assert_eq!(circles, 8, "four corner discs per rounded layer");
    }

    #[test]
    fn texture_emits_nine_patch() {
        let sb = StyleBox::Texture(StyleBoxTexture {
            texture_path: "panel.png".into(),
            texture_margin: [4.0; 4],
            axis_h: AxisStretchMode::Tile,
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::ZERO, Vector2::new(64.0, 64.0)));
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], DrawCommand::DrawNinePatch { .. }));
    }

    #[test]
    fn line_emits_rect() {
        let sb = StyleBox::Line(StyleBoxLine {
            color: Color::WHITE,
            thickness: 3.0,
            vertical: false,
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::ZERO, Vector2::new(40.0, 20.0)));
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            DrawCommand::DrawRect { rect, .. } => {
                assert!((rect.size.y - 3.0).abs() < 1e-5, "horizontal line thickness");
            }
            other => panic!("expected DrawRect, got {other:?}"),
        }
    }

    #[test]
    fn parse_tscn_stylebox_flat() {
        let tscn = r#"
[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="panel_sb"]
bg_color = Color(0.1, 0.2, 0.3, 1)
border_width_left = 2
border_width_top = 2
border_color = Color(1, 1, 1, 1)
corner_radius_top_left = 6
corner_radius_top_right = 6

[sub_resource type="StyleBoxLine" id="sep"]
color = Color(0.5, 0.5, 0.5, 1)
thickness = 2

[resource]
"#;
        let boxes = parse_styleboxes_from_tscn(tscn);
        assert_eq!(boxes.len(), 2);
        match boxes.get("panel_sb") {
            Some(StyleBox::Flat(f)) => {
                assert!((f.bg_color.b - 0.3).abs() < 1e-5);
                assert!((f.border_width[0] - 2.0).abs() < 1e-5);
                assert!((f.corner_radius[0] - 6.0).abs() < 1e-5);
            }
            other => panic!("expected StyleBoxFlat, got {other:?}"),
        }
        assert!(matches!(boxes.get("sep"), Some(StyleBox::Line(_))));
    }

    #[test]
    fn content_margins_derive_from_border() {
        let sb = StyleBox::Flat(StyleBoxFlat {
            border_width: [3.0, 4.0, 5.0, 6.0],
            ..Default::default()
        });
        assert_eq!(sb.content_margins(), [3.0, 4.0, 5.0, 6.0]);
    }
}
