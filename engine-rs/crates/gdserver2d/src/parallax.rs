//! Parallax layer for scrolling background effects.
//!
//! A [`ParallaxLayer`] offsets its contents relative to camera movement,
//! scaled by [`motion_scale`](ParallaxLayer::motion_scale), creating a
//! depth-based scrolling effect.

use gdcore::math::Vector2;

/// A parallax scrolling layer.
///
/// When the camera moves by `delta`, this layer's effective offset is
/// `delta * motion_scale + motion_offset`, optionally wrapped by
/// [`motion_mirroring`](Self::motion_mirroring) for infinite scrolling.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallaxLayer {
    /// Scale factor applied to camera movement. `(1, 1)` moves with the camera;
    /// `(0.5, 0.5)` moves at half speed (distant background).
    pub motion_scale: Vector2,
    /// Static offset applied in addition to the scaled camera movement.
    pub motion_offset: Vector2,
    /// Repeat period of the layer's contents, in pixels (Godot's
    /// `ParallaxLayer.motion_mirroring`).
    ///
    /// A non-zero axis wraps the computed offset into `[0, mirroring)` so the
    /// layer tiles forever; `0` (the default) disables mirroring on that axis.
    pub motion_mirroring: Vector2,
}

impl ParallaxLayer {
    /// Creates a new parallax layer with the given motion scale.
    pub fn new(motion_scale: Vector2) -> Self {
        Self {
            motion_scale,
            motion_offset: Vector2::ZERO,
            motion_mirroring: Vector2::ZERO,
        }
    }

    /// Computes the effective offset for this layer given a camera position.
    ///
    /// Equivalent to [`compute_offset_scaled`](Self::compute_offset_scaled)
    /// with a camera zoom of `1.0`.
    pub fn compute_offset(&self, camera_position: Vector2) -> Vector2 {
        self.compute_offset_scaled(camera_position, 1.0)
    }

    /// Computes the effective offset for a base offset and camera zoom,
    /// mirroring Godot's `ParallaxLayer::set_base_offset_and_scale`.
    ///
    /// The static `motion_offset` and the mirroring period both scale with
    /// `scale`, while the base offset is scaled by `motion_scale` — that is what
    /// keeps a mirrored layer seamless as the camera zooms.
    pub fn compute_offset_scaled(&self, base_offset: Vector2, scale: f32) -> Vector2 {
        let mut ofs = Vector2::new(
            base_offset.x * self.motion_scale.x + self.motion_offset.x * scale,
            base_offset.y * self.motion_scale.y + self.motion_offset.y * scale,
        );
        // Godot: `new_ofs -= den * floor(new_ofs / den)` — a euclidean modulo
        // that folds the offset back into one repeat period.
        ofs.x = mirror_axis(ofs.x, self.motion_mirroring.x * scale);
        ofs.y = mirror_axis(ofs.y, self.motion_mirroring.y * scale);
        ofs
    }
}

/// Wraps `value` into `[0, period)` when `period` is non-zero; otherwise returns
/// `value` untouched (mirroring disabled on that axis).
fn mirror_axis(value: f32, period: f32) -> f32 {
    if period == 0.0 || !period.is_finite() {
        return value;
    }
    value - period * (value / period).floor()
}

impl Default for ParallaxLayer {
    fn default() -> Self {
        Self {
            motion_scale: Vector2::ONE,
            motion_offset: Vector2::ZERO,
            motion_mirroring: Vector2::ZERO,
        }
    }
}

/// Drives a set of [`ParallaxLayer`]s from a single scroll position, mirroring
/// Godot's `ParallaxBackground` (a CanvasLayer whose children are layers).
///
/// The background composes one base offset from the camera's scroll position
/// and hands it to each layer, which then applies its own motion scale and
/// mirroring.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallaxBackground {
    /// Scroll position, normally driven by the active Camera2D.
    pub scroll_offset: Vector2,
    /// Constant offset added to the scroll position.
    pub scroll_base_offset: Vector2,
    /// Scale applied to [`scroll_offset`](Self::scroll_offset) before layers
    /// apply their own motion scale.
    pub scroll_base_scale: Vector2,
    /// When true, layers ignore the camera's zoom (Godot's
    /// `scroll_ignore_camera_zoom`).
    pub scroll_ignore_camera_zoom: bool,
}

