//! pat-p3p1m: environment fog and ReflectionProbes render on the GPU in the
//! editor / Play viewport, instead of sending the whole frame to the software
//! rasterizer.
//!
//! The official 3D platformer demo authors both (a fogged `WorldEnvironment`
//! and seven probes), so every Play frame of it was software-rasterized at
//! ~80 ms. `render_editor_3d_frame_gpu` now applies the same fog pass on its
//! read-back depth and shades probe ambient + captured probe reflections in
//! WGSL.
//!
//! # What is asserted
//!
//! * The frame stays on the GPU (`software_fallback_frames` does not move) —
//!   this is what the old gate violated.
//! * The GPU frame shows the effect: GPU-with vs GPU-without differs by about
//!   as much as software-with vs software-without (the effect's own size,
//!   measured within ONE backend).
//! * The GPU frame agrees with the software reference: their difference is far
//!   below the effect's size. A ratio of two deltas, not a pixel comparison —
//!   the two rasterizers differ slightly everywhere (see
//!   `gpu_play_view_reflection_probe_contributes`).

#![cfg(feature = "gpu-render")]

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, render_editor_3d_frame, render_editor_3d_frame_gpu,
    software_fallback_frames,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::environment::ToneMapper;

const W: u32 = 128;
const H: u32 = 128;

fn load(tscn: &str) -> SceneTree {
    gdobject::class_db::register_3d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// The editor viewport with its furniture off and a raw (linear, glow-free)
/// post-process, so the deltas measure the effect rather than bloom.
fn viewport(fog: bool) -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;
    vp.grid.visible = false;
    vp.show_origin = false;
    vp.environment.environment.tone_mapper = ToneMapper::Linear;
    vp.environment.environment.glow_enabled = false;
    if fog {
        vp.environment.environment.fog_enabled = true;
        vp.environment.environment.fog_density = 0.15;
    }
    vp
}

fn to_rgb_bytes(data: &[u8]) -> Vec<(f32, f32, f32)> {
    data.chunks_exact(4)
        .map(|p| {
            (
                p[2] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[0] as f32 / 255.0,
            )
        })
        .collect()
}

fn gpu_rgb(tscn: &str, vp: &Viewport3D) -> Vec<(f32, f32, f32)> {
    let tree = load(tscn);
    let inputs = editor_3d_render_inputs(vp, &tree);
    let before = software_fallback_frames();
    let img = render_editor_3d_frame_gpu(vp, &inputs);
    assert_eq!(
        software_fallback_frames(),
        before,
        "the GPU entry point fell back to the software rasterizer for this scene \
         — that is the ~80 ms/frame Play path pat-p3p1m removed"
    );
    to_rgb_bytes(&img.data)
}

fn software_rgb(tscn: &str, vp: &Viewport3D) -> Vec<(f32, f32, f32)> {
    let tree = load(tscn);
    render_editor_3d_frame(vp, &tree)
        .pixels
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

fn mean_abs_diff(a: &[(f32, f32, f32)], b: &[(f32, f32, f32)]) -> f32 {
    assert_eq!(a.len(), b.len());
    let total: f32 = a
        .iter()
        .zip(b)
        .map(|(x, y)| (x.0 - y.0).abs() + (x.1 - y.1).abs() + (x.2 - y.2).abs())
        .sum();
    total / (a.len() as f32 * 3.0)
}

fn gpu_available() -> bool {
    gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_some()
}

/// A lit box filling much of the view, at a distance where fog shows.
const BOX_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
transform = Transform3D(3, 0, 0, 0, 3, 0, 0, 0, 3, 0, 0, 0)
mesh = "primitive:BoxMesh"
"#;

#[test]
fn environment_fog_renders_on_the_gpu_and_matches_software() {
    if !gpu_available() {
        eprintln!("SKIP: no GPU adapter, the GPU entry point renders in software anyway");
        return;
    }
    let (fog, clear) = (viewport(true), viewport(false));
    let sw_fog = software_rgb(BOX_SCENE, &fog);
    let sw_clear = software_rgb(BOX_SCENE, &clear);
    let gpu_fog = gpu_rgb(BOX_SCENE, &fog);
    let gpu_clear = gpu_rgb(BOX_SCENE, &clear);

    let delta_fog = mean_abs_diff(&sw_fog, &sw_clear);
    assert!(
        delta_fog > 0.02,
        "fixture: fog must change the software frame ({delta_fog})"
    );
    let gpu_effect = mean_abs_diff(&gpu_fog, &gpu_clear);
    assert!(
        gpu_effect > delta_fog * 0.5,
        "the GPU frame must show the fog: GPU delta {gpu_effect} vs software {delta_fog}"
    );
    let backend = mean_abs_diff(&gpu_fog, &sw_fog);
    assert!(
        backend < delta_fog / 4.0,
        "GPU fog must match the software reference: backend delta {backend} vs fog {delta_fog}"
    );
}

/// A mirror (metallic 1, roughness 0) facing a red wall, inside a probe: the
/// probe's captured cubemap is what puts red on the mirror.
fn mirror_scene(probe: bool) -> String {
    let probe = if probe {
        "[node name=\"Probe\" type=\"ReflectionProbe\" parent=\".\"]\nsize = Vector3(20, 20, 20)\n"
    } else {
        ""
    };
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="StandardMaterial3D" id="Mirror_1"]
albedo_color = Color(1, 1, 1, 1)
metallic = 1.0
roughness = 0.0

[sub_resource type="StandardMaterial3D" id="Wall_1"]
albedo_color = Color(1, 0, 0, 1)
roughness = 1.0

[node name="Root" type="Node3D"]

[node name="Mirror" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
surface_material_override/0 = SubResource("Mirror_1")

[node name="Wall" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, -1, 0, 0, 0, -1, -2, 2, -2)
mesh = "primitive:PlaneMesh"
surface_material_override/0 = SubResource("Wall_1")

{probe}
"#
    )
}

