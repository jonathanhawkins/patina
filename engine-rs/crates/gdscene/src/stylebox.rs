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
use gdserver2d::canvas::{DrawCommand, NinePatchAxis};
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
    /// Maps onto the draw-command axis mode (pat-0q2kx.148).
    ///
    /// The two enums are deliberately separate types: `DrawCommand` lives in
    /// `gdserver2d`, which cannot see this one, so the resource-side enum is
    /// translated at the point the command is emitted.
    pub fn to_nine_patch_axis(self) -> NinePatchAxis {
        match self {
            AxisStretchMode::Stretch => NinePatchAxis::Stretch,
            AxisStretchMode::Tile => NinePatchAxis::Tile,
            AxisStretchMode::TileFit => NinePatchAxis::TileFit,
        }
    }

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
    /// Godot `draw_center` — when false the background fill is skipped and only
    /// the border is painted (an outline box). **Implemented.**
    pub draw_center: bool,
    /// Godot `expand_margin_*` `[l, t, r, b]` — grows the drawn box *outside*
    /// its layout rect without affecting layout. **Implemented.**
    pub expand_margin: [f32; 4],
    /// Godot `corner_detail` — arc tessellation segments per corner.
    ///
    /// ⚠️ Stored and parsed, **not yet honoured**: corners are drawn as discs by
    /// the existing `DrawCircle` primitive, whose tessellation the rasterizer
    /// owns. Wiring this needs a segment count on that primitive.
    pub corner_detail: i32,
    /// Godot `skew` — shears the box. ⚠️ Stored and parsed, **not yet honoured**:
    /// the `DrawRect`/`DrawCircle` primitives have no shear, so this needs a
    /// transformed-quad primitive.
    pub skew: Vector2,
    /// Godot `border_blend` — gradient from border colour to bg across the
    /// border. ⚠️ Stored and parsed, **not yet honoured**: needs a gradient fill.
    pub border_blend: bool,
    /// Godot `anti_aliasing`. ⚠️ Stored and parsed, **not yet honoured**: the
    /// software rasterizer's edge coverage is owned by `gdrender2d`.
    pub anti_aliasing: bool,
    /// Godot `anti_aliasing_size` (`aa_size`), in px. See `anti_aliasing`.
    pub aa_size: f32,
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
            // Godot's documented StyleBoxFlat defaults.
            draw_center: true,
            expand_margin: [0.0; 4],
            corner_detail: 8,
            skew: Vector2::ZERO,
            border_blend: false,
            anti_aliasing: true,
            aa_size: 1.0,
        }
    }
}

impl StyleBoxFlat {
    /// The largest corner radius, used only for the drop shadow — a single
    /// outward-expanded silhouette rather than the box's own outline.
    ///
    /// ⚠️ This is NOT the box's rounding. Folding the four corners to their max
    /// for the box itself is exactly the bug pat-0q2kx.148 fixes; the box goes
    /// through `push_rounded_per_corner` with all four values.
    fn shadow_radius(&self) -> f32 {
        self.corner_radius.iter().copied().fold(0.0, f32::max)
    }

    fn draw(&self, layout_rect: Rect2) -> Vec<DrawCommand> {
        let mut out = Vec::new();
        let radii = self.corner_radius;
        // Godot's `expand_margin_*` grows what is *painted* outside the layout
        // rect without changing layout — used for glows and focus rings
        // (pat-0q2kx.148).
        let rect = Rect2::new(
            Vector2::new(
                layout_rect.position.x - self.expand_margin[0],
                layout_rect.position.y - self.expand_margin[1],
            ),
            Vector2::new(
                (layout_rect.size.x + self.expand_margin[0] + self.expand_margin[2]).max(0.0),
                (layout_rect.size.y + self.expand_margin[1] + self.expand_margin[3]).max(0.0),
            ),
        );

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
            push_rounded(&mut out, sr, self.shadow_radius() + s, self.shadow_color);
        }

