//! pat-s3rrv.122 — which way does `UV.x` run in the SOFTWARE rasterizer?
//!
//! `gpu_shader_uniforms_and_textures_bound` fails on its own precondition:
//!
//! ```text
//! the software reference itself has no gradient (-0.235) — the fixture is wrong,
//! not the GPU path
//! ```
//!
//! The GPU assertions in that test **pass**. The software path produces a
//! gradient of the same magnitude and the **opposite sign**, and both renderers
//! consume the identical `Mesh3D::cube(2.0)` — so the mesh data cannot be the
//! cause.
//!
//! # What this probe separates
//!
//! The cube has six faces and the software rasterizer does **no backface
//! culling** — `rasterizer.rs:113` says outright that it handles "both CW and
//! CCW winding" — so it draws every face and lets the depth test choose. That
//! leaves two very different explanations for the sign, needing opposite fixes:
//!
//! 1. **Wrong face.** The far `-Z` face wins the depth test. Its `u_dir` is
//!    `-X` (`mesh.rs:747`), which would produce exactly the observed negative
//!    gradient — while the near `+Z` face's `u_dir` is `+X`.
//! 2. **Flipped UV.** The software fragment stage runs `UV.x` backwards
//!    regardless of geometry.
//!
//! ⚠️ **A SINGLE front-facing quad decides it.** One face means "wrong face" is
//! impossible, so the sign here is attributable to the UV path alone:
//!
//! * gradient **positive** ⇒ the software UV path is correct, and the cube
//!   failure is face selection (explanation 1);
//! * gradient **negative** ⇒ the software flips `UV.x` (explanation 2).

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::renderer::SoftwareRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const MID_ROW: usize = (H / 2) as usize;

/// The same shader the failing test uses: red scaled by `UV.x`.
const TINT_SHADER: &str = "shader_type spatial;\nuniform float tint_r = 0.0;\nvoid fragment() { ALBEDO = vec3(tint_r * UV.x, 0.0, 0.0); }";

/// The cube the failing test uses, unchanged.
///
/// ⚠️ My first attempt built a single front-facing QUAD, reasoning that one face
/// makes "wrong face" impossible. **It never rasterized** — the row came back
/// entirely background — so the probe reported a fixture failure instead of an
/// answer. A zero-thickness mesh is evidently not a shape this path draws, and
/// rather than debug a new fixture I reuse the known-good cube and vary only the
/// TRANSFORM, which is the one thing the failing test never varies.
fn cube_mesh() -> Mesh3D {
    Mesh3D::cube(2.0)
}

/// Renders the cube at `yaw` radians about Y and returns the UV.x gradient
/// across the camera-facing face.
///
/// At yaw 0 the **+Z** face is toward the camera (`u_dir = +X`, so a POSITIVE
/// gradient is correct). At yaw π the **−Z** face is (`u_dir = −X`), and its
/// UV.x therefore runs the other way in world space — but after the rotation it
/// points the SAME way on screen, so a correct renderer gives a positive
/// gradient at BOTH yaws.
fn software_gradient(yaw: f32) -> (f32, usize) {
    let mut sw = SoftwareRenderer3D::new();

    let mut light = Light3D::directional(Light3DId(1));
    light.direction = Vector3::new(0.0, 0.0, -1.0);
    light.energy = 1.0;
    sw.add_light(light.id);
    sw.update_light(&light);

    let inst = sw.create_instance();
    sw.set_mesh(inst, cube_mesh());
    sw.set_transform(
        inst,
        // ⚠️ Rotate the IDENTITY first, then translate. `rotated` also rotates
        // the origin (`origin: rot.xform(self.origin)`), so translating first
        // would swing the cube around the camera instead of spinning it in
        // place — and the probe would compare two different viewpoints.
        Transform3D::IDENTITY
            .rotated(Vector3::new(0.0, 1.0, 0.0), yaw)
            .translated(Vector3::new(0.0, 0.0, -4.0)),
    );
    sw.set_visible(inst, true);
    sw.set_shader_material(inst, tint_material());

    let frame = sw.render_frame(&Viewport3D::new(W, H));
    let (l, r) = lit_span(&frame, MID_ROW);
    let gradient = pixel(&frame, r - 2, MID_ROW).r - pixel(&frame, l + 2, MID_ROW).r;
    // ⚠️ The width comes back too, and it is not decoration. Without it there is
    // no way to tell "the gradient is geometry-independent" from "the rotation
    // was never applied" — which is exactly the mistake the first version of
    // this probe made when yaw 0 and yaw PI returned bit-identical floats.
    (gradient, r - l)
}

fn tint_material() -> ShaderMaterial3D {
    let mut m = ShaderMaterial3D::new();
    m.shader = Some(Shader3D::new(ShaderType3D::Spatial, TINT_SHADER));
    m.set_shader_parameter("tint_r", Variant::Float(1.0));
    m
}

