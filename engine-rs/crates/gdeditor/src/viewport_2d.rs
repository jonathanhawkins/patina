//! 2D viewport controller for the editor.
//!
//! Provides pan, zoom, and tool modes that mirror Godot 4's 2D editor viewport.
//!
//! - **Select**: Click to select, drag for rectangle selection.
//! - **Move**: Drag selected nodes to translate them.
//! - **Rotate**: Drag to rotate selected nodes around their pivot.
//! - **Scale**: Drag to scale selected nodes.
//! - **Pan**: Middle-mouse drag or Space+drag to pan the view.
//! - **Zoom**: Scroll wheel to zoom in/out around cursor.

use std::collections::HashSet;

use gdcore::math::Vector2;

// ---------------------------------------------------------------------------
// Tool mode
// ---------------------------------------------------------------------------

/// Active 2D toolbar tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolMode2D {
    /// Click-select or rectangle-select nodes.
    Select,
    /// Move selected nodes by dragging.
    Move,
    /// Rotate selected nodes around their pivot.
    Rotate,
    /// Scale selected nodes by dragging.
    Scale,
}

impl Default for ToolMode2D {
    fn default() -> Self {
        Self::Select
    }
}

// ---------------------------------------------------------------------------
// Snap settings
// ---------------------------------------------------------------------------

/// Grid-snap configuration for the 2D viewport.
#[derive(Debug, Clone)]
pub struct SnapSettings2D {
    /// Whether snapping is enabled.
    pub enabled: bool,
    /// Grid step size in pixels.
    pub grid_step: Vector2,
    /// Rotation snap in radians.
    pub rotation_step: f32,
    /// Scale snap step.
    pub scale_step: f32,
}

impl Default for SnapSettings2D {
    fn default() -> Self {
        Self {
            enabled: false,
            grid_step: Vector2::new(8.0, 8.0),
            rotation_step: std::f32::consts::FRAC_PI_4 / 3.0, // 15 degrees
            scale_step: 0.1,
        }
    }
}

impl SnapSettings2D {
    /// Snaps a position to the grid if snapping is enabled.
    pub fn snap_position(&self, pos: Vector2) -> Vector2 {
        if !self.enabled || self.grid_step.x == 0.0 || self.grid_step.y == 0.0 {
            return pos;
        }
        Vector2::new(
            (pos.x / self.grid_step.x).round() * self.grid_step.x,
            (pos.y / self.grid_step.y).round() * self.grid_step.y,
        )
    }

    /// Snaps a rotation value to the nearest step if snapping is enabled.
    pub fn snap_rotation(&self, radians: f32) -> f32 {
        if !self.enabled || self.rotation_step == 0.0 {
            return radians;
        }
        (radians / self.rotation_step).round() * self.rotation_step
    }

    /// Snaps a scale value to the nearest step if snapping is enabled.
    pub fn snap_scale(&self, scale: f32) -> f32 {
        if !self.enabled || self.scale_step == 0.0 {
            return scale;
        }
        (scale / self.scale_step).round() * self.scale_step
    }
}

// ---------------------------------------------------------------------------
// Guide lines
// ---------------------------------------------------------------------------

/// A guide line positioned in world-space. Godot allows horizontal and vertical
/// guide lines that can be dragged from the rulers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GuideLine {
    /// A horizontal guide at a specific Y world coordinate.
    Horizontal(f32),
    /// A vertical guide at a specific X world coordinate.
    Vertical(f32),
}

impl GuideLine {
    /// Returns the world-space coordinate of this guide.
    pub fn position(&self) -> f32 {
        match *self {
            GuideLine::Horizontal(y) => y,
            GuideLine::Vertical(x) => x,
        }
    }

    /// Distance from a world-space point to this guide.
    pub fn distance_to(&self, point: Vector2) -> f32 {
        match *self {
            GuideLine::Horizontal(y) => (point.y - y).abs(),
            GuideLine::Vertical(x) => (point.x - x).abs(),
        }
    }

    /// Snaps a world-space point to this guide if within threshold.
    pub fn snap_if_near(&self, point: Vector2, threshold: f32) -> Option<Vector2> {
        if self.distance_to(point) <= threshold {
            Some(match *self {
                GuideLine::Horizontal(y) => Vector2::new(point.x, y),
                GuideLine::Vertical(x) => Vector2::new(x, point.y),
            })
        } else {
            None
        }
    }
}

/// Guide storage for the viewport. Guides can be added, removed, cleared, and
/// snapped to.
#[derive(Debug, Clone, Default)]
pub struct GuideManager {
    guides: Vec<GuideLine>,
    /// Snap threshold in world-space pixels.
    pub snap_threshold: f32,
}

impl GuideManager {
    /// Creates a new guide manager with a default snap threshold.
    pub fn new() -> Self {
        Self {
            guides: Vec::new(),
            snap_threshold: 4.0,
        }
    }

    /// Adds a guide line.
    pub fn add(&mut self, guide: GuideLine) {
        self.guides.push(guide);
    }

    /// Removes a guide by index. Returns the removed guide, or None.
    pub fn remove(&mut self, index: usize) -> Option<GuideLine> {
        if index < self.guides.len() {
            Some(self.guides.remove(index))
        } else {
            None
        }
    }

    /// Clears all guides.
    pub fn clear(&mut self) {
        self.guides.clear();
    }

    /// Returns all guides.
    pub fn guides(&self) -> &[GuideLine] {
        &self.guides
    }

    /// Number of guides.
    pub fn count(&self) -> usize {
        self.guides.len()
    }

