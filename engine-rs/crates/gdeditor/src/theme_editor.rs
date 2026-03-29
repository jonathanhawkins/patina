//! Theme editor with live preview of control styling.
//!
//! Provides a [`ThemeEditor`] that mirrors Godot's theme editor panel.
//! Users can define theme overrides (colors, constants, fonts, StyleBoxes)
//! for each control type and see a live preview of how controls would look.

use std::collections::HashMap;

use gdcore::math::Color;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Serde helper for gdcore::Color (not Serialize/Deserialize itself)
// ---------------------------------------------------------------------------

mod serde_color {
    use gdcore::math::Color;
    use serde::{self, Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct ColorRepr(f32, f32, f32, f32);

    pub fn serialize<S: Serializer>(color: &Color, s: S) -> Result<S::Ok, S::Error> {
        ColorRepr(color.r, color.g, color.b, color.a).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        let ColorRepr(r, g, b, a) = ColorRepr::deserialize(d)?;
        Ok(Color::new(r, g, b, a))
    }
}

mod serde_color_opt {
    use gdcore::math::Color;
    use serde::{self, Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct ColorRepr(f32, f32, f32, f32);

    pub fn serialize<S: Serializer>(color: &Option<Color>, s: S) -> Result<S::Ok, S::Error> {
        match color {
            Some(c) => s.serialize_some(&ColorRepr(c.r, c.g, c.b, c.a)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color>, D::Error> {
        let opt: Option<ColorRepr> = Option::deserialize(d)?;
        Ok(opt.map(|ColorRepr(r, g, b, a)| Color::new(r, g, b, a)))
    }
}

// ---------------------------------------------------------------------------
// Theme data model
// ---------------------------------------------------------------------------

/// A named font specification within a theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeFont {
    /// Font family name (e.g. "Noto Sans").
    pub family: String,
    /// Point size.
    pub size: u32,
    /// Whether the font is bold.
    pub bold: bool,
    /// Whether the font is italic.
    pub italic: bool,
}

impl Default for ThemeFont {
    fn default() -> Self {
        Self {
            family: "Noto Sans".into(),
            size: 14,
            bold: false,
            italic: false,
        }
    }
}

/// A StyleBox describes the visual frame/background of a control.
///
/// Mirrors Godot's `StyleBoxFlat` — the most common StyleBox type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleBoxFlat {
    /// Background color.
    #[serde(with = "serde_color")]
    pub bg_color: Color,
    /// Border color.
    #[serde(with = "serde_color")]
    pub border_color: Color,
    /// Border widths: left, top, right, bottom.
    pub border_width: [u32; 4],
    /// Corner radii: top-left, top-right, bottom-right, bottom-left.
    pub corner_radius: [u32; 4],
    /// Content margin: left, top, right, bottom.
    pub content_margin: [f32; 4],
    /// Whether anti-aliasing is enabled for the border.
    pub anti_aliased: bool,
}

impl Default for StyleBoxFlat {
    fn default() -> Self {
        Self {
            bg_color: Color::new(0.2, 0.2, 0.2, 1.0),
            border_color: Color::new(0.4, 0.4, 0.4, 1.0),
            border_width: [1, 1, 1, 1],
            corner_radius: [3, 3, 3, 3],
            content_margin: [4.0, 4.0, 4.0, 4.0],
            anti_aliased: true,
        }
    }
}

/// A serializable color wrapper (since `gdcore::Color` doesn't derive Serialize).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SerdeColor(pub f32, pub f32, pub f32, pub f32);

impl From<Color> for SerdeColor {
    fn from(c: Color) -> Self {
        Self(c.r, c.g, c.b, c.a)
    }
}

impl From<SerdeColor> for Color {
    fn from(c: SerdeColor) -> Self {
        Color::new(c.0, c.1, c.2, c.3)
    }
}

/// The types of overrides that can be applied in a theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ThemeItem {
    /// A color override.
    Color(SerdeColor),
    /// An integer constant (e.g. margin, separation).
    Constant(i32),
    /// A font override.
    Font(ThemeFont),
    /// A StyleBox override.
    StyleBox(StyleBoxFlat),
    /// A font-size override (separate from the font resource).
    FontSize(u32),
    /// An icon color tint.
    IconColor(SerdeColor),
}

impl ThemeItem {
    /// Creates a color override from a `Color`.
    pub fn color(c: Color) -> Self {
        Self::Color(SerdeColor::from(c))
    }

