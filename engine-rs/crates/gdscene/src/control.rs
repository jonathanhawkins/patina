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

/// Mouse filter mode matching Godot's `Control.MouseFilter`.
///
/// Controls whether a Control receives mouse/touch events during viewport
/// hit-testing and whether those events keep propagating to nodes behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseFilter {
    /// Receives mouse events and stops propagation to controls behind it.
    Stop,
    /// Receives mouse events but also lets them pass to controls behind it.
    Pass,
    /// Ignores mouse events entirely; they pass through to controls behind it.
    Ignore,
}

impl MouseFilter {
    /// Returns the Godot integer representation.
    fn to_int(self) -> i64 {
        match self {
            MouseFilter::Stop => 0,
            MouseFilter::Pass => 1,
            MouseFilter::Ignore => 2,
        }
    }

    /// Converts from Godot integer representation.
    fn from_int(v: i64) -> Self {
        match v {
            1 => MouseFilter::Pass,
            2 => MouseFilter::Ignore,
            _ => MouseFilter::Stop,
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

/// Sets the `"position"` property on a Control node.
pub fn set_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("position", Variant::Vector2(pos));
    }
}

/// Gets the `"position"` property on a Control node, defaulting to [`Vector2::ZERO`].
pub fn get_position(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

// -- Mouse filter / hit-testing ---------------------------------------------

/// Sets the `"mouse_filter"` property on a Control node.
pub fn set_mouse_filter(tree: &mut SceneTree, node_id: NodeId, filter: MouseFilter) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("mouse_filter", Variant::Int(filter.to_int()));
    }
}

/// Gets the `"mouse_filter"` property, defaulting to [`MouseFilter::Stop`]
/// (Godot's default, `MOUSE_FILTER_STOP`).
pub fn get_mouse_filter(tree: &SceneTree, node_id: NodeId) -> MouseFilter {
    tree.get_node(node_id)
        .map(|n| match n.get_property("mouse_filter") {
            Variant::Int(i) => MouseFilter::from_int(i),
            _ => MouseFilter::Stop,
        })
        .unwrap_or(MouseFilter::Stop)
}

/// Returns `true` if `point` lies within the axis-aligned rect at `position`
/// with the given `size` (half-open: includes the top-left edge, excludes the
/// bottom-right edge, matching Godot's `Rect2::has_point`).
pub fn rect_has_point(position: Vector2, size: Vector2, point: Vector2) -> bool {
    point.x >= position.x
        && point.y >= position.y
        && point.x < position.x + size.x
        && point.y < position.y + size.y
}

/// Returns `true` if `point` (in the control's coordinate space) lies within
/// this Control's rect (`position` + `size`). Used for viewport hit-testing
/// before routing a mouse event to the control.
pub fn control_has_point(tree: &SceneTree, node_id: NodeId, point: Vector2) -> bool {
    rect_has_point(get_position(tree, node_id), get_size(tree, node_id), point)
}

/// Recursively finds the topmost (front-most, last-drawn) Control whose global
/// rect contains `point` and whose `mouse_filter` is not [`MouseFilter::Ignore`].
///
/// `parent_global` is the accumulated global position of `node_id`'s parent.
/// Children are drawn on top of their parent, and later children on top of
/// earlier ones, so the search visits children in reverse order first and
/// returns the first (front-most) hit before testing the node itself.
fn hit_test_control(
    tree: &SceneTree,
    node_id: NodeId,
    point: Vector2,
    parent_global: Vector2,
) -> Option<NodeId> {
    let (is_ctrl, children) = match tree.get_node(node_id) {
        Some(n) => (
            n.is_class("Control") || CONTROL_HIT_CLASSES.contains(&n.class_name()),
            n.children().to_vec(),
        ),
        None => return None,
    };
    let global_pos = if is_ctrl {
        parent_global + get_position(tree, node_id)
    } else {
        parent_global
    };
    for &child in children.iter().rev() {
        if let Some(hit) = hit_test_control(tree, child, point, global_pos) {
            return Some(hit);
        }
    }
    if is_ctrl
        && get_mouse_filter(tree, node_id) != MouseFilter::Ignore
        && rect_has_point(global_pos, get_size(tree, node_id), point)
    {
        return Some(node_id);
    }
    None
}

/// Returns the topmost Control at `point` (in the viewport's global coordinate
/// space), respecting `mouse_filter`: [`MouseFilter::Ignore`] controls are
/// transparent to the mouse and skipped. Walks the whole tree from the root in
/// front-to-back draw order. This is the entry point for viewport mouse
/// hit-testing before an event is routed to a control.
pub fn control_at_point(tree: &SceneTree, point: Vector2) -> Option<NodeId> {
    hit_test_control(tree, tree.root_id(), point, Vector2::ZERO)
}

