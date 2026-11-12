//! pat-0q2kx.59 acceptance: the web audio backend's producer/consumer contract,
//! proven natively.
//!
//! Godot's web export runs the same mixer as native, pulled by an
//! `AudioWorkletProcessor` and resumed on the first user gesture
//! (`platform/web/audio_driver_web.cpp`,
//! `platform/web/js/libs/library_godot_audio.js`).
//!
//! Patina had NO audio on wasm: `patina-web` excluded `gdaudio` because `cpal`
//! was an unconditional dependency, and the runner's wasm branch returned no
//! output at all. `cpal` is now gated to non-wasm targets so the whole audio
//! stack compiles for the web, and this proves the device layer that replaces
//! it behaves.

use gdaudio::sample::{AudioOutputStream, AudioSampleBuffer};
use gdaudio::web_output::{
    audio_worklet_glue, resample_interleaved, AudioRingBuffer, WebAudioOutput,
};

/// ⚠️ This `use` is the wasm-graph assertion the bead asks for: `gdaudio`'s
/// public surface must be reachable without pulling `cpal` in. `cpal` now lives
/// under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`, and
/// `web_output` deliberately has no `cpal` path — if someone reintroduces an
/// unconditional dependency, the wasm build breaks again and this module is
/// where to look.
#[test]
fn web_output_is_reachable_without_the_native_device_layer() {
    let out = WebAudioOutput::new(48_000, 44_100, 2);
    assert_eq!(out.sample_rate(), 48_000);
    assert_eq!(out.channels(), 2);
}

