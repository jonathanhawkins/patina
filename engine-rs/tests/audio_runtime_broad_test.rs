//! pat-4jow: Audio runtime basics and broadened test harness.
//!
//! Tests cover:
//! 1. AudioServer full lifecycle (play, mix, stop, stream cleanup)
//! 2. Multi-bus routing and isolation
//! 3. Audio stream playback state machine exhaustive transitions
//! 4. Mixer bus ordering and management
//! 5. Deterministic mixing output verification
//! 6. AudioBuffer edge cases (empty, mono, stereo, high sample rate)
//! 7. NullAudioOutput as deterministic sink
//! 8. Volume dB-to-linear conversion precision
//! 9. Loop mode behavior (forward loop wrap, no-loop stop)
//! 10. Concurrent stream mixing (additive correctness)
//! 11. AudioStreamPlayer node integration with scene tree
//! 12. Resource-to-audio pipeline (WAV decode + playback)

use gdaudio::{
    AudioBuffer, AudioBus, AudioMixer, AudioOutputStream, AudioSampleBuffer, AudioServer,
    AudioStreamPlayback, LoopMode, NullAudioOutput, PlaybackId, PlaybackState,
};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

// ===========================================================================
// Helper: create a DC (constant value) audio buffer
// ===========================================================================

fn dc_buffer(value: f32, frames: usize, sample_rate: u32, channels: u16) -> AudioBuffer {
    AudioBuffer {
        samples: vec![value; frames * channels as usize],
        sample_rate,
        channels,
    }
}

// ===========================================================================
// 1. AudioServer full lifecycle
// ===========================================================================

#[test]
fn server_play_mix_stop_lifecycle() {
    let mut server = AudioServer::new();
    assert_eq!(server.active_stream_count(), 0);

    let buf = dc_buffer(0.5, 4410, 44100, 1); // 0.1s mono
    let id = server.play(buf);

    assert_eq!(server.active_stream_count(), 1);
    assert!(server.is_playing(id));

    // Mix one frame batch
    let output = server.mix(1024);
    assert_eq!(output.len(), 1024 * 2); // stereo output

    server.stop(id);
    assert_eq!(server.active_stream_count(), 0);
    assert!(!server.is_playing(id));
}

#[test]
fn server_stream_auto_removes_on_completion() {
    let mut server = AudioServer::new();
    let buf = dc_buffer(1.0, 441, 44100, 1); // 0.01s — very short
    let id = server.play(buf);
    assert!(server.is_playing(id));

    // Mix enough frames to exhaust the stream
    for _ in 0..10 {
        server.mix(4410);
    }

    // Stream should have been auto-removed
    assert!(!server.is_playing(id));
}

#[test]
fn server_stop_nonexistent_id_no_panic() {
    let mut server = AudioServer::new();
    // Stop a stream that doesn't exist — must not panic
    server.stop(PlaybackId(9999));
    assert_eq!(server.active_stream_count(), 0);
}

#[test]
fn server_multiple_play_stop_cycles() {
    let mut server = AudioServer::new();

    for _ in 0..50 {
        let buf = dc_buffer(0.3, 4410, 44100, 1);
        let id = server.play(buf);
        server.mix(512);
        server.stop(id);
    }

    assert_eq!(server.active_stream_count(), 0);
}

// ===========================================================================
// 2. Multi-bus routing and isolation
// ===========================================================================

#[test]
fn play_on_specific_bus() {
    let mut server = AudioServer::new();
    server.mixer_mut().add_bus("SFX");
    server.mixer_mut().add_bus("Music");

    let sfx_buf = dc_buffer(0.8, 4410, 44100, 1);
    let music_buf = dc_buffer(0.5, 4410, 44100, 1);

    let sfx_id = server.play_on_bus(sfx_buf, "SFX");
    let music_id = server.play_on_bus(music_buf, "Music");

    assert_eq!(server.active_stream_count(), 2);
    assert!(server.is_playing(sfx_id));
    assert!(server.is_playing(music_id));

    let output = server.mix(1024);
    assert!(!output.is_empty());
}

#[test]
fn muted_bus_produces_silence() {
    let mut server = AudioServer::new();
    server.mixer_mut().add_bus("SFX");

    // Mute SFX bus
    if let Some(idx) = server.mixer().get_bus_by_name("SFX") {
        server.mixer_mut().get_bus_mut(idx).unwrap().set_mute(true);
    }

    let buf = dc_buffer(1.0, 4410, 44100, 1);
    let _id = server.play_on_bus(buf, "SFX");

    let output = server.mix(1024);

    // All samples should be zero because the bus is muted
    let max_sample = output.iter().copied().fold(0.0f32, f32::max);
    assert!(
        max_sample < 1e-6,
        "muted bus must produce silence, got max={max_sample}"
    );
}

