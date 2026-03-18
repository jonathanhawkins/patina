//! Window creation and management.
//!
//! Provides `WindowConfig` with builder-pattern construction, mirroring
//! Godot's project-settings window configuration. Also defines the
//! `WindowManager` trait for platform-agnostic window backends and a
//! `HeadlessWindow` implementation for testing.

use crate::input::{InputEvent, Key, MouseButton};
use gdcore::math::Vector2;

// ---------------------------------------------------------------------------
// WindowId
// ---------------------------------------------------------------------------

/// Opaque identifier for an OS window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

// ---------------------------------------------------------------------------
// WindowEvent
// ---------------------------------------------------------------------------

/// Events produced by a window backend.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// The window was resized to the given dimensions.
    Resized { width: u32, height: u32 },
    /// The user requested the window to close.
    CloseRequested,
    /// The window gained input focus.
    FocusGained,
    /// The window lost input focus.
    FocusLost,
    /// A keyboard input event.
    KeyInput {
        key: Key,
        pressed: bool,
        shift: bool,
        ctrl: bool,
        alt: bool,
    },
    /// A mouse button input event.
    MouseInput {
        button: MouseButton,
        pressed: bool,
        position: Vector2,
    },
    /// Mouse motion within the window.
    MouseMotion {
        position: Vector2,
        relative: Vector2,
    },
}

impl WindowEvent {
    /// Converts this window event into an `InputEvent`, if applicable.
    pub fn to_input_event(&self) -> Option<InputEvent> {
        match self {
            WindowEvent::KeyInput {
                key,
                pressed,
                shift,
                ctrl,
                alt,
            } => Some(InputEvent::Key {
                key: *key,
                pressed: *pressed,
                shift: *shift,
                ctrl: *ctrl,
                alt: *alt,
            }),
            WindowEvent::MouseInput {
                button,
                pressed,
                position,
            } => Some(InputEvent::MouseButton {
                button: *button,
                pressed: *pressed,
                position: *position,
            }),
            WindowEvent::MouseMotion { position, relative } => Some(InputEvent::MouseMotion {
                position: *position,
                relative: *relative,
            }),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// WindowManager trait
// ---------------------------------------------------------------------------

/// Platform-agnostic window management interface.
pub trait WindowManager {
    /// Creates a new window with the given config, returning its id.
    fn create_window(&mut self, config: &WindowConfig) -> WindowId;

    /// Sets the title of an existing window.
    fn set_title(&mut self, id: WindowId, title: &str);

    /// Sets the size of an existing window.
    fn set_size(&mut self, id: WindowId, width: u32, height: u32);

    /// Sets fullscreen mode for a window.
    fn set_fullscreen(&mut self, id: WindowId, fullscreen: bool);

    /// Closes a window.
    fn close(&mut self, id: WindowId);

    /// Polls for pending events, returning them all.
    fn poll_events(&mut self) -> Vec<WindowEvent>;

    /// Returns `true` if the window with the given id is still open.
    fn is_open(&self, id: WindowId) -> bool;
}

// ---------------------------------------------------------------------------
// HeadlessWindow
// ---------------------------------------------------------------------------

/// A headless window backend for testing.
///
/// Does not interact with the OS. Events are queued manually via
/// `push_event` and drained via `poll_events`.
#[derive(Debug)]
pub struct HeadlessWindow {
    next_id: u64,
    open_windows: std::collections::HashSet<WindowId>,
    titles: std::collections::HashMap<WindowId, String>,
    sizes: std::collections::HashMap<WindowId, (u32, u32)>,
    fullscreen: std::collections::HashMap<WindowId, bool>,
    event_queue: Vec<WindowEvent>,
}

impl HeadlessWindow {
    /// Creates a new headless window backend.
    pub fn new() -> Self {
        Self {
            next_id: 1,
            open_windows: std::collections::HashSet::new(),
            titles: std::collections::HashMap::new(),
            sizes: std::collections::HashMap::new(),
            fullscreen: std::collections::HashMap::new(),
            event_queue: Vec::new(),
        }
    }

    /// Manually enqueue an event (for testing).
    pub fn push_event(&mut self, event: WindowEvent) {
        self.event_queue.push(event);
    }

    /// Returns the title of a window.
    pub fn get_title(&self, id: WindowId) -> Option<&str> {
        self.titles.get(&id).map(|s| s.as_str())
    }

    /// Returns the size of a window.
    pub fn get_size(&self, id: WindowId) -> Option<(u32, u32)> {
        self.sizes.get(&id).copied()
    }

    /// Returns whether a window is fullscreen.
    pub fn get_fullscreen(&self, id: WindowId) -> Option<bool> {
        self.fullscreen.get(&id).copied()
    }
}

impl Default for HeadlessWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowManager for HeadlessWindow {
    fn create_window(&mut self, config: &WindowConfig) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        self.open_windows.insert(id);
        self.titles.insert(id, config.title.clone());
        self.sizes.insert(id, (config.width, config.height));
        self.fullscreen.insert(id, config.fullscreen);
        id
    }

    fn set_title(&mut self, id: WindowId, title: &str) {
        if self.open_windows.contains(&id) {
            self.titles.insert(id, title.to_string());
        }
    }

    fn set_size(&mut self, id: WindowId, width: u32, height: u32) {
        if self.open_windows.contains(&id) {
            self.sizes.insert(id, (width, height));
        }
    }

    fn set_fullscreen(&mut self, id: WindowId, fullscreen: bool) {
        if self.open_windows.contains(&id) {
            self.fullscreen.insert(id, fullscreen);
        }
    }

    fn close(&mut self, id: WindowId) {
        self.open_windows.remove(&id);
        self.titles.remove(&id);
        self.sizes.remove(&id);
        self.fullscreen.remove(&id);
    }

    fn poll_events(&mut self) -> Vec<WindowEvent> {
        std::mem::take(&mut self.event_queue)
    }

    fn is_open(&self, id: WindowId) -> bool {
        self.open_windows.contains(&id)
    }
}

/// Configuration for the application window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    /// Window width in pixels.
    pub width: u32,
    /// Window height in pixels.
    pub height: u32,
    /// Window title.
    pub title: String,
    /// Whether the window is fullscreen.
    pub fullscreen: bool,
    /// Whether vsync is enabled.
    pub vsync: bool,
    /// Whether the window can be resized.
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            title: "Patina Engine".to_string(),
            fullscreen: false,
            vsync: true,
            resizable: true,
        }
    }
}

