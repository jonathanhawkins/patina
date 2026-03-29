//! Global Illumination stub nodes: VoxelGI and LightmapGI.
//!
//! These are stub types for Godot's GI nodes. Full voxel-cone-tracing and
//! lightmap baking are out of scope for the initial port; these types carry
//! the configuration surface so that scenes using VoxelGI or LightmapGI can
//! be loaded, saved, and round-tripped without data loss.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;

// ===========================================================================
// VoxelGI
// ===========================================================================

/// Unique identifier for a VoxelGI instance in the rendering server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoxelGIId(pub u64);

/// Quality preset for VoxelGI.
///
/// Maps to Godot's `VoxelGI.Subdiv` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoxelGISubdiv {
    /// 64×64×64 voxel grid.
    Subdiv64 = 0,
    /// 128×128×128 voxel grid.
    #[default]
    Subdiv128 = 1,
    /// 256×256×256 voxel grid.
    Subdiv256 = 2,
    /// 512×512×512 voxel grid.
    Subdiv512 = 3,
}

/// Stub for Godot's VoxelGI node.
///
/// VoxelGI provides real-time global illumination via voxel cone tracing.
/// This stub stores the configuration but does not perform actual baking.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelGI {
    /// Unique identifier.
    pub id: VoxelGIId,
    /// World-space transform.
    pub transform: Transform3D,
    /// Half-extents of the voxel grid volume.
    pub size: Vector3,
    /// Voxel grid resolution.
    pub subdiv: VoxelGISubdiv,
    /// Camera attributes path (if any).
    pub camera_attributes_path: Option<String>,
    /// Energy multiplier for indirect light.
    pub energy: f32,
    /// Bias to reduce light leaking through thin surfaces.
    pub bias: f32,
    /// Normal bias to reduce self-illumination artifacts.
    pub normal_bias: f32,
    /// Propagation factor controlling how far light bounces spread.
    pub propagation: f32,
    /// Whether interior mode is enabled (no sky contribution).
    pub interior: bool,
    /// Whether the VoxelGI probe data has been baked.
    pub baked: bool,
}

impl VoxelGI {
    /// Creates a new VoxelGI with Godot-compatible defaults.
    pub fn new(id: VoxelGIId) -> Self {
        Self {
            id,
            transform: Transform3D::IDENTITY,
            size: Vector3::new(20.0, 20.0, 20.0),
            subdiv: VoxelGISubdiv::default(),
            camera_attributes_path: None,
            energy: 1.0,
            bias: 1.5,
            normal_bias: 0.0,
            propagation: 0.7,
            interior: false,
            baked: false,
        }
    }

    /// Returns the axis-aligned bounding box of the VoxelGI volume in world space.
    pub fn world_aabb_min_max(&self) -> (Vector3, Vector3) {
        let center = self.transform.origin;
        let half = self.size * 0.5;
        (center - half, center + half)
    }

    /// Returns `true` if the given world-space point is inside the GI volume.
    pub fn contains_point(&self, point: Vector3) -> bool {
        let (min, max) = self.world_aabb_min_max();
        point.x >= min.x
            && point.x <= max.x
            && point.y >= min.y
            && point.y <= max.y
            && point.z >= min.z
            && point.z <= max.z
    }

    /// Returns the number of voxels per axis for the current subdivision.
    pub fn grid_resolution(&self) -> u32 {
        match self.subdiv {
            VoxelGISubdiv::Subdiv64 => 64,
            VoxelGISubdiv::Subdiv128 => 128,
            VoxelGISubdiv::Subdiv256 => 256,
            VoxelGISubdiv::Subdiv512 => 512,
        }
    }
}

// ===========================================================================
// LightmapGI
// ===========================================================================

/// Unique identifier for a LightmapGI instance in the rendering server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightmapGIId(pub u64);

/// Bake quality preset for LightmapGI.
///
/// Maps to Godot's `LightmapGI.BakeQuality` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightmapBakeQuality {
    /// Fast bake with lower sample count.
    Low = 0,
    /// Balanced quality and speed.
    #[default]
    Medium = 1,
    /// High quality with more samples.
    High = 2,
    /// Maximum quality, very slow.
    Ultra = 3,
}