/// Control-family classes recognized during mouse hit-testing. Used as a
/// fallback to ClassDB (`is_class("Control")`) so hit-testing works even when
/// the ClassDB inheritance table has not been populated (e.g. bare unit tests),
/// mirroring [`crate::control_render`]'s control detection.
const CONTROL_HIT_CLASSES: &[&str] = &[
    "Control",
    "Panel",
    "PanelContainer",
    "Label",
    "RichTextLabel",
    "ColorRect",
    "TextureRect",
    "NinePatchRect",
    "BaseButton",
    "Button",
    "CheckBox",
    "CheckButton",
    "OptionButton",
    "MenuButton",
    "LinkButton",
    "TextureButton",
    "LineEdit",
    "TextEdit",
    "HSlider",
    "VSlider",
    "SpinBox",
    "ProgressBar",
    "ScrollBar",
    "HScrollBar",
    "VScrollBar",
    "ItemList",
    "Tree",
    "TabBar",
    "TabContainer",
    "VBoxContainer",
    "HBoxContainer",
    "BoxContainer",
    "GridContainer",
    "MarginContainer",
    "CenterContainer",
    "ScrollContainer",
];

/// Button-family classes whose click emits the `pressed` signal.
const BUTTON_CLASSES: &[&str] = &[
    "BaseButton",
    "Button",
    "CheckBox",
    "CheckButton",
    "OptionButton",
    "MenuButton",
    "LinkButton",
    "TextureButton",
];

/// Returns `true` if the node is a `BaseButton` (or a subclass such as
/// `Button`/`CheckBox`). Checks ClassDB inheritance first, then falls back to a
/// known-class list (mirroring [`crate::control_render`]'s control detection).
fn node_is_base_button(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.is_class("BaseButton") || BUTTON_CLASSES.contains(&n.class_name()))
        .unwrap_or(false)
}

/// Routes a mouse click at `point` (viewport global coordinates) to the topmost
/// Control under it, honoring `mouse_filter`. If that control is a
/// [`BaseButton`](node_is_base_button), emits its `pressed` signal (Godot's
/// default release action). Returns the control that received the click, or
/// `None` if the point is over no control.
///
/// This is the runtime entry point exercised by the app-level test: connecting
/// a callback to a Button's `pressed` signal and calling this on the button's
/// rect fires the callback.
pub fn click_at_point(tree: &mut SceneTree, point: Vector2) -> Option<NodeId> {
    let hit = control_at_point(tree, point)?;
    if node_is_base_button(tree, hit) {
        tree.emit_signal(hit, "pressed", &[]);
    }
    Some(hit)
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

/// Sets the `"columns"` property on a GridContainer.
pub fn set_columns(tree: &mut SceneTree, node_id: NodeId, cols: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("columns", Variant::Int(cols));
    }
}

/// Gets the `"columns"` property, defaulting to `1`.
pub fn get_columns(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("columns") {
            Variant::Int(i) => i,
            _ => 1,
        })
        .unwrap_or(1)
}

// ===========================================================================
// Layout solver
// ===========================================================================

/// Resolves a Control node's final position and size from its anchors,
/// offsets, minimum size, and grow direction given the parent rect size.
///
/// Mirrors Godot's `Control::_compute_anchors_rect` rules:
///   x1 = parent.x * anchor_left  + offset_left
///   x2 = parent.x * anchor_right + offset_right
/// (and likewise for y). When the resolved width/height is less than the
/// effective minimum (componentwise max of `custom_minimum_size` and
/// `min_size`), the rect grows outward per `grow_horizontal`/`grow_vertical`.
pub fn resolve_control_layout(tree: &mut SceneTree, node_id: NodeId, parent: Vector2) {
    let al = get_anchor_left(tree, node_id);
    let at = get_anchor_top(tree, node_id);
    let ar = get_anchor_right(tree, node_id);
    let ab = get_anchor_bottom(tree, node_id);

    let ol = get_offset_left(tree, node_id);
    let ot = get_offset_top(tree, node_id);
    let or_ = get_offset_right(tree, node_id);
    let ob = get_offset_bottom(tree, node_id);

    let mut x1 = parent.x * al + ol;
    let mut y1 = parent.y * at + ot;
    let mut x2 = parent.x * ar + or_;
    let mut y2 = parent.y * ab + ob;

    let custom_min = get_custom_minimum_size(tree, node_id);
    let inner_min = get_min_size(tree, node_id);
    let min_w = custom_min.x.max(inner_min.x);
    let min_h = custom_min.y.max(inner_min.y);

    let width = x2 - x1;
    if width < min_w {
        let delta = min_w - width;
        match get_grow_direction_h(tree, node_id) {
            GrowDirection::End => x2 = x1 + min_w,
            GrowDirection::Begin => x1 = x2 - min_w,
            GrowDirection::Both => {
                x1 -= delta * 0.5;
                x2 = x1 + min_w;
            }
        }
    }

    let height = y2 - y1;
    if height < min_h {
        let delta = min_h - height;
        match get_grow_direction_v(tree, node_id) {
            GrowDirection::End => y2 = y1 + min_h,
            GrowDirection::Begin => y1 = y2 - min_h,
            GrowDirection::Both => {
                y1 -= delta * 0.5;
                y2 = y1 + min_h;
            }
        }
    }

    set_position(tree, node_id, Vector2::new(x1, y1));
    set_size(tree, node_id, Vector2::new(x2 - x1, y2 - y1));
}

