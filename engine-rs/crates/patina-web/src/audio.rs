//! The web audio backend: the engine's mixer feeding an `AudioWorklet`
//! (pat-u3l8c.112).
//!
//! Godot's web export runs the *same* `AudioServer` as native and lets an
//! `AudioWorkletProcessor` pull mixed frames from it
//! (`platform/web/audio_driver_web.cpp`). This module is that arrangement:
//!
//! ```text
//!   AudioServer::mix(n)            the engine's real mixer, at mix_rate
//!     -> WebAudioOutput            resamples mix_rate -> AudioContext rate
//!       -> AudioRingBuffer         absorbs the block/quantum size mismatch
//!         -> pull_block(quantum)   what the worklet's `process()` asks for
//! ```
//!
//! Deliberately **portable Rust with no browser bindings**, so the whole chain
//! is exercised by native `cargo test`. The browser half — creating the
//! `AudioContext`, registering the processor, resuming on a user gesture — is
//! [`crate::web_audio`], which is a thin `wasm_bindgen` shell over this type.
//!
//! Two properties this file exists to guarantee:
//!
//! * **A pull never blocks and never short-reads.** `process()` must hand the
//!   browser a full render quantum every time or the graph glitches, so an
//!   underrun is zero-padded rather than refused.
//! * **Latency stays bounded.** The ring is topped up toward a target depth
//!   rather than "as much as we can mix", so a fast host cannot run minutes of
//!   audio ahead of what the listener hears.

use gdaudio::decode::AudioBuffer;
use gdaudio::output::mix_and_write;
use gdaudio::server::{AudioServer, PlaybackId};
use gdaudio::web_output::{SharedWebAudioOutput, WebAudioOutput};

/// Frames an `AudioWorkletProcessor` renders per `process()` call.
///
/// 128 in every browser that ships AudioWorklet — the spec fixes the render
/// quantum. Used only as the default pull size; [`WebAudioEngine::pull_block`]
/// takes whatever the caller asks for so a future spec change (or a test) can
/// use another size.
pub const RENDER_QUANTUM_FRAMES: usize = 128;

/// Name the `AudioWorkletProcessor` is registered under.
///
/// `registerProcessor` and `new AudioWorkletNode` must agree on it, so it is
/// defined once here and handed to JS rather than spelled in both places.
/// Lives beside the portable engine so the glue contract is testable natively.
pub const PROCESSOR_NAME: &str = "patina-audio-out";

/// How far ahead of the worklet the ring is kept, in render quanta.
///
/// Four quanta is ~11 ms at 48 kHz: enough that a main-thread hiccup between
/// two `process()` calls does not starve the graph, short enough that a
/// gameplay sound is not heard noticeably late. Godot's web driver keeps a
/// comparably small buffer for the same reason.
pub const TARGET_QUEUE_QUANTA: usize = 4;

/// How many frames to mix before serving a pull of `requested` frames.
///
/// The ring already holds `queued` frames. Two things must hold at once:
///
/// * the pull must be satisfiable — mix at least the shortfall, or the worklet
///   gets padded silence while the mixer sits idle with audio to give;
/// * latency must stay bounded — top up only to `target`, never "as much as
///   possible", or a host that pulls faster than real time queues unbounded
///   audio and every later sound is heard late.
///
/// Returns 0 when the ring is already deep enough, which is the common case at
/// steady state.
pub fn frames_to_mix(queued: usize, requested: usize, target: usize) -> usize {
    // The deeper of "enough to serve this pull" and "enough to reach target".
    let want = requested.max(target);
    want.saturating_sub(queued)
}

/// A running web audio backend: the engine's mixer plus the ring the worklet
/// drains.
///
/// Owns an [`AudioServer`], so anything that plays on native plays here —
/// `play_buffer` takes the same decoded [`AudioBuffer`] the WAV/OGG decoders
/// produce, and bus volumes/effects apply on the way out.
pub struct WebAudioEngine {
    server: AudioServer,
    output: WebAudioOutput,
    /// Rate the browser's `AudioContext` runs at; what `pull_block` returns.
    context_rate: u32,
    /// Frames of headroom kept queued ahead of the worklet.
    target_queue_frames: usize,
    channels: u16,
    /// Frames handed to the worklet so far — the observable "audio is flowing"
    /// counter the demo page reports.
    frames_pulled: u64,
}

