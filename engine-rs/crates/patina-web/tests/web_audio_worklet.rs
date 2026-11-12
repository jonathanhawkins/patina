//! End-to-end: the engine's mixer feeding an `AudioWorkletProcessor`
//! (pat-u3l8c.112).
//!
//! Before this bead the web build had no audio at all — `gdaudio` did not
//! compile for `wasm32` (its `PlatformAudioOutput` re-export resolved to
//! nothing once `cpal` was target-gated), so `patina-web` linked no mixer and
//! the browser had nothing to pull from.
//!
//! These tests drive the same chain the browser drives, minus the browser:
//!
//! ```text
//!   decode_auto(wav) -> AudioServer::mix -> WebAudioOutput (resample+ring)
//!     -> pull_block(128)  ==  what AudioWorkletProcessor::process() receives
//! ```
//!
//! The simulated worklet loop below asks for 128-frame quanta exactly as a
//! browser does, so a regression that makes `process()` receive silence, a
//! short buffer, or unboundedly delayed audio fails here rather than in a
//! listening test.

use patina_web::audio::{WebAudioEngine, PROCESSOR_NAME, RENDER_QUANTUM_FRAMES};

/// The engine's mix rate — Godot's `audio/driver/mix_rate` default.
const MIX_RATE: u32 = 44_100;
/// A rate the browser might pick. Deliberately *different* from `MIX_RATE`:
/// equal rates would let a missing resample step pass unnoticed.
const CONTEXT_RATE: u32 = 48_000;

/// Builds a 16-bit mono WAV of a sine tone, the way a real asset arrives.
fn wav_tone(hz: f32, seconds: f32, rate: u32) -> Vec<u8> {
    let frames = (seconds * rate as f32) as usize;
    let data_size = frames * 2;
    let mut wav = Vec::with_capacity(44 + data_size);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_size) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_size as u32).to_le_bytes());
    let step = std::f32::consts::TAU * hz / rate as f32;
    for i in 0..frames {
        let s = ((step * i as f32).sin() * 0.6 * 32767.0) as i16;
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

/// Runs `quanta` worklet `process()` calls, returning every sample the worklet
/// received, in order.
fn drive_worklet(engine: &mut WebAudioEngine, quanta: usize) -> Vec<f32> {
    let channels = engine.channels() as usize;
    let mut received = Vec::with_capacity(quanta * RENDER_QUANTUM_FRAMES * channels);
    for _ in 0..quanta {
        let block = engine.pull_block(RENDER_QUANTUM_FRAMES);
        assert_eq!(
            block.len(),
            RENDER_QUANTUM_FRAMES * channels,
            "process() must be handed a full render quantum every call"
        );
        received.extend_from_slice(&block);
    }
    received
}

/// The acceptance: a real decoded asset played through the engine's mixer comes
/// out of the worklet's pull as audible audio, at the browser's rate, in full
/// quanta, without latency growing.
#[test]
fn mixer_feeds_audio_worklet_blocks() {
    let mut engine = WebAudioEngine::new(CONTEXT_RATE, MIX_RATE);
    assert_eq!(engine.mix_rate(), MIX_RATE, "the mixer keeps its own rate");
    assert_eq!(
        engine.context_rate(),
        CONTEXT_RATE,
        "blocks are produced at the rate the browser chose"
    );

    let wav = wav_tone(440.0, 0.25, MIX_RATE);
    engine
        .play_encoded(&wav)
        .expect("a RIFF/PCM WAV should decode and play through the mixer");

    // Half a second of browser render quanta — long enough to contain the whole
    // 0.25 s tone plus the silence after it.
    let quanta = (CONTEXT_RATE as usize / 2).div_ceil(RENDER_QUANTUM_FRAMES);
    let received = drive_worklet(&mut engine, quanta);

    let channels = engine.channels() as usize;
    let peak = received.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak > 0.3,
        "the worklet only ever received silence (peak {peak}) — the mixer is not reaching it"
    );

    // Where the audio stops tells us how long it played *at the context rate*.
    // A missing resample would stretch or squash this by the 48000/44100 ratio,
    // which is the audible "everything is flat and slow" bug.
    let last_audible = received
        .iter()
        .rposition(|s| s.abs() > 0.05)
        .expect("some audible sample");
    let audible_frames = last_audible / channels + 1;
    let expected = (0.25 * CONTEXT_RATE as f32) as usize;
    let drift = (audible_frames as f32 - expected as f32).abs() / expected as f32;
    assert!(
        drift < 0.05,
        "audible span {audible_frames} frames vs {expected} expected at the context rate \
         ({:.1}% off) — the mix rate is not being resampled to the context rate",
        drift * 100.0
    );

    // Latency must not creep: after driving well past the source's length the
    // ring is still shallow, not seconds deep.
    assert!(
        engine.queued_frames() <= 8 * RENDER_QUANTUM_FRAMES,
        "queue grew to {} frames — audio would lag the picture",
        engine.queued_frames()
    );
    assert_eq!(
        engine.frames_pulled(),
        (quanta * RENDER_QUANTUM_FRAMES) as u64
    );
}

/// The JS the page evaluates has to agree with the Rust side about the
/// processor name and the channel interleaving, or `new AudioWorkletNode`
/// throws `InvalidStateError` and nothing is ever heard.
#[test]
fn worklet_glue_matches_the_backend_contract() {
    let engine = WebAudioEngine::new(CONTEXT_RATE, MIX_RATE);
    let glue = engine.worklet_glue();

    assert!(
        glue.contains(PROCESSOR_NAME),
        "the glue must register the name the backend reports"
    );
    assert!(glue.contains("registerProcessor"));
    assert!(glue.contains("AudioWorkletNode"));
    assert!(
        glue.contains("outputChannelCount"),
        "the node must be built with this engine's channel count"
    );
    // ⚠️ The node's channel count must be the width `pull_block` interleaves
    // at, which the mixer owns — not a constant written twice. A mismatch does
    // not throw; it de-interleaves the ring against the wrong stride and the
    // stereo image comes apart.
    assert!(
        glue.contains(&format!("PATINA_CHANNELS = {}", engine.channels())),
        "the worklet's channel count must follow the engine's, which is {}",
        engine.channels()
    );
    // ⚠️ Without a gesture-driven resume the whole graph runs and stays
    // inaudible — browsers start every AudioContext suspended.
    assert!(glue.contains("pointerdown") && glue.contains("keydown"));
    assert!(glue.contains("context.resume()"));
}

/// With nothing playing, `process()` still has to be handed a full block. A
/// short buffer is a graph error; garbage is an audible click.
#[test]
fn an_idle_mixer_produces_silent_full_blocks() {
    let mut engine = WebAudioEngine::new(CONTEXT_RATE, MIX_RATE);
    let received = drive_worklet(&mut engine, 16);
    assert!(
        received.iter().all(|s| *s == 0.0),
        "an idle mixer must produce silence, not garbage"
    );
}

/// Audio is inaudible until a user gesture resumes the context, and the backend
/// has to be able to report which state it is in — the demo page shows it.
#[test]
fn the_backend_reports_the_gesture_lock_state() {
    let mut engine = WebAudioEngine::new(CONTEXT_RATE, MIX_RATE);
    assert!(!engine.is_unlocked());
    // Mixing still works while locked; only the browser output is silent.
    engine.play_tone(220.0, 0.1, 0.5);
    let block = engine.pull_block(RENDER_QUANTUM_FRAMES);
    assert!(block.iter().any(|s| s.abs() > 0.01));

    engine.mark_unlocked();
    assert!(engine.is_unlocked());
}