/// Lightmap generation mode for LightmapGI.
///
/// Maps to Godot's `LightmapGI.GenerateProbes` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightmapProbeGeneration {
    /// Do not generate light probes automatically.
    #[default]
    Disabled = 0,
    /// Generate probes at low density.
    Low = 1,
    /// Generate probes at medium density.
    Medium = 2,
    /// Generate probes at high density.
    High = 3,
}

/// Stub for Godot's LightmapGI node.
///
/// LightmapGI provides baked global illumination using lightmaps.
/// This stub stores the configuration but does not perform actual baking.
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapGI {
    /// Unique identifier.
    pub id: LightmapGIId,
    /// World-space transform.
    pub transform: Transform3D,
    /// Bake quality preset.
    pub quality: LightmapBakeQuality,
    /// Number of light bounces during baking.
    pub bounces: u32,
    /// Lightmap texel density (texels per unit).
    pub texel_scale: f32,
    /// Whether to use denoiser on the baked result.
    pub use_denoiser: bool,
    /// Denoiser strength (0.0 = none, 1.0 = full).
    pub denoiser_strength: f32,
    /// Whether directional lightmaps are generated.
    pub directional: bool,
    /// Whether interior mode is enabled (no sky contribution).
    pub interior: bool,
    /// Energy multiplier for the baked indirect light.
    pub energy: f32,
    /// Bias to reduce shadow acne in baked lightmaps.
    pub bias: f32,
    /// Maximum texture size for the lightmap atlas.
    pub max_texture_size: u32,
    /// Automatic probe generation mode.
    pub generate_probes: LightmapProbeGeneration,
    /// Camera attributes path (if any).
    pub camera_attributes_path: Option<String>,
    /// Whether lightmap data has been baked.
    pub baked: bool,
    /// Path to baked lightmap data resource (if baked).
    pub light_data_path: Option<String>,
}