impl WebAudioEngine {
    /// Builds an engine whose output feeds a context running at
    /// `context_rate` Hz.
    ///
    /// The mixer runs at `mix_rate` (Godot's `audio/driver/mix_rate`, 44100 by
    /// default) regardless of what the browser chose, and [`WebAudioOutput`]
    /// resamples between them — a 44.1 kHz mix played straight into a 48 kHz
    /// context runs ~8.8% slow and flat.
    ///
    /// A zero `context_rate` (a browser that has not started its context yet)
    /// falls back to 48 kHz rather than producing a degenerate resample ratio.
    ///
    /// ⚠️ The channel count is **not** a parameter: it is read off the mixer.
    /// [`AudioServer::mix`] interleaves `output_channels()` channels and there
    /// is no setter for it, so accepting a count from the caller would let the
    /// ring be de-interleaved against a different width than it was written
    /// with — audible as a swapped/garbled stereo image rather than as a
    /// failure.
    pub fn new(context_rate: u32, mix_rate: u32) -> Self {
        let context_rate = if context_rate == 0 {
            48_000
        } else {
            context_rate
        };
        let server = AudioServer::with_sample_rate(mix_rate);
        // Ask the server for both rather than assuming: it clamps the rate it
        // was given, and it owns the channel count outright.
        let effective_mix_rate = server.output_sample_rate();
        let channels = server.output_channels().max(1);
        Self {
            output: WebAudioOutput::new(context_rate, effective_mix_rate, channels),
            server,
            context_rate,
            target_queue_frames: TARGET_QUEUE_QUANTA * RENDER_QUANTUM_FRAMES,
            channels,
            frames_pulled: 0,
        }
    }

    /// The `AudioContext` rate this engine produces samples at.
    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    /// The rate the mixer runs at, which is usually *not* the context rate.
    pub fn mix_rate(&self) -> u32 {
        self.server.output_sample_rate()
    }

    /// Output channel count (interleaved in every block).
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The engine's mixer, for bus volume/effect changes.
    pub fn server_mut(&mut self) -> &mut AudioServer {
        &mut self.server
    }

    /// The engine's mixer.
    pub fn server(&self) -> &AudioServer {
        &self.server
    }

    /// Whether a user gesture has resumed the `AudioContext`.
    ///
    /// Browsers start every context suspended and refuse to play until a
    /// gesture resumes it, so this is the difference between "the graph is
    /// running" and "the graph is running and audible".
    pub fn is_unlocked(&self) -> bool {
        self.output.is_unlocked()
    }

    /// Records that the context was resumed by a user gesture.
    pub fn mark_unlocked(&mut self) {
        self.output.mark_unlocked();
    }

    /// Frames handed to the worklet since start.
    pub fn frames_pulled(&self) -> u64 {
        self.frames_pulled
    }

    /// Samples (not frames) currently queued for the worklet.
    pub fn queued_samples(&self) -> usize {
        self.output.queued()
    }

    /// Frames currently queued for the worklet.
    pub fn queued_frames(&self) -> usize {
        self.output.queued() / self.channels.max(1) as usize
    }

    /// Starts playing a decoded buffer through the mixer, as on native.
    pub fn play_buffer(&mut self, buffer: AudioBuffer) -> PlaybackId {
        self.server.play(buffer)
    }

    /// Decodes WAV/OGG bytes and plays them.
    ///
    /// Errors are returned as strings because the only caller is JS, which
    /// cannot pattern-match a `DecodeError` anyway.
    pub fn play_encoded(&mut self, bytes: &[u8]) -> Result<PlaybackId, String> {
        let buffer = gdaudio::decode::decode_auto(bytes).map_err(|e| e.to_string())?;
        Ok(self.server.play(buffer))
    }

    /// Plays a sine tone — the demo's "is anything audible at all" signal.
    ///
    /// Synthesised rather than loaded so the browser harness needs no asset
    /// fetch, and generated at the *mixer's* rate so it exercises the resample
    /// step on the way to the context instead of bypassing it.
    pub fn play_tone(&mut self, hz: f32, seconds: f32, amplitude: f32) -> PlaybackId {
        let rate = self.server.output_sample_rate();
        let frames = ((seconds.max(0.0) * rate as f32) as usize).max(1);
        let step = std::f32::consts::TAU * hz / rate as f32;
        let samples = (0..frames)
            .map(|i| (step * i as f32).sin() * amplitude)
            .collect();
        self.play_buffer(AudioBuffer {
            samples,
            sample_rate: rate,
            channels: 1,
        })
    }

