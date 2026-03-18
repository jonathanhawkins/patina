//! UI/Control node property helpers for Control, Label, Button, TextureRect,
//! LineEdit, Panel, and container nodes.
//!
//! Follows the same pattern as [`node2d`](crate::node2d): typed helper
//! functions that read and write well-known properties on nodes stored in
//! a [`SceneTree`].

use gdcore::math::{Color, Vector2};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ===========================================================================
// Enums
// ===========================================================================

/// Anchor presets matching Godot's `Control.LayoutPreset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorPreset {
    /// Anchors (0,0,1,1) — fills the parent.
    FullRect,
    /// Anchors (0.5,0.5,0.5,0.5) — centered point.
    Center,
    /// Anchors (0,0,0,0) — top-left corner.
    TopLeft,
    /// Anchors (1,0,1,0) — top-right corner.
    TopRight,
    /// Anchors (0,1,0,1) — bottom-left corner.
    BottomLeft,
    /// Anchors (1,1,1,1) — bottom-right corner.
    BottomRight,
    /// Anchors (0,0.5,0,0.5) — center of left edge.
    CenterLeft,
    /// Anchors (1,0.5,1,0.5) — center of right edge.
    CenterRight,
    /// Anchors (0,0,1,0) — full width at top.
    TopWide,
    /// Anchors (0,1,1,1) — full width at bottom.
    BottomWide,
    /// Anchors (0,0,0,1) — full height on left.
    LeftWide,
    /// Anchors (1,0,1,1) — full height on right.
    RightWide,
}

/// Size flags matching Godot's `Control.SizeFlags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeFlags {
    /// Fill the available space (default).
    Fill,
    /// Expand to take remaining space.
    Expand,
    /// Shrink to minimum size, centered.
    ShrinkCenter,
    /// Shrink to minimum size, aligned to end.
    ShrinkEnd,
}

impl SizeFlags {
    /// Returns the Godot integer representation for this flag.
    fn to_int(self) -> i64 {
        match self {
            SizeFlags::Fill => 1,
            SizeFlags::Expand => 3, // FILL | EXPAND
            SizeFlags::ShrinkCenter => 4,
            SizeFlags::ShrinkEnd => 8,
        }
    }

    /// Converts from Godot integer representation.
    fn from_int(v: i64) -> Self {
        match v {
            3 => SizeFlags::Expand,
            4 => SizeFlags::ShrinkCenter,
            8 => SizeFlags::ShrinkEnd,
            _ => SizeFlags::Fill,
        }
    }
}

/// Focus mode matching Godot's `Control.FocusMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusMode {
    /// Cannot receive focus.
    None,
    /// Focus on click only.
    Click,
    /// Focus on click and keyboard navigation.
    All,
}

impl FocusMode {
    /// Returns the Godot integer representation.
    fn to_int(self) -> i64 {
        match self {
            FocusMode::None => 0,
            FocusMode::Click => 1,
            FocusMode::All => 2,
        }
    }

    /// Converts from Godot integer representation.
    fn from_int(v: i64) -> Self {
        match v {
            1 => FocusMode::Click,
            2 => FocusMode::All,
            _ => FocusMode::None,
        }
    }
}

/// Horizontal/vertical text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    /// Left or top alignment.
    Left,
    /// Center alignment.
    Center,
    /// Right or bottom alignment.
    Right,
}

impl TextAlign {
    fn to_int(self) -> i64 {
        match self {
            TextAlign::Left => 0,
            TextAlign::Center => 1,
            TextAlign::Right => 2,
        }
    }

    fn from_int(v: i64) -> Self {
        match v {
            1 => TextAlign::Center,
            2 => TextAlign::Right,
            _ => TextAlign::Left,
        }
    }
}

/// Stretch mode for TextureRect nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StretchMode {
    /// Scale to fill the rect.
    Scale,
    /// Keep original size.
    Keep,
    /// Keep aspect ratio, fit inside rect.
    KeepAspect,
    /// Keep aspect ratio, centered inside rect.
    KeepAspectCentered,
}

impl StretchMode {
    fn to_int(self) -> i64 {
        match self {
            StretchMode::Scale => 0,
            StretchMode::Keep => 1,
            StretchMode::KeepAspect => 5,
            StretchMode::KeepAspectCentered => 6,
        }
    }

    fn from_int(v: i64) -> Self {
        match v {
            1 => StretchMode::Keep,
            5 => StretchMode::KeepAspect,
            6 => StretchMode::KeepAspectCentered,
            _ => StretchMode::Scale,
        }
    }
}

