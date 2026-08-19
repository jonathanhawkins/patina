//! pat-s3rrv.78 — the browser (`fetch`) HTTP backend, mirroring `godot_js_fetch_*`.
//!
//! Godot ground truth: `platform/web/http_client_web.h:41-51` declares the C
//! surface (`create` / `state_get` / `read_headers` / `read_chunk` /
//! `http_status_get` / `is_chunked` / `free`), implemented in
//! `platform/web/js/libs/library_godot_fetch.js`. This module is the Rust side
//! of the same shape.
//!
//! ⚠️ **This module holds no lifecycle rules.** Every "what state is this in"
//! and "what should the next poll do" decision is delegated to
//! [`crate::fetch_state`], which is target-independent and unit-tested on the
//! host. That split is deliberate: code behind `#[cfg(target_arch = "wasm32")]`
//! is never type-checked by a native build and never *run* by any build in this
//! repo, so anything subtle placed here would be unprovable. What is left here
//! is only the JS plumbing.
//!
//! Verify this file with a build for `wasm32-unknown-unknown` of `-p patina-web`
//! (the command inside `scripts/check-wasm.sh`); `patina-web` depends on this
//! crate directly, so that build type-checks the block below. A native test run
//! proves nothing about it beyond that it parses.

/// Handle for one in-flight fetch — the `int` id `godot_js_fetch_create` returns.
pub type FetchId = i32;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::collections::VecDeque;

    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::{JsCast, JsValue as RawJsValue};

    use super::FetchId;
    use crate::fetch_state::{
        apply_read, drain_chunks, fetch_state, is_chunked, read_action, FetchProgress, FetchState,
        ReadAction,
    };

    /// One in-flight fetch. Mirrors the object `GodotFetch.create` builds
    /// (`library_godot_fetch.js:81-90`).
    #[derive(Default)]
    struct Fetch {
        /// The flags [`fetch_state`] and [`read_action`] consult.
        progress: FetchProgress,
        status: u16,
        chunked: bool,
        headers: Vec<(String, String)>,
        /// Decoded body chunks, oldest first.
        chunks: VecDeque<Vec<u8>>,
        reader: Option<RawJsValue>,
        /// ⚠️ Closures handed to JS must outlive this call. JS holds the
        /// function; dropping the `Closure` invalidates it and the callback
        /// traps when the promise settles. Same reason `js_bridge.rs:481-484`
        /// parks its callbacks for the lifetime of the module.
        keepalive: Vec<Closure<dyn FnMut(RawJsValue)>>,
        /// `Headers.forEach` takes a two-argument callback, so it needs its own
        /// type — see [`capture_headers`].
        keepalive2: Vec<Closure<dyn FnMut(RawJsValue, RawJsValue)>>,
    }

    thread_local! {
        static FETCHES: RefCell<HashMap<FetchId, Fetch>> = RefCell::new(HashMap::new());
        static NEXT_ID: Cell<FetchId> = Cell::new(1);
    }

    fn next_id() -> FetchId {
        NEXT_ID.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1).max(1));
            id
        })
    }

    fn with_fetch<R>(id: FetchId, f: impl FnOnce(&mut Fetch) -> R) -> Option<R> {
        FETCHES.with(|m| m.borrow_mut().get_mut(&id).map(f))
    }

    fn key(name: &str) -> RawJsValue {
        RawJsValue::from_str(name)
    }

    /// Marks a request failed. Every JS rejection path lands here, matching
    /// `GodotFetch.onerror` (`library_godot_fetch.js:71-78`).
    fn mark_errored(id: FetchId) {
        with_fetch(id, |f| f.progress.errored = true);
    }

    /// Reads every header off a `Response` into the registry.
    ///
    /// ⚠️ `Headers.forEach` invokes its callback as `(value, name)` — **value
    /// first**. Godot's own code spells it `function (value, header)`
    /// (`library_godot_fetch.js:53`). Reading them in the intuitive order gives
    /// a header list with names and values transposed, which looks plausible in
    /// a debugger and breaks every consumer that matches on a header name.
    fn capture_headers(id: FetchId, response: &RawJsValue) {
        let headers = match js_sys::Reflect::get(response, &key("headers")) {
            Ok(h) => h,
            Err(_) => return,
        };
        let for_each = match js_sys::Reflect::get(&headers, &key("forEach")) {
            Ok(f) => f,
            Err(_) => return,
        };
        let Ok(for_each) = for_each.dyn_into::<js_sys::Function>() else {
            return;
        };

        let cb = Closure::wrap(Box::new(move |value: RawJsValue, name: RawJsValue| {
            let (Some(name), Some(value)) = (name.as_string(), value.as_string()) else {
                return;
            };
            with_fetch(id, |f| f.headers.push((name, value)));
        }) as Box<dyn FnMut(RawJsValue, RawJsValue)>);

        let _ = for_each.call1(&headers, cb.as_ref());
        with_fetch(id, |f| f.keepalive2.push(cb));
    }

    /// Handles the resolved `Response`, mirroring `GodotFetch.onresponse`
    /// (`library_godot_fetch.js:47-67`).
    fn on_response(id: FetchId, response: RawJsValue) {
        capture_headers(id, &response);

        let status = js_sys::Reflect::get(&response, &key("status"))
            .ok()
            .and_then(|s| s.as_f64())
            .unwrap_or(0.0) as u16;

        // ⚠️ `body` is legitimately null (notably for HEAD; Chromium and Firefox
        // disagree — godotengine/godot#76825). `fetch_state` relies on knowing
        // this, so record it rather than treating it as an error.
        let body = js_sys::Reflect::get(&response, &key("body")).unwrap_or(RawJsValue::NULL);
        let body_is_null = body.is_null() || body.is_undefined();

        let reader = if body_is_null {
            None
        } else {
            js_sys::Reflect::get(&body, &key("getReader"))
                .ok()
                .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                .and_then(|f| f.call0(&body).ok())
        };

        with_fetch(id, |f| {
            f.status = status;
            f.progress.has_response = true;
            f.progress.body_is_null = body_is_null;
            f.progress.has_reader = reader.is_some();
            f.reader = reader;
            let pairs: Vec<(&str, &str)> = f
                .headers
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_str()))
                .collect();
            f.chunked = is_chunked(pairs);
        });
    }

    /// Handles one `reader.read()` result, mirroring `GodotFetch.onread`
    /// (`library_godot_fetch.js:35-45`).
    fn on_read(id: FetchId, result: RawJsValue) {
        let done = js_sys::Reflect::get(&result, &key("done"))
            .ok()
            .and_then(|d| d.as_bool())
            .unwrap_or(false);

        let value = js_sys::Reflect::get(&result, &key("value")).unwrap_or(RawJsValue::UNDEFINED);
        let bytes = if value.is_undefined() || value.is_null() {
            None
        } else {
            Some(js_sys::Uint8Array::new(&value).to_vec())
        };

        with_fetch(id, |f| {
            if apply_read(&mut f.progress, bytes.is_some(), done) {
                if let Some(b) = bytes {
                    f.chunks.push_back(b);
                }
            }
        });
    }

    /// Attaches `then`/`catch` handlers to a promise, parking the closures.
    fn settle(id: FetchId, promise: RawJsValue, on_ok: impl FnMut(RawJsValue) + 'static) {
        let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
            mark_errored(id);
            return;
        };
        let ok = Closure::wrap(Box::new(on_ok) as Box<dyn FnMut(RawJsValue)>);
        let err = Closure::wrap(
            Box::new(move |_e: RawJsValue| mark_errored(id)) as Box<dyn FnMut(RawJsValue)>
        );
        let _ = promise.then(&ok).catch(&err);
        with_fetch(id, |f| {
            f.keepalive.push(ok);
            f.keepalive.push(err);
        });
    }

    /// `godot_js_fetch_create`. Returns the handle to poll.
    pub fn create(method: &str, url: &str, headers: &[(String, String)], body: &[u8]) -> FetchId {
        let id = next_id();
        FETCHES.with(|m| m.borrow_mut().insert(id, Fetch::default()));

        let global = js_sys::global();
        let init = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&init, &key("method"), &key(method));

        let hdrs = js_sys::Object::new();
        for (name, value) in headers {
            let _ = js_sys::Reflect::set(&hdrs, &key(name), &key(value));
        }
        let _ = js_sys::Reflect::set(&init, &key("headers"), &hdrs);

        // ⚠️ An empty body is OMITTED, not sent as a zero-length buffer: `fetch`
        // rejects a GET or HEAD that carries a body at all.
        if !body.is_empty() {
            let _ =
                js_sys::Reflect::set(&init, &key("body"), &js_sys::Uint8Array::from(body).into());
        }

        let fetch_fn = js_sys::Reflect::get(&global, &key("fetch"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        let Some(fetch_fn) = fetch_fn else {
            mark_errored(id);
            return id;
        };

        match fetch_fn.call2(&global, &key(url), &init) {
            Ok(promise) => settle(id, promise, move |response| on_response(id, response)),
            Err(_) => mark_errored(id),
        }
        id
    }

    /// `godot_js_fetch_state_get`. An unknown handle is `Error`, as upstream.
    pub fn state(id: FetchId) -> FetchState {
        with_fetch(id, |f| fetch_state(f.progress)).unwrap_or(FetchState::Error)
    }

    /// Drives the body stream one step. Mirrors `GodotFetch.read`.
    ///
    /// Called every frame while the state is `Body`; the decision of what to do
    /// is [`read_action`]'s, not this function's.
    pub fn poll(id: FetchId) {
        let Some(action) = with_fetch(id, |f| read_action(f.progress)) else {
            return;
        };
        match action {
            ReadAction::Idle => {}
            ReadAction::ReleaseReader => {
                with_fetch(id, |f| {
                    f.reader = None;
                    f.progress.has_reader = false;
                });
            }
            // ⚠️ No stream to read, so synthesise completion. Without this a
            // null-body response never leaves `Body`.
            ReadAction::EmulateClose => {
                with_fetch(id, |f| {
                    f.progress.reading = false;
                    f.progress.done = true;
                });
            }
            ReadAction::Read => {
                let reader = with_fetch(id, |f| f.reader.clone()).flatten();
                let Some(reader) = reader else { return };
                let read_fn = js_sys::Reflect::get(&reader, &key("read"))
                    .ok()
                    .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
                let Some(read_fn) = read_fn else {
                    mark_errored(id);
                    return;
                };
                // Mark BEFORE dispatching: the promise can settle before this
                // returns, and `read_action` must not issue a second read.
                with_fetch(id, |f| f.progress.reading = true);
                match read_fn.call0(&reader) {
                    Ok(promise) => settle(id, promise, move |result| on_read(id, result)),
                    Err(_) => mark_errored(id),
                }
            }
        }
    }

    /// `godot_js_fetch_read_chunk`. Copies out at most `out.len()` bytes and
    /// returns how many were written.
    ///
    /// ⚠️ A partially consumed chunk is pushed back at the FRONT, so a small
    /// buffer drains the stream in order rather than dropping the remainder.
    pub fn read_chunk(id: FetchId, out: &mut [u8]) -> usize {
        with_fetch(id, |f| drain_chunks(&mut f.chunks, out)).unwrap_or(0)
    }

    /// `godot_js_fetch_http_status_get`. `0` until the response arrives.
    pub fn http_status(id: FetchId) -> u16 {
        with_fetch(id, |f| f.status).unwrap_or(0)
    }

    /// `godot_js_fetch_is_chunked`.
    pub fn is_chunked_response(id: FetchId) -> bool {
        with_fetch(id, |f| f.chunked).unwrap_or(false)
    }

    /// The response headers as `"Name: Value"` lines, the shape
    /// `request_completed` carries.
    pub fn headers(id: FetchId) -> Vec<String> {
        with_fetch(id, |f| {
            f.headers.iter().map(|(n, v)| format!("{n}: {v}")).collect()
        })
        .unwrap_or_default()
    }

    /// `godot_js_fetch_free`. Drops the entry and with it the parked closures.
    pub fn free(id: FetchId) {
        FETCHES.with(|m| m.borrow_mut().remove(&id));
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::{create, free, headers, http_status, is_chunked_response, poll, read_chunk, state};
