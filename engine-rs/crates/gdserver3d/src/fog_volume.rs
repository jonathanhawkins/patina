//! FogVolume node for volumetric fog regions in 3D scenes.
//!
//! Implements Godot's `FogVolume` node which defines a region of volumetric
//! fog in 3D space. Each volume has a shape, size, and material controlling
//! the fog appearance within its bounds.

use gdcore::math::{Color, Vector3};

// ---------------------------------------------------------------------------
// FogVolumeShape
// ---------------------------------------------------------------------------

/// Shape of the fog volume region.
///
/// Maps to Godot's `RenderingServer.FogVolumeShape` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FogVolumeShape {
    /// Ellipsoid fog region (default).
    #[default]
    Ellipsoid,
    /// Cone fog region.
    Cone,
    /// Cylinder fog region.
    Cylinder,
    /// Axis-aligned box fog region.
    Box,
    /// World-spanning fog (ignores size).
    World,
}

impl FogVolumeShape {
    /// Returns the integer value matching Godot's enum ordering.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Ellipsoid => 0,
            Self::Cone => 1,
            Self::Cylinder => 2,
            Self::Box => 3,
            Self::World => 4,
        }
    }

    /// Creates from Godot's integer enum value.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Ellipsoid,
            1 => Self::Cone,
            2 => Self::Cylinder,
            3 => Self::Box,
            4 => Self::World,
            _ => Self::Ellipsoid,
        }
    }
}

// ---------------------------------------------------------------------------
// FogMaterial
// ---------------------------------------------------------------------------

/// Material controlling the appearance of fog within a FogVolume.
///
/// Maps to Godot's `FogMaterial` resource.
#[derive(Debug, Clone, PartialEq)]
pub struct FogMaterial {
    /// Fog density within this volume. Higher = thicker fog.
    pub density: f32,
    /// Base color of the fog.
    pub albedo: Color,
    /// Emission color (self-illumination of the fog).
    pub emission: Color,
    /// Height falloff factor. Higher = fog fades faster with altitude.
    pub height_falloff: f32,
    /// Edge fade distance. Controls how sharply the fog boundary transitions.
    pub edge_fade: f32,
    /// Density texture (path). Empty means uniform density.
    pub density_texture: String,
}