/// Grow direction for Control layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowDirection {
    /// Grow toward the beginning (left or top).
    Begin,
    /// Grow toward the end (right or bottom).
    End,
    /// Grow in both directions.
    Both,
}

impl GrowDirection {
    fn to_int(self) -> i64 {
        match self {
            GrowDirection::Begin => 0,
            GrowDirection::End => 1,
            GrowDirection::Both => 2,
        }
    }

    fn from_int(v: i64) -> Self {
        match v {
            0 => GrowDirection::Begin,
            2 => GrowDirection::Both,
            _ => GrowDirection::End,
        }
    }
}

// ===========================================================================
// Control base — anchors and offsets
// ===========================================================================

/// Sets the left anchor value (0.0–1.0).
pub fn set_anchor_left(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("anchor_left", Variant::Float(val as f64));
    }
}

/// Gets the left anchor value, defaulting to `0.0`.
pub fn get_anchor_left(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("anchor_left") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the top anchor value (0.0–1.0).
pub fn set_anchor_top(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("anchor_top", Variant::Float(val as f64));
    }
}

/// Gets the top anchor value, defaulting to `0.0`.
pub fn get_anchor_top(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("anchor_top") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the right anchor value (0.0–1.0).
pub fn set_anchor_right(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("anchor_right", Variant::Float(val as f64));
    }
}

/// Gets the right anchor value, defaulting to `0.0`.
pub fn get_anchor_right(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("anchor_right") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the bottom anchor value (0.0–1.0).
pub fn set_anchor_bottom(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("anchor_bottom", Variant::Float(val as f64));
    }
}

/// Gets the bottom anchor value, defaulting to `0.0`.
pub fn get_anchor_bottom(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("anchor_bottom") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

// -- Offsets ----------------------------------------------------------------

/// Sets the left offset in pixels.
pub fn set_offset_left(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("offset_left", Variant::Float(val as f64));
    }
}

/// Gets the left offset, defaulting to `0.0`.
pub fn get_offset_left(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("offset_left") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the top offset in pixels.
pub fn set_offset_top(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("offset_top", Variant::Float(val as f64));
    }
}

/// Gets the top offset, defaulting to `0.0`.
pub fn get_offset_top(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("offset_top") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the right offset in pixels.
pub fn set_offset_right(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("offset_right", Variant::Float(val as f64));
    }
}

/// Gets the right offset, defaulting to `0.0`.
pub fn get_offset_right(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("offset_right") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the bottom offset in pixels.
pub fn set_offset_bottom(tree: &mut SceneTree, node_id: NodeId, val: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("offset_bottom", Variant::Float(val as f64));
    }
}

/// Gets the bottom offset, defaulting to `0.0`.
pub fn get_offset_bottom(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("offset_bottom") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

// -- Size -------------------------------------------------------------------

/// Sets the `"size"` property on a Control node.
pub fn set_size(tree: &mut SceneTree, node_id: NodeId, size: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("size", Variant::Vector2(size));
    }
}

/// Gets the `"size"` property, defaulting to [`Vector2::ZERO`].
pub fn get_size(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Sets the `"min_size"` property on a Control node.
pub fn set_min_size(tree: &mut SceneTree, node_id: NodeId, size: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("min_size", Variant::Vector2(size));
    }
}

/// Gets the `"min_size"` property, defaulting to [`Vector2::ZERO`].
pub fn get_min_size(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("min_size") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Sets the `"custom_minimum_size"` property on a Control node.
pub fn set_custom_minimum_size(tree: &mut SceneTree, node_id: NodeId, size: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("custom_minimum_size", Variant::Vector2(size));
    }
}

/// Gets the `"custom_minimum_size"` property, defaulting to [`Vector2::ZERO`].
pub fn get_custom_minimum_size(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("custom_minimum_size") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

// -- Grow direction ---------------------------------------------------------

/// Sets the horizontal grow direction.
pub fn set_grow_direction_h(tree: &mut SceneTree, node_id: NodeId, dir: GrowDirection) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("grow_horizontal", Variant::Int(dir.to_int()));
    }
}

/// Gets the horizontal grow direction, defaulting to [`GrowDirection::End`].
pub fn get_grow_direction_h(tree: &SceneTree, node_id: NodeId) -> GrowDirection {
    tree.get_node(node_id)
        .map(|n| match n.get_property("grow_horizontal") {
            Variant::Int(i) => GrowDirection::from_int(i),
            _ => GrowDirection::End,
        })
        .unwrap_or(GrowDirection::End)
}

