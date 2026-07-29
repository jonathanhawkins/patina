//! UI/Control node property helpers for Control, Label, Button, TextureRect,
//! LineEdit, Panel, and container nodes.
//!
//! Follows the same pattern as [`node2d`](crate::node2d): typed helper
//! functions that read and write well-known properties on nodes stored in
//! a [`SceneTree`].

use gdcore::math::{Color, Vector2};
use gdresource::bit_map::BitMap;
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

/// Size flags matching Godot's `Control.SizeFlags` — a **bitmask**, not an
/// exclusive enum (upstream/godot/scene/gui/control.h `SizeFlags`).
///
/// Godot tests individual bits (`get_h_size_flags().has_flag(SIZE_EXPAND)`), so
/// combinations like `EXPAND` alone (2), `EXPAND | SHRINK_CENTER` (6) or the
/// `0` default (`SHRINK_BEGIN`) all carry meaning. Modelling this as an
/// exclusive enum collapsed every un-listed combination onto `Fill`, so an
/// `EXPAND`-only child never expanded (pat-0q2kx.83).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord, Hash)]
pub struct SizeFlags(i64);

impl SizeFlags {
    /// No bits set — Godot's `SIZE_SHRINK_BEGIN`: minimum size, packed at the
    /// start of the cell.
    pub const SHRINK_BEGIN: SizeFlags = SizeFlags(0);
    /// `SIZE_FILL` (1): stretch to fill the cell the container allotted.
    pub const FILL: SizeFlags = SizeFlags(1);
    /// `SIZE_EXPAND` (2): claim a share of the container's leftover space.
    pub const EXPAND: SizeFlags = SizeFlags(2);
    /// `SIZE_EXPAND_FILL` (3) — the editor's "Expand Fill", i.e. `EXPAND|FILL`.
    pub const EXPAND_FILL: SizeFlags = SizeFlags(3);
    /// `SIZE_SHRINK_CENTER` (4): minimum size, centered in the cell.
    pub const SHRINK_CENTER: SizeFlags = SizeFlags(4);
    /// `SIZE_SHRINK_END` (8): minimum size, pushed to the end of the cell.
    pub const SHRINK_END: SizeFlags = SizeFlags(8);

    /// Returns the Godot integer representation for this mask.
    pub const fn to_int(self) -> i64 {
        self.0
    }

    /// Converts from Godot's integer representation, preserving every bit.
    pub const fn from_int(v: i64) -> Self {
        SizeFlags(v)
    }

    /// Whether every bit of `flag` is set, mirroring Godot's
    /// `BitField::has_flag`. Always `false` for the empty mask.
    pub const fn has_flag(self, flag: SizeFlags) -> bool {
        flag.0 != 0 && (self.0 & flag.0) == flag.0
    }
}

impl std::ops::BitOr for SizeFlags {
    type Output = SizeFlags;

    fn bitor(self, rhs: SizeFlags) -> SizeFlags {
        SizeFlags(self.0 | rhs.0)
    }
}

/// `BoxContainer.alignment` — where the child run is packed when the container
/// has leftover space and nothing stretched
/// (upstream/godot/scene/gui/box_container.cpp `_resort`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoxAlignment {
    /// Pack at the start of the axis (Godot default).
    #[default]
    Begin,
    /// Center the run within the container.
    Center,
    /// Pack at the end of the axis.
    End,
}

impl BoxAlignment {
    /// Returns the Godot integer representation.
    pub const fn to_int(self) -> i64 {
        match self {
            BoxAlignment::Begin => 0,
            BoxAlignment::Center => 1,
            BoxAlignment::End => 2,
        }
    }

