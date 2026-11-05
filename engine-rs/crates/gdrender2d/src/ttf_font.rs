//! TrueType/OpenType font rasterization, glyph atlas, and text shaping
//! (pat-u3l8c.69).
//!
//! [`TtfFont`] parses a TTF/OTF via `ab_glyph` and exposes real glyph metrics
//! (advances, kerning, ascent/descent) plus per-glyph rasterization at arbitrary
//! pixel sizes. [`GlyphAtlas`] packs rasterized glyph coverage into a single
//! grayscale texture (shelf packing) and caches it by glyph+size. Free
//! functions provide text measurement, word-wrap, ellipsis truncation, and
//! blitting shaped text (with kerning) into a [`FrameBuffer`].

use std::collections::HashMap;

use ab_glyph::{point, Font, FontVec, Glyph, PxScale, PxScaleFont, ScaleFont};
use gdcore::math::{Color, Rect2, Vector2};

use crate::renderer::FrameBuffer;

/// A parsed TrueType/OpenType font.
pub struct TtfFont {
    font: FontVec,
}

impl std::fmt::Debug for TtfFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtfFont").finish_non_exhaustive()
    }
}

/// A rasterized glyph: an alpha coverage bitmap plus placement metrics.
#[derive(Debug, Clone)]
pub struct RasterGlyph {
    /// Bitmap width in pixels.
    pub width: usize,
    /// Bitmap height in pixels.
    pub height: usize,
    /// `width * height` alpha coverage values (0..=255), row-major.
    pub coverage: Vec<u8>,
    /// X offset of the bitmap left edge from the pen position.
    pub left: f32,
    /// Y offset of the bitmap top edge from the baseline (usually negative).
    pub top: f32,
    /// Horizontal advance to the next pen position.
    pub advance: f32,
}

