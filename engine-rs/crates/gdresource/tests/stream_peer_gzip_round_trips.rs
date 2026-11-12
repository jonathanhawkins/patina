//! pat-s3rrv.236 — `StreamPeerGZIP` round-trips gzip and zlib streams.
//!
//! Godot ground truth: `upstream/godot/core/io/stream_peer_gzip.cpp` and
//! `doc/classes/StreamPeerGZIP.xml`.
//!
//! ⚠️ An INTEGRATION target on purpose. `gdresource`'s lib suite is large and
//! shared; gating a bead on `--lib` means any one unrelated red test cancels the
//! run before yours executes — that is exactly how pat-s3rrv.235 lost a
//! verification. Everything under test here is public.
//!
//! ⚠️ Error codes are asserted as NUMBERS, not through the named constants.
//! Asserting `ERR_INVALID_PARAMETER == ERR_INVALID_PARAMETER` would hold even if
//! the constant carried the wrong value; the literals pin the wire contract.

use gdresource::crc32;
use gdresource::stream_peer_gzip::{
    gzip_strip_header, gzip_wrap, StreamPeerGzip, DEFAULT_BUFFER_SIZE, GZIP_HEADER,
};

/// Godot `@GlobalScope.Error` values, spelled out so the test pins them.
const OK: i64 = 0;
const ERR_UNAVAILABLE: i64 = 2;
const ERR_ALREADY_IN_USE: i64 = 22;
const ERR_INVALID_PARAMETER: i64 = 31;

fn payload() -> Vec<u8> {
    // 64 KiB, highly compressible — the size the acceptance names.
    b"patina-gzip-roundtrip-"
        .iter()
        .copied()
        .cycle()
        .take(64 * 1024)
        .collect()
}

#[test]
fn gzip_round_trips_and_carries_the_magic() {
    let data = payload();
    let mut c = StreamPeerGzip::new();
    assert_eq!(c.start_compression(false, DEFAULT_BUFFER_SIZE), OK);
    assert_eq!(c.put_data(&data), OK);
    assert_eq!(c.finish(), OK);
    let compressed = c.get_all();

    assert_eq!(
        &compressed[..2],
        &[0x1f, 0x8b],
        "gzip mode must emit the gzip magic"
    );
    assert!(
        compressed.len() < data.len() / 2,
        "64 KiB of repeating text must compress materially, got {}",
        compressed.len()
    );

    let mut d = StreamPeerGzip::new();
    assert_eq!(d.start_decompression(false, DEFAULT_BUFFER_SIZE), OK);
    assert_eq!(d.put_data(&compressed), OK);
    assert_eq!(d.get_all(), data, "byte-exact recovery");
}

/// 🔑 `use_deflate` is **zlib**, not raw deflate.
///
/// ⚠️ The bead calls this "header-less raw deflate". A zlib stream has no
/// *gzip* magic either, so an assertion that only checks for the absence of
/// `1f 8b` passes both ways — this pins the zlib header positively.
#[test]
fn use_deflate_selects_zlib_not_raw_deflate() {
    let data = payload();
    let mut c = StreamPeerGzip::new();
    assert_eq!(c.start_compression(true, DEFAULT_BUFFER_SIZE), OK);
    c.put_data(&data);
    c.finish();
    let compressed = c.get_all();

    assert_ne!(&compressed[..2], &[0x1f, 0x8b], "not a gzip container");
    // zlib: CMF low nibble 8 = deflate, and (CMF<<8 | FLG) % 31 == 0.
    assert_eq!(compressed[0] & 0x0f, 8, "CM=8 (deflate) in the zlib header");
    let check = ((compressed[0] as u16) << 8) | compressed[1] as u16;
    assert_eq!(check % 31, 0, "zlib header checksum must validate");

    let mut d = StreamPeerGzip::new();
    assert_eq!(d.start_decompression(true, DEFAULT_BUFFER_SIZE), OK);
    d.put_data(&compressed);
    assert_eq!(d.get_all(), data);
}