    /// Converts from Godot integer representation (unknown values → `Begin`).
    pub const fn from_int(v: i64) -> Self {
        match v {
            1 => BoxAlignment::Center,
            2 => BoxAlignment::End,
            _ => BoxAlignment::Begin,
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

/// Stretch mode for `TextureRect` nodes — Godot's `TextureRect.StretchMode`
/// (`scene/gui/texture_rect.h`), all seven constants in declaration order.
///
/// ⚠️ **These integers were wrong before pat-0q2kx.179.** The table read
/// `Keep => 1`, `KeepAspect => 5`, `KeepAspectCentered => 6` and had no `Tile`,
/// `KeepCentered` or `KeepAspectCovered` at all — so **every non-zero
/// `stretch_mode` in a `.tscn` was misread**: `1` (TILE) became `Keep`, `4`
/// (KEEP_ASPECT) fell through to `Scale`, `5` became `KeepAspect` and `6` became
/// `KeepAspectCentered`. A wrong constant table is worse than a missing feature,
/// because it silently renders the wrong thing instead of nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StretchMode {
    /// `STRETCH_SCALE` = 0 — scale to fill the rect. Godot's default.
    #[default]
    Scale,
    /// `STRETCH_TILE` = 1 — repeat the texture at its natural size.
    Tile,
    /// `STRETCH_KEEP` = 2 — draw at original size, anchored top-left.
    Keep,
    /// `STRETCH_KEEP_CENTERED` = 3 — original size, centred in the rect.
    KeepCentered,
    /// `STRETCH_KEEP_ASPECT` = 4 — fit inside the rect, anchored top-left.
    KeepAspect,
    /// `STRETCH_KEEP_ASPECT_CENTERED` = 5 — fit inside the rect, centred.
    KeepAspectCentered,
    /// `STRETCH_KEEP_ASPECT_COVERED` = 6 — cover the rect, cropping overflow.
    KeepAspectCovered,
}

impl StretchMode {
    /// The Godot integer constant for this mode.
    pub fn to_int(self) -> i64 {
        match self {
            StretchMode::Scale => 0,
            StretchMode::Tile => 1,
            StretchMode::Keep => 2,
            StretchMode::KeepCentered => 3,
            StretchMode::KeepAspect => 4,
            StretchMode::KeepAspectCentered => 5,
            StretchMode::KeepAspectCovered => 6,
        }
    }

    /// Reads Godot's integer constant, falling back to `Scale` (the documented
    /// default) for anything out of range.
    pub fn from_int(v: i64) -> Self {
        match v {
            1 => StretchMode::Tile,
            2 => StretchMode::Keep,
            3 => StretchMode::KeepCentered,
            4 => StretchMode::KeepAspect,
            5 => StretchMode::KeepAspectCentered,
            6 => StretchMode::KeepAspectCovered,
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
            _ => default_mouse_filter(n),
        })
        .unwrap_or(MouseFilter::Stop)
}

/// The `mouse_filter` a class is born with, for a node that carries no explicit
/// property (pat-s3rrv.13).
///
/// Godot sets these in each class's **constructor** and therefore never
/// serializes them, so a `.tscn`-authored Label arrives with no `mouse_filter`
/// at all. Defaulting everything to STOP made that Label swallow every click
/// meant for whatever sits behind it — the single most visible symptom of this
/// gap, since a full-rect Label over a Button is an ordinary layout.
///
/// Sources: `scene/gui/label.cpp:1477` (IGNORE), `scene/gui/container.cpp:233`
/// and `scene/gui/texture_rect.cpp:261` (PASS), `scene/gui/panel.cpp:56` (STOP).
///
/// Container subclasses are listed by name **as well as** via the `Container`
/// chain check, because `is_class`'s ancestor lookup goes through ClassDB, which
/// is not always populated in unit tests — the explicit names keep the default
/// correct either way.
fn default_mouse_filter(node: &crate::node::Node) -> MouseFilter {
    const IGNORE: &[&str] = &[
        "Label",
        "RichTextLabel",
        "Separator",
        "HSeparator",
        "VSeparator",
    ];
    const PASS: &[&str] = &[
        "Container",
        "BoxContainer",
        "HBoxContainer",
        "VBoxContainer",
        "GridContainer",
        "CenterContainer",
        "MarginContainer",
        "PanelContainer",
        "ScrollContainer",
        "SplitContainer",
        "HSplitContainer",
        "VSplitContainer",
        "TabContainer",
        "FlowContainer",
        "HFlowContainer",
        "VFlowContainer",
        "AspectRatioContainer",
        "SubViewportContainer",
        "TextureRect",
        "NinePatchRect",
        "ColorRect",
    ];
    if IGNORE.iter().any(|c| node.is_class(c)) {
        MouseFilter::Ignore
    } else if PASS.iter().any(|c| node.is_class(c)) {
        MouseFilter::Pass
    } else {
        MouseFilter::Stop
    }
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
///
/// For a `TextureButton` carrying a `texture_click_mask`, the rect test is only
/// the first half — see [`texture_button_mask_allows_point`].
pub fn control_has_point(tree: &SceneTree, node_id: NodeId, point: Vector2) -> bool {
    let position = get_position(tree, node_id);
    if !rect_has_point(position, get_size(tree, node_id), point) {
        return false;
    }
    texture_button_mask_allows_point(tree, node_id, point - position)
}

/// Resolves a `TextureButton`'s `texture_click_mask` into a [`BitMap`], or
/// `None` when the node is not a TextureButton or carries no mask
/// (pat-0q2kx.257).
///
/// Three authoring paths reach the same mask, and all three are accepted because
/// each is what some real caller produces:
///
/// 1. A `.tscn` writes `texture_click_mask = SubResource("id")`, which the parser
///    leaves as the string `"SubResource:<id>"` — the bits live on the
///    sub-resource, copied onto the node as `__click_mask__data` at instancing
///    (see `packed_scene.rs`). This is checked first, because the string in
///    `texture_click_mask` itself carries no bits.
/// 2. A `.tres`/script-assigned `BitMap` arrives as `Variant::Resource`, with the
///    payload under the resource's own `data` property.
/// 3. A hand-built node (and most tests) can put the `data` dictionary straight
///    on the property.
pub fn texture_button_click_mask(tree: &SceneTree, node_id: NodeId) -> Option<BitMap> {
    let node = tree.get_node(node_id)?;
    // The mask is a TextureButton-only property; every other Control must keep
    // its plain rect test, so this gate is what stops the mask logic from
    // leaking into the rest of the UI.
    if !(node.is_class("TextureButton") || node.class_name() == "TextureButton") {
        return None;
    }
    if let Variant::Dictionary(d) = node.get_property("__click_mask__data") {
        return BitMap::from_data_dict(&d);
    }
    match node.get_property("texture_click_mask") {
        Variant::Resource(r) if r.class_name == "BitMap" => match r.properties.get("data") {
            Some(Variant::Dictionary(d)) => BitMap::from_data_dict(d),
            _ => None,
        },
        Variant::Dictionary(d) => BitMap::from_data_dict(&d),
        _ => None,
    }
}

/// Whether a `TextureButton`'s click mask admits `local_point` (a point already
/// relative to the control's own top-left corner).
///
/// Returns `true` for every node without a mask, so this composes with a plain
/// rect test: the mask can only ever *reject* a point the rect accepted, which
/// is how Godot's `TextureButton::has_point` overrides `Control::has_point`
/// (`scene/gui/texture_button.cpp:62`).
///
/// The point is scaled from the control's rect into the mask's resolution, which
/// is Godot's general case (`scale = mask_size / _position_rect.size`); a control
/// with no area uses the mask 1:1, as Godot does when `_position_rect` is empty.
///
/// ⚠️ Godot additionally special-cases `STRETCH_TILE` (wrapping the point modulo
/// the mask) and `STRETCH_KEEP_ASPECT_COVERED` (offsetting by the texture
/// region). Neither is mapped here: Patina's TextureButton does not yet model
/// `stretch_mode` or a texture region at all, so there is nothing to read — and
/// inventing a mapping for a mode the renderer does not implement would encode a
/// guess as parity. The default `STRETCH_SCALE` path is exact.
pub fn texture_button_mask_allows_point(
    tree: &SceneTree,
    node_id: NodeId,
    local_point: Vector2,
) -> bool {
    let Some(mask) = texture_button_click_mask(tree, node_id) else {
        return true;
    };
    let mask_size = mask.get_size();
    // An empty mask cannot answer the question; falling through to the rect test
    // keeps a button with a blank mask clickable rather than silently dead.
    if mask_size.x <= 0 || mask_size.y <= 0 {
        return true;
    }
    let control_size = get_size(tree, node_id);
    let (scale_x, scale_y) = if control_size.x > 0.0 && control_size.y > 0.0 {
        (
            mask_size.x as f32 / control_size.x,
            mask_size.y as f32 / control_size.y,
        )
    } else {
        (1.0, 1.0)
    };
    let mx = (local_point.x * scale_x).floor() as i32;
    let my = (local_point.y * scale_y).floor() as i32;
    // Godot bails out before sampling when the mapped point leaves the mask.
    if mx < 0 || my < 0 || mx >= mask_size.x || my >= mask_size.y {
        return false;
    }
    mask.get_bit(mx, my)
}

/// Whether `node_id` participates in Control layout — a real `Control` subclass
/// per ClassDB, or one of the known Control classes when the ClassDB chain is
/// unavailable (the same fallback the hit test uses).
pub fn node_is_control(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.is_class("Control") || CONTROL_HIT_CLASSES.contains(&n.class_name()))
        .unwrap_or(false)
}

/// The control's position in viewport (global) space — its own `position` plus
/// every Control ancestor's, which is exactly what [`hit_test_control`]
/// accumulates on the way down. Non-control ancestors contribute nothing, so a
/// Control parented under a plain `Node` keeps its own coordinates.
pub fn get_global_position(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    let mut pos = Vector2::ZERO;
    let mut cursor = Some(node_id);
    while let Some(id) = cursor {
        if node_is_control(tree, id) {
            pos = pos + get_position(tree, id);
        }
        cursor = tree.get_node(id).and_then(|n| n.parent());
    }
    pos
}

/// The control's rect in viewport (global) space. This is the rect a click is
/// tested against, so anything mapping a click onto sub-parts of a control
/// (rows, tabs) must measure from here.
pub fn get_global_rect(tree: &SceneTree, node_id: NodeId) -> gdcore::math::Rect2 {
    gdcore::math::Rect2::new(get_global_position(tree, node_id), get_size(tree, node_id))
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
        Some(n) => {
            // Godot's `_gui_find_control_at_pos` rejects an invisible CanvasItem
            // outright, before any rect or filter test (viewport.cpp:1819-1861).
            // Patina never consulted visibility, so a hidden Panel or a closed
            // menu still swallowed every click over its rect (pat-s3rrv.13).
            //
            // The node's OWN flag is the right one here, not `is_visible_in_tree`:
            // this walk is top-down, so an invisible ancestor has already
            // returned before we reach its children.
            if matches!(n.get_property("visible"), Variant::Bool(false)) {
                return None;
            }
            (
                n.is_class("Control") || CONTROL_HIT_CLASSES.contains(&n.class_name()),
                n.children().to_vec(),
            )
        }
        None => return None,
    };
    let global_pos = if is_ctrl {
        parent_global + get_position(tree, node_id)
    } else {
        parent_global
    };
    // A `SubViewportContainer` hosts a whole sub-scene drawn at `1/stretch_shrink`
    // scale, so a click inside it must be re-targeted into the sub-viewport's own
    // coordinate space before the children are tested — otherwise a Button that
    // renders at half scale is only hit at its unscaled position, i.e. never
    // where the user actually clicked (pat-0q2kx.178).
    //
    // `subviewport_container_to_viewport_coord` already existed for exactly this
    // and had no caller outside its own test.
    let mut child_point = if is_subviewport_container(tree, node_id) {
        let (stretch, shrink) = subviewport_container_stretch(tree, node_id);
        global_pos + subviewport_container_to_viewport_coord(point - global_pos, stretch, shrink)
    } else {
        point
    };
    // Godot's `Control::has_point` maps the query through the INVERSE of the
    // control's transform, so a rotated/scaled control is hit where it is drawn
    // rather than where its unrotated rect used to be
    // (`control.cpp::get_transform`). Patina compared against the axis-aligned
    // rect only, so a 45°-rotated panel still swallowed clicks in its old
    // corners and missed them along its new diagonal (pat-0q2kx.181).
    //
    // The mapping applies to this control's own rect test AND to everything
    // beneath it, since children inherit the parent's frame.
    if is_ctrl {
        child_point =
            global_pos + inverse_control_transform(tree, node_id, child_point - global_pos);
    }
    // A control that clips its contents cannot be hit *through* — Godot refuses
    // to descend into children when the point misses the clipper's own rect
    // (viewport.cpp:1819-1861), because anything out there is not drawn. Patina
    // read `clip_contents` only in the render pass, so a child scrolled outside
    // a clipping parent was still clickable where nothing was visible.
    let clips = is_ctrl
        && tree
            .get_node(node_id)
            .is_some_and(|n| matches!(n.get_property("clip_contents"), Variant::Bool(true)));
    if !clips || rect_has_point(global_pos, get_size(tree, node_id), child_point) {
        for &child in children.iter().rev() {
            // `top_level` detaches a node from its parent's transform, so Godot
            // excludes it from the parent's hit walk — it is picked up at the
            // viewport level instead, not underneath this control.
            if tree
                .get_node(child)
                .is_some_and(|n| matches!(n.get_property("top_level"), Variant::Bool(true)))
            {
                continue;
            }
            if let Some(hit) = hit_test_control(tree, child, child_point, global_pos) {
                return Some(hit);
            }
        }
    }
    // A TextureButton's click mask makes the node transparent to clicks over its
    // clear pixels: returning `None` here lets the search continue outward, so
    // the click reaches whatever sits behind the button rather than being
    // swallowed by its bounding box (pat-0q2kx.257).
    if is_ctrl
        && get_mouse_filter(tree, node_id) != MouseFilter::Ignore
        && rect_has_point(global_pos, get_size(tree, node_id), child_point)
        && texture_button_mask_allows_point(tree, node_id, child_point - global_pos)
    {
        return Some(node_id);
    }
    None
}

/// Maps a control-local point through the INVERSE of the control's
/// rotation/scale about its `pivot_offset` (pat-0q2kx.181).
///
/// Godot builds the forward transform as
/// `translate(pivot) * rotate(rotation) * scale(scale) * translate(-pivot)`,
/// so the inverse un-scales and un-rotates about the same pivot. `rotation` is
/// stored in RADIANS on the node (Godot exposes `rotation_degrees` separately).
///
/// Returns `local` unchanged for an untransformed control, which keeps the
/// common case exact rather than routing it through float trig.
fn inverse_control_transform(tree: &SceneTree, node_id: NodeId, local: Vector2) -> Vector2 {
    let Some(node) = tree.get_node(node_id) else {
        return local;
    };
    let rotation = match node.get_property("rotation") {
        Variant::Float(v) => v as f32,
        Variant::Int(v) => v as f32,
        _ => 0.0,
    };
    let scale = match node.get_property("scale") {
        Variant::Vector2(v) if v.x != 0.0 && v.y != 0.0 => v,
        // A zero scale is degenerate — dividing by it would produce NaN and make
        // every later comparison false, silently swallowing the click.
        _ => Vector2::new(1.0, 1.0),
    };
    if rotation == 0.0 && scale.x == 1.0 && scale.y == 1.0 {
        return local;
    }
    let pivot = match node.get_property("pivot_offset") {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    };

    let v = local - pivot;
    // Un-rotate, then un-scale — the reverse order of the forward transform.
    let (sin, cos) = (-rotation).sin_cos();
    let rotated = Vector2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    Vector2::new(rotated.x / scale.x, rotated.y / scale.y) + pivot
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
    // Container <- Control in Godot, and its absence here was its own bug: with
    // ClassDB unpopulated the container was invisible to hit-testing, so its
    // rect never contributed a global offset and the sub-scene beneath it could
    // not be reached at all (pat-0q2kx.178).
    "SubViewportContainer",
    "FlowContainer",
    "HFlowContainer",
    "VFlowContainer",
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
pub fn node_is_base_button(tree: &SceneTree, node_id: NodeId) -> bool {
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
    // An item view resolves the click further, onto the row or tab under the
    // point (pat-0q2kx.85).
    crate::item_views::item_view_click(tree, hit, point, false);
    Some(hit)
}

/// [`click_at_point`] as a double-click: on an `ItemList` row this is Godot's
/// `item_activated` action (`item_list.cpp` `gui_input`), which a single click
/// never fires.
pub fn double_click_at_point(tree: &mut SceneTree, point: Vector2) -> Option<NodeId> {
    let hit = control_at_point(tree, point)?;
    crate::item_views::item_view_click(tree, hit, point, true);
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

/// Gets the horizontal size flags, defaulting to [`SizeFlags::FILL`].
/// A Control's layout direction, mirroring Godot's
/// `Control.LayoutDirection` (pat-nzgdu.142).
///
/// `Inherited` takes the direction from the parent chain; `Locale` follows the
/// application locale (treated as LTR until a locale service exists);
/// `Ltr`/`Rtl` force a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutDirection {
    /// Inherit from the parent Control (Godot default).
    #[default]
    Inherited,
    /// Follow the application locale.
    Locale,
    /// Force left-to-right.
    Ltr,
    /// Force right-to-left.
    Rtl,
}

impl LayoutDirection {
    /// Godot's `Control.LayoutDirection` integer (0..=3).
    pub fn from_int(v: i64) -> Self {
        match v {
            1 => Self::Locale,
            2 => Self::Ltr,
            3 => Self::Rtl,
            _ => Self::Inherited,
        }
    }

    /// The Godot integer for this direction.
    pub fn to_int(self) -> i64 {
        match self {
            Self::Inherited => 0,
            Self::Locale => 1,
            Self::Ltr => 2,
            Self::Rtl => 3,
        }
    }
}

/// Sets a Control's layout direction (Godot's `Control.layout_direction`).
pub fn set_layout_direction(tree: &mut SceneTree, node_id: NodeId, dir: LayoutDirection) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("layout_direction", Variant::Int(dir.to_int()));
    }
}

/// Returns a Control's own `layout_direction` (without resolving `Inherited`).
pub fn get_layout_direction(tree: &SceneTree, node_id: NodeId) -> LayoutDirection {
    tree.get_node(node_id)
        .map(|n| match n.get_property("layout_direction") {
            Variant::Int(i) => LayoutDirection::from_int(i),
            _ => LayoutDirection::Inherited,
        })
        .unwrap_or_default()
}

/// Whether `node_id` lays out right-to-left, resolving `Inherited` up the
/// parent chain (Godot's `Control.is_layout_rtl()`).
///
/// `Locale` resolves to LTR until a locale service exists; an unset chain is
/// LTR, so existing scenes are unaffected.
pub fn is_layout_rtl(tree: &SceneTree, node_id: NodeId) -> bool {
    let mut current = Some(node_id);
    while let Some(id) = current {
        match get_layout_direction(tree, id) {
            LayoutDirection::Rtl => return true,
            LayoutDirection::Ltr => return false,
            // Locale has no locale service yet -> LTR, matching Godot's
            // behaviour for an LTR locale.
            LayoutDirection::Locale => return false,
            LayoutDirection::Inherited => {
                current = tree.get_node(id).and_then(|n| n.parent());
            }
        }
    }
    false
}

pub fn get_h_size_flags(tree: &SceneTree, node_id: NodeId) -> SizeFlags {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size_flags_horizontal") {
            Variant::Int(i) => SizeFlags::from_int(i),
            _ => SizeFlags::FILL,
        })
        .unwrap_or(SizeFlags::FILL)
}

/// Sets the vertical size flags on a Control node.
pub fn set_v_size_flags(tree: &mut SceneTree, node_id: NodeId, flags: SizeFlags) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("size_flags_vertical", Variant::Int(flags.to_int()));
    }
}

/// Gets the vertical size flags, defaulting to [`SizeFlags::FILL`].
pub fn get_v_size_flags(tree: &SceneTree, node_id: NodeId) -> SizeFlags {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size_flags_vertical") {
            Variant::Int(i) => SizeFlags::from_int(i),
            _ => SizeFlags::FILL,
        })
        .unwrap_or(SizeFlags::FILL)
}

/// Sets a control's `size_flags_stretch_ratio` — the weight with which it
/// shares expand space among sibling `Expand` controls in a BoxContainer.
pub fn set_stretch_ratio(tree: &mut SceneTree, node_id: NodeId, ratio: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("size_flags_stretch_ratio", Variant::Float(ratio as f64));
    }
}

/// Gets a control's `size_flags_stretch_ratio` (Godot default `1.0`).
pub fn get_stretch_ratio(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size_flags_stretch_ratio") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => 1.0,
        })
        .unwrap_or(1.0)
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

