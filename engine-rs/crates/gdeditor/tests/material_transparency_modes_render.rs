//! pat-s3rrv.30 acceptance: `StandardMaterial3D`'s transparency, alpha-scissor
//! and cull modes reach pixels — in BOTH renderers.
//!
//! `gdrender3d/src/render_modes.rs` has carried `CullMode`, `Transparency`,
//! `should_cull`, `alpha_scissor_discard` and `writes_depth` since pat-u3l8c.34
//! with **zero consumers**, and `StandardMaterial3D` had no field to feed them:
//! the `.tscn` parser read `cull_mode` only as `matches!(.., Int(2))` — enough
//! to say "cull nothing", not "cull the front faces" — and never looked at
//! `transparency` at all. So an authored alpha or front-culled material
//! rendered exactly as if it had said nothing.
//!
//! Each case is asserted against a control that differs only in the property
//! under test, so a failure means the property was ignored rather than that the
//! fixture drew nothing.

#![cfg(feature = "gpu-render")]

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, render_editor_3d_frame, render_editor_3d_frame_gpu,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 64;
const H: u32 = 64;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    Viewport3D::new(W, H)
}

/// Software frame as linear `Color`s.
fn software_rgb(tscn: &str) -> Vec<Color> {
    render_editor_3d_frame(&viewport(), &load(tscn)).pixels
}

/// GPU frame, unpacked from BGRA bytes to the same representation.
fn gpu_rgb(tscn: &str) -> Vec<Color> {
    let tree = load(tscn);
    let vp = viewport();
    let inputs = editor_3d_render_inputs(&vp, &tree);
    render_editor_3d_frame_gpu(&vp, &inputs)
        .data
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

/// ⚠️ The GPU entry point falls back to software when there is no adapter, so
/// without this the GPU half of every assertion below would silently be a
/// second software run — passing while testing nothing.
fn gpu_available() -> bool {
    gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_some()
}

fn differing(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-3 || (x.g - y.g).abs() > 1e-3 || (x.b - y.b).abs() > 1e-3
        })
        .count()
}

/// A red quad in front of a green one, with `transparency` and albedo alpha
/// substituted in. `1` = Alpha (blend), `0` = Disabled (opaque control).
fn two_quads(transparency: i32, near_alpha: f32) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="StandardMaterial3D" id="Near_1"]
albedo_color = Color(1, 0, 0, {near_alpha})
transparency = {transparency}

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Far" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -2)
mesh = "primitive:PlaneMesh"

[node name="Near" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
mesh = "primitive:PlaneMesh"
material = SubResource("Near_1")
"#
    )
}

/// One quad whose material is AlphaScissor with the given albedo alpha; a
/// sub-threshold alpha must discard it entirely.
fn scissor_quad(alpha: f32) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="StandardMaterial3D" id="Cut_1"]
albedo_color = Color(1, 0, 0, {alpha})
transparency = 2
alpha_scissor_threshold = 0.5

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Quad" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
material = SubResource("Cut_1")
"#
    )
}

/// An empty scene — what "the quad was discarded" must look like.
const EMPTY_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true
"#;

/// A cube with the given `cull_mode`.
fn culled_cube(cull_mode: i32) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="StandardMaterial3D" id="Cull_1"]
albedo_color = Color(1, 0, 0, 1)
cull_mode = {cull_mode}

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Cube" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"
material = SubResource("Cull_1")
"#
    )
}

/// Runs one case through both renderers, so neither backend can pass alone.
fn both(tscn: &str, gpu: bool) -> Vec<(&'static str, Vec<Color>)> {
    let mut out = vec![("software", software_rgb(tscn))];
    if gpu {
        out.push(("gpu", gpu_rgb(tscn)));
    }
    out
}

/// **The acceptance test.**
#[test]
fn material_transparency_modes_render() {
    let gpu = gpu_available();
    if !gpu {
        eprintln!(
            "NOTE: no GPU adapter — the wgpu half is skipped. The software half \
             still runs and is asserted in full."
        );
    }

    // -- Case 1: transparency = Alpha blends the far surface through ----------
    //
    // Compared against the SAME scene with transparency disabled, so the only
    // difference is the mode under test.
    for (backend, blended) in both(&two_quads(1, 0.5), gpu) {
        let opaque = if backend == "software" {
            software_rgb(&two_quads(0, 1.0))
        } else {
            gpu_rgb(&two_quads(0, 1.0))
        };
        let changed = differing(&blended, &opaque);
        assert!(
            changed > 0,
            "[{backend}] an alpha-blended near quad must not render identically \
             to an opaque one — {changed} pixels differed. Zero means \
             `transparency` never reached the renderer."
        );
    }

    // -- Case 2: AlphaScissor discards sub-threshold texels -------------------
    for (backend, cut) in both(&scissor_quad(0.2), gpu) {
        let (kept, empty) = if backend == "software" {
            (software_rgb(&scissor_quad(0.9)), software_rgb(EMPTY_SCENE))
        } else {
            (gpu_rgb(&scissor_quad(0.9)), gpu_rgb(EMPTY_SCENE))
        };
        // Baseline first: an above-threshold quad really is drawn, so a failure
        // below means the scissor is broken rather than the fixture empty.
        assert!(
            differing(&kept, &empty) > 0,
            "[{backend}] fixture check: an above-threshold AlphaScissor quad must \
             be drawn, else the discard assertion proves nothing"
        );
        assert_eq!(
            differing(&cut, &empty),
            0,
            "[{backend}] alpha 0.2 under a 0.5 scissor threshold must discard the \
             surface entirely, leaving the background"
        );
    }

    // -- Case 3: cull_mode = Front shows the far side of the cube -------------
    for (backend, front) in both(&culled_cube(1), gpu) {
        let back = if backend == "software" {
            software_rgb(&culled_cube(0))
        } else {
            gpu_rgb(&culled_cube(0))
        };
        let changed = differing(&front, &back);
        assert!(
            changed > 0,
            "[{backend}] cull_mode = Front must render differently from the \
             default Back cull — {changed} pixels differed. Zero means cull_mode \
             is still collapsed to the old `double_sided` boolean, which could \
             not express Front at all."
        );
    }
}
