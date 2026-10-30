//! The browser end of the web frame loop and the input listeners
//! (pat-u3l8c.111).
//!
//! Before this module, the two halves of the web runtime existed but nothing
//! joined them: [`crate::engine::WebEngine`] could advance a `MainLoop` from a
//! millisecond timestamp (pat-s3rrv.40) and [`crate::browser_input`] could
//! translate DOM event fields into `InputEvent`s (pat-s3rrv.41), but no
//! `requestAnimationFrame` was ever requested and no `addEventListener` was
//! ever called — `grep -c request_animation_frame src/` was 0. The shipped
//! wasm module installed a panic hook and stopped there.
//!
//! ⚠️ **This module holds no logic.** Every decision — frame pacing, delta
//! carry, stall clamping, hidden-tab throttling, key/pointer/wheel/touch
//! translation, gamepad edge detection, fps averaging — lives in
//! [`crate::engine`], [`crate::browser_input`] and [`crate::FpsMeter`], all of
//! which are target-independent and covered by native tests. That split is
//! deliberate and is the same rule `gdplatform::fetch_web` states: code behind
//! `cfg(target_arch = "wasm32")` is never type-checked by a native build and
//! never *run* by any build in this repo, so anything subtle placed here would
//! be unprovable. What is left here is reading fields off DOM objects.
//!
//! Verify this file with `scripts/check-wasm.sh`, which builds
//! `-p patina-web --target wasm32-unknown-unknown`. A native test run proves
//! nothing about it beyond that it parses.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use gdcore::math::Vector2;
use gdplatform::input::InputEvent;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

use crate::browser_input::{
    self, CanvasMapping, GamepadButtonSample, GamepadSnapshot, GamepadTracker, TouchTracker,
    WheelDeltaMode,
};
use crate::engine::WebEngine;
use crate::FpsMeter;

/// Everything one page's engine owns, behind a single `RefCell` so the
/// listeners and the frame callback can share it.
///
/// One cell rather than one per field: a listener that had to take two borrows
/// (engine + tracker) would panic the moment a frame callback interleaved, and
/// on the browser main thread there is no real concurrency to gain from finer
/// locking.
struct Runtime {
    engine: WebEngine,
    touches: TouchTracker,
    gamepads: GamepadTracker,
    fps: FpsMeter,
    /// Frames that actually stepped the loop (a coalesced callback does not).
    frames: u32,
    /// Events handed to the engine, so a page can show that input arrives.
    events: u32,
    /// Debug label for the most recent event, for the dev harness.
    last_input: String,
}

impl Runtime {
    fn new(tree: SceneTree) -> Self {
        Self {
            engine: WebEngine::new(tree),
            touches: TouchTracker::new(),
            gamepads: GamepadTracker::new(),
            fps: FpsMeter::new(1000.0),
            frames: 0,
            events: 0,
            last_input: String::new(),
        }
    }

    /// Pushes one translated event into the engine and records it.
    fn push(&mut self, event: InputEvent) {
        self.last_input = format!("{event:?}");
        self.events += 1;
        self.engine.push_input(event);
    }

    fn push_all(&mut self, events: impl IntoIterator<Item = InputEvent>) {
        for event in events {
            self.push(event);
        }
    }
}

/// A running web engine: the `MainLoop`, its animation-frame driver and the
/// DOM listeners feeding it.
///
/// Held by the JS host for the lifetime of the page. The getters exist so the
/// loop is *observable* from the page — "60fps" is the acceptance for this
/// work, and a number the page can print is how it gets checked in a browser.
#[wasm_bindgen]
pub struct WebEngineHandle {
    runtime: Rc<RefCell<Runtime>>,
}

#[wasm_bindgen]
impl WebEngineHandle {
    /// Frames that actually advanced the loop since start.
    #[wasm_bindgen(getter)]
    pub fn frames(&self) -> u32 {
        self.runtime.borrow().frames
    }

    /// Frames per second over the last completed one-second window.
    #[wasm_bindgen(getter)]
    pub fn fps(&self) -> f64 {
        self.runtime.borrow().fps.fps()
    }

    /// Simulated time in seconds, after clamping and hidden-tab throttling —
    /// deliberately not wall-clock time since start.
    #[wasm_bindgen(getter)]
    pub fn elapsed(&self) -> f64 {
        self.runtime.borrow().engine.elapsed()
    }

    /// Input events delivered to the engine since start.
    #[wasm_bindgen(getter)]
    pub fn events(&self) -> u32 {
        self.runtime.borrow().events
    }

    /// Debug rendering of the most recent input event, or an empty string.
    #[wasm_bindgen(getter, js_name = lastInput)]
    pub fn last_input(&self) -> String {
        self.runtime.borrow().last_input.clone()
    }

