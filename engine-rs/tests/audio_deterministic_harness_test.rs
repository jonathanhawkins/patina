//! Deterministic audio runtime test harness (pat-4jow).
//!
//! Exercises the full audio pipeline: buffer creation → AudioServer → mix →
//! NullAudioOutput. All tests are frame-exact and produce bit-reproducible
//! results — no platform audio backend is involved.

use gdaudio::{
    AudioBuffer, AudioBus, AudioMixer, AudioOutputStream, AudioSampleBuffer, AudioServer,
    AudioStreamPlayback, ChannelLayout, LoopMode, NullAudioOutput, PlaybackState,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Creates a mono DC (constant-value) AudioBuffer with the given value and frame count.
fn dc_buffer(value: f32, frames: usize) -> AudioBuffer {
    AudioBuffer {
        samples: vec![value; frames],
        sample_rate: 44100,
        channels: 1,
    }
}

/// Creates a stereo DC AudioBuffer (left and right have independent values).
fn stereo_dc_buffer(left: f32, right: f32, frames: usize) -> AudioBuffer {
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        samples.push(left);
        samples.push(right);
    }
    AudioBuffer {
        samples,
        sample_rate: 44100,
        channels: 2,
    }
}

/// Mixes N frames from the server and feeds them through a NullAudioOutput,
/// returning the raw mixed sample buffer for inspection.
fn mix_and_output(
    server: &mut AudioServer,
    frames: usize,
    output: &mut NullAudioOutput,
) -> Vec<f32> {
    let mixed = server.mix(frames);
    let buf = AudioSampleBuffer {
        sample_rate: server.output_sample_rate(),
        channels: server.output_channels(),
        channel_layout: ChannelLayout::Stereo,
        samples: mixed.clone(),
    };
    output.write_samples(&buf);
    mixed
}

// ---------------------------------------------------------------------------
// Full-pipeline tests
// ---------------------------------------------------------------------------

#[test]
fn harness_full_pipeline_dc_mono_through_null_output() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    let buf = dc_buffer(0.75, 44100); // 1 second of 0.75
    server.play(buf);

    let mixed = mix_and_output(&mut server, 256, &mut output);

    // Mono DC 0.75 → both stereo channels should be 0.75
    assert_eq!(mixed.len(), 256 * 2);
    for frame in 0..256 {
        let l = mixed[frame * 2];
        let r = mixed[frame * 2 + 1];
        assert!(
            (l - 0.75).abs() < 1e-5,
            "frame {frame} L: expected 0.75, got {l}"
        );
        assert!(
            (r - 0.75).abs() < 1e-5,
            "frame {frame} R: expected 0.75, got {r}"
        );
    }
    assert_eq!(output.frames_written, 256);
}

#[test]
fn harness_full_pipeline_stereo_preserves_channels() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    let buf = stereo_dc_buffer(0.3, 0.9, 44100);
    server.play(buf);

    let mixed = mix_and_output(&mut server, 128, &mut output);

    for frame in 0..128 {
        let l = mixed[frame * 2];
        let r = mixed[frame * 2 + 1];
        assert!((l - 0.3).abs() < 1e-5, "frame {frame} L: {l}");
        assert!((r - 0.9).abs() < 1e-5, "frame {frame} R: {r}");
    }
}

#[test]
fn harness_multi_stream_additive_mixing() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    server.play(dc_buffer(0.2, 44100));
    server.play(dc_buffer(0.3, 44100));
    server.play(dc_buffer(0.1, 44100));

    let mixed = mix_and_output(&mut server, 64, &mut output);

    // 0.2 + 0.3 + 0.1 = 0.6
    for frame in 0..64 {
        let l = mixed[frame * 2];
        assert!(
            (l - 0.6).abs() < 1e-4,
            "frame {frame}: expected 0.6, got {l}"
        );
    }
}

#[test]
fn harness_bus_volume_attenuation() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    let sfx_idx = server.mixer_mut().add_bus("SFX");
    server
        .mixer_mut()
        .get_bus_mut(sfx_idx)
        .unwrap()
        .set_volume_db(-20.0); // 0.1 linear

    let buf = dc_buffer(1.0, 44100);
    server.play_on_bus(buf, "SFX");

    let mixed = mix_and_output(&mut server, 32, &mut output);

    for frame in 0..32 {
        let l = mixed[frame * 2];
        assert!(
            (l - 0.1).abs() < 0.01,
            "frame {frame}: expected ~0.1, got {l}"
        );
    }
}

