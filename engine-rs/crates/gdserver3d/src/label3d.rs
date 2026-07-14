//! Label3D: text rendered as camera-facing 3D geometry (pat-z35r2.12).
//!
//! Mirrors Godot's `Label3D`: the label's text is laid out as a row of
//! per-glyph quads in 3D space ([`Label3D::glyph_quads`]), sized by
//! `pixel_size`, and — when a billboard mode is set — the whole label is
//! oriented toward the camera via [`Label3D::billboard_basis`] (identical
//! semantics to [`crate::sprite3d::Sprite3D::billboard_basis`]).
//!
//! Before this the `Label3D` class existed only in ClassDB and never produced
//! geometry, so a `Label3D` node rendered nothing in the 3D/software path.
//!
//! The glyph layout uses a fixed monospace cell (matching the engine's 5×7
//! bitmap font metrics) so the node has real, renderable geometry on the
//! software path without pulling a font crate into `gdserver3d` (which depends
//! only on `gdcore`/`gdvariant`). A renderer maps each quad to its glyph in a
//! font atlas; this server type provides the geometry + billboard basis.

use crate::sprite3d::BillboardMode3D;
use gdcore::math::Vector3;
use gdcore::math3d::Basis;

/// Monospace glyph cell width in pixels (matches the engine's 5×7 bitmap font).
pub const GLYPH_CELL_WIDTH: f32 = 5.0;
/// Monospace glyph cell height in pixels.
pub const GLYPH_CELL_HEIGHT: f32 = 7.0;
/// Horizontal gap between adjacent glyph cells in pixels.
pub const GLYPH_GAP: f32 = 1.0;

/// A 2D text label rendered in 3D space, mirroring Godot's `Label3D`.
#[derive(Debug, Clone)]
pub struct Label3D {
    /// The text to display.
    pub text: String,
    /// World units per pixel (Godot default 0.01).
    pub pixel_size: f32,
    /// Scale applied to the base monospace glyph cell (acts as font size).
    pub font_scale: f32,
    /// Billboard mode controlling how the label faces the camera.
    pub billboard: BillboardMode3D,
    /// When true the text row is centered on the node origin; otherwise it
    /// starts at the origin and extends along +X.
    pub centered: bool,
    /// Text modulate color (RGBA), applied by the renderer.
    pub modulate: [f32; 4],
}