impl WindowConfig {
    /// Creates a new `WindowConfig` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the window width.
    pub fn with_width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }

    /// Sets the window height.
    pub fn with_height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    /// Sets both width and height.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets fullscreen mode.
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Sets vsync.
    pub fn with_vsync(mut self, vsync: bool) -> Self {
        self.vsync = vsync;
        self
    }

    /// Sets whether the window is resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_config_default_values() {
        let cfg = WindowConfig::default();
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);
        assert_eq!(cfg.title, "Patina Engine");
        assert!(!cfg.fullscreen);
        assert!(cfg.vsync);
        assert!(cfg.resizable);
    }

    #[test]
    fn window_config_builder_pattern() {
        let cfg = WindowConfig::new()
            .with_size(1920, 1080)
            .with_title("My Game")
            .with_fullscreen(true)
            .with_vsync(false)
            .with_resizable(false);

        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.title, "My Game");
        assert!(cfg.fullscreen);
        assert!(!cfg.vsync);
        assert!(!cfg.resizable);
    }

    #[test]
    fn headless_window_create_and_query() {
        let mut wm = HeadlessWindow::new();
        let id = wm.create_window(&WindowConfig::default());
        assert!(wm.is_open(id));
        assert_eq!(wm.get_title(id), Some("Patina Engine"));
        assert_eq!(wm.get_size(id), Some((1280, 720)));
        assert_eq!(wm.get_fullscreen(id), Some(false));
    }

    #[test]
    fn headless_window_set_title() {
        let mut wm = HeadlessWindow::new();
        let id = wm.create_window(&WindowConfig::default());
        wm.set_title(id, "New Title");
        assert_eq!(wm.get_title(id), Some("New Title"));
    }

    #[test]
    fn headless_window_set_size() {
        let mut wm = HeadlessWindow::new();
        let id = wm.create_window(&WindowConfig::default());
        wm.set_size(id, 800, 600);
        assert_eq!(wm.get_size(id), Some((800, 600)));
    }

    #[test]
    fn headless_window_set_fullscreen() {
        let mut wm = HeadlessWindow::new();
        let id = wm.create_window(&WindowConfig::default());
        wm.set_fullscreen(id, true);
        assert_eq!(wm.get_fullscreen(id), Some(true));
    }

    #[test]
    fn headless_window_close() {
        let mut wm = HeadlessWindow::new();
        let id = wm.create_window(&WindowConfig::default());
        assert!(wm.is_open(id));
        wm.close(id);
        assert!(!wm.is_open(id));
    }

    #[test]
    fn headless_window_multiple_windows() {
        let mut wm = HeadlessWindow::new();
        let id1 = wm.create_window(&WindowConfig::new().with_title("Win 1"));
        let id2 = wm.create_window(&WindowConfig::new().with_title("Win 2"));
        assert_ne!(id1, id2);
        assert!(wm.is_open(id1));
        assert!(wm.is_open(id2));
        wm.close(id1);
        assert!(!wm.is_open(id1));
        assert!(wm.is_open(id2));
    }

    #[test]
    fn headless_window_poll_events_drains_queue() {
        let mut wm = HeadlessWindow::new();
        wm.push_event(WindowEvent::FocusGained);
        wm.push_event(WindowEvent::CloseRequested);
        let events = wm.poll_events();
        assert_eq!(events.len(), 2);
        assert_eq!(wm.poll_events().len(), 0);
    }

    #[test]
    fn window_event_to_input_event_key() {
        let evt = WindowEvent::KeyInput {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let input = evt.to_input_event().unwrap();
        assert!(matches!(input, InputEvent::Key { key: Key::Space, pressed: true, .. }));
    }

    #[test]
    fn window_event_to_input_event_mouse() {
        let evt = WindowEvent::MouseInput {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(10.0, 20.0),
        };
        let input = evt.to_input_event().unwrap();
        assert!(matches!(input, InputEvent::MouseButton { button: MouseButton::Left, .. }));
    }

    #[test]
    fn window_event_to_input_event_motion() {
        let evt = WindowEvent::MouseMotion {
            position: Vector2::new(5.0, 5.0),
            relative: Vector2::new(1.0, 1.0),
        };
        assert!(evt.to_input_event().is_some());
    }

    #[test]
    fn window_event_non_input_returns_none() {
        assert!(WindowEvent::CloseRequested.to_input_event().is_none());
        assert!(WindowEvent::FocusGained.to_input_event().is_none());
        assert!(WindowEvent::FocusLost.to_input_event().is_none());
        assert!((WindowEvent::Resized { width: 800, height: 600 }).to_input_event().is_none());
    }

    #[test]
    fn headless_window_set_on_closed_window_is_noop() {
        let mut wm = HeadlessWindow::new();
        let id = wm.create_window(&WindowConfig::default());
        wm.close(id);
        wm.set_title(id, "Should not crash");
        wm.set_size(id, 100, 100);
        wm.set_fullscreen(id, true);
        assert!(!wm.is_open(id));
    }
}