impl LightmapGI {
    /// Creates a new LightmapGI with Godot-compatible defaults.
    pub fn new(id: LightmapGIId) -> Self {
        Self {
            id,
            transform: Transform3D::IDENTITY,
            quality: LightmapBakeQuality::default(),
            bounces: 3,
            texel_scale: 1.0,
            use_denoiser: true,
            denoiser_strength: 0.1,
            directional: false,
            interior: false,
            energy: 1.0,
            bias: 0.0005,
            max_texture_size: 16384,
            generate_probes: LightmapProbeGeneration::default(),
            camera_attributes_path: None,
            baked: false,
            light_data_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- VoxelGI tests --------------------------------------------------------

    #[test]
    fn voxelgi_defaults_match_godot() {
        let gi = VoxelGI::new(VoxelGIId(1));
        assert_eq!(gi.size, Vector3::new(20.0, 20.0, 20.0));
        assert_eq!(gi.subdiv, VoxelGISubdiv::Subdiv128);
        assert!((gi.energy - 1.0).abs() < f32::EPSILON);
        assert!((gi.bias - 1.5).abs() < f32::EPSILON);
        assert!((gi.normal_bias - 0.0).abs() < f32::EPSILON);
        assert!((gi.propagation - 0.7).abs() < f32::EPSILON);
        assert!(!gi.interior);
        assert!(!gi.baked);
        assert!(gi.camera_attributes_path.is_none());
    }

    #[test]
    fn voxelgi_id_equality() {
        assert_eq!(VoxelGIId(10), VoxelGIId(10));
        assert_ne!(VoxelGIId(10), VoxelGIId(20));
    }

    #[test]
    fn voxelgi_contains_point_at_center() {
        let gi = VoxelGI::new(VoxelGIId(1));
        assert!(gi.contains_point(Vector3::ZERO));
    }

    #[test]
    fn voxelgi_contains_point_at_edge() {
        let gi = VoxelGI::new(VoxelGIId(1));
        assert!(gi.contains_point(Vector3::new(10.0, 10.0, 10.0)));
        assert!(!gi.contains_point(Vector3::new(10.1, 0.0, 0.0)));
    }

    #[test]
    fn voxelgi_contains_point_with_transform() {
        let mut gi = VoxelGI::new(VoxelGIId(1));
        gi.transform.origin = Vector3::new(100.0, 0.0, 0.0);
        let (min, max) = gi.world_aabb_min_max();
        assert!((min.x - 90.0).abs() < f32::EPSILON);
        assert!((max.x - 110.0).abs() < f32::EPSILON);
        assert!(gi.contains_point(Vector3::new(100.0, 0.0, 0.0)));
        assert!(!gi.contains_point(Vector3::ZERO));
    }

    #[test]
    fn voxelgi_grid_resolution() {
        let mut gi = VoxelGI::new(VoxelGIId(1));
        assert_eq!(gi.grid_resolution(), 128);

        gi.subdiv = VoxelGISubdiv::Subdiv64;
        assert_eq!(gi.grid_resolution(), 64);

        gi.subdiv = VoxelGISubdiv::Subdiv256;
        assert_eq!(gi.grid_resolution(), 256);

        gi.subdiv = VoxelGISubdiv::Subdiv512;
        assert_eq!(gi.grid_resolution(), 512);
    }

    #[test]
    fn voxelgi_subdiv_values() {
        assert_eq!(VoxelGISubdiv::Subdiv64 as u32, 0);
        assert_eq!(VoxelGISubdiv::Subdiv128 as u32, 1);
        assert_eq!(VoxelGISubdiv::Subdiv256 as u32, 2);
        assert_eq!(VoxelGISubdiv::Subdiv512 as u32, 3);
    }

    // -- LightmapGI tests ----------------------------------------------------

    #[test]
    fn lightmapgi_defaults_match_godot() {
        let gi = LightmapGI::new(LightmapGIId(1));
        assert_eq!(gi.quality, LightmapBakeQuality::Medium);
        assert_eq!(gi.bounces, 3);
        assert!((gi.texel_scale - 1.0).abs() < f32::EPSILON);
        assert!(gi.use_denoiser);
        assert!((gi.denoiser_strength - 0.1).abs() < f32::EPSILON);
        assert!(!gi.directional);
        assert!(!gi.interior);
        assert!((gi.energy - 1.0).abs() < f32::EPSILON);
        assert!((gi.bias - 0.0005).abs() < f32::EPSILON);
        assert_eq!(gi.max_texture_size, 16384);
        assert_eq!(gi.generate_probes, LightmapProbeGeneration::Disabled);
        assert!(gi.camera_attributes_path.is_none());
        assert!(!gi.baked);
        assert!(gi.light_data_path.is_none());
    }

    #[test]
    fn lightmapgi_id_equality() {
        assert_eq!(LightmapGIId(10), LightmapGIId(10));
        assert_ne!(LightmapGIId(10), LightmapGIId(20));
    }

    #[test]
    fn lightmapgi_bake_quality_values() {
        assert_eq!(LightmapBakeQuality::Low as u32, 0);
        assert_eq!(LightmapBakeQuality::Medium as u32, 1);
        assert_eq!(LightmapBakeQuality::High as u32, 2);
        assert_eq!(LightmapBakeQuality::Ultra as u32, 3);
    }

    #[test]
    fn lightmapgi_probe_generation_values() {
        assert_eq!(LightmapProbeGeneration::Disabled as u32, 0);
        assert_eq!(LightmapProbeGeneration::Low as u32, 1);
        assert_eq!(LightmapProbeGeneration::Medium as u32, 2);
        assert_eq!(LightmapProbeGeneration::High as u32, 3);
    }

    #[test]
    fn lightmapgi_set_light_data() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        assert!(gi.light_data_path.is_none());

        gi.light_data_path = Some("res://lightmap_data.lmbake".to_string());
        gi.baked = true;
        assert!(gi.baked);
        assert_eq!(
            gi.light_data_path.as_deref(),
            Some("res://lightmap_data.lmbake")
        );
    }

    #[test]
    fn lightmapgi_interior_disables_sky() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        assert!(!gi.interior);
        gi.interior = true;
        assert!(gi.interior);
    }
}
