//! pat-0q2kx.132 acceptance: `FogVolume` / `Decal` / `ReflectionProbe` /
//! `OccluderInstance3D` scene nodes reach the wired render path.
//!
//! Godot ground truth: `upstream/godot/scene/3d/{fog_volume,decal,
//! reflection_probe,occluder_instance_3d}.cpp`.
//!
//! All four were implemented on the render side — `apply_fog_volume_pass`,
//! `apply_decal_pass`, `is_occluded`, `sample_reflection_probes`, plus a rich
//! type behind each (`gdserver3d` decal.rs 709 lines, fog_volume.rs 880,
//! occluder.rs 788, reflection_probe.rs) — and every one of those passes was
//! already called from `render_frame`. What did not exist was a **producer**:
//! nothing reachable from a scene ever called `add_decal` / `add_fog_volume` /
//! `add_occluder`. Fog volumes were collected into an editor preview list
//! nobody read (and without the node transform, so they sat at the origin);
//! probes only reached a test-only adapter; decals and occluders had zero
//! producers. Authoring any of the four changed not one pixel.
//!
//! ⚠️ Everything here arrives as `.tscn` text. The assertions are on the
//! collected render inputs *and* on rendered pixels, because a snapshot that
//! collects the nodes but never hands them to the renderer is exactly the bug
//! this bead is about.

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, render_editor_3d_frame};
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
    vp.width = 96;
    vp.height = 96;
    vp
}

/// A floor plane under the camera, with a fog volume, a decal and a probe
/// placed away from the origin — the transform is what the old preview list
/// dropped.
const ENVFX_SCENE: &str = r#"[gd_scene format=3]

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

[node name="Probe" type="ReflectionProbe" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 3, 0)
size = Vector3(8, 8, 8)
intensity = 2.0
"#;

/// The same scene with the effect nodes removed — the control that proves the
/// nodes are what changes the pixels.
const BARE_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
"#;

/// A wall occluder between the camera and a cube.
const OCCLUDER_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Hidden" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -6)
mesh = "primitive:BoxMesh"

[node name="Wall" type="OccluderInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -2)
occluder = "BoxOccluder3D"
__occluder__size = Vector3(10, 10, 1)
"#;

/// A fog volume that is hidden — Godot drops a hidden `VisualInstance3D` from
/// the scenario rather than dimming it.
const HIDDEN_FOG_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Fog" type="FogVolume" parent="."]
visible = false
size = Vector3(4, 4, 4)
"#;

/// How many pixels differ between two frames of the same size.
fn differing_pixels(a: &[gdcore::math::Color], b: &[gdcore::math::Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-4 || (x.g - y.g).abs() > 1e-4 || (x.b - y.b).abs() > 1e-4
        })
        .count()
}

/// **The acceptance test.** The four env-effect node classes are collected from
/// `.tscn` text with their transforms, reach the renderer, and change the frame.
#[test]
fn envfx_nodes_render_from_scene() {
    let tree = load(ENVFX_SCENE);
    let vp = viewport();
    let inputs = editor_3d_render_inputs(&vp, &tree);

    // -- (1) collected, with the node transform, not the origin --------------
    assert_eq!(
        inputs.env_effects.fog_volumes.len(),
        1,
        "the `.tscn` FogVolume must reach the render inputs"
    );
    let (volume, fog_tf) = &inputs.env_effects.fog_volumes[0];
    assert_eq!(
        (fog_tf.origin.x, fog_tf.origin.y, fog_tf.origin.z),
        (0.0, 1.0, 2.0),
        "the fog volume must carry its node's global transform — the old preview \
         list built the volume without one, so every fog volume sat at the origin"
    );
    assert!(
        volume.size.x > 3.9 && volume.size.x < 4.1,
        "the authored size must survive the parse, got {:?}",
        volume.size
    );

    assert_eq!(
        inputs.env_effects.decals.len(),
        1,
        "the Decal must reach the render inputs"
    );
    let decal = &inputs.env_effects.decals[0];
    assert_eq!(
        (
            decal.global_transform.origin.x,
            decal.global_transform.origin.z
        ),
        (1.0, 0.0),
        "the decal must be placed at its node transform"
    );
    assert_eq!(
        (decal.modulate.r, decal.modulate.g, decal.modulate.b),
        (1.0, 0.0, 0.0),
        "the authored `modulate` must survive the parse"
    );

    assert_eq!(
        inputs.env_effects.reflection_probes.len(),
        1,
        "the ReflectionProbe must reach the render inputs"
    );
    let probe = &inputs.env_effects.reflection_probes[0];
    assert!(
        (probe.intensity - 2.0).abs() < 1e-4,
        "the authored intensity must survive the parse, got {}",
        probe.intensity
    );
    assert!(
        (probe.transform.origin.y - 3.0).abs() < 1e-4,
        "the probe must be placed at its node transform"
    );

    // -- (2) …and they reach the renderer: pixels change ---------------------
    let with_fx = render_editor_3d_frame(&vp, &tree);
    let bare = render_editor_3d_frame(&vp, &load(BARE_SCENE));
    assert_eq!(
        with_fx.pixels.len(),
        bare.pixels.len(),
        "both frames are the same size"
    );
    let changed = differing_pixels(&with_fx.pixels, &bare.pixels);
    assert!(
        changed > 0,
        "a FogVolume + Decal over the camera path must change *some* pixels; \
         collecting them into a snapshot the renderer never sees is the bug this \
         bead is about"
    );
    // But not the whole frame: an effect that repaints everything is a bug of a
    // different kind, and would pass a naive "something changed" check.
    assert!(
        changed < with_fx.pixels.len(),
        "the effects must be bounded, not repaint every pixel ({changed} of {})",
        with_fx.pixels.len()
    );
}

