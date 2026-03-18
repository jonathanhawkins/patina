//! Input event handling and device management.
//!
//! Provides key/mouse enums, input events, an `InputState` singleton-style
//! tracker, and an `InputMap` for binding actions to physical inputs.

use std::collections::{HashMap, HashSet};

use gdcore::math::Vector2;

// ---------------------------------------------------------------------------
// Key
// ---------------------------------------------------------------------------

/// Physical/logical key codes, mirroring Godot's `Key` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    Space, Enter, Escape, Tab,
    Shift, Ctrl, Alt,
    Up, Down, Left, Right,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Backspace, Delete, Insert, Home, End, PageUp, PageDown,
    CapsLock, Comma, Period, Slash, Semicolon, Quote,
    BracketLeft, BracketRight, Backslash, Minus, Equal,
}

// ---------------------------------------------------------------------------
// MouseButton
// ---------------------------------------------------------------------------

/// Mouse button identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    WheelUp,
    WheelDown,
}

// ---------------------------------------------------------------------------
// InputEvent
// ---------------------------------------------------------------------------

/// A single input event, analogous to Godot's `InputEvent` hierarchy.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A keyboard key event.
    Key {
        key: Key,
        pressed: bool,
        shift: bool,
        ctrl: bool,
        alt: bool,
    },
    /// A mouse button event.
    MouseButton {
        button: MouseButton,
        pressed: bool,
        position: Vector2,
    },
    /// Mouse motion event.
    MouseMotion {
        position: Vector2,
        relative: Vector2,
    },
    /// A named action event (e.g. from `InputMap`).
    Action {
        action: String,
        pressed: bool,
        strength: f32,
    },
}

// ---------------------------------------------------------------------------
// ActionBinding
// ---------------------------------------------------------------------------

/// What physical input is bound to an action.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionBinding {
    /// A keyboard key binding.
    KeyBinding(Key),
    /// A mouse button binding.
    MouseBinding(MouseButton),
}

// ---------------------------------------------------------------------------
// InputMap
// ---------------------------------------------------------------------------

/// Maps named actions to physical key/mouse bindings.
///
/// Mirrors Godot's `InputMap` singleton. Actions are created with a deadzone,
/// then one or more bindings are attached.
#[derive(Debug, Clone)]
pub struct InputMap {
    /// Deadzone per action.
    deadzones: HashMap<String, f32>,
    /// Bindings per action.
    bindings: HashMap<String, Vec<ActionBinding>>,
}

impl InputMap {
    /// Creates an empty input map.
    pub fn new() -> Self {
        Self {
            deadzones: HashMap::new(),
            bindings: HashMap::new(),
        }
    }

    /// Registers a new action with the given deadzone.
    pub fn add_action(&mut self, name: impl Into<String>, deadzone: f32) {
        let name = name.into();
        self.deadzones.insert(name.clone(), deadzone);
        self.bindings.entry(name).or_default();
    }

    /// Binds a physical input to an existing action.
    ///
    /// Does nothing if the action has not been registered with `add_action`.
    pub fn action_add_event(&mut self, name: impl Into<String>, binding: ActionBinding) {
        let name = name.into();
        if let Some(list) = self.bindings.get_mut(&name) {
            list.push(binding);
        }
    }

