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

use crate::audio::{SceneAudioRing, WebAudioEngine, PROCESSOR_NAME, RENDER_QUANTUM_FRAMES};
use crate::browser_runtime::WebEngineHandle;
use crate::mp3_bridge::BrowserMp3Decoder;

/// A live audio backend: the browser's `AudioContext` plus the engine mixer
/// feeding it.
///
/// Held by the JS host for the lifetime of the page — dropping it drops the
/// context and the mixer with it.
#[wasm_bindgen]
pub struct WebAudio {
    context: web_sys::AudioContext,
    engine: WebAudioEngine,
    /// The running scene's audio, once [`WebAudio::attach_scene_audio`] has
    /// routed the `MainLoop`'s `AudioServerBridge` into this same worklet
    /// (pat-mgixk.4). `None` until then, which is the state in which every
    /// `AudioStreamPlayer` in a web game was silent.
    scene: Option<SceneAudioRing>,
    /// PCM the browser decoded for MP3 streams the wasm build has no codec for
    /// (pat-mgixk.4). Shares its store with the clone installed as `gdaudio`'s
    /// host decoder, so anything registered here is visible to the scene loader.
    mp3: BrowserMp3Decoder,
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
    ///
    /// Marks the scene's ring too when one is attached: the gesture is a
    /// property of the ONE `AudioContext` both producers feed, so leaving the
    /// scene ring's flag false would make it report a suspended context
    /// forever.
    #[wasm_bindgen(js_name = markUnlocked)]
    pub fn mark_unlocked(&mut self) {
        self.engine.mark_unlocked();
        if let Some(scene) = self.scene.as_mut() {
            scene.mark_unlocked();
        }
    }

    /// Mixes and returns `frames` frames of interleaved audio for the worklet.
    ///
    /// Always returns `frames * channels` samples — `process()` must fill its
    /// whole output buffer, so an underrun comes back zero-padded.
    #[wasm_bindgen(js_name = pullBlock)]
    pub fn pull_block(&mut self, frames: usize) -> Vec<f32> {
        let mut block = self.engine.pull_block(frames);
        // One device output, two producers: the scene's `AudioServer` (driven
        // by `MainLoop::step`) and this module's demo server (`playTone` /
        // `playEncoded`). Summing them is what a single output does with two
        // buses; the alternative — a second `AudioContext` for the scene —
        // would build the page a whole second audio graph to say the same
        // thing. `zip` rather than indexing so a channel-count skew between
        // the two truncates instead of panicking inside a worklet pull.
        if let Some(scene) = self.scene.as_mut() {
            for (out, s) in block.iter_mut().zip(scene.pull_block(frames)) {
                *out += s;
            }
        }
        block
    }

    /// Routes a running engine's scene audio into this backend's worklet pulls
    /// (pat-mgixk.4).
    ///
    /// Until this is called the page hears only `playTone`/`playEncoded`: the
    /// `MainLoop`'s `AudioServerBridge` has no output attached, so it mixes
    /// every `AudioStreamPlayer` in the loaded scene and throws the frames
    /// away. Attaching through *this* object rather than through the engine
    /// handle is deliberate — the ring has to be built for the rate the
    /// browser actually chose for the `AudioContext`, and that is owned here.
    ///
    /// ⚠️ Attach before the engine's first frame: honouring a mix rate the
    /// bridge was not built with replaces its `AudioServer`, which would cut
    /// off anything already playing.
    #[wasm_bindgen(js_name = attachSceneAudio)]
    pub fn attach_scene_audio(&mut self, engine: &WebEngineHandle) {
        let ring = engine.attach_audio_output(self.engine.context_rate(), self.engine.mix_rate());
        self.scene = Some(ring);
    }

    /// Whether the scene's audio has been routed into this output.
    ///
    /// A page that reports `false` here is a page whose `AudioStreamPlayer`
    /// nodes cannot be heard, which is otherwise indistinguishable from a
    /// scene that simply has no sound in it.
    #[wasm_bindgen(getter, js_name = sceneAttached)]
    pub fn scene_attached(&self) -> bool {
        self.scene.is_some()
    }

    /// Frames of scene audio the frame loop has mixed but the worklet has not
    /// taken, or 0 when no scene is attached.
    #[wasm_bindgen(getter, js_name = sceneQueuedFrames)]
    pub fn scene_queued_frames(&self) -> usize {
        self.scene.as_ref().map_or(0, SceneAudioRing::queued_frames)
    }

    /// Lets the engine decode MP3 streams out of browser-decoded PCM
    /// (pat-mgixk.4).
    ///
    /// Call once, before loading a scene that names an `.mp3`. Until it is
    /// called `gdaudio` has no MP3 codec on wasm32 at all — `minimp3` builds C
    /// sources and is gated to native targets — so every `AudioStreamMP3`
    /// decodes to `UnsupportedFormat` and plays silence.
    ///
    /// Registering is a second step ([`Self::register_decoded_mp3`]): the
    /// browser's `decodeAudioData` is async and the engine loads a stream
    /// synchronously inside a frame, so the PCM has to be in hand before the
    /// scene asks for it.
    #[wasm_bindgen(js_name = enableBrowserMp3Decoding)]
    pub fn enable_browser_mp3_decoding(&self) {
        self.mp3.install();
    }

    /// Registers PCM the browser decoded for one MP3 asset.
    ///
    /// `planar` is channel-major — all of channel 0, then all of channel 1 —
    /// which is `decodeAudioData`'s own shape: the page concatenates the
    /// `getChannelData(c)` arrays and hands them over without interleaving.
    /// `mp3` must be the exact bytes the scene will load, because that is the
    /// key the engine looks the PCM up by (the decode hook is handed bytes, not
    /// a path).
    ///
    /// Returns how many streams are now registered. Rejects PCM that cannot be
    /// played — a zero rate or channel count, or a length that is not a whole
    /// number of frames — rather than storing something that would play silence
    /// while reporting success.
    #[wasm_bindgen(js_name = registerDecodedMp3)]
    pub fn register_decoded_mp3(
        &self,
        mp3: &[u8],
        planar: &[f32],
        sample_rate: u32,
        channels: u16,
    ) -> Result<usize, JsValue> {
        self.mp3
            .register_planar(mp3, planar, sample_rate, channels)
            .map_err(JsValue::from)
    }

    /// How many browser-decoded MP3 streams are held.
    ///
    /// A page reporting 0 while its scene names an `.mp3` is a page whose music
    /// cannot play, which is otherwise indistinguishable from a scene that has
    /// no music in it.
    #[wasm_bindgen(getter, js_name = decodedMp3Count)]
    pub fn decoded_mp3_count(&self) -> usize {
        self.mp3.registered()
    }

    /// Drops the PCM for one asset, returning whether it was held. The decoded
    /// form is an order of magnitude larger than the file, so a game that is
    /// done with a track should give it back.
    #[wasm_bindgen(js_name = forgetDecodedMp3)]
    pub fn forget_decoded_mp3(&self, mp3: &[u8]) -> bool {
        self.mp3.forget(mp3)
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
    Ok(WebAudio {
        context,
        engine,
        // The scene is routed in separately: the engine handle does not exist
        // yet at the point a page creates its audio context.
        scene: None,
        // Created empty and NOT installed: installing is the page's explicit
        // `enableBrowserMp3Decoding` call, so merely starting audio never
        // changes how MP3 decoding behaves.
        mp3: BrowserMp3Decoder::new(),
    })
}
