//! Smoke tests for AudioStreamPlayer stub behavior (pat-kaa).
//!
//! These tests prove that the AudioStreamPlayer node can be created, configured,
//! and dropped without panicking. They validate the audio milestone stub contract
//! defined in EXIT_CRITERIA.md — no actual audio playback occurs.

use gdaudio::{AudioMixer, AudioStreamPlayback, LoopMode, PlaybackState};
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
