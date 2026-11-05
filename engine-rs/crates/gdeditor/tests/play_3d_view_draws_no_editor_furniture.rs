//! pat-p3p1m: Play on a 3D scene must never draw editor furniture.
//!
//! The official 3D platformer demo, played while its Player (which carries the
//! game's Camera3D) failed to instance, showed the EDITOR's grid, red/blue
//! origin axes and cyan collision/selection wireframes in the Game tab. The
//! render loop fell back to a clone of the editor viewport + its editor render
//! inputs whenever the running tree had no current Camera3D.
//!
//! Godot draws no World3D without a camera: the viewport is just the project's
//! `default_clear_color`. With a camera it renders through that camera.
//!
//! These drive the same entry point the editor binary's Play loop calls
//! (`play_3d::prepare_play_3d_frame` -> `PreparedPlay3DFrame::render`).

use gdcore::math::Color;
use gdeditor::play_3d::{prepare_play_3d_frame, Play3DScene};
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::Render3DKind;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const W: u32 = 128;
const H: u32 = 96;

/// A mesh, a collision shape (cyan wireframe in the editor) and an Area3D (an
/// editor marker cube) — everything the editor viewport decorates — and NO
/// camera.
const NO_CAMERA: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="BoxShape3D" id="Box_1"]
size = Vector3(1, 1, 1)

[node name="Game" type="Node3D"]

[node name="Coin" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"

[node name="Area" type="Area3D" parent="."]

[node name="Shape" type="CollisionShape3D" parent="Area"]
shape = SubResource("Box_1")
"#;

/// The same scene with a current Camera3D looking at the coin from +Z.
fn with_camera(z: f32) -> String {
    format!(
        "{NO_CAMERA}\n[node name=\"Cam\" type=\"Camera3D\" parent=\".\"]\ntransform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, {z})\ncurrent = true\n"
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

fn node_id(tree: &SceneTree, name: &str) -> u64 {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|&id| tree.get_node(id).map(|n| n.name() == name).unwrap_or(false))
        .unwrap_or_else(|| panic!("no node named {name}"))
        .raw()
}

/// The editor viewport as the designer left it: grid + axes on, orbit camera
/// framing the origin, and the coin and its collision shape SELECTED (so a
/// leak would also draw selection outlines and the move gizmo).
fn editor_viewport(tree: &SceneTree) -> Viewport3D {
    let mut vp = Viewport3D::new(W, H);
    assert!(vp.grid.visible, "fixture: the editor grid starts visible");
    vp.selection.select(node_id(tree, "Coin"));
    vp.selection.select(node_id(tree, "Shape"));
    vp
}

const CLEAR: Color = Color::new(0.3, 0.3, 0.3, 1.0);

#[test]
fn a_3d_scene_with_no_camera_plays_as_the_clear_colour_only() {
    let tree = load(NO_CAMERA);
    let editor_vp = editor_viewport(&tree);
    let frame = prepare_play_3d_frame(&tree, &editor_vp, false, CLEAR, W, H);

    // Whole frame: every pixel is the clear colour (0.3 -> 77), so no grid
    // line, axis, collision wireframe, selection box or mesh got drawn.
    let img = frame.render();
    assert_eq!((img.width, img.height), (W, H));
    let expected = [77u8, 77, 77, 255];
    let off: Vec<usize> = img
        .data
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, px)| *px != expected)
        .map(|(i, _)| i)
        .collect();
    assert!(
        off.is_empty(),
        "{} of {} pixels are not the clear colour (first at {:?}: {:?}) — editor furniture leaked into Play",
        off.len(),
        W * H,
        off.first().map(|i| (i % W as usize, i / W as usize)),
        off.first().map(|i| &img.data[i * 4..i * 4 + 4]),
    );
    assert!(
        matches!(frame.scene, Play3DScene::NoCamera { .. }),
        "no current Camera3D must not fall back to the editor camera"
    );
    assert!(!frame.draws_world());
}

