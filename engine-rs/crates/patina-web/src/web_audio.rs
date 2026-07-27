//! Browser bindings for the web audio backend (pat-u3l8c.112).
//!
//! The counterpart to [`crate::canvas`]: where that binds a `<canvas>` to the
//! renderer, this binds an `AudioContext` to the engine's mixer. All the audio
//! logic lives in [`crate::audio`] (portable, natively tested); this module only
//! does the three things that need a browser:
//!
//! 1. construct the `AudioContext` and read the sample rate it chose — the
//!    browser picks it, the engine does not get a vote, and the mixer output
//!    has to be resampled to match;
//! 2. hand JS the `AudioWorkletProcessor` source ([`gdaudio::audio_worklet_glue`])
//!    to register;
//! 3. serve `pullBlock()` from the mixer each time the worklet asks.
//!
//! The pull is driven from the worklet's `port.onmessage` on the main thread
//! rather than from inside `process()`. That is the same shape Godot's web
//! driver uses, and it is forced: the wasm module is instantiated on the main
//! thread, so the audio-render thread cannot call into it directly.
//!
//! ⚠️ Nothing is audible until a user gesture resumes the context. Browsers
//! start every `AudioContext` suspended, so the glue installs one-shot
//! `pointerdown`/`keydown` listeners and reports back through
//! [`WebAudio::mark_unlocked`].
//!
//! Build it with:
//! `cargo check -p patina-web --target wasm32-unknown-unknown`

use wasm_bindgen::prelude::*;

use crate::audio::{WebAudioEngine, PROCESSOR_NAME, RENDER_QUANTUM_FRAMES};

/// A live audio backend: the browser's `AudioContext` plus the engine mixer
/// feeding it.
///
/// Held by the JS host for the lifetime of the page — dropping it drops the
/// context and the mixer with it.
#[wasm_bindgen]
pub struct WebAudio {
    context: web_sys::AudioContext,
    engine: WebAudioEngine,
}

#[wasm_bindgen]
impl WebAudio {
    /// The `AudioContext` to connect the worklet node to.
    #[wasm_bindgen(getter)]
    pub fn context(&self) -> web_sys::AudioContext {
        self.context.clone()
    }

    /// The rate the browser chose for its context, in Hz — 48000 on most
    /// hardware, 44100 on some.
    #[wasm_bindgen(getter, js_name = contextRate)]
    pub fn context_rate(&self) -> u32 {
        self.engine.context_rate()
    }

    /// The rate the engine mixes at (Godot's `audio/driver/mix_rate`).
    ///
    /// Usually *different* from [`Self::context_rate`]; the gap between the two
    /// is what the resampler exists for.
    #[wasm_bindgen(getter, js_name = mixRate)]
    pub fn mix_rate(&self) -> u32 {
        self.engine.mix_rate()
    }

    /// Interleaved channel count of every block `pullBlock` returns.
    #[wasm_bindgen(getter)]
    pub fn channels(&self) -> u16 {
        self.engine.channels()
    }

    /// Frames per `process()` call — what JS should pass to `pullBlock`.
    #[wasm_bindgen(getter, js_name = renderQuantum)]
    pub fn render_quantum(&self) -> usize {
        RENDER_QUANTUM_FRAMES
    }

    /// The processor name to register and instantiate under.
    #[wasm_bindgen(getter, js_name = processorName)]
    pub fn processor_name(&self) -> String {
        PROCESSOR_NAME.to_string()
    }

    /// The `AudioWorkletProcessor` module source to register.
    ///
    /// Emitted as a string so the page can turn it into a Blob URL — a worklet
    /// module has to be fetched by URL and cannot be an inline function.
    #[wasm_bindgen(getter, js_name = workletGlue)]
    pub fn worklet_glue(&self) -> String {
        self.engine.worklet_glue()
    }

    /// Whether a user gesture has resumed the context yet. Until it has, the
    /// graph runs but nothing is heard.
    #[wasm_bindgen(getter, js_name = isUnlocked)]
    pub fn is_unlocked(&self) -> bool {
        self.engine.is_unlocked()
    }

    /// Frames handed to the worklet so far — a monotonically rising number here
    /// is the proof that audio is actually flowing.
    #[wasm_bindgen(getter, js_name = framesPulled)]
    pub fn frames_pulled(&self) -> f64 {
        // f64 rather than u64: JS numbers are doubles, and at 48 kHz the exact
        // integer range lasts ~5900 years.
        self.engine.frames_pulled() as f64
    }

    /// Frames currently queued ahead of the worklet.
    #[wasm_bindgen(getter, js_name = queuedFrames)]
    pub fn queued_frames(&self) -> usize {
        self.engine.queued_frames()
    }

    /// Records that the gesture-unlock listener resumed the context.
    #[wasm_bindgen(js_name = markUnlocked)]
    pub fn mark_unlocked(&mut self) {
        self.engine.mark_unlocked();
    }

    /// Mixes and returns `frames` frames of interleaved audio for the worklet.
    ///
    /// Always returns `frames * channels` samples — `process()` must fill its
    /// whole output buffer, so an underrun comes back zero-padded.
    #[wasm_bindgen(js_name = pullBlock)]
    pub fn pull_block(&mut self, frames: usize) -> Vec<f32> {
        self.engine.pull_block(frames)
    }

    /// Plays a sine tone through the mixer. The demo's audibility check.
    #[wasm_bindgen(js_name = playTone)]
    pub fn play_tone(&mut self, hz: f32, seconds: f32, amplitude: f32) {
        self.engine.play_tone(hz, seconds, amplitude);
    }

    /// Decodes WAV/OGG bytes and plays them through the mixer.
    #[wasm_bindgen(js_name = playEncoded)]
    pub fn play_encoded(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.engine
            .play_encoded(bytes)
            .map(|_| ())
            .map_err(JsValue::from)
    }
}

/// Starts the audio backend, creating the browser `AudioContext`.
///
/// `mix_rate` is the engine's mix rate (44100 is Godot's default); the context
/// rate is whatever the browser reports and is read back off the live context
/// rather than assumed. The channel count is the mixer's — see
/// [`WebAudioEngine::new`] — and is reported through [`WebAudio::channels`].
///
/// Rejects with a message naming the failing stage. The most common failure is
/// not an error at all — a context created before any user gesture starts
/// suspended, which is why the caller must also install the gesture unlock.
#[wasm_bindgen(js_name = startWebAudio)]
pub fn start_web_audio(mix_rate: u32) -> Result<WebAudio, JsValue> {
    let context = web_sys::AudioContext::new()
        .map_err(|e| JsValue::from(format!("could not create an AudioContext: {e:?}")))?;
    // `sampleRate` is a float in the Web Audio spec but is always an integral
    // rate in practice; round rather than truncate so a 47999.9999 never
    // becomes 47999.
    let context_rate = context.sample_rate().round().max(0.0) as u32;
    let engine = WebAudioEngine::new(context_rate, mix_rate);
    Ok(WebAudio { context, engine })
}
