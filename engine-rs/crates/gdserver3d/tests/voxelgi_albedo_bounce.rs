//! pat-s3rrv.114: `VoxelGI` bakes a bounce off scene SURFACES, not just lights.
//!
//! 🔑 The defect this pins is the reason the bead's scene-level acceptance was
//! impossible, not merely unwired. `VoxelGI::bake` accumulates only LIGHT
//! colour — it iterates `&[Light3D]` and never sees geometry or albedo. The
//! canonical GI demo is a RED WALL bleeding onto a white one, and a wall is not
//! a light, so it contributed exactly zero. Every consumer in the engine could
//! have been wired perfectly and the pixels would not have moved.
//!
//! `bake_with_surfaces` adds the missing term: each surface reflects
//! `albedo * direct_light_at(surface)` into the grid with the same `1/d²`
//! falloff and `propagation` scale the direct pass already uses.

use gdcore::math::{Color, Vector3};
use gdserver3d::gi::{GiSurfaceSample, VoxelGI, VoxelGIId};
use gdserver3d::light::{Light3D, Light3DId};

/// A point inside the default volume. `size` is FULL extents and
/// `world_aabb_min_max` halves it, so the default `Vector3(20, 20, 20)` spans
/// ±10 — not ±20, which the field's doc comment implied until this bead.
const PROBE: Vector3 = Vector3::new(5.0, 5.0, 5.0);

fn white_light() -> Light3D {
    Light3D::directional(Light3DId(1))
}

/// **The acceptance for the bake half.** A red surface tints the grid red, and
/// the same bake without it does not.
///
/// This is a DIFFERENTIAL: the two bakes are identical except for the surface,
/// so the assertion holds regardless of the absolute irradiance the direct pass
/// happens to produce. An absolute threshold would have to be re-tuned every
/// time `propagation` or the falloff changed, and would say nothing about which
/// term produced the colour.
#[test]
fn a_red_surface_bleeds_red_into_the_voxel_grid() {
    let lights = [white_light()];

    // CONTROL: direct light only — a white light must leave a neutral grid.
    let mut without = VoxelGI::new(VoxelGIId(1));
    without.bake_with_surfaces(&lights, &[]);
    let plain = without.sample(PROBE);
    assert!(
        plain.r > 0.0,
        "the direct pass must light the grid at all, or the comparison below \
         is between two blacks: got {plain:?}"
    );
    assert!(
        (plain.r - plain.g).abs() < 1e-6 && (plain.r - plain.b).abs() < 1e-6,
        "a WHITE light with no surfaces must bake a neutral cell, got {plain:?}"
    );

    // The bounce: a red surface sitting at the probed cell.
    let mut with_red = VoxelGI::new(VoxelGIId(2));
    with_red.bake_with_surfaces(
        &lights,
        &[GiSurfaceSample {
            position: PROBE,
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
        }],
    );
    let bled = with_red.sample(PROBE);

    assert!(
        bled.r > plain.r,
        "a red surface must ADD red irradiance: {} vs {} without it",
        bled.r,
        plain.r
    );
    // The green and blue channels are what make this colour bleed rather than
    // a brightness change — a bug that scaled all three would pass the
    // assertion above and fail these.
    assert!(
        (bled.g - plain.g).abs() < 1e-6,
        "a pure red surface must not change green: {} vs {}",
        bled.g,
        plain.g
    );
    assert!(
        (bled.b - plain.b).abs() < 1e-6,
        "a pure red surface must not change blue: {} vs {}",
        bled.b,
        plain.b
    );
    assert!(
        bled.r > bled.g,
        "the bled cell must read red-tinted, got {bled:?}"
    );
}

/// A surface outside the volume cannot tint it, and an unbaked volume samples
/// black — so a scene with no GI is bit-identical to one that never had it.
#[test]
fn gi_adds_nothing_where_it_does_not_apply() {
    let lights = [white_light()];

    // Unbaked: nothing at all.
    let fresh = VoxelGI::new(VoxelGIId(3));
    let unbaked = fresh.sample(PROBE);
    assert_eq!(
        (unbaked.r, unbaked.g, unbaked.b),
        (0.0, 0.0, 0.0),
        "an unbaked volume must sample black"
    );

    // Baked, but sampled far outside the ±20 volume.
    let mut baked = VoxelGI::new(VoxelGIId(4));
    baked.bake_with_surfaces(
        &lights,
        &[GiSurfaceSample {
            position: PROBE,
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
        }],
    );
    let outside = baked.sample(Vector3::new(500.0, 500.0, 500.0));
    assert_eq!(
        (outside.r, outside.g, outside.b),
        (0.0, 0.0, 0.0),
        "outside the volume a VoxelGI must contribute nothing"
    );
}

/// `sample_voxel_gi_at` sums the volumes that contain the point.
#[test]
fn the_aggregator_sums_only_containing_volumes() {
    let lights = [white_light()];
    let red = GiSurfaceSample {
        position: PROBE,
        albedo: Color::new(1.0, 0.0, 0.0, 1.0),
    };

    let mut a = VoxelGI::new(VoxelGIId(5));
    a.bake_with_surfaces(&lights, &[red]);
    let single = a.sample(PROBE);

    let mut b = VoxelGI::new(VoxelGIId(6));
    b.bake_with_surfaces(&lights, &[red]);
    // Moved far away, so it does not contain PROBE and must add nothing.
    b.transform.origin = Vector3::new(1000.0, 0.0, 0.0);

    let summed = gdserver3d::gi::sample_voxel_gi_at(&[a, b], PROBE);
    assert!(
        (summed.r - single.r).abs() < 1e-6,
        "a volume that does not contain the point must add nothing: {} vs {}",
        summed.r,
        single.r
    );
}