    /// Whether the driver currently considers the tab hidden.
    #[wasm_bindgen(getter)]
    pub fn hidden(&self) -> bool {
        self.runtime.borrow().engine.is_hidden()
    }

    /// Fingers currently down.
    #[wasm_bindgen(getter)]
    pub fn touches(&self) -> usize {
        self.runtime.borrow().touches.len()
    }

    /// Standard-mapped gamepads being tracked.
    #[wasm_bindgen(getter)]
    pub fn gamepads(&self) -> usize {
        self.runtime.borrow().gamepads.len()
    }

    /// Caps the frame rate (Godot's `Engine.max_fps`); `0` removes the cap.
    #[wasm_bindgen(js_name = setMaxFps)]
    pub fn set_max_fps(&self, fps: u32) {
        self.runtime.borrow_mut().engine.set_max_fps(fps);
    }
}

/// The browser `Window`, or a message naming why there is none.
fn window() -> Result<web_sys::Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("no `window`: not on a browser main thread"))
}

/// How the canvas currently maps CSS pixels to project pixels.
///
/// Recomputed per event rather than cached: the canvas moves when the page
/// scrolls or reflows, and a stale origin puts every click at an offset.
fn mapping_for(canvas: &web_sys::HtmlCanvasElement) -> CanvasMapping {
    let rect = canvas.get_bounding_client_rect();
    CanvasMapping {
        origin_css: Vector2::new(rect.left() as f32, rect.top() as f32),
        device_pixel_ratio: crate::canvas::device_pixel_ratio() as f32,
    }
}

/// `target.addEventListener(name, handler)`, leaking the closure.
///
/// The closure must outlive the call: JS keeps the function, and dropping the
/// `Closure` invalidates it so the listener traps when it next fires. These
/// listeners live as long as the page, so `forget` is the honest lifetime
/// rather than a leak to be fixed — the same reason
/// `gdplatform::fetch_web` parks its callbacks for the module's lifetime.
fn listen<E, F>(target: &web_sys::EventTarget, name: &str, handler: F) -> Result<(), JsValue>
where
    E: wasm_bindgen::convert::FromWasmAbi + 'static,
    F: FnMut(E) + 'static,
{
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(E)>);
    target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

