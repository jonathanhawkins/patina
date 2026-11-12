//! pat-s3rrv.29 acceptance: the **GPU** backend honours env effects and
//! `GeometryInstance3D` settings, not just the software one.
//!
//! `envfx_nodes_render_from_scene` already proves the producer side and the
//! software consumer. This is the half that shipped broken: the four env-effect
//! methods on `RenderingServer3D` were DEFAULT NO-OPS, `SoftwareRenderer3D`
//! overrode all four and `GpuRenderer3D` overrode none — and `gdeditor`'s
//! default feature is `gpu-render`, with `render_editor_3d_frame_gpu` driving
//! both the editor viewport and the Play frame. So a `.tscn` Decal or FogVolume
//! changed nothing in the shipped editor while the software tests stayed green.
//!
//! # Why this test insists on a real adapter
//!
//! ⚠️ `render_editor_3d_frame_gpu` **falls back to the software path** when no
//! GPU adapter is present. A parity test that ignored that would be comparing
//! software against software: it would pass on a GPU-less machine while proving
//! nothing whatsoever about the GPU backend. So the test probes for an adapter
//! first and skips loudly, rather than reporting a vacuous success.

// The GPU entry point and `GpuRenderer3D` only exist with `gpu-render`, which
// is `gdeditor`'s DEFAULT feature — but a `--no-default-features` build would
// otherwise fail to compile this file rather than simply skipping it.
#![cfg(feature = "gpu-render")]

use gdcore::math::{Color, Vector3};
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, render_editor_3d_frame, render_editor_3d_frame_gpu,
};
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = 64;
    vp.height = 64;
    vp
}

/// Floor + fog volume + decal, and a far cube that `visibility_range_end` culls.
const FX_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Fog" type="FogVolume" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 2)
size = Vector3(4, 4, 4)
density = 0.8

[node name="Splat" type="Decal" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0)
size = Vector3(3, 3, 3)
modulate = Color(1, 0, 0, 1)
albedo_mix = 1.0
"#;

/// The same scene with the effect nodes removed — the control that proves the
/// nodes are what changes the pixels, rather than the two renderers merely
/// agreeing on an unchanged frame.
const BARE_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
"#;

/// Floor + box + sun, rendered through the GPU path — the fixture shape proven
/// by `geometryinstance_cast_shadow_and_visibility_range_consumed`.
///
/// ⚠️ Built programmatically, not from `.tscn`. An unlit box renders black
/// against a black background and is indistinguishable from an absent one, so
/// the light is load-bearing: my first version omitted it and the fixture check
/// below correctly rejected it. No `.tscn` fixture in the repo authors a 3D
/// light, so this mirrors the proven programmatic setup rather than inventing
/// scene-text syntax for one.
///
/// `box_range_end`: `None` draws an unrestricted box, `Some(e)` sets
/// `visibility_range_end = e`, and `with_box = false` omits it entirely.
fn range_scene_gpu(with_box: bool, box_range_end: Option<f32>) -> Vec<Color> {
    // The render list and light collector filter on `is_class(...)`, which reads
    // the ClassDB — without registration nothing is collected and every
    // assertion here would pass vacuously.
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut floor = Node::new("Floor", "MeshInstance3D");
    floor.set_property("mesh", Variant::String("primitive:PlaneMesh".to_string()));
    let floor_id = tree.add_child(root, floor).unwrap();
    node3d::set_position(&mut tree, floor_id, Vector3::ZERO);

    if with_box {
        let mut boxed = Node::new("Box", "MeshInstance3D");
        boxed.set_property("mesh", Variant::String("primitive:BoxMesh".to_string()));
        if let Some(end) = box_range_end {
            boxed.set_property("visibility_range_end", Variant::Float(end as f64));
        }
        let box_id = tree.add_child(root, boxed).unwrap();
        node3d::set_position(&mut tree, box_id, Vector3::new(0.0, 1.0, 0.0));
    }

    // A DirectionalLight3D shining straight down: identity basis points its −Z
    // along −Z, so a −90° turn about X aims it at −Y.
    let sun = tree
        .add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    node3d::set_position(&mut tree, sun, Vector3::new(0.0, 4.0, 0.0));
    node3d::set_rotation(
        &mut tree,
        sun,
        Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0),
    );

    // ⚠️ 96x72 and the box/sun placement are copied verbatim from
    // `geometryinstance_cast_shadow_and_visibility_range_consumed`, which proves
    // this exact framing draws the box. Aspect ratio changes what the camera
    // frames, so inventing my own dimensions here would risk the box falling
    // outside the view — a fixture failure that reads as a feature failure.
    let vp = Viewport3D::new(96, 72);
    let inputs = editor_3d_render_inputs(&vp, &tree);
    bgra_to_rgb(render_editor_3d_frame_gpu(&vp, &inputs))
}

/// Renders through the GPU entry point and returns linear-ish RGB per pixel.
///
/// `Bgra8Image` is byte BGRA; the software path yields float `Color`. Both are
/// reduced to the same representation so a comparison is meaningful.
fn gpu_rgb(tscn: &str) -> Vec<Color> {
    let tree = load(tscn);
    let vp = viewport();
    let inputs = editor_3d_render_inputs(&vp, &tree);
    bgra_to_rgb(render_editor_3d_frame_gpu(&vp, &inputs))
}

/// `Bgra8Image` is byte BGRA; unpack to the same `Color` representation the
/// software path yields so the two are comparable.
fn bgra_to_rgb(img: gdrender2d::renderer::Bgra8Image) -> Vec<Color> {
    img.data
        .chunks_exact(4)
        .map(|px| {
            Color::new(
                px[2] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[0] as f32 / 255.0,
                1.0,
            )
        })
        .collect()
}

