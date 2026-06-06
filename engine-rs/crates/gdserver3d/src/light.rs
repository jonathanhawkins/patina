//! 3D light types for the rendering server.

use gdcore::math::Color;
use gdcore::math::Vector3;

/// Unique identifier for a 3D light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Light3DId(pub u64);

/// The type of 3D light source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    /// Infinite directional light (like the sun).
    Directional,
    /// Point light that emits in all directions from a position.
    Point,
    /// Spot light that emits in a cone from a position.
    Spot,
}

/// Godot-compatible shadow mode for OmniLight3D.
///
/// Determines how the shadow map is constructed for point lights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmniShadowMode {
    /// Render shadows using a dual-paraboloid map (Godot default).
    DualParaboloid,
    /// Render shadows using a 6-face cubemap (higher quality).
    Cube,
}

impl Default for OmniShadowMode {
    fn default() -> Self {
        Self::DualParaboloid
    }
}

/// Index of a cubemap face (+X, -X, +Y, -Y, +Z, -Z).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CubeFace {
    PositiveX = 0,
    NegativeX = 1,
    PositiveY = 2,
    NegativeY = 3,
    PositiveZ = 4,
    NegativeZ = 5,
}

impl CubeFace {
    /// All six faces in order.
    pub const ALL: [CubeFace; 6] = [
        Self::PositiveX,
        Self::NegativeX,
        Self::PositiveY,
        Self::NegativeY,
        Self::PositiveZ,
        Self::NegativeZ,
    ];

    /// Returns the forward direction vector for this cube face.
    pub fn forward(&self) -> Vector3 {
        match self {
            Self::PositiveX => Vector3::new(1.0, 0.0, 0.0),
            Self::NegativeX => Vector3::new(-1.0, 0.0, 0.0),
            Self::PositiveY => Vector3::new(0.0, 1.0, 0.0),
            Self::NegativeY => Vector3::new(0.0, -1.0, 0.0),
            Self::PositiveZ => Vector3::new(0.0, 0.0, 1.0),
            Self::NegativeZ => Vector3::new(0.0, 0.0, -1.0),
        }
    }

    /// Returns the up vector for this cube face.
    pub fn up(&self) -> Vector3 {
        match self {
            Self::PositiveX | Self::NegativeX => Vector3::new(0.0, -1.0, 0.0),
            Self::PositiveY => Vector3::new(0.0, 0.0, 1.0),
            Self::NegativeY => Vector3::new(0.0, 0.0, -1.0),
            Self::PositiveZ | Self::NegativeZ => Vector3::new(0.0, -1.0, 0.0),
        }
    }
}

/// A cubemap shadow map for an omnidirectional point light.
///
/// Stores depth values for 6 faces, each `resolution x resolution` pixels.
/// Depth values represent the linear distance from the light source.
#[derive(Debug, Clone)]
pub struct ShadowCubemap {
    /// Resolution of each face (width = height = resolution).
    pub resolution: u32,
    /// Depth data for all 6 faces. Each face is `resolution * resolution` f32 values.
    /// Face order matches [`CubeFace`] enum order: +X, -X, +Y, -Y, +Z, -Z.
    pub faces: [Vec<f32>; 6],
}

impl ShadowCubemap {
    /// Creates a new shadow cubemap with all faces cleared to max depth.
    pub fn new(resolution: u32) -> Self {
        let face_size = (resolution * resolution) as usize;
        Self {
            resolution,
            faces: std::array::from_fn(|_| vec![f32::MAX; face_size]),
        }
    }

    /// Clears all faces to max depth.
    pub fn clear(&mut self) {
        for face in &mut self.faces {
            face.fill(f32::MAX);
        }
    }

    /// Returns the depth at the given face, pixel coordinate.
    pub fn get_depth(&self, face: CubeFace, x: u32, y: u32) -> f32 {
        if x >= self.resolution || y >= self.resolution {
            return f32::MAX;
        }
        self.faces[face as usize][(y * self.resolution + x) as usize]
    }

