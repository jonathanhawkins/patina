//! The real `navigator.xr` binding for [`crate::xr`] (pat-s3rrv.218).
//!
//! Godot ships WebXR in its web export (`modules/webxr/config.py:1-2`), where
//! `WebXRInterfaceJS` drives the session from `navigator.xr` and forwards the
//! browser's events as Godot signals. Patina had the whole session state
//! machine in [`crate::xr`] but nothing to drive it: its only input was a
//! `WebXrStub` installed from a script-visible `_set_test_stub` seam.
//!
//! This module is the missing half. It follows `js_bridge.rs`'s convention of
//! reaching JavaScript **reflectively through `js-sys`** rather than pulling in
//! `web-sys`: everything WebXR needs (`navigator.xr`, `isSessionSupported`,
//! `requestSession`, `addEventListener`) is reachable through `Reflect`, so
//! gdplatform does not have to track `web-sys`'s per-DOM-type feature list.
//!
//! ⚠️ The browser is asynchronous and the frame loop is not. Nothing here calls
//! into the interface directly — every callback records a [`WebXrFrameEvent`]
//! in a thread-local queue, and [`crate::xr::WebXrInterface::poll`] drains it on
//! the next frame. That is also why a JS callback can never re-enter script.
//!
//! ⚠️ **This module is only compiled for `wasm32`.** The workspace's test lane
//! builds native, so nothing here is exercised by the test runner. It is gated
//! precisely so a mistake in it cannot break the native build.

#![cfg(target_arch = "wasm32")]

use crate::xr::{TargetRayMode, WebXrFrameEvent, WebXrInputEvent};
use std::cell::RefCell;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::{JsCast, JsValue as RawJsValue};

thread_local! {
    /// Events recorded by browser callbacks, awaiting the next frame.
    static EVENTS: RefCell<Vec<WebXrFrameEvent>> = const { RefCell::new(Vec::new()) };
    /// Closures handed to JavaScript. Kept alive for the module's lifetime:
    /// JS holds the function, so dropping the `Closure` would leave it dangling.
    static LISTENERS: RefCell<Vec<Closure<dyn FnMut(RawJsValue)>>> =
        RefCell::new(Vec::new());
    /// The live `XRSession`, once `requestSession` resolves.
    static SESSION: RefCell<Option<RawJsValue>> = RefCell::new(None);
}

/// `target[property]`, reflectively.
fn get(target: &RawJsValue, property: &str) -> Option<RawJsValue> {
    js_sys::Reflect::get(target, &RawJsValue::from_str(property)).ok()
}

/// `target.method(...args)` with `target` as the receiver.
fn call(target: &RawJsValue, method: &str, args: &[RawJsValue]) -> Option<RawJsValue> {
    let f = get(target, method)?.dyn_into::<js_sys::Function>().ok()?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    js_sys::Reflect::apply(&f, target, &arr).ok()
}

/// `navigator.xr`, or `None` in a browser without WebXR.
pub fn navigator_xr() -> Option<RawJsValue> {
    let global = js_sys::global();
    let navigator = get(&global, "navigator")?;
    let xr = get(&navigator, "xr")?;
    if xr.is_undefined() || xr.is_null() {
        None
    } else {
        Some(xr)
    }
}

/// Whether this browser exposes WebXR at all.
pub fn is_available() -> bool {
    navigator_xr().is_some()
}

/// Records an event for the next [`drain_events`].
fn record(event: WebXrFrameEvent) {
    EVENTS.with(|e| e.borrow_mut().push(event));
}

/// Takes everything the browser reported since the last frame.
///
/// Called from `WebXrInterface::poll`, so browser events reach script through
/// the ordinary frame tick rather than from inside a JS callback.
pub fn drain_events() -> Vec<WebXrFrameEvent> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

/// Subscribes `target.addEventListener(name, cb)`, keeping the closure alive.
fn listen<F>(target: &RawJsValue, handler_name: &str, mut handler: F)
where
    F: FnMut(RawJsValue) + 'static,
{
    let cb =
        Closure::wrap(Box::new(move |ev: RawJsValue| handler(ev)) as Box<dyn FnMut(RawJsValue)>);
    let _ = call(
        target,
        "addEventListener",
        &[RawJsValue::from_str(handler_name), cb.as_ref().clone()],
    );
    LISTENERS.with(|l| l.borrow_mut().push(cb));
}

/// The `inputSource` index carried by a select/squeeze event.
///
/// The browser hands over the `XRInputSource` object itself; the interface keys
/// on a small integer, so the source's position in `session.inputSources` is
/// what identifies it.
fn input_source_index(event: &RawJsValue) -> usize {
    let Some(session) = SESSION.with(|s| s.borrow().clone()) else {
        return 0;
    };
    let (Some(source), Some(list)) = (get(event, "inputSource"), get(&session, "inputSources"))
    else {
        return 0;
    };
    let Ok(list) = list.dyn_into::<js_sys::Array>() else {
        return 0;
    };
    for i in 0..list.length() {
        if list.get(i) == source {
            return i as usize;
        }
    }
    0
}

