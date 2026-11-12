//! pat-0q2kx.30 — Play/game view: a running 3D scene is framed by its **own**
//! current `Camera3D` and drawn with a game-mode render list — no editor grid,
//! origin lines, or orange camera/light marker cubes, and the camera's `cull_mask`
//! applied.
//!
//! Before this, Play cloned the editor's orbit viewport, so pressing Play on a 3D
//! scene showed the editor's viewpoint plus its furniture instead of the game.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, game_3d_render_inputs, game_camera_for_play, render_editor_3d_frame,
    render_game_3d_frame, strip_editor_only_items, viewport_for_scene_camera, PrimitiveShape,
    Render3DItem, Render3DKind,
};
use gdscene::camera3d::{current_camera3d, set_cull_mask};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 96;
const H: u32 = 72;

struct Fixture {
    tree: SceneTree,
    camera: NodeId,
    mesh: NodeId,
    collision: NodeId,
}

/// A mesh node and a directional light on the origin, with a `Camera3D` two units
/// down +Z looking at them (identity basis = looking down −Z, as in Godot) with a
/// non-default `fov`.
///
/// The light's own −Z points at the mesh's camera-facing side, so that face is lit
/// and the centre of the frame is not background.
fn fixture(camera_claims_current: bool) -> Fixture {
    // The render list filters on `is_class("Node3D")`, which reads the ClassDB —
    // without this the 3D classes are unknown and the list comes back empty, so
    // every assertion below would pass vacuously.
    gdobject::class_db::register_3d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mesh = tree
        .add_child(root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    // ⚠️ A REAL mesh, and the assertion below depends on it (pat-mgixk.51).
    //
    // This node is this test's stand-in for "scene content", but it used to carry
    // no `mesh` at all — and a mesh-less MeshInstance3D resolves to
    // `PrimitiveShape::Box`, the same placeholder a plain Node3D gets. So
    // "game mode ... keeps scene content" was really pinning the PLACEHOLDER, and
    // the fixture could not tell real geometry from editor furniture. Godot draws
    // nothing for a mesh-less MeshInstance3D either, so the game path now strips
    // `Box`; giving the node an actual mesh is what makes it content.
    //
    // `primitive:BoxMesh` is the repo's own generated primitive, read by
    // `node3d::get_mesh_path` straight off the `mesh` property.
    tree.get_node_mut(mesh)
        .unwrap()
        .set_property("mesh", Variant::String("primitive:BoxMesh".to_string()));
    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    let collision = tree
        .add_child(root, Node::new("Body", "CollisionShape3D"))
        .unwrap();
    tree.get_node_mut(collision)
        .unwrap()
        .set_property("shape", Variant::String("shape:BoxShape3D".to_string()));
    let camera = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    let node = tree.get_node_mut(camera).unwrap();
    node.set_property(
        "transform",
        Variant::Transform3D(Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 2.0),
        }),
    );
    node.set_property("fov", Variant::Float(60.0));
    if camera_claims_current {
        node.set_property("current", Variant::Bool(true));
    }
    Fixture {
        tree,
        camera,
        mesh,
        collision,
    }
}

/// Counts the editor-only marker items (the camera/light gizmo cubes).
fn marker_count(items: &[Render3DItem]) -> usize {
    items
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                Render3DKind::Primitive {
                    shape: PrimitiveShape::Camera | PrimitiveShape::Light,
                }
            )
        })
        .count()
}

#[test]
fn play_renders_from_the_scene_camera_not_the_editor_orbit_camera() {
    let f = fixture(true);

    let game = render_game_3d_frame(&f.tree, W, H).expect("a 3D scene with a Camera3D renders");
    assert_eq!(
        (game.width, game.height),
        (W, H),
        "frame is the requested size"
    );

    // The mesh the camera points at is centred in its view; the corner is
    // background. This is what proves the frame was rendered through that camera
    // rather than merely allocated.
    let px = |x: u32, y: u32| game.pixels[(y * W + x) as usize];
    assert_ne!(
        px(W / 2, H / 2),
        px(0, 0),
        "the node the scene camera points at is drawn at the centre of the frame",
    );

    // The editor's orbit camera frames the same scene differently, so the game
    // frame cannot be the editor viewport's frame under another name.
    let orbit = render_editor_3d_frame(&Viewport3D::new(W, H), &f.tree);
    assert_ne!(
        game.pixels, orbit.pixels,
        "Play uses the scene's Camera3D, not the editor's orbit camera",
    );

    // The camera's *rotation* frames the shot too, not just its position: turned
    // 180° about Y from the same spot, it looks away from the scene.
    let mut turned = fixture(true);
    turned
        .tree
        .get_node_mut(turned.camera)
        .unwrap()
        .set_property(
            "transform",
            Variant::Transform3D(Transform3D {
                basis: Basis::from_euler(Vector3::new(0.0, std::f32::consts::PI, 0.0)),
                origin: Vector3::new(0.0, 0.0, 2.0),
            }),
        );
    let away = render_game_3d_frame(&turned.tree, W, H).expect("still renders");
    assert_ne!(
        game.pixels, away.pixels,
        "the scene camera's basis is honoured — turning it around changes the frame",
    );
}