impl TtfFont {
    /// Parses a font from TTF/OTF bytes.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        FontVec::try_from_vec(data)
            .map(|font| Self { font })
            .map_err(|e| format!("invalid font data: {e}"))
    }

    /// The engine's built-in default UI font (Cantarell, SIL OFL 1.1), embedded
    /// into the binary so text renders without an explicit font resource —
    /// mirroring Godot's built-in default font. Set it on a renderer at app
    /// startup via [`crate::renderer::SoftwareRenderer::use_default_font`].
    pub fn builtin() -> Self {
        const DEFAULT: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/default_font.ttf"
        ));
        Self::from_bytes(DEFAULT.to_vec()).expect("embedded default font is a valid TTF")
    }

    fn scaled(&self, px_size: f32) -> PxScaleFont<&FontVec> {
        self.font.as_scaled(PxScale::from(px_size))
    }

    /// The `px_size` (this type's unit: the pixel height of `ascent -
    /// descent`) that renders the font at an EM of `em_px` pixels — Godot's
    /// `font_size` (FreeType `FT_Set_Pixel_Sizes` sets the em). The two differ
    /// by the face's `height / units_per_em`, ~1.27 for the built-in face.
    pub fn px_size_for_em(&self, em_px: f32) -> f32 {
        match self.font.units_per_em() {
            Some(upem) if upem > 0.0 => em_px * self.font.height_unscaled() / upem,
            _ => em_px,
        }
    }

    /// Whether the font contains a glyph for `ch` (id 0 is `.notdef`).
    pub fn has_glyph(&self, ch: char) -> bool {
        self.font.glyph_id(ch).0 != 0
    }

    /// Horizontal advance of `ch` at `px_size`.
    pub fn advance(&self, ch: char, px_size: f32) -> f32 {
        let s = self.scaled(px_size);
        s.h_advance(self.font.glyph_id(ch))
    }

    /// Kerning adjustment applied between `a` and `b` at `px_size` (0 if none).
    pub fn kern(&self, a: char, b: char, px_size: f32) -> f32 {
        let s = self.scaled(px_size);
        s.kern(self.font.glyph_id(a), self.font.glyph_id(b))
    }

    /// Distance from the baseline up to the top of the line (positive).
    pub fn ascent(&self, px_size: f32) -> f32 {
        self.scaled(px_size).ascent()
    }

    /// Distance from the baseline down to the bottom of the line (positive).
    pub fn descent(&self, px_size: f32) -> f32 {
        -self.scaled(px_size).descent()
    }

    /// Recommended line-to-line spacing (`ascent + descent + line_gap`).
    pub fn line_height(&self, px_size: f32) -> f32 {
        let s = self.scaled(px_size);
        s.ascent() - s.descent() + s.line_gap()
    }

    /// Rasterizes `ch` at `px_size` into an alpha coverage bitmap.
    ///
    /// Returns `None` for glyphs with no outline (whitespace, `.notdef`); the
    /// advance is still available via [`advance`](Self::advance).
    pub fn rasterize(&self, ch: char, px_size: f32) -> Option<RasterGlyph> {
        let s = self.scaled(px_size);
        let gid = self.font.glyph_id(ch);
        let advance = s.h_advance(gid);
        let glyph: Glyph = gid.with_scale_and_position(PxScale::from(px_size), point(0.0, 0.0));
        let outlined = self.font.outline_glyph(glyph)?;
        let bounds = outlined.px_bounds();
        let w = (bounds.max.x - bounds.min.x).ceil().max(0.0) as usize;
        let h = (bounds.max.y - bounds.min.y).ceil().max(0.0) as usize;
        if w == 0 || h == 0 {
            return None;
        }
        let mut coverage = vec![0u8; w * h];
        outlined.draw(|gx, gy, c| {
            let (gx, gy) = (gx as usize, gy as usize);
            if gx < w && gy < h {
                coverage[gy * w + gx] = (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        });
        Some(RasterGlyph {
            width: w,
            height: h,
            coverage,
            left: bounds.min.x,
            top: bounds.min.y,
            advance,
        })
    }

    /// Width of a single line of text (advances + kerning), no line breaks.
    pub fn measure_line(&self, text: &str, px_size: f32) -> f32 {
        let mut width = 0.0;
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            if let Some(p) = prev {
                width += self.kern(p, ch, px_size);
            }
            width += self.advance(ch, px_size);
            prev = Some(ch);
        }
        width
    }

    /// Bounding size of (possibly multi-line) text: max line width by total
    /// line height.
    pub fn measure(&self, text: &str, px_size: f32) -> Vector2 {
        let lines = text.split('\n');
        let mut max_w = 0.0f32;
        let mut count = 0;
        for line in lines {
            max_w = max_w.max(self.measure_line(line, px_size));
            count += 1;
        }
        Vector2::new(max_w, count as f32 * self.line_height(px_size))
    }

    /// Wraps `text` to lines no wider than `max_width`, breaking at whitespace.
    /// Existing `\n` are hard breaks. A single word wider than `max_width` is
    /// broken between characters.
    pub fn wrap(&self, text: &str, px_size: f32, max_width: f32) -> Vec<String> {
        let mut out = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.measure_line(&candidate, px_size) <= max_width || line.is_empty() {
                    // A single over-long word (line was empty) is broken by char.
                    if line.is_empty() && self.measure_line(word, px_size) > max_width {
                        for chunk in self.break_word(word, px_size, max_width) {
                            out.push(chunk);
                        }
                        line = out.pop().unwrap_or_default();
                    } else {
                        line = candidate;
                    }
                } else {
                    out.push(std::mem::take(&mut line));
                    line = word.to_string();
                }
            }
            out.push(line);
        }
        out
    }

    /// Breaks a single word into character chunks each within `max_width`.
    fn break_word(&self, word: &str, px_size: f32, max_width: f32) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut cur = String::new();
        for ch in word.chars() {
            let candidate = format!("{cur}{ch}");
            if self.measure_line(&candidate, px_size) > max_width && !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
                cur.push(ch);
            } else {
                cur = candidate;
            }
        }
        if !cur.is_empty() {
            chunks.push(cur);
        }
        chunks
    }

    /// Truncates `text` to fit `max_width`, appending an ellipsis (`…`) when it
    /// would otherwise overflow. Returns `text` unchanged if it already fits.
    pub fn ellipsize(&self, text: &str, px_size: f32, max_width: f32) -> String {
        if self.measure_line(text, px_size) <= max_width {
            return text.to_string();
        }
        const ELLIPSIS: char = '…';
        let ell_w = self.advance(ELLIPSIS, px_size);
        if ell_w > max_width {
            return String::new();
        }
        let mut kept = String::new();
        for ch in text.chars() {
            let candidate = format!("{kept}{ch}");
            if self.measure_line(&candidate, px_size) + ell_w > max_width {
                break;
            }
            kept = candidate;
        }
        kept.push(ELLIPSIS);
        kept
    }
}

/// One cached glyph within a [`GlyphAtlas`].
#[derive(Debug, Clone, Copy)]
pub struct AtlasGlyph {
    /// X of the glyph's top-left in the atlas texture.
    pub x: usize,
    /// Y of the glyph's top-left in the atlas texture.
    pub y: usize,
    /// Glyph bitmap width.
    pub w: usize,
    /// Glyph bitmap height.
    pub h: usize,
    /// X offset from the pen position.
    pub left: f32,
    /// Y offset from the baseline.
    pub top: f32,
    /// Horizontal advance.
    pub advance: f32,
}

