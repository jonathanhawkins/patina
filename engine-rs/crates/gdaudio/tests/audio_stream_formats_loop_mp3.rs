//! pat-nzgdu.29 — audio stream formats: real MP3 decode + WAV forward loop.
//!
//! Asserts (1) a real .mp3 asset decodes to a non-silent `AudioBuffer` via the
//! public decode API (the minimp3 path from pat-p0s82.13), and (2) a
//! forward-looping stream with `loop_begin`/`loop_end` set keeps producing
//! audio past its nominal length, wrapping at `loop_end` back to `loop_begin`.

use gdaudio::{decode_mp3, AudioStreamPlayback, LoopMode};

/// A tiny real LAME-encoded MP3 committed as a fixture (shared with the
/// pat-p0s82.13 decode test).
static TONE_MP3: &[u8] = include_bytes!("fixtures/test_tone.mp3");

#[test]
fn audio_stream_formats_loop_mp3() {
    // 1. A real .mp3 asset decodes to a non-silent AudioBuffer.
    let buf = decode_mp3(TONE_MP3).expect("real ISO MP3 should decode");
    assert!(!buf.samples.is_empty(), "MP3 must yield PCM samples");
    assert!(
        buf.samples.iter().any(|s| s.abs() > 1e-4),
        "decoded MP3 must be non-silent"
    );

    // 2. A forward-looping stream (loop_begin/loop_end set) wraps at loop_end
    //    and keeps producing audio past its nominal length.
    let mut pb = AudioStreamPlayback::new(1.0); // 1.0 s nominal length
    pb.set_loop_mode(LoopMode::Forward);
    pb.set_loop_begin(0.2);
    pb.set_loop_end(0.8);
    pb.play();

    // Drive to 3.0 s total — three times the nominal length.
    for _ in 0..30 {
        pb.advance(0.1);
    }

    // Still emitting after exceeding its length...
    assert!(
        pb.is_playing(),
        "forward loop must keep playing past its nominal length"
    );
    // ...with the position wrapped into the loop window [loop_begin, loop_end).
    let pos = pb.get_playback_position();
    assert!(
        (0.2..0.8).contains(&pos),
        "position must wrap within [loop_begin, loop_end); got {pos}"
    );

    // Loop points are readable back (Godot loop_begin/loop_end parity).
    assert!((pb.loop_begin() - 0.2).abs() < 1e-6);
    assert!((pb.loop_end() - 0.8).abs() < 1e-6);
}
