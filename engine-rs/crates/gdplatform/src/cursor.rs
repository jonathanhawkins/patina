//! Cursor shape and custom cursor support.
//!
//! Mirrors Godot's `DisplayServer` cursor API:
//! - [`CursorShape`] — standard system cursor shapes (arrow, hand, crosshair, etc.)
//! - [`CustomCursor`] — RGBA image cursor with hotspot
//! - [`CursorManager`] — manages current cursor state, custom cursor registry

use crate::window::WindowIcon;

// ---------------------------------------------------------------------------
// CursorShape
// ---------------------------------------------------------------------------

/// Standard cursor shapes matching Godot's `DisplayServer.CursorShape` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CursorShape {
    /// Default arrow cursor.
    Arrow = 0,
    /// I-beam text cursor.
    Ibeam = 1,
    /// Pointing hand cursor (links, buttons).
    PointingHand = 2,
    /// Crosshair cursor.
    Cross = 3,
    /// Wait/busy cursor (hourglass).
    Wait = 4,
    /// Busy arrow (arrow with hourglass).
    Busy = 5,
    /// Drag cursor.
    Drag = 6,
    /// Can-drop cursor.
    CanDrop = 7,
    /// Forbidden/not-allowed cursor.
    Forbidden = 8,
    /// Vertical resize cursor.
    Vsize = 9,
    /// Horizontal resize cursor.
    Hsize = 10,
    /// Bottom-right to top-left diagonal resize.
    Bdiagsize = 11,
    /// Top-left to bottom-right diagonal resize.
    Fdiagsize = 12,
    /// Move/all-direction cursor.
    Move = 13,
    /// Vertical split cursor.
    Vsplit = 14,
    /// Horizontal split cursor.
    Hsplit = 15,
    /// Help cursor (arrow with question mark).
    Help = 16,
}

impl CursorShape {
    /// Total number of standard cursor shapes.
    pub const COUNT: usize = 17;

    /// Returns the shape name as a human-readable string.
    pub fn name(&self) -> &'static str {
        match self {
            CursorShape::Arrow => "Arrow",
            CursorShape::Ibeam => "I-Beam",
            CursorShape::PointingHand => "Pointing Hand",
            CursorShape::Cross => "Cross",
            CursorShape::Wait => "Wait",
            CursorShape::Busy => "Busy",
            CursorShape::Drag => "Drag",
            CursorShape::CanDrop => "Can Drop",
            CursorShape::Forbidden => "Forbidden",
            CursorShape::Vsize => "Vertical Resize",
            CursorShape::Hsize => "Horizontal Resize",
            CursorShape::Bdiagsize => "Back Diagonal Resize",
            CursorShape::Fdiagsize => "Forward Diagonal Resize",
            CursorShape::Move => "Move",
            CursorShape::Vsplit => "Vertical Split",
            CursorShape::Hsplit => "Horizontal Split",
            CursorShape::Help => "Help",
        }
    }

    /// Converts from a u8 value, returning `None` for out-of-range values.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(CursorShape::Arrow),
            1 => Some(CursorShape::Ibeam),
            2 => Some(CursorShape::PointingHand),
            3 => Some(CursorShape::Cross),
            4 => Some(CursorShape::Wait),
            5 => Some(CursorShape::Busy),
            6 => Some(CursorShape::Drag),
            7 => Some(CursorShape::CanDrop),
            8 => Some(CursorShape::Forbidden),
            9 => Some(CursorShape::Vsize),
            10 => Some(CursorShape::Hsize),
            11 => Some(CursorShape::Bdiagsize),
            12 => Some(CursorShape::Fdiagsize),
            13 => Some(CursorShape::Move),
            14 => Some(CursorShape::Vsplit),
            15 => Some(CursorShape::Hsplit),
            16 => Some(CursorShape::Help),
            _ => None,
        }
    }
}

impl Default for CursorShape {
    fn default() -> Self {
        CursorShape::Arrow
    }
}

// ---------------------------------------------------------------------------
// CustomCursor
// ---------------------------------------------------------------------------

/// A custom cursor created from an RGBA image with a hotspot offset.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomCursor {
    /// The cursor image (RGBA pixels).
    pub image: WindowIcon,
    /// X offset of the click hotspot from the top-left corner.
    pub hotspot_x: u32,
    /// Y offset of the click hotspot from the top-left corner.
    pub hotspot_y: u32,
}

