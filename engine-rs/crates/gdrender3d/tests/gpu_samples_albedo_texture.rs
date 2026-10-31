//! pat-mgixk.52, albedo half: the **wgpu** pipeline samples
//! `BaseMaterial3D.albedo_texture` per fragment.
//!
//! Before this, `fs_main` took albedo from the `model` uniform only
//! (`let albedo = model.albedo;`) and the pipeline had no texture bindings at
//! all outside the group-3 custom-shader samplers. So a textured
//! StandardMaterial3D, Sprite3D or Label3D rendered as a **flat colour** in the
//! shipped editor and Play viewport while the software path textured it
//! (`shader.rs:218`).
//!
//! # Why this test drives BOTH entry points
//!
//! The albedo map is bound in group 1 (the model group), which is the only one
//! of groups 0-2 rebuilt per draw. That group is assembled at **two** separate
//! call sites — one per render path — and a bind group that does not fill every
//! entry its layout declares is a *runtime validation failure*, not a compile
//! error. Testing one site would leave the other able to abort every draw:
//!
//! * `render_frame` (the `RenderingServer3D` trait method) → the cached
//!   draw-call path, keyed off `GpuDrawCall`.
//! * `render_editor_color_bgra` → the editor/Play path (`GpuDrawEntry`), which
//!   is what `render_editor_3d_frame_gpu` and therefore the actual viewport
//!   uses — the bead's wiring requirement.
//!
//! # Why the fixture is an UNLIT checkered cube
//!
//! Unlit shading returns `albedo + emission` directly, so the assertions are
//! about texture sampling and nothing else — no lambert term, no ambient, no
//! light direction to get wrong. The camera-facing quad of `Mesh3D::cube`
//! carries a full 0..1 UV square (verified in `mesh.rs`: each face pushes
//! corners `[0,0] [1,0] [1,1] [0,1]`), so a 2x2 checker lands as four
//! quadrants on that face.
//!
//! Pure red and pure blue are chosen because they are **fixed points of the
//! sRGB transfer function** in every channel (0 and 1 map to 0 and 1). The
//! assertions therefore hold whether the decode happens in the sampler, in the
//! shader, or not at all — this test proves the texture is *sampled*, and
//! deliberately does not also encode a claim about colour space, which it
//! cannot check. A mid-grey fixture would be needed for that and is called out
//! as remaining scope rather than asserted here.
//!
//! ⚠️ `GpuRenderer3D::new()` returning `None` means no adapter, which these
//! tests report as a loud SKIP. A vacuous green would defeat the purpose.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode, TextureImage};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;

const W: u32 = 96;
const H: u32 = 96;

/// `None` means no adapter, which is a skip and not a pass.
fn gpu_renderer() -> Option<GpuRenderer3D> {
    match GpuRenderer3D::new() {
        Some(g) => Some(g),
        None => {
            eprintln!(
                "SKIP: no wgpu adapter available; the albedo-texture half of \
                 pat-mgixk.52 cannot be exercised on this machine. This is a \
                 SKIP, not a pass."
            );
            None
        }
    }
}

/// The 2x2 red/blue checker the acceptance names.
fn checker() -> TextureImage {
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    TextureImage::new(2, 2, vec![red, blue, blue, red])
}

/// An unlit cube wearing the checker, sized so its camera-facing quad covers
/// roughly the middle 44px of a 96px frame with background visible around it.
///
/// Front face sits 2.1 ahead of the camera (centre -2.5, half-extent 0.4); the
/// visible half-height there is ~0.87 units, so the quad spans ~46% of the
/// half-frame.
fn build_scene<R: RenderingServer3D>(r: &mut R, texture: Option<TextureImage>) {
    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(0.8));
    r.set_material(
        inst,
        Material3D {
            // White, so the texel passes through the `albedo * texel` multiply
            // unchanged and a failure cannot be blamed on the flat colour.
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            shading_mode: ShadingMode::Unlit,
            albedo_texture: texture.map(std::sync::Arc::new),
            ..Default::default()
        },
    );
    r.set_transform(
        inst,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -2.5)),
    );
    r.set_visible(inst, true);
}

/// Strongly red-dominant: red leads BOTH other channels by a wide margin.
///
/// The margin is what keeps the black background and the flat-white
/// pre-change render out of the count — both have `r == g == b`.
fn is_red(c: (f32, f32, f32)) -> bool {
    c.0 > c.1 + 0.25 && c.0 > c.2 + 0.25
}

fn is_blue(c: (f32, f32, f32)) -> bool {
    c.2 > c.0 + 0.25 && c.2 > c.1 + 0.25
}

