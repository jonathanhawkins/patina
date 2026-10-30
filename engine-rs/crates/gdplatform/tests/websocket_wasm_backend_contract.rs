//! The portable half of the wasm32 browser-WebSocket backend (pat-s3rrv.264).
//!
//! The browser transport in `gdplatform::websocket_web` is split so that
//! everything except the `js_sys::Reflect` plumbing compiles and runs on native:
//! the `readyState` mapping, the text/binary receive queue with its
//! `max_queued_packets` cap, and the server-role policy that makes
//! `WebSocketMultiplayerPeer::create_server` answer `ERR_UNAVAILABLE` in a Web
//! build. Those three are what this file pins.
//!
//! ⚠️ **What this file cannot prove.** `cfg(target_arch = "wasm32")` code is not
//! compiled into a native test binary, so no assertion here exercises the wasm
//! branch of `create_server` or a single line of the `browser` module. The
//! `cargo check -p gdplatform --target wasm32-unknown-unknown` gate is what
//! covers those, and a browser run (`scripts/web.sh --runtime`) is what covers
//! the event plumbing. Every test below is therefore written to fail on a
//! *wrong* policy rather than merely to agree with the code that implements it:
//! the queue tests distinguish refuse-newest from evict-oldest, and the error
//! tests assert the code is `ERR_UNAVAILABLE` **and not** `FAILED`.

use gdplatform::network::{NetworkError, ERR_UNAVAILABLE, FAILED};
use gdplatform::websocket_multiplayer::{
    server_role_available, web_create_server_error, WebSocketMultiplayerPeer,
};
use gdplatform::websocket_peer::{WebSocketError, WebSocketPeer, WebSocketState};
use gdplatform::websocket_web::{
    ready_state_from_browser, BrowserPacketQueue, DEFAULT_MAX_QUEUED_PACKETS, READY_STATE_CLOSED,
    READY_STATE_CLOSING, READY_STATE_CONNECTING, READY_STATE_OPEN,
};

// ---------------------------------------------------------------------------
// readyState mapping
// ---------------------------------------------------------------------------

/// The browser constants are the WHATWG values, not something derived.
///
/// Pinned as literals on purpose: the mapping function below is an identity in
/// practice, so a transposed constant would make every mapping assertion agree
/// with itself and still be wrong on a real socket.
#[test]
fn browser_ready_state_constants_are_the_whatwg_values() {
    assert_eq!(READY_STATE_CONNECTING, 0);
    assert_eq!(READY_STATE_OPEN, 1);
    assert_eq!(READY_STATE_CLOSING, 2);
    assert_eq!(READY_STATE_CLOSED, 3);
}

#[test]
fn each_browser_ready_state_maps_to_its_godot_state() {
    assert_eq!(
        ready_state_from_browser(READY_STATE_CONNECTING),
        WebSocketState::Connecting
    );
    assert_eq!(
        ready_state_from_browser(READY_STATE_OPEN),
        WebSocketState::Open
    );
    assert_eq!(
        ready_state_from_browser(READY_STATE_CLOSING),
        WebSocketState::Closing
    );
    assert_eq!(
        ready_state_from_browser(READY_STATE_CLOSED),
        WebSocketState::Closed
    );
}

/// Godot's `WebSocketPeer.State` and the browser's `readyState` are two separate
/// specs that happen to agree numerically; this is the round trip that says so.
///
/// If either side renumbers, this fails here rather than in a browser, where the
/// symptom would be a client stuck in the wrong state with no error.
#[test]
fn the_mapping_round_trips_every_godot_state() {
    for state in [
        WebSocketState::Connecting,
        WebSocketState::Open,
        WebSocketState::Closing,
        WebSocketState::Closed,
    ] {
        assert_eq!(
            ready_state_from_browser(state.as_godot_code()),
            state,
            "{state:?} must survive a trip through the browser's numbering"
        );
    }
}