        // A SHEARED box cannot be expressed as axis-aligned rectangles plus
        // circular corners, so it takes the polygon emitter instead
        // (pat-s3rrv.215).
        //
        // ⚠️ Deliberately gated on `skew != 0` rather than replacing the
        // rect/disc path outright. That path is exact for an unsheared box,
        // cheaper to rasterize, and is what every existing stylebox test and
        // golden image was produced against; swapping it wholesale would change
        // the pixels of every themed control in the engine to gain nothing on
        // the boxes that are not skewed.
        if self.skew != Vector2::ZERO {
            let border = self.border_width.iter().any(|w| *w > 0.0) && self.border_color.a > 0.0;

            // Anti-aliasing: a feathered ring `aa_size` px outside the shape,
            // drawn FIRST so the solid box covers its inner half and only the
            // outer fringe survives as partial-alpha edge pixels
            // (pat-s3rrv.215).
            //
            // ⚠️ This is the bead's own suggested approach ("emit a feathered
            // outer ring of aa_size px"), NOT Godot's. Godot computes real
            // per-pixel edge coverage; this approximates it with one
            // half-alpha ring, which softens the edge but does not match
            // Godot's gradient exactly.
            //
            // 🔑 It is confined to the SKEWED path on purpose. `anti_aliasing`
            // defaults to TRUE, so feathering the ordinary rect/disc path would
            // move the edge pixels of every themed control in the engine and
            // every render golden with it. Scoping it here makes the property
            // observable without touching a single existing box; the unskewed
            // path still wants real coverage AA in the rasterizer.
            if self.anti_aliasing && self.aa_size > 0.0 {
                let a = self.aa_size;
                let feather_rect = Rect2::new(
                    Vector2::new(rect.position.x - a, rect.position.y - a),
                    Vector2::new(rect.size.x + 2.0 * a, rect.size.y + 2.0 * a),
                );
                // Growing the radius with the rect keeps the corner arc
                // concentric — the same trick the drop shadow above uses.
                let feather_radii = radii.map(|r| if r > 0.0 { r + a } else { 0.0 });
                let edge = if border {
                    self.border_color
                } else {
                    self.bg_color
                };
                out.push(DrawCommand::DrawPolygon {
                    points: rounded_box_outline(
                        feather_rect,
                        feather_radii,
                        self.corner_detail,
                        self.skew,
                    ),
                    colors: vec![Color::new(edge.r, edge.g, edge.b, edge.a * 0.5)],
                });
            }

            if border {
                out.push(DrawCommand::DrawPolygon {
                    points: rounded_box_outline(rect, radii, self.corner_detail, self.skew),
                    colors: vec![self.border_color],
                });
            }
            if self.draw_center {
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
                let inset = |a: usize, b: usize| {
                    (radii[a] - self.border_width[a].max(self.border_width[b])).max(0.0)
                };
                let inner_radii = [inset(0, 1), inset(1, 2), inset(2, 3), inset(3, 0)];
                out.push(DrawCommand::DrawPolygon {
                    points: rounded_box_outline(inner, inner_radii, self.corner_detail, self.skew),
                    colors: vec![self.bg_color],
                });
            }
            return out;
        }

        let has_border = self.border_width.iter().any(|w| *w > 0.0) && self.border_color.a > 0.0;
        // ⚠️ The layered draw below (a border-coloured box, then the centre
        // over it) is only right when the centre is drawn AND opaque. With
        // `draw_center = false` it filled the WHOLE box with the border colour
        // — Godot's default focus frame (a 2px border, no centre) painted the
        // focused button solid white — and a translucent centre composited
        // over the border colour instead of over what is behind the box
        // (pat-9ww3e). Those cases draw the border as four edge strips.
        if has_border && (!self.draw_center || self.bg_color.a < 1.0) {
            let [bl, bt, br, bb] = self.border_width;
            let (p, s) = (rect.position, rect.size);
            let mut strip = |x: f32, y: f32, w: f32, h: f32| {
                if w > 0.0 && h > 0.0 {
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
                        color: self.border_color,
                        filled: true,
                    });
                }
            };
            strip(p.x, p.y, s.x, bt);
            strip(p.x, p.y + s.y - bb, s.x, bb);
            strip(p.x, p.y + bt, bl, s.y - bt - bb);
            strip(p.x + s.x - br, p.y + bt, br, s.y - bt - bb);
            if self.draw_center {
                let inner = Rect2::new(
                    Vector2::new(p.x + bl, p.y + bt),
                    Vector2::new((s.x - bl - br).max(0.0), (s.y - bt - bb).max(0.0)),
                );
                let inset = |a: usize, b: usize| {
                    (radii[a] - self.border_width[a].max(self.border_width[b])).max(0.0)
                };
                let inner_radii = [inset(0, 1), inset(1, 2), inset(2, 3), inset(3, 0)];
                push_rounded_per_corner(&mut out, inner, inner_radii, self.bg_color);
            }
            return out;
        }
        if has_border {
            // Border layer, then the background inset by the border widths.
            push_rounded_per_corner(&mut out, rect, radii, self.border_color);
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
            // Each corner's inner radius shrinks by the borders meeting at it,
            // so a square corner stays square inside the border too.
            let inset = |a: usize, b: usize| {
                (radii[a] - self.border_width[a].max(self.border_width[b])).max(0.0)
            };
            let inner_radii = [inset(0, 1), inset(1, 2), inset(2, 3), inset(3, 0)];
            // `draw_center = false` paints the border only, leaving the middle
            // transparent — Godot's outline-box mode.
            if self.draw_center {
                push_rounded_per_corner(&mut out, inner, inner_radii, self.bg_color);
            }
        } else if self.draw_center {
            push_rounded_per_corner(&mut out, rect, radii, self.bg_color);
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
    /// Godot `StyleBoxTexture.region_rect` — the sub-rectangle of the SOURCE
    /// texture the nine patches are cut from (pat-s3rrv.215).
    ///
    /// ⚠️ A **zero-size** rect means "use the whole texture". That is Godot's
    /// default and it is what every existing atlas-free stylebox relies on, so
    /// the zero case must stay byte-for-byte identical to the behaviour before
    /// this field existed. Not an `Option`, matching the same decision on
    /// `DrawCommand::DrawNinePatch::region`: the engine's own default IS a zero
    /// `Rect2`, and an `Option` would let `None` and `Some(ZERO)` drift apart
    /// while meaning the same thing.
    pub region_rect: Rect2,
}