impl Default for FogMaterial {
    fn default() -> Self {
        Self {
            density: 1.0,
            albedo: Color::WHITE,
            emission: Color::new(0.0, 0.0, 0.0, 1.0),
            height_falloff: 0.0,
            edge_fade: 0.1,
            density_texture: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// FogVolume
// ---------------------------------------------------------------------------

/// A 3D fog volume node defining a region of volumetric fog.
///
/// FogVolume works with the Environment's volumetric fog system to create
/// localized fog effects. The volume's transform determines its position
/// and orientation in the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct FogVolume {
    /// Shape of the fog region.
    pub shape: FogVolumeShape,
    /// Size of the volume in local space (width, height, depth).
    /// For World shape, this is ignored.
    pub size: Vector3,
    /// Material controlling fog appearance.
    pub material: FogMaterial,
}

impl Default for FogVolume {
    fn default() -> Self {
        Self {
            shape: FogVolumeShape::default(),
            size: Vector3::new(2.0, 2.0, 2.0),
            material: FogMaterial::default(),
        }
    }
}

impl FogVolume {
    /// Creates a new FogVolume with the given shape and size.
    pub fn new(shape: FogVolumeShape, size: Vector3) -> Self {
        Self {
            shape,
            size,
            material: FogMaterial::default(),
        }
    }

    /// Creates a world-spanning fog volume (size is ignored).
    pub fn world() -> Self {
        Self {
            shape: FogVolumeShape::World,
            size: Vector3::ZERO,
            material: FogMaterial::default(),
        }
    }

    /// Creates a box-shaped fog volume.
    pub fn box_shape(size: Vector3) -> Self {
        Self::new(FogVolumeShape::Box, size)
    }

    /// Creates an ellipsoid fog volume.
    pub fn ellipsoid(size: Vector3) -> Self {
        Self::new(FogVolumeShape::Ellipsoid, size)
    }

    /// Creates a cylinder fog volume.
    pub fn cylinder(radius: f32, height: f32) -> Self {
        Self::new(
            FogVolumeShape::Cylinder,
            Vector3::new(radius * 2.0, height, radius * 2.0),
        )
    }

    /// Creates a cone fog volume.
    pub fn cone(radius: f32, height: f32) -> Self {
        Self::new(
            FogVolumeShape::Cone,
            Vector3::new(radius * 2.0, height, radius * 2.0),
        )
    }

    /// Tests whether a local-space point is inside this fog volume.
    ///
    /// The point should be in the volume's local coordinate space
    /// (pre-transform). For World shape, always returns true.
    pub fn contains_point(&self, local_point: Vector3) -> bool {
        match self.shape {
            FogVolumeShape::World => true,
            FogVolumeShape::Box => {
                let half = self.size * 0.5;
                local_point.x.abs() <= half.x
                    && local_point.y.abs() <= half.y
                    && local_point.z.abs() <= half.z
            }
            FogVolumeShape::Ellipsoid => {
                let half = self.size * 0.5;
                if half.x < 1e-6 || half.y < 1e-6 || half.z < 1e-6 {
                    return false;
                }
                let nx = local_point.x / half.x;
                let ny = local_point.y / half.y;
                let nz = local_point.z / half.z;
                nx * nx + ny * ny + nz * nz <= 1.0
            }
            FogVolumeShape::Cylinder => {
                let half = self.size * 0.5;
                if half.x < 1e-6 || half.y < 1e-6 {
                    return false;
                }
                let radius = half.x; // x and z define the radius
                let nx = local_point.x / radius;
                let nz = local_point.z / radius;
                nx * nx + nz * nz <= 1.0 && local_point.y.abs() <= half.y
            }
            FogVolumeShape::Cone => {
                let half = self.size * 0.5;
                if half.y < 1e-6 || half.x < 1e-6 {
                    return false;
                }
                // Cone narrows from bottom (y = -half.y) to top (y = half.y)
                let t = (local_point.y + half.y) / self.size.y; // 0 at bottom, 1 at top
                if t < 0.0 || t > 1.0 {
                    return false;
                }
                let cone_radius = half.x * (1.0 - t);
                if cone_radius < 1e-6 {
                    return false;
                }
                let nx = local_point.x / cone_radius;
                let nz = local_point.z / cone_radius;
                nx * nx + nz * nz <= 1.0
            }
        }
    }

    /// Samples the fog density at a local-space point.
    ///
    /// Returns 0.0 if the point is outside the volume. Otherwise returns
    /// the material density, optionally modulated by height falloff and
    /// edge fade.
    pub fn sample_density(&self, local_point: Vector3) -> f32 {
        if !self.contains_point(local_point) {
            return 0.0;
        }

        let mut density = self.material.density;

        // Apply height falloff (exponential decay with altitude)
        if self.material.height_falloff > 0.0 {
            let half_y = self.size.y * 0.5;
            let normalized_height = if half_y > 1e-6 {
                ((local_point.y + half_y) / self.size.y).clamp(0.0, 1.0)
            } else {
                0.5
            };
            density *= (-self.material.height_falloff * normalized_height).exp();
        }

        // Apply edge fade
        if self.material.edge_fade > 0.0 && self.shape != FogVolumeShape::World {
            let edge_factor = self.edge_distance(local_point);
            let fade = (edge_factor / self.material.edge_fade).clamp(0.0, 1.0);
            density *= fade;
        }

        density
    }

    /// Returns the approximate distance from a local-space point to the
    /// nearest edge of the volume. Used for edge fade calculations.
    fn edge_distance(&self, local_point: Vector3) -> f32 {
        match self.shape {
            FogVolumeShape::World => f32::MAX,
            FogVolumeShape::Box => {
                let half = self.size * 0.5;
                let dx = half.x - local_point.x.abs();
                let dy = half.y - local_point.y.abs();
                let dz = half.z - local_point.z.abs();
                dx.min(dy).min(dz).max(0.0)
            }
            FogVolumeShape::Ellipsoid => {
                let half = self.size * 0.5;
                if half.x < 1e-6 || half.y < 1e-6 || half.z < 1e-6 {
                    return 0.0;
                }
                let nx = local_point.x / half.x;
                let ny = local_point.y / half.y;
                let nz = local_point.z / half.z;
                let r = (nx * nx + ny * ny + nz * nz).sqrt();
                if r < 1e-6 {
                    return half.x.min(half.y).min(half.z);
                }
                (1.0 - r) * half.x.min(half.y).min(half.z)
            }
            FogVolumeShape::Cylinder | FogVolumeShape::Cone => {
                let half = self.size * 0.5;
                let dy = half.y - local_point.y.abs();
                let radius = half.x;
                let dr =
                    radius - (local_point.x * local_point.x + local_point.z * local_point.z).sqrt();
                dy.min(dr).max(0.0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // -- FogVolumeShape ----------------------------------------------------

    #[test]
    fn shape_default_is_ellipsoid() {
        assert_eq!(FogVolumeShape::default(), FogVolumeShape::Ellipsoid);
    }

    #[test]
    fn shape_godot_int_roundtrip() {
        for shape in [
            FogVolumeShape::Ellipsoid,
            FogVolumeShape::Cone,
            FogVolumeShape::Cylinder,
            FogVolumeShape::Box,
            FogVolumeShape::World,
        ] {
            assert_eq!(FogVolumeShape::from_godot_int(shape.to_godot_int()), shape);
        }
    }

    #[test]
    fn shape_unknown_int_defaults_ellipsoid() {
        assert_eq!(
            FogVolumeShape::from_godot_int(99),
            FogVolumeShape::Ellipsoid
        );
    }

    // -- FogMaterial -------------------------------------------------------

    #[test]
    fn material_defaults() {
        let m = FogMaterial::default();
        assert!(approx(m.density, 1.0));
        assert_eq!(m.albedo, Color::WHITE);
        assert!(approx(m.height_falloff, 0.0));
        assert!(approx(m.edge_fade, 0.1));
        assert!(m.density_texture.is_empty());
    }

    // -- FogVolume constructors -------------------------------------------

    #[test]
    fn default_volume() {
        let v = FogVolume::default();
        assert_eq!(v.shape, FogVolumeShape::Ellipsoid);
        assert!(approx(v.size.x, 2.0));
        assert!(approx(v.size.y, 2.0));
        assert!(approx(v.size.z, 2.0));
    }

    #[test]
    fn box_constructor() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 6.0, 8.0));
        assert_eq!(v.shape, FogVolumeShape::Box);
        assert!(approx(v.size.x, 4.0));
        assert!(approx(v.size.y, 6.0));
    }

    #[test]
    fn ellipsoid_constructor() {
        let v = FogVolume::ellipsoid(Vector3::new(3.0, 3.0, 3.0));
        assert_eq!(v.shape, FogVolumeShape::Ellipsoid);
    }

    #[test]
    fn cylinder_constructor() {
        let v = FogVolume::cylinder(2.0, 5.0);
        assert_eq!(v.shape, FogVolumeShape::Cylinder);
        assert!(approx(v.size.x, 4.0)); // diameter = 2 * radius
        assert!(approx(v.size.y, 5.0));
    }

    #[test]
    fn cone_constructor() {
        let v = FogVolume::cone(3.0, 10.0);
        assert_eq!(v.shape, FogVolumeShape::Cone);
        assert!(approx(v.size.x, 6.0));
        assert!(approx(v.size.y, 10.0));
    }

    #[test]
    fn world_constructor() {
        let v = FogVolume::world();
        assert_eq!(v.shape, FogVolumeShape::World);
    }

    // -- contains_point: Box -----------------------------------------------

    #[test]
    fn box_contains_origin() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::ZERO));
    }

