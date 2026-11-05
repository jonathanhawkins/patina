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

use crate::stylebox::{StyleBox, StyleBoxFlat, StyleBoxLine};

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
    /// Icon (texture) resources — Godot's sixth theme data type
    /// (`Theme.DATA_TYPE_ICON`), pat-0q2kx.180.
    ///
    /// Unlike the five above, icons have no dedicated map: they live in the
    /// generic `items` store, which is keyed by category and already handles
    /// any type. A separate map would buy nothing here and would need every
    /// construction site updated.
    Icon,
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
    /// Generic properties stored as Variant (fonts, legacy styleboxes, etc.).
    items: HashMap<(ThemePropertyType, ThemeKey), Variant>,
    /// Typed StyleBox properties (pat-u3l8c.70).
    styleboxes: HashMap<ThemeKey, StyleBox>,
    /// Icon items — the texture path per type/name (pat-0q2kx.150).
    icons: HashMap<ThemeKey, String>,
    /// Font items — the font resource path per type/name.
    fonts: HashMap<ThemeKey, String>,
    /// Type variations: variation name -> base type.
    ///
    /// Godot lets a theme define e.g. `FlatButton` deriving from `Button`, and
    /// every lookup that misses on the variation falls through to its base
    /// (`theme.cpp` `get_type_variation_base`). Without this table a variation
    /// resolved to nothing at all.
    type_variations: HashMap<String, String>,
    /// `Theme.default_font` — the font used when a Control names none.
    pub default_font: Option<String>,
    /// `Theme.default_font_size`.
    pub default_font_size: Option<i64>,
    /// `Theme.default_base_scale`.
    pub default_base_scale: Option<f32>,
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
            styleboxes: HashMap::new(),
            icons: HashMap::new(),
            fonts: HashMap::new(),
            type_variations: HashMap::new(),
            default_font: None,
            default_font_size: None,
            default_base_scale: None,
        }
    }

    // -- Type variations (pat-0q2kx.150) -------------------------------------

    /// Declares `variation` as deriving from `base_type`
    /// (`Theme.set_type_variation`).
    pub fn set_type_variation(&mut self, variation: &str, base_type: &str) {
        self.type_variations
            .insert(variation.to_string(), base_type.to_string());
    }

    /// The base type a variation derives from, if it is one.
    pub fn get_type_variation_base(&self, variation: &str) -> Option<&str> {
        self.type_variations.get(variation).map(String::as_str)
    }

    /// Every declared variation name.
    pub fn get_type_variation_list(&self) -> Vec<String> {
        let mut out: Vec<String> = self.type_variations.keys().cloned().collect();
        out.sort();
        out
    }

    /// The lookup chain for `control_type`: itself, then each variation base in
    /// turn.
    ///
    /// Cycle-guarded — a theme that declares `A` based on `B` based on `A` is
    /// malformed, and Godot tolerates rather than hangs on it.
    fn type_chain(&self, control_type: &str) -> Vec<String> {
        let mut chain = vec![control_type.to_string()];
        let mut current = control_type.to_string();
        while let Some(base) = self.type_variations.get(&current) {
            if chain.contains(base) {
                break;
            }
            chain.push(base.clone());
            current = base.clone();
        }
        chain
    }

    // -- Icons ---------------------------------------------------------------

    /// Sets an icon item's texture path (`Theme.set_icon`).
    pub fn set_icon(&mut self, control_type: &str, property: &str, path: &str) {
        self.icons.insert(
            (control_type.to_string(), property.to_string()),
            path.to_string(),
        );
    }

    /// An icon item's texture path, resolving type variations.
    pub fn get_icon(&self, control_type: &str, property: &str) -> Option<&str> {
        self.type_chain(control_type).into_iter().find_map(|t| {
            self.icons
                .get(&(t, property.to_string()))
                .map(String::as_str)
        })
    }

    /// Whether an icon item exists (variation-aware).
    pub fn has_icon(&self, control_type: &str, property: &str) -> bool {
        self.get_icon(control_type, property).is_some()
    }

    // -- Fonts ---------------------------------------------------------------

    /// Sets a font item's resource path (`Theme.set_font`).
    pub fn set_font(&mut self, control_type: &str, property: &str, path: &str) {
        self.fonts.insert(
            (control_type.to_string(), property.to_string()),
            path.to_string(),
        );
    }

    /// A font item's resource path, resolving type variations.
    pub fn get_font(&self, control_type: &str, property: &str) -> Option<&str> {
        self.type_chain(control_type).into_iter().find_map(|t| {
            self.fonts
                .get(&(t, property.to_string()))
                .map(String::as_str)
        })
    }

    /// Whether a font item exists (variation-aware).
    pub fn has_font(&self, control_type: &str, property: &str) -> bool {
        self.get_font(control_type, property).is_some()
    }

    // -- Whole-theme operations ----------------------------------------------

    /// Copies every item from `other` into this theme, overwriting collisions
    /// (`Theme.merge_with`).
    pub fn merge_with(&mut self, other: &Theme) {
        self.colors.extend(other.colors.clone());
        self.font_sizes.extend(other.font_sizes.clone());
        self.constants.extend(other.constants.clone());
        self.items.extend(other.items.clone());
        self.styleboxes.extend(other.styleboxes.clone());
        self.icons.extend(other.icons.clone());
        self.fonts.extend(other.fonts.clone());
        self.type_variations.extend(other.type_variations.clone());
        // Godot's merge only adopts a default the receiver has not set.
        self.default_font = self.default_font.clone().or(other.default_font.clone());
        self.default_font_size = self.default_font_size.or(other.default_font_size);
        self.default_base_scale = self.default_base_scale.or(other.default_base_scale);
    }

    /// Drops every item and variation (`Theme.clear`).
    pub fn clear(&mut self) {
        self.colors.clear();
        self.font_sizes.clear();
        self.constants.clear();
        self.items.clear();
        self.styleboxes.clear();
        self.icons.clear();
        self.fonts.clear();
        self.type_variations.clear();
        self.default_font = None;
        self.default_font_size = None;
        self.default_base_scale = None;
    }

    /// Every control type this theme defines an item for, sorted
    /// (`Theme.get_type_list`).
    pub fn get_type_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |t: &String| {
            if !out.contains(t) {
                out.push(t.clone());
            }
        };
        for (t, _) in self.colors.keys() {
            push(t);
        }
        for (t, _) in self.font_sizes.keys() {
            push(t);
        }
        for (t, _) in self.constants.keys() {
            push(t);
        }
        for (t, _) in self.styleboxes.keys() {
            push(t);
        }
        for (t, _) in self.icons.keys() {
            push(t);
        }
        for (t, _) in self.fonts.keys() {
            push(t);
        }
        out.sort();
        out
    }

    /// The item names of a category for one type, sorted
    /// (`Theme.get_color_list` and friends).
    pub fn get_item_list(&self, control_type: &str, category: ThemePropertyType) -> Vec<String> {
        let collect = |keys: Vec<&ThemeKey>| -> Vec<String> {
            let mut out: Vec<String> = keys
                .into_iter()
                .filter(|(t, _)| t == control_type)
                .map(|(_, n)| n.clone())
                .collect();
            out.sort();
            out
        };
        match category {
            ThemePropertyType::Color => collect(self.colors.keys().collect()),
            ThemePropertyType::FontSize => collect(self.font_sizes.keys().collect()),
            ThemePropertyType::Constant => collect(self.constants.keys().collect()),
            ThemePropertyType::Stylebox => collect(self.styleboxes.keys().collect()),
            ThemePropertyType::Font => collect(self.fonts.keys().collect()),
            // Icons live only in the generic `items` store, so their names come
            // from there rather than from a dedicated map.
            ThemePropertyType::Icon => collect(
                self.items
                    .keys()
                    .filter(|(cat, _)| *cat == ThemePropertyType::Icon)
                    .map(|(_, key)| key)
                    .collect(),
            ),
        }
    }

    // -- StyleBoxes (typed) --------------------------------------------------

    /// Sets a typed [`StyleBox`] for a control type/property (pat-u3l8c.70).
    pub fn set_stylebox(&mut self, control_type: &str, property: &str, stylebox: StyleBox) {
        self.styleboxes
            .insert((control_type.to_string(), property.to_string()), stylebox);
    }

    /// Gets a typed [`StyleBox`], returning `None` if not set.
    pub fn get_stylebox(&self, control_type: &str, property: &str) -> Option<&StyleBox> {
        self.type_chain(control_type)
            .into_iter()
            .find_map(|t| self.styleboxes.get(&(t, property.to_string())))
    }

    /// Returns `true` if a typed StyleBox exists for the given key.
    pub fn has_stylebox(&self, control_type: &str, property: &str) -> bool {
        self.get_stylebox(control_type, property).is_some()
    }

    /// Loads all StyleBox sub-resources from a `.tscn`/`.tres` string and
    /// assigns them under `control_type` keyed by their sub-resource id.
    ///
    /// Convenience for parsing a theme resource into typed styleboxes.
    pub fn load_styleboxes_from_tscn(&mut self, control_type: &str, text: &str) {
        for (id, sb) in crate::stylebox::parse_styleboxes_from_tscn(text) {
            self.styleboxes.insert((control_type.to_string(), id), sb);
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
        self.type_chain(control_type)
            .into_iter()
            .find_map(|t| self.colors.get(&(t, property.to_string())).copied())
    }

    /// Returns `true` if the color property exists.
    pub fn has_color(&self, control_type: &str, property: &str) -> bool {
        self.get_color(control_type, property).is_some()
    }

    // -- Font sizes ----------------------------------------------------------

    /// Sets a font size property for a control type.
    pub fn set_font_size(&mut self, control_type: &str, property: &str, size: i64) {
        self.font_sizes
            .insert((control_type.to_string(), property.to_string()), size);
    }

    /// Gets a font size property, returning `None` if not set.
    pub fn get_font_size(&self, control_type: &str, property: &str) -> Option<i64> {
        self.type_chain(control_type)
            .into_iter()
            .find_map(|t| self.font_sizes.get(&(t, property.to_string())).copied())
            // A theme-wide `default_font_size` backs the common `font_size`
            // item when no type declares one, as it does in Godot.
            .or(if property == "font_size" {
                self.default_font_size
            } else {
                None
            })
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
        self.type_chain(control_type)
            .into_iter()
            .find_map(|t| self.constants.get(&(t, property.to_string())).copied())
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

        // --- Separators (pat-nzgdu.197) ---
        // Godot builds these as a StyleBoxLine of thickness round(scale) = 1,
        // and gives each a `separation` of 4 — the minimum size a separator
        // reserves in its container (`scene/theme/default_theme.cpp:734-739`,
        // `:1063-1069`). VSeparator is the same line rotated, which is why the
        // orientation lives on the stylebox rather than on a node property.
        let separator_color = Color::new(0.5, 0.5, 0.5, 1.0);
        theme.set_stylebox(
            "HSeparator",
            "separator",
            StyleBox::Line(StyleBoxLine {
                color: separator_color,
                thickness: 1.0,
                vertical: false,
                ..StyleBoxLine::default()
            }),
        );
        theme.set_stylebox(
            "VSeparator",
            "separator",
            StyleBox::Line(StyleBoxLine {
                color: separator_color,
                thickness: 1.0,
                vertical: true,
                ..StyleBoxLine::default()
            }),
        );
        theme.set_constant("HSeparator", "separation", 4);
        theme.set_constant("VSeparator", "separation", 4);

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
        // Godot builds each button state with
        // `make_flat_stylebox(color, 4, 4, 4, 4)` + `corner_radius 3`
        // (`scene/theme/default_theme.cpp`), which is why a themed Button has
        // rounded corners and insets its text. These used to be bare
        // `Variant::Color` placeholders, so border/corner/margin/focus/disabled
        // were all unrepresentable (pat-0q2kx.145).
        set_flat(
            &mut theme,
            "Button",
            "normal",
            Color::new(0.2, 0.2, 0.2, 1.0),
        );
        set_flat(
            &mut theme,
            "Button",
            "hover",
            Color::new(0.3, 0.3, 0.3, 1.0),
        );
        set_flat(
            &mut theme,
            "Button",
            "pressed",
            Color::new(0.15, 0.15, 0.15, 1.0),
        );
        set_flat(
            &mut theme,
            "Button",
            "disabled",
            Color::new(0.2, 0.2, 0.2, 0.5),
        );
        // Godot's focus box is a transparent fill with a visible border, so it
        // draws OVER the state box rather than replacing it.
        theme.set_stylebox(
            "Button",
            "focus",
            StyleBox::Flat(StyleBoxFlat {
                bg_color: Color::new(0.0, 0.0, 0.0, 0.0),
                border_width: [1.0; 4],
                border_color: Color::new(0.6, 0.6, 0.6, 1.0),
                corner_radius: [DEFAULT_CORNER_RADIUS; 4],
                content_margin: [DEFAULT_CONTENT_MARGIN; 4],
                ..StyleBoxFlat::default()
            }),
        );

        // --- Panel ---
        set_flat(
            &mut theme,
            "Panel",
            "panel",
            Color::new(0.15, 0.15, 0.15, 1.0),
        );
        theme.set_constant("Panel", "content_margin_left", 4);
        theme.set_constant("Panel", "content_margin_top", 4);
        theme.set_constant("Panel", "content_margin_right", 4);
        theme.set_constant("Panel", "content_margin_bottom", 4);

        // --- The rest of the control family (pat-0q2kx.145) ---
        //
        // Godot's default theme gives every common control a panel/background
        // stylebox; Patina seeded only five control types, so anything else
        // rendered with no themed background at all. Each of these is Godot's
        // `make_flat_stylebox(color, 4, 4, 4, 4, corner 3)` shape — the same
        // margins and radius as Button above.
        //
        // The PALETTE is Patina's existing dark scheme rather than Godot's
        // derived one: matching Godot's exact per-control colours needs the
        // `base_color`/`accent_color` derivation from `default_theme.cpp`,
        // which is a separate pass. The STRUCTURE (a real StyleBoxFlat with
        // margins and radius, not a bare colour) is what was missing.
        let bg = Color::new(0.15, 0.15, 0.15, 1.0);
        let field_bg = Color::new(0.1, 0.1, 0.1, 1.0);
        for (control, property) in [
            // Text entry / views read as sunken fields.
            ("LineEdit", "normal"),
            ("TextEdit", "normal"),
            ("CodeEdit", "normal"),
            ("SpinBox", "normal"),
        ] {
            set_flat(&mut theme, control, property, field_bg);
        }
        for (control, property) in [
            ("PanelContainer", "panel"),
            ("Tree", "panel"),
            ("ItemList", "panel"),
            ("PopupMenu", "panel"),
            ("PopupPanel", "panel"),
            ("TabContainer", "panel"),
            ("ScrollContainer", "panel"),
            ("GraphEdit", "panel"),
            ("RichTextLabel", "normal"),
            ("ProgressBar", "background"),
            ("HScrollBar", "scroll"),
            ("VScrollBar", "scroll"),
            ("HSlider", "slider"),
            ("VSlider", "slider"),
            ("TabBar", "tab_selected"),
            ("OptionButton", "normal"),
            ("MenuButton", "normal"),
            ("CheckBox", "normal"),
            ("CheckButton", "normal"),
            ("TextureButton", "normal"),
            ("AcceptDialog", "panel"),
            ("Window", "embedded_border"),
        ] {
            set_flat(&mut theme, control, property, bg);
        }

        godot_default_styleboxes(&mut theme);
        theme
    }
}