impl ParallaxBackground {
    /// Creates a background with Godot's defaults (no offset, unit base scale).
    pub fn new() -> Self {
        Self {
            scroll_offset: Vector2::ZERO,
            scroll_base_offset: Vector2::ZERO,
            scroll_base_scale: Vector2::ONE,
            scroll_ignore_camera_zoom: false,
        }
    }

    /// The base offset handed to every child layer, mirroring Godot's
    /// `ParallaxBackground::_update_scroll`.
    pub fn base_offset(&self) -> Vector2 {
        Vector2::new(
            self.scroll_base_offset.x + self.scroll_offset.x * self.scroll_base_scale.x,
            self.scroll_base_offset.y + self.scroll_offset.y * self.scroll_base_scale.y,
        )
    }

    /// The offset `layer` should be drawn at for the current scroll position and
    /// camera `zoom`. Honors [`scroll_ignore_camera_zoom`](Self::scroll_ignore_camera_zoom).
    pub fn layer_offset(&self, layer: &ParallaxLayer, zoom: f32) -> Vector2 {
        let scale = if self.scroll_ignore_camera_zoom {
            1.0
        } else {
            zoom
        };
        layer.compute_offset_scaled(self.base_offset(), scale)
    }
}

impl Default for ParallaxBackground {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallax_default_moves_with_camera() {
        let layer = ParallaxLayer::default();
        let offset = layer.compute_offset(Vector2::new(100.0, 50.0));
        assert!((offset.x - 100.0).abs() < 1e-6);
        assert!((offset.y - 50.0).abs() < 1e-6);
    }

    #[test]
    fn parallax_half_speed() {
        let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
        let offset = layer.compute_offset(Vector2::new(100.0, 200.0));
        assert!((offset.x - 50.0).abs() < 1e-6);
        assert!((offset.y - 100.0).abs() < 1e-6);
    }

    #[test]
    fn parallax_with_static_offset() {
        let mut layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
        layer.motion_offset = Vector2::new(10.0, 20.0);
        let offset = layer.compute_offset(Vector2::new(100.0, 200.0));
        assert!((offset.x - 60.0).abs() < 1e-6);
        assert!((offset.y - 120.0).abs() < 1e-6);
    }

    #[test]
    fn parallax_zero_scale_stays_fixed() {
        let layer = ParallaxLayer::new(Vector2::ZERO);
        let offset = layer.compute_offset(Vector2::new(999.0, 999.0));
        assert!((offset.x).abs() < 1e-6);
        assert!((offset.y).abs() < 1e-6);
    }

    // -- pat-u3l8c.31: motion mirroring + ParallaxBackground ------------------

    /// Mirroring is off by default, so the offset grows without bound exactly as
    /// it did before this bead.
    #[test]
    fn parallax_without_mirroring_does_not_wrap() {
        let layer = ParallaxLayer::default();
        assert_eq!(layer.motion_mirroring, Vector2::ZERO);
        let offset = layer.compute_offset(Vector2::new(5000.0, 0.0));
        assert!((offset.x - 5000.0).abs() < 1e-3);
    }

    /// The headline of the bead's parallax acceptance: a mirrored layer scrolls
    /// infinitely. Moving the camera by exactly one repeat period must land on
    /// the identical offset — that is what makes the tiling seamless.
    #[test]
    fn parallax_mirroring_repeats_seamlessly() {
        let mut layer = ParallaxLayer::new(Vector2::new(0.5, 1.0));
        layer.motion_mirroring = Vector2::new(256.0, 0.0);

        // motion_scale.x = 0.5, so one 256px period takes 512px of camera travel.
        let a = layer.compute_offset(Vector2::new(100.0, 0.0));
        let b = layer.compute_offset(Vector2::new(100.0 + 512.0, 0.0));
        let c = layer.compute_offset(Vector2::new(100.0 - 512.0, 0.0));
        assert!(
            (a.x - b.x).abs() < 1e-3,
            "one period of camera travel must repeat the layer offset ({} vs {})",
            a.x,
            b.x,
        );
        assert!(
            (a.x - c.x).abs() < 1e-3,
            "scrolling backwards must repeat too ({} vs {})",
            a.x,
            c.x,
        );
    }

