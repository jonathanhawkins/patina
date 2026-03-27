//! ReflectionProbe type for local cubemap reflections.
//!
//! A ReflectionProbe captures a cubemap of its surroundings, providing
//! local reflections for nearby meshes. Mirrors Godot's ReflectionProbe node.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;

/// Unique identifier for a reflection probe in the rendering server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReflectionProbeId(pub u64);

/// How the reflection probe updates its cubemap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReflectionProbeUpdateMode {
    /// Update the cubemap once on creation.
    #[default]
    Once = 0,
    /// Update the cubemap every frame.
    Always = 1,
}

/// How the reflection probe contributes ambient light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReflectionProbeAmbientMode {
    /// Disabled — no ambient contribution.
    Disabled = 0,
    /// Use the environment's ambient light.
    #[default]
    Environment = 1,
    /// Use a constant color for ambient light.
    ConstantColor = 2,
}

/// A reflection probe that captures local cubemap reflections.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionProbe {
    /// Unique identifier.
    pub id: ReflectionProbeId,
    /// World-space transform.
    pub transform: Transform3D,
    /// Half-extents of the probe's influence box (Godot `size`).
    pub size: Vector3,
    /// Offset of the probe's capture origin relative to its position.
    pub origin_offset: Vector3,
    /// Whether box projection is enabled for parallax-corrected reflections.
    pub box_projection: bool,
    /// Whether the probe is interior-only (no sky contribution).
    pub interior: bool,
    /// Whether the probe should capture shadows in the cubemap.
    pub enable_shadows: bool,
    /// Maximum distance for objects included in the cubemap capture.
    pub max_distance: f32,
    /// Reflection intensity multiplier.
    pub intensity: f32,
    /// How the cubemap updates.
    pub update_mode: ReflectionProbeUpdateMode,
    /// How the probe contributes ambient light.
    pub ambient_mode: ReflectionProbeAmbientMode,
    /// Ambient color (used when `ambient_mode` is `ConstantColor`).
    pub ambient_color: Color,
    /// Ambient color energy multiplier.
    pub ambient_color_energy: f32,
    /// Cull mask controlling which visual layers are captured.
    pub cull_mask: u32,
    /// LOD threshold for mesh detail in the cubemap.
    pub mesh_lod_threshold: f32,
}

impl ReflectionProbe {
    /// Creates a new reflection probe with Godot-compatible defaults.
    pub fn new(id: ReflectionProbeId) -> Self {
        Self {
            id,
            transform: Transform3D::IDENTITY,
            size: Vector3::new(20.0, 20.0, 20.0),
            origin_offset: Vector3::ZERO,
            box_projection: false,
            interior: false,
            enable_shadows: false,
            max_distance: 0.0,
            intensity: 1.0,
            update_mode: ReflectionProbeUpdateMode::default(),
            ambient_mode: ReflectionProbeAmbientMode::default(),
            ambient_color: Color::BLACK,
            ambient_color_energy: 1.0,
            cull_mask: 0xFFFFF, // 20 bits, Godot default
            mesh_lod_threshold: 1.0,
        }
    }

    /// Returns the axis-aligned bounding box of the probe's influence volume
    /// in world space, accounting for `size` and `origin_offset`.
    pub fn world_aabb_min_max(&self) -> (Vector3, Vector3) {
        let center = self.transform.origin + self.origin_offset;
        let half = self.size * 0.5;
        (center - half, center + half)
    }

    /// Returns `true` if the given world-space point is inside the probe's
    /// influence box.
    pub fn contains_point(&self, point: Vector3) -> bool {
        let (min, max) = self.world_aabb_min_max();
        point.x >= min.x
            && point.x <= max.x
            && point.y >= min.y
            && point.y <= max.y
            && point.z >= min.z
            && point.z <= max.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_probe_values_match_godot() {
        let probe = ReflectionProbe::new(ReflectionProbeId(1));
        assert_eq!(probe.size, Vector3::new(20.0, 20.0, 20.0));
        assert_eq!(probe.origin_offset, Vector3::ZERO);
        assert!(!probe.box_projection);
        assert!(!probe.interior);
        assert!(!probe.enable_shadows);
        assert!((probe.max_distance - 0.0).abs() < f32::EPSILON);
        assert!((probe.intensity - 1.0).abs() < f32::EPSILON);
        assert_eq!(probe.update_mode, ReflectionProbeUpdateMode::Once);
        assert_eq!(probe.ambient_mode, ReflectionProbeAmbientMode::Environment);
        assert_eq!(probe.ambient_color, Color::BLACK);
        assert!((probe.ambient_color_energy - 1.0).abs() < f32::EPSILON);
        assert_eq!(probe.cull_mask, 0xFFFFF);
        assert!((probe.mesh_lod_threshold - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_id_equality() {
        assert_eq!(ReflectionProbeId(10), ReflectionProbeId(10));
        assert_ne!(ReflectionProbeId(10), ReflectionProbeId(20));
    }

    #[test]
    fn contains_point_at_center() {
        let probe = ReflectionProbe::new(ReflectionProbeId(1));
        assert!(probe.contains_point(Vector3::ZERO));
    }

    #[test]
    fn contains_point_at_edge() {
        let probe = ReflectionProbe::new(ReflectionProbeId(1));
        // Half-extent is 10.0, so edge is at ±10.
        assert!(probe.contains_point(Vector3::new(10.0, 10.0, 10.0)));
        assert!(!probe.contains_point(Vector3::new(10.1, 0.0, 0.0)));
    }

    #[test]
    fn contains_point_with_offset() {
        let mut probe = ReflectionProbe::new(ReflectionProbeId(1));
        probe.origin_offset = Vector3::new(5.0, 0.0, 0.0);
        // Center shifted to (5, 0, 0), so range on X is -5..15.
        assert!(probe.contains_point(Vector3::new(14.0, 0.0, 0.0)));
        assert!(!probe.contains_point(Vector3::new(-6.0, 0.0, 0.0)));
    }

    #[test]
    fn world_aabb_with_transform() {
        let mut probe = ReflectionProbe::new(ReflectionProbeId(1));
        probe.transform.origin = Vector3::new(100.0, 0.0, 0.0);
        let (min, max) = probe.world_aabb_min_max();
        assert!((min.x - 90.0).abs() < f32::EPSILON);
        assert!((max.x - 110.0).abs() < f32::EPSILON);
    }

    #[test]
    fn update_mode_variants() {
        assert_eq!(ReflectionProbeUpdateMode::Once as u32, 0);
        assert_eq!(ReflectionProbeUpdateMode::Always as u32, 1);
    }

    #[test]
    fn ambient_mode_variants() {
        assert_eq!(ReflectionProbeAmbientMode::Disabled as u32, 0);
        assert_eq!(ReflectionProbeAmbientMode::Environment as u32, 1);
        assert_eq!(ReflectionProbeAmbientMode::ConstantColor as u32, 2);
    }
}