fn effective_min(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    let cm = get_custom_minimum_size(tree, node_id);
    let mm = get_min_size(tree, node_id);
    Vector2::new(cm.x.max(mm.x), cm.y.max(mm.y))
}

fn arrange_vbox(tree: &mut SceneTree, children: &[NodeId], total: Vector2, sep: f32) {
    let n = children.len();
    if n == 0 {
        return;
    }
    let mins: Vec<f32> = children.iter().map(|&c| effective_min(tree, c).y).collect();
    let sum_min: f32 = mins.iter().sum();
    let total_sep = sep * (n as f32 - 1.0).max(0.0);
    let extra = (total.y - sum_min - total_sep).max(0.0);

    let expands: Vec<bool> = children
        .iter()
        .map(|&c| get_v_size_flags(tree, c) == SizeFlags::Expand)
        .collect();
    let expand_count = expands.iter().filter(|&&b| b).count();
    let per_expand = if expand_count > 0 {
        extra / expand_count as f32
    } else {
        0.0
    };

    let mut y = 0.0;
    for (i, &c) in children.iter().enumerate() {
        let h = mins[i] + if expands[i] { per_expand } else { 0.0 };
        set_position(tree, c, Vector2::new(0.0, y));
        set_size(tree, c, Vector2::new(total.x, h));
        y += h + sep;
    }
}

fn arrange_hbox(tree: &mut SceneTree, children: &[NodeId], total: Vector2, sep: f32) {
    let n = children.len();
    if n == 0 {
        return;
    }
    let mins: Vec<f32> = children.iter().map(|&c| effective_min(tree, c).x).collect();
    let sum_min: f32 = mins.iter().sum();
    let total_sep = sep * (n as f32 - 1.0).max(0.0);
    let extra = (total.x - sum_min - total_sep).max(0.0);

    let expands: Vec<bool> = children
        .iter()
        .map(|&c| get_h_size_flags(tree, c) == SizeFlags::Expand)
        .collect();
    let expand_count = expands.iter().filter(|&&b| b).count();
    let per_expand = if expand_count > 0 {
        extra / expand_count as f32
    } else {
        0.0
    };

    let mut x = 0.0;
    for (i, &c) in children.iter().enumerate() {
        let w = mins[i] + if expands[i] { per_expand } else { 0.0 };
        set_position(tree, c, Vector2::new(x, 0.0));
        set_size(tree, c, Vector2::new(w, total.y));
        x += w + sep;
    }
}

fn arrange_grid(tree: &mut SceneTree, children: &[NodeId], sep: f32, cols: usize) {
    if children.is_empty() || cols == 0 {
        return;
    }
    let rows = children.len().div_ceil(cols);
    let mins: Vec<Vector2> = children.iter().map(|&c| effective_min(tree, c)).collect();

    let mut col_widths = vec![0f32; cols];
    let mut row_heights = vec![0f32; rows];
    for (i, m) in mins.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        if m.x > col_widths[col] {
            col_widths[col] = m.x;
        }
        if m.y > row_heights[row] {
            row_heights[row] = m.y;
        }
    }

    let mut x_offsets = vec![0f32; cols];
    for c in 1..cols {
        x_offsets[c] = x_offsets[c - 1] + col_widths[c - 1] + sep;
    }
    let mut y_offsets = vec![0f32; rows];
    for r in 1..rows {
        y_offsets[r] = y_offsets[r - 1] + row_heights[r - 1] + sep;
    }

    for (i, &c) in children.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        set_position(tree, c, Vector2::new(x_offsets[col], y_offsets[row]));
        set_size(tree, c, Vector2::new(col_widths[col], row_heights[row]));
    }
}