/// Maps `XRInputSource.targetRayMode` onto the interface's enum.
fn target_ray_mode(source: &RawJsValue) -> TargetRayMode {
    match get(source, "targetRayMode")
        .and_then(|v| v.as_string())
        .as_deref()
    {
        Some("gaze") => TargetRayMode::Gaze,
        Some("tracked-pointer") => TargetRayMode::TrackedPointer,
        Some("screen") => TargetRayMode::Screen,
        _ => TargetRayMode::Unknown,
    }
}

/// Wires every session event Godot forwards onto the recording queue.
///
/// Mirrors `webxr_interface_js.cpp`'s callback set: session `end`,
/// `visibilitychange`, reference-space `reset`, `inputsourceschange`, and the
/// four select/squeeze events.
pub fn attach_session(session: RawJsValue) {
    SESSION.with(|s| *s.borrow_mut() = Some(session.clone()));

    listen(&session, "end", |_| record(WebXrFrameEvent::SessionEnded));

    let vis_target = session.clone();
    listen(&session, "visibilitychange", move |_| {
        let state = get(&vis_target, "visibilityState")
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "visible".to_string());
        record(WebXrFrameEvent::VisibilityState(state));
    });

    listen(&session, "reset", |_| {
        record(WebXrFrameEvent::ReferenceSpaceReset)
    });

    listen(&session, "inputsourceschange", |event| {
        for (key, connected) in [("added", true), ("removed", false)] {
            let Some(list) = get(&event, key).and_then(|v| v.dyn_into::<js_sys::Array>().ok())
            else {
                continue;
            };
            for i in 0..list.length() {
                let source = list.get(i);
                let id = i as usize;
                record(if connected {
                    WebXrFrameEvent::InputSourceConnected {
                        input_source_id: id,
                        target_ray_mode: target_ray_mode(&source),
                    }
                } else {
                    WebXrFrameEvent::InputSourceDisconnected {
                        input_source_id: id,
                    }
                });
            }
        }
    });

    for (event_name, event) in [
        ("selectstart", WebXrInputEvent::SelectStart),
        ("selectend", WebXrInputEvent::SelectEnd),
        ("squeezestart", WebXrInputEvent::SqueezeStart),
        ("squeezeend", WebXrInputEvent::SqueezeEnd),
    ] {
        listen(&session, event_name, move |ev| {
            record(WebXrFrameEvent::Input {
                event,
                input_source_id: input_source_index(&ev),
            })
        });
    }
}

/// `navigator.xr.isSessionSupported(mode)`, resolved onto `sink`.
///
/// The promise is asynchronous, so the answer arrives on a later frame — which
/// is exactly how Godot's `session_supported` signal behaves.
pub fn is_session_supported<F>(mode: &str, sink: F)
where
    F: FnMut(bool) + 'static,
{
    let mut sink = sink;
    let Some(xr) = navigator_xr() else {
        sink(false);
        return;
    };
    let Some(promise) = call(&xr, "isSessionSupported", &[RawJsValue::from_str(mode)]) else {
        sink(false);
        return;
    };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
        sink(false);
        return;
    };
    let cb = Closure::wrap(Box::new(move |v: RawJsValue| {
        sink(v.as_bool().unwrap_or(false));
    }) as Box<dyn FnMut(RawJsValue)>);
    let _ = promise.then(&cb);
    LISTENERS.with(|l| l.borrow_mut().push(cb));
}

/// `navigator.xr.requestSession(mode, { requiredFeatures, optionalFeatures })`.
///
/// On resolve the session is attached and its events begin queueing; on reject
/// nothing is queued and the caller's own failure path applies.
pub fn request_session(mode: &str, required: &[String], optional: &[String]) {
    let Some(xr) = navigator_xr() else {
        return;
    };
    let options = js_sys::Object::new();
    for (key, list) in [
        ("requiredFeatures", required),
        ("optionalFeatures", optional),
    ] {
        let arr = js_sys::Array::new();
        for f in list {
            arr.push(&RawJsValue::from_str(f));
        }
        let _ = js_sys::Reflect::set(&options, &RawJsValue::from_str(key), &arr);
    }
    let Some(promise) = call(
        &xr,
        "requestSession",
        &[RawJsValue::from_str(mode), options.into()],
    ) else {
        return;
    };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
        return;
    };
    let cb = Closure::wrap(
        Box::new(|session: RawJsValue| attach_session(session)) as Box<dyn FnMut(RawJsValue)>
    );
    let _ = promise.then(&cb);
    LISTENERS.with(|l| l.borrow_mut().push(cb));
}
