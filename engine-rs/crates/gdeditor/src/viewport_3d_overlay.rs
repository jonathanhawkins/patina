//! The 3D viewport's reference overlay (origin axes) and view toolbar.
//!
//! Godot's 3D editor draws a ground grid plus three coloured origin axes (X
//! red, Y green, Z blue), and a small toolbar to toggle the grid, switch
//! perspective/orthographic projection, and snap to standard views. The grid
//! and camera presets already live in
//! [`viewport_3d`](crate::viewport_3d); this module adds the origin axes and
//! wires the toolbar actions over that public API, in its own file to avoid
//! growing the large `viewport_3d.rs`.

use gdcore::math::{Color, Vector3};

use crate::viewport_3d::Viewport3D;

/// One of the three world-space origin axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginAxis {
    /// The X axis (red).
    X,
    /// The Y axis (green).
    Y,
    /// The Z axis (blue).
    Z,
}

impl OriginAxis {
    /// The unit direction of this axis.
    pub fn direction(self) -> Vector3 {
        match self {
            OriginAxis::X => Vector3::new(1.0, 0.0, 0.0),
            OriginAxis::Y => Vector3::new(0.0, 1.0, 0.0),
            OriginAxis::Z => Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// The conventional colour of this axis (Godot's red/green/blue X/Y/Z).
    pub fn color(self) -> Color {
        match self {
            OriginAxis::X => Color::new(1.0, 0.0, 0.0, 1.0),
            OriginAxis::Y => Color::new(0.0, 1.0, 0.0, 1.0),
            OriginAxis::Z => Color::new(0.0, 0.0, 1.0, 1.0),
        }
    }
}

/// A drawable, coloured origin-axis line spanning the origin along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisLine {
    /// Which axis this line represents.
    pub axis: OriginAxis,
    /// Start point (the negative end).
    pub start: Vector3,
    /// End point (the positive end).
    pub end: Vector3,
    /// The axis colour.
    pub color: Color,
}

/// A standard 3D view preset, mirroring Godot's numpad views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    /// Looking down −Z.
    Front,
    /// Looking straight down (top-down).
    Top,
    /// Looking down −X (right side).
    Right,
}

impl Viewport3D {
    /// The three coloured origin axis lines, each spanning `[-length, +length]`
    /// along its axis through the world origin — the reference gizmo drawn in
    /// the 3D viewport.
    pub fn origin_axes(&self, length: f32) -> Vec<AxisLine> {
        [OriginAxis::X, OriginAxis::Y, OriginAxis::Z]
            .into_iter()
            .map(|axis| {
                let d = axis.direction();
                AxisLine {
                    axis,
                    start: Vector3::new(-d.x * length, -d.y * length, -d.z * length),
                    end: Vector3::new(d.x * length, d.y * length, d.z * length),
                    color: axis.color(),
                }
            })
            .collect()
    }

    // -- view toolbar actions --------------------------------------------

    /// Toggles ground-grid visibility, returning the new visibility.
    pub fn toolbar_toggle_grid(&mut self) -> bool {
        self.grid.visible = !self.grid.visible;
        self.grid.visible
    }

    /// Toggles between perspective and orthographic projection.
    pub fn toolbar_toggle_projection(&mut self) {
        self.camera.toggle_projection();
    }

    /// Snaps the camera to a standard view preset.
    pub fn toolbar_set_view(&mut self, preset: ViewPreset) {
        match preset {
            ViewPreset::Front => self.camera.snap_to_front(),
            ViewPreset::Top => self.camera.snap_to_top(),
            ViewPreset::Right => self.camera.snap_to_right(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::{Projection, Viewport3D};

    /// Acceptance (pat-kka71.5): the 3D viewport exposes a ground grid and three
    /// coloured origin axes, and the view toolbar toggles the grid, flips the
    /// projection, and snaps to view presets.
    #[test]
    fn editor3d_grid_and_axes() {
        let mut vp = Viewport3D::default();

        // Three coloured origin axes through the world origin.
        let axes = vp.origin_axes(10.0);
        assert_eq!(axes.len(), 3);

        let x = axes.iter().find(|a| a.axis == OriginAxis::X).unwrap();
        assert_eq!(x.color, Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(x.start, Vector3::new(-10.0, 0.0, 0.0));
        assert_eq!(x.end, Vector3::new(10.0, 0.0, 0.0));

        let y = axes.iter().find(|a| a.axis == OriginAxis::Y).unwrap();
        assert_eq!(y.color, Color::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(y.end, Vector3::new(0.0, 10.0, 0.0));

        let z = axes.iter().find(|a| a.axis == OriginAxis::Z).unwrap();
        assert_eq!(z.color, Color::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(z.end, Vector3::new(0.0, 0.0, 10.0));

        // Grid is visible by default and emits lines; the toolbar hides/shows it.
        assert!(!vp.grid.generate_lines().is_empty(), "grid visible by default");
        assert!(!vp.toolbar_toggle_grid(), "toggle hides the grid");
        assert!(vp.grid.generate_lines().is_empty(), "hidden grid draws no lines");
        assert!(vp.toolbar_toggle_grid(), "toggle restores the grid");
        assert!(!vp.grid.generate_lines().is_empty());

        // Projection toggle flips perspective <-> orthographic.
        let proj0 = vp.camera.projection;
        vp.toolbar_toggle_projection();
        assert_ne!(vp.camera.projection, proj0);
        assert!(matches!(
            vp.camera.projection,
            Projection::Perspective | Projection::Orthographic
        ));

        // View presets snap the camera: Front looks down −Z (yaw 0, pitch 0).
        vp.toolbar_set_view(ViewPreset::Front);
        assert_eq!(vp.camera.yaw, 0.0);
        assert_eq!(vp.camera.pitch, 0.0);
        // Top looks straight down (pitch is driven negative).
        vp.toolbar_set_view(ViewPreset::Top);
        assert!(vp.camera.pitch < 0.0, "top view looks downward");
    }
}
