//! pat-t42ca on BOTH render paths: inside a `ReflectionProbe` whose
//! `ambient_mode` is the default `Environment`, Godot REPLACES the ambient
//! light with the probe capture's most-blurred level along the normal
//! (`scene_forward_lights_inc.glsl` `reflection_process` 981-992, then
//! `scene_forward_clustered.glsl` 2082-2096). Patina added
//! `ambient_color * energy` (black by default) and kept the flat environment
//! ambient, so a probe in a dark room lit nothing differently — platformer_3d's
//! shadowed floor read 63 where Godot reads 39 (69 in Godot with every probe
//! set to `ambient_mode = 0`).
//!
//! The scene: a box in a probe; its capture differs from the flat ambient, so
//! replacing the ambient must change the box. The control is the same
//! probe with `ambient_mode = 0`; the reflection and the editor marker are
//! common to both arms.

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

fn scene(ambient_mode: i32) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
mesh = "primitive:BoxMesh"

[node name="Probe" type="ReflectionProbe" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
size = Vector3(8, 8, 8)
blend_distance = 0.0
ambient_mode = {ambient_mode}
"#
    )
}

fn load(tscn: &str) -> SceneTree {
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
    vp.camera.override_transform =
        Some(Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, 3.0)));
    vp.grid.visible = false;
    // No sky and no sun: the capture is the dark scene, and the box is lit
    // mostly by ambient, which is the term under test.
    vp.environment.sky_visible = false;
    vp.environment.sun_enabled = false;
    vp.environment.environment.tone_mapper = ToneMapper::Linear;
    vp.environment.environment.glow_enabled = false;
    vp
}

fn gpu_luma(tscn: &str) -> Vec<f32> {
    let tree = load(tscn);
    let vp = viewport();
    let inputs = editor_3d_render_inputs(&vp, &tree);
    let img = render_editor_3d_frame_gpu(&vp, &inputs);
    img.data
        .chunks_exact(4)
        .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / (3.0 * 255.0))
        .collect()
}

fn software_luma(tscn: &str) -> Vec<f32> {
    let tree = load(tscn);
    let fb = render_editor_3d_frame(&viewport(), &tree);
    fb.pixels
        .iter()
        .map(|c| {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0;
            (q(c.r) + q(c.g) + q(c.b)) / 3.0
        })
        .collect()
}

fn mean(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

fn mean_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len().max(1) as f32
}

#[test]
fn software_environment_probe_replaces_the_flat_ambient() {
    let env = software_luma(&scene(1));
    let off = software_luma(&scene(0));
    assert!(
        mean(&off) > 0.02,
        "fixture: the disabled-ambient box must be visibly lit, got mean {}",
        mean(&off)
    );
    // Before pat-t42ca an Environment probe added its black `ambient_color`,
    // so these two frames were byte-identical.
    let delta = mean_abs_diff(&env, &off);
    assert!(
        delta > 0.01,
        "an Environment-mode probe must REPLACE the flat ambient with its \
         capture, so its frame must differ from ambient_mode = 0: mean |delta| \
         {delta} (means {} vs {})",
        mean(&env),
        mean(&off)
    );
}

#[test]
fn gpu_environment_probe_matches_the_software_reference() {
    if gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_none() {
        eprintln!("SKIP gpu_environment_probe_matches_the_software_reference: no GPU adapter");
        return;
    }
    let gpu_env = gpu_luma(&scene(1));
    let gpu_off = gpu_luma(&scene(0));
    let sw_env = software_luma(&scene(1));
    let sw_off = software_luma(&scene(0));
    let delta_mode = mean_abs_diff(&gpu_env, &gpu_off);
    assert!(
        delta_mode > 0.01,
        "the GPU path must replace the ambient too: mean |delta| {delta_mode} \
         between ambient_mode 1 and 0"
    );
    // Same direction and size as software, and close to it.
    let sw_shift = mean(&sw_env) - mean(&sw_off);
    let gpu_shift = mean(&gpu_env) - mean(&gpu_off);
    assert!(
        sw_shift * gpu_shift > 0.0 && (gpu_shift - sw_shift).abs() < sw_shift.abs() / 3.0,
        "GPU and software must move the same way: shift {gpu_shift} vs {sw_shift}"
    );
    let delta_backend = mean_abs_diff(&gpu_env, &sw_env);
    assert!(
        delta_backend < delta_mode / 3.0,
        "GPU and software must agree on the probe ambient: backend delta \
         {delta_backend} vs mode delta {delta_mode}"
    );
}
