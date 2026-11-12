//! pat-nzgdu.214 — a `Texture3D` density map, sampled by a `FogVolume`, picks
//! the correct depth layer.
//!
//! ⚠️ **The ClassDB registration this bead asks for was already in place.**
//! `Texture` (virtual base), `Texture2DArray`, `Texture3D`, `TextureLayered`
//! and the four `*RD` wrappers are all registered, and gdobject's
//! `texture_layered_array_binds_in_pipeline` test already covers
//! `class_exists`/abstractness for them. What was missing was real layered/volume
//! *data* and a consumer of it — the "samples the correct layer" half.
//!
//! This drives the CPU fog path: a `Texture3DVolume` with distinct per-layer
//! values is assigned to a `FogVolume` density map, and sampling at different
//! depths must select the matching layer. (gdserver3d fog is CPU, so "a rendered
//! frame" is a CPU density sample.)

use gdcore::math::Vector3;
use gdserver3d::fog_volume::{FogMaterial, FogVolume, FogVolumeShape, Texture3DVolume};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// A box-shaped FogVolume of the given size with falloff/edge-fade disabled, so
/// a density-volume sample reads through as the raw density.
fn box_fog(size: Vector3) -> FogVolume {
    let mut material = FogMaterial::default();
    material.density = 1.0;
    material.height_falloff = 0.0;
    material.edge_fade = 0.0;
    FogVolume {
        shape: FogVolumeShape::Box,
        size,
        material,
    }
}

#[test]
fn texture_layered_volume_registration_test() {
    // A 1x1x3 volume: three depth layers with distinct density values.
    let vol = Texture3DVolume::from_layers(1, 1, vec![vec![0.1], vec![0.5], vec![0.9]]);
    assert_eq!(vol.depth(), 3);
    assert_eq!(vol.width(), 1);
    assert_eq!(vol.height(), 1);

    // The sampler selects the layer by the w (depth) coordinate.
    assert_eq!(vol.sample(0.5, 0.5, 0.0), 0.1, "front layer");
    assert_eq!(vol.sample(0.5, 0.5, 0.5), 0.5, "middle layer");
    assert_eq!(vol.sample(0.5, 0.5, 1.0), 0.9, "back layer");

    // Assign it as a FogVolume density map on a 2x2x2 box (local space [-1, 1]
    // per axis); density = 1.0, so the sampled layer value IS the density.
    let mut fog = box_fog(Vector3::new(2.0, 2.0, 2.0));
    fog.material.density_volume = Some(vol);

    // z = -1 -> front layer (0.1), z = 0 -> middle (0.5), z = +1 -> back (0.9).
    let front = fog.sample_density(Vector3::new(0.0, 0.0, -0.99));
    let middle = fog.sample_density(Vector3::new(0.0, 0.0, 0.0));
    let back = fog.sample_density(Vector3::new(0.0, 0.0, 0.99));
    assert!(
        approx(front, 0.1),
        "front voxel picks layer 0 (0.1), got {front}"
    );
    assert!(
        approx(middle, 0.5),
        "middle voxel picks layer 1 (0.5), got {middle}"
    );
    assert!(
        approx(back, 0.9),
        "back voxel picks layer 2 (0.9), got {back}"
    );

    // The layers really are distinguished — not one constant read three times.
    assert!(front != middle && middle != back && front != back);

    // Outside the box the density is zero regardless of the volume.
    assert_eq!(fog.sample_density(Vector3::new(5.0, 0.0, 0.0)), 0.0);
}

#[test]
fn density_volume_modulates_the_base_density() {
    // Base density 0.4 scaled by a volume whose single layer is 0.5 -> 0.2.
    let vol = Texture3DVolume::from_layers(1, 1, vec![vec![0.5]]);
    let mut fog = box_fog(Vector3::new(2.0, 2.0, 2.0));
    fog.material.density = 0.4;
    fog.material.density_volume = Some(vol);

    let d = fog.sample_density(Vector3::ZERO);
    assert!(approx(d, 0.2), "0.4 * 0.5 = 0.2, got {d}");
}

#[test]
fn no_density_volume_leaves_base_density_untouched() {
    let mut fog = box_fog(Vector3::new(2.0, 2.0, 2.0));
    fog.material.density = 0.7;
    assert!(fog.material.density_volume.is_none());

    let d = fog.sample_density(Vector3::ZERO);
    assert!(
        approx(d, 0.7),
        "base density unchanged without a volume, got {d}"
    );
}

#[test]
fn a_2x2_layer_samples_the_correct_texel_within_the_layer() {
    // One depth layer, a 2x2 grid: [[a, b], [c, d]] laid out row-major.
    let vol = Texture3DVolume::from_layers(2, 2, vec![vec![0.1, 0.2, 0.3, 0.4]]);
    // u selects column, v selects row: (0,0)->0.1, (1,0)->0.2, (0,1)->0.3, (1,1)->0.4.
    assert_eq!(vol.sample(0.0, 0.0, 0.0), 0.1);
    assert_eq!(vol.sample(1.0, 0.0, 0.0), 0.2);
    assert_eq!(vol.sample(0.0, 1.0, 0.0), 0.3);
    assert_eq!(vol.sample(1.0, 1.0, 0.0), 0.4);
}

#[test]
fn short_layers_are_padded_so_sampling_never_panics() {
    // A layer shorter than width*height is zero-padded rather than panicking.
    let vol = Texture3DVolume::from_layers(2, 2, vec![vec![0.9]]);
    assert_eq!(vol.sample(0.0, 0.0, 0.0), 0.9, "the provided value");
    assert_eq!(vol.sample(1.0, 1.0, 0.0), 0.0, "padded to zero");

    // An empty volume samples zero, never panics.
    let empty = Texture3DVolume::default();
    assert!(empty.is_empty());
    assert_eq!(empty.sample(0.5, 0.5, 0.5), 0.0);
}