    /// A mirrored offset always folds into `[0, period)`, including for negative
    /// camera positions — Godot wraps with a euclidean modulo, not a truncating
    /// remainder (which would go negative).
    #[test]
    fn parallax_mirroring_wraps_into_period_even_when_negative() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(100.0, 50.0);

        for camera_x in [-350.0f32, -1.0, 0.0, 40.0, 250.0, 1000.0] {
            let ofs = layer.compute_offset(Vector2::new(camera_x, -37.0));
            assert!(
                (0.0..100.0).contains(&ofs.x),
                "x offset {} must fold into [0, 100) for camera {}",
                ofs.x,
                camera_x,
            );
            assert!(
                (0.0..50.0).contains(&ofs.y),
                "y offset {} must fold into [0, 50)",
                ofs.y,
            );
        }
        // -350 with scale 1 wraps to 50 (not -50, which a truncating % gives).
        let ofs = layer.compute_offset(Vector2::new(-350.0, 0.0));
        assert!((ofs.x - 50.0).abs() < 1e-3, "expected 50, got {}", ofs.x);
    }

    /// Mirroring only applies per-axis: a zero axis keeps scrolling freely.
    #[test]
    fn parallax_mirroring_is_per_axis() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(100.0, 0.0);
        let ofs = layer.compute_offset(Vector2::new(250.0, 250.0));
        assert!((ofs.x - 50.0).abs() < 1e-3, "x wraps, got {}", ofs.x);
        assert!(
            (ofs.y - 250.0).abs() < 1e-3,
            "y is unmirrored, got {}",
            ofs.y
        );
    }

    /// The background composes scroll_offset * base_scale + base_offset and
    /// hands that to the layer, which applies its own motion scale.
    #[test]
    fn background_composes_base_offset_for_layers() {
        let mut bg = ParallaxBackground::new();
        bg.scroll_offset = Vector2::new(100.0, 40.0);
        bg.scroll_base_offset = Vector2::new(10.0, 5.0);
        bg.scroll_base_scale = Vector2::new(2.0, 1.0);
        // base = 10 + 100*2 = 210 (x), 5 + 40*1 = 45 (y)
        assert!((bg.base_offset().x - 210.0).abs() < 1e-3);
        assert!((bg.base_offset().y - 45.0).abs() < 1e-3);

        let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
        let ofs = bg.layer_offset(&layer, 1.0);
        assert!((ofs.x - 105.0).abs() < 1e-3, "210 * 0.5, got {}", ofs.x);
        assert!((ofs.y - 22.5).abs() < 1e-3, "45 * 0.5, got {}", ofs.y);
    }

    /// `scroll_ignore_camera_zoom` pins the layer scale to 1 so a zoomed camera
    /// does not stretch the layer's static offset or its mirroring period.
    #[test]
    fn background_ignore_camera_zoom_pins_scale() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_offset = Vector2::new(20.0, 0.0);

        let mut bg = ParallaxBackground::new();
        bg.scroll_offset = Vector2::new(100.0, 0.0);

        // Zoom 2 scales the static motion_offset: 100 + 20*2 = 140.
        let zoomed = bg.layer_offset(&layer, 2.0);
        assert!((zoomed.x - 140.0).abs() < 1e-3, "got {}", zoomed.x);

        // Ignoring zoom pins scale to 1: 100 + 20*1 = 120.
        bg.scroll_ignore_camera_zoom = true;
        let pinned = bg.layer_offset(&layer, 2.0);
        assert!((pinned.x - 120.0).abs() < 1e-3, "got {}", pinned.x);
    }

    /// Zoom scales the mirroring period too, so a zoomed mirrored layer still
    /// tiles seamlessly (period 100 at zoom 2 repeats every 200 units).
    #[test]
    fn background_zoom_scales_mirroring_period() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(100.0, 0.0);

        let mut bg = ParallaxBackground::new();
        bg.scroll_offset = Vector2::new(30.0, 0.0);
        let a = bg.layer_offset(&layer, 2.0);

        bg.scroll_offset = Vector2::new(30.0 + 200.0, 0.0);
        let b = bg.layer_offset(&layer, 2.0);
        assert!(
            (a.x - b.x).abs() < 1e-3,
            "at zoom 2 the layer must repeat every 200 units ({} vs {})",
            a.x,
            b.x,
        );
    }
}