#[test]
fn the_clear_colour_is_the_projects() {
    let tree = load(NO_CAMERA);
    let editor_vp = editor_viewport(&tree);
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let img = prepare_play_3d_frame(&tree, &editor_vp, false, red, W, H).render();
    assert!(img.data.chunks_exact(4).all(|px| px == [0, 0, 255, 255]));
}

#[test]
fn a_3d_scene_with_a_camera_renders_through_it_without_editor_items() {
    let tree = load(&with_camera(3.0));
    let editor_vp = editor_viewport(&tree);
    let frame = prepare_play_3d_frame(&tree, &editor_vp, false, CLEAR, W, H);

    let (vp, inputs) = match &frame.scene {
        Play3DScene::Camera { vp, inputs } => (vp, inputs),
        _ => panic!("a current Camera3D must frame Play"),
    };
    // Through the scene camera, not the editor orbit camera.
    let cam = vp.camera.override_transform.expect("pinned to the Camera3D");
    assert_eq!(cam.origin, gdcore::math::Vector3::new(0.0, 0.0, 3.0));
    // No editor furniture in the pipeline's inputs.
    assert!(!vp.grid.visible, "grid must be off in Play");
    assert!(!vp.show_origin, "origin axes must be off in Play");
    assert!(vp.selection.selected_nodes.is_empty(), "no selection outline in Play");
    let coin = node_id(&tree, "Coin");
    assert!(
        inputs.items.iter().any(|i| i.node_id == coin),
        "the coin mesh is drawn"
    );
    for item in &inputs.items {
        assert!(
            !matches!(
                item.kind,
                Render3DKind::Primitive { .. } | Render3DKind::Collision { .. }
            ),
            "editor-only item in a Play frame: node {} {:?}",
            item.node_id,
            std::mem::discriminant(&item.kind)
        );
    }

    // Whole-frame differential: the coin is visible (the frame differs from the
    // same camera with the coin hidden), and moving the CAMERA moves the frame
    // (it is the scene camera that frames it).
    let near = frame.render();
    let mut hidden = load(&with_camera(3.0));
    let coin_id = gdscene::node::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(
        node_id(&hidden, "Coin"),
    ));
    hidden
        .get_node_mut(coin_id)
        .unwrap()
        .set_property("visible", gdvariant::Variant::Bool(false));
    let empty = prepare_play_3d_frame(&hidden, &editor_viewport(&hidden), false, CLEAR, W, H)
        .render();
    let far_tree = load(&with_camera(8.0));
    let far = prepare_play_3d_frame(&far_tree, &editor_viewport(&far_tree), false, CLEAR, W, H)
        .render();
    let diff = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    assert!(diff(&near.data, &empty.data) > 400, "the coin is drawn through the camera");
    assert!(diff(&near.data, &far.data) > 400, "the Camera3D's transform frames the view");

    // With the coin hidden, what is left is the background only: no grid, no
    // wireframe, no marker. A camera-only scene (nothing else 3D) renders the
    // same bytes.
    let bare = load(
        "[gd_scene format=3]\n\n[node name=\"Game\" type=\"Node3D\"]\n\n[node name=\"Cam\" type=\"Camera3D\" parent=\".\"]\ntransform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 3)\ncurrent = true\n",
    );
    let bare_img = prepare_play_3d_frame(&bare, &editor_viewport(&hidden), false, CLEAR, W, H)
        .render();
    assert_eq!(
        diff(&empty.data, &bare_img.data),
        0,
        "the camera frame of a scene whose only drawable is hidden must be the bare background"
    );
}

#[test]
fn the_camera_override_keeps_the_editor_camera() {
    let tree = load(&with_camera(3.0));
    let editor_vp = editor_viewport(&tree);
    let frame = prepare_play_3d_frame(&tree, &editor_vp, true, CLEAR, W, H);
    assert!(matches!(frame.scene, Play3DScene::EditorCamera { .. }));
}
