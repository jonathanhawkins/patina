//! pat-s3rrv.90 — the Light3D property set survives the **Play** path, not just
//! the editor viewport.
//!
//! The bead's wiring requirement names both: `.tscn` Light3D nodes ->
//! `scene_lights`/`sync_light_properties` -> `setup_editor_lighting_3d` ->
//! `render_editor_3d_frame(_gpu)`, *"in both the editor viewport and the Play
//! view"*. The editor half is covered by
//! `light3d_full_property_set_reaches_editor_render`; this is the other half.
//!
//! Play does not re-read the tree. `game_3d_render_inputs` builds the editor
//! snapshot and then subtracts from it — `strip_editor_only_items` drops
//! `editor_only` lights, `strip_cull_masked_items` drops camera-culled meshes.
//! Both are `retain` calls over the same `SceneLight3D` values, so the
//! properties are structurally safe today.
//!
//! ⚠️ **Structurally safe is not tested.** These strippers are the natural place
//! for a future change to start rebuilding or re-deriving the light list, and
//! nothing would fail: the editor test would stay green while Play silently
//! rendered with default biases. That is the regression this file exists to
//! catch, and it is the same shape as the `setup_editor_lighting_3d` drop that
//! made five properties inert for four increments of this bead.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, game_3d_render_inputs, viewport_for_scene_camera, SceneLight3D,
    SceneLightKind,
};
use gdscene::camera3d::set_cull_mask;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 48;

/// Every value here is deliberately NON-default, so an assertion that the Play
/// light matches cannot be satisfied by both sides falling back to the same
/// engine default.
const AUTHORED: &[(&str, f64)] = &[
    ("shadow_opacity", 0.25),
    ("shadow_bias", 0.07),
    ("shadow_normal_bias", 1.75),
    ("light_specular", 0.3),
    ("directional_shadow_max_distance", 55.0),
    ("directional_shadow_fade_start", 0.4),
];

struct Fixture {
    tree: SceneTree,
    camera: NodeId,
}

/// A `MeshInstance3D`, an `OmniLight3D` carrying the authored property set, and a
/// `Camera3D` two units down +Z.
fn fixture(editor_only_light: bool) -> Fixture {
    // The render list and light collector both filter on `is_class(..)`, which
    // reads the ClassDB — without this nothing is collected and every assertion
    // here passes vacuously.
    gdobject::class_db::register_3d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mesh = tree
        .add_child(root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    tree.get_node_mut(mesh)
        .unwrap()
        .set_property("mesh", Variant::String("primitive:BoxMesh".to_string()));

    let light = tree
        .add_child(root, Node::new("Bulb", "OmniLight3D"))
        .unwrap();
    {
        let node = tree.get_node_mut(light).unwrap();
        for (name, value) in AUTHORED {
            node.set_property(name, Variant::Float(*value));
        }
        node.set_property("light_cull_mask", Variant::Int(0b1011));
        node.set_property("shadow_caster_mask", Variant::Int(0b0110));
        node.set_property("light_negative", Variant::Bool(true));
        if editor_only_light {
            node.set_property("editor_only", Variant::Bool(true));
        }
    }

    let camera = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    tree.get_node_mut(camera).unwrap().set_property(
        "transform",
        Variant::Transform3D(Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 2.0),
        }),
    );

    Fixture { tree, camera }
}

fn only_omni(lights: &[SceneLight3D]) -> SceneLight3D {
    *lights
        .iter()
        .find(|l| l.kind == SceneLightKind::Omni)
        .expect("the scene must contribute its OmniLight3D")
}

