//! pat-s3rrv.78 — the web HTTP client's poll/read lifecycle, held to the C++.
//!
//! Godot ground truth: `platform/web/http_client_web.cpp` — `poll` at :205-256
//! and `read_response_body_chunk` at :159-182.
//!
//! 🔑 These exist because the wasm backend cannot be exercised here at all:
//! `#[cfg(target_arch = "wasm32")]` code is parsed but never type-checked by a
//! native build and never run by any build in this repo. The ordering rules in
//! those two functions are the ones that silently corrupt real responses, so
//! they were deliberately kept out of the target-gated layer and this file is
//! what actually pins them.

use gdplatform::fetch_state::FetchState;
use gdplatform::web_request_driver::{FetchBackend, PollError, WebClientStatus, WebRequestDriver};

/// A backend driven by a script of states, one consumed per observation.
///
/// ⚠️ `state()` is called more than once per frame by design (poll reads it,
/// then `read_response_body_chunk` reads it again), so the script advances on
/// every call rather than per poll — that is what makes an ordering mistake
/// visible instead of being smoothed over by a constant.
///
/// 🔑 **The script is indexed by `state()` CALLS, not by polls.** The three
/// handshake polls consume none: upstream's `Resolving`/`Connecting`/
/// `Connected` arms never reach `godot_js_fetch_state_get`. Nor does the guard
/// in `read_response_body_chunk` that rejects a read outside the body phase.
/// Counting one state per poll silently shifts the whole script by one and
/// makes a test assert against the wrong frame — which is how the first draft
/// of the two `read_response_body_chunk` cases came out red.
struct FakeBackend {
    states: Vec<FetchState>,
    state_idx: std::cell::Cell<usize>,
    status: u16,
    headers: Option<Vec<String>>,
    chunks: Vec<Vec<u8>>,
    header_reads: usize,
    read_calls: usize,
    status_reads: std::cell::Cell<usize>,
}

impl FakeBackend {
    fn new(states: &[FetchState]) -> Self {
        Self {
            states: states.to_vec(),
            state_idx: std::cell::Cell::new(0),
            status: 200,
            headers: Some(vec!["Content-Type: text/plain".to_string()]),
            chunks: Vec::new(),
            header_reads: 0,
            read_calls: 0,
            status_reads: std::cell::Cell::new(0),
        }
    }

    fn with_chunks(mut self, chunks: &[&[u8]]) -> Self {
        self.chunks = chunks.iter().map(|c| c.to_vec()).collect();
        self
    }

    fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    fn with_broken_headers(mut self) -> Self {
        self.headers = None;
        self
    }

    fn with_headers(mut self, lines: &[&str]) -> Self {
        self.headers = Some(lines.iter().map(|l| l.to_string()).collect());
        self
    }
}

impl FetchBackend for FakeBackend {
    fn state(&self) -> FetchState {
        let i = self.state_idx.get();
        // The last scripted state repeats, so a test only has to script the
        // transitions it cares about.
        let s = self.states[i.min(self.states.len() - 1)];
        self.state_idx.set(i + 1);
        s
    }

    fn http_status(&self) -> u16 {
        self.status_reads.set(self.status_reads.get() + 1);
        self.status
    }

    fn read_headers(&mut self) -> Option<Vec<String>> {
        self.header_reads += 1;
        self.headers.clone()
    }

    fn read_chunk(&mut self, out: &mut [u8]) -> usize {
        self.read_calls += 1;
        if self.chunks.is_empty() {
            return 0;
        }
        let mut chunk = self.chunks.remove(0);
        let take = chunk.len().min(out.len());
        out[..take].copy_from_slice(&chunk[..take]);
        if take < chunk.len() {
            self.chunks.insert(0, chunk.split_off(take));
        }
        take
    }
}

/// Walks the driver to `Requesting` the way a real caller does.
fn requesting(backend: &mut FakeBackend) -> WebRequestDriver {
    let mut d = WebRequestDriver::new();
    d.begin_connect();
    assert_eq!(d.status(), WebClientStatus::Resolving);
    d.poll(backend).unwrap();
    assert_eq!(d.status(), WebClientStatus::Connecting);
    d.poll(backend).unwrap();
    assert_eq!(d.status(), WebClientStatus::Connected);
    d.begin_request();
    d
}