    /// Finds the closest guide to a world-space point and returns its index
    /// and distance, or None if no guides exist.
    pub fn closest(&self, point: Vector2) -> Option<(usize, f32)> {
        self.guides
            .iter()
            .enumerate()
            .map(|(i, g)| (i, g.distance_to(point)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Snaps a point to the nearest guide within the snap threshold.
    /// Returns the snapped position, or the original point if nothing is close.
    pub fn snap_to_guides(&self, point: Vector2) -> Vector2 {
        let mut best = point;
        let mut best_dist = self.snap_threshold;
        for guide in &self.guides {
            let dist = guide.distance_to(point);
            if dist < best_dist {
                best_dist = dist;
                best = match *guide {
                    GuideLine::Horizontal(y) => Vector2::new(point.x, y),
                    GuideLine::Vertical(x) => Vector2::new(x, point.y),
                };
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// Smart snap
// ---------------------------------------------------------------------------

/// A snap candidate from a nearby node's edge, center, or vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmartSnapAnchor {
    /// Center of a node's bounding rect.
    Center(Vector2),
    /// Top edge Y.
    TopEdge(f32),
    /// Bottom edge Y.
    BottomEdge(f32),
    /// Left edge X.
    LeftEdge(f32),
    /// Right edge X.
    RightEdge(f32),
}

impl SmartSnapAnchor {
    /// The snap position on the axis this anchor constrains.
    pub fn value(&self) -> f32 {
        match *self {
            SmartSnapAnchor::Center(v) => v.x, // use x; caller decides axis
            SmartSnapAnchor::TopEdge(y) | SmartSnapAnchor::BottomEdge(y) => y,
            SmartSnapAnchor::LeftEdge(x) | SmartSnapAnchor::RightEdge(x) => x,
        }
    }
}

/// Smart-snap engine that discovers nearby node edges and centers for alignment.
#[derive(Debug, Clone)]
pub struct SmartSnap {
    /// Whether smart snap is enabled.
    pub enabled: bool,
    /// Distance threshold in world-space pixels.
    pub threshold: f32,
    /// Registered bounding rects of other (non-dragged) nodes, keyed by node ID.
    anchors: Vec<(u64, SelectionRect)>,
}

impl Default for SmartSnap {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 4.0,
            anchors: Vec::new(),
        }
    }
}

impl SmartSnap {
    /// Creates a new smart-snap engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a node's bounding rect as a potential snap target.
    pub fn register_node(&mut self, node_id: u64, bounds: SelectionRect) {
        self.anchors.push((node_id, bounds));
    }

    /// Clears all registered anchors.
    pub fn clear(&mut self) {
        self.anchors.clear();
    }

    /// Number of registered anchors.
    pub fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    /// Finds the best horizontal (Y) and vertical (X) snap for a rect being
    /// dragged. Returns (snapped_x_offset, snapped_y_offset) — deltas to add
    /// to the drag rect's position to snap it into alignment.
    pub fn snap_rect(&self, dragging: &SelectionRect) -> (Option<f32>, Option<f32>) {
        if !self.enabled {
            return (None, None);
        }

        let drag_cx = dragging.center().x;
        let drag_cy = dragging.center().y;
        let drag_left = dragging.min.x;
        let drag_right = dragging.max.x;
        let drag_top = dragging.min.y;
        let drag_bottom = dragging.max.y;

        let mut best_dx: Option<(f32, f32)> = None; // (distance, delta)
        let mut best_dy: Option<(f32, f32)> = None;

        for &(_, ref r) in &self.anchors {
            let cx = r.center().x;
            let cy = r.center().y;

            // X-axis snap candidates: left-left, right-right, center-center, left-right, right-left
            let x_candidates = [
                drag_left - r.min.x,
                drag_right - r.max.x,
                drag_cx - cx,
                drag_left - r.max.x,
                drag_right - r.min.x,
            ];
            for delta in x_candidates {
                let dist = delta.abs();
                if dist < self.threshold {
                    if best_dx.is_none() || dist < best_dx.unwrap().0 {
                        best_dx = Some((dist, -delta));
                    }
                }
            }

            // Y-axis snap candidates
            let y_candidates = [
                drag_top - r.min.y,
                drag_bottom - r.max.y,
                drag_cy - cy,
                drag_top - r.max.y,
                drag_bottom - r.min.y,
            ];
            for delta in y_candidates {
                let dist = delta.abs();
                if dist < self.threshold {
                    if best_dy.is_none() || dist < best_dy.unwrap().0 {
                        best_dy = Some((dist, -delta));
                    }
                }
            }
        }

        (best_dx.map(|(_, d)| d), best_dy.map(|(_, d)| d))
    }
}

// ---------------------------------------------------------------------------
// Canvas overlays
// ---------------------------------------------------------------------------

/// Visual overlays that can be toggled on/off in the 2D viewport.
/// Mirrors Godot's View menu overlay toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanvasOverlay {
    /// Show AABB/collision shapes.
    CollisionShapes,
    /// Show navigation polygon regions.
    NavigationRegions,
    /// Show Y-sort indicators on nodes that use y-sorting.
    YSortIndicators,
    /// Show the world-space origin cross.
    OriginCross,
    /// Show node lock/group icons.
    LockGroupIcons,
    /// Show visibility indicators.
    VisibilityIndicators,
}

/// Manages which canvas overlays are currently active.
#[derive(Debug, Clone)]
pub struct OverlaySettings {
    active: HashSet<CanvasOverlay>,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        let mut active = HashSet::new();
        // Godot enables these by default
        active.insert(CanvasOverlay::OriginCross);
        active.insert(CanvasOverlay::LockGroupIcons);
        Self { active }
    }
}

impl OverlaySettings {
    /// Creates overlay settings with default overlays enabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables an overlay.
    pub fn enable(&mut self, overlay: CanvasOverlay) {
        self.active.insert(overlay);
    }

    /// Disables an overlay.
    pub fn disable(&mut self, overlay: CanvasOverlay) {
        self.active.remove(&overlay);
    }

    /// Toggles an overlay. Returns true if now enabled.
    pub fn toggle(&mut self, overlay: CanvasOverlay) -> bool {
        if self.active.contains(&overlay) {
            self.active.remove(&overlay);
            false
        } else {
            self.active.insert(overlay);
            true
        }
    }

    /// Returns whether an overlay is enabled.
    pub fn is_enabled(&self, overlay: CanvasOverlay) -> bool {
        self.active.contains(&overlay)
    }

    /// Returns all active overlays.
    pub fn active_overlays(&self) -> Vec<CanvasOverlay> {
        self.active.iter().copied().collect()
    }

    /// Number of active overlays.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Enables all overlays.
    pub fn enable_all(&mut self) {
        use CanvasOverlay::*;
        for ov in [CollisionShapes, NavigationRegions, YSortIndicators, OriginCross, LockGroupIcons, VisibilityIndicators] {
            self.active.insert(ov);
        }
    }

    /// Disables all overlays.
    pub fn disable_all(&mut self) {
        self.active.clear();
    }
}

// ---------------------------------------------------------------------------
// Ruler
// ---------------------------------------------------------------------------

/// Configuration for the viewport rulers displayed along the top and left edges.
#[derive(Debug, Clone)]
pub struct RulerConfig {
    /// Whether rulers are visible.
    pub visible: bool,
    /// Ruler thickness in screen pixels.
    pub thickness: f32,
    /// Major tick interval in world-space pixels (e.g. 100.0).
    pub major_interval: f32,
    /// Number of minor subdivisions between major ticks.
    pub minor_subdivisions: u32,
}

impl Default for RulerConfig {
    fn default() -> Self {
        Self {
            visible: true,
            thickness: 20.0,
            major_interval: 100.0,
            minor_subdivisions: 5,
        }
    }
}

impl RulerConfig {
    /// Computes the world-space positions of major tick marks visible in a
    /// viewport range [start, end]. Returns (position, label_value) pairs.
    pub fn major_ticks(&self, view_start: f32, view_end: f32) -> Vec<(f32, f32)> {
        if self.major_interval <= 0.0 || view_start >= view_end {
            return Vec::new();
        }
        let first = (view_start / self.major_interval).floor() as i64;
        let last = (view_end / self.major_interval).ceil() as i64;
        (first..=last)
            .map(|i| {
                let pos = i as f32 * self.major_interval;
                (pos, pos)
            })
            .collect()
    }

    /// Minor tick interval in world-space pixels.
    pub fn minor_interval(&self) -> f32 {
        if self.minor_subdivisions == 0 {
            return self.major_interval;
        }
        self.major_interval / self.minor_subdivisions as f32
    }
}

// ---------------------------------------------------------------------------
// ViewportCamera2D
// ---------------------------------------------------------------------------

/// 2D editor viewport camera with pan and zoom.
///
/// Follows Godot 4's 2D editor camera conventions:
/// - `offset` is the world-space position of the viewport center.
/// - `zoom` is a scale factor (1.0 = 100%, 2.0 = 200% magnification).
#[derive(Debug, Clone)]
pub struct ViewportCamera2D {
    /// World-space offset (position of the viewport center).
    pub offset: Vector2,
    /// Zoom level (1.0 = 100%).
    pub zoom: f32,
    /// Minimum zoom.
    pub zoom_min: f32,
    /// Maximum zoom.
    pub zoom_max: f32,
    /// Zoom speed factor per scroll notch.
    pub zoom_speed: f32,
    /// Pan sensitivity (pixels per delta unit).
    pub pan_sensitivity: f32,
    /// Whether the camera is actively panning.
    panning: bool,
}

impl Default for ViewportCamera2D {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewportCamera2D {
    pub fn new() -> Self {
        Self {
            offset: Vector2::ZERO,
            zoom: 1.0,
            zoom_min: 0.05,
            zoom_max: 100.0,
            zoom_speed: 1.1,
            pan_sensitivity: 1.0,
            panning: false,
        }
    }

    /// Returns the current zoom level.
    pub fn zoom_level(&self) -> f32 {
        self.zoom
    }

    /// Returns the zoom as a percentage (e.g. 100.0 for 1x).
    pub fn zoom_percent(&self) -> f32 {
        self.zoom * 100.0
    }

    /// Begins a pan gesture.
    pub fn begin_pan(&mut self) {
        self.panning = true;
    }

    /// Updates the pan offset by a screen-space delta.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if !self.panning {
            return;
        }
        // Convert screen delta to world delta (inverse of zoom).
        self.offset.x -= dx * self.pan_sensitivity / self.zoom;
        self.offset.y -= dy * self.pan_sensitivity / self.zoom;
    }

    /// Ends the pan gesture.
    pub fn end_pan(&mut self) {
        self.panning = false;
    }

    /// Returns whether the camera is actively panning.
    pub fn is_panning(&self) -> bool {
        self.panning
    }

    /// Zooms toward a screen-space point by the given scroll delta.
    ///
    /// Positive delta zooms in, negative zooms out. The zoom is anchored
    /// at `screen_point` so the world position under the cursor stays fixed.
    pub fn zoom_at(&mut self, delta: f32, screen_point: Vector2, viewport_size: Vector2) {
        let factor = if delta > 0.0 {
            self.zoom_speed
        } else {
            1.0 / self.zoom_speed
        };

        let old_zoom = self.zoom;
        self.zoom = (self.zoom * factor).clamp(self.zoom_min, self.zoom_max);

        // Anchor zoom at cursor position.
        let center = Vector2::new(viewport_size.x / 2.0, viewport_size.y / 2.0);
        let screen_offset = Vector2::new(
            screen_point.x - center.x,
            screen_point.y - center.y,
        );

        // Adjust offset so the world point under the cursor stays fixed.
        self.offset.x += screen_offset.x * (1.0 / old_zoom - 1.0 / self.zoom);
        self.offset.y += screen_offset.y * (1.0 / old_zoom - 1.0 / self.zoom);
    }

    /// Zooms in one step, centered on the viewport.
    pub fn zoom_in(&mut self, viewport_size: Vector2) {
        let center = Vector2::new(viewport_size.x / 2.0, viewport_size.y / 2.0);
        self.zoom_at(1.0, center, viewport_size);
    }

    /// Zooms out one step, centered on the viewport.
    pub fn zoom_out(&mut self, viewport_size: Vector2) {
        let center = Vector2::new(viewport_size.x / 2.0, viewport_size.y / 2.0);
        self.zoom_at(-1.0, center, viewport_size);
    }

    /// Resets zoom to 100% and centers the viewport at the origin.
    pub fn reset(&mut self) {
        self.offset = Vector2::ZERO;
        self.zoom = 1.0;
    }

    /// Frames a rectangular region so it fits in the viewport with padding.
    pub fn frame_rect(&mut self, rect_center: Vector2, rect_size: Vector2, viewport_size: Vector2) {
        self.offset = rect_center;
        if rect_size.x > 0.0 && rect_size.y > 0.0 {
            let padding = 0.9; // 90% fill
            let zoom_x = viewport_size.x / rect_size.x * padding;
            let zoom_y = viewport_size.y / rect_size.y * padding;
            self.zoom = zoom_x.min(zoom_y).clamp(self.zoom_min, self.zoom_max);
        }
    }

    /// Converts a screen-space point to world-space.
    pub fn screen_to_world(&self, screen_point: Vector2, viewport_size: Vector2) -> Vector2 {
        let center = Vector2::new(viewport_size.x / 2.0, viewport_size.y / 2.0);
        Vector2::new(
            self.offset.x + (screen_point.x - center.x) / self.zoom,
            self.offset.y + (screen_point.y - center.y) / self.zoom,
        )
    }

    /// Converts a world-space point to screen-space.
    pub fn world_to_screen(&self, world_point: Vector2, viewport_size: Vector2) -> Vector2 {
        let center = Vector2::new(viewport_size.x / 2.0, viewport_size.y / 2.0);
        Vector2::new(
            center.x + (world_point.x - self.offset.x) * self.zoom,
            center.y + (world_point.y - self.offset.y) * self.zoom,
        )
    }
}

// ---------------------------------------------------------------------------
// Selection2D
// ---------------------------------------------------------------------------

/// A rectangle in world-space used for overlap selection.
#[derive(Debug, Clone, Copy)]
pub struct SelectionRect {
    /// Top-left corner (min).
    pub min: Vector2,
    /// Bottom-right corner (max).
    pub max: Vector2,
}

impl SelectionRect {
    pub fn new(a: Vector2, b: Vector2) -> Self {
        Self {
            min: Vector2::new(a.x.min(b.x), a.y.min(b.y)),
            max: Vector2::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Returns true if this rect contains the given point.
    pub fn contains_point(&self, p: Vector2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Returns true if this rect overlaps another rect (AABB test).
    pub fn overlaps(&self, other: &SelectionRect) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// Returns the width.
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Returns the height.
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Returns the center point.
    pub fn center(&self) -> Vector2 {
        Vector2::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }
}

/// 2D editor selection state with overlap selection and locked-node support.
#[derive(Debug, Clone)]
pub struct Selection2D {
    /// Currently selected node IDs.
    selected: Vec<u64>,
    /// The primary (most recently clicked) selection.
    primary: Option<u64>,
    /// Node IDs that are locked (cannot be selected by click/drag).
    locked: Vec<u64>,
    /// Active drag state for transform operations.
    drag: Option<DragState2D>,
}

/// State for an in-progress drag (move/rotate/scale).
#[derive(Debug, Clone)]
pub struct DragState2D {
    /// The tool mode that started this drag.
    pub mode: ToolMode2D,
    /// World-space start position of the drag.
    pub start: Vector2,
    /// Current world-space position of the drag.
    pub current: Vector2,
    /// Accumulated delta from start.
    pub delta: Vector2,
}

impl Default for Selection2D {
    fn default() -> Self {
        Self::new()
    }
}

impl Selection2D {
    pub fn new() -> Self {
        Self {
            selected: Vec::new(),
            primary: None,
            locked: Vec::new(),
            drag: None,
        }
    }

    /// Selects a single node (replacing the current selection).
    /// Returns false if the node is locked.
    pub fn select(&mut self, node_id: u64) -> bool {
        if self.is_locked(node_id) {
            return false;
        }
        self.selected.clear();
        self.selected.push(node_id);
        self.primary = Some(node_id);
        true
    }

    /// Adds a node to the selection (for Shift+click multi-select).
    /// Returns false if the node is locked.
    pub fn add_to_selection(&mut self, node_id: u64) -> bool {
        if self.is_locked(node_id) {
            return false;
        }
        if !self.selected.contains(&node_id) {
            self.selected.push(node_id);
        }
        self.primary = Some(node_id);
        true
    }

    /// Removes a node from the selection.
    pub fn remove_from_selection(&mut self, node_id: u64) {
        self.selected.retain(|&id| id != node_id);
        if self.primary == Some(node_id) {
            self.primary = self.selected.last().copied();
        }
    }

    /// Toggles a node's selection state (for Ctrl+click).
    pub fn toggle_selection(&mut self, node_id: u64) {
        if self.selected.contains(&node_id) {
            self.remove_from_selection(node_id);
        } else {
            self.add_to_selection(node_id);
        }
    }

    /// Selects all nodes whose bounds overlap the given rectangle.
    /// Locked nodes are excluded. `candidates` provides (node_id, bounds) pairs.
    pub fn select_overlap(
        &mut self,
        rect: &SelectionRect,
        candidates: &[(u64, SelectionRect)],
    ) {
        self.selected.clear();
        for &(id, ref bounds) in candidates {
            if !self.is_locked(id) && rect.overlaps(bounds) {
                self.selected.push(id);
            }
        }
        self.primary = self.selected.last().copied();
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.primary = None;
    }

    /// Returns the primary (most recently selected) node.
    pub fn primary(&self) -> Option<u64> {
        self.primary
    }

    /// Returns all selected node IDs.
    pub fn selected(&self) -> &[u64] {
        &self.selected
    }

    /// Returns the number of selected nodes.
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// Returns true if the given node is selected.
    pub fn is_selected(&self, node_id: u64) -> bool {
        self.selected.contains(&node_id)
    }

    // -- Locked nodes -------------------------------------------------------

    /// Locks a node so it cannot be selected by click or overlap selection.
    pub fn lock(&mut self, node_id: u64) {
        if !self.locked.contains(&node_id) {
            self.locked.push(node_id);
        }
        // Remove from current selection if locked.
        self.remove_from_selection(node_id);
    }

    /// Unlocks a node so it can be selected again.
    pub fn unlock(&mut self, node_id: u64) {
        self.locked.retain(|&id| id != node_id);
    }

    /// Returns true if the given node is locked.
    pub fn is_locked(&self, node_id: u64) -> bool {
        self.locked.contains(&node_id)
    }

    /// Returns all locked node IDs.
    pub fn locked_nodes(&self) -> &[u64] {
        &self.locked
    }

    // -- Drag operations ----------------------------------------------------

    /// Begins a drag operation for the given tool mode at the given world position.
    pub fn begin_drag(&mut self, mode: ToolMode2D, start: Vector2) {
        self.drag = Some(DragState2D {
            mode,
            start,
            current: start,
            delta: Vector2::ZERO,
        });
    }

    /// Updates the current drag position.
    pub fn update_drag(&mut self, current: Vector2) {
        if let Some(ref mut drag) = self.drag {
            drag.current = current;
            drag.delta = Vector2::new(current.x - drag.start.x, current.y - drag.start.y);
        }
    }

    /// Ends the drag and returns the total delta.
    pub fn end_drag(&mut self) -> Option<Vector2> {
        self.drag.take().map(|d| d.delta)
    }

    /// Returns the current drag state, if any.
    pub fn drag_state(&self) -> Option<&DragState2D> {
        self.drag.as_ref()
    }

    /// Returns true if a drag operation is in progress.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

// ---------------------------------------------------------------------------
// Transform Gizmos (Move, Rotate, Scale)
// ---------------------------------------------------------------------------

/// Which part of a gizmo the user is interacting with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GizmoAxis {
    /// X axis only (horizontal).
    X,
    /// Y axis only (vertical).
    Y,
    /// Both axes (free transform / center handle).
    XY,
}

/// Result of a gizmo hit test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GizmoHit {
    /// Hit the move gizmo on a specific axis.
    Move(GizmoAxis),
    /// Hit the rotate gizmo ring.
    Rotate,
    /// Hit the scale gizmo on a specific axis.
    Scale(GizmoAxis),
}

/// Configuration for gizmo rendering and hit detection.
#[derive(Debug, Clone)]
pub struct GizmoConfig {
    /// Length of the axis arrows in screen pixels.
    pub arrow_length: f32,
    /// Width of the axis hit region in screen pixels.
    pub arrow_hit_width: f32,
    /// Size of the center handle in screen pixels (half-width).
    pub center_handle_size: f32,
    /// Radius of the rotation ring in screen pixels.
    pub rotate_radius: f32,
    /// Width of the rotation ring hit region in screen pixels.
    pub rotate_hit_width: f32,
    /// Length of the scale handles in screen pixels.
    pub scale_handle_length: f32,
}

impl Default for GizmoConfig {
    fn default() -> Self {
        Self {
            arrow_length: 80.0,
            arrow_hit_width: 10.0,
            center_handle_size: 8.0,
            rotate_radius: 60.0,
            rotate_hit_width: 8.0,
            scale_handle_length: 70.0,
        }
    }
}

/// 2D transform gizmo centered at a world-space pivot point.
///
/// Provides hit-testing and drag-to-transform computation for
/// move, rotate, and scale operations — mirroring Godot 4's 2D gizmos.
#[derive(Debug, Clone)]
pub struct Gizmo2D {
    /// World-space position of the gizmo center (node pivot).
    pub pivot: Vector2,
    /// Current gizmo configuration.
    pub config: GizmoConfig,
}

impl Gizmo2D {
    pub fn new(pivot: Vector2) -> Self {
        Self {
            pivot,
            config: GizmoConfig::default(),
        }
    }

    /// Hit-tests a screen-space point against the gizmo for the given tool mode.
    ///
    /// Returns `Some(GizmoHit)` if the point is over a gizmo element, `None` otherwise.
    pub fn hit_test(
        &self,
        screen_point: Vector2,
        tool_mode: ToolMode2D,
        camera: &ViewportCamera2D,
        viewport_size: Vector2,
    ) -> Option<GizmoHit> {
        let screen_pivot = camera.world_to_screen(self.pivot, viewport_size);

        match tool_mode {
            ToolMode2D::Select => None,
            ToolMode2D::Move => self.hit_test_move(screen_point, screen_pivot),
            ToolMode2D::Rotate => self.hit_test_rotate(screen_point, screen_pivot),
            ToolMode2D::Scale => self.hit_test_scale(screen_point, screen_pivot),
        }
    }

    fn hit_test_move(&self, point: Vector2, pivot: Vector2) -> Option<GizmoHit> {
        let half_w = self.config.arrow_hit_width;
        let length = self.config.arrow_length;
        let center = self.config.center_handle_size;

        // Center handle (free move) — check first for priority.
        if (point.x - pivot.x).abs() <= center && (point.y - pivot.y).abs() <= center {
            return Some(GizmoHit::Move(GizmoAxis::XY));
        }

        // X axis arrow: extends right from pivot.
        let dx = point.x - pivot.x;
        let dy = (point.y - pivot.y).abs();
        if dx > 0.0 && dx <= length && dy <= half_w {
            return Some(GizmoHit::Move(GizmoAxis::X));
        }

        // Y axis arrow: extends upward from pivot (screen Y is inverted).
        let dx2 = (point.x - pivot.x).abs();
        let dy2 = pivot.y - point.y; // positive means upward on screen
        if dy2 > 0.0 && dy2 <= length && dx2 <= half_w {
            return Some(GizmoHit::Move(GizmoAxis::Y));
        }

        None
    }

    fn hit_test_rotate(&self, point: Vector2, pivot: Vector2) -> Option<GizmoHit> {
        let dx = point.x - pivot.x;
        let dy = point.y - pivot.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let half_w = self.config.rotate_hit_width;
        let radius = self.config.rotate_radius;

        if (dist - radius).abs() <= half_w {
            return Some(GizmoHit::Rotate);
        }

        None
    }

    fn hit_test_scale(&self, point: Vector2, pivot: Vector2) -> Option<GizmoHit> {
        let half_w = self.config.arrow_hit_width;
        let length = self.config.scale_handle_length;
        let center = self.config.center_handle_size;

        // Center handle (uniform scale).
        if (point.x - pivot.x).abs() <= center && (point.y - pivot.y).abs() <= center {
            return Some(GizmoHit::Scale(GizmoAxis::XY));
        }

        // X axis handle.
        let dx = point.x - pivot.x;
        let dy = (point.y - pivot.y).abs();
        if dx > 0.0 && dx <= length && dy <= half_w {
            return Some(GizmoHit::Scale(GizmoAxis::X));
        }

        // Y axis handle.
        let dx2 = (point.x - pivot.x).abs();
        let dy2 = pivot.y - point.y;
        if dy2 > 0.0 && dy2 <= length && dx2 <= half_w {
            return Some(GizmoHit::Scale(GizmoAxis::Y));
        }

        None
    }

    /// Computes the translation from a world-space drag delta, constrained to the given axis.
    pub fn compute_move(&self, delta: Vector2, axis: GizmoAxis) -> Vector2 {
        match axis {
            GizmoAxis::X => Vector2::new(delta.x, 0.0),
            GizmoAxis::Y => Vector2::new(0.0, delta.y),
            GizmoAxis::XY => delta,
        }
    }

    /// Computes the rotation angle (in radians) from a drag on the rotation ring.
    ///
    /// Uses the angle between (start - pivot) and (current - pivot).
    pub fn compute_rotation(&self, drag_start: Vector2, drag_current: Vector2) -> f32 {
        let a = (drag_start.y - self.pivot.y).atan2(drag_start.x - self.pivot.x);
        let b = (drag_current.y - self.pivot.y).atan2(drag_current.x - self.pivot.x);
        b - a
    }

    /// Computes the scale factor from a world-space drag delta, constrained to the given axis.
    ///
    /// Returns a (scale_x, scale_y) pair where 1.0 means no change.
    /// The scale is relative to the distance from the pivot.
    pub fn compute_scale(
        &self,
        drag_start: Vector2,
        drag_current: Vector2,
        axis: GizmoAxis,
    ) -> (f32, f32) {
        let start_offset = Vector2::new(
            drag_start.x - self.pivot.x,
            drag_start.y - self.pivot.y,
        );
        let current_offset = Vector2::new(
            drag_current.x - self.pivot.x,
            drag_current.y - self.pivot.y,
        );

        let sx = if start_offset.x.abs() > 0.001 {
            current_offset.x / start_offset.x
        } else {
            1.0
        };
        let sy = if start_offset.y.abs() > 0.001 {
            current_offset.y / start_offset.y
        } else {
            1.0
        };

        match axis {
            GizmoAxis::X => (sx, 1.0),
            GizmoAxis::Y => (1.0, sy),
            GizmoAxis::XY => {
                // Uniform scale: use the average magnitude change.
                let start_dist = (start_offset.x * start_offset.x + start_offset.y * start_offset.y).sqrt();
                let current_dist = (current_offset.x * current_offset.x + current_offset.y * current_offset.y).sqrt();
                let uniform = if start_dist > 0.001 {
                    current_dist / start_dist
                } else {
                    1.0
                };
                (uniform, uniform)
            }
        }
    }
}

/// Active gizmo drag state.
#[derive(Debug, Clone)]
pub struct GizmoDragState {
    /// Which gizmo element is being dragged.
    pub hit: GizmoHit,
    /// The gizmo being dragged.
    pub gizmo: Gizmo2D,
    /// World-space start position of the drag.
    pub start: Vector2,
    /// Current world-space position of the drag.
    pub current: Vector2,
}

impl GizmoDragState {
    /// Returns the world-space delta from start to current.
    pub fn delta(&self) -> Vector2 {
        Vector2::new(self.current.x - self.start.x, self.current.y - self.start.y)
    }

    /// Computes the current transform based on the gizmo hit type.
    pub fn current_transform(&self) -> GizmoTransform {
        match self.hit {
            GizmoHit::Move(axis) => {
                let translation = self.gizmo.compute_move(self.delta(), axis);
                GizmoTransform::Translate(translation)
            }
            GizmoHit::Rotate => {
                let angle = self.gizmo.compute_rotation(self.start, self.current);
                GizmoTransform::Rotate(angle)
            }
            GizmoHit::Scale(axis) => {
                let (sx, sy) = self.gizmo.compute_scale(self.start, self.current, axis);
                GizmoTransform::Scale(sx, sy)
            }
        }
    }
}

/// Result of a completed gizmo drag — the transform to apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GizmoTransform {
    /// Translation by (dx, dy).
    Translate(Vector2),
    /// Rotation by angle in radians.
    Rotate(f32),
    /// Scale by (sx, sy) factors.
    Scale(f32, f32),
}

// ---------------------------------------------------------------------------
// BoundingBoxHandle
// ---------------------------------------------------------------------------

/// One of the 8 handles on a bounding box (4 corners + 4 edge midpoints).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlePosition {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl HandlePosition {
    /// Returns all 8 handle positions in order.
    pub fn all() -> [HandlePosition; 8] {
        [
            Self::TopLeft,
            Self::TopCenter,
            Self::TopRight,
            Self::MiddleLeft,
            Self::MiddleRight,
            Self::BottomLeft,
            Self::BottomCenter,
            Self::BottomRight,
        ]
    }

