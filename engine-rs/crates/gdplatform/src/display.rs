// TODO(pat-oa3): Normalize display and window state flow — DisplayServer should
// be the single authority for window state. Examples/tests should not create
// windows directly. Resize, vsync, and focus state should flow through
// DisplayServer events. See PLATFORM_ROADMAP.md.

//! Display server for multi-window management and input routing.
//!
//! The `DisplayServer` manages multiple windows through the `WindowManager`
//! trait and routes input-related window events into an `InputState`.

use std::collections::HashMap;

use crate::input::InputState;
use crate::window::{HeadlessWindow, WindowConfig, WindowEvent, WindowId, WindowManager};

/// Vsync mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VsyncMode {
    /// Vsync disabled — uncapped frame rate.
    Disabled,
    /// Standard vsync — wait for vertical blank.
    #[default]
    Enabled,
    /// Adaptive vsync — vsync when above refresh rate, tear when below.
    Adaptive,
}

/// Manages multiple windows and routes their events to an `InputState`.
#[derive(Debug)]
pub struct DisplayServer {
    windows: HashMap<WindowId, HeadlessWindow>,
    next_id: u64,
    vsync: VsyncMode,
}

impl DisplayServer {
    /// Creates a new display server.
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            next_id: 1,
            vsync: VsyncMode::default(),
        }
    }

    /// Creates a window backed by a `HeadlessWindow`, returning its id.
    pub fn create_window(&mut self, config: &WindowConfig) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        let mut backend = HeadlessWindow::new();
        // We use our own id, so just create and store
        let _inner_id = backend.create_window(config);
        self.windows.insert(id, backend);
        id
    }

    /// Returns a mutable reference to the underlying `HeadlessWindow` backend.
    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut HeadlessWindow> {
        self.windows.get_mut(&id)
    }

    /// Returns the number of open windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Sets the vsync mode.
    pub fn set_vsync(&mut self, mode: VsyncMode) {
        self.vsync = mode;
    }

    /// Returns the current vsync mode.
    pub fn vsync(&self) -> VsyncMode {
        self.vsync
    }

    /// Returns the screen size (stubbed — returns a default).
    pub fn get_screen_size(&self) -> (u32, u32) {
        (1920, 1080)
    }

    /// Polls all windows for events, routes input events to the given
    /// `InputState`, and returns all collected window events.
    pub fn poll_events(&mut self, input: &mut InputState) -> Vec<(WindowId, WindowEvent)> {
        let mut all_events = Vec::new();
        for (&id, backend) in &mut self.windows {
            let events = backend.poll_events();
            for event in events {
                if let Some(input_event) = event.to_input_event() {
                    input.process_event(input_event);
                }
                all_events.push((id, event));
            }
        }
        all_events
    }

    /// Closes a window and removes it from the server.
    pub fn close_window(&mut self, id: WindowId) {
        self.windows.remove(&id);
    }

    /// Returns `true` if the given window is managed and open.
    pub fn is_open(&self, id: WindowId) -> bool {
        self.windows.contains_key(&id)
    }
}

impl Default for DisplayServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{InputState, Key, MouseButton};
    use crate::window::WindowEvent;
    use gdcore::math::Vector2;

    #[test]
    fn display_server_create_window() {
        let mut ds = DisplayServer::new();
        let id = ds.create_window(&WindowConfig::default());
        assert!(ds.is_open(id));
        assert_eq!(ds.window_count(), 1);
    }

    #[test]
    fn display_server_close_window() {
        let mut ds = DisplayServer::new();
        let id = ds.create_window(&WindowConfig::default());
        ds.close_window(id);
        assert!(!ds.is_open(id));
        assert_eq!(ds.window_count(), 0);
    }

    #[test]
    fn display_server_multiple_windows() {
        let mut ds = DisplayServer::new();
        let id1 = ds.create_window(&WindowConfig::new().with_title("A"));
        let id2 = ds.create_window(&WindowConfig::new().with_title("B"));
        assert_eq!(ds.window_count(), 2);
        ds.close_window(id1);
        assert_eq!(ds.window_count(), 1);
        assert!(ds.is_open(id2));
    }

    #[test]
    fn display_server_routes_key_input_to_input_state() {
        let mut ds = DisplayServer::new();
        let id = ds.create_window(&WindowConfig::default());
        let backend = ds.get_window_mut(id).unwrap();
        backend.push_event(WindowEvent::KeyInput {
            key: Key::W,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let mut input = InputState::new();
        ds.poll_events(&mut input);
        assert!(input.is_key_pressed(Key::W));
    }

    #[test]
    fn display_server_routes_mouse_input_to_input_state() {
        let mut ds = DisplayServer::new();
        let id = ds.create_window(&WindowConfig::default());
        let backend = ds.get_window_mut(id).unwrap();
        backend.push_event(WindowEvent::MouseInput {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(100.0, 200.0),
        });

        let mut input = InputState::new();
        ds.poll_events(&mut input);
        assert!(input.is_mouse_button_pressed(MouseButton::Left));
    }

    #[test]
    fn display_server_non_input_events_not_routed() {
        let mut ds = DisplayServer::new();
        let id = ds.create_window(&WindowConfig::default());
        let backend = ds.get_window_mut(id).unwrap();
        backend.push_event(WindowEvent::FocusGained);
        backend.push_event(WindowEvent::CloseRequested);

        let mut input = InputState::new();
        let events = ds.poll_events(&mut input);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn display_server_vsync_config() {
        let mut ds = DisplayServer::new();
        assert_eq!(ds.vsync(), VsyncMode::Enabled);
        ds.set_vsync(VsyncMode::Disabled);
        assert_eq!(ds.vsync(), VsyncMode::Disabled);
        ds.set_vsync(VsyncMode::Adaptive);
        assert_eq!(ds.vsync(), VsyncMode::Adaptive);
    }

    #[test]
    fn display_server_screen_size_stub() {
        let ds = DisplayServer::new();
        assert_eq!(ds.get_screen_size(), (1920, 1080));
    }
}