/// Renders the same scene through the software entry point, quantised to 8 bits
/// so the two paths are compared on equal terms.
fn software_rgb(tscn: &str) -> Vec<Color> {
    let tree = load(tscn);
    let vp = viewport();
    let fb = render_editor_3d_frame(&vp, &tree);
    fb.pixels
        .iter()
        .map(|c| {
            Color::new(
                (c.r.clamp(0.0, 1.0) * 255.0).round() / 255.0,
                (c.g.clamp(0.0, 1.0) * 255.0).round() / 255.0,
                (c.b.clamp(0.0, 1.0) * 255.0).round() / 255.0,
                1.0,
            )
        })
        .collect()
}

fn differing_pixels(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-4 || (x.g - y.g).abs() > 1e-4 || (x.b - y.b).abs() > 1e-4
        })
        .count()
}

/// Mean absolute per-channel difference — the parity metric. A threshold rather
/// than equality: the two backends rasterise independently, so edge pixels
/// legitimately differ by a quantisation step.
fn mean_abs_diff(a: &[Color], b: &[Color]) -> f32 {
    if a.is_empty() {
        return 0.0;
    }
    let total: f32 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (x.r - y.r).abs() + (x.g - y.g).abs() + (x.b - y.b).abs())
        .sum();
    total / (a.len() as f32 * 3.0)
}

/// Whether a real GPU adapter exists. Without one `render_editor_3d_frame_gpu`
/// silently renders through the software fallback, which would make every
/// assertion below vacuously true.
fn gpu_available() -> bool {
    gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_some()
}

/// **The acceptance test.**
#[test]
fn gpu_env_effects_render_from_scene() {
    if !gpu_available() {
        eprintln!(
            "SKIP gpu_env_effects_render_from_scene: no GPU adapter on this machine. \
             The GPU entry point falls back to software, so running the assertions \
             would compare software against software and pass without testing anything."
        );
        return;
    }

    // -- Link 1: the effects change the GPU frame at all -----------------------
    //
    // This is the bead in one assertion: before it, the GPU backend discarded
    // decals and fog volumes, so these two frames were byte-identical.
    let gpu_fx = gpu_rgb(FX_SCENE);
    let gpu_bare = gpu_rgb(BARE_SCENE);
    let changed = differing_pixels(&gpu_fx, &gpu_bare);
    assert!(
        changed > 0,
        "a .tscn Decal + FogVolume must change the GPU frame; {changed} pixels differed \
         from the effect-free control. Zero means the GPU backend is still dropping \
         add_decal/add_fog_volume on the floor."
    );

    // -- Link 2: and it changes it the SAME way the software backend does ------
    //
    // Guards against "the GPU does something, but something else" — the passes
    // must run in the software renderer's order (decal, then fog), or the
    // frames diverge even though both changed.
    let sw_fx = software_rgb(FX_SCENE);
    let sw_bare = software_rgb(BARE_SCENE);
    assert_eq!(
        gpu_fx.len(),
        sw_fx.len(),
        "both backends must render the same extent"
    );

    // ⚠️ Baseline, then compare — NOT a magic tolerance. The two backends
    // rasterise independently and differ somewhat even on a plain scene; that
    // inherent difference is measured here rather than guessed at. The claim is
    // then relative: adding the effects must not make the backends disagree
    // *more* than they already do. An absolute threshold would encode this
    // machine's rasteriser and fail for reasons unrelated to the feature.
    let baseline = mean_abs_diff(&gpu_bare, &sw_bare);
    let with_fx = mean_abs_diff(&gpu_fx, &sw_fx);
    assert!(
        with_fx <= baseline + 0.05,
        "the effects must land the same way on both backends: without them the \
         backends differ by {baseline:.4} per channel, with them by {with_fx:.4}. \
         A jump here — while link 1 passes — means the GPU ran the passes in the \
         wrong order or with different math, not that it ignored them."
    );

    // -- Link 3: visibility_range_end culls on the GPU path too ----------------
    // 0.5 is the value the software-side test proves culls this exact box at
    // this exact camera distance — not a number I picked.
    let gpu_ranged = range_scene_gpu(true, Some(0.5));
    let gpu_range_control = range_scene_gpu(true, None);
    let gpu_range_empty = range_scene_gpu(false, None);

    // Baseline first: the fixture really does draw a cube when unrestricted.
    // Without this, a fixture that drew nothing would make the culling
    // assertion below pass for entirely the wrong reason.
    let control_drew = differing_pixels(&gpu_range_control, &gpu_range_empty);
    assert!(
        control_drew > 0,
        "fixture check: the unrestricted cube must actually be drawn, else the \
         culling assertion proves nothing. Got {control_drew} differing pixels \
         against an empty scene."
    );

    // The claim: with `visibility_range_end` exceeded, the frame matches the
    // scene where the cube is absent entirely. Equality is a stronger statement
    // than "some pixels differ from the control" — it says the mesh contributed
    // nothing at all, which is what culling means.
    let ranged_vs_empty = differing_pixels(&gpu_ranged, &gpu_range_empty);
    assert_eq!(
        ranged_vs_empty, 0,
        "a mesh beyond visibility_range_end must not be drawn at all, so the frame \
         must be identical to one with no mesh; {ranged_vs_empty} pixels differed. \
         Non-zero means the GPU backend is still ignoring GeometryInstance3D \
         settings and drawing it."
    );
}