/// A grayscale glyph atlas that packs rasterized glyphs (shelf packing) and
/// caches them by `(glyph_id, quantized_size)`.
#[derive(Debug)]
pub struct GlyphAtlas {
    /// Atlas texture width.
    pub width: usize,
    /// Atlas texture height.
    pub height: usize,
    /// `width * height` grayscale coverage, row-major.
    pub pixels: Vec<u8>,
    cache: HashMap<(u16, u32), AtlasGlyph>,
    cursor_x: usize,
    cursor_y: usize,
    shelf_height: usize,
}

impl GlyphAtlas {
    /// Creates an atlas with an initial `width x height` texture.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; width * height],
            cache: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            shelf_height: 0,
        }
    }

    /// Number of distinct glyphs currently packed.
    pub fn glyph_count(&self) -> usize {
        self.cache.len()
    }

    fn quantize(px_size: f32) -> u32 {
        (px_size * 4.0).round() as u32
    }

    /// Returns the atlas entry for `ch` at `px_size`, rasterizing and packing it
    /// on first use. Returns `None` for glyphs with no outline (e.g. space).
    pub fn get_or_insert(&mut self, font: &TtfFont, ch: char, px_size: f32) -> Option<AtlasGlyph> {
        let key = (font.font.glyph_id(ch).0, Self::quantize(px_size));
        if let Some(g) = self.cache.get(&key) {
            return Some(*g);
        }
        let raster = font.rasterize(ch, px_size)?;
        let (w, h) = (raster.width, raster.height);

        // Shelf placement: advance along the current row, wrapping to a new
        // shelf and growing the texture height as needed.
        if self.cursor_x + w > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.shelf_height;
            self.shelf_height = 0;
        }
        while self.cursor_y + h > self.height {
            self.grow_height();
        }
        let (gx, gy) = (self.cursor_x, self.cursor_y);
        for row in 0..h {
            let dst = (gy + row) * self.width + gx;
            let src = row * w;
            self.pixels[dst..dst + w].copy_from_slice(&raster.coverage[src..src + w]);
        }
        self.cursor_x += w + 1;
        self.shelf_height = self.shelf_height.max(h + 1);

        let entry = AtlasGlyph {
            x: gx,
            y: gy,
            w,
            h,
            left: raster.left,
            top: raster.top,
            advance: raster.advance,
        };
        self.cache.insert(key, entry);
        Some(entry)
    }

    fn grow_height(&mut self) {
        let new_height = (self.height * 2).max(self.height + 1);
        let mut pixels = vec![0u8; self.width * new_height];
        pixels[..self.pixels.len()].copy_from_slice(&self.pixels);
        self.pixels = pixels;
        self.height = new_height;
    }
}

/// Draws `text` into `fb` at `pos` (top-left of the first line) in `color`,
/// shaping with kerning and rasterizing/caching glyphs through `atlas`.
/// Handles `\n` line breaks.
pub fn draw_ttf_text(
    fb: &mut FrameBuffer,
    atlas: &mut GlyphAtlas,
    font: &TtfFont,
    text: &str,
    pos: Vector2,
    px_size: f32,
    color: Color,
) {
    draw_ttf_text_with_fallbacks(fb, atlas, font, &[], text, pos, px_size, color)
}

/// Picks the face that actually has `ch`: the primary if it covers the glyph,
/// else the first fallback that does (pat-0q2kx.146).
///
/// Godot's `Font.fallbacks` is a per-glyph chain, not a per-string one — a run
/// of Latin text with one CJK character pulls that single character from the
/// fallback face and leaves the rest on the primary. Returning the primary when
/// nothing covers `ch` is deliberate: that is what draws `.notdef`, which is the
/// correct visible signal for a glyph no face in the chain has.
fn face_for<'a>(primary: &'a TtfFont, fallbacks: &'a [TtfFont], ch: char) -> &'a TtfFont {
    if primary.has_glyph(ch) {
        return primary;
    }
    fallbacks
        .iter()
        .find(|f| f.has_glyph(ch))
        .unwrap_or(primary)
}

