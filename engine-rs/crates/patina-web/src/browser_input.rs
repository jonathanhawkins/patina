//! Browser event → engine input translation (pat-s3rrv.41).
//!
//! The browser hands a game DOM events in CSS pixels with its own key names,
//! wheel units and gamepad layout. Godot's `InputState` wants
//! [`gdplatform::input::InputEvent`] in project pixels with physical keycodes.
//! Nothing bridged the two: `gdrender2d`'s `BrowserInputEvent` covers only
//! `Key` and belongs to the dev HTTP server, and `Key::from_name` — which
//! already accepts browser `KeyboardEvent.code` spellings like `"ArrowRight"` —
//! had **zero callers** anywhere in the workspace.
//!
//! Everything here is plain data in and plain data out, with no `web_sys` types,
//! so the whole translation is testable on the host. The browser listeners are a
//! thin adapter that reads fields off the DOM event and calls these.

use gdcore::math::Vector2;
use gdplatform::input::{GamepadAxis, InputEvent, Key, MouseButton};

/// How a canvas maps CSS pixels to project pixels.
///
/// A browser reports pointer coordinates relative to the viewport in CSS
/// pixels; the engine wants them relative to the canvas in project pixels. On a
/// HiDPI display those differ by `device_pixel_ratio`, and the canvas is rarely
/// at the window origin — ignoring either is why a click lands somewhere other
/// than where the cursor is.
#[derive(Debug, Clone, Copy)]
pub struct CanvasMapping {
    /// Canvas bounding-box origin in CSS pixels (`getBoundingClientRect`).
    pub origin_css: Vector2,
    /// `window.devicePixelRatio`.
    pub device_pixel_ratio: f32,
}

impl Default for CanvasMapping {
    fn default() -> Self {
        Self {
            origin_css: Vector2::new(0.0, 0.0),
            device_pixel_ratio: 1.0,
        }
    }
}

impl CanvasMapping {
    /// Converts a viewport-relative CSS point to a canvas-local project pixel.
    pub fn to_project(&self, client_css: Vector2) -> Vector2 {
        let dpr = if self.device_pixel_ratio > 0.0 {
            self.device_pixel_ratio
        } else {
            1.0
        };
        Vector2::new(
            (client_css.x - self.origin_css.x) * dpr,
            (client_css.y - self.origin_css.y) * dpr,
        )
    }
}

/// `WheelEvent.deltaMode`, which decides what `deltaY` is counted in.
///
/// A browser reports the same physical scroll as ~100 in PIXEL mode, 3 in LINE
/// mode and 1 in PAGE mode. Treating the raw number as a magnitude — as any code
/// that ignores this enum must — makes a line-mode mouse scroll ~33× weaker than
/// a pixel-mode trackpad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDeltaMode {
    /// `DOM_DELTA_PIXEL` (0).
    Pixel,
    /// `DOM_DELTA_LINE` (1).
    Line,
    /// `DOM_DELTA_PAGE` (2).
    Page,
}

impl WheelDeltaMode {
    /// From the numeric `deltaMode`; anything unexpected is treated as pixels.
    pub fn from_dom(mode: u32) -> Self {
        match mode {
            1 => Self::Line,
            2 => Self::Page,
            _ => Self::Pixel,
        }
    }

    /// CSS pixels one unit in this mode represents.
    ///
    /// 16 px/line and 800 px/page match the conventional browser defaults; the
    /// exact values matter less than that the three modes end up comparable.
    pub fn pixels_per_unit(self) -> f32 {
        match self {
            Self::Pixel => 1.0,
            Self::Line => 16.0,
            Self::Page => 800.0,
        }
    }
}

/// Normalizes a `KeyboardEvent.code` to the spelling [`Key::from_name`] expects.
///
/// The DOM spells a letter `"KeyA"` and a digit `"Digit1"`, while `from_name`
/// takes the bare `"A"` / `"1"`. It DOES already accept the arrow and named keys
/// verbatim (`"ArrowRight"`, `"Space"`, `"Enter"`), so only the two prefixed
/// families need stripping — which is exactly why this was easy to miss: a
/// translator tested only on `"ArrowRight"` looks correct while dropping every
/// letter, i.e. WASD movement silently does nothing.
///
/// `Digit1` and `Numpad1` are deliberately NOT merged: they are distinct
/// physical keys and Godot distinguishes them.
fn normalize_key_code(code: &str) -> &str {
    if let Some(rest) = code.strip_prefix("Key") {
        // "KeyA" -> "A", but never turn a bare "Key" into "".
        if !rest.is_empty() {
            return rest;
        }
    }
    if let Some(rest) = code.strip_prefix("Digit") {
        if !rest.is_empty() {
            return rest;
        }
    }
    code
}