/// Gets the GridContainer horizontal cell gap: the
/// `theme_override_constants/h_separation` override when present, otherwise the
/// generic `separation` override. Mirrors Godot's split
/// `h_separation`/`v_separation` GridContainer theme constants.
pub fn get_h_separation(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .and_then(
            |n| match n.get_property("theme_override_constants/h_separation") {
                Variant::Int(i) => Some(i),
                _ => None,
            },
        )
        .unwrap_or_else(|| get_separation(tree, node_id))
}

/// Gets the GridContainer vertical cell gap — see [`get_h_separation`].
pub fn get_v_separation(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .and_then(
            |n| match n.get_property("theme_override_constants/v_separation") {
                Variant::Int(i) => Some(i),
                _ => None,
            },
        )
        .unwrap_or_else(|| get_separation(tree, node_id))
}

/// Sets a BoxContainer's `"alignment"` property.
pub fn set_box_alignment(tree: &mut SceneTree, node_id: NodeId, alignment: BoxAlignment) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("alignment", Variant::Int(alignment.to_int()));
    }
}

/// Gets a BoxContainer's `"alignment"`, defaulting to [`BoxAlignment::Begin`].
pub fn get_box_alignment(tree: &SceneTree, node_id: NodeId) -> BoxAlignment {
    tree.get_node(node_id)
        .map(|n| match n.get_property("alignment") {
            Variant::Int(i) => BoxAlignment::from_int(i),
            _ => BoxAlignment::Begin,
        })
        .unwrap_or_default()
}

/// Whether a box container lays out along the vertical axis.
///
/// `VBoxContainer` is always vertical and `HBoxContainer` always horizontal
/// (Godot's subclasses pin the flag in their constructors); a bare
/// `BoxContainer` reads its `vertical` property, which defaults to `false`.
pub fn is_box_vertical(tree: &SceneTree, node_id: NodeId) -> bool {
    let class = tree
        .get_node(node_id)
        .map(|n| n.class_name().to_string())
        .unwrap_or_default();
    match class.as_str() {
        "VBoxContainer" => true,
        "HBoxContainer" => false,
        _ => tree
            .get_node(node_id)
            .map(|n| matches!(n.get_property("vertical"), Variant::Bool(true)))
            .unwrap_or(false),
    }
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

/// Places `child` inside the cell `(cell_pos, cell_size)` its container just
/// allotted it, honouring the per-axis size-flag bits.
///
/// Ports Godot's `Container::fit_child_in_rect`
/// (upstream/godot/scene/gui/container.cpp): an axis **without** the `FILL` bit
/// collapses the child to its minimum size on that axis and then aligns it —
/// `SHRINK_END` to the far edge, `SHRINK_CENTER` to the middle (floored, as in
/// Godot), otherwise to the near edge. Under RTL the horizontal near/far edges
/// swap, matching upstream.
fn fit_child_in_rect(
    tree: &mut SceneTree,
    child: NodeId,
    cell_pos: Vector2,
    cell_size: Vector2,
    rtl: bool,
) {
    let minsize = effective_min(tree, child);
    let mut pos = cell_pos;
    let mut size = cell_size;

    let h = get_h_size_flags(tree, child);
    if !h.has_flag(SizeFlags::FILL) {
        size.x = minsize.x;
        if h.has_flag(SizeFlags::SHRINK_END) {
            pos.x += if rtl { 0.0 } else { cell_size.x - minsize.x };
        } else if h.has_flag(SizeFlags::SHRINK_CENTER) {
            pos.x += ((cell_size.x - minsize.x) / 2.0).floor();
        } else if rtl {
            pos.x += cell_size.x - minsize.x;
        }
    }

    let v = get_v_size_flags(tree, child);
    if !v.has_flag(SizeFlags::FILL) {
        size.y = minsize.y;
        if v.has_flag(SizeFlags::SHRINK_END) {
            pos.y += cell_size.y - minsize.y;
        } else if v.has_flag(SizeFlags::SHRINK_CENTER) {
            pos.y += ((cell_size.y - minsize.y) / 2.0).floor();
        }
    }

    set_position(tree, child, pos);
    set_size(tree, child, size);
}

/// Arranges `children` along a BoxContainer's main axis.
///
/// Ports Godot's `BoxContainer::_resort`
/// (upstream/godot/scene/gui/box_container.cpp) rather than approximating it:
///
/// * a child stretches only when its **main-axis** flags carry the `EXPAND`
///   bit — including `EXPAND` alone (2), which the old exclusive-enum model
///   mapped onto `Fill` and never grew (pat-0q2kx.83);
/// * leftover space is split by `size_flags_stretch_ratio`, and the refit loop
///   drops any stretcher whose share falls below its minimum size, re-splitting
///   among the rest;
/// * when nothing stretched, `alignment` packs the whole run at the begin,
///   center, or end of the axis;
/// * each child is then placed with [`fit_child_in_rect`], so the cross axis
///   (and a main axis without `FILL`) honours `FILL`/`SHRINK_CENTER`/`SHRINK_END`;
/// * under RTL a horizontal box lays its children out from the last to the
///   first, so the logically-first child ends up at the right edge
///   (pat-nzgdu.142).
fn arrange_box(
    tree: &mut SceneTree,
    children: &[NodeId],
    total: Vector2,
    sep: f32,
    vertical: bool,
    alignment: BoxAlignment,
    rtl: bool,
) {
    let n = children.len();
    if n == 0 {
        return;
    }

    let axis_of = |v: Vector2| if vertical { v.y } else { v.x };
    let mins: Vec<f32> = children
        .iter()
        .map(|&c| axis_of(effective_min(tree, c)))
        .collect();
    let ratios: Vec<f32> = children
        .iter()
        .map(|&c| get_stretch_ratio(tree, c).max(0.0))
        .collect();
    // The EXPAND *bit* on the main axis — not an equality test against a single
    // enum value.
    let mut will_stretch: Vec<bool> = children
        .iter()
        .map(|&c| {
            let flags = if vertical {
                get_v_size_flags(tree, c)
            } else {
                get_h_size_flags(tree, c)
            };
            flags.has_flag(SizeFlags::EXPAND)
        })
        .collect();

    let stretch_min: f32 = mins.iter().sum();
    let mut stretch_avail: f32 = will_stretch
        .iter()
        .zip(mins.iter())
        .filter_map(|(&s, &m)| if s { Some(m) } else { None })
        .sum();
    let mut stretch_ratio_total: f32 = will_stretch
        .iter()
        .zip(ratios.iter())
        .filter_map(|(&s, &r)| if s { Some(r) } else { None })
        .sum();
    let mut final_size = mins.clone();

    let stretch_max = axis_of(total) - (n as f32 - 1.0) * sep;
    let stretch_diff = (stretch_max - stretch_min).max(0.0);
    stretch_avail += stretch_diff;

    // Successively discard stretchers that cannot even reach their minimum,
    // re-dividing the pool among the survivors (upstream's second pass).
    let mut has_stretched = false;
    while stretch_ratio_total > 0.0 {
        has_stretched = true;
        let mut refit_successful = true;
        let mut error = 0.0f32;
        for i in 0..n {
            if !will_stretch[i] {
                continue;
            }
            let desired = stretch_avail * ratios[i] / stretch_ratio_total;
            error += desired - desired.floor();
            if desired < mins[i] {
                will_stretch[i] = false;
                stretch_ratio_total -= ratios[i];
                stretch_avail -= mins[i];
                final_size[i] = mins[i];
                refit_successful = false;
                break;
            }
            final_size[i] = desired.floor();
            if error >= 1.0 {
                final_size[i] += 1.0;
                error -= 1.0;
            }
        }
        if refit_successful {
            break;
        }
    }

    // With nothing stretching, `alignment` decides where the run sits.
    let mut ofs = 0.0f32;
    if !has_stretched {
        ofs = match (alignment, vertical, rtl) {
            (BoxAlignment::Begin, false, true) => stretch_diff,
            (BoxAlignment::Begin, _, _) => 0.0,
            (BoxAlignment::Center, _, _) => (stretch_diff / 2.0).floor(),
            (BoxAlignment::End, false, true) => 0.0,
            (BoxAlignment::End, _, _) => stretch_diff,
        };
    }

    // RTL horizontal boxes walk the child list backwards while the cursor still
    // advances left-to-right, so the first child lands on the right.
    let order: Vec<usize> = if rtl && !vertical {
        (0..n).rev().collect()
    } else {
        (0..n).collect()
    };

    let mut first = true;
    for (idx, &i) in order.iter().enumerate() {
        if first {
            first = false;
        } else {
            ofs += sep;
        }
        let from = ofs;
        let mut to = ofs + final_size[i];
        // The last stretching child absorbs the rounding residue so the run
        // ends exactly on the container edge.
        if will_stretch[i] && idx == n - 1 {
            to = axis_of(total);
        }
        let size = to - from;
        let (cell_pos, cell_size) = if vertical {
            (Vector2::new(0.0, from), Vector2::new(total.x, size))
        } else {
            (Vector2::new(from, 0.0), Vector2::new(size, total.y))
        };
        fit_child_in_rect(tree, children[i], cell_pos, cell_size, rtl);
        ofs = to;
    }
}

/// Reflows `children` into rows (horizontal flow) or columns (vertical flow),
/// wrapping to a new line whenever the next child would overflow the container's
/// main-axis extent. Mirrors Godot's HFlowContainer / VFlowContainer at the
/// default `BEGIN` alignment: each child keeps its effective minimum size and is
/// packed from the start of its line, and a line's cross-axis thickness is the
/// largest child thickness on that line.
///
/// `vertical` selects the flow direction: `false` fills rows left-to-right and
/// advances downward (HFlowContainer); `true` fills columns top-to-bottom and
/// advances rightward (VFlowContainer). At least one child is placed per line so
/// an oversized child never produces an empty line. `sep` (the container's
/// `separation` theme override) is used for both the in-line gap between children
/// and the gap between wrapped lines.
fn arrange_flow(
    tree: &mut SceneTree,
    children: &[NodeId],
    total: Vector2,
    sep: f32,
    vertical: bool,
) {
    if children.is_empty() {
        return;
    }
    let mins: Vec<Vector2> = children.iter().map(|&c| effective_min(tree, c)).collect();
    let main_limit = if vertical { total.y } else { total.x };

    let mut main = 0.0f32; // cursor along the current line
    let mut cross = 0.0f32; // start of the current line on the cross axis
    let mut line_thickness = 0.0f32; // max child cross-size on the current line
    let mut line_started = false;

    for (i, &c) in children.iter().enumerate() {
        // main_size grows the line; cross_size decides how thick the line is.
        let (main_size, cross_size) = if vertical {
            (mins[i].y, mins[i].x)
        } else {
            (mins[i].x, mins[i].y)
        };
        // Wrap when the child would overflow the line, but always keep at least
        // one child per line so an oversized child does not spawn an empty line.
        if line_started && main + main_size > main_limit {
            cross += line_thickness + sep;
            main = 0.0;
            line_thickness = 0.0;
            line_started = false;
        }
        let pos = if vertical {
            Vector2::new(cross, main)
        } else {
            Vector2::new(main, cross)
        };
        set_position(tree, c, pos);
        set_size(tree, c, mins[i]);
        main += main_size + sep;
        if cross_size > line_thickness {
            line_thickness = cross_size;
        }
        line_started = true;
    }
}

/// Arranges a GridContainer's children, distributing leftover space to the
/// columns and rows that contain an `EXPAND` child.
///
/// Ports Godot's `GridContainer::_notification(NOTIFICATION_SORT_CHILDREN)`
/// (upstream/godot/scene/gui/grid_container.cpp): a column is expandable when
/// **any** of its children carries the horizontal `EXPAND` bit (rows likewise
/// for the vertical bit); the leftover width is then split evenly among the
/// expanding columns, dropping the widest one whenever the even share would
/// fall under its minimum width. Each cell finally goes through
/// [`fit_child_in_rect`], so a non-`FILL` child shrinks and aligns inside its
/// cell instead of being stretched (pat-0q2kx.83).
fn arrange_grid(
    tree: &mut SceneTree,
    children: &[NodeId],
    total: Vector2,
    h_sep: f32,
    v_sep: f32,
    cols: usize,
    rtl: bool,
) {
    if children.is_empty() || cols == 0 {
        return;
    }
    let rows = children.len().div_ceil(cols);
    let mins: Vec<Vector2> = children.iter().map(|&c| effective_min(tree, c)).collect();

    let mut col_minw = vec![0f32; cols];
    let mut row_minh = vec![0f32; rows];
    let mut col_expanded = vec![false; cols];
    let mut row_expanded = vec![false; rows];
    for (i, m) in mins.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        if m.x > col_minw[col] {
            col_minw[col] = m.x;
        }
        if m.y > row_minh[row] {
            row_minh[row] = m.y;
        }
        if get_h_size_flags(tree, children[i]).has_flag(SizeFlags::EXPAND) {
            col_expanded[col] = true;
        }
        if get_v_size_flags(tree, children[i]).has_flag(SizeFlags::EXPAND) {
            row_expanded[row] = true;
        }
    }
    // Godot treats trailing columns with no child at all as expanded.
    for c in children.len()..cols {
        col_expanded[c] = true;
    }

    let max_col = children.len().min(cols);
    let max_row = rows;

    // Space left for the expanding tracks once the fixed ones and the gaps are
    // paid for.
    let mut remaining_w = total.x;
    for c in 0..cols {
        if !col_expanded[c] {
            remaining_w -= col_minw[c];
        }
    }
    let mut remaining_h = total.y;
    for r in 0..rows {
        if !row_expanded[r] {
            remaining_h -= row_minh[r];
        }
    }
    remaining_w -= h_sep * (max_col as f32 - 1.0).max(0.0);
    remaining_h -= v_sep * (max_row as f32 - 1.0).max(0.0);

    // Drop the widest expanding column while an even share would starve one of
    // them, exactly as upstream does.
    loop {
        let live: Vec<usize> = (0..cols).filter(|&c| col_expanded[c]).collect();
        if live.is_empty() {
            break;
        }
        let share = remaining_w / live.len() as f32;
        if live.iter().all(|&c| share >= col_minw[c]) {
            break;
        }
        let widest = *live
            .iter()
            .max_by(|&&a, &&b| col_minw[a].total_cmp(&col_minw[b]))
            .expect("live is non-empty");
        col_expanded[widest] = false;
        remaining_w -= col_minw[widest];
    }
    loop {
        let live: Vec<usize> = (0..rows).filter(|&r| row_expanded[r]).collect();
        if live.is_empty() {
            break;
        }
        let share = remaining_h / live.len() as f32;
        if live.iter().all(|&r| share >= row_minh[r]) {
            break;
        }
        let tallest = *live
            .iter()
            .max_by(|&&a, &&b| row_minh[a].total_cmp(&row_minh[b]))
            .expect("live is non-empty");
        row_expanded[tallest] = false;
        remaining_h -= row_minh[tallest];
    }

    let col_expand_count = col_expanded.iter().filter(|&&e| e).count();
    let row_expand_count = row_expanded.iter().filter(|&&e| e).count();
    let col_expand = if col_expand_count > 0 {
        (remaining_w / col_expand_count as f32).floor()
    } else {
        0.0
    };
    let row_expand = if row_expand_count > 0 {
        (remaining_h / row_expand_count as f32).floor()
    } else {
        0.0
    };

    let col_size = |c: usize| {
        if col_expanded[c] {
            col_expand
        } else {
            col_minw[c]
        }
    };
    let row_size = |r: usize| {
        if row_expanded[r] {
            row_expand
        } else {
            row_minh[r]
        }
    };

    let mut x_offsets = vec![0f32; cols];
    for c in 1..cols {
        x_offsets[c] = x_offsets[c - 1] + col_size(c - 1) + h_sep;
    }
    let mut y_offsets = vec![0f32; rows];
    for r in 1..rows {
        y_offsets[r] = y_offsets[r - 1] + row_size(r - 1) + v_sep;
    }

    for (i, &c) in children.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let size = Vector2::new(col_size(col), row_size(row));
        // Under RTL the columns are mirrored so column 0 sits on the right.
        let x = if rtl {
            total.x - x_offsets[col] - size.x
        } else {
            x_offsets[col]
        };
        fit_child_in_rect(tree, c, Vector2::new(x, y_offsets[row]), size, rtl);
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
        "VBoxContainer" | "HBoxContainer" | "BoxContainer" => {
            let rtl = is_layout_rtl(tree, container_id);
            let vertical = is_box_vertical(tree, container_id);
            let alignment = get_box_alignment(tree, container_id);
            arrange_box(tree, &children, total, sep, vertical, alignment, rtl)
        }
        // FlowContainer defaults to horizontal flow (Godot's `vertical` property
        // is false by default); H/VFlowContainer force the direction.
        "FlowContainer" | "HFlowContainer" => arrange_flow(tree, &children, total, sep, false),
        "VFlowContainer" => arrange_flow(tree, &children, total, sep, true),
        "GridContainer" => {
            let cols = get_columns(tree, container_id).max(1) as usize;
            let h_sep = get_h_separation(tree, container_id) as f32;
            let v_sep = get_v_separation(tree, container_id) as f32;
            let rtl = is_layout_rtl(tree, container_id);
            arrange_grid(tree, &children, total, h_sep, v_sep, cols, rtl);
        }
        "MarginContainer" => {
            let m = get_margins(tree, container_id);
            arrange_margin(tree, &children, total, m);
        }
        "CenterContainer" => arrange_center(tree, &children, total),
        "ScrollContainer" => {
            let scroll = get_scroll(tree, container_id);
            arrange_scroll(tree, container_id, &children, total, scroll);
        }
        "TabContainer" => {
            let current = get_current_tab(tree, container_id);
            let strip_h = get_tab_bar_height(tree, container_id);
            arrange_tabs(tree, &children, total, current, strip_h);
        }
        // Split containers place their first two children on either side of a
        // draggable divider positioned by `split_offset` (pat-nzgdu.158).
        "SplitContainer" | "HSplitContainer" => {
            let offset = get_split_offset(tree, container_id);
            arrange_split(tree, &children, total, sep, offset, false);
        }
        "VSplitContainer" => {
            let offset = get_split_offset(tree, container_id);
            arrange_split(tree, &children, total, sep, offset, true);
        }
        // SubViewportContainer sizes its hosted child (the SubViewport) to fill
        // the container (pat-nzgdu.158).
        "SubViewportContainer" => {
            if let Some(&child) = children.first() {
                set_position(tree, child, Vector2::ZERO);
                set_size(tree, child, total);
            }
        }
        _ => {}
    }
}