    /// Creates an icon color override from a `Color`.
    pub fn icon_color(c: Color) -> Self {
        Self::IconColor(SerdeColor::from(c))
    }
}

/// Overrides for a single control type (e.g. "Button", "Label").
///
/// Maps item name → [`ThemeItem`].
pub type ControlOverrides = HashMap<String, ThemeItem>;

/// A full Godot-compatible theme resource.
///
/// Organized as `control_type → (item_name → ThemeItem)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeResource {
    /// Default base font for the theme.
    pub default_font: Option<ThemeFont>,
    /// Default base font size.
    pub default_font_size: Option<u32>,
    /// Per-control-type overrides.
    pub overrides: HashMap<String, ControlOverrides>,
}

impl ThemeResource {
    /// Creates an empty theme resource.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an override for a given control type and item name.
    pub fn set_item(&mut self, control_type: &str, item_name: &str, item: ThemeItem) {
        self.overrides
            .entry(control_type.to_string())
            .or_default()
            .insert(item_name.to_string(), item);
    }

    /// Gets an override for a given control type and item name.
    pub fn get_item(&self, control_type: &str, item_name: &str) -> Option<&ThemeItem> {
        self.overrides.get(control_type)?.get(item_name)
    }

    /// Removes an override. Returns the removed item if it existed.
    pub fn remove_item(&mut self, control_type: &str, item_name: &str) -> Option<ThemeItem> {
        let control = self.overrides.get_mut(control_type)?;
        let removed = control.remove(item_name);
        if control.is_empty() {
            self.overrides.remove(control_type);
        }
        removed
    }

    /// Checks whether a specific override exists.
    pub fn has_item(&self, control_type: &str, item_name: &str) -> bool {
        self.overrides
            .get(control_type)
            .map_or(false, |c| c.contains_key(item_name))
    }

    /// Lists all control types that have overrides.
    pub fn control_types(&self) -> Vec<&str> {
        let mut types: Vec<&str> = self.overrides.keys().map(|s| s.as_str()).collect();
        types.sort();
        types
    }

    /// Lists all item names for a given control type.
    pub fn item_names(&self, control_type: &str) -> Vec<&str> {
        match self.overrides.get(control_type) {
            Some(items) => {
                let mut names: Vec<&str> = items.keys().map(|s| s.as_str()).collect();
                names.sort();
                names
            }
            None => Vec::new(),
        }
    }

    /// Returns the total number of overrides across all control types.
    pub fn override_count(&self) -> usize {
        self.overrides.values().map(|items| items.len()).sum()
    }

    /// Resolves a color for a control, falling back to a default.
    pub fn resolve_color(&self, control_type: &str, item_name: &str, default: Color) -> Color {
        match self.get_item(control_type, item_name) {
            Some(ThemeItem::Color(c)) => Color::from(*c),
            _ => default,
        }
    }

    /// Resolves a constant for a control, falling back to a default.
    pub fn resolve_constant(&self, control_type: &str, item_name: &str, default: i32) -> i32 {
        match self.get_item(control_type, item_name) {
            Some(ThemeItem::Constant(c)) => *c,
            _ => default,
        }
    }

    /// Resolves a StyleBox for a control, falling back to a default.
    pub fn resolve_stylebox(&self, control_type: &str, item_name: &str) -> Option<&StyleBoxFlat> {
        match self.get_item(control_type, item_name) {
            Some(ThemeItem::StyleBox(sb)) => Some(sb),
            _ => None,
        }
    }

    /// Resolves a font for a control, falling back to the theme default.
    pub fn resolve_font(&self, control_type: &str, item_name: &str) -> Option<&ThemeFont> {
        match self.get_item(control_type, item_name) {
            Some(ThemeItem::Font(f)) => Some(f),
            _ => self.default_font.as_ref(),
        }
    }

