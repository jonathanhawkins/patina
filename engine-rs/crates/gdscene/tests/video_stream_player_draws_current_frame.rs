//! pat-s3rrv.174 acceptance: a playing VideoStreamPlayer draws its current
//! frame into the control rect, sized by `expand`.
//!
//! pat-s3rrv.171 connected the feature — instancing binds a stream, the frame
//! loop advances it, and a draw arm emits a textured command. This pins the
//! part that arm did not yet honour: **`expand` decides the destination size**.
//! Godot fills the control when it is true and keeps the source size when
//! false, and it defaults to **false** (doc/classes/VideoStreamPlayer.xml).

use gdcore::math::Vector2;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use std::path::PathBuf;
use std::sync::OnceLock;

/// 4x2 frames — deliberately NOT the control's size, so "fills the control"
/// and "keeps the source size" are distinguishable. Equal sizes would make the
/// expand assertions pass for either behaviour.
const FRAME_W: u32 = 4;
const FRAME_H: u32 = 2;
const CONTROL_W: f32 = 320.0;
const CONTROL_H: f32 = 180.0;

fn fixture_video() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"PATVID01");
    v.extend_from_slice(&FRAME_W.to_le_bytes());
    v.extend_from_slice(&FRAME_H.to_le_bytes());
    v.extend_from_slice(&3u32.to_le_bytes());
    v.extend_from_slice(&10.0f32.to_le_bytes());
    for shade in [255u8, 128, 0] {
        for _ in 0..(FRAME_W * FRAME_H) {
            v.extend_from_slice(&[shade, shade, shade, 255]);
        }
    }
    v
}

fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("patina_video_draw_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");
        std::fs::write(dir.join("clip.pvid"), fixture_video()).expect("fixture is writable");
        gdcore::file_access::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

fn scene_src(expand: bool, with_stream: bool) -> String {
    let stream_line = if with_stream {
        "stream = ExtResource(\"1_v\")\n"
    } else {
        ""
    };
    let ext = if with_stream {
        "[ext_resource type=\"VideoStream\" path=\"res://clip.pvid\" id=\"1_v\"]\n\n"
    } else {
        ""
    };
    format!(
        "[gd_scene load_steps=2 format=3]\n\n{ext}[node name=\"Root\" type=\"Control\"]\n\n\
         [node name=\"Screen\" type=\"VideoStreamPlayer\" parent=\".\"]\n\
         offset_right = {CONTROL_W}\n\
         offset_bottom = {CONTROL_H}\n\
         expand = {expand}\n\
         autoplay = true\n{stream_line}"
    )
}

fn load(expand: bool, with_stream: bool) -> (SceneTree, NodeId, NodeId) {
    res_root();
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(&scene_src(expand, with_stream)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let screen = tree
        .find_child(scene_root, "Screen", false)
        .expect("the scene has a Screen node");
    (tree, scene_root, screen)
}

fn video_rects(tree: &mut SceneTree, root: NodeId) -> Vec<gdcore::math::Rect2> {
    let theme = gdscene::theme::Theme::default_theme();
    let mut viewport = Viewport::new(
        CONTROL_W as u32,
        CONTROL_H as u32,
        gdcore::math::Color::new(0.0, 0.0, 0.0, 0.0),
    );
    gdscene::control_render::render_control_tree(
        tree,
        root,
        &theme,
        &mut viewport,
        Vector2::new(CONTROL_W, CONTROL_H),
    );
    viewport
        .get_sorted_items()
        .iter()
        .flat_map(|item| item.commands.iter())
        .filter_map(|c| match c {
            DrawCommand::DrawTextureRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect()
}

/// `expand = true` fills the control rect.
#[test]
fn expand_true_fills_the_control_rect() {
    let (mut tree, root, _screen) = load(true, true);
    tree.process_video_players(0.1);

    let rects = video_rects(&mut tree, root);
    assert!(!rects.is_empty(), "a playing video drew nothing");
    assert!(
        rects
            .iter()
            .any(|r| (r.size.x - CONTROL_W).abs() < 0.5 && (r.size.y - CONTROL_H).abs() < 0.5),
        "expand = true must fill the control rect; got {rects:?}"
    );
}

/// `expand = false` keeps the source size.
///
/// ⚠️ The frames are 4x2 against a 320x180 control precisely so this cannot
/// pass by accident: if the arm ignored `expand` and always used the control
/// rect, this is the assertion that catches it.
#[test]
fn expand_false_keeps_the_source_size() {
    let (mut tree, root, _screen) = load(false, true);
    tree.process_video_players(0.1);

    let rects = video_rects(&mut tree, root);
    assert!(!rects.is_empty(), "a playing video drew nothing");
    assert!(
        rects
            .iter()
            .any(|r| (r.size.x - FRAME_W as f32).abs() < 0.5
                && (r.size.y - FRAME_H as f32).abs() < 0.5),
        "expand = false must keep the {FRAME_W}x{FRAME_H} source size, not the \
         control rect; got {rects:?}"
    );
}

/// A draw item is produced on every frame while playing, and the position
/// advances.
#[test]
fn a_playing_video_draws_every_frame_and_advances_its_position() {
    let (mut tree, root, screen) = load(true, true);

    let mut positions = Vec::new();
    for _ in 0..2 {
        tree.process_video_players(0.1);
        positions.push(tree.get_video_player(screen).expect("bound").position_secs);
        assert!(
            !video_rects(&mut tree, root).is_empty(),
            "a frame produced no draw item while the video was playing"
        );
    }

    assert!(
        positions[1] > positions[0],
        "stream_position did not advance across frames: {positions:?}"
    );
}

/// No stream assigned: nothing is drawn, and no placeholder marker either.
#[test]
fn a_player_without_a_stream_draws_nothing() {
    let (mut tree, root, screen) = load(true, false);
    tree.process_video_players(0.1);

    assert!(
        tree.get_video_player(screen).is_none(),
        "a player with no stream must not be bound"
    );
    assert!(
        video_rects(&mut tree, root).is_empty(),
        "a player with no stream drew something — Godot shows nothing, not a marker"
    );
}