#[test]
fn harness_bus_mute_produces_silence() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    server.mixer_mut().get_bus_mut(0).unwrap().set_mute(true);
    server.play(dc_buffer(1.0, 44100));

    let mixed = mix_and_output(&mut server, 128, &mut output);

    assert!(
        mixed.iter().all(|&s| s.abs() < 1e-6),
        "muted master bus should produce silence"
    );
    assert_eq!(output.frames_written, 128);
}

#[test]
fn harness_multi_bus_isolation() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    // Master at 0 dB, SFX at -inf (muted)
    let sfx_idx = server.mixer_mut().add_bus("SFX");
    server
        .mixer_mut()
        .get_bus_mut(sfx_idx)
        .unwrap()
        .set_mute(true);

    // Stream on Master (audible) and stream on SFX (muted)
    server.play(dc_buffer(0.5, 44100));
    server.play_on_bus(dc_buffer(0.5, 44100), "SFX");

    let mixed = mix_and_output(&mut server, 64, &mut output);

    // Only the Master stream should be heard
    for frame in 0..64 {
        let l = mixed[frame * 2];
        assert!(
            (l - 0.5).abs() < 1e-4,
            "frame {frame}: expected 0.5 (SFX muted), got {l}"
        );
    }
}

#[test]
fn harness_stream_auto_cleanup_after_end() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    // 100-frame buffer (~2.3 ms at 44100)
    let id = server.play(dc_buffer(1.0, 100));
    assert!(server.is_playing(id));

    // Mix more frames than the buffer contains — should auto-stop
    let _ = mix_and_output(&mut server, 44100, &mut output);

    assert!(!server.is_playing(id));
    assert_eq!(server.active_stream_count(), 0);
}

#[test]
fn harness_sequential_mix_calls_advance_position() {
    let mut server = AudioServer::new();

    // 1 second buffer
    let id = server.play(dc_buffer(1.0, 44100));

    // Mix 256 frames twice — stream should still be alive
    let _ = server.mix(256);
    assert!(server.is_playing(id));
    let _ = server.mix(256);
    assert!(server.is_playing(id));

    // Total mixed: 512 frames out of 44100 — plenty left
    assert_eq!(server.active_stream_count(), 1);
}

#[test]
fn harness_null_output_accumulates_frames_across_calls() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    server.play(dc_buffer(0.5, 44100));

    mix_and_output(&mut server, 100, &mut output);
    mix_and_output(&mut server, 200, &mut output);
    mix_and_output(&mut server, 300, &mut output);

    assert_eq!(output.frames_written, 600);
}

#[test]
fn harness_empty_server_mix_is_silence() {
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);

    let mixed = mix_and_output(&mut server, 512, &mut output);

    assert_eq!(mixed.len(), 512 * 2);
    assert!(mixed.iter().all(|&s| s == 0.0));
    assert_eq!(output.frames_written, 512);
}

// ---------------------------------------------------------------------------
// Playback state machine deterministic tests
// ---------------------------------------------------------------------------

#[test]
fn harness_playback_state_machine_deterministic() {
    let mut pb = AudioStreamPlayback::new(10.0);

    // Stopped → Playing
    assert_eq!(pb.state(), PlaybackState::Stopped);
    pb.play();
    assert_eq!(pb.state(), PlaybackState::Playing);

    // Advance exactly 2.5 seconds
    pb.advance(2.5);
    assert!((pb.get_playback_position() - 2.5).abs() < 1e-6);

    // Playing → Paused (position preserved)
    pb.pause();
    assert_eq!(pb.state(), PlaybackState::Paused);
    assert!((pb.get_playback_position() - 2.5).abs() < 1e-6);

    // Advance while paused — no change
    pb.advance(1.0);
    assert!((pb.get_playback_position() - 2.5).abs() < 1e-6);

    // Resume → advance → stop
    pb.play();
    pb.advance(1.0);
    assert!((pb.get_playback_position() - 3.5).abs() < 1e-6);
    pb.stop();
    assert_eq!(pb.state(), PlaybackState::Stopped);
    assert_eq!(pb.get_playback_position(), 0.0);
}