/// Lays out a `PanelContainer`: every child fills the container's rect **inset
/// by the panel stylebox's content margins** (Godot's
/// `PanelContainer::_notification` SORT_CHILDREN, scene/gui/panel_container.cpp).
///
/// The margins are passed in rather than looked up because layout has no theme:
/// resolving `panel` needs the theme-override chain, which only the render walk
/// carries. `control_render` is therefore the caller — see its `PanelContainer`
/// branch. Keeping the geometry here and the resolution there is what stops this
/// from needing a `&Theme` parameter on the whole layout pipeline.
///
/// ⚠️ A child is never given a negative size: with margins wider than the
/// container, Godot collapses the content rect rather than inverting it, and an
/// inverted rect would make hit-testing select the *outside* of the panel.
pub fn arrange_panel_container(
    tree: &mut SceneTree,
    container_id: NodeId,
    total: Vector2,
    content_margins: [f32; 4],
) {
    set_size(tree, container_id, total);
    let [l, t, r, b] = content_margins;
    let inner = Vector2::new((total.x - l - r).max(0.0), (total.y - t - b).max(0.0));
    let children: Vec<NodeId> = tree
        .get_node(container_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    for child in children {
        set_position(tree, child, Vector2::new(l, t));
        set_size(tree, child, inner);
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
            | "FlowContainer"
            | "HFlowContainer"
            | "VFlowContainer"
            | "SplitContainer"
            | "HSplitContainer"
            | "VSplitContainer"
            | "SubViewportContainer"
    )
}

/// Sets the SplitContainer `"split_offset"` — the pixel amount the divider is
/// shifted from the container's midpoint. Dragging the divider updates this.
pub fn set_split_offset(tree: &mut SceneTree, node_id: NodeId, offset: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("split_offset", Variant::Float(offset as f64));
    }
}