impl Default for Label3D {
    fn default() -> Self {
        Self {
            text: String::new(),
            pixel_size: 0.01,
            font_scale: 1.0,
            billboard: BillboardMode3D::Disabled,
            centered: true,
            modulate: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

impl Label3D {
    /// Creates a new empty label with Godot-default properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Horizontal advance (world units) of one glyph cell, including the gap.
    fn advance(&self) -> f32 {
        (GLYPH_CELL_WIDTH + GLYPH_GAP) * self.font_scale * self.pixel_size
    }

    /// Total world-space width of the laid-out text. Every character (including
    /// whitespace) advances the cursor, so this reflects the full row width.
    pub fn world_width(&self) -> f32 {
        self.text.chars().count() as f32 * self.advance()
    }

    /// World-space height of a glyph cell.
    pub fn cell_height(&self) -> f32 {
        GLYPH_CELL_HEIGHT * self.font_scale * self.pixel_size
    }

    /// Number of visible (non-whitespace) glyphs that will be rendered.
    pub fn visible_glyph_count(&self) -> usize {
        self.text.chars().filter(|c| !c.is_whitespace()).count()
    }

    /// Generates one quad (4 corners CCW from bottom-left) per *visible*
    /// character, laid out left-to-right along +X on the local XY plane and
    /// vertically centered on the baseline. Whitespace advances the cursor but
    /// emits no quad. Returns an empty vec for empty/whitespace-only text.
    ///
    /// These quads are the software-path geometry a renderer rasterizes (one
    /// glyph per quad); combined with [`Label3D::billboard_basis`] they let a
    /// `Label3D` node actually render in the 3D view.
    pub fn glyph_quads(&self) -> Vec<[Vector3; 4]> {
        let adv = self.advance();
        let half_h = self.cell_height() * 0.5;
        let glyph_w = GLYPH_CELL_WIDTH * self.font_scale * self.pixel_size;

        // Left edge of the row; center the whole row when `centered`.
        let mut cursor_x = if self.centered {
            -self.world_width() * 0.5
        } else {
            0.0
        };

        let mut quads = Vec::new();
        for ch in self.text.chars() {
            if !ch.is_whitespace() {
                let x0 = cursor_x;
                let x1 = cursor_x + glyph_w;
                quads.push([
                    Vector3::new(x0, -half_h, 0.0), // bottom-left
                    Vector3::new(x1, -half_h, 0.0), // bottom-right
                    Vector3::new(x1, half_h, 0.0),  // top-right
                    Vector3::new(x0, half_h, 0.0),  // top-left
                ]);
            }
            cursor_x += adv;
        }
        quads
    }

    /// Computes the rotation basis for billboard orientation, identical in
    /// semantics to [`crate::sprite3d::Sprite3D::billboard_basis`]: the label's
    /// local +Z is oriented toward the camera. Returns [`Basis::IDENTITY`] when
    /// billboarding is disabled or the camera/label are coincident.
    pub fn billboard_basis(&self, label_pos: Vector3, camera_pos: Vector3) -> Basis {
        let world_up = Vector3::UP;
        let delta = camera_pos - label_pos;

        match self.billboard {
            BillboardMode3D::Disabled => Basis::IDENTITY,
            BillboardMode3D::Enabled | BillboardMode3D::Particles => {
                if delta.length_squared() < 1e-12 {
                    return Basis::IDENTITY;
                }
                let forward = delta.normalized();
                let right_unnorm = world_up.cross(forward);
                if right_unnorm.length_squared() < 1e-12 {
                    return Basis::IDENTITY;
                }
                let right = right_unnorm.normalized();
                let up = forward.cross(right).normalized();
                Basis {
                    x: right,
                    y: up,
                    z: forward,
                }
            }
            BillboardMode3D::YBillboard => {
                let flat = Vector3::new(delta.x, 0.0, delta.z);
                if flat.length_squared() < 1e-12 {
                    return Basis::IDENTITY;
                }
                let forward = flat.normalized();
                let right = world_up.cross(forward).normalized();
                Basis {
                    x: right,
                    y: world_up,
                    z: forward,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v_approx(a: Vector3, b: Vector3) -> bool {
        (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5
    }

    #[test]
    fn glyph_quads_one_per_visible_char() {
        let mut label = Label3D::new();
        label.text = "AB".into();
        assert_eq!(label.glyph_quads().len(), 2);
        assert_eq!(label.visible_glyph_count(), 2);
    }

    #[test]
    fn glyph_quads_skip_whitespace_but_advance() {
        let mut label = Label3D::new();
        label.text = "A B".into();
        // Two visible glyphs; the space emits no quad.
        assert_eq!(label.glyph_quads().len(), 2);
        assert_eq!(label.visible_glyph_count(), 2);
        // The row is still 3 cells wide (space advances the cursor).
        assert!((label.world_width() - 3.0 * label.advance()).abs() < 1e-6);
    }

    #[test]
    fn empty_text_has_no_glyphs() {
        let label = Label3D::new();
        assert!(label.glyph_quads().is_empty());
        assert_eq!(label.visible_glyph_count(), 0);
    }

    #[test]
    fn glyphs_advance_left_to_right() {
        let mut label = Label3D::new();
        label.centered = false;
        label.text = "AB".into();
        let q = label.glyph_quads();
        assert!(q[1][0].x > q[0][0].x, "glyphs must advance along +X: {q:?}");
    }

    #[test]
    fn billboard_disabled_is_identity() {
        let label = Label3D::new(); // Disabled by default
        let b = label.billboard_basis(Vector3::ZERO, Vector3::new(5.0, 0.0, 5.0));
        assert!(v_approx(b.x, Vector3::new(1.0, 0.0, 0.0)));
        assert!(v_approx(b.y, Vector3::new(0.0, 1.0, 0.0)));
        assert!(v_approx(b.z, Vector3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn billboard_enabled_faces_camera() {
        let mut label = Label3D::new();
        label.billboard = BillboardMode3D::Enabled;
        let label_pos = Vector3::ZERO;
        let camera_pos = Vector3::new(0.0, 0.0, 10.0);
        let b = label.billboard_basis(label_pos, camera_pos);
        // Local +Z (forward) points toward the camera, so the label faces it.
        let forward = (camera_pos - label_pos).normalized();
        assert!(v_approx(b.z, forward), "forward {:?} vs {forward:?}", b.z);
    }

    #[test]
    fn y_billboard_keeps_world_up_fixed() {
        let mut label = Label3D::new();
        label.billboard = BillboardMode3D::YBillboard;
        // Camera above and to the side; Y stays world-up regardless of height.
        let b = label.billboard_basis(Vector3::ZERO, Vector3::new(3.0, 8.0, 4.0));
        assert!(v_approx(b.y, Vector3::UP));
    }
}