#[test]
fn game_mode_drops_editor_furniture_but_keeps_scene_content() {
    let f = fixture(true);
    let viewport = viewport_for_scene_camera(&f.tree, f.camera, W, H);

    // Editor mode carries a marker cube for the Camera3D and for the light —
    // asserted first so a later zero cannot pass by the fixture having none.
    let mut inputs = editor_3d_render_inputs(&viewport, &f.tree);
    assert_eq!(
        marker_count(&inputs.items),
        2,
        "editor list carries the Camera3D and DirectionalLight3D markers",
    );
    assert!(
        inputs
            .items
            .iter()
            .any(|item| matches!(item.kind, Render3DKind::Collision { .. })),
        "editor list carries the CollisionShape3D wireframe",
    );

    strip_editor_only_items(&mut inputs);
    assert_eq!(
        marker_count(&inputs.items),
        0,
        "game mode draws none of the editor's camera/light markers",
    );
    assert!(
        !inputs
            .items
            .iter()
            .any(|item| item.node_id == f.collision.raw()),
        "game mode draws no collision wireframes (Godot hides them unless the \
         debug toggle is on)",
    );
    assert!(
        inputs.items.iter().any(|item| item.node_id == f.mesh.raw()),
        "game mode drops editor furniture, not scene content — the mesh node stays",
    );

    // Same camera, so any pixel difference is furniture alone: the editor frame
    // still has the grid, the origin lines and the marker cubes.
    let editor_same_camera = render_editor_3d_frame(&viewport, &f.tree);
    let game = render_game_3d_frame(&f.tree, W, H).expect("a 3D scene with a Camera3D renders");
    assert_ne!(
        game.pixels, editor_same_camera.pixels,
        "with the camera held fixed, the game frame still differs: no grid/origin/markers",
    );
}

#[test]
fn cull_mask_excludes_nodes_off_the_cameras_layers() {
    let mut f = fixture(true);
    let viewport = viewport_for_scene_camera(&f.tree, f.camera, W, H);

    // Godot defaults: VisualInstance3D.layers = 1, Camera3D.cull_mask = all 20
    // layers — so an unconfigured scene draws everything.
    let inputs = game_3d_render_inputs(&f.tree, f.camera, &viewport);
    assert!(
        inputs.items.iter().any(|item| item.node_id == f.mesh.raw()),
        "by default the mesh is on a layer the camera renders",
    );

    // Move the mesh to layer 2 and restrict the camera to layer 1.
    f.tree
        .get_node_mut(f.mesh)
        .unwrap()
        .set_property("layers", Variant::Int(0b10));
    set_cull_mask(&mut f.tree, f.camera, 0b01);

    let inputs = game_3d_render_inputs(&f.tree, f.camera, &viewport);
    assert!(
        !inputs.items.iter().any(|item| item.node_id == f.mesh.raw()),
        "the camera's cull_mask does not include the mesh's layer, so it is not drawn",
    );
}

#[test]
fn camera_override_keeps_the_editor_camera_while_the_game_runs() {
    let f = fixture(true);
    assert_eq!(
        game_camera_for_play(&f.tree, false),
        Some(f.camera),
        "normally Play renders from the scene's current Camera3D",
    );
    assert_eq!(
        game_camera_for_play(&f.tree, true),
        None,
        "with the runtime camera override on, Play falls back to the editor camera",
    );
}

#[test]
fn current_camera_wins_over_tree_order_and_a_camera_less_scene_renders_nothing() {
    // Godot: a Camera3D claiming `current` is the one the viewport renders from,
    // even when an earlier camera exists.
    let mut f = fixture(false);
    let root = f.tree.root_id();
    let second = f
        .tree
        .add_child(root, Node::new("Cam2", "Camera3D"))
        .unwrap();
    assert_eq!(
        current_camera3d(&f.tree),
        Some(f.camera),
        "with no camera claiming `current`, the first camera in tree order renders",
    );

    f.tree
        .get_node_mut(second)
        .unwrap()
        .set_property("current", Variant::Bool(true));
    assert_eq!(
        current_camera3d(&f.tree),
        Some(second),
        "the camera claiming `current` wins over tree order",
    );

    // No Camera3D at all: the game view has nothing to render from, and says so
    // rather than inventing a camera (the Play branch then falls back).
    let mut empty = SceneTree::new();
    let empty_root = empty.root_id();
    empty
        .add_child(empty_root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    assert_eq!(current_camera3d(&empty), None, "no Camera3D in the scene");
    assert!(
        render_game_3d_frame(&empty, W, H).is_none(),
        "a camera-less 3D scene yields no game frame",
    );
}
