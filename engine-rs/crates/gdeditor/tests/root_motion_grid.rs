//! pat-0q2kx.249 — `RootMotionView` draws a counter-scrolled grid in the editor
//! viewport, and nothing at all in Play.
//!
//! Godot ground truth: `scene/animation/root_motion_view.cpp` — an editor-only
//! visualisation whose grid scrolls *opposite* the AnimationMixer's root motion,
//! so an animator can inspect locomotion while the character stays put. It hides
//! at runtime.
//!
//! `gdscene/src/root_motion_view.rs` already modelled the counter-scroll maths but
//! had no consumer outside its own test — the editor render path never looked at
//! the node. This drives the real `editor_3d_render_inputs` snapshot, which is
//! what `draw_editor_overlays_3d` renders from.
//!
//! ⚠️ Why no stored scroll state: root motion is applied straight onto the mixer's
//! root node by `SceneTree::process_animations` (`apply_root_motion_delta`), so
//! the node's *current position* already is the accumulated travel. Moving the
//! node is therefore exactly what "stepping a frame" does to the grid, which is
//! what the second assertion below exercises.

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, strip_editor_only_items};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A character driven by an `AnimationPlayer`, with a `RootMotionView` pointed at
/// that player. `cell_size` is 1 so a travel of 0.25 lands unambiguously inside
/// one cell rather than wrapping.
const SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node3D"]

[node name="Character" type="Node3D" parent="."]

[node name="Anim" type="AnimationPlayer" parent="Character"]

[node name="RootMotion" type="RootMotionView" parent="."]
animation_path = NodePath("../Character/Anim")
cell_size = 1.0
radius = 4.0
color = Color(0, 1, 0, 1)
"#;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// Moves the node root motion drives, as `apply_root_motion_delta` does each
/// animation frame.
fn advance_root_motion(tree: &mut SceneTree, travel: gdcore::math::Vector3) {
    let character = find(tree, "Character");
    let cur = match tree.get_node(character).map(|n| n.get_property("position")) {
        Some(Variant::Vector3(v)) => v,
        _ => gdcore::math::Vector3::ZERO,
    };
    tree.get_node_mut(character)
        .unwrap()
        .set_property("position", Variant::Vector3(cur + travel));
}

/// **The acceptance test.** The editor snapshot carries a grid whose offset moves
/// between two stepped frames; the Play snapshot carries none.
#[test]
fn root_motion_view_editor_grid() {
    gdobject::class_db::register_3d_classes();
    let mut tree = load(SCENE);
    let viewport = Viewport3D::default();

    // -- frame 1 -------------------------------------------------------------
    let first = editor_3d_render_inputs(&viewport, &tree)
        .root_motion_grid
        .expect("a scene with a RootMotionView must produce a grid in the editor");
    assert!(
        (first.cell_size - 1.0).abs() < 1e-4 && (first.radius - 4.0).abs() < 1e-4,
        "the grid must carry the node's authored cell_size/radius, got {first:?}"
    );
    assert!(
        first.color.g > 0.9 && first.color.r < 0.1,
        "the grid must use the node's color, got {:?}",
        first.color
    );

    // -- frame 2, after the animation drove the character forward ------------
    advance_root_motion(&mut tree, gdcore::math::Vector3::new(0.0, 0.0, 0.25));
    let second = editor_3d_render_inputs(&viewport, &tree)
        .root_motion_grid
        .expect("still a grid");
    assert!(
        (second.offset.z - first.offset.z).abs() > 1e-4,
        "the grid must scroll as root motion accumulates: {:?} -> {:?}",
        first.offset,
        second.offset
    );
    // 0.25 of travel in a 1.0 cell wraps to 0.25 — the grid slides a quarter cell
    // rather than jumping a whole one.
    assert!(
        (second.offset.z - 0.25).abs() < 1e-4,
        "0.25 travel in a 1.0 cell must wrap to 0.25, got {:?}",
        second.offset
    );

    // -- Play draws nothing --------------------------------------------------
    let mut play = editor_3d_render_inputs(&viewport, &tree);
    strip_editor_only_items(&mut play);
    assert!(
        play.root_motion_grid.is_none(),
        "RootMotionView is editor-only and must not reach a game frame"
    );
}

/// The wrap is Godot's `fposmod`, not `%`: travel past a whole cell comes back
/// into the cell instead of running away, and a *negative* travel wraps forward
/// rather than staying negative (which would drag the grid out of its own cell).
#[test]
fn grid_offset_wraps_into_one_cell() {
    gdobject::class_db::register_3d_classes();
    let viewport = Viewport3D::default();

    let mut tree = load(SCENE);
    advance_root_motion(&mut tree, gdcore::math::Vector3::new(0.0, 0.0, 3.25));
    let g = editor_3d_render_inputs(&viewport, &tree)
        .root_motion_grid
        .expect("grid");
    assert!(
        (g.offset.z - 0.25).abs() < 1e-4,
        "3.25 of travel must wrap to 0.25 in a 1.0 cell, got {:?}",
        g.offset
    );

    let mut tree = load(SCENE);
    advance_root_motion(&mut tree, gdcore::math::Vector3::new(0.0, 0.0, -0.25));
    let g = editor_3d_render_inputs(&viewport, &tree)
        .root_motion_grid
        .expect("grid");
    assert!(
        (g.offset.z - 0.75).abs() < 1e-4,
        "-0.25 must wrap FORWARD to 0.75 (fposmod), not stay negative, got {:?}",
        g.offset
    );
}

/// A scene with no `RootMotionView` produces no grid — the control that keeps the
/// assertions above from passing for an implementation that always emits one.
#[test]
fn scene_without_the_node_has_no_grid() {
    gdobject::class_db::register_3d_classes();
    let tree = load("[gd_scene format=3]\n\n[node name=\"World\" type=\"Node3D\"]\n");
    let viewport = Viewport3D::default();
    assert!(
        editor_3d_render_inputs(&viewport, &tree)
            .root_motion_grid
            .is_none(),
        "no RootMotionView means no grid"
    );
}
