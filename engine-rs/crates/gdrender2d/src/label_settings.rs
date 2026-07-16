//! Label styling resources — `LabelSettings`, `FontVariation`, and `SystemFont`.
//!
//! These mirror Godot's per-`Label` styling resources. A `LabelSettings` holds
//! the font/size/color/outline/shadow/spacing a `Label` applies instead of its
//! theme values; a `FontVariation` wraps a base font with variation
//! coordinates, embolden, and per-type spacing; a `SystemFont` resolves an
//! OS-installed family by name with a deterministic fallback.
//!
//! This module provides the resource data model and the pure metric helpers
//! (spacing-adjusted advances, family resolution). Registering the classes,
//! parsing them from `.tres`, and applying them in the `Label` draw path are
//! wired elsewhere.

use gdcore::math::{Color, Vector2};

// ---------------------------------------------------------------------------
// LabelSettings
// ---------------------------------------------------------------------------

/// Per-`Label` style resource (Godot `LabelSettings`). When a `Label` carries
/// one, these values override the equivalent theme items.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelSettings {
    /// Path to the font resource, if overridden.
    pub font: Option<String>,
    /// Font size in pixels.
    pub font_size: i64,
    /// Text fill color.
    pub font_color: Color,
    /// Outline thickness in pixels (`0` disables the outline).
    pub outline_size: i64,
    /// Outline color.
    pub outline_color: Color,
    /// Shadow size in pixels.
    pub shadow_size: i64,
    /// Shadow color (a transparent color disables the shadow).
    pub shadow_color: Color,
    /// Shadow offset from the text, in pixels.
    pub shadow_offset: Vector2,
    /// Extra spacing added between lines, in pixels.
    pub line_spacing: f32,
    /// Extra spacing added between paragraphs, in pixels.
    pub paragraph_spacing: f32,
}

impl Default for LabelSettings {
    fn default() -> Self {
        Self {
            font: None,
            font_size: 16,
            font_color: Color::WHITE,
            outline_size: 0,
            outline_color: Color::BLACK,
            shadow_size: 1,
            shadow_color: Color::TRANSPARENT,
            shadow_offset: Vector2::new(1.0, 1.0),
            line_spacing: 3.0,
            paragraph_spacing: 0.0,
        }
    }
}

impl LabelSettings {
    /// Creates label settings with Godot's defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a drop shadow should be drawn (its color is not fully transparent).
    pub fn has_shadow(&self) -> bool {
        self.shadow_color.a > 0.0
    }

    /// Whether an outline should be drawn (positive size and a visible color).
    pub fn has_outline(&self) -> bool {
        self.outline_size > 0 && self.outline_color.a > 0.0
    }
}

// ---------------------------------------------------------------------------
// FontVariation
// ---------------------------------------------------------------------------

/// Which metric a [`FontVariation`] spacing value adjusts
/// (Godot `TextServer.SpacingType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSpacing {
    /// Space added to each glyph's horizontal advance.
    Glyph,
    /// Extra space added to whitespace glyphs.
    Space,
    /// Space added above each line.
    Top,
    /// Space added below each line.
    Bottom,
}

/// A base font plus variation coordinates, embolden, and per-type spacing
/// (Godot `FontVariation`).
#[derive(Debug, Clone, PartialEq)]
pub struct FontVariation {
    /// Path to the base font resource.
    pub base_font: Option<String>,
    /// Fake-bold strength (Godot `variation_embolden`); `0.0` leaves the font
    /// unmodified.
    pub embolden: f32,
    /// Per-type spacing, indexed by [`FontSpacing`] as `[glyph, space, top, bottom]`.
    spacing: [f32; 4],
    /// OpenType variation axis coordinates, e.g. `("wght", 700.0)`.
    pub variation_coords: Vec<(String, f32)>,
}

impl Default for FontVariation {
    fn default() -> Self {
        Self {
            base_font: None,
            embolden: 0.0,
            spacing: [0.0; 4],
            variation_coords: Vec::new(),
        }
    }
}

impl FontVariation {
    /// Creates an unmodified font variation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the spacing for `which`.
    pub fn set_spacing(&mut self, which: FontSpacing, value: f32) {
        self.spacing[which as usize] = value;
    }

    /// Returns the spacing for `which`.
    pub fn get_spacing(&self, which: FontSpacing) -> f32 {
        self.spacing[which as usize]
    }

    /// Applies spacing to a glyph's base horizontal `advance`, matching Godot:
    /// every glyph gains [`FontSpacing::Glyph`], and whitespace glyphs
    /// additionally gain [`FontSpacing::Space`].
    pub fn advance_with_spacing(&self, advance: f32, is_whitespace: bool) -> f32 {
        let mut adjusted = advance + self.spacing[FontSpacing::Glyph as usize];
        if is_whitespace {
            adjusted += self.spacing[FontSpacing::Space as usize];
        }
        adjusted
    }

