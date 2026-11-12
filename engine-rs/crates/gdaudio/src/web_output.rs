//! Web (wasm) audio output — an [`AudioOutputStream`] feeding an
//! `AudioWorkletProcessor` (pat-0q2kx.59).
//!
//! Godot's web export runs the *same* mixer as native and lets an
//! `AudioWorkletProcessor` pull from it (`platform/web/audio_driver_web.cpp`,
//! `platform/web/js/libs/library_godot_audio.js`), resuming the `AudioContext`
//! on the first user gesture because browsers refuse to start audio without
//! one.
//!
//! Patina had no audio on wasm at all: `patina-web` excluded `gdaudio` because
//! of its unconditional `cpal` dependency, and the wasm branch of the runner's
//! output selection returned `None`. With `cpal` now gated to non-wasm targets,
//! the whole audio stack compiles for the web and this module supplies the
//! device layer.
//!
//! Everything here is **portable Rust with no browser bindings**, so the
//! producer/consumer contract is testable natively. The browser half is the JS
//! string [`audio_worklet_glue`] emits, which the wasm host evaluates.

use std::sync::{Arc, Mutex};

use crate::sample::{AudioOutputStream, AudioSampleBuffer};

/// A single-producer/single-consumer ring of interleaved `f32` samples.
///
/// The engine writes whole mixed blocks; the worklet reads whatever its render
/// quantum is (128 frames in every current browser). Those sizes do not divide
/// evenly, which is the entire reason a ring sits between them.
///
/// ⚠️ On overflow the ring drops the OLDEST samples rather than refusing the
/// write. A stalled consumer must not make the producer block the frame loop,
/// and stale audio is worse than a glitch — this is what Godot's web driver
/// does when the worklet falls behind.
#[derive(Debug)]
pub struct AudioRingBuffer {
    data: Vec<f32>,
    /// Next write position.
    head: usize,
    /// Next read position.
    tail: usize,
    /// Samples currently readable. Tracked separately because `head == tail` is
    /// ambiguous between empty and full.
    len: usize,
}

impl AudioRingBuffer {
    /// Creates a ring holding `capacity` samples (not frames).
    ///
    /// A zero capacity is promoted to 1 so the modular arithmetic below never
    /// divides by zero.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            data: vec![0.0; capacity],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    /// Samples available to read.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the ring holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Total sample capacity.
    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    /// Appends `samples`, dropping the oldest on overflow. Returns how many
    /// were written (always `samples.len()`; the drop happens at the read end).
    pub fn write(&mut self, samples: &[f32]) -> usize {
        let cap = self.data.len();
        for &s in samples {
            self.data[self.head] = s;
            self.head = (self.head + 1) % cap;
            if self.len == cap {
                // Full: the write just overwrote the oldest sample, so the read
                // cursor has to move with it.
                self.tail = (self.tail + 1) % cap;
            } else {
                self.len += 1;
            }
        }
        samples.len()
    }

    /// Fills `out` with the oldest samples, zero-padding any shortfall.
    ///
    /// Returns how many real samples were read. Zero-padding rather than
    /// short-reading matters: a worklet must always hand back a full render
    /// quantum, and silence is the correct filler for an underrun.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let cap = self.data.len();
        let n = out.len().min(self.len);
        for slot in out.iter_mut().take(n) {
            *slot = self.data[self.tail];
            self.tail = (self.tail + 1) % cap;
        }
        self.len -= n;
        for slot in out.iter_mut().skip(n) {
            *slot = 0.0;
        }
        n
    }
}

