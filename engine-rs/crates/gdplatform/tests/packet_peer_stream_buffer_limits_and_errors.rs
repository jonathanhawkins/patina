//! pat-s3rrv.235 — `PacketPeerStream`'s buffer limits and Godot error codes.
//!
//! Godot ground truth: `upstream/godot/core/io/packet_peer.cpp`
//! (`put_packet`, `get_packet`, `get_input_buffer_max_size`,
//! `set_input_buffer_max_size`) and `core/register_core_types.cpp:350` for the
//! `network/limits/packet_peer_stream/max_buffer_po2` default of 16.
//!
//! # ⚠️ Why these live here and not beside the framing unit tests
//!
//! `gdplatform`'s lib suite is **697 tests**, and
//! `export::tests::executor_run_with_multiple_platforms` fails at HEAD
//! ("packaging failed for web" — it needs an export template this environment
//! has no way to provide). With nextest's fail-fast that kills the run at
//! test 141, so anything reached later is unverifiable through `--lib`.
//! An integration target is independently runnable and cannot be gated by a
//! broken sibling.
//!
//! # ⚠️ Two numbers the bead gets wrong
//!
//! - Oversize is **`ERR_INVALID_PARAMETER` (31)**, not `ERR_OUT_OF_MEMORY`.
//!   `ERR_OUT_OF_MEMORY` occurs once in `packet_peer.cpp`, on
//!   `PacketPeer::put_var` exceeding `encode_buffer_max_size` — a different
//!   method and a different buffer.
//! - The buffer accessors report `size - 4`, so the default is **65532**.

use gdplatform::PacketPeerStream;

/// 🔑 The default max size is **65532**, not 65536.
///
/// Godot sizes the buffer to `1 << max_buffer_po2` (po2 defaults to 16) but
/// the getter returns `input_buffer.size() - 4`. Asserting 65536 here would
/// be asserting the obvious wrong guess.
#[test]
fn default_buffer_max_sizes_reserve_the_length_prefix() {
    let p = PacketPeerStream::new();
    assert_eq!(p.input_buffer_max_size(), 65532);
    assert_eq!(p.output_buffer_max_size(), 65532);
}

/// 🔑 The setter is NOT store-what-you-were-given.
///
/// Godot rounds up to a power of two *including* the 4-byte prefix, so
/// `set(1000)` reads back as 1020. A test asserting 1000 would pass only
/// against an implementation that had dropped Godot's rounding.
#[test]
fn set_buffer_max_size_rounds_up_a_power_of_two_including_the_prefix() {
    let mut p = PacketPeerStream::new();
    p.set_output_buffer_max_size(1000);
    assert_eq!(
        p.output_buffer_max_size(),
        1020,
        "next_power_of_2(1004) - 4"
    );
    p.set_input_buffer_max_size(1000);
    assert_eq!(p.input_buffer_max_size(), 1020);

    // An exact power of two still gains the prefix, so it rounds to the NEXT
    // one: 1024 + 4 -> 2048 -> reports 2044.
    p.set_output_buffer_max_size(1024);
    assert_eq!(p.output_buffer_max_size(), 2044);

    // Negative is refused and leaves the buffer as it was (Godot's
    // ERR_FAIL_COND_MSG), rather than panicking on the `as usize` cast.
    p.set_output_buffer_max_size(-1);
    assert_eq!(p.output_buffer_max_size(), 2044);
}

/// 🔑 Oversize is `ERR_INVALID_PARAMETER` (31), **not** `ERR_OUT_OF_MEMORY`.
///
/// ⚠️ The bead's acceptance says OUT_OF_MEMORY. That code does appear in
/// `core/io/packet_peer.cpp`, but on `PacketPeer::put_var` exceeding
/// `encode_buffer_max_size` — a different method and a different buffer.
/// `PacketPeerStream::put_packet` guards with
/// `ERR_FAIL_COND_V(p_buffer_size + 4 > output_buffer.size(), ERR_INVALID_PARAMETER)`.
#[test]
fn oversize_packet_is_invalid_parameter_and_writes_nothing() {
    let mut p = PacketPeerStream::new();
    p.set_output_buffer_max_size(16); // -> buffer 32, so 28 bytes usable
    assert_eq!(p.output_buffer_max_size(), 28);

    assert_eq!(p.put_packet(&[0u8; 28]), 0, "28 + 4 == 32 fits exactly");
    assert_eq!(p.get_available_packet_count(), 1);

    // 🔑 29 + 4 > 32. The guard counts the PREFIX, so the largest accepted
    // packet is four bytes smaller than the buffer — an off-by-four here
    // would accept this one.
    assert_eq!(p.put_packet(&[0u8; 29]), 31, "ERR_INVALID_PARAMETER");
    assert_eq!(
        p.get_available_packet_count(),
        1,
        "a rejected packet must not be partially framed into the stream"
    );
}

/// `get_packet_error` reports `ERR_UNAVAILABLE` (2) with nothing queued, and
/// `OK` after a successful read.
#[test]
fn get_packet_error_tracks_the_last_read() {
    let mut p = PacketPeerStream::new();
    assert_eq!(p.get_packet(), None);
    assert_eq!(p.get_packet_error(), 2, "ERR_UNAVAILABLE when empty");

    p.put_packet(&[7, 7]);
    assert_eq!(p.get_packet(), Some(vec![7, 7]));
    assert_eq!(p.get_packet_error(), 0, "OK after a successful read");

    // ⚠️ A partial frame is ERR_UNAVAILABLE too, not a malformed-stream
    // error — the bytes stay buffered for the next feed.
    p.feed(&[0x04, 0x00, 0x00, 0x00, 0x01]);
    assert_eq!(p.get_packet(), None);
    assert_eq!(p.get_packet_error(), 2);
}
