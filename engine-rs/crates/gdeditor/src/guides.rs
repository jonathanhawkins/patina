//! Ruler guides for the 2D viewport.
//!
//! Guides are dragged out of the rulers: the top (horizontal) ruler produces a
//! horizontal guide (a constant-`y` line) and the left (vertical) ruler a
//! vertical guide (a constant-`x` line). Guides can be repositioned after
//! creation, and — when snapping is enabled — a dragged node snaps its position
//! to nearby guides.

use gdcore::math::Vector2;

/// A guide's orientation. A horizontal guide is a constant-`y` line; a vertical
/// guide is a constant-`x` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideOrientation {
    /// A horizontal line, dragged from the top ruler, fixed at a `y` value.
    Horizontal,
    /// A vertical line, dragged from the left ruler, fixed at an `x` value.
    Vertical,
}

/// A single guide line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    /// Orientation of the guide.
    pub orientation: GuideOrientation,
    /// The guide's world-space coordinate (`y` if horizontal, `x` if vertical).
    pub position: f32,
}

/// The set of guides currently placed in the viewport.
#[derive(Debug, Clone, Default)]
pub struct GuideSet {
    guides: Vec<Guide>,
}

impl GuideSet {
    /// Creates an empty guide set.
    pub fn new() -> Self {
        Self { guides: Vec::new() }
    }

    /// All placed guides, in creation order.
    pub fn guides(&self) -> &[Guide] {
        &self.guides
    }

    /// Number of placed guides.
    pub fn len(&self) -> usize {
        self.guides.len()
    }

    /// Whether no guides are placed.
    pub fn is_empty(&self) -> bool {
        self.guides.is_empty()
    }

    /// Creates a guide by dragging it out of a ruler, at world coordinate
    /// `position`. Returns the new guide's index.
    pub fn create_from_ruler(&mut self, orientation: GuideOrientation, position: f32) -> usize {
        self.guides.push(Guide {
            orientation,
            position,
        });
        self.guides.len() - 1
    }

    /// Repositions the guide at `index` to a new coordinate. Returns `false`
    /// if the index is out of range.
    pub fn move_guide(&mut self, index: usize, position: f32) -> bool {
        match self.guides.get_mut(index) {
            Some(g) => {
                g.position = position;
                true
            }
            None => false,
        }
    }

    /// Removes the guide at `index` (e.g. dragged back onto the ruler).
    pub fn remove_guide(&mut self, index: usize) -> bool {
        if index < self.guides.len() {
            self.guides.remove(index);
            true
        } else {
            false
        }
    }

    /// Snaps a node position to nearby guides when `enabled`: vertical guides
    /// snap the `x` coordinate and horizontal guides snap the `y`, each to the
    /// nearest guide within `threshold`. Returns the position unchanged when
    /// snapping is off or no guide is in range.
    pub fn snap(&self, pos: Vector2, threshold: f32, enabled: bool) -> Vector2 {
        if !enabled {
            return pos;
        }
        let mut out = pos;
        let mut best_dx = threshold;
        let mut best_dy = threshold;
        for g in &self.guides {
            match g.orientation {
                GuideOrientation::Vertical => {
                    let d = (pos.x - g.position).abs();
                    if d <= best_dx {
                        best_dx = d;
                        out.x = g.position;
                    }
                }
                GuideOrientation::Horizontal => {
                    let d = (pos.y - g.position).abs();
                    if d <= best_dy {
                        best_dy = d;
                        out.y = g.position;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-wrfmu): dragging from a ruler creates a guide, the guide
    /// can be repositioned, and node drag snaps to it when snapping is on.
    #[test]
    fn viewport_guides_create_move_and_snap() {
        let mut guides = GuideSet::new();
        assert!(guides.is_empty());

        // Drag a vertical guide out of the left ruler (x=100) and a horizontal
        // guide out of the top ruler (y=50).
        let v = guides.create_from_ruler(GuideOrientation::Vertical, 100.0);
        let h = guides.create_from_ruler(GuideOrientation::Horizontal, 50.0);
        assert_eq!(guides.len(), 2);
        assert_eq!(
            guides.guides()[v],
            Guide {
                orientation: GuideOrientation::Vertical,
                position: 100.0
            }
        );
        assert_eq!(guides.guides()[h].orientation, GuideOrientation::Horizontal);

        // The guide can be repositioned.
        assert!(guides.move_guide(v, 120.0));
        assert_eq!(guides.guides()[v].position, 120.0);
        assert!(!guides.move_guide(99, 0.0), "moving a missing guide fails");

        // With snapping on, a node near the guides snaps to them (x to the
        // vertical guide, y to the horizontal guide).
        let threshold = 8.0;
        assert_eq!(
            guides.snap(Vector2::new(124.0, 47.0), threshold, true),
            Vector2::new(120.0, 50.0),
            "node snaps to the nearby guides"
        );
        // A node outside the threshold of every guide is unchanged.
        assert_eq!(
            guides.snap(Vector2::new(300.0, 300.0), threshold, true),
            Vector2::new(300.0, 300.0),
            "a node far from all guides does not snap"
        );
        // With snapping off, the node is never moved.
        assert_eq!(
            guides.snap(Vector2::new(124.0, 47.0), threshold, false),
            Vector2::new(124.0, 47.0),
            "snapping off leaves the node free"
        );

        // A guide can be removed (dragged back onto the ruler).
        assert!(guides.remove_guide(h));
        assert_eq!(guides.len(), 1);
    }
}
