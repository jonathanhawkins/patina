//! pat-h7sz7 on BOTH render paths.
//!
//! 1. Probes are blended PER FRAGMENT. Patina picked one probe per instance
//!    at its origin, so a GridMap wall tile whose origin sat outside every
//!    probe box took the bright environment ambient over its whole surface
//!    while the next tile took the dark probe ambient: the platformer's f318
//!    wall showed a lit band with a hard horizontal edge where Godot's wall
//!    is uniformly dark. Here a wall's origin is outside the probe box but its
//!    right half is inside: only that half may change when the probe is added.
//! 2. Outside every probe a metal reflects the sky radiance map; Patina
//!    reflected black. The control is the same scene with
//!    `reflected_light_source = 1` (Disabled).

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
const H: u32 = 64;

/// A wide wall centred on x = 0, and (optionally) a probe whose box covers
/// only x > 0.5 — so the wall's ORIGIN is outside it.
fn wall_scene(with_probe: bool) -> String {
    let probe = if with_probe {
        r#"
[node name="Probe" type="ReflectionProbe" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 4.25, 0, 0)
size = Vector3(7.5, 6, 6)
blend_distance = 0.0
"#
    } else {
        ""
    };
    format!(
        r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="1"]
size = Vector3(8, 2, 0.2)

[node name="Root" type="Node3D"]

[node name="Wall" type="MeshInstance3D" parent="."]
mesh = SubResource("1")
{probe}"#
    )
}