impl CustomCursor {
    /// Creates a new custom cursor from an image and hotspot.
    ///
    /// The hotspot is clamped to the image dimensions.
    pub fn new(image: WindowIcon, hotspot_x: u32, hotspot_y: u32) -> Self {
        Self {
            hotspot_x: hotspot_x.min(image.width.saturating_sub(1)),
            hotspot_y: hotspot_y.min(image.height.saturating_sub(1)),
            image,
        }
    }

    /// Returns the image dimensions as (width, height).
    pub fn size(&self) -> (u32, u32) {
        (self.image.width, self.image.height)
    }
}

// ---------------------------------------------------------------------------
// CursorManager
// ---------------------------------------------------------------------------

/// Manages cursor state: current shape, custom cursor overrides, and visibility.
///
/// Mirrors Godot's `DisplayServer` cursor management. Custom cursors can be
/// registered per [`CursorShape`], replacing the default system cursor for
/// that shape.
#[derive(Debug)]
pub struct CursorManager {
    /// The currently active cursor shape.
    current_shape: CursorShape,
    /// Custom cursor overrides indexed by CursorShape discriminant.
    custom_cursors: [Option<CustomCursor>; CursorShape::COUNT],
    /// Whether the cursor is visible.
    visible: bool,
    /// Whether the cursor is confined to the window.
    confined: bool,
    /// Current cursor position (set by the windowing backend).
    position: (f32, f32),
}

impl CursorManager {
    /// Creates a new cursor manager with default state.
    pub fn new() -> Self {
        Self {
            current_shape: CursorShape::Arrow,
            custom_cursors: Default::default(),
            visible: true,
            confined: false,
            position: (0.0, 0.0),
        }
    }

    // -- Shape ----------------------------------------------------------------

    /// Returns the current cursor shape.
    pub fn current_shape(&self) -> CursorShape {
        self.current_shape
    }

    /// Sets the current cursor shape.
    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.current_shape = shape;
    }

    // -- Custom cursors -------------------------------------------------------

    /// Registers a custom cursor image for the given shape.
    ///
    /// When this shape is active, the custom cursor will be displayed
    /// instead of the default system cursor.
    pub fn set_custom_cursor(&mut self, shape: CursorShape, cursor: CustomCursor) {
        self.custom_cursors[shape as usize] = Some(cursor);
    }

    /// Clears the custom cursor for the given shape, reverting to default.
    pub fn clear_custom_cursor(&mut self, shape: CursorShape) {
        self.custom_cursors[shape as usize] = None;
    }

    /// Returns the custom cursor for the given shape, if one is set.
    pub fn get_custom_cursor(&self, shape: CursorShape) -> Option<&CustomCursor> {
        self.custom_cursors[shape as usize].as_ref()
    }

    /// Returns whether a custom cursor is set for the given shape.
    pub fn has_custom_cursor(&self, shape: CursorShape) -> bool {
        self.custom_cursors[shape as usize].is_some()
    }

    /// Returns the active custom cursor (for the current shape), if any.
    pub fn active_custom_cursor(&self) -> Option<&CustomCursor> {
        self.get_custom_cursor(self.current_shape)
    }

    /// Clears all custom cursors.
    pub fn clear_all_custom_cursors(&mut self) {
        for slot in &mut self.custom_cursors {
            *slot = None;
        }
    }

    // -- Visibility -----------------------------------------------------------

    /// Returns whether the cursor is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Sets cursor visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    // -- Confinement ----------------------------------------------------------

    /// Returns whether the cursor is confined to the window.
    pub fn is_confined(&self) -> bool {
        self.confined
    }

    /// Sets whether the cursor is confined to the window.
    pub fn set_confined(&mut self, confined: bool) {
        self.confined = confined;
    }

    // -- Position -------------------------------------------------------------

    /// Returns the current cursor position.
    pub fn position(&self) -> (f32, f32) {
        self.position
    }

    /// Updates the cursor position (called by the windowing backend).
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.position = (x, y);
    }
}

impl Default for CursorManager {
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