#[test]
fn bus_volume_attenuates_output() {
    let mut server = AudioServer::new();
    server.mixer_mut().add_bus("Quiet");

    // Set bus volume to -20 dB (~0.1 linear)
    if let Some(idx) = server.mixer().get_bus_by_name("Quiet") {
        server
            .mixer_mut()
            .get_bus_mut(idx)
            .unwrap()
            .set_volume_db(-20.0);
    }

    let buf = dc_buffer(1.0, 44100, 44100, 1); // 1.0 DC for 1 second
    let _id = server.play_on_bus(buf, "Quiet");

    let output = server.mix(1024);

    // At -20 dB, linear gain is ~0.1, so samples should be ~0.1
    let max_sample = output.iter().copied().fold(0.0f32, f32::max);
    assert!(
        max_sample < 0.15 && max_sample > 0.05,
        "bus at -20dB should attenuate to ~0.1, got {max_sample}"
    );
}

// ===========================================================================
// 3. Playback state machine exhaustive transitions
// ===========================================================================

#[test]
fn playback_state_transitions_complete() {
    let mut pb = AudioStreamPlayback::new(5.0);

    // Initial: Stopped
    assert_eq!(pb.state(), PlaybackState::Stopped);
    assert!(!pb.is_playing());

    // Stopped -> Playing
    pb.play();
    assert_eq!(pb.state(), PlaybackState::Playing);
    assert!(pb.is_playing());

    // Playing -> Paused
    pb.pause();
    assert_eq!(pb.state(), PlaybackState::Paused);
    assert!(!pb.is_playing());

    // Paused -> Playing
    pb.play();
    assert_eq!(pb.state(), PlaybackState::Playing);

    // Playing -> Stopped
    pb.stop();
    assert_eq!(pb.state(), PlaybackState::Stopped);

    // Stopped -> Playing -> Stopped (direct)
    pb.play();
    pb.stop();
    assert_eq!(pb.state(), PlaybackState::Stopped);
}

#[test]
fn playback_pause_while_stopped_stays_stopped() {
    let mut pb = AudioStreamPlayback::new(5.0);
    pb.pause(); // no-op when stopped
    assert_eq!(pb.state(), PlaybackState::Stopped);
}

#[test]
fn playback_double_play_stays_playing() {
    let mut pb = AudioStreamPlayback::new(5.0);
    pb.play();
    pb.play(); // second play is idempotent
    assert_eq!(pb.state(), PlaybackState::Playing);
}

// ===========================================================================
// 4. Mixer bus ordering and management
// ===========================================================================

#[test]
fn mixer_master_bus_always_first() {
    let mixer = AudioMixer::new();
    assert_eq!(mixer.bus_count(), 1);
    let master = mixer.get_bus(0).unwrap();
    assert_eq!(master.name(), "Master");
}

#[test]
fn mixer_add_remove_buses() {
    let mut mixer = AudioMixer::new();
    mixer.add_bus("SFX");
    mixer.add_bus("Music");
    mixer.add_bus("Voice");
    assert_eq!(mixer.bus_count(), 4); // Master + 3

    // Remove Music (at index 2)
    mixer.remove_bus(2);
    assert_eq!(mixer.bus_count(), 3);

    // Master is still at index 0
    assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
}

#[test]
fn mixer_get_bus_by_name() {
    let mut mixer = AudioMixer::new();
    mixer.add_bus("SFX");

    assert!(mixer.get_bus_by_name("Master").is_some());
    assert!(mixer.get_bus_by_name("SFX").is_some());
    assert!(mixer.get_bus_by_name("NonExistent").is_none());
}

#[test]
fn mixer_move_bus_preserves_master() {
    let mut mixer = AudioMixer::new();
    mixer.add_bus("A");
    mixer.add_bus("B");
    mixer.add_bus("C");

    // Move B to position 1 (after master)
    mixer.move_bus(2, 1);

    // Master must still be at 0
    assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
}

// ===========================================================================
// 5. Deterministic mixing output verification
// ===========================================================================

#[test]
fn mix_deterministic_same_input_same_output() {
    let run = || {
        let mut server = AudioServer::new();
        let buf = dc_buffer(0.5, 44100, 44100, 1);
        server.play(buf);
        server.mix(1024)
    };

    let a = run();
    let b = run();
    assert_eq!(a, b, "mixing must be deterministic");
}

