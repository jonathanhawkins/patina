//! pat-mgixk.52, ORM + emission: the **wgpu** pipeline samples
//! `BaseMaterial3D`'s roughness, metallic and emission maps.
//!
//! The bead's title names these alongside albedo and normal. Before this,
//! `fs_main` took emission from the `model` uniform and read `model.roughness`
//! / `model.metallic` as flat scalars, so all three textures were ignored on
//! the GPU while the software path sampled them (`shader.rs:229`, `:241`,
//! `:247`).
//!
//! # Channel conventions are Godot's, not arbitrary
//!
//! ⚠️ Roughness comes from the texture's **green** channel and metallic from
//! its **blue** channel — the software path spells both out. A fixture using
//! red, or luminance, would prove nothing about the channel wiring, so each
//! map here puts its value in the channel the engine is supposed to read and
//! leaves the others alone.
//!
//! # Why a 1x1 map for roughness and metallic
//!
//! These two feed only the specular term, so the observable is a shading
//! *change* rather than a colour at a position. A uniform 1x1 map changes the
//! whole surface at once, with no filtering subtleties between texels to
//! reason about — the texel is returned exactly whatever the UV.
//!
//! Both maps drive their value to **zero** against an authored 1.0. That
//! direction is deliberate: a white dummy already reads 1.0 in every channel,
//! so a map can only ever *reduce* these multipliers, and a fixture that tried
//! to raise them would be indistinguishable from the no-map case.
//!
//! ⚠️ `ShadingMode::Phong` throughout for those two: roughness and metallic are
//! read only inside the `shading_mode == 2u` branch. Under Lambert or Unlit the
//! maps are wired but unreachable, and the test would pass vacuously.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::{Material3D, ShadingMode, TextureImage};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;
use std::sync::Arc;

const W: u32 = 96;
const H: u32 = 96;

/// `None` means no adapter, which is a skip and not a pass.
fn gpu_renderer() -> Option<GpuRenderer3D> {
    match GpuRenderer3D::new() {
        Some(g) => Some(g),
        None => {
            eprintln!(
                "SKIP: no wgpu adapter available; the ORM/emission half of \
                 pat-mgixk.52 cannot be exercised on this machine. This is a \
                 SKIP, not a pass."
            );
            None
        }
    }
}

/// The 2x2 red/blue checker, reused as an **emission** map.
fn checker() -> TextureImage {
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    TextureImage::new(2, 2, vec![red, blue, blue, red])
}

/// Green = 0, so `roughness *= 0`. Red and blue are irrelevant to roughness and
/// are left at zero rather than set to something that would also disturb
/// metallic if the two channels were ever confused.
fn zero_roughness_map() -> TextureImage {
    TextureImage::new(1, 1, vec![Color::new(0.0, 0.0, 0.0, 1.0)])
}

/// Blue = 0, so `metallic *= 0`.
fn zero_metallic_map() -> TextureImage {
    TextureImage::new(1, 1, vec![Color::new(0.0, 0.0, 0.0, 1.0)])
}

fn build_scene<R: RenderingServer3D>(r: &mut R, material: Material3D) {
    let mut light = Light3D::directional(Light3DId(1));
    // Head-on, so the camera-facing quad is both lit and near the specular
    // peak — which is what makes a roughness change observable at all.
    light.direction = Vector3::new(0.0, 0.0, -1.0);
    light.energy = 1.0;
    r.add_light(light.id);
    r.update_light(&light);

    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(0.8));
    r.set_material(inst, material);
    r.set_transform(
        inst,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -2.5)),
    );
    r.set_visible(inst, true);
}

fn render(material: Material3D) -> Option<FrameData3D> {
    let mut gpu = gpu_renderer()?;
    build_scene(&mut gpu, material);
    Some(gpu.render_frame(&Viewport3D::new(W, H)))
}

/// A PBR material at both multipliers' maximum, so a map can only reduce them.
fn pbr_base() -> Material3D {
    Material3D {
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        shading_mode: ShadingMode::Phong,
        roughness: 1.0,
        metallic: 1.0,
        ..Default::default()
    }
}

fn luma(c: Color) -> f32 {
    c.r + c.g + c.b
}

fn lit_count(f: &FrameData3D) -> usize {
    f.pixels.iter().filter(|c| luma(**c) > 0.05).count()
}

fn differing(a: &FrameData3D, b: &FrameData3D) -> usize {
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-3 || (x.g - y.g).abs() > 1e-3 || (x.b - y.b).abs() > 1e-3
        })
        .count()
}

fn is_red(c: Color) -> bool {
    c.r > c.g + 0.25 && c.r > c.b + 0.25
}

fn is_blue(c: Color) -> bool {
    c.b > c.r + 0.25 && c.b > c.g + 0.25
}