/// Sets the vertical grow direction.
pub fn set_grow_direction_v(tree: &mut SceneTree, node_id: NodeId, dir: GrowDirection) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("grow_vertical", Variant::Int(dir.to_int()));
    }
}

/// Gets the vertical grow direction, defaulting to [`GrowDirection::End`].
pub fn get_grow_direction_v(tree: &SceneTree, node_id: NodeId) -> GrowDirection {
    tree.get_node(node_id)
        .map(|n| match n.get_property("grow_vertical") {
            Variant::Int(i) => GrowDirection::from_int(i),
            _ => GrowDirection::End,
        })
        .unwrap_or(GrowDirection::End)
}

// -- Anchor presets ---------------------------------------------------------

/// Applies an anchor preset, setting all four anchor values at once.
pub fn apply_anchor_preset(tree: &mut SceneTree, node_id: NodeId, preset: AnchorPreset) {
    let (left, top, right, bottom) = match preset {
        AnchorPreset::FullRect => (0.0, 0.0, 1.0, 1.0),
        AnchorPreset::Center => (0.5, 0.5, 0.5, 0.5),
        AnchorPreset::TopLeft => (0.0, 0.0, 0.0, 0.0),
        AnchorPreset::TopRight => (1.0, 0.0, 1.0, 0.0),
        AnchorPreset::BottomLeft => (0.0, 1.0, 0.0, 1.0),
        AnchorPreset::BottomRight => (1.0, 1.0, 1.0, 1.0),
        AnchorPreset::CenterLeft => (0.0, 0.5, 0.0, 0.5),
        AnchorPreset::CenterRight => (1.0, 0.5, 1.0, 0.5),
        AnchorPreset::TopWide => (0.0, 0.0, 1.0, 0.0),
        AnchorPreset::BottomWide => (0.0, 1.0, 1.0, 1.0),
        AnchorPreset::LeftWide => (0.0, 0.0, 0.0, 1.0),
        AnchorPreset::RightWide => (1.0, 0.0, 1.0, 1.0),
    };
    set_anchor_left(tree, node_id, left);
    set_anchor_top(tree, node_id, top);
    set_anchor_right(tree, node_id, right);
    set_anchor_bottom(tree, node_id, bottom);
}

// ===========================================================================
// SizeFlags
// ===========================================================================

/// Sets the horizontal size flags on a Control node.
pub fn set_h_size_flags(tree: &mut SceneTree, node_id: NodeId, flags: SizeFlags) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("size_flags_horizontal", Variant::Int(flags.to_int()));
    }
}

/// Gets the horizontal size flags, defaulting to [`SizeFlags::Fill`].
pub fn get_h_size_flags(tree: &SceneTree, node_id: NodeId) -> SizeFlags {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size_flags_horizontal") {
            Variant::Int(i) => SizeFlags::from_int(i),
            _ => SizeFlags::Fill,
        })
        .unwrap_or(SizeFlags::Fill)
}

/// Sets the vertical size flags on a Control node.
pub fn set_v_size_flags(tree: &mut SceneTree, node_id: NodeId, flags: SizeFlags) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("size_flags_vertical", Variant::Int(flags.to_int()));
    }
}

/// Gets the vertical size flags, defaulting to [`SizeFlags::Fill`].
pub fn get_v_size_flags(tree: &SceneTree, node_id: NodeId) -> SizeFlags {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size_flags_vertical") {
            Variant::Int(i) => SizeFlags::from_int(i),
            _ => SizeFlags::Fill,
        })
        .unwrap_or(SizeFlags::Fill)
}

// ===========================================================================
// Container helpers
// ===========================================================================

/// Sets the `"separation"` theme override on VBoxContainer/HBoxContainer.
pub fn set_separation(tree: &mut SceneTree, node_id: NodeId, px: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("theme_override_constants/separation", Variant::Int(px));
    }
}

/// Gets the `"separation"` theme override, defaulting to `0`.
pub fn get_separation(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(
            |n| match n.get_property("theme_override_constants/separation") {
                Variant::Int(i) => i,
                _ => 0,
            },
        )
        .unwrap_or(0)
}

// ===========================================================================
// Label
// ===========================================================================

/// Sets the `"text"` property on a Label node.
pub fn set_label_text(tree: &mut SceneTree, node_id: NodeId, text: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("text", Variant::String(text.to_owned()));
    }
}