    /// Sets the depth at the given face, pixel coordinate if closer than existing.
    /// Returns `true` if the depth was written (closer than existing).
    pub fn test_and_set(&mut self, face: CubeFace, x: u32, y: u32, depth: f32) -> bool {
        if x >= self.resolution || y >= self.resolution {
            return false;
        }
        let idx = (y * self.resolution + x) as usize;
        let slot = &mut self.faces[face as usize][idx];
        if depth < *slot {
            *slot = depth;
            true
        } else {
            false
        }
    }

    /// Samples the cubemap given a direction vector from light to fragment.
    ///
    /// Returns the stored depth for the texel that the direction maps to.
    pub fn sample(&self, direction: Vector3) -> f32 {
        let (face, u, v) = Self::direction_to_face_uv(direction);
        let x = ((u * self.resolution as f32) as u32).min(self.resolution.saturating_sub(1));
        let y = ((v * self.resolution as f32) as u32).min(self.resolution.saturating_sub(1));
        self.get_depth(face, x, y)
    }

    /// Converts a direction vector to (face, u, v) coordinates.
    fn direction_to_face_uv(dir: Vector3) -> (CubeFace, f32, f32) {
        let ax = dir.x.abs();
        let ay = dir.y.abs();
        let az = dir.z.abs();

        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if dir.x > 0.0 {
                (CubeFace::PositiveX, -dir.z, -dir.y, ax)
            } else {
                (CubeFace::NegativeX, dir.z, -dir.y, ax)
            }
        } else if ay >= ax && ay >= az {
            if dir.y > 0.0 {
                (CubeFace::PositiveY, dir.x, dir.z, ay)
            } else {
                (CubeFace::NegativeY, dir.x, -dir.z, ay)
            }
        } else if dir.z > 0.0 {
            (CubeFace::PositiveZ, dir.x, -dir.y, az)
        } else {
            (CubeFace::NegativeZ, -dir.x, -dir.y, az)
        };

        // Avoid division by zero for zero-length directions.
        if ma < f32::EPSILON {
            return (face, 0.5, 0.5);
        }

        let u = (sc / ma * 0.5 + 0.5).clamp(0.0, 1.0);
        let v = (tc / ma * 0.5 + 0.5).clamp(0.0, 1.0);

        (face, u, v)
    }
}

/// A 3D light source.
#[derive(Debug, Clone, PartialEq)]
pub struct Light3D {
    /// Unique identifier.
    pub id: Light3DId,
    /// Type of light.
    pub light_type: LightType,
    /// Light color.
    pub color: Color,
    /// Light energy/intensity multiplier.
    pub energy: f32,
    /// Position in world space (ignored for directional lights).
    pub position: Vector3,
    /// Direction the light points (relevant for directional and spot lights).
    pub direction: Vector3,
    /// Spot angle in radians (only for spot lights, half-angle of the cone).
    pub spot_angle: f32,
    /// Maximum range (for point and spot lights, 0 = infinite).
    pub range: f32,
    /// Distance attenuation curve exponent (1.0 = linear, 2.0 = quadratic).
    pub attenuation: f32,
    /// Spot cone angle attenuation curve exponent.
    pub spot_angle_attenuation: f32,
    /// Whether the light casts shadows.
    pub shadow_enabled: bool,
    /// Shadow mode for OmniLight3D (ignored for other light types).
    pub omni_shadow_mode: OmniShadowMode,
}

impl Light3D {
    /// Creates a new directional light pointing downward.
    pub fn directional(id: Light3DId) -> Self {
        Self {
            id,
            light_type: LightType::Directional,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            position: Vector3::ZERO,
            direction: Vector3::new(0.0, -1.0, 0.0),
            spot_angle: 0.0,
            range: 0.0,
            attenuation: 1.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
            omni_shadow_mode: OmniShadowMode::default(),
        }
    }