/// Translates a `keydown`/`keyup`.
///
/// `code` is the PHYSICAL key (`KeyboardEvent.code`), so a game's WASD keeps
/// working on an AZERTY layout — which is why Godot keys off physical scancodes
/// too. Returns `None` for a code the engine has no mapping for, so an unknown
/// key is dropped rather than mapped to something arbitrary.
pub fn key_event(
    code: &str,
    pressed: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
) -> Option<InputEvent> {
    Key::from_name(normalize_key_code(code)).map(|key| InputEvent::Key {
        key,
        pressed,
        shift,
        ctrl,
        alt,
    })
}

/// Translates a `pointermove`/`mousemove`.
pub fn pointer_move(
    client_css: Vector2,
    movement_css: Vector2,
    mapping: CanvasMapping,
) -> InputEvent {
    let dpr = if mapping.device_pixel_ratio > 0.0 {
        mapping.device_pixel_ratio
    } else {
        1.0
    };
    InputEvent::MouseMotion {
        position: mapping.to_project(client_css),
        // `movementX/Y` is already a delta, so it scales but does not translate.
        relative: Vector2::new(movement_css.x * dpr, movement_css.y * dpr),
    }
}

/// Translates a `pointerdown`/`pointerup`.
///
/// `button` is the DOM numbering: 0 left, 1 middle, 2 right.
pub fn pointer_button(
    button: u16,
    pressed: bool,
    client_css: Vector2,
    mapping: CanvasMapping,
) -> Option<InputEvent> {
    let button = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => return None,
    };
    Some(InputEvent::MouseButton {
        button,
        pressed,
        position: mapping.to_project(client_css),
    })
}

/// Translates a `wheel` into Godot's wheel-button press/release pair.
///
/// Godot models scrolling as `MOUSE_BUTTON_WHEEL_UP`/`_DOWN` rather than an
/// axis, and a wheel button is reported pressed and immediately released — so
/// this returns both events. `delta_y` is normalized through `deltaMode` first,
/// which is what makes a line-mode mouse and a pixel-mode trackpad comparable.
///
/// Returns an empty vec for a zero delta rather than a spurious click.
pub fn wheel(
    delta_y: f32,
    mode: WheelDeltaMode,
    client_css: Vector2,
    mapping: CanvasMapping,
) -> Vec<InputEvent> {
    let pixels = delta_y * mode.pixels_per_unit();
    if pixels == 0.0 {
        return Vec::new();
    }
    // The DOM's +Y is downward, which is a scroll DOWN.
    let button = if pixels > 0.0 {
        MouseButton::WheelDown
    } else {
        MouseButton::WheelUp
    };
    let position = mapping.to_project(client_css);
    vec![
        InputEvent::MouseButton {
            button,
            pressed: true,
            position,
        },
        InputEvent::MouseButton {
            button,
            pressed: false,
            position,
        },
    ]
}

/// Translates one axis of a `Gamepad` poll.
///
/// The Gamepad API reports axes already in -1..=1 with the same index order
/// Godot uses for the sticks, so this is a pass-through plus a deadzone. The
/// deadzone is applied here because the browser does not apply one and a
/// resting stick reports small non-zero values forever.
pub fn gamepad_axis(index: u32, value: f32, gamepad_id: u32, deadzone: f32) -> Option<InputEvent> {
    let axis = match index {
        0 => GamepadAxis::LeftStickX,
        1 => GamepadAxis::LeftStickY,
        2 => GamepadAxis::RightStickX,
        3 => GamepadAxis::RightStickY,
        _ => return None,
    };
    let value = if value.abs() < deadzone { 0.0 } else { value };
    Some(InputEvent::GamepadAxis {
        axis,
        value,
        gamepad_id,
    })
}