/// Gets the `"text"` property from a Label node, defaulting to `""`.
pub fn get_label_text(tree: &SceneTree, node_id: NodeId) -> String {
    tree.get_node(node_id)
        .map(|n| match n.get_property("text") {
            Variant::String(s) => s,
            _ => String::new(),
        })
        .unwrap_or_default()
}

/// Sets the font size theme override on a Label.
pub fn set_font_size(tree: &mut SceneTree, node_id: NodeId, size: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("theme_override_font_sizes/font_size", Variant::Int(size));
    }
}

/// Gets the font size theme override, defaulting to `16`.
pub fn get_font_size(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(
            |n| match n.get_property("theme_override_font_sizes/font_size") {
                Variant::Int(i) => i,
                _ => 16,
            },
        )
        .unwrap_or(16)
}

/// Sets the horizontal alignment on a Label.
pub fn set_h_align(tree: &mut SceneTree, node_id: NodeId, align: TextAlign) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("horizontal_alignment", Variant::Int(align.to_int()));
    }
}

/// Gets the horizontal alignment, defaulting to [`TextAlign::Left`].
pub fn get_h_align(tree: &SceneTree, node_id: NodeId) -> TextAlign {
    tree.get_node(node_id)
        .map(|n| match n.get_property("horizontal_alignment") {
            Variant::Int(i) => TextAlign::from_int(i),
            _ => TextAlign::Left,
        })
        .unwrap_or(TextAlign::Left)
}

/// Sets the vertical alignment on a Label.
pub fn set_v_align(tree: &mut SceneTree, node_id: NodeId, align: TextAlign) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("vertical_alignment", Variant::Int(align.to_int()));
    }
}

/// Gets the vertical alignment, defaulting to [`TextAlign::Left`].
pub fn get_v_align(tree: &SceneTree, node_id: NodeId) -> TextAlign {
    tree.get_node(node_id)
        .map(|n| match n.get_property("vertical_alignment") {
            Variant::Int(i) => TextAlign::from_int(i),
            _ => TextAlign::Left,
        })
        .unwrap_or(TextAlign::Left)
}

/// Sets the `"autowrap_mode"` on a Label (`0` = off, `1` = arbitrary, etc.).
pub fn set_autowrap(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        // 0 = Off, 1 = Arbitrary (the most common "on" mode).
        let mode = if enabled { 1i64 } else { 0i64 };
        node.set_property("autowrap_mode", Variant::Int(mode));
    }
}

/// Gets whether autowrap is enabled on a Label.
pub fn get_autowrap(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("autowrap_mode") {
            Variant::Int(i) => i != 0,
            _ => false,
        })
        .unwrap_or(false)
}

// ===========================================================================
// Button
// ===========================================================================

/// Sets the `"text"` property on a Button node.
pub fn set_button_text(tree: &mut SceneTree, node_id: NodeId, text: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("text", Variant::String(text.to_owned()));
    }
}

/// Gets the `"text"` property from a Button node, defaulting to `""`.
pub fn get_button_text(tree: &SceneTree, node_id: NodeId) -> String {
    tree.get_node(node_id)
        .map(|n| match n.get_property("text") {
            Variant::String(s) => s,
            _ => String::new(),
        })
        .unwrap_or_default()
}

/// Sets the `"disabled"` property on a Button node.
pub fn set_disabled(tree: &mut SceneTree, node_id: NodeId, disabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("disabled", Variant::Bool(disabled));
    }
}

/// Returns whether the Button is disabled, defaulting to `false`.
pub fn is_disabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("disabled") {
            Variant::Bool(b) => b,
            _ => false,
        })
        .unwrap_or(false)
}

// ===========================================================================
// TextureRect
// ===========================================================================

/// Sets the `"texture"` property on a TextureRect node.
pub fn set_texture_rect_path(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("texture", Variant::String(path.to_owned()));
    }
}

/// Gets the `"texture"` property from a TextureRect node.
pub fn get_texture_rect_path(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("texture") {
            Variant::String(s) => Some(s),
            _ => None,
        })
}

/// Sets the `"stretch_mode"` on a TextureRect.
pub fn set_stretch_mode(tree: &mut SceneTree, node_id: NodeId, mode: StretchMode) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("stretch_mode", Variant::Int(mode.to_int()));
    }
}

