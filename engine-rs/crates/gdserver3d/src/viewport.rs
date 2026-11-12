//! 3D viewport with camera parameters.

use gdcore::math3d::Transform3D;

use crate::environment::Environment3D;

/// A 3D viewport with camera parameters.
#[derive(Debug, Clone)]
pub struct Viewport3D {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Camera world-space transform.
    pub camera_transform: Transform3D,
    /// Vertical field of view in radians.
    pub fov: f32,
    /// Near clipping plane distance.
    pub near: f32,
    /// Far clipping plane distance.
    pub far: f32,
    /// Optional environment controlling background, fog, and ambient lighting.
    pub environment: Option<Environment3D>,
}

impl Viewport3D {
    /// Creates a new 3D viewport with sensible defaults.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            camera_transform: Transform3D::IDENTITY,
            fov: std::f32::consts::FRAC_PI_4,
            near: 0.05,
            far: 4000.0,
            environment: None,
        }
    }

    /// Returns the aspect ratio (width / height).
    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_defaults() {
        let vp = Viewport3D::new(1920, 1080);
        assert_eq!(vp.width, 1920);
        assert_eq!(vp.height, 1080);
        assert!((vp.fov - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    }

    #[test]
    fn viewport_aspect() {
        let vp = Viewport3D::new(1920, 1080);
        assert!((vp.aspect() - 1920.0 / 1080.0).abs() < 1e-5);
    }

    #[test]
    fn viewport_square() {
        let vp = Viewport3D::new(512, 512);
        assert!((vp.aspect() - 1.0).abs() < 1e-5);
    }
}