/// Gets the SplitContainer `"split_offset"`, defaulting to `0.0` (centered).
pub fn get_split_offset(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("split_offset") {
            Variant::Float(v) => v as f32,
            Variant::Int(v) => v as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// The `[lo, hi]` range the divider of a two-child SplitContainer may occupy,
/// in pixels along the split axis.
///
/// Ports Godot's `SplitContainer::_get_valid_range`
/// (upstream/godot/scene/gui/split_container.cpp) for the single-dragger case:
/// the divider cannot move closer to the start than the first child's minimum
/// size, nor closer to the end than the separation plus the second child's
/// minimum size. When the container is too small to honour both minimums the
/// lower bound wins, matching Godot's `CLAMP`.
fn split_valid_range(
    tree: &SceneTree,
    children: &[NodeId],
    total: Vector2,
    sep: f32,
    vertical: bool,
) -> (f32, f32) {
    let axis = if vertical { total.y } else { total.x };
    let min_of = |c: NodeId| {
        let m = effective_min(tree, c);
        if vertical {
            m.y
        } else {
            m.x
        }
    };
    let lo = children.first().map(|&c| min_of(c)).unwrap_or(0.0);
    let hi = axis - sep - children.get(1).map(|&c| min_of(c)).unwrap_or(0.0);
    (lo, hi)
}

/// Clamps `offset` so the divider stays inside [`split_valid_range`], returning
/// the clamped **offset** (not the absolute divider position).
fn clamp_split_offset_value(
    tree: &SceneTree,
    children: &[NodeId],
    total: Vector2,
    sep: f32,
    offset: f32,
    vertical: bool,
) -> f32 {
    let axis = if vertical { total.y } else { total.x };
    let default_pos = ((axis - sep) * 0.5).max(0.0);
    let (lo, hi) = split_valid_range(tree, children, total, sep, vertical);
    // `.min(hi).max(lo)` reproduces Godot's CLAMP, where the lower bound wins
    // when the two bounds cross.
    let pos = (default_pos + offset).min(hi).max(lo);
    pos - default_pos
}

/// Clamps a SplitContainer's stored `split_offset` against its children's
/// minimum sizes for the given container size — Godot's
/// `SplitContainer::clamp_split_offset()`.
///
/// Returns `true` when the stored value actually changed.
pub fn clamp_split_offset(tree: &mut SceneTree, container_id: NodeId, total: Vector2) -> bool {
    let children: Vec<NodeId> = tree
        .get_node(container_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    let vertical = tree
        .get_node(container_id)
        .map(|n| n.class_name() == "VSplitContainer")
        .unwrap_or(false);
    let sep = get_separation(tree, container_id) as f32;
    let current = get_split_offset(tree, container_id);
    let clamped = clamp_split_offset_value(tree, &children, total, sep, current, vertical);
    if (clamped - current).abs() < f32::EPSILON {
        return false;
    }
    set_split_offset(tree, container_id, clamped);
    true
}

/// Applies a divider drag to a SplitContainer: clamps `new_offset` against both
/// children's minimum sizes and, when the stored `split_offset` actually
/// changes, emits Godot's `dragged(offset)` signal.
///
/// Returns `true` when the offset changed. `total` is the container's current
/// size, which the clamp needs to know where the end of the axis is.
pub fn drag_split(
    tree: &mut SceneTree,
    container_id: NodeId,
    new_offset: f32,
    total: Vector2,
) -> bool {
    let children: Vec<NodeId> = tree
        .get_node(container_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    let vertical = tree
        .get_node(container_id)
        .map(|n| n.class_name() == "VSplitContainer")
        .unwrap_or(false);
    let sep = get_separation(tree, container_id) as f32;
    let current = get_split_offset(tree, container_id);
    let clamped = clamp_split_offset_value(tree, &children, total, sep, new_offset, vertical);
    if (clamped - current).abs() < f32::EPSILON {
        return false;
    }
    set_split_offset(tree, container_id, clamped);
    tree.emit_signal(container_id, "dragged", &[Variant::Float(clamped as f64)]);
    true
}

/// Lays out a SplitContainer's first two children on either side of a divider.
///
/// The divider sits at the container's midpoint along the split axis, shifted
/// by `split_offset` and **clamped by both children's minimum sizes** so a
/// dragged divider can never crush a pane below its minimum
/// (upstream/godot/scene/gui/split_container.cpp `_get_valid_range`,
/// pat-0q2kx.83). `sep` is the divider gap. `vertical` splits top/bottom,
/// otherwise left/right. Children past the first two are left unpositioned
/// (Godot only lays out the first two).
fn arrange_split(
    tree: &mut SceneTree,
    children: &[NodeId],
    total: Vector2,
    sep: f32,
    split_offset: f32,
    vertical: bool,
) {
    if children.is_empty() {
        return;
    }
    let axis = if vertical { total.y } else { total.x };
    let usable = (axis - sep).max(0.0);
    let default_pos = usable * 0.5;
    let (lo, hi) = split_valid_range(tree, children, total, sep, vertical);
    // Clamp to the minimum-size range first, then to the container itself so a
    // pane never goes negative when the minimums cannot be satisfied at all.
    let first = (default_pos + split_offset)
        .min(hi)
        .max(lo)
        .clamp(0.0, usable);
    let second = (usable - first).max(0.0);

    let c0 = children[0];
    set_position(tree, c0, Vector2::ZERO);
    if vertical {
        set_size(tree, c0, Vector2::new(total.x, first));
    } else {
        set_size(tree, c0, Vector2::new(first, total.y));
    }

    if let Some(&c1) = children.get(1) {
        if vertical {
            set_position(tree, c1, Vector2::new(0.0, first + sep));
            set_size(tree, c1, Vector2::new(total.x, second));
        } else {
            set_position(tree, c1, Vector2::new(first + sep, 0.0));
            set_size(tree, c1, Vector2::new(second, total.y));
        }
    }
}

/// Maps a click at `local` (SubViewportContainer-local pixels) into the hosted
/// SubViewport's coordinate space (pat-nzgdu.158). With `stretch` enabled the
/// sub-viewport is `stretch_shrink`× smaller than the container, so the click
/// divides by the shrink factor; otherwise the mapping is 1:1. Mirrors Godot's
/// `SubViewportContainer` input forwarding.
/// True when `node_id` is a `SubViewportContainer`.
///
/// Checked by class name as well as ClassDB so hit-testing still works in bare
/// unit tests where the inheritance table is unpopulated — the same fallback
/// [`CONTROL_HIT_CLASSES`] uses.
fn is_subviewport_container(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.class_name() == "SubViewportContainer" || n.is_class("SubViewportContainer"))
        .unwrap_or(false)
}

/// Reads a `SubViewportContainer`'s `stretch` / `stretch_shrink`.
///
/// Godot's defaults are `stretch = false` and `stretch_shrink = 1`, and a shrink
/// below 1 would invert the mapping, so it is clamped.
fn subviewport_container_stretch(tree: &SceneTree, node_id: NodeId) -> (bool, i64) {
    let Some(node) = tree.get_node(node_id) else {
        return (false, 1);
    };
    let stretch = matches!(node.get_property("stretch"), Variant::Bool(true));
    let shrink = match node.get_property("stretch_shrink") {
        Variant::Int(v) if v >= 1 => v,
        _ => 1,
    };
    (stretch, shrink)
}

pub fn subviewport_container_to_viewport_coord(
    local: Vector2,
    stretch: bool,
    stretch_shrink: i64,
) -> Vector2 {
    if stretch && stretch_shrink > 1 {
        let s = stretch_shrink as f32;
        Vector2::new(local.x / s, local.y / s)
    } else {
        local
    }
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

/// Writes a ScrollContainer's scroll offsets (pat-0q2kx.86).
///
/// Stored as `Float` so a clamped fractional wheel step survives; `get_scroll`
/// accepts either numeric variant.
pub fn set_scroll(tree: &mut SceneTree, node_id: NodeId, scroll: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("scroll_horizontal", Variant::Float(scroll.x as f64));
        node.set_property("scroll_vertical", Variant::Float(scroll.y as f64));
    }
}

/// The extent a ScrollContainer's content occupies — the size
/// [`arrange_scroll`] gives the content child, i.e. its minimum size expanded
/// to at least the viewport (pat-0q2kx.86).
///
/// Returns the container's own size when it has no children, so a childless
/// container reports nothing to scroll rather than a bogus overflow.
pub fn scroll_content_size(tree: &SceneTree, container_id: NodeId, view: Vector2) -> Vector2 {
    let children = tree
        .get_node(container_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    let mut content = view;
    for c in children {
        let min = effective_min(tree, c);
        content = Vector2::new(content.x.max(min.x), content.y.max(min.y));
    }
    content
}

/// How far a ScrollContainer can scroll before the content's far edge reaches
/// the viewport's — Godot clamps `scroll_*` to this, which is why scrolling
/// past the end parks at the bottom instead of running off into blank space
/// (`ScrollContainer::_update_scrollbar_position`).
pub fn max_scroll(tree: &SceneTree, container_id: NodeId, view: Vector2) -> Vector2 {
    let content = scroll_content_size(tree, container_id, view);
    Vector2::new((content.x - view.x).max(0.0), (content.y - view.y).max(0.0))
}

/// Height of a TabContainer's tab-header strip, in pixels. Godot's default
/// theme tab height is ~32px for the built-in font.
pub const TAB_BAR_HEIGHT: f32 = 32.0;

/// Horizontal padding applied on each side of a tab title, in pixels — the
/// stand-in for Godot's `tab_selected`/`tab_unselected` StyleBox content
/// margins.
pub const TAB_H_PADDING: f32 = 8.0;

/// Height of a TabContainer's tab strip.
///
/// Godot derives this from the theme (`TabContainer::_get_tab_height` sums the
/// tab StyleBox margins and the font height); Patina reads the
/// `theme_override_constants/tab_height` override when the scene provides one
/// and otherwise falls back to [`TAB_BAR_HEIGHT`] (pat-0q2kx.83). A tabs-hidden
/// container (`tabs_visible = false`) has no strip at all.
pub fn get_tab_bar_height(tree: &SceneTree, node_id: NodeId) -> f32 {
    let Some(node) = tree.get_node(node_id) else {
        return TAB_BAR_HEIGHT;
    };
    if matches!(node.get_property("tabs_visible"), Variant::Bool(false)) {
        return 0.0;
    }
    match node.get_property("theme_override_constants/tab_height") {
        Variant::Int(i) => i.max(0) as f32,
        Variant::Float(f) => (f as f32).max(0.0),
        _ => TAB_BAR_HEIGHT,
    }
}

/// The title shown on tab `index` of a TabContainer.
///
/// Godot titles a tab after its child node's name unless `set_tab_title()`
/// overrode it, which it stores on the child; Patina reads that override from
/// the child's `_tab_name` property.
pub fn get_tab_title(tree: &SceneTree, container_id: NodeId, index: usize) -> String {
    let Some(&child) = tree
        .get_node(container_id)
        .and_then(|n| n.children().get(index))
    else {
        return String::new();
    };
    tree.get_node(child)
        .map(|n| match n.get_property("_tab_name") {
            Variant::String(s) if !s.is_empty() => s,
            _ => n.name().to_string(),
        })
        .unwrap_or_default()
}

/// The font scale the tab strip draws its titles at, shared by the renderer and
/// the click hit-test so the two never disagree.
fn tab_font_scale(tree: &SceneTree, container_id: NodeId) -> u32 {
    crate::control_render::text_scale(get_font_size(tree, container_id))
}

/// The tab strip's per-tab `(x, width)` spans, in container-local pixels.
///
/// Each tab is as wide as its title plus [`TAB_H_PADDING`] on both sides, laid
/// out left to right from the container's origin — the geometry both
/// [`crate::control_render`] and [`tab_at_point`] use.
pub fn tab_rects(tree: &SceneTree, container_id: NodeId) -> Vec<(f32, f32)> {
    let count = tree
        .get_node(container_id)
        .map(|n| n.children().len())
        .unwrap_or(0);
    let scale = tab_font_scale(tree, container_id);
    let mut out = Vec::with_capacity(count);
    let mut x = 0.0f32;
    for i in 0..count {
        let title = get_tab_title(tree, container_id, i);
        let w = crate::control_render::text_width(&title, scale) + TAB_H_PADDING * 2.0;
        out.push((x, w));
        x += w;
    }
    out
}

/// The viewport-global origin of `node_id`, summing the positions of it and its
/// Control ancestors — the same accumulation [`control_at_point`] performs on
/// the way down.
fn control_global_origin(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    let mut origin = Vector2::ZERO;
    let mut current = Some(node_id);
    while let Some(id) = current {
        let is_ctrl = tree
            .get_node(id)
            .map(|n| n.is_class("Control") || CONTROL_HIT_CLASSES.contains(&n.class_name()))
            .unwrap_or(false);
        if is_ctrl {
            origin = origin + get_position(tree, id);
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }
    origin
}

/// The index of the tab whose header the viewport-global `point` falls on, or
/// `None` when the point misses the strip.
pub fn tab_at_point(tree: &SceneTree, container_id: NodeId, point: Vector2) -> Option<usize> {
    let origin = control_global_origin(tree, container_id);
    let local = Vector2::new(point.x - origin.x, point.y - origin.y);
    let strip_h = get_tab_bar_height(tree, container_id);
    if local.y < 0.0 || local.y >= strip_h || local.x < 0.0 {
        return None;
    }
    tab_rects(tree, container_id)
        .into_iter()
        .position(|(x, w)| local.x >= x && local.x < x + w)
}

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
///
/// The offset is clamped to `content - viewport` and written back to the
/// container (pat-0q2kx.86). Godot clamps in the scrollbars themselves — a
/// `scroll_vertical` past the end parks the content's bottom edge at the
/// viewport's bottom rather than scrolling into blank space — and a script that
/// assigned the out-of-range value reads the clamped one back.
fn arrange_scroll(
    tree: &mut SceneTree,
    container_id: NodeId,
    children: &[NodeId],
    total: Vector2,
    scroll: Vector2,
) {
    let limit = max_scroll(tree, container_id, total);
    let clamped = Vector2::new(scroll.x.clamp(0.0, limit.x), scroll.y.clamp(0.0, limit.y));
    if clamped != scroll {
        set_scroll(tree, container_id, clamped);
    }
    for &c in children {
        let min = effective_min(tree, c);
        let size = Vector2::new(min.x.max(total.x), min.y.max(total.y));
        set_position(tree, c, Vector2::new(-clamped.x, -clamped.y));
        set_size(tree, c, size);
    }
}

/// TabContainer: only the child at `current` is visible, filling the area below
/// the tab-header strip (whose height comes from the theme, see
/// [`get_tab_bar_height`]). Non-current children are collapsed to zero size.
fn arrange_tabs(
    tree: &mut SceneTree,
    children: &[NodeId],
    total: Vector2,
    current: usize,
    strip_h: f32,
) {
    let content = Vector2::new(total.x, (total.y - strip_h).max(0.0));
    for (i, &c) in children.iter().enumerate() {
        if i == current {
            set_position(tree, c, Vector2::new(0.0, strip_h));
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

// ---------------------------------------------------------------------------
// BaseButton semantics (pat-s3rrv.12)
// ---------------------------------------------------------------------------
//
// Godot's `BaseButton` (upstream/godot/scene/gui/base_button.cpp) is more than
// "emit `pressed` on click": it filters by `button_mask`, chooses the firing
// EDGE with `action_mode`, flips `button_pressed` and emits `toggled(bool)` in
// `toggle_mode`, and enforces mutual exclusion across a `button_group`.
//
// Patina registered every one of those properties in ClassDB and *drew*
// `button_pressed` (control_render.rs), but nothing ever wrote them — the GUI
// stage emitted `button_down`/`button_up`/`pressed` for ANY mouse button and
// knew nothing about toggling. These helpers are that missing half.

/// Godot's `ACTION_MODE_BUTTON_PRESS` — fire on the down edge.
pub const ACTION_MODE_BUTTON_PRESS: i64 = 0;

/// `MOUSE_BUTTON_MASK_LEFT`: Godot's `button_mask` default, and the reason a
/// right-click on a Button does nothing at all
/// (`base_button.cpp` — the event is ignored before any state changes).
pub const DEFAULT_BUTTON_MASK: i64 = 1;

/// Reads an integer property, falling back to `default` when unset or of
/// another type.
fn int_prop_or(tree: &SceneTree, node_id: NodeId, key: &str, default: i64) -> i64 {
    match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::Int(i)) => i,
        Some(Variant::Float(f)) => f as i64,
        _ => default,
    }
}

/// Reads a bool property, defaulting to `false`.
fn bool_prop_or_false(tree: &SceneTree, node_id: NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(node_id).map(|n| n.get_property(key)),
        Some(Variant::Bool(true))
    )
}

/// `BaseButton.button_mask` — which mouse buttons act on this button. Godot
/// defaults to LEFT only.
pub fn button_mask(tree: &SceneTree, node_id: NodeId) -> i64 {
    int_prop_or(tree, node_id, "button_mask", DEFAULT_BUTTON_MASK)
}

/// Whether `button_index` (1-based, as Godot numbers mouse buttons) is in this
/// button's mask. Godot builds the mask as `1 << (button_index - 1)`.
pub fn button_mask_accepts(tree: &SceneTree, node_id: NodeId, button_index: i64) -> bool {
    if button_index < 1 {
        return false;
    }
    button_mask(tree, node_id) & (1 << (button_index - 1)) != 0
}

/// `BaseButton.toggle_mode`.
pub fn button_is_toggle(tree: &SceneTree, node_id: NodeId) -> bool {
    bool_prop_or_false(tree, node_id, "toggle_mode")
}

/// `BaseButton.button_pressed` — the toggle state, not "is the mouse down".
pub fn button_is_pressed(tree: &SceneTree, node_id: NodeId) -> bool {
    bool_prop_or_false(tree, node_id, "button_pressed")
}

/// `BaseButton.action_mode`. Godot's default is `ACTION_MODE_BUTTON_RELEASE`
/// (1) — the click completes on release, which is why dragging off a button
/// cancels it.
pub fn button_action_mode(tree: &SceneTree, node_id: NodeId) -> i64 {
    int_prop_or(tree, node_id, "action_mode", 1)
}

/// `BaseButton.keep_pressed_outside` — when true the press survives the pointer
/// leaving the rect, so releasing outside still fires.
pub fn keeps_pressed_outside(tree: &SceneTree, node_id: NodeId) -> bool {
    bool_prop_or_false(tree, node_id, "keep_pressed_outside")
}

/// The button's `button_group`, as the group resource's identity string. Empty
/// means ungrouped.
///
/// A `.tscn` writes `button_group = ExtResource("…")` / `SubResource("…")`, and
/// the loader leaves that reference text (or resolves it to a path companion),
/// so any two buttons naming the SAME resource share a group — which is exactly
/// the comparison Godot's `Ref<ButtonGroup>` equality makes.
pub fn button_group_of(tree: &SceneTree, node_id: NodeId) -> String {
    let Some(node) = tree.get_node(node_id) else {
        return String::new();
    };
    match node.get_property("__extres__button_group") {
        Variant::String(s) if !s.is_empty() => return s,
        _ => {}
    }
    match node.get_property("button_group") {
        Variant::String(s) => s,
        _ => String::new(),
    }
}

/// Every other button in the tree sharing `node_id`'s `button_group`.
fn button_group_siblings(tree: &SceneTree, node_id: NodeId) -> Vec<NodeId> {
    let group = button_group_of(tree, node_id);
    if group.is_empty() {
        return Vec::new();
    }
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|&id| id != node_id)
        .filter(|&id| node_is_base_button(tree, id))
        .filter(|&id| button_group_of(tree, id) == group)
        .collect()
}

/// Sets `button_pressed` and emits `toggled(pressed)` when it actually changed,
/// unpressing any group siblings — Godot's `set_pressed` + `_unpress_group`.
///
/// Returns whether the state changed. Emitting only on a real change matches
/// Godot, where `set_pressed` early-returns if the value is already there;
/// re-emitting would make a grouped click report a spurious `toggled(true)` on
/// the button that was already on.
pub fn set_button_pressed(tree: &mut SceneTree, node_id: NodeId, pressed: bool) -> bool {
    if button_is_pressed(tree, node_id) == pressed {
        return false;
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("button_pressed", Variant::Bool(pressed));
    }
    if pressed {
        for sibling in button_group_siblings(tree, node_id) {
            if button_is_pressed(tree, sibling) {
                if let Some(node) = tree.get_node_mut(sibling) {
                    node.set_property("button_pressed", Variant::Bool(false));
                }
                tree.emit_signal(sibling, "toggled", &[Variant::Bool(false)]);
                tree.call_script_with_access(sibling, "_toggled", &[Variant::Bool(false)]);
            }
        }
    }
    tree.emit_signal(node_id, "toggled", &[Variant::Bool(pressed)]);
    // `_toggled(bool)` is BaseButton's virtual, alongside the signal
    // (`GDVIRTUAL_CALL(_toggled, …)`); a subclass script overrides it instead of
    // connecting to itself.
    tree.call_script_with_access(node_id, "_toggled", &[Variant::Bool(pressed)]);
    true
}

/// Runs the button's action: the toggle flip (if any) followed by `pressed`.
///
/// Port of `BaseButton::_pressed()` plus the toggle branch of `on_action_event`
/// (`base_button.cpp`). A grouped button that is already pressed does NOT
/// un-toggle — Godot keeps one member of a group active unless the group allows
/// unpressing.
pub fn fire_button_action(tree: &mut SceneTree, node_id: NodeId) {
    if button_is_toggle(tree, node_id) {
        let currently = button_is_pressed(tree, node_id);
        let grouped = !button_group_of(tree, node_id).is_empty();
        let next = if currently && grouped {
            true
        } else {
            !currently
        };
        set_button_pressed(tree, node_id, next);
    }
    tree.emit_signal(node_id, "pressed", &[]);
    tree.call_script_with_access(node_id, "_pressed", &[]);
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

// ---------------------------------------------------------------------------
// Focus traversal (pat-0q2kx.84)
//
// Godot resolves the *effective* focus mode from `focus_mode`, then walks the
// Control tree to answer Tab / Shift-Tab (`find_next_valid_focus` /
// `find_prev_valid_focus`) and the arrow keys
// (`find_valid_focus_neighbor`) — upstream/godot/scene/gui/control.cpp.
// Patina stored the three focus properties but had none of the walks, so Tab
// did nothing and `focus_next` was inert data.
// ---------------------------------------------------------------------------

/// Control classes that default to `FOCUS_ALL` in Godot when a scene does not
/// serialize an explicit `focus_mode` (the interactive families). A `.tscn`
/// Button never writes its default `focus_mode`, so without this fallback no
/// authored Button would ever be reachable by click or by Tab.
const FOCUSABLE_BY_DEFAULT: &[&str] = &[
    "LineEdit",
    "TextEdit",
    "CodeEdit",
    "SpinBox",
    "OptionButton",
    "MenuButton",
    "CheckBox",
    "CheckButton",
    "Button",
    "LinkButton",
    "TextureButton",
    "BaseButton",
    "HSlider",
    "VSlider",
    "ItemList",
    "Tree",
    "TabBar",
];

/// The effective [`FocusMode`] for `node_id`: the explicit `focus_mode`
/// property when present, otherwise the Godot class default.
///
/// Unlike [`get_focus_mode`], which reports only what the property literally
/// holds, this is what the runtime acts on. Any non-zero stored value that is
/// not `FOCUS_CLICK` is treated as `FOCUS_ALL`, so an unknown future mode stays
/// focusable rather than silently dropping out of the tab ring.
pub fn resolved_focus_mode(tree: &SceneTree, node_id: NodeId) -> FocusMode {
    let Some(node) = tree.get_node(node_id) else {
        return FocusMode::None;
    };
    match node.get_property("focus_mode") {
        Variant::Int(0) => FocusMode::None,
        Variant::Int(1) => FocusMode::Click,
        Variant::Int(_) => FocusMode::All,
        _ => {
            if node.is_class("BaseButton") || FOCUSABLE_BY_DEFAULT.contains(&node.class_name()) {
                FocusMode::All
            } else {
                FocusMode::None
            }
        }
    }
}

/// Whether keyboard navigation (Tab, arrows) may land on `node_id`.
///
/// Godot requires `FOCUS_ALL` *and* visibility in tree — a `FOCUS_CLICK`
/// control can be clicked into but is skipped by Tab
/// (`Control::find_next_valid_focus`).
pub fn is_keyboard_focusable(tree: &SceneTree, node_id: NodeId) -> bool {
    resolved_focus_mode(tree, node_id) == FocusMode::All
        && crate::node2d::is_visible_in_tree(tree, node_id)
}

/// Whether `node_id` can hold focus at all — `FOCUS_CLICK` or `FOCUS_ALL`, and
/// visible. This is Godot's `Control::_is_focusable`, which is what an explicit
/// `focus_next`/`focus_neighbor_*` override is validated against.
pub fn is_focusable(tree: &SceneTree, node_id: NodeId) -> bool {
    resolved_focus_mode(tree, node_id) != FocusMode::None
        && crate::node2d::is_visible_in_tree(tree, node_id)
}

/// The four sides used by `focus_neighbor_*`, in Godot's `Side` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusSide {
    /// `SIDE_LEFT` — arrow-left / `ui_left`.
    Left,
    /// `SIDE_TOP` — arrow-up / `ui_up`.
    Top,
    /// `SIDE_RIGHT` — arrow-right / `ui_right`.
    Right,
    /// `SIDE_BOTTOM` — arrow-down / `ui_down`.
    Bottom,
}

impl FocusSide {
    /// The property name Godot serializes this side's override under.
    pub fn property(self) -> &'static str {
        match self {
            FocusSide::Left => "focus_neighbor_left",
            FocusSide::Top => "focus_neighbor_top",
            FocusSide::Right => "focus_neighbor_right",
            FocusSide::Bottom => "focus_neighbor_bottom",
        }
    }

    /// The unit vector pointing away from the control along this side.
    fn direction(self) -> Vector2 {
        match self {
            FocusSide::Left => Vector2::new(-1.0, 0.0),
            FocusSide::Top => Vector2::new(0.0, -1.0),
            FocusSide::Right => Vector2::new(1.0, 0.0),
            FocusSide::Bottom => Vector2::new(0.0, 1.0),
        }
    }
}

/// Sets a `focus_neighbor_*` node path override for arrow-key navigation.
pub fn set_focus_neighbor(tree: &mut SceneTree, node_id: NodeId, side: FocusSide, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(side.property(), Variant::String(path.to_owned()));
    }
}

/// Gets a `focus_neighbor_*` path override, if set.
pub fn get_focus_neighbor(tree: &SceneTree, node_id: NodeId, side: FocusSide) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property(side.property()) {
            Variant::String(s) if !s.is_empty() => Some(s),
            Variant::NodePath(p) if !p.to_string().is_empty() => Some(p.to_string()),
            _ => None,
        })
}

