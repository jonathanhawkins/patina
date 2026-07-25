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
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space,
    Enter,
    Escape,
    Tab,
    Shift,
    Ctrl,
    Alt,
    Up,
    Down,
    Left,
    Right,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    CapsLock,
    Comma,
    Period,
    Slash,
    Semicolon,
    Quote,
    BracketLeft,
    BracketRight,
    Backslash,
    Minus,
    Equal,
}

impl Key {
    /// Returns the string name for this key, matching the names used by
    /// GDScript's `Input.is_action_pressed()` and the editor's key event strings.
    pub fn name(self) -> &'static str {
        match self {
            Key::A => "A",
            Key::B => "B",
            Key::C => "C",
            Key::D => "D",
            Key::E => "E",
            Key::F => "F",
            Key::G => "G",
            Key::H => "H",
            Key::I => "I",
            Key::J => "J",
            Key::K => "K",
            Key::L => "L",
            Key::M => "M",
            Key::N => "N",
            Key::O => "O",
            Key::P => "P",
            Key::Q => "Q",
            Key::R => "R",
            Key::S => "S",
            Key::T => "T",
            Key::U => "U",
            Key::V => "V",
            Key::W => "W",
            Key::X => "X",
            Key::Y => "Y",
            Key::Z => "Z",
            Key::Num0 => "0",
            Key::Num1 => "1",
            Key::Num2 => "2",
            Key::Num3 => "3",
            Key::Num4 => "4",
            Key::Num5 => "5",
            Key::Num6 => "6",
            Key::Num7 => "7",
            Key::Num8 => "8",
            Key::Num9 => "9",
            Key::Space => " ",
            Key::Enter => "Enter",
            Key::Escape => "Escape",
            Key::Tab => "Tab",
            Key::Shift => "Shift",
            Key::Ctrl => "Ctrl",
            Key::Alt => "Alt",
            Key::Up => "ArrowUp",
            Key::Down => "ArrowDown",
            Key::Left => "ArrowLeft",
            Key::Right => "ArrowRight",
            Key::F1 => "F1",
            Key::F2 => "F2",
            Key::F3 => "F3",
            Key::F4 => "F4",
            Key::F5 => "F5",
            Key::F6 => "F6",
            Key::F7 => "F7",
            Key::F8 => "F8",
            Key::F9 => "F9",
            Key::F10 => "F10",
            Key::F11 => "F11",
            Key::F12 => "F12",
            Key::Backspace => "Backspace",
            Key::Delete => "Delete",
            Key::Insert => "Insert",
            Key::Home => "Home",
            Key::End => "End",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::CapsLock => "CapsLock",
            Key::Comma => ",",
            Key::Period => ".",
            Key::Slash => "/",
            Key::Semicolon => ";",
            Key::Quote => "'",
            Key::BracketLeft => "[",
            Key::BracketRight => "]",
            Key::Backslash => "\\",
            Key::Minus => "-",
            Key::Equal => "=",
        }
    }

    /// Parses a browser/Godot key name string back into a `Key` enum.
    ///
    /// Returns `None` if the name is unrecognized. Accepts both lowercase
    /// single-char keys (`"a"`) and the canonical names from [`Key::name`].
    pub fn from_name(name: &str) -> Option<Key> {
        match name {
            "A" | "a" => Some(Key::A),
            "B" | "b" => Some(Key::B),
            "C" | "c" => Some(Key::C),
            "D" | "d" => Some(Key::D),
            "E" | "e" => Some(Key::E),
            "F" | "f" => Some(Key::F),
            "G" | "g" => Some(Key::G),
            "H" | "h" => Some(Key::H),
            "I" | "i" => Some(Key::I),
            "J" | "j" => Some(Key::J),
            "K" | "k" => Some(Key::K),
            "L" | "l" => Some(Key::L),
            "M" | "m" => Some(Key::M),
            "N" | "n" => Some(Key::N),
            "O" | "o" => Some(Key::O),
            "P" | "p" => Some(Key::P),
            "Q" | "q" => Some(Key::Q),
            "R" | "r" => Some(Key::R),
            "S" | "s" => Some(Key::S),
            "T" | "t" => Some(Key::T),
            "U" | "u" => Some(Key::U),
            "V" | "v" => Some(Key::V),
            "W" | "w" => Some(Key::W),
            "X" | "x" => Some(Key::X),
            "Y" | "y" => Some(Key::Y),
            "Z" | "z" => Some(Key::Z),
            "0" => Some(Key::Num0),
            "1" => Some(Key::Num1),
            "2" => Some(Key::Num2),
            "3" => Some(Key::Num3),
            "4" => Some(Key::Num4),
            "5" => Some(Key::Num5),
            "6" => Some(Key::Num6),
            "7" => Some(Key::Num7),
            "8" => Some(Key::Num8),
            "9" => Some(Key::Num9),
            " " | "Space" => Some(Key::Space),
            "Enter" => Some(Key::Enter),
            "Escape" => Some(Key::Escape),
            "Tab" => Some(Key::Tab),
            "Shift" => Some(Key::Shift),
            "Ctrl" | "Control" => Some(Key::Ctrl),
            "Alt" => Some(Key::Alt),
            "ArrowUp" => Some(Key::Up),
            "ArrowDown" => Some(Key::Down),
            "ArrowLeft" => Some(Key::Left),
            "ArrowRight" => Some(Key::Right),
            "F1" => Some(Key::F1),
            "F2" => Some(Key::F2),
            "F3" => Some(Key::F3),
            "F4" => Some(Key::F4),
            "F5" => Some(Key::F5),
            "F6" => Some(Key::F6),
            "F7" => Some(Key::F7),
            "F8" => Some(Key::F8),
            "F9" => Some(Key::F9),
            "F10" => Some(Key::F10),
            "F11" => Some(Key::F11),
            "F12" => Some(Key::F12),
            "Backspace" => Some(Key::Backspace),
            "Delete" => Some(Key::Delete),
            "Insert" => Some(Key::Insert),
            "Home" => Some(Key::Home),
            "End" => Some(Key::End),
            "PageUp" => Some(Key::PageUp),
            "PageDown" => Some(Key::PageDown),
            "CapsLock" => Some(Key::CapsLock),
            "," => Some(Key::Comma),
            "." => Some(Key::Period),
            "/" => Some(Key::Slash),
            ";" => Some(Key::Semicolon),
            "'" => Some(Key::Quote),
            "[" => Some(Key::BracketLeft),
            "]" => Some(Key::BracketRight),
            "\\" => Some(Key::Backslash),
            "-" => Some(Key::Minus),
            "=" => Some(Key::Equal),
            _ => None,
        }
    }
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

impl GamepadButton {
    /// Maps a W3C "standard gamepad" button index — the position in the
    /// `buttons[]` array reported by the browser `navigator.getGamepads()` — to
    /// a semantic [`GamepadButton`]. Returns `None` for indices outside the
    /// standard layout so non-standard-mapped pads are ignored rather than
    /// mis-bound (matching how Godot's web build only trusts `mapping ==
    /// "standard"`).
    pub fn from_standard(index: usize) -> Option<GamepadButton> {
        Some(match index {
            0 => GamepadButton::FaceA, // bottom (Xbox A / PS Cross)
            1 => GamepadButton::FaceB, // right  (Xbox B / PS Circle)
            2 => GamepadButton::FaceX, // left   (Xbox X / PS Square)
            3 => GamepadButton::FaceY, // top    (Xbox Y / PS Triangle)
            4 => GamepadButton::LeftShoulder,
            5 => GamepadButton::RightShoulder,
            6 => GamepadButton::LeftTrigger,
            7 => GamepadButton::RightTrigger,
            8 => GamepadButton::Select, // Back / View / Share
            9 => GamepadButton::Start,  // Forward / Menu / Options
            10 => GamepadButton::LeftStick,
            11 => GamepadButton::RightStick,
            12 => GamepadButton::DPadUp,
            13 => GamepadButton::DPadDown,
            14 => GamepadButton::DPadLeft,
            15 => GamepadButton::DPadRight,
            16 => GamepadButton::Guide, // vendor / home
            _ => return None,
        })
    }
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

impl GamepadAxis {
    /// Maps a W3C "standard gamepad" axis index — the position in the `axes[]`
    /// array reported by the browser `navigator.getGamepads()` — to a semantic
    /// [`GamepadAxis`]. Returns `None` for indices outside the standard layout
    /// (the standard mapping exposes exactly the two thumbsticks; analog
    /// triggers arrive as `buttons[6]`/`buttons[7]` values, not axes).
    pub fn from_standard(index: usize) -> Option<GamepadAxis> {
        Some(match index {
            0 => GamepadAxis::LeftStickX,
            1 => GamepadAxis::LeftStickY,
            2 => GamepadAxis::RightStickX,
            3 => GamepadAxis::RightStickY,
            _ => return None,
        })
    }
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