// -------------------------------------------------------------- handshake

/// ⚠️ The faked handshake really does cost three polls.
///
/// `Resolving`/`Connecting`/`Connected` each fall through on their own poll
/// (`http_client_web.cpp:208-217`). There is no network behind them on the web,
/// so the temptation is to collapse them — but that changes the earliest frame
/// on which `request_completed` can fire.
#[test]
fn the_faked_handshake_takes_one_poll_per_step() {
    let mut b = FakeBackend::new(&[FetchState::Requesting]);
    let mut d = WebRequestDriver::new();

    d.begin_connect();
    assert_eq!(d.status(), WebClientStatus::Resolving);

    d.poll(&mut b).unwrap();
    assert_eq!(d.status(), WebClientStatus::Connecting);

    d.poll(&mut b).unwrap();
    assert_eq!(d.status(), WebClientStatus::Connected);

    // Connected is stable: it does not advance on its own.
    d.poll(&mut b).unwrap();
    assert_eq!(d.status(), WebClientStatus::Connected);
}

/// Polling an idle client is `ERR_UNCONFIGURED`, not a silent no-op.
#[test]
fn polling_while_disconnected_is_unconfigured() {
    let mut b = FakeBackend::new(&[FetchState::Requesting]);
    let mut d = WebRequestDriver::new();
    assert_eq!(d.poll(&mut b), Err(PollError::Unconfigured));
}

// ----------------------------------------------------------------- poll

