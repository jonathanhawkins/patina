//! pat-mgixk.52, probe reflections: a `ReflectionProbe` must not lose its
//! contribution in the default editor / Play viewport.
//!
//! Both renderers *receive* probes — `populate_editor_scene_3d` calls
//! `add_reflection_probe` + `update_reflection_probe` for each
//! (`viewport_3d_render.rs:2398`) — but on the GPU side those only fill storage
//! and a query accessor (`wgpu_pipeline.rs:4891`); nothing reads them back
//! during shading. The software path adds the probe's ambient into
//! `entry_ambient` (`renderer.rs:1128-1136`), which multiplies albedo. So a
//! probe-lit surface rendered through `render_editor_3d_frame_gpu` came back
//! with the probe silently dropped.
//!
//! The bead allowed "route to software when probes exist" instead of porting
//! probe shading into the WGSL. pat-p3p1m replaced that routing (it made every
//! probe-lit Play frame software-rasterized) with probe ambient + reflections
//! shaded in WGSL, so this now pins the GPU shading against the software
//! reference rather than the routing.
//!
//! # Why the assertion is a RATIO of two deltas, not a pixel comparison
//!
//! ⚠️ The obvious test — render the probe scene and a probe-less control
//! through the GPU entry point and compare — is **confounded** after the fix:
//! the probe scene routes to software while the control still runs on the GPU,
//! so any difference mixes "the probe contributed" with "the two backends
//! rasterise independently". The existing `gpu_env_effects_render_from_scene`
//! notes that same backend disagreement is real and worth a tolerance.
//!
//! So nothing here compares across backends. Two deltas are measured instead:
//!
//! * `delta_probe` — software with the probe vs software without it. This is
//!   the probe's own magnitude, measured entirely within one backend, and it
//!   doubles as the fixture check: if the probe changes nothing, every other
//!   assertion would be vacuous.
//! * `delta_backend` — the GPU **entry point** with the probe vs software with
//!   the probe. The routing fix makes these the same code path, so this must be
//!   far smaller than `delta_probe`.
//!
//! At HEAD the GPU entry point shades on the GPU and drops the probe, so
//! `delta_backend` is roughly `delta_probe` and the ratio assertion fails.

#![cfg(feature = "gpu-render")]

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, render_editor_3d_frame, render_editor_3d_frame_gpu,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::environment::ToneMapper;

const W: u32 = 96;
const H: u32 = 96;

/// A white Lambert box, lit, with nothing else in the scene.
const CONTROL_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
mesh = "primitive:BoxMesh"
"#;

/// The same box, plus a `ReflectionProbe` whose box contains it and whose
/// ambient is pure red at full energy.
///
/// `ambient_mode = 2` is `ConstantColor`; `sample_probes_at` skips a probe whose
/// mode is `Disabled` (0), so leaving this out would make the probe contribute
/// nothing and the fixture check below would catch it.
const PROBE_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
mesh = "primitive:BoxMesh"

[node name="Probe" type="ReflectionProbe" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
size = Vector3(8, 8, 8)
ambient_mode = 2
ambient_color = Color(1, 0, 0, 1)
ambient_color_energy = 1.0
intensity = 1.0
"#;

fn load(tscn: &str) -> SceneTree {
    // ⚠️ Without this the render list is EMPTY: it collects only
    // `Node3D`-derived nodes, which is a ClassDB query, and an unseeded
    // registry answers false for everything. Both frames would come back blank
    // and identical, which reads as "the probe did nothing".
    gdobject::class_db::register_3d_classes();

    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;
    // Pinned, so the box is framed by construction rather than by the default
    // orbit camera. `override_transform` bypasses the orbit derivation.
    vp.camera.override_transform =
        Some(Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, 3.0)));
    // Grid and origin axes off. They are identical in both arms here, so they
    // cancel in the deltas rather than biasing them — but they are a large
    // constant area of unchanged pixels, which DILUTES `delta_probe` (a mean
    // over the whole frame) and weakens the ratio assertion for no reason.
    vp.grid.visible = false;
    // No sky: its ambient would swamp the probe's, and a procedural sky can
    // route the GPU entry point to software on its own — which would make the
    // backend delta below meaningless.
    vp.environment.sky_visible = false;
    vp.environment.sun_enabled = true;
    vp.environment.sun_direction = Vector3::new(0.0, 0.0, -1.0);
    vp.environment.sun_energy = 1.0;
    // Linear and glow-free so the probe's red shows as red rather than as a
    // tonemapped, bloomed approximation of it.
    vp.environment.environment.tone_mapper = ToneMapper::Linear;
    vp.environment.environment.glow_enabled = false;
    vp
}