/// ⚠️ An unreadable `readyState` must read as CLOSED, never CONNECTING.
///
/// `Reflect::get` on a socket the browser has torn down can answer `undefined`,
/// which the transport turns into a sentinel. Mapping that to CONNECTING would
/// park a client forever, waiting for a handshake that can never complete — a
/// hang, not an error, which is the expensive kind of wrong.
#[test]
fn an_unknown_ready_state_reads_as_closed() {
    for raw in [-1, 4, 99, i32::MIN, i32::MAX] {
        assert_eq!(
            ready_state_from_browser(raw),
            WebSocketState::Closed,
            "readyState {raw} is not a state the browser defines"
        );
    }
}

// ---------------------------------------------------------------------------
// Receive queue: text vs binary framing
// ---------------------------------------------------------------------------

/// `MessageEvent.data` is a JS string for a text frame and an `ArrayBuffer` for
/// a binary one, and the distinction has to reach `was_string_packet()`.
#[test]
fn the_queue_keeps_each_packets_frame_type_and_bytes() {
    let mut q = BrowserPacketQueue::new();
    q.push_text("hello");
    q.push_binary(vec![0x00, 0xff, 0x7f]);
    q.push_text("");

    assert_eq!(q.len(), 3);
    // Asserting the payload AND the flag together: a transport that mixed up
    // the two paths would still produce three packets in the right order.
    assert_eq!(q.pop(), Some((b"hello".to_vec(), true)));
    assert_eq!(q.pop(), Some((vec![0x00, 0xff, 0x7f], false)));
    assert_eq!(q.pop(), Some((Vec::new(), true)), "an empty text frame");
    assert_eq!(q.pop(), None);
    assert!(q.is_empty());
}

/// A binary frame whose bytes are valid UTF-8 is still a binary frame.
///
/// This is the case that a "guess from the payload" implementation gets wrong,
/// and it is not hypothetical: the multiplayer envelope in
/// `websocket_multiplayer.rs` is binary and frequently all-ASCII.
#[test]
fn printable_bytes_do_not_make_a_binary_packet_a_string_packet() {
    let mut q = BrowserPacketQueue::new();
    q.push_binary(b"plain ascii".to_vec());
    assert_eq!(q.pop(), Some((b"plain ascii".to_vec(), false)));
}

/// Text payloads are stored as UTF-8, so `get_string_from_utf8()` reads back
/// what was sent — including non-ASCII.
#[test]
fn text_packets_are_stored_as_utf8() {
    let mut q = BrowserPacketQueue::new();
    q.push_text("héllo ☃");
    let (bytes, was_string) = q.pop().expect("one packet");
    assert!(was_string);
    assert_eq!(String::from_utf8(bytes).expect("valid utf-8"), "héllo ☃");
}

#[test]
fn an_empty_queue_pops_nothing_and_has_refused_nothing() {
    let mut q = BrowserPacketQueue::new();
    assert_eq!(q.len(), 0);
    assert!(q.is_empty());
    assert_eq!(q.pop(), None);
    assert_eq!(q.refused(), 0);
    assert_eq!(q.max_queued_packets(), DEFAULT_MAX_QUEUED_PACKETS);
}

#[test]
fn the_default_cap_is_godots_4096() {
    assert_eq!(DEFAULT_MAX_QUEUED_PACKETS, 4096);
}

/// ⚠️ A full queue REFUSES the newest packet; it does not evict the oldest.
///
/// This is the assertion that tells the two policies apart. Both leave `len()`
/// at the cap, so checking the length proves nothing — the surviving payloads
/// are what distinguishes them. Evicting the oldest is the intuitive choice for
/// a bounded queue and silently discards data the application was already told
/// it would receive; Godot refuses instead
/// (`ERR_FAIL_COND_V(queued >= max, ERR_OUT_OF_MEMORY)`, `wsl_peer.cpp:783`).
#[test]
fn a_full_queue_refuses_the_newest_packet_rather_than_evicting_the_oldest() {
    let mut q = BrowserPacketQueue::with_max(2);
    q.push_text("first");
    q.push_text("second");
    q.push_text("third");

    assert_eq!(q.len(), 2, "the cap holds");
    assert_eq!(q.refused(), 1, "the drop is counted, not silent");
    assert_eq!(
        q.pop(),
        Some((b"first".to_vec(), true)),
        "the OLDEST packet survived — an evict-oldest queue would hold 'second'"
    );
    assert_eq!(q.pop(), Some((b"second".to_vec(), true)));
    assert_eq!(q.pop(), None, "'third' was refused, so it is not queued");
}

