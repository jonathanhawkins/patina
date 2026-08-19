//! pat-s3rrv.78 — the web `fetch` lifecycle rules, checked against the JS source.
//!
//! Godot ground truth: `platform/web/js/libs/library_godot_fetch.js`
//! (`state_get` at :160-181, `read` at :117-134, `onresponse` at :47-67,
//! `onread` at :35-45).
//!
//! 🔑 These tests exist because the wasm backend itself cannot be exercised
//! here. `#[cfg(target_arch = "wasm32")]` code is parsed but never type-checked
//! by a native build, and never *run* by any build in this repo. So the rules
//! that are easy to get wrong were deliberately kept out of the target-gated
//! layer, and this file is what actually holds them to the original.

use gdplatform::fetch_state::{
    apply_read, fetch_state, is_chunked, read_action, FetchProgress, FetchState, ReadAction,
};

/// A fetch that has resolved and is streaming: the ordinary case.
fn streaming() -> FetchProgress {
    FetchProgress {
        has_response: true,
        has_reader: true,
        ..Default::default()
    }
}

// ------------------------------------------------------------- state_get

#[test]
fn the_state_ordinals_match_the_c_enum() {
    // `godot_js_fetch_state_t` (http_client_web.h:41-46). ⚠️ Error is -1.
    assert_eq!(FetchState::Requesting.as_i32(), 0);
    assert_eq!(FetchState::Body.as_i32(), 1);
    assert_eq!(FetchState::Done.as_i32(), 2);
    assert_eq!(FetchState::Error.as_i32(), -1);
}

#[test]
fn a_pending_promise_is_requesting() {
    assert_eq!(
        fetch_state(FetchProgress::default()),
        FetchState::Requesting
    );
}

#[test]
fn a_resolved_response_with_a_reader_is_body() {
    assert_eq!(fetch_state(streaming()), FetchState::Body);
}

#[test]
fn a_drained_stream_is_done() {
    let p = FetchProgress {
        has_reader: false,
        done: true,
        ..streaming()
    };
    assert_eq!(fetch_state(p), FetchState::Done);
}

/// 🔑 The null-body case: `Body`, not `Requesting` and not `Done`.
///
/// ⚠️ This is the one that hangs a real game. `response.body` is legitimately
/// null for `HEAD` (and Chromium/Firefox disagree — godotengine/godot#76825).
/// Upstream's condition is `reader || (body == null && !done)`; an
/// implementation that only checks for a reader leaves the request stuck in
/// `Requesting` forever, with no error and nothing to debug.
#[test]
fn a_null_body_response_reports_body_so_the_lifecycle_can_close() {
    let p = FetchProgress {
        has_response: true,
        has_reader: false,
        body_is_null: true,
        ..Default::default()
    };
    assert_eq!(
        fetch_state(p),
        FetchState::Body,
        "a null body must still report Body, or a HEAD request never completes"
    );

    // …and once the emulated closure has run, it completes normally.
    let closed = FetchProgress { done: true, ..p };
    assert_eq!(fetch_state(closed), FetchState::Done);
}

/// ⚠️ An error outranks a response that already arrived.
#[test]
fn an_error_wins_over_a_resolved_response() {
    let p = FetchProgress {
        errored: true,
        ..streaming()
    };
    assert_eq!(
        fetch_state(p),
        FetchState::Error,
        "the error check is first in state_get and must stay first"
    );
}

/// A response with no reader, no null body and not done is malformed → Error.
///
/// This is upstream's final fall-through. Returning `Requesting` here instead
/// would be the "safe-looking" choice and would spin forever.
#[test]
fn a_response_with_no_stream_and_no_completion_is_an_error() {
    let p = FetchProgress {
        has_response: true,
        ..Default::default()
    };
    assert_eq!(fetch_state(p), FetchState::Error);
}

// ----------------------------------------------------------------- read

#[test]
fn a_ready_stream_is_read() {
    assert_eq!(read_action(streaming()), ReadAction::Read);
}

/// ⚠️ An outstanding read is not re-issued.
#[test]
fn a_read_in_flight_is_left_alone() {
    let p = FetchProgress {
        reading: true,
        ..streaming()
    };
    assert_eq!(read_action(p), ReadAction::Idle);
}

/// 🔑 A finished stream releases its reader instead of being read again.
///
/// Upstream checks `obj.done` INSIDE the `reader && !reading` branch and
/// returns early. Reading a completed reader queues another promise that
/// re-enters `onread` after the request is already finished.
#[test]
fn a_completed_stream_releases_the_reader_rather_than_reading_again() {
    let p = FetchProgress {
        done: true,
        ..streaming()
    };
    assert_eq!(read_action(p), ReadAction::ReleaseReader);
}

/// The null-body path drives the emulated closure.
#[test]
fn a_null_body_emulates_a_stream_closure() {
    let p = FetchProgress {
        has_response: true,
        has_reader: false,
        body_is_null: true,
        ..Default::default()
    };
    assert_eq!(read_action(p), ReadAction::EmulateClose);
}

// ------------------------------------------------------------- onresponse

#[test]
fn chunked_is_detected_case_insensitively_on_both_halves() {
    assert!(is_chunked([("Transfer-Encoding", "chunked")]));
    // ⚠️ Godot lowercases the VALUE too, and servers do send this spelling.
    assert!(is_chunked([("transfer-encoding", "Chunked")]));
    assert!(is_chunked([("TRANSFER-ENCODING", " CHUNKED ")]));
    assert!(is_chunked([
        ("Content-Type", "text/plain"),
        ("Transfer-Encoding", "chunked"),
    ]));
}

#[test]
fn a_non_chunked_response_is_not_flagged() {
    assert!(!is_chunked([("Content-Length", "12")]));
    assert!(!is_chunked([("Transfer-Encoding", "gzip")]));
    assert!(!is_chunked(std::iter::empty()));
    // ⚠️ Not a substring match: `chunked` must be the whole value.
    assert!(!is_chunked([("Transfer-Encoding", "chunked-ish")]));
}

// ----------------------------------------------------------------- onread

/// ⚠️ A zero-length chunk with `done == false` is legal and is NOT completion.
#[test]
fn an_empty_chunk_does_not_end_the_stream() {
    let mut p = FetchProgress {
        reading: true,
        ..streaming()
    };
    let got = apply_read(&mut p, false, false);

    assert!(!got, "no value arrived");
    assert!(!p.done, "an empty chunk must not be read as completion");
    assert!(!p.reading, "the outstanding read is always cleared");
    assert_eq!(
        fetch_state(p),
        FetchState::Body,
        "and the request stays in Body so the next poll reads again"
    );
}

#[test]
fn a_final_chunk_completes_the_stream() {
    let mut p = FetchProgress {
        reading: true,
        ..streaming()
    };
    assert!(apply_read(&mut p, true, true));
    assert!(p.done);
    assert_eq!(read_action(p), ReadAction::ReleaseReader);
}