/// The GPU entry point's frame, as 8-bit RGB.
fn gpu_rgb(tscn: &str) -> Vec<(f32, f32, f32)> {
    let tree = load(tscn);
    let vp = viewport();
    let inputs = editor_3d_render_inputs(&vp, &tree);
    let img = render_editor_3d_frame_gpu(&vp, &inputs);
    img.data
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

/// The software reference frame, quantised to 8 bits so it is comparable with
/// the byte-BGRA the GPU entry point returns.
fn software_rgb(tscn: &str) -> Vec<(f32, f32, f32)> {
    let tree = load(tscn);
    let vp = viewport();
    let fb = render_editor_3d_frame(&vp, &tree);
    fb.pixels
        .iter()
        .map(|c| {
            (
                (c.r.clamp(0.0, 1.0) * 255.0).round() / 255.0,
                (c.g.clamp(0.0, 1.0) * 255.0).round() / 255.0,
                (c.b.clamp(0.0, 1.0) * 255.0).round() / 255.0,
            )
        })
        .collect()
}

/// Mean absolute per-channel difference.
fn mean_abs_diff(a: &[(f32, f32, f32)], b: &[(f32, f32, f32)]) -> f32 {
    assert_eq!(a.len(), b.len(), "frames must be the same size");
    if a.is_empty() {
        return 0.0;
    }
    let total: f32 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (x.0 - y.0).abs() + (x.1 - y.1).abs() + (x.2 - y.2).abs())
        .sum();
    total / (a.len() as f32 * 3.0)
}

/// Whether a real GPU adapter exists. Without one `render_editor_3d_frame_gpu`
/// renders in software anyway, so `delta_backend` would be zero for a reason
/// that has nothing to do with this change.
fn gpu_available() -> bool {
    gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_some()
}

#[test]
fn gpu_play_view_reflection_probe_contributes() {
    if !gpu_available() {
        eprintln!(
            "SKIP gpu_play_view_reflection_probe_contributes: no GPU adapter on \
             this machine, so the GPU entry point renders in software \
             regardless and the assertion would hold for the wrong reason. \
             This is a SKIP, not a pass."
        );
        return;
    }

    let sw_probe = software_rgb(PROBE_SCENE);
    let sw_control = software_rgb(CONTROL_SCENE);
    let gpu_probe = gpu_rgb(PROBE_SCENE);

    // Fixture check, within ONE backend: the probe has to change the picture
    // before "the GPU entry agrees with software" means anything.
    let delta_probe = mean_abs_diff(&sw_probe, &sw_control);
    assert!(
        delta_probe > 0.01,
        "fixture precondition: a ConstantColor red probe containing the box must \
         change the software render, got mean |delta| {delta_probe}. If this \
         fires the probe is not contributing at all — check `ambient_mode = 2` \
         and that the probe's box contains the mesh origin."
    );

    // The probe's ambient is pure red, so it must show up as red specifically —
    // a probe wired to the wrong channel, or a generic brightening, would pass
    // the magnitude check above but not this.
    let red_gain: f32 = sw_probe
        .iter()
        .zip(sw_control.iter())
        .map(|(p, c)| p.0 - c.0)
        .sum();
    let blue_gain: f32 = sw_probe
        .iter()
        .zip(sw_control.iter())
        .map(|(p, c)| p.2 - c.2)
        .sum();
    assert!(
        red_gain > blue_gain * 4.0 && red_gain > 1.0,
        "a red probe must add red, not luminance: total red gain {red_gain}, \
         blue gain {blue_gain}"
    );

    // ⚠️ THE DEFECT. The GPU entry point must now agree with the software
    // reference on this scene, because a scene with probes routes there. At
    // HEAD it shades on the GPU and drops the probe entirely, so this delta is
    // about as large as `delta_probe` itself.
    let delta_backend = mean_abs_diff(&gpu_probe, &sw_probe);
    assert!(
        delta_backend < delta_probe / 4.0,
        "with a ReflectionProbe in the scene, `render_editor_3d_frame_gpu` must \
         agree with the software reference: backend delta \
         {delta_backend} against the probe's own delta {delta_probe}. A backend \
         delta of the same order means the GPU path is still shading without \
         the probe."
    );
}