/// Draining frees the room back up — the cap bounds what is *queued*, not how
/// many packets the socket may ever deliver.
#[test]
fn room_freed_by_draining_accepts_further_packets() {
    let mut q = BrowserPacketQueue::with_max(1);
    q.push_text("a");
    q.push_text("b");
    assert_eq!(q.refused(), 1);

    assert_eq!(q.drain(), vec![(b"a".to_vec(), true)]);
    q.push_text("c");
    assert_eq!(q.len(), 1);
    assert_eq!(q.pop(), Some((b"c".to_vec(), true)));
    assert_eq!(q.refused(), 1, "the refusal count is cumulative");
}

#[test]
fn drain_returns_every_packet_oldest_first_and_empties_the_queue() {
    let mut q = BrowserPacketQueue::new();
    q.push_text("one");
    q.push_binary(vec![2]);
    assert_eq!(
        q.drain(),
        vec![(b"one".to_vec(), true), (vec![2], false)],
        "drain preserves arrival order and frame types"
    );
    assert!(q.is_empty());
}

/// ⚠️ A cap of zero would refuse every inbound message, which reads as a dead
/// socket rather than as a misconfiguration, so it is raised to one — matching
/// `WebSocketPeer::set_max_queued_packets`, which clamps with `max(1)`.
#[test]
fn a_zero_cap_is_raised_to_one_rather_than_blackholing_the_socket() {
    let mut q = BrowserPacketQueue::with_max(0);
    assert_eq!(q.max_queued_packets(), 1);
    q.push_text("still delivered");
    assert_eq!(q.pop(), Some((b"still delivered".to_vec(), true)));
}

#[test]
fn recapping_keeps_already_queued_packets() {
    let mut q = BrowserPacketQueue::with_max(4);
    q.push_text("a");
    q.push_text("b");
    q.set_max_queued_packets(1);

    assert_eq!(q.max_queued_packets(), 1);
    assert_eq!(
        q.len(),
        2,
        "shrinking the cap does not discard what is held"
    );
    q.push_text("c");
    assert_eq!(q.refused(), 1, "but nothing new fits until it drains");
    assert_eq!(q.len(), 2);
}

#[test]
fn clear_drops_queued_packets_and_keeps_the_refusal_count() {
    let mut q = BrowserPacketQueue::with_max(1);
    q.push_text("a");
    q.push_text("refused");
    q.clear();
    assert!(q.is_empty());
    assert_eq!(q.refused(), 1);
}

// ---------------------------------------------------------------------------
// Server role: ERR_UNAVAILABLE on the wasm path
// ---------------------------------------------------------------------------

/// The Godot error-code positions this file asserts against.
#[test]
fn the_godot_error_codes_are_the_enum_positions() {
    assert_eq!(FAILED, 1);
    assert_eq!(ERR_UNAVAILABLE, 2);
}

/// `create_server` in a Web build answers `ERR_UNAVAILABLE`, not `FAILED`.
///
/// Godot's web platform stubs the listen socket out entirely
/// (`platform/web/net_socket_web.h:51-60`) and
/// `WebSocketMultiplayerPeer::create_server` reports `ERR_UNAVAILABLE` there.
/// The distinction is load-bearing: `FAILED` invites a script to retry, and no
/// number of retries will give a browser page a listen socket.
#[test]
fn the_web_server_role_error_is_err_unavailable_and_not_failed() {
    let err = web_create_server_error();
    assert!(
        matches!(err, NetworkError::Unavailable(_)),
        "expected Unavailable, got {err:?}"
    );
    assert_eq!(err.godot_error_code(), ERR_UNAVAILABLE);
    assert_ne!(
        err.godot_error_code(),
        FAILED,
        "a browser's missing server role is not a generic failure"
    );
}