/// Gets the stretch mode, defaulting to [`StretchMode::Scale`].
pub fn get_stretch_mode(tree: &SceneTree, node_id: NodeId) -> StretchMode {
    tree.get_node(node_id)
        .map(|n| match n.get_property("stretch_mode") {
            Variant::Int(i) => StretchMode::from_int(i),
            _ => StretchMode::Scale,
        })
        .unwrap_or(StretchMode::Scale)
}

// ===========================================================================
// LineEdit
// ===========================================================================

/// Sets the `"text"` property on a LineEdit node.
pub fn set_line_edit_text(tree: &mut SceneTree, node_id: NodeId, text: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("text", Variant::String(text.to_owned()));
    }
}

/// Gets the `"text"` property from a LineEdit node, defaulting to `""`.
pub fn get_line_edit_text(tree: &SceneTree, node_id: NodeId) -> String {
    tree.get_node(node_id)
        .map(|n| match n.get_property("text") {
            Variant::String(s) => s,
            _ => String::new(),
        })
        .unwrap_or_default()
}

/// Sets the `"placeholder_text"` property on a LineEdit.
pub fn set_placeholder(tree: &mut SceneTree, node_id: NodeId, text: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("placeholder_text", Variant::String(text.to_owned()));
    }
}

/// Gets the placeholder text, defaulting to `""`.
pub fn get_placeholder(tree: &SceneTree, node_id: NodeId) -> String {
    tree.get_node(node_id)
        .map(|n| match n.get_property("placeholder_text") {
            Variant::String(s) => s,
            _ => String::new(),
        })
        .unwrap_or_default()
}

/// Sets the `"max_length"` property on a LineEdit (`0` = unlimited).
pub fn set_max_length(tree: &mut SceneTree, node_id: NodeId, max: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("max_length", Variant::Int(max));
    }
}

/// Gets the max length, defaulting to `0` (unlimited).
pub fn get_max_length(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("max_length") {
            Variant::Int(i) => i,
            _ => 0,
        })
        .unwrap_or(0)
}

/// Sets the `"editable"` property on a LineEdit.
pub fn set_editable(tree: &mut SceneTree, node_id: NodeId, editable: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("editable", Variant::Bool(editable));
    }
}

/// Returns whether the LineEdit is editable, defaulting to `true`.
pub fn is_editable(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("editable") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

// ===========================================================================
// Panel
// ===========================================================================

/// Sets a background color override on a Panel (stored as a theme override).
pub fn set_bg_color(tree: &mut SceneTree, node_id: NodeId, color: Color) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            "theme_override_styles/panel/bg_color",
            Variant::Color(color),
        );
    }
}

/// Gets the background color override, defaulting to [`Color::WHITE`].
pub fn get_bg_color(tree: &SceneTree, node_id: NodeId) -> Color {
    tree.get_node(node_id)
        .map(
            |n| match n.get_property("theme_override_styles/panel/bg_color") {
                Variant::Color(c) => c,
                _ => Color::WHITE,
            },
        )
        .unwrap_or(Color::WHITE)
}

/// Sets a border color override on a Panel.
pub fn set_border_color(tree: &mut SceneTree, node_id: NodeId, color: Color) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            "theme_override_styles/panel/border_color",
            Variant::Color(color),
        );
    }
}

/// Gets the border color override, defaulting to [`Color::BLACK`].
pub fn get_border_color(tree: &SceneTree, node_id: NodeId) -> Color {
    tree.get_node(node_id)
        .map(
            |n| match n.get_property("theme_override_styles/panel/border_color") {
                Variant::Color(c) => c,
                _ => Color::BLACK,
            },
        )
        .unwrap_or(Color::BLACK)
}

/// Sets a corner radius override on a Panel (all corners).
pub fn set_corner_radius(tree: &mut SceneTree, node_id: NodeId, radius: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            "theme_override_styles/panel/corner_radius",
            Variant::Int(radius),
        );
    }
}

/// Gets the corner radius override, defaulting to `0`.
pub fn get_corner_radius(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(
            |n| match n.get_property("theme_override_styles/panel/corner_radius") {
                Variant::Int(i) => i,
                _ => 0,
            },
        )
        .unwrap_or(0)
}

// ===========================================================================
// Focus
// ===========================================================================

/// Sets the focus mode on a Control node.
pub fn set_focus_mode(tree: &mut SceneTree, node_id: NodeId, mode: FocusMode) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("focus_mode", Variant::Int(mode.to_int()));
    }
}

/// Gets the focus mode, defaulting to [`FocusMode::None`].
pub fn get_focus_mode(tree: &SceneTree, node_id: NodeId) -> FocusMode {
    tree.get_node(node_id)
        .map(|n| match n.get_property("focus_mode") {
            Variant::Int(i) => FocusMode::from_int(i),
            _ => FocusMode::None,
        })
        .unwrap_or(FocusMode::None)
}