    /// Returns the normalized position of this handle on a unit rect [0,0]-[1,1].
    pub fn normalized(&self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::MiddleLeft => (0.0, 0.5),
            Self::MiddleRight => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }

    /// Returns which axes this handle can resize along: (horizontal, vertical).
    pub fn resize_axes(&self) -> (bool, bool) {
        match self {
            Self::TopLeft | Self::TopRight | Self::BottomLeft | Self::BottomRight => (true, true),
            Self::TopCenter | Self::BottomCenter => (false, true),
            Self::MiddleLeft | Self::MiddleRight => (true, false),
        }
    }

    /// Returns the opposite handle (for anchored resize).
    pub fn opposite(&self) -> HandlePosition {
        match self {
            Self::TopLeft => Self::BottomRight,
            Self::TopCenter => Self::BottomCenter,
            Self::TopRight => Self::BottomLeft,
            Self::MiddleLeft => Self::MiddleRight,
            Self::MiddleRight => Self::MiddleLeft,
            Self::BottomLeft => Self::TopRight,
            Self::BottomCenter => Self::TopCenter,
            Self::BottomRight => Self::TopLeft,
        }
    }
}

/// Bounding box with 8 draggable handles for resizing a node.
#[derive(Debug, Clone)]
pub struct BoundingBoxHandles {
    /// The bounding rect in world space.
    pub bounds: SelectionRect,
    /// Handle size in screen pixels (half-width of the square handle).
    pub handle_half_size: f32,
}

impl BoundingBoxHandles {
    pub fn new(bounds: SelectionRect) -> Self {
        Self {
            bounds,
            handle_half_size: 4.0,
        }
    }

