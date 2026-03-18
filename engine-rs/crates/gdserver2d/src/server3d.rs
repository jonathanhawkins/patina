//! 3D rendering server trait definitions and supporting types.
//!
//! Defines `RenderingServer3D`, the abstract interface for 3D rendering
//! backends, along with `Instance3D`, `Viewport3D`, and projection math.

use gdcore::math::Color;
use gdcore::math3d::Transform3D;

use crate::material::Material3D;
use crate::mesh::Mesh3D;

/// Unique identifier for a 3D render instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instance3DId(pub u64);

/// A renderable 3D instance in the scene.
#[derive(Debug, Clone)]
pub struct Instance3D {
    /// Unique identifier.
    pub id: Instance3DId,
    /// Mesh geometry (if assigned).
    pub mesh: Option<Mesh3D>,
    /// Surface material (if assigned).
    pub material: Option<Material3D>,
    /// World-space transform.
    pub transform: Transform3D,
    /// Whether the instance is visible.
    pub visible: bool,
}

impl Instance3D {
    /// Creates a new instance with default settings.
    pub fn new(id: Instance3DId) -> Self {
        Self {
            id,
            mesh: None,
            material: None,
            transform: Transform3D::IDENTITY,
            visible: true,
        }
    }
}

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
}

impl Viewport3D {
    /// Creates a new 3D viewport with sensible defaults.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            camera_transform: Transform3D::IDENTITY,
            fov: std::f32::consts::FRAC_PI_4, // 45 degrees
            near: 0.05,
            far: 4000.0,
        }
    }

    /// Returns the aspect ratio (width / height).
    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

/// Frame data produced by a 3D rendering pass.
#[derive(Debug, Clone)]
pub struct FrameData3D {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Raw pixel data as a flat array of RGBA colors.
    pub pixels: Vec<Color>,
    /// Depth buffer values (one per pixel), if available.
    pub depth: Vec<f32>,
}

/// Computes a perspective projection matrix.
///
/// Returns a column-major 4×4 matrix matching Godot's projection conventions.
pub fn perspective_projection_matrix(fov: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let half_fov_tan = (fov * 0.5).tan();
    let f = 1.0 / half_fov_tan;
    let range_inv = 1.0 / (near - far);

    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, (far + near) * range_inv, -1.0],
        [0.0, 0.0, 2.0 * far * near * range_inv, 0.0],
    ]
}

/// Abstract rendering server for 3D content.
///
/// Implementations manage 3D instances and produce rendered frames.
pub trait RenderingServer3D {
    /// Creates a new 3D instance and returns its unique ID.
    fn create_instance(&mut self) -> Instance3DId;

    /// Frees a 3D instance by ID.
    fn free_instance(&mut self, id: Instance3DId);

    /// Assigns a mesh to an instance.
    fn set_mesh(&mut self, id: Instance3DId, mesh: Mesh3D);

    /// Assigns a material to an instance.
    fn set_material(&mut self, id: Instance3DId, material: Material3D);

    /// Sets the world-space transform for an instance.
    fn set_transform(&mut self, id: Instance3DId, transform: Transform3D);

    /// Sets visibility for an instance.
    fn set_visible(&mut self, id: Instance3DId, visible: bool);

    /// Renders a frame for the given 3D viewport.
    fn render_frame_3d(&mut self, viewport: &Viewport3D) -> FrameData3D;
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;
    use gdcore::math3d::Basis;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn instance3d_creation() {
        let inst = Instance3D::new(Instance3DId(1));
        assert_eq!(inst.id, Instance3DId(1));
        assert!(inst.visible);
        assert!(inst.mesh.is_none());
        assert!(inst.material.is_none());
        assert_eq!(inst.transform, Transform3D::IDENTITY);
    }

    #[test]
    fn instance3d_id_equality() {
        let a = Instance3DId(42);
        let b = Instance3DId(42);
        let c = Instance3DId(99);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn viewport3d_creation() {
        let vp = Viewport3D::new(1920, 1080);
        assert_eq!(vp.width, 1920);
        assert_eq!(vp.height, 1080);
        assert!(approx_eq(vp.fov, std::f32::consts::FRAC_PI_4));
        assert!(approx_eq(vp.near, 0.05));
        assert!(approx_eq(vp.far, 4000.0));
    }

    #[test]
    fn viewport3d_aspect_ratio() {
        let vp = Viewport3D::new(1920, 1080);
        assert!(approx_eq(vp.aspect(), 1920.0 / 1080.0));
    }

    #[test]
    fn viewport3d_square_aspect() {
        let vp = Viewport3D::new(512, 512);
        assert!(approx_eq(vp.aspect(), 1.0));
    }

    #[test]
    fn perspective_matrix_identity_properties() {
        let fov = std::f32::consts::FRAC_PI_2; // 90 degrees
        let m = perspective_projection_matrix(fov, 1.0, 0.1, 100.0);
        // With 90° fov and aspect 1.0, f = 1/tan(45°) = 1.0
        assert!(approx_eq(m[0][0], 1.0));
        assert!(approx_eq(m[1][1], 1.0));
        // w component row
        assert!(approx_eq(m[0][3], 0.0));
        assert!(approx_eq(m[1][3], 0.0));
        assert!(approx_eq(m[2][3], -1.0));
        assert!(approx_eq(m[3][3], 0.0));
    }

    #[test]
    fn perspective_matrix_aspect_scaling() {
        let m = perspective_projection_matrix(std::f32::consts::FRAC_PI_2, 2.0, 0.1, 100.0);
        // m[0][0] should be f/aspect = 1.0/2.0 = 0.5
        assert!(approx_eq(m[0][0], 0.5));
        assert!(approx_eq(m[1][1], 1.0));
    }

    #[test]
    fn instance3d_with_mesh_and_material() {
        let mut inst = Instance3D::new(Instance3DId(5));
        inst.mesh = Some(crate::mesh::Mesh3D::cube(1.0));
        inst.material = Some(Material3D::default());
        assert!(inst.mesh.is_some());
        assert!(inst.material.is_some());
    }

    #[test]
    fn instance3d_set_transform() {
        let mut inst = Instance3D::new(Instance3DId(10));
        let t = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(5.0, 10.0, 15.0),
        };
        inst.transform = t;
        assert_eq!(inst.transform.origin, Vector3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn instance3d_visibility_toggle() {
        let mut inst = Instance3D::new(Instance3DId(1));
        assert!(inst.visible);
        inst.visible = false;
        assert!(!inst.visible);
    }

    #[test]
    fn frame_data_3d_creation() {
        let frame = FrameData3D {
            width: 100,
            height: 100,
            pixels: vec![Color::BLACK; 10000],
            depth: vec![1.0; 10000],
        };
        assert_eq!(frame.pixels.len(), 10000);
        assert_eq!(frame.depth.len(), 10000);
    }
}
