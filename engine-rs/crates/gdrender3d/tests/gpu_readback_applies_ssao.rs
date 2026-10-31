//! pat-mgixk.52, SSAO half: the **wgpu** readback runs ambient occlusion.
//!
//! The software renderer calls `apply_ssao_env` on its framebuffer
//! (`renderer.rs:683`, `:1305`), so `Environment.ssao_enabled` darkens creases
//! there. The GPU readback did not: it went decals → fog → VoxelGI → fog
//! volumes → `apply_camera_post` and skipped occlusion entirely. A scene with
//! SSAO on therefore came back **bit-identical** to the same scene with SSAO
//! off, and no existing test noticed because every SSAO test drove the
//! software path or called `apply_ssao` directly as a unit.
//!
//! # Why this test lives in gdrender3d
//!
//! `render_frame` is a `RenderingServer3D` trait method and `GpuRenderer3D`
//! implements it in this crate, so the scene can be built through the trait
//! with no `gdeditor` or `.tscn` dependency — the same reasoning
//! `gpu_shader_uniforms_and_textures_bound.rs` records.
//!
//! # Why it insists on a real adapter
//!
//! ⚠️ `GpuRenderer3D::new()` returns `None` with no adapter. Treating that as
//! a pass would prove nothing, so this test **skips loudly** instead. A
//! vacuous green here is exactly the failure this bead exists to correct.
//!
//! # Why the fixture is a small cube in front of a big wall
//!
//! `apply_ssao`'s kernel occludes a pixel when a neighbour within `radius` is
//! **closer by more than 0.02** in stored depth. Two facts fix the geometry:
//!
//! * The depth buffer holds raw `z/w` with background `1.0`. With this
//!   projection (`near = 0.05`, `far = 4000`) that is
//!   `1.000025 - 0.1/d` for a surface `d` ahead of the camera — strongly
//!   nonlinear, so a *distant* pair of surfaces has almost no depth gap. A
//!   cube at `d = 4` against the background differs by only ~0.012 and would
//!   never trip the kernel: the test would be green before and after the fix,
//!   for the wrong reason.
//! * Occlusion darkens the **farther** surface, so that surface has to be lit.
//!   A silhouette against a black background multiplies zero by the occlusion
//!   factor and changes nothing.
//!
//! Hence: a lit wall at `d = 50` (depth ~0.998) with a small lit cube at
//! `d = 1.7` (depth ~0.941) in front of it. The gap is ~0.057, nearly 3x the
//! kernel's threshold, and the wall pixels ringing the cube's silhouette are
//! the ones that must darken.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::environment::Environment3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;

const W: u32 = 96;
const H: u32 = 96;

/// Centre of the frame, which the cube's camera-facing quad covers. Its
/// half-width on screen is ~20px, so this sample and its 2px kernel are
/// entirely inside the quad.
const CUBE_X: usize = (W / 2) as usize;
const CUBE_Y: usize = (H / 2) as usize;

/// A wall sample far from the silhouette: the cube's edge is ~27px away in
/// both axes, well outside the kernel, and the wall extends past the frame so
/// there is no other depth discontinuity nearby.
const WALL_X: usize = 6;
const WALL_Y: usize = 6;

/// `None` means no adapter, which is a skip and not a pass.
fn gpu_renderer() -> Option<GpuRenderer3D> {
    match GpuRenderer3D::new() {
        Some(g) => Some(g),
        None => {
            eprintln!(
                "SKIP: no wgpu adapter available; the GPU readback half of \
                 pat-mgixk.52 cannot be exercised on this machine. This is a \
                 SKIP, not a pass."
            );
            None
        }
    }
}

fn viewport(ssao: bool) -> Viewport3D {
    let mut vp = Viewport3D::new(W, H);
    vp.environment = Some(Environment3D {
        ssao_enabled: ssao,
        // The default radius. `apply_ssao_env` maps it to
        // `(1.0 * 2.0).round().clamp(1.0, 8.0)` = a 2px kernel, the same
        // neighbourhood `apply_ssao`'s own unit fixture uses.
        ssao_radius: 1.0,
        ssao_intensity: 2.0,
        ..Default::default()
    });
    vp
}

/// A small lit cube standing off a large lit wall, both facing the camera.
///
/// Generic rather than `&mut dyn` so the call sequence matches the other
/// two-backend tests exactly.
fn build_scene<R: RenderingServer3D>(r: &mut R) {
    let mut light = Light3D::directional(Light3DId(1));
    // Straight down -Z, so the quads the camera sees are the lit ones. The
    // default direction would light faces we never sample and both frames
    // would read black — which SSAO cannot change, making the test vacuous.
    light.direction = Vector3::new(0.0, 0.0, -1.0);
    light.energy = 1.0;
    r.add_light(light.id);
    r.update_light(&light);

    // The wall: 200 units across, front face 50 ahead of the camera. The
    // visible half-height at that distance is ~20.7 units, so a half-extent of
    // 100 covers the whole frame and the wall has no silhouette of its own.
    let wall = r.create_instance();
    r.set_mesh(wall, Mesh3D::cube(200.0));
    r.set_material(wall, Material3D::default());
    r.set_transform(
        wall,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -150.0)),
    );
    r.set_visible(wall, true);

    // The occluder: front face 1.7 ahead, ~41px wide on a 96px frame.
    let cube = r.create_instance();
    r.set_mesh(cube, Mesh3D::cube(0.6));
    r.set_material(cube, Material3D::default());
    r.set_transform(
        cube,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -2.0)),
    );
    r.set_visible(cube, true);
}

