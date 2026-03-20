//! Bitmap font rendering for the Patina Engine.
//!
//! Provides a simple 5×7 monospace bitmap font with glyph data for ASCII
//! characters, along with higher-level [`BitmapFont`] and [`FontFile`] types
//! that mirror Godot's font API surface.

use gdcore::math::{Color, Rect2, Vector2};

use crate::renderer::FrameBuffer;

// ===========================================================================
// Built-in glyph constants
// ===========================================================================

/// Width of each glyph cell in pixels.
pub const CHAR_WIDTH: u32 = 5;
/// Height of each glyph cell in pixels.
pub const CHAR_HEIGHT: u32 = 7;
/// Gap between glyphs in pixels.
pub const CHAR_GAP: u32 = 1;

/// Returns the 5×7 bitmap rows for the given character.
///
/// Each `u8` represents one row of pixels (MSB is the leftmost column).
/// Unknown characters return a filled rectangle as a fallback glyph.
pub fn char_bitmap(ch: char) -> [u8; 7] {
    match ch {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        'a' => [
            0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111,
        ],
        'b' => [
            0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b00000, 0b00000, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'd' => [
            0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b10001, 0b01111,
        ],
        'e' => [
            0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110,
        ],
        'f' => [
            0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000,
        ],
        'g' => [
            0b00000, 0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b01110,
        ],
        'h' => [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'j' => [
            0b00010, 0b00000, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'k' => [
            0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010,
        ],
        'l' => [
            0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'm' => [
            0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10101, 0b10001,
        ],
        'n' => [
            0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000,
        ],
        'q' => [
            0b00000, 0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001,
        ],
        'r' => [
            0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000,
        ],
        's' => [
            0b00000, 0b00000, 0b01110, 0b10000, 0b01110, 0b00001, 0b11110,
        ],
        't' => [
            0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110,
        ],
        'u' => [
            0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101,
        ],
        'v' => [
            0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'w' => [
            0b00000, 0b00000, 0b10001, 0b10001, 0b10101, 0b10101, 0b01010,
        ],
        'x' => [
            0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001,
        ],
        'y' => [
            0b00000, 0b00000, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110,
        ],
        'z' => [
            0b00000, 0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        ':' => [
            0b00000, 0b00100, 0b00000, 0b00000, 0b00000, 0b00100, 0b00000,
        ],
        ';' => [
            0b00000, 0b00100, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '=' => [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '\\' => [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
        '\'' => [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '"' => [
            0b01010, 0b01010, 0b10100, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
        '#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        '$' => [
            0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100,
        ],
        '%' => [
            0b11001, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b10011,
        ],
        '&' => [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        '*' => [
            0b00000, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0b00000,
        ],
        '<' => [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '{' => [
            0b00110, 0b00100, 0b00100, 0b01000, 0b00100, 0b00100, 0b00110,
        ],
        '}' => [
            0b01100, 0b00100, 0b00100, 0b00010, 0b00100, 0b00100, 0b01100,
        ],
        '~' => [
            0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000,
        ],
        '`' => [
            0b01000, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '^' => [
            0b00100, 0b01010, 0b10001, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}

/// Computes the pixel width of a single line of text at the given scale.
pub fn text_width(text: &str, scale: u32) -> u32 {
    let sc = scale.max(1);
    let n = text.len() as u32;
    if n == 0 {
        0
    } else {
        n * CHAR_WIDTH * sc + (n - 1) * CHAR_GAP * sc
    }
}

/// Computes the pixel height of (possibly multi-line) text at the given scale.
pub fn text_height(text: &str, scale: u32) -> u32 {
    let sc = scale.max(1);
    let l = text.split('\n').count() as u32;
    if l == 0 {
        0
    } else {
        l * CHAR_HEIGHT * sc + (l - 1) * CHAR_GAP * sc
    }
}

/// Returns `true` if the built-in bitmap font has a glyph for `ch`.
pub fn has_glyph(ch: char) -> bool {
    matches!(ch, 'A'..='Z'|'a'..='z'|'0'..='9'|'.'|','|'!'|'?'|':'|';'|'-'|'_'|'+'|'='|'('|')'|'/'|'\\'|'\''|'"'|'@'|'#'|'$'|'%'|'&'|'*'|'<'|'>'|'['|']'|'{'|'}'|'~'|'`'|'^'|'|'|' ')
}

// ===========================================================================
// Glyph / BitmapFont / FontFile
// ===========================================================================

/// A single glyph's metrics and atlas location.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// Character code this glyph represents.
    pub character: char,
    /// Region in the font atlas texture (for atlas-backed fonts).
    pub atlas_rect: Rect2,
    /// Horizontal advance after rendering this glyph.
    pub advance: f32,
}

/// A bitmap font that maps characters to glyphs.
///
/// For v1, each character is rendered as a colored rectangle (monospace
/// approximation). The glyph table provides the API surface so that UI
/// nodes (Label, Button) can query font metrics and render text.
#[derive(Debug, Clone)]
pub struct BitmapFont {
    /// Font name (e.g. "default_bitmap").
    pub name: String,
    /// Width of each character cell in pixels.
    pub char_width: u32,
    /// Height of each character cell in pixels.
    pub char_height: u32,
    /// Horizontal gap between characters in pixels.
    pub char_gap: u32,
    /// Glyph table: character → glyph data.
    glyphs: Vec<Glyph>,
}

impl BitmapFont {
    /// Creates the built-in 5×7 bitmap font.
    pub fn builtin() -> Self {
        let mut glyphs = Vec::new();
        // Register all known ASCII glyphs.
        let chars: Vec<char> = ('A'..='Z')
            .chain('a'..='z')
            .chain('0'..='9')
            .chain(" .,!?:;-_+=()/'\"@#$%&*<>[]{}~`^|\\".chars())
            .collect();
        for ch in chars {
            glyphs.push(Glyph {
                character: ch,
                atlas_rect: Rect2::new(
                    Vector2::ZERO,
                    Vector2::new(CHAR_WIDTH as f32, CHAR_HEIGHT as f32),
                ),
                advance: (CHAR_WIDTH + CHAR_GAP) as f32,
            });
        }
        Self {
            name: "builtin_5x7".to_string(),
            char_width: CHAR_WIDTH,
            char_height: CHAR_HEIGHT,
            char_gap: CHAR_GAP,
            glyphs,
        }
    }

    /// Creates a bitmap font with custom cell dimensions.
    pub fn new(name: &str, char_width: u32, char_height: u32, char_gap: u32) -> Self {
        Self {
            name: name.to_string(),
            char_width,
            char_height,
            char_gap,
            glyphs: Vec::new(),
        }
    }

    /// Adds a glyph to the font.
    pub fn add_glyph(&mut self, glyph: Glyph) {
        self.glyphs.push(glyph);
    }

    /// Looks up the glyph for a character.
    pub fn get_glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.iter().find(|g| g.character == ch)
    }

    /// Returns the number of registered glyphs.
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Computes the pixel width of a single line of text.
    pub fn string_width(&self, text: &str) -> u32 {
        let n = text.len() as u32;
        if n == 0 {
            0
        } else {
            n * self.char_width + (n - 1) * self.char_gap
        }
    }

    /// Computes the pixel height of a single line.
    pub fn line_height(&self) -> u32 {
        self.char_height
    }
}

impl Default for BitmapFont {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Font-level metrics, analogous to Godot's FontFile.
///
/// Stores typographic measurements that describe the overall font geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFile {
    /// Font family name.
    pub family: String,
    /// Distance from the baseline to the top of tall glyphs (in pixels).
    pub ascent: f32,
    /// Distance from the baseline to the bottom of descenders (positive value, in pixels).
    pub descent: f32,
    /// Total line height including leading.
    pub line_height: f32,
    /// Font size in pixels.
    pub size: f32,
}

impl FontFile {
    /// Creates font metrics for the built-in 5×7 font.
    pub fn builtin() -> Self {
        Self {
            family: "builtin_5x7".to_string(),
            ascent: 5.0,
            descent: 2.0,
            line_height: CHAR_HEIGHT as f32,
            size: CHAR_HEIGHT as f32,
        }
    }

    /// Creates font metrics with custom values.
    pub fn new(family: &str, ascent: f32, descent: f32, line_height: f32, size: f32) -> Self {
        Self {
            family: family.to_string(),
            ascent,
            descent,
            line_height,
            size,
        }
    }

    /// Returns the total height (ascent + descent).
    pub fn total_height(&self) -> f32 {
        self.ascent + self.descent
    }
}

impl Default for FontFile {
    fn default() -> Self {
        Self::builtin()
    }
}

// ===========================================================================
// draw_string — rasterizes text into a FrameBuffer
// ===========================================================================

/// Renders a string into the framebuffer using the built-in bitmap font.
///
/// Each character is drawn pixel-by-pixel from its 5×7 bitmap data.
/// `position` is the top-left corner of the first character. `scale`
/// magnifies each pixel (1 = native, 2 = doubled, etc.).
pub fn draw_string(
    fb: &mut FrameBuffer,
    font: &BitmapFont,
    position: Vector2,
    text: &str,
    color: Color,
    scale: u32,
) {
    let sc = scale.max(1);
    let mut cursor_x = position.x as i32;
    let base_y = position.y as i32;

    for ch in text.chars() {
        if ch == '\n' {
            // Newlines are not handled in this single-line helper;
            // use draw_string_multiline for multi-line text.
            continue;
        }

        let bitmap = char_bitmap(ch);

        for (row_idx, &row_bits) in bitmap.iter().enumerate() {
            for col in 0..font.char_width {
                let bit = (row_bits >> (font.char_width - 1 - col)) & 1;
                if bit == 1 {
                    // Draw a scale×scale block for this pixel.
                    for sy in 0..sc {
                        for sx in 0..sc {
                            let px = cursor_x + (col * sc + sx) as i32;
                            let py = base_y + (row_idx as u32 * sc + sy) as i32;
                            if px >= 0
                                && py >= 0
                                && (px as u32) < fb.width
                                && (py as u32) < fb.height
                            {
                                fb.set_pixel(px as u32, py as u32, color);
                            }
                        }
                    }
                }
            }
        }

        cursor_x += (font.char_width * sc + font.char_gap * sc) as i32;
    }
}

/// Renders multi-line text into the framebuffer.
///
/// Lines are split on `'\n'` and rendered top-to-bottom.
pub fn draw_string_multiline(
    fb: &mut FrameBuffer,
    font: &BitmapFont,
    position: Vector2,
    text: &str,
    color: Color,
    scale: u32,
) {
    let sc = scale.max(1);
    let line_spacing = font.char_height * sc + font.char_gap * sc;

    for (line_idx, line) in text.split('\n').enumerate() {
        let line_y = position.y + (line_idx as u32 * line_spacing) as f32;
        draw_string(fb, font, Vector2::new(position.x, line_y), line, color, sc);
    }
}

/// Returns the bounding rect for rendered text.
pub fn string_bounding_rect(font: &BitmapFont, text: &str, scale: u32) -> Rect2 {
    let sc = scale.max(1);
    let lines: Vec<&str> = text.split('\n').collect();
    let max_width = lines
        .iter()
        .map(|l| font.string_width(l) * sc)
        .max()
        .unwrap_or(0);
    let height = if lines.is_empty() {
        0
    } else {
        lines.len() as u32 * font.char_height * sc + (lines.len() as u32 - 1) * font.char_gap * sc
    };
    Rect2::new(Vector2::ZERO, Vector2::new(max_width as f32, height as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper() {
        for c in 'A'..='Z' {
            assert!(has_glyph(c));
            assert!(char_bitmap(c).iter().any(|&r| r != 0));
        }
    }

    #[test]
    fn lower() {
        for c in 'a'..='z' {
            assert!(has_glyph(c));
            assert!(char_bitmap(c).iter().any(|&r| r != 0));
        }
    }

    #[test]
    fn digits() {
        for c in '0'..='9' {
            assert!(has_glyph(c));
            assert!(char_bitmap(c).iter().any(|&r| r != 0));
        }
    }

    #[test]
    fn punct() {
        for c in ".,!?:;-_+=()".chars() {
            assert!(has_glyph(c));
        }
    }

    #[test]
    fn space() {
        assert!(char_bitmap(' ').iter().all(|&r| r == 0));
    }

    #[test]
    fn widths() {
        assert_eq!(text_width("", 1), 0);
        assert_eq!(text_width("A", 1), 5);
        assert_eq!(text_width("ABC", 1), 17);
        assert_eq!(text_width("AB", 2), 22);
    }

    #[test]
    fn heights() {
        assert_eq!(text_height("Hi", 1), 7);
        assert_eq!(text_height("A\nB", 1), 15);
        assert_eq!(text_height("A\nB", 2), 30);
    }

    #[test]
    fn bits() {
        for c in ('A'..='Z')
            .chain('a'..='z')
            .chain('0'..='9')
            .chain(" .,!?:;-_+=()".chars())
        {
            for &r in &char_bitmap(c) {
                assert!(r <= 0b11111);
            }
        }
    }

    // --- BitmapFont tests ---

    #[test]
    fn builtin_font_has_all_ascii_glyphs() {
        let font = BitmapFont::builtin();
        assert!(font.glyph_count() > 60);
        assert!(font.get_glyph('A').is_some());
        assert!(font.get_glyph('z').is_some());
        assert!(font.get_glyph('5').is_some());
        assert!(font.get_glyph(' ').is_some());
    }

    #[test]
    fn bitmap_font_string_width() {
        let font = BitmapFont::builtin();
        assert_eq!(font.string_width(""), 0);
        assert_eq!(font.string_width("A"), 5);
        // "AB" = 5 + 1 + 5 = 11
        assert_eq!(font.string_width("AB"), 11);
        assert_eq!(font.string_width("ABC"), 17);
    }

    #[test]
    fn bitmap_font_line_height() {
        let font = BitmapFont::builtin();
        assert_eq!(font.line_height(), CHAR_HEIGHT);
    }

    #[test]
    fn custom_bitmap_font() {
        let mut font = BitmapFont::new("custom", 8, 12, 2);
        font.add_glyph(Glyph {
            character: 'X',
            atlas_rect: Rect2::new(Vector2::ZERO, Vector2::new(8.0, 12.0)),
            advance: 10.0,
        });
        assert_eq!(font.glyph_count(), 1);
        assert!(font.get_glyph('X').is_some());
        assert!(font.get_glyph('Y').is_none());
        assert_eq!(font.string_width("XX"), 18); // 8 + 2 + 8
    }

    // --- FontFile tests ---

    #[test]
    fn font_file_builtin_metrics() {
        let ff = FontFile::builtin();
        assert_eq!(ff.ascent, 5.0);
        assert_eq!(ff.descent, 2.0);
        assert_eq!(ff.line_height, 7.0);
        assert_eq!(ff.total_height(), 7.0);
    }

    #[test]
    fn font_file_custom_metrics() {
        let ff = FontFile::new("Arial", 12.0, 4.0, 18.0, 16.0);
        assert_eq!(ff.family, "Arial");
        assert_eq!(ff.total_height(), 16.0);
        assert_eq!(ff.size, 16.0);
    }

    // --- draw_string tests ---

    #[test]
    fn draw_string_renders_pixels() {
        let font = BitmapFont::builtin();
        let mut fb = FrameBuffer::new(30, 10, Color::BLACK);
        let white = Color::WHITE;
        draw_string(&mut fb, &font, Vector2::new(0.0, 0.0), "A", white, 1);

        // The 'A' bitmap has lit pixels; check that at least some are white.
        let mut found = false;
        for y in 0..CHAR_HEIGHT {
            for x in 0..CHAR_WIDTH {
                if fb.get_pixel(x, y) == white {
                    found = true;
                }
            }
        }
        assert!(found, "draw_string should light up pixels for 'A'");
    }

    #[test]
    fn draw_string_empty_is_noop() {
        let font = BitmapFont::builtin();
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        draw_string(&mut fb, &font, Vector2::ZERO, "", Color::WHITE, 1);
        // All pixels remain black.
        for y in 0..10 {
            for x in 0..10 {
                assert_eq!(fb.get_pixel(x, y), Color::BLACK);
            }
        }
    }

    #[test]
    fn draw_string_scale_doubles_size() {
        let font = BitmapFont::builtin();
        let mut fb1 = FrameBuffer::new(20, 20, Color::BLACK);
        let mut fb2 = FrameBuffer::new(20, 20, Color::BLACK);

        draw_string(&mut fb1, &font, Vector2::ZERO, "I", Color::WHITE, 1);
        draw_string(&mut fb2, &font, Vector2::ZERO, "I", Color::WHITE, 2);

        let count1 = fb1.pixels.iter().filter(|p| **p == Color::WHITE).count();
        let count2 = fb2.pixels.iter().filter(|p| **p == Color::WHITE).count();
        // At scale 2, each pixel becomes 2×2, so roughly 4× the lit pixels.
        assert_eq!(count2, count1 * 4);
    }

    #[test]
    fn draw_string_clips_to_framebuffer() {
        let font = BitmapFont::builtin();
        let mut fb = FrameBuffer::new(3, 3, Color::BLACK);
        // Drawing at (0,0) in a 3x3 fb should not panic.
        draw_string(&mut fb, &font, Vector2::ZERO, "A", Color::WHITE, 1);
        // Just verify no panic and some pixels are set.
        let lit = fb.pixels.iter().filter(|p| **p == Color::WHITE).count();
        assert!(lit > 0);
    }

    #[test]
    fn draw_string_multiline_renders_two_lines() {
        let font = BitmapFont::builtin();
        let mut fb = FrameBuffer::new(30, 30, Color::BLACK);
        draw_string_multiline(&mut fb, &font, Vector2::ZERO, "AB\nCD", Color::WHITE, 1);

        // First line should have lit pixels in the top CHAR_HEIGHT rows.
        let mut top_lit = false;
        for y in 0..CHAR_HEIGHT {
            for x in 0..30 {
                if fb.get_pixel(x, y) == Color::WHITE {
                    top_lit = true;
                }
            }
        }
        assert!(top_lit);

        // Second line starts at y = CHAR_HEIGHT + CHAR_GAP = 8.
        let mut bot_lit = false;
        let start_y = CHAR_HEIGHT + CHAR_GAP;
        for y in start_y..(start_y + CHAR_HEIGHT) {
            for x in 0..30 {
                if fb.get_pixel(x, y) == Color::WHITE {
                    bot_lit = true;
                }
            }
        }
        assert!(bot_lit);
    }

    #[test]
    fn string_bounding_rect_single_line() {
        let font = BitmapFont::builtin();
        let r = string_bounding_rect(&font, "Hello", 1);
        assert_eq!(r.size.x, font.string_width("Hello") as f32);
        assert_eq!(r.size.y, CHAR_HEIGHT as f32);
    }

    #[test]
    fn string_bounding_rect_multiline() {
        let font = BitmapFont::builtin();
        let r = string_bounding_rect(&font, "AB\nCDE", 1);
        // Width = max of "AB" (11) and "CDE" (17) = 17
        assert_eq!(r.size.x, 17.0);
        // Height = 2*7 + 1*1 = 15
        assert_eq!(r.size.y, 15.0);
    }
}
