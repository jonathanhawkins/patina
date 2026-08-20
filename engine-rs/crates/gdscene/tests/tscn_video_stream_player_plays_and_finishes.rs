//! pat-s3rrv.171 acceptance: a `.tscn` VideoStreamPlayer actually plays.
//!
//! Three halves of this feature each looked complete on their own and were not
//! connected to anything:
//!
//! - the decoder and player (`paused`, `advance`, `get_video_texture`) were
//!   reachable only from unit tests;
//! - `process_video_players` advanced a map that **nothing populated**, because
//!   `SceneTree::add_video_player` had no callers;
//! - the render path recognised the class as a Control — ClassDB parents it
//!   there — but `draw_control` had no arm for it, so it fell through to
//!   `_ => {}` and a video drew nothing.
//!
//! So a scene could carry a video, show it in the inspector, and never produce
//! a frame or a pixel. This drives the whole chain from a loaded scene.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::DrawCommand;
use gdvariant::Variant;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Three visually distinct 1x1 RGBA frames at 10 fps — a 0.3s clip.
///
/// Distinct frames are the point: identical ones would make "the texture
/// changed" unobservable and the advance assertion vacuous.
fn fixture_video() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"PATVID01");
    v.extend_from_slice(&1u32.to_le_bytes()); // width
    v.extend_from_slice(&1u32.to_le_bytes()); // height
    v.extend_from_slice(&3u32.to_le_bytes()); // frame count
    v.extend_from_slice(&10.0f32.to_le_bytes()); // fps
    v.extend_from_slice(&[255, 0, 0, 255]); // red
    v.extend_from_slice(&[0, 255, 0, 255]); // green
    v.extend_from_slice(&[0, 0, 255, 255]); // blue
    v
}

fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("patina_video_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");
        std::fs::write(dir.join("clip.pvid"), fixture_video()).expect("fixture is writable");
        gdcore::file_access::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="VideoStream" path="res://clip.pvid" id="1_v"]

[node name="Root" type="Control"]

[node name="Screen" type="VideoStreamPlayer" parent="."]
offset_right = 320.0
offset_bottom = 180.0
stream = ExtResource("1_v")
autoplay = true
"#;

/// Instances the scene; binding happens inside `add_packed_scene_to_tree`.
fn load() -> (SceneTree, NodeId, NodeId) {
    res_root();
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let screen = tree
        .find_child(scene_root, "Screen", false)
        .expect("the scene has a Screen node");
    (tree, scene_root, screen)
}

/// Clause 1 and 2: instancing attaches a playback, and the frame loop advances it.
#[test]
fn instancing_attaches_a_playback_that_the_frame_loop_advances() {
    let (mut tree, _root, screen) = load();

    let first = tree
        .get_video_player(screen)
        .expect("instancing must attach a playback — nothing else populates the map")
        .get_video_texture()
        .cloned()
        .expect("a bound player exposes its current frame");

    // Two frame intervals at 10 fps.
    tree.process_video_players(0.1);
    tree.process_video_players(0.1);

    let later = tree
        .get_video_player(screen)
        .expect("still bound")
        .get_video_texture()
        .cloned()
        .expect("still has a frame");

    assert_ne!(
        first, later,
        "the texture did not change across two frame intervals — the player is \
         attached but not advancing"
    );
}

/// Clause 3: the render pass emits a textured command over the player rect.
#[test]
fn the_render_pass_emits_a_textured_command_for_the_player() {
    // ⚠️ This clause needs ClassDB. `is_control()` accepts a node either by
    // `is_class("Control")` — which walks the ClassDB chain — or by membership
    // of `CONTROL_CLASSES`, and VideoStreamPlayer is only in the former. With
    // an unpopulated registry the node is not recognised as a Control, the walk
    // never descends into it, and the draw arm is never reached. The runtime
    // always registers at boot; a bare test does not.
    gdobject::class_db::register_all_classes();
    let (mut tree, root, _screen) = load();

    let theme = gdscene::theme::Theme::default_theme();
    let mut viewport =
        gdserver2d::viewport::Viewport::new(320, 180, gdcore::math::Color::new(0.0, 0.0, 0.0, 0.0));
    gdscene::control_render::render_control_tree(
        &mut tree,
        root,
        &theme,
        &mut viewport,
        gdcore::math::Vector2::new(320.0, 180.0),
    );

    let textured = viewport
        .get_sorted_items()
        .iter()
        .flat_map(|item| item.commands.iter())
        .any(|c| matches!(c, DrawCommand::DrawTextureRect { .. }));

    assert!(
        textured,
        "no textured draw command for the video player — it reaches draw_control \
         as a Control but produced nothing"
    );
}

/// Clause 4: `paused` freezes the texture without ending playback.
#[test]
fn pausing_freezes_the_texture() {
    let (mut tree, _root, screen) = load();

    tree.process_video_players(0.1);
    let player = tree.get_video_player_mut(screen).expect("bound");
    player.set_paused(true);
    let frozen = player
        .get_video_texture()
        .cloned()
        .expect("a paused player still shows its frame");

    tree.process_video_players(0.1);
    tree.process_video_players(0.1);

    let after = tree
        .get_video_player(screen)
        .expect("bound")
        .get_video_texture()
        .cloned()
        .expect("still has a frame");

    assert_eq!(
        frozen, after,
        "the texture advanced while paused — paused must freeze, not merely slow"
    );
}

/// Clause 5: running to the end emits `finished` to a connected script.
#[test]
fn reaching_the_end_emits_finished_to_a_script_callback() {
    let (mut tree, root, screen) = load();

    let listener = tree
        .add_child(root, gdscene::node::Node::new("Listener", "Node"))
        .expect("listener is added");
    tree.attach_script(
        listener,
        Box::new(
            gdscene::GDScriptNodeInstance::from_source(
                "extends Node\nvar done = 0\nfunc _on_finished():\n\tdone += 1\n",
                listener,
            )
            .expect("script parses"),
        ),
    );
    tree.connect_signal(
        screen,
        "finished",
        gdobject::signal::Connection::new(listener.object_id(), "_on_finished"),
    );

    // The clip is 0.3s; run past the end.
    for _ in 0..5 {
        tree.process_video_players(0.1);
    }

    let seen = tree
        .get_script(listener)
        .expect("listener has a script")
        .get_property("done");
    assert_eq!(
        seen,
        Some(Variant::Int(1)),
        "reaching the end must emit `finished` exactly once to the connected script"
    );
}