#[test]
fn a_pending_fetch_stays_in_requesting_without_reading_headers() {
    let mut b = FakeBackend::new(&[FetchState::Requesting]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap();

    assert_eq!(d.status(), WebClientStatus::Requesting);
    assert_eq!(
        b.header_reads, 0,
        "headers are not read until the body phase"
    );
    assert!(!d.has_response());
}

#[test]
fn a_resolved_fetch_reads_headers_once_and_enters_body() {
    let mut b = FakeBackend::new(&[FetchState::Requesting, FetchState::Body]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap(); // still Requesting
    d.poll(&mut b).unwrap(); // -> Body

    assert_eq!(d.status(), WebClientStatus::Body);
    assert_eq!(d.headers(), &["Content-Type: text/plain".to_string()][..]);
    assert!(d.has_response());
    assert_eq!(b.header_reads, 1, "headers are read exactly once");
}

/// 🔑 The status code is sampled BEFORE the state is inspected — so an errored
/// request still reports the code the server sent.
///
/// ⚠️ Upstream's line order is `polled_response_code = ...;` then
/// `js_state = ...;`. Sampling lazily at the `Requesting → Body` transition
/// looks equivalent and leaves `get_response_code()` at 0 for every failed
/// request, which is exactly when a game most wants to read it.
#[test]
fn the_status_code_is_sampled_even_on_the_poll_that_errors() {
    let mut b = FakeBackend::new(&[FetchState::Error]).with_status(503);
    let mut d = requesting(&mut b);

    assert_eq!(d.poll(&mut b), Err(PollError::ConnectionError));

    assert_eq!(d.status(), WebClientStatus::ConnectionError);
    assert_eq!(
        d.response_code(),
        503,
        "the code is sampled before the state check, so an error keeps it"
    );
}

/// It is also sampled on polls where nothing else happens.
#[test]
fn the_status_code_is_sampled_on_every_requesting_poll() {
    let mut b = FakeBackend::new(&[FetchState::Requesting]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap();
    d.poll(&mut b).unwrap();
    d.poll(&mut b).unwrap();

    assert_eq!(b.status_reads.get(), 3);
}

/// ⚠️ A fetch that is already `Done` on the first poll must still have its
/// headers read.
///
/// Upstream falls through to `read_headers` for both `BODY` and `DONE` — only
/// `REQUESTING` and `ERROR` short-circuit. A small response really can resolve
/// and drain before the client's next frame; treating `Done` as "too late for
/// headers" loses them entirely.
#[test]
fn a_fetch_already_done_on_the_first_poll_still_captures_headers() {
    let mut b = FakeBackend::new(&[FetchState::Done]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap();

    assert_eq!(b.header_reads, 1);
    assert!(d.has_response());
    assert_eq!(
        d.status(),
        WebClientStatus::Body,
        "it enters Body first; the next poll is what completes it"
    );
}

/// A header parse failure is a connection error, not an empty header set.
#[test]
fn unparseable_headers_are_a_connection_error() {
    let mut b = FakeBackend::new(&[FetchState::Body]).with_broken_headers();
    let mut d = requesting(&mut b);

    assert_eq!(d.poll(&mut b), Err(PollError::ConnectionError));
    assert_eq!(d.status(), WebClientStatus::ConnectionError);
}

#[test]
fn a_drained_stream_completes_by_returning_to_disconnected() {
    let mut b = FakeBackend::new(&[FetchState::Body, FetchState::Done]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap(); // -> Body
    assert_eq!(d.status(), WebClientStatus::Body);

    d.poll(&mut b).unwrap(); // Done -> Disconnected
    assert_eq!(
        d.status(),
        WebClientStatus::Disconnected,
        "Disconnected is the terminal SUCCESS state, not 'never started'"
    );
    assert!(d.is_finished());
    assert!(
        d.has_response(),
        "and the captured headers survive completion"
    );
}

/// ⚠️ In the body phase, going BACK to `Requesting` is an error.
///
/// The `else if (state != BODY)` arm catches it. A port that only handles
/// `Done` and otherwise waits will spin forever on a backend that reports a
/// stale or recycled handle.
#[test]
fn slipping_back_to_requesting_during_the_body_phase_is_an_error() {
    let mut b = FakeBackend::new(&[FetchState::Body, FetchState::Requesting]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap(); // -> Body
    assert_eq!(d.poll(&mut b), Err(PollError::ConnectionError));
    assert_eq!(d.status(), WebClientStatus::ConnectionError);
}

#[test]
fn an_error_during_the_body_phase_is_terminal() {
    let mut b = FakeBackend::new(&[FetchState::Body, FetchState::Error]);
    let mut d = requesting(&mut b);

    d.poll(&mut b).unwrap();
    assert_eq!(d.poll(&mut b), Err(PollError::ConnectionError));

    // And it stays errored on every later poll.
    assert_eq!(d.poll(&mut b), Err(PollError::ConnectionError));
    assert!(d.is_finished());
}

// ------------------------------------------------- read_response_body_chunk

/// 🔑 The final chunk is returned on the very call that completes the stream.
///
/// ⚠️ This is the assertion the module exists for. Upstream reads the chunk and
/// only *then* looks at the state; checking `Done` first and returning early
/// loses the last read. With a one-piece body the two orders are
/// indistinguishable, so the bug ships and then truncates every response large
/// enough to need a second read.
#[test]
fn the_chunk_that_completes_the_stream_is_still_returned() {
    let mut b =
        FakeBackend::new(&[FetchState::Body, FetchState::Done]).with_chunks(&[&b"tail"[..]]);
    let mut d = requesting(&mut b);
    d.poll(&mut b).unwrap(); // -> Body

    let mut out = [0u8; 16];
    let n = d.read_response_body_chunk(&mut b, &mut out).unwrap();

    assert_eq!(n, 4, "the read happens BEFORE the state check");
    assert_eq!(&out[..n], b"tail");
    assert_eq!(
        d.status(),
        WebClientStatus::Disconnected,
        "and the same call still observes completion"
    );
}

#[test]
fn reading_drains_across_several_calls_in_order() {
    let mut b = FakeBackend::new(&[FetchState::Body]).with_chunks(&[&b"abc"[..], &b"def"[..]]);
    let mut d = requesting(&mut b);
    d.poll(&mut b).unwrap();

    let mut out = [0u8; 3];
    let mut got = Vec::new();
    for _ in 0..2 {
        let n = d.read_response_body_chunk(&mut b, &mut out).unwrap();
        got.extend_from_slice(&out[..n]);
    }
    assert_eq!(got, b"abcdef");
}

/// ⚠️ A zero-byte read is not completion and not an error.
///
/// Upstream's `if (!read) return chunk;` returns an empty array — but only
/// *after* the state check has already run. The stream stays in the body phase.
#[test]
fn an_empty_read_leaves_the_stream_in_the_body_phase() {
    let mut b = FakeBackend::new(&[FetchState::Body]);
    let mut d = requesting(&mut b);
    d.poll(&mut b).unwrap();

    let mut out = [0u8; 8];
    assert_eq!(d.read_response_body_chunk(&mut b, &mut out).unwrap(), 0);
    assert_eq!(d.status(), WebClientStatus::Body);
    assert!(!d.is_finished());
}

/// Reading outside the body phase is refused (upstream's `ERR_FAIL_COND_V`).
#[test]
fn reading_before_the_body_phase_is_refused() {
    let mut b = FakeBackend::new(&[FetchState::Requesting]).with_chunks(&[&b"nope"[..]]);
    let mut d = requesting(&mut b);

    let mut out = [0u8; 8];
    assert_eq!(
        d.read_response_body_chunk(&mut b, &mut out),
        Err(PollError::ConnectionError)
    );
    assert_eq!(
        d.status(),
        WebClientStatus::Requesting,
        "the guard must not itself change the status"
    );
}

/// A read that errors the stream still hands back what it managed to read.
#[test]
fn a_read_that_errors_the_stream_still_returns_its_bytes() {
    let mut b =
        FakeBackend::new(&[FetchState::Body, FetchState::Error]).with_chunks(&[&b"partial"[..]]);
    let mut d = requesting(&mut b);
    d.poll(&mut b).unwrap();

    let mut out = [0u8; 16];
    let n = d.read_response_body_chunk(&mut b, &mut out).unwrap();

    assert_eq!(&out[..n], b"partial");
    assert_eq!(d.status(), WebClientStatus::ConnectionError);
}

// ---------------------------------------------------------------- reuse

/// `begin_connect` clears the previous response so a reused client cannot
/// report the last request's code or headers.
#[test]
fn beginning_a_new_connection_clears_the_previous_response() {
    let mut b = FakeBackend::new(&[FetchState::Body, FetchState::Done]).with_status(404);
    let mut d = requesting(&mut b);
    d.poll(&mut b).unwrap();
    d.poll(&mut b).unwrap();
    assert_eq!(d.response_code(), 404);
    assert!(d.has_response());

    d.begin_connect();

    assert_eq!(d.response_code(), 0);
    assert!(
        !d.has_response(),
        "stale headers must not leak into the next request"
    );
    let no_headers: &[String] = &[];
    assert_eq!(d.headers(), no_headers);
}

// ============================================================ WebTransfer
//
// 🔑 These cover what used to live inside `#[cfg(target_arch = "wasm32")]`
// `drive_web` — the handshake advance, the drain loop, and the outcome
// classification. None of it was reachable by any test, on any target, because
// a native build parses cfg-gated code and type-checks nothing in it. Moving it
// behind the `FetchBackend` trait is what makes the frame stepping provable.

use gdplatform::web_request_driver::{
    split_header_lines, TransferOutcome, WebTransfer, DEFAULT_DOWNLOAD_CHUNK_SIZE,
};

/// ⚠️ The state script is indexed by `state()` CALLS. A transfer spends its
/// first two `step`s on the handshake, which consumes NONE — so the first
/// scripted state is read on the third `step`.
fn stepped(b: FakeBackend) -> WebTransfer<FakeBackend> {
    WebTransfer::new(b)
}

#[test]
fn a_transfer_spends_its_handshake_frames_before_the_request_goes_live() {
    let mut t = stepped(FakeBackend::new(&[FetchState::Requesting]));

    assert_eq!(t.status(), WebClientStatus::Resolving);
    assert!(t.step().is_none());
    assert_eq!(t.status(), WebClientStatus::Connecting);
    assert!(t.step().is_none());
    assert_eq!(t.status(), WebClientStatus::Connected);
    assert!(t.step().is_none());
    assert_eq!(
        t.status(),
        WebClientStatus::Requesting,
        "the request goes live only after the faked handshake"
    );
}

#[test]
fn a_transfer_completes_with_its_body_and_headers() {
    let mut t = stepped(
        FakeBackend::new(&[FetchState::Body, FetchState::Done]).with_chunks(&[&b"hello"[..]]),
    );
    assert!(t.step().is_none());
    assert!(t.step().is_none());

    let outcome = t.step().expect("finishes on the frame the stream closes");
    assert_eq!(
        outcome,
        TransferOutcome::Completed {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            body: b"hello".to_vec(),
        }
    );
}

/// 🔑 Several stream chunks are concatenated in ONE frame.
///
/// ⚠️ This is the drain loop, and it had no coverage before. `read_chunk`
/// returns one stream chunk per call, so without the loop a multi-chunk
/// response still arrives — just one chunk per frame. That is not a crash and
/// not corruption; it is a response that takes hundreds of frames and looks
/// like a slow network.
#[test]
fn several_stream_chunks_are_concatenated_in_one_frame() {
    let first = vec![b'a'; 3000];
    let second = vec![b'b'; 3000];
    let mut t = stepped(
        FakeBackend::new(&[
            FetchState::Body,
            FetchState::Body,
            FetchState::Body,
            FetchState::Done,
        ])
        .with_chunks(&[&first[..], &second[..]]),
    );
    assert!(t.step().is_none());
    assert!(t.step().is_none());

    let outcome = t.step().expect("completes");
    match outcome {
        TransferOutcome::Completed { body, .. } => {
            assert_eq!(body.len(), 6000, "both chunks, in one frame");
            assert!(body[..3000].iter().all(|&b| b == b'a'));
            assert!(body[3000..].iter().all(|&b| b == b'b'), "and in order");
        }
        other => panic!("expected completion, got {other:?}"),
    }
}

/// ⚠️ A failed transfer reports the code the server sent, not 0.
#[test]
fn a_failed_transfer_carries_the_sampled_status_code() {
    let mut t = stepped(FakeBackend::new(&[FetchState::Error]).with_status(503));
    assert!(t.step().is_none());
    assert!(t.step().is_none());

    assert_eq!(t.step(), Some(TransferOutcome::Failed { status: 503 }));
}

/// ⚠️ Spent after it yields an outcome: the body is moved out, so a second call
/// could only report the same completion with an EMPTY body.
#[test]
fn a_finished_transfer_yields_its_outcome_exactly_once() {
    let mut t =
        stepped(FakeBackend::new(&[FetchState::Body, FetchState::Done]).with_chunks(&[&b"x"[..]]));
    assert!(t.step().is_none());
    assert!(t.step().is_none());
    assert!(t.step().is_some());

    assert!(
        t.step().is_none(),
        "a spent transfer must not hand out a second, empty completion"
    );
}

/// A transfer whose fetch is still pending just keeps returning `None`.
#[test]
fn a_pending_transfer_reports_nothing() {
    let mut t = stepped(FakeBackend::new(&[FetchState::Requesting]));
    for _ in 0..6 {
        assert!(t.step().is_none());
    }
    assert_eq!(t.status(), WebClientStatus::Requesting);
}

// ------------------------------------------------------- split_header_lines

/// 🔑 Split on the FIRST colon — values contain more of them.
#[test]
fn a_header_value_containing_colons_survives_intact() {
    let lines = vec![
        "Location: https://example.com:8443/next".to_string(),
        "Date: Tue, 19 Aug 2026 21:00:00 GMT".to_string(),
    ];
    assert_eq!(
        split_header_lines(&lines),
        vec![
            (
                "Location".to_string(),
                "https://example.com:8443/next".to_string()
            ),
            (
                "Date".to_string(),
                "Tue, 19 Aug 2026 21:00:00 GMT".to_string()
            ),
        ]
    );
}

#[test]
fn header_names_and_values_are_trimmed_and_junk_lines_dropped() {
    let lines = vec![
        "  X-Spaced  :   yes  ".to_string(),
        "HTTP/1.1 200 OK".to_string(), // the status line is not a header
        String::new(),
    ];
    assert_eq!(
        split_header_lines(&lines),
        vec![("X-Spaced".to_string(), "yes".to_string())],
        "a line with no colon is not a header and must be dropped"
    );
}

/// The transfer path and the raw splitter agree.
#[test]
fn a_transfer_splits_the_headers_it_reports() {
    let mut t = stepped(
        FakeBackend::new(&[FetchState::Body, FetchState::Done])
            .with_headers(&["X-A: 1", "X-B: two:three"]),
    );
    assert!(t.step().is_none());
    assert!(t.step().is_none());

    match t.step().expect("completes") {
        TransferOutcome::Completed { headers, .. } => assert_eq!(
            headers,
            vec![
                ("X-A".to_string(), "1".to_string()),
                ("X-B".to_string(), "two:three".to_string()),
            ]
        ),
        other => panic!("expected completion, got {other:?}"),
    }
}

// ------------------------------------------------------- download_chunk_size

/// 🔑 The read buffer is 65536, matching `HTTPRequest.download_chunk_size`.
///
/// ⚠️ I shipped 4096 here first. That is precisely the value Godot's class
/// reference offers as a deliberate *reduction* — "set this to a lower value
/// (e.g. 4096 for 4 KiB) when downloading small files to decrease memory usage
/// at the cost of download speeds" — so defaulting to it silently hands every
/// download that trade-off, at 16x the read iterations. Upstream defaults are
/// `read_limit = 65536` (`platform/web/http_client_web.h:66`) and
/// `read_chunk_size = 65536` (`core/io/http_client_tcp.h:75`).
#[test]
fn the_default_chunk_size_matches_godot() {
    assert_eq!(DEFAULT_DOWNLOAD_CHUNK_SIZE, 65536);
    let t = WebTransfer::new(FakeBackend::new(&[FetchState::Requesting]));
    assert_eq!(t.chunk_size(), 65536);
}

/// A body larger than one buffer is split across reads and fully accumulated.
///
/// ⚠️ This is the test that would have caught the 4096 default. With a 65536
/// buffer a 70000-byte chunk takes two reads plus the empty one that observes
/// closure; with a 4096 buffer it needs eighteen, so the scripted stream closes
/// underneath it and the body arrives TRUNCATED — not merely slow.
#[test]
fn a_body_larger_than_the_buffer_is_split_across_reads() {
    let big = vec![b'z'; 70_000];
    let mut t = WebTransfer::new(
        FakeBackend::new(&[
            FetchState::Body,
            FetchState::Body,
            FetchState::Body,
            FetchState::Done,
        ])
        .with_chunks(&[&big[..]]),
    );
    assert!(t.step().is_none());
    assert!(t.step().is_none());

    match t.step().expect("completes") {
        TransferOutcome::Completed { body, .. } => {
            assert_eq!(body.len(), 70_000, "the whole body, not one buffer of it");
            assert!(body.iter().all(|&b| b == b'z'));
        }
        other => panic!("expected completion, got {other:?}"),
    }
    assert_eq!(
        t.backend().read_calls,
        3,
        "65536 + 4464 + the empty read that observes closure"
    );
}

/// `set_download_chunk_size` actually changes how much is read per iteration.
#[test]
fn a_configured_chunk_size_is_honoured() {
    let payload = vec![b'q'; 2500];
    let mut t = WebTransfer::new(
        FakeBackend::new(&[
            FetchState::Body,
            FetchState::Body,
            FetchState::Body,
            FetchState::Body,
            FetchState::Done,
        ])
        .with_chunks(&[&payload[..]]),
    );
    t.set_chunk_size(1000);
    assert_eq!(t.chunk_size(), 1000);

    assert!(t.step().is_none());
    assert!(t.step().is_none());

    match t.step().expect("completes") {
        TransferOutcome::Completed { body, .. } => assert_eq!(body.len(), 2500),
        other => panic!("expected completion, got {other:?}"),
    }
    assert_eq!(
        t.backend().read_calls,
        4,
        "1000 + 1000 + 500 + the empty read"
    );
}

/// ⚠️ A zero chunk size is floored to 1, not honoured.
///
/// A zero-length buffer makes `read_chunk` return 0 forever, so the drain loop
/// breaks every frame and the body never arrives — a hang with no error.
#[test]
fn a_zero_chunk_size_is_floored() {
    let mut t = WebTransfer::new(FakeBackend::new(&[FetchState::Requesting]));
    t.set_chunk_size(0);
    assert_eq!(t.chunk_size(), 1);
}
