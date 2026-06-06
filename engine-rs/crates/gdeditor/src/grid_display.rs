//! The configurable 2D editor grid.
//!
//! Computes the grid lines drawn in the 2D viewport from a step size, an origin
//! offset, and a primary-line interval (every Nth line is a heavier "major"
//! line). Visibility is toggled from the View menu; while hidden the grid
//! produces no lines.
//!
//! Lines are reported per axis for a given visible world-space range, so the
//! renderer only emits the lines actually on screen.

/// A single grid line at a world-space coordinate (an x for a vertical line, a
/// y for a horizontal one), flagged if it is a primary (major) line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    /// World-space position of the line along its axis.
    pub position: f32,
    /// Whether this is a primary (major) line.
    pub primary: bool,
}

/// The editor grid's display configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct GridDisplay {
    /// Spacing between grid lines in world units.
    pub step: f32,
    /// Grid origin offset `[x, y]`; lines pass through this point.
    pub offset: [f32; 2],
    /// Every Nth line from the origin is a primary line (`0` = no primaries).
    pub primary_interval: u32,
    /// Whether the grid is currently drawn.
    pub visible: bool,
}

impl Default for GridDisplay {
    fn default() -> Self {
        Self {
            step: 16.0,
            offset: [0.0, 0.0],
            primary_interval: 8,
            visible: true,
        }
    }
}

impl GridDisplay {
    /// Toggles whether the grid is drawn (View-menu visibility).
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Whether the grid is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Vertical grid lines (each a constant x) within `[min_x, max_x]`.
    pub fn vertical_lines(&self, min_x: f32, max_x: f32) -> Vec<GridLine> {
        self.lines_along(min_x, max_x, self.offset[0])
    }

    /// Horizontal grid lines (each a constant y) within `[min_y, max_y]`.
    pub fn horizontal_lines(&self, min_y: f32, max_y: f32) -> Vec<GridLine> {
        self.lines_along(min_y, max_y, self.offset[1])
    }

    /// Computes the grid lines along one axis between `min` and `max`, aligned
    /// to `offset` + multiples of `step`. Empty when hidden or the step is
    /// non-positive.
    fn lines_along(&self, min: f32, max: f32, offset: f32) -> Vec<GridLine> {
        if !self.visible || self.step <= 0.0 || max < min {
            return Vec::new();
        }
        let first = ((min - offset) / self.step).ceil() as i64;
        let last = ((max - offset) / self.step).floor() as i64;
        let mut lines = Vec::new();
        for i in first..=last {
            let position = offset + (i as f32) * self.step;
            let primary =
                self.primary_interval > 0 && i.rem_euclid(self.primary_interval as i64) == 0;
            lines.push(GridLine { position, primary });
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(lines: &[GridLine]) -> Vec<f32> {
        lines.iter().map(|l| l.position).collect()
    }

    /// Acceptance (pat-suv2e): enabling the grid draws lines at the configured
    /// step and offset, and toggling visibility hides them.
    #[test]
    fn viewport_grid_display_respects_step_offset() {
        let mut grid = GridDisplay {
            step: 10.0,
            offset: [5.0, 5.0],
            primary_interval: 4,
            visible: true,
        };

        // Lines fall on offset + multiples of step, within the visible range.
        let v = grid.vertical_lines(0.0, 40.0);
        assert_eq!(positions(&v), vec![5.0, 15.0, 25.0, 35.0], "vertical lines honor step + offset");
        let h = grid.horizontal_lines(0.0, 25.0);
        assert_eq!(positions(&h), vec![5.0, 15.0, 25.0], "horizontal lines honor step + offset");

        // The line on the origin (offset) is primary; the next few are not.
        assert!(v[0].primary, "the line at the offset origin is primary");
        assert!(!v[1].primary && !v[2].primary && !v[3].primary, "intermediate lines are minor");

        // Changing the step changes the spacing.
        grid.step = 20.0;
        assert_eq!(
            positions(&grid.vertical_lines(0.0, 40.0)),
            vec![5.0, 25.0],
            "a larger step spaces the lines further apart"
        );

        // Toggling visibility off hides every line.
        grid.set_visible(false);
        assert!(!grid.is_visible());
        assert!(grid.vertical_lines(0.0, 40.0).is_empty(), "hidden grid draws no vertical lines");
        assert!(grid.horizontal_lines(0.0, 40.0).is_empty(), "hidden grid draws no horizontal lines");
    }

    /// Primary lines recur at the configured interval.
    #[test]
    fn primary_lines_recur_at_interval() {
        let grid = GridDisplay {
            step: 10.0,
            offset: [0.0, 0.0],
            primary_interval: 4,
            visible: true,
        };
        // Indices 0..=9 → positions 0,10,...,90; primaries at i = 0, 4, 8.
        let v = grid.vertical_lines(0.0, 95.0);
        let primaries: Vec<f32> = v.iter().filter(|l| l.primary).map(|l| l.position).collect();
        assert_eq!(primaries, vec![0.0, 40.0, 80.0], "primary lines every 4th step");
    }
}
