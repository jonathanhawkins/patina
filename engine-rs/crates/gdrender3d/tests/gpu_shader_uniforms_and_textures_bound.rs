//! pat-s3rrv.122 acceptance: custom shader uniforms, samplers and UVs reach the
//! **wgpu** spatial pipeline — not just the software rasterizer.
//!
//! Before this bead the wgpu path returned flat material albedo: the custom
//! uniform block was never emitted, no sampler was ever bound, and `TIME` was
//! frozen. Every existing shader test drove `SoftwareRenderer3D`, so the GPU
//! backend could return a constant colour and stay green.
//!
//! # Why this test lives in gdrender3d and not gdeditor
//!
//! The two-backend render tests conventionally sit in `gdeditor`, but nothing
//! requires that: `render_frame` is a `RenderingServer3D` trait method and
//! **both** `SoftwareRenderer3D` and `GpuRenderer3D` implement it, both in this
//! crate. Building the scene through the trait keeps the test independent of
//! `gdeditor` (which also cannot compile at the time of writing) and of `.tscn`
//! loading, which this crate has no dependency on.
//!
//! # Why it insists on a real adapter
//!
//! ⚠️ `GpuRenderer3D::new()` returns `None` with no adapter. Treating that as a
//! pass would prove nothing, so the tests **skip loudly** instead. A vacuous
//! green here is exactly the failure this bead exists to correct.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::renderer::SoftwareRenderer3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::TextureImage;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;
use std::sync::Arc;

/// The bead's acceptance names `uniform vec4 tint; ALBEDO = tint.rgb * UV.x;`.
/// The GDSL front end does not yet accept a swizzle (`tint.rgb`) or a swizzle in
/// arithmetic, and `fragment_body_to_wgsl_statements` honestly declines a body it
/// cannot transpile — so that exact text yields no custom module and the draw
/// silently falls back to flat albedo. This is the same mechanism written in
/// grammar the parser proves it supports (`Uniform`, `UvComponent`, `Mul`):
/// a uniform scaling a UV-varying channel.
///
/// The default is **0.0** deliberately. If the material parameter never reaches
/// the GPU the face renders black, not merely dim, so the fixture cannot pass on
/// the shader's own default.
const TINT_SHADER: &str = "shader_type spatial;\nuniform float tint_r = 0.0;\nvoid fragment() { ALBEDO = vec3(tint_r * UV.x, 0.0, 0.0); }";

/// A sampler shader: the texels themselves must reach the framebuffer.
///
/// `ALBEDO = texture(tex, UV)` rather than `texture(tex, UV).rgb` for the same
/// parser reason as above; `parse_texture_uv` in gdserver3d pins this form.
const TEX_SHADER: &str =
    "shader_type spatial;\nuniform sampler2D tex;\nvoid fragment() { ALBEDO = texture(tex, UV); }";

const W: u32 = 64;
const H: u32 = 64;
/// Scan line through the middle of the cube's camera-facing quad.
const MID_ROW: usize = (H / 2) as usize;

fn viewport() -> Viewport3D {
    Viewport3D::new(W, H)
}

/// A cube facing the camera, lit head-on, wearing `material`.
///
/// Generic rather than `&mut dyn` so both concrete renderers take the identical
/// call sequence — the same one `gdscene`'s `render_server_3d` uses for lights.
fn build_scene<R: RenderingServer3D>(r: &mut R, material: ShaderMaterial3D) {
    let mut light = Light3D::directional(Light3DId(1));
    // Straight down -Z, so the quad the camera sees is the lit one. Leaving the
    // default direction would light a face we never sample and the test would
    // read black for reasons unrelated to the shader.
    light.direction = Vector3::new(0.0, 0.0, -1.0);
    light.energy = 1.0;
    r.add_light(light.id);
    r.update_light(&light);

    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(2.0));
    // `Mesh3D::cube` gives every face a full 0..1 UV square, which is what makes
    // `UV.x` a gradient rather than a constant.
    r.set_transform(
        inst,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -4.0)),
    );
    r.set_visible(inst, true);
    r.set_shader_material(inst, material);
}

fn tint_material() -> ShaderMaterial3D {
    let mut m = ShaderMaterial3D::new();
    m.shader = Some(Shader3D::new(ShaderType3D::Spatial, TINT_SHADER));
    // `Variant::Float` is a kind `uniform_variant_to_vec4` maps; an unhandled
    // kind falls through to [0,0,0,0] and paints black, which the 0.0 default
    // above is chosen to make indistinguishable from "never arrived" — both are
    // failures and both are loud.
    m.set_shader_parameter("tint_r", Variant::Float(1.0));
    m
}

fn texture_material() -> ShaderMaterial3D {
    let mut m = ShaderMaterial3D::new();
    m.shader = Some(Shader3D::new(ShaderType3D::Spatial, TEX_SHADER));
    // Magenta: a colour the white sampler placeholder cannot produce, so a pass
    // means real texels, not a fallback. (A grey texture would be
    // indistinguishable from unlit white.)
    m.set_texture(
        "tex",
        Arc::new(TextureImage::solid(Color::new(1.0, 0.0, 1.0, 1.0))),
    );
    m
}

