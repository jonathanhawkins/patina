//! pat-0q2kx.58 acceptance: every Godot `AudioStreamWAV` loop mode reaches the
//! mixer's read cursor.
//!
//! Godot's `LoopMode` is `LOOP_DISABLED/FORWARD/PINGPONG/BACKWARD` with a
//! `loop_begin`/`loop_end` region (`doc/classes/AudioStreamWAV.xml`): a looping
//! stream keeps producing sound past its nominal length and never emits
//! `finished`, ping-pong reads the region forwards then backwards, and backward
//! reads it in reverse.
//!
//! Patina had only `None`/`Forward`, and the mixer always stepped its read
//! cursor FORWARD from the playback position — so even once the playback clock
//! ran backwards, the samples read would still have ascended.
//!
//! These drive the real `AudioServer::mix` and assert on the produced samples.

use gdaudio::server::AudioServer;
use gdaudio::stream::LoopMode;
use gdaudio::AudioBuffer;

/// Mono ramp where sample `i == i as f32`, so the direction the read cursor
/// travels is directly visible in the mixed output.
fn ramp(frames: usize) -> AudioBuffer {
    AudioBuffer {
        samples: (0..frames).map(|i| i as f32).collect(),
        sample_rate: 44100,
        channels: 1,
    }
}

/// Left-channel samples of a mixed block (the mixer writes interleaved stereo).
fn left_channel(out: &[f32], channels: usize) -> Vec<f32> {
    out.iter().step_by(channels).copied().collect()
}

/// **The acceptance test.**
#[test]
fn audio_stream_loop_modes() {
    // -- (a) a forward loop outlives its own length and never finishes --------
    {
        let mut server = AudioServer::new();
        let id = server.play(ramp(100));
        server
            .stream_mut(id)
            .expect("stream registered")
            .set_loop_mode(LoopMode::Forward);

        // 300 frames through a 100-frame stream: three times its length.
        let out = server.mix(300);
        let ch = server.output_channels() as usize;
        let left = left_channel(&out, ch);
        assert!(
            left.iter().any(|s| s.abs() > 0.0),
            "a looping stream must still be producing sound past its length"
        );
        assert!(
            server.take_finished().is_empty(),
            "a looping stream must never report finished"
        );
    }

    // -- (b) a non-looping stream finishes exactly once -----------------------
    {
        let mut server = AudioServer::new();
        let id = server.play(ramp(100));
        server.mix(300);
        assert_eq!(
            server.take_finished(),
            vec![id],
            "a non-looping stream finishes once when it runs out"
        );
        assert!(
            server.take_finished().is_empty(),
            "the finished queue is drained"
        );
    }

    // -- (c) loop_begin/loop_end restrict the wrap region ---------------------
    //
    // With the region [50, 100) of a 100-frame ramp, every sample read must come
    // from that window — never from frame 0, which is what a whole-stream wrap
    // would produce.
    {
        let mut server = AudioServer::new();
        let id = server.play(ramp(100));
        {
            let pb = server.stream_mut(id).expect("stream");
            pb.set_loop_mode(LoopMode::Forward);
            pb.set_loop_begin(50.0 / 44100.0);
            pb.set_loop_end(100.0 / 44100.0);
        }
        let ch = server.output_channels() as usize;
        // Advance past the first pass so the cursor is inside the loop region.
        server.mix(120);
        let left = left_channel(&server.mix(60), ch);
        assert!(
            left.iter().all(|&s| s >= 49.0),
            "post-wrap samples must come from the [50,100) loop window, got {:?}",
            &left[..left.len().min(8)]
        );
    }

    // -- (d) ping-pong reads forwards, then backwards -------------------------
    {
        let mut server = AudioServer::new();
        let id = server.play(ramp(100));
        server
            .stream_mut(id)
            .expect("stream")
            .set_loop_mode(LoopMode::PingPong);
        let ch = server.output_channels() as usize;

        // First block stays inside the ramp: ascending.
        let first = left_channel(&server.mix(60), ch);
        assert!(
            first[first.len() - 1] > first[0],
            "the outbound leg of a ping-pong ascends, got {:?}..{:?}",
            first[0],
            first[first.len() - 1]
        );

        // Second block crosses the far bound (60 + 60 > 100), which flips the
        // phase in `advance`; the direction is per-block, so the reversal shows
        // up in the block AFTER the crossing.
        server.mix(60);

        let second = left_channel(&server.mix(60), ch);
        assert!(
            second[second.len() - 1] < second[0],
            "the return leg of a ping-pong descends, got {:?}..{:?}",
            second[0],
            second[second.len() - 1]
        );
        assert!(
            server.take_finished().is_empty(),
            "a ping-pong loop never finishes"
        );
    }

    // -- (e) a backward loop reads descending ---------------------------------
    {
        let mut server = AudioServer::new();
        let id = server.play(ramp(100));
        server
            .stream_mut(id)
            .expect("stream")
            .set_loop_mode(LoopMode::Backward);
        let ch = server.output_channels() as usize;

        let left = left_channel(&server.mix(40), ch);
        assert!(
            left[left.len() - 1] < left[0],
            "a backward loop reads the sample in reverse, got {:?}..{:?}",
            left[0],
            left[left.len() - 1]
        );
        assert!(
            server.take_finished().is_empty(),
            "a backward loop never finishes"
        );
    }
}