impl Default for StyleBoxTexture {
    fn default() -> Self {
        Self {
            texture_path: String::new(),
            texture_margin: [0.0; 4],
            expand_margin: [0.0; 4],
            region_rect: Rect2::new(Vector2::ZERO, Vector2::ZERO),
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
            // The authored per-axis modes now reach the rasterizer instead of
            // being modelled on the resource and dropped (pat-0q2kx.148).
            axis_h: self.axis_h.to_nine_patch_axis(),
            axis_v: self.axis_v.to_nine_patch_axis(),
            modulate: self.modulate,
            // 🔑 The authored region now reaches the rasterizer. It was
            // hardcoded to zero here, so an atlas-backed stylebox stretched the
            // ENTIRE source image over the box — the patches were cut from the
            // whole sheet rather than from the intended tile.
            region: self.region_rect,
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
    /// Content margins `[l, t, r, b]`; `< 0` means unset (pat-0q2kx.148).
    ///
    /// Godot declares `content_margin_*` on the **base** `StyleBox`, so every
    /// subtype carries padding — not just `StyleBoxFlat`.
    pub content_margin: [f32; 4],
}

/// `StyleBoxEmpty` — draws nothing, but still carries content margins.
///
/// This is Godot's standard way to add padding to a Control without painting
/// anything (`scene/resources/style_box.cpp`: `content_margin_*` lives on the
/// base class). Patina modelled `Empty` as a unit variant, so an authored
/// `content_margin_left = 8` was silently dropped and the child's layout never
/// moved (pat-0q2kx.148).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StyleBoxEmpty {
    /// Content margins `[l, t, r, b]`; `< 0` means unset.
    pub content_margin: [f32; 4],
}

impl Default for StyleBoxEmpty {
    fn default() -> Self {
        // Godot's `content_margin_*` default is -1 ("unset").
        Self {
            content_margin: [-1.0; 4],
        }
    }
}

impl Default for StyleBoxLine {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            thickness: 1.0,
            grow_begin: 1.0,
            grow_end: 1.0,
            vertical: false,
            // Godot's `content_margin_*` default is -1 ("unset").
            content_margin: [-1.0; 4],
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
    /// An empty (invisible) stylebox that can still carry content margins.
    Empty(StyleBoxEmpty),
}

impl StyleBox {
    /// The colour this box fills its rect with, if it fills one
    /// (pat-0q2kx.150).
    ///
    /// `Flat` and `Line` have a literal colour; a `Texture` box paints pixels
    /// rather than a flat fill, and `Empty` paints nothing — both report `None`
    /// so a caller can fall back rather than invent a colour.
    pub fn bg_color(&self) -> Option<Color> {
        match self {
            StyleBox::Flat(f) => Some(f.bg_color),
            StyleBox::Line(l) => Some(l.color),
            StyleBox::Texture(_) | StyleBox::Empty(_) => None,
        }
    }
}

/// Resolves Godot's `-1` ("unset") content margins to `0`.
///
/// `StyleBoxFlat` derives its unset margins from the border width instead; the
/// marginless subtypes have no border to fall back on (pat-0q2kx.148).
fn resolve_unset_margins(margins: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0; 4];
    for i in 0..4 {
        out[i] = if margins[i] >= 0.0 { margins[i] } else { 0.0 };
    }
    out
}

impl StyleBox {
    /// Emits draw commands to render this stylebox filling `rect`.
    pub fn draw(&self, rect: Rect2) -> Vec<DrawCommand> {
        match self {
            StyleBox::Flat(f) => f.draw(rect),
            StyleBox::Texture(t) => t.draw(rect),
            StyleBox::Line(l) => l.draw(rect),
            StyleBox::Empty(_) => Vec::new(),
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
            // Unset (`< 0`) resolves to 0 here: unlike Flat there is no border
            // width to derive a default from.
            StyleBox::Line(l) => resolve_unset_margins(l.content_margin),
            StyleBox::Empty(e) => resolve_unset_margins(e.content_margin),
        }
    }

    /// The Godot class name of this stylebox kind.
    pub fn class_name(&self) -> &'static str {
        match self {
            StyleBox::Flat(_) => "StyleBoxFlat",
            StyleBox::Texture(_) => "StyleBoxTexture",
            StyleBox::Line(_) => "StyleBoxLine",
            StyleBox::Empty(_) => "StyleBoxEmpty",
        }
    }
}

/// Appends draw commands filling `rect` with a rounded rectangle of `color`.
/// A radius of 0 emits a single filled rect; otherwise two cross bars plus four
/// corner discs approximate the rounded shape.
fn push_rounded(out: &mut Vec<DrawCommand>, rect: Rect2, radius: f32, color: Color) {
    push_rounded_per_corner(out, rect, [radius; 4], color);
}

/// Clamps `radii` so no pair along an edge exceeds that edge's length.
///
/// Godot scales every radius by one common factor rather than clipping them
/// individually (`style_box_flat.cpp`), so an over-large set shrinks in
/// proportion and keeps its relative shape.
fn clamp_corner_radii(rect: Rect2, radii: [f32; 4]) -> [f32; 4] {
    let [tl, tr, br, bl] = radii.map(|r| r.max(0.0));
    let (w, h) = (rect.size.x, rect.size.y);
    let mut scale: f32 = 1.0;
    for (sum, extent) in [(tl + tr, w), (bl + br, w), (tl + bl, h), (tr + br, h)] {
        if sum > extent && sum > 0.0 {
            scale = scale.min(extent / sum);
        }
    }
    [tl * scale, tr * scale, br * scale, bl * scale]
}