#[test]
fn mix_empty_server_produces_silence() {
    let mut server = AudioServer::new();
    let output = server.mix(1024);
    assert_eq!(output.len(), 2048); // 1024 frames * 2 channels
    assert!(
        output.iter().all(|&s| s == 0.0),
        "empty server must produce silence"
    );
}

#[test]
fn mix_output_frame_count_matches_request() {
    let mut server = AudioServer::new();
    let buf = dc_buffer(1.0, 44100, 44100, 2);
    server.play(buf);

    for frames in [128, 256, 512, 1024, 4096] {
        let output = server.mix(frames);
        assert_eq!(
            output.len(),
            frames * 2,
            "output must have frames*channels samples for {frames} frames"
        );
    }
}

// ===========================================================================
// 6. AudioBuffer edge cases
// ===========================================================================

#[test]
fn empty_buffer_plays_without_panic() {
    let mut server = AudioServer::new();
    let buf = AudioBuffer {
        samples: vec![],
        sample_rate: 44100,
        channels: 2,
    };
    let id = server.play(buf);
    let _ = server.mix(1024);
    // Stream should still exist but produce silence
    let _ = server.is_playing(id);
}

#[test]
fn mono_buffer_upmixed_to_stereo() {
    let mut server = AudioServer::new();
    let buf = dc_buffer(0.7, 44100, 44100, 1); // mono
    server.play(buf);

    let output = server.mix(100);
    // Output is stereo — both channels should have the signal
    for frame in 0..100 {
        let l = output[frame * 2];
        let r = output[frame * 2 + 1];
        assert!(
            (l - r).abs() < 1e-6,
            "mono upmix must produce identical L/R: l={l}, r={r}"
        );
        assert!(l > 0.5, "signal must be present: l={l}");
    }
}

#[test]
fn stereo_buffer_passes_through() {
    let mut server = AudioServer::new();
    // Create stereo buffer with different L/R values
    let frames = 1000;
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        samples.push(0.3); // left
        samples.push(0.7); // right
    }
    let buf = AudioBuffer {
        samples,
        sample_rate: 44100,
        channels: 2,
    };
    server.play(buf);

    let output = server.mix(100);
    // First frame should preserve L/R difference
    let l = output[0];
    let r = output[1];
    assert!(
        (l - 0.3).abs() < 0.05 && (r - 0.7).abs() < 0.05,
        "stereo must preserve channels: l={l}, r={r}"
    );
}

#[test]
fn high_sample_rate_buffer_plays() {
    let mut server = AudioServer::new();
    let buf = dc_buffer(0.5, 96000, 96000, 2); // 96kHz
    let id = server.play(buf);
    let output = server.mix(1024);
    assert!(!output.is_empty());
    // Should still be playing (96kHz buffer = 1 second)
    assert!(server.is_playing(id));
}

// ===========================================================================
// 7. NullAudioOutput as deterministic sink
// ===========================================================================

#[test]
fn null_output_tracks_frames_written() {
    let mut output = NullAudioOutput::new(44100, 2);
    assert_eq!(output.sample_rate(), 44100);
    assert_eq!(output.channels(), 2);

    let mut buf = AudioSampleBuffer::new(44100, 2);
    buf.samples = vec![0.0f32; 2048]; // 1024 frames * 2 channels
    output.write_samples(&buf);

    assert_eq!(output.frames_written, 1024);
}

#[test]
fn null_output_accumulates_across_writes() {
    let mut output = NullAudioOutput::new(44100, 2);

    for _ in 0..10 {
        let mut buf = AudioSampleBuffer::new(44100, 2);
        buf.samples = vec![0.0f32; 200]; // 100 frames * 2 channels
        output.write_samples(&buf);
    }

    assert_eq!(output.frames_written, 1000); // 10 * 100
}

// ===========================================================================
// 8. Volume dB-to-linear conversion precision
// ===========================================================================

#[test]
fn db_to_linear_unity() {
    let linear = AudioBus::db_to_linear(0.0);
    assert!((linear - 1.0).abs() < 1e-6, "0 dB must be 1.0 linear");
}

#[test]
fn db_to_linear_minus_6db() {
    let linear = AudioBus::db_to_linear(-6.0);
    // -6 dB ≈ 0.5012
    assert!(
        (linear - 0.5012).abs() < 0.01,
        "-6 dB must be ~0.5: got {linear}"
    );
}

#[test]
fn db_to_linear_minus_20db() {
    let linear = AudioBus::db_to_linear(-20.0);
    assert!(
        (linear - 0.1).abs() < 0.01,
        "-20 dB must be ~0.1: got {linear}"
    );
}