/// Arranges the children of a container node (`VBoxContainer`,
/// `HBoxContainer`, or `GridContainer`) within the given total size.
///
/// Uses each child's effective minimum size (componentwise max of
/// `custom_minimum_size` and `min_size`), the container's `separation`
/// theme override, and the per-child `size_flags_horizontal` /
/// `size_flags_vertical` to distribute remaining space among `Expand`
/// children. GridContainer additionally reads its `columns` property and
/// sizes each cell to the max of its column's width and its row's height.
pub fn arrange_container(tree: &mut SceneTree, container_id: NodeId, total: Vector2) {
    set_size(tree, container_id, total);
    let class = tree
        .get_node(container_id)
        .map(|n| n.class_name().to_string())
        .unwrap_or_default();
    let children: Vec<NodeId> = tree
        .get_node(container_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    let sep = get_separation(tree, container_id) as f32;

    match class.as_str() {
        "VBoxContainer" => arrange_vbox(tree, &children, total, sep),
        "HBoxContainer" | "BoxContainer" => arrange_hbox(tree, &children, total, sep),
        "GridContainer" => {
            let cols = get_columns(tree, container_id).max(1) as usize;
            arrange_grid(tree, &children, sep, cols);
        }
        "MarginContainer" => {
            let m = get_margins(tree, container_id);
            arrange_margin(tree, &children, total, m);
        }
        "CenterContainer" => arrange_center(tree, &children, total),
        "ScrollContainer" => {
            let scroll = get_scroll(tree, container_id);
            arrange_scroll(tree, &children, total, scroll);
        }
        "TabContainer" => {
            let current = get_current_tab(tree, container_id);
            arrange_tabs(tree, &children, total, current);
        }
        _ => {}
    }
}

/// Returns `true` for the container classes [`arrange_container`] lays out.
pub fn is_container_class_name(class: &str) -> bool {
    matches!(
        class,
        "VBoxContainer"
            | "HBoxContainer"
            | "BoxContainer"
            | "GridContainer"
            | "MarginContainer"
            | "CenterContainer"
            | "ScrollContainer"
            | "TabContainer"
    )
}

/// Recursively arranges `container_id` and every nested container beneath it, so
/// a resize / child change propagates through the whole layout subtree — the
/// "queue_sort" re-sort pipeline (pat-u3l8c.71).
///
/// [`arrange_container`] alone positions and sizes a container's direct
/// children but does not descend into container children (a `MarginContainer`
/// inside a `VBoxContainer` keeps its child unarranged). This walks depth-first:
/// each child that is itself a container is re-arranged against the size its
/// parent just allocated it.
pub fn sort_container_tree(tree: &mut SceneTree, container_id: NodeId, total: Vector2) {
    arrange_container(tree, container_id, total);
    let children: Vec<NodeId> = tree
        .get_node(container_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    for child in children {
        let class = tree
            .get_node(child)
            .map(|n| n.class_name().to_string())
            .unwrap_or_default();
        if is_container_class_name(&class) {
            let child_size = get_size(tree, child);
            sort_container_tree(tree, child, child_size);
        }
    }
}

/// Per-side margins (`left`, `top`, `right`, `bottom`) resolved from a
/// MarginContainer's `theme_override_constants/margin_*` overrides. Each
/// defaults to `0`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Reads the four `theme_override_constants/margin_*` constants of a
/// MarginContainer.
pub fn get_margins(tree: &SceneTree, node_id: NodeId) -> Margins {
    let read = |key: &str| -> f32 {
        tree.get_node(node_id)
            .map(|n| match n.get_property(key) {
                Variant::Int(i) => i as f32,
                Variant::Float(f) => f as f32,
                _ => 0.0,
            })
            .unwrap_or(0.0)
    };
    Margins {
        left: read("theme_override_constants/margin_left"),
        top: read("theme_override_constants/margin_top"),
        right: read("theme_override_constants/margin_right"),
        bottom: read("theme_override_constants/margin_bottom"),
    }
}

/// Sets a single `theme_override_constants/margin_<side>` on a MarginContainer.
/// `side` is one of `"left"`, `"top"`, `"right"`, `"bottom"`.
pub fn set_margin(tree: &mut SceneTree, node_id: NodeId, side: &str, px: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            &format!("theme_override_constants/margin_{side}"),
            Variant::Int(px),
        );
    }
}

/// Gets the `"current_tab"` index of a TabContainer, defaulting to `0`.
pub fn get_current_tab(tree: &SceneTree, node_id: NodeId) -> usize {
    tree.get_node(node_id)
        .map(|n| match n.get_property("current_tab") {
            Variant::Int(i) => i.max(0) as usize,
            _ => 0,
        })
        .unwrap_or(0)
}

/// Sets the `"current_tab"` index of a TabContainer.
pub fn set_current_tab(tree: &mut SceneTree, node_id: NodeId, index: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("current_tab", Variant::Int(index.max(0)));
    }
}