fn pixel(frame: &FrameData3D, x: usize, y: usize) -> Color {
    frame.pixels[y * frame.width as usize + x]
}

fn lit_span(frame: &FrameData3D, y: usize) -> (usize, usize) {
    let w = frame.width as usize;
    let row = &frame.pixels[y * w..(y + 1) * w];
    let lit = |c: &Color| c.r + c.g + c.b > 0.02;
    let first = row
        .iter()
        .position(lit)
        .expect("row is entirely background — the quad never rasterized");
    let last = row.iter().rposition(lit).expect("first implies last");
    assert!(
        last - first >= 6,
        "lit span is {} px, too narrow to read a gradient",
        last - first
    );
    (first, last)
}

/// The software path DOES consume `UV.x`, and rotation DOES reach the vertex
/// stage. Both true today, and both worth pinning.
///
/// ⚠️ This test was added for a mechanical reason worth keeping on record: while
/// the probe below was `#[ignore]`d, this target had no live test, and a target
/// whose only test is ignored runs **zero** tests — the runner then exits `4`
/// ("no tests were run") and the target is unverifiable rather than green. The
/// probe is live again now that pat-s3rrv.122 is fixed, so that reason has
/// lapsed; this stays because it is a genuine precondition, not filler. It is
/// about the two facts the probe *depends on*, so if either regressed the
/// probe's verdict would be meaningless rather than merely wrong.
#[test]
fn software_uv_gradient_face_probe_preconditions_hold() {
    let (front, front_w) = software_gradient(0.0);
    let (_, turned_w) = software_gradient(std::f32::consts::FRAC_PI_4);

    assert!(
        front.abs() > 0.05,
        "the software path must consume UV.x at all — a flat fill ({front:.3}) \
         would make every direction claim vacuous"
    );
    assert_ne!(
        front_w, turned_w,
        "rotation must reach the vertex stage: a cube's silhouette is {front_w} \
         px head-on and must differ at 45 degrees (2 vs 2*sqrt(2) wide)"
    );
}

// ✅ UN-IGNORED (pat-s3rrv.122 fixed). This was left asserting the RIGHT answer
// while the bug stood, rather than inverted to match it, so that the fix would
// turn it green instead of appearing to break the suite — and removing the
// attribute was named as the last line of that change. This is that line.
//
// The answer the probe returned: neither of the two explanations in the module
// header. The software UV path was correct and so was face selection; what was
// wrong was the FACE CULL. `should_cull` compared screen-space winding as if Y
// grew upward, so `CullMode::Back` — Godot's default — culled every
// camera-FACING face and let the far side of the mesh draw instead. The far -Z
// face carries `u_dir = -X` (`mesh.rs`), which is exactly the mirrored gradient
// measured here. See `cull_matches_screen_space_winding.rs`.
//
// It stays as a regression guard: it is an end-to-end reading of the rendered
// pixels, so it fails if any stage between the mesh and the framebuffer starts
// mirroring UVs again, whether or not the cull is the cause next time.
#[test]
fn software_uv_gradient_face_probe() {
    let (front, front_w) = software_gradient(0.0);
    let (turned, turned_w) = software_gradient(std::f32::consts::FRAC_PI_4);

    // (1) Did the rotation happen? Asserted FIRST: every reading below is
    // meaningless if the renderer drew the same image twice.
    assert_ne!(
        front_w, turned_w,
        "the silhouette is {front_w} px at BOTH yaw 0 and yaw PI/4 — a cube \
         is 2 wide head-on and 2*sqrt(2) at 45 degrees, so this width MUST \
         change. It did not, which means the rotation was never applied: the \
         bug to chase is set_transform dropping rotation, and nothing this \
         probe says about UV can be trusted until that is fixed"
    );

    // (2) Is there a gradient to read at all?
    assert!(
        front.abs() > 0.05,
        "no gradient head-on ({front:.3}) — the software path is not consuming \
         UV.x, a separate failure from the one under test"
    );

    // (3) The direction. The camera-facing face's `u_dir` points screen-right at
    // any yaw, so a correct renderer gives a POSITIVE gradient head-on.
    assert!(
        front > 0.0,
        "SOFTWARE UV.x RUNS BACKWARDS: head-on gradient is {front:.3}, expected \
         positive. The camera-facing +Z face has u_dir = +X (mesh.rs:747) and \
         +X is screen-right (right = forward x up = -Z x +Y = +X). Every \
         individual stage of this path checks out — projection has a positive \
         X scale, the view matrix is a plain inverse, screen mapping flips only \
         Y, and both the vertex stage and the fragment hand-off pass UV \
         through unchanged — so the fault is in their composition, not in any \
         one of them. (45-degree reading {turned:.3}, widths {front_w} vs \
         {turned_w})"
    );
}
