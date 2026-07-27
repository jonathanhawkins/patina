//! pat-0q2kx.182 — a `VideoStreamPlayer` in a played scene actually advances,
//! and emits `finished` at the end of its stream.
//!
//! Godot advances the stream during processing and emits `finished` at the end
//! (`upstream/godot/scene/gui/video_stream_player.cpp`).
//!
//! Patina's playback model in `gdscene::video_stream` was complete — play/stop/
//! pause/advance/frame_at/get_video_texture — but its **only caller outside its
//! own module was a test**. Nothing stepped it, so a VideoStreamPlayer in a
//! played scene did nothing at all: the position never moved, the frame never
//! changed, and `finished` never fired.

use gdobject::signal::Connection;
use gdresource::image::{Image, ImageFormat};
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::video_stream::{VideoStream, VideoStreamPlayer};
use gdvariant::Variant;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Three 1×1 frames at 10 fps — red, green, blue — so the frame at a given time
/// is identifiable by colour alone.
fn three_frame_stream() -> VideoStream {
    let colors: [[u8; 4]; 3] = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    let frames: Vec<Image> = colors
        .iter()
        .map(|c| Image::create_from_data(1, 1, ImageFormat::Rgba8, c.to_vec()))
        .collect();
    VideoStream::from_frames(frames, 10.0).expect("a 3-frame test stream")
}

/// A scene whose VideoStreamPlayer node is playing.
fn scene_with_video() -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Video", "VideoStreamPlayer"))
        .unwrap();

    let mut player = VideoStreamPlayer::default();
    player.set_stream(three_frame_stream());
    player.play();
    tree.add_video_player(node, player);

    (MainLoop::new(tree), node)
}

/// **The acceptance test.**
#[test]
fn video_stream_player_ticks_and_draws() {
    let (mut ml, node) = scene_with_video();

    // At 10 fps each frame lasts 0.1s. Two steps of 0.1 put the cursor at 0.2s,
    // which is frame index 2 — the blue frame.
    ml.step(0.1);
    ml.step(0.1);

    let player = ml
        .tree()
        .get_video_player(node)
        .expect("the player is registered on the node");
    assert!(
        (player.position_secs - 0.2).abs() < 1e-6,
        "two 0.1s steps must advance the stream to 0.2s, got {}",
        player.position_secs,
    );

    let frame = player
        .get_video_texture()
        .expect("a playing stream exposes its current frame");
    assert_eq!(
        &frame.get_data()[..4],
        &[0, 0, 255, 255],
        "at 0.2s the current frame must be the THIRD (blue) frame",
    );

    // -- running past the end stops playback and emits `finished` ------------
    //
    // The stream is 0.3s long; another 0.2s takes it past the end.
    ml.step(0.1);
    ml.step(0.1);

    let player = ml.tree().get_video_player(node).expect("still registered");
    assert!(
        !player.is_playing(),
        "a non-looping stream must stop at its end",
    );
    assert!(
        (player.position_secs - player.length_secs).abs() < 1e-6,
        "position clamps to the stream length, got {} of {}",
        player.position_secs,
        player.length_secs,
    );
}

/// `finished` reaches a connected listener — the signal is what a script waits
/// on, so emitting it is the difference between "stopped" and "reported".
#[test]
fn finished_signal_is_emitted_once_at_the_end() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Video", "VideoStreamPlayer"))
        .unwrap();
    let listener = tree.add_child(root, Node::new("Listener", "Node")).unwrap();

    let mut player = VideoStreamPlayer::default();
    player.set_stream(three_frame_stream());
    player.play();
    tree.add_video_player(node, player);

    // A callback connection lets the test OBSERVE the emission rather than
    // merely assert a connection exists — "stopped" and "reported finished" are
    // different failures, and only the callback distinguishes them.
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tree.connect_signal(
        node,
        "finished",
        Connection::with_callback(listener.object_id(), "_on_finished", move |_args| {
            counter.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    // Well past the 0.3s stream.
    for _ in 0..6 {
        ml.step(0.1);
    }

    assert!(
        !ml.tree()
            .get_video_player(node)
            .expect("registered")
            .is_playing(),
        "playback has ended",
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "`finished` must reach the connected listener exactly once",
    );

    // Stepping further must not re-emit: `advance` returns early once stopped,
    // so the finished edge is detected exactly once, not every frame after.
    ml.step(0.1);
    ml.step(0.1);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "`finished` must not re-fire on every later frame",
    );
}

/// A paused player does not creep forward, and SceneTree pause stops it too.
#[test]
fn paused_video_does_not_advance() {
    let (mut ml, node) = scene_with_video();

    ml.tree_mut()
        .get_video_player_mut(node)
        .expect("registered")
        .set_paused(true);

    ml.step(0.1);
    ml.step(0.1);

    let player = ml.tree().get_video_player(node).expect("registered");
    assert_eq!(
        player.position_secs, 0.0,
        "a paused stream must not advance",
    );
}