/// Fills `rect` with **independent per-corner radii**, ordered
/// `[top_left, top_right, bottom_right, bottom_left]` as Godot's
/// `corner_radius_*` are (pat-0q2kx.148).
///
/// Previously every corner was drawn with the *maximum* of the four radii, so a
/// `StyleBoxFlat` with only `corner_radius_top_left` set rendered four identical
/// rounded corners — the authored shape was unreachable.
///
/// The decomposition stays within the existing `DrawRect`/`DrawCircle`
/// primitives, so no new rasterizer support is needed:
///
/// * a full-width middle band between the top and bottom corner zones,
/// * a top and bottom strip spanning the gap between that edge's two corners,
/// * per corner, a disc of that corner's radius plus a filler rect covering the
///   remainder of the corner zone when the opposite corner on the same edge is
///   taller.
///
/// A zero-radius corner contributes no disc and is covered by the strips, which
/// is what makes it render square.
fn push_rounded_per_corner(out: &mut Vec<DrawCommand>, rect: Rect2, radii: [f32; 4], color: Color) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 || color.a <= 0.0 {
        return;
    }
    let [tl, tr, br, bl] = clamp_corner_radii(rect, radii);
    if tl <= 0.0 && tr <= 0.0 && br <= 0.0 && bl <= 0.0 {
        out.push(DrawCommand::DrawRect {
            rect,
            color,
            filled: true,
        });
        return;
    }
    // ⚠️ A TRANSLUCENT box cannot be built from overlapping pieces: each corner
    // disc overlaps the bands beside it, and where they overlap the colour
    // blends twice, so Godot's 60%-alpha default buttons came out with
    // darker blotches at every corner (pat-9ww3e). It is one polygon instead.
    if color.a < 1.0 {
        out.push(DrawCommand::DrawPolygon {
            points: rounded_box_outline(rect, [tl, tr, br, bl], 6, Vector2::ZERO),
            colors: vec![color],
        });
        return;
    }
    let (x, y, w, h) = (rect.position.x, rect.position.y, rect.size.x, rect.size.y);
    let top = tl.max(tr);
    let bottom = bl.max(br);

    let mut rect_at = |px: f32, py: f32, pw: f32, ph: f32| {
        if pw > 0.0 && ph > 0.0 {
            out.push(DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::new(px, py), Vector2::new(pw, ph)),
                color,
                filled: true,
            });
        }
    };

    // Middle band: everything between the corner zones, full width.
    rect_at(x, y + top, w, h - top - bottom);
    // Edge strips between the two corners of that edge.
    rect_at(x + tl, y, w - tl - tr, top);
    rect_at(x + bl, y + h - bottom, w - bl - br, bottom);
    // Filler for a corner whose radius is smaller than its edge's zone.
    rect_at(x, y + tl, tl, top - tl);
    rect_at(x + w - tr, y + tr, tr, top - tr);
    rect_at(x, y + h - bottom, bl, bottom - bl);
    rect_at(x + w - br, y + h - bottom, br, bottom - br);

    // One disc per rounded corner; a zero-radius corner stays square.
    for (r, cx, cy) in [
        (tl, x + tl, y + tl),
        (tr, x + w - tr, y + tr),
        (br, x + w - br, y + h - br),
        (bl, x + bl, y + h - bl),
    ] {
        if r > 0.0 {
            out.push(DrawCommand::DrawCircle {
                center: Vector2::new(cx, cy),
                radius: r,
                color,
            });
        }
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
                draw_center: props
                    .get("draw_center")
                    .and_then(variant_bool)
                    .unwrap_or(def.draw_center),
                expand_margin: [
                    f("expand_margin_left", 0.0),
                    f("expand_margin_top", 0.0),
                    f("expand_margin_right", 0.0),
                    f("expand_margin_bottom", 0.0),
                ],
                corner_detail: f("corner_detail", def.corner_detail as f32) as i32,
                // ⚠️ Godot writes `skew` as a SINGLE Vector2 member — its doc
                // XML declares `skew: Vector2` — so a real `.tres` carries
                // `skew = Vector2(0.4, 0)`. Reading only `skew_x`/`skew_y`
                // dropped it silently, which stayed invisible for as long as
                // `skew` was unused at draw time (pat-s3rrv.215).
                //
                // Both spellings are accepted: the Vector2 is what Godot
                // authors, and the split floats are what this parser's own
                // tests and any scene saved against it already use. The Vector2
                // wins when present.
                skew: match props.get("skew") {
                    Some(Variant::Vector2(v)) => *v,
                    _ => Vector2::new(f("skew_x", 0.0), f("skew_y", 0.0)),
                },
                border_blend: props
                    .get("border_blend")
                    .and_then(variant_bool)
                    .unwrap_or(def.border_blend),
                anti_aliasing: props
                    .get("anti_aliasing")
                    .and_then(variant_bool)
                    .unwrap_or(def.anti_aliasing),
                aa_size: f("anti_aliasing_size", def.aa_size),
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
                // ⚠️ An ABSENT `region_rect` must stay the zero rect, which
                // means "the whole texture". Substituting the box's own size
                // here would silently make every atlas-free stylebox sample a
                // region instead of the full image.
                region_rect: match props.get("region_rect") {
                    Some(Variant::Rect2(r)) => *r,
                    _ => def.region_rect,
                },
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
                // `StyleBoxLine` carries only the base-class content margins.
                // `draw_center`, `expand_margin`, `corner_detail`, `skew`,
                // `border_blend` and the anti-aliasing pair belong to
                // `StyleBoxFlat` and had been copy-pasted into this arm:
                // assigning them here is E0560, and reading them off `def`
                // (a `StyleBoxLine`) is E0609 — 12 errors that broke the
                // `gdscene` lib-test target. Matches Godot, where
                // `StyleBoxLine` exposes only color/thickness/grow_begin/
                // grow_end/vertical plus the base content margins.
                content_margin: [
                    f("content_margin_left", -1.0),
                    f("content_margin_top", -1.0),
                    f("content_margin_right", -1.0),
                    f("content_margin_bottom", -1.0),
                ],
            }))
        }
        // `StyleBoxEmpty` is Godot's padding carrier: it paints nothing but its
        // base-class content margins still drive Control layout (pat-0q2kx.148).
        "StyleBoxEmpty" => Some(StyleBox::Empty(StyleBoxEmpty {
            content_margin: [
                f("content_margin_left", -1.0),
                f("content_margin_top", -1.0),
                f("content_margin_right", -1.0),
                f("content_margin_bottom", -1.0),
            ],
        })),
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
        let n: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        return match n.len() {
            4 => Some(Variant::Color(Color::new(n[0], n[1], n[2], n[3]))),
            3 => Some(Variant::Color(Color::new(n[0], n[1], n[2], 1.0))),
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("Vector2(").and_then(|x| x.strip_suffix(')')) {
        let n: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
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
        assert!(matches!(
            cmds[0],
            DrawCommand::DrawRect { filled: true, .. }
        ));
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
        let cmds = sb.draw(Rect2::new(
            Vector2::new(10.0, 10.0),
            Vector2::new(80.0, 60.0),
        ));
        // border layer (3 bands + 4 discs) + bg layer (3 bands + 4 discs) = 14.
        // Three bands, not a two-bar cross: each corner keeps its OWN radius
        // (`each_corner_keeps_its_own_radius`), so the top and bottom bands are
        // inset by their own corners rather than one shared max.
        assert_eq!(cmds.len(), 14);
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
                assert!(
                    (rect.size.y - 3.0).abs() < 1e-5,
                    "horizontal line thickness"
                );
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

#[cfg(test)]
mod base_content_margin_tests {
    use super::*;

    /// `StyleBoxEmpty` is Godot's padding carrier: it paints nothing but its
    /// base-class `content_margin_*` must still reach Control layout. Before
    /// pat-0q2kx.148 `Empty` was a unit variant, so an authored margin was
    /// dropped outright and `content_margins()` always answered zero.
    #[test]
    fn empty_stylebox_carries_content_margins() {
        let sb = StyleBox::Empty(StyleBoxEmpty {
            content_margin: [8.0, 2.0, 4.0, 6.0],
        });
        assert_eq!(sb.content_margins(), [8.0, 2.0, 4.0, 6.0]);
        // It still draws nothing — carrying padding must not paint.
        assert!(sb
            .draw(Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0)))
            .is_empty());
        assert_eq!(sb.class_name(), "StyleBoxEmpty");
    }

    /// An unset (`-1`) margin resolves to 0 rather than leaking the sentinel
    /// into layout as a negative inset.
    #[test]
    fn unset_margins_resolve_to_zero_not_negative_one() {
        let sb = StyleBox::Empty(StyleBoxEmpty::default());
        assert_eq!(sb.content_margins(), [0.0; 4]);

        // Mixed: only left authored.
        let sb = StyleBox::Empty(StyleBoxEmpty {
            content_margin: [8.0, -1.0, -1.0, -1.0],
        });
        assert_eq!(sb.content_margins(), [8.0, 0.0, 0.0, 0.0]);
    }

    /// `StyleBoxLine` carries them too — `content_margin_*` is on the base
    /// class, not a Flat-only field.
    #[test]
    fn line_stylebox_carries_content_margins() {
        let sb = StyleBox::Line(StyleBoxLine {
            content_margin: [3.0, -1.0, 5.0, -1.0],
            ..Default::default()
        });
        assert_eq!(sb.content_margins(), [3.0, 0.0, 5.0, 0.0]);
    }

    /// The `.tscn` keys parse for both marginless subtypes, which is what makes
    /// an authored padding stylebox work end to end.
    #[test]
    fn tscn_content_margin_keys_parse_for_empty_and_line() {
        let props = |pairs: &[(&str, f64)]| -> std::collections::HashMap<String, Variant> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), Variant::Float(*v)))
                .collect()
        };

        let empty = stylebox_from_props(
            "StyleBoxEmpty",
            &props(&[("content_margin_left", 8.0), ("content_margin_bottom", 3.0)]),
        )
        .expect("StyleBoxEmpty parses");
        assert_eq!(empty.content_margins(), [8.0, 0.0, 0.0, 3.0]);

        let line = stylebox_from_props(
            "StyleBoxLine",
            &props(&[("thickness", 2.0), ("content_margin_top", 7.0)]),
        )
        .expect("StyleBoxLine parses");
        assert_eq!(line.content_margins(), [0.0, 7.0, 0.0, 0.0]);
    }
}