    /// Resolves a font size for a control, falling back to the theme default.
    pub fn resolve_font_size(&self, control_type: &str, item_name: &str) -> Option<u32> {
        match self.get_item(control_type, item_name) {
            Some(ThemeItem::FontSize(s)) => Some(*s),
            _ => self.default_font_size,
        }
    }
}

// ---------------------------------------------------------------------------
// Preview model
// ---------------------------------------------------------------------------

/// A preview control rendered with the current theme.
///
/// This is a lightweight description — actual rendering happens in the
/// editor's UI layer (HTML/canvas). The preview model supplies resolved
/// style values so the renderer can draw the control.
#[derive(Debug, Clone)]
pub struct PreviewControl {
    /// The control type (e.g. "Button").
    pub control_type: String,
    /// Display label shown inside or next to the control.
    pub label: String,
    /// Resolved colors for this control.
    pub colors: HashMap<String, Color>,
    /// Resolved constants.
    pub constants: HashMap<String, i32>,
    /// Resolved StyleBoxes.
    pub styleboxes: HashMap<String, StyleBoxFlat>,
    /// Resolved font (if any override).
    pub font: Option<ThemeFont>,
    /// Resolved font size.
    pub font_size: Option<u32>,
}

/// Built-in preview controls and their standard theme items.
const PREVIEW_CONTROLS: &[(&str, &str, &[(&str, PreviewItemKind)])] = &[
    (
        "Button",
        "Button",
        &[
            ("font_color", PreviewItemKind::Color),
            ("font_color_hover", PreviewItemKind::Color),
            ("font_color_pressed", PreviewItemKind::Color),
            ("normal", PreviewItemKind::StyleBox),
            ("hover", PreviewItemKind::StyleBox),
            ("pressed", PreviewItemKind::StyleBox),
            ("disabled", PreviewItemKind::StyleBox),
        ],
    ),
    (
        "Label",
        "Label",
        &[
            ("font_color", PreviewItemKind::Color),
            ("font_shadow_color", PreviewItemKind::Color),
            ("shadow_offset_x", PreviewItemKind::Constant),
            ("shadow_offset_y", PreviewItemKind::Constant),
        ],
    ),
    (
        "LineEdit",
        "Line Edit",
        &[
            ("font_color", PreviewItemKind::Color),
            ("caret_color", PreviewItemKind::Color),
            ("selection_color", PreviewItemKind::Color),
            ("normal", PreviewItemKind::StyleBox),
            ("focus", PreviewItemKind::StyleBox),
        ],
    ),
    ("Panel", "Panel", &[("panel", PreviewItemKind::StyleBox)]),
    (
        "CheckBox",
        "CheckBox",
        &[
            ("font_color", PreviewItemKind::Color),
            ("normal", PreviewItemKind::StyleBox),
            ("hover", PreviewItemKind::StyleBox),
        ],
    ),
    (
        "ProgressBar",
        "ProgressBar",
        &[
            ("font_color", PreviewItemKind::Color),
            ("background", PreviewItemKind::StyleBox),
            ("fill", PreviewItemKind::StyleBox),
        ],
    ),
];

/// The kind of a preview theme item — used in the preview definition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewItemKind {
    Color,
    Constant,
    StyleBox,
}

// ---------------------------------------------------------------------------
// Theme Editor
// ---------------------------------------------------------------------------

/// The theme editor panel.
///
/// Holds the theme being edited, the currently selected control type,
/// and generates preview data for the UI layer to render live.
#[derive(Debug)]
pub struct ThemeEditor {
    /// The theme resource being edited.
    theme: ThemeResource,
    /// The currently selected control type in the editor UI.
    selected_control: Option<String>,
    /// Whether the live preview pane is visible.
    preview_visible: bool,
    /// Change counter — incremented on each mutation so the UI layer
    /// can detect when it needs to re-render.
    revision: u64,
}

impl ThemeEditor {
    /// Creates a new theme editor with an empty theme.
    pub fn new() -> Self {
        Self {
            theme: ThemeResource::new(),
            selected_control: None,
            preview_visible: true,
            revision: 0,
        }
    }

    /// Creates a theme editor initialized with an existing theme resource.
    pub fn with_theme(theme: ThemeResource) -> Self {
        Self {
            theme,
            selected_control: None,
            preview_visible: true,
            revision: 0,
        }
    }

    /// Returns a reference to the underlying theme resource.
    pub fn theme(&self) -> &ThemeResource {
        &self.theme
    }

    /// Returns a mutable reference to the underlying theme resource.
    ///
    /// Callers should call [`bump_revision`] after mutations.
    pub fn theme_mut(&mut self) -> &mut ThemeResource {
        &mut self.theme
    }

    /// Returns the current revision counter.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Bumps the revision counter (signals the UI to re-render).
    pub fn bump_revision(&mut self) {
        self.revision += 1;
    }

    /// Selects a control type for editing.
    pub fn select_control(&mut self, control_type: &str) {
        self.selected_control = Some(control_type.to_string());
        tracing::debug!("Theme editor: selected control type '{}'", control_type);
    }

    /// Clears the control type selection.
    pub fn deselect_control(&mut self) {
        self.selected_control = None;
    }

    /// Returns the currently selected control type.
    pub fn selected_control(&self) -> Option<&str> {
        self.selected_control.as_deref()
    }

    /// Toggles the live preview pane visibility.
    pub fn toggle_preview(&mut self) {
        self.preview_visible = !self.preview_visible;
    }

    /// Returns whether the live preview is visible.
    pub fn is_preview_visible(&self) -> bool {
        self.preview_visible
    }

