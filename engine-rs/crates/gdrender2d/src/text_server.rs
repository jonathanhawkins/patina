//! TextServer: text metrics for Label (pat-z35r2.8, TTF via pat-u3l8c.69).
//!
//! When a real TTF is loaded ([`TextServerLite::with_ttf`]), advances,
//! measurement, word-wrap, and ellipsis come from the outline font (with
//! kerning). Otherwise it falls back to the built-in 5×7 bitmap approximation.

use crate::font::BitmapFont;
use crate::ttf_font::TtfFont;

/// Text metrics server backing `Label`/`Button` text layout.
#[derive(Debug, Default)]
pub struct TextServerLite {
    #[allow(dead_code)]
    font: BitmapFont,
    ttf: Option<TtfFont>,
}

impl TextServerLite {
    /// Creates a bitmap-backed text server.
    pub fn new() -> Self {
        Self {
            font: BitmapFont::builtin(),
            ttf: None,
        }
    }

    /// Creates a TTF-backed text server using outline metrics + kerning.
    pub fn with_ttf(ttf: TtfFont) -> Self {
        Self {
            font: BitmapFont::builtin(),
            ttf: Some(ttf),
        }
    }

    /// Whether this server is backed by a real outline font.
    pub fn has_ttf(&self) -> bool {
        self.ttf.is_some()
    }

    /// Advance width of a single character at `font_size`.
    pub fn get_char_advance(&self, ch: char, font_size: f32) -> f32 {
        if let Some(ttf) = &self.ttf {
            return ttf.advance(ch, font_size);
        }
        let scale = font_size / 8.0;
        let base = if ch == ' ' { 3.0 } else { 6.0 };
        base * scale
    }

    /// Width of a full string at `font_size` (kerned when TTF-backed).
    pub fn get_string_size(&self, text: &str, font_size: f32) -> f32 {
        if let Some(ttf) = &self.ttf {
            return ttf.measure_line(text, font_size);
        }
        text.chars()
            .map(|c| self.get_char_advance(c, font_size))
            .sum()
    }

    /// Word-wraps `text` to `max_width`. Bitmap fallback wraps on advance sums.
    pub fn wrap(&self, text: &str, font_size: f32, max_width: f32) -> Vec<String> {
        if let Some(ttf) = &self.ttf {
            return ttf.wrap(text, font_size, max_width);
        }
        let mut out = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.get_string_size(&candidate, font_size) <= max_width || line.is_empty() {
                    line = candidate;
                } else {
                    out.push(std::mem::take(&mut line));
                    line = word.to_string();
                }
            }
            out.push(line);
        }
        out
    }

    /// Truncates `text` with an ellipsis to fit `max_width`.
    pub fn ellipsize(&self, text: &str, font_size: f32, max_width: f32) -> String {
        if let Some(ttf) = &self.ttf {
            return ttf.ellipsize(text, font_size, max_width);
        }
        if self.get_string_size(text, font_size) <= max_width {
            return text.to_string();
        }
        let ell = self.get_char_advance('…', font_size);
        let mut kept = String::new();
        for ch in text.chars() {
            let candidate = format!("{kept}{ch}");
            if self.get_string_size(&candidate, font_size) + ell > max_width {
                break;
            }
            kept = candidate;
        }
        kept.push('…');
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_assets/Cantarell-Regular.ttf"
    ));

    #[test]
    fn string_size_grows_with_length() {
        let ts = TextServerLite::new();
        let a = ts.get_string_size("Hi", 16.0);
        let b = ts.get_string_size("Hello", 16.0);
        assert!(b > a);
    }

    #[test]
    fn ttf_backed_uses_outline_metrics() {
        let ttf = TtfFont::from_bytes(FONT.to_vec()).unwrap();
        let ts = TextServerLite::with_ttf(ttf);
        assert!(ts.has_ttf());
        assert!(ts.get_char_advance('W', 32.0) > ts.get_char_advance('i', 32.0));
        let full = ts.get_string_size("Hello, world!", 24.0);
        assert!(full > 0.0);

        let lines = ts.wrap("one two three four five", 24.0, full / 2.0);
        assert!(lines.len() >= 2);
        let e = ts.ellipsize("Hello, world!", 24.0, full / 2.0);
        assert!(e.ends_with('…'));
        assert!(ts.get_string_size(&e, 24.0) <= full / 2.0);
    }

    #[test]
    fn bitmap_fallback_wrap_and_ellipsize() {
        let ts = TextServerLite::new();
        let lines = ts.wrap("aaa bbb ccc ddd", 16.0, ts.get_string_size("aaa bbb", 16.0));
        assert!(lines.len() >= 2);
        let e = ts.ellipsize("aaaaaaaaaa", 16.0, ts.get_string_size("aaa", 16.0));
        assert!(e.ends_with('…'));
    }
}
