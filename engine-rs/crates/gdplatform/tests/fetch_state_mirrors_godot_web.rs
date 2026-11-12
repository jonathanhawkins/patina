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

// ------------------------------------------------------------ read_chunk

use gdplatform::fetch_state::drain_chunks;
use std::collections::VecDeque;

fn queue(chunks: &[&[u8]]) -> VecDeque<Vec<u8>> {
    chunks.iter().map(|c| c.to_vec()).collect()
}

#[test]
fn draining_an_empty_queue_writes_nothing() {
    let mut q: VecDeque<Vec<u8>> = VecDeque::new();
    let mut out = [0u8; 8];
    assert_eq!(drain_chunks(&mut q, &mut out), 0);
}

#[test]
fn a_buffer_larger_than_the_queue_takes_everything_in_order() {
    let mut q = queue(&[&b"abc"[..], &b"de"[..], &b"f"[..]]);
    let mut out = [0u8; 16];

    assert_eq!(drain_chunks(&mut q, &mut out), 6);
    assert_eq!(&out[..6], b"abcdef");
    assert!(q.is_empty(), "the queue is fully drained");
}

/// 🔑 A chunk that does not fit is split, and the REMAINDER stays at the front.
///
/// ⚠️ This is the assertion that matters. Dropping the tail silently truncates
/// the body; pushing it at the back reorders it. Both are invisible whenever a
/// response happens to fit the buffer, which is most hand tests — so the failure
/// only shows up on large payloads.
#[test]
fn a_partially_consumed_chunk_resumes_where_it_stopped() {
    let mut q = queue(&[&b"HelloWorld"[..], &b"!!"[..]]);
    let mut out = [0u8; 4];

    assert_eq!(drain_chunks(&mut q, &mut out), 4);
    assert_eq!(&out, b"Hell");

    assert_eq!(drain_chunks(&mut q, &mut out), 4);
    assert_eq!(&out, b"oWor", "the split resumes, it does not restart");

    assert_eq!(drain_chunks(&mut q, &mut out), 4);
    assert_eq!(&out, b"ld!!", "and it flows into the next chunk");

    assert_eq!(drain_chunks(&mut q, &mut out), 0, "nothing left");
}

/// A chunk that exactly fills the buffer leaves no empty remainder behind.
#[test]
fn an_exactly_filling_chunk_leaves_the_queue_clean() {
    let mut q = queue(&[&b"abcd"[..]]);
    let mut out = [0u8; 4];

    assert_eq!(drain_chunks(&mut q, &mut out), 4);
    assert_eq!(&out, b"abcd");
    assert!(
        q.is_empty(),
        "an exact fit must not push back a zero-length remainder, got {q:?}"
    );
}

/// A zero-length output buffer consumes nothing — it must not eat a chunk.
#[test]
fn a_zero_length_buffer_consumes_nothing() {
    let mut q = queue(&[&b"abc"[..]]);
    let mut out: [u8; 0] = [];

    assert_eq!(drain_chunks(&mut q, &mut out), 0);
    assert_eq!(q.len(), 1, "the chunk must survive a zero-length read");
    assert_eq!(q[0], b"abc".to_vec());
}
