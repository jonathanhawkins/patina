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

use gdscene::frame_driver::FrameDriver;
use gdscene::main_loop::{FrameOutput, MainLoop};
use gdscene::scene_tree::SceneTree;

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
}