#[test]
fn db_to_linear_minus_infinity() {
    let linear = AudioBus::db_to_linear(-80.0);
    assert!(linear < 0.001, "-80 dB must be near zero: got {linear}");
}

#[test]
fn db_to_linear_positive_gain() {
    let linear = AudioBus::db_to_linear(6.0);
    // +6 dB ≈ 1.995
    assert!(
        (linear - 2.0).abs() < 0.1,
        "+6 dB must be ~2.0: got {linear}"
    );
}

// ===========================================================================
// 9. Loop mode behavior
// ===========================================================================

#[test]
fn forward_loop_wraps_position() {
    let mut pb = AudioStreamPlayback::new(2.0); // 2 seconds
    pb.set_loop_mode(LoopMode::Forward);
    pb.play();

    // Advance past the end
    pb.advance(2.5);

    // Position should wrap
    let pos = pb.get_playback_position();
    assert!(pos < 2.0, "forward loop must wrap position: got {pos}");
    assert_eq!(pb.state(), PlaybackState::Playing, "must keep playing");
}

#[test]
fn no_loop_stops_at_end() {
    let mut pb = AudioStreamPlayback::new(1.0);
    pb.set_loop_mode(LoopMode::None);
    pb.play();

    pb.advance(1.5); // past end

    assert_eq!(
        pb.state(),
        PlaybackState::Stopped,
        "no-loop must stop at end"
    );
}

#[test]
fn seek_clamps_to_bounds() {
    let mut pb = AudioStreamPlayback::new(3.0);
    pb.play();

    pb.seek(-1.0);
    assert!(pb.get_playback_position() >= 0.0, "seek must clamp to 0");

    pb.seek(100.0);
    assert!(
        pb.get_playback_position() <= 3.0,
        "seek must clamp to length"
    );
}

// ===========================================================================
// 10. Concurrent stream mixing (additive correctness)
// ===========================================================================

#[test]
fn two_streams_mix_additively() {
    let mut server = AudioServer::new();
    let buf_a = dc_buffer(0.3, 44100, 44100, 1);
    let buf_b = dc_buffer(0.4, 44100, 44100, 1);

    server.play(buf_a);
    server.play(buf_b);

    let output = server.mix(100);

    // Each frame should be ~0.7 (0.3 + 0.4)
    for frame in 0..100 {
        let sample = output[frame * 2]; // left channel
        assert!(
            (sample - 0.7).abs() < 0.05,
            "additive mix should be ~0.7: got {sample} at frame {frame}"
        );
    }
}

#[test]
fn ten_streams_mix_without_clipping_guard() {
    let mut server = AudioServer::new();

    for _ in 0..10 {
        let buf = dc_buffer(0.1, 44100, 44100, 1);
        server.play(buf);
    }

    assert_eq!(server.active_stream_count(), 10);

    let output = server.mix(100);

    // Sum should be ~1.0 (10 * 0.1)
    let sample = output[0];
    assert!(
        (sample - 1.0).abs() < 0.1,
        "10 streams of 0.1 should sum to ~1.0: got {sample}"
    );
}

// ===========================================================================
// 11. AudioStreamPlayer node integration with scene tree
// ===========================================================================

#[test]
fn audio_node_in_scene_tree_with_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut audio = Node::new("BGMusic", "AudioStreamPlayer");
    audio.set_property("volume_db", Variant::Float(-6.0));
    audio.set_property("bus", Variant::String("Music".into()));
    audio.set_property("autoplay", Variant::Bool(true));

    let id = tree.add_child(root, audio).unwrap();
    let node = tree.get_node(id).unwrap();

    assert_eq!(node.get_property("volume_db"), Variant::Float(-6.0));
    assert_eq!(node.get_property("bus"), Variant::String("Music".into()));
    assert_eq!(node.get_property("autoplay"), Variant::Bool(true));
}

#[test]
fn multiple_audio_nodes_in_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let types = [
        "AudioStreamPlayer",
        "AudioStreamPlayer2D",
        "AudioStreamPlayer3D",
    ];

    for (i, typ) in types.iter().enumerate() {
        let node = Node::new(&format!("Audio_{i}"), *typ);
        tree.add_child(root, node).unwrap();
    }

    assert_eq!(tree.node_count(), 4); // root + 3 audio nodes
}

