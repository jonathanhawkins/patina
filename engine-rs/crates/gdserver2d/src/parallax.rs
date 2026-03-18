//! Parallax layer for scrolling background effects.
//!
//! A [`ParallaxLayer`] offsets its contents relative to camera movement,
//! scaled by [`motion_scale`](ParallaxLayer::motion_scale), creating a
//! depth-based scrolling effect.

use gdcore::math::Vector2;

/// A parallax scrolling layer.
///
/// When the camera moves by `delta`, this layer's effective offset is
/// `delta * motion_scale + motion_offset`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallaxLayer {
    /// Scale factor applied to camera movement. `(1, 1)` moves with the camera;
    /// `(0.5, 0.5)` moves at half speed (distant background).
    pub motion_scale: Vector2,
    /// Static offset applied in addition to the scaled camera movement.
    pub motion_offset: Vector2,
}

impl ParallaxLayer {
    /// Creates a new parallax layer with the given motion scale.
    pub fn new(motion_scale: Vector2) -> Self {
        Self {
            motion_scale,
            motion_offset: Vector2::ZERO,
        }
    }

    /// Computes the effective offset for this layer given a camera position.
    pub fn compute_offset(&self, camera_position: Vector2) -> Vector2 {
        Vector2::new(
            camera_position.x * self.motion_scale.x + self.motion_offset.x,
            camera_position.y * self.motion_scale.y + self.motion_offset.y,
        )
    }
}

impl Default for ParallaxLayer {
    fn default() -> Self {
        Self {
            motion_scale: Vector2::ONE,
            motion_offset: Vector2::ZERO,
        }
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
}