/// [`draw_ttf_text`] with a Godot-style per-glyph fallback chain
/// (pat-0q2kx.146).
///
/// Metrics stay on the **primary** face — ascent, line height and the baseline
/// come from it — so a fallback glyph sits on the same baseline as its
/// neighbours instead of shifting the line. Only the glyph outline and its
/// advance come from the covering face, which is what Godot's text server does.
#[allow(clippy::too_many_arguments)]
pub fn draw_ttf_text_with_fallbacks(
    fb: &mut FrameBuffer,
    atlas: &mut GlyphAtlas,
    font: &TtfFont,
    fallbacks: &[TtfFont],
    text: &str,
    pos: Vector2,
    px_size: f32,
    color: Color,
) {
    let ascent = font.ascent(px_size);
    let line_height = font.line_height(px_size);
    let mut baseline_y = pos.y + ascent;
    let mut pen_x = pos.x;
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if ch == '\n' {
            baseline_y += line_height;
            pen_x = pos.x;
            prev = None;
            continue;
        }
        // Kerning is a property of the face the pair is drawn from; a pair
        // split across two faces has no shared kern table, so skip it.
        let face = face_for(font, fallbacks, ch);
        if let Some(p) = prev {
            if face.has_glyph(p) {
                pen_x += face.kern(p, ch, px_size);
            }
        }
        if let Some(g) = atlas.get_or_insert(face, ch, px_size) {
            let ox = (pen_x + g.left).round() as i32;
            let oy = (baseline_y + g.top).round() as i32;
            for gy in 0..g.h {
                for gx in 0..g.w {
                    let cov = atlas.pixels[(g.y + gy) * atlas.width + (g.x + gx)];
                    if cov == 0 {
                        continue;
                    }
                    let px = ox + gx as i32;
                    let py = oy + gy as i32;
                    if px < 0 || py < 0 {
                        continue;
                    }
                    let alpha = (cov as f32 / 255.0) * color.a;
                    fb.blend_pixel(
                        px as u32,
                        py as u32,
                        Color::new(color.r, color.g, color.b, alpha),
                    );
                }
            }
            pen_x += g.advance;
        } else {
            // No outline (e.g. space): still advance the pen.
            pen_x += font.advance(ch, px_size);
        }
        prev = Some(ch);
    }
}