    /// Mixes whatever the ring is short of and returns exactly `frames` frames
    /// of interleaved audio for the worklet.
    ///
    /// Always returns `frames * channels` samples: `process()` has to fill its
    /// whole output buffer, and silence is the correct filler for an underrun.
    pub fn pull_block(&mut self, frames: usize) -> Vec<f32> {
        let channels = self.channels.max(1) as usize;
        let needed = frames_to_mix(self.queued_frames(), frames, self.target_queue_frames);
        if needed > 0 {
            // The mixed block is written through `WebAudioOutput`, so it is
            // resampled to the context rate and queued in the ring — the same
            // call native uses to feed cpal.
            mix_and_write(&mut self.server, &mut self.output, needed);
        }
        let mut block = vec![0.0f32; frames * channels];
        self.output.drain(&mut block);
        self.frames_pulled += frames as u64;
        block
    }

    /// Pulls one browser render quantum.
    pub fn pull_quantum(&mut self) -> Vec<f32> {
        self.pull_block(RENDER_QUANTUM_FRAMES)
    }

    /// The `AudioWorkletProcessor` module source the page registers.
    ///
    /// Emitted as a string because a worklet module must be fetched by URL and
    /// cannot be an inline function — the page turns this into a Blob URL. It
    /// carries this engine's channel count so `outputChannelCount` matches the
    /// interleaving [`Self::pull_block`] produces.
    pub fn worklet_glue(&self) -> String {
        gdaudio::audio_worklet_glue(PROCESSOR_NAME, self.channels)
    }
}

/// The scene path: the ring the `MainLoop`'s `AudioServerBridge` fills and the
/// `AudioWorkletProcessor` drains (pat-mgixk.4).
///
/// Which mixer feeds it is the entire difference from [`WebAudioEngine`], and
/// that difference was the bug. `WebAudioEngine` owns a *second*
/// `AudioServer` that only [`WebAudioEngine::play_tone`] and
/// [`WebAudioEngine::play_encoded`] ever reach, while the loaded scene's audio
/// is mixed by the `MainLoop`'s `AudioServerBridge`. With nothing attached to
/// that bridge it mixed every `AudioStreamPlayer` into `last_mix` and dropped
/// it, so a web game was silent however correct its scene was. Godot's web
/// export has exactly one `AudioServer` and the worklet pulls from it
/// (`platform/web/audio_driver_web.cpp`); this is the consumer end of that
/// arrangement.
///
/// ⚠️ A pull here **never mixes**. That is the point, not an omission: the
/// frames come from `MainLoop::step`, so if the frame loop stops, audio stops,
/// exactly as on native. Topping the ring up inside `pull_block` would
/// re-introduce a mixer the scene does not drive — which is what made the old
/// demo path look like it worked.
pub struct SceneAudioRing {
    ring: SharedWebAudioOutput,
    /// Frames handed to the worklet so far — the observable "scene audio is
    /// flowing" counter, distinct from the demo path's.
    frames_pulled: u64,
}

impl SceneAudioRing {
    /// Wraps the consumer end of a ring already attached to a `MainLoop`.
    ///
    /// Built by [`crate::engine::WebEngine::attach_audio_output`], which is
    /// what attaches the producer end — constructing one by hand gives a ring
    /// nothing writes to.
    pub fn new(ring: SharedWebAudioOutput) -> Self {
        Self {
            ring,
            frames_pulled: 0,
        }
    }

    /// The `AudioContext` rate blocks come out at.
    pub fn context_rate(&self) -> u32 {
        self.ring.context_rate()
    }

    /// Interleaved channel count of every block [`Self::pull_block`] returns.
    pub fn channels(&self) -> u16 {
        self.ring.channels()
    }

    /// Frames handed to the worklet since start.
    pub fn frames_pulled(&self) -> u64 {
        self.frames_pulled
    }

    /// Frames the frame loop has mixed that the worklet has not taken yet.
    pub fn queued_frames(&self) -> usize {
        self.ring.queued_frames()
    }

    /// Whether a user gesture has resumed the `AudioContext`.
    pub fn is_unlocked(&self) -> bool {
        self.ring.is_unlocked()
    }

    /// Records that the gesture-unlock listener resumed the context.
    pub fn mark_unlocked(&mut self) {
        self.ring.mark_unlocked();
    }

    /// The shared ring, for inspecting it or attaching another producer.
    pub fn ring(&self) -> &SharedWebAudioOutput {
        &self.ring
    }

    /// Returns exactly `frames` frames of interleaved audio for the worklet.
    ///
    /// Always `frames * channels` samples: `process()` has to fill its whole
    /// output buffer every call, and silence is the correct filler for an
    /// underrun.
    pub fn pull_block(&mut self, frames: usize) -> Vec<f32> {
        let channels = self.ring.channels().max(1) as usize;
        let mut block = vec![0.0f32; frames * channels];
        self.ring.drain(&mut block);
        self.frames_pulled += frames as u64;
        block
    }