    /// Returns `true` when `event` matches any binding for `action`.
    pub fn event_matches_action(&self, event: &InputEvent, action: &str) -> bool {
        let Some(binds) = self.bindings.get(action) else {
            return false;
        };
        for bind in binds {
            match (event, bind) {
                (InputEvent::Key { key, .. }, ActionBinding::KeyBinding(k)) if key == k => {
                    return true;
                }
                (InputEvent::MouseButton { button, .. }, ActionBinding::MouseBinding(b))
                    if button == b =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Returns all action names.
    pub fn actions(&self) -> impl Iterator<Item = &String> {
        self.deadzones.keys()
    }

    /// Returns bindings for an action.
    pub fn get_bindings(&self, action: &str) -> Option<&[ActionBinding]> {
        self.bindings.get(action).map(|v| v.as_slice())
    }
}

impl Default for InputMap {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// InputState
// ---------------------------------------------------------------------------

/// Tracks the current state of all inputs, analogous to Godot's `Input` singleton.
///
/// Call `process_event` for each event, query state with `is_key_pressed` etc.,
/// and call `flush_frame` at the end of each frame to reset per-frame state.
#[derive(Debug, Clone)]
pub struct InputState {
    keys_pressed: HashSet<Key>,
    keys_just_pressed: HashSet<Key>,
    keys_just_released: HashSet<Key>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_just_pressed: HashSet<MouseButton>,
    mouse_just_released: HashSet<MouseButton>,
    mouse_position: Vector2,
    /// Action state: pressed right now.
    actions_pressed: HashSet<String>,
    /// Actions that became pressed this frame.
    actions_just_pressed: HashSet<String>,
    /// Actions that were released this frame.
    actions_just_released: HashSet<String>,
    /// Optional input map for action resolution.
    input_map: Option<InputMap>,
}

impl InputState {
    /// Creates a new, empty input state.
    pub fn new() -> Self {
        Self {
            keys_pressed: HashSet::new(),
            keys_just_pressed: HashSet::new(),
            keys_just_released: HashSet::new(),
            mouse_pressed: HashSet::new(),
            mouse_just_pressed: HashSet::new(),
            mouse_just_released: HashSet::new(),
            mouse_position: Vector2::ZERO,
            actions_pressed: HashSet::new(),
            actions_just_pressed: HashSet::new(),
            actions_just_released: HashSet::new(),
            input_map: None,
        }
    }

    /// Attaches an `InputMap` so that physical events automatically resolve to actions.
    pub fn set_input_map(&mut self, map: InputMap) {
        self.input_map = Some(map);
    }

    /// Processes a single input event, updating internal state.
    pub fn process_event(&mut self, event: InputEvent) {
        // Resolve actions via InputMap before processing the raw event.
        if let Some(map) = &self.input_map {
            let map = map.clone();
            for action in map.actions() {
                if map.event_matches_action(&event, action) {
                    let pressed = match &event {
                        InputEvent::Key { pressed, .. } => *pressed,
                        InputEvent::MouseButton { pressed, .. } => *pressed,
                        _ => continue,
                    };
                    if pressed {
                        if self.actions_pressed.insert(action.clone()) {
                            self.actions_just_pressed.insert(action.clone());
                        }
                    } else if self.actions_pressed.remove(action) {
                        self.actions_just_released.insert(action.clone());
                    }
                }
            }
        }

        match event {
            InputEvent::Key { key, pressed, .. } => {
                if pressed {
                    if self.keys_pressed.insert(key) {
                        self.keys_just_pressed.insert(key);
                    }
                } else if self.keys_pressed.remove(&key) {
                    self.keys_just_released.insert(key);
                }
            }
            InputEvent::MouseButton {
                button,
                pressed,
                position,
            } => {
                self.mouse_position = position;
                if pressed {
                    if self.mouse_pressed.insert(button) {
                        self.mouse_just_pressed.insert(button);
                    }
                } else if self.mouse_pressed.remove(&button) {
                    self.mouse_just_released.insert(button);
                }
            }
            InputEvent::MouseMotion { position, .. } => {
                self.mouse_position = position;
            }
            InputEvent::Action {
                ref action,
                pressed,
                ..
            } => {
                if pressed {
                    if self.actions_pressed.insert(action.clone()) {
                        self.actions_just_pressed.insert(action.clone());
                    }
                } else if self.actions_pressed.remove(action) {
                    self.actions_just_released.insert(action.clone());
                }
            }
        }
    }

    /// Returns `true` if the given key is currently held down.
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// Returns `true` if the given mouse button is currently held down.
    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.contains(&button)
    }

    /// Returns `true` if the named action is currently pressed.
    pub fn is_action_pressed(&self, name: &str) -> bool {
        self.actions_pressed.contains(name)
    }

    /// Returns `true` if the action became pressed this frame.
    pub fn is_action_just_pressed(&self, name: &str) -> bool {
        self.actions_just_pressed.contains(name)
    }

    /// Returns `true` if the action was released this frame.
    pub fn is_action_just_released(&self, name: &str) -> bool {
        self.actions_just_released.contains(name)
    }

    /// Returns the current mouse position.
    pub fn get_mouse_position(&self) -> Vector2 {
        self.mouse_position
    }

    /// Clears per-frame state (just_pressed / just_released).
    ///
    /// Call this at the end of each frame, before processing the next batch of events.
    pub fn flush_frame(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
        self.actions_just_pressed.clear();
        self.actions_just_released.clear();
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_variants_are_distinct() {
        assert_ne!(Key::A, Key::B);
        assert_ne!(Key::Num0, Key::Num9);
        assert_ne!(Key::Space, Key::Enter);
        assert_ne!(Key::F1, Key::F12);
        assert_ne!(Key::Up, Key::Down);
        assert_ne!(Key::Shift, Key::Ctrl);
    }

    #[test]
    fn input_event_key_creation() {
        let evt = InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        if let InputEvent::Key { key, pressed, .. } = evt {
            assert_eq!(key, Key::A);
            assert!(pressed);
        } else {
            panic!("expected Key event");
        }
    }

    #[test]
    fn input_event_mouse_button_creation() {
        let evt = InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(100.0, 200.0),
        };
        if let InputEvent::MouseButton {
            button, position, ..
        } = evt
        {
            assert_eq!(button, MouseButton::Left);
            assert_eq!(position.x, 100.0);
        } else {
            panic!("expected MouseButton event");
        }
    }

    #[test]
    fn input_event_mouse_motion_creation() {
        let evt = InputEvent::MouseMotion {
            position: Vector2::new(10.0, 20.0),
            relative: Vector2::new(1.0, 2.0),
        };
        if let InputEvent::MouseMotion { position, relative } = evt {
            assert_eq!(position, Vector2::new(10.0, 20.0));
            assert_eq!(relative, Vector2::new(1.0, 2.0));
        } else {
            panic!("expected MouseMotion event");
        }
    }

    #[test]
    fn input_event_action_creation() {
        let evt = InputEvent::Action {
            action: "jump".to_string(),
            pressed: true,
            strength: 1.0,
        };
        if let InputEvent::Action {
            action, strength, ..
        } = evt
        {
            assert_eq!(action, "jump");
            assert_eq!(strength, 1.0);
        } else {
            panic!("expected Action event");
        }
    }

    #[test]
    fn input_state_key_press() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::W,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.is_key_pressed(Key::W));
        assert!(!state.is_key_pressed(Key::S));
    }

    #[test]
    fn input_state_key_release() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::W,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.is_key_pressed(Key::W));

