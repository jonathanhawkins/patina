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
    /// The backquote/grave key (Godot `Key::QUOTELEFT`) — distinct from
    /// [`Key::Quote`], which is the apostrophe. `ui_swap_input_direction` binds
    /// Ctrl+Backquote (pat-0q2kx.125).
    QuoteLeft,
    /// The context-menu key (Godot `Key::MENU`), bound by `ui_menu`.
    Menu,
}

impl Key {
    /// The Godot `KEY_*` keycode for this key (pat-s3rrv.14).
    ///
    /// Inverse of [`godot_keycode_to_key`], and generated FROM that table so the
    /// two cannot drift: `InputEventKey.keycode` is an int in Godot
    /// (`KEY_W == 87`), and every ported project compares against the `KEY_*`
    /// constants. Patina used to hand scripts the key's *name* string ("W"),
    /// which no real Godot code ever compares against.
    ///
    /// Godot's non-printable keys live above `1 << 22` (`KEY_ESCAPE` =
    /// 4194305), which is why the codes below are not contiguous.
    pub fn to_godot_keycode(self) -> i64 {
        match self {
            Key::Space => 32,
            Key::Quote => 39,
            Key::Comma => 44,
            Key::Minus => 45,
            Key::Period => 46,
            Key::Slash => 47,
            Key::Num0 => 48,
            Key::Num1 => 49,
            Key::Num2 => 50,
            Key::Num3 => 51,
            Key::Num4 => 52,
            Key::Num5 => 53,
            Key::Num6 => 54,
            Key::Num7 => 55,
            Key::Num8 => 56,
            Key::Num9 => 57,
            Key::Semicolon => 59,
            Key::Equal => 61,
            Key::A => 65,
            Key::B => 66,
            Key::C => 67,
            Key::D => 68,
            Key::E => 69,
            Key::F => 70,
            Key::G => 71,
            Key::H => 72,
            Key::I => 73,
            Key::J => 74,
            Key::K => 75,
            Key::L => 76,
            Key::M => 77,
            Key::N => 78,
            Key::O => 79,
            Key::P => 80,
            Key::Q => 81,
            Key::R => 82,
            Key::S => 83,
            Key::T => 84,
            Key::U => 85,
            Key::V => 86,
            Key::W => 87,
            Key::X => 88,
            Key::Y => 89,
            Key::Z => 90,
            Key::BracketLeft => 91,
            Key::Backslash => 92,
            Key::BracketRight => 93,
            Key::QuoteLeft => 96,
            Key::Escape => 4194305,
            Key::Tab => 4194306,
            Key::Backspace => 4194308,
            Key::Enter => 4194309,
            Key::Insert => 4194311,
            Key::Delete => 4194312,
            Key::Home => 4194317,
            Key::End => 4194318,
            Key::Left => 4194319,
            Key::Up => 4194320,
            Key::Right => 4194321,
            Key::Down => 4194322,
            Key::PageUp => 4194323,
            Key::PageDown => 4194324,
            Key::Shift => 4194325,
            Key::Ctrl => 4194326,
            Key::Alt => 4194328,
            Key::CapsLock => 4194329,
            Key::F1 => 4194332,
            Key::F2 => 4194333,
            Key::F3 => 4194334,
            Key::F4 => 4194335,
            Key::F5 => 4194336,
            Key::F6 => 4194337,
            Key::F7 => 4194338,
            Key::F8 => 4194339,
            Key::F9 => 4194340,
            Key::F10 => 4194341,
            Key::F11 => 4194342,
            Key::F12 => 4194343,
            Key::Menu => 4194370,
        }
    }

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
            Key::QuoteLeft => "`",
            Key::Menu => "Menu",
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
            // The browser reports these by `KeyboardEvent.code`/`key`; both
            // spellings are accepted so a routed web event resolves either way
            // (pat-0q2kx.125).
            "`" | "Backquote" => Some(Key::QuoteLeft),
            "Menu" | "ContextMenu" => Some(Key::Menu),
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
    /// Horizontal wheel tilt left (`MOUSE_BUTTON_WHEEL_LEFT`, 6).
    WheelLeft,
    /// Horizontal wheel tilt right (`MOUSE_BUTTON_WHEEL_RIGHT`, 7).
    WheelRight,
    /// The "back" thumb button (`MOUSE_BUTTON_XBUTTON1`, 8).
    XButton1,
    /// The "forward" thumb button (`MOUSE_BUTTON_XBUTTON2`, 9).
    XButton2,
}

impl MouseButton {
    /// The Godot `MOUSE_BUTTON_*` index for this button (pat-s3rrv.14).
    ///
    /// Godot numbers them from 1 (`MOUSE_BUTTON_LEFT` = 1, RIGHT = 2, MIDDLE =
    /// 3, WHEEL_UP = 4, WHEEL_DOWN = 5) and exposes the value as
    /// `InputEventMouseButton.button_index`. Patina used to hand scripts the
    /// Rust debug string ("Left"), which no Godot code compares against.
    pub fn to_godot_index(self) -> i64 {
        match self {
            MouseButton::Left => 1,
            MouseButton::Right => 2,
            MouseButton::Middle => 3,
            MouseButton::WheelUp => 4,
            MouseButton::WheelDown => 5,
            MouseButton::WheelLeft => 6,
            MouseButton::WheelRight => 7,
            MouseButton::XButton1 => 8,
            MouseButton::XButton2 => 9,
        }
    }

    /// The `button_mask` bit this button contributes
    /// (`MOUSE_BUTTON_MASK_LEFT` = 1 << 0, etc.), as carried by
    /// `InputEventMouse.button_mask`.
    pub fn to_godot_mask(self) -> i64 {
        1 << (self.to_godot_index() - 1)
    }
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
    /// The Godot `JoyButton` integer for this button — the value a script
    /// passes to `Input.is_joy_button_pressed()` as `JOY_BUTTON_*`
    /// (upstream/godot/core/input/input_enums.h `enum class JoyButton`).
    ///
    /// This is **not** the W3C standard-mapping index: the browser reports the
    /// shoulder buttons at 4/5 and the D-pad at 12..15, while Godot numbers
    /// BACK=4, GUIDE=5, START=6, the sticks 7/8, the shoulders 9/10 and the
    /// D-pad 11..14. Confusing the two silently binds the wrong button, so the
    /// conversion lives here rather than at each call site (pat-0q2kx.88).
    ///
    /// `LeftTrigger`/`RightTrigger` have no `JoyButton` at all in Godot — they
    /// are the analog axes `JOY_AXIS_TRIGGER_LEFT`/`_RIGHT` — so they map to
    /// `None`.
    pub fn to_godot_index(self) -> Option<i64> {
        Some(match self {
            GamepadButton::FaceA => 0,
            GamepadButton::FaceB => 1,
            GamepadButton::FaceX => 2,
            GamepadButton::FaceY => 3,
            GamepadButton::Select => 4, // BACK
            GamepadButton::Guide => 5,
            GamepadButton::Start => 6,
            GamepadButton::LeftStick => 7,
            GamepadButton::RightStick => 8,
            GamepadButton::LeftShoulder => 9,
            GamepadButton::RightShoulder => 10,
            GamepadButton::DPadUp => 11,
            GamepadButton::DPadDown => 12,
            GamepadButton::DPadLeft => 13,
            GamepadButton::DPadRight => 14,
            // Analog triggers are axes in Godot, not buttons.
            GamepadButton::LeftTrigger | GamepadButton::RightTrigger => return None,
        })
    }

    /// Inverse of [`to_godot_index`](Self::to_godot_index).
    pub fn from_godot_index(index: i64) -> Option<GamepadButton> {
        Some(match index {
            0 => GamepadButton::FaceA,
            1 => GamepadButton::FaceB,
            2 => GamepadButton::FaceX,
            3 => GamepadButton::FaceY,
            4 => GamepadButton::Select,
            5 => GamepadButton::Guide,
            6 => GamepadButton::Start,
            7 => GamepadButton::LeftStick,
            8 => GamepadButton::RightStick,
            9 => GamepadButton::LeftShoulder,
            10 => GamepadButton::RightShoulder,
            11 => GamepadButton::DPadUp,
            12 => GamepadButton::DPadDown,
            13 => GamepadButton::DPadLeft,
            14 => GamepadButton::DPadRight,
            _ => return None,
        })
    }

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
    /// The Godot `JoyAxis` integer for this axis — what a script passes to
    /// `Input.get_joy_axis()` as `JOY_AXIS_*`
    /// (upstream/godot/core/input/input_enums.h `enum class JoyAxis`).
    ///
    /// Here the standard-mapping index and Godot's value happen to agree for
    /// the four thumbstick axes; the analog triggers differ, being buttons in
    /// the browser layout and axes 4/5 in Godot (pat-0q2kx.88).
    pub fn to_godot_index(self) -> i64 {
        match self {
            GamepadAxis::LeftStickX => 0,
            GamepadAxis::LeftStickY => 1,
            GamepadAxis::RightStickX => 2,
            GamepadAxis::RightStickY => 3,
            GamepadAxis::LeftTriggerAnalog => 4,
            GamepadAxis::RightTriggerAnalog => 5,
        }
    }

