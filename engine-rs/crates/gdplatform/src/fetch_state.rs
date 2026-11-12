//! pat-s3rrv.78 — the state machine behind Godot's web (`fetch`) HTTP backend.
//!
//! Godot ground truth: `platform/web/js/libs/library_godot_fetch.js` plus the
//! `godot_js_fetch_*` surface declared in `platform/web/http_client_web.h:41-51`.
//! On the web export Godot does not open sockets; it calls `fetch()`, then polls
//! `godot_js_fetch_state_get` each frame and drains the body through
//! `godot_js_fetch_read_chunk`.
//!
//! ⚠️ **Why this is a separate, target-independent module.** The obvious way to
//! write the wasm backend is one `#[cfg(target_arch = "wasm32")]` block holding
//! both the JS interop and the lifecycle rules. That would be untestable on the
//! host — and worse, only *partly* checkable anywhere: `#[cfg]`-disabled code is
//! still parsed on a native build but never type-checked, so a native verify
//! catches a missing semicolon in it and nothing else. The rules below are the
//! part that is easy to get wrong and have nothing to do with JS, so they live
//! here, compiled and tested on every target. The interop shell that calls them
//! stays thin.
//!
//! 🔑 The trap this module exists to encode: **`response.body` may be `null`**
//! (notably for `HEAD`). Chromium and Firefox disagree about it — see
//! `godotengine/godot#76825`, cited in the JS source. Godot handles it by
//! reporting `Body` anyway and emulating a stream closure, so the request still
//! completes. A port that waits for a reader that never arrives hangs forever,
//! and only for some methods on some browsers.

/// Mirrors `godot_js_fetch_state_t` (`platform/web/http_client_web.h:41-46`).
///
/// ⚠️ The discriminants are the values the C API passes across the boundary and
/// are relied on by the polling loop; `Error` is **-1**, not a large positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum FetchState {
    /// The `fetch()` promise has not resolved yet.
    Requesting = 0,
    /// A response arrived; the body is still being drained.
    Body = 1,
    /// The body has been fully read.
    Done = 2,
    /// The request failed, or the handle is unknown.
    Error = -1,
}

impl FetchState {
    /// The integer the C ABI carries.
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// What the JS side records for one in-flight fetch.
///
/// Field-for-field the parts of `GodotFetch`'s per-request object that
/// `state_get` and `read` consult (`library_godot_fetch.js:80-97`), so the rules
/// below can be read directly against the original.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchProgress {
    /// `obj.error` is set — the promise rejected.
    pub errored: bool,
    /// `obj.response` is set — the `fetch()` promise resolved.
    pub has_response: bool,
    /// `obj.reader` is non-null — a `ReadableStreamDefaultReader` was obtained.
    pub has_reader: bool,
    /// `obj.response.body == null`. See the module note: legal, and not rare.
    pub body_is_null: bool,
    /// `obj.done` — the reader reported `{ done: true }`.
    pub done: bool,
    /// `obj.reading` — a `read()` is outstanding.
    pub reading: bool,
}

/// Ports `godot_js_fetch_state_get` (`library_godot_fetch.js:160-181`) exactly.
///
/// The order of the checks is load-bearing and is preserved verbatim:
///
/// 1. `error` wins over everything, including a response that already arrived.
/// 2. no response yet → `Requesting`.
/// 3. `reader || (body == null && !done)` → `Body`. ⚠️ The second half of that
///    disjunction is the null-body case: with no reader and no body, Godot still
///    reports `Body` so `read` gets a chance to run the emulated closure. Drop
///    it and a `HEAD` request never leaves `Requesting`.
/// 4. `done` → `Done`.
/// 5. anything else → `Error`.
pub fn fetch_state(p: FetchProgress) -> FetchState {
    if p.errored {
        return FetchState::Error;
    }
    if !p.has_response {
        return FetchState::Requesting;
    }
    if p.has_reader || (p.body_is_null && !p.done) {
        return FetchState::Body;
    }
    if p.done {
        return FetchState::Done;
    }
    FetchState::Error
}