    /// Creates a new point light at the given position.
    pub fn point(id: Light3DId, position: Vector3) -> Self {
        Self {
            id,
            light_type: LightType::Point,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            position,
            direction: Vector3::ZERO,
            spot_angle: 0.0,
            range: 10.0,
            attenuation: 1.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
            omni_shadow_mode: OmniShadowMode::default(),
        }
    }

    /// Creates a new spot light at the given position and direction.
    pub fn spot(id: Light3DId, position: Vector3, direction: Vector3) -> Self {
        Self {
            id,
            light_type: LightType::Spot,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            position,
            direction,
            spot_angle: std::f32::consts::FRAC_PI_4,
            range: 10.0,
            attenuation: 1.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
            omni_shadow_mode: OmniShadowMode::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_light_defaults() {
        let light = Light3D::directional(Light3DId(1));
        assert_eq!(light.light_type, LightType::Directional);
        assert!((light.energy - 1.0).abs() < f32::EPSILON);
        assert!(!light.shadow_enabled);
    }

    #[test]
    fn point_light_at_position() {
        let light = Light3D::point(Light3DId(2), Vector3::new(5.0, 10.0, 0.0));
        assert_eq!(light.light_type, LightType::Point);
        assert_eq!(light.position, Vector3::new(5.0, 10.0, 0.0));
        assert!((light.range - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spot_light_with_direction() {
        let light = Light3D::spot(
            Light3DId(3),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        assert_eq!(light.light_type, LightType::Spot);
        assert!((light.spot_angle - std::f32::consts::FRAC_PI_4).abs() < f32::EPSILON);
        assert!((light.attenuation - 1.0).abs() < f32::EPSILON);
        assert!((light.spot_angle_attenuation - 1.0).abs() < f32::EPSILON);
        assert!((light.range - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spot_light_custom_attenuation() {
        let mut light = Light3D::spot(
            Light3DId(4),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.attenuation = 2.0;
        light.spot_angle_attenuation = 0.5;
        light.spot_angle = std::f32::consts::FRAC_PI_6; // 30 degrees
        light.range = 20.0;
        light.shadow_enabled = true;

        assert!((light.attenuation - 2.0).abs() < f32::EPSILON);
        assert!((light.spot_angle_attenuation - 0.5).abs() < f32::EPSILON);
        assert!((light.spot_angle - std::f32::consts::FRAC_PI_6).abs() < f32::EPSILON);
        assert!((light.range - 20.0).abs() < f32::EPSILON);
        assert!(light.shadow_enabled);
    }

    #[test]
    fn point_light_default_attenuation() {
        let light = Light3D::point(Light3DId(5), Vector3::ZERO);
        assert!((light.attenuation - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn light_id_equality() {
        assert_eq!(Light3DId(10), Light3DId(10));
        assert_ne!(Light3DId(10), Light3DId(20));
    }

    #[test]
    fn omni_shadow_mode_default_is_dual_paraboloid() {
        assert_eq!(OmniShadowMode::default(), OmniShadowMode::DualParaboloid);
    }

    #[test]
    fn point_light_default_omni_shadow_mode() {
        let light = Light3D::point(Light3DId(10), Vector3::ZERO);
        assert_eq!(light.omni_shadow_mode, OmniShadowMode::DualParaboloid);
    }

    // ── CubeFace tests ──

    #[test]
    fn cube_face_all_has_six_faces() {
        assert_eq!(CubeFace::ALL.len(), 6);
    }

    #[test]
    fn cube_face_forward_vectors_are_unit_length() {
        for face in CubeFace::ALL {
            let fwd = face.forward();
            let len = (fwd.x * fwd.x + fwd.y * fwd.y + fwd.z * fwd.z).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-5,
                "face {:?} forward not unit: {}",
                face,
                len
            );
        }
    }

    #[test]
    fn cube_face_up_vectors_are_unit_length() {
        for face in CubeFace::ALL {
            let up = face.up();
            let len = (up.x * up.x + up.y * up.y + up.z * up.z).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-5,
                "face {:?} up not unit: {}",
                face,
                len
            );
        }
    }

    #[test]
    fn cube_face_forward_and_up_are_orthogonal() {
        for face in CubeFace::ALL {
            let dot = face.forward().dot(face.up());
            assert!(dot.abs() < 1e-5, "face {:?} forward·up = {}", face, dot);
        }
    }

    // ── ShadowCubemap tests ──

    #[test]
    fn shadow_cubemap_new_initialized_to_max() {
        let cm = ShadowCubemap::new(4);
        assert_eq!(cm.resolution, 4);
        for face in CubeFace::ALL {
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(cm.get_depth(face, x, y), f32::MAX);
                }
            }
        }
    }

    #[test]
    fn shadow_cubemap_test_and_set() {
        let mut cm = ShadowCubemap::new(4);
        assert!(cm.test_and_set(CubeFace::PositiveX, 1, 2, 5.0));
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 1, 2), 5.0);
        // Closer depth wins.
        assert!(cm.test_and_set(CubeFace::PositiveX, 1, 2, 3.0));
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 1, 2), 3.0);
        // Farther depth is rejected.
        assert!(!cm.test_and_set(CubeFace::PositiveX, 1, 2, 4.0));
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 1, 2), 3.0);
    }

    #[test]
    fn shadow_cubemap_out_of_bounds() {
        let mut cm = ShadowCubemap::new(4);
        assert_eq!(cm.get_depth(CubeFace::PositiveX, 10, 10), f32::MAX);
        assert!(!cm.test_and_set(CubeFace::PositiveX, 10, 10, 1.0));
    }

    #[test]
    fn shadow_cubemap_clear() {
        let mut cm = ShadowCubemap::new(4);
        cm.test_and_set(CubeFace::PositiveZ, 0, 0, 1.0);
        cm.clear();
        assert_eq!(cm.get_depth(CubeFace::PositiveZ, 0, 0), f32::MAX);
    }

    #[test]
    fn shadow_cubemap_sample_axis_aligned() {
        let mut cm = ShadowCubemap::new(8);
        // Write a depth to the center of the +X face.
        cm.test_and_set(CubeFace::PositiveX, 4, 4, 7.5);
        // Sampling in the +X direction should hit that face.
        let depth = cm.sample(Vector3::new(1.0, 0.0, 0.0));
        assert!(
            depth < f32::MAX,
            "expected written depth on +X face, got MAX"
        );
    }

    #[test]
    fn shadow_cubemap_sample_negative_z() {
        let mut cm = ShadowCubemap::new(8);
        // Fill the entire -Z face with depth 3.0.
        for y in 0..8 {
            for x in 0..8 {
                cm.test_and_set(CubeFace::NegativeZ, x, y, 3.0);
            }
        }
        let depth = cm.sample(Vector3::new(0.0, 0.0, -1.0));
        assert!((depth - 3.0).abs() < 1e-5);
    }

    #[test]
    fn shadow_cubemap_faces_are_independent() {
        let mut cm = ShadowCubemap::new(4);
        cm.test_and_set(CubeFace::PositiveX, 0, 0, 2.0);
        // Other faces should remain at MAX.
        assert_eq!(cm.get_depth(CubeFace::NegativeX, 0, 0), f32::MAX);
        assert_eq!(cm.get_depth(CubeFace::PositiveY, 0, 0), f32::MAX);
    }

    #[test]
    fn shadow_cubemap_zero_direction_does_not_panic() {
        let cm = ShadowCubemap::new(4);
        // Zero vector should not panic, just return some depth.
        let _depth = cm.sample(Vector3::ZERO);
    }
}
