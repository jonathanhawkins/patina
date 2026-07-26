//! pat-0q2kx.110: OmniLight3D / SpotLight3D reach the wired render path.
//!
//! Godot shades with all three `Light3D` subclasses. Patina's renderer supported
//! Point and Spot all along, but the **product** extraction gathered only
//! `DirectionalLight3D` — so an omni or spot light in a scene emitted nothing
//! and showed up solely as an editor marker cube.
//!
//! The collection half was fixed by pat-0q2kx.41 (`scene_lights`). What was
//! still missing, and what this file adds, is the bead's own gate plus
//! `Light3D.editor_only`: a light flagged editor-only must light the **editor
//! viewport** and contribute nothing to a **game** frame. Those two paths share
//! one `Editor3DRenderInputs` snapshot, so getting it wrong in either direction
//! is invisible without a test that checks both.

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, strip_editor_only_items, SceneLightKind,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// An omni above a plane and a spot aimed at a cube, with **no** directional
/// light — so anything lit is lit by the positional lights alone.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Cube" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 0, 0)
mesh = "primitive:BoxMesh"

[node name="Omni" type="OmniLight3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 3, 0)
omni_range = 12.0
light_energy = 2.0

[node name="Spot" type="SpotLight3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 0)
spot_range = 10.0
spot_angle = 35.0
"#;

fn inputs_for(tscn: &str) -> gdeditor::viewport_3d_render::Editor3DRenderInputs {
    let tree = load(tscn);
    let viewport = Viewport3D::default();
    editor_3d_render_inputs(&viewport, &tree)
}

/// **The acceptance test.** Omni and spot reach the render path with their own
/// range/cone, and `editor_only` splits editor from Play.
#[test]
fn omni_spot_light_scene_render() {
    let inputs = inputs_for(SCENE);

    // -- (1) both positional lights are collected, with no directional --------
    let omni: Vec<_> = inputs
        .scene_lights
        .iter()
        .filter(|l| l.kind == SceneLightKind::Omni)
        .collect();
    let spot: Vec<_> = inputs
        .scene_lights
        .iter()
        .filter(|l| l.kind == SceneLightKind::Spot)
        .collect();

    assert_eq!(
        omni.len(),
        1,
        "the OmniLight3D must reach the wired render path; zero means \
         extraction still skips everything but DirectionalLight3D"
    );
    assert_eq!(spot.len(), 1, "the SpotLight3D must reach it too");
    assert!(
        !inputs
            .scene_lights
            .iter()
            .any(|l| l.kind == SceneLightKind::Directional),
        "this scene has no DirectionalLight3D, so anything lit is lit by the \
         positional lights alone"
    );

    // -- (2) their range/cone travel with them, not defaults -----------------
    //
    // A collector that produced the right *kind* but dropped the falloff would
    // still pass (1); these pin the values the shading actually uses.
    assert!(
        (omni[0].range - 12.0).abs() < 1e-3,
        "omni_range must travel to the render path, got {}",
        omni[0].range
    );
    assert!(
        spot[0].range > 0.0 && spot[0].spot_angle > 0.0,
        "spot_range and spot_angle must travel with the spot light, got \
         range={} angle={}",
        spot[0].range,
        spot[0].spot_angle
    );
    assert!(
        (omni[0].energy - 2.0).abs() < 1e-3,
        "light_energy must travel, got {}",
        omni[0].energy
    );

    // -- (3) editor_only lights the editor but not Play ----------------------
    let editor_only_scene = SCENE.replace(
        "[node name=\"Omni\" type=\"OmniLight3D\" parent=\".\"]",
        "[node name=\"Omni\" type=\"OmniLight3D\" parent=\".\"]\neditor_only = true",
    );
    let mut inputs = inputs_for(&editor_only_scene);

    assert!(
        inputs
            .scene_lights
            .iter()
            .any(|l| l.kind == SceneLightKind::Omni),
        "an editor_only light must still light the EDITOR viewport"
    );

    // The Play path strips it.
    strip_editor_only_items(&mut inputs);
    assert!(
        !inputs
            .scene_lights
            .iter()
            .any(|l| l.kind == SceneLightKind::Omni),
        "an editor_only light must contribute nothing to a game frame"
    );
    assert!(
        inputs
            .scene_lights
            .iter()
            .any(|l| l.kind == SceneLightKind::Spot),
        "stripping editor_only lights must not remove the ordinary ones — the \
         spot has no editor_only flag and must survive"
    );
}

/// Without `editor_only`, the Play path keeps every light. This is the mirror
/// that stops "strip all lights" from passing the acceptance case.
#[test]
fn play_keeps_ordinary_positional_lights() {
    let mut inputs = inputs_for(SCENE);
    let before = inputs.scene_lights.len();
    assert!(before >= 2, "the scene contributes two positional lights");

    strip_editor_only_items(&mut inputs);
    assert_eq!(
        inputs.scene_lights.len(),
        before,
        "no light is editor_only here, so the Play path must keep all of them"
    );
}

/// A scene with no lights at all collects none — so the counts above come from
/// the nodes, not from a preview/fallback light quietly injected downstream.
#[test]
fn a_scene_without_lights_collects_none() {
    let inputs = inputs_for(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Cube" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"
"#,
    );
    assert!(
        inputs.scene_lights.is_empty(),
        "a scene with no Light3D nodes must collect no scene lights, got {}",
        inputs.scene_lights.len()
    );
}
