//! pat-3u1wu / pat-wyei2: the **wgpu** pipeline samples a material's maps with
//! the sampler its `texture_filter` asks for, and composites alpha-blended
//! draws farthest first.
//!
//! * `texture_filter` (pat-3u1wu): the GPU had ONE bilinear sampler for every
//!   material, so the 3D platformer's pixel-art `tile_material.tres`
//!   (`texture_filter = 4`, NEAREST_WITH_MIPMAPS_ANISOTROPIC) came out blurred.
//!   A 2x2 red/blue checker magnified over ~40 px makes the difference
//!   countable: bilinear blends neighbouring texels into purple across most
//!   of the quad; nearest leaves no purple at all.
//! * Blend order (pat-wyei2): the editor/Play path drew in instance creation
//!   order, so a transparent surface created AFTER one in front of it was
//!   composited over it. The platformer's coin counter is a Label3D with four
//!   darker `Parallax` Label3D children stacked just behind it; creation order
//!   put the darkest copy on top.
//!
//! Red and blue are fixed points of the sRGB transfer, so no assertion here
//! depends on where the colour decode happens.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode, TextureFilter, TextureImage, Transparency};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;
use std::sync::Arc;

const W: u32 = 96;
const H: u32 = 96;

fn gpu() -> Option<GpuRenderer3D> {
    let r = GpuRenderer3D::new();
    if r.is_none() {
        eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
    }
    r
}

fn checker() -> TextureImage {
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    TextureImage::new(2, 2, vec![red, blue, blue, red])
}

fn checker_cube(r: &mut GpuRenderer3D, filter: TextureFilter) {
    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(0.8));
    r.set_material(
        inst,
        Material3D {
            shading_mode: ShadingMode::Unlit,
            albedo_texture: Some(Arc::new(checker())),
            texture_filter: filter,
            ..Default::default()
        },
    );
    r.set_transform(
        inst,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -2.5)),
    );
    r.set_visible(inst, true);
}

fn rgb(p: &[u8]) -> (f32, f32, f32) {
    (
        p[2] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[0] as f32 / 255.0,
    )
}

/// (pure red-or-blue texels, blended red+blue texels) on the quad.
fn pure_and_mixed(bgra: &[u8]) -> (usize, usize) {
    let mut pure = 0;
    let mut mixed = 0;
    for p in bgra.chunks_exact(4) {
        let (r, _g, b) = rgb(p);
        if r + b < 0.3 {
            continue; // background
        }
        if r > 0.12 && b > 0.12 {
            mixed += 1;
        } else {
            pure += 1;
        }
    }
    (pure, mixed)
}

fn editor_frame(r: &mut GpuRenderer3D) -> Vec<u8> {
    r.render_editor_color_bgra(&Viewport3D::new(W, H), Color::new(0.0, 0.0, 0.0, 1.0))
        .2
}

#[test]
fn nearest_texture_filter_samples_without_blending_texels() {
    let Some(mut r) = gpu() else { return };

    r.reset_scene();
    checker_cube(&mut r, TextureFilter::LinearWithMipmaps);
    let (lin_pure, lin_mixed) = pure_and_mixed(&editor_frame(&mut r));

    r.reset_scene();
    checker_cube(&mut r, TextureFilter::NearestWithMipmapsAnisotropic);
    let (near_pure, near_mixed) = pure_and_mixed(&editor_frame(&mut r));

    // CONTROL: bilinear really does blend at this magnification, so a zero
    // below is the filter and not a fixture that cannot produce purple.
    assert!(
        lin_mixed > 200,
        "precondition: the default LINEAR filter must blend the magnified 2x2 \
         checker into purple; got {lin_mixed} mixed / {lin_pure} pure"
    );
    assert!(
        near_pure > 1000,
        "the nearest-filtered quad must draw: {near_pure} pure texels"
    );
    // Edge pixels can straddle the quad's silhouette, nothing else blends.
    assert!(
        near_mixed * 50 < near_pure,
        "texture_filter NEAREST_WITH_MIPMAPS_ANISOTROPIC must not blend texels: \
         {near_mixed} purple vs {near_pure} pure (bilinear gave {lin_mixed} purple)"
    );
}

/// The trait `render_frame` path builds its own per-draw sampler too.
#[test]
fn nearest_texture_filter_reaches_the_render_frame_path() {
    let Some(mut r) = gpu() else { return };
    checker_cube(&mut r, TextureFilter::Nearest);
    let frame = r.render_frame(&Viewport3D::new(W, H));
    let mut pure = 0;
    let mut mixed = 0;
    for c in &frame.pixels {
        if c.r + c.b < 0.3 {
            continue;
        }
        if c.r > 0.12 && c.b > 0.12 {
            mixed += 1;
        } else {
            pure += 1;
        }
    }
    assert!(pure > 1000, "quad must draw: {pure}");
    assert!(
        mixed * 50 < pure,
        "render_frame blended a NEAREST map: {mixed} vs {pure}"
    );
}

fn alpha_quad(r: &mut GpuRenderer3D, z: f32, color: Color) {
    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(1.0));
    r.set_material(
        inst,
        Material3D {
            albedo: color,
            shading_mode: ShadingMode::Unlit,
            transparency: Transparency::Alpha,
            ..Default::default()
        },
    );
    r.set_transform(
        inst,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, z)),
    );
    r.set_visible(inst, true);
}

#[test]
fn blended_draws_composite_farthest_first_not_in_creation_order() {
    let Some(mut r) = gpu() else { return };
    let red = Color::new(1.0, 0.0, 0.0, 0.6);
    let blue = Color::new(0.0, 0.0, 1.0, 0.6);
    let centre = ((H / 2 * W + W / 2) * 4) as usize;

    // NEAR red created first, FAR blue second: creation order would put the
    // far blue on top.
    r.reset_scene();
    alpha_quad(&mut r, -3.0, red);
    alpha_quad(&mut r, -6.0, blue);
    let near_first = editor_frame(&mut r);
    // Same scene, created far-first: the order every renderer must converge on.
    r.reset_scene();
    alpha_quad(&mut r, -6.0, blue);
    alpha_quad(&mut r, -3.0, red);
    let far_first = editor_frame(&mut r);

    let (nr, _, nb) = rgb(&near_first[centre..centre + 4]);
    // The near red is composited LAST, so it dominates the centre.
    assert!(
        nr > nb + 0.2,
        "the nearer alpha surface must be composited over the farther one \
         whatever order they were created in: centre r={nr:.2} b={nb:.2}"
    );
    assert!(
        near_first == far_first,
        "blended draws must not depend on creation order"
    );
}