/// 🔑 EVERY AUTHORED PROPERTY IS IDENTICAL IN THE PLAY LIST AND THE EDITOR LIST.
///
/// Compared field by field against the editor's own snapshot rather than against
/// hardcoded expectations: the point is not "Play has 0.07" but "Play has
/// whatever the editor has", which stays true if a default is ever corrected and
/// fails the moment the two paths diverge.
#[test]
fn authored_light_properties_survive_the_play_render_inputs() {
    let f = fixture(false);
    let viewport = viewport_for_scene_camera(&f.tree, f.camera, W, H);

    let editor = only_omni(&editor_3d_render_inputs(&viewport, &f.tree).scene_lights);
    let play = only_omni(&game_3d_render_inputs(&f.tree, f.camera, &viewport).scene_lights);

    // Guard the comparison itself: if the editor side had silently defaulted,
    // "both sides agree" would be true and meaningless.
    assert_eq!(
        editor.shadow_bias, 0.07,
        "the editor snapshot must carry the AUTHORED bias, or this test only \
         proves two defaults are equal"
    );
    assert!(editor.negative, "and the authored light_negative");

    assert_eq!(play.cull_mask, editor.cull_mask);
    assert_eq!(play.shadow_caster_mask, editor.shadow_caster_mask);
    assert_eq!(play.shadow_opacity, editor.shadow_opacity);
    assert_eq!(play.shadow_bias, editor.shadow_bias);
    assert_eq!(play.shadow_normal_bias, editor.shadow_normal_bias);
    assert_eq!(play.specular, editor.specular);
    assert_eq!(play.negative, editor.negative);
    assert_eq!(
        play.directional_shadow_max_distance,
        editor.directional_shadow_max_distance
    );
    assert_eq!(
        play.directional_shadow_fade_start,
        editor.directional_shadow_fade_start
    );
    assert_eq!(play.temperature, editor.temperature);
    assert_eq!(play.color, editor.color, "temperature tint included");
}

/// 🔑 THE CAMERA'S `cull_mask` STRIPS MESHES AND NEVER LIGHTS.
///
/// ⚠️ Godot's `Camera3D.cull_mask` selects which `VisualInstance3D` layers the
/// camera draws. A light is not drawn, it illuminates — culling it would darken
/// the whole scene rather than hide one object, and the failure would look like
/// a lighting bug rather than a culling one.
///
/// The mesh half is the positive control: without it, "the light survived" is
/// also true of a cull mask that did nothing at all.
#[test]
fn the_cameras_cull_mask_strips_meshes_but_never_lights() {
    // ⚠️ Both runs are PLAY runs, differing only in the cull mask. Comparing
    // the culled Play list against the EDITOR list would also shrink because
    // `strip_editor_only_items` removes the camera and light marker cubes — the
    // assertion would pass with cull masking doing nothing at all.
    let play_with_mask = |mask: i64| {
        let mut f = fixture(false);
        set_cull_mask(&mut f.tree, f.camera, mask);
        let viewport = viewport_for_scene_camera(&f.tree, f.camera, W, H);
        game_3d_render_inputs(&f.tree, f.camera, &viewport)
    };

    // Layer 1 is where the mesh and the light both sit by default.
    let permissive = play_with_mask(0b1);
    let restrictive = play_with_mask(0b10);

    assert!(
        !permissive.items.is_empty(),
        "the permissive run must draw something, or the comparison below is \
         between two empty lists"
    );
    assert!(
        restrictive.items.len() < permissive.items.len(),
        "a cull mask excluding layer 1 must drop drawables: permissive={}, \
         restrictive={}",
        permissive.items.len(),
        restrictive.items.len()
    );
    assert_eq!(
        restrictive.scene_lights.len(),
        permissive.scene_lights.len(),
        "but it must not drop a single light — cull_mask is a VisualInstance \
         filter, and culling lights would darken the scene instead of hiding an \
         object"
    );
    assert_eq!(
        only_omni(&restrictive.scene_lights).shadow_bias,
        0.07,
        "and the surviving light keeps its authored properties"
    );
}

/// 🔑 AN `editor_only` LIGHT REACHES THE EDITOR AND NOT PLAY.
///
/// Godot's `Light3D.editor_only`: the light exists to illuminate the editing
/// viewport and must not ship in the running game. Both halves asserted — the
/// editor half is what separates "Play drops it" from "the scene never
/// contributed it".
#[test]
fn an_editor_only_light_reaches_the_editor_but_not_play() {
    let f = fixture(true);
    let viewport = viewport_for_scene_camera(&f.tree, f.camera, W, H);

    let editor = editor_3d_render_inputs(&viewport, &f.tree);
    assert!(
        editor
            .scene_lights
            .iter()
            .any(|l| l.kind == SceneLightKind::Omni),
        "an editor_only light must still light the EDITOR viewport"
    );

    let play = game_3d_render_inputs(&f.tree, f.camera, &viewport);
    assert!(
        !play
            .scene_lights
            .iter()
            .any(|l| l.kind == SceneLightKind::Omni),
        "but it must not reach a game frame"
    );
}
