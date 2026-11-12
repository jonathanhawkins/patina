//! Baking the scene's `VoxelGI` volumes so their indirect light reaches pixels
//! (pat-s3rrv.114).
//!
//! # The gap this closes
//!
//! Every other link in the chain already existed. The collector fills
//! [`SceneEnvEffects3D::voxel_gis`], both renderers carry a `voxel_gis` list,
//! and `gdrender3d`'s shading pass modulates each pixel by
//! `sample_voxel_gi_at`. But **nothing ever baked the volumes**, and an unbaked
//! volume samples pure black — which the shading pass explicitly skips, with a
//! comment saying so. Authoring a VoxelGI therefore changed no pixel, which is
//! the same "complete subsystem with no caller" shape the bead was opened for,
//! one level deeper than where it was first found.
//!
//! ⚠️ The bead's named test passed throughout. It asserts that the authored
//! `energy`/`propagation`/`interior` survive into the collected list — a
//! *collection* test that samples no pixels at all. A green acceptance test is
//! not the same as the acceptance holding.

use gdserver3d::gi::{GiSurfaceSample, VoxelGI};
use gdserver3d::light::Light3D;

use crate::viewport_3d_render::{scene_light_to_light3d, Render3DItem, Render3DKind, SceneLight3D};

/// The bounced-light samples a bake reads, taken from the scene's meshes.
///
/// Only mesh-bearing items contribute: the editor also emits `Primitive` markers
/// for cameras and lights and `Collision` wireframes for shapes, and neither is
/// real geometry. Bouncing light off a light's own gizmo would tint the room
/// with whatever colour the marker happens to be drawn in.
///
/// An item's authored material wins over the flat `albedo` fallback, because
/// that is the colour the frame is actually shaded with.
pub fn gi_surfaces(items: &[Render3DItem]) -> Vec<GiSurfaceSample> {
    items
        .iter()
        .filter(|it| matches!(it.kind, Render3DKind::Mesh { .. }))
        .filter_map(|it| {
            let albedo = it.material.as_ref().map(|m| m.albedo).or(it.albedo)?;
            Some(GiSurfaceSample {
                position: it.global_transform.origin,
                albedo,
            })
        })
        .collect()
}

/// Bakes every collected volume against the scene's lights and surfaces.
///
/// ⚠️ Uses [`scene_light_to_light3d`] rather than converting the lights again
/// here. A second copy of that twenty-field mapping would let the bake be lit by
/// subtly different lights than the frame it belongs to — a disagreement with no
/// symptom except wrong-coloured bounce.
///
/// Skips the walk entirely when there is nothing to bake, so the overwhelmingly
/// common scene — no VoxelGI at all — pays only an `is_empty` check.
pub fn bake_voxel_gis(
    volumes: &mut [VoxelGI],
    scene_lights: &[SceneLight3D],
    items: &[Render3DItem],
) {
    if volumes.is_empty() {
        return;
    }
    let lights: Vec<Light3D> = scene_lights
        .iter()
        .enumerate()
        .map(|(i, sl)| {
            scene_light_to_light3d(sl, gdserver3d::light::Light3DId(9_000_001 + i as u64))
        })
        .collect();
    let surfaces = gi_surfaces(items);
    for gi in volumes.iter_mut() {
        gi.bake_with_surfaces(&lights, &surfaces);
    }
}