#[cfg(test)]
mod per_corner_radius_tests {
    use super::*;

    fn discs(cmds: &[DrawCommand]) -> Vec<(Vector2, f32)> {
        cmds.iter()
            .filter_map(|c| match c {
                DrawCommand::DrawCircle { center, radius, .. } => Some((*center, *radius)),
                _ => None,
            })
            .collect()
    }

    /// **The behaviour the bead is about.** One rounded corner and three square
    /// ones. Before this, all four `corner_radius` values were folded to their
    /// max, so this authored shape rendered as four identical rounded corners.
    #[test]
    fn one_rounded_corner_emits_exactly_one_disc() {
        let sb = StyleBox::Flat(StyleBoxFlat {
            corner_radius: [12.0, 0.0, 0.0, 0.0],
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::ZERO, Vector2::new(100.0, 50.0)));
        let d = discs(&cmds);
        assert_eq!(
            d.len(),
            1,
            "only the top-left corner is rounded, so exactly one disc: got {d:?}"
        );
        assert_eq!(d[0].1, 12.0, "and it carries top_left's radius");
        assert_eq!(
            d[0].0,
            Vector2::new(12.0, 12.0),
            "centred one radius in from the top-left"
        );
    }

    /// Distinct radii are kept distinct rather than unified.
    #[test]
    fn each_corner_keeps_its_own_radius() {
        let sb = StyleBox::Flat(StyleBoxFlat {
            corner_radius: [10.0, 4.0, 8.0, 0.0],
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::ZERO, Vector2::new(100.0, 60.0)));
        let mut radii: Vec<f32> = discs(&cmds).into_iter().map(|(_, r)| r).collect();
        radii.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            radii,
            vec![4.0, 8.0, 10.0],
            "three rounded corners keep their own radii; the zero one is square"
        );
    }

    /// A fully square box still emits a single rect and no discs — the fast path
    /// must not regress into the corner decomposition.
    #[test]
    fn square_box_is_a_single_rect() {
        let sb = StyleBox::Flat(StyleBoxFlat {
            corner_radius: [0.0; 4],
            ..Default::default()
        });
        let cmds = sb.draw(Rect2::new(Vector2::ZERO, Vector2::new(30.0, 20.0)));
        assert!(discs(&cmds).is_empty(), "no corner discs for a square box");
        assert_eq!(cmds.len(), 1, "and one rect, not a decomposition");
    }

    /// Over-large radii scale down together, keeping their proportions, rather
    /// than each being clipped independently (Godot's behaviour).
    #[test]
    fn oversized_radii_scale_proportionally() {
        // 40 + 40 = 80 across a 40px width → scale 0.5.
        let clamped = clamp_corner_radii(
            Rect2::new(Vector2::ZERO, Vector2::new(40.0, 40.0)),
            [40.0, 40.0, 0.0, 0.0],
        );
        assert_eq!(clamped[0], 20.0);
        assert_eq!(clamped[1], 20.0);
        // A set that already fits is untouched.
        let kept = clamp_corner_radii(
            Rect2::new(Vector2::ZERO, Vector2::new(100.0, 100.0)),
            [10.0, 4.0, 8.0, 0.0],
        );
        assert_eq!(kept, [10.0, 4.0, 8.0, 0.0]);
    }
}

