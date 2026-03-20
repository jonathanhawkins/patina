//! Theme system for Control nodes.
//!
//! Provides a Godot-compatible theming API where each Control type can query
//! appearance properties (colors, fonts, font sizes, constants, styleboxes)
//! from a [`Theme`] resource. A global [`ThemeDB`] holds the default theme
//! used when nodes do not override with a custom theme.
//!
//! Theme properties are stored as `(control_type, property_name) → Variant`
//! mappings, organized by property category.

use gdcore::math::Color;
use gdvariant::Variant;
use std::collections::HashMap;

// ===========================================================================
// Theme property categories
// ===========================================================================

/// The category of a theme property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemePropertyType {
    /// Color properties (e.g. font_color, bg_color).
    Color,
    /// Font resource references.
    Font,
    /// Font size in pixels.
    FontSize,
    /// Integer constants (e.g. margin, separation).
    Constant,
    /// Stylebox resources (stored as Variant for flexibility).
    Stylebox,
}

// ===========================================================================
// Theme
// ===========================================================================

/// A composite key for theme lookups: `(control_type, property_name)`.
type ThemeKey = (String, String);

/// A theme resource mapping `(control_type, property_name)` to a [`Variant`]
/// value for each property category.
///
/// Mirrors Godot's Theme resource. Controls query the theme for their
/// appearance settings during rendering.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name for identification.
    pub name: String,
    /// Color properties.
    colors: HashMap<ThemeKey, Color>,
    /// Font size properties.
    font_sizes: HashMap<ThemeKey, i64>,
    /// Integer constant properties.
    constants: HashMap<ThemeKey, i64>,
    /// Generic properties stored as Variant (fonts, styleboxes, etc.).
    items: HashMap<(ThemePropertyType, ThemeKey), Variant>,
}

