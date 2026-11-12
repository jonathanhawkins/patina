//! Smoke tests for AudioStreamPlayer (pat-kaa, pat-0uov2).
//!
//! Covers node creation, ClassDB registration, and play/stop against AudioServer.

use gdaudio::{
    AudioBuffer, AudioMixer, AudioServer, AudioStreamPlayback, AudioStreamPlayer, LoopMode,
    PlaybackState,
};
use gdobject::class_db::{
    class_exists, clear_for_testing, instantiate, is_parent_class, register_audio_classes,
    register_class, ClassRegistration,
};
use gdobject::object::GodotObject;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

#[test]
fn audio_node_can_be_created_without_panic() {
    let node = Node::new("MyAudio", "AudioStreamPlayer");
    assert_eq!(node.name(), "MyAudio");
    assert_eq!(node.class_name(), "AudioStreamPlayer");
}

#[test]
fn audio_node_has_expected_properties() {
    let mut node = Node::new("BGMusic", "AudioStreamPlayer");
    // Set properties that an AudioStreamPlayer would have
    node.set_property("volume_db", Variant::Float(0.0));
    node.set_property("bus", Variant::String("Master".into()));
    node.set_property("playing", Variant::Bool(false));
    node.set_property("stream_length", Variant::Float(0.0));

    assert_eq!(node.get_property("volume_db"), Variant::Float(0.0));
    assert_eq!(node.get_property("bus"), Variant::String("Master".into()));
    assert_eq!(node.get_property("playing"), Variant::Bool(false));
}

#[test]
fn audio_node_in_scene_tree_no_panic() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let audio = Node::new("SFX", "AudioStreamPlayer");
    let audio_id = tree.add_child(root, audio).unwrap();

    // Verify it's in the tree
    let node = tree.get_node(audio_id).unwrap();
    assert_eq!(node.name(), "SFX");
    assert_eq!(node.class_name(), "AudioStreamPlayer");

    // Remove it — should not panic
    tree.remove_node(audio_id).unwrap();
}

#[test]
fn audio_playback_full_lifecycle_no_panic() {
    let mut pb = AudioStreamPlayback::new(5.0);
    assert_eq!(pb.state(), PlaybackState::Stopped);

    pb.play();
    assert_eq!(pb.state(), PlaybackState::Playing);
    assert!(pb.is_playing());

    pb.advance(1.0);
    assert!((pb.get_playback_position() - 1.0).abs() < 1e-6);

    pb.set_volume_db(-12.0);
    assert_eq!(pb.volume_db(), -12.0);

    pb.set_bus("SFX");
    assert_eq!(pb.get_bus(), "SFX");

    pb.seek(3.0);
    assert!((pb.get_playback_position() - 3.0).abs() < 1e-6);

    pb.pause();
    assert_eq!(pb.state(), PlaybackState::Paused);

    pb.play();
    pb.stop();
    assert_eq!(pb.state(), PlaybackState::Stopped);
    assert_eq!(pb.get_playback_position(), 0.0);
}

#[test]
fn audio_playback_loop_and_end_no_panic() {
    // No-loop: stops at end
    let mut pb = AudioStreamPlayback::new(2.0);
    pb.play();
    pb.advance(3.0); // past end
    assert_eq!(pb.state(), PlaybackState::Stopped);

    // Loop: wraps around
    let mut pb2 = AudioStreamPlayback::new(4.0);
    pb2.set_loop_mode(LoopMode::Forward);
    pb2.play();
    pb2.advance(5.0); // wraps to 1.0
    assert!(pb2.is_playing());
    assert!((pb2.get_playback_position() - 1.0).abs() < 1e-6);
}

#[test]
fn audio_mixer_bus_management_no_panic() {
    let mut mixer = AudioMixer::new();
    assert_eq!(mixer.bus_count(), 1); // Master

    let sfx = mixer.add_bus("SFX");
    let music = mixer.add_bus("Music");
    assert_eq!(mixer.bus_count(), 3);

    // Configure buses
    mixer.get_bus_mut(sfx).unwrap().set_volume_db(-6.0);
    mixer.get_bus_mut(music).unwrap().set_mute(true);

    // Move and remove
    mixer.move_bus(music, 1);
    assert_eq!(mixer.get_bus(1).unwrap().name(), "Music");
    mixer.remove_bus(1);
    assert_eq!(mixer.bus_count(), 2);
}

