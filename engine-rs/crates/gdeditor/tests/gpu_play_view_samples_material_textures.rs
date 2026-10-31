//! pat-mgixk.52 **acceptance**: the editor/Play GPU viewport samples
//! `BaseMaterial3D` textures and runs SSAO.
//!
//! `render_editor_3d_frame_gpu` (`viewport_3d_render.rs:3079`) is the default
//! editor *and* Play renderer, and its WGSL took albedo from a flat uniform,
//! shaded from the interpolated geometric normal, and skipped `apply_ssao_env`
//! entirely. So a textured `StandardMaterial3D`, `Sprite3D` or `Label3D`
//! rendered as a flat colour in the shipped viewport while the software path
//! textured it — this test enters through that exact function, which is the
//! bead's wiring requirement.
//!
//! # How the texture gets in
//!
//! `Render3DItem.material` is a public `Option<Material3D>`, and
//! `populate_editor_scene_3d` (`:2459`) forwards the authored material **whole**
//! to the renderer, texture fields included. So the fixture builds the scene
//! from `.tscn` and then sets the material on the mesh items.
//!
//! ⚠️ The pixels are injected rather than authored in the `.tscn` because a
//! `.tscn` cannot carry a 2x2 image: the editor resolves material textures by
//! *path*, through `MaterialTextureCache::get_or_load`, which reads a file from
//! disk. No fixture in the repo authors one, and writing an image file would
//! test the loader rather than the shader. Everything downstream of
//! `inputs.items` — which is the whole render — is the real path.
//!
//! # Two silent fallbacks this fixture must disarm
//!
//! Both would make every assertion below pass while proving nothing about the
//! GPU, because the software path already implements all three features:
//!
//! 1. `render_editor_3d_frame_gpu` returns the **software** frame when
//!    `render_env.fog_enabled || sky_needs_software` (`:3115`). The editor's
//!    default preview environment installs a procedural `Sky`, so
//!    [`viewport`] sets `sky_visible = false`, which makes
//!    `editor_render_environment` null the sky and the fallback unreachable.
//! 2. The default preview environment is **filmic + glow** (Godot parity).
//!    Glow *bleeds one texel's colour into its neighbour*, which is exactly
//!    what the checker assertions measure, and filmic desaturates highlights.
//!    Both are turned off: this test is about texture sampling, not the preview
//!    post chain.
//!
//! The red check is the backstop for GPU-ness: at HEAD the GPU path returns a
//! flat albedo and an unperturbed normal, so these assertions fail there. Were
//! the test silently running in software it would pass at HEAD too, and be
//! rejected.
//!
//! A third trap of the same family lives in [`load`]: the render list collects
//! only `Node3D`-derived nodes, which is a ClassDB query, so without
//! `register_3d_classes()` the scene yields **no items at all**.
//!
//! # Why the camera is pinned, and the arithmetic behind the SSAO fixture
//!
//! 🔑 `editor_render_viewport` (`:2620`) takes the camera transform **and
//! near/far from the editor camera**, not from the scene's `Camera3D` — a
//! `current = true` camera in the scene text does not frame the view at all.
//! So the fixture sets `camera.override_transform`, the documented bypass of
//! the orbit derivation, and the view is then exactly known: eye at `(0, 0, 3)`
//! looking down `-Z`, `near = 0.05`, vertical fov 70 degrees.
//!
//! That matters because `apply_ssao` only occludes across a stored-depth gap
//! **greater than 0.02**, and stored depth is `z/w` = `1.000025 - 0.1/d` for a
//! surface `d` ahead — strongly nonlinear. ⚠️ The obvious fixture does NOT
//! work: a unit box at the origin with the default camera 5 units out sits at
//! `d ≈ 4.5` (depth 0.9778) against a wall at `d = 15` (depth 0.9934), a gap of
//! **0.0156** — under the threshold, so SSAO would silently not fire and the
//! failure would look like a broken implementation. Here the box front face is
//! at `d = 1.5` (depth 0.9334) and the wall front face at `d = 13` (depth
//! 0.9923): gap **0.0589**, nearly 3x the threshold.
//!
//! Occlusion also darkens the **farther** surface, so the wall has to be lit —
//! a silhouette against a black backdrop multiplies zero and changes nothing.