impl Theme {
    /// Creates an empty theme.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            colors: HashMap::new(),
            font_sizes: HashMap::new(),
            constants: HashMap::new(),
            items: HashMap::new(),
        }
    }

    // -- Colors --------------------------------------------------------------

    /// Sets a color property for a control type.
    pub fn set_color(&mut self, control_type: &str, property: &str, color: Color) {
        self.colors
            .insert((control_type.to_string(), property.to_string()), color);
    }

    /// Gets a color property, returning `None` if not set.
    pub fn get_color(&self, control_type: &str, property: &str) -> Option<Color> {
        self.colors
            .get(&(control_type.to_string(), property.to_string()))
            .copied()
    }

    /// Returns `true` if the color property exists.
    pub fn has_color(&self, control_type: &str, property: &str) -> bool {
        self.colors
            .contains_key(&(control_type.to_string(), property.to_string()))
    }

    // -- Font sizes ----------------------------------------------------------

    /// Sets a font size property for a control type.
    pub fn set_font_size(&mut self, control_type: &str, property: &str, size: i64) {
        self.font_sizes
            .insert((control_type.to_string(), property.to_string()), size);
    }

    /// Gets a font size property, returning `None` if not set.
    pub fn get_font_size(&self, control_type: &str, property: &str) -> Option<i64> {
        self.font_sizes
            .get(&(control_type.to_string(), property.to_string()))
            .copied()
    }

    /// Returns `true` if the font size property exists.
    pub fn has_font_size(&self, control_type: &str, property: &str) -> bool {
        self.font_sizes
            .contains_key(&(control_type.to_string(), property.to_string()))
    }

    // -- Constants -----------------------------------------------------------

    /// Sets an integer constant property for a control type.
    pub fn set_constant(&mut self, control_type: &str, property: &str, value: i64) {
        self.constants
            .insert((control_type.to_string(), property.to_string()), value);
    }

    /// Gets an integer constant, returning `None` if not set.
    pub fn get_constant(&self, control_type: &str, property: &str) -> Option<i64> {
        self.constants
            .get(&(control_type.to_string(), property.to_string()))
            .copied()
    }

    /// Returns `true` if the constant property exists.
    pub fn has_constant(&self, control_type: &str, property: &str) -> bool {
        self.constants
            .contains_key(&(control_type.to_string(), property.to_string()))
    }

    // -- Generic Variant items (fonts, styleboxes) ---------------------------

    /// Sets a theme item by category, control type, and property name.
    pub fn set_item(
        &mut self,
        category: ThemePropertyType,
        control_type: &str,
        property: &str,
        value: Variant,
    ) {
        self.items.insert(
            (category, (control_type.to_string(), property.to_string())),
            value,
        );
    }

    /// Gets a theme item by category, control type, and property name.
    pub fn get_item(
        &self,
        category: ThemePropertyType,
        control_type: &str,
        property: &str,
    ) -> Option<&Variant> {
        self.items
            .get(&(category, (control_type.to_string(), property.to_string())))
    }

    /// Returns `true` if a theme item exists for the given parameters.
    pub fn has_item(
        &self,
        category: ThemePropertyType,
        control_type: &str,
        property: &str,
    ) -> bool {
        self.items
            .contains_key(&(category, (control_type.to_string(), property.to_string())))
    }

    /// Removes a theme item. Returns the old value if it existed.
    pub fn remove_item(
        &mut self,
        category: ThemePropertyType,
        control_type: &str,
        property: &str,
    ) -> Option<Variant> {
        self.items
            .remove(&(category, (control_type.to_string(), property.to_string())))
    }

    /// Creates the engine default theme with sensible defaults for
    /// Label, Button, and Panel controls.
    pub fn default_theme() -> Self {
        let mut theme = Self::new("default");

        // --- Label ---
        theme.set_color("Label", "font_color", Color::WHITE);
        theme.set_font_size("Label", "font_size", 16);
        theme.set_color("Label", "font_shadow_color", Color::new(0.0, 0.0, 0.0, 0.0));
        theme.set_constant("Label", "shadow_offset_x", 1);
        theme.set_constant("Label", "shadow_offset_y", 1);
        theme.set_constant("Label", "line_spacing", 3);

        // --- Button ---
        theme.set_color("Button", "font_color", Color::new(0.875, 0.875, 0.875, 1.0));
        theme.set_color("Button", "font_hover_color", Color::WHITE);
        theme.set_color("Button", "font_pressed_color", Color::WHITE);
        theme.set_color(
            "Button",
            "font_disabled_color",
            Color::new(0.875, 0.875, 0.875, 0.5),
        );
        theme.set_font_size("Button", "font_size", 16);
        theme.set_constant("Button", "h_separation", 4);
        // Stylebox for normal state (dark gray bg).
        theme.set_item(
            ThemePropertyType::Stylebox,
            "Button",
            "normal",
            Variant::Color(Color::new(0.2, 0.2, 0.2, 1.0)),
        );
        theme.set_item(
            ThemePropertyType::Stylebox,
            "Button",
            "hover",
            Variant::Color(Color::new(0.3, 0.3, 0.3, 1.0)),
        );
        theme.set_item(
            ThemePropertyType::Stylebox,
            "Button",
            "pressed",
            Variant::Color(Color::new(0.15, 0.15, 0.15, 1.0)),
        );

        // --- Panel ---
        theme.set_item(
            ThemePropertyType::Stylebox,
            "Panel",
            "panel",
            Variant::Color(Color::new(0.15, 0.15, 0.15, 1.0)),
        );
        theme.set_constant("Panel", "content_margin_left", 4);
        theme.set_constant("Panel", "content_margin_top", 4);
        theme.set_constant("Panel", "content_margin_right", 4);
        theme.set_constant("Panel", "content_margin_bottom", 4);

        theme
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

// ===========================================================================
// ThemeDB — global default theme singleton
// ===========================================================================

/// Global theme database, analogous to Godot's ThemeDB singleton.
///
/// Holds the project-wide default theme that controls fall back to
/// when they have no custom theme assigned.
#[derive(Debug, Clone)]
pub struct ThemeDB {
    /// The global default theme.
    default_theme: Theme,
}

impl ThemeDB {
    /// Creates a new ThemeDB with the engine's default theme.
    pub fn new() -> Self {
        Self {
            default_theme: Theme::default_theme(),
        }
    }

    /// Returns a reference to the global default theme.
    pub fn default_theme(&self) -> &Theme {
        &self.default_theme
    }

    /// Returns a mutable reference to the global default theme.
    pub fn default_theme_mut(&mut self) -> &mut Theme {
        &mut self.default_theme
    }

    /// Replaces the global default theme entirely.
    pub fn set_default_theme(&mut self, theme: Theme) {
        self.default_theme = theme;
    }

    /// Queries a color from the default theme, returning `fallback` if missing.
    pub fn get_color_or(&self, control_type: &str, property: &str, fallback: Color) -> Color {
        self.default_theme
            .get_color(control_type, property)
            .unwrap_or(fallback)
    }

    /// Queries a font size from the default theme, returning `fallback` if missing.
    pub fn get_font_size_or(&self, control_type: &str, property: &str, fallback: i64) -> i64 {
        self.default_theme
            .get_font_size(control_type, property)
            .unwrap_or(fallback)
    }

    /// Queries a constant from the default theme, returning `fallback` if missing.
    pub fn get_constant_or(&self, control_type: &str, property: &str, fallback: i64) -> i64 {
        self.default_theme
            .get_constant(control_type, property)
            .unwrap_or(fallback)
    }
}

impl Default for ThemeDB {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_theme_returns_none() {
        let theme = Theme::new("empty");
        assert!(theme.get_color("Label", "font_color").is_none());
        assert!(theme.get_font_size("Label", "font_size").is_none());
        assert!(theme.get_constant("Panel", "margin").is_none());
    }

    #[test]
    fn set_and_get_color() {
        let mut theme = Theme::new("test");
        let red = Color::rgb(1.0, 0.0, 0.0);
        theme.set_color("Label", "font_color", red);
        assert_eq!(theme.get_color("Label", "font_color"), Some(red));
        assert!(theme.has_color("Label", "font_color"));
        assert!(!theme.has_color("Label", "other"));
    }

    #[test]
    fn set_and_get_font_size() {
        let mut theme = Theme::new("test");
        theme.set_font_size("Button", "font_size", 24);
        assert_eq!(theme.get_font_size("Button", "font_size"), Some(24));
        assert!(theme.has_font_size("Button", "font_size"));
    }

    #[test]
    fn set_and_get_constant() {
        let mut theme = Theme::new("test");
        theme.set_constant("Button", "h_separation", 8);
        assert_eq!(theme.get_constant("Button", "h_separation"), Some(8));
        assert!(theme.has_constant("Button", "h_separation"));
    }

    #[test]
    fn set_and_get_generic_item() {
        let mut theme = Theme::new("test");
        let val = Variant::Color(Color::rgb(0.5, 0.5, 0.5));
        theme.set_item(ThemePropertyType::Stylebox, "Panel", "panel", val.clone());
        assert_eq!(
            theme.get_item(ThemePropertyType::Stylebox, "Panel", "panel"),
            Some(&val)
        );
        assert!(theme.has_item(ThemePropertyType::Stylebox, "Panel", "panel"));
        assert!(!theme.has_item(ThemePropertyType::Font, "Panel", "panel"));
    }

    #[test]
    fn remove_item() {
        let mut theme = Theme::new("test");
        theme.set_item(
            ThemePropertyType::Stylebox,
            "Button",
            "normal",
            Variant::Int(42),
        );
        let removed = theme.remove_item(ThemePropertyType::Stylebox, "Button", "normal");
        assert_eq!(removed, Some(Variant::Int(42)));
        assert!(!theme.has_item(ThemePropertyType::Stylebox, "Button", "normal"));
    }

    #[test]
    fn default_theme_has_label_defaults() {
        let theme = Theme::default_theme();
        assert_eq!(theme.get_color("Label", "font_color"), Some(Color::WHITE));
        assert_eq!(theme.get_font_size("Label", "font_size"), Some(16));
        assert!(theme.has_constant("Label", "line_spacing"));
    }

    #[test]
    fn default_theme_has_button_defaults() {
        let theme = Theme::default_theme();
        assert!(theme.has_color("Button", "font_color"));
        assert!(theme.has_color("Button", "font_hover_color"));
        assert!(theme.has_color("Button", "font_pressed_color"));
        assert!(theme.has_color("Button", "font_disabled_color"));
        assert_eq!(theme.get_font_size("Button", "font_size"), Some(16));
        assert!(theme.has_item(ThemePropertyType::Stylebox, "Button", "normal"));
        assert!(theme.has_item(ThemePropertyType::Stylebox, "Button", "hover"));
        assert!(theme.has_item(ThemePropertyType::Stylebox, "Button", "pressed"));
    }

    #[test]
    fn default_theme_has_panel_defaults() {
        let theme = Theme::default_theme();
        assert!(theme.has_item(ThemePropertyType::Stylebox, "Panel", "panel"));
        assert_eq!(theme.get_constant("Panel", "content_margin_left"), Some(4));
    }

    #[test]
    fn theme_db_wraps_default() {
        let db = ThemeDB::new();
        assert_eq!(
            db.default_theme().get_color("Label", "font_color"),
            Some(Color::WHITE)
        );
    }

    #[test]
    fn theme_db_fallback_queries() {
        let db = ThemeDB::new();
        assert_eq!(
            db.get_color_or("Label", "font_color", Color::BLACK),
            Color::WHITE
        );
        assert_eq!(
            db.get_color_or("Missing", "missing", Color::BLACK),
            Color::BLACK
        );
        assert_eq!(db.get_font_size_or("Label", "font_size", 12), 16);
        assert_eq!(db.get_font_size_or("Missing", "missing", 12), 12);
        assert_eq!(db.get_constant_or("Label", "line_spacing", 0), 3);
        assert_eq!(db.get_constant_or("Missing", "missing", 99), 99);
    }

    #[test]
    fn theme_db_replace_default() {
        let mut db = ThemeDB::new();
        let mut custom = Theme::new("custom");
        custom.set_color("Label", "font_color", Color::rgb(1.0, 0.0, 0.0));
        db.set_default_theme(custom);
        assert_eq!(
            db.default_theme().get_color("Label", "font_color"),
            Some(Color::rgb(1.0, 0.0, 0.0))
        );
    }

    #[test]
    fn theme_db_mutate_default() {
        let mut db = ThemeDB::new();
        db.default_theme_mut()
            .set_color("Custom", "accent", Color::rgb(0.0, 1.0, 0.0));
        assert_eq!(
            db.default_theme().get_color("Custom", "accent"),
            Some(Color::rgb(0.0, 1.0, 0.0))
        );
    }

    #[test]
    fn different_control_types_are_independent() {
        let mut theme = Theme::new("test");
        theme.set_color("Label", "font_color", Color::WHITE);
        theme.set_color("Button", "font_color", Color::rgb(0.5, 0.5, 0.5));
        assert_ne!(
            theme.get_color("Label", "font_color"),
            theme.get_color("Button", "font_color")
        );
    }

    #[test]
    fn theme_default_trait() {
        let theme = Theme::default();
        assert_eq!(theme.name, "default");
        assert!(theme.has_color("Label", "font_color"));
    }
}
