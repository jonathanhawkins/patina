//! Glyph outline extraction: font bytes → flattened 2D contours.
//!
//! `gdserver3d::text_mesh::TextMesh::generate_from_contours` has always been
//! able to extrude and triangulate glyph contours, and nothing in the tree
//! could produce any — it had zero non-test callers because there was no
//! outline source at all (pat-s3rrv.199). This is that source.
//!
//! Outlines come from `ttf-parser`, which this crate already parses fonts with
//! for metrics. Quadratic and cubic segments are flattened to line segments,
//! because a contour here is a plain polygon.
//!
//! ## Conventions
//!
//! - **Y is up**, as in the font's own coordinate system, matching the XY plane
//!   `TextMesh` extrudes along Z.
//! - Each contour is a **closed polygon with no repeated final point**, which
//!   is what `generate_from_contours` expects — a duplicated closing vertex
//!   produces a degenerate triangle.
//! - Glyphs are laid out left to right by their horizontal advance. There is no
//!   kerning, wrapping, or bidi here; this is the geometry layer.

use gdcore::math::Vector2;

/// Default flattening tolerance in world units, mirroring Godot's `curve_step`.
pub const DEFAULT_CURVE_STEP: f32 = 0.5;

/// Bounds on the t-space step Godot walks a curve with:
/// `CLAMP(curve_step / chord_length, 0.01, 0.5)`, i.e. between 2 and 100
/// segments per curve. Transcribed from `primitive_meshes.cpp`, not chosen.
const MIN_CURVE_T_STEP: f32 = 0.01;
const MAX_CURVE_T_STEP: f32 = 0.5;

/// Flattened outlines for a run of text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextOutline {
    /// One closed polygon per glyph loop, in world units.
    ///
    /// ⚠️ A glyph's counter (the hole in an `o`) arrives as its own contour;
    /// nothing here marks it as a hole, and `generate_from_contours` extrudes
    /// each contour independently. Filled counters are a known limitation of
    /// that triangulator, not of this extraction.
    pub contours: Vec<Vec<Vector2>>,
    /// Total advance width of the run, in the same units as the contours.
    pub advance: f32,
}

/// Extracts flattened contours for `text` from an outline font.
///
/// `font_size` and `pixel_size` follow Godot's `TextMesh`: the glyph em box is
/// scaled to `font_size * pixel_size` world units. `curve_step` is the maximum
/// world-space deviation tolerated when flattening a curve.
///
/// Returns `None` if the bytes are not a parseable outline font, so a caller
/// can fall back rather than treat an unreadable font as empty text.
pub fn text_outline(
    font: &[u8],
    text: &str,
    font_size: f32,
    pixel_size: f32,
    curve_step: f32,
) -> Option<TextOutline> {
    let face = ttf_parser::Face::parse(font, 0).ok()?;

    let units_per_em = face.units_per_em();
    if units_per_em == 0 {
        return None;
    }
    // Font units → world units. `units_per_em` is what makes this independent
    // of the font's internal grid, which varies (1000 for CFF, 2048 for many
    // TrueType faces) and would otherwise scale glyphs differently per font.
    let px_scale = font_size / units_per_em as f32;
    let scale = px_scale * pixel_size;

    let mut out = TextOutline::default();
    let mut pen_x = 0.0f32;

    for ch in text.chars() {
        let Some(glyph) = face.glyph_index(ch) else {
            // An unmapped character contributes no geometry, but a missing
            // glyph should not collapse the rest of the run onto it.
            continue;
        };

        let mut flattener = Flattener::new(scale, px_scale, pen_x, curve_step);
        // `outline_glyph` returns None for a glyph with no contours — a space,
        // typically — which is not an error and must still advance the pen.
        face.outline_glyph(glyph, &mut flattener);
        out.contours.append(&mut flattener.finish());

        let advance = face.glyph_hor_advance(glyph).unwrap_or(0);
        pen_x += advance as f32 * scale;
    }

    out.advance = pen_x;
    Some(out)
}