// The GPU entry point exists only with `gpu-render`, which is `gdeditor`'s
// DEFAULT feature — but a `--no-default-features` build would otherwise fail to
// compile this file rather than simply skipping it.
#![cfg(feature = "gpu-render")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, render_editor_3d_frame_gpu, Editor3DRenderInputs, Render3DKind,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::environment::ToneMapper;
use gdserver3d::material::{Material3D, ShadingMode, TextureImage};
use std::sync::Arc;

const W: u32 = 128;
const H: u32 = 128;

/// A unit box 1.5 ahead of the eye. No `Camera3D` node: the editor camera is
/// what frames the view (see the module docs), and omitting it also removes one
/// gizmo from the frame.
const QUAD_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Quad" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
mesh = "primitive:BoxMesh"
"#;

/// The same box plus a large wall behind it — a depth crease for SSAO. The
/// wall's 40x scale puts its front face at `z = -10` and covers the frame.
const CREASE_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Quad" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)
mesh = "primitive:BoxMesh"

[node name="Wall" type="MeshInstance3D" parent="."]
transform = Transform3D(40, 0, 0, 0, 40, 0, 0, 0, 40, 0, 0, -30)
mesh = "primitive:BoxMesh"
"#;

fn load(tscn: &str) -> SceneTree {
    // ⚠️ ClassDB is NOT populated implicitly, and `build_render_list` collects
    // only **Node3D-derived** nodes — a class-chain query
    // (`viewport_3d.rs:1027`). With an empty registry `is_class("Node3D")` is
    // false for every node, so the render list comes back EMPTY: no mesh item,
    // nothing to texture, and every pixel assertion would be measuring a blank
    // frame. 26 other `gdeditor` tests register for exactly this reason.
    //
    // Omitting this is what made the first version of this test fail, and it
    // failed on the `set_mesh_materials` precondition rather than on a pixel
    // count — which is the only reason the cause was legible.
    gdobject::class_db::register_3d_classes();

    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// The editor viewport, with both software fallbacks disarmed and the camera
/// pinned. See the module docs for why each line is here.
fn viewport(ssao: bool) -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;

    // Eye at (0, 0, 3) looking down -Z, bypassing the orbit derivation.
    vp.camera.override_transform =
        Some(Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, 3.0)));

    // Fallback 1: no sky resource at all, so `sky_needs_software` is false.
    vp.environment.sky_visible = false;
    // The preview sun is the only light once the sky ambient is gone (the
    // scene authors no `DirectionalLight3D`, so `setup_editor_lighting_3d`
    // falls back to it), and both the normal-map and SSAO clauses need lit
    // surfaces. Its direction is assigned straight to `Light3D::direction`
    // (`viewport_3d_render.rs:2516`), so `(0, 0, -1)` lights a +Z-facing
    // surface head-on — the same vector, with the same meaning, as the two
    // already-passing `gdrender3d` fixtures for this bead. Pinned rather than
    // left at the 45-degree default so the lighting is not a variable.
    vp.environment.sun_enabled = true;
    vp.environment.sun_direction = Vector3::new(0.0, 0.0, -1.0);
    vp.environment.sun_energy = 1.0;
    // Fallback 2: glow bleeds colour between texels; filmic desaturates.
    vp.environment.environment.tone_mapper = ToneMapper::Linear;
    vp.environment.environment.glow_enabled = false;
    vp.environment.environment.ssao_enabled = ssao;
    vp.environment.environment.ssao_radius = 1.0;
    vp.environment.environment.ssao_intensity = 2.0;
    vp
}

/// Sets `material` on every mesh item, leaving the editor's own primitive
/// gizmos alone.
fn set_mesh_materials(inputs: &mut Editor3DRenderInputs, material: &Material3D) {
    let mut touched = 0usize;
    for item in inputs.items.iter_mut() {
        if matches!(item.kind, Render3DKind::Mesh { .. }) {
            item.material = Some(material.clone());
            touched += 1;
        }
    }
    assert!(
        touched > 0,
        "fixture precondition: the scene produced no Render3DKind::Mesh item, so \
         there is no surface to texture — check the `primitive:BoxMesh` spelling"
    );
}

