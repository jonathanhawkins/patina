//! Grid snapping for dragged and newly-created nodes.
//!
//! When grid snapping is enabled, a node's candidate position is quantized to
//! the nearest grid intersection, honoring the grid step and offset from the
//! shared [`SnapConfig`]. When disabled, the position is returned unchanged so
//! the node moves freely.
//!
//! This reuses [`SnapConfig::snap_position`] so node snapping and the snap
//! dialog stay consistent.

use crate::snap_config::SnapConfig;
use gdcore::math::Vector2;

/// Snaps a node's candidate position to the grid when `enabled`, using the
/// grid step and offset from `config`. Returns the position unchanged when
/// snapping is off.
pub fn snap_node_to_grid(pos: Vector2, config: &SnapConfig, enabled: bool) -> Vector2 {
    if enabled {
        config.snap_position(pos)
    } else {
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SnapConfig {
        SnapConfig {
            grid_step: 16.0,
            grid_offset: [0.0, 0.0],
            rotation_step_deg: 15.0,
            scale_step: 0.1,
        }
    }

    /// Acceptance (pat-vv5o1): with grid snap on, dragging a node lands its
    /// position on the nearest grid intersection (honoring step and offset);
    /// with snap off it moves freely.
    #[test]
    fn viewport_grid_snapping_quantizes_position() {
        let config = config();

        // Snap on: the drop position lands on the nearest 16px intersection.
        assert_eq!(
            snap_node_to_grid(Vector2::new(20.0, 30.0), &config, true),
            Vector2::new(16.0, 32.0),
            "position snaps to the nearest grid intersection"
        );
        // A position already on an intersection is unchanged.
        assert_eq!(
            snap_node_to_grid(Vector2::new(32.0, 48.0), &config, true),
            Vector2::new(32.0, 48.0),
            "an on-grid position stays put"
        );

        // The grid offset shifts the intersections.
        let offset = SnapConfig {
            grid_offset: [4.0, 4.0],
            ..config.clone()
        };
        assert_eq!(
            snap_node_to_grid(Vector2::new(20.0, 30.0), &offset, true),
            Vector2::new(20.0, 36.0),
            "snapping honors the grid offset"
        );

        // Snap off: the node moves freely, exactly to the candidate position.
        assert_eq!(
            snap_node_to_grid(Vector2::new(20.0, 30.0), &config, false),
            Vector2::new(20.0, 30.0),
            "with snapping off the node is not quantized"
        );
    }
}