#[test]
fn audio_node_group_management() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let sfx1 = Node::new("SFX1", "AudioStreamPlayer");
    let sfx2 = Node::new("SFX2", "AudioStreamPlayer");
    let music = Node::new("BGM", "AudioStreamPlayer");

    let id1 = tree.add_child(root, sfx1).unwrap();
    let id2 = tree.add_child(root, sfx2).unwrap();
    let id3 = tree.add_child(root, music).unwrap();

    let _ = tree.add_to_group(id1, "sfx");
    let _ = tree.add_to_group(id2, "sfx");
    let _ = tree.add_to_group(id3, "music");

    let sfx_group = tree.get_nodes_in_group("sfx");
    assert_eq!(sfx_group.len(), 2);

    let music_group = tree.get_nodes_in_group("music");
    assert_eq!(music_group.len(), 1);
}

// ===========================================================================
// 12. WAV decode + playback pipeline
// ===========================================================================

#[test]
fn wav_decode_synthetic_plays_through_server() {
    // Create a minimal valid WAV in memory
    let sample_rate: u32 = 44100;
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let num_frames: u32 = 4410; // 0.1 seconds

    let data_size = num_frames * channels as u32 * (bits_per_sample as u32 / 8);
    let mut wav_data: Vec<u8> = Vec::new();

    // RIFF header
    wav_data.extend_from_slice(b"RIFF");
    wav_data.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav_data.extend_from_slice(b"WAVE");

    // fmt chunk
    wav_data.extend_from_slice(b"fmt ");
    wav_data.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav_data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav_data.extend_from_slice(&channels.to_le_bytes());
    wav_data.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    wav_data.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * bits_per_sample / 8;
    wav_data.extend_from_slice(&block_align.to_le_bytes());
    wav_data.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    wav_data.extend_from_slice(b"data");
    wav_data.extend_from_slice(&data_size.to_le_bytes());

    // Write a 440 Hz sine wave as 16-bit PCM
    for i in 0..num_frames {
        let t = i as f32 / sample_rate as f32;
        let sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
        let i16_sample = (sample * 32767.0) as i16;
        wav_data.extend_from_slice(&i16_sample.to_le_bytes());
    }

    // Decode
    let sample_buf = gdaudio::decode_wav(&wav_data).unwrap();
    assert_eq!(sample_buf.sample_rate, 44100);
    assert_eq!(sample_buf.channels, 1);
    assert_eq!(sample_buf.samples.len(), 4410);

    // Convert to AudioBuffer for server playback
    let buffer = AudioBuffer {
        samples: sample_buf.samples,
        sample_rate: sample_buf.sample_rate,
        channels: sample_buf.channels,
    };

    // Play through server
    let mut server = AudioServer::new();
    let id = server.play(buffer);
    assert!(server.is_playing(id));

    let output = server.mix(1024);
    assert!(!output.is_empty());

    // Verify output contains non-zero audio (sine wave)
    let has_signal = output.iter().any(|&s| s.abs() > 0.01);
    assert!(has_signal, "decoded WAV must produce audible output");
}

#[test]
fn wav_decode_silence_produces_silence() {
    let sample_rate: u32 = 44100;
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let num_frames: u32 = 1000;

    let data_size = num_frames * channels as u32 * (bits_per_sample as u32 / 8);
    let mut wav_data: Vec<u8> = Vec::new();

    wav_data.extend_from_slice(b"RIFF");
    wav_data.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav_data.extend_from_slice(b"WAVE");
    wav_data.extend_from_slice(b"fmt ");
    wav_data.extend_from_slice(&16u32.to_le_bytes());
    wav_data.extend_from_slice(&1u16.to_le_bytes());
    wav_data.extend_from_slice(&channels.to_le_bytes());
    wav_data.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    wav_data.extend_from_slice(&byte_rate.to_le_bytes());
    wav_data.extend_from_slice(&(channels * bits_per_sample / 8).to_le_bytes());
    wav_data.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav_data.extend_from_slice(b"data");
    wav_data.extend_from_slice(&data_size.to_le_bytes());

    // All zero samples (silence)
    for _ in 0..num_frames {
        wav_data.extend_from_slice(&0i16.to_le_bytes());
    }

    let sample_buf = gdaudio::decode_wav(&wav_data).unwrap();
    let buffer = AudioBuffer {
        samples: sample_buf.samples,
        sample_rate: sample_buf.sample_rate,
        channels: sample_buf.channels,
    };
    let mut server = AudioServer::new();
    server.play(buffer);
    let output = server.mix(500);

    let max = output.iter().copied().fold(0.0f32, |a, b| a.max(b.abs()));
    assert!(
        max < 1e-6,
        "silence WAV must produce silence output: max={max}"
    );
}