/// Renders `text` as a `Label` would within `rect`.
///
/// With `autowrap`, the text is word-wrapped to the rect width; otherwise each
/// line is ellipsized to fit the width on a single line. Lines are drawn
/// top-down and clipped to the rect height. Returns the number of lines drawn.
#[allow(clippy::too_many_arguments)]
pub fn draw_label_text(
    fb: &mut FrameBuffer,
    atlas: &mut GlyphAtlas,
    font: &TtfFont,
    text: &str,
    rect: Rect2,
    px_size: f32,
    color: Color,
    autowrap: bool,
) -> usize {
    let lines: Vec<String> = if autowrap {
        font.wrap(text, px_size, rect.size.x)
    } else {
        text.split('\n')
            .map(|line| font.ellipsize(line, px_size, rect.size.x))
            .collect()
    };
    let line_height = font.line_height(px_size);
    let bottom = rect.position.y + rect.size.y;
    let mut y = rect.position.y;
    let mut drawn = 0;
    for line in &lines {
        if y + line_height > bottom {
            break;
        }
        draw_ttf_text(
            fb,
            atlas,
            font,
            line,
            Vector2::new(rect.position.x, y),
            px_size,
            color,
        );
        y += line_height;
        drawn += 1;
    }
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_assets/Cantarell-Regular.ttf"
    ));

    fn font() -> TtfFont {
        TtfFont::from_bytes(FONT.to_vec()).expect("Cantarell parses")
    }

    #[test]
    fn parses_and_reports_metrics() {
        let f = font();
        assert!(f.has_glyph('H'));
        assert!(f.advance('H', 32.0) > 0.0);
        assert!(f.advance(' ', 32.0) > 0.0);
        assert!(f.ascent(32.0) > 0.0);
        assert!(f.descent(32.0) > 0.0);
        assert!(f.line_height(32.0) > f.ascent(32.0));
        // Advances scale with size.
        assert!(f.advance('H', 64.0) > f.advance('H', 32.0) * 1.5);
    }

    #[test]
    fn builtin_default_font_loads_and_renders() {
        // The embedded default font parses and has usable glyphs.
        let f = TtfFont::builtin();
        assert!(f.has_glyph('A'));
        assert!(f.advance('A', 16.0) > 0.0);
        assert!(f.rasterize('A', 24.0).is_some());
    }

    #[test]
    fn rasterizes_glyph_coverage() {
        let f = font();
        let g = f.rasterize('H', 48.0).expect("H has an outline");
        assert!(g.width > 0 && g.height > 0);
        // Some pixels are covered.
        assert!(g.coverage.iter().any(|&c| c > 0));
        // Whitespace has no outline.
        assert!(f.rasterize(' ', 48.0).is_none());
    }

    #[test]
    fn non_ascii_glyphs() {
        let f = font();
        assert!(f.has_glyph('é'));
        assert!(f.has_glyph('ñ'));
        assert!(f.rasterize('é', 40.0).is_some());
    }

    #[test]
    fn measure_grows_with_length() {
        let f = font();
        let a = f.measure_line("Hi", 24.0);
        let b = f.measure_line("Hello", 24.0);
        assert!(b > a && a > 0.0);
        let size = f.measure("Hi", 24.0);
        assert!(size.x > 0.0 && size.y > 0.0);
        // Two lines are taller than one.
        assert!(f.measure("a\nb", 24.0).y > f.measure("a", 24.0).y);
    }

    #[test]
    fn wrap_breaks_long_text() {
        let f = font();
        let text = "the quick brown fox jumps over the lazy dog";
        let full = f.measure_line(text, 24.0);
        let max_w = full / 3.0;
        let lines = f.wrap(text, 24.0, max_w);
        assert!(lines.len() >= 3, "expected multiple wrapped lines");
        for line in &lines {
            assert!(
                f.measure_line(line, 24.0) <= max_w + f.advance('m', 24.0),
                "line exceeds max width: {line:?}"
            );
        }
        // Hard newline forces a break.
        assert_eq!(f.wrap("a\nb", 24.0, 10_000.0), vec!["a", "b"]);
    }

    #[test]
    fn ellipsize_truncates() {
        let f = font();
        let text = "Hello, world!";
        let full = f.measure_line(text, 24.0);
        // Fits: unchanged.
        assert_eq!(f.ellipsize(text, 24.0, full + 10.0), text);
        // Narrow: truncated with an ellipsis, fits the budget.
        let narrow = full / 2.0;
        let out = f.ellipsize(text, 24.0, narrow);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() < text.chars().count() + 1);
        assert!(f.measure_line(&out, 24.0) <= narrow);
    }

    #[test]
    fn draws_text_into_framebuffer() {
        let f = font();
        let mut atlas = GlyphAtlas::new(256, 64);
        let mut fb = FrameBuffer::new(200, 60, Color::new(0.0, 0.0, 0.0, 1.0));
        draw_ttf_text(
            &mut fb,
            &mut atlas,
            &f,
            "Hi",
            Vector2::new(4.0, 4.0),
            32.0,
            Color::new(1.0, 1.0, 1.0, 1.0),
        );
        // Glyphs were rasterized into the atlas.
        assert!(atlas.glyph_count() >= 2);
        // At least some foreground pixels were drawn (non-black).
        let drawn = fb.pixels.iter().filter(|c| c.r > 0.1).count();
        assert!(drawn > 0, "expected rendered glyph pixels");
    }

    #[test]
    fn label_autowraps_and_clips_to_rect() {
        let f = font();
        let mut atlas = GlyphAtlas::new(256, 256);
        let text = "the quick brown fox jumps over the lazy dog";

        // Tall rect: all wrapped lines fit and are drawn.
        let mut fb = FrameBuffer::new(140, 240, Color::new(0.0, 0.0, 0.0, 1.0));
        let n = draw_label_text(
            &mut fb,
            &mut atlas,
            &f,
            text,
            Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(120.0, 236.0)),
            20.0,
            Color::WHITE,
            true,
        );
        assert!(
            n >= 3,
            "text should wrap to multiple lines in a narrow rect"
        );
        assert!(fb.pixels.iter().any(|c| c.r > 0.1), "glyph pixels drawn");

        // Short rect: lines beyond the height are clipped.
        let mut fb2 = FrameBuffer::new(140, 44, Color::new(0.0, 0.0, 0.0, 1.0));
        let n2 = draw_label_text(
            &mut fb2,
            &mut atlas,
            &f,
            text,
            Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(120.0, 40.0)),
            20.0,
            Color::WHITE,
            true,
        );
        assert!(n2 < n, "shorter rect should clip to fewer lines");
    }

    #[test]
    fn atlas_caches_repeated_glyphs() {
        let f = font();
        let mut atlas = GlyphAtlas::new(128, 128);
        let a = atlas.get_or_insert(&f, 'A', 20.0).unwrap();
        let b = atlas.get_or_insert(&f, 'A', 20.0).unwrap();
        assert_eq!((a.x, a.y), (b.x, b.y));
        assert_eq!(atlas.glyph_count(), 1);
        // A different size is a distinct cache entry.
        atlas.get_or_insert(&f, 'A', 40.0).unwrap();
        assert_eq!(atlas.glyph_count(), 2);
    }
}
