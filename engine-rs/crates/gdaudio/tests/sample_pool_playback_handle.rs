//! pat-nzgdu.187 — a registered sample can be triggered, and its playback handle
//! reports playing state.
//!
//! The acceptance's behavioural clause. Note what this deliberately does NOT
//! claim: Godot's sample path is a *driver* feature whose base implementation
//! returns a constant `false` (`servers/audio/audio_server.h:143`), and the only
//! real implementation in the engine is the web driver, which asks the browser.
//! This is the engine-side registry a driver sits behind — it answers from real
//! state rather than a constant, which is exactly the part worth testing.

use gdaudio::sample::AudioSampleBuffer;
use gdaudio::sample_pool::{LoopMode, RegisteredSample, SampleId, SamplePlaybackId, SamplePool};

/// A buffer of `frames` mono frames at 100 Hz, so 100 frames == 1 second.
fn buffer(frames: usize) -> AudioSampleBuffer {
    let mut b = AudioSampleBuffer::new(100, 1);
    b.samples = vec![0.0; frames];
    // `frame_count()` is `samples.len() / channels`, so this holds ONLY because
    // the buffer is mono. Making it stereo would silently halve every duration
    // below and quietly break the retire/wrap tests.
    debug_assert_eq!(b.frame_count(), frames, "helper must yield `frames` frames");
    b
}

/// The acceptance: register → trigger → the handle reports playing.
#[test]
fn audiosample_register_and_playback_handle() {
    let mut pool = SamplePool::new();

    let id = pool.register(RegisteredSample::new(buffer(100)));
    assert!(pool.is_registered(id), "the sample is in the pool");
    assert_eq!(pool.registered_count(), 1);

    // Registering does not start anything — that is the whole point of a pool.
    assert_eq!(pool.active_count(), 0, "registering must not play");

    let pb = pool
        .start(id)
        .expect("a registered sample can be triggered");
    assert!(
        pool.is_playing(pb),
        "the playback handle must report playing state"
    );
    assert_eq!(pool.active_count(), 1);

    // ...and it stops when told to.
    assert!(pool.stop(pb));
    assert!(
        !pool.is_playing(pb),
        "a stopped handle is no longer playing"
    );
    assert_eq!(pool.active_count(), 0);
    // The sample stays registered: stopping a playback is not unregistering.
    assert!(pool.is_registered(id), "the buffer is still in the pool");
}

/// ⚠️ Triggering a sample the driver never received must NOT mint a handle. A
/// handle that reports "playing" for a sample that does not exist is worse than
/// no handle: it would look like working audio with silence.
#[test]
fn unregistered_sample_cannot_be_triggered() {
    let mut pool = SamplePool::new();
    assert!(
        pool.start(SampleId(999)).is_none(),
        "an unregistered sample cannot play"
    );
    assert_eq!(pool.active_count(), 0);

    // An unknown handle is not playing, and asking must not panic.
    assert!(!pool.is_playing(SamplePlaybackId(999)));
    assert!(!pool.stop(SamplePlaybackId(999)));
    assert_eq!(pool.playback_position(SamplePlaybackId(999)), 0.0);
}

/// A one-shot must stop reporting itself as playing once it drains — otherwise
/// `is_playing` would be a constant `true` and mean nothing.
#[test]
fn one_shot_retires_when_it_drains() {
    let mut pool = SamplePool::new();
    let id = pool.register(RegisteredSample::new(buffer(100))); // 1 second
    let pb = pool.start(id).unwrap();

    pool.advance(50);
    assert!(pool.is_playing(pb), "still playing halfway through");
    assert!(
        (pool.playback_position(pb) - 0.5).abs() < 1e-6,
        "50 frames at 100 Hz is 0.5s, got {}",
        pool.playback_position(pb)
    );

    pool.advance(60); // past the end
    assert!(
        !pool.is_playing(pb),
        "a drained one-shot must retire, not report playing forever"
    );
    assert_eq!(pool.active_count(), 0);
}

/// ⚠️ A looping sample must NOT retire — that is the difference from a one-shot,
/// and the reason `LoopMode` exists.
#[test]
fn looping_sample_keeps_playing_past_the_end() {
    let mut pool = SamplePool::new();
    let id = pool.register(RegisteredSample::new(buffer(100)).with_loop(LoopMode::Forward));
    let pb = pool.start(id).unwrap();

    pool.advance(250); // two and a half times through
    assert!(
        pool.is_playing(pb),
        "a looping sample plays until stopped, never drains"
    );
    // ...and the playhead wrapped rather than running off the end.
    let pos = pool.playback_position(pb);
    assert!(
        (0.0..1.0).contains(&pos),
        "the playhead must wrap into the buffer, got {pos}"
    );
    assert!(
        (pos - 0.5).abs() < 1e-6,
        "250 frames into a 100-frame loop is frame 50 (0.5s), got {pos}"
    );
}

