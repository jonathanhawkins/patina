//! Platform backend trait for runtime-owned frame driving.
//!
//! [`PlatformBackend`] abstracts the platform-specific event loop so that
//! [`MainLoop`](gdscene::MainLoop) can drive frames without knowing whether
//! events come from winit, a browser, a test harness, or a headless runner.
//!
//! Callback-driven backends (e.g. winit) should use
//! [`MainLoop::run_frame`](gdscene::MainLoop::run_frame) inside their
//! callback. Pull-based backends can use [`MainLoop::run`](gdscene::MainLoop::run).

use crate::window::{WindowConfig, WindowEvent};

/// Abstraction over the platform's window and event system.
///
/// Implementations feed window events into the engine and report platform
/// state (quit signal, window size). Rendering/presentation is intentionally
/// outside this trait because it depends on the rendering backend
/// (`gdrender2d`, GPU, etc.) which is not always available.
pub trait PlatformBackend {
    /// Drains all pending window events since the last call.
    fn poll_events(&mut self) -> Vec<WindowEvent>;

    /// Returns `true` when the platform signals that the app should exit
    /// (e.g. window closed, Ctrl+C).
    fn should_quit(&self) -> bool;

    /// Returns the current primary window dimensions `(width, height)`.
    fn window_size(&self) -> (u32, u32);

    /// Called once at the end of each frame. Backends can use this for
    /// vsync, frame pacing, or cleanup.
    fn end_frame(&mut self) {}
}

/// A headless platform backend for tests and batch simulations.
///
/// Events are queued manually via [`push_event`](HeadlessPlatform::push_event)
/// and drained by the engine each frame. No OS window is created.
///
/// # Example
///
/// ```
/// use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
///
/// let mut platform = HeadlessPlatform::new(640, 480);
/// assert!(!platform.should_quit());
/// assert_eq!(platform.window_size(), (640, 480));
/// assert!(platform.poll_events().is_empty());
/// ```
#[derive(Debug)]
pub struct HeadlessPlatform {
    events: Vec<WindowEvent>,
    quit: bool,
    size: (u32, u32),
    frames_run: u64,
    max_frames: Option<u64>,
}

impl HeadlessPlatform {
    /// Creates a new headless platform with the given window dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            events: Vec::new(),
            quit: false,
            size: (width, height),
            frames_run: 0,
            max_frames: None,
        }
    }

    /// Creates a headless platform from a [`WindowConfig`].
    pub fn from_config(config: &WindowConfig) -> Self {
        Self::new(config.width, config.height)
    }

    /// Sets the maximum number of frames before `should_quit` returns true.
    pub fn with_max_frames(mut self, max: u64) -> Self {
        self.max_frames = Some(max);
        self
    }

    /// Queues a window event to be returned by the next `poll_events` call.
    pub fn push_event(&mut self, event: WindowEvent) {
        self.events.push(event);
    }

    /// Queues multiple events at once.
    pub fn push_events(&mut self, events: impl IntoIterator<Item = WindowEvent>) {
        self.events.extend(events);
    }

    /// Signals that the platform should quit.
    pub fn request_quit(&mut self) {
        self.quit = true;
    }

    /// Sets the reported window size.
    pub fn set_size(&mut self, width: u32, height: u32) {
        self.size = (width, height);
    }

    /// Returns the number of frames completed so far.
    pub fn frames_run(&self) -> u64 {
        self.frames_run
    }
}

impl PlatformBackend for HeadlessPlatform {
    fn poll_events(&mut self) -> Vec<WindowEvent> {
        std::mem::take(&mut self.events)
    }

    fn should_quit(&self) -> bool {
        if self.quit {
            return true;
        }
        if let Some(max) = self.max_frames {
            return self.frames_run >= max;
        }
        false
    }

    fn window_size(&self) -> (u32, u32) {
        self.size
    }

    fn end_frame(&mut self) {
        self.frames_run += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Key;

    #[test]
    fn headless_default_state() {
        let p = HeadlessPlatform::new(640, 480);
        assert!(!p.should_quit());
        assert_eq!(p.window_size(), (640, 480));
        assert_eq!(p.frames_run(), 0);
    }

    #[test]
    fn headless_from_config() {
        let config = WindowConfig::default();
        let p = HeadlessPlatform::from_config(&config);
        assert_eq!(p.window_size(), (config.width, config.height));
    }

    #[test]
    fn headless_poll_events_drains() {
        let mut p = HeadlessPlatform::new(640, 480);
        p.push_event(WindowEvent::FocusGained);
        p.push_event(WindowEvent::CloseRequested);
        let events = p.poll_events();
        assert_eq!(events.len(), 2);
        assert!(p.poll_events().is_empty());
    }

    #[test]
    fn headless_push_events_batch() {
        let mut p = HeadlessPlatform::new(640, 480);
        p.push_events(vec![WindowEvent::FocusGained, WindowEvent::FocusLost]);
        assert_eq!(p.poll_events().len(), 2);
    }

    #[test]
    fn headless_request_quit() {
        let mut p = HeadlessPlatform::new(640, 480);
        assert!(!p.should_quit());
        p.request_quit();
        assert!(p.should_quit());
    }

    #[test]
    fn headless_max_frames_quit() {
        let mut p = HeadlessPlatform::new(640, 480).with_max_frames(3);
        assert!(!p.should_quit());
        p.end_frame();
        p.end_frame();
        assert!(!p.should_quit());
        p.end_frame();
        assert!(p.should_quit());
    }

    #[test]
    fn headless_set_size() {
        let mut p = HeadlessPlatform::new(640, 480);
        p.set_size(1920, 1080);
        assert_eq!(p.window_size(), (1920, 1080));
    }

    #[test]
    fn headless_input_events_convert() {
        let mut p = HeadlessPlatform::new(640, 480);
        p.push_event(WindowEvent::KeyInput {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        let events = p.poll_events();
        assert_eq!(events.len(), 1);
        assert!(events[0].to_input_event().is_some());
    }

    #[test]
    fn headless_frames_run_increments() {
        let mut p = HeadlessPlatform::new(640, 480);
        assert_eq!(p.frames_run(), 0);
        p.end_frame();
        assert_eq!(p.frames_run(), 1);
        p.end_frame();
        assert_eq!(p.frames_run(), 2);
    }
}