/// Reads `navigator.getGamepads()` into the snapshot
/// [`GamepadTracker::poll`] differences.
///
/// A slot can be `null` (a pad that was unplugged) and a pad can report
/// `connected == false`; both are skipped so they read as "gone" and the
/// tracker releases whatever they held.
fn poll_gamepads(navigator: &web_sys::Navigator) -> Vec<GamepadSnapshot> {
    let pads = match navigator.get_gamepads() {
        Ok(pads) => pads,
        // A browser without the Gamepad API, or one that refuses it in an
        // insecure context, throws rather than returning an empty list.
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for index in 0..pads.length() {
        let Ok(pad) = pads.get(index).dyn_into::<web_sys::Gamepad>() else {
            continue;
        };
        if !pad.connected() {
            continue;
        }
        let buttons = pad.buttons();
        let mut samples = Vec::with_capacity(buttons.length() as usize);
        for i in 0..buttons.length() {
            let Ok(button) = buttons.get(i).dyn_into::<web_sys::GamepadButton>() else {
                continue;
            };
            samples.push(GamepadButtonSample {
                pressed: button.pressed(),
                value: button.value() as f32,
            });
        }
        let axes_array = pad.axes();
        let mut axes = Vec::with_capacity(axes_array.length() as usize);
        for i in 0..axes_array.length() {
            axes.push(axes_array.get(i).as_f64().unwrap_or(0.0) as f32);
        }
        out.push(GamepadSnapshot {
            id: pad.index(),
            standard_mapping: pad.mapping() == web_sys::GamepadMappingType::Standard,
            buttons: samples,
            axes,
        });
    }
    out
}

/// A viewport-relative CSS point from a mouse-family event.
fn client_point(event: &web_sys::MouseEvent) -> Vector2 {
    Vector2::new(event.client_x() as f32, event.client_y() as f32)
}

/// Subscribes the keyboard, pointer, wheel and touch listeners.
fn install_listeners(
    runtime: &Rc<RefCell<Runtime>>,
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<(), JsValue> {
    let window = window()?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("`window` has no `document`"))?;

    // --- keyboard -------------------------------------------------------
    //
    // On the window, not the canvas: a <canvas> only receives key events when
    // it is focused, which needs a tabindex and a click first, so a game bound
    // to the canvas appears dead until the player happens to click it.
    for (name, pressed) in [("keydown", true), ("keyup", false)] {
        let runtime = runtime.clone();
        listen(
            window.as_ref(),
            name,
            move |event: web_sys::KeyboardEvent| {
                // An auto-repeating keydown is a held key, not a new press;
                // Godot reports it as an echo and a game that treats it as a
                // press fires once per repeat interval.
                if pressed && event.repeat() {
                    return;
                }
                if let Some(input) = browser_input::key_event(
                    &event.code(),
                    pressed,
                    event.shift_key(),
                    event.ctrl_key(),
                    event.alt_key(),
                ) {
                    // Only for keys the engine actually consumed: swallowing
                    // every key would break browser shortcuts and tabbing.
                    event.prevent_default();
                    runtime.borrow_mut().push(input);
                }
            },
        )?;
    }

    // --- pointer --------------------------------------------------------
    {
        let runtime = runtime.clone();
        let canvas_for_move = canvas.clone();
        listen(
            canvas.as_ref(),
            "pointermove",
            move |event: web_sys::PointerEvent| {
                let mapping = mapping_for(&canvas_for_move);
                let input = browser_input::pointer_move(
                    client_point(event.as_ref()),
                    Vector2::new(event.movement_x() as f32, event.movement_y() as f32),
                    mapping,
                );
                runtime.borrow_mut().push(input);
            },
        )?;
    }

    for (name, pressed) in [("pointerdown", true), ("pointerup", false)] {
        let runtime = runtime.clone();
        let canvas_for_button = canvas.clone();
        listen(
            canvas.as_ref(),
            name,
            move |event: web_sys::PointerEvent| {
                let mapping = mapping_for(&canvas_for_button);
                // `MouseEvent.button` is signed in the DOM IDL; a negative
                // value is not a button this engine knows.
                let button = event.button();
                if button < 0 {
                    return;
                }
                if let Some(input) = browser_input::pointer_button(
                    button as u16,
                    pressed,
                    client_point(event.as_ref()),
                    mapping,
                ) {
                    runtime.borrow_mut().push(input);
                }
            },
        )?;
    }

    {
        let runtime = runtime.clone();
        let canvas_for_wheel = canvas.clone();
        listen(
            canvas.as_ref(),
            "wheel",
            move |event: web_sys::WheelEvent| {
                let mapping = mapping_for(&canvas_for_wheel);
                let events = browser_input::wheel(
                    event.delta_y() as f32,
                    WheelDeltaMode::from_dom(event.delta_mode()),
                    client_point(event.as_ref()),
                    mapping,
                );
                if events.is_empty() {
                    return;
                }
                // A scroll the game consumed must not also scroll the page.
                event.prevent_default();
                runtime.borrow_mut().push_all(events);
            },
        )?;
    }

    // --- touch ----------------------------------------------------------
    //
    // `changedTouches` rather than `touches`: the former is the fingers this
    // event is *about*, the latter every finger still down, so using `touches`
    // would re-report untouched fingers on every event.
    {
        let runtime = runtime.clone();
        let canvas_for_touch = canvas.clone();
        listen(
            canvas.as_ref(),
            "touchstart",
            move |event: web_sys::TouchEvent| {
                let mapping = mapping_for(&canvas_for_touch);
                let now = now_ms();
                event.prevent_default();
                let touches = event.changed_touches();
                let mut runtime = runtime.borrow_mut();
                for i in 0..touches.length() {
                    let Some(touch) = touches.get(i) else { continue };
                    let input = runtime.touches.touch_begin(
                        touch_index(&touch),
                        Vector2::new(touch.client_x() as f32, touch.client_y() as f32),
                        mapping,
                        now,
                    );
                    runtime.push(input);
                }
            },
        )?;
    }

    {
        let runtime = runtime.clone();
        let canvas_for_touch = canvas.clone();
        listen(
            canvas.as_ref(),
            "touchmove",
            move |event: web_sys::TouchEvent| {
                let mapping = mapping_for(&canvas_for_touch);
                let now = now_ms();
                // Without this a drag on the canvas scrolls or zooms the page.
                event.prevent_default();
                let touches = event.changed_touches();
                let mut runtime = runtime.borrow_mut();
                for i in 0..touches.length() {
                    let Some(touch) = touches.get(i) else { continue };
                    let input = runtime.touches.touch_move(
                        touch_index(&touch),
                        Vector2::new(touch.client_x() as f32, touch.client_y() as f32),
                        touch.force(),
                        mapping,
                        now,
                    );
                    if let Some(input) = input {
                        runtime.push(input);
                    }
                }
            },
        )?;
    }

    for (name, canceled) in [("touchend", false), ("touchcancel", true)] {
        let runtime = runtime.clone();
        let canvas_for_touch = canvas.clone();
        listen(
            canvas.as_ref(),
            name,
            move |event: web_sys::TouchEvent| {
                let mapping = mapping_for(&canvas_for_touch);
                let touches = event.changed_touches();
                let mut runtime = runtime.borrow_mut();
                for i in 0..touches.length() {
                    let Some(touch) = touches.get(i) else { continue };
                    let input = runtime.touches.touch_end(
                        touch_index(&touch),
                        Vector2::new(touch.client_x() as f32, touch.client_y() as f32),
                        canceled,
                        mapping,
                    );
                    runtime.push(input);
                }
            },
        )?;
    }

    // --- visibility -----------------------------------------------------
    //
    // A hidden tab stops getting animation frames and gamepad samples. Telling
    // the driver throttles it instead of letting the next visible frame arrive
    // with minutes of banked delta; releasing the held input stops a key or
    // finger that was down at the moment of hiding from staying down forever.
    {
        let runtime = runtime.clone();
        let document_for_visibility = document.clone();
        listen(
            document.as_ref(),
            "visibilitychange",
            move |_event: web_sys::Event| {
                let hidden = document_for_visibility.hidden();
                let mut runtime = runtime.borrow_mut();
                runtime.engine.set_hidden(hidden);
                if hidden {
                    let touch_releases = runtime.touches.release_all();
                    runtime.push_all(touch_releases);
                    let pad_releases = runtime.gamepads.release_all();
                    runtime.push_all(pad_releases);
                }
            },
        )?;
    }

    Ok(())
}

/// The browser's `Touch.identifier` as the engine's finger index.
///
/// The DOM identifier is a signed, browser-assigned integer that is only
/// guaranteed unique among live touches; Safari hands out large values and
/// Firefox has handed out negative ones. Casting preserves distinctness, which
/// is all the engine needs of it.
fn touch_index(touch: &web_sys::Touch) -> u32 {
    touch.identifier() as u32
}

/// `performance.now()`, falling back to 0.0 where there is no window.
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// Requests the next animation frame for `callback`.
fn request_frame(callback: &Closure<dyn FnMut(f64)>) -> Result<(), JsValue> {
    window()?.request_animation_frame(callback.as_ref().unchecked_ref())?;
    Ok(())
}

/// Installs the `requestAnimationFrame` loop.
///
/// The callback re-requests itself and returns immediately — it never blocks,
/// which is the whole point: the browser main thread is also the thread that
/// paints and handles input, so a loop that parked it would freeze the page.
/// Timestamps go straight to [`WebEngine::frame`], which decides whether this
/// callback is a step at all.
fn install_frame_loop(
    runtime: &Rc<RefCell<Runtime>>,
    navigator: web_sys::Navigator,
) -> Result<(), JsValue> {
    // The callback has to name itself to re-request, so it is built into a cell
    // it also holds a handle to. This is the shape the wasm-bindgen guide uses
    // for a self-rescheduling animation frame.
    let slot: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let slot_for_callback = slot.clone();
    let runtime = runtime.clone();

    *slot.borrow_mut() = Some(Closure::wrap(Box::new(move |now_ms: f64| {
        {
            let mut rt = runtime.borrow_mut();
            // Gamepads are polled, not evented, so a frame is the only place to
            // sample them. Before the step, so this frame's `_input` sees them.
            let snapshot = poll_gamepads(&navigator);
            let pad_events = rt.gamepads.poll(&snapshot);
            rt.push_all(pad_events);

            if rt.engine.frame(now_ms).is_some() {
                rt.frames += 1;
            }
            rt.fps.frame(now_ms);
        }
        // Re-request outside the borrow: `request_animation_frame` can invoke
        // JS, and a callback re-entering while the cell is borrowed would
        // panic instead of dropping a frame.
        if let Some(callback) = slot_for_callback.borrow().as_ref() {
            let _ = request_frame(callback);
        }
    }) as Box<dyn FnMut(f64)>));

    let callback = slot.borrow();
    let callback = callback
        .as_ref()
        .ok_or_else(|| JsValue::from_str("frame callback was not installed"))?;
    request_frame(callback)
}

/// Starts the engine on the `<canvas id="...">` named by `canvas_id`.
///
/// Wires the DOM listeners, kicks off the `requestAnimationFrame` loop, and
/// hands back a handle whose getters let the page observe the rate and the
/// input stream.
///
/// The scene is a bare `Node2D` root: loading a project's scene tree is the
/// export pipeline's job (pat-u3l8c.114), and the loop and the input path do
/// not depend on what is in the tree.
#[wasm_bindgen(js_name = startEngine)]
pub fn start_engine(canvas_id: String) -> Result<WebEngineHandle, JsValue> {
    let canvas = crate::canvas::canvas_by_id(&canvas_id).map_err(JsValue::from)?;
    let navigator = window()?.navigator();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Node2D", "Node2D"))
        .map_err(|e| JsValue::from_str(&format!("could not build the scene: {e:?}")))?;

    let runtime = Rc::new(RefCell::new(Runtime::new(tree)));
    install_listeners(&runtime, &canvas)?;
    install_frame_loop(&runtime, navigator)?;
    Ok(WebEngineHandle { runtime })
}