/// Resolves a focus-override NodePath, accepting both the absolute form
/// (`/root/Main/Next`) and the relative form (`../Next`) a scene author writes.
fn resolve_focus_path(tree: &SceneTree, from: NodeId, path: &str) -> Option<NodeId> {
    if path.starts_with('/') {
        tree.get_node_by_path(path)
    } else {
        tree.get_node_relative(from, path)
    }
}

/// Every Control in the tree, in Godot's focus-walk order (depth-first,
/// top-down). `find_next_valid_focus` steps forward through this sequence and
/// `find_prev_valid_focus` steps backward — Godot's `_next_control` /
/// `_prev_control` are exactly the successor and predecessor of a preorder
/// walk.
fn controls_in_focus_order(tree: &SceneTree) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|&id| node_is_control(tree, id))
        .collect()
}

/// Shared body of [`find_next_valid_focus`] and [`find_prev_valid_focus`]:
/// honor the explicit override, else scan the focus order in `step` direction
/// from `from`, wrapping around, for the first keyboard-focusable Control.
fn find_valid_focus(
    tree: &SceneTree,
    from: NodeId,
    override_path: Option<String>,
    forward: bool,
) -> Option<NodeId> {
    // Godot tries the manual override first and only falls through to the walk
    // when the target is not focusable (control.cpp:2419-2428).
    if let Some(path) = override_path {
        if let Some(target) = resolve_focus_path(tree, from, &path) {
            if is_focusable(tree, target) {
                return Some(target);
            }
        }
    }

    let order = controls_in_focus_order(tree);
    let start = order.iter().position(|&id| id == from)?;
    let len = order.len();
    // Start at the neighbor and wrap all the way back to `from`, so a ring of
    // one focusable control returns itself rather than nothing.
    (1..=len)
        .map(|i| {
            let offset = if forward { i } else { len - i };
            order[(start + offset) % len]
        })
        .find(|&id| id != from && is_keyboard_focusable(tree, id))
}