/// Resamples interleaved audio between two sample rates by linear
/// interpolation (pat-0q2kx.59).
///
/// The engine mixes at its own rate; the browser's `AudioContext` picks its
/// own (48 kHz on most hardware, 44.1 kHz on some). Feeding 44.1 kHz frames to
/// a 48 kHz context without adapting plays everything ~8.8% slow and flat.
///
/// ⚠️ Interpolates WITHIN each channel — a naive interpolation across the
/// interleaved buffer would blend left into right and collapse the stereo
/// image.
pub fn resample_interleaved(
    input: &[f32],
    channels: u16,
    from_rate: u32,
    to_rate: u32,
) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if input.is_empty() || from_rate == 0 || to_rate == 0 || from_rate == to_rate {
        return input.to_vec();
    }
    let in_frames = input.len() / channels;
    if in_frames == 0 {
        return Vec::new();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_frames = ((in_frames as f64) / ratio).round() as usize;
    let mut out = Vec::with_capacity(out_frames * channels);

    for f in 0..out_frames {
        let src = f as f64 * ratio;
        let i0 = src.floor() as usize;
        let frac = (src - i0 as f64) as f32;
        let i1 = (i0 + 1).min(in_frames - 1);
        for c in 0..channels {
            let a = input[i0.min(in_frames - 1) * channels + c];
            let b = input[i1 * channels + c];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// An [`AudioOutputStream`] that queues mixed frames for an
/// `AudioWorkletProcessor` to drain (pat-0q2kx.59).
///
/// Adapts from the engine's mix rate to the `AudioContext`'s rate on the way
/// in, so the worklet always reads samples already at its own rate.
#[derive(Debug)]
pub struct WebAudioOutput {
    /// The rate the browser's AudioContext runs at.
    context_rate: u32,
    /// The rate the engine mixes at.
    mix_rate: u32,
    channels: u16,
    ring: AudioRingBuffer,
    /// Whether the AudioContext has been resumed by a user gesture. Until it
    /// has, the browser will not play anything.
    unlocked: bool,
}

impl WebAudioOutput {
    /// Creates an output feeding a context running at `context_rate`.
    ///
    /// The ring is sized to roughly a quarter second, which absorbs a slow
    /// frame without letting latency grow unbounded.
    pub fn new(context_rate: u32, mix_rate: u32, channels: u16) -> Self {
        let ch = channels.max(1) as usize;
        let capacity = (context_rate as usize / 4).max(1024) * ch;
        Self {
            context_rate,
            mix_rate,
            channels: channels.max(1),
            ring: AudioRingBuffer::new(capacity),
            unlocked: false,
        }
    }

    /// The `AudioContext` sample rate this output feeds.
    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    /// Whether a user gesture has resumed the context yet.
    pub fn is_unlocked(&self) -> bool {
        self.unlocked
    }

    /// Marks the context resumed, which the JS glue reports after the first
    /// `pointerdown`/`keydown`.
    pub fn mark_unlocked(&mut self) {
        self.unlocked = true;
    }

    /// Drains up to `out.len()` samples for the worklet, zero-padding an
    /// underrun.
    pub fn drain(&mut self, out: &mut [f32]) -> usize {
        self.ring.read(out)
    }

    /// Samples currently queued.
    pub fn queued(&self) -> usize {
        self.ring.len()
    }
}

impl AudioOutputStream for WebAudioOutput {
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        let samples = &buffer.samples;
        if samples.is_empty() {
            return 0;
        }
        let adapted =
            resample_interleaved(samples, self.channels, self.mix_rate, self.context_rate);
        let ch = self.channels.max(1) as usize;
        self.ring.write(&adapted);
        adapted.len() / ch
    }

    fn sample_rate(&self) -> u32 {
        self.context_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    /// Fed at the mix rate; resamples to the context rate itself.
    fn input_sample_rate(&self) -> u32 {
        self.mix_rate
    }
}

/// A clonable handle to ONE [`WebAudioOutput`], so the mixer side and the
/// worklet side hold the *same* ring (pat-mgixk.4).
///
/// An [`AudioOutputStream`] is handed to the engine by value —
/// `AudioServerBridge::attach_output` takes a `Box<dyn AudioOutputStream>` and
/// owns it — so a bare [`WebAudioOutput`] given to a `MainLoop` is unreachable
/// from the page's `pullBlock`. That is exactly how web audio came to be
/// silent: the scene's bridge had no output attached at all, and the only
/// `WebAudioOutput` in the process belonged to a second, demo-only
/// `AudioServer` that no `AudioStreamPlayer` ever reached. With this handle one
/// clone goes into the bridge as the producer and the other stays with the
/// runtime as the consumer the `AudioWorkletProcessor` drains.
///
/// `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>`, for the same reason
/// [`crate::output::PlatformAudioOutput`] uses it: the boxed trait object can
/// be stored anywhere in the engine, and a `!Send` handle inside it would make
/// every `MainLoop` that attaches one un-sendable on native. On the browser
/// main thread there is only one thread, so the lock is never contended.
#[derive(Debug, Clone)]
pub struct SharedWebAudioOutput {
    inner: Arc<Mutex<WebAudioOutput>>,
    /// Cached so a consumer's block geometry never depends on taking the lock.
    /// De-interleaving the ring at a different width than it was written with
    /// is heard as a garbled stereo image rather than as a failure, so the
    /// channel count must not be able to change under a failed lock.
    context_rate: u32,
    channels: u16,
}

impl SharedWebAudioOutput {
    /// A shared ring feeding a context at `context_rate`, written by a mixer
    /// running at `mix_rate`.
    pub fn new(context_rate: u32, mix_rate: u32, channels: u16) -> Self {
        Self::from_output(WebAudioOutput::new(context_rate, mix_rate, channels))
    }

    /// Shares an output that has already been built.
    pub fn from_output(output: WebAudioOutput) -> Self {
        let context_rate = output.context_rate();
        // Spelled as a trait call rather than `output.channels()` so it cannot
        // silently start resolving to a future inherent method of another
        // meaning.
        let channels = AudioOutputStream::channels(&output).max(1);
        Self {
            inner: Arc::new(Mutex::new(output)),
            context_rate,
            channels,
        }
    }

    /// The `AudioContext` rate the ring holds samples at.
    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    /// Interleaved channel count of everything in the ring.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Samples (not frames) queued for the worklet.
    pub fn queued(&self) -> usize {
        self.with(|o| o.queued())
    }

    /// Frames queued for the worklet.
    pub fn queued_frames(&self) -> usize {
        self.queued() / self.channels.max(1) as usize
    }

    /// Drains up to `out.len()` samples, zero-padding an underrun.
    pub fn drain(&self, out: &mut [f32]) -> usize {
        self.with(|o| o.drain(out))
    }

    /// Whether a user gesture has resumed the context yet.
    pub fn is_unlocked(&self) -> bool {
        self.with(|o| o.is_unlocked())
    }

    /// Records that a gesture resumed the context.
    ///
    /// Takes `&self`: the unlock is reported by the page through whichever
    /// clone it happens to hold, and both refer to the same flag.
    pub fn mark_unlocked(&self) {
        self.with(|o| o.mark_unlocked());
    }

    /// Runs `f` against the shared output.
    ///
    /// ⚠️ Poison-tolerant on purpose. A panic on one side of this ring must not
    /// silence the page for the rest of its life, and what the lock guards is a
    /// plain sample buffer with no invariant an unwind could leave broken —
    /// the worst case is one glitched block.
    fn with<T>(&self, f: impl FnOnce(&mut WebAudioOutput) -> T) -> T {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // `&mut *guard` rather than `&mut guard`: an explicit reborrow through
        // `DerefMut` instead of relying on a coercion at a generic call site.
        f(&mut guard)
    }
}

impl AudioOutputStream for SharedWebAudioOutput {
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        self.with(|o| o.write_samples(buffer))
    }

    fn sample_rate(&self) -> u32 {
        self.context_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn input_sample_rate(&self) -> u32 {
        self.with(|o| o.mix_rate)
    }
}

/// Blocks the processor keeps requested-but-not-yet-delivered.
///
/// ⚠️ Must be > 1. The host that answers a `need-block` request lives on the
/// main thread, so a processor that only asks for a replacement *after*
/// consuming one is always exactly one main-thread turn behind — every time the
/// page does a long task the graph gets silence. Keeping a few in flight buys
/// that many render quanta (~8 ms at 3 x 128 frames / 48 kHz) of slack.
pub const WORKLET_QUEUE_DEPTH: usize = 3;

/// The JS the wasm host evaluates to stand up the worklet (pat-0q2kx.59).
///
/// Registers an `AudioWorkletProcessor` that pulls from the module's ring and
/// installs one-shot `pointerdown`/`keydown` listeners that resume the
/// `AudioContext` — browsers refuse to start audio without a user gesture, so
/// without those listeners the graph runs and stays silent.
///
/// Emitted as a string rather than a static asset so the contract is
/// assertable from a native test.
pub fn audio_worklet_glue(processor_name: &str, channels: u16) -> String {
    let queue_depth = WORKLET_QUEUE_DEPTH;
    format!(
        r#"// Patina web audio glue (pat-0q2kx.59, pat-u3l8c.112)
const PATINA_PROCESSOR = "{processor_name}";
const PATINA_CHANNELS = {channels};

export function patinaRegisterWorklet(context, pullFrames) {{
  const src = `
    // A worklet module gets its own global scope, so every value the host side
    // knows has to be baked into this source rather than referenced.
    const PATINA_QUEUE_DEPTH = {queue_depth};

    class PatinaProcessor extends AudioWorkletProcessor {{
      constructor() {{
        super();
        this._queue = [];
        // Requests posted but not yet answered. Without this the top-up loop
        // below re-asks for the same blocks every render quantum.
        this._pending = 0;
        this.port.onmessage = (e) => {{
          this._queue.push(e.data);
          if (this._pending > 0) this._pending--;
        }};
      }}
      process(inputs, outputs) {{
        const out = outputs[0];
        const block = this._queue.shift();
        const want = out.length;
        for (let ch = 0; ch < want; ch++) {{
          const dst = out[ch];
          for (let i = 0; i < dst.length; i++) {{
            const idx = i * want + ch;
            // A short or missing block reads back undefined, which lands in the
            // Float32Array as NaN and is an audible click; silence is the
            // correct filler for an underrun.
            dst[i] = block && idx < block.length ? block[idx] : 0;
          }}
        }}
        // Keep PATINA_QUEUE_DEPTH blocks queued or in flight. Asking for a
        // single replacement here would leave the graph one main-thread turn
        // from silence on every render quantum.
        while (this._queue.length + this._pending < PATINA_QUEUE_DEPTH) {{
          this._pending++;
          this.port.postMessage("need-block");
        }}
        return true;
      }}
    }}
    registerProcessor("${{PATINA_PROCESSOR}}", PatinaProcessor);
  `;
  const url = URL.createObjectURL(new Blob([src], {{ type: "application/javascript" }}));
  return context.audioWorklet.addModule(url).then(() => {{
    const node = new AudioWorkletNode(context, PATINA_PROCESSOR, {{
      outputChannelCount: [PATINA_CHANNELS],
    }});
    node.port.onmessage = () => {{ node.port.postMessage(pullFrames()); }};
    node.connect(context.destination);
    return node;
  }});
}}

// Browsers start an AudioContext suspended until a user gesture. Resume on the
// first pointerdown or keydown, then remove the listeners.
export function patinaInstallGestureUnlock(context, onUnlocked) {{
  const resume = () => {{
    context.resume().then(() => {{ if (onUnlocked) onUnlocked(); }});
    window.removeEventListener("pointerdown", resume);
    window.removeEventListener("keydown", resume);
  }};
  window.addEventListener("pointerdown", resume, {{ once: false }});
  window.addEventListener("keydown", resume, {{ once: false }});
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_reads_back_what_was_written() {
        let mut ring = AudioRingBuffer::new(64);
        ring.write(&[1.0, 2.0, 3.0]);
        let mut out = [0.0; 3];
        assert_eq!(ring.read(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        assert!(ring.is_empty());
    }

    /// An underrun zero-pads rather than short-reading — a worklet must always
    /// return a full render quantum.
    #[test]
    fn ring_zero_pads_an_underrun() {
        let mut ring = AudioRingBuffer::new(16);
        ring.write(&[1.0, 2.0]);
        let mut out = [9.0; 4];
        assert_eq!(ring.read(&mut out), 2, "only two real samples");
        assert_eq!(out, [1.0, 2.0, 0.0, 0.0]);
    }

    /// ⚠️ Overflow drops the OLDEST samples: a stalled consumer must never
    /// block the frame loop, and stale audio is worse than a glitch.
    #[test]
    fn ring_overflow_drops_the_oldest() {
        let mut ring = AudioRingBuffer::new(4);
        ring.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(ring.len(), 4);
        let mut out = [0.0; 4];
        ring.read(&mut out);
        assert_eq!(out, [3.0, 4.0, 5.0, 6.0], "the two oldest were dropped");
    }

    #[test]
    fn resampling_is_a_no_op_at_a_matching_rate() {
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_interleaved(&input, 2, 48000, 48000), input);
    }

    /// ⚠️ Channels must not bleed into each other: a constant-per-channel
    /// signal has to stay constant per channel after resampling.
    #[test]
    fn resampling_keeps_channels_separate() {
        // Left is always -1.0, right always +1.0.
        let input: Vec<f32> = (0..64).flat_map(|_| [-1.0f32, 1.0]).collect();
        let out = resample_interleaved(&input, 2, 44100, 48000);
        for pair in out.chunks_exact(2) {
            assert!((pair[0] + 1.0).abs() < 1e-5, "left leaked: {pair:?}");
            assert!((pair[1] - 1.0).abs() < 1e-5, "right leaked: {pair:?}");
        }
    }

    #[test]
    fn web_output_reports_the_context_rate() {
        let out = WebAudioOutput::new(48000, 44100, 2);
        assert_eq!(out.sample_rate(), 48000, "the browser's rate wins");
        assert_eq!(out.channels(), 2);
        assert!(!out.is_unlocked(), "audio starts locked until a gesture");
    }

    #[test]
    fn glue_registers_a_processor_and_a_gesture_unlock() {
        let glue = audio_worklet_glue("patina-out", 2);
        assert!(glue.contains("AudioWorkletProcessor"));
        assert!(glue.contains("registerProcessor"));
        assert!(glue.contains("pointerdown"));
        assert!(glue.contains("keydown"));
        assert!(glue.contains("context.resume()"));
    }

    /// ⚠️ The processor must keep more than one block requested. Replacing one
    /// block per `process()` leaves the graph permanently one main-thread turn
    /// from silence, which is heard as constant crackle rather than as an
    /// outright failure (pat-u3l8c.112).
    #[test]
    fn glue_keeps_several_blocks_in_flight() {
        assert!(
            WORKLET_QUEUE_DEPTH > 1,
            "a depth of 1 is the starvation bug this constant exists to prevent"
        );
        let glue = audio_worklet_glue("patina-out", 2);
        assert!(
            glue.contains(&format!("PATINA_QUEUE_DEPTH = {WORKLET_QUEUE_DEPTH}")),
            "the depth must be baked into the worklet source — a worklet module \
             cannot see the host module's scope"
        );
        assert!(
            glue.contains("this._queue.length + this._pending"),
            "in-flight requests must be counted, or the top-up loop re-asks every quantum"
        );
    }

    /// A short block must read as silence, not `undefined` — an `undefined`
    /// stored into a `Float32Array` becomes NaN, which is an audible click.
    #[test]
    fn glue_pads_a_short_block_with_silence() {
        let glue = audio_worklet_glue("patina-out", 2);
        assert!(glue.contains("idx < block.length"));
    }

    /// ⚠️ The whole point of the shared handle: what the producer writes, the
    /// consumer reads. `AudioServerBridge::attach_output` takes the stream by
    /// value, so before this existed the ring the engine wrote into could not
    /// be drained by anyone — which is how web audio came to be silent
    /// (pat-mgixk.4).
    #[test]
    fn a_shared_output_reads_back_what_the_other_clone_wrote() {
        let consumer = SharedWebAudioOutput::new(48_000, 48_000, 2);
        let mut producer = consumer.clone();

        let mut buffer = AudioSampleBuffer::new(48_000, 2);
        buffer.samples = vec![0.25, -0.25, 0.5, -0.5];
        assert_eq!(producer.write_samples(&buffer), 2, "two stereo frames");

        assert_eq!(consumer.queued(), 4, "the clone must see the same ring");
        assert_eq!(consumer.queued_frames(), 2);
        let mut out = [9.0f32; 4];
        assert_eq!(consumer.drain(&mut out), 4);
        assert_eq!(out, [0.25, -0.25, 0.5, -0.5]);
        assert!(consumer.queued() == 0, "the ring is one ring, not two");
    }

    /// The geometry a consumer computes blocks from must not be able to drift
    /// from what the producer interleaved at.
    #[test]
    fn a_shared_output_reports_the_context_rate_and_the_mixers_channels() {
        let shared = SharedWebAudioOutput::new(48_000, 44_100, 2);
        assert_eq!(shared.context_rate(), 48_000, "the browser's rate wins");
        assert_eq!(shared.channels(), 2);
        assert_eq!(
            AudioOutputStream::sample_rate(&shared),
            48_000,
            "the engine must be told the rate it has to resample TO"
        );
        assert_eq!(AudioOutputStream::channels(&shared), 2);
    }

    /// Resampling has to happen on the way INTO the shared ring, exactly as it
    /// does for a bare [`WebAudioOutput`] — a 44.1 kHz mix queued untouched for
    /// a 48 kHz context plays ~8.8% slow and flat.
    #[test]
    fn a_shared_output_resamples_into_the_ring() {
        let consumer = SharedWebAudioOutput::new(48_000, 44_100, 2);
        let mut producer = consumer.clone();

        let mut buffer = AudioSampleBuffer::new(44_100, 2);
        // 441 stereo frames at 44.1 kHz is 10 ms, which is 480 frames at 48 kHz.
        buffer.samples = vec![0.5; 441 * 2];
        let frames = producer.write_samples(&buffer);

        assert_eq!(frames, 480, "441 frames at 44.1 kHz is 480 at 48 kHz");
        assert_eq!(consumer.queued_frames(), 480);
    }

    /// A gesture unlock reported through one clone must be visible from the
    /// other — the page holds one and the engine the other.
    #[test]
    fn a_shared_output_shares_the_gesture_unlock() {
        let consumer = SharedWebAudioOutput::new(48_000, 48_000, 2);
        let producer = consumer.clone();
        assert!(
            !consumer.is_unlocked(),
            "audio starts locked until a gesture"
        );
        producer.mark_unlocked();
        assert!(consumer.is_unlocked());
    }

    /// ⚠️ The worklet body is carried inside a JS **template literal**, so a
    /// single backtick anywhere in it — including in a comment — closes the
    /// literal early and registers a truncated processor. The glue still looks
    /// plausible as a string and every `contains` assertion above still passes;
    /// the only symptom is that the browser throws on `addModule` and the page
    /// is silent. Assert the body actually reaches its `registerProcessor`
    /// call (pat-u3l8c.112).
    #[test]
    fn the_worklet_source_is_not_truncated_by_a_stray_backtick() {
        let glue = audio_worklet_glue("patina-out", 2);
        let after_open = glue
            .split_once("const src = `")
            .expect("the glue must build a worklet module source")
            .1;
        let body = after_open
            .split_once('`')
            .expect("the template literal must be closed")
            .0;
        assert!(
            body.contains("registerProcessor"),
            "a backtick inside the worklet source closed its template literal early — \
             the browser would register a truncated processor. Body was:\n{body}"
        );
        assert!(
            body.contains("class PatinaProcessor"),
            "the processor class must live inside the worklet module, not outside it"
        );
    }
}