/// The occluder clause: a wall between the camera and a cube culls it.
#[test]
fn occluder_instance_culls_geometry_behind_it() {
    let vp = viewport();
    let tree = load(OCCLUDER_SCENE);
    let inputs = editor_3d_render_inputs(&vp, &tree);

    assert_eq!(
        inputs.env_effects.occluders.len(),
        1,
        "the `.tscn` OccluderInstance3D must reach the render inputs"
    );
    let occluder = &inputs.env_effects.occluders[0];
    assert!(
        occluder.occluder.is_some(),
        "the occluder's shape must be resolved from the node; a shapeless \
         occluder culls nothing"
    );
    assert!(
        (occluder.transform.origin.z + 2.0).abs() < 1e-4,
        "the occluder must sit at its node transform"
    );

    // The cube behind the wall is culled, so the frame matches one with no cube
    // at all. Comparing against a cube-less scene rather than asserting a raw
    // pixel count keeps this honest about *what* changed.
    // The control keeps the Wall node and drops only the cube, so the sole
    // difference between the two frames is the cube itself. Dropping the Wall
    // too would also drop whatever the editor draws for that node, and the
    // comparison would be measuring the marker instead of the culling.
    let with_wall = render_editor_3d_frame(&vp, &tree);
    let no_cube = render_editor_3d_frame(
        &vp,
        &load(
            r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Wall" type="OccluderInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -2)
occluder = "BoxOccluder3D"
__occluder__size = Vector3(10, 10, 1)
"#,
        ),
    );
    assert_eq!(
        differing_pixels(&with_wall.pixels, &no_cube.pixels),
        0,
        "a cube fully behind a wall occluder must not be drawn, so the frame \
         must match a scene with no cube at all"
    );
}

/// A hidden effect node contributes nothing — Godot drops a hidden
/// `VisualInstance3D` from the scenario rather than merely dimming it.
#[test]
fn hidden_env_effect_nodes_are_skipped() {
    let tree = load(HIDDEN_FOG_SCENE);
    let inputs = editor_3d_render_inputs(&viewport(), &tree);
    assert!(
        inputs.env_effects.fog_volumes.is_empty(),
        "a hidden FogVolume must not reach the renderer, got {:?}",
        inputs.env_effects.fog_volumes
    );
}

/// The Play path shares the same snapshot builder, so the effects must reach a
/// game frame too — not just the editor viewport.
#[test]
fn env_effects_reach_the_play_path() {
    let tree = load(ENVFX_SCENE);
    let camera = tree
        .get_node_by_path("/root/Root/Camera")
        .expect("the scene has a camera");
    let vp = viewport();
    let inputs = gdeditor::viewport_3d_render::game_3d_render_inputs(&tree, camera, &vp);
    assert_eq!(
        inputs.env_effects.fog_volumes.len(),
        1,
        "the Play path must carry the scene's fog volumes"
    );
    assert_eq!(
        inputs.env_effects.decals.len(),
        1,
        "the Play path must carry the scene's decals"
    );
}