        state.process_event(InputEvent::Key {
            key: Key::W,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(!state.is_key_pressed(Key::W));
    }

    #[test]
    fn input_state_mouse_button_tracking() {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::ZERO,
        });
        assert!(state.is_mouse_button_pressed(MouseButton::Left));
        assert!(!state.is_mouse_button_pressed(MouseButton::Right));
    }

    #[test]
    fn input_state_just_pressed_true_on_press_frame() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.keys_just_pressed.contains(&Key::Space));
    }

    #[test]
    fn input_state_just_pressed_false_after_flush() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        state.flush_frame();
        assert!(!state.keys_just_pressed.contains(&Key::Space));
        // Key should still be pressed though.
        assert!(state.is_key_pressed(Key::Space));
    }

    #[test]
    fn input_state_just_released_on_release_frame() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        state.flush_frame();
        state.process_event(InputEvent::Key {
            key: Key::A,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.keys_just_released.contains(&Key::A));
    }

    #[test]
    fn input_map_add_action_and_bind_key() {
        let mut map = InputMap::new();
        map.add_action("jump", 0.5);
        map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));

        let evt = InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(map.event_matches_action(&evt, "jump"));
    }

    #[test]
    fn input_map_action_with_multiple_bindings() {
        let mut map = InputMap::new();
        map.add_action("move_left", 0.0);
        map.action_add_event("move_left", ActionBinding::KeyBinding(Key::A));
        map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));

        let evt_a = InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let evt_left = InputEvent::Key {
            key: Key::Left,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(map.event_matches_action(&evt_a, "move_left"));
        assert!(map.event_matches_action(&evt_left, "move_left"));
    }

    #[test]
    fn input_state_with_input_map_action_pressed() {
        let mut map = InputMap::new();
        map.add_action("shoot", 0.0);
        map.action_add_event("shoot", ActionBinding::MouseBinding(MouseButton::Left));

        let mut state = InputState::new();
        state.set_input_map(map);

        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::ZERO,
        });

        assert!(state.is_action_pressed("shoot"));
        assert!(state.is_action_just_pressed("shoot"));

        state.flush_frame();
        assert!(state.is_action_pressed("shoot"));
        assert!(!state.is_action_just_pressed("shoot"));

        // Release
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: false,
            position: Vector2::ZERO,
        });
        assert!(!state.is_action_pressed("shoot"));
        assert!(state.is_action_just_released("shoot"));
    }

    #[test]
    fn mouse_position_tracking() {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseMotion {
            position: Vector2::new(320.0, 240.0),
            relative: Vector2::new(5.0, -3.0),
        });
        assert_eq!(state.get_mouse_position(), Vector2::new(320.0, 240.0));
    }
}