    fn test_icon(w: u32, h: u32) -> WindowIcon {
        WindowIcon::new(w, h, vec![0u8; (w * h * 4) as usize]).unwrap()
    }

    #[test]
    fn cursor_shape_default_is_arrow() {
        assert_eq!(CursorShape::default(), CursorShape::Arrow);
    }

    #[test]
    fn cursor_shape_from_u8_valid() {
        for i in 0..17u8 {
            assert!(
                CursorShape::from_u8(i).is_some(),
                "shape {i} should be valid"
            );
        }
    }

    #[test]
    fn cursor_shape_from_u8_invalid() {
        assert!(CursorShape::from_u8(17).is_none());
        assert!(CursorShape::from_u8(255).is_none());
    }

    #[test]
    fn cursor_shape_names_nonempty() {
        for i in 0..17u8 {
            let shape = CursorShape::from_u8(i).unwrap();
            assert!(!shape.name().is_empty());
        }
    }

    #[test]
    fn cursor_shape_count() {
        assert_eq!(CursorShape::COUNT, 17);
    }

    #[test]
    fn custom_cursor_clamps_hotspot() {
        let icon = test_icon(16, 16);
        let cursor = CustomCursor::new(icon, 100, 200);
        assert_eq!(cursor.hotspot_x, 15);
        assert_eq!(cursor.hotspot_y, 15);
    }

    #[test]
    fn custom_cursor_size() {
        let icon = test_icon(32, 24);
        let cursor = CustomCursor::new(icon, 0, 0);
        assert_eq!(cursor.size(), (32, 24));
    }

    #[test]
    fn manager_default_state() {
        let mgr = CursorManager::new();
        assert_eq!(mgr.current_shape(), CursorShape::Arrow);
        assert!(mgr.is_visible());
        assert!(!mgr.is_confined());
        assert_eq!(mgr.position(), (0.0, 0.0));
        assert!(mgr.active_custom_cursor().is_none());
    }

    #[test]
    fn manager_set_shape() {
        let mut mgr = CursorManager::new();
        mgr.set_cursor_shape(CursorShape::PointingHand);
        assert_eq!(mgr.current_shape(), CursorShape::PointingHand);
    }

    #[test]
    fn manager_custom_cursor_lifecycle() {
        let mut mgr = CursorManager::new();
        let icon = test_icon(16, 16);
        let cursor = CustomCursor::new(icon, 8, 8);

        mgr.set_custom_cursor(CursorShape::Arrow, cursor);
        assert!(mgr.has_custom_cursor(CursorShape::Arrow));
        assert!(mgr.active_custom_cursor().is_some());

        mgr.clear_custom_cursor(CursorShape::Arrow);
        assert!(!mgr.has_custom_cursor(CursorShape::Arrow));
        assert!(mgr.active_custom_cursor().is_none());
    }

    #[test]
    fn manager_visibility_toggle() {
        let mut mgr = CursorManager::new();
        mgr.set_visible(false);
        assert!(!mgr.is_visible());
        mgr.set_visible(true);
        assert!(mgr.is_visible());
    }

    #[test]
    fn manager_confinement_toggle() {
        let mut mgr = CursorManager::new();
        mgr.set_confined(true);
        assert!(mgr.is_confined());
    }

    #[test]
    fn manager_position_update() {
        let mut mgr = CursorManager::new();
        mgr.set_position(100.5, 200.3);
        let (x, y) = mgr.position();
        assert!((x - 100.5).abs() < f32::EPSILON);
        assert!((y - 200.3).abs() < f32::EPSILON);
    }

    #[test]
    fn manager_clear_all_custom_cursors() {
        let mut mgr = CursorManager::new();
        mgr.set_custom_cursor(CursorShape::Arrow, CustomCursor::new(test_icon(8, 8), 0, 0));
        mgr.set_custom_cursor(CursorShape::Cross, CustomCursor::new(test_icon(8, 8), 4, 4));
        assert!(mgr.has_custom_cursor(CursorShape::Arrow));
        assert!(mgr.has_custom_cursor(CursorShape::Cross));

        mgr.clear_all_custom_cursors();
        assert!(!mgr.has_custom_cursor(CursorShape::Arrow));
        assert!(!mgr.has_custom_cursor(CursorShape::Cross));
    }
}
