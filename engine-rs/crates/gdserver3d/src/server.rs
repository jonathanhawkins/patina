//! Abstract 3D rendering server trait.

use gdcore::math::Color;
use gdcore::math3d::Transform3D;

use crate::decal::Decal3D;
use crate::fog_volume::FogVolume;
use crate::instance::{GeometryInstanceSettings, Instance3DId};
use crate::light::{Light3D, Light3DId};
use crate::material::Material3D;
use crate::mesh::Mesh3D;
use crate::multimesh::MultiMesh3D;
use crate::occluder::OccluderInstance3D;
use crate::reflection_probe::{ReflectionProbe, ReflectionProbeId};
use crate::shader::ShaderMaterial3D;
use crate::viewport::Viewport3D;

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

    /// Assigns a shader material to an instance, overriding the standard material for color.
    fn set_shader_material(&mut self, id: Instance3DId, material: ShaderMaterial3D);

    /// Assigns a [`MultiMesh3D`] resource to an instance for instanced rendering.
    ///
    /// When a multimesh is set, the renderer draws the shared mesh once per
    /// instance entry, composing the instance's world transform with each
    /// per-entry transform and applying per-entry colors.
    fn set_multimesh(&mut self, id: Instance3DId, multimesh: MultiMesh3D);

    /// Clears the multimesh from an instance.
    fn clear_multimesh(&mut self, id: Instance3DId);

    /// Sets the world-space transform for an instance.
    fn set_transform(&mut self, id: Instance3DId, transform: Transform3D);

    /// Sets visibility for an instance.
    fn set_visible(&mut self, id: Instance3DId, visible: bool);

    /// Sets an instance's `GeometryInstance3D` render settings — shadow casting
    /// mode, visibility range, transparency, overlay material, cull margins
    /// (pat-nzgdu.76).
    ///
    /// **Required** (pat-s3rrv.29). This used to default to a no-op so that a
    /// renderer which does not model per-instance settings stayed
    /// trait-compatible. What actually happened is that `GpuRenderer3D` — the
    /// backend `gdeditor` ships by default — never implemented it, so every
    /// `cast_shadow` / `visibility_range` / `transparency` a scene authored was
    /// silently discarded while the software renderer's tests stayed green. A
    /// silent no-op made the gap invisible; a missing impl is a compile error.
    fn set_geometry_instance_settings(
        &mut self,
        id: Instance3DId,
        settings: GeometryInstanceSettings,
    );

    /// Adds a light to the scene and returns its ID.
    fn add_light(&mut self, id: Light3DId);

    /// Removes a light from the scene.
    fn remove_light(&mut self, id: Light3DId);

    /// Updates a light's properties (direction, color, energy, shadow, etc.).
    fn update_light(&mut self, light: &Light3D);

    /// Adds a reflection probe to the scene.
    fn add_reflection_probe(&mut self, id: ReflectionProbeId);

    /// Updates a reflection probe's properties (transform, size, intensity,
    /// ambient mode, etc.). Implementations should match by `probe.id`.
    ///
    /// Default implementation is a no-op so non-probe-aware renderers stay
    /// trait-compatible without behaviour changes.
    fn update_reflection_probe(&mut self, _probe: &ReflectionProbe) {}

    /// Removes a reflection probe from the scene.
    fn remove_reflection_probe(&mut self, id: ReflectionProbeId);

    /// Adds a `FogVolume` at `transform` to this frame's scene (pat-0q2kx.132).
    ///
    /// **Required** (pat-s3rrv.29) — see
    /// [`set_geometry_instance_settings`](Self::set_geometry_instance_settings)
    /// for why these four stopped being defaulted. A backend that genuinely has
    /// no fog-volume pass should say so with an explicit empty body, which is
    /// visible in review, rather than inheriting silence from the trait.
    fn add_fog_volume(&mut self, volume: FogVolume, transform: Transform3D);

    /// Adds a `Decal` to this frame's scene (pat-0q2kx.132). The decal carries
    /// its own `global_transform`. **Required** (pat-s3rrv.29).
    fn add_decal(&mut self, decal: Decal3D);

    /// Adds an `OccluderInstance3D` to this frame's occlusion set
    /// (pat-0q2kx.132). **Required** (pat-s3rrv.29) — a renderer that does not
    /// cull may implement it as an explicit no-op, which is correct, just
    /// slower, and unlike a trait default it is visible at the call site.
    fn add_occluder(&mut self, occluder: OccluderInstance3D);

    /// Renders a frame for the given 3D viewport.
    fn render_frame(&mut self, viewport: &Viewport3D) -> FrameData3D;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_data_creation() {
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