    /// Sets an OpenType variation coordinate, replacing any existing value for
    /// `tag` (e.g. `"wght"`, `"wdth"`).
    pub fn set_variation_coord(&mut self, tag: impl Into<String>, value: f32) {
        let tag = tag.into();
        for entry in &mut self.variation_coords {
            if entry.0 == tag {
                entry.1 = value;
                return;
            }
        }
        self.variation_coords.push((tag, value));
    }

    /// Returns the value of the OpenType variation coordinate `tag`, if set.
    pub fn get_variation_coord(&self, tag: &str) -> Option<f32> {
        self.variation_coords
            .iter()
            .find(|(t, _)| t.as_str() == tag)
            .map(|(_, v)| *v)
    }
}

// ---------------------------------------------------------------------------
// SystemFont
// ---------------------------------------------------------------------------

/// Font hinting mode (Godot `TextServer.Hinting`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontHinting {
    /// No hinting.
    None,
    /// Light hinting.
    Light,
    /// Full hinting (Godot's default).
    #[default]
    Normal,
}

/// A system-installed font reference resolved by family name (Godot
/// `SystemFont`).
#[derive(Debug, Clone, PartialEq)]
pub struct SystemFont {
    /// Ordered preferred family names.
    pub font_names: Vec<String>,
    /// Whether antialiasing is enabled.
    pub antialiasing: bool,
    /// Hinting mode.
    pub hinting: FontHinting,
    /// Whether to fall back to any available family when none of the preferred
    /// names match.
    pub allow_system_fallback: bool,
}

impl Default for SystemFont {
    fn default() -> Self {
        Self {
            font_names: Vec::new(),
            antialiasing: true,
            hinting: FontHinting::Normal,
            allow_system_fallback: true,
        }
    }
}

impl SystemFont {
    /// Creates an empty system-font reference with Godot's defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves against the `available` installed families: returns the first
    /// preferred name present (case-insensitive), else the first available
    /// family when [`allow_system_fallback`](Self::allow_system_fallback) is
    /// set, else `None`.
    pub fn resolve_family<'a>(&self, available: &'a [String]) -> Option<&'a str> {
        for want in &self.font_names {
            if let Some(found) = available.iter().find(|a| a.eq_ignore_ascii_case(want)) {
                return Some(found.as_str());
            }
        }
        if self.allow_system_fallback {
            available.first().map(String::as_str)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_settings_defaults_and_flags() {
        let ls = LabelSettings::new();
        assert_eq!(ls.font_size, 16);
        assert_eq!(ls.font_color, Color::WHITE);
        assert!(!ls.has_outline());
        assert!(!ls.has_shadow()); // transparent shadow by default

        let mut ls = LabelSettings::new();
        ls.outline_size = 2;
        ls.outline_color = Color::BLACK;
        ls.shadow_color = Color::new(0.0, 0.0, 0.0, 0.5);
        assert!(ls.has_outline());
        assert!(ls.has_shadow());
    }

    #[test]
    fn font_variation_spacing_and_coords() {
        let mut fv = FontVariation::new();
        assert_eq!(fv.advance_with_spacing(10.0, false), 10.0);

        fv.set_spacing(FontSpacing::Glyph, 2.0);
        fv.set_spacing(FontSpacing::Space, 3.0);
        assert_eq!(fv.get_spacing(FontSpacing::Glyph), 2.0);
        // Non-whitespace glyph: +glyph only.
        assert_eq!(fv.advance_with_spacing(10.0, false), 12.0);
        // Whitespace glyph: +glyph +space.
        assert_eq!(fv.advance_with_spacing(10.0, true), 15.0);

        // Variation coords upsert (second set replaces the first).
        fv.set_variation_coord("wght", 400.0);
        fv.set_variation_coord("wght", 700.0);
        assert_eq!(fv.get_variation_coord("wght"), Some(700.0));
        assert_eq!(fv.get_variation_coord("wdth"), None);
        assert_eq!(fv.variation_coords.len(), 1);
    }

    #[test]
    fn system_font_family_resolution() {
        let available = vec!["Arial".to_string(), "Noto Sans".to_string()];
        let mut sf = SystemFont::new();
        sf.font_names = vec!["Helvetica".into(), "noto sans".into()];
        // First preferred missing; second matches case-insensitively.
        assert_eq!(sf.resolve_family(&available), Some("Noto Sans"));

        // None match, fallback on -> first available family.
        sf.font_names = vec!["Missing".into()];
        assert_eq!(sf.resolve_family(&available), Some("Arial"));

        // None match, fallback off -> None.
        sf.allow_system_fallback = false;
        assert_eq!(sf.resolve_family(&available), None);

        // Empty available set -> None even with fallback.
        sf.allow_system_fallback = true;
        assert_eq!(sf.resolve_family(&[]), None);
    }
}