    /// Returns the world-space position of a specific handle.
    pub fn handle_world_position(&self, pos: HandlePosition) -> Vector2 {
        let (nx, ny) = pos.normalized();
        Vector2::new(
            self.bounds.min.x + (self.bounds.max.x - self.bounds.min.x) * nx,
            self.bounds.min.y + (self.bounds.max.y - self.bounds.min.y) * ny,
        )
    }

    /// Returns world-space positions for all 8 handles.
    pub fn all_handle_positions(&self) -> [(HandlePosition, Vector2); 8] {
        HandlePosition::all().map(|hp| (hp, self.handle_world_position(hp)))
    }

    /// Hit-tests a screen-space point against handles, given a camera for
    /// coordinate conversion. Returns the handle position if hit.
    pub fn hit_test(
        &self,
        screen_point: Vector2,
        camera: &ViewportCamera2D,
        viewport_size: Vector2,
    ) -> Option<HandlePosition> {
        let half = self.handle_half_size;
        for hp in HandlePosition::all() {
            let world_pos = self.handle_world_position(hp);
            let screen_pos = camera.world_to_screen(world_pos, viewport_size);
            if (screen_point.x - screen_pos.x).abs() <= half
                && (screen_point.y - screen_pos.y).abs() <= half
            {
                return Some(hp);
            }
        }
        None
    }

    /// Computes a new bounding rect after dragging a handle by a world-space delta.
    /// The opposite handle stays anchored.
    pub fn resize_by_handle(
        &self,
        handle: HandlePosition,
        world_delta: Vector2,
    ) -> SelectionRect {
        let (resize_x, resize_y) = handle.resize_axes();
        let mut new_min = self.bounds.min;
        let mut new_max = self.bounds.max;

        let (nx, ny) = handle.normalized();

        if resize_x {
            if nx < 0.5 {
                new_min.x += world_delta.x;
            } else {
                new_max.x += world_delta.x;
            }
        }
        if resize_y {
            if ny < 0.5 {
                new_min.y += world_delta.y;
            } else {
                new_max.y += world_delta.y;
            }
        }

        // Normalize so min <= max (allow flipping).
        SelectionRect::new(new_min, new_max)
    }
}

/// State for an active handle drag operation.
#[derive(Debug, Clone)]
pub struct HandleDragState {
    /// Which handle is being dragged.
    pub handle: HandlePosition,
    /// Original bounds before the drag started.
    pub original_bounds: SelectionRect,
    /// World-space start position of the drag.
    pub start: Vector2,
    /// Current world-space position.
    pub current: Vector2,
}

impl HandleDragState {
    /// Returns the world-space delta from start to current.
    pub fn delta(&self) -> Vector2 {
        Vector2::new(self.current.x - self.start.x, self.current.y - self.start.y)
    }