#[cfg(test)]
mod flat_field_tests {
    use super::*;

    fn rects(cmds: &[DrawCommand]) -> Vec<Rect2> {
        cmds.iter()
            .filter_map(|c| match c {
                DrawCommand::DrawRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    /// `draw_center = false` paints the border only — Godot's outline-box mode.
    /// With it true the same box also fills the middle.
    #[test]
    fn draw_center_false_skips_the_background_fill() {
        let base = StyleBoxFlat {
            bg_color: Color::new(1.0, 0.0, 0.0, 1.0),
            border_color: Color::new(0.0, 1.0, 0.0, 1.0),
            border_width: [2.0; 4],
            ..Default::default()
        };
        let rect = Rect2::new(Vector2::ZERO, Vector2::new(40.0, 30.0));

        let filled = StyleBox::Flat(base.clone()).draw(rect);
        let outline = StyleBox::Flat(StyleBoxFlat {
            draw_center: false,
            ..base
        })
        .draw(rect);

        // ⚠️ Not a command COUNT: the old outline emitted fewer commands and was
        // still wrong — its "border layer" was a border-coloured box over the
        // whole rect, so the outline box painted its middle solid (pat-9ww3e;
        // Godot's focus frame drew focused buttons white). What matters is that
        // nothing it paints covers the centre.
        let centre = Vector2::new(20.0, 15.0);
        let covers_centre = |c: &DrawCommand| match c {
            DrawCommand::DrawRect { rect, .. } => {
                centre.x >= rect.position.x
                    && centre.x < rect.position.x + rect.size.x
                    && centre.y >= rect.position.y
                    && centre.y < rect.position.y + rect.size.y
            }
            DrawCommand::DrawPolygon { .. } => true,
            _ => false,
        };
        assert!(
            !outline.iter().any(covers_centre),
            "draw_center = false must leave the middle unpainted: {outline:?}"
        );
        assert!(filled.iter().any(covers_centre), "draw_center = true fills the middle");
        // The border colour is still painted; the bg colour is not.
        let has_bg = |cmds: &[DrawCommand]| {
            cmds.iter().any(|c| {
                matches!(c,
                DrawCommand::DrawRect { color, .. } if color.r > 0.9 && color.g < 0.1)
            })
        };
        assert!(has_bg(&filled), "draw_center = true fills with bg_color");
        assert!(
            !has_bg(&outline),
            "draw_center = false must not paint bg_color at all"
        );
    }

    /// `expand_margin_*` grows what is painted *outside* the layout rect, without
    /// the caller changing the rect it passes in.
    #[test]
    fn expand_margin_paints_outside_the_layout_rect() {
        let rect = Rect2::new(Vector2::new(10.0, 10.0), Vector2::new(20.0, 20.0));
        let plain = StyleBox::Flat(StyleBoxFlat::default()).draw(rect);
        let expanded = StyleBox::Flat(StyleBoxFlat {
            expand_margin: [4.0, 0.0, 0.0, 0.0],
            ..Default::default()
        })
        .draw(rect);

        let left_of = |cmds: &[DrawCommand]| {
            rects(cmds)
                .into_iter()
                .map(|r| r.position.x)
                .fold(f32::INFINITY, f32::min)
        };
        assert_eq!(left_of(&plain), 10.0, "unexpanded starts at the rect");
        assert_eq!(
            left_of(&expanded),
            6.0,
            "a 4px left expand_margin must paint 4px further left"
        );
    }

    /// The fields Godot has that this port stores but does not yet rasterize
    /// still round-trip from `.tscn`, so authoring them is not silently lost.
    #[test]
    fn unrasterized_fields_still_parse_from_props() {
        let props: std::collections::HashMap<String, Variant> = [
            ("corner_detail", Variant::Float(16.0)),
            ("skew_x", Variant::Float(0.25)),
            ("anti_aliasing_size", Variant::Float(2.0)),
            ("draw_center", Variant::Bool(false)),
            ("expand_margin_top", Variant::Float(3.0)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        let sb = stylebox_from_props("StyleBoxFlat", &props).expect("parses");
        let StyleBox::Flat(f) = sb else {
            panic!("expected a Flat")
        };
        assert_eq!(f.corner_detail, 16);
        assert_eq!(f.skew.x, 0.25);
        assert_eq!(f.aa_size, 2.0);
        assert!(!f.draw_center);
        assert_eq!(f.expand_margin[1], 3.0);
        // Defaults survive for keys the block omitted.
        assert!(f.anti_aliasing, "Godot's anti_aliasing default is true");
    }
}

#[cfg(test)]
mod per_corner_coverage_tests {
    use super::*;

    /// Whether `p` is painted by any command the decomposition emitted.
    fn covered(cmds: &[DrawCommand], p: Vector2) -> bool {
        const EPS: f32 = 1e-4;
        cmds.iter().any(|c| match c {
            DrawCommand::DrawRect { rect, .. } => {
                rect.size.x > 0.0
                    && rect.size.y > 0.0
                    && p.x >= rect.position.x - EPS
                    && p.x <= rect.position.x + rect.size.x + EPS
                    && p.y >= rect.position.y - EPS
                    && p.y <= rect.position.y + rect.size.y + EPS
            }
            DrawCommand::DrawCircle { center, radius, .. } => {
                let (dx, dy) = (p.x - center.x, p.y - center.y);
                dx * dx + dy * dy <= radius * radius + EPS
            }
            _ => false,
        })
    }

    /// Whether `p` lies outside the intended rounded silhouette — i.e. beyond a
    /// corner's arc, where being unpainted is correct.
    fn outside_silhouette(w: f32, h: f32, r: [f32; 4], p: Vector2) -> bool {
        let [tl, tr, br, bl] = r;
        let corners = [
            (tl, Vector2::new(tl, tl), p.x < tl && p.y < tl),
            (tr, Vector2::new(w - tr, tr), p.x > w - tr && p.y < tr),
            (
                br,
                Vector2::new(w - br, h - br),
                p.x > w - br && p.y > h - br,
            ),
            (bl, Vector2::new(bl, h - bl), p.x < bl && p.y > h - bl),
        ];
        corners.iter().any(|(rad, c, in_corner_box)| {
            *rad > 0.0 && *in_corner_box && {
                let (dx, dy) = (p.x - c.x, p.y - c.y);
                dx * dx + dy * dy > rad * rad
            }
        })
    }

    /// **The decomposition has no seams.** An additive rect+disc decomposition
    /// that leaves a one-pixel gap between the middle band and a corner filler
    /// would look plausible and render a visible crack, and the disc-count
    /// assertions in `per_corner_radius_tests` would not notice.
    ///
    /// So: for a spread of radius combinations, every sampled point inside the
    /// intended rounded silhouette must be painted by *something*.
    #[test]
    fn per_corner_decomposition_covers_the_whole_shape() {
        let cases: &[(f32, f32, [f32; 4])] = &[
            (100.0, 50.0, [12.0, 0.0, 0.0, 0.0]),
            (100.0, 50.0, [0.0, 0.0, 0.0, 0.0]),
            (100.0, 50.0, [8.0, 8.0, 8.0, 8.0]),
            (100.0, 50.0, [12.0, 4.0, 0.0, 0.0]),
            (100.0, 60.0, [10.0, 4.0, 8.0, 0.0]),
            // Over-large: exercises the proportional clamp.
            (40.0, 40.0, [40.0, 40.0, 0.0, 0.0]),
            (30.0, 80.0, [15.0, 15.0, 15.0, 15.0]),
            (80.0, 30.0, [5.0, 25.0, 5.0, 25.0]),
            (50.0, 50.0, [25.0, 0.0, 25.0, 0.0]),
        ];

        for &(w, h, radii) in cases {
            let rect = Rect2::new(Vector2::ZERO, Vector2::new(w, h));
            let mut cmds = Vec::new();
            push_rounded_per_corner(&mut cmds, rect, radii, Color::new(1.0, 1.0, 1.0, 1.0));
            // Compare against the *clamped* radii — that is the shape actually
            // being drawn when a set overflows its edge.
            let effective = clamp_corner_radii(rect, radii);

            const N: u32 = 40;
            for i in 0..=N {
                for j in 0..=N {
                    let p = Vector2::new(w * i as f32 / N as f32, h * j as f32 / N as f32);
                    if outside_silhouette(w, h, effective, p) {
                        continue;
                    }
                    assert!(
                        covered(&cmds, p),
                        "seam at {p:?} for {w}x{h} radii {radii:?} \
                         (clamped {effective:?}) — the decomposition left a gap"
                    );
                }
            }
        }
    }
}

/// Builds the outline of a rounded, optionally sheared box as a closed polygon
/// ring (pat-s3rrv.215).
///
/// Ported from `upstream/godot/scene/resources/style_box_flat.cpp`
/// `draw_rounded_rectangle` (:313-392).
///
/// # Why a polygon at all
///
/// The existing [`push_rounded_per_corner`] decomposes a rounded box into
/// axis-aligned `DrawRect`s plus a `DrawCircle` per corner. That is exact for an
/// UNSHEARED box and cheaper to rasterize, but a sheared box is not a union of
/// axis-aligned rectangles and a sheared corner is not a circle — so `skew` and
/// `corner_detail` are unreachable from that representation. This emitter is
/// what makes both observable.
///
/// # The two details worth transcribing
///
/// - **The shear pivots on the rect's CENTRE and its sign is negative**:
///   `x' = x - skew.x * (y - cy)`, `y' = y - skew.y * (x - cx)`. With a positive
///   `skew.x` that pushes the TOP edge right and the BOTTOM edge left. Pivoting
///   on the top-left corner instead — the intuitive choice — shears the box the
///   same amount but slides it sideways as well, so it no longer sits in its
///   own layout rect.
/// - **A square box drops to one segment per corner**, not `corner_detail`:
///   Godot sets `adapted_corner_detail = any radius > 0 ? corner_detail : 1`, so
///   a sharp-cornered box emits 8 vertices however high the detail is set.
///
/// The ring has `4 * (detail + 1)` vertices, wound the way Godot winds it: each
/// corner's arc starts at angle π and sweeps a quarter turn, beginning at the
/// top-left.
pub fn rounded_box_outline(
    rect: Rect2,
    radii: [f32; 4],
    corner_detail: i32,
    skew: Vector2,
) -> Vec<Vector2> {
    let [tl, tr, br, bl] = clamp_corner_radii(rect, radii);
    // ⚠️ `.max(1)` is not cosmetic: Godot divides by this value
    // (`detail / (double)adapted_corner_detail`), so a zero would be a division
    // by zero rather than simply a coarse box.
    let detail = if tl > 0.0 || tr > 0.0 || br > 0.0 || bl > 0.0 {
        corner_detail.max(1)
    } else {
        1
    };

    let (x, y, w, h) = (rect.position.x, rect.position.y, rect.size.x, rect.size.y);
    let (cx, cy) = (x + w * 0.5, y + h * 0.5);

    // Arc centres and radii in Godot's corner order: top-left, top-right,
    // bottom-right, bottom-left.
    let corners = [
        (tl, x + tl, y + tl),
        (tr, x + w - tr, y + tr),
        (br, x + w - br, y + h - br),
        (bl, x + bl, y + h - bl),
    ];

    let quarter = std::f32::consts::FRAC_PI_2;
    let mut points = Vec::with_capacity(4 * (detail as usize + 1));
    for (corner_idx, (r, ax, ay)) in corners.iter().enumerate() {
        for step in 0..=detail {
            let t = step as f32 / detail as f32;
            let angle = (corner_idx as f32 + t) * quarter + std::f32::consts::PI;
            let px = r * angle.cos() + ax;
            let py = r * angle.sin() + ay;
            // The shear, exactly as Godot spells it.
            points.push(Vector2::new(
                px - skew.x * (py - cy),
                py - skew.y * (px - cx),
            ));
        }
    }
    points
}