/// A smooth metal box under a procedural sky, with no probe anywhere, its
/// camera-facing side tilted 30 degrees up so the mirror direction is the sky
/// at 60 degrees elevation (well clear of the horizon line).
fn metal_scene(reflected_light_source: i32) -> String {
    format!(
        r#"[gd_scene format=3]

[sub_resource type="ProceduralSkyMaterial" id="sm"]

[sub_resource type="Sky" id="sky"]
sky_material = SubResource("sm")

[sub_resource type="Environment" id="env"]
background_mode = 2
sky = SubResource("sky")
ambient_light_source = 2
ambient_light_color = Color(0.3, 0.3, 0.3, 1)
reflected_light_source = {reflected_light_source}
tonemap_mode = 0

[sub_resource type="StandardMaterial3D" id="metal"]
metallic = 1.0
roughness = 0.1

[sub_resource type="BoxMesh" id="box"]

[node name="Root" type="Node3D"]

[node name="WorldEnvironment" type="WorldEnvironment" parent="."]
environment = SubResource("env")

[node name="Box" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 0.866025, 0.5, 0, -0.5, 0.866025, 3, 0, 0)
mesh = SubResource("box")
surface_material_override/0 = SubResource("metal")
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

fn viewport(camera: Vector3) -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;
    vp.camera.override_transform = Some(Transform3D::IDENTITY.translated(camera));
    vp.grid.visible = false;
    vp.environment.sky_visible = false;
    vp.environment.sun_enabled = false;
    vp.environment.environment.tone_mapper = ToneMapper::Linear;
    vp.environment.environment.glow_enabled = false;
    vp
}

/// The viewport a running game renders `tree` with: its WorldEnvironment
/// installed as the authored environment (what Play does), else the preview.
fn game_viewport(tree: &SceneTree, camera: Vector3) -> Viewport3D {
    let mut vp = viewport(camera);
    if let Some(env) = gdscene::world_environment::runtime_environment_3d(tree, None) {
        vp.environment.environment = env;
        vp.environment.authored = true;
    }
    vp
}

fn gpu_rgb(tscn: &str, camera: Vector3) -> Vec<[f32; 3]> {
    let tree = load(tscn);
    let vp = game_viewport(&tree, camera);
    let inputs = editor_3d_render_inputs(&vp, &tree);
    let img = render_editor_3d_frame_gpu(&vp, &inputs);
    img.data
        .chunks_exact(4)
        // `Bgra8Image`: blue first.
        .map(|p| {
            [
                p[2] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[0] as f32 / 255.0,
            ]
        })
        .collect()
}

fn software_rgb(tscn: &str, camera: Vector3) -> Vec<[f32; 3]> {
    let tree = load(tscn);
    let fb = render_editor_3d_frame(&game_viewport(&tree, camera), &tree);
    fb.pixels
        .iter()
        .map(|c| {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0;
            [q(c.r), q(c.g), q(c.b)]
        })
        .collect()
}

fn has_gpu() -> bool {
    gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_some()
}

/// Mean |delta| of the two frames over the columns `cols` of the middle row
/// band (where the wall is).
fn band_delta(a: &[[f32; 3]], b: &[[f32; 3]], cols: std::ops::Range<u32>) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in H / 2 - 4..H / 2 + 4 {
        for x in cols.clone() {
            let i = (y * W + x) as usize;
            sum += (0..3).map(|c| (a[i][c] - b[i][c]).abs()).sum::<f32>() / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

fn assert_per_pixel(render: impl Fn(&str, Vector3) -> Vec<[f32; 3]>, path: &str) {
    // The camera at x = 0 sees the wall from x = -2.x to +2.x; the screen
    // centre column is world x = 0, the probe box starts at x = 0.5.
    let cam = Vector3::new(0.0, 0.0, 3.0);
    let without = render(&wall_scene(false), cam);
    let with = render(&wall_scene(true), cam);
    let left = band_delta(&without, &with, 4..W / 2 - 12);
    let right = band_delta(&without, &with, W / 2 + 12..W - 4);
    assert!(
        left < 0.004,
        "{path}: the wall's half OUTSIDE the probe box must not change: {left}"
    );
    assert!(
        right > 0.01,
        "{path}: the wall's half INSIDE the probe box must take the probe's \
         ambient even though the wall's origin is outside the box (per-pixel \
         blending); mean |delta| {right}"
    );
}

#[test]
fn software_probe_applies_per_fragment_not_per_instance_origin() {
    assert_per_pixel(software_rgb, "software");
}

#[test]
fn gpu_probe_applies_per_fragment_not_per_instance_origin() {
    if !has_gpu() {
        eprintln!("SKIP gpu_probe_applies_per_fragment_not_per_instance_origin: no GPU adapter");
        return;
    }
    assert_per_pixel(gpu_rgb, "gpu");
}

/// Mean colour of the centre patch (the box face).
fn centre(px: &[[f32; 3]]) -> [f32; 3] {
    let mut acc = [0.0f32; 3];
    let mut n = 0.0;
    for y in H / 2 - 3..H / 2 + 3 {
        for x in W / 2 - 3..W / 2 + 3 {
            let p = px[(y * W + x) as usize];
            for c in 0..3 {
                acc[c] += p[c];
            }
            n += 1.0;
        }
    }
    acc.map(|v| v / n)
}

fn assert_sky_reflection(render: impl Fn(&str, Vector3) -> Vec<[f32; 3]>, path: &str) -> [f32; 3] {
    let cam = Vector3::new(3.0, 0.0, 3.0);
    let reflects = centre(&render(&metal_scene(0), cam));
    let disabled = centre(&render(&metal_scene(1), cam));
    // Control: a metal has no diffuse and there is no light — with
    // reflections disabled it is near-black. ⚠️ The ambient is NOT disabled:
    // Godot scales the reflection by a specular occlusion taken from the
    // ambient's luminance (`scene_forward_clustered.glsl:2137-2144`), so with
    // no ambient Godot's metal is black too.
    assert!(
        disabled.iter().all(|&c| c < 0.05),
        "{path}: control: a metal with reflected_light_source = Disabled is near-black: {disabled:?} (BG: {reflects:?})"
    );
    assert!(
        reflects[2] > disabled[2] + 0.1 && reflects[2] >= reflects[0],
        "{path}: outside every probe a metal must reflect the sky radiance \
         (the default BG source with a sky background): {reflects:?}"
    );
    reflects
}

#[test]
fn software_metal_reflects_the_sky_outside_every_probe() {
    assert_sky_reflection(software_rgb, "software");
}

#[test]
fn gpu_metal_reflects_the_sky_like_the_software_reference() {
    if !has_gpu() {
        eprintln!("SKIP gpu_metal_reflects_the_sky_like_the_software_reference: no GPU adapter");
        return;
    }
    let gpu = assert_sky_reflection(gpu_rgb, "gpu");
    let sw = assert_sky_reflection(software_rgb, "software");
    for c in 0..3 {
        assert!(
            (gpu[c] - sw[c]).abs() < 0.04,
            "GPU and software must reflect the same sky: {gpu:?} vs {sw:?}"
        );
    }
}