/// Reads a ScrollContainer's `(scroll_horizontal, scroll_vertical)` offsets in
/// pixels, each defaulting to `0`.
pub fn get_scroll(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    let read = |key: &str| -> f32 {
        tree.get_node(node_id)
            .map(|n| match n.get_property(key) {
                Variant::Int(i) => i as f32,
                Variant::Float(f) => f as f32,
                _ => 0.0,
            })
            .unwrap_or(0.0)
    };
    Vector2::new(read("scroll_horizontal"), read("scroll_vertical"))
}

/// Height of a TabContainer's tab-header strip, in pixels. Godot's default
/// theme tab height is ~32px for the built-in font.
pub const TAB_BAR_HEIGHT: f32 = 32.0;

/// MarginContainer: every child fills the container rect inset by the margins.
fn arrange_margin(tree: &mut SceneTree, children: &[NodeId], total: Vector2, m: Margins) {
    let inner = Vector2::new(
        (total.x - m.left - m.right).max(0.0),
        (total.y - m.top - m.bottom).max(0.0),
    );
    for &c in children {
        set_position(tree, c, Vector2::new(m.left, m.top));
        set_size(tree, c, inner);
    }
}

/// CenterContainer: each child keeps its effective minimum size and is centered
/// within the container.
fn arrange_center(tree: &mut SceneTree, children: &[NodeId], total: Vector2) {
    for &c in children {
        let size = effective_min(tree, c);
        let pos = Vector2::new(
            ((total.x - size.x) * 0.5).max(0.0),
            ((total.y - size.y) * 0.5).max(0.0),
        );
        set_position(tree, c, pos);
        set_size(tree, c, size);
    }
}

/// ScrollContainer: the (single) content child is sized to the larger of its
/// minimum size and the viewport, then shifted by the negative scroll offset so
/// content scrolls under the clip rect.
fn arrange_scroll(tree: &mut SceneTree, children: &[NodeId], total: Vector2, scroll: Vector2) {
    for &c in children {
        let min = effective_min(tree, c);
        let size = Vector2::new(min.x.max(total.x), min.y.max(total.y));
        set_position(tree, c, Vector2::new(-scroll.x, -scroll.y));
        set_size(tree, c, size);
    }
}

/// TabContainer: only the child at `current` is visible, filling the area below
/// the tab-header strip. Non-current children are collapsed to zero size.
fn arrange_tabs(tree: &mut SceneTree, children: &[NodeId], total: Vector2, current: usize) {
    let content = Vector2::new(total.x, (total.y - TAB_BAR_HEIGHT).max(0.0));
    for (i, &c) in children.iter().enumerate() {
        if i == current {
            set_position(tree, c, Vector2::new(0.0, TAB_BAR_HEIGHT));
            set_size(tree, c, content);
        } else {
            set_position(tree, c, Vector2::ZERO);
            set_size(tree, c, Vector2::ZERO);
        }
    }
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
// Range value model (Slider / ProgressBar / SpinBox / ScrollBar) — pat-u3l8c.72
// ===========================================================================

/// Reads an `f64` property (accepting `Float`/`Int`), falling back to `default`.
fn get_f64_property(tree: &SceneTree, node_id: NodeId, key: &str, default: f64) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property(key) {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            _ => default,
        })
        .unwrap_or(default)
}

/// `Range.min_value` (Godot default `0.0`).
pub fn get_range_min(tree: &SceneTree, node_id: NodeId) -> f64 {
    get_f64_property(tree, node_id, "min_value", 0.0)
}

/// `Range.max_value` (Godot default `100.0`).
pub fn get_range_max(tree: &SceneTree, node_id: NodeId) -> f64 {
    get_f64_property(tree, node_id, "max_value", 100.0)
}

/// `Range.step` (Godot default `1.0`). A step of `0` disables snapping.
pub fn get_range_step(tree: &SceneTree, node_id: NodeId) -> f64 {
    get_f64_property(tree, node_id, "step", 1.0)
}

/// `Range.page` (Godot default `0.0`) — the scrollbar/slider page size that
/// reduces the effective maximum to `max - page`.
pub fn get_range_page(tree: &SceneTree, node_id: NodeId) -> f64 {
    get_f64_property(tree, node_id, "page", 0.0)
}

/// `Range.value` (Godot default `0.0`).
pub fn get_range_value(tree: &SceneTree, node_id: NodeId) -> f64 {
    get_f64_property(tree, node_id, "value", 0.0)
}