/// Sets the `"focus_next"` node path for keyboard navigation.
pub fn set_focus_next(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("focus_next", Variant::String(path.to_owned()));
    }
}

/// Gets the `"focus_next"` path, if set.
pub fn get_focus_next(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("focus_next") {
            Variant::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })
}

/// Sets the `"focus_previous"` node path for keyboard navigation.
pub fn set_focus_previous(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("focus_previous", Variant::String(path.to_owned()));
    }
}

/// Gets the `"focus_previous"` path, if set.
pub fn get_focus_previous(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("focus_previous") {
            Variant::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn make_tree() -> SceneTree {
        SceneTree::new()
    }

    fn add_control(tree: &mut SceneTree, name: &str, class: &str) -> NodeId {
        let root = tree.root_id();
        let node = Node::new(name, class);
        tree.add_child(root, node).unwrap()
    }

    // -- Anchors ------------------------------------------------------------

    #[test]
    fn anchor_defaults_are_zero() {
        let tree = make_tree();
        let root = tree.root_id();
        assert_eq!(get_anchor_left(&tree, root), 0.0);
        assert_eq!(get_anchor_top(&tree, root), 0.0);
        assert_eq!(get_anchor_right(&tree, root), 0.0);
        assert_eq!(get_anchor_bottom(&tree, root), 0.0);
    }

    #[test]
    fn set_get_anchors() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Ctrl", "Control");

        set_anchor_left(&mut tree, id, 0.1);
        set_anchor_top(&mut tree, id, 0.2);
        set_anchor_right(&mut tree, id, 0.9);
        set_anchor_bottom(&mut tree, id, 0.8);

        assert!((get_anchor_left(&tree, id) - 0.1).abs() < 1e-4);
        assert!((get_anchor_top(&tree, id) - 0.2).abs() < 1e-4);
        assert!((get_anchor_right(&tree, id) - 0.9).abs() < 1e-4);
        assert!((get_anchor_bottom(&tree, id) - 0.8).abs() < 1e-4);
    }

    #[test]
    fn apply_anchor_preset_full_rect() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Bg", "Control");

        apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        assert!((get_anchor_left(&tree, id) - 0.0).abs() < 1e-4);
        assert!((get_anchor_top(&tree, id) - 0.0).abs() < 1e-4);
        assert!((get_anchor_right(&tree, id) - 1.0).abs() < 1e-4);
        assert!((get_anchor_bottom(&tree, id) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn apply_anchor_preset_center() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "C", "Control");

        apply_anchor_preset(&mut tree, id, AnchorPreset::Center);

        assert!((get_anchor_left(&tree, id) - 0.5).abs() < 1e-4);
        assert!((get_anchor_top(&tree, id) - 0.5).abs() < 1e-4);
        assert!((get_anchor_right(&tree, id) - 0.5).abs() < 1e-4);
        assert!((get_anchor_bottom(&tree, id) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn apply_anchor_preset_corners() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "C", "Control");

        apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        assert_eq!(get_anchor_left(&tree, id), 0.0);
        assert_eq!(get_anchor_top(&tree, id), 0.0);

        apply_anchor_preset(&mut tree, id, AnchorPreset::BottomRight);
        assert!((get_anchor_left(&tree, id) - 1.0).abs() < 1e-4);
        assert!((get_anchor_bottom(&tree, id) - 1.0).abs() < 1e-4);
    }

    // -- Offsets ------------------------------------------------------------

    #[test]
    fn offset_defaults_are_zero() {
        let tree = make_tree();
        let root = tree.root_id();
        assert_eq!(get_offset_left(&tree, root), 0.0);
        assert_eq!(get_offset_top(&tree, root), 0.0);
        assert_eq!(get_offset_right(&tree, root), 0.0);
        assert_eq!(get_offset_bottom(&tree, root), 0.0);
    }

    #[test]
    fn set_get_offsets() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Ctrl", "Control");

        set_offset_left(&mut tree, id, 10.0);
        set_offset_top(&mut tree, id, 20.0);
        set_offset_right(&mut tree, id, 310.0);
        set_offset_bottom(&mut tree, id, 220.0);

        assert!((get_offset_left(&tree, id) - 10.0).abs() < 1e-4);
        assert!((get_offset_top(&tree, id) - 20.0).abs() < 1e-4);
        assert!((get_offset_right(&tree, id) - 310.0).abs() < 1e-4);
        assert!((get_offset_bottom(&tree, id) - 220.0).abs() < 1e-4);
    }

    // -- Size ---------------------------------------------------------------

    #[test]
    fn size_default_is_zero() {
        let tree = make_tree();
        assert_eq!(get_size(&tree, tree.root_id()), Vector2::ZERO);
    }

    #[test]
    fn set_get_size() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Panel", "Panel");

        set_size(&mut tree, id, Vector2::new(800.0, 600.0));
        assert_eq!(get_size(&tree, id), Vector2::new(800.0, 600.0));
    }

    #[test]
    fn set_get_custom_minimum_size() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Ctrl", "Control");

        set_custom_minimum_size(&mut tree, id, Vector2::new(100.0, 50.0));
        assert_eq!(
            get_custom_minimum_size(&tree, id),
            Vector2::new(100.0, 50.0)
        );
    }

    // -- Grow direction -----------------------------------------------------

    #[test]
    fn grow_direction_default_is_end() {
        let tree = make_tree();
        assert_eq!(
            get_grow_direction_h(&tree, tree.root_id()),
            GrowDirection::End
        );
        assert_eq!(
            get_grow_direction_v(&tree, tree.root_id()),
            GrowDirection::End
        );
    }

    #[test]
    fn set_get_grow_direction() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Ctrl", "Control");

        set_grow_direction_h(&mut tree, id, GrowDirection::Both);
        set_grow_direction_v(&mut tree, id, GrowDirection::Begin);

        assert_eq!(get_grow_direction_h(&tree, id), GrowDirection::Both);
        assert_eq!(get_grow_direction_v(&tree, id), GrowDirection::Begin);
    }

    // -- SizeFlags ----------------------------------------------------------

    #[test]
    fn size_flags_default_is_fill() {
        let tree = make_tree();
        assert_eq!(get_h_size_flags(&tree, tree.root_id()), SizeFlags::Fill);
        assert_eq!(get_v_size_flags(&tree, tree.root_id()), SizeFlags::Fill);
    }

    #[test]
    fn set_get_size_flags() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Ctrl", "Control");

        set_h_size_flags(&mut tree, id, SizeFlags::Expand);
        set_v_size_flags(&mut tree, id, SizeFlags::ShrinkCenter);

        assert_eq!(get_h_size_flags(&tree, id), SizeFlags::Expand);
        assert_eq!(get_v_size_flags(&tree, id), SizeFlags::ShrinkCenter);
    }

    // -- Container separation -----------------------------------------------

    #[test]
    fn separation_default_is_zero() {
        let tree = make_tree();
        assert_eq!(get_separation(&tree, tree.root_id()), 0);
    }

    #[test]
    fn set_get_separation() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "VBox", "VBoxContainer");

        set_separation(&mut tree, id, 8);
        assert_eq!(get_separation(&tree, id), 8);
    }

    // -- Label --------------------------------------------------------------

    #[test]
    fn label_text_roundtrip() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Lbl", "Label");

        assert_eq!(get_label_text(&tree, id), "");
        set_label_text(&mut tree, id, "Hello, World!");
        assert_eq!(get_label_text(&tree, id), "Hello, World!");
    }

    #[test]
    fn label_font_size() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Lbl", "Label");

        assert_eq!(get_font_size(&tree, id), 16);
        set_font_size(&mut tree, id, 24);
        assert_eq!(get_font_size(&tree, id), 24);
    }

    #[test]
    fn label_alignment() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Lbl", "Label");

        set_h_align(&mut tree, id, TextAlign::Center);
        set_v_align(&mut tree, id, TextAlign::Right);

        assert_eq!(get_h_align(&tree, id), TextAlign::Center);
        assert_eq!(get_v_align(&tree, id), TextAlign::Right);
    }

    #[test]
    fn label_autowrap() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Lbl", "Label");

        assert!(!get_autowrap(&tree, id));
        set_autowrap(&mut tree, id, true);
        assert!(get_autowrap(&tree, id));
    }

    // -- Button -------------------------------------------------------------

    #[test]
    fn button_text_roundtrip() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Btn", "Button");

        set_button_text(&mut tree, id, "Click Me");
        assert_eq!(get_button_text(&tree, id), "Click Me");
    }

    #[test]
    fn button_disabled() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Btn", "Button");

        assert!(!is_disabled(&tree, id));
        set_disabled(&mut tree, id, true);
        assert!(is_disabled(&tree, id));
    }

    // -- TextureRect --------------------------------------------------------

    #[test]
    fn texture_rect_path_roundtrip() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Tex", "TextureRect");

        assert_eq!(get_texture_rect_path(&tree, id), None);
        set_texture_rect_path(&mut tree, id, "res://sprites/hero.png");
        assert_eq!(
            get_texture_rect_path(&tree, id),
            Some("res://sprites/hero.png".into())
        );
    }

    #[test]
    fn texture_rect_stretch_mode() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Tex", "TextureRect");

        assert_eq!(get_stretch_mode(&tree, id), StretchMode::Scale);
        set_stretch_mode(&mut tree, id, StretchMode::KeepAspect);
        assert_eq!(get_stretch_mode(&tree, id), StretchMode::KeepAspect);
    }

    // -- LineEdit -----------------------------------------------------------

    #[test]
    fn line_edit_text_roundtrip() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Input", "LineEdit");

        set_line_edit_text(&mut tree, id, "user input");
        assert_eq!(get_line_edit_text(&tree, id), "user input");
    }

    #[test]
    fn line_edit_placeholder() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Input", "LineEdit");

        set_placeholder(&mut tree, id, "Enter name...");
        assert_eq!(get_placeholder(&tree, id), "Enter name...");
    }

    #[test]
    fn line_edit_max_length() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Input", "LineEdit");

        assert_eq!(get_max_length(&tree, id), 0);
        set_max_length(&mut tree, id, 32);
        assert_eq!(get_max_length(&tree, id), 32);
    }

    #[test]
    fn line_edit_editable() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Input", "LineEdit");

        assert!(is_editable(&tree, id));
        set_editable(&mut tree, id, false);
        assert!(!is_editable(&tree, id));
    }

    // -- Panel --------------------------------------------------------------

    #[test]
    fn panel_bg_color() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Bg", "Panel");

        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        set_bg_color(&mut tree, id, red);
        assert_eq!(get_bg_color(&tree, id), red);
    }

    #[test]
    fn panel_border_and_radius() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Bg", "Panel");

        let blue = Color::rgb(0.0, 0.0, 1.0);
        set_border_color(&mut tree, id, blue);
        set_corner_radius(&mut tree, id, 8);

        assert_eq!(get_border_color(&tree, id), blue);
        assert_eq!(get_corner_radius(&tree, id), 8);
    }

    // -- Focus --------------------------------------------------------------

    #[test]
    fn focus_mode_default_is_none() {
        let tree = make_tree();
        assert_eq!(get_focus_mode(&tree, tree.root_id()), FocusMode::None);
    }

    #[test]
    fn set_get_focus_mode() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Btn", "Button");

        set_focus_mode(&mut tree, id, FocusMode::All);
        assert_eq!(get_focus_mode(&tree, id), FocusMode::All);

        set_focus_mode(&mut tree, id, FocusMode::Click);
        assert_eq!(get_focus_mode(&tree, id), FocusMode::Click);
    }

    #[test]
    fn focus_next_previous() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Btn", "Button");

        assert_eq!(get_focus_next(&tree, id), None);
        assert_eq!(get_focus_previous(&tree, id), None);

        set_focus_next(&mut tree, id, "../NextButton");
        set_focus_previous(&mut tree, id, "../PrevButton");

        assert_eq!(get_focus_next(&tree, id), Some("../NextButton".into()));
        assert_eq!(get_focus_previous(&tree, id), Some("../PrevButton".into()));
    }

    // -- Wide presets -------------------------------------------------------

    #[test]
    fn apply_anchor_preset_wide_variants() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "C", "Control");

        apply_anchor_preset(&mut tree, id, AnchorPreset::TopWide);
        assert!((get_anchor_left(&tree, id) - 0.0).abs() < 1e-4);
        assert!((get_anchor_top(&tree, id) - 0.0).abs() < 1e-4);
        assert!((get_anchor_right(&tree, id) - 1.0).abs() < 1e-4);
        assert!((get_anchor_bottom(&tree, id) - 0.0).abs() < 1e-4);

        apply_anchor_preset(&mut tree, id, AnchorPreset::LeftWide);
        assert!((get_anchor_left(&tree, id) - 0.0).abs() < 1e-4);
        assert!((get_anchor_right(&tree, id) - 0.0).abs() < 1e-4);
        assert!((get_anchor_bottom(&tree, id) - 1.0).abs() < 1e-4);
    }
}
