//! pat-0q2kx.54 (serialization half): an `AnimationPlayer`'s clips are written
//! into the `.tscn` and survive a save → re-load round-trip.
//!
//! Godot stores editor-authored animations *inside the scene file*: one
//! `[sub_resource type="Animation"]` per clip carrying `length`/`loop_mode` and
//! `tracks/N/*`, gathered by an `[sub_resource type="AnimationLibrary"]` that the
//! player's `libraries` points at (`editor/animation_track_editor.cpp`,
//! `scene/resources/animation.cpp`).
//!
//! `scene_saver.rs` had ZERO animation handling, so saving a scene silently
//! dropped every clip: an animation authored in the editor existed only in
//! memory and was gone on re-open. These drive the real
//! `TscnSaver::save_tree` → `PackedScene::from_tscn` →
//! `attach_scene_animation_players` path and assert on the recovered clip.

use gdcore::math::Vector2;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode};
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;
use gdvariant::Variant;

/// A scene whose `AnimationPlayer` owns a two-keyframe `slide` clip driving the
/// sibling sprite's `position` — the shape the editor's keyframe route produces.
fn authored_scene() -> (SceneTree, gdscene::NodeId) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .unwrap();
    let sprite = tree
        .add_child(root, Node::new("Sprite2D", "Sprite2D"))
        .unwrap();
    tree.get_node_mut(sprite)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));

    let player_node = tree
        .add_child(root, Node::new("AnimationPlayer", "AnimationPlayer"))
        .unwrap();

    // `AnimationTrack::new` takes the property path; the target node is a
    // separate field (an empty `node_path` would address the player itself).
    let mut track = AnimationTrack::new("position");
    track.node_path = "Sprite2D".to_string();
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector2(Vector2::new(0.0, 0.0)),
    ));
    track.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Vector2(Vector2::new(100.0, 0.0)),
    ));

    let mut clip = Animation::new("slide", 1.0);
    clip.loop_mode = LoopMode::None;
    clip.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(clip);
    player.autoplay = Some("slide".to_string());
    tree.attach_animation_player(player_node, player).unwrap();

    (tree, root)
}

/// **The acceptance test.** Saving writes parseable Animation sub-resources, and
/// re-loading recovers the clip with its keyframes.
#[test]
fn animation_saves_into_tscn_and_reloads() {
    let (tree, root) = authored_scene();
    let saved = TscnSaver::save_tree(&tree, root);

    // -- the text carries Godot's own shapes ---------------------------------
    assert!(
        saved.contains("[sub_resource type=\"Animation\""),
        "the saved scene must declare an Animation sub-resource:\n{saved}"
    );
    assert!(
        saved.contains("[sub_resource type=\"AnimationLibrary\""),
        "clips are gathered by an AnimationLibrary:\n{saved}"
    );
    assert!(
        saved.contains("tracks/0/path = NodePath(\"Sprite2D:position\")"),
        "the track addresses the sprite's property:\n{saved}"
    );
    assert!(
        saved.contains("autoplay = \"slide\""),
        "the player's autoplay clip is preserved:\n{saved}"
    );

    // -- and re-loading rebuilds the player from that text -------------------
    let scene = PackedScene::from_tscn(&saved).expect("the saved scene re-parses");
    let mut reloaded = SceneTree::new();
    let reload_root = reloaded.root_id();
    let new_root =
        add_packed_scene_to_tree(&mut reloaded, reload_root, &scene).expect("scene instances");

    let player_node = reloaded
        .get_node_or_null(new_root, "AnimationPlayer")
        .expect("AnimationPlayer node survived the round-trip");
    let player = reloaded
        .get_animation_player(player_node)
        .expect("scene load rebuilt the AnimationPlayer from the saved libraries");

    let clip = player
        .animations
        .get("slide")
        .expect("the 'slide' clip came back");
    assert!(
        (clip.length - 1.0).abs() < 1e-6,
        "clip length survived, got {}",
        clip.length
    );
    assert_eq!(clip.tracks.len(), 1, "the value track survived");

    let track = &clip.tracks[0];
    assert_eq!(track.node_path, "Sprite2D");
    assert_eq!(track.property_path, "position");
    let keys = track.keyframes();
    assert_eq!(keys.len(), 2, "both keyframes survived");
    assert_eq!(keys[0].value, Variant::Vector2(Vector2::new(0.0, 0.0)));
    assert_eq!(keys[1].value, Variant::Vector2(Vector2::new(100.0, 0.0)));
    assert!((keys[1].time - 1.0).abs() < 1e-6);
}

/// Re-saving an unchanged scene is byte-stable: sub-resource ids are derived
/// from node and clip names, not minted per save, so a save/load/save cycle does
/// not churn the file and make diffs unreadable.
#[test]
fn animation_save_is_stable_across_roundtrips() {
    let (tree, root) = authored_scene();
    let first = TscnSaver::save_tree(&tree, root);

    let scene = PackedScene::from_tscn(&first).expect("parses");
    let mut reloaded = SceneTree::new();
    let reload_root = reloaded.root_id();
    let new_root = add_packed_scene_to_tree(&mut reloaded, reload_root, &scene).expect("instances");
    let second = TscnSaver::save_tree(&reloaded, new_root);

    assert!(
        second.contains("[sub_resource type=\"Animation\""),
        "the second save still carries the clip:\n{second}"
    );
    assert_eq!(
        first, second,
        "save → load → save must be byte-identical\n--- first ---\n{first}\n--- second ---\n{second}"
    );
}
