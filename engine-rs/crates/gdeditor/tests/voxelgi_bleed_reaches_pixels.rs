//! pat-s3rrv.114 — a `VoxelGI` actually changes PIXELS.
//!
//! # Why this file exists next to `voxelgi_lightmap_scene_render`
//!
//! That suite is the bead's named acceptance and it passes — but it samples
//! **zero pixels**. Every assertion in it is of the form "the authored `energy`
//! survived into `fx.voxel_gis`", which proves the *collector* runs. The bead
//! asks for something else entirely: *"renders visible colour bleed onto the
//! white wall (its pixels shift toward red)"*.
//!
//! Both are worth having, so this adds the pixel half rather than rewriting the
//! collection half.
//!
//! # The bug it caught
//!
//! Every link looked wired — collector, `voxel_gis` on both renderers, and the
//! per-pixel `sample_voxel_gi_at` modulation in `gdrender3d`. But **nothing
//! baked the volumes**, and an unbaked volume samples pure black, which the
//! shading pass explicitly skips. So a VoxelGI changed no pixel while every
//! individual piece was present and green.
//!
//! ⚠️ That is why this test asserts on the FRAME and not on any intermediate
//! list. A list assertion cannot distinguish "collected and baked and shaded"
//! from "collected"; it was a green list assertion that hid this for two passes.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = 48;
    vp.height = 48;
    vp
}

/// A red surface beside a white one, lit, with a VoxelGI covering both.
///
/// ⚠️ The `size` is authored large enough to enclose both meshes. A volume that
/// does not contain the sampled fragment returns black and is skipped, which
/// would read as "the bake did nothing" rather than "the fragment was outside".
const WITH_GI: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Sun" type="DirectionalLight3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 4, 2)
light_energy = 1.5

[node name="RedWall" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, -1.2, 0, 0)
mesh = "primitive:BoxMesh"

[node name="WhiteWall" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1.2, 0, 0)
mesh = "primitive:BoxMesh"

[node name="GI" type="VoxelGI" parent="."]
size = Vector3(8, 8, 8)
energy = 2.0
"#;

/// Identical, minus the VoxelGI node.
fn without_gi() -> String {
    let cut = WITH_GI
        .find("[node name=\"GI\"")
        .expect("fixture has a GI node");
    WITH_GI[..cut].to_string()
}

/// Mean redness bias of a frame: how far red runs ahead of the other channels.
///
/// A single scalar rather than a per-pixel comparison because the bounce is
/// diffuse and low-amplitude — asserting on one pixel would pin the sampling
/// geometry rather than the effect.
fn red_bias(pixels: &[Color]) -> f32 {
    if pixels.is_empty() {
        return 0.0;
    }
    let sum: f32 = pixels.iter().map(|p| p.r - 0.5 * (p.g + p.b)).sum();
    sum / pixels.len() as f32
}

// ===========================================================================

/// **The acceptance's pixel half.** A VoxelGI in the scene tints the frame
/// redder than the same scene without it.
#[test]
fn voxelgi_bleed_reaches_pixels() {
    let vp = viewport();

    // The control is rendered FIRST and its value captured, so "redder" is
    // measured against a real frame rather than an assumed one.
    let plain = render_editor_3d_frame(&vp, &load(&without_gi()));
    let lit = render_editor_3d_frame(&vp, &load(WITH_GI));

    assert!(
        !plain.pixels.is_empty() && plain.pixels.len() == lit.pixels.len(),
        "both frames must have the same, non-zero pixel count: {} vs {}",
        plain.pixels.len(),
        lit.pixels.len()
    );

    let before = red_bias(&plain.pixels);
    let after = red_bias(&lit.pixels);

    // ⚠️ Strictly greater, with a margin: an unbaked volume samples black and
    // the shading pass skips it, so the two frames come back BYTE-IDENTICAL —
    // which is exactly the bug this test exists to catch, and it would satisfy
    // any `>=` comparison.
    assert!(
        after > before + 1e-4,
        "a VoxelGI covering a red surface must bleed red into the frame: \
         red bias without GI = {before}, with GI = {after}. Equal values mean \
         the volume contributed nothing — most likely it was never baked, since \
         an unbaked volume samples black and the shading pass skips black"
    );
}

/// Removing the node removes the effect.
///
/// The other direction, and the one that catches a frame that is redder for
/// some reason unrelated to the GI — a differently-seeded renderer, say.
#[test]
fn voxelgi_bleed_reaches_pixels_absent_node_leaves_the_frame_alone() {
    let vp = viewport();
    let a = render_editor_3d_frame(&vp, &load(&without_gi()));
    let b = render_editor_3d_frame(&vp, &load(&without_gi()));

    assert_eq!(
        a.pixels, b.pixels,
        "the same GI-less scene must render identically twice — if it does not, \
         the comparison in the acceptance is measuring renderer noise"
    );
}
