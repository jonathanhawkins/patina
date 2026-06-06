//! The viewport mode toolbar: select / pan / ruler tool modes and a live
//! zoom-percentage indicator.
//!
//! These are the viewport *navigation* modes (distinct from the transform
//! tools in [`crate::viewport_2d::ToolMode2D`]). Exactly one mode is active at a
//! time, and the toolbar surfaces a zoom indicator driven by the camera's
//! current zoom percentage.

/// A viewport toolbar tool mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportToolMode {
    /// Click / rectangle select nodes.
    Select,
    /// Pan (scroll) the view by dragging.
    Pan,
    /// Measure distances with the ruler.
    Ruler,
}

/// The viewport toolbar state: the active tool mode and the zoom indicator.
#[derive(Debug, Clone)]
pub struct ViewportToolbar {
    active: ViewportToolMode,
    zoom_percent: f32,
}

impl Default for ViewportToolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewportToolbar {
    /// Creates a toolbar defaulting to the Select mode at 100% zoom.
    pub fn new() -> Self {
        Self {
            active: ViewportToolMode::Select,
            zoom_percent: 100.0,
        }
    }

    /// All tool modes the toolbar exposes, in button order.
    pub fn modes() -> [ViewportToolMode; 3] {
        [
            ViewportToolMode::Select,
            ViewportToolMode::Pan,
            ViewportToolMode::Ruler,
        ]
    }

    /// The currently active tool mode.
    pub fn active_mode(&self) -> ViewportToolMode {
        self.active
    }

    /// Switches the active tool mode (pressing a toolbar button).
    pub fn set_mode(&mut self, mode: ViewportToolMode) {
        self.active = mode;
    }

    /// Whether `mode`'s button is the active (pressed) one.
    pub fn is_active(&self, mode: ViewportToolMode) -> bool {
        self.active == mode
    }

    /// Updates the zoom indicator from the camera's current zoom percentage.
    pub fn set_zoom_percent(&mut self, percent: f32) {
        self.zoom_percent = percent;
    }

    /// The current zoom percentage shown by the indicator.
    pub fn zoom_percent(&self) -> f32 {
        self.zoom_percent
    }

    /// The zoom indicator label, e.g. `"150%"` (rounded to a whole percent).
    pub fn zoom_label(&self) -> String {
        format!("{}%", self.zoom_percent.round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_2d::ViewportCamera2D;
    use gdcore::math::Vector2;

    /// Acceptance (pat-h88ip): the toolbar provides select, pan, and ruler tool
    /// modes with the active mode reflected in button state, and shows a live
    /// zoom-percentage indicator.
    #[test]
    fn viewport_mode_toolbar() {
        let mut toolbar = ViewportToolbar::new();

        // The toolbar exposes exactly the select / pan / ruler modes.
        assert_eq!(
            ViewportToolbar::modes(),
            [
                ViewportToolMode::Select,
                ViewportToolMode::Pan,
                ViewportToolMode::Ruler
            ]
        );

        // Default is Select at 100%.
        assert_eq!(toolbar.active_mode(), ViewportToolMode::Select);
        assert!(toolbar.is_active(ViewportToolMode::Select));
        assert_eq!(toolbar.zoom_label(), "100%");

        // Switching modes reflects in button state — exactly one is active.
        toolbar.set_mode(ViewportToolMode::Pan);
        assert!(toolbar.is_active(ViewportToolMode::Pan));
        assert!(!toolbar.is_active(ViewportToolMode::Select));

        toolbar.set_mode(ViewportToolMode::Ruler);
        assert_eq!(toolbar.active_mode(), ViewportToolMode::Ruler);
        let active = ViewportToolbar::modes()
            .iter()
            .filter(|m| toolbar.is_active(**m))
            .count();
        assert_eq!(active, 1, "exactly one toolbar mode is active at a time");

        // The zoom indicator is live: it tracks the camera's zoom percentage.
        let mut camera = ViewportCamera2D::new();
        let viewport = Vector2::new(800.0, 600.0);
        toolbar.set_zoom_percent(camera.zoom_percent());
        assert_eq!(toolbar.zoom_label(), "100%");

        camera.zoom_in(viewport);
        toolbar.set_zoom_percent(camera.zoom_percent());
        assert!(
            toolbar.zoom_percent() > 100.0,
            "the zoom indicator reflects a zoomed-in camera"
        );

        // The indicator rounds to a whole percent.
        toolbar.set_zoom_percent(149.6);
        assert_eq!(toolbar.zoom_label(), "150%");
    }
}