/// Any pixel distinguishable from the black clear — proves geometry rasterized
/// at all, so a count of zero red/blue cannot be blamed on an empty frame.
fn is_lit(c: (f32, f32, f32)) -> bool {
    c.0 + c.1 + c.2 > 0.1
}

fn frame_rgb(frame: &FrameData3D) -> Vec<(f32, f32, f32)> {
    frame.pixels.iter().map(|c| (c.r, c.g, c.b)).collect()
}

/// `render_editor_color_bgra` hands back tight top-down **BGRA** bytes.
fn bgra_rgb(bytes: &[u8]) -> Vec<(f32, f32, f32)> {
    bytes
        .chunks_exact(4)
        .map(|p| {
            (
                p[2] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[0] as f32 / 255.0,
            )
        })
        .collect()
}

/// The shared verdict for one rendered frame, so both paths are held to the
/// identical standard.
fn assert_checker_sampled(px: &[(f32, f32, f32)], path: &str) {
    assert_eq!(px.len(), (W * H) as usize, "{path}: unexpected frame size");

    let lit = px.iter().filter(|c| is_lit(**c)).count();
    assert!(
        lit > 200,
        "{path} precondition: the cube must rasterize before anything can be \
         said about its texture; only {lit} non-background pixels"
    );

    let red = px.iter().filter(|c| is_red(**c)).count();
    let blue = px.iter().filter(|c| is_blue(**c)).count();

    // ⚠️ THIS is the defect. With a flat albedo every pixel of the quad is the
    // same white, so `red` and `blue` are both ZERO however many pixels are
    // lit. Two colours on one surface can only come from a per-fragment
    // texture fetch.
    assert!(
        red > 20 && blue > 20,
        "{path}: a 2x2 red/blue checker albedo_texture must put BOTH colours on \
         the quad, got {red} red-dominant and {blue} blue-dominant pixels out \
         of {lit} lit. Zero of either means the wgpu pipeline is still taking \
         albedo from the flat model uniform and ignoring the texture."
    );

    // The checker is half red and half blue and the quad is axis-aligned and
    // centred, so the two counts should be close. A wildly lopsided split would
    // mean the UVs reach only part of the texture — sampled, but wrongly.
    let (lo, hi) = (red.min(blue) as f32, red.max(blue) as f32);
    assert!(
        hi / lo < 5.0,
        "{path}: a symmetric checker should cover the quad evenly; got \
         {red} red vs {blue} blue, which suggests the UVs or the wrap mode \
         reach only part of the texture"
    );
}

#[test]
fn gpu_render_frame_samples_the_albedo_texture() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };
    build_scene(&mut gpu, Some(checker()));
    let frame = gpu.render_frame(&Viewport3D::new(W, H));
    assert_checker_sampled(&frame_rgb(&frame), "render_frame");
}

#[test]
fn gpu_editor_path_samples_the_albedo_texture() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };
    build_scene(&mut gpu, Some(checker()));
    let (w, h, bytes) =
        gpu.render_editor_color_bgra(&Viewport3D::new(W, H), Color::new(0.0, 0.0, 0.0, 1.0));
    assert_eq!((w, h), (W, H), "editor path returned an unexpected size");
    assert_checker_sampled(&bgra_rgb(&bytes), "render_editor_color_bgra");
}

/// The no-op guarantee: a material with no albedo map binds a 1x1 white texel,
/// and white is the multiplicative identity — so an untextured draw must come
/// back exactly as it did before this change.
///
/// This is the assertion that would catch the dummy texture being the wrong
/// colour, which would otherwise dim or tint **every** untextured scene in the
/// editor while the textured tests above stayed green.
#[test]
fn an_untextured_material_is_unaffected() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };
    build_scene(&mut gpu, None);
    let frame = gpu.render_frame(&Viewport3D::new(W, H));
    let px = frame_rgb(&frame);

    let lit = px.iter().filter(|c| is_lit(**c)).count();
    assert!(
        lit > 200,
        "precondition: the untextured cube must still rasterize; {lit} lit pixels"
    );
    // Unlit white albedo through the white dummy must stay white: neutral and
    // bright. A dummy of any other colour shows up here as a tint or a dim.
    let mut off_white = 0usize;
    for c in px.iter().filter(|c| is_lit(**c)) {
        let max = c.0.max(c.1).max(c.2);
        let min = c.0.min(c.1).min(c.2);
        if max - min > 0.02 || max < 0.9 {
            off_white += 1;
        }
    }
    assert_eq!(
        off_white, 0,
        "an untextured unlit white material must render neutral white; \
         {off_white} of {lit} lit pixels were tinted or dimmed, which means the \
         default albedo texture is not 1x1 white"
    );
}