/// Sets `Range.value`, snapping to `step` and clamping to
/// `[min_value, max_value - page]` (Godot `Range::set_value` semantics).
///
/// Returns `true` when the stored value actually changed — the caller emits the
/// `value_changed` signal in that case (Slider drag, SpinBox typing, etc.).
pub fn set_range_value(tree: &mut SceneTree, node_id: NodeId, value: f64) -> bool {
    let min = get_range_min(tree, node_id);
    let max = get_range_max(tree, node_id);
    let step = get_range_step(tree, node_id);
    let page = get_range_page(tree, node_id);
    let old = get_range_value(tree, node_id);

    let mut v = value;
    if step > 0.0 {
        v = min + ((v - min) / step).round() * step;
    }
    let effective_max = (max - page).max(min);
    v = v.clamp(min, effective_max);

    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("value", Variant::Float(v));
    }
    (v - old).abs() > f64::EPSILON
}

/// The value's normalised position in `[0, 1]` within `[min, max]` — the fill
/// fraction used by ProgressBar / Slider grabbers. `0.0` for a degenerate range.
pub fn get_range_ratio(tree: &SceneTree, node_id: NodeId) -> f32 {
    let min = get_range_min(tree, node_id);
    let max = get_range_max(tree, node_id);
    let value = get_range_value(tree, node_id);
    let span = max - min;
    if span.abs() < 1e-9 {
        return 0.0;
    }
    (((value - min) / span) as f32).clamp(0.0, 1.0)
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

    // -- Range value model (pat-u3l8c.72) -----------------------------------

    #[test]
    fn range_value_clamps_and_reports_change() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "S", "HSlider");
        // Defaults: min 0, max 100, step 1.
        assert!(set_range_value(&mut tree, id, 50.0), "0 -> 50 is a change");
        assert_eq!(get_range_value(&tree, id), 50.0);
        // Re-setting the same value reports no change.
        assert!(!set_range_value(&mut tree, id, 50.0));
        // Clamp above the max and below the min.
        set_range_value(&mut tree, id, 250.0);
        assert_eq!(get_range_value(&tree, id), 100.0);
        set_range_value(&mut tree, id, -20.0);
        assert_eq!(get_range_value(&tree, id), 0.0);
    }

    #[test]
    fn range_value_snaps_to_step() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "S", "HSlider");
        tree.get_node_mut(id)
            .unwrap()
            .set_property("step", Variant::Float(5.0));
        set_range_value(&mut tree, id, 12.0);
        assert_eq!(get_range_value(&tree, id), 10.0); // nearest multiple of 5
    }

    #[test]
    fn range_ratio_is_normalized_position() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "P", "ProgressBar");
        set_range_value(&mut tree, id, 25.0);
        assert!((get_range_ratio(&tree, id) - 0.25).abs() < 1e-4);
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
    fn mouse_filter_default_is_stop() {
        let tree = make_tree();
        assert_eq!(get_mouse_filter(&tree, tree.root_id()), MouseFilter::Stop);
    }

    #[test]
    fn set_get_mouse_filter() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Panel", "Control");
        set_mouse_filter(&mut tree, id, MouseFilter::Pass);
        assert_eq!(get_mouse_filter(&tree, id), MouseFilter::Pass);
        set_mouse_filter(&mut tree, id, MouseFilter::Ignore);
        assert_eq!(get_mouse_filter(&tree, id), MouseFilter::Ignore);
    }

    #[test]
    fn rect_has_point_is_half_open() {
        let pos = Vector2::new(10.0, 20.0);
        let size = Vector2::new(100.0, 50.0);
        assert!(rect_has_point(pos, size, Vector2::new(10.0, 20.0))); // top-left inclusive
        assert!(rect_has_point(pos, size, Vector2::new(50.0, 40.0)));
        assert!(!rect_has_point(pos, size, Vector2::new(110.0, 40.0))); // right edge exclusive
        assert!(!rect_has_point(pos, size, Vector2::new(50.0, 70.0))); // bottom edge exclusive
        assert!(!rect_has_point(pos, size, Vector2::new(9.0, 40.0)));
    }

    #[test]
    fn control_has_point_uses_position_and_size() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Btn", "Button");
        set_position(&mut tree, id, Vector2::new(5.0, 5.0));
        set_size(&mut tree, id, Vector2::new(20.0, 10.0));
        assert!(control_has_point(&tree, id, Vector2::new(6.0, 6.0)));
        assert!(!control_has_point(&tree, id, Vector2::new(30.0, 6.0)));
    }

    #[test]
    fn control_at_point_finds_topmost() {
        let mut tree = make_tree();
        let back = add_control(&mut tree, "Back", "Control");
        set_position(&mut tree, back, Vector2::new(0.0, 0.0));
        set_size(&mut tree, back, Vector2::new(100.0, 100.0));
        let front = add_control(&mut tree, "Front", "Control");
        set_position(&mut tree, front, Vector2::new(10.0, 10.0));
        set_size(&mut tree, front, Vector2::new(50.0, 50.0));

        // Inside both → the later-added (front-most) control wins.
        assert_eq!(
            control_at_point(&tree, Vector2::new(20.0, 20.0)),
            Some(front)
        );
        // Inside only the back control.
        assert_eq!(
            control_at_point(&tree, Vector2::new(80.0, 80.0)),
            Some(back)
        );
        // Outside both.
        assert_eq!(control_at_point(&tree, Vector2::new(200.0, 200.0)), None);
    }

    #[test]
    fn control_at_point_respects_mouse_filter_ignore() {
        let mut tree = make_tree();
        let back = add_control(&mut tree, "Back", "Control");
        set_position(&mut tree, back, Vector2::new(0.0, 0.0));
        set_size(&mut tree, back, Vector2::new(100.0, 100.0));
        let front = add_control(&mut tree, "Front", "Control");
        set_position(&mut tree, front, Vector2::new(0.0, 0.0));
        set_size(&mut tree, front, Vector2::new(100.0, 100.0));
        set_mouse_filter(&mut tree, front, MouseFilter::Ignore);

        // Front ignores the mouse → the event falls through to the back control.
        assert_eq!(
            control_at_point(&tree, Vector2::new(20.0, 20.0)),
            Some(back)
        );
    }

    #[test]
    fn control_at_point_uses_global_position_of_nested_child() {
        let mut tree = make_tree();
        let parent = add_control(&mut tree, "Parent", "Control");
        set_position(&mut tree, parent, Vector2::new(50.0, 50.0));
        set_size(&mut tree, parent, Vector2::new(100.0, 100.0));
        let child = tree
            .add_child(parent, Node::new("Child", "Control"))
            .unwrap();
        set_position(&mut tree, child, Vector2::new(10.0, 10.0));
        set_size(&mut tree, child, Vector2::new(20.0, 20.0));

        // Child global rect is (60,60)..(80,80).
        assert_eq!(
            control_at_point(&tree, Vector2::new(65.0, 65.0)),
            Some(child)
        );
        // Inside parent but outside child.
        assert_eq!(
            control_at_point(&tree, Vector2::new(55.0, 55.0)),
            Some(parent)
        );
    }

    #[test]
    fn click_on_button_emits_pressed() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = make_tree();
        let btn = add_control(&mut tree, "Btn", "Button");
        set_position(&mut tree, btn, Vector2::new(10.0, 10.0));
        set_size(&mut tree, btn, Vector2::new(40.0, 20.0));

        let count = Arc::new(AtomicUsize::new(0));
        let count_cb = Arc::clone(&count);
        let conn = gdobject::signal::Connection::with_callback(
            btn.object_id(),
            "on_pressed",
            move |_args| {
                count_cb.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            },
        );
        tree.connect_signal(btn, "pressed", conn);

        // Clicking inside the button emits `pressed`; the observer fires once.
        assert_eq!(
            click_at_point(&mut tree, Vector2::new(20.0, 15.0)),
            Some(btn)
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // A click outside the button emits nothing.
        assert_eq!(click_at_point(&mut tree, Vector2::new(200.0, 200.0)), None);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clicking_button_emits_pressed_observed_from_gdscript() {
        // App-level test: a GDScript-connected handler observes a Button's
        // `pressed` signal when the button is clicked via viewport hit-testing.
        let mut tree = make_tree();
        let root = tree.root_id();

        let btn = tree.add_child(root, Node::new("Btn", "Button")).unwrap();
        set_position(&mut tree, btn, Vector2::new(10.0, 10.0));
        set_size(&mut tree, btn, Vector2::new(80.0, 30.0));

        // Listener node whose GDScript observes the button's `pressed`.
        let listener = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
        let script = "\
extends Node
var clicked = false
func _on_pressed():
    self.clicked = true
";
        let inst = crate::scripting::GDScriptNodeInstance::from_source(script, listener).unwrap();
        tree.attach_script(listener, Box::new(inst));

        // Connect the Button's `pressed` to the listener's method (script dispatch).
        tree.connect_signal(
            btn,
            "pressed",
            gdobject::signal::Connection::new(listener.object_id(), "_on_pressed"),
        );

        // Simulate a mouse click inside the button's rect.
        click_at_point(&mut tree, Vector2::new(20.0, 20.0));

        // GDScript observed the press via the emitted `pressed` signal.
        assert_eq!(
            tree.get_script(listener).unwrap().get_property("clicked"),
            Some(Variant::Bool(true))
        );
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

    // -- Containers: Margin / Center / Scroll / Tab -------------------------

    /// Adds a container of `class` under root and `n` plain Control children.
    fn container_with_children(class: &str, n: usize) -> (SceneTree, NodeId, Vec<NodeId>) {
        let mut tree = make_tree();
        let root = tree.root_id();
        let cid = tree.add_child(root, Node::new("C", class)).unwrap();
        let kids: Vec<NodeId> = (0..n)
            .map(|i| {
                tree.add_child(cid, Node::new(&format!("k{i}"), "Control"))
                    .unwrap()
            })
            .collect();
        (tree, cid, kids)
    }

    #[test]
    fn margin_container_insets_child() {
        let (mut tree, cid, kids) = container_with_children("MarginContainer", 1);
        set_margin(&mut tree, cid, "left", 10);
        set_margin(&mut tree, cid, "top", 5);
        set_margin(&mut tree, cid, "right", 20);
        set_margin(&mut tree, cid, "bottom", 15);

        arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

        let pos = get_position(&tree, kids[0]);
        let size = get_size(&tree, kids[0]);
        assert_eq!(pos, Vector2::new(10.0, 5.0));
        // 200 - 10 - 20 = 170, 100 - 5 - 15 = 80.
        assert_eq!(size, Vector2::new(170.0, 80.0));
    }

    #[test]
    fn center_container_centers_child_at_min_size() {
        let (mut tree, cid, kids) = container_with_children("CenterContainer", 1);
        set_custom_minimum_size(&mut tree, kids[0], Vector2::new(40.0, 20.0));

        arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

        let pos = get_position(&tree, kids[0]);
        let size = get_size(&tree, kids[0]);
        assert_eq!(size, Vector2::new(40.0, 20.0));
        // (200-40)/2 = 80, (100-20)/2 = 40.
        assert_eq!(pos, Vector2::new(80.0, 40.0));
    }

    #[test]
    fn scroll_container_shifts_content_by_offset() {
        let (mut tree, cid, kids) = container_with_children("ScrollContainer", 1);
        set_custom_minimum_size(&mut tree, kids[0], Vector2::new(500.0, 400.0));
        tree.get_node_mut(cid)
            .unwrap()
            .set_property("scroll_vertical", Variant::Int(120));

        arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

        let pos = get_position(&tree, kids[0]);
        let size = get_size(&tree, kids[0]);
        // Content larger than viewport keeps its min size, shifted up by scroll.
        assert_eq!(size, Vector2::new(500.0, 400.0));
        assert_eq!(pos, Vector2::new(0.0, -120.0));
    }

    #[test]
    fn sort_container_tree_arranges_nested_containers() {
        // Outer MarginContainer(10) > Inner MarginContainer(5) > Control child.
        let mut tree = make_tree();
        let root = tree.root_id();
        let outer = tree
            .add_child(root, Node::new("Outer", "MarginContainer"))
            .unwrap();
        let inner = tree
            .add_child(outer, Node::new("Inner", "MarginContainer"))
            .unwrap();
        let child = tree
            .add_child(inner, Node::new("Child", "Control"))
            .unwrap();
        for side in ["left", "top", "right", "bottom"] {
            set_margin(&mut tree, outer, side, 10);
            set_margin(&mut tree, inner, side, 5);
        }

        sort_container_tree(&mut tree, outer, Vector2::new(200.0, 100.0));

        // Outer insets the inner container by 10 on each side.
        assert_eq!(get_position(&tree, inner), Vector2::new(10.0, 10.0));
        assert_eq!(get_size(&tree, inner), Vector2::new(180.0, 80.0));
        // The recursion then arranges the inner container, insetting its child
        // by 5 — without recursion the child would stay unarranged.
        assert_eq!(get_position(&tree, child), Vector2::new(5.0, 5.0));
        assert_eq!(get_size(&tree, child), Vector2::new(170.0, 70.0));
    }

    #[test]
    fn tab_container_shows_only_current_tab() {
        let (mut tree, cid, kids) = container_with_children("TabContainer", 3);
        set_current_tab(&mut tree, cid, 1);

        arrange_container(&mut tree, cid, Vector2::new(300.0, 200.0));

        // Non-current tabs collapse to zero; current tab fills below the header.
        assert_eq!(get_size(&tree, kids[0]), Vector2::ZERO);
        assert_eq!(get_size(&tree, kids[2]), Vector2::ZERO);
        let cur_size = get_size(&tree, kids[1]);
        let cur_pos = get_position(&tree, kids[1]);
        assert_eq!(cur_pos, Vector2::new(0.0, TAB_BAR_HEIGHT));
        assert_eq!(cur_size, Vector2::new(300.0, 200.0 - TAB_BAR_HEIGHT));
    }
}