    /// Pulls one browser render quantum.
    pub fn pull_quantum(&mut self) -> Vec<f32> {
        self.pull_block(RENDER_QUANTUM_FRAMES)
    }

    /// The `AudioWorkletProcessor` module source the page registers.
    ///
    /// Carries this ring's channel count so `outputChannelCount` matches the
    /// interleaving [`Self::pull_block`] produces.
    pub fn worklet_glue(&self) -> String {
        gdaudio::audio_worklet_glue(PROCESSOR_NAME, self.ring.channels())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Peak absolute sample — "did anything audible come out".
    fn peak(block: &[f32]) -> f32 {
        block.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn mixes_the_shortfall_and_tops_up_to_target() {
        // Empty ring, small pull: mix enough to reach the target depth, not
        // just the pull.
        assert_eq!(frames_to_mix(0, 128, 512), 512);
        // Partly filled: only the remainder.
        assert_eq!(frames_to_mix(400, 128, 512), 112);
        // Deep enough already: mix nothing.
        assert_eq!(frames_to_mix(512, 128, 512), 0);
        assert_eq!(frames_to_mix(9000, 128, 512), 0);
    }

    /// ⚠️ A pull larger than the target must still be fully served — otherwise
    /// the ring is topped to `target`, the worklet asks for more, and the tail
    /// of every block is silence.
    #[test]
    fn a_pull_larger_than_the_target_is_still_satisfied() {
        assert_eq!(frames_to_mix(0, 2048, 512), 2048);
        assert_eq!(frames_to_mix(500, 2048, 512), 1548);
    }

    #[test]
    fn a_block_is_always_a_full_quantum() {
        let mut engine = WebAudioEngine::new(48_000, 44_100);
        // Nothing playing: still a full, silent block. `process()` must fill
        // its output buffer every call.
        let block = engine.pull_quantum();
        assert_eq!(block.len(), RENDER_QUANTUM_FRAMES * 2);
        assert_eq!(peak(&block), 0.0, "silence, not garbage");
    }

    #[test]
    fn a_tone_reaches_the_worklet_through_the_mixer() {
        let mut engine = WebAudioEngine::new(48_000, 44_100);
        engine.play_tone(440.0, 1.0, 0.5);

        // Enough quanta to get past the tone's first zero crossings.
        let mut loudest = 0.0f32;
        for _ in 0..8 {
            loudest = loudest.max(peak(&engine.pull_quantum()));
        }
        assert!(
            loudest > 0.1,
            "the mixer's output never reached the worklet block (peak {loudest})"
        );
        assert_eq!(engine.frames_pulled(), 8 * RENDER_QUANTUM_FRAMES as u64);
    }

    /// The browser picks the rate; the mixer keeps its own. Reporting the
    /// context rate is what makes the resample step correct.
    #[test]
    fn output_runs_at_the_context_rate_not_the_mix_rate() {
        let engine = WebAudioEngine::new(48_000, 44_100);
        assert_eq!(engine.context_rate(), 48_000);
        assert_eq!(engine.mix_rate(), 44_100);
    }

    #[test]
    fn a_missing_context_rate_falls_back_rather_than_degenerating() {
        let engine = WebAudioEngine::new(0, 44_100);
        assert_eq!(engine.context_rate(), 48_000);
    }

    /// ⚠️ Latency must not grow: pulling for a long time may not leave the ring
    /// deeper and deeper. This is the regression that makes web audio drift
    /// seconds behind the picture.
    #[test]
    fn queue_depth_stays_bounded_over_many_pulls() {
        let mut engine = WebAudioEngine::new(48_000, 44_100);
        engine.play_tone(220.0, 30.0, 0.3);
        for _ in 0..2000 {
            engine.pull_quantum();
        }
        let queued = engine.queued_frames();
        assert!(
            queued <= TARGET_QUEUE_QUANTA * RENDER_QUANTUM_FRAMES,
            "queue grew to {queued} frames — latency is unbounded"
        );
    }

    #[test]
    fn audio_starts_locked_until_a_gesture() {
        let mut engine = WebAudioEngine::new(48_000, 44_100);
        assert!(!engine.is_unlocked(), "browsers start a context suspended");
        engine.mark_unlocked();
        assert!(engine.is_unlocked());
    }

    #[test]
    fn encoded_bytes_that_are_not_audio_report_an_error() {
        let mut engine = WebAudioEngine::new(48_000, 44_100);
        assert!(engine.play_encoded(b"not audio at all").is_err());
    }
}
