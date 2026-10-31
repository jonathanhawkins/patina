//! The web runtime's engine handle: a `MainLoop` driven by animation-frame
//! timestamps (pat-s3rrv.40).
//!
//! `gdscene::frame_driver::FrameDriver` already implements everything a browser
//! frame loop needs — coalescing callbacks that arrive inside the frame budget,
//! carrying the leftover delta rather than dropping it, clamping a long stall so
//! a backgrounded tab does not simulate minutes at once, and throttling while
//! hidden. It had **zero production callers**: the only references outside its
//! own module were in its own test, so nothing in the shipped web runtime
//! advanced a frame at all.
//!
//! This handle is deliberately **wasm-independent** — it takes a timestamp in
//! milliseconds and nothing else. `requestAnimationFrame` hands its callback
//! exactly that, so the browser entry is a one-line adapter, and the whole frame
//! pipeline stays testable on the host with synthetic timestamps instead of only
//! in a browser.

use gdaudio::server::AudioServer;
use gdaudio::web_output::SharedWebAudioOutput;
use gdscene::frame_driver::FrameDriver;
use gdscene::main_loop::{FrameOutput, MainLoop};
use gdscene::scene_tree::SceneTree;

use crate::audio::SceneAudioRing;

/// A running scene plus the driver that advances it.
pub struct WebEngine {
    main_loop: MainLoop,
    driver: FrameDriver,
}

impl WebEngine {
    /// Wraps an already-populated scene tree.
    pub fn new(tree: SceneTree) -> Self {
        Self {
            main_loop: MainLoop::new(tree),
            driver: FrameDriver::new(),
        }
    }

    /// Advances the engine for an animation-frame callback at `now_ms`.
    ///
    /// Returns the frame's output, or `None` when this callback produced no
    /// step — the first callback (which only establishes the time origin), or
    /// one that arrived inside the frame budget. A `None` is not lost time: the
    /// delta is carried into the next step.
    ///
    /// `now_ms` is the browser's `DOMHighResTimeStamp`, so this is exactly the
    /// value `requestAnimationFrame` passes its callback.
    pub fn frame(&mut self, now_ms: f64) -> Option<FrameOutput> {
        self.driver.tick(now_ms, &mut self.main_loop)
    }

    /// Marks the tab hidden or visible.
    ///
    /// A hidden tab is throttled rather than stopped, which is what keeps a
    /// backgrounded game's timers advancing at a reduced rate instead of
    /// freezing and then lurching forward on return.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.driver.set_hidden(hidden);
    }

    /// Whether the tab is currently marked hidden.
    pub fn is_hidden(&self) -> bool {
        self.driver.is_hidden()
    }

    /// Caps the frame rate (Godot's `Engine.max_fps`).
    pub fn set_max_fps(&mut self, fps: u32) {
        self.driver.set_max_fps(fps);
    }

    /// Total simulated time in seconds, after clamping and throttling.
    pub fn elapsed(&self) -> f64 {
        self.driver.elapsed()
    }

    /// Drops banked time and forgets the time origin — for re-entering the loop
    /// after a long pause that should not be simulated.
    pub fn reset_timing(&mut self) {
        self.driver.reset();
    }

    /// Feeds a translated browser event into the engine (pat-s3rrv.41).
    ///
    /// Goes through `MainLoop::push_event`, which both queues the event for
    /// `_input` dispatch on the next step AND updates the pollable state behind
    /// `Input.is_action_pressed`. Feeding `InputState` directly would give a
    /// script the action but never the event.
    pub fn push_input(&mut self, event: gdplatform::input::InputEvent) {
        self.main_loop.push_event(event);
    }

    /// Feeds several events in order — e.g. the press/release pair a wheel
    /// notch produces.
    pub fn push_inputs(&mut self, events: impl IntoIterator<Item = gdplatform::input::InputEvent>) {
        for e in events {
            self.main_loop.push_event(e);
        }
    }

    /// The scene being driven.
    pub fn tree(&self) -> &SceneTree {
        self.main_loop.tree()
    }

    /// Mutable access to the scene, for wiring a scene up after construction.
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        self.main_loop.tree_mut()
    }

    /// The underlying loop, for callers needing physics counters or settings.
    pub fn main_loop(&self) -> &MainLoop {
        &self.main_loop
    }

    /// Mutable access to the loop, e.g. to set `physics_ticks_per_second`.
    pub fn main_loop_mut(&mut self) -> &mut MainLoop {
        &mut self.main_loop
    }

    /// Routes this scene's audio into a ring an `AudioWorkletProcessor` can
    /// drain, and hands back the consumer end (pat-mgixk.4).
    ///
    /// Godot's web export runs exactly ONE `AudioServer` and lets the worklet
    /// pull mixed frames from it (`platform/web/audio_driver_web.cpp`), so every
    /// `AudioStreamPlayer` in a running scene is audible once a gesture has
    /// unlocked the context. Patina's web runtime attached *nothing*:
    /// `AudioServerBridge::process` mixed the whole scene into `last_mix` and
    /// discarded it, while the page's `pullBlock` drained a separate demo mixer
    /// that no scene node ever reached. A web game was therefore silent. This
    /// call is the missing `attach_output`.
    ///
    /// `context_rate` is the rate the browser's `AudioContext` reported; `0`
    /// (a context that has not started yet) falls back to 48 kHz rather than
    /// making the resample ratio degenerate. `mix_rate` is the project's
    /// `audio/driver/mix_rate`; pass `0` to keep whatever rate the bridge's
    /// server already runs at.
    ///
    /// ⚠️ Call this **before the first frame**. Honouring a mix rate the
    /// bridge was not built with means replacing its `AudioServer`, which drops
    /// any voice already playing — unobservable at startup, an audible cut
    /// mid-game.
    pub fn attach_audio_output(&mut self, context_rate: u32, mix_rate: u32) -> SceneAudioRing {
        // A degenerate ratio would resample the whole mix to nothing.
        let context_rate = if context_rate == 0 {
            48_000
        } else {
            context_rate
        };
        if mix_rate != 0 && self.main_loop.audio_bridge().server().output_sample_rate() != mix_rate
        {
            *self.main_loop.audio_bridge_mut().server_mut() =
                AudioServer::with_sample_rate(mix_rate);
        }
        // Both read back off the server rather than assumed:
        // `with_sample_rate` clamps what it is given, and the channel count is
        // the mixer's alone. De-interleaving the ring at a different width than
        // it was written with is heard as a garbled stereo image rather than as
        // a failure, so it must come from the one place that decides it.
        let effective_mix_rate = self.main_loop.audio_bridge().server().output_sample_rate();
        let channels = self
            .main_loop
            .audio_bridge()
            .server()
            .output_channels()
            .max(1);
        let ring = SharedWebAudioOutput::new(context_rate, effective_mix_rate, channels);
        self.main_loop
            .audio_bridge_mut()
            .attach_output(Box::new(ring.clone()));
        SceneAudioRing::new(ring)
    }
}
