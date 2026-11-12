//! pat-s3rrv.114 — a `VoxelGI` node in a `.tscn` reaches the render path.
//!
//! Godot ground truth: `upstream/godot/scene/3d/voxel_gi.cpp`,
//! `doc/classes/VoxelGI.xml` — `size` defaults to `Vector3(20, 20, 20)` and
//! `subdiv` to **1** (128³).
//!
//! ## ⚠️ The gap was the PRODUCER, not the shading
//!
//! `gdserver3d::gi` is a 962-line module with a full `VoxelGI` type and
//! `sample_voxel_gi_at`, and **both** renderers already carried a `voxel_gis`
//! list plus `add_voxel_gi`/`update_voxel_gi` on the `RenderingServer3D` trait.
//! What did not exist was anything that read a `VoxelGI` node out of the scene
//! tree: `scene_env_effects` collected FogVolume, Decal, ReflectionProbe and
//! OccluderInstance3D and skipped this one. So the engine could shade GI it was
//! never given, and authoring a VoxelGI changed no pixel.
//!
//! That shape — a complete subsystem with no caller — is why the assertions
//! below are about **carriage**: that the node's authored values arrive at the
//! renderer's list, which is precisely the link that was absent.

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::editor_3d_render_inputs;
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
    vp.width = 64;
    vp.height = 64;
    vp
}

/// ⚠️ `size` and `subdiv` are authored AWAY from their defaults on purpose.
/// `VoxelGI::new` already seeds 20×20×20 and Subdiv128, so a collector that
/// built the volume and never read the node would still produce those values —
/// and an assertion against the defaults would pass on exactly the bug this
/// bead is about.
const GI_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 6)

[node name="GI" type="VoxelGI" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 3, 4, 5)
size = Vector3(8, 6, 4)
subdiv = 2
energy = 2.5
propagation = 0.25
interior = true
"#;

/// The same scene with the GI node removed — the control.
const NO_GI_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 6)
"#;

/// **The acceptance.** A `VoxelGI` node is collected with its authored values.
#[test]
fn voxelgi_lightmap_scene_render() {
    let tree = load(GI_SCENE);
    let inputs = editor_3d_render_inputs(&viewport(), &tree);
    let gis = &inputs.env_effects.voxel_gis;

    assert_eq!(
        gis.len(),
        1,
        "the scene's VoxelGI node must reach the render inputs; got {} volumes",
        gis.len()
    );
    let gi = &gis[0];

    // Authored, not defaulted — see the note on GI_SCENE.
    assert_eq!(
        gi.size,
        gdcore::math::Vector3::new(8.0, 6.0, 4.0),
        "the node's authored `size` must be read, not VoxelGI::new's 20x20x20"
    );
    // ⚠️ `subdiv = 2` is Subdiv256. Godot's DEFAULT is 1 (128³), not the zeroth
    // constant, so a collector defaulting a missing property to zero would pick
    // Subdiv64 — a plausible value that is wrong in both directions.
    assert_eq!(
        gi.subdiv,
        gdserver3d::gi::VoxelGISubdiv::Subdiv256,
        "subdiv = 2 is Subdiv256"
    );
    assert_eq!(gi.energy, 2.5, "authored energy must survive");
    assert_eq!(gi.propagation, 0.25, "authored propagation must survive");
    assert!(gi.interior, "authored interior flag must survive");

    // The transform is the half a preview-style collector drops: it builds the
    // volume but leaves it at the origin, so the GI lights the wrong region.
    assert_eq!(
        gi.transform.origin,
        gdcore::math::Vector3::new(3.0, 4.0, 5.0),
        "the node's global transform must come across, or the volume sits at \
         the origin however it was placed"
    );
}

/// The control: no `VoxelGI` node, no volumes.
///
/// Without this, a collector that pushed a volume unconditionally — or one
/// seeded with a default entry — would satisfy every assertion above.
#[test]
fn voxelgi_lightmap_scene_render_absent_node_collects_nothing() {
    let tree = load(NO_GI_SCENE);
    let inputs = editor_3d_render_inputs(&viewport(), &tree);
    assert!(
        inputs.env_effects.voxel_gis.is_empty(),
        "a scene with no VoxelGI must collect none; got {}",
        inputs.env_effects.voxel_gis.len()
    );
}

/// A `VoxelGI` must not be mistaken for one of the other effect families.
///
/// ⚠️ The collector's branch chain ends in an `else` that builds an
/// `OccluderInstance3D`. Adding `VoxelGI` to the guard without giving it its own
/// arm would send every GI node down that final branch — collected, but as an
/// occluder, silently changing what the scene occludes.
#[test]
fn voxelgi_lightmap_scene_render_does_not_leak_into_other_effect_lists() {
    let tree = load(GI_SCENE);
    let fx = editor_3d_render_inputs(&viewport(), &tree).env_effects;

    assert_eq!(fx.voxel_gis.len(), 1, "precondition: the GI was collected");
    assert!(
        fx.occluders.is_empty(),
        "a VoxelGI must not fall through to the occluder branch; got {} \
         occluders",
        fx.occluders.len()
    );
    assert!(fx.decals.is_empty(), "nor become a decal");
    assert!(fx.reflection_probes.is_empty(), "nor a reflection probe");
    assert!(fx.fog_volumes.is_empty(), "nor a fog volume");
}