    /// Inverse of [`to_godot_index`](Self::to_godot_index).
    pub fn from_godot_index(index: i64) -> Option<GamepadAxis> {
        Some(match index {
            0 => GamepadAxis::LeftStickX,
            1 => GamepadAxis::LeftStickY,
            2 => GamepadAxis::RightStickX,
            3 => GamepadAxis::RightStickY,
            4 => GamepadAxis::LeftTriggerAnalog,
            5 => GamepadAxis::RightTriggerAnalog,
            _ => return None,
        })
    }

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
    /// A screen touch event (Godot `InputEventScreenTouch`).
    ScreenTouch {
        /// Finger index; the browser's `Touch.identifier`.
        index: u32,
        position: Vector2,
        pressed: bool,
        /// Second tap of a double tap (`InputEventScreenTouch.double_tap`).
        /// Computed by [`InputState`] — see [`DOUBLE_TAP_FRAMES`].
        double_tap: bool,
        /// The touch was interrupted rather than lifted — a `touchcancel`
        /// (`InputEventScreenTouch.canceled`).
        canceled: bool,
    },
    /// A screen drag event (Godot `InputEventScreenDrag`).
    ScreenDrag {
        index: u32,
        position: Vector2,
        relative: Vector2,
        velocity: Vector2,
        /// Pen pressure in `0.0..=1.0` (`InputEventScreenDrag.pressure`); `0.0`
        /// for a plain finger.
        pressure: f32,
        /// Pen tilt in `-1.0..=1.0` per axis (`InputEventScreenDrag.tilt`).
        tilt: Vector2,
    },
    /// A decoded MIDI message (Godot `InputEventMIDI`), pat-0q2kx.246.
    ///
    /// Built by [`decode_midi_message`] from the raw bytes a MIDI port
    /// delivers; the field set and the per-message decoding are Godot's
    /// (`core/os/midi_driver.cpp` `MIDIDriver::send_event`).
    ///
    /// Every field is present on every message because Godot's
    /// `InputEventMIDI` is one concrete class with one field set — a
    /// `NOTE_ON` simply leaves `controller_number` at 0. Scripts are expected
    /// to switch on [`message`](Self::Midi::message) before reading, exactly
    /// as they do in Godot.
    Midi {
        /// MIDI channel `0..=15`, from the low nibble of the status byte.
        /// `0` for System messages, which carry no channel.
        channel: u8,
        /// The `MIDIMessage` enum value: the status byte's high nibble for
        /// Voice messages (`0x9` for `NOTE_ON`), or the whole byte for
        /// System messages (`0xF0` for `SYSTEM_EXCLUSIVE`).
        message: u8,
        /// Note number for NOTE_ON/NOTE_OFF/AFTERTOUCH. For PITCH_BEND this
        /// is the **14-bit** bend value, not a note.
        pitch: u16,
        /// Note velocity for NOTE_ON/NOTE_OFF.
        velocity: u8,
        /// Controller number for CONTROL_CHANGE.
        controller_number: u8,
        /// Controller value for CONTROL_CHANGE.
        controller_value: u8,
        /// Patch/program number for PROGRAM_CHANGE.
        instrument: u8,
        /// Pressure for AFTERTOUCH (per note) and CHANNEL_PRESSURE.
        pressure: u8,
        /// Index of the port the message arrived on, mirroring Godot's
        /// `InputEvent.device`.
        device: u32,
    },
    /// A named action event (e.g. from `InputMap`).
    Action {
        action: String,
        pressed: bool,
        strength: f32,
    },
    /// A pinch-zoom gesture (Godot `InputEventMagnifyGesture`).
    ///
    /// Godot's hierarchy is `InputEventGesture(position)` <
    /// `InputEventMagnifyGesture(factor)`, and `InputEventGesture` itself
    /// derives from `InputEventWithModifiers`. No modifier fields are carried
    /// here because no source populates them yet; the builder reports them as
    /// `false` rather than leaving a script's `event.shift_pressed` as Nil.
    MagnifyGesture {
        /// Where the gesture is centred (`InputEventGesture.position`).
        position: Vector2,
        /// Scale delta for this frame (`InputEventMagnifyGesture.factor`).
        factor: f32,
    },
    /// A two-finger pan gesture (Godot `InputEventPanGesture`).
    PanGesture {
        /// Where the gesture is centred (`InputEventGesture.position`).
        position: Vector2,
        /// Pan delta for this frame (`InputEventPanGesture.delta`).
        delta: Vector2,
    },
}

// ---------------------------------------------------------------------------
// MIDI decoding (pat-0q2kx.246)
// ---------------------------------------------------------------------------

/// `MIDIMessage` values Godot's decoder branches on. Named rather than spelled
/// as literals at the match sites because `0xB` vs `0xC` is a one-character
/// error that produces a plausible event on the wrong field.
///
/// These are the @GlobalScope `MIDI_MESSAGE_*` integers, which is what a script
/// compares against — `MIDI_MESSAGE_NOTE_ON == 9`.
pub mod midi_message {
    pub const NONE: u8 = 0;
    pub const NOTE_OFF: u8 = 0x8;
    pub const NOTE_ON: u8 = 0x9;
    pub const AFTERTOUCH: u8 = 0xA;
    pub const CONTROL_CHANGE: u8 = 0xB;
    pub const PROGRAM_CHANGE: u8 = 0xC;
    pub const CHANNEL_PRESSURE: u8 = 0xD;
    pub const PITCH_BEND: u8 = 0xE;
    pub const SYSTEM_EXCLUSIVE: u8 = 0xF0;
    pub const QUARTER_FRAME: u8 = 0xF1;
    pub const SONG_POSITION_POINTER: u8 = 0xF2;
    pub const SONG_SELECT: u8 = 0xF3;
}

/// Godot's `MIDIDriver::Parser::status_to_msg_enum`
/// (`core/os/midi_driver.cpp:64`): a Voice status byte reports its **high
/// nibble** (`0x90` -> `0x9`, NOTE_ON) while a System byte reports itself
/// whole (`0xF0` -> `0xF0`). A byte with the top bit clear is data, not a
/// status, and has no message.
pub fn midi_status_to_message(status: u8) -> u8 {
    if status & 0x80 == 0 {
        midi_message::NONE
    } else if status < 0xF0 {
        status >> 4
    } else {
        status
    }
}

/// Godot's `MIDIDriver::Parser::channel` (`core/os/midi_driver.cpp:98`).
///
/// ⚠️ Only **Voice** messages (`0x80..=0xEF`) carry a channel. A System
/// message reports channel 0 — its low nibble is part of the message id, so
/// masking it in would report, say, `SONG_SELECT` (`0xF3`) on channel 3.
pub fn midi_channel(status: u8) -> u8 {
    if (0x80..0xF0).contains(&status) {
        status & 0x0F
    } else {
        0
    }
}

/// Number of data bytes a message requires, from Godot's
/// `MIDIDriver::Parser::expected_data` (`core/os/midi_driver.cpp:79`).
pub fn midi_expected_data(message: u8) -> usize {
    match message {
        midi_message::NOTE_OFF
        | midi_message::NOTE_ON
        | midi_message::AFTERTOUCH
        | midi_message::CONTROL_CHANGE
        | midi_message::PITCH_BEND
        | midi_message::SONG_POSITION_POINTER => 2,
        midi_message::PROGRAM_CHANGE
        | midi_message::CHANNEL_PRESSURE
        | midi_message::QUARTER_FRAME
        | midi_message::SONG_SELECT => 1,
        _ => 0,
    }
}

/// Decode one raw MIDI message into an [`InputEvent::Midi`].
///
/// `packet` is a status byte followed by its data bytes — exactly what the Web
/// MIDI API hands to `MIDIMessageEvent.data`. Mirrors Godot's
/// `MIDIDriver::send_event` (`core/os/midi_driver.cpp:105`) field for field.
///
/// Returns `None` when the packet cannot produce an event, which Godot treats
/// the same way (`ERR_FAIL_COND` there, a dropped message here):
///
/// * empty, or starting with a data byte rather than a status byte — a
///   fragment, not a message;
/// * fewer data bytes than the message requires. Truncating instead would
///   invent a velocity of 0, and NOTE_ON with velocity 0 is the conventional
///   spelling of NOTE_OFF — a dropped message is recoverable, a silently
///   inverted one is not.
pub fn decode_midi_message(packet: &[u8], device: u32) -> Option<InputEvent> {
    let status = *packet.first()?;
    let message = midi_status_to_message(status);
    if message == midi_message::NONE {
        return None;
    }
    let data = &packet[1..];
    if data.len() < midi_expected_data(message) {
        return None;
    }

    // Which fields a message fills is Godot's table, transcribed arm for arm
    // from `MIDIDriver::send_event`. Anything a given message does not fill
    // stays 0 — `InputEventMIDI` is one concrete class with one field set, so
    // a NOTE_ON really does carry a `controller_number` of 0.
    let mut pitch = 0u16;
    let mut velocity = 0u8;
    let mut controller_number = 0u8;
    let mut controller_value = 0u8;
    let mut instrument = 0u8;
    let mut pressure = 0u8;

    match message {
        midi_message::NOTE_OFF | midi_message::NOTE_ON => {
            pitch = u16::from(data[0]);
            velocity = data[1];
        }
        midi_message::AFTERTOUCH => {
            pitch = u16::from(data[0]);
            pressure = data[1];
        }
        midi_message::CONTROL_CHANGE => {
            controller_number = data[0];
            controller_value = data[1];
        }
        midi_message::PROGRAM_CHANGE => instrument = data[0],
        midi_message::CHANNEL_PRESSURE => pressure = data[0],
        // 14-bit, LSB first: `(msb << 7) | lsb`. Note the operand order —
        // reversing it still yields a number in range, so a swap looks like a
        // wheel that moves erratically rather than like a decode bug.
        midi_message::PITCH_BEND => {
            pitch = (u16::from(data[1]) << 7) | u16::from(data[0]);
        }
        // QUARTER_FRAME / SONG_POSITION_POINTER / SONG_SELECT carry data Godot
        // does not decode either; the message id still reaches the script,
        // which is what Godot delivers.
        _ => {}
    }

    Some(InputEvent::Midi {
        channel: midi_channel(status),
        message,
        pitch,
        velocity,
        controller_number,
        controller_value,
        instrument,
        pressure,
        device,
    })
}

// ---------------------------------------------------------------------------
// ActionBinding
// ---------------------------------------------------------------------------

/// A key plus the modifier keys a binding requires (Godot's `InputEventKey`
/// carrying a `KeyModifierMask`), e.g. Shift+Tab for `ui_focus_prev`.
///
/// Godot's non-exact action matching is a **subset** test: the binding's
/// modifiers must all be held, but the event may hold others
/// (`core/input/input_event.cpp` `InputEventKey::action_match`:
/// `(action_mask & key_mask) == action_mask`). So a plain `Tab` binding also
/// matches Shift+Tab, while a Shift+Tab binding does not match a bare Tab.
///
/// Patina's [`InputEvent::Key`] carries no Meta/Command bit, so Godot's
/// `CMD_OR_CTRL` resolves to [`ctrl`](Self::ctrl) — its non-macOS meaning — and
/// the `.macos` builtin variants are not installed. See
/// [`InputMap::install_builtin_ui_actions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    /// The key itself.
    pub key: Key,
    /// Shift must be held.
    pub shift: bool,
    /// Control must be held.
    pub ctrl: bool,
    /// Alt must be held.
    pub alt: bool,
}