/// The default theme's styleboxes for the controls a game UI is made of, with
/// Godot's OWN colours and margins (`scene/theme/default_theme.cpp`,
/// pat-9ww3e) rather than Patina's earlier opaque dark scheme — a Button there
/// was an opaque (0.2, 0.2, 0.2) where Godot's is a 60%-alpha (0.1, 0.1, 0.1),
/// and a PanelContainer inset its child by 4px where Godot's insets by 0.
fn godot_default_styleboxes(theme: &mut Theme) {
    // `style_normal_color` and friends.
    let normal = Color::new(0.1, 0.1, 0.1, 0.6);
    let hover = Color::new(0.225, 0.225, 0.225, 0.6);
    let pressed = Color::new(0.0, 0.0, 0.0, 0.6);
    let disabled = Color::new(0.1, 0.1, 0.1, 0.3);
    let focus = Color::new(1.0, 1.0, 1.0, 0.75);
    let progress = Color::new(1.0, 1.0, 1.0, 0.4);
    let flat = |bg: Color, margin: f32, radius: f32| {
        StyleBox::Flat(StyleBoxFlat {
            bg_color: bg,
            corner_radius: [radius; 4],
            content_margin: [margin; 4],
            ..StyleBoxFlat::default()
        })
    };
    let set = |theme: &mut Theme, class: &str, name: &str, sb: StyleBox| {
        if let Some(bg) = sb.bg_color() {
            theme.set_item(ThemePropertyType::Stylebox, class, name, Variant::Color(bg));
        }
        theme.set_stylebox(class, name, sb);
    };
    // Panel / PanelContainer: `make_flat_stylebox(style_normal_color, 0, 0, 0, 0)`.
    set(theme, "Panel", "panel", flat(normal, 0.0, DEFAULT_CORNER_RADIUS));
    set(theme, "PanelContainer", "panel", flat(normal, 0.0, DEFAULT_CORNER_RADIUS));
    // Button: `make_flat_stylebox(<state colour>)`, margins 4.
    for (name, bg) in [
        ("normal", normal),
        ("hover", hover),
        ("pressed", pressed),
        ("disabled", disabled),
    ] {
        set(theme, "Button", name, flat(bg, DEFAULT_CONTENT_MARGIN, DEFAULT_CORNER_RADIUS));
    }
    // Button focus: no fill, a 2px `style_focus_color` border, expanded 2px
    // so it sits flush OUTSIDE the button (`focus->set_expand_margin_all(2)`).
    theme.set_stylebox(
        "Button",
        "focus",
        StyleBox::Flat(StyleBoxFlat {
            bg_color: focus,
            draw_center: false,
            border_width: [2.0; 4],
            border_color: focus,
            expand_margin: [2.0; 4],
            corner_radius: [DEFAULT_CORNER_RADIUS; 4],
            content_margin: [DEFAULT_CONTENT_MARGIN; 4],
            ..StyleBoxFlat::default()
        }),
    );
    // CheckBox: every state is a StyleBoxEmpty with 4px content margins.
    for name in ["normal", "pressed", "hover", "hover_pressed", "disabled"] {
        // An EMPTY box has no background, so no legacy colour item either
        // (`stylebox_bg` would otherwise paint the old flat colour).
        theme.remove_item(ThemePropertyType::Stylebox, "CheckBox", name);
        theme.set_stylebox(
            "CheckBox",
            name,
            StyleBox::Empty(crate::stylebox::StyleBoxEmpty {
                content_margin: [DEFAULT_CONTENT_MARGIN; 4],
            }),
        );
    }
    theme.set_constant("CheckBox", "h_separation", 4);
    theme.set_color("CheckBox", "font_color", Color::new(0.875, 0.875, 0.875, 1.0));
    theme.set_color("CheckBox", "font_pressed_color", Color::WHITE);
    // Sliders: `make_flat_stylebox(colour, 4, 4, 4, 4, 4)`.
    for class in ["HSlider", "VSlider"] {
        set(theme, class, "slider", flat(normal, 4.0, 4.0));
        set(theme, class, "grabber_area", flat(progress, 4.0, 4.0));
        set(theme, class, "grabber_area_highlight", flat(focus, 4.0, 4.0));
    }
}