/// Turns `ttf-parser`'s curve callbacks into flat polygons.
struct Flattener {
    contours: Vec<Vec<Vector2>>,
    current: Vec<Vector2>,
    /// Font units → world units.
    scale: f32,
    /// Font units → PIXELS at the current font size. `curve_step` is documented
    /// "in pixels" (`suffix:px`), and Godot measures the chord in this space —
    /// applying the tolerance in world units instead makes every curve one
    /// segment at typical `pixel_size` values, because a glyph is then well
    /// under one world unit tall.
    px_scale: f32,
    origin_x: f32,
    curve_step: f32,
    /// Pen position in FONT units — curve control points arrive unscaled, so
    /// the subdivision maths stays in that space and only emitted points are
    /// converted.
    last: (f32, f32),
}

impl Flattener {
    fn new(scale: f32, px_scale: f32, origin_x: f32, curve_step: f32) -> Self {
        Self {
            contours: Vec::new(),
            current: Vec::new(),
            scale,
            px_scale,
            origin_x,
            curve_step: curve_step.max(1e-4),
            last: (0.0, 0.0),
        }
    }

    fn emit(&mut self, x: f32, y: f32) {
        self.current
            .push(Vector2::new(self.origin_x + x * self.scale, y * self.scale));
    }

    /// Segment count for a curve, by Godot's own rule.
    ///
    /// `primitive_meshes.cpp`: `step = CLAMP(curve_step / (p0 - pN).length(),
    /// 0.01, 0.5)`, then `t` is walked by `step`. Note it measures the **chord**
    /// between the curve's endpoints, not the control polygon, and does it in
    /// pixel space — both details matter and neither is in the class docs.
    fn segments_for(&self, p0: (f32, f32), pn: (f32, f32)) -> u32 {
        let dx = (pn.0 - p0.0) * self.px_scale;
        let dy = (pn.1 - p0.1) * self.px_scale;
        let chord = (dx * dx + dy * dy).sqrt();
        if chord <= f32::EPSILON {
            // A zero-length chord would divide by zero; one segment is the
            // degenerate answer and keeps the contour well-formed.
            return 1;
        }
        let step = (self.curve_step / chord).clamp(MIN_CURVE_T_STEP, MAX_CURVE_T_STEP);
        (1.0 / step).ceil() as u32
    }

    /// Closes the contour in progress, dropping a repeated final point.
    fn flush(&mut self) {
        if self.current.len() < 3 {
            // Fewer than three points cannot bound an area; triangulating one
            // yields nothing at best and a degenerate triangle at worst.
            self.current.clear();
            return;
        }
        // TrueType contours often end by returning to their start. The
        // triangulator wants the loop implicit, so the duplicate is dropped.
        let first = self.current[0];
        let last = self.current[self.current.len() - 1];
        if (first.x - last.x).abs() < 1e-6 && (first.y - last.y).abs() < 1e-6 {
            self.current.pop();
        }
        if self.current.len() >= 3 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    fn finish(mut self) -> Vec<Vec<Vector2>> {
        // A font need not emit a final `close`, so flush whatever is open.
        self.flush();
        self.contours
    }
}

impl ttf_parser::OutlineBuilder for Flattener {
    fn move_to(&mut self, x: f32, y: f32) {
        // A new contour begins; whatever was open ends here.
        self.flush();
        self.last = (x, y);
        self.emit(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.last = (x, y);
        self.emit(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last;
        let segments = self.segments_for(p0, (x, y));
        for step in 1..=segments {
            let t = step as f32 / segments as f32;
            let inv = 1.0 - t;
            let bx = inv * inv * p0.0 + 2.0 * inv * t * x1 + t * t * x;
            let by = inv * inv * p0.1 + 2.0 * inv * t * y1 + t * t * y;
            self.emit(bx, by);
        }
        self.last = (x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.last;
        let segments = self.segments_for(p0, (x, y));
        for step in 1..=segments {
            let t = step as f32 / segments as f32;
            let inv = 1.0 - t;
            let (a, b, c, d) = (
                inv * inv * inv,
                3.0 * inv * inv * t,
                3.0 * inv * t * t,
                t * t * t,
            );
            let bx = a * p0.0 + b * x1 + c * x2 + d * x;
            let by = a * p0.1 + b * y1 + c * y2 + d * y;
            self.emit(bx, by);
        }
        self.last = (x, y);
    }

    fn close(&mut self) {
        self.flush();
    }
}