/// A loop region must wrap into `[loop_begin, loop_end)`, not back to frame 0.
///
/// ⚠️ `loop_begin`/`loop_end` are `pub` and the wrap that reads them is the most
/// error-prone line in the module, yet every other test leaves them at 0 — so
/// without this the `loop_end != 0` branch and the `loop_begin` clamp are never
/// executed at all.
#[test]
fn loop_region_wraps_within_its_bounds() {
    let mut pool = SamplePool::new();
    let mut s = RegisteredSample::new(buffer(100)).with_loop(LoopMode::Forward);
    s.loop_begin = 50;
    s.loop_end = 100;
    let id = pool.register(s);

    // The registered sample keeps what it was given.
    let got = pool.get(id).expect("registered");
    assert_eq!(got.loop_begin, 50);
    assert_eq!(got.loop_end, 100);

    let pb = pool.start(id).unwrap();
    pool.advance(250);
    assert!(pool.is_playing(pb), "a looping sample never drains");

    // begin=50, end=100, span=50 -> 50 + ((250 - 50) % 50) = 50 -> 0.5s.
    let pos = pool.playback_position(pb);
    assert!(
        (pos - 0.5).abs() < 1e-6,
        "the playhead must wrap inside the loop region, got {pos}"
    );
    assert!(
        pos >= 0.5,
        "it must never fall back before loop_begin (0.5s), got {pos}"
    );
}

/// ⚠️ Paused is not stopped: a paused playback is still active and must resume
/// where it left off. Retiring it on pause would silently kill held sounds.
#[test]
fn paused_playback_stays_active_and_does_not_advance() {
    let mut pool = SamplePool::new();
    let id = pool.register(RegisteredSample::new(buffer(100)));
    let pb = pool.start(id).unwrap();

    pool.advance(20);
    let at_pause = pool.playback_position(pb);
    assert!(pool.set_paused(pb, true));
    assert!(pool.is_paused(pb));

    pool.advance(500); // far past the end
    assert!(
        pool.is_playing(pb),
        "a paused playback is still active — it has not finished"
    );
    assert_eq!(
        pool.playback_position(pb),
        at_pause,
        "a paused playhead must not move"
    );

    // Resuming picks up where it stopped.
    assert!(pool.set_paused(pb, false));
    pool.advance(10);
    assert!(
        pool.playback_position(pb) > at_pause,
        "resuming must advance again"
    );
}

/// Unregistering a sample must stop what it was playing — a driver cannot keep
/// playing a buffer that has been freed.
#[test]
fn unregistering_stops_its_playbacks() {
    let mut pool = SamplePool::new();
    let id = pool.register(RegisteredSample::new(buffer(100)));
    let a = pool.start(id).unwrap();
    let b = pool.start(id).unwrap();
    assert_eq!(pool.active_count(), 2, "one sample can play twice at once");
    assert_ne!(a, b, "each trigger gets its own handle");

    assert!(pool.unregister(id));
    assert!(!pool.is_registered(id));
    assert!(
        !pool.is_playing(a),
        "freeing the buffer stops its playbacks"
    );
    assert!(!pool.is_playing(b));
    assert_eq!(pool.active_count(), 0);
}

/// Pitch scale changes how fast the playhead runs — the sample path's only
/// speed control, since it never touches the mixer.
#[test]
fn pitch_scale_changes_playback_speed() {
    let mut pool = SamplePool::new();
    let id = pool.register(RegisteredSample::new(buffer(1000)));

    let normal = pool.start(id).unwrap();
    let fast = pool.start(id).unwrap();
    assert!(pool.set_pitch_scale(fast, 2.0));

    pool.advance(100);
    let n = pool.playback_position(normal);
    let f = pool.playback_position(fast);
    assert!(
        (f - n * 2.0).abs() < 1e-6,
        "double pitch must advance twice as far: {n} vs {f}"
    );
}

/// Godot's default is a one-shot: a sample must not loop unless asked.
#[test]
fn default_loop_mode_is_disabled() {
    let s = RegisteredSample::new(buffer(10));
    assert_eq!(
        s.loop_mode,
        LoopMode::Disabled,
        "Godot's AudioSample defaults to LOOP_DISABLED"
    );
    assert_eq!(LoopMode::default(), LoopMode::Disabled);
    // Discriminants match Godot's C++ declaration order — the driver and the
    // engine agree on them across the FFI boundary.
    assert_eq!(LoopMode::Disabled as i32, 0);
    assert_eq!(LoopMode::Forward as i32, 1);
    assert_eq!(LoopMode::PingPong as i32, 2);
    assert_eq!(LoopMode::Backward as i32, 3);
}