impl KeyCombo {
    /// A binding on `key` with no modifier requirement.
    pub fn new(key: Key) -> Self {
        Self {
            key,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    /// Requires Shift in addition to any modifiers already set.
    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Requires Control (also Godot's `CMD_OR_CTRL` off macOS).
    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// Requires Alt.
    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Whether an event holding `(shift, ctrl, alt)` satisfies this binding's
    /// modifier requirement — the subset rule quoted on [`KeyCombo`].
    fn modifiers_satisfied_by(&self, shift: bool, ctrl: bool, alt: bool) -> bool {
        (!self.shift || shift) && (!self.ctrl || ctrl) && (!self.alt || alt)
    }
}

/// Godot's `InputMap::DEFAULT_DEADZONE` — the deadzone an action gets when it
/// does not declare one.
///
/// Ground truth: `core/input/input_map.h:55`
/// (`static constexpr float DEFAULT_DEADZONE = 0.2f`), applied by
/// `load_from_project_settings` at `input_map.cpp:320`.
///
/// Named rather than spelled 0.2 at each site because it is now read in two
/// crates — here, for an action a `project.godot` declares without one, and in
/// the editor, for an action the editor itself contributes to a Play session
/// (pat-mgixk.24).
pub const DEFAULT_DEADZONE: f32 = 0.2;

/// What physical input is bound to an action.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionBinding {
    /// A keyboard key binding with no modifier requirement.
    KeyBinding(Key),
    /// A keyboard binding that also requires modifier keys (pat-0q2kx.89).
    KeyWithModifiers(KeyCombo),
    /// A mouse button binding.
    MouseBinding(MouseButton),
    /// A gamepad button binding.
    GamepadButtonBinding(GamepadButton),
    /// A gamepad axis binding (positive direction).
    GamepadAxisBinding(GamepadAxis),
    /// A gamepad axis binding with an explicit direction — Godot's
    /// `InputEventJoypadMotion` carries a signed `axis_value`, and a binding
    /// only *presses* on deflection with the same sign (pat-0q2kx.89). This is
    /// what makes left-stick left and right separate actions.
    GamepadAxisDirection {
        /// The axis.
        axis: GamepadAxis,
        /// `true` binds positive deflection, `false` negative.
        positive: bool,
    },
}

impl ActionBinding {
    /// A key binding requiring `Shift`.
    pub fn shift_key(key: Key) -> Self {
        ActionBinding::KeyWithModifiers(KeyCombo::new(key).shift())
    }

    /// A key binding requiring `Ctrl` (Godot's `CMD_OR_CTRL` off macOS).
    pub fn ctrl_key(key: Key) -> Self {
        ActionBinding::KeyWithModifiers(KeyCombo::new(key).ctrl())
    }

    /// The negative-direction binding of `axis`.
    pub fn axis_negative(axis: GamepadAxis) -> Self {
        ActionBinding::GamepadAxisDirection {
            axis,
            positive: false,
        }
    }

    /// The positive-direction binding of `axis`.
    pub fn axis_positive(axis: GamepadAxis) -> Self {
        ActionBinding::GamepadAxisDirection {
            axis,
            positive: true,
        }
    }
}

/// Godot's `InputEventJoypadMotion::action_match` for a binding on
/// `positive` deflection receiving `value` (`core/input/input_event.cpp`).
///
/// The binding matches the axis regardless of direction; what direction decides
/// is whether it *presses*. A value of exactly zero counts as the same
/// direction, so returning to centre releases rather than leaving the action
/// stuck. The deadzone is a floor on pressing **and** the low end of the
/// strength remap, so strength rises from 0 at the deadzone to 1 at full
/// deflection instead of jumping to the deadzone's value.
fn axis_match(value: f32, positive: bool, deadzone: f32) -> ActionMatch {
    let abs = value.abs();
    let same_direction = value == 0.0 || (value < 0.0) != positive;
    let pressed = same_direction && abs >= deadzone;
    let strength = if !pressed {
        0.0
    } else if deadzone >= 1.0 {
        // Godot special-cases this: inverse_lerp would divide by zero.
        1.0
    } else {
        ((abs - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0)
    };
    ActionMatch {
        pressed,
        strength,
        // Raw strength deliberately ignores the deadzone, but not the
        // direction — the opposite stick half reads 0, not |v|.
        raw_strength: if same_direction { abs } else { 0.0 },
    }
}

/// The outcome of testing one event against one action, mirroring the
/// out-parameters of Godot's `InputEvent::action_match`
/// (`core/input/input_event.cpp`).
///
/// A *match* means the event is bound to the action at all; `pressed` is a
/// separate question. An axis event on a bound axis matches even when deflected
/// the wrong way — it just reports `pressed = false`, which is how Godot
/// releases the opposite action as the stick crosses centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActionMatch {
    /// Whether the action is pressed by this event.
    pub pressed: bool,
    /// Deadzone-remapped strength in `0.0..=1.0` (Godot `get_action_strength`).
    pub strength: f32,
    /// Strength ignoring the deadzone (Godot `get_action_raw_strength`).
    pub raw_strength: f32,
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
    /// events (deadzone 0.5), mirroring `InputMap::get_builtins`
    /// (`upstream/godot/core/input/input_map.cpp`) as installed by
    /// `load_from_project_settings`. Project-declared actions override these —
    /// see [`load_from_project_godot`].
    ///
    /// Two deliberate departures from the upstream table, both because
    /// [`InputEvent::Key`] carries no Meta/Command bit:
    ///
    /// * Godot's `CMD_OR_CTRL` is installed as Ctrl — its meaning on every
    ///   platform except macOS.
    /// * The `.macos` action variants (`ui_close_dialog.macos`,
    ///   `ui_text_caret_line_start.macos`, …) are **not** installed, since they
    ///   exist only to swap Ctrl for Command. Feature-tagged overrides are
    ///   Godot's `get_builtins_with_feature_overrides_applied`, a separate
    ///   mechanism from this table.
    ///
    /// Keys absent from [`Key`] are skipped: `KP_ENTER` drops from the actions
    /// that also bind `ENTER`, and `ui_menu` / `ui_swap_input_direction` — whose
    /// only bindings are `MENU` and `QUOTELEFT` — are not installed at all,
    /// since an action with no reachable binding can never fire.
    ///
    /// [`load_from_project_godot`]: InputMap::load_from_project_godot
    pub fn install_builtin_ui_actions(&mut self) {
        use ActionBinding as B;
        // Shorthands matching the upstream spelling, so this table can be diffed
        // against `input_map.cpp` line by line.
        let k = B::KeyBinding;
        let shift = B::shift_key;
        // Godot `CMD_OR_CTRL` and `CTRL` both land on Ctrl here (see above).
        let ctrl = B::ctrl_key;
        let ctrl_shift = |key: Key| B::KeyWithModifiers(KeyCombo::new(key).ctrl().shift());
        let ctrl_alt = |key: Key| B::KeyWithModifiers(KeyCombo::new(key).ctrl().alt());
        let alt = |key: Key| B::KeyWithModifiers(KeyCombo::new(key).alt());
        let pad = B::GamepadButtonBinding;

        let builtins: Vec<(&str, Vec<ActionBinding>)> = vec![
            // ----- UI navigation -----
            // Godot binds no joypad button to accept/cancel — only `ui_select`
            // carries one (JoyButton::Y). Patina previously added FaceA/FaceB
            // here; they are gone because this table's whole job is to be the
            // upstream one (pat-0q2kx.89). A project wanting them declares them
            // in `project.godot`, which overrides the builtin.
            ("ui_accept", vec![k(Key::Enter), k(Key::Space)]),
            ("ui_select", vec![k(Key::Space), pad(GamepadButton::FaceY)]),
            ("ui_cancel", vec![k(Key::Escape)]),
            ("ui_close_dialog", vec![k(Key::Escape)]),
            ("ui_focus_next", vec![k(Key::Tab)]),
            // The headline modifier binding: Shift+Tab, not a shoulder button.
            ("ui_focus_prev", vec![shift(Key::Tab)]),
            (
                "ui_left",
                vec![
                    k(Key::Left),
                    pad(GamepadButton::DPadLeft),
                    B::axis_negative(GamepadAxis::LeftStickX),
                ],
            ),
            (
                "ui_right",
                vec![
                    k(Key::Right),
                    pad(GamepadButton::DPadRight),
                    B::axis_positive(GamepadAxis::LeftStickX),
                ],
            ),
            (
                "ui_up",
                vec![
                    k(Key::Up),
                    pad(GamepadButton::DPadUp),
                    B::axis_negative(GamepadAxis::LeftStickY),
                ],
            ),
            (
                "ui_down",
                vec![
                    k(Key::Down),
                    pad(GamepadButton::DPadDown),
                    B::axis_positive(GamepadAxis::LeftStickY),
                ],
            ),
            ("ui_page_up", vec![k(Key::PageUp)]),
            ("ui_page_down", vec![k(Key::PageDown)]),
            ("ui_home", vec![k(Key::Home)]),
            ("ui_end", vec![k(Key::End)]),
            // ----- UI basic shortcuts -----
            ("ui_cut", vec![ctrl(Key::X), shift(Key::Delete)]),
            ("ui_copy", vec![ctrl(Key::C), ctrl(Key::Insert)]),
            ("ui_focus_mode", vec![ctrl(Key::M)]),
            ("ui_paste", vec![ctrl(Key::V), shift(Key::Insert)]),
            ("ui_undo", vec![ctrl(Key::Z)]),
            ("ui_redo", vec![ctrl_shift(Key::Z), ctrl(Key::Y)]),
            // ----- UI text input shortcuts -----
            ("ui_text_completion_query", vec![ctrl(Key::Space)]),
            (
                "ui_text_completion_accept",
                vec![shift(Key::Tab), shift(Key::Enter)],
            ),
            (
                "ui_text_completion_replace",
                vec![k(Key::Tab), k(Key::Enter)],
            ),
            ("ui_text_newline", vec![k(Key::Enter)]),
            ("ui_text_newline_blank", vec![ctrl(Key::Enter)]),
            ("ui_text_newline_above", vec![ctrl_shift(Key::Enter)]),
            ("ui_text_indent", vec![k(Key::Tab)]),
            ("ui_text_dedent", vec![shift(Key::Tab)]),
            (
                "ui_text_backspace",
                vec![k(Key::Backspace), shift(Key::Backspace)],
            ),
            ("ui_text_backspace_word", vec![ctrl(Key::Backspace)]),
            ("ui_text_backspace_all_to_left", vec![]),
            ("ui_text_delete", vec![k(Key::Delete)]),
            ("ui_text_delete_word", vec![ctrl(Key::Delete)]),
            ("ui_text_delete_all_to_right", vec![]),
            ("ui_text_caret_left", vec![k(Key::Left)]),
            ("ui_text_caret_word_left", vec![ctrl(Key::Left)]),
            ("ui_text_caret_right", vec![k(Key::Right)]),
            ("ui_text_caret_word_right", vec![ctrl(Key::Right)]),
            ("ui_text_caret_up", vec![k(Key::Up)]),
            ("ui_text_caret_down", vec![k(Key::Down)]),
            ("ui_text_caret_line_start", vec![k(Key::Home)]),
            ("ui_text_caret_line_end", vec![k(Key::End)]),
            ("ui_text_caret_page_up", vec![k(Key::PageUp)]),
            ("ui_text_caret_page_down", vec![k(Key::PageDown)]),
            ("ui_text_caret_document_start", vec![ctrl(Key::Home)]),
            ("ui_text_caret_document_end", vec![ctrl(Key::End)]),
            ("ui_text_caret_add_below", vec![ctrl_shift(Key::Down)]),
            ("ui_text_caret_add_above", vec![ctrl_shift(Key::Up)]),
            ("ui_text_scroll_up", vec![ctrl(Key::Up)]),
            ("ui_text_scroll_down", vec![ctrl(Key::Down)]),
            ("ui_text_select_all", vec![ctrl(Key::A)]),
            ("ui_text_select_word_under_caret", vec![alt(Key::G)]),
            (
                "ui_text_add_selection_for_next_occurrence",
                vec![ctrl(Key::D)],
            ),
            (
                "ui_text_skip_selection_for_next_occurrence",
                vec![ctrl_alt(Key::D)],
            ),
            ("ui_text_clear_carets_and_selection", vec![k(Key::Escape)]),
            ("ui_text_toggle_insert_mode", vec![k(Key::Insert)]),
            // Upstream binds the context-menu key here (input_map.cpp:773-775).
            ("ui_menu", vec![k(Key::Menu)]),
            ("ui_text_submit", vec![k(Key::Enter)]),
            ("ui_unicode_start", vec![ctrl_shift(Key::U)]),
            // ----- GraphEdit -----
            ("ui_graph_duplicate", vec![ctrl(Key::D)]),
            ("ui_graph_delete", vec![k(Key::Delete)]),
            ("ui_graph_follow_left", vec![ctrl(Key::Left)]),
            ("ui_graph_follow_right", vec![ctrl(Key::Right)]),
            // ----- FileDialog -----
            ("ui_filedialog_delete", vec![k(Key::Delete)]),
            ("ui_filedialog_up_one_level", vec![k(Key::Backspace)]),
            ("ui_filedialog_refresh", vec![k(Key::F5)]),
            ("ui_filedialog_show_hidden", vec![k(Key::H)]),
            ("ui_filedialog_find", vec![ctrl(Key::F)]),
            ("ui_filedialog_focus_path", vec![ctrl(Key::L)]),
            // ----- Misc -----
            // Ctrl+Backquote upstream (`Key::QUOTELEFT | CMD_OR_CTRL`,
            // input_map.cpp:846-847) — flips RTL/LTR text entry.
            ("ui_swap_input_direction", vec![ctrl(Key::QuoteLeft)]),
            ("ui_accessibility_drag_and_drop", vec![]),
            (
                "ui_colorpicker_delete_preset",
                vec![pad(GamepadButton::FaceX), k(Key::Delete)],
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

    /// Tests `event` against every binding of `action`, returning Godot's
    /// `action_match` triple for the first binding that matches (pat-0q2kx.89).
    ///
    /// `None` means the event is not bound to this action at all. `Some` with
    /// `pressed = false` means it *is* bound but does not press — an axis
    /// deflected below the deadzone or in the opposite direction, or any
    /// release event.
    ///
    /// Godot: `core/input/input_event.cpp` `InputEventKey::action_match`
    /// (modifier subset test), `InputEventJoypadMotion::action_match`
    /// (same-direction test, `|v| >= deadzone`, and
    /// `CLAMP(inverse_lerp(deadzone, 1, |v|), 0, 1)` strength).
    pub fn action_match(&self, event: &InputEvent, action: &str) -> Option<ActionMatch> {
        let binds = self.bindings.get(action)?;
        let deadzone = self.get_deadzone(action);
        // A digital binding is fully described by its pressed flag.
        let digital = |pressed: bool| {
            Some(ActionMatch {
                pressed,
                strength: if pressed { 1.0 } else { 0.0 },
                raw_strength: if pressed { 1.0 } else { 0.0 },
            })
        };
        for bind in binds {
            match (event, bind) {
                // A bare binding has no modifier requirement, so the subset test
                // is vacuously satisfied whatever the event holds — Ctrl+C still
                // fires an action bound to plain C.
                (InputEvent::Key { key, pressed, .. }, ActionBinding::KeyBinding(k))
                    if key == k =>
                {
                    return digital(*pressed);
                }
                (
                    InputEvent::Key {
                        key,
                        pressed,
                        shift,
                        ctrl,
                        alt,
                    },
                    ActionBinding::KeyWithModifiers(combo),
                ) if key == &combo.key => {
                    // Godot only applies the modifier test to a press; a release
                    // matches so the action can drop even if the user let go of
                    // the modifier first.
                    if *pressed && !combo.modifiers_satisfied_by(*shift, *ctrl, *alt) {
                        continue;
                    }
                    return digital(*pressed);
                }
                (
                    InputEvent::MouseButton {
                        button, pressed, ..
                    },
                    ActionBinding::MouseBinding(b),
                ) if button == b => {
                    return digital(*pressed);
                }
                (
                    InputEvent::GamepadButton {
                        button, pressed, ..
                    },
                    ActionBinding::GamepadButtonBinding(b),
                ) if button == b => {
                    return digital(*pressed);
                }
                (
                    InputEvent::GamepadAxis { axis, value, .. },
                    ActionBinding::GamepadAxisBinding(a),
                ) if axis == a => {
                    // The legacy binding form means "positive direction".
                    return Some(axis_match(*value, true, deadzone));
                }
                (
                    InputEvent::GamepadAxis { axis, value, .. },
                    ActionBinding::GamepadAxisDirection { axis: a, positive },
                ) if axis == a => {
                    return Some(axis_match(*value, *positive, deadzone));
                }
                _ => {}
            }
        }
        None
    }

    /// Returns `true` when `event` presses `action`.
    ///
    /// This is the "does this event fire the action" question, so an axis event
    /// below the deadzone or deflected the wrong way answers `false` even though
    /// it is bound. Use [`action_match`](Self::action_match) to tell "not bound"
    /// from "bound but not pressed" — the release path needs that distinction.
    pub fn event_matches_action(&self, event: &InputEvent, action: &str) -> bool {
        match self.action_match(event, action) {
            // Releases still count as a match here: callers that only ask this
            // question treat a key-up on a bound key as belonging to the action.
            Some(m) => match event {
                InputEvent::GamepadAxis { .. } => m.pressed,
                _ => true,
            },
            None => false,
        }
    }

    /// Returns all action names.
    pub fn actions(&self) -> impl Iterator<Item = &String> {
        self.deadzones.keys()
    }

    /// Returns bindings for an action.
    pub fn get_bindings(&self, action: &str) -> Option<&[ActionBinding]> {
        self.bindings.get(action).map(|v| v.as_slice())
    }

    // -- Script-facing InputMap API (pat-0q2kx.89) --------------------------
    //
    // Godot exposes InputMap as a singleton scripts mutate at runtime; these
    // are the methods behind `InputMap.has_action(...)` and friends.

    /// Whether `action` exists (Godot `InputMap.has_action`).
    pub fn has_action(&self, action: &str) -> bool {
        self.deadzones.contains_key(action)
    }

    /// Removes `action` and all of its bindings (Godot `InputMap.erase_action`).
    pub fn erase_action(&mut self, action: &str) {
        self.deadzones.remove(action);
        self.bindings.remove(action);
    }

    /// Sets an existing action's deadzone (Godot
    /// `InputMap.action_set_deadzone`). Unknown actions are ignored.
    pub fn action_set_deadzone(&mut self, action: &str, deadzone: f32) {
        if let Some(slot) = self.deadzones.get_mut(action) {
            *slot = deadzone;
        }
    }

    /// Removes one binding from `action` (Godot `InputMap.action_erase_event`).
    pub fn action_erase_event(&mut self, action: &str, binding: &ActionBinding) {
        if let Some(list) = self.bindings.get_mut(action) {
            list.retain(|b| b != binding);
        }
    }

    /// Removes every binding from `action`, keeping the action itself (Godot
    /// `InputMap.action_erase_events`).
    pub fn action_erase_events(&mut self, action: &str) {
        if let Some(list) = self.bindings.get_mut(action) {
            list.clear();
        }
    }

    /// Sorted action names (Godot `InputMap.get_actions`).
    ///
    /// Sorted because the backing map is a `HashMap`, and a script iterating
    /// actions must not see a different order run to run.
    pub fn get_actions(&self) -> Vec<String> {
        let mut names: Vec<String> = self.deadzones.keys().cloned().collect();
        names.sort();
        names
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
    /// Action strength values (for analog inputs like gamepad axes), with the
    /// deadzone remapped away — Godot `Input.get_action_strength`.
    action_strengths: HashMap<String, f32>,
    /// Action strength ignoring the deadzone — Godot
    /// `Input.get_action_raw_strength` (pat-0q2kx.89). Kept separate because a
    /// stick at 0.6 with a 0.1 deadzone reads 0.56 strength but 0.6 raw.
    action_raw_strengths: HashMap<String, f32>,
    /// Optional input map for action resolution.
    input_map: Option<InputMap>,
    /// Current mouse capture/visibility mode (`Input.set_mouse_mode`).
    mouse_mode: MouseMode,
    /// Connected joypad device ids (`Input.get_connected_joypads`).
    connected_joypads: HashSet<u32>,
    /// Active joypad vibration per device: `(weak, strong, duration_seconds)`.
    joy_vibration: HashMap<u32, (f32, f32, f32)>,
    /// Mouse motion accumulated since the last `flush_frame`, in pixels — the
    /// numerator of Godot's `get_last_mouse_velocity` (pat-0q2kx.92).
    mouse_motion_this_frame: Vector2,
    // -- Touch (pat-0q2kx.93) ----------------------------------------------
    /// Last known position of each finger currently down.
    touch_positions: HashMap<u32, Vector2>,
    /// Frame number at which each finger last went down, for double-tap.
    touch_last_press_frame: HashMap<u32, u64>,
    /// Monotonic frame counter, advanced by [`flush_frame`](Self::flush_frame).
    /// Double-tap is measured in frames because `InputState` has no clock.
    frame_counter: u64,
    /// The finger currently driving the emulated mouse, if any. Godot's
    /// `mouse_from_touch_index`: only the **first** finger down translates, and
    /// only its release ends the emulated press (core/input/input.cpp).
    mouse_from_touch_index: Option<u32>,
    /// `input_devices/pointing/emulate_mouse_from_touch` — ON by default in
    /// Godot, which is what lets a touch press a Button.
    emulate_mouse_from_touch: bool,
    /// `input_devices/pointing/emulate_touch_from_mouse` — OFF by default.
    emulate_touch_from_mouse: bool,

    // -- Device sensors (pat-0q2kx.247) --
    //
    // Godot's web build feeds `devicemotion` / `deviceorientation` into these
    // (platform/web/js/libs/library_godot_input.js). All four read as ZERO
    // until a platform posts a reading, which is also what a desktop build
    // reports — so a script polling them on a sensorless device sees Godot's
    // behaviour rather than an error.
    accelerometer: gdcore::math::Vector3,
    gravity: gdcore::math::Vector3,
    gyroscope: gdcore::math::Vector3,
    /// Generally stays zero on web: no browser exposes a raw magnetometer.
    magnetometer: gdcore::math::Vector3,
}

/// How many frames after a finger's last press a second press still counts as a
/// double tap.
///
/// Godot's platform layers flag `double_tap` using a 0.25 s window; `InputState`
/// has no clock, so this is that window at 60 Hz. Frame-based keeps it
/// deterministic under test (pat-0q2kx.93).
pub const DOUBLE_TAP_FRAMES: u64 = 15;

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
            action_raw_strengths: HashMap::new(),
            input_map: None,
            mouse_mode: MouseMode::Visible,
            connected_joypads: HashSet::new(),
            joy_vibration: HashMap::new(),
            mouse_motion_this_frame: Vector2::ZERO,
            touch_positions: HashMap::new(),
            touch_last_press_frame: HashMap::new(),
            frame_counter: 0,
            mouse_from_touch_index: None,
            // Godot's project-settings default is ON, which is why a touch can
            // press a Button without the game doing anything.
            emulate_mouse_from_touch: true,
            emulate_touch_from_mouse: false,
            accelerometer: gdcore::math::Vector3::ZERO,
            gravity: gdcore::math::Vector3::ZERO,
            gyroscope: gdcore::math::Vector3::ZERO,
            magnetometer: gdcore::math::Vector3::ZERO,
        }
    }

    /// Godot's `Input.get_accelerometer()` — device acceleration including
    /// gravity, m/s² (pat-0q2kx.247).
    pub fn accelerometer(&self) -> gdcore::math::Vector3 {
        self.accelerometer
    }

    /// Godot's `Input.get_gravity()` — the gravity component alone.
    pub fn gravity(&self) -> gdcore::math::Vector3 {
        self.gravity
    }

    /// Godot's `Input.get_gyroscope()` — angular velocity, rad/s.
    pub fn gyroscope(&self) -> gdcore::math::Vector3 {
        self.gyroscope
    }

    /// Godot's `Input.get_magnetometer()`. Stays ZERO on web, where no browser
    /// exposes a raw magnetometer — reporting zero is the honest answer, and
    /// matches what Godot's own web build returns.
    pub fn magnetometer(&self) -> gdcore::math::Vector3 {
        self.magnetometer
    }

    /// Platform hook for a `devicemotion` reading.
    ///
    /// These setters are part of Godot's script API too (`Input.set_*`), not
    /// merely an internal feed — a game may inject synthetic readings for
    /// testing, which is why they are public rather than crate-private.
    pub fn set_accelerometer(&mut self, v: gdcore::math::Vector3) {
        self.accelerometer = v;
    }

    pub fn set_gravity(&mut self, v: gdcore::math::Vector3) {
        self.gravity = v;
    }

    pub fn set_gyroscope(&mut self, v: gdcore::math::Vector3) {
        self.gyroscope = v;
    }

    pub fn set_magnetometer(&mut self, v: gdcore::math::Vector3) {
        self.magnetometer = v;
    }

    /// Mouse motion accumulated since the last [`flush_frame`](Self::flush_frame),
    /// in pixels. Divide by the frame delta for Godot's
    /// `Input.get_last_mouse_velocity` (pat-0q2kx.92).
    pub fn mouse_motion_this_frame(&self) -> Vector2 {
        self.mouse_motion_this_frame
    }

    /// Attaches an `InputMap` so that physical events automatically resolve to actions.
    pub fn set_input_map(&mut self, map: InputMap) {
        self.input_map = Some(map);
    }

    /// The attached `InputMap`, if any.
    pub fn input_map(&self) -> Option<&InputMap> {
        self.input_map.as_ref()
    }

    /// The attached `InputMap` for mutation, creating an empty one if the state
    /// has none yet (pat-0q2kx.89).
    ///
    /// Runtime rebinding through `InputMap.add_action` has to work on a state
    /// nobody handed a map to, so this makes one rather than dropping the call.
    pub fn input_map_mut(&mut self) -> &mut InputMap {
        self.input_map.get_or_insert_with(InputMap::new)
    }

    /// Action name → its real bindings (pat-s3rrv.14).
    ///
    /// The count mirror below answers "does this action exist and how many
    /// events feed it". Godot's `InputEvent.is_action_pressed(name)` asks a
    /// different question — does *this event* match one of that action's
    /// bindings — and a count cannot answer it. This is the mirror that can.
    pub fn input_map_action_bindings(&self) -> HashMap<String, Vec<ActionBinding>> {
        match &self.input_map {
            Some(map) => map
                .get_actions()
                .into_iter()
                .map(|a| {
                    let bindings = map.get_bindings(&a).map(<[ActionBinding]>::to_vec);
                    (a, bindings.unwrap_or_default())
                })
                .collect(),
            None => HashMap::new(),
        }
    }

    /// Action name → bound-event count, the read mirror a script phase needs for
    /// `InputMap.has_action` / `get_actions` / `action_has_event`.
    pub fn input_map_action_event_counts(&self) -> HashMap<String, usize> {
        match &self.input_map {
            Some(map) => map
                .get_actions()
                .into_iter()
                .map(|a| {
                    let count = map.get_bindings(&a).map(|b| b.len()).unwrap_or(0);
                    (a, count)
                })
                .collect(),
            None => HashMap::new(),
        }
    }

    /// Processes a single input event, updating internal state.
    pub fn process_event(&mut self, event: InputEvent) {
        // Resolve actions via InputMap before processing the raw event.
        //
        // `action_match` answers Godot's three questions at once — bound?
        // pressed? how strongly? — so an axis crossing centre releases the
        // action it was driving instead of leaving it stuck pressed
        // (pat-0q2kx.89).
        if let Some(map) = &self.input_map {
            let map = map.clone();
            for action in map.actions() {
                let Some(m) = map.action_match(&event, action) else {
                    continue;
                };
                // Events with no press semantics (motion, touch) never reach a
                // binding, but guard anyway so they cannot clear action state.
                if matches!(
                    event,
                    InputEvent::MouseMotion { .. }
                        | InputEvent::ScreenTouch { .. }
                        | InputEvent::ScreenDrag { .. }
                        | InputEvent::Action { .. }
                        // No `ActionBinding` pairs with a MIDI message, so
                        // `action_match` already answers `None` for one
                        // (pat-0q2kx.246). Listed anyway, per this guard's
                        // stated purpose, so that adding a MIDI binding kind
                        // later is a deliberate act rather than an accident.
                        | InputEvent::Midi { .. }
                        // Gestures have no press semantics and no
                        // `ActionBinding` pairs with one, so `action_match`
                        // already answers `None`. Listed for the same reason
                        // MIDI is: binding a gesture later should be a
                        // deliberate act, not an accident.
                        | InputEvent::MagnifyGesture { .. }
                        | InputEvent::PanGesture { .. }
                ) {
                    continue;
                }
                self.action_strengths.insert(action.clone(), m.strength);
                self.action_raw_strengths
                    .insert(action.clone(), m.raw_strength);
                if m.pressed {
                    if self.actions_pressed.insert(action.clone()) {
                        self.actions_just_pressed.insert(action.clone());
                    }
                } else if self.actions_pressed.remove(action) {
                    self.actions_just_released.insert(action.clone());
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
                // The reverse of the touch path: with the setting on, a left
                // click also lands as finger 0 (pat-0q2kx.93).
                self.emulate_touch_from_mouse_event(button, position, pressed);
            }
            InputEvent::MouseMotion {
                position, relative, ..
            } => {
                self.mouse_position = position;
                // Accumulated so several motion events inside one frame add up
                // (a fast drag produces many); the frame owner divides by the
                // delta to get Godot's velocity and clears it in `flush_frame`
                // (pat-0q2kx.92).
                self.mouse_motion_this_frame = self.mouse_motion_this_frame + relative;
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
            InputEvent::ScreenTouch {
                index,
                position,
                pressed,
                ..
            } => {
                if pressed {
                    self.touch_positions.insert(index, position);
                    self.touch_last_press_frame
                        .insert(index, self.frame_counter);
                } else {
                    // Godot erases the entry rather than keeping a fossil, since
                    // an OS may never reuse the pointer index.
                    self.touch_positions.remove(&index);
                }
                self.emulate_mouse_from_touch_event(index, position, pressed);
            }
            InputEvent::ScreenDrag {
                index, position, ..
            } => {
                self.touch_positions.insert(index, position);
                // Only the finger driving the emulated mouse moves the cursor.
                if self.emulate_mouse_from_touch && self.mouse_from_touch_index == Some(index) {
                    self.mouse_position = position;
                }
            }
            InputEvent::Action {
                ref action,
                pressed,
                strength,
            } => {
                self.action_strengths.insert(action.clone(), strength);
                self.action_raw_strengths.insert(action.clone(), strength);
                if pressed {
                    if self.actions_pressed.insert(action.clone()) {
                        self.actions_just_pressed.insert(action.clone());
                    }
                } else if self.actions_pressed.remove(action) {
                    self.actions_just_released.insert(action.clone());
                }
            }
            // MIDI carries no held state (pat-0q2kx.246). Godot's `Input`
            // singleton has no `is_midi_*` query either: `InputEventMIDI` goes
            // straight through `parse_input_event` to `_input` and nothing
            // accumulates. A NOTE_ON does **not** become a pressed key here —
            // tracking it would invent a "held note" API Godot does not have,
            // and one that no NOTE_OFF path would reliably clear.
            InputEvent::Midi { .. } => {}
            // Gestures accumulate no state either. Godot's `Input` singleton
            // exposes no gesture query — a magnify/pan goes straight through to
            // `_input` and nothing is held. In particular this does NOT move
            // `mouse_position`: a trackpad gesture carries a position but does
            // not move the cursor, and writing it here would drag the pointer
            // to the gesture centre on every pinch.
            InputEvent::MagnifyGesture { .. } | InputEvent::PanGesture { .. } => {}
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

    /// The action's strength **ignoring its deadzone** — Godot
    /// `Input.get_action_raw_strength` (pat-0q2kx.89).
    ///
    /// Differs from [`get_action_strength`](Self::get_action_strength) only for
    /// analog bindings: a stick at 0.6 under a 0.1 deadzone reads 0.56 there and
    /// 0.6 here. Digital bindings report the same 1.0 / 0.0 from both.
    pub fn get_action_raw_strength(&self, name: &str) -> f32 {
        if self.actions_pressed.contains(name) {
            self.action_raw_strengths
                .get(name)
                .copied()
                .or_else(|| self.action_strengths.get(name).copied())
                .unwrap_or(1.0)
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
        self.mouse_motion_this_frame = Vector2::ZERO;
        // Advances the clock double-tap detection measures against.
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    // -- Touch + pointer emulation (pat-0q2kx.93) ---------------------------

    /// Godot's `Input.set_emulate_mouse_from_touch`. ON by default, matching
    /// the `input_devices/pointing/emulate_mouse_from_touch` project setting.
    pub fn set_emulate_mouse_from_touch(&mut self, enable: bool) {
        self.emulate_mouse_from_touch = enable;
    }

    /// Whether touch synthesizes mouse events
    /// (`Input.is_emulating_mouse_from_touch`).
    pub fn is_emulating_mouse_from_touch(&self) -> bool {
        self.emulate_mouse_from_touch
    }

    /// Godot's `Input.set_emulate_touch_from_mouse`. OFF by default.
    pub fn set_emulate_touch_from_mouse(&mut self, enable: bool) {
        self.emulate_touch_from_mouse = enable;
    }

    /// Whether the mouse synthesizes touch events
    /// (`Input.is_emulating_touch_from_mouse`).
    pub fn is_emulating_touch_from_mouse(&self) -> bool {
        self.emulate_touch_from_mouse
    }

    /// The finger currently driving the emulated mouse, if any
    /// (`mouse_from_touch_index`, pat-s3rrv.77).
    ///
    /// Exposed because the emulated mouse EVENT is synthesized outside this
    /// type — `MainLoop` owns both the state and the scene tree, and only an
    /// event can reach the GUI stage. The decision has to be read BEFORE
    /// `process_event` runs, since processing a release clears this.
    pub fn mouse_from_touch_index(&self) -> Option<u32> {
        self.mouse_from_touch_index
    }

    /// Whether finger `index` is currently down.
    pub fn is_touch_pressed(&self, index: u32) -> bool {
        self.touch_positions.contains_key(&index)
    }

    /// The last known position of finger `index`, if it is down.
    pub fn touch_position(&self, index: u32) -> Option<Vector2> {
        self.touch_positions.get(&index).copied()
    }

    /// How many fingers are currently down.
    pub fn touch_count(&self) -> usize {
        self.touch_positions.len()
    }

    /// Whether a press of finger `index` on the current frame would be the
    /// second tap of a double tap — i.e. it last went down within
    /// [`DOUBLE_TAP_FRAMES`].
    ///
    /// The producer calls this to fill `ScreenTouch::double_tap` before pushing
    /// the event, mirroring how Godot's platform layers flag it (pat-0q2kx.93).
    pub fn would_be_double_tap(&self, index: u32) -> bool {
        match self.touch_last_press_frame.get(&index) {
            Some(&last) => self.frame_counter.saturating_sub(last) <= DOUBLE_TAP_FRAMES,
            None => false,
        }
    }

    /// Synthesizes the left mouse button from a touch, mirroring Godot's
    /// `emulate_mouse_from_touch` block in `core/input/input.cpp`.
    ///
    /// Only the **first** finger down translates (`mouse_from_touch_index`), and
    /// only that finger's release ends the emulated press — so a second finger
    /// landing while the first is held does not click again, and lifting the
    /// second does not release the button.
    fn emulate_mouse_from_touch_event(&mut self, index: u32, position: Vector2, pressed: bool) {
        if !self.emulate_mouse_from_touch {
            return;
        }
        let translate = if pressed {
            if self.mouse_from_touch_index.is_none() {
                self.mouse_from_touch_index = Some(index);
                true
            } else {
                false
            }
        } else if self.mouse_from_touch_index == Some(index) {
            self.mouse_from_touch_index = None;
            true
        } else {
            false
        };
        if !translate {
            return;
        }
        // Applied directly rather than re-entering `process_event`, so an
        // emulated event can never bounce back through `emulate_touch_from_mouse`
        // and loop.
        self.mouse_position = position;
        if pressed {
            if self.mouse_pressed.insert(MouseButton::Left) {
                self.mouse_just_pressed.insert(MouseButton::Left);
            }
        } else if self.mouse_pressed.remove(&MouseButton::Left) {
            self.mouse_just_released.insert(MouseButton::Left);
        }
    }

    /// Synthesizes finger 0 from a left mouse button, mirroring Godot's
    /// `emulate_touch_from_mouse`. The inverse of
    /// [`emulate_mouse_from_touch_event`](Self::emulate_mouse_from_touch_event),
    /// and likewise applied directly to avoid a feedback loop.
    fn emulate_touch_from_mouse_event(
        &mut self,
        button: MouseButton,
        position: Vector2,
        pressed: bool,
    ) {
        if !self.emulate_touch_from_mouse || button != MouseButton::Left {
            return;
        }
        if pressed {
            self.touch_positions.insert(0, position);
            self.touch_last_press_frame.insert(0, self.frame_counter);
        } else {
            self.touch_positions.remove(&0);
        }
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

    /// Moves the cursor to `position`, mirroring Godot's `Input.warp_mouse`
    /// (upstream/godot/core/input/input.cpp). Until this existed, the position
    /// could only change through a `MouseMotion` event, so a script's
    /// `Input.warp_mouse` had nowhere to land (pat-0q2kx.24).
    ///
    /// This updates the engine's notion of where the cursor is; moving the real OS
    /// cursor is the platform backend's job.
    pub fn warp_mouse(&mut self, position: Vector2) {
        self.mouse_position = position;
    }

    // -- Programmatic actions (Input.action_press / action_release) ---------

    /// Presses `action` programmatically with the given strength, mirroring
    /// Godot's `Input.action_press`. Updates just-pressed edge state so
    /// `is_action_just_pressed` reports it for this frame.
    pub fn action_press(&mut self, action: &str, strength: f32) {
        self.action_strengths.insert(action.to_string(), strength);
        // A scripted press has no physical deflection behind it, so raw and
        // remapped strength are the same value (Godot's `Input::action_press`
        // sets both from `p_strength`).
        self.action_raw_strengths
            .insert(action.to_string(), strength);
        if self.actions_pressed.insert(action.to_string()) {
            self.actions_just_pressed.insert(action.to_string());
        }
    }

    /// Releases `action` programmatically (Godot's `Input.action_release`),
    /// updating just-released edge state.
    pub fn action_release(&mut self, action: &str) {
        self.action_strengths.insert(action.to_string(), 0.0);
        self.action_raw_strengths.insert(action.to_string(), 0.0);
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
            action_raw_strengths: self.action_raw_strengths.clone(),
            gamepad_buttons_pressed: self.gamepad_buttons_pressed.clone(),
            gamepad_axis_values: self.gamepad_axis_values.clone(),
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
    action_raw_strengths: HashMap<String, f32>,
    /// Gamepad buttons held, keyed by `(device, button)` (pat-0q2kx.88).
    gamepad_buttons_pressed: HashSet<(u32, GamepadButton)>,
    /// Latest value per `(device, axis)` (pat-0q2kx.88).
    gamepad_axis_values: HashMap<(u32, GamepadAxis), f32>,
}

impl InputSnapshot {
    /// Returns `true` if the given key is held down.
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// Returns `true` if `button` is held on joypad `device`
    /// (`Input.is_joy_button_pressed`).
    ///
    /// Carried on the snapshot rather than read live so a script sees the same
    /// pad state for the whole frame, exactly as it does for keys and actions
    /// (pat-0q2kx.88).
    pub fn is_gamepad_button_pressed(&self, device: u32, button: GamepadButton) -> bool {
        self.gamepad_buttons_pressed.contains(&(device, button))
    }

    /// The value of `axis` on joypad `device`, `0.0` when untouched
    /// (`Input.get_joy_axis`).
    pub fn get_gamepad_axis_value(&self, device: u32, axis: GamepadAxis) -> f32 {
        self.gamepad_axis_values
            .get(&(device, axis))
            .copied()
            .unwrap_or(0.0)
    }

    /// Every `(device, button)` pair currently held, for mirroring into a
    /// script-facing snapshot.
    pub fn pressed_gamepad_buttons(&self) -> impl Iterator<Item = (u32, GamepadButton)> + '_ {
        self.gamepad_buttons_pressed.iter().copied()
    }

    /// Every `(device, axis)` value, for mirroring into a script-facing
    /// snapshot.
    pub fn gamepad_axis_values(&self) -> impl Iterator<Item = ((u32, GamepadAxis), f32)> + '_ {
        self.gamepad_axis_values.iter().map(|(k, v)| (*k, *v))
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

    /// The action's strength **ignoring its deadzone** — Godot
    /// `Input.get_action_raw_strength` (pat-0q2kx.89).
    ///
    /// Differs from [`get_action_strength`](Self::get_action_strength) only for
    /// analog bindings: a stick at 0.6 under a 0.1 deadzone reads 0.56 there and
    /// 0.6 here. Digital bindings report the same 1.0 / 0.0 from both.
    pub fn get_action_raw_strength(&self, name: &str) -> f32 {
        if self.actions_pressed.contains(name) {
            self.action_raw_strengths
                .get(name)
                .copied()
                .or_else(|| self.action_strengths.get(name).copied())
                .unwrap_or(1.0)
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

    /// The engine-resolved set of actions currently pressed (pat-0q2kx.88).
    ///
    /// Mirrored into the script-facing snapshot so an action bound to a
    /// gamepad button resolves; a key-name map cannot express such a binding.
    pub fn pressed_actions(&self) -> HashSet<String> {
        self.actions_pressed.clone()
    }

    /// The actions that became pressed this frame (pat-0q2kx.88).
    pub fn just_pressed_actions(&self) -> HashSet<String> {
        self.actions_just_pressed.clone()
    }

    /// The actions released this frame (pat-0q2kx.88).
    pub fn just_released_actions(&self) -> HashSet<String> {
        self.actions_just_released.clone()
    }

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
        //
        // ⚠️ The default is 0.2, not 0.5. Godot's `InputMap::DEFAULT_DEADZONE`
        // is `0.2f` (`core/input/input_map.h:55`) and
        // `load_from_project_settings` applies it for an action with no
        // `deadzone` key (`input_map.cpp:320`). 0.5 made every stick-bound
        // action in such a project need more than twice the deflection before
        // it registered — a control that feels broken rather than one that
        // obviously is (pat-mgixk.24).
        let deadzone = Self::extract_float(block, "\"deadzone\"")
            .or_else(|| Self::extract_float(block, "deadzone"))
            .unwrap_or(DEFAULT_DEADZONE);

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
                    // Godot serializes the modifier mask as separate
                    // `*_pressed` fields; a binding that carries none stays the
                    // plain `KeyBinding` form (pat-0q2kx.89).
                    let combo = KeyCombo {
                        key,
                        shift: Self::extract_bool(obj_str, "\"shift_pressed\""),
                        // Godot's `meta_pressed` is Command on macOS and the
                        // Windows/Super key elsewhere; Patina's key event has no
                        // Meta bit, so it folds into Ctrl the same way the
                        // builtin `CMD_OR_CTRL` bindings do.
                        ctrl: Self::extract_bool(obj_str, "\"ctrl_pressed\"")
                            || Self::extract_bool(obj_str, "\"meta_pressed\""),
                        alt: Self::extract_bool(obj_str, "\"alt_pressed\""),
                    };
                    let binding = if combo.shift || combo.ctrl || combo.alt {
                        ActionBinding::KeyWithModifiers(combo)
                    } else {
                        ActionBinding::KeyBinding(key)
                    };
                    map.action_add_event(action, binding);
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

        // Extract gamepad bindings (pat-0q2kx.89). Godot serializes these as
        // `Object(InputEventJoypadButton,…,"button_index":0,…)` and
        // `Object(InputEventJoypadMotion,…,"axis":0,"axis_value":-1.0,…)`; the
        // parser previously dropped both, so a controller-bound project action
        // silently had no gamepad binding at all.
        let mut search = block.as_bytes();
        while let Some(pos) = find_substr(search, b"Object(InputEventJoypadButton") {
            let rest = &search[pos..];
            if let Some(end) = find_matching_paren(&rest[6..]).map(|e| e + 6) {
                let obj_str = std::str::from_utf8(&rest[..end + 1]).unwrap_or("");
                if let Some(button) = Self::extract_int(obj_str, "\"button_index\"")
                    .and_then(GamepadButton::from_godot_index)
                {
                    map.action_add_event(action, ActionBinding::GamepadButtonBinding(button));
                }
                search = &rest[end + 1..];
            } else {
                break;
            }
        }

        let mut search = block.as_bytes();
        while let Some(pos) = find_substr(search, b"Object(InputEventJoypadMotion") {
            let rest = &search[pos..];
            if let Some(end) = find_matching_paren(&rest[6..]).map(|e| e + 6) {
                let obj_str = std::str::from_utf8(&rest[..end + 1]).unwrap_or("");
                if let Some(axis) =
                    Self::extract_int(obj_str, "\"axis\"").and_then(GamepadAxis::from_godot_index)
                {
                    // The sign of `axis_value` is the direction the binding
                    // presses on; Godot writes -1.0 / 1.0.
                    let positive = Self::extract_float(obj_str, "\"axis_value\"")
                        .map(|v| v >= 0.0)
                        .unwrap_or(true);
                    map.action_add_event(
                        action,
                        ActionBinding::GamepadAxisDirection { axis, positive },
                    );
                }
                search = &rest[end + 1..];
            } else {
                break;
            }
        }
    }

    /// Reads a `"label":true` flag from a serialized event object, defaulting to
    /// `false` for an absent or `false` field.
    fn extract_bool(s: &str, key: &str) -> bool {
        let Some(pos) = s.find(key) else {
            return false;
        };
        let rest = &s[pos + key.len()..];
        rest.trim_start()
            .trim_start_matches(':')
            .trim_start()
            .starts_with("true")
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

    /// Extracts a Godot keycode and maps it to our [`Key`] enum.
    ///
    /// Godot serializes **both** `keycode` and `physical_keycode` on every
    /// `InputEventKey` and leaves the unused one at `0` (`KEY_NONE`) — a plain
    /// key binding writes `"keycode":65,"physical_keycode":0`, a physical one
    /// the reverse. So a zero means "not this kind of binding" and must not win.
    ///
    /// This previously read `physical_keycode` first *without* the zero check,
    /// so it took the `0` and dropped the binding: every keyboard action in a
    /// real `project.godot` silently vanished (pat-0q2kx.89). Preferring
    /// `keycode` also matches `InputEventKey::action_match`, which tests
    /// `keycode` first and only falls back to `physical_keycode` when the
    /// logical key is `NONE` (`core/input/input_event.cpp`).
    fn extract_keycode(obj_str: &str) -> Option<Key> {
        let non_zero = |label: &str| Self::extract_int(obj_str, label).filter(|c| *c != 0);
        let code = non_zero("\"keycode\"").or_else(|| non_zero("\"physical_keycode\""))?;
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
            6 => Some(MouseButton::WheelLeft),
            7 => Some(MouseButton::WheelRight),
            8 => Some(MouseButton::XButton1),
            9 => Some(MouseButton::XButton2),
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

// -- Public Godot-code mappers (pat-0q2kx.89) ------------------------------
//
// Runtime rebinding from GDScript names controls the way `project.godot` does —
// by Godot's integer codes — so the same mappings the `[input]` parser uses are
// exposed for callers outside this module.

/// Maps a Godot keycode (`@GlobalScope.KEY_*`) to a [`Key`], or `None` for a key
/// Patina does not model.
pub fn key_from_godot_keycode(code: i64) -> Option<Key> {
    godot_keycode_to_key(code)
}

/// Maps a Godot `MouseButton` index to a [`MouseButton`].
pub fn mouse_button_from_godot(index: i64) -> Option<MouseButton> {
    Some(match index {
        1 => MouseButton::Left,
        2 => MouseButton::Right,
        3 => MouseButton::Middle,
        4 => MouseButton::WheelUp,
        5 => MouseButton::WheelDown,
        6 => MouseButton::WheelLeft,
        7 => MouseButton::WheelRight,
        8 => MouseButton::XButton1,
        9 => MouseButton::XButton2,
        _ => return None,
    })
}

/// Maps a Godot `JoyButton` index to a [`GamepadButton`].
pub fn gamepad_button_from_godot(index: i64) -> Option<GamepadButton> {
    GamepadButton::from_godot_index(index)
}

/// Maps a Godot `JoyAxis` index to a [`GamepadAxis`].
pub fn gamepad_axis_from_godot(index: i64) -> Option<GamepadAxis> {
    GamepadAxis::from_godot_index(index)
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
        // Godot `Key::QUOTELEFT = 0x0060` and `Key::MENU = SPECIAL | 0x42`
        // (core/os/keyboard.h), where SPECIAL is `1 << 22` (pat-0q2kx.125).
        96 => Some(Key::QuoteLeft),
        4194370 => Some(Key::Menu),
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
                            "WheelLeft" | "wheel_left" => Some(MouseButton::WheelLeft),
                            "WheelRight" | "wheel_right" => Some(MouseButton::WheelRight),
                            "XButton1" | "xbutton1" => Some(MouseButton::XButton1),
                            "XButton2" | "xbutton2" => Some(MouseButton::XButton2),
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
            double_tap: false,
            canceled: false,
        };
        if let InputEvent::ScreenTouch {
            index,
            position,
            pressed,
            ..
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
            pressure: 0.0,
            tilt: Vector2::ZERO,
        };
        if let InputEvent::ScreenDrag {
            index,
            position,
            relative,
            velocity,
            ..
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

    // -- pat-0q2kx.93: touch pipeline + pointer emulation -------------------

    fn touch(index: u32, x: f32, y: f32, pressed: bool) -> InputEvent {
        InputEvent::ScreenTouch {
            index,
            position: Vector2::new(x, y),
            pressed,
            double_tap: false,
            canceled: false,
        }
    }

    fn drag(index: u32, x: f32, y: f32) -> InputEvent {
        InputEvent::ScreenDrag {
            index,
            position: Vector2::new(x, y),
            relative: Vector2::new(1.0, 1.0),
            velocity: Vector2::ZERO,
            pressure: 0.0,
            tilt: Vector2::ZERO,
        }
    }

    /// `process_event` used to explicitly no-op both touch variants, so no
    /// finger state existed anywhere in the engine.
    #[test]
    fn touch_press_release_tracks_finger_state() {
        let mut state = InputState::new();
        assert_eq!(state.touch_count(), 0);

        state.process_event(touch(0, 10.0, 20.0, true));
        assert!(state.is_touch_pressed(0));
        assert_eq!(state.touch_position(0), Some(Vector2::new(10.0, 20.0)));
        assert_eq!(state.touch_count(), 1);

        // A drag moves the finger without changing its pressed state.
        state.process_event(drag(0, 30.0, 40.0));
        assert_eq!(state.touch_position(0), Some(Vector2::new(30.0, 40.0)));
        assert!(state.is_touch_pressed(0));

        state.process_event(touch(0, 30.0, 40.0, false));
        assert!(!state.is_touch_pressed(0));
        assert_eq!(state.touch_position(0), None, "released fingers are erased");
        assert_eq!(state.touch_count(), 0);
    }

    /// Multiple fingers are tracked independently.
    #[test]
    fn multiple_fingers_tracked_independently() {
        let mut state = InputState::new();
        state.process_event(touch(0, 1.0, 1.0, true));
        state.process_event(touch(1, 2.0, 2.0, true));
        assert_eq!(state.touch_count(), 2);

        state.process_event(touch(0, 1.0, 1.0, false));
        assert!(!state.is_touch_pressed(0));
        assert!(state.is_touch_pressed(1), "finger 1 is unaffected");
        assert_eq!(state.touch_count(), 1);
    }

    /// The headline emulation clause: with `emulate_mouse_from_touch` ON (the
    /// Godot default) a touch presses the left mouse button, which is what lets
    /// a touch operate a Button.
    #[test]
    fn touch_emulates_left_mouse_button_by_default() {
        let mut state = InputState::new();
        assert!(
            state.is_emulating_mouse_from_touch(),
            "Godot's project default is ON"
        );

        state.process_event(touch(0, 100.0, 50.0, true));
        assert!(
            state.is_mouse_button_pressed(MouseButton::Left),
            "a touch must synthesize a left mouse press"
        );
        assert!(state.is_mouse_button_just_pressed(MouseButton::Left));
        assert_eq!(state.get_mouse_position(), Vector2::new(100.0, 50.0));

        state.process_event(touch(0, 100.0, 50.0, false));
        assert!(!state.is_mouse_button_pressed(MouseButton::Left));
        assert!(state.is_mouse_button_just_released(MouseButton::Left));
    }

    /// Only the *first* finger drives the emulated mouse
    /// (Godot's `mouse_from_touch_index`): a second finger must not re-click,
    /// and lifting it must not release the button.
    #[test]
    fn only_the_first_finger_drives_the_emulated_mouse() {
        let mut state = InputState::new();
        state.process_event(touch(0, 10.0, 10.0, true));
        state.flush_frame();

        state.process_event(touch(1, 90.0, 90.0, true));
        assert!(
            !state.is_mouse_button_just_pressed(MouseButton::Left),
            "a second finger must not press the mouse again"
        );
        assert_eq!(
            state.get_mouse_position(),
            Vector2::new(10.0, 10.0),
            "the cursor stays with the first finger"
        );

        state.process_event(touch(1, 90.0, 90.0, false));
        assert!(
            state.is_mouse_button_pressed(MouseButton::Left),
            "lifting the second finger must not release the button"
        );

        state.process_event(touch(0, 10.0, 10.0, false));
        assert!(
            !state.is_mouse_button_pressed(MouseButton::Left),
            "lifting the driving finger releases it"
        );
    }

    /// A drag of the driving finger moves the emulated cursor; a drag of any
    /// other finger does not.
    #[test]
    fn drag_moves_the_emulated_cursor_only_for_the_driving_finger() {
        let mut state = InputState::new();
        state.process_event(touch(0, 10.0, 10.0, true));
        state.process_event(touch(1, 50.0, 50.0, true));

        state.process_event(drag(1, 70.0, 70.0));
        assert_eq!(
            state.get_mouse_position(),
            Vector2::new(10.0, 10.0),
            "a non-driving finger must not move the cursor"
        );

        state.process_event(drag(0, 25.0, 25.0));
        assert_eq!(state.get_mouse_position(), Vector2::new(25.0, 25.0));
    }

    /// Turning the setting off suppresses the synthesis entirely.
    #[test]
    fn emulate_mouse_from_touch_can_be_disabled() {
        let mut state = InputState::new();
        state.set_emulate_mouse_from_touch(false);

        state.process_event(touch(0, 10.0, 10.0, true));
        assert!(!state.is_mouse_button_pressed(MouseButton::Left));
        assert!(
            state.is_touch_pressed(0),
            "the touch itself still registers"
        );
    }

    /// The reverse direction, off by default.
    #[test]
    fn mouse_emulates_touch_only_when_enabled() {
        let mut state = InputState::new();
        assert!(!state.is_emulating_touch_from_mouse(), "OFF by default");

        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(5.0, 5.0),
        });
        assert!(!state.is_touch_pressed(0), "no touch without the setting");

        let mut state = InputState::new();
        state.set_emulate_touch_from_mouse(true);
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(5.0, 5.0),
        });
        assert!(state.is_touch_pressed(0), "left click lands as finger 0");
        assert_eq!(state.touch_position(0), Some(Vector2::new(5.0, 5.0)));

        // Right button is not a finger.
        state.process_event(InputEvent::MouseButton {
            button: MouseButton::Right,
            pressed: true,
            position: Vector2::new(6.0, 6.0),
        });
        assert_eq!(
            state.touch_count(),
            1,
            "only the left button emulates touch"
        );
    }

    /// Emulation must not feed back on itself: with **both** directions on, one
    /// touch produces one mouse press and one finger, not an unbounded bounce.
    #[test]
    fn emulation_does_not_loop_when_both_directions_are_on() {
        let mut state = InputState::new();
        state.set_emulate_mouse_from_touch(true);
        state.set_emulate_touch_from_mouse(true);

        state.process_event(touch(0, 10.0, 10.0, true));
        assert!(state.is_mouse_button_pressed(MouseButton::Left));
        assert_eq!(state.touch_count(), 1);
    }

    /// A quick second tap of the same finger is a double tap; a slow one is not.
    #[test]
    fn double_tap_window_is_frame_bounded() {
        let mut state = InputState::new();
        assert!(
            !state.would_be_double_tap(0),
            "a first-ever tap cannot be a double tap"
        );

        state.process_event(touch(0, 10.0, 10.0, true));
        state.process_event(touch(0, 10.0, 10.0, false));
        state.flush_frame();
        assert!(
            state.would_be_double_tap(0),
            "a tap one frame later is within the window"
        );

        // Walk past the window.
        for _ in 0..=DOUBLE_TAP_FRAMES {
            state.flush_frame();
        }
        assert!(
            !state.would_be_double_tap(0),
            "past DOUBLE_TAP_FRAMES it is a fresh tap"
        );
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
        assert_eq!(
            GamepadButton::from_standard(12),
            Some(GamepadButton::DPadUp)
        );
        assert_eq!(
            GamepadButton::from_standard(15),
            Some(GamepadButton::DPadRight)
        );
        assert_eq!(GamepadButton::from_standard(16), Some(GamepadButton::Guide));
        assert_eq!(GamepadButton::from_standard(17), None);
        assert_eq!(GamepadAxis::from_standard(0), Some(GamepadAxis::LeftStickX));
        assert_eq!(
            GamepadAxis::from_standard(3),
            Some(GamepadAxis::RightStickY)
        );
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
        assert!(
            (len - 1.0).abs() < 0.001,
            "diagonal normalized, got len={len}"
        );
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

    /// pat-mgixk.24 — an action with no `deadzone` key takes Godot's
    /// `DEFAULT_DEADZONE`, which is **0.2**.
    ///
    /// Ground truth: `core/input/input_map.h:55`
    /// (`static constexpr float DEFAULT_DEADZONE = 0.2f`) and
    /// `input_map.cpp:320`
    /// (`action.has("deadzone") ? (float)action["deadzone"] : DEFAULT_DEADZONE`).
    ///
    /// 🔑 The parser defaulted to 0.5, so every stick-bound action in a project
    /// that omits the key needed more than TWICE the deflection before it
    /// registered. The declared case is asserted beside it, so a fix that
    /// simply hard-coded 0.2 for everything would fail too.
    #[test]
    fn project_action_without_a_deadzone_takes_godots_default() {
        let content = r#"
[input]
fire={
"events": [Object(InputEventJoypadButton,"resource_local_to_scene":false,"resource_name":"","device":-1,"button_index":0,"pressure":0.0,"pressed":false)]
}
aim={
"deadzone": 0.75,
"events": [Object(InputEventJoypadMotion,"resource_local_to_scene":false,"resource_name":"","device":-1,"axis":0,"axis_value":1.0)]
}
"#;
        let map = InputMap::load_from_project_godot(content);
        assert_eq!(
            map.get_deadzone("fire"),
            0.2,
            "an action that omits `deadzone` must take DEFAULT_DEADZONE (0.2), \
             not 0.5 — half the stick travel is the difference"
        );
        assert_eq!(
            map.get_deadzone("aim"),
            0.75,
            "a declared deadzone is still honoured"
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
        // `load_from_project_godot` mirrors Godot's
        // `InputMap::load_from_project_settings`, which installs the builtin
        // `ui_*` set *before* applying the project's `[input]` section — so the
        // map is never just the project's own actions. What the project
        // declares is the only NON-builtin action (pat-0q2kx.89; this used to
        // assert a bare count of 1, which the builtin install already broke).
        let project_actions: Vec<String> = map
            .actions()
            .filter(|a| !a.starts_with("ui_"))
            .cloned()
            .collect();
        assert_eq!(project_actions, vec!["jump".to_string()]);
        assert!(
            map.has_action("ui_accept"),
            "the Godot builtins come along with any project load"
        );
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