#[test]
fn reflection_probe_reflections_render_on_the_gpu_and_match_software() {
    if !gpu_available() {
        eprintln!("SKIP: no GPU adapter, the GPU entry point renders in software anyway");
        return;
    }
    let vp = viewport(false);
    let (with, without) = (mirror_scene(true), mirror_scene(false));
    let sw_with = software_rgb(&with, &vp);
    let sw_without = software_rgb(&without, &vp);
    let gpu_with = gpu_rgb(&with, &vp);
    // Twice: the second frame reuses the cached `Once` capture.
    let gpu_with_again = gpu_rgb(&with, &vp);
    let gpu_without = gpu_rgb(&without, &vp);

    let delta_probe = mean_abs_diff(&sw_with, &sw_without);
    assert!(
        delta_probe > 0.005,
        "fixture: the probe must change the software mirror ({delta_probe})"
    );
    // The reflection carries the RED wall (and the preview sky): red gain must
    // clearly exceed blue gain, on the GPU as in software.
    let gain = |a: &[(f32, f32, f32)], b: &[(f32, f32, f32)]| {
        let r: f32 = a.iter().zip(b).map(|(x, y)| x.0 - y.0).sum();
        let bl: f32 = a.iter().zip(b).map(|(x, y)| x.2 - y.2).sum();
        (r, bl)
    };
    let (sw_red, sw_blue) = gain(&sw_with, &sw_without);
    let (red_gain, blue_gain) = gain(&gpu_with, &gpu_without);
    eprintln!("software red/blue gain {sw_red}/{sw_blue}, gpu {red_gain}/{blue_gain}");
    assert!(
        sw_red > sw_blue * 1.5,
        "fixture: the software mirror reflects the red wall ({sw_red}/{sw_blue})"
    );
    assert!(
        red_gain > 1.0 && red_gain > blue_gain * 1.5,
        "the GPU mirror must reflect the red wall: red gain {red_gain}, blue {blue_gain}"
    );
    let gpu_effect = mean_abs_diff(&gpu_with, &gpu_without);
    assert!(
        gpu_effect > delta_probe * 0.5,
        "the GPU frame must show the probe reflection: GPU delta {gpu_effect} vs software {delta_probe}"
    );
    let backend = mean_abs_diff(&gpu_with, &sw_with);
    // The metal mirror WITHOUT a probe must agree too. The GPU PBR path used to
    // light a metal with the full diffuse ambient (software: `albedo_lin`, i.e.
    // none), which measured 0.0186 here and halved the visible reflection;
    // with the fix it is ~0.002.
    let backend_without = mean_abs_diff(&gpu_without, &sw_without);
    assert!(
        backend_without < 0.006,
        "a metal surface must take no diffuse ambient on the GPU, as in software: \
         backend delta {backend_without}"
    );
    assert!(
        backend < delta_probe / 3.0,
        "GPU probe reflection must match the software reference: backend {backend} vs probe {delta_probe}"
    );
    assert_eq!(
        mean_abs_diff(&gpu_with, &gpu_with_again),
        0.0,
        "a cached Once capture must render the same frame"
    );
}