/// Godot's default flat stylebox content margin (`make_flat_stylebox`'s
/// `margin_*` default of 4) — pat-0q2kx.145.
const DEFAULT_CONTENT_MARGIN: f32 = 4.0;
/// Godot's default flat stylebox corner radius (`corner_radius 3`).
const DEFAULT_CORNER_RADIUS: f32 = 3.0;

/// Registers a Godot-shaped flat stylebox under `control_type`/`property`.
///
/// Writes BOTH stores from one value: the typed [`StyleBox`] (which carries the
/// corner radius and content margins a `Variant::Color` cannot express) and the
/// legacy `Variant::Color` item. `control_render::stylebox_bg` still reads the
/// legacy entry, so driving both from a single `bg_color` is what keeps the
/// renderer and the typed theme from disagreeing — two stores answering the
/// same question differently is the failure this seeds against.
fn set_flat(theme: &mut Theme, control_type: &str, property: &str, bg_color: Color) {
    theme.set_item(
        ThemePropertyType::Stylebox,
        control_type,
        property,
        Variant::Color(bg_color),
    );
    theme.set_stylebox(
        control_type,
        property,
        StyleBox::Flat(StyleBoxFlat {
            bg_color,
            corner_radius: [DEFAULT_CORNER_RADIUS; 4],
            content_margin: [DEFAULT_CONTENT_MARGIN; 4],
            ..StyleBoxFlat::default()
        }),
    );
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

// ===========================================================================
// Godot `.tres` theme format (pat-0q2kx.150)
// ===========================================================================

/// Parses a Godot-authored Theme resource.
///
/// A real theme `.tres` keys its items in the `[resource]` section as
/// `Type/category/name = value` (`Button/styles/normal`,
/// `Button/colors/font_color`, …), with styleboxes as `SubResource(...)`
/// references and fonts as `ExtResource(...)`. Type variations appear as
/// `Variation/base_type = &"Button"`, and the theme-wide defaults as plain
/// `default_font_size` / `default_base_scale` keys.
///
/// Patina's previous ingestion (`load_styleboxes_from_tscn`) keyed styleboxes
/// by *sub-resource id* under a caller-supplied type, so a genuine Godot theme
/// file applied nothing at all — the ids it produced (`StyleBoxFlat_abc`) are
/// not item names any Control ever asks for.
pub fn parse_theme_tres(text: &str) -> Theme {
    let mut theme = Theme::new("Theme");

    // Sub-resource styleboxes, by id, so a `SubResource("…")` reference in the
    // `[resource]` section can be resolved to the real box.
    let boxes = crate::stylebox::parse_styleboxes_from_tscn(text);

    let mut in_resource = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            // Only the `[resource]` block holds the theme's own items; the
            // `[sub_resource]` blocks were consumed above.
            in_resource = line.starts_with("[resource");
            continue;
        }
        if !in_resource {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());

        // Theme-wide defaults.
        match key {
            "default_font_size" => {
                if let Ok(n) = value.parse::<i64>() {
                    theme.default_font_size = Some(n);
                }
                continue;
            }
            "default_base_scale" => {
                if let Ok(f) = value.parse::<f32>() {
                    theme.default_base_scale = Some(f);
                }
                continue;
            }
            "default_font" => {
                if let Some(path) = ext_resource_path(text, value) {
                    theme.default_font = Some(path);
                }
                continue;
            }
            _ => {}
        }

        let parts: Vec<&str> = key.split('/').collect();
        match parts.as_slice() {
            // `FlatButton/base_type = &"Button"` declares a type variation.
            [variation, "base_type"] => {
                let base = value.trim_start_matches('&').trim_matches('"');
                theme.set_type_variation(variation, base);
            }
            [control_type, category, name] => match *category {
                "styles" => {
                    if let Some(id) = sub_resource_id(value) {
                        if let Some(sb) = boxes.get(&id) {
                            theme.set_stylebox(control_type, name, sb.clone());
                        }
                    }
                }
                "colors" => {
                    if let Some(c) = parse_color_literal(value) {
                        theme.set_color(control_type, name, c);
                    }
                }
                "font_sizes" => {
                    if let Ok(n) = value.parse::<i64>() {
                        theme.set_font_size(control_type, name, n);
                    }
                }
                "constants" => {
                    if let Ok(n) = value.parse::<i64>() {
                        theme.set_constant(control_type, name, n);
                    }
                }
                "icons" => {
                    if let Some(path) = ext_resource_path(text, value) {
                        theme.set_icon(control_type, name, &path);
                    }
                }
                "fonts" => {
                    if let Some(path) = ext_resource_path(text, value) {
                        theme.set_font(control_type, name, &path);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    theme
}

/// The id inside `SubResource("id")` / `SubResource( id )`.
fn sub_resource_id(value: &str) -> Option<String> {
    let inner = value
        .trim()
        .strip_prefix("SubResource(")?
        .strip_suffix(')')?;
    Some(inner.trim().trim_matches('"').to_string())
}

/// Resolves `ExtResource("id")` to the `path=` of that `[ext_resource]` line.
///
/// A theme's fonts and icons are external resources, so the value alone is just
/// an id — the path has to come from the header block.
fn ext_resource_path(text: &str, value: &str) -> Option<String> {
    let id = value
        .trim()
        .strip_prefix("ExtResource(")?
        .strip_suffix(')')?
        .trim()
        .trim_matches('"')
        .to_string();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("[ext_resource") {
            continue;
        }
        let has_id = line
            .split_whitespace()
            .any(|tok| tok == format!("id=\"{id}\"") || tok == format!("id={id}"));
        if !has_id {
            continue;
        }
        for tok in line.split_whitespace() {
            if let Some(p) = tok.strip_prefix("path=") {
                return Some(p.trim_matches('"').trim_end_matches(']').to_string());
            }
        }
    }
    None
}

/// Parses a `Color(r, g, b, a)` literal.
fn parse_color_literal(value: &str) -> Option<Color> {
    let inner = value.trim().strip_prefix("Color(")?.strip_suffix(')')?;
    let parts: Vec<f32> = inner
        .split(',')
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    Some(Color::new(
        parts[0],
        parts[1],
        parts[2],
        parts.get(3).copied().unwrap_or(1.0),
    ))
}

impl Theme {
    /// Loads a Godot-authored theme `.tres`, replacing this theme's contents.
    pub fn from_tres(text: &str) -> Theme {
        parse_theme_tres(text)
    }
}