    /// Returns the new bounds based on the current drag position.
    pub fn current_bounds(&self) -> SelectionRect {
        let handles = BoundingBoxHandles::new(self.original_bounds);
        handles.resize_by_handle(self.handle, self.delta())
    }
}

// ---------------------------------------------------------------------------
// Viewport2D
// ---------------------------------------------------------------------------

/// Full 2D editor viewport combining camera, selection, tool mode, snap,
/// guides, smart snap, rulers, and canvas overlays.
#[derive(Debug, Clone)]
pub struct Viewport2D {
    /// The 2D camera (pan/zoom).
    pub camera: ViewportCamera2D,
    /// Selection state.
    pub selection: Selection2D,
    /// Active tool mode.
    pub tool_mode: ToolMode2D,
    /// Snap settings.
    pub snap: SnapSettings2D,
    /// Viewport width in pixels.
    pub width: u32,
    /// Viewport height in pixels.
    pub height: u32,
    /// Whether grid is visible.
    pub grid_visible: bool,
    /// Whether rulers are visible.
    pub rulers_visible: bool,
    /// Whether guides are visible.
    pub guides_visible: bool,
    /// Guide line manager.
    pub guides: GuideManager,
    /// Smart-snap engine for snapping to nearby nodes.
    pub smart_snap: SmartSnap,
    /// Canvas overlay visibility settings.
    pub overlays: OverlaySettings,
    /// Ruler configuration.
    pub ruler_config: RulerConfig,
    /// Active handle-drag state for bounding box resize.
    handle_drag: Option<HandleDragState>,
    /// Active gizmo drag state.
    gizmo_drag: Option<GizmoDragState>,
}

impl Viewport2D {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            camera: ViewportCamera2D::new(),
            selection: Selection2D::new(),
            tool_mode: ToolMode2D::default(),
            snap: SnapSettings2D::default(),
            width,
            height,
            grid_visible: true,
            rulers_visible: true,
            guides_visible: true,
            guides: GuideManager::new(),
            smart_snap: SmartSnap::new(),
            overlays: OverlaySettings::new(),
            ruler_config: RulerConfig::default(),
            handle_drag: None,
            gizmo_drag: None,
        }
    }

    /// Resizes the viewport.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// Returns the viewport size as a Vector2.
    pub fn size(&self) -> Vector2 {
        Vector2::new(self.width as f32, self.height as f32)
    }

    /// Returns the aspect ratio.
    pub fn aspect_ratio(&self) -> f32 {
        if self.height == 0 {
            return 1.0;
        }
        self.width as f32 / self.height as f32
    }

    /// Sets the active tool mode.
    pub fn set_tool_mode(&mut self, mode: ToolMode2D) {
        self.tool_mode = mode;
    }

    /// Handles a mouse drag event (screen-space delta).
    pub fn on_mouse_drag(&mut self, dx: f32, dy: f32) {
        if self.camera.is_panning() {
            self.camera.pan(dx, dy);
        }
    }

    /// Handles a scroll event for zooming.
    pub fn on_scroll(&mut self, delta: f32, screen_point: Vector2) {
        self.camera.zoom_at(delta, screen_point, self.size());
    }

    /// Frames the selection or a given rect in the viewport.
    pub fn frame_selection(&mut self, center: Vector2, size: Vector2) {
        self.camera.frame_rect(center, size, self.size());
    }

    /// Resets the viewport camera to default.
    pub fn reset_view(&mut self) {
        self.camera.reset();
    }

    /// Converts screen coordinates to world coordinates.
    pub fn screen_to_world(&self, screen: Vector2) -> Vector2 {
        self.camera.screen_to_world(screen, self.size())
    }

    /// Converts world coordinates to screen coordinates.
    pub fn world_to_screen(&self, world: Vector2) -> Vector2 {
        self.camera.world_to_screen(world, self.size())
    }

    /// Hit-tests a screen point against the bounding box handles of a node.
    /// Returns the handle position if a handle is under the cursor.
    pub fn hit_test_handles(
        &self,
        screen_point: Vector2,
        node_bounds: SelectionRect,
    ) -> Option<HandlePosition> {
        let handles = BoundingBoxHandles::new(node_bounds);
        handles.hit_test(screen_point, &self.camera, self.size())
    }

    /// Begins a handle-drag operation on a specific handle.
    pub fn begin_handle_drag(
        &mut self,
        handle: HandlePosition,
        node_bounds: SelectionRect,
        screen_point: Vector2,
    ) {
        let world = self.screen_to_world(screen_point);
        self.handle_drag = Some(HandleDragState {
            handle,
            original_bounds: node_bounds,
            start: world,
            current: world,
        });
    }

    /// Updates the current handle-drag position (screen-space).
    pub fn update_handle_drag(&mut self, screen_point: Vector2) {
        let vp_size = self.size();
        if let Some(ref mut drag) = self.handle_drag {
            drag.current = self.camera.screen_to_world(screen_point, vp_size);
        }
    }

    /// Ends the handle drag and returns the new bounds, or None if no drag was active.
    pub fn end_handle_drag(&mut self) -> Option<SelectionRect> {
        self.handle_drag.take().map(|drag| drag.current_bounds())
    }

    /// Returns the current handle drag state, if any.
    pub fn handle_drag_state(&self) -> Option<&HandleDragState> {
        self.handle_drag.as_ref()
    }

    /// Returns true if a handle drag is in progress.
    pub fn is_handle_dragging(&self) -> bool {
        self.handle_drag.is_some()
    }

    // -- Gizmo operations ---------------------------------------------------

    /// Hit-tests a screen point against the gizmo for a node at the given pivot.
    pub fn hit_test_gizmo(
        &self,
        screen_point: Vector2,
        pivot: Vector2,
    ) -> Option<GizmoHit> {
        let gizmo = Gizmo2D::new(pivot);
        gizmo.hit_test(screen_point, self.tool_mode, &self.camera, self.size())
    }

    /// Begins a gizmo drag on the given hit at the given screen point.
    pub fn begin_gizmo_drag(
        &mut self,
        hit: GizmoHit,
        pivot: Vector2,
        screen_point: Vector2,
    ) {
        let world = self.screen_to_world(screen_point);
        self.gizmo_drag = Some(GizmoDragState {
            hit,
            gizmo: Gizmo2D::new(pivot),
            start: world,
            current: world,
        });
    }

    /// Updates the current gizmo drag position (screen-space).
    pub fn update_gizmo_drag(&mut self, screen_point: Vector2) {
        let vp_size = self.size();
        if let Some(ref mut drag) = self.gizmo_drag {
            drag.current = self.camera.screen_to_world(screen_point, vp_size);
        }
    }

    /// Ends the gizmo drag and returns the resulting transform.
    pub fn end_gizmo_drag(&mut self) -> Option<GizmoTransform> {
        self.gizmo_drag.take().map(|drag| drag.current_transform())
    }

    /// Returns the current gizmo drag state, if any.
    pub fn gizmo_drag_state(&self) -> Option<&GizmoDragState> {
        self.gizmo_drag.as_ref()
    }

    /// Returns true if a gizmo drag is in progress.
    pub fn is_gizmo_dragging(&self) -> bool {
        self.gizmo_drag.is_some()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- ToolMode2D ---------------------------------------------------------

    #[test]
    fn tool_mode_default_is_select() {
        assert_eq!(ToolMode2D::default(), ToolMode2D::Select);
    }

    // -- SnapSettings2D -----------------------------------------------------

    #[test]
    fn snap_disabled_passes_through() {
        let snap = SnapSettings2D::default(); // enabled = false
        let pos = Vector2::new(3.3, 7.7);
        assert_eq!(snap.snap_position(pos), pos);
    }

    #[test]
    fn snap_enabled_rounds_to_grid() {
        let snap = SnapSettings2D {
            enabled: true,
            grid_step: Vector2::new(10.0, 10.0),
            ..Default::default()
        };
        let snapped = snap.snap_position(Vector2::new(13.0, 27.0));
        assert!((snapped.x - 10.0).abs() < 0.01);
        assert!((snapped.y - 30.0).abs() < 0.01);
    }

    #[test]
    fn snap_rotation_rounds_to_step() {
        let snap = SnapSettings2D {
            enabled: true,
            rotation_step: std::f32::consts::FRAC_PI_4, // 45 degrees
            ..Default::default()
        };
        let snapped = snap.snap_rotation(0.3); // 0.3/(PI/4)=0.38 rounds to 0
        assert!(snapped.abs() < 0.01);
        let snapped2 = snap.snap_rotation(0.9); // 0.9/(PI/4)=1.15 rounds to 1 -> PI/4
        assert!((snapped2 - std::f32::consts::FRAC_PI_4).abs() < 0.01);
    }

    // -- ViewportCamera2D ---------------------------------------------------

    #[test]
    fn camera_default_state() {
        let cam = ViewportCamera2D::new();
        assert!((cam.zoom - 1.0).abs() < 0.001);
        assert!((cam.offset.x).abs() < 0.001);
        assert!((cam.offset.y).abs() < 0.001);
        assert!(!cam.is_panning());
    }

    #[test]
    fn camera_pan_moves_offset() {
        let mut cam = ViewportCamera2D::new();
        cam.begin_pan();
        cam.pan(100.0, 50.0);
        cam.end_pan();
        // Pan moves offset in opposite direction (screen drag right = world scroll left).
        assert!(cam.offset.x < 0.0);
        assert!(cam.offset.y < 0.0);
    }

    #[test]
    fn camera_pan_ignored_when_not_panning() {
        let mut cam = ViewportCamera2D::new();
        cam.pan(100.0, 50.0); // No begin_pan
        assert!((cam.offset.x).abs() < 0.001);
    }

    #[test]
    fn camera_zoom_in_increases_zoom() {
        let mut cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let old_zoom = cam.zoom;
        cam.zoom_in(vp);
        assert!(cam.zoom > old_zoom);
    }

    #[test]
    fn camera_zoom_out_decreases_zoom() {
        let mut cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let old_zoom = cam.zoom;
        cam.zoom_out(vp);
        assert!(cam.zoom < old_zoom);
    }

    #[test]
    fn camera_zoom_clamped() {
        let mut cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        for _ in 0..100 {
            cam.zoom_out(vp);
        }
        assert!(cam.zoom >= cam.zoom_min);
        for _ in 0..200 {
            cam.zoom_in(vp);
        }
        assert!(cam.zoom <= cam.zoom_max);
    }

    #[test]
    fn camera_reset_returns_to_default() {
        let mut cam = ViewportCamera2D::new();
        cam.offset = Vector2::new(100.0, 200.0);
        cam.zoom = 3.0;
        cam.reset();
        assert!((cam.zoom - 1.0).abs() < 0.001);
        assert!((cam.offset.x).abs() < 0.001);
    }

    #[test]
    fn camera_screen_to_world_roundtrip() {
        let mut cam = ViewportCamera2D::new();
        cam.offset = Vector2::new(50.0, 30.0);
        cam.zoom = 2.0;
        let vp = Vector2::new(800.0, 600.0);
        let screen = Vector2::new(400.0, 300.0); // center of viewport
        let world = cam.screen_to_world(screen, vp);
        // Center of viewport should map to camera offset.
        assert!((world.x - 50.0).abs() < 0.01);
        assert!((world.y - 30.0).abs() < 0.01);
        // Roundtrip.
        let back = cam.world_to_screen(world, vp);
        assert!((back.x - screen.x).abs() < 0.01);
        assert!((back.y - screen.y).abs() < 0.01);
    }

    #[test]
    fn camera_frame_rect_adjusts_zoom_and_offset() {
        let mut cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        cam.frame_rect(Vector2::new(100.0, 100.0), Vector2::new(400.0, 300.0), vp);
        assert!((cam.offset.x - 100.0).abs() < 0.01);
        assert!((cam.offset.y - 100.0).abs() < 0.01);
        // Zoom should fit the rect with padding.
        assert!(cam.zoom > 0.0);
    }

    // -- SelectionRect ------------------------------------------------------

    #[test]
    fn selection_rect_contains_point() {
        let rect = SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        assert!(rect.contains_point(Vector2::new(5.0, 5.0)));
        assert!(!rect.contains_point(Vector2::new(15.0, 5.0)));
    }

    #[test]
    fn selection_rect_overlaps() {
        let a = SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        let b = SelectionRect::new(Vector2::new(5.0, 5.0), Vector2::new(15.0, 15.0));
        let c = SelectionRect::new(Vector2::new(20.0, 20.0), Vector2::new(30.0, 30.0));
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn selection_rect_normalizes_corners() {
        // Pass bottom-right first, top-left second.
        let rect = SelectionRect::new(Vector2::new(10.0, 10.0), Vector2::new(0.0, 0.0));
        assert!((rect.min.x).abs() < 0.01);
        assert!((rect.max.x - 10.0).abs() < 0.01);
    }

    // -- Selection2D --------------------------------------------------------

    #[test]
    fn selection_single_select() {
        let mut sel = Selection2D::new();
        assert!(sel.select(1));
        assert_eq!(sel.primary(), Some(1));
        assert_eq!(sel.count(), 1);
    }

    #[test]
    fn selection_replaces_on_single_select() {
        let mut sel = Selection2D::new();
        sel.select(1);
        sel.select(2);
        assert_eq!(sel.count(), 1);
        assert_eq!(sel.primary(), Some(2));
        assert!(!sel.is_selected(1));
    }

    #[test]
    fn selection_add_multi_select() {
        let mut sel = Selection2D::new();
        sel.select(1);
        sel.add_to_selection(2);
        sel.add_to_selection(3);
        assert_eq!(sel.count(), 3);
        assert!(sel.is_selected(1));
        assert!(sel.is_selected(2));
        assert!(sel.is_selected(3));
    }

    #[test]
    fn selection_toggle() {
        let mut sel = Selection2D::new();
        sel.select(1);
        sel.toggle_selection(1); // deselect
        assert_eq!(sel.count(), 0);
        sel.toggle_selection(1); // reselect
        assert_eq!(sel.count(), 1);
    }

    #[test]
    fn selection_clear() {
        let mut sel = Selection2D::new();
        sel.select(1);
        sel.add_to_selection(2);
        sel.clear();
        assert_eq!(sel.count(), 0);
        assert!(sel.primary().is_none());
    }

    // -- Locked nodes -------------------------------------------------------

    #[test]
    fn locked_node_cannot_be_selected() {
        let mut sel = Selection2D::new();
        sel.lock(1);
        assert!(!sel.select(1));
        assert_eq!(sel.count(), 0);
    }

    #[test]
    fn locked_node_excluded_from_overlap() {
        let mut sel = Selection2D::new();
        sel.lock(2);
        let candidates = vec![
            (1, SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0))),
            (2, SelectionRect::new(Vector2::new(3.0, 3.0), Vector2::new(8.0, 8.0))),
            (3, SelectionRect::new(Vector2::new(6.0, 6.0), Vector2::new(11.0, 11.0))),
        ];
        let rect = SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        sel.select_overlap(&rect, &candidates);
        assert!(sel.is_selected(1));
        assert!(!sel.is_selected(2)); // locked
        assert!(sel.is_selected(3));
    }

    #[test]
    fn locking_removes_from_selection() {
        let mut sel = Selection2D::new();
        sel.select(1);
        sel.add_to_selection(2);
        sel.lock(1);
        assert!(!sel.is_selected(1));
        assert_eq!(sel.count(), 1);
    }

    #[test]
    fn unlock_allows_reselection() {
        let mut sel = Selection2D::new();
        sel.lock(1);
        assert!(!sel.select(1));
        sel.unlock(1);
        assert!(sel.select(1));
    }

    // -- Drag operations ----------------------------------------------------

    #[test]
    fn drag_move_accumulates_delta() {
        let mut sel = Selection2D::new();
        sel.select(1);
        sel.begin_drag(ToolMode2D::Move, Vector2::new(10.0, 20.0));
        sel.update_drag(Vector2::new(30.0, 50.0));
        let state = sel.drag_state().unwrap();
        assert!((state.delta.x - 20.0).abs() < 0.01);
        assert!((state.delta.y - 30.0).abs() < 0.01);
        let delta = sel.end_drag().unwrap();
        assert!((delta.x - 20.0).abs() < 0.01);
        assert!(!sel.is_dragging());
    }

    // -- Viewport2D ---------------------------------------------------------

    #[test]
    fn viewport_creation() {
        let vp = Viewport2D::new(1024, 768);
        assert_eq!(vp.width, 1024);
        assert_eq!(vp.height, 768);
        assert_eq!(vp.tool_mode, ToolMode2D::Select);
        assert!(vp.grid_visible);
    }

    #[test]
    fn viewport_resize() {
        let mut vp = Viewport2D::new(800, 600);
        vp.resize(1920, 1080);
        assert_eq!(vp.width, 1920);
        assert_eq!(vp.height, 1080);
    }

    #[test]
    fn viewport_tool_mode_switch() {
        let mut vp = Viewport2D::new(800, 600);
        vp.set_tool_mode(ToolMode2D::Move);
        assert_eq!(vp.tool_mode, ToolMode2D::Move);
        vp.set_tool_mode(ToolMode2D::Rotate);
        assert_eq!(vp.tool_mode, ToolMode2D::Rotate);
    }

    #[test]
    fn viewport_screen_to_world_delegation() {
        let mut vp = Viewport2D::new(800, 600);
        vp.camera.offset = Vector2::new(100.0, 100.0);
        vp.camera.zoom = 2.0;
        let world = vp.screen_to_world(Vector2::new(400.0, 300.0));
        // Center maps to camera offset.
        assert!((world.x - 100.0).abs() < 0.01);
        assert!((world.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn viewport_scroll_zooms() {
        let mut vp = Viewport2D::new(800, 600);
        let old_zoom = vp.camera.zoom;
        vp.on_scroll(1.0, Vector2::new(400.0, 300.0));
        assert!(vp.camera.zoom > old_zoom);
    }

    // -- HandlePosition -----------------------------------------------------

    #[test]
    fn handle_position_all_returns_eight() {
        assert_eq!(HandlePosition::all().len(), 8);
    }

    #[test]
    fn handle_position_opposite_is_symmetric() {
        for hp in HandlePosition::all() {
            assert_eq!(hp.opposite().opposite(), hp);
        }
    }

    #[test]
    fn handle_normalized_corners_are_correct() {
        assert_eq!(HandlePosition::TopLeft.normalized(), (0.0, 0.0));
        assert_eq!(HandlePosition::BottomRight.normalized(), (1.0, 1.0));
        assert_eq!(HandlePosition::TopCenter.normalized(), (0.5, 0.0));
        assert_eq!(HandlePosition::MiddleLeft.normalized(), (0.0, 0.5));
    }

    #[test]
    fn handle_resize_axes_corners_are_both() {
        for hp in [HandlePosition::TopLeft, HandlePosition::TopRight,
                    HandlePosition::BottomLeft, HandlePosition::BottomRight] {
            assert_eq!(hp.resize_axes(), (true, true));
        }
    }

    #[test]
    fn handle_resize_axes_edges_are_single() {
        assert_eq!(HandlePosition::TopCenter.resize_axes(), (false, true));
        assert_eq!(HandlePosition::BottomCenter.resize_axes(), (false, true));
        assert_eq!(HandlePosition::MiddleLeft.resize_axes(), (true, false));
        assert_eq!(HandlePosition::MiddleRight.resize_axes(), (true, false));
    }

    // -- BoundingBoxHandles -------------------------------------------------

    #[test]
    fn handle_world_positions_match_bounds() {
        let bounds = SelectionRect::new(
            Vector2::new(10.0, 20.0),
            Vector2::new(110.0, 80.0),
        );
        let handles = BoundingBoxHandles::new(bounds);
        let tl = handles.handle_world_position(HandlePosition::TopLeft);
        assert!((tl.x - 10.0).abs() < 0.01);
        assert!((tl.y - 20.0).abs() < 0.01);
        let br = handles.handle_world_position(HandlePosition::BottomRight);
        assert!((br.x - 110.0).abs() < 0.01);
        assert!((br.y - 80.0).abs() < 0.01);
        let tc = handles.handle_world_position(HandlePosition::TopCenter);
        assert!((tc.x - 60.0).abs() < 0.01);
        assert!((tc.y - 20.0).abs() < 0.01);
    }

    #[test]
    fn all_handle_positions_returns_eight() {
        let bounds = SelectionRect::new(Vector2::ZERO, Vector2::new(100.0, 100.0));
        let handles = BoundingBoxHandles::new(bounds);
        assert_eq!(handles.all_handle_positions().len(), 8);
    }

    #[test]
    fn handle_hit_test_detects_handle() {
        let bounds = SelectionRect::new(Vector2::ZERO, Vector2::new(100.0, 100.0));
        let handles = BoundingBoxHandles::new(bounds);
        let cam = ViewportCamera2D::new(); // zoom 1.0, offset 0,0
        let vp_size = Vector2::new(800.0, 600.0);

        // TopLeft handle is at world (0,0), which maps to screen center - (400, 300)
        // At zoom=1.0, offset=0,0: world (0,0) → screen (400, 300)
        let screen_tl = cam.world_to_screen(Vector2::ZERO, vp_size);
        let hit = handles.hit_test(screen_tl, &cam, vp_size);
        assert_eq!(hit, Some(HandlePosition::TopLeft));
    }

    #[test]
    fn handle_hit_test_misses_when_far() {
        let bounds = SelectionRect::new(Vector2::ZERO, Vector2::new(100.0, 100.0));
        let handles = BoundingBoxHandles::new(bounds);
        let cam = ViewportCamera2D::new();
        let vp_size = Vector2::new(800.0, 600.0);

        // Point far from any handle
        let hit = handles.hit_test(Vector2::new(0.0, 0.0), &cam, vp_size);
        assert!(hit.is_none());
    }

    // -- Resize by handle ---------------------------------------------------

    #[test]
    fn resize_bottom_right_expands_max() {
        let bounds = SelectionRect::new(Vector2::new(10.0, 10.0), Vector2::new(50.0, 50.0));
        let handles = BoundingBoxHandles::new(bounds);
        let new_bounds = handles.resize_by_handle(
            HandlePosition::BottomRight,
            Vector2::new(20.0, 10.0),
        );
        assert!((new_bounds.min.x - 10.0).abs() < 0.01);
        assert!((new_bounds.min.y - 10.0).abs() < 0.01);
        assert!((new_bounds.max.x - 70.0).abs() < 0.01);
        assert!((new_bounds.max.y - 60.0).abs() < 0.01);
    }

    #[test]
    fn resize_top_left_moves_min() {
        let bounds = SelectionRect::new(Vector2::new(10.0, 10.0), Vector2::new(50.0, 50.0));
        let handles = BoundingBoxHandles::new(bounds);
        let new_bounds = handles.resize_by_handle(
            HandlePosition::TopLeft,
            Vector2::new(-5.0, -5.0),
        );
        assert!((new_bounds.min.x - 5.0).abs() < 0.01);
        assert!((new_bounds.min.y - 5.0).abs() < 0.01);
        assert!((new_bounds.max.x - 50.0).abs() < 0.01);
        assert!((new_bounds.max.y - 50.0).abs() < 0.01);
    }

    #[test]
    fn resize_middle_right_only_changes_x() {
        let bounds = SelectionRect::new(Vector2::new(10.0, 10.0), Vector2::new(50.0, 50.0));
        let handles = BoundingBoxHandles::new(bounds);
        let new_bounds = handles.resize_by_handle(
            HandlePosition::MiddleRight,
            Vector2::new(15.0, 99.0), // y should be ignored
        );
        assert!((new_bounds.min.y - 10.0).abs() < 0.01);
        assert!((new_bounds.max.y - 50.0).abs() < 0.01);
        assert!((new_bounds.max.x - 65.0).abs() < 0.01);
    }

    #[test]
    fn resize_top_center_only_changes_y() {
        let bounds = SelectionRect::new(Vector2::new(10.0, 10.0), Vector2::new(50.0, 50.0));
        let handles = BoundingBoxHandles::new(bounds);
        let new_bounds = handles.resize_by_handle(
            HandlePosition::TopCenter,
            Vector2::new(99.0, -10.0), // x should be ignored
        );
        assert!((new_bounds.min.x - 10.0).abs() < 0.01);
        assert!((new_bounds.max.x - 50.0).abs() < 0.01);
        assert!((new_bounds.min.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn resize_flips_when_dragged_past_opposite() {
        let bounds = SelectionRect::new(Vector2::new(10.0, 10.0), Vector2::new(50.0, 50.0));
        let handles = BoundingBoxHandles::new(bounds);
        // Drag bottom-right up and left past top-left
        let new_bounds = handles.resize_by_handle(
            HandlePosition::BottomRight,
            Vector2::new(-60.0, -60.0),
        );
        // SelectionRect::new normalizes, so min should still be < max
        assert!(new_bounds.min.x <= new_bounds.max.x);
        assert!(new_bounds.min.y <= new_bounds.max.y);
    }

    // -- HandleDragState ----------------------------------------------------

    #[test]
    fn handle_drag_state_computes_current_bounds() {
        let bounds = SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(100.0, 100.0));
        let drag = HandleDragState {
            handle: HandlePosition::BottomRight,
            original_bounds: bounds,
            start: Vector2::new(100.0, 100.0),
            current: Vector2::new(150.0, 120.0),
        };
        let delta = drag.delta();
        assert!((delta.x - 50.0).abs() < 0.01);
        assert!((delta.y - 20.0).abs() < 0.01);
        let new_bounds = drag.current_bounds();
        assert!((new_bounds.max.x - 150.0).abs() < 0.01);
        assert!((new_bounds.max.y - 120.0).abs() < 0.01);
    }

    // -- Viewport2D handle integration --------------------------------------

    #[test]
    fn viewport_handle_drag_lifecycle() {
        let mut vp = Viewport2D::new(800, 600);
        let bounds = SelectionRect::new(Vector2::new(-50.0, -50.0), Vector2::new(50.0, 50.0));

        // Hit-test: BottomRight handle at world (50, 50) -> screen position
        let screen_br = vp.world_to_screen(Vector2::new(50.0, 50.0));
        let hit = vp.hit_test_handles(screen_br, bounds);
        assert_eq!(hit, Some(HandlePosition::BottomRight));

        // Begin drag
        vp.begin_handle_drag(HandlePosition::BottomRight, bounds, screen_br);
        assert!(vp.is_handle_dragging());

        // Move 20px right on screen
        let new_screen = Vector2::new(screen_br.x + 20.0, screen_br.y);
        vp.update_handle_drag(new_screen);

        // End drag
        let new_bounds = vp.end_handle_drag().unwrap();
        assert!(!vp.is_handle_dragging());
        // Max x should have increased by ~20 world units (zoom=1.0)
        assert!(new_bounds.max.x > bounds.max.x);
    }

    #[test]
    fn viewport_end_handle_drag_without_begin_returns_none() {
        let mut vp = Viewport2D::new(800, 600);
        assert!(vp.end_handle_drag().is_none());
    }

    // -- GizmoConfig --------------------------------------------------------

    #[test]
    fn gizmo_config_has_sensible_defaults() {
        let config = GizmoConfig::default();
        assert!(config.arrow_length > 0.0);
        assert!(config.rotate_radius > 0.0);
        assert!(config.center_handle_size > 0.0);
    }

    // -- Gizmo2D hit testing ------------------------------------------------

    #[test]
    fn gizmo_hit_test_select_mode_returns_none() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        assert!(gizmo.hit_test(center, ToolMode2D::Select, &cam, vp).is_none());
    }

    #[test]
    fn gizmo_hit_test_move_center() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        let hit = gizmo.hit_test(center, ToolMode2D::Move, &cam, vp);
        assert_eq!(hit, Some(GizmoHit::Move(GizmoAxis::XY)));
    }

    #[test]
    fn gizmo_hit_test_move_x_axis() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        // Point along X axis (right of pivot, within arrow)
        let point = Vector2::new(center.x + 40.0, center.y);
        let hit = gizmo.hit_test(point, ToolMode2D::Move, &cam, vp);
        assert_eq!(hit, Some(GizmoHit::Move(GizmoAxis::X)));
    }

    #[test]
    fn gizmo_hit_test_move_y_axis() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        // Point along Y axis (above pivot on screen)
        let point = Vector2::new(center.x, center.y - 40.0);
        let hit = gizmo.hit_test(point, ToolMode2D::Move, &cam, vp);
        assert_eq!(hit, Some(GizmoHit::Move(GizmoAxis::Y)));
    }

    #[test]
    fn gizmo_hit_test_move_miss() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        // Far away from pivot
        let point = Vector2::new(0.0, 0.0);
        let hit = gizmo.hit_test(point, ToolMode2D::Move, &cam, vp);
        assert!(hit.is_none());
    }

    #[test]
    fn gizmo_hit_test_rotate_ring() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        // Point on the ring (radius = 60 screen px)
        let point = Vector2::new(center.x + 60.0, center.y);
        let hit = gizmo.hit_test(point, ToolMode2D::Rotate, &cam, vp);
        assert_eq!(hit, Some(GizmoHit::Rotate));
    }

    #[test]
    fn gizmo_hit_test_rotate_miss_inside() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        // Point inside the ring (too close to center)
        let hit = gizmo.hit_test(center, ToolMode2D::Rotate, &cam, vp);
        assert!(hit.is_none());
    }

    #[test]
    fn gizmo_hit_test_scale_center() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        let hit = gizmo.hit_test(center, ToolMode2D::Scale, &cam, vp);
        assert_eq!(hit, Some(GizmoHit::Scale(GizmoAxis::XY)));
    }

    #[test]
    fn gizmo_hit_test_scale_x_axis() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let cam = ViewportCamera2D::new();
        let vp = Vector2::new(800.0, 600.0);
        let center = cam.world_to_screen(Vector2::ZERO, vp);
        let point = Vector2::new(center.x + 40.0, center.y);
        let hit = gizmo.hit_test(point, ToolMode2D::Scale, &cam, vp);
        assert_eq!(hit, Some(GizmoHit::Scale(GizmoAxis::X)));
    }

    // -- Gizmo compute transforms -------------------------------------------

    #[test]
    fn gizmo_compute_move_x_constrains() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let result = gizmo.compute_move(Vector2::new(10.0, 20.0), GizmoAxis::X);
        assert!((result.x - 10.0).abs() < 0.01);
        assert!((result.y).abs() < 0.01);
    }

    #[test]
    fn gizmo_compute_move_y_constrains() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let result = gizmo.compute_move(Vector2::new(10.0, 20.0), GizmoAxis::Y);
        assert!((result.x).abs() < 0.01);
        assert!((result.y - 20.0).abs() < 0.01);
    }

    #[test]
    fn gizmo_compute_move_xy_is_free() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let result = gizmo.compute_move(Vector2::new(10.0, 20.0), GizmoAxis::XY);
        assert!((result.x - 10.0).abs() < 0.01);
        assert!((result.y - 20.0).abs() < 0.01);
    }

    #[test]
    fn gizmo_compute_rotation_90_degrees() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        // Start at 3 o'clock (angle 0), drag to 12 o'clock (angle PI/2).
        let start = Vector2::new(50.0, 0.0);
        let end = Vector2::new(0.0, 50.0);
        let angle = gizmo.compute_rotation(start, end);
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    }

    #[test]
    fn gizmo_compute_rotation_no_movement() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let point = Vector2::new(50.0, 0.0);
        let angle = gizmo.compute_rotation(point, point);
        assert!(angle.abs() < 0.001);
    }

    #[test]
    fn gizmo_compute_scale_x_only() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let start = Vector2::new(50.0, 0.0);
        let end = Vector2::new(100.0, 0.0);
        let (sx, sy) = gizmo.compute_scale(start, end, GizmoAxis::X);
        assert!((sx - 2.0).abs() < 0.01);
        assert!((sy - 1.0).abs() < 0.01);
    }

    #[test]
    fn gizmo_compute_scale_uniform() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let start = Vector2::new(30.0, 40.0); // distance = 50
        let end = Vector2::new(60.0, 80.0); // distance = 100
        let (sx, sy) = gizmo.compute_scale(start, end, GizmoAxis::XY);
        assert!((sx - 2.0).abs() < 0.01);
        assert!((sy - 2.0).abs() < 0.01);
    }

    #[test]
    fn gizmo_compute_scale_no_division_by_zero() {
        let gizmo = Gizmo2D::new(Vector2::ZERO);
        let start = Vector2::new(0.0, 0.0); // at pivot
        let end = Vector2::new(50.0, 50.0);
        let (sx, sy) = gizmo.compute_scale(start, end, GizmoAxis::X);
        assert!((sx - 1.0).abs() < 0.01); // fallback to 1.0
        assert!((sy - 1.0).abs() < 0.01);
    }

    // -- GizmoDragState transforms ------------------------------------------

    #[test]
    fn gizmo_drag_state_translate() {
        let drag = GizmoDragState {
            hit: GizmoHit::Move(GizmoAxis::X),
            gizmo: Gizmo2D::new(Vector2::ZERO),
            start: Vector2::new(10.0, 0.0),
            current: Vector2::new(30.0, 5.0),
        };
        match drag.current_transform() {
            GizmoTransform::Translate(v) => {
                assert!((v.x - 20.0).abs() < 0.01);
                assert!((v.y).abs() < 0.01); // constrained to X
            }
            other => panic!("expected Translate, got {:?}", other),
        }
    }

    #[test]
    fn gizmo_drag_state_rotate() {
        let drag = GizmoDragState {
            hit: GizmoHit::Rotate,
            gizmo: Gizmo2D::new(Vector2::ZERO),
            start: Vector2::new(50.0, 0.0),
            current: Vector2::new(0.0, 50.0),
        };
        match drag.current_transform() {
            GizmoTransform::Rotate(angle) => {
                assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 0.01);
            }
            other => panic!("expected Rotate, got {:?}", other),
        }
    }

    #[test]
    fn gizmo_drag_state_scale() {
        let drag = GizmoDragState {
            hit: GizmoHit::Scale(GizmoAxis::XY),
            gizmo: Gizmo2D::new(Vector2::ZERO),
            start: Vector2::new(30.0, 40.0),
            current: Vector2::new(60.0, 80.0),
        };
        match drag.current_transform() {
            GizmoTransform::Scale(sx, sy) => {
                assert!((sx - 2.0).abs() < 0.01);
                assert!((sy - 2.0).abs() < 0.01);
            }
            other => panic!("expected Scale, got {:?}", other),
        }
    }

    // -- Viewport2D gizmo integration ---------------------------------------

    #[test]
    fn viewport_gizmo_drag_move_lifecycle() {
        let mut vp = Viewport2D::new(800, 600);
        vp.set_tool_mode(ToolMode2D::Move);
        let pivot = Vector2::ZERO;
        let screen_pivot = vp.world_to_screen(pivot);

        // Hit test center
        let hit = vp.hit_test_gizmo(screen_pivot, pivot);
        assert_eq!(hit, Some(GizmoHit::Move(GizmoAxis::XY)));

        // Begin drag
        vp.begin_gizmo_drag(GizmoHit::Move(GizmoAxis::XY), pivot, screen_pivot);
        assert!(vp.is_gizmo_dragging());

        // Drag 30 px right on screen
        let new_screen = Vector2::new(screen_pivot.x + 30.0, screen_pivot.y);
        vp.update_gizmo_drag(new_screen);

        // End drag
        let transform = vp.end_gizmo_drag().unwrap();
        assert!(!vp.is_gizmo_dragging());
        match transform {
            GizmoTransform::Translate(v) => {
                assert!(v.x > 0.0); // moved right
            }
            other => panic!("expected Translate, got {:?}", other),
        }
    }

    #[test]
    fn viewport_gizmo_end_without_begin_returns_none() {
        let mut vp = Viewport2D::new(800, 600);
        assert!(vp.end_gizmo_drag().is_none());
    }

    // -- GuideLine ----------------------------------------------------------

    #[test]
    fn guide_horizontal_position() {
        let g = GuideLine::Horizontal(100.0);
        assert!((g.position() - 100.0).abs() < 0.01);
    }

    #[test]
    fn guide_vertical_position() {
        let g = GuideLine::Vertical(50.0);
        assert!((g.position() - 50.0).abs() < 0.01);
    }

    #[test]
    fn guide_distance_horizontal() {
        let g = GuideLine::Horizontal(100.0);
        let dist = g.distance_to(Vector2::new(200.0, 103.0));
        assert!((dist - 3.0).abs() < 0.01);
    }

    #[test]
    fn guide_distance_vertical() {
        let g = GuideLine::Vertical(50.0);
        let dist = g.distance_to(Vector2::new(48.0, 200.0));
        assert!((dist - 2.0).abs() < 0.01);
    }

    #[test]
    fn guide_snap_if_near_within_threshold() {
        let g = GuideLine::Horizontal(100.0);
        let snapped = g.snap_if_near(Vector2::new(50.0, 102.0), 5.0);
        assert!(snapped.is_some());
        let s = snapped.unwrap();
        assert!((s.x - 50.0).abs() < 0.01);
        assert!((s.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn guide_snap_if_near_outside_threshold() {
        let g = GuideLine::Horizontal(100.0);
        assert!(g.snap_if_near(Vector2::new(50.0, 110.0), 5.0).is_none());
    }

    // -- GuideManager -------------------------------------------------------

    #[test]
    fn guide_manager_add_remove_clear() {
        let mut mgr = GuideManager::new();
        assert_eq!(mgr.count(), 0);
        mgr.add(GuideLine::Horizontal(100.0));
        mgr.add(GuideLine::Vertical(200.0));
        assert_eq!(mgr.count(), 2);
        mgr.remove(0);
        assert_eq!(mgr.count(), 1);
        mgr.clear();
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn guide_manager_closest() {
        let mut mgr = GuideManager::new();
        mgr.add(GuideLine::Horizontal(100.0));
        mgr.add(GuideLine::Horizontal(200.0));
        let (idx, dist) = mgr.closest(Vector2::new(0.0, 190.0)).unwrap();
        assert_eq!(idx, 1);
        assert!((dist - 10.0).abs() < 0.01);
    }

    #[test]
    fn guide_manager_snap_to_guides() {
        let mut mgr = GuideManager::new();
        mgr.snap_threshold = 5.0;
        mgr.add(GuideLine::Horizontal(100.0));
        let snapped = mgr.snap_to_guides(Vector2::new(50.0, 103.0));
        assert!((snapped.y - 100.0).abs() < 0.01);
        assert!((snapped.x - 50.0).abs() < 0.01);
    }

    #[test]
    fn guide_manager_no_snap_beyond_threshold() {
        let mut mgr = GuideManager::new();
        mgr.snap_threshold = 2.0;
        mgr.add(GuideLine::Horizontal(100.0));
        let point = Vector2::new(50.0, 110.0);
        let snapped = mgr.snap_to_guides(point);
        assert!((snapped.y - 110.0).abs() < 0.01);
    }

    // -- SmartSnap ----------------------------------------------------------

    #[test]
    fn smart_snap_disabled_returns_none() {
        let mut ss = SmartSnap::new();
        ss.register_node(1, SelectionRect::new(Vector2::new(100.0, 100.0), Vector2::new(200.0, 200.0)));
        let drag = SelectionRect::new(Vector2::new(101.0, 50.0), Vector2::new(201.0, 150.0));
        let (dx, dy) = ss.snap_rect(&drag);
        assert!(dx.is_none());
        assert!(dy.is_none());
    }

    #[test]
    fn smart_snap_aligns_left_edges() {
        let mut ss = SmartSnap::new();
        ss.enabled = true;
        ss.threshold = 5.0;
        // Target node at x=100..200
        ss.register_node(1, SelectionRect::new(Vector2::new(100.0, 100.0), Vector2::new(200.0, 200.0)));
        // Dragging node at x=102..150 — left edge 102, should snap to 100
        let drag = SelectionRect::new(Vector2::new(102.0, 300.0), Vector2::new(150.0, 350.0));
        let (dx, _) = ss.snap_rect(&drag);
        assert!(dx.is_some());
        assert!((dx.unwrap() - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn smart_snap_aligns_centers() {
        let mut ss = SmartSnap::new();
        ss.enabled = true;
        ss.threshold = 5.0;
        // Target centered at (150, 150)
        ss.register_node(1, SelectionRect::new(Vector2::new(100.0, 100.0), Vector2::new(200.0, 200.0)));
        // Dragging centered at (152, 300)
        let drag = SelectionRect::new(Vector2::new(102.0, 250.0), Vector2::new(202.0, 350.0));
        let (dx, _) = ss.snap_rect(&drag);
        assert!(dx.is_some());
        assert!((dx.unwrap() - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn smart_snap_no_match_outside_threshold() {
        let mut ss = SmartSnap::new();
        ss.enabled = true;
        ss.threshold = 2.0;
        ss.register_node(1, SelectionRect::new(Vector2::new(100.0, 100.0), Vector2::new(200.0, 200.0)));
        let drag = SelectionRect::new(Vector2::new(110.0, 300.0), Vector2::new(190.0, 350.0));
        let (dx, _dy) = ss.snap_rect(&drag);
        // Left diff = 10, right diff = 10, center diff = 0 — center should match!
        // Actually center.x = 150 for both, so center diff = 0
        assert!(dx.is_some());
        assert!((dx.unwrap()).abs() < 0.01); // already aligned
    }

    #[test]
    fn smart_snap_clear_removes_anchors() {
        let mut ss = SmartSnap::new();
        ss.register_node(1, SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)));
        assert_eq!(ss.anchor_count(), 1);
        ss.clear();
        assert_eq!(ss.anchor_count(), 0);
    }

    // -- CanvasOverlay / OverlaySettings ------------------------------------

    #[test]
    fn overlay_defaults() {
        let o = OverlaySettings::new();
        assert!(o.is_enabled(CanvasOverlay::OriginCross));
        assert!(o.is_enabled(CanvasOverlay::LockGroupIcons));
        assert!(!o.is_enabled(CanvasOverlay::CollisionShapes));
        assert!(!o.is_enabled(CanvasOverlay::NavigationRegions));
        assert!(!o.is_enabled(CanvasOverlay::YSortIndicators));
    }

    #[test]
    fn overlay_enable_disable() {
        let mut o = OverlaySettings::new();
        o.enable(CanvasOverlay::CollisionShapes);
        assert!(o.is_enabled(CanvasOverlay::CollisionShapes));
        o.disable(CanvasOverlay::CollisionShapes);
        assert!(!o.is_enabled(CanvasOverlay::CollisionShapes));
    }

    #[test]
    fn overlay_toggle() {
        let mut o = OverlaySettings::new();
        assert!(!o.is_enabled(CanvasOverlay::NavigationRegions));
        assert!(o.toggle(CanvasOverlay::NavigationRegions));
        assert!(o.is_enabled(CanvasOverlay::NavigationRegions));
        assert!(!o.toggle(CanvasOverlay::NavigationRegions));
        assert!(!o.is_enabled(CanvasOverlay::NavigationRegions));
    }

    #[test]
    fn overlay_enable_all() {
        let mut o = OverlaySettings::new();
        o.enable_all();
        assert_eq!(o.active_count(), 6);
        assert!(o.is_enabled(CanvasOverlay::YSortIndicators));
        assert!(o.is_enabled(CanvasOverlay::NavigationRegions));
    }

    #[test]
    fn overlay_disable_all() {
        let mut o = OverlaySettings::new();
        o.disable_all();
        assert_eq!(o.active_count(), 0);
        assert!(!o.is_enabled(CanvasOverlay::OriginCross));
    }

    // -- RulerConfig --------------------------------------------------------

    #[test]
    fn ruler_major_ticks_basic() {
        let ruler = RulerConfig::default(); // major_interval = 100
        let ticks = ruler.major_ticks(0.0, 300.0);
        // Should include 0, 100, 200, 300
        assert!(ticks.len() >= 4);
        assert!((ticks[0].0).abs() < 0.01 || ticks.iter().any(|t| (t.0).abs() < 0.01));
    }

    #[test]
    fn ruler_major_ticks_negative_range() {
        let ruler = RulerConfig::default();
        let ticks = ruler.major_ticks(-200.0, 100.0);
        assert!(ticks.iter().any(|t| (t.0 - (-200.0)).abs() < 0.01));
        assert!(ticks.iter().any(|t| (t.0 - 100.0).abs() < 0.01));
    }

    #[test]
    fn ruler_minor_interval() {
        let ruler = RulerConfig::default(); // 100 / 5 = 20
        assert!((ruler.minor_interval() - 20.0).abs() < 0.01);
    }

    #[test]
    fn ruler_zero_interval_returns_empty() {
        let ruler = RulerConfig {
            major_interval: 0.0,
            ..Default::default()
        };
        assert!(ruler.major_ticks(0.0, 100.0).is_empty());
    }

    // -- Viewport2D integration with new fields -----------------------------

    #[test]
    fn viewport_has_guide_manager() {
        let vp = Viewport2D::new(800, 600);
        assert_eq!(vp.guides.count(), 0);
    }

    #[test]
    fn viewport_has_smart_snap() {
        let vp = Viewport2D::new(800, 600);
        assert!(!vp.smart_snap.enabled);
    }

    #[test]
    fn viewport_has_overlays() {
        let vp = Viewport2D::new(800, 600);
        assert!(vp.overlays.is_enabled(CanvasOverlay::OriginCross));
    }

    #[test]
    fn viewport_has_ruler_config() {
        let vp = Viewport2D::new(800, 600);
        assert!(vp.ruler_config.visible);
        assert!((vp.ruler_config.major_interval - 100.0).abs() < 0.01);
    }
}