/// The code mapping discriminates rather than answering `ERR_UNAVAILABLE` for
/// everything — the control for the test above.
#[test]
fn every_other_network_error_still_reports_failed() {
    for err in [
        NetworkError::NoPeer,
        NetworkError::Transport("socket closed".into()),
        NetworkError::UnregisteredRPC("shoot".into()),
        NetworkError::PeerUnreachable(gdplatform::network::PeerId(3)),
    ] {
        assert_eq!(
            err.godot_error_code(),
            FAILED,
            "{err:?} must keep the generic FAILED code"
        );
    }
}

/// The native host HAS a server role, and `create_server` really takes it.
///
/// This is the control for the early return added to `create_server`: a
/// mistakenly inverted or unconditional guard would refuse here, so the native
/// server role is pinned by a binding socket rather than by reading the policy
/// function back to itself.
#[test]
fn the_native_build_has_a_server_role_and_binds() {
    assert!(server_role_available(), "native hosts can listen");

    let mut peer = WebSocketMultiplayerPeer::new();
    let addr = peer
        .create_server(("127.0.0.1", 0))
        .expect("a native build binds a listener");
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert_ne!(addr.port(), 0, "an ephemeral port was assigned");
}

// ---------------------------------------------------------------------------
// The native transport is unchanged by the seam
// ---------------------------------------------------------------------------

/// The five `browser_*` shims decline on native, so `connect_to_url` still
/// refuses `wss://` here.
///
/// ⚠️ This is the regression guard for the seam itself: if the browser shim ever
/// returned `Some(..)` on a native build, `wss://` would appear to connect and
/// then never open, because there is no TLS stack behind it.
#[test]
fn native_connect_to_url_still_refuses_wss() {
    let mut peer = WebSocketPeer::new();
    let err = peer
        .connect_to_url("wss://example.invalid/socket")
        .expect_err("native builds have no TLS stack");
    assert!(
        matches!(err, WebSocketError::TlsUnsupported),
        "expected TlsUnsupported, got {err:?}"
    );
    assert_eq!(peer.get_ready_state(), WebSocketState::Closed);
}

#[test]
fn a_non_websocket_scheme_is_refused_before_any_transport_is_chosen() {
    let mut peer = WebSocketPeer::new();
    let err = peer
        .connect_to_url("http://example.invalid/socket")
        .expect_err("http:// is not a WebSocket URL");
    assert!(
        matches!(err, WebSocketError::InvalidUrl(_)),
        "expected InvalidUrl, got {err:?}"
    );
}

/// A fresh peer reports no outbound backlog: the browser shim declines and the
/// native path keeps no outbound queue.
#[test]
fn a_fresh_peer_reports_no_buffered_outbound_bytes() {
    let peer = WebSocketPeer::new();
    assert_eq!(peer.get_current_outbound_buffered_amount(), 0);
    assert_eq!(peer.get_ready_state(), WebSocketState::Closed);
    assert_eq!(peer.get_close_code(), -1);
}

/// `WebSocketPeer::new()` must not panic.
///
/// ⚠️ Trivial-looking and not trivial: it calls `Instant::now()` for the
/// heartbeat clock, and `std::time::Instant::now()` *panics* on
/// `wasm32-unknown-unknown`. The constructor now uses `web_time::Instant`,
/// which this exercises on native and which is the only reason a browser build
/// can construct a peer at all (pat-s3rrv.264).
#[test]
fn constructing_a_peer_reads_a_clock_that_exists_on_every_target() {
    let a = WebSocketPeer::new();
    let b = WebSocketPeer::default();
    assert_eq!(a.get_ready_state(), WebSocketState::Closed);
    assert_eq!(b.get_heartbeat_interval(), 0.0, "Godot's default");
    assert_eq!(b.get_max_queued_packets(), 4096, "Godot's default");
}
