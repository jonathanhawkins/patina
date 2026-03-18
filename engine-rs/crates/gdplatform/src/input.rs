//! Input event handling and device management.
//!
//! Provides key/mouse/gamepad enums, input events, an `InputState` singleton-style
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
// GamepadButton
// ---------------------------------------------------------------------------

/// Gamepad button identifiers, mirroring Godot's `JoyButton` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    FaceA,
    FaceB,
    FaceX,
    FaceY,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    LeftStick,
    RightStick,
    Start,
    Select,
    Guide,
}

// ---------------------------------------------------------------------------
// GamepadAxis
// ---------------------------------------------------------------------------

/// Gamepad axis identifiers, mirroring Godot's `JoyAxis` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTriggerAnalog,
    RightTriggerAnalog,
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
    /// A gamepad button event.
    GamepadButton {
        button: GamepadButton,
        pressed: bool,
        gamepad_id: u32,
    },
    /// A gamepad axis event.
    GamepadAxis {
        axis: GamepadAxis,
        value: f32,
        gamepad_id: u32,
    },
    /// A screen touch event.
    ScreenTouch {
        index: u32,
        position: Vector2,
        pressed: bool,
    },
    /// A screen drag event.
    ScreenDrag {
        index: u32,
        position: Vector2,
        relative: Vector2,
        velocity: Vector2,
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
    /// A gamepad button binding.
    GamepadButtonBinding(GamepadButton),
    /// A gamepad axis binding (positive direction).
    GamepadAxisBinding(GamepadAxis),
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

    /// Returns the deadzone for an action, defaulting to 0.0.
    pub fn get_deadzone(&self, action: &str) -> f32 {
        self.deadzones.get(action).copied().unwrap_or(0.0)
    }

    /// Returns `true` when `event` matches any binding for `action`.
    ///
    /// For gamepad axis events, the axis value must exceed the action's deadzone.
    pub fn event_matches_action(&self, event: &InputEvent, action: &str) -> bool {
        let Some(binds) = self.bindings.get(action) else {
            return false;
        };
        let deadzone = self.get_deadzone(action);
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
                (
                    InputEvent::GamepadButton { button, .. },
                    ActionBinding::GamepadButtonBinding(b),
                ) if button == b => {
                    return true;
                }
                (InputEvent::GamepadAxis { axis, value, .. }, ActionBinding::GamepadAxisBinding(a))
                    if axis == a =>
                {
                    return value.abs() > deadzone;
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
    /// Gamepad buttons currently held, keyed by (gamepad_id, button).
    gamepad_buttons_pressed: HashSet<(u32, GamepadButton)>,
    /// Gamepad buttons that became pressed this frame.
    gamepad_buttons_just_pressed: HashSet<(u32, GamepadButton)>,
    /// Gamepad buttons released this frame.
    gamepad_buttons_just_released: HashSet<(u32, GamepadButton)>,
    /// Current gamepad axis values, keyed by (gamepad_id, axis).
    gamepad_axis_values: HashMap<(u32, GamepadAxis), f32>,
    /// Action state: pressed right now.
    actions_pressed: HashSet<String>,
    /// Actions that became pressed this frame.
    actions_just_pressed: HashSet<String>,
    /// Actions that were released this frame.
    actions_just_released: HashSet<String>,
    /// Action strength values (for analog inputs like gamepad axes).
    action_strengths: HashMap<String, f32>,
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
            gamepad_buttons_pressed: HashSet::new(),
            gamepad_buttons_just_pressed: HashSet::new(),
            gamepad_buttons_just_released: HashSet::new(),
            gamepad_axis_values: HashMap::new(),
            actions_pressed: HashSet::new(),
            actions_just_pressed: HashSet::new(),
            actions_just_released: HashSet::new(),
            action_strengths: HashMap::new(),
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
                        InputEvent::GamepadButton { pressed, .. } => *pressed,
                        InputEvent::GamepadAxis { value, .. } => {
                            // Axis is "pressed" when value exceeds deadzone
                            let dz = map.get_deadzone(action);
                            value.abs() > dz
                        }
                        _ => continue,
                    };
                    // For axis events, also store the strength.
                    if let InputEvent::GamepadAxis { value, .. } = &event {
                        self.action_strengths.insert(action.clone(), value.abs());
                    } else if pressed {
                        self.action_strengths.insert(action.clone(), 1.0);
                    } else {
                        self.action_strengths.insert(action.clone(), 0.0);
                    }
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
            InputEvent::GamepadButton {
                button,
                pressed,
                gamepad_id,
            } => {
                let key = (gamepad_id, button);
                if pressed {
                    if self.gamepad_buttons_pressed.insert(key) {
                        self.gamepad_buttons_just_pressed.insert(key);
                    }
                } else if self.gamepad_buttons_pressed.remove(&key) {
                    self.gamepad_buttons_just_released.insert(key);
                }
            }
            InputEvent::GamepadAxis {
                axis,
                value,
                gamepad_id,
            } => {
                self.gamepad_axis_values.insert((gamepad_id, axis), value);
            }
            InputEvent::ScreenTouch { .. } | InputEvent::ScreenDrag { .. } => {
                // Touch events are forwarded to the action system but don't
                // have dedicated state tracking beyond that.
            }
            InputEvent::Action {
                ref action,
                pressed,
                strength,
            } => {
                self.action_strengths.insert(action.clone(), strength);
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

    /// Returns `true` if the given gamepad button is currently held down.
    pub fn is_gamepad_button_pressed(&self, gamepad_id: u32, button: GamepadButton) -> bool {
        self.gamepad_buttons_pressed.contains(&(gamepad_id, button))
    }

    /// Returns the current value of a gamepad axis.
    pub fn get_gamepad_axis_value(&self, gamepad_id: u32, axis: GamepadAxis) -> f32 {
        self.gamepad_axis_values
            .get(&(gamepad_id, axis))
            .copied()
            .unwrap_or(0.0)
    }

    /// Returns the current mouse position.
    pub fn get_mouse_position(&self) -> Vector2 {
        self.mouse_position
    }

    /// Returns the strength of an action (0.0–1.0).
    ///
    /// For digital inputs this is 0.0 or 1.0; for analog inputs it reflects
    /// the axis magnitude.
    pub fn get_action_strength(&self, name: &str) -> f32 {
        if self.actions_pressed.contains(name) {
            self.action_strengths.get(name).copied().unwrap_or(1.0)
        } else {
            0.0
        }
    }

    /// Returns a value between -1.0 and 1.0 based on two actions.
    ///
    /// Mirrors Godot's `Input.get_axis(negative_action, positive_action)`.
    pub fn get_axis(&self, negative: &str, positive: &str) -> f32 {
        let neg = self.get_action_strength(negative);
        let pos = self.get_action_strength(positive);
        (pos - neg).clamp(-1.0, 1.0)
    }

    /// Returns a 2D input vector based on four directional actions.
    ///
    /// Mirrors Godot's `Input.get_vector(neg_x, pos_x, neg_y, pos_y)`.
    pub fn get_vector(
        &self,
        neg_x: &str,
        pos_x: &str,
        neg_y: &str,
        pos_y: &str,
    ) -> Vector2 {
        let x = self.get_axis(neg_x, pos_x);
        let y = self.get_axis(neg_y, pos_y);
        let v = Vector2::new(x, y);
        let len_sq = v.x * v.x + v.y * v.y;
        if len_sq > 1.0 {
            let len = len_sq.sqrt();
            Vector2::new(v.x / len, v.y / len)
        } else {
            v
        }
    }

    /// Clears per-frame state (just_pressed / just_released).
    ///
    /// Call this at the end of each frame, before processing the next batch of events.
    pub fn flush_frame(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
        self.gamepad_buttons_just_pressed.clear();
        self.gamepad_buttons_just_released.clear();
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

    // --- New tests for expanded input system ---

    #[test]
    fn gamepad_button_variants_are_distinct() {
        assert_ne!(GamepadButton::FaceA, GamepadButton::FaceB);
        assert_ne!(GamepadButton::DPadUp, GamepadButton::DPadDown);
        assert_ne!(GamepadButton::LeftShoulder, GamepadButton::RightShoulder);
        assert_ne!(GamepadButton::Start, GamepadButton::Select);
        assert_ne!(GamepadButton::LeftStick, GamepadButton::Guide);
    }

    #[test]
    fn gamepad_axis_variants_are_distinct() {
        assert_ne!(GamepadAxis::LeftStickX, GamepadAxis::LeftStickY);
        assert_ne!(GamepadAxis::RightStickX, GamepadAxis::RightStickY);
        assert_ne!(GamepadAxis::LeftTriggerAnalog, GamepadAxis::RightTriggerAnalog);
    }

    #[test]
    fn input_event_gamepad_button_creation() {
        let evt = InputEvent::GamepadButton {
            button: GamepadButton::FaceA,
            pressed: true,
            gamepad_id: 0,
        };
        if let InputEvent::GamepadButton {
            button,
            pressed,
            gamepad_id,
        } = evt
        {
            assert_eq!(button, GamepadButton::FaceA);
            assert!(pressed);
            assert_eq!(gamepad_id, 0);
        } else {
            panic!("expected GamepadButton event");
        }
    }

    #[test]
    fn input_event_gamepad_axis_creation() {
        let evt = InputEvent::GamepadAxis {
            axis: GamepadAxis::LeftStickX,
            value: 0.75,
            gamepad_id: 1,
        };
        if let InputEvent::GamepadAxis {
            axis,
            value,
            gamepad_id,
        } = evt
        {
            assert_eq!(axis, GamepadAxis::LeftStickX);
            assert_eq!(value, 0.75);
            assert_eq!(gamepad_id, 1);
        } else {
            panic!("expected GamepadAxis event");
        }
    }

    #[test]
    fn input_event_screen_touch_creation() {
        let evt = InputEvent::ScreenTouch {
            index: 0,
            position: Vector2::new(100.0, 200.0),
            pressed: true,
        };
        if let InputEvent::ScreenTouch {
            index,
            position,
            pressed,
        } = evt
        {
            assert_eq!(index, 0);
            assert_eq!(position, Vector2::new(100.0, 200.0));
            assert!(pressed);
        } else {
            panic!("expected ScreenTouch event");
        }
    }

    #[test]
    fn input_event_screen_drag_creation() {
        let evt = InputEvent::ScreenDrag {
            index: 1,
            position: Vector2::new(50.0, 60.0),
            relative: Vector2::new(5.0, 6.0),
            velocity: Vector2::new(100.0, 120.0),
        };
        if let InputEvent::ScreenDrag {
            index,
            position,
            relative,
            velocity,
        } = evt
        {
            assert_eq!(index, 1);
            assert_eq!(position, Vector2::new(50.0, 60.0));
            assert_eq!(relative, Vector2::new(5.0, 6.0));
            assert_eq!(velocity, Vector2::new(100.0, 120.0));
        } else {
            panic!("expected ScreenDrag event");
        }
    }

    #[test]
    fn gamepad_button_press_and_release() {
        let mut state = InputState::new();
        state.process_event(InputEvent::GamepadButton {
            button: GamepadButton::FaceA,
            pressed: true,
            gamepad_id: 0,
        });
        assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
        assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
        assert!(!state.is_gamepad_button_pressed(1, GamepadButton::FaceA));

        state.process_event(InputEvent::GamepadButton {
            button: GamepadButton::FaceA,
            pressed: false,
            gamepad_id: 0,
        });
        assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    }

    #[test]
    fn gamepad_button_just_pressed_clears_on_flush() {
        let mut state = InputState::new();
        state.process_event(InputEvent::GamepadButton {
            button: GamepadButton::Start,
            pressed: true,
            gamepad_id: 0,
        });
        assert!(state.gamepad_buttons_just_pressed.contains(&(0, GamepadButton::Start)));

        state.flush_frame();
        assert!(!state.gamepad_buttons_just_pressed.contains(&(0, GamepadButton::Start)));
        assert!(state.is_gamepad_button_pressed(0, GamepadButton::Start));
    }

    #[test]
    fn gamepad_axis_value_tracking() {
        let mut state = InputState::new();
        state.process_event(InputEvent::GamepadAxis {
            axis: GamepadAxis::LeftStickX,
            value: 0.8,
            gamepad_id: 0,
        });
        assert_eq!(state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX), 0.8);
        assert_eq!(state.get_gamepad_axis_value(0, GamepadAxis::LeftStickY), 0.0);
    }

    #[test]
    fn input_map_gamepad_button_binding() {
        let mut map = InputMap::new();
        map.add_action("jump", 0.0);
        map.action_add_event("jump", ActionBinding::GamepadButtonBinding(GamepadButton::FaceA));

        let evt = InputEvent::GamepadButton {
            button: GamepadButton::FaceA,
            pressed: true,
            gamepad_id: 0,
        };
        assert!(map.event_matches_action(&evt, "jump"));

        let evt_b = InputEvent::GamepadButton {
            button: GamepadButton::FaceB,
            pressed: true,
            gamepad_id: 0,
        };
        assert!(!map.event_matches_action(&evt_b, "jump"));
    }

    #[test]
    fn input_map_gamepad_axis_binding_with_deadzone() {
        let mut map = InputMap::new();
        map.add_action("move_right", 0.2);
        map.action_add_event("move_right", ActionBinding::GamepadAxisBinding(GamepadAxis::LeftStickX));

        // Below deadzone
        let evt_low = InputEvent::GamepadAxis {
            axis: GamepadAxis::LeftStickX,
            value: 0.1,
            gamepad_id: 0,
        };
        assert!(!map.event_matches_action(&evt_low, "move_right"));

        // Above deadzone
        let evt_high = InputEvent::GamepadAxis {
            axis: GamepadAxis::LeftStickX,
            value: 0.5,
            gamepad_id: 0,
        };
        assert!(map.event_matches_action(&evt_high, "move_right"));
    }

    #[test]
    fn gamepad_button_action_resolution() {
        let mut map = InputMap::new();
        map.add_action("fire", 0.0);
        map.action_add_event("fire", ActionBinding::GamepadButtonBinding(GamepadButton::RightTrigger));

        let mut state = InputState::new();
        state.set_input_map(map);

        state.process_event(InputEvent::GamepadButton {
            button: GamepadButton::RightTrigger,
            pressed: true,
            gamepad_id: 0,
        });
        assert!(state.is_action_pressed("fire"));
        assert!(state.is_action_just_pressed("fire"));

        state.flush_frame();
        state.process_event(InputEvent::GamepadButton {
            button: GamepadButton::RightTrigger,
            pressed: false,
            gamepad_id: 0,
        });
        assert!(!state.is_action_pressed("fire"));
        assert!(state.is_action_just_released("fire"));
    }

    #[test]
    fn get_axis_with_keys() {
        let mut map = InputMap::new();
        map.add_action("move_left", 0.0);
        map.add_action("move_right", 0.0);
        map.action_add_event("move_left", ActionBinding::KeyBinding(Key::A));
        map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));

        let mut state = InputState::new();
        state.set_input_map(map);

        // Nothing pressed → 0.0
        assert_eq!(state.get_axis("move_left", "move_right"), 0.0);

        // Press right → 1.0
        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(state.get_axis("move_left", "move_right"), 1.0);

        // Also press left → 0.0 (cancel out)
        state.process_event(InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(state.get_axis("move_left", "move_right"), 0.0);

        // Release right → -1.0
        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(state.get_axis("move_left", "move_right"), -1.0);
    }

    #[test]
    fn get_vector_returns_normalized_diagonal() {
        let mut map = InputMap::new();
        map.add_action("left", 0.0);
        map.add_action("right", 0.0);
        map.add_action("up", 0.0);
        map.add_action("down", 0.0);
        map.action_add_event("left", ActionBinding::KeyBinding(Key::A));
        map.action_add_event("right", ActionBinding::KeyBinding(Key::D));
        map.action_add_event("up", ActionBinding::KeyBinding(Key::W));
        map.action_add_event("down", ActionBinding::KeyBinding(Key::S));

        let mut state = InputState::new();
        state.set_input_map(map);

        // Press right + down → should be normalized
        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        state.process_event(InputEvent::Key {
            key: Key::S,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let v = state.get_vector("left", "right", "up", "down");
        let len = (v.x * v.x + v.y * v.y).sqrt();
        assert!((len - 1.0).abs() < 0.001, "diagonal vector should be normalized, got len={len}");
        assert!(v.x > 0.0);
        assert!(v.y > 0.0);
    }

    #[test]
    fn get_vector_single_axis() {
        let mut map = InputMap::new();
        map.add_action("left", 0.0);
        map.add_action("right", 0.0);
        map.add_action("up", 0.0);
        map.add_action("down", 0.0);
        map.action_add_event("right", ActionBinding::KeyBinding(Key::D));

        let mut state = InputState::new();
        state.set_input_map(map);

        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let v = state.get_vector("left", "right", "up", "down");
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 0.0);
    }

    #[test]
    fn action_strength_from_action_event() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Action {
            action: "throttle".to_string(),
            pressed: true,
            strength: 0.6,
        });
        assert_eq!(state.get_action_strength("throttle"), 0.6);

        state.process_event(InputEvent::Action {
            action: "throttle".to_string(),
            pressed: false,
            strength: 0.0,
        });
        assert_eq!(state.get_action_strength("throttle"), 0.0);
    }
}