// ===========================================================================
// pat-0uov2: ClassDB registration + AudioServer-driven play/stop
// ===========================================================================

fn dc_buffer(frames: usize) -> AudioBuffer {
    AudioBuffer {
        samples: vec![0.5; frames],
        sample_rate: 44100,
        channels: 1,
    }
}

#[test]
fn audio_stream_player_classdb_registration() {
    clear_for_testing();
    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_audio_classes();

    assert!(class_exists("AudioStreamPlayer"));
    assert!(class_exists("AudioStreamPlayer2D"));
    assert!(class_exists("AudioStreamPlayer3D"));
    assert!(is_parent_class("AudioStreamPlayer", "Node"));
    assert!(is_parent_class("AudioStreamPlayer2D", "Node2D"));
    assert!(is_parent_class("AudioStreamPlayer3D", "Node3D"));

    let player = instantiate("AudioStreamPlayer").expect("instantiate AudioStreamPlayer");
    assert_eq!(player.get_property("volume_db"), Variant::Float(0.0));
    assert_eq!(player.get_property("pitch_scale"), Variant::Float(1.0));
    assert_eq!(player.get_property("autoplay"), Variant::Bool(false));
    assert_eq!(player.get_property("bus"), Variant::String("Master".into()));
    assert_eq!(player.get_property("playing"), Variant::Bool(false));
}

#[test]
fn audio_stream_player_play_stop_is_playing() {
    let mut server = AudioServer::new();
    let mut player = AudioStreamPlayer::new();
    player.set_stream(dc_buffer(44100));
    player.set_volume_db(-3.0);
    player.set_bus("Master");

    assert!(!player.is_playing(&server));
    assert!(player.play(&mut server).is_some());
    assert!(player.is_playing(&server));
    assert_eq!(server.active_stream_count(), 1);

    // Mix a few frames to prove the voice is live in the server.
    let out = server.mix(64);
    assert_eq!(out.len(), 64 * 2);

    player.stop(&mut server);
    assert!(!player.is_playing(&server));
    assert_eq!(server.active_stream_count(), 0);
}

#[test]
fn audio_stream_player_autoplay_on_enter_tree() {
    let mut server = AudioServer::new();
    let mut player = AudioStreamPlayer::new();
    player.set_stream(dc_buffer(22050));
    player.set_autoplay(true);

    assert!(player.on_enter_tree(&mut server).is_some());
    assert!(player.is_playing(&server));
}

#[test]
fn audio_stream_player_scene_node_with_server_playback() {
    // Scene node holds property bag; runtime player drives the server.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("BGM", "AudioStreamPlayer");
    node.set_property("volume_db", Variant::Float(-6.0));
    node.set_property("bus", Variant::String("Master".into()));
    node.set_property("autoplay", Variant::Bool(true));
    node.set_property("playing", Variant::Bool(false));
    let id = tree.add_child(root, node).unwrap();

    let scene_node = tree.get_node(id).unwrap();
    assert_eq!(scene_node.class_name(), "AudioStreamPlayer");

    let mut server = AudioServer::new();
    let mut player = AudioStreamPlayer::new();
    player.set_stream(dc_buffer(44100));
    if let Variant::Float(db) = scene_node.get_property("volume_db") {
        player.set_volume_db(db as f32);
    }
    if let Variant::Bool(true) = scene_node.get_property("autoplay") {
        player.set_autoplay(true);
    }
    player.on_enter_tree(&mut server);
    assert!(player.is_playing(&server));

    // Reflect playing state back onto the node property bag.
    tree.get_node_mut(id)
        .unwrap()
        .set_property("playing", Variant::Bool(true));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("playing"),
        Variant::Bool(true)
    );

    player.stop(&mut server);
    tree.get_node_mut(id)
        .unwrap()
        .set_property("playing", Variant::Bool(false));
    assert!(!player.is_playing(&server));
}
