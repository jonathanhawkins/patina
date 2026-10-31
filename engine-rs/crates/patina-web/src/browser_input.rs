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

use gdcore::math::{Transform2D, Vector2};
use gdplatform::input::{GamepadAxis, GamepadButton, InputEvent, Key, MouseButton};

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
    /// Canvas (backing) pixels -> project coordinates: the inverse of the
    /// stretch transform the renderer applies (pat-mgixk.74).
    ///
    /// 🔑 Without this, [`Self::to_project`] stopped at canvas pixels despite
    /// its name. Under any stretch mode those are NOT project coordinates, so
    /// on a 320x180 project shown at 640x480 every click landed at double the
    /// intended position and anything inside a letterbox bar mapped onto real
    /// content.
    ///
    /// ⚠️ Defaults to IDENTITY, so a project that does not stretch maps exactly
    /// as it did before this field existed.
    pub canvas_to_project: Transform2D,
}

impl Default for CanvasMapping {
    fn default() -> Self {
        Self {
            origin_css: Vector2::new(0.0, 0.0),
            device_pixel_ratio: 1.0,
            canvas_to_project: Transform2D::IDENTITY,
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
        let canvas_px = Vector2::new(
            (client_css.x - self.origin_css.x) * dpr,
            (client_css.y - self.origin_css.y) * dpr,
        );
        // ...and the rest of the way into project space (pat-mgixk.74).
        // IDENTITY when the project does not stretch, so this is a no-op there.
        self.canvas_to_project.xform(canvas_px)
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

// ---------------------------------------------------------------------------
// Touch (pat-u3l8c.111)
// ---------------------------------------------------------------------------

/// Per-finger bookkeeping a `touchmove` needs and the DOM does not provide.
///
/// A `TouchEvent` reports only where each finger *is*. Godot's
/// `InputEventScreenDrag` also carries `relative` (how far it moved since the
/// last report) and `velocity` (px/sec), so a drag has to be differenced
/// against the previous sample — per finger, because two fingers interleave in
/// one event stream and sharing one "last position" would make a two-finger
/// pinch report each finger jumping to the other's place.
///
/// Target-independent on purpose: the browser listener reads `Touch.clientX`
/// and `Touch.identifier` off the DOM event and calls these, so the whole
/// drag/velocity derivation is exercised by a host test.
#[derive(Debug, Default, Clone)]
pub struct TouchTracker {
    /// Live fingers. A `Vec` rather than a map: a touchscreen reports at most a
    /// handful of simultaneous fingers, so a linear scan beats hashing, and
    /// the order is the order they went down.
    fingers: Vec<Finger>,
}

#[derive(Debug, Clone, Copy)]
struct Finger {
    /// The browser's `Touch.identifier`, as handed to the engine.
    index: u32,
    /// Last reported position, in project pixels.
    position: Vector2,
    /// Timestamp of that report, in milliseconds.
    at_ms: f64,
}

impl TouchTracker {
    /// A tracker with no fingers down.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fingers currently down.
    pub fn len(&self) -> usize {
        self.fingers.len()
    }

    /// Whether no finger is down.
    pub fn is_empty(&self) -> bool {
        self.fingers.is_empty()
    }

    fn slot(&self, index: u32) -> Option<usize> {
        self.fingers.iter().position(|f| f.index == index)
    }

    /// Translates a `touchstart` for one finger.
    ///
    /// `double_tap` is left `false`: Godot computes it from the gap between
    /// taps, and in this engine that is [`gdplatform::input::InputState`]'s job
    /// (see `InputEvent::ScreenTouch::double_tap`), so deciding it here would
    /// either duplicate or fight that.
    pub fn touch_begin(
        &mut self,
        index: u32,
        client_css: Vector2,
        mapping: CanvasMapping,
        now_ms: f64,
    ) -> InputEvent {
        let position = mapping.to_project(client_css);
        let finger = Finger {
            index,
            position,
            at_ms: now_ms,
        };
        match self.slot(index) {
            // A second `touchstart` for a live identifier should not stack a
            // duplicate finger that then never lifts.
            Some(i) => self.fingers[i] = finger,
            None => self.fingers.push(finger),
        }
        InputEvent::ScreenTouch {
            index,
            position,
            pressed: true,
            double_tap: false,
            canceled: false,
        }
    }

    /// Translates a `touchmove` for one finger.
    ///
    /// Returns `None` for a finger that is not down — a `touchmove` whose
    /// `touchstart` was never seen (the page loaded with a finger already on
    /// the glass, or the listener was attached mid-gesture) has no previous
    /// sample, so its `relative` would be the whole screen.
    ///
    /// `force` is the DOM `Touch.force` in `0.0..=1.0`, which maps onto Godot's
    /// `InputEventScreenDrag.pressure`. `tilt` is zero: the Touch API reports
    /// none, and a stylus that does arrives as a `PointerEvent` instead.
    pub fn touch_move(
        &mut self,
        index: u32,
        client_css: Vector2,
        force: f32,
        mapping: CanvasMapping,
        now_ms: f64,
    ) -> Option<InputEvent> {
        let slot = self.slot(index)?;
        let position = mapping.to_project(client_css);
        let previous = self.fingers[slot];
        let relative = Vector2::new(
            position.x - previous.position.x,
            position.y - previous.position.y,
        );
        // Velocity is px/sec, so a non-advancing or backwards clock has to
        // yield zero rather than an infinity or a negative speed.
        let dt_secs = (now_ms - previous.at_ms) / 1000.0;
        let velocity = if dt_secs > 0.0 {
            let dt = dt_secs as f32;
            Vector2::new(relative.x / dt, relative.y / dt)
        } else {
            Vector2::ZERO
        };
        self.fingers[slot] = Finger {
            index,
            position,
            at_ms: now_ms,
        };
        Some(InputEvent::ScreenDrag {
            index,
            position,
            relative,
            velocity,
            pressure: force.clamp(0.0, 1.0),
            tilt: Vector2::ZERO,
        })
    }

    /// Translates a `touchend` (`canceled = false`) or `touchcancel`
    /// (`canceled = true`) for one finger.
    ///
    /// Always emits: a release that is dropped because the finger was not
    /// tracked leaves a script's `_input` believing the touch is still down.
    /// Godot distinguishes the two because a canceled touch should not be read
    /// as a tap (`InputEventScreenTouch.canceled`).
    pub fn touch_end(
        &mut self,
        index: u32,
        client_css: Vector2,
        canceled: bool,
        mapping: CanvasMapping,
    ) -> InputEvent {
        if let Some(slot) = self.slot(index) {
            self.fingers.remove(slot);
        }
        InputEvent::ScreenTouch {
            index,
            position: mapping.to_project(client_css),
            pressed: false,
            double_tap: false,
            canceled,
        }
    }

    /// Releases every finger — for a `blur`/visibility change, after which the
    /// browser stops reporting the gesture and no `touchend` ever arrives.
    pub fn release_all(&mut self) -> Vec<InputEvent> {
        self.fingers
            .drain(..)
            .map(|f| InputEvent::ScreenTouch {
                index: f.index,
                position: f.position,
                pressed: false,
                double_tap: false,
                canceled: true,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Gamepad polling (pat-u3l8c.111)
// ---------------------------------------------------------------------------

/// Floor below which a resting axis is reported as exactly zero.
///
/// **Not a Godot constant.** Godot keeps deadzones per *action* in `InputMap`,
/// and `Input.get_joy_axis` reports the raw value. This floor exists for a
/// different reason: `navigator.getGamepads()` is polled, not evented, and a
/// stick at rest reports small drifting values forever, so without a floor
/// every frame would emit a fresh axis event for a controller nobody is
/// touching. Pinning rest to exactly `0.0` is what makes the
/// change-detection in [`GamepadTracker::poll`] settle.
pub const GAMEPAD_AXIS_JITTER_FLOOR: f32 = 0.08;

/// One button as `navigator.getGamepads()` reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GamepadButtonSample {
    /// `GamepadButton.pressed`.
    pub pressed: bool,
    /// `GamepadButton.value`, the analog travel in `0.0..=1.0`. Only the
    /// triggers are really analog, but the browser reports it for every button.
    pub value: f32,
}

/// One pad as `navigator.getGamepads()` reports it, in DOM order.
#[derive(Debug, Clone, PartialEq)]
pub struct GamepadSnapshot {
    /// `Gamepad.index`, which is the slot the engine uses as `gamepad_id`.
    pub id: u32,
    /// Whether `Gamepad.mapping == "standard"`.
    ///
    /// Godot's web build only trusts a standard-mapped pad, because the button
    /// and axis *indices* are what carry the meaning — on a non-standard pad
    /// index 0 is whatever the driver put there, so binding it to "A" is worse
    /// than ignoring the pad.
    pub standard_mapping: bool,
    /// `Gamepad.buttons`, in W3C standard-mapping order.
    pub buttons: Vec<GamepadButtonSample>,
    /// `Gamepad.axes`, in W3C standard-mapping order.
    pub axes: Vec<f32>,
}

/// Previous poll for one pad, so the next one can be differenced.
#[derive(Debug, Clone)]
struct PadState {
    id: u32,
    /// Last emitted pressed-state per standard button index.
    buttons: Vec<bool>,
    /// Last emitted value per engine axis, keyed by the axis itself so the two
    /// trigger axes (which come from `buttons[6]`/`buttons[7]`, not `axes[]`)
    /// share one table with the sticks.
    axes: Vec<(GamepadAxis, f32)>,
}

/// Turns the polled Gamepad API into edge-triggered engine events.
///
/// The Gamepad API has no events for button or axis state — a page must read
/// `navigator.getGamepads()` every frame and work out what changed. Feeding the
/// raw poll to the engine would push a full set of events 60 times a second for
/// an untouched controller; `Input.is_joy_button_pressed` would still read
/// correctly, but `_input` would be called with a held button on every single
/// frame, which is not what a Godot script is written against.
///
/// So this holds the previous poll and emits only transitions. It is
/// target-independent — the browser listener's whole job is to copy
/// `Gamepad.buttons`/`axes` into [`GamepadSnapshot`]s.
#[derive(Debug, Clone)]
pub struct GamepadTracker {
    pads: Vec<PadState>,
    jitter_floor: f32,
}

// Written out rather than derived: a derived `Default` would leave
// `jitter_floor` at 0.0, i.e. no jitter suppression at all, and the whole
// point of the field is that it is not zero.
impl Default for GamepadTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl GamepadTracker {
    /// A tracker with no pads seen yet, using [`GAMEPAD_AXIS_JITTER_FLOOR`].
    pub fn new() -> Self {
        Self {
            pads: Vec::new(),
            jitter_floor: GAMEPAD_AXIS_JITTER_FLOOR,
        }
    }

    /// A tracker with an explicit axis jitter floor.
    pub fn with_jitter_floor(floor: f32) -> Self {
        Self {
            pads: Vec::new(),
            jitter_floor: floor.clamp(0.0, 1.0),
        }
    }

    /// Pads currently being tracked.
    pub fn len(&self) -> usize {
        self.pads.len()
    }

    /// Whether no pad is being tracked.
    pub fn is_empty(&self) -> bool {
        self.pads.is_empty()
    }

    fn floor(&self, value: f32) -> f32 {
        if value.abs() < self.jitter_floor {
            0.0
        } else {
            value
        }
    }

    /// Differences one poll against the last and returns the events for it.
    ///
    /// A pad that was present last poll and is absent now has been
    /// disconnected: every button it held is released and every deflected axis
    /// is zeroed, because otherwise a controller unplugged mid-jump leaves the
    /// jump button pressed forever.
    pub fn poll(&mut self, snapshot: &[GamepadSnapshot]) -> Vec<InputEvent> {
        let mut events = Vec::new();

        // Disconnects first, so a slot that was reused by a different pad in
        // the same poll releases the old one's buttons before pressing the
        // new one's.
        let present: Vec<u32> = snapshot
            .iter()
            .filter(|p| p.standard_mapping)
            .map(|p| p.id)
            .collect();
        let mut dropped = Vec::new();
        for (slot, pad) in self.pads.iter().enumerate() {
            if !present.contains(&pad.id) {
                dropped.push(slot);
            }
        }
        for slot in dropped.iter().rev() {
            let pad = self.pads.remove(*slot);
            events.extend(Self::release_pad(&pad));
        }

        for pad in snapshot.iter().filter(|p| p.standard_mapping) {
            let slot = match self.pads.iter().position(|p| p.id == pad.id) {
                Some(slot) => slot,
                None => {
                    self.pads.push(PadState {
                        id: pad.id,
                        buttons: Vec::new(),
                        axes: Vec::new(),
                    });
                    self.pads.len() - 1
                }
            };

            for (index, sample) in pad.buttons.iter().enumerate() {
                let Some(button) = GamepadButton::from_standard(index) else {
                    // Outside the standard layout: no meaning to bind to.
                    continue;
                };
                // The W3C standard layout puts the triggers at buttons[6] and
                // buttons[7], but Godot models them as the analog axes
                // JOY_AXIS_TRIGGER_LEFT/_RIGHT — `GamepadButton::to_godot_index`
                // returns None for them precisely because they are not
                // JoyButtons. Emitting a press here would be dropped on the
                // way to a script, so the analog travel becomes an axis event.
                let as_axis = match button {
                    GamepadButton::LeftTrigger => Some(GamepadAxis::LeftTriggerAnalog),
                    GamepadButton::RightTrigger => Some(GamepadAxis::RightTriggerAnalog),
                    _ => None,
                };
                if let Some(axis) = as_axis {
                    let value = self.floor(sample.value.clamp(0.0, 1.0));
                    if let Some(event) = Self::axis_event(&mut self.pads[slot], axis, value) {
                        events.push(event);
                    }
                    continue;
                }

                let pressed = sample.pressed;
                let pad_state = &mut self.pads[slot];
                if pad_state.buttons.len() <= index {
                    pad_state.buttons.resize(index + 1, false);
                }
                if pad_state.buttons[index] == pressed {
                    continue;
                }
                pad_state.buttons[index] = pressed;
                events.push(InputEvent::GamepadButton {
                    button,
                    pressed,
                    gamepad_id: pad.id,
                });
            }

            for (index, raw) in pad.axes.iter().enumerate() {
                let Some(axis) = GamepadAxis::from_standard(index) else {
                    continue;
                };
                let value = self.floor(raw.clamp(-1.0, 1.0));
                if let Some(event) = Self::axis_event(&mut self.pads[slot], axis, value) {
                    events.push(event);
                }
            }
        }

        events
    }

    /// Records `value` for `axis` and returns the event if it changed.
    fn axis_event(pad: &mut PadState, axis: GamepadAxis, value: f32) -> Option<InputEvent> {
        let id = pad.id;
        match pad.axes.iter_mut().find(|(a, _)| *a == axis) {
            Some(entry) => {
                if entry.1 == value {
                    return None;
                }
                entry.1 = value;
            }
            None => {
                // First sight of this axis. A pad that reports a resting stick
                // as exactly 0.0 should not emit a "moved to zero" event the
                // frame it connects.
                pad.axes.push((axis, value));
                if value == 0.0 {
                    return None;
                }
            }
        }
        Some(InputEvent::GamepadAxis {
            axis,
            value,
            gamepad_id: id,
        })
    }

    /// Release events for every button held and axis deflected on `pad`.
    fn release_pad(pad: &PadState) -> Vec<InputEvent> {
        let mut events = Vec::new();
        for (index, held) in pad.buttons.iter().enumerate() {
            if !held {
                continue;
            }
            if let Some(button) = GamepadButton::from_standard(index) {
                events.push(InputEvent::GamepadButton {
                    button,
                    pressed: false,
                    gamepad_id: pad.id,
                });
            }
        }
        for (axis, value) in &pad.axes {
            if *value != 0.0 {
                events.push(InputEvent::GamepadAxis {
                    axis: *axis,
                    value: 0.0,
                    gamepad_id: pad.id,
                });
            }
        }
        events
    }

    /// Releases everything on every pad, without waiting for a poll that shows
    /// them gone — for a visibility change, where the browser stops delivering
    /// gamepad state while the tab is hidden.
    pub fn release_all(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();
        for pad in &self.pads {
            events.extend(Self::release_pad(pad));
        }
        self.pads.clear();
        events
    }
}