    /// Sets a theme item and bumps the revision.
    pub fn set_item(&mut self, control_type: &str, item_name: &str, item: ThemeItem) {
        self.theme.set_item(control_type, item_name, item);
        self.revision += 1;
    }

    /// Removes a theme item and bumps the revision.
    pub fn remove_item(&mut self, control_type: &str, item_name: &str) -> Option<ThemeItem> {
        let removed = self.theme.remove_item(control_type, item_name);
        if removed.is_some() {
            self.revision += 1;
        }
        removed
    }

    /// Generates preview data for all built-in preview controls.
    ///
    /// The UI layer uses this to render a live preview of how controls
    /// look with the current theme applied.
    pub fn generate_preview(&self) -> Vec<PreviewControl> {
        PREVIEW_CONTROLS
            .iter()
            .map(|(control_type, label, items)| {
                let mut colors = HashMap::new();
                let mut constants = HashMap::new();
                let mut styleboxes = HashMap::new();

                for (item_name, kind) in *items {
                    match kind {
                        PreviewItemKind::Color => {
                            let c = self
                                .theme
                                .resolve_color(control_type, item_name, Color::WHITE);
                            colors.insert((*item_name).to_string(), c);
                        }
                        PreviewItemKind::Constant => {
                            let c = self.theme.resolve_constant(control_type, item_name, 0);
                            constants.insert((*item_name).to_string(), c);
                        }
                        PreviewItemKind::StyleBox => {
                            let sb = self
                                .theme
                                .resolve_stylebox(control_type, item_name)
                                .cloned()
                                .unwrap_or_default();
                            styleboxes.insert((*item_name).to_string(), sb);
                        }
                    }
                }

                let font = self.theme.resolve_font(control_type, "font").cloned();
                let font_size = self.theme.resolve_font_size(control_type, "font_size");

                PreviewControl {
                    control_type: (*control_type).to_string(),
                    label: (*label).to_string(),
                    colors,
                    constants,
                    styleboxes,
                    font,
                    font_size,
                }
            })
            .collect()
    }

    /// Lists all control types that the editor knows about (both built-in
    /// preview controls and any custom ones the user has added overrides for).
    pub fn all_control_types(&self) -> Vec<String> {
        let mut types: Vec<String> = PREVIEW_CONTROLS
            .iter()
            .map(|(ct, _, _)| (*ct).to_string())
            .collect();

        // Add any custom types the user has defined overrides for.
        for ct in self.theme.control_types() {
            if !types.iter().any(|t| t == ct) {
                types.push(ct.to_string());
            }
        }
        types.sort();
        types
    }