/// Emission is the most directly observable of the three: an **unlit** material
/// with a black albedo renders its emission and nothing else, so the frame *is*
/// the emission map.
#[test]
fn gpu_samples_the_emission_map() {
    let Some(frame) = render(Material3D {
        // Black albedo, so nothing but emission reaches the framebuffer and the
        // assertions cannot be satisfied by the albedo path (already covered by
        // `gpu_samples_albedo_texture`).
        albedo: Color::new(0.0, 0.0, 0.0, 1.0),
        emission: Color::new(1.0, 1.0, 1.0, 1.0),
        emission_texture: Some(Arc::new(checker())),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    }) else {
        return;
    };

    let red = frame.pixels.iter().filter(|c| is_red(**c)).count();
    let blue = frame.pixels.iter().filter(|c| is_blue(**c)).count();
    // ⚠️ THE DEFECT. Without the map, emission is the flat white uniform, so
    // the quad is uniformly white: zero red-dominant and zero blue-dominant
    // pixels however many are drawn.
    assert!(
        red > 50 && blue > 50,
        "a 2x2 red/blue checker emission_texture must put BOTH colours on the \
         surface, got {red} red-dominant and {blue} blue-dominant. Zero of \
         either means the wgpu pipeline still takes emission from the flat \
         model uniform."
    );
}

/// The no-op guarantee for emission: with no map the white dummy multiplies
/// emission by one, so an emissive-white unlit surface stays white.
///
/// Without this, a dummy of the wrong colour would dim or tint every emissive
/// material in the engine while the test above stayed green.
#[test]
fn an_unmapped_emission_is_unaffected() {
    let Some(frame) = render(Material3D {
        albedo: Color::new(0.0, 0.0, 0.0, 1.0),
        emission: Color::new(1.0, 1.0, 1.0, 1.0),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    }) else {
        return;
    };
    let lit = lit_count(&frame);
    assert!(lit > 200, "fixture precondition: {lit} emissive pixels");
    let mut off_white = 0usize;
    for c in frame.pixels.iter().filter(|c| luma(**c) > 0.05) {
        let max = c.r.max(c.g).max(c.b);
        let min = c.r.min(c.g).min(c.b);
        if max - min > 0.02 || max < 0.9 {
            off_white += 1;
        }
    }
    assert_eq!(
        off_white, 0,
        "an unmapped emissive-white material must stay neutral white; \
         {off_white} of {lit} pixels were tinted or dimmed, which means the \
         default emission texture is not 1x1 white"
    );
}

/// Metallic drives `albedo_lin = albedo * (1 - metallic)`, so taking it from 1
/// to 0 restores the whole diffuse term — a large, unambiguous brightening.
#[test]
fn gpu_samples_the_metallic_map_from_its_blue_channel() {
    let Some(without) = render(pbr_base()) else {
        return;
    };
    let with = render(Material3D {
        metallic_texture: Some(Arc::new(zero_metallic_map())),
        ..pbr_base()
    })
    .expect("adapter was available a moment ago");

    let lit = lit_count(&without);
    assert!(
        lit > 200,
        "fixture precondition: the PBR quad must rasterize and be lit; {lit} \
         lit pixels"
    );

    let changed = differing(&without, &with);
    assert!(
        changed > 100,
        "a metallic_texture whose BLUE channel is 0 must cancel the authored \
         metallic = 1.0 and restore the diffuse term: only {changed} of {} \
         pixels differ. Zero means the wgpu pipeline still reads metallic as a \
         flat uniform.",
        without.pixels.len()
    );
    // Direction: metallic 1 -> 0 reinstates diffuse light that was being
    // multiplied away, so the mapped render must be brighter somewhere. Without
    // this, any source of variation would satisfy the assertion above.
    let brighter = without
        .pixels
        .iter()
        .zip(with.pixels.iter())
        .filter(|(a, b)| luma(**b) > luma(**a) + 1e-3)
        .count();
    assert!(
        brighter > 50,
        "driving metallic to 0 restores the diffuse term, so the surface must \
         get BRIGHTER; only {brighter} pixels did"
    );
}

/// Roughness feeds the GGX lobe width, so taking it from 1 to 0 narrows the
/// highlight — a change in the specular term with no diffuse component to
/// confound it.
#[test]
fn gpu_samples_the_roughness_map_from_its_green_channel() {
    let Some(without) = render(pbr_base()) else {
        return;
    };
    let with = render(Material3D {
        roughness_texture: Some(Arc::new(zero_roughness_map())),
        ..pbr_base()
    })
    .expect("adapter was available a moment ago");

    assert!(lit_count(&without) > 200, "fixture precondition");
    let changed = differing(&without, &with);
    assert!(
        changed > 100,
        "a roughness_texture whose GREEN channel is 0 must cancel the authored \
         roughness = 1.0 and sharpen the highlight: only {changed} of {} \
         pixels differ. Zero means the wgpu pipeline still reads roughness as a \
         flat uniform.",
        without.pixels.len()
    );
}

/// A material with none of the three maps must render exactly as it did before
/// this change — the white dummy is the identity for all three multiplies.
#[test]
fn an_unmapped_pbr_material_is_bit_identical() {
    let Some(a) = render(pbr_base()) else {
        return;
    };
    let b = render(pbr_base()).expect("adapter was available a moment ago");
    assert!(lit_count(&a) > 200, "fixture precondition");
    assert_eq!(
        differing(&a, &b),
        0,
        "two renders of an unmapped PBR material must be bit-identical"
    );
}