#[test]
fn harness_playback_loop_deterministic_wrap() {
    let mut pb = AudioStreamPlayback::new(4.0);
    pb.set_loop_mode(LoopMode::Forward);
    pb.play();

    // Advance exactly to wrap point
    pb.advance(4.5); // 4.5 % 4.0 = 0.5
    assert!(pb.is_playing());
    assert!(
        (pb.get_playback_position() - 0.5).abs() < 1e-6,
        "pos = {}",
        pb.get_playback_position()
    );

    // Multiple wraps
    pb.advance(8.5); // 0.5 + 8.5 = 9.0, 9.0 % 4.0 = 1.0
    assert!(
        (pb.get_playback_position() - 1.0).abs() < 1e-6,
        "pos after multi-wrap = {}",
        pb.get_playback_position()
    );
}

#[test]
fn harness_playback_no_loop_stops_exactly_at_end() {
    let mut pb = AudioStreamPlayback::new(2.0);
    pb.play();

    pb.advance(2.5); // Past end
    assert_eq!(pb.state(), PlaybackState::Stopped);
    assert_eq!(pb.get_playback_position(), 0.0);
}

#[test]
fn harness_playback_seek_clamp_deterministic() {
    let mut pb = AudioStreamPlayback::new(5.0);

    pb.seek(-10.0);
    assert_eq!(pb.get_playback_position(), 0.0);

    pb.seek(3.0);
    assert!((pb.get_playback_position() - 3.0).abs() < 1e-6);

    pb.seek(100.0);
    assert!((pb.get_playback_position() - 5.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Mixer deterministic tests
// ---------------------------------------------------------------------------

#[test]
fn harness_mixer_bus_db_to_linear_precision() {
    let mut bus = AudioBus::new("Test");

    // 0 dB = 1.0
    assert!((bus.volume_linear() - 1.0).abs() < 1e-6);

    // -6 dB ≈ 0.5012
    bus.set_volume_db(-6.0);
    let expected = 10.0f32.powf(-6.0 / 20.0);
    assert!(
        (bus.volume_linear() - expected).abs() < 1e-5,
        "got {}",
        bus.volume_linear()
    );

    // -20 dB ≈ 0.1
    bus.set_volume_db(-20.0);
    assert!((bus.volume_linear() - 0.1).abs() < 1e-5);

    // -60 dB ≈ 0.001
    bus.set_volume_db(-60.0);
    assert!((bus.volume_linear() - 0.001).abs() < 1e-5);
}

#[test]
fn harness_mixer_bus_ordering_stable() {
    let mut mixer = AudioMixer::new();
    mixer.add_bus("SFX");
    mixer.add_bus("Music");
    mixer.add_bus("Voice");

    assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
    assert_eq!(mixer.get_bus(1).unwrap().name(), "SFX");
    assert_eq!(mixer.get_bus(2).unwrap().name(), "Music");
    assert_eq!(mixer.get_bus(3).unwrap().name(), "Voice");

    // Move Voice to position 1
    mixer.move_bus(3, 1);
    assert_eq!(mixer.get_bus(1).unwrap().name(), "Voice");
    assert_eq!(mixer.get_bus(2).unwrap().name(), "SFX");
    assert_eq!(mixer.get_bus(3).unwrap().name(), "Music");
}

// ---------------------------------------------------------------------------
// WAV decode roundtrip through server
// ---------------------------------------------------------------------------

#[test]
fn harness_wav_decode_roundtrip_through_server() {
    // Build a known 16-bit WAV: 4 frames of silence
    let sample_data = vec![0u8; 8]; // 4 frames × 2 bytes each
    let wav_bytes = gdaudio::wav::build_wav_bytes(44100, 1, 16, 1, &sample_data);

    // Decode with the custom decoder to verify the WAV is valid
    let custom_buf = gdaudio::decode_wav(&wav_bytes).unwrap();
    assert_eq!(custom_buf.samples.len(), 4);
    assert!(custom_buf.samples.iter().all(|&s| s.abs() < 1e-5));

    // Also decode via hound-based decoder for the AudioServer
    let server_buf = gdaudio::decode::decode_wav(&wav_bytes).unwrap();
    assert_eq!(server_buf.channels, 1);
    assert_eq!(server_buf.sample_rate, 44100);
    assert_eq!(server_buf.frame_count(), 4);

    // Play through server and verify silence output
    let mut server = AudioServer::new();
    let mut output = NullAudioOutput::new(44100, 2);
    server.play(server_buf);

    let mixed = mix_and_output(&mut server, 4, &mut output);
    assert!(
        mixed.iter().all(|&s| s.abs() < 1e-5),
        "silence WAV should produce silence output"
    );
}

#[test]
fn harness_wav_decode_nonzero_through_server() {
    // 16-bit PCM: value 16384 = 0x4000 (half positive range → ~0.5)
    let sample_data: Vec<u8> = vec![
        0x00, 0x40, // 16384
        0x00, 0x40, // 16384
    ];
    let wav_bytes = gdaudio::wav::build_wav_bytes(44100, 1, 16, 1, &sample_data);

    let server_buf = gdaudio::decode::decode_wav(&wav_bytes).unwrap();
    let mut server = AudioServer::new();
    server.play(server_buf);

    let mixed = server.mix(2);
    // Each mono sample ~0.5 → both stereo channels ~0.5
    assert!(
        (mixed[0] - 0.5).abs() < 0.02,
        "L[0]: expected ~0.5, got {}",
        mixed[0]
    );
    assert!(
        (mixed[1] - 0.5).abs() < 0.02,
        "R[0]: expected ~0.5, got {}",
        mixed[1]
    );
}

// ---------------------------------------------------------------------------
// NullAudioOutput contract tests
// ---------------------------------------------------------------------------

#[test]
fn harness_null_output_sample_rate_and_channels() {
    let output = NullAudioOutput::new(48000, 2);
    assert_eq!(output.sample_rate(), 48000);
    assert_eq!(output.channels(), 2);
}

#[test]
fn harness_null_output_mono_config() {
    let mut output = NullAudioOutput::new(22050, 1);
    let mut buf = AudioSampleBuffer::new(22050, 1);
    buf.samples = vec![0.5; 100];

    let written = output.write_samples(&buf);
    assert_eq!(written, 100);
    assert_eq!(output.frames_written, 100);
}

// ---------------------------------------------------------------------------
// AudioSampleBuffer / ChannelLayout
// ---------------------------------------------------------------------------

#[test]
fn harness_sample_buffer_duration_precision() {
    let mut buf = AudioSampleBuffer::new(44100, 2);
    // Exactly 1 second of stereo: 44100 frames × 2 channels
    buf.samples = vec![0.0; 44100 * 2];
    assert_eq!(buf.frame_count(), 44100);
    assert!((buf.duration_secs() - 1.0).abs() < 1e-5);
}

#[test]
fn harness_channel_layout_variants() {
    assert_eq!(
        AudioSampleBuffer::new(44100, 1).channel_layout,
        ChannelLayout::Mono
    );
    assert_eq!(
        AudioSampleBuffer::new(44100, 2).channel_layout,
        ChannelLayout::Stereo
    );
    assert_eq!(
        AudioSampleBuffer::new(44100, 4).channel_layout,
        ChannelLayout::Custom(4)
    );
    assert_eq!(
        AudioSampleBuffer::new(44100, 8).channel_layout,
        ChannelLayout::Custom(8)
    );
}

// ---------------------------------------------------------------------------
// Server frame-stepping determinism
// ---------------------------------------------------------------------------

#[test]
fn harness_frame_stepping_produces_consistent_output() {
    // Mix in small chunks and verify each chunk is consistent
    let mut server = AudioServer::new();
    let buf = dc_buffer(0.42, 44100);
    server.play(buf);

    for _step in 0..10 {
        let mixed = server.mix(64);
        for frame in 0..64 {
            let l = mixed[frame * 2];
            assert!(
                (l - 0.42).abs() < 1e-5,
                "inconsistent output at step {_step}, frame {frame}: {l}"
            );
        }
    }
}

#[test]
fn harness_interleaved_play_stop_determinism() {
    let mut server = AudioServer::new();

    let id1 = server.play(dc_buffer(0.3, 44100));
    let _ = server.mix(100);

    let _id2 = server.play(dc_buffer(0.2, 44100));
    let mixed = server.mix(100);

    // After adding second stream: 0.3 + 0.2 = 0.5
    assert!(
        (mixed[0] - 0.5).abs() < 1e-4,
        "expected 0.5, got {}",
        mixed[0]
    );

    // Stop first stream
    server.stop(id1);
    let mixed = server.mix(100);

    // Only second stream: 0.2
    assert!(
        (mixed[0] - 0.2).abs() < 1e-4,
        "expected 0.2 after stop, got {}",
        mixed[0]
    );
}