/// `render_editor_3d_frame_gpu` returns byte BGRA, top-down and tight.
fn gpu_rgb(tscn: &str, vp: &Viewport3D, material: &Material3D) -> Vec<(f32, f32, f32)> {
    let tree = load(tscn);
    let mut inputs = editor_3d_render_inputs(vp, &tree);
    set_mesh_materials(&mut inputs, material);
    let img = render_editor_3d_frame_gpu(vp, &inputs);
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

/// Whether a real GPU adapter exists. Without one `render_editor_3d_frame_gpu`
/// silently renders through the software fallback, which already implements all
/// three features — so every assertion below would be vacuously true.
fn gpu_available() -> bool {
    gdrender3d::wgpu_pipeline::GpuRenderer3D::new().is_some()
}

/// The bead's 2x2 red/blue checker.
fn checker() -> TextureImage {
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    TextureImage::new(2, 2, vec![red, blue, blue, red])
}

/// A uniform tangent-space tilt of ~45 degrees toward +tangent.
///
/// `r` and `b` are 1.0 so they store as 255 and decode back to exactly 1.0;
/// only `g` is the inexact 0.5 (128/255), where a 0.004 error changes nothing.
/// Choosing 0.5 for `b` would drive the Z term to its `1e-4` floor and make the
/// tilt degenerate.
fn tilt_map() -> TextureImage {
    TextureImage::new(1, 1, vec![Color::new(1.0, 0.5, 1.0, 1.0)])
}

fn is_red(c: (f32, f32, f32)) -> bool {
    c.0 > c.1 + 0.25 && c.0 > c.2 + 0.25
}

fn is_blue(c: (f32, f32, f32)) -> bool {
    c.2 > c.0 + 0.25 && c.2 > c.1 + 0.25
}

fn luma(c: (f32, f32, f32)) -> f32 {
    c.0 + c.1 + c.2
}

/// Bounding box of the checkered surface: every pixel that reads as one of the
/// two texture colours. Derived rather than hard-coded, so the assertions do not
/// also encode a claim about the projection.
fn checker_bounds(px: &[(f32, f32, f32)]) -> (usize, usize, usize, usize) {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for (i, c) in px.iter().enumerate() {
        if is_red(*c) || is_blue(*c) {
            let (x, y) = (i % W as usize, i / W as usize);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0, y0, x1, y1)
}

/// **The acceptance test.**
#[test]
fn gpu_play_view_samples_material_textures() {
    if !gpu_available() {
        eprintln!(
            "SKIP gpu_play_view_samples_material_textures: no GPU adapter on this \
             machine, so `render_editor_3d_frame_gpu` would render in software and \
             every assertion would be vacuous. This is a SKIP, not a pass."
        );
        return;
    }

    // ── Clause 1: the albedo texture reaches the fragment shader ──────────
    //
    // Unlit so the checker's colours pass through without a lighting term:
    // this clause is about sampling, not shading.
    let textured = Material3D {
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        shading_mode: ShadingMode::Unlit,
        albedo_texture: Some(Arc::new(checker())),
        ..Default::default()
    };
    let checker_px = gpu_rgb(QUAD_SCENE, &viewport(false), &textured);
    assert_eq!(checker_px.len(), (W * H) as usize, "unexpected frame size");

    let red = checker_px.iter().filter(|c| is_red(**c)).count();
    let blue = checker_px.iter().filter(|c| is_blue(**c)).count();

    // ⚠️ THIS is the defect. With a flat albedo every pixel of the box face is
    // the same white, so both counts are ZERO however many pixels are drawn.
    // Two colours on one surface can only come from a per-fragment fetch.
    assert!(
        red > 50 && blue > 50,
        "a 2x2 red/blue checker albedo_texture must put BOTH colours on the \
         surface in the Play view: got {red} red-dominant and {blue} \
         blue-dominant pixels. Zero of either means the wgpu pipeline is still \
         taking albedo from the flat model uniform."
    );

    // Structure, not just presence: a 2x2 checker is red/blue on one diagonal
    // and blue/red on the other. Sampling the four quadrant centres of the
    // surface's own bounding box pins that layout without assuming which way
    // the UVs run — flipping V swaps *which* diagonal is red and leaves the
    // diagonal relationship intact. A single-texel fetch for the whole face, or
    // a UV scale that reached only one row, breaks this and not the count above.
    let (x0, y0, x1, y1) = checker_bounds(&checker_px);
    assert!(
        x1 > x0 + 8 && y1 > y0 + 8,
        "the checkered surface must be more than 8px across to have quadrants; \
         got x {x0}..{x1}, y {y0}..{y1}"
    );
    let qx = [x0 + (x1 - x0) / 4, x0 + 3 * (x1 - x0) / 4];
    let qy = [y0 + (y1 - y0) / 4, y0 + 3 * (y1 - y0) / 4];
    let at = |x: usize, y: usize| checker_px[y * W as usize + x];
    let (tl, tr) = (at(qx[0], qy[0]), at(qx[1], qy[0]));
    let (bl, br) = (at(qx[0], qy[1]), at(qx[1], qy[1]));
    let diag_red = is_red(tl) && is_red(br) && is_blue(tr) && is_blue(bl);
    let diag_blue = is_blue(tl) && is_blue(br) && is_red(tr) && is_red(bl);
    assert!(
        diag_red || diag_blue,
        "the four quadrant centres must alternate like a 2x2 checker; got \
         tl={tl:?} tr={tr:?} bl={bl:?} br={br:?}"
    );

    // ── Clause 2: the normal map perturbs the shading normal ──────────────
    //
    // Lambert, because the unlit branch returns before `normal` is ever read —
    // an unlit surface could not observe a normal map at all.
    let flat_lit = Material3D {
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        ..Default::default()
    };
    let normal_mapped = Material3D {
        normal_texture: Some(Arc::new(tilt_map())),
        normal_scale: 1.0,
        ..flat_lit.clone()
    };
    let flat_px = gpu_rgb(QUAD_SCENE, &viewport(false), &flat_lit);
    let mapped_px = gpu_rgb(QUAD_SCENE, &viewport(false), &normal_mapped);

    let lit = flat_px.iter().filter(|c| luma(**c) > 0.15).count();
    assert!(
        lit > 300,
        "fixture precondition: the flat surface must be LIT before a normal map \
         can change its shading; only {lit} pixels above the backdrop. If this \
         fires, the preview sun is not reaching the surface and the clause below \
         would be vacuous."
    );
    let normal_changed = flat_px
        .iter()
        .zip(mapped_px.iter())
        .filter(|(a, b)| (luma(**a) - luma(**b)).abs() > 1.0 / 255.0)
        .count();
    assert!(
        normal_changed > 100,
        "a normal-mapped surface must differ from the flat one: only \
         {normal_changed} of {} pixels differ. Zero means the wgpu pipeline \
         still shades from the interpolated normal and ignores normal_texture.",
        flat_px.len()
    );

    // ── Clause 3: ssao_enabled darkens the crease ─────────────────────────
    //
    // Same scene plus a lit wall behind the box; see the module docs for the
    // depth-gap arithmetic that makes the crease exceed `apply_ssao`'s
    // threshold.
    let off_px = gpu_rgb(CREASE_SCENE, &viewport(false), &flat_lit);
    let on_px = gpu_rgb(CREASE_SCENE, &viewport(true), &flat_lit);

    let wall_lit = off_px.iter().filter(|c| luma(**c) > 0.15).count();
    assert!(
        wall_lit > 2000,
        "fixture precondition: the wall must be lit and fill the frame before \
         occlusion can darken it; only {wall_lit} lit pixels"
    );
    let mut darkened = 0usize;
    let mut brightened = 0usize;
    for (a, b) in off_px.iter().zip(on_px.iter()) {
        let d = luma(*a) - luma(*b);
        if d > 1.0 / 255.0 {
            darkened += 1;
        } else if d < -1.0 / 255.0 {
            brightened += 1;
        }
    }
    assert!(
        darkened > 50,
        "ssao_enabled must darken the crease where the box meets the wall: only \
         {darkened} pixels got darker. Zero means the editor GPU readback still \
         skips apply_ssao_env."
    );
    // Direction: occlusion only ever removes light. Without this, any source of
    // variation between the two renders would satisfy the assertion above.
    assert_eq!(
        brightened, 0,
        "SSAO must only darken; {brightened} pixels got brighter"
    );
}