/// What a poll of the body should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadAction {
    /// Nothing to do this frame (a read is already outstanding, or there is no
    /// stream to drive).
    Idle,
    /// Issue `reader.read()`.
    Read,
    /// Release the reader: the stream reported completion.
    ReleaseReader,
    /// ⚠️ No reader and no body: synthesise `{ value: undefined, done: true }`
    /// so the lifecycle still closes.
    EmulateClose,
}

/// Ports `GodotFetch.read` (`library_godot_fetch.js:117-134`).
///
/// ⚠️ `ReleaseReader` is checked **before** issuing a read: upstream's branch is
/// `if (obj.reader && !obj.reading) { if (obj.done) { obj.reader = null; return; } … }`,
/// so a stream that has finished is dropped rather than read once more. Reading
/// a completed reader is not merely wasteful — it queues another promise whose
/// resolution re-enters `onread` after the request is considered finished.
pub fn read_action(p: FetchProgress) -> ReadAction {
    if p.has_reader && !p.reading {
        if p.done {
            return ReadAction::ReleaseReader;
        }
        return ReadAction::Read;
    }
    if !p.has_reader && p.body_is_null {
        return ReadAction::EmulateClose;
    }
    ReadAction::Idle
}

/// Whether the response is `Transfer-Encoding: chunked`, per
/// `GodotFetch.onresponse` (`library_godot_fetch.js:52-58`).
///
/// ⚠️ Both the header NAME and its VALUE are lowercased and trimmed before
/// comparison. Header names are case-insensitive by RFC, but so is the value
/// here by Godot's choice, and servers do send `Chunked`. Comparing the raw
/// value misses those.
pub fn is_chunked<'a>(headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> bool {
    headers.into_iter().any(|(name, value)| {
        name.trim().eq_ignore_ascii_case("transfer-encoding")
            && value.trim().eq_ignore_ascii_case("chunked")
    })
}

/// Copies queued body bytes into `out`, returning how many were written.
///
/// Ports the buffer side of `godot_js_fetch_read_chunk`
/// (`library_godot_fetch.js:213-237`), where the caller supplies a fixed buffer
/// and the stream is drained across however many calls it takes.
///
/// ⚠️ **A partially consumed chunk is pushed back at the FRONT.** Dropping the
/// remainder silently truncates the body, and pushing it at the back reorders
/// it — both produce a response that arrives intact for small payloads and
/// corrupt for large ones, which is the worst way for this to fail.
///
/// 🔑 This lives here rather than beside the JS interop for the same reason as
/// the rest of the module: it is real logic with real edge cases, and code
/// behind `#[cfg(target_arch = "wasm32")]` is never type-checked by a native
/// build nor run by any build here. Left in the interop layer it would have been
/// permanently unprovable.
pub fn drain_chunks(chunks: &mut std::collections::VecDeque<Vec<u8>>, out: &mut [u8]) -> usize {
    let mut written = 0usize;
    while written < out.len() {
        let Some(mut chunk) = chunks.pop_front() else {
            break;
        };
        let take = chunk.len().min(out.len() - written);
        out[written..written + take].copy_from_slice(&chunk[..take]);
        written += take;
        if take < chunk.len() {
            chunks.push_front(chunk.split_off(take));
        }
    }
    written
}

/// Applies one `onread` result, mirroring `GodotFetch.onread`
/// (`library_godot_fetch.js:35-45`).
///
/// ⚠️ `reading` is cleared unconditionally, whether or not a value arrived, and
/// `done` is taken from the result rather than inferred from an empty chunk. A
/// zero-length chunk with `done == false` is legal for a stream and must not be
/// read as completion.
pub fn apply_read(p: &mut FetchProgress, got_value: bool, done: bool) -> bool {
    p.reading = false;
    p.done = done;
    got_value
}