    #[test]
    fn box_contains_interior() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn box_excludes_outside() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(!v.contains_point(Vector3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn box_edge_is_inside() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::new(2.0, 0.0, 0.0)));
    }

    // -- contains_point: Ellipsoid -----------------------------------------

    #[test]
    fn ellipsoid_contains_origin() {
        let v = FogVolume::ellipsoid(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::ZERO));
    }

    #[test]
    fn ellipsoid_excludes_outside() {
        let v = FogVolume::ellipsoid(Vector3::new(2.0, 2.0, 2.0));
        assert!(!v.contains_point(Vector3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn ellipsoid_contains_near_surface() {
        let v = FogVolume::ellipsoid(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::new(1.9, 0.0, 0.0)));
    }

    // -- contains_point: World ---------------------------------------------

    #[test]
    fn world_contains_everything() {
        let v = FogVolume::world();
        assert!(v.contains_point(Vector3::ZERO));
        assert!(v.contains_point(Vector3::new(1e6, 1e6, 1e6)));
        assert!(v.contains_point(Vector3::new(-1e6, -1e6, -1e6)));
    }

    // -- contains_point: Cylinder ------------------------------------------

    #[test]
    fn cylinder_contains_center() {
        let v = FogVolume::cylinder(2.0, 4.0);
        assert!(v.contains_point(Vector3::ZERO));
    }

    #[test]
    fn cylinder_excludes_outside_radius() {
        let v = FogVolume::cylinder(2.0, 4.0);
        assert!(!v.contains_point(Vector3::new(2.5, 0.0, 0.0)));
    }

    #[test]
    fn cylinder_excludes_above_height() {
        let v = FogVolume::cylinder(2.0, 4.0);
        assert!(!v.contains_point(Vector3::new(0.0, 3.0, 0.0)));
    }

    // -- contains_point: Cone ----------------------------------------------

    #[test]
    fn cone_contains_bottom_center() {
        let v = FogVolume::cone(3.0, 10.0);
        // Bottom is at y = -half.y = -5
        assert!(v.contains_point(Vector3::new(0.0, -4.9, 0.0)));
    }

    #[test]
    fn cone_excludes_top_edge() {
        let v = FogVolume::cone(3.0, 10.0);
        // At top (y = +5), radius should be ~0
        assert!(!v.contains_point(Vector3::new(1.0, 4.9, 0.0)));
    }

    #[test]
    fn cone_contains_wide_bottom() {
        let v = FogVolume::cone(3.0, 10.0);
        // At bottom (y = -5), radius = 3.0
        assert!(v.contains_point(Vector3::new(2.5, -4.5, 0.0)));
    }

    // -- sample_density ----------------------------------------------------

    #[test]
    fn density_zero_outside() {
        let v = FogVolume::box_shape(Vector3::new(2.0, 2.0, 2.0));
        assert!(approx(v.sample_density(Vector3::new(5.0, 0.0, 0.0)), 0.0));
    }

    #[test]
    fn density_matches_material_inside() {
        let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        v.material.density = 0.5;
        v.material.height_falloff = 0.0;
        v.material.edge_fade = 0.0;
        let d = v.sample_density(Vector3::ZERO);
        assert!(approx(d, 0.5), "Expected 0.5, got {d}");
    }

    #[test]
    fn density_with_height_falloff() {
        let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        v.material.density = 1.0;
        v.material.height_falloff = 2.0;
        v.material.edge_fade = 0.0;
        let bottom = v.sample_density(Vector3::new(0.0, -1.9, 0.0));
        let top = v.sample_density(Vector3::new(0.0, 1.9, 0.0));
        assert!(
            bottom > top,
            "Fog should be denser at bottom: bottom={bottom}, top={top}"
        );
    }

    #[test]
    fn density_world_always_material_density() {
        let mut v = FogVolume::world();
        v.material.density = 0.3;
        v.material.height_falloff = 0.0;
        let d = v.sample_density(Vector3::new(100.0, 200.0, 300.0));
        assert!(approx(d, 0.3));
    }

    #[test]
    fn edge_fade_reduces_density_near_boundary() {
        let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        v.material.density = 1.0;
        v.material.height_falloff = 0.0;
        v.material.edge_fade = 0.5;
        let center = v.sample_density(Vector3::ZERO);
        let near_edge = v.sample_density(Vector3::new(1.9, 0.0, 0.0));
        assert!(
            near_edge < center,
            "Edge fade should reduce density near boundary: center={center}, edge={near_edge}"
        );
    }
}