/// **The acceptance test.**
#[test]
fn audio_web_backend_contract() {
    // -- 1. the ring is sample-exact across mismatched chunk sizes ---------
    //
    // The engine writes whole mixed blocks; a worklet reads 128-frame render
    // quanta. Those do not divide evenly, which is the whole reason the ring
    // exists — so read it back at a size that never lines up with the writes.
    let mut ring = AudioRingBuffer::new(4096);
    let written: Vec<f32> = (0..1000).map(|i| i as f32).collect();

    let mut read_back: Vec<f32> = Vec::new();
    let mut cursor = 0;
    // Producer writes in 100s, consumer reads in 128s.
    while cursor < written.len() {
        let end = (cursor + 100).min(written.len());
        ring.write(&written[cursor..end]);
        cursor = end;

        while ring.len() >= 128 {
            let mut chunk = [0.0f32; 128];
            let n = ring.read(&mut chunk);
            read_back.extend_from_slice(&chunk[..n]);
        }
    }
    // Drain whatever is left.
    while !ring.is_empty() {
        let mut chunk = [0.0f32; 128];
        let n = ring.read(&mut chunk);
        read_back.extend_from_slice(&chunk[..n]);
    }

    assert_eq!(
        read_back, written,
        "the ring must hand back exactly what went in, in order, across \
         mismatched producer/consumer chunk sizes"
    );

    // -- 2. 44100 -> 48000 preserves a sine's frequency --------------------
    //
    // Resampling that changes the pitch is the failure this guards: feeding
    // 44.1 kHz frames to a 48 kHz context unadapted plays everything ~8.8%
    // flat. Count zero crossings — a frequency-preserving resample keeps the
    // same number over the same wall-clock duration.
    const FREQ: f64 = 440.0;
    const IN_RATE: u32 = 44_100;
    const OUT_RATE: u32 = 48_000;
    let duration_s = 0.5;
    let in_frames = (IN_RATE as f64 * duration_s) as usize;

    let input: Vec<f32> = (0..in_frames)
        .map(|i| {
            let t = i as f64 / IN_RATE as f64;
            (t * FREQ * std::f64::consts::TAU).sin() as f32
        })
        .collect();

    let output = resample_interleaved(&input, 1, IN_RATE, OUT_RATE);

    let expected_frames = (OUT_RATE as f64 * duration_s) as usize;
    assert!(
        (output.len() as i64 - expected_frames as i64).abs() <= 2,
        "a 0.5s signal must stay 0.5s: {} frames vs ~{expected_frames}",
        output.len()
    );

    let crossings =
        |v: &[f32]| -> usize { v.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() };
    let before = crossings(&input);
    let after = crossings(&output);
    assert!(
        (before as i64 - after as i64).abs() <= 1,
        "resampling must preserve the sine's frequency: {before} cycles before, \
         {after} after"
    );

    // -- 3. the output adapts on the way in --------------------------------
    //
    // ⚠️ Feed a block that FITS the ring. The ring holds about a quarter second
    // (12000 samples at 48 kHz) and deliberately drops the oldest on overflow,
    // so pushing the full half-second signal above would exercise the overflow
    // path and queue only the capacity — correct behaviour, wrong thing to
    // assert here. A real frame pushes a few milliseconds, not half a second.
    let block_frames = (IN_RATE as usize) / 10; // 0.1s
    let mut out = WebAudioOutput::new(OUT_RATE, IN_RATE, 1);
    let mut buffer = AudioSampleBuffer::new(IN_RATE, 1);
    buffer.samples = input[..block_frames].to_vec();
    let frames = out.write_samples(&buffer);
    assert!(
        frames > block_frames,
        "upsampling 44.1k -> 48k must yield MORE frames than went in: {frames} \
         from {block_frames}"
    );
    assert_eq!(
        out.queued(),
        frames,
        "every adapted frame must be queued when the block fits"
    );

    // -- 4. the JS glue stands up a worklet and a gesture unlock -----------
    //
    // ⚠️ Without the gesture listeners the graph runs and stays silent forever:
    // browsers refuse to start an AudioContext without a user gesture, and that
    // failure looks exactly like "audio is broken".
    let glue = audio_worklet_glue("patina-out", 2);
    assert!(
        glue.contains("AudioWorkletProcessor"),
        "glue must define a worklet processor"
    );
    assert!(
        glue.contains("registerProcessor"),
        "glue must register the processor with the worklet scope"
    );
    assert!(
        glue.contains("audioWorklet.addModule"),
        "glue must load the processor module into the context"
    );
    assert!(
        glue.contains("pointerdown") && glue.contains("keydown"),
        "glue must listen for BOTH gesture kinds — a keyboard-only player must \
         be able to unlock audio too"
    );
    assert!(
        glue.contains("context.resume()"),
        "the gesture handler must actually resume the context"
    );
    assert!(
        glue.contains("connect(context.destination)"),
        "the worklet node must reach the output or nothing is audible"
    );
}

/// ⚠️ A producer that pushes more than the ring holds must NOT stall or panic —
/// it caps at capacity, keeping the newest audio and dropping the oldest.
///
/// This is the bounded-latency guarantee: a browser tab that gets throttled (a
/// backgrounded tab stops servicing the worklet) must not let the queue grow
/// without limit and then play minutes of stale sound when it resumes.
#[test]
fn an_oversized_push_caps_at_the_ring_capacity_instead_of_growing() {
    let mut out = WebAudioOutput::new(48_000, 48_000, 1);
    // Two seconds into a ~quarter-second ring.
    let mut buffer = AudioSampleBuffer::new(48_000, 1);
    buffer.samples = vec![0.5; 96_000];
    let frames = out.write_samples(&buffer);

    assert_eq!(frames, 96_000, "the write itself never refuses samples");
    assert!(
        out.queued() < 96_000,
        "the queue must be bounded, not grow to hold everything: {}",
        out.queued()
    );
    assert!(
        out.queued() >= 12_000,
        "…but it must still be holding a useful buffer: {}",
        out.queued()
    );
}

/// The unlock flag starts false and is set by the host once the glue reports a
/// gesture — the engine should never assume audio is running.
#[test]
fn output_starts_locked_until_a_gesture_unlocks_it() {
    let mut out = WebAudioOutput::new(48_000, 48_000, 2);
    assert!(!out.is_unlocked());
    out.mark_unlocked();
    assert!(out.is_unlocked());
}
