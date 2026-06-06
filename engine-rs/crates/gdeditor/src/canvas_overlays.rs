//! Toggleable canvas debug overlays for the 2D viewport.
//!
//! The editor can draw several diagnostic overlays on the canvas — the
//! visibility rect, navigation polygons, and the y-sort ordering preview —
//! each independently toggled from the View menu. An overlay is drawn only
//! while enabled; toggling it off removes it from the draw set.

/// A canvas debug overlay that can be drawn over the 2D viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanvasOverlay {
    /// The camera/visibility rectangle.
    VisibilityRect,
    /// Navigation polygons.
    NavigationPolygons,
    /// The y-sort ordering preview.
    YSortPreview,
}

/// Tracks which canvas debug overlays are currently enabled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasOverlays {
    visibility_rect: bool,
    navigation: bool,
    y_sort: bool,
}

impl CanvasOverlays {
    /// Creates an overlay set with every overlay disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every overlay kind, in display order.
    pub fn all() -> [CanvasOverlay; 3] {
        [
            CanvasOverlay::VisibilityRect,
            CanvasOverlay::NavigationPolygons,
            CanvasOverlay::YSortPreview,
        ]
    }

    /// Enables or disables a specific overlay.
    pub fn set(&mut self, overlay: CanvasOverlay, enabled: bool) {
        match overlay {
            CanvasOverlay::VisibilityRect => self.visibility_rect = enabled,
            CanvasOverlay::NavigationPolygons => self.navigation = enabled,
            CanvasOverlay::YSortPreview => self.y_sort = enabled,
        }
    }

    /// Flips the enabled state of an overlay.
    pub fn toggle(&mut self, overlay: CanvasOverlay) {
        self.set(overlay, !self.is_enabled(overlay));
    }

    /// Whether `overlay` is currently enabled (and therefore drawn).
    pub fn is_enabled(&self, overlay: CanvasOverlay) -> bool {
        match overlay {
            CanvasOverlay::VisibilityRect => self.visibility_rect,
            CanvasOverlay::NavigationPolygons => self.navigation,
            CanvasOverlay::YSortPreview => self.y_sort,
        }
    }

    /// The overlays that should be drawn this frame, in display order.
    pub fn active(&self) -> Vec<CanvasOverlay> {
        Self::all()
            .into_iter()
            .filter(|&o| self.is_enabled(o))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-3w161): enabling the visibility-rect, navigation, and
    /// y-sort overlays each draws its respective overlay, and toggling off
    /// removes it.
    #[test]
    fn viewport_canvas_debug_overlays_toggle() {
        let mut overlays = CanvasOverlays::new();
        // Nothing is drawn initially.
        assert!(overlays.active().is_empty());

        // Enabling each overlay adds exactly it to the draw set.
        overlays.set(CanvasOverlay::VisibilityRect, true);
        assert!(overlays.is_enabled(CanvasOverlay::VisibilityRect));
        assert_eq!(overlays.active(), vec![CanvasOverlay::VisibilityRect]);

        overlays.set(CanvasOverlay::NavigationPolygons, true);
        overlays.set(CanvasOverlay::YSortPreview, true);
        assert_eq!(
            overlays.active(),
            vec![
                CanvasOverlay::VisibilityRect,
                CanvasOverlay::NavigationPolygons,
                CanvasOverlay::YSortPreview,
            ],
            "all three overlays draw when enabled, in display order"
        );

        // Toggling one off removes only that overlay.
        overlays.toggle(CanvasOverlay::NavigationPolygons);
        assert!(!overlays.is_enabled(CanvasOverlay::NavigationPolygons));
        assert_eq!(
            overlays.active(),
            vec![CanvasOverlay::VisibilityRect, CanvasOverlay::YSortPreview],
            "toggling navigation off removes just it"
        );

        // Toggling the rest off clears the draw set.
        overlays.toggle(CanvasOverlay::VisibilityRect);
        overlays.set(CanvasOverlay::YSortPreview, false);
        assert!(overlays.active().is_empty(), "no overlays drawn once all are off");
    }
}