/// The Control that Tab should move focus to, or `None` when nothing else can
/// take keyboard focus (Godot `Control.find_next_valid_focus`).
pub fn find_next_valid_focus(tree: &SceneTree, from: NodeId) -> Option<NodeId> {
    find_valid_focus(tree, from, get_focus_next(tree, from), true)
}

/// The Control that Shift-Tab should move focus to (Godot
/// `Control.find_prev_valid_focus`).
pub fn find_prev_valid_focus(tree: &SceneTree, from: NodeId) -> Option<NodeId> {
    find_valid_focus(tree, from, get_focus_previous(tree, from), false)
}

/// The first keyboard-focusable Control in the tree — where Tab lands when
/// nothing holds focus yet.
pub fn first_keyboard_focusable(tree: &SceneTree) -> Option<NodeId> {
    controls_in_focus_order(tree)
        .into_iter()
        .find(|&id| is_keyboard_focusable(tree, id))
}

/// The Control an arrow key should move focus to (Godot
/// `Control.find_valid_focus_neighbor`).
///
/// An explicit `focus_neighbor_<side>` wins. Otherwise this is Godot's
/// geometric search (control.cpp `_get_focus_neighbor` /
/// `_window_find_focus_neighbor`): among the keyboard-focusable Controls that
/// extend past `from`'s far edge along the side's direction, pick the one with
/// the smallest squared gap between the two rects.
pub fn find_valid_focus_neighbor(
    tree: &SceneTree,
    from: NodeId,
    side: FocusSide,
) -> Option<NodeId> {
    if let Some(path) = get_focus_neighbor(tree, from, side) {
        if let Some(target) = resolve_focus_path(tree, from, &path) {
            if is_focusable(tree, target) {
                return Some(target);
            }
        }
    }

    let dir = side.direction();
    let from_pos = get_global_position(tree, from);
    let from_size = get_size(tree, from);
    let from_center = from_pos + from_size * 0.5;
    // How far `from` itself reaches along the direction — a candidate must beat
    // this to count as "on that side" (control.cpp:2676-2678).
    let min_d = dir.dot(from_pos).max(dir.dot(from_pos + from_size));

    let mut best: Option<(f32, NodeId)> = None;
    for id in controls_in_focus_order(tree) {
        if id == from || !is_keyboard_focusable(tree, id) {
            continue;
        }
        let pos = get_global_position(tree, id);
        let size = get_size(tree, id);
        let reach = dir.dot(pos).max(dir.dot(pos + size));
        if reach <= min_d + f32::EPSILON {
            continue;
        }

        // Godot's shortest rect-to-rect distance: mirror the candidate into the
        // first quadrant, then measure the gap left after removing both
        // half-extents. Overlapping axes contribute nothing.
        let center_delta = (pos + size * 0.5) - from_center;
        let abs = Vector2::new(center_delta.x.abs(), center_delta.y.abs());
        let gap = abs - size * 0.5 - from_size * 0.5;
        let dist_sq = if gap.x > 0.0 { gap.x * gap.x } else { 0.0 }
            + if gap.y > 0.0 { gap.y * gap.y } else { 0.0 };

        let better = match best {
            Some((best_dist, _)) => dist_sq < best_dist,
            None => true,
        };
        if better {
            best = Some((dist_sq, id));
        }
    }
    best.map(|(_, id)| id)
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

// ---------------------------------------------------------------------------
// Theme color override resolution (pat-nzgdu.89)
// ---------------------------------------------------------------------------

/// Godot `Control.add_theme_color_override(name, color)`: stores a per-node
/// color override under the `theme_override_colors/<name>` property, which wins
/// over any assigned or default Theme during resolution.
pub fn add_theme_color_override(tree: &mut SceneTree, node_id: NodeId, name: &str, color: Color) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            &format!("theme_override_colors/{name}"),
            Variant::Color(color),
        );
    }
}

/// Whether a local `theme_override_colors/<name>` override is set on the node.
pub fn has_theme_color_override(tree: &SceneTree, node_id: NodeId, name: &str) -> bool {
    tree.get_node(node_id)
        .map(|n| {
            matches!(
                n.get_property(&format!("theme_override_colors/{name}")),
                Variant::Color(_)
            )
        })
        .unwrap_or(false)
}

/// Godot `Control.remove_theme_color_override(name)`: clears the local override
/// so resolution falls back to the assigned/default Theme.
pub fn remove_theme_color_override(tree: &mut SceneTree, node_id: NodeId, name: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&format!("theme_override_colors/{name}"), Variant::Nil);
    }
}

/// Resolves a Control's theme color for `name`, mirroring Godot's lookup order:
/// (1) a local `add_theme_color_override`; then (2) the `assigned` Theme,
/// checking the `theme_type_variation` type first and then the control's own
/// type; then (3) the `default` Theme with the same variation/type order.
/// Returns `None` if the color is unset everywhere.
/// The `theme_override_*` property prefix Godot uses for each data type
/// (pat-0q2kx.180).
///
/// These names are the wire format: a `.tscn` writes
/// `theme_override_font_sizes/font_size = 24`, so getting one wrong means the
/// override is silently ignored rather than erroring.
pub fn theme_override_prefix(kind: crate::theme::ThemePropertyType) -> &'static str {
    use crate::theme::ThemePropertyType as T;
    match kind {
        T::Color => "theme_override_colors",
        T::Constant => "theme_override_constants",
        T::Font => "theme_override_fonts",
        T::FontSize => "theme_override_font_sizes",
        T::Stylebox => "theme_override_styles",
        T::Icon => "theme_override_icons",
    }
}

/// Godot's full theme lookup for any of the six data types (pat-0q2kx.180).
///
/// `Control::_get_theme_item` (`scene/gui/control.cpp`) resolves, in order:
///
/// 1. the node's own `theme_override_<kind>/<name>`;
/// 2. the node's own theme resource;
/// 3. **each ancestor's** theme resource, nearest first;
/// 4. the default (project/ThemeDB) theme.
///
/// At every theme it tries `theme_type_variation` before `control_type`, so a
/// `BigButton` entry wins over `Button`.
///
/// The previous [`resolve_theme_color`] handled colours only and stopped at
/// step 2 — it never walked ancestors, so a theme set on a parent Control could
/// not reach its children at all. It also had no production callers.
///
/// `node_themes` maps a node to the theme resource assigned to it. Callers with
/// no per-node themes pass an empty map and get the override → default chain.
pub fn resolve_theme_item(
    tree: &SceneTree,
    node_id: NodeId,
    kind: crate::theme::ThemePropertyType,
    control_type: &str,
    variation: &str,
    name: &str,
    node_themes: &std::collections::HashMap<NodeId, crate::theme::Theme>,
    default: &crate::theme::Theme,
) -> Option<Variant> {
    // 1. A local override beats every theme in the chain.
    if let Some(node) = tree.get_node(node_id) {
        let key = format!("{}/{name}", theme_override_prefix(kind));
        let v = node.get_property(&key);
        if !matches!(v, Variant::Nil) {
            return Some(v);
        }
    }

    // 2 + 3. This node's theme, then each ancestor's, nearest first. Godot
    // stops at the first theme that HAS the item rather than at the first node
    // that has a theme, so a parent theme still answers when the child's theme
    // is silent about this item.
    let mut current = Some(node_id);
    while let Some(id) = current {
        if let Some(theme) = node_themes.get(&id) {
            if let Some(v) = theme_item_with_variation(theme, kind, control_type, variation, name) {
                return Some(v);
            }
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }

    // 4. Default theme, variation first.
    theme_item_with_variation(default, kind, control_type, variation, name)
}

/// One theme's lookup: the variation type before the base control type.
fn theme_item_with_variation(
    theme: &crate::theme::Theme,
    kind: crate::theme::ThemePropertyType,
    control_type: &str,
    variation: &str,
    name: &str,
) -> Option<Variant> {
    if !variation.is_empty() {
        if let Some(v) = theme_item_typed(theme, kind, variation, name) {
            return Some(v);
        }
    }
    theme_item_typed(theme, kind, control_type, name)
}

/// Reads one item, preferring the typed maps (which the `.tscn` loader and the
/// default theme populate) and falling back to the generic store.
fn theme_item_typed(
    theme: &crate::theme::Theme,
    kind: crate::theme::ThemePropertyType,
    control_type: &str,
    name: &str,
) -> Option<Variant> {
    use crate::theme::ThemePropertyType as T;
    match kind {
        T::Color => theme.get_color(control_type, name).map(Variant::Color),
        T::Constant => theme.get_constant(control_type, name).map(Variant::Int),
        _ => None,
    }
    .or_else(|| theme.get_item(kind, control_type, name).cloned())
}

pub fn resolve_theme_color(
    tree: &SceneTree,
    node_id: NodeId,
    control_type: &str,
    variation: &str,
    assigned: Option<&crate::theme::Theme>,
    default: &crate::theme::Theme,
    name: &str,
) -> Option<Color> {
    // 1. Local override wins.
    if let Some(node) = tree.get_node(node_id) {
        if let Variant::Color(c) = node.get_property(&format!("theme_override_colors/{name}")) {
            return Some(c);
        }
    }
    // 2. Assigned Theme: variation type first, then the base control type.
    if let Some(theme) = assigned {
        if !variation.is_empty() {
            if let Some(c) = theme.get_color(variation, name) {
                return Some(c);
            }
        }
        if let Some(c) = theme.get_color(control_type, name) {
            return Some(c);
        }
    }
    // 3. Default Theme: variation type first, then the base control type.
    if !variation.is_empty() {
        if let Some(c) = default.get_color(variation, name) {
            return Some(c);
        }
    }
    default.get_color(control_type, name)
}

// ---------------------------------------------------------------------------
// Mouse cursor shape (pat-0q2kx.175)
// ---------------------------------------------------------------------------

/// Godot's `Control.CursorShape`, in `scene/gui/control.h` declaration order.
///
/// `Arrow` is enum-zero **and** the documented default of
/// `mouse_default_cursor_shape`, which is unusual for a Godot enum — most default
/// to something other than their zero value, so this one is safe to fall back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// `CURSOR_ARROW` — the default.
    #[default]
    Arrow,
    /// `CURSOR_IBEAM` — text editing.
    IBeam,
    /// `CURSOR_POINTING_HAND` — links.
    PointingHand,
    /// `CURSOR_CROSS`.
    Cross,
    /// `CURSOR_WAIT`.
    Wait,
    /// `CURSOR_BUSY`.
    Busy,
    /// `CURSOR_DRAG` — a drag in progress.
    Drag,
    /// `CURSOR_CAN_DROP` — hovering a valid drop target.
    CanDrop,
    /// `CURSOR_FORBIDDEN`.
    Forbidden,
    /// `CURSOR_VSIZE` — vertical resize.
    VSize,
    /// `CURSOR_HSIZE` — horizontal resize.
    HSize,
    /// `CURSOR_BDIAGSIZE`.
    BDiagSize,
    /// `CURSOR_FDIAGSIZE`.
    FDiagSize,
    /// `CURSOR_MOVE`.
    Move,
    /// `CURSOR_VSPLIT`.
    VSplit,
    /// `CURSOR_HSPLIT`.
    HSplit,
    /// `CURSOR_HELP`.
    Help,
}