fn pixel(frame: &FrameData3D, x: usize, y: usize) -> Color {
    frame.pixels[y * frame.width as usize + x]
}

fn luma(c: Color) -> f32 {
    c.r + c.g + c.b
}

#[test]
fn gpu_readback_applies_ssao_from_the_environment() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };
    build_scene(&mut gpu);

    let off = gpu.render_frame(&viewport(false));
    let on = gpu.render_frame(&viewport(true));

    assert_eq!(off.pixels.len(), (W * H) as usize);
    assert_eq!(on.pixels.len(), off.pixels.len());

    // Preconditions. `apply_ssao_env` scales colour, so an unlit fixture is one
    // SSAO provably cannot change and every assertion below would pass on a
    // black frame.
    let wall_off = pixel(&off, WALL_X, WALL_Y);
    let cube_off = pixel(&off, CUBE_X, CUBE_Y);
    assert!(
        luma(wall_off) > 0.05,
        "fixture precondition: the wall at ({WALL_X},{WALL_Y}) must be lit \
         before occlusion can darken it, got {wall_off:?}"
    );
    assert!(
        luma(cube_off) > 0.05,
        "fixture precondition: the cube at ({CUBE_X},{CUBE_Y}) must be lit \
         and in front of the wall, got {cube_off:?}"
    );

    // The defect itself: with SSAO off and on, the readback used to return the
    // same pixels because the occlusion pass was never called.
    let mut changed = 0usize;
    let mut brightened = Vec::new();
    for i in 0..off.pixels.len() {
        let a = off.pixels[i];
        let b = on.pixels[i];
        let delta = luma(a) - luma(b);
        if delta.abs() > 1e-3 {
            changed += 1;
            if delta < 0.0 {
                brightened.push((i % W as usize, i / W as usize, luma(a), luma(b)));
            }
        }
    }
    assert!(
        changed > 8,
        "enabling Environment.ssao_enabled must change the wgpu readback: \
         {changed} of {} pixels differ. Zero means the GPU path still skips \
         apply_ssao_env, so SSAO is silently ignored on this backend.",
        off.pixels.len()
    );

    // Direction, not just difference: occlusion only ever removes light. This
    // is what distinguishes the SSAO pass from any other source of variation
    // between two renders.
    assert!(
        brightened.is_empty(),
        "SSAO must only darken; {} pixel(s) got brighter, first {:?}",
        brightened.len(),
        brightened.first()
    );

    // Locality. A global dim — a stray exposure or tonemap change — would also
    // satisfy the two assertions above. These two say the darkening is the
    // crease and nothing else: the wall far from the silhouette and the cube's
    // own flat quad have no closer neighbour within the kernel, so their
    // occlusion factor is exactly 1.0.
    let wall_on = pixel(&on, WALL_X, WALL_Y);
    assert!(
        (luma(wall_off) - luma(wall_on)).abs() < 1e-3,
        "unoccluded wall at ({WALL_X},{WALL_Y}) must be untouched: \
         {wall_off:?} -> {wall_on:?}. A change here is a global dim, not \
         ambient occlusion."
    );
    let cube_on = pixel(&on, CUBE_X, CUBE_Y);
    assert!(
        (luma(cube_off) - luma(cube_on)).abs() < 1e-3,
        "the cube's flat camera-facing quad has no closer neighbour, so \
         ({CUBE_X},{CUBE_Y}) must be untouched: {cube_off:?} -> {cube_on:?}"
    );
}

/// The complement, and the reason the pass is called unconditionally: a scene
/// with SSAO disabled must come back exactly as it did before this change.
///
/// `apply_ssao_env` self-gates on `ssao_enabled && ssao_intensity > 0.0`, and
/// this pins that gate at the readback rather than trusting the comment.
#[test]
fn ssao_disabled_leaves_the_gpu_readback_untouched() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };
    build_scene(&mut gpu);

    let mut zero_intensity = viewport(true);
    if let Some(env) = zero_intensity.environment.as_mut() {
        env.ssao_intensity = 0.0;
    }

    let off = gpu.render_frame(&viewport(false));
    let gated = gpu.render_frame(&zero_intensity);

    assert!(
        luma(pixel(&off, WALL_X, WALL_Y)) > 0.05,
        "fixture precondition: the wall must be lit"
    );
    for i in 0..off.pixels.len() {
        let a = off.pixels[i];
        let b = gated.pixels[i];
        assert!(
            (luma(a) - luma(b)).abs() < 1e-4,
            "ssao_intensity = 0.0 must be a no-op at pixel {i}: {a:?} -> {b:?}"
        );
    }
}
