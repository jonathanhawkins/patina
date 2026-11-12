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

/// Drives `travel` of root motion through the REAL animation pipeline, so the
/// mixer's accumulator fills — which is what the grid reads (pat-mgixk.78).
///
/// 🔑 This used to write `Character.position` directly, which was a faithful
/// stand-in only while `apply_root_motion_delta` did the same thing each frame.
/// The grid now reads `AnimationPlayer::root_motion().position_accumulator`, so
/// poking the property would no longer reach the quantity under test and both
/// offset assertions would fail while the grid was working correctly.
///
/// ⚠️ The clip is `0 -> ZERO`, `2 -> travel * 2`, advanced by `1.0`, so the
/// sampled delta is exactly `travel` at the HALFWAY point. Landing mid-clip is
/// deliberate: a step to the end could stop or loop the clip, and a loop would
/// wrap the new time back to 0 and invert the delta. The shape is copied from
/// `animation_root_motion_method_test.rs:126-140`.
///
/// ⚠️ `root_motion_track` names a `TrackType::Property` track, not a bone track
/// — which is what `root_motion_frame` has to accept for this to report
/// anything at all.
fn advance_root_motion(tree: &mut SceneTree, travel: gdcore::math::Vector3) {
    use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};

    let anim_node = find(tree, "Anim");

    let mut anim = Animation::new("walk", 2.0);
    let mut pos = AnimationTrack::with_type("", "position", TrackType::Property);
    pos.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector3(gdcore::math::Vector3::ZERO),
    ));
    pos.add_keyframe(KeyFrame::linear(
        2.0,
        Variant::Vector3(gdcore::math::Vector3::new(
            travel.x * 2.0,
            travel.y * 2.0,
            travel.z * 2.0,
        )),
    ));
    anim.tracks.push(pos);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("walk");
    // `".."` to match the `root_node` NODE property ClassDB seeds on every
    // `.tscn` mixer (`class_db.rs:5428`), so the auto-apply that is still in
    // place targets `Character` exactly as the old helper did. The struct
    // field's own default is `"."`, which would target the player itself.
    player.root_node = "..".to_string();
    player.root_motion_track = Some("position".to_string());
    tree.attach_animation_player(anim_node, player).unwrap();

    tree.process_animations(1.0);
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