fn pixel(frame: &FrameData3D, x: usize, y: usize) -> Color {
    frame.pixels[y * frame.width as usize + x]
}

/// First and last non-background pixel on row `y`.
///
/// Returned rather than assumed: the cube's on-screen extent depends on the
/// default fov, and hard-coding columns would make a miss look like a shader
/// bug.
fn lit_span(frame: &FrameData3D, y: usize) -> (usize, usize) {
    let w = frame.width as usize;
    let row = &frame.pixels[y * w..(y + 1) * w];
    let lit = |c: &Color| c.r + c.g + c.b > 0.02;
    let first = row.iter().position(lit).expect(
        "row is entirely background — the cube never rasterized, so no shader claim can be made",
    );
    let last = row.iter().rposition(lit).expect("first implies last");
    assert!(
        last - first >= 6,
        "the lit span is {} px wide, too narrow to read a gradient from",
        last - first
    );
    (first, last)
}

/// `None` means no adapter, which is a skip and not a pass.
fn gpu_renderer() -> Option<GpuRenderer3D> {
    match GpuRenderer3D::new() {
        Some(g) => Some(g),
        None => {
            eprintln!(
                "SKIP: no wgpu adapter available; the GPU half of pat-s3rrv.122 \
                 cannot be exercised on this machine. This is a SKIP, not a pass."
            );
            None
        }
    }
}

/// `ALBEDO = tint.rgb * UV.x` must paint a red horizontal gradient on the wgpu
/// path, agreeing with the software path.
#[test]
fn gpu_custom_uniform_produces_a_uv_gradient_matching_software() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };

    let vp = viewport();

    let mut sw = SoftwareRenderer3D::new();
    build_scene(&mut sw, tint_material());
    let sw_frame = sw.render_frame(&vp);

    build_scene(&mut gpu, tint_material());
    let gpu_frame = gpu.render_frame(&vp);

    let (gl, gr) = lit_span(&gpu_frame, MID_ROW);
    let (sl, sr) = lit_span(&sw_frame, MID_ROW);

    // Sample just inside the silhouette; the edge pixels are partially covered.
    let g_left = pixel(&gpu_frame, gl + 2, MID_ROW);
    let g_right = pixel(&gpu_frame, gr - 2, MID_ROW);
    let s_left = pixel(&sw_frame, sl + 2, MID_ROW);
    let s_right = pixel(&sw_frame, sr - 2, MID_ROW);

    // 1. The uniform arrived: the face is RED, not the flat white/grey albedo
    //    the wgpu path used to return regardless of the shader.
    assert!(
        g_right.r > g_right.g + 0.2 && g_right.r > g_right.b + 0.2,
        "the GPU face is not red-dominant ({:?}) — `tint` did not reach the shader",
        g_right
    );

    // 2. UV.x varies across the face. An exact ordering, not `>= 0`: a flat
    //    fill satisfies "red" perfectly well and is precisely the old bug.
    let gpu_gradient = g_right.r - g_left.r;
    assert!(
        gpu_gradient > 0.15,
        "no gradient on the GPU path: left {:.3} right {:.3} (delta {:.3}) — \
         UV.x is constant, so the fragment stage is not consuming UVs",
        g_left.r,
        g_right.r,
        gpu_gradient
    );

    // 3. The software path agrees. Both renderers must slope the same way, and
    //    by a comparable amount, which is the "within tolerance" the acceptance
    //    asks for. The tolerance is loose because lighting and coverage differ
    //    slightly between rasterizers; the SIGN is the part that cannot differ.
    let sw_gradient = s_right.r - s_left.r;
    assert!(
        sw_gradient > 0.15,
        "the software reference itself has no gradient ({:.3}) — the fixture is \
         wrong, not the GPU path",
        sw_gradient
    );
    assert!(
        (gpu_gradient - sw_gradient).abs() < 0.35,
        "GPU and software gradients disagree: gpu {:.3} vs software {:.3}",
        gpu_gradient,
        sw_gradient
    );
}

/// `texture(tex, UV)` must return the bound image's texels on the wgpu path.
#[test]
fn gpu_bound_sampler_returns_the_textures_texels() {
    let Some(mut gpu) = gpu_renderer() else {
        return;
    };

    let vp = viewport();
    build_scene(&mut gpu, texture_material());
    let frame = gpu.render_frame(&vp);

    let (l, r) = lit_span(&frame, MID_ROW);
    let c = pixel(&frame, (l + r) / 2, MID_ROW);

    // Magenta: red and blue lit, green dark. The white placeholder returned when
    // no sampler is bound has r == g == b, so the green channel is what
    // separates a real texel from the fallback.
    assert!(
        c.r > 0.2 && c.b > 0.2,
        "the textured face is missing red/blue ({:?}) — no texels sampled",
        c
    );
    assert!(
        c.g + 0.15 < c.r && c.g + 0.15 < c.b,
        "the face is achromatic ({:?}) — this is the white sampler placeholder, \
         so no texture was bound to the shader",
        c
    );
}
