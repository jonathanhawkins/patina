//! pat-s3rrv.114: a `VoxelGI` registered with the renderer reaches the point
//! the shading loop samples.
//!
//! `gdserver3d::gi` carried a complete VoxelGI implementation with ZERO
//! consumers — `grep` for it outside its own module found the lib re-export, a
//! ClassDB registration and a create-dialog tooltip. Neither shading loop
//! mentioned it, so a `.tscn` VoxelGI changed no pixel.
//!
//! This pins the renderer half of the fix: the `RenderingServer3D` methods
//! store the volume, and `sample_voxel_gi` — the call the shading loop makes
//! beside `sample_reflection_probes` — returns its baked contribution.

use gdcore::math::{Color, Vector3};
use gdrender3d::renderer::SoftwareRenderer3D;
use gdserver3d::gi::{GiSurfaceSample, VoxelGI, VoxelGIId};
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::server::RenderingServer3D;

/// A point inside the default volume. `size` is FULL extents and
/// `world_aabb_min_max` halves it, so the default `Vector3(20, 20, 20)` spans
/// ±10 — not ±20, which the field's doc comment implied until this bead.
const PROBE: Vector3 = Vector3::new(5.0, 5.0, 5.0);

/// A volume baked with a red surface sitting at [`PROBE`].
fn red_bled_volume(id: u64) -> VoxelGI {
    let mut gi = VoxelGI::new(VoxelGIId(id));
    gi.bake_with_surfaces(
        &[Light3D::directional(Light3DId(1))],
        &[GiSurfaceSample {
            position: PROBE,
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
        }],
    );
    gi
}

/// **The wiring.** A registered volume is sampled where the shading loop asks.
#[test]
fn a_registered_voxel_gi_is_sampled_by_the_renderer() {
    let mut r = SoftwareRenderer3D::new();

    // CONTROL: with nothing registered the renderer must contribute nothing.
    // Without this, a sampler that returned a constant tint would pass the
    // assertion below while breaking every scene that has no GI node.
    let none = r.sample_voxel_gi(PROBE);
    assert_eq!(
        (none.r, none.g, none.b),
        (0.0, 0.0, 0.0),
        "a renderer with no VoxelGI must add nothing"
    );
    assert!(r.voxel_gis().is_empty());

    let gi = red_bled_volume(1);
    r.add_voxel_gi(gi.id);
    r.update_voxel_gi(&gi);

    assert_eq!(r.voxel_gis().len(), 1, "the volume must be stored");

    let sampled = r.sample_voxel_gi(PROBE);
    assert!(
        sampled.r > 0.0,
        "the registered volume must contribute at the sampled point, got {sampled:?}"
    );
    assert!(
        sampled.r > sampled.g,
        "the red surface baked into the volume must reach the renderer as a RED \
         tint, not a neutral brightening: {sampled:?}"
    );
}

/// `add_` is idempotent and `update_` replaces by id, matching the reflection
/// probe methods next to them — the editor calls both every frame.
#[test]
fn re_registering_the_same_volume_does_not_duplicate_it() {
    let mut r = SoftwareRenderer3D::new();
    let gi = red_bled_volume(7);

    for _ in 0..3 {
        r.add_voxel_gi(gi.id);
        r.update_voxel_gi(&gi);
    }

    assert_eq!(
        r.voxel_gis().len(),
        1,
        "three frames of add+update must leave ONE volume, not three — the \
         editor re-pushes every frame and a duplicate would triple the GI"
    );

    // And the contribution must not have tripled with it.
    let once = {
        let mut solo = SoftwareRenderer3D::new();
        solo.add_voxel_gi(gi.id);
        solo.update_voxel_gi(&gi);
        solo.sample_voxel_gi(PROBE)
    };
    let thrice = r.sample_voxel_gi(PROBE);
    assert!(
        (thrice.r - once.r).abs() < 1e-6,
        "re-registering must not accumulate: {} vs {}",
        thrice.r,
        once.r
    );
}

/// Removing the volume takes its contribution with it.
#[test]
fn removing_a_volume_removes_its_contribution() {
    let mut r = SoftwareRenderer3D::new();
    let gi = red_bled_volume(3);
    r.add_voxel_gi(gi.id);
    r.update_voxel_gi(&gi);
    assert!(r.sample_voxel_gi(PROBE).r > 0.0);

    r.remove_voxel_gi(gi.id);

    let gone = r.sample_voxel_gi(PROBE);
    assert_eq!(
        (gone.r, gone.g, gone.b),
        (0.0, 0.0, 0.0),
        "a removed volume must stop contributing"
    );
}
