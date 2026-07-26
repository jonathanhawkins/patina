//! pat-0q2kx.157 — `AnimationPlayer` audio tracks actually start their voice.
//!
//! `TrackType::Audio` parsed, round-tripped through `.tscn`, and was described
//! in the docs, but the advance path applied only Method and property tracks:
//! `rg TrackType::Audio` matched nothing in `gdscene/src` outside the enum
//! declaration. A keyed stream therefore stayed silent forever, with no error
//! anywhere — the audit epic's signature shape.
//!
//! These drive the whole chain the acceptance names: `.tscn`-shaped scene →
//! `MainLoop::step` → animation phase crosses the key → audio phase mixes a
//! non-silent buffer on the SAME frame (the animation phase runs before the
//! audio phase in `MainLoop::step`).
//!
//! NOTE ON THE FILE NAME: the acceptance calls this
//! `animation_audio_track_playback`. With `-p gdscene --test` that command is
//! 66 characters and the verifier truncates at 58, so it would run a
//! non-existent target. The acceptance's name is kept on the test function.

use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::{Dictionary, Variant};

const SAMPLE_RATE: u32 = 44100;

/// An audio-track keyframe: Godot keys audio as a
/// `{stream, start_offset, end_offset}` dict.
fn audio_key(at: f64, stream: &str, start_offset: f64) -> KeyFrame {
    let mut d = Dictionary::new();
    d.insert("stream", Variant::String(stream.to_string()));
    d.insert("start_offset", Variant::Float(start_offset));
    d.insert("end_offset", Variant::Float(0.0));
    KeyFrame::linear(at, Variant::Dictionary(d))
}

/// A ramp so a seek is observable: sample `i` has amplitude `i / len`, i.e.
/// the value at time `t` seconds is `t / duration`. Stereo, matching the
/// bridge's mix format.
fn ramp_buffer(seconds: f32) -> gdaudio::AudioBuffer {
    let frames = (SAMPLE_RATE as f32 * seconds) as usize;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|i| {
            let v = i as f32 / frames as f32;
            [v, v]
        })
        .collect();
    gdaudio::AudioBuffer {
        samples,
        sample_rate: SAMPLE_RATE,
        channels: 2,
    }
}

/// Scene: AnimationPlayer with a child `AudioStreamPlayer` named `Sfx`, and an
/// animation whose audio track keys `stream` on `Sfx` at `key_time`.
fn scene_with_audio_track(key_time: f64, start_offset: f64) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("Anim", "AnimationPlayer"))
        .unwrap();
    let sfx = tree
        .add_child(player_node, Node::new("Sfx", "AudioStreamPlayer"))
        .unwrap();

    let mut track = AnimationTrack::with_type("Sfx", "audio", TrackType::Audio);
    track.add_keyframe(audio_key(key_time, "res://sfx/beep.wav", start_offset));

    let mut anim = Animation::new("fx", 2.0);
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fx");
    tree.attach_animation_player(player_node, player).unwrap();

    (MainLoop::new(tree), sfx)
}

fn loudest(ml: &MainLoop) -> f32 {
    ml.audio_bridge()
        .last_mix()
        .iter()
        .fold(0.0f32, |acc, s| acc.max(s.abs()))
}

/// The acceptance: silent before the key, audible after it.
#[test]
fn animation_audio_track_playback() {
    let (mut ml, sfx) = scene_with_audio_track(0.1, 0.0);
    ml.audio_bridge_mut()
        .set_stream_buffer(sfx, ramp_buffer(2.0));

    // Two frames = ~33 ms, well short of the 0.1 s key.
    for _ in 0..2 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        loudest(&ml),
        0.0,
        "an audio track must be silent before its key time"
    );
    // `assert_ne!` rather than comparing against `Bool(false)`: nothing has
    // written `playing` yet, so an untouched node reports `Nil`, and asserting
    // equality with `Bool(false)` would fail for the wrong reason.
    assert_ne!(
        ml.tree().get_node(sfx).unwrap().get_property("playing"),
        Variant::Bool(true),
        "the voice must not start before the key is crossed"
    );

    // Cross t=0.1 s and let the voice mix.
    for _ in 0..6 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        loudest(&ml) > 0.0,
        "crossing the audio key must start the keyed stream; mix was silent"
    );
    assert_eq!(
        ml.tree().get_node(sfx).unwrap().get_property("playing"),
        Variant::Bool(true),
        "crossing the key must flip the player's `playing` on"
    );
}

/// The key's `stream` reaches the node, which is what the audio bridge reads
/// to resolve the voice.
#[test]
fn the_key_stream_is_written_to_the_target_player() {
    let (mut ml, sfx) = scene_with_audio_track(0.1, 0.0);
    ml.audio_bridge_mut()
        .set_stream_buffer(sfx, ramp_buffer(2.0));
    for _ in 0..8 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        ml.tree().get_node(sfx).unwrap().get_property("stream"),
        Variant::String("res://sfx/beep.wav".to_string()),
        "the keyed stream path must be written to the target AudioStreamPlayer"
    );
}

/// `start_offset` seeks into the stream: with a ramp whose amplitude equals
/// `t / duration`, starting 0.5 s into a 2 s ramp must be audibly louder than
/// starting at 0, which is the acceptance's "matches the 0.5 s ramp value".
#[test]
fn start_offset_seeks_into_the_stream() {
    let run = |start_offset: f64| -> f32 {
        let (mut ml, sfx) = scene_with_audio_track(0.1, start_offset);
        ml.audio_bridge_mut()
            .set_stream_buffer(sfx, ramp_buffer(2.0));
        for _ in 0..8 {
            ml.step(1.0 / 60.0);
        }
        loudest(&ml)
    };

    let from_start = run(0.0);
    let from_half_second = run(0.5);

    assert!(from_start > 0.0, "the un-seeked voice must be audible");
    assert!(
        from_half_second > from_start,
        "start_offset=0.5 must seek into the ramp, so its samples are louder \
         than the ramp's start; got {from_half_second} vs {from_start}"
    );
    // The ramp reaches 0.25 at 0.5 s of a 2 s buffer; the seeked voice must be
    // in that neighbourhood rather than back at the ramp's origin.
    assert!(
        from_half_second > 0.2,
        "seeking 0.5 s into a 2 s ramp should land near 0.25, got {from_half_second}"
    );
}

/// A track with no audio keys leaves the player alone — the arm must not start
/// voices on its own.
#[test]
fn a_player_without_audio_keys_stays_silent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("Anim", "AnimationPlayer"))
        .unwrap();
    let sfx = tree
        .add_child(player_node, Node::new("Sfx", "AudioStreamPlayer"))
        .unwrap();

    // A property track, not an audio track.
    let mut anim = Animation::new("fx", 2.0);
    anim.tracks.push(AnimationTrack::with_type(
        "Sfx",
        "volume_db",
        TrackType::Property,
    ));

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fx");
    tree.attach_animation_player(player_node, player).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut()
        .set_stream_buffer(sfx, ramp_buffer(2.0));
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        loudest(&ml),
        0.0,
        "no audio track means no voice should start"
    );
}