impl CursorShape {
    /// Maps Godot's integer constant onto a shape, falling back to [`Arrow`] for
    /// anything out of range.
    ///
    /// [`Arrow`]: CursorShape::Arrow
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::IBeam,
            2 => Self::PointingHand,
            3 => Self::Cross,
            4 => Self::Wait,
            5 => Self::Busy,
            6 => Self::Drag,
            7 => Self::CanDrop,
            8 => Self::Forbidden,
            9 => Self::VSize,
            10 => Self::HSize,
            11 => Self::BDiagSize,
            12 => Self::FDiagSize,
            13 => Self::Move,
            14 => Self::VSplit,
            15 => Self::HSplit,
            16 => Self::Help,
            _ => Self::Arrow,
        }
    }

    /// The Godot integer constant for this shape.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Arrow => 0,
            Self::IBeam => 1,
            Self::PointingHand => 2,
            Self::Cross => 3,
            Self::Wait => 4,
            Self::Busy => 5,
            Self::Drag => 6,
            Self::CanDrop => 7,
            Self::Forbidden => 8,
            Self::VSize => 9,
            Self::HSize => 10,
            Self::BDiagSize => 11,
            Self::FDiagSize => 12,
            Self::Move => 13,
            Self::VSplit => 14,
            Self::HSplit => 15,
            Self::Help => 16,
        }
    }

    /// The CSS `cursor` keyword for this shape, so the web shell can set it
    /// directly (pat-0q2kx.175).
    pub fn css_name(self) -> &'static str {
        match self {
            Self::Arrow => "default",
            Self::IBeam => "text",
            Self::PointingHand => "pointer",
            Self::Cross => "crosshair",
            Self::Wait => "wait",
            Self::Busy => "progress",
            Self::Drag => "grabbing",
            Self::CanDrop => "copy",
            Self::Forbidden => "not-allowed",
            Self::VSize => "ns-resize",
            Self::HSize => "ew-resize",
            Self::BDiagSize => "nesw-resize",
            Self::FDiagSize => "nwse-resize",
            Self::Move => "move",
            Self::VSplit => "row-resize",
            Self::HSplit => "col-resize",
            Self::Help => "help",
        }
    }
}

/// The cursor shape a control declares — Godot's `Control.get_cursor_shape`
/// default behaviour, which returns `mouse_default_cursor_shape`
/// (pat-0q2kx.175).
///
/// A node with no such property (or a non-integer value) resolves to
/// [`CursorShape::Arrow`], matching the registered default rather than failing.
pub fn get_cursor_shape(tree: &SceneTree, node_id: NodeId) -> CursorShape {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property("mouse_default_cursor_shape"))
    {
        Some(Variant::Int(i)) => CursorShape::from_godot_int(i),
        _ => CursorShape::Arrow,
    }
}

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
        assert_eq!(get_h_size_flags(&tree, tree.root_id()), SizeFlags::FILL);
        assert_eq!(get_v_size_flags(&tree, tree.root_id()), SizeFlags::FILL);
    }

    #[test]
    fn set_get_size_flags() {
        let mut tree = make_tree();
        let id = add_control(&mut tree, "Ctrl", "Control");

        set_h_size_flags(&mut tree, id, SizeFlags::EXPAND_FILL);
        set_v_size_flags(&mut tree, id, SizeFlags::SHRINK_CENTER);

        assert_eq!(get_h_size_flags(&tree, id), SizeFlags::EXPAND_FILL);
        assert_eq!(get_v_size_flags(&tree, id), SizeFlags::SHRINK_CENTER);
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
    fn split_container_layout_test() {
        // (1) The split + subviewport container family registers in ClassDB.
        gdobject::class_db::register_2d_classes();
        for cls in [
            "SplitContainer",
            "HSplitContainer",
            "VSplitContainer",
            "SubViewportContainer",
        ] {
            assert!(
                gdobject::class_db::class_exists(cls),
                "{cls} is registered in ClassDB",
            );
        }

        // (2) Dragging an HSplitContainer divider repositions both children.
        let (mut tree, split, kids) = container_with_children("HSplitContainer", 2);
        let total = Vector2::new(200.0, 100.0);
        arrange_container(&mut tree, split, total);
        let a0 = get_size(&tree, kids[0]);
        let b0 = get_size(&tree, kids[1]);
        // Drag the divider 40px right of center.
        set_split_offset(&mut tree, split, 40.0);
        arrange_container(&mut tree, split, total);
        let a1 = get_size(&tree, kids[0]);
        let b1 = get_size(&tree, kids[1]);
        assert_ne!(a0, a1, "dragging the divider resizes the first pane");
        assert_ne!(b0, b1, "dragging the divider resizes the second pane");
        let sep = get_separation(&tree, split) as f32;
        assert!(
            (a1.x + b1.x + sep - total.x).abs() < 0.01,
            "the two panes tile the container width (minus the divider gap)",
        );

        // (3) A SubViewportContainer forwards a click into sub-viewport space.
        let mapped = subviewport_container_to_viewport_coord(Vector2::new(100.0, 80.0), true, 2);
        assert_eq!(
            mapped,
            Vector2::new(50.0, 40.0),
            "a click divides by stretch_shrink into the sub-viewport's coordinate space",
        );
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

    // -- pat-nzgdu.142 (clause c): RTL layout mirroring ----------------------
    //
    // NOTE: a stray `#[test]` used to sit above this banner, giving the function
    // below TWO `#[test]` attributes — E0428, which stopped `-p gdscene --lib`
    // compiling entirely. It went unnoticed for a long time because `--test
    // <name>` targets do NOT build a crate's `#[cfg(test)]` module, so every
    // integration test stayed green while the unit-test target was dead
    // (removed under pat-0q2kx.196).

    /// The bead's RTL clause: a Control with `layout_direction = RTL` mirrors
    /// its children's positions — the first child sits at the right edge — while
    /// sizes are untouched.
    #[test]
    fn rtl_layout_mirrors_hbox_child_positions() {
        // Two Expand children split 200px evenly -> 100px each.
        let (mut tree, cid, kids) = container_with_children("HBoxContainer", 2);
        set_h_size_flags(&mut tree, kids[0], SizeFlags::EXPAND_FILL);
        set_h_size_flags(&mut tree, kids[1], SizeFlags::EXPAND_FILL);
        set_layout_direction(&mut tree, cid, LayoutDirection::Rtl);
        assert!(is_layout_rtl(&tree, cid));

        arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

        // Sizes are unchanged by direction.
        assert_eq!(get_size(&tree, kids[0]).x, 100.0);
        assert_eq!(get_size(&tree, kids[1]).x, 100.0);
        // Mirrored: child 0 (first in logical order) is on the RIGHT.
        assert_eq!(
            get_position(&tree, kids[0]).x,
            100.0,
            "the first child must sit at the right edge under RTL",
        );
        assert_eq!(get_position(&tree, kids[1]).x, 0.0);
    }

    /// LTR is the default, so existing scenes lay out exactly as before.
    #[test]
    fn ltr_is_the_default_and_unmirrored() {
        let (mut tree, cid, kids) = container_with_children("HBoxContainer", 2);
        set_h_size_flags(&mut tree, kids[0], SizeFlags::EXPAND_FILL);
        set_h_size_flags(&mut tree, kids[1], SizeFlags::EXPAND_FILL);
        assert_eq!(get_layout_direction(&tree, cid), LayoutDirection::Inherited);
        assert!(!is_layout_rtl(&tree, cid), "unset layout is LTR");

        arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

        assert_eq!(get_position(&tree, kids[0]).x, 0.0);
        assert_eq!(get_position(&tree, kids[1]).x, 100.0);
    }

    /// `Inherited` (the default) resolves up the parent chain, so marking an
    /// ancestor RTL flips its descendant containers — Godot's behaviour.
    #[test]
    fn rtl_is_inherited_from_an_ancestor() {
        let (mut tree, cid, _kids) = container_with_children("HBoxContainer", 2);
        let parent = tree.get_node(cid).unwrap().parent().expect("has a parent");

        // The container itself stays Inherited; the ancestor is RTL.
        set_layout_direction(&mut tree, parent, LayoutDirection::Rtl);
        assert_eq!(get_layout_direction(&tree, cid), LayoutDirection::Inherited);
        assert!(
            is_layout_rtl(&tree, cid),
            "Inherited must resolve RTL from an ancestor",
        );

        // An explicit LTR on the container overrides the RTL ancestor.
        set_layout_direction(&mut tree, cid, LayoutDirection::Ltr);
        assert!(
            !is_layout_rtl(&tree, cid),
            "explicit LTR wins over inheritance"
        );
    }

    /// Godot's LayoutDirection integers round-trip (0=Inherited, 3=RTL).
    #[test]
    fn layout_direction_ints_match_godot() {
        for (i, d) in [
            (0, LayoutDirection::Inherited),
            (1, LayoutDirection::Locale),
            (2, LayoutDirection::Ltr),
            (3, LayoutDirection::Rtl),
        ] {
            assert_eq!(LayoutDirection::from_int(i), d);
            assert_eq!(d.to_int(), i);
        }
        assert_eq!(LayoutDirection::default(), LayoutDirection::Inherited);
    }

    #[test]
    fn hbox_expand_space_splits_by_stretch_ratio() {
        // Two Expand children with stretch ratios 1 and 3 share the container's
        // 200px width 1:3 (no min sizes, no separation) -> 50 / 150.
        let (mut tree, cid, kids) = container_with_children("HBoxContainer", 2);
        set_h_size_flags(&mut tree, kids[0], SizeFlags::EXPAND_FILL);
        set_h_size_flags(&mut tree, kids[1], SizeFlags::EXPAND_FILL);
        set_stretch_ratio(&mut tree, kids[0], 1.0);
        set_stretch_ratio(&mut tree, kids[1], 3.0);

        arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

        assert_eq!(get_size(&tree, kids[0]).x, 50.0);
        assert_eq!(get_size(&tree, kids[1]).x, 150.0);
        // Laid out sequentially with no separation.
        assert_eq!(get_position(&tree, kids[0]).x, 0.0);
        assert_eq!(get_position(&tree, kids[1]).x, 50.0);

        // No regression: equal (default) ratios still split expand space evenly.
        let (mut tree2, cid2, kids2) = container_with_children("HBoxContainer", 2);
        set_h_size_flags(&mut tree2, kids2[0], SizeFlags::EXPAND_FILL);
        set_h_size_flags(&mut tree2, kids2[1], SizeFlags::EXPAND_FILL);
        arrange_container(&mut tree2, cid2, Vector2::new(200.0, 100.0));
        assert_eq!(get_size(&tree2, kids2[0]).x, 100.0);
        assert_eq!(get_size(&tree2, kids2[1]).x, 100.0);
    }

    #[test]
    fn hflow_container_wraps_children_when_width_overflows() {
        // Three 100x20 children in a 250-wide HFlowContainer: two fit on the
        // first row (0, 100), the third overflows (100+100 > 250 fits, but
        // 200+100 > 250) and wraps to a new row at y = row height (20).
        let (mut tree, cid, kids) = container_with_children("HFlowContainer", 3);
        for &k in &kids {
            set_custom_minimum_size(&mut tree, k, Vector2::new(100.0, 20.0));
        }
        arrange_container(&mut tree, cid, Vector2::new(250.0, 200.0));

        assert_eq!(get_position(&tree, kids[0]), Vector2::new(0.0, 0.0));
        assert_eq!(get_position(&tree, kids[1]), Vector2::new(100.0, 0.0));
        // Wrapped to the second row.
        assert_eq!(get_position(&tree, kids[2]), Vector2::new(0.0, 20.0));
        // Each child keeps its own minimum size.
        assert_eq!(get_size(&tree, kids[2]), Vector2::new(100.0, 20.0));
    }

    #[test]
    fn vflow_container_wraps_children_when_height_overflows() {
        // Vertical flow: three 20x100 children in a 250-tall VFlowContainer fill
        // the first column top-to-bottom (0, 100) then wrap to a new column at
        // x = column width (20).
        let (mut tree, cid, kids) = container_with_children("VFlowContainer", 3);
        for &k in &kids {
            set_custom_minimum_size(&mut tree, k, Vector2::new(20.0, 100.0));
        }
        arrange_container(&mut tree, cid, Vector2::new(200.0, 250.0));

        assert_eq!(get_position(&tree, kids[0]), Vector2::new(0.0, 0.0));
        assert_eq!(get_position(&tree, kids[1]), Vector2::new(0.0, 100.0));
        // Wrapped to the second column.
        assert_eq!(get_position(&tree, kids[2]), Vector2::new(20.0, 0.0));
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