    /// Registers Godot's built-in `ui_*` actions with their default key/gamepad
    /// events (deadzone 0.5), mirroring `InputMap::load_from_project_settings`.
    /// Project-declared actions override these — see [`load_from_project_godot`].
    ///
    /// Note: modifier combos (e.g. Shift+Tab for `ui_focus_prev`) and keys not
    /// in [`Key`] (Home/End/PageUp/PageDown, the text-editing set) aren't yet
    /// expressible; the common navigation/accept/cancel/select set is covered.
    ///
    /// [`load_from_project_godot`]: InputMap::load_from_project_godot
    pub fn install_builtin_ui_actions(&mut self) {
        let builtins: Vec<(&str, Vec<ActionBinding>)> = vec![
            (
                "ui_accept",
                vec![
                    ActionBinding::KeyBinding(Key::Enter),
                    ActionBinding::KeyBinding(Key::Space),
                    ActionBinding::GamepadButtonBinding(GamepadButton::FaceA),
                ],
            ),
            ("ui_select", vec![ActionBinding::KeyBinding(Key::Space)]),
            (
                "ui_cancel",
                vec![
                    ActionBinding::KeyBinding(Key::Escape),
                    ActionBinding::GamepadButtonBinding(GamepadButton::FaceB),
                ],
            ),
            ("ui_focus_next", vec![ActionBinding::KeyBinding(Key::Tab)]),
            (
                "ui_focus_prev",
                vec![ActionBinding::GamepadButtonBinding(
                    GamepadButton::LeftShoulder,
                )],
            ),
            (
                "ui_left",
                vec![
                    ActionBinding::KeyBinding(Key::Left),
                    ActionBinding::GamepadButtonBinding(GamepadButton::DPadLeft),
                ],
            ),
            (
                "ui_right",
                vec![
                    ActionBinding::KeyBinding(Key::Right),
                    ActionBinding::GamepadButtonBinding(GamepadButton::DPadRight),
                ],
            ),
            (
                "ui_up",
                vec![
                    ActionBinding::KeyBinding(Key::Up),
                    ActionBinding::GamepadButtonBinding(GamepadButton::DPadUp),
                ],
            ),
            (
                "ui_down",
                vec![
                    ActionBinding::KeyBinding(Key::Down),
                    ActionBinding::GamepadButtonBinding(GamepadButton::DPadDown),
                ],
            ),
        ];
        for (name, bindings) in builtins {
            self.add_action(name, 0.5);
            for binding in bindings {
                self.action_add_event(name, binding);
            }
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
                (
                    InputEvent::GamepadAxis { axis, value, .. },
                    ActionBinding::GamepadAxisBinding(a),
                ) if axis == a => {
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
/// Mouse capture/visibility mode, mirroring Godot's `Input.MouseMode`
/// (`Input.set_mouse_mode` / `MOUSE_MODE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MouseMode {
    /// Cursor visible and free — Godot `MOUSE_MODE_VISIBLE` (0).
    #[default]
    Visible,
    /// Cursor hidden but free — `MOUSE_MODE_HIDDEN` (1).
    Hidden,
    /// Cursor hidden and locked to the window center — `MOUSE_MODE_CAPTURED` (2).
    Captured,
    /// Cursor visible but confined to the window — `MOUSE_MODE_CONFINED` (3).
    Confined,
    /// Cursor hidden and confined — `MOUSE_MODE_CONFINED_HIDDEN` (4).
    ConfinedHidden,
}

impl MouseMode {
    /// Returns the Godot integer value for this mode.
    pub fn to_godot(self) -> i64 {
        match self {
            MouseMode::Visible => 0,
            MouseMode::Hidden => 1,
            MouseMode::Captured => 2,
            MouseMode::Confined => 3,
            MouseMode::ConfinedHidden => 4,
        }
    }

    /// Parses a Godot mouse-mode integer, returning `None` for unknown values.
    pub fn from_godot(value: i64) -> Option<MouseMode> {
        Some(match value {
            0 => MouseMode::Visible,
            1 => MouseMode::Hidden,
            2 => MouseMode::Captured,
            3 => MouseMode::Confined,
            4 => MouseMode::ConfinedHidden,
            _ => return None,
        })
    }
}

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
    /// Current mouse capture/visibility mode (`Input.set_mouse_mode`).
    mouse_mode: MouseMode,
    /// Connected joypad device ids (`Input.get_connected_joypads`).
    connected_joypads: HashSet<u32>,
    /// Active joypad vibration per device: `(weak, strong, duration_seconds)`.
    joy_vibration: HashMap<u32, (f32, f32, f32)>,
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
            mouse_mode: MouseMode::Visible,
            connected_joypads: HashSet::new(),
            joy_vibration: HashMap::new(),
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

    /// Deadzone for `action` from the bound [`InputMap`], or `0.0` if unmapped.
    fn action_deadzone(&self, action: &str) -> f32 {
        self.input_map
            .as_ref()
            .map(|m| m.get_deadzone(action))
            .unwrap_or(0.0)
    }

    /// Returns a 2D input vector based on four directional actions.
    ///
    /// Mirrors Godot's `Input.get_vector(neg_x, pos_x, neg_y, pos_y)`
    /// (core/input/input.cpp): the raw axis vector is formed first, then a
    /// **circular** deadzone (the average of the four actions' deadzones) is
    /// applied to the vector's *length* and the result renormalized — so a
    /// diagonal is length-1 and small analog drift inside the deadzone reads as
    /// zero. (pat-nzgdu.9)
    pub fn get_vector(&self, neg_x: &str, pos_x: &str, neg_y: &str, pos_y: &str) -> Vector2 {
        let raw = Vector2::new(
            self.get_action_strength(pos_x) - self.get_action_strength(neg_x),
            self.get_action_strength(pos_y) - self.get_action_strength(neg_y),
        );
        let deadzone = 0.25
            * (self.action_deadzone(pos_x)
                + self.action_deadzone(neg_x)
                + self.action_deadzone(pos_y)
                + self.action_deadzone(neg_y));
        let length = (raw.x * raw.x + raw.y * raw.y).sqrt();
        if length <= deadzone {
            Vector2::new(0.0, 0.0)
        } else if length > 1.0 {
            // Clamp to the unit circle.
            Vector2::new(raw.x / length, raw.y / length)
        } else {
            // Rescale the (deadzone, 1] band back to (0, 1] along the direction.
            let scale = ((length - deadzone) / (1.0 - deadzone)) / length;
            Vector2::new(raw.x * scale, raw.y * scale)
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

    // -- Mouse mode (Input.set_mouse_mode / get_mouse_mode) -----------------

    /// Sets the mouse capture/visibility mode (`Input.set_mouse_mode`).
    pub fn set_mouse_mode(&mut self, mode: MouseMode) {
        self.mouse_mode = mode;
    }

    /// Returns the current mouse capture/visibility mode (`Input.get_mouse_mode`).
    pub fn mouse_mode(&self) -> MouseMode {
        self.mouse_mode
    }

    // -- Programmatic actions (Input.action_press / action_release) ---------

    /// Presses `action` programmatically with the given strength, mirroring
    /// Godot's `Input.action_press`. Updates just-pressed edge state so
    /// `is_action_just_pressed` reports it for this frame.
    pub fn action_press(&mut self, action: &str, strength: f32) {
        self.action_strengths.insert(action.to_string(), strength);
        if self.actions_pressed.insert(action.to_string()) {
            self.actions_just_pressed.insert(action.to_string());
        }
    }

    /// Releases `action` programmatically (Godot's `Input.action_release`),
    /// updating just-released edge state.
    pub fn action_release(&mut self, action: &str) {
        self.action_strengths.insert(action.to_string(), 0.0);
        if self.actions_pressed.remove(action) {
            self.actions_just_released.insert(action.to_string());
        }
    }

    // -- Joypad enumeration & vibration -------------------------------------

    /// Marks joypad `device` as connected.
    pub fn connect_joypad(&mut self, device: u32) {
        self.connected_joypads.insert(device);
    }

    /// Marks joypad `device` as disconnected and clears any active vibration.
    pub fn disconnect_joypad(&mut self, device: u32) {
        self.connected_joypads.remove(&device);
        self.joy_vibration.remove(&device);
    }

    /// Returns the connected joypad device ids in ascending order
    /// (`Input.get_connected_joypads`).
    pub fn connected_joypads(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.connected_joypads.iter().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Starts a vibration on `device` (`Input.start_joy_vibration`): `weak` and
    /// `strong` motor magnitudes in `[0,1]`, for `duration` seconds
    /// (`0` = indefinite).
    pub fn start_joy_vibration(&mut self, device: u32, weak: f32, strong: f32, duration: f32) {
        self.joy_vibration.insert(device, (weak, strong, duration));
    }

    /// Stops any vibration on `device` (`Input.stop_joy_vibration`).
    pub fn stop_joy_vibration(&mut self, device: u32) {
        self.joy_vibration.remove(&device);
    }

    /// Returns the active vibration `(weak, strong, duration)` for `device`,
    /// or `None` if it is not vibrating.
    pub fn joy_vibration(&self, device: u32) -> Option<(f32, f32, f32)> {
        self.joy_vibration.get(&device).copied()
    }

    /// Applies one poll of a W3C "standard mapping" browser gamepad (a single
    /// `navigator.getGamepads()` entry) for `device`: marks the pad connected,
    /// then normalizes and pushes its button/axis state through the same
    /// `process_event` path physical devices use. `buttons`/`axes` follow the
    /// standard layout (see [`GamepadButton::from_standard`] /
    /// [`GamepadAxis::from_standard`]); indices outside it are ignored.
    ///
    /// This is the normalization the `/api/runtime/input/gamepad` producer runs
    /// so a browser controller reaches `is_gamepad_button_pressed`
    /// (`Input.is_joy_button_pressed`) / `get_gamepad_axis_value`
    /// (`Input.get_joy_axis`) and any bound action in a running scene.
    pub fn apply_standard_gamepad_frame(&mut self, device: u32, buttons: &[bool], axes: &[f32]) {
        self.connect_joypad(device);
        for (index, &pressed) in buttons.iter().enumerate() {
            if let Some(button) = GamepadButton::from_standard(index) {
                self.process_event(InputEvent::GamepadButton {
                    button,
                    pressed,
                    gamepad_id: device,
                });
            }
        }
        for (index, &value) in axes.iter().enumerate() {
            if let Some(axis) = GamepadAxis::from_standard(index) {
                self.process_event(InputEvent::GamepadAxis {
                    axis,
                    value,
                    gamepad_id: device,
                });
            }
        }
    }

    /// Returns `true` if the given key became pressed this frame.
    pub fn is_key_just_pressed(&self, key: Key) -> bool {
        self.keys_just_pressed.contains(&key)
    }

    /// Returns `true` if the given key was released this frame.
    pub fn is_key_just_released(&self, key: Key) -> bool {
        self.keys_just_released.contains(&key)
    }

    /// Returns `true` if the given mouse button became pressed this frame.
    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed.contains(&button)
    }

    /// Returns `true` if the given mouse button was released this frame.
    pub fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released.contains(&button)
    }

    /// Takes a point-in-time snapshot of the current input state.
    pub fn snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            keys_pressed: self.keys_pressed.clone(),
            keys_just_pressed: self.keys_just_pressed.clone(),
            keys_just_released: self.keys_just_released.clone(),
            mouse_pressed: self.mouse_pressed.clone(),
            mouse_just_pressed: self.mouse_just_pressed.clone(),
            mouse_just_released: self.mouse_just_released.clone(),
            mouse_position: self.mouse_position,
            actions_pressed: self.actions_pressed.clone(),
            actions_just_pressed: self.actions_just_pressed.clone(),
            actions_just_released: self.actions_just_released.clone(),
            action_strengths: self.action_strengths.clone(),
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// InputSnapshot
// ---------------------------------------------------------------------------

/// A frozen point-in-time capture of the input state.
///
/// Created via [`InputState::snapshot`]. This is the engine-owned read-only
/// view that game scripts and systems query each frame. It intentionally omits
/// mutation methods so consumers cannot alter the canonical input state.
#[derive(Debug, Clone)]
pub struct InputSnapshot {
    keys_pressed: HashSet<Key>,
    keys_just_pressed: HashSet<Key>,
    keys_just_released: HashSet<Key>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_just_pressed: HashSet<MouseButton>,
    mouse_just_released: HashSet<MouseButton>,
    mouse_position: Vector2,
    actions_pressed: HashSet<String>,
    actions_just_pressed: HashSet<String>,
    actions_just_released: HashSet<String>,
    action_strengths: HashMap<String, f32>,
}

impl InputSnapshot {
    /// Returns `true` if the given key is held down.
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// Returns `true` if the given key became pressed this frame.
    pub fn is_key_just_pressed(&self, key: Key) -> bool {
        self.keys_just_pressed.contains(&key)
    }

    /// Returns `true` if the given key was released this frame.
    pub fn is_key_just_released(&self, key: Key) -> bool {
        self.keys_just_released.contains(&key)
    }

    /// Returns `true` if the given mouse button is held down.
    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.contains(&button)
    }

    /// Returns `true` if the given mouse button became pressed this frame.
    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed.contains(&button)
    }

    /// Returns `true` if the given mouse button was released this frame.
    pub fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released.contains(&button)
    }

    /// Returns the current mouse position.
    pub fn get_mouse_position(&self) -> Vector2 {
        self.mouse_position
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

    /// Returns the strength of an action (0.0–1.0).
    pub fn get_action_strength(&self, name: &str) -> f32 {
        if self.actions_pressed.contains(name) {
            self.action_strengths.get(name).copied().unwrap_or(1.0)
        } else {
            0.0
        }
    }

    /// Returns a value between -1.0 and 1.0 based on two actions.
    pub fn get_axis(&self, negative: &str, positive: &str) -> f32 {
        let neg = self.get_action_strength(negative);
        let pos = self.get_action_strength(positive);
        (pos - neg).clamp(-1.0, 1.0)
    }

    /// Returns a 2D input vector based on four directional actions.
    pub fn get_vector(&self, neg_x: &str, pos_x: &str, neg_y: &str, pos_y: &str) -> Vector2 {
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

    // -- Bridge methods for script-facing InputSnapshot conversion -----------

    /// Returns the string names of all currently pressed keys.
    ///
    /// Used to bridge from the typed `Key` enum to the string-based
    /// `gdscene::scripting::InputSnapshot` that GDScript scripts consume.
    pub fn pressed_key_names(&self) -> Vec<String> {
        self.keys_pressed
            .iter()
            .map(|k| k.name().to_string())
            .collect()
    }

    /// Returns the string names of keys that became pressed this frame.
    pub fn just_pressed_key_names(&self) -> Vec<String> {
        self.keys_just_pressed
            .iter()
            .map(|k| k.name().to_string())
            .collect()
    }

    /// Returns the string names of keys that were released this frame
    /// (pat-nzgdu.9).
    pub fn just_released_key_names(&self) -> Vec<String> {
        self.keys_just_released
            .iter()
            .map(|k| k.name().to_string())
            .collect()
    }

    /// Returns a map of action name → key name strings for all pressed actions.
    ///
    /// This provides a simplified view matching the `input_map` field of
    /// `gdscene::scripting::InputSnapshot`. Only includes actions that are
    /// currently pressed. The key names are derived from the pressed keys,
    /// not from the original InputMap bindings.
    pub fn action_pressed_key_map(&self) -> HashMap<String, Vec<String>> {
        let pressed_names: Vec<String> = self.pressed_key_names();
        let mut map = HashMap::new();
        for action in &self.actions_pressed {
            map.insert(action.clone(), pressed_names.clone());
        }
        map
    }
}

// ---------------------------------------------------------------------------
// InputMap: project.godot loader
// ---------------------------------------------------------------------------

impl InputMap {
    /// Parses input map entries from a Godot `project.godot` file.
    ///
    /// Looks for `[input]` section entries of the form:
    /// ```text
    /// action_name={
    /// "deadzone": 0.5,
    /// "events": [Object(InputEventKey,"resource_local_to_scene":false,...,"keycode":32,...)]
    /// }
    /// ```
    ///
    /// This is a best-effort parser for the subset of bindings Patina supports.
    pub fn load_from_project_godot(content: &str) -> Self {
        let mut map = InputMap::new();
        // Godot injects the built-in ui_* actions first; project [input] entries
        // then override only the actions they declare (see parse_action_block).
        map.install_builtin_ui_actions();
        let mut in_input_section = false;

        // Collect lines belonging to the [input] section.
        let mut current_action: Option<String> = None;
        let mut current_block = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Detect section headers.
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Flush any pending action.
                if let Some(action) = current_action.take() {
                    Self::parse_action_block(&mut map, &action, &current_block);
                    current_block.clear();
                }
                in_input_section = trimmed == "[input]";
                continue;
            }

            if !in_input_section {
                continue;
            }

            // Lines like: move_left={
            if let Some(eq_pos) = trimmed.find('=') {
                // Flush previous action.
                if let Some(action) = current_action.take() {
                    Self::parse_action_block(&mut map, &action, &current_block);
                    current_block.clear();
                }
                let name = trimmed[..eq_pos].trim().trim_matches('"');
                current_action = Some(name.to_string());
                current_block.push_str(&trimmed[eq_pos + 1..]);
            } else if current_action.is_some() {
                current_block.push_str(trimmed);
            }
        }

        // Flush last action.
        if let Some(action) = current_action.take() {
            Self::parse_action_block(&mut map, &action, &current_block);
        }

        map
    }

    /// Parses a single action's value block and populates the map.
    fn parse_action_block(map: &mut InputMap, action: &str, block: &str) {
        // Extract deadzone.
        let deadzone = Self::extract_float(block, "\"deadzone\"")
            .or_else(|| Self::extract_float(block, "deadzone"))
            .unwrap_or(0.5);

        map.add_action(action, deadzone);
        // A declared action replaces any built-in default bindings (Godot
        // override semantics): start it from an empty binding list.
        map.bindings.insert(action.to_string(), Vec::new());

        // Extract key bindings from InputEventKey objects.
        // Look for keycode values.
        let mut search = block.as_bytes();
        while let Some(pos) = find_substr(search, b"Object(InputEventKey") {
            let rest = &search[pos..];
            // Find the end of this Object(...).
            if let Some(end) = find_matching_paren(&rest[6..]).map(|e| e + 6) {
                let obj_str = std::str::from_utf8(&rest[..end + 1]).unwrap_or("");
                if let Some(key) = Self::extract_keycode(obj_str) {
                    map.action_add_event(action, ActionBinding::KeyBinding(key));
                }
                search = &rest[end + 1..];
            } else {
                break;
            }
        }

        // Extract mouse button bindings from InputEventMouseButton objects.
        let mut search = block.as_bytes();
        while let Some(pos) = find_substr(search, b"Object(InputEventMouseButton") {
            let rest = &search[pos..];
            if let Some(end) = find_matching_paren(&rest[6..]).map(|e| e + 6) {
                let obj_str = std::str::from_utf8(&rest[..end + 1]).unwrap_or("");
                if let Some(btn) = Self::extract_mouse_button(obj_str) {
                    map.action_add_event(action, ActionBinding::MouseBinding(btn));
                }
                search = &rest[end + 1..];
            } else {
                break;
            }
        }
    }

    /// Extracts a float value after a given key label.
    fn extract_float(s: &str, key: &str) -> Option<f32> {
        let pos = s.find(key)?;
        let rest = &s[pos + key.len()..];
        // Skip colon and whitespace.
        let rest = rest.trim_start().trim_start_matches(':').trim_start();
        // Parse leading float.
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    }

    /// Extracts a Godot keycode and maps it to our Key enum.
    fn extract_keycode(obj_str: &str) -> Option<Key> {
        // Look for "keycode":VALUE or "physical_keycode":VALUE
        let code = Self::extract_int(obj_str, "\"physical_keycode\"")
            .or_else(|| Self::extract_int(obj_str, "\"keycode\""))?;
        godot_keycode_to_key(code)
    }

    /// Extracts a mouse button_index from an InputEventMouseButton.
    fn extract_mouse_button(obj_str: &str) -> Option<MouseButton> {
        let idx = Self::extract_int(obj_str, "\"button_index\"")?;
        match idx {
            1 => Some(MouseButton::Left),
            2 => Some(MouseButton::Right),
            3 => Some(MouseButton::Middle),
            4 => Some(MouseButton::WheelUp),
            5 => Some(MouseButton::WheelDown),
            _ => None,
        }
    }

    /// Extracts an integer value after a given key label.
    fn extract_int(s: &str, key: &str) -> Option<i64> {
        let pos = s.find(key)?;
        let rest = &s[pos + key.len()..];
        let rest = rest.trim_start().trim_start_matches(':').trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '-')
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    }
}

/// Finds the position of a substring in a byte slice.
fn find_substr(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Finds the closing paren that matches the first `(` after the start.
fn find_matching_paren(s: &[u8]) -> Option<usize> {
    let start = s.iter().position(|&b| b == b'(')?;
    let mut depth = 0;
    for (i, &b) in s[start..].iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Maps a Godot keycode integer to our [`Key`] enum.
fn godot_keycode_to_key(code: i64) -> Option<Key> {
    match code {
        // Letters (Godot: KEY_A=65 .. KEY_Z=90 or 4194305+)
        65 => Some(Key::A),
        66 => Some(Key::B),
        67 => Some(Key::C),
        68 => Some(Key::D),
        69 => Some(Key::E),
        70 => Some(Key::F),
        71 => Some(Key::G),
        72 => Some(Key::H),
        73 => Some(Key::I),
        74 => Some(Key::J),
        75 => Some(Key::K),
        76 => Some(Key::L),
        77 => Some(Key::M),
        78 => Some(Key::N),
        79 => Some(Key::O),
        80 => Some(Key::P),
        81 => Some(Key::Q),
        82 => Some(Key::R),
        83 => Some(Key::S),
        84 => Some(Key::T),
        85 => Some(Key::U),
        86 => Some(Key::V),
        87 => Some(Key::W),
        88 => Some(Key::X),
        89 => Some(Key::Y),
        90 => Some(Key::Z),
        // Digits
        48 => Some(Key::Num0),
        49 => Some(Key::Num1),
        50 => Some(Key::Num2),
        51 => Some(Key::Num3),
        52 => Some(Key::Num4),
        53 => Some(Key::Num5),
        54 => Some(Key::Num6),
        55 => Some(Key::Num7),
        56 => Some(Key::Num8),
        57 => Some(Key::Num9),
        // Special keys
        32 => Some(Key::Space),
        4194309 => Some(Key::Enter),
        4194305 => Some(Key::Escape),
        4194306 => Some(Key::Tab),
        4194325 => Some(Key::Shift),
        4194326 => Some(Key::Ctrl),
        4194328 => Some(Key::Alt),
        4194320 => Some(Key::Up),
        4194322 => Some(Key::Down),
        4194319 => Some(Key::Left),
        4194321 => Some(Key::Right),
        4194332 => Some(Key::F1),
        4194333 => Some(Key::F2),
        4194334 => Some(Key::F3),
        4194335 => Some(Key::F4),
        4194336 => Some(Key::F5),
        4194337 => Some(Key::F6),
        4194338 => Some(Key::F7),
        4194339 => Some(Key::F8),
        4194340 => Some(Key::F9),
        4194341 => Some(Key::F10),
        4194342 => Some(Key::F11),
        4194343 => Some(Key::F12),
        4194308 => Some(Key::Backspace),
        4194312 => Some(Key::Delete),
        4194311 => Some(Key::Insert),
        // Godot's navigation block, from `core/os/keyboard.h` (SPECIAL =
        // 0x400000): HOME = SPECIAL|0x0D = 4194317, END = 0x0E, PAGEUP = 0x13,
        // PAGEDOWN = 0x14, CAPSLOCK = 0x19 (pat-0q2kx.20).
        //
        // These were previously shifted four slots low — 4194313 mapped to Home
        // when it is really PAUSE — so a `project.godot` binding to KEY_HOME
        // silently resolved to CapsLock and every arrow/navigation keycode in
        // this range bound to the wrong key. Values Patina's `Key` enum has no
        // variant for (PAUSE/PRINT/SYSREQ/CLEAR) are left unmapped rather than
        // aliased onto an unrelated key.
        4194317 => Some(Key::Home),
        4194318 => Some(Key::End),
        4194323 => Some(Key::PageUp),
        4194324 => Some(Key::PageDown),
        4194329 => Some(Key::CapsLock),
        44 => Some(Key::Comma),
        46 => Some(Key::Period),
        47 => Some(Key::Slash),
        59 => Some(Key::Semicolon),
        39 => Some(Key::Quote),
        91 => Some(Key::BracketLeft),
        93 => Some(Key::BracketRight),
        92 => Some(Key::Backslash),
        45 => Some(Key::Minus),
        61 => Some(Key::Equal),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// InputMap: JSON loader
// ---------------------------------------------------------------------------

impl InputMap {
    /// Loads an input map from a JSON string.
    ///
    /// Expected format:
    /// ```json
    /// {
    ///   "actions": {
    ///     "move_left": {
    ///       "deadzone": 0.5,
    ///       "keys": ["A", "ArrowLeft"]
    ///     },
    ///     "jump": {
    ///       "keys": ["Space"]
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// Key names use the same strings as [`Key::from_name`].
    /// The `deadzone` field is optional (defaults to 0.0).
    pub fn load_from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

        let actions = value
            .get("actions")
            .and_then(|v| v.as_object())
            .ok_or_else(|| "missing \"actions\" object".to_string())?;

        let mut map = InputMap::new();

        for (action_name, action_def) in actions {
            let deadzone = action_def
                .get("deadzone")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;

            map.add_action(action_name, deadzone);

            if let Some(keys) = action_def.get("keys").and_then(|v| v.as_array()) {
                for key_val in keys {
                    if let Some(key_name) = key_val.as_str() {
                        if let Some(key) = Key::from_name(key_name) {
                            map.action_add_event(action_name, ActionBinding::KeyBinding(key));
                        }
                    }
                }
            }

            if let Some(mouse) = action_def.get("mouse_buttons").and_then(|v| v.as_array()) {
                for btn_val in mouse {
                    if let Some(btn_name) = btn_val.as_str() {
                        let btn = match btn_name {
                            "Left" | "left" => Some(MouseButton::Left),
                            "Right" | "right" => Some(MouseButton::Right),
                            "Middle" | "middle" => Some(MouseButton::Middle),
                            "WheelUp" | "wheel_up" => Some(MouseButton::WheelUp),
                            "WheelDown" | "wheel_down" => Some(MouseButton::WheelDown),
                            _ => None,
                        };
                        if let Some(b) = btn {
                            map.action_add_event(action_name, ActionBinding::MouseBinding(b));
                        }
                    }
                }
            }
        }

        Ok(map)
    }

    /// Loads an input map from a JSON file on disk.
    pub fn load_from_json_file(path: &std::path::Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("failed to read {:?}: {e}", path))?;
        Self::load_from_json(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Godot KEY_* mapping (pat-0q2kx.20) ---------------------------------
    //
    // Ground truth: `upstream/godot/core/os/keyboard.h`, where `SPECIAL =
    // 0x400000` and the navigation block runs INSERT = SPECIAL|0x07, DELETE
    // 0x08, PAUSE 0x09, PRINT 0x0A, SYSREQ 0x0B, CLEAR 0x0C, HOME 0x0D, END
    // 0x0E, arrows 0x0F..0x12, PAGEUP 0x13, PAGEDOWN 0x14, CAPSLOCK 0x19.
    //
    // The table was shifted four slots low through this range: 4194313 (really
    // PAUSE) mapped to Home, and 4194317 — Godot's actual HOME — mapped to
    // CapsLock. A `project.godot` binding of KEY_HOME therefore resolved
    // silently to Caps Lock, and nothing failed, because no test pinned these
    // against the enum they come from.

    /// `SPECIAL` base from `core/os/keyboard.h`; each constant is written as
    /// `SPECIAL | offset` so it diffs against the header by eye.
    const SPECIAL: i64 = 0x40_0000;

    #[test]
    fn navigation_keycodes_match_godot_enum() {
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x0D),
            Some(Key::Home),
            "HOME"
        );
        assert_eq!(godot_keycode_to_key(SPECIAL | 0x0E), Some(Key::End), "END");
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x13),
            Some(Key::PageUp),
            "PAGEUP"
        );
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x14),
            Some(Key::PageDown),
            "PAGEDOWN"
        );
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x19),
            Some(Key::CapsLock),
            "CAPSLOCK"
        );
    }

    #[test]
    fn arrow_and_edit_keycodes_match_godot_enum() {
        // Already correct; pinned so the fix above cannot regress them.
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x0F),
            Some(Key::Left),
            "LEFT"
        );
        assert_eq!(godot_keycode_to_key(SPECIAL | 0x10), Some(Key::Up), "UP");
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x11),
            Some(Key::Right),
            "RIGHT"
        );
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x12),
            Some(Key::Down),
            "DOWN"
        );
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x07),
            Some(Key::Insert),
            "INSERT"
        );
        assert_eq!(
            godot_keycode_to_key(SPECIAL | 0x08),
            Some(Key::Delete),
            "DELETE"
        );
    }

    /// The four codes that previously *stole* the navigation keys are Godot's
    /// PAUSE/PRINT/SYSREQ/CLEAR. Patina's `Key` enum has no variant for them, so
    /// they must resolve to `None` rather than aliasing onto an unrelated key —
    /// a wrong mapping is worse than an absent one, because it binds silently.
    #[test]
    fn unmodelled_special_keys_do_not_alias_onto_other_keys() {
        for (offset, name) in [
            (0x09, "PAUSE"),
            (0x0A, "PRINT"),
            (0x0B, "SYSREQ"),
            (0x0C, "CLEAR"),
        ] {
            assert_eq!(
                godot_keycode_to_key(SPECIAL | offset),
                None,
                "{name} has no Key variant and must not alias onto another key"
            );
        }
    }

    // pat-u3l8c.22: mouse mode, programmatic actions, joypad enumeration and
    // vibration state (the gdplatform foundation for the Input singleton).

    #[test]
    fn mouse_mode_defaults_to_visible_and_can_be_set() {
        let mut input = InputState::new();
        assert_eq!(input.mouse_mode(), MouseMode::Visible);
        input.set_mouse_mode(MouseMode::Captured);
        assert_eq!(input.mouse_mode(), MouseMode::Captured);
    }

    #[test]
    fn mouse_mode_godot_int_mapping() {
        assert_eq!(MouseMode::Visible.to_godot(), 0);
        assert_eq!(MouseMode::Captured.to_godot(), 2);
        assert_eq!(MouseMode::ConfinedHidden.to_godot(), 4);
        assert_eq!(MouseMode::from_godot(2), Some(MouseMode::Captured));
        assert_eq!(MouseMode::from_godot(4), Some(MouseMode::ConfinedHidden));
        assert_eq!(MouseMode::from_godot(99), None);
    }

    #[test]
    fn action_press_and_release_update_action_state() {
        let mut input = InputState::new();
        assert!(!input.is_action_pressed("jump"));

        input.action_press("jump", 1.0);
        assert!(input.is_action_pressed("jump"));
        assert!(input.is_action_just_pressed("jump"));

        // Edge state clears next frame; held state persists.
        input.flush_frame();
        assert!(input.is_action_pressed("jump"));
        assert!(!input.is_action_just_pressed("jump"));

        input.action_release("jump");
        assert!(!input.is_action_pressed("jump"));
        assert!(input.is_action_just_released("jump"));
    }

    #[test]
    fn joypad_connect_enumerate_and_disconnect() {
        let mut input = InputState::new();
        assert!(input.connected_joypads().is_empty());

        input.connect_joypad(2);
        input.connect_joypad(0);
        // Enumerated in ascending order regardless of connect order.
        assert_eq!(input.connected_joypads(), vec![0, 2]);

        input.disconnect_joypad(0);
        assert_eq!(input.connected_joypads(), vec![2]);
    }

    #[test]
    fn joy_vibration_start_stop_and_cleared_on_disconnect() {
        let mut input = InputState::new();
        assert_eq!(input.joy_vibration(0), None);

        input.start_joy_vibration(0, 0.5, 0.9, 1.0);
        assert_eq!(input.joy_vibration(0), Some((0.5, 0.9, 1.0)));

        input.stop_joy_vibration(0);
        assert_eq!(input.joy_vibration(0), None);

        // Disconnecting a device clears any active vibration too.
        input.connect_joypad(1);
        input.start_joy_vibration(1, 0.2, 0.2, 0.0);
        input.disconnect_joypad(1);
        assert_eq!(input.joy_vibration(1), None);
    }

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
        assert_ne!(
            GamepadAxis::LeftTriggerAnalog,
            GamepadAxis::RightTriggerAnalog
        );
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
        assert!(state
            .gamepad_buttons_just_pressed
            .contains(&(0, GamepadButton::Start)));

        state.flush_frame();
        assert!(!state
            .gamepad_buttons_just_pressed
            .contains(&(0, GamepadButton::Start)));
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
        assert_eq!(
            state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX),
            0.8
        );
        assert_eq!(
            state.get_gamepad_axis_value(0, GamepadAxis::LeftStickY),
            0.0
        );
    }

    // pat-nzgdu.48: browser "standard mapping" gamepad normalization. A poll of
    // navigator.getGamepads() (button 0 = A down, axis 0 = left-stick-X) fed
    // through apply_standard_gamepad_frame reaches the joy query API and lists
    // the device connected — the InputState half of the gamepad_input_e2e
    // acceptance (the HTTP /api/runtime/input/gamepad endpoint + the interpreter
    // Input.is_joy_* dispatch are a separate, cross-cutting lane).
    #[test]
    fn gamepad_standard_mapping_frame() {
        // W3C standard button/axis indices map to the right semantic enums.
        assert_eq!(GamepadButton::from_standard(0), Some(GamepadButton::FaceA));
        assert_eq!(GamepadButton::from_standard(1), Some(GamepadButton::FaceB));
        assert_eq!(GamepadButton::from_standard(12), Some(GamepadButton::DPadUp));
        assert_eq!(GamepadButton::from_standard(15), Some(GamepadButton::DPadRight));
        assert_eq!(GamepadButton::from_standard(16), Some(GamepadButton::Guide));
        assert_eq!(GamepadButton::from_standard(17), None);
        assert_eq!(GamepadAxis::from_standard(0), Some(GamepadAxis::LeftStickX));
        assert_eq!(GamepadAxis::from_standard(3), Some(GamepadAxis::RightStickY));
        assert_eq!(GamepadAxis::from_standard(4), None);

        // Simulate one getGamepads() frame for device 0: face-button A down,
        // left-stick-X = 0.8.
        let mut state = InputState::new();
        let mut buttons = [false; 17];
        buttons[0] = true; // JOY_BUTTON_A
        let axes = [0.8f32, 0.0, 0.0, 0.0]; // JOY_AXIS_LEFT_X = 0.8
        state.apply_standard_gamepad_frame(0, &buttons, &axes);

        assert!(
            state.is_gamepad_button_pressed(0, GamepadButton::FaceA),
            "A down must reach is_joy_button_pressed"
        );
        assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
        assert!(
            (state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.8).abs() < 1e-6,
            "left-stick-X must reach get_joy_axis"
        );
        assert_eq!(
            state.connected_joypads(),
            vec![0],
            "device 0 must be listed by get_connected_joypads"
        );

        // A second frame releasing the button clears the pressed state.
        let buttons_up = [false; 17];
        state.apply_standard_gamepad_frame(0, &buttons_up, &axes);
        assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    }

    #[test]
    fn input_map_gamepad_button_binding() {
        let mut map = InputMap::new();
        map.add_action("jump", 0.0);
        map.action_add_event(
            "jump",
            ActionBinding::GamepadButtonBinding(GamepadButton::FaceA),
        );

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
        map.action_add_event(
            "move_right",
            ActionBinding::GamepadAxisBinding(GamepadAxis::LeftStickX),
        );

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
        map.action_add_event(
            "fire",
            ActionBinding::GamepadButtonBinding(GamepadButton::RightTrigger),
        );

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
        assert!(
            (len - 1.0).abs() < 0.001,
            "diagonal vector should be normalized, got len={len}"
        );
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

    /// pat-nzgdu.9 — circular deadzone: a non-zero deadzone still yields a
    /// normalized diagonal for digital (full-strength) inputs.
    #[test]
    fn get_vector_circular_deadzone_normalizes_digital_diagonal() {
        let mut map = InputMap::new();
        for a in ["left", "right", "up", "down"] {
            map.add_action(a, 0.5); // non-zero deadzone
        }
        map.action_add_event("right", ActionBinding::KeyBinding(Key::D));
        map.action_add_event("down", ActionBinding::KeyBinding(Key::S));
        let mut state = InputState::new();
        state.set_input_map(map);
        for key in [Key::D, Key::S] {
            state.process_event(InputEvent::Key {
                key,
                pressed: true,
                shift: false,
                ctrl: false,
                alt: false,
            });
        }
        let v = state.get_vector("left", "right", "up", "down");
        let len = (v.x * v.x + v.y * v.y).sqrt();
        assert!((len - 1.0).abs() < 0.001, "diagonal normalized, got len={len}");
        assert!(v.x > 0.0 && v.y > 0.0);
    }

    /// pat-nzgdu.9 — circular deadzone rescales analog magnitude in the
    /// (deadzone, 1] band and zeros anything at or below the deadzone.
    #[test]
    fn get_vector_circular_deadzone_analog_rescale_and_zero() {
        let mut map = InputMap::new();
        for a in ["left", "right", "up", "down"] {
            map.add_action(a, 0.5);
        }
        let mut state = InputState::new();
        state.set_input_map(map);

        // Above deadzone: raw x=0.6, dz=0.5 → 0.6*((0.6-0.5)/(1-0.5))/0.6 = 0.2.
        state.process_event(InputEvent::Action {
            action: "right".to_string(),
            pressed: true,
            strength: 0.6,
        });
        let v = state.get_vector("left", "right", "up", "down");
        assert!(
            (v.x - 0.2).abs() < 0.02,
            "0.6 magnitude rescales to ~0.2 past a 0.5 deadzone, got {}",
            v.x
        );
        assert_eq!(v.y, 0.0);

        // At/below deadzone: raw x=0.3 <= dz 0.5 → zero vector.
        state.process_event(InputEvent::Action {
            action: "right".to_string(),
            pressed: true,
            strength: 0.3,
        });
        let v = state.get_vector("left", "right", "up", "down");
        assert_eq!(v.x, 0.0, "below-deadzone magnitude reads as zero");
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

    // -----------------------------------------------------------------------
    // pat-isw: InputSnapshot API tests
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_captures_key_state() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::W,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        let snap = state.snapshot();
        assert!(snap.is_key_pressed(Key::W));
        assert!(snap.is_key_just_pressed(Key::W));
        assert!(!snap.is_key_pressed(Key::S));
    }

    #[test]
    fn snapshot_captures_mouse_state() {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseMotion {
            position: Vector2::new(100.0, 200.0),
            relative: Vector2::new(1.0, 2.0),
        });
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(100.0, 200.0),
        });
        let snap = state.snapshot();
        assert!(snap.is_mouse_button_pressed(MouseButton::Left));
        assert!(snap.is_mouse_button_just_pressed(MouseButton::Left));
        assert!(!snap.is_mouse_button_pressed(MouseButton::Right));
        assert_eq!(snap.get_mouse_position(), Vector2::new(100.0, 200.0));
    }

    #[test]
    fn snapshot_captures_action_state() {
        let mut map = InputMap::new();
        map.add_action("jump", 0.0);
        map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));

        let mut state = InputState::new();
        state.set_input_map(map);
        state.process_event(InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let snap = state.snapshot();
        assert!(snap.is_action_pressed("jump"));
        assert!(snap.is_action_just_pressed("jump"));
        assert!(!snap.is_action_just_released("jump"));
        assert_eq!(snap.get_action_strength("jump"), 1.0);
    }

    #[test]
    fn snapshot_is_frozen_after_further_events() {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        let snap = state.snapshot();

        // Further mutation on InputState does not affect snapshot.
        state.process_event(InputEvent::Key {
            key: Key::A,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(snap.is_key_pressed(Key::A), "snapshot should remain frozen");
        assert!(
            !state.is_key_pressed(Key::A),
            "state should reflect release"
        );
    }

    #[test]
    fn snapshot_get_axis_and_vector() {
        let mut map = InputMap::new();
        map.add_action("left", 0.0);
        map.add_action("right", 0.0);
        map.add_action("up", 0.0);
        map.add_action("down", 0.0);
        map.action_add_event("right", ActionBinding::KeyBinding(Key::D));
        map.action_add_event("down", ActionBinding::KeyBinding(Key::S));

        let mut state = InputState::new();
        state.set_input_map(map);
        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let snap = state.snapshot();
        assert_eq!(snap.get_axis("left", "right"), 1.0);
        let v = snap.get_vector("left", "right", "up", "down");
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 0.0);
    }

    // -----------------------------------------------------------------------
    // pat-g9k: Keyboard action snapshot tests
    // -----------------------------------------------------------------------

    #[test]
    fn keyboard_action_press_just_pressed_per_frame() {
        let mut map = InputMap::new();
        map.add_action("move_right", 0.0);
        map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));

        let mut state = InputState::new();
        state.set_input_map(map);

        // Frame 1: press D
        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        let snap1 = state.snapshot();
        assert!(snap1.is_action_pressed("move_right"));
        assert!(snap1.is_action_just_pressed("move_right"));
        assert!(!snap1.is_action_just_released("move_right"));

        // Frame 2: still held, no just_pressed
        state.flush_frame();
        let snap2 = state.snapshot();
        assert!(snap2.is_action_pressed("move_right"));
        assert!(!snap2.is_action_just_pressed("move_right"));

        // Frame 3: release
        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        let snap3 = state.snapshot();
        assert!(!snap3.is_action_pressed("move_right"));
        assert!(snap3.is_action_just_released("move_right"));

        // Frame 4: nothing
        state.flush_frame();
        let snap4 = state.snapshot();
        assert!(!snap4.is_action_pressed("move_right"));
        assert!(!snap4.is_action_just_released("move_right"));
    }

    #[test]
    fn keyboard_action_multiple_keys_same_action() {
        let mut map = InputMap::new();
        map.add_action("jump", 0.0);
        map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
        map.action_add_event("jump", ActionBinding::KeyBinding(Key::W));

        let mut state = InputState::new();
        state.set_input_map(map);

        // Press Space → action active.
        state.process_event(InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.snapshot().is_action_pressed("jump"));

        // Release Space → action released (even though W could also trigger it).
        state.flush_frame();
        state.process_event(InputEvent::Key {
            key: Key::Space,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(!state.snapshot().is_action_pressed("jump"));

        // Press W → action active again.
        state.flush_frame();
        state.process_event(InputEvent::Key {
            key: Key::W,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.snapshot().is_action_pressed("jump"));
    }

    #[test]
    fn keyboard_action_strength_is_one_for_digital() {
        let mut map = InputMap::new();
        map.add_action("fire", 0.0);
        map.action_add_event("fire", ActionBinding::KeyBinding(Key::Enter));

        let mut state = InputState::new();
        state.set_input_map(map);
        state.process_event(InputEvent::Key {
            key: Key::Enter,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let snap = state.snapshot();
        assert_eq!(snap.get_action_strength("fire"), 1.0);
    }

    #[test]
    fn keyboard_just_pressed_and_released_public_api() {
        let mut state = InputState::new();

        state.process_event(InputEvent::Key {
            key: Key::Escape,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.is_key_just_pressed(Key::Escape));
        assert!(!state.is_key_just_released(Key::Escape));

        state.flush_frame();
        assert!(!state.is_key_just_pressed(Key::Escape));

        state.process_event(InputEvent::Key {
            key: Key::Escape,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.is_key_just_released(Key::Escape));
    }

    // -----------------------------------------------------------------------
    // pat-vih: Input map loading from project.godot
    // -----------------------------------------------------------------------

    // pat-nzgdu.47: a project.godot with no [input] section still boots with
    // Godot's built-in ui_* actions; a project that declares its own ui_accept
    // overrides only that action while the rest keep their defaults.
    #[test]
    fn default_ui_actions_e2e() {
        let tab = InputEvent::Key {
            key: Key::Tab,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let enter = InputEvent::Key {
            key: Key::Enter,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let esc = InputEvent::Key {
            key: Key::Escape,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };

        // (1) No [input] section -> the built-in ui_* actions are present.
        let no_input = "[application]\nconfig/name=\"Demo\"\n";
        let map = InputMap::load_from_project_godot(no_input);
        assert!(
            map.event_matches_action(&tab, "ui_focus_next"),
            "Tab must trigger ui_focus_next by default"
        );
        assert!(
            map.event_matches_action(&enter, "ui_accept"),
            "Enter must trigger ui_accept by default"
        );

        // Through the InputState layer: pressing Tab makes ui_focus_next pressed.
        let mut input = InputState::new();
        input.set_input_map(map);
        input.process_event(tab.clone());
        assert!(
            input.is_action_pressed("ui_focus_next"),
            "is_action_pressed(ui_focus_next) must be true after pressing Tab"
        );

        // (2) A project that declares its own ui_accept overrides ONLY that
        // action; ui_cancel / ui_focus_next keep their built-in defaults.
        let with_accept = r#"
[input]
ui_accept={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"device":-1,"keycode":0,"physical_keycode":65,"unicode":97)]
}
"#;
        let map2 = InputMap::load_from_project_godot(with_accept);
        let key_a = InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(
            map2.event_matches_action(&key_a, "ui_accept"),
            "declared ui_accept must match its own key (A)"
        );
        assert!(
            !map2.event_matches_action(&enter, "ui_accept"),
            "override must REPLACE the default Enter binding, not append to it"
        );
        assert!(
            map2.event_matches_action(&esc, "ui_cancel"),
            "undeclared ui_cancel must keep its default Escape binding"
        );
        assert!(
            map2.event_matches_action(&tab, "ui_focus_next"),
            "undeclared ui_focus_next must keep its default Tab binding"
        );
    }

    #[test]
    fn load_input_map_from_project_godot_basic() {
        let content = r#"
[input]
move_left={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":0,"physical_keycode":65,"key_label":0,"unicode":97)]
}
move_right={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":0,"physical_keycode":68,"key_label":0,"unicode":100)]
}
"#;
        let map = InputMap::load_from_project_godot(content);
        assert!(map.get_bindings("move_left").is_some());
        assert!(map.get_bindings("move_right").is_some());
        assert_eq!(map.get_deadzone("move_left"), 0.5);

        // Verify the key binding resolves correctly.
        let evt_a = InputEvent::Key {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(map.event_matches_action(&evt_a, "move_left"));

        let evt_d = InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(map.event_matches_action(&evt_d, "move_right"));
    }

    #[test]
    fn load_input_map_with_space_key() {
        let content = r#"
[input]
jump={
"deadzone": 0.2,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"keycode":32,"physical_keycode":32)]
}
"#;
        let map = InputMap::load_from_project_godot(content);
        let evt = InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(map.event_matches_action(&evt, "jump"));
        assert_eq!(map.get_deadzone("jump"), 0.2);
    }

    #[test]
    fn load_input_map_multiple_bindings_per_action() {
        let content = r#"
[input]
move_left={
"deadzone": 0.5,
"events": [Object(InputEventKey,"physical_keycode":65), Object(InputEventKey,"physical_keycode":4194319)]
}
"#;
        let map = InputMap::load_from_project_godot(content);
        let bindings = map.get_bindings("move_left").unwrap();
        assert_eq!(bindings.len(), 2);

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
    fn load_input_map_mouse_button_binding() {
        let content = r#"
[input]
shoot={
"deadzone": 0.0,
"events": [Object(InputEventMouseButton,"resource_local_to_scene":false,"button_index":1)]
}
"#;
        let map = InputMap::load_from_project_godot(content);
        let bindings = map.get_bindings("shoot").unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0], ActionBinding::MouseBinding(MouseButton::Left));
    }

    #[test]
    fn load_input_map_ignores_other_sections() {
        let content = r#"
[application]
config/name="MyGame"

[input]
jump={
"deadzone": 0.5,
"events": [Object(InputEventKey,"physical_keycode":32)]
}

[rendering]
quality/filters/msaa=2
"#;
        let map = InputMap::load_from_project_godot(content);
        assert!(map.get_bindings("jump").is_some());
        // Should only have the one action.
        assert_eq!(map.actions().count(), 1);
    }

    #[test]
    fn load_input_map_integrates_with_input_state() {
        let content = r#"
[input]
ui_accept={
"deadzone": 0.5,
"events": [Object(InputEventKey,"physical_keycode":4194309)]
}
"#;
        let map = InputMap::load_from_project_godot(content);
        let mut state = InputState::new();
        state.set_input_map(map);

        state.process_event(InputEvent::Key {
            key: Key::Enter,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        let snap = state.snapshot();
        assert!(snap.is_action_pressed("ui_accept"));
        assert!(snap.is_action_just_pressed("ui_accept"));
    }

    // -----------------------------------------------------------------------
    // pat-aro: Mouse position and button routing tests
    // -----------------------------------------------------------------------

    #[test]
    fn mouse_position_updates_from_motion_events() {
        let mut state = InputState::new();
        assert_eq!(state.get_mouse_position(), Vector2::ZERO);

        state.process_event(InputEvent::MouseMotion {
            position: Vector2::new(50.0, 75.0),
            relative: Vector2::new(50.0, 75.0),
        });
        let snap = state.snapshot();
        assert_eq!(snap.get_mouse_position(), Vector2::new(50.0, 75.0));

        state.process_event(InputEvent::MouseMotion {
            position: Vector2::new(200.0, 300.0),
            relative: Vector2::new(150.0, 225.0),
        });
        let snap2 = state.snapshot();
        assert_eq!(snap2.get_mouse_position(), Vector2::new(200.0, 300.0));
        // Original snapshot remains frozen.
        assert_eq!(snap.get_mouse_position(), Vector2::new(50.0, 75.0));
    }

    #[test]
    fn mouse_button_press_release_through_snapshot() {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Right,
            pressed: true,
            position: Vector2::new(10.0, 20.0),
        });

        let snap = state.snapshot();
        assert!(snap.is_mouse_button_pressed(MouseButton::Right));
        assert!(snap.is_mouse_button_just_pressed(MouseButton::Right));
        assert!(!snap.is_mouse_button_pressed(MouseButton::Left));
        assert_eq!(snap.get_mouse_position(), Vector2::new(10.0, 20.0));

        state.flush_frame();
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Right,
            pressed: false,
            position: Vector2::new(10.0, 20.0),
        });
        let snap2 = state.snapshot();
        assert!(!snap2.is_mouse_button_pressed(MouseButton::Right));
        assert!(snap2.is_mouse_button_just_released(MouseButton::Right));
    }

    #[test]
    fn mouse_button_updates_position() {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(400.0, 300.0),
        });
        assert_eq!(state.get_mouse_position(), Vector2::new(400.0, 300.0));
    }

    #[test]
    fn mouse_action_routing_through_input_map() {
        let mut map = InputMap::new();
        map.add_action("primary_fire", 0.0);
        map.action_add_event(
            "primary_fire",
            ActionBinding::MouseBinding(MouseButton::Left),
        );
        map.add_action("secondary_fire", 0.0);
        map.action_add_event(
            "secondary_fire",
            ActionBinding::MouseBinding(MouseButton::Right),
        );

        let mut state = InputState::new();
        state.set_input_map(map);

        // Click left at position (100, 200).
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(100.0, 200.0),
        });

        let snap = state.snapshot();
        assert!(snap.is_action_pressed("primary_fire"));
        assert!(!snap.is_action_pressed("secondary_fire"));
        assert_eq!(snap.get_mouse_position(), Vector2::new(100.0, 200.0));

        // Click right.
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Right,
            pressed: true,
            position: Vector2::new(150.0, 250.0),
        });
        let snap2 = state.snapshot();
        assert!(snap2.is_action_pressed("primary_fire"));
        assert!(snap2.is_action_pressed("secondary_fire"));
        assert_eq!(snap2.get_mouse_position(), Vector2::new(150.0, 250.0));
    }

    #[test]
    fn mouse_just_pressed_clears_on_flush() {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Middle,
            pressed: true,
            position: Vector2::ZERO,
        });
        assert!(state.is_mouse_button_just_pressed(MouseButton::Middle));

        state.flush_frame();
        assert!(!state.is_mouse_button_just_pressed(MouseButton::Middle));
        assert!(state.is_mouse_button_pressed(MouseButton::Middle));
    }

    // -----------------------------------------------------------------------
    // pat-vih: JSON input map loading
    // -----------------------------------------------------------------------

    #[test]
    fn load_json_basic_actions() {
        let json = r#"{
            "actions": {
                "jump": { "keys": ["Space"] },
                "move_left": { "keys": ["A"] }
            }
        }"#;
        let map = InputMap::load_from_json(json).unwrap();
        assert!(map.get_bindings("jump").is_some());
        assert!(map.get_bindings("move_left").is_some());
        assert_eq!(map.get_bindings("jump").unwrap().len(), 1);
    }

    #[test]
    fn load_json_multiple_keys_per_action() {
        let json = r#"{
            "actions": {
                "move_left": { "keys": ["A", "ArrowLeft"] }
            }
        }"#;
        let map = InputMap::load_from_json(json).unwrap();
        let bindings = map.get_bindings("move_left").unwrap();
        assert_eq!(bindings.len(), 2);

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
    fn load_json_deadzone() {
        let json = r#"{
            "actions": {
                "dash": { "deadzone": 0.3, "keys": ["Shift"] }
            }
        }"#;
        let map = InputMap::load_from_json(json).unwrap();
        assert!((map.get_deadzone("dash") - 0.3).abs() < 1e-6);
    }

    #[test]
    fn load_json_deadzone_defaults_to_zero() {
        let json = r#"{ "actions": { "jump": { "keys": ["Space"] } } }"#;
        let map = InputMap::load_from_json(json).unwrap();
        assert_eq!(map.get_deadzone("jump"), 0.0);
    }

    #[test]
    fn load_json_mouse_button_binding() {
        let json = r#"{
            "actions": {
                "shoot": { "keys": ["Enter"], "mouse_buttons": ["Left"] }
            }
        }"#;
        let map = InputMap::load_from_json(json).unwrap();
        let bindings = map.get_bindings("shoot").unwrap();
        assert_eq!(bindings.len(), 2); // Enter + Left mouse

        let evt = InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::ZERO,
        };
        assert!(map.event_matches_action(&evt, "shoot"));
    }

    #[test]
    fn load_json_overrides_default_map() {
        // Start with a default map that has "jump" → W
        let mut default_map = InputMap::new();
        default_map.add_action("jump", 0.0);
        default_map.action_add_event("jump", ActionBinding::KeyBinding(Key::W));

        // Load a JSON map with "jump" → Space
        let json = r#"{ "actions": { "jump": { "keys": ["Space"] } } }"#;
        let custom_map = InputMap::load_from_json(json).unwrap();

        // Custom map should NOT match W, should match Space
        let evt_w = InputEvent::Key {
            key: Key::W,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let evt_space = InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(!custom_map.event_matches_action(&evt_w, "jump"));
        assert!(custom_map.event_matches_action(&evt_space, "jump"));

        // Original default still works with W
        assert!(default_map.event_matches_action(&evt_w, "jump"));
    }

    #[test]
    fn load_json_integrates_with_input_state() {
        let json = r#"{
            "actions": {
                "fire": { "keys": ["Enter"] },
                "move_right": { "keys": ["D"] }
            }
        }"#;
        let map = InputMap::load_from_json(json).unwrap();

        let mut state = InputState::new();
        state.set_input_map(map);

        state.process_event(InputEvent::Key {
            key: Key::D,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.is_action_pressed("move_right"));
        assert!(!state.is_action_pressed("fire"));

        state.process_event(InputEvent::Key {
            key: Key::Enter,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert!(state.is_action_pressed("fire"));
    }

    #[test]
    fn load_json_invalid_returns_error() {
        assert!(InputMap::load_from_json("not json").is_err());
        assert!(InputMap::load_from_json("{}").is_err()); // missing "actions"
        assert!(InputMap::load_from_json(r#"{"actions": "bad"}"#).is_err());
    }

    #[test]
    fn load_json_file_from_disk() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/input_map.json");
        let map = InputMap::load_from_json_file(&path).unwrap();

        // Verify actions from the fixture
        assert!(map.get_bindings("move_left").is_some());
        assert!(map.get_bindings("move_right").is_some());
        assert!(map.get_bindings("jump").is_some());
        assert!(map.get_bindings("shoot").is_some());
        assert!(map.get_bindings("dash").is_some());
        assert!(map.get_bindings("pause").is_some());

        // move_left has 2 keys (A, ArrowLeft)
        assert_eq!(map.get_bindings("move_left").unwrap().len(), 2);

        // shoot has key + mouse = 2 bindings
        assert_eq!(map.get_bindings("shoot").unwrap().len(), 2);

        // dash has deadzone 0.2
        assert!((map.get_deadzone("dash") - 0.2).abs() < 1e-6);

        // Verify action resolution
        let evt = InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(map.event_matches_action(&evt, "jump"));
    }
}

/// Context for drag-and-drop operations, tracking accepted file paths
/// and the current drop position.
#[derive(Debug, Clone, Default)]
pub struct DropContext {
    /// File paths that have been dropped.
    pub files: Vec<String>,
    /// Drop position in window coordinates.
    pub position: (f32, f32),
    /// Whether a drop operation is currently active.
    pub active: bool,
}

impl DropContext {
    /// Creates a new empty drop context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the drop context state.
    pub fn clear(&mut self) {
        self.files.clear();
        self.position = (0.0, 0.0);
        self.active = false;
    }

    /// Signals that a drag operation has entered the window.
    pub fn drag_enter(&mut self) {
        self.active = true;
    }

    /// Accepts dropped files, ending the drag hover state.
    pub fn drop_files(&mut self, files: Vec<String>) {
        self.files.extend(files);
        self.active = false;
    }

    /// Returns `true` if a drag is currently hovering over the window.
    pub fn is_hovering(&self) -> bool {
        self.active
    }

    /// Returns `true` if there are pending files that haven't been consumed.
    pub fn has_pending_files(&self) -> bool {
        !self.files.is_empty()
    }

    /// Returns the number of pending dropped files.
    pub fn pending_count(&self) -> usize {
        self.files.len()
    }

    /// Takes all pending files, clearing the internal list.
    pub fn take_pending_files(&mut self) -> Vec<String> {
        std::mem::take(&mut self.files)
    }
}