    /// Returns the standard theme items for a given built-in control type.
    pub fn standard_items_for(control_type: &str) -> Vec<(&'static str, &'static str)> {
        for (ct, _, items) in PREVIEW_CONTROLS {
            if *ct == control_type {
                return items
                    .iter()
                    .map(|(name, kind)| {
                        let kind_str = match kind {
                            PreviewItemKind::Color => "Color",
                            PreviewItemKind::Constant => "Constant",
                            PreviewItemKind::StyleBox => "StyleBox",
                        };
                        (*name, kind_str)
                    })
                    .collect();
            }
        }
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Typed override editing methods
// ---------------------------------------------------------------------------

impl ThemeEditor {
    /// Sets a color override for a control type.
    pub fn set_color(&mut self, control_type: &str, item_name: &str, color: Color) {
        self.set_item(control_type, item_name, ThemeItem::color(color));
    }

    /// Gets a color override for a control type, if one exists.
    pub fn get_color(&self, control_type: &str, item_name: &str) -> Option<Color> {
        match self.theme.get_item(control_type, item_name) {
            Some(ThemeItem::Color(c)) => Some(Color::from(*c)),
            _ => None,
        }
    }

    /// Sets a font override for a control type.
    pub fn set_font(&mut self, control_type: &str, item_name: &str, font: ThemeFont) {
        self.set_item(control_type, item_name, ThemeItem::Font(font));
    }

    /// Gets a font override for a control type, if one exists.
    pub fn get_font(&self, control_type: &str, item_name: &str) -> Option<&ThemeFont> {
        match self.theme.get_item(control_type, item_name) {
            Some(ThemeItem::Font(f)) => Some(f),
            _ => None,
        }
    }

    /// Sets a StyleBox override for a control type.
    pub fn set_stylebox(&mut self, control_type: &str, item_name: &str, stylebox: StyleBoxFlat) {
        self.set_item(control_type, item_name, ThemeItem::StyleBox(stylebox));
    }

    /// Gets a StyleBox override for a control type, if one exists.
    pub fn get_stylebox(&self, control_type: &str, item_name: &str) -> Option<&StyleBoxFlat> {
        match self.theme.get_item(control_type, item_name) {
            Some(ThemeItem::StyleBox(sb)) => Some(sb),
            _ => None,
        }
    }

    /// Sets a constant override for a control type.
    pub fn set_constant(&mut self, control_type: &str, item_name: &str, value: i32) {
        self.set_item(control_type, item_name, ThemeItem::Constant(value));
    }

    /// Gets a constant override, if one exists.
    pub fn get_constant(&self, control_type: &str, item_name: &str) -> Option<i32> {
        match self.theme.get_item(control_type, item_name) {
            Some(ThemeItem::Constant(c)) => Some(*c),
            _ => None,
        }
    }

    /// Sets the default font for the entire theme.
    pub fn set_default_font(&mut self, font: ThemeFont) {
        self.theme.default_font = Some(font);
        self.revision += 1;
    }

    /// Sets the default font size for the entire theme.
    pub fn set_default_font_size(&mut self, size: u32) {
        self.theme.default_font_size = Some(size);
        self.revision += 1;
    }

    /// Sets a font size override for a control type.
    pub fn set_font_size(&mut self, control_type: &str, item_name: &str, size: u32) {
        self.set_item(control_type, item_name, ThemeItem::FontSize(size));
    }

    /// Gets a font size override, if one exists.
    pub fn get_font_size(&self, control_type: &str, item_name: &str) -> Option<u32> {
        match self.theme.get_item(control_type, item_name) {
            Some(ThemeItem::FontSize(s)) => Some(*s),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Batch operations and presets
// ---------------------------------------------------------------------------

/// A color palette that can be applied across all controls.
#[derive(Debug, Clone)]
pub struct ThemeColorPalette {
    /// Primary font color.
    pub font_color: Color,
    /// Background color for panels/frames.
    pub bg_color: Color,
    /// Accent/highlight color (hover, focus).
    pub accent_color: Color,
    /// Disabled/muted color.
    pub disabled_color: Color,
    /// Border color.
    pub border_color: Color,
}

impl ThemeEditor {
    /// Applies a color palette across standard controls.
    ///
    /// Sets `font_color` on all text controls, background StyleBoxes on
    /// panels, and hover/focus colors where applicable.
    pub fn apply_palette(&mut self, palette: &ThemeColorPalette) {
        // Font colors for all text controls.
        for ct in &["Button", "Label", "LineEdit", "CheckBox", "ProgressBar"] {
            self.set_color(ct, "font_color", palette.font_color);
        }

        // Button states.
        self.set_color("Button", "font_color_hover", palette.accent_color);
        self.set_color("Button", "font_color_pressed", palette.accent_color);

        // LineEdit states.
        self.set_color("LineEdit", "caret_color", palette.accent_color);
        self.set_color("LineEdit", "selection_color", palette.accent_color);

        // Panel background.
        self.set_stylebox(
            "Panel",
            "panel",
            StyleBoxFlat {
                bg_color: palette.bg_color,
                border_color: palette.border_color,
                ..Default::default()
            },
        );

        // Button normal/hover/disabled StyleBoxes.
        self.set_stylebox(
            "Button",
            "normal",
            StyleBoxFlat {
                bg_color: palette.bg_color,
                border_color: palette.border_color,
                ..Default::default()
            },
        );
        self.set_stylebox(
            "Button",
            "hover",
            StyleBoxFlat {
                bg_color: palette.accent_color,
                border_color: palette.accent_color,
                ..Default::default()
            },
        );
        self.set_stylebox(
            "Button",
            "disabled",
            StyleBoxFlat {
                bg_color: palette.disabled_color,
                border_color: palette.disabled_color,
                ..Default::default()
            },
        );

        // ProgressBar fill.
        self.set_stylebox(
            "ProgressBar",
            "fill",
            StyleBoxFlat {
                bg_color: palette.accent_color,
                border_color: palette.accent_color,
                ..Default::default()
            },
        );
        self.set_stylebox(
            "ProgressBar",
            "background",
            StyleBoxFlat {
                bg_color: palette.bg_color,
                border_color: palette.border_color,
                ..Default::default()
            },
        );
    }

    /// Copies all overrides from one control type to another.
    ///
    /// Useful for creating a new control type based on an existing one.
    /// Returns the number of items copied.
    pub fn copy_overrides(&mut self, from_control: &str, to_control: &str) -> usize {
        let items: Vec<(String, ThemeItem)> = match self.theme.overrides.get(from_control) {
            Some(overrides) => overrides
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            None => return 0,
        };
        let count = items.len();
        for (name, item) in items {
            self.theme
                .overrides
                .entry(to_control.to_string())
                .or_default()
                .insert(name, item);
        }
        if count > 0 {
            self.revision += 1;
        }
        count
    }

    /// Removes all overrides for a control type.
    ///
    /// Returns the number of items removed.
    pub fn clear_control_overrides(&mut self, control_type: &str) -> usize {
        match self.theme.overrides.remove(control_type) {
            Some(items) => {
                let count = items.len();
                if count > 0 {
                    self.revision += 1;
                }
                count
            }
            None => 0,
        }
    }

    /// Resets the theme to empty.
    pub fn clear_all(&mut self) {
        self.theme = ThemeResource::new();
        self.revision += 1;
    }

    /// Returns a list of all overrides for a control type, with their names and kinds.
    pub fn list_overrides(&self, control_type: &str) -> Vec<OverrideEntry> {
        match self.theme.overrides.get(control_type) {
            Some(items) => {
                let mut entries: Vec<OverrideEntry> = items
                    .iter()
                    .map(|(name, item)| OverrideEntry {
                        name: name.clone(),
                        kind: match item {
                            ThemeItem::Color(_) => OverrideKind::Color,
                            ThemeItem::Constant(_) => OverrideKind::Constant,
                            ThemeItem::Font(_) => OverrideKind::Font,
                            ThemeItem::StyleBox(_) => OverrideKind::StyleBox,
                            ThemeItem::FontSize(_) => OverrideKind::FontSize,
                            ThemeItem::IconColor(_) => OverrideKind::IconColor,
                        },
                    })
                    .collect();
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                entries
            }
            None => Vec::new(),
        }
    }

    /// Returns the total number of overrides across all control types.
    pub fn total_override_count(&self) -> usize {
        self.theme.override_count()
    }
}

/// The kind of a theme override entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideKind {
    Color,
    Constant,
    Font,
    StyleBox,
    FontSize,
    IconColor,
}

/// An entry describing a single override in the editor.
#[derive(Debug, Clone)]
pub struct OverrideEntry {
    /// The item name.
    pub name: String,
    /// The kind of override.
    pub kind: OverrideKind,
}

// ---------------------------------------------------------------------------
// Theme presets
// ---------------------------------------------------------------------------

impl ThemeEditor {
    /// Loads the "Godot Dark" preset — a dark theme similar to Godot's default.
    pub fn load_preset_dark(&mut self) {
        self.clear_all();
        let palette = ThemeColorPalette {
            font_color: Color::new(0.875, 0.875, 0.875, 1.0),
            bg_color: Color::new(0.17, 0.17, 0.17, 1.0),
            accent_color: Color::new(0.35, 0.55, 0.83, 1.0),
            disabled_color: Color::new(0.3, 0.3, 0.3, 1.0),
            border_color: Color::new(0.35, 0.35, 0.35, 1.0),
        };
        self.apply_palette(&palette);
        self.set_default_font(ThemeFont {
            family: "Noto Sans".into(),
            size: 14,
            bold: false,
            italic: false,
        });
        self.set_default_font_size(14);
    }

    /// Loads the "Godot Light" preset — a light theme.
    pub fn load_preset_light(&mut self) {
        self.clear_all();
        let palette = ThemeColorPalette {
            font_color: Color::new(0.1, 0.1, 0.1, 1.0),
            bg_color: Color::new(0.93, 0.93, 0.93, 1.0),
            accent_color: Color::new(0.24, 0.47, 0.78, 1.0),
            disabled_color: Color::new(0.7, 0.7, 0.7, 1.0),
            border_color: Color::new(0.6, 0.6, 0.6, 1.0),
        };
        self.apply_palette(&palette);
        self.set_default_font(ThemeFont {
            family: "Noto Sans".into(),
            size: 14,
            bold: false,
            italic: false,
        });
        self.set_default_font_size(14);
    }

    /// Returns the names of available built-in presets.
    pub fn available_presets() -> &'static [&'static str] {
        &["Dark", "Light"]
    }

    /// Loads a preset by name. Returns `false` if the preset name is unknown.
    pub fn load_preset(&mut self, name: &str) -> bool {
        match name {
            "Dark" => {
                self.load_preset_dark();
                true
            }
            "Light" => {
                self.load_preset_light();
                true
            }
            _ => false,
        }
    }
}

impl Default for ThemeEditor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_theme_resource() {
        let theme = ThemeResource::new();
        assert_eq!(theme.override_count(), 0);
        assert!(theme.control_types().is_empty());
    }

    #[test]
    fn set_and_get_color_override() {
        let mut theme = ThemeResource::new();
        theme.set_item(
            "Button",
            "font_color",
            ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        assert!(theme.has_item("Button", "font_color"));
        assert_eq!(theme.override_count(), 1);
        match theme.get_item("Button", "font_color") {
            Some(ThemeItem::Color(c)) => {
                assert_eq!(Color::from(*c), Color::new(1.0, 0.0, 0.0, 1.0))
            }
            other => panic!("expected Color, got {:?}", other),
        }
    }

    #[test]
    fn set_and_get_constant_override() {
        let mut theme = ThemeResource::new();
        theme.set_item("Label", "shadow_offset_x", ThemeItem::Constant(2));
        assert_eq!(theme.resolve_constant("Label", "shadow_offset_x", 0), 2);
    }

    #[test]
    fn set_and_get_stylebox_override() {
        let mut theme = ThemeResource::new();
        let sb = StyleBoxFlat {
            bg_color: Color::new(0.0, 0.0, 1.0, 1.0),
            ..Default::default()
        };
        theme.set_item("Button", "normal", ThemeItem::StyleBox(sb.clone()));
        let resolved = theme.resolve_stylebox("Button", "normal").unwrap();
        assert_eq!(resolved.bg_color, Color::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn set_and_get_font_override() {
        let mut theme = ThemeResource::new();
        let font = ThemeFont {
            family: "Roboto".into(),
            size: 16,
            bold: true,
            italic: false,
        };
        theme.set_item("Button", "font", ThemeItem::Font(font.clone()));
        let resolved = theme.resolve_font("Button", "font").unwrap();
        assert_eq!(resolved.family, "Roboto");
        assert!(resolved.bold);
    }

    #[test]
    fn font_falls_back_to_default() {
        let mut theme = ThemeResource::new();
        theme.default_font = Some(ThemeFont {
            family: "FallbackFont".into(),
            ..Default::default()
        });
        // No override for Button font, should fall back to default.
        let resolved = theme.resolve_font("Button", "font").unwrap();
        assert_eq!(resolved.family, "FallbackFont");
    }

    #[test]
    fn font_size_falls_back_to_default() {
        let mut theme = ThemeResource::new();
        theme.default_font_size = Some(18);
        assert_eq!(theme.resolve_font_size("Button", "font_size"), Some(18));
    }

    #[test]
    fn remove_item() {
        let mut theme = ThemeResource::new();
        theme.set_item(
            "Button",
            "font_color",
            ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        assert!(theme.has_item("Button", "font_color"));
        let removed = theme.remove_item("Button", "font_color");
        assert!(removed.is_some());
        assert!(!theme.has_item("Button", "font_color"));
        // Control type should be removed too since it has no more items.
        assert!(theme.control_types().is_empty());
    }

    #[test]
    fn remove_nonexistent_item_returns_none() {
        let mut theme = ThemeResource::new();
        assert!(theme.remove_item("Button", "nonexistent").is_none());
    }

    #[test]
    fn control_types_sorted() {
        let mut theme = ThemeResource::new();
        theme.set_item("Panel", "panel", ThemeItem::StyleBox(Default::default()));
        theme.set_item("Button", "normal", ThemeItem::StyleBox(Default::default()));
        theme.set_item("Label", "font_color", ThemeItem::color(Color::WHITE));
        let types = theme.control_types();
        assert_eq!(types, vec!["Button", "Label", "Panel"]);
    }

    #[test]
    fn item_names_sorted() {
        let mut theme = ThemeResource::new();
        theme.set_item("Button", "normal", ThemeItem::StyleBox(Default::default()));
        theme.set_item("Button", "font_color", ThemeItem::color(Color::WHITE));
        theme.set_item("Button", "hover", ThemeItem::StyleBox(Default::default()));
        let names = theme.item_names("Button");
        assert_eq!(names, vec!["font_color", "hover", "normal"]);
    }

    #[test]
    fn resolve_color_fallback() {
        let theme = ThemeResource::new();
        let c = theme.resolve_color("Button", "font_color", Color::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(c, Color::new(0.0, 1.0, 0.0, 1.0));
    }

    #[test]
    fn resolve_constant_fallback() {
        let theme = ThemeResource::new();
        assert_eq!(theme.resolve_constant("Label", "shadow_offset_x", 5), 5);
    }

    #[test]
    fn theme_editor_new() {
        let editor = ThemeEditor::new();
        assert_eq!(editor.revision(), 0);
        assert!(editor.selected_control().is_none());
        assert!(editor.is_preview_visible());
    }

    #[test]
    fn theme_editor_select_control() {
        let mut editor = ThemeEditor::new();
        editor.select_control("Button");
        assert_eq!(editor.selected_control(), Some("Button"));
        editor.deselect_control();
        assert!(editor.selected_control().is_none());
    }

    #[test]
    fn theme_editor_set_item_bumps_revision() {
        let mut editor = ThemeEditor::new();
        assert_eq!(editor.revision(), 0);
        editor.set_item(
            "Button",
            "font_color",
            ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        assert_eq!(editor.revision(), 1);
        editor.set_item(
            "Label",
            "font_color",
            ThemeItem::color(Color::new(0.0, 0.0, 1.0, 1.0)),
        );
        assert_eq!(editor.revision(), 2);
    }

    #[test]
    fn theme_editor_remove_item_bumps_revision() {
        let mut editor = ThemeEditor::new();
        editor.set_item(
            "Button",
            "font_color",
            ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        let rev = editor.revision();
        editor.remove_item("Button", "font_color");
        assert_eq!(editor.revision(), rev + 1);
    }

    #[test]
    fn theme_editor_remove_nonexistent_no_bump() {
        let mut editor = ThemeEditor::new();
        let rev = editor.revision();
        editor.remove_item("Button", "nonexistent");
        assert_eq!(editor.revision(), rev);
    }

    #[test]
    fn theme_editor_toggle_preview() {
        let mut editor = ThemeEditor::new();
        assert!(editor.is_preview_visible());
        editor.toggle_preview();
        assert!(!editor.is_preview_visible());
        editor.toggle_preview();
        assert!(editor.is_preview_visible());
    }

    #[test]
    fn theme_editor_generate_preview() {
        let mut editor = ThemeEditor::new();
        editor.set_item(
            "Button",
            "font_color",
            ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        let preview = editor.generate_preview();
        assert!(!preview.is_empty());
        let button_preview = preview.iter().find(|p| p.control_type == "Button").unwrap();
        assert_eq!(button_preview.label, "Button");
        assert_eq!(
            *button_preview.colors.get("font_color").unwrap(),
            Color::new(1.0, 0.0, 0.0, 1.0)
        );
    }

    #[test]
    fn theme_editor_preview_defaults_when_no_overrides() {
        let editor = ThemeEditor::new();
        let preview = editor.generate_preview();
        let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
        // With no overrides, colors resolve to WHITE (the default).
        assert_eq!(*button.colors.get("font_color").unwrap(), Color::WHITE);
    }

    #[test]
    fn theme_editor_all_control_types_includes_custom() {
        let mut editor = ThemeEditor::new();
        editor.set_item("MyCustomWidget", "bg", ThemeItem::color(Color::BLACK));
        let types = editor.all_control_types();
        assert!(types.contains(&"Button".to_string()));
        assert!(types.contains(&"MyCustomWidget".to_string()));
    }

    #[test]
    fn standard_items_for_button() {
        let items = ThemeEditor::standard_items_for("Button");
        assert!(!items.is_empty());
        assert!(items
            .iter()
            .any(|(name, kind)| *name == "font_color" && *kind == "Color"));
        assert!(items
            .iter()
            .any(|(name, kind)| *name == "normal" && *kind == "StyleBox"));
    }

    #[test]
    fn standard_items_for_unknown_control() {
        let items = ThemeEditor::standard_items_for("UnknownControl");
        assert!(items.is_empty());
    }

    #[test]
    fn theme_editor_with_existing_theme() {
        let mut theme = ThemeResource::new();
        theme.set_item("Panel", "panel", ThemeItem::StyleBox(Default::default()));
        let editor = ThemeEditor::with_theme(theme);
        assert!(editor.theme().has_item("Panel", "panel"));
    }

    #[test]
    fn stylebox_default_values() {
        let sb = StyleBoxFlat::default();
        assert_eq!(sb.border_width, [1, 1, 1, 1]);
        assert_eq!(sb.corner_radius, [3, 3, 3, 3]);
        assert!(sb.anti_aliased);
    }

    #[test]
    fn theme_resource_serialization_roundtrip() {
        let mut theme = ThemeResource::new();
        theme.default_font = Some(ThemeFont::default());
        theme.default_font_size = Some(16);
        theme.set_item(
            "Button",
            "font_color",
            ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        theme.set_item("Button", "normal", ThemeItem::StyleBox(Default::default()));
        theme.set_item("Label", "shadow_offset_x", ThemeItem::Constant(2));

        let json = serde_json::to_string(&theme).expect("serialize");
        let deserialized: ThemeResource = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.override_count(), theme.override_count());
        assert!(deserialized.has_item("Button", "font_color"));
        assert!(deserialized.has_item("Button", "normal"));
        assert!(deserialized.has_item("Label", "shadow_offset_x"));
        assert_eq!(deserialized.default_font_size, Some(16));
    }
}