/// The two containers must not silently accept each other.
#[test]
fn a_container_mismatch_yields_nothing_rather_than_garbage() {
    let data = b"mismatch".to_vec();
    let mut c = StreamPeerGzip::new();
    c.start_compression(false, DEFAULT_BUFFER_SIZE); // gzip
    c.put_data(&data);
    c.finish();
    let gz = c.get_all();

    let mut d = StreamPeerGzip::new();
    d.start_decompression(true, DEFAULT_BUFFER_SIZE); // zlib reader
    d.put_data(&gz);
    assert_eq!(
        d.get_available_bytes(),
        0,
        "a zlib reader must not decode a gzip member"
    );
}

/// Godot's guard order and error codes.
#[test]
fn start_guards_match_godots_order_and_codes() {
    let mut p = StreamPeerGzip::new();
    assert_eq!(
        p.start_compression(false, 0),
        ERR_INVALID_PARAMETER,
        "buffer_size must be positive"
    );
    assert_eq!(p.start_compression(false, -1), ERR_INVALID_PARAMETER);

    assert_eq!(p.start_compression(false, DEFAULT_BUFFER_SIZE), OK);
    // ⚠️ Already-open is checked BEFORE the size, so a second start with a
    // bad size still reports ALREADY_IN_USE.
    assert_eq!(p.start_compression(false, 0), ERR_ALREADY_IN_USE);
    assert_eq!(
        p.start_decompression(false, DEFAULT_BUFFER_SIZE),
        ERR_ALREADY_IN_USE
    );

    p.clear();
    assert_eq!(
        p.start_decompression(false, DEFAULT_BUFFER_SIZE),
        OK,
        "clear() reopens the peer"
    );
}

/// `finish()` is a compressor-only operation.
#[test]
fn finish_is_unavailable_unless_compressing() {
    let mut p = StreamPeerGzip::new();
    assert_eq!(p.finish(), ERR_UNAVAILABLE, "closed peer");

    p.start_decompression(false, DEFAULT_BUFFER_SIZE);
    assert_eq!(
        p.finish(),
        ERR_UNAVAILABLE,
        "a decompressing peer must refuse finish(), not quietly succeed"
    );

    // And put_data before any start is a parameter error, not a panic.
    let mut q = StreamPeerGzip::new();
    assert_eq!(q.put_data(b"x"), ERR_INVALID_PARAMETER);
}

/// A short read reports `ERR_UNAVAILABLE` and still yields what it had.
#[test]
fn get_data_reports_a_short_read() {
    let mut c = StreamPeerGzip::new();
    c.start_compression(false, DEFAULT_BUFFER_SIZE);
    c.put_data(b"abc");
    c.finish();
    let n = c.get_available_bytes();

    let (err, some) = c.get_data(2);
    assert_eq!(err, OK);
    assert_eq!(some.len(), 2);

    let (err, rest) = c.get_data(n); // more than remains
    assert_eq!(err, ERR_UNAVAILABLE);
    assert_eq!(
        rest.len(),
        n - 2,
        "a short read still returns the remainder"
    );
    assert_eq!(c.get_available_bytes(), 0);
}

/// Our gzip writer must interoperate with the header parser byte for byte.
#[test]
fn gzip_wrap_and_strip_are_inverse() {
    let original = b"round trip me".to_vec();
    // Level 6 is what the module uses; any level round-trips, so this pins
    // the wrapper, not the compressor's tuning.
    let raw = miniz_oxide::deflate::compress_to_vec(&original, 6);
    let wrapped = gzip_wrap(&raw, &original);

    assert_eq!(&wrapped[..10], &GZIP_HEADER);
    let body = gzip_strip_header(&wrapped).expect("header parses");
    let end = body.len() - 8;
    assert_eq!(&body[..end], &raw[..], "the deflate body survives intact");

    // The trailer is CRC-32 then ISIZE, both little-endian.
    let crc = u32::from_le_bytes([body[end], body[end + 1], body[end + 2], body[end + 3]]);
    let isize_field =
        u32::from_le_bytes([body[end + 4], body[end + 5], body[end + 6], body[end + 7]]);
    assert_eq!(crc, crc32(&original));
    assert_eq!(isize_field, original.len() as u32);
}
