//! The snap configuration dialog model.
//!
//! Holds the editor's snap settings — grid step & offset, rotation snap step,
//! and scale snap step — and applies them to candidate transforms. The whole
//! config serializes to JSON so the dialog's values persist across editor
//! sessions (mirroring the JSON persistence used elsewhere in the editor).
//!
//! Steps of `0` (or negative) disable that axis of snapping, leaving the input
//! value untouched.

use gdcore::math::Vector2;
use serde::{Deserialize, Serialize};

/// User-configurable snap steps for the viewport, as edited in the snap dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapConfig {
    /// Grid cell size in pixels. Positions snap to multiples of this.
    pub grid_step: f32,
    /// Grid origin offset `[x, y]` in pixels — the grid lines pass through here.
    pub grid_offset: [f32; 2],
    /// Rotation snap increment in degrees.
    pub rotation_step_deg: f32,
    /// Scale snap increment (e.g. `0.1` snaps scale to tenths).
    pub scale_step: f32,
}

impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            grid_step: 8.0,
            grid_offset: [0.0, 0.0],
            rotation_step_deg: 15.0,
            scale_step: 0.1,
        }
    }
}

/// Rounds `value` to the nearest multiple of `step` offset by `offset`. A
/// non-positive `step` disables snapping and returns `value` unchanged.
fn snap_scalar(value: f32, step: f32, offset: f32) -> f32 {
    if step <= 0.0 {
        return value;
    }
    ((value - offset) / step).round() * step + offset
}

impl SnapConfig {
    /// Snaps a position to the configured grid (step + offset), per axis.
    pub fn snap_position(&self, pos: Vector2) -> Vector2 {
        Vector2::new(
            snap_scalar(pos.x, self.grid_step, self.grid_offset[0]),
            snap_scalar(pos.y, self.grid_step, self.grid_offset[1]),
        )
    }

    /// Snaps a rotation (in degrees) to the configured rotation step.
    pub fn snap_rotation_deg(&self, angle_deg: f32) -> f32 {
        snap_scalar(angle_deg, self.rotation_step_deg, 0.0)
    }

    /// Snaps a scale factor to the configured scale step.
    pub fn snap_scale(&self, scale: f32) -> f32 {
        snap_scalar(scale, self.scale_step, 0.0)
    }

    /// Serializes the config to JSON for persistence across sessions.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Restores a config from previously persisted JSON, or `None` if invalid.
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-nrsm6): changing grid step, rotation step, and scale step
    /// in the snap dialog updates snapping behavior, and the values persist
    /// after a reload.
    #[test]
    fn viewport_snap_config_persists_and_applies() {
        // The user edits the dialog: grid step 10 (offset 2,2), rotation 45°,
        // scale step 0.25.
        let config = SnapConfig {
            grid_step: 10.0,
            grid_offset: [2.0, 2.0],
            rotation_step_deg: 45.0,
            scale_step: 0.25,
        };

        // Each edited step changes snapping behavior.
        let snapped_pos = config.snap_position(Vector2::new(23.0, 26.0));
        assert!(
            (snapped_pos.x - 22.0).abs() < 1e-4 && (snapped_pos.y - 22.0).abs() < 1e-4,
            "grid step + offset snap the position, got {snapped_pos:?}"
        );
        assert!(
            (config.snap_rotation_deg(50.0) - 45.0).abs() < 1e-4,
            "rotation snaps to the 45° step"
        );
        assert!(
            (config.snap_scale(0.3) - 0.25).abs() < 1e-4,
            "scale snaps to the 0.25 step"
        );

        // A different grid step yields different snapping — the setting matters.
        // Probe a point that straddles the two steps' grid lines: with offset 2,
        // x=33 snaps to 32 at step 10 but to 42 at step 20.
        let coarser = SnapConfig {
            grid_step: 20.0,
            ..config.clone()
        };
        let probe = Vector2::new(33.0, 26.0);
        assert!(
            (coarser.snap_position(probe).x - config.snap_position(probe).x).abs() > 1e-4,
            "changing the grid step changes where positions snap"
        );

        // Persist to JSON and reload (simulating a new session).
        let persisted = config.to_json();
        let reloaded = SnapConfig::from_json(&persisted).expect("persisted config reloads");
        assert_eq!(reloaded, config, "all snap values survive a save/reload");

        // The reloaded config snaps identically to the original.
        assert_eq!(
            reloaded.snap_position(Vector2::new(23.0, 26.0)),
            config.snap_position(Vector2::new(23.0, 26.0)),
            "reloaded config applies the same snapping behavior"
        );
        assert_eq!(reloaded.snap_rotation_deg(50.0), config.snap_rotation_deg(50.0));
        assert_eq!(reloaded.snap_scale(0.3), config.snap_scale(0.3));

        // Defaults are sane and a zero step disables that axis of snapping.
        let mut no_grid = SnapConfig::default();
        no_grid.grid_step = 0.0;
        let p = Vector2::new(13.3, 7.7);
        assert_eq!(no_grid.snap_position(p), p, "a zero grid step disables grid snap");
    }
}
