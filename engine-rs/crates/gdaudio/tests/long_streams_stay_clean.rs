//! Music crackled in Dodge the Creeps while short sound effects sounded fine.
//! Three independent causes, each pinned here:
//!
//! 1. The playhead was an `f32` count of seconds, and every read position was
//!    derived from it in f32. Minutes into a track the cursor sits millions of
//!    source samples in, where an f32 resolves only WHOLE samples: the linear
//!    interpolation fraction vanished and every sample snapped to a neighbour.
//!    Measured on a 440 Hz tone: the waveform's curvature was 12x rougher at
//!    200 s than at 10 s. A one-second effect never got far enough to notice.
//! 2. The device queue popped with `Vec::remove(0)` (O(n) per sample, inside
//!    the real-time callback) and wrote silence whenever the 60 Hz producer and
//!    the device clock beat against each other.
//! 3. (bridge, `gdscene`) the server mixed at the project's 44.1 kHz into a
//!    48 kHz device, so the device drained ~9% faster than the game filled it.

use gdaudio::decode::AudioBuffer;
use gdaudio::server::AudioServer;
use gdaudio::SharedRing;

/// Max |second difference| of `out[a..b]` — near-zero curvature for a pure
/// tone, large wherever samples jump.
fn roughness(out: &[f32], a: usize, b: usize) -> f32 {
    (a.max(2)..b)
        .map(|i| (out[i] - 2.0 * out[i - 1] + out[i - 2]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_tone_minutes_in_is_as_smooth_as_at_the_start() {
    let src_rate = 44_100u32;
    let secs = 240usize;
    let samples: Vec<f32> = (0..src_rate as usize * secs)
        .map(|i| {
            (std::f64::consts::TAU * 440.0 * i as f64 / f64::from(src_rate)).sin() as f32 * 0.5
        })
        .collect();
    let mut server = AudioServer::new();
    server.set_output_sample_rate(48_000);
    server.set_output_channels(1);
    server.play(AudioBuffer {
        samples,
        sample_rate: src_rate,
        channels: 1,
    });

    // The game's own block size: one 60 Hz frame at 48 kHz.
    let mut out = Vec::new();
    for _ in 0..(48_000 * (secs - 1) / 800) {
        out.extend(server.mix(800));
    }
    let early = roughness(&out, 10 * 48_000, 20 * 48_000);
    let late = roughness(&out, 200 * 48_000, 210 * 48_000);
    assert!(early > 0.0, "the tone is playing");
    // With the f32 cursor `late` was ~0.031 against ~0.0025 early (12x).
    assert!(
        late < early * 1.5,
        "200 s in the same tone must be as smooth as 10 s in: early {early}, late {late}"
    );
}

#[test]
fn a_jittery_sixty_hz_producer_never_starves_the_device() {
    // 60 Hz pushes of 800 frames vs a device pulling 512-frame callbacks at
    // 48 kHz: the two clocks beat, so the queue's level swings every frame.
    let (rate, ch) = (48_000u32, 2u16);
    let mut ring = SharedRing::default();
    let block = vec![0.25f32; 800 * ch as usize];
    let mut out = vec![0.0f32; 512 * ch as usize];
    let (mut produced, mut consumed) = (0.0f64, 0.0f64);
    // 20 s of simulated time in 1 ms steps.
    for ms in 0..20_000 {
        let t = f64::from(ms) / 1000.0;
        while produced <= t {
            ring.push(&block, rate, ch);
            produced += 1.0 / 60.0;
        }
        while consumed + 512.0 / f64::from(rate) <= t {
            ring.pull(&mut out, rate, ch);
            consumed += 512.0 / f64::from(rate);
        }
    }
    assert_eq!(
        ring.underruns, 0,
        "a steady producer must never leave a gap"
    );
    assert!(
        ring.frames_consumed > 48_000 * 19,
        "the device actually played"
    );
}

#[test]
fn the_backlog_is_bounded_and_whole_frames_are_dropped() {
    let (rate, ch) = (48_000u32, 2u16);
    let mut ring = SharedRing::default();
    // Interleaved L=1, R=-1, so a channel swap would be visible.
    let block: Vec<f32> = (0..2000)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    for _ in 0..100 {
        ring.push(&block, rate, ch);
    }
    let cap = (gdaudio::output::MAX_BACKLOG_SECS * rate as f32) as usize * ch as usize;
    assert!(
        ring.samples.len() <= cap,
        "queued {} > cap {cap}",
        ring.samples.len()
    );
    assert_eq!(
        ring.samples.front(),
        Some(&1.0),
        "the queue still starts on a LEFT sample"
    );
}

#[test]
fn an_underrun_reprimes_instead_of_clicking_per_callback() {
    let (rate, ch) = (48_000u32, 1u16);
    let mut ring = SharedRing::default();
    let mut out = vec![9.0f32; 512];
    // Not enough queued to start: silence, and no underrun is counted.
    ring.push(&vec![0.5; 100], rate, ch);
    ring.pull(&mut out, rate, ch);
    assert!(out.iter().all(|s| *s == 0.0));
    assert_eq!(ring.underruns, 0, "priming is not an underrun");
}
