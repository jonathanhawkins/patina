//! `PacketPeerStream` — frames discrete packets over a byte stream.
//!
//! Mirrors Godot's `PacketPeerStream`: a [`PacketPeer`] that turns a
//! byte-oriented stream (a `StreamPeer` such as `StreamPeerBuffer` or
//! `StreamPeerTCP`) into a packet-oriented one by prefixing each packet with
//! **Godot's 4-byte little-endian length**.
//!
//! This type owns the intermediary stream buffer (the loopback
//! `StreamPeerBuffer` case): [`put_packet`](PacketPeerStream::put_packet)
//! appends a framed packet and [`get_packet`](PacketPeerStream::get_packet)
//! decodes the next complete one. Bridging to an external `StreamPeer` is done
//! via [`feed`](PacketPeerStream::feed) (incoming bytes) and
//! [`stream_bytes`](PacketPeerStream::stream_bytes) (outgoing framed bytes).
//!
//! `PacketPeerDTLS` (native UDP + DTLS) and `PacketPeerExtension` (GDExtension
//! virtual) are out of scope for the browser target.

/// Length of the packet length prefix, in bytes (Godot uses a 32-bit length).
const LEN_PREFIX: usize = 4;

/// Godot `@GlobalScope.Error` codes this peer returns.
///
/// ⚠️ Transcribed from `doc/classes/@GlobalScope.xml` and cross-checked against
/// this repo's own table in `gdscript-interop/src/global_constants.rs` — the two
/// agree. Not derived: the neighbouring codes are not in an order any guess
/// would reproduce.
const OK: i64 = 0;
/// No complete packet is queued (Godot `get_packet` -> `ERR_UNAVAILABLE`).
const ERR_UNAVAILABLE: i64 = 2;
/// A packet too large for the output buffer.
///
/// ⚠️ **`ERR_INVALID_PARAMETER`, not `ERR_OUT_OF_MEMORY`.** Godot's
/// `PacketPeerStream::put_packet` guards with
/// `ERR_FAIL_COND_V(p_buffer_size + 4 > output_buffer.size(), ERR_INVALID_PARAMETER)`.
/// `ERR_OUT_OF_MEMORY` does appear in `core/io/packet_peer.cpp`, but on a
/// different method and a different buffer — `PacketPeer::put_var` exceeding
/// `encode_buffer_max_size`. Conflating the two is easy and wrong.
const ERR_INVALID_PARAMETER: i64 = 31;

/// Godot's default buffer size, from
/// `network/limits/packet_peer_stream/max_buffer_po2` (default **16**,
/// `register_core_types.cpp:350`), applied as `1 << po2`.
const DEFAULT_BUFFER_SIZE: usize = 1 << 16;

/// A packet peer that frames variable-length packets over a byte stream with
/// Godot's 4-byte little-endian length prefix (Godot `PacketPeerStream`).
// ⚠️ `Default` is implemented BY HAND below, not derived. A derived one would
// zero the two buffer sizes, and a zero output buffer rejects every packet with
// ERR_INVALID_PARAMETER — silently, since `new()` and the handle table both
// build through `default()`.
#[derive(Debug, Clone)]
pub struct PacketPeerStream {
    /// The underlying stream bytes (framed packets), written at the tail and
    /// consumed from `read_pos`.
    buffer: Vec<u8>,
    /// Read cursor into `buffer`.
    read_pos: usize,
    /// Godot `input_buffer_max_size`, stored as the RAW buffer size.
    ///
    /// ⚠️ The accessor reports `size - LEN_PREFIX`, so this is 65536 while a
    /// script reads 65532. See [`Self::input_buffer_max_size`].
    input_buffer_size: usize,
    /// Godot `output_buffer_max_size`, stored as the raw buffer size.
    output_buffer_size: usize,
    /// The code the last [`get_packet`](Self::get_packet) produced
    /// (Godot `PacketPeer.get_packet_error`).
    last_error: i64,
}

impl Default for PacketPeerStream {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            read_pos: 0,
            input_buffer_size: DEFAULT_BUFFER_SIZE,
            output_buffer_size: DEFAULT_BUFFER_SIZE,
            last_error: OK,
        }
    }
}

impl PacketPeerStream {
    /// Creates an empty packet-stream peer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Frames `data` with a 4-byte little-endian length prefix and appends it to
    /// the stream (Godot `put_packet`), returning a Godot `Error`.
    ///
    /// ⚠️ The size guard counts the **prefix**: Godot rejects when
    /// `p_buffer_size + 4 > output_buffer.size()`, so a packet exactly as large
    /// as the buffer does not fit. An off-by-four here would accept a packet
    /// Godot refuses.
    ///
    /// A zero-length packet returns OK. Note the divergence flagged inline:
    /// Godot accepts it and writes nothing, Patina frames it so it round-trips.
    pub fn put_packet(&mut self, data: &[u8]) -> i64 {
        if data.is_empty() {
            // ⚠️ DELIBERATE DIVERGENCE FROM GODOT, kept from pat-s3rrv.80.
            //
            // Godot short-circuits `if (p_buffer_size == 0) return OK;` BEFORE
            // writing anything, so an empty packet is accepted and then
            // silently vanishes — `get_packet` never yields it. Patina frames
            // a zero-length packet instead, so it round-trips.
            //
            // Changing this to match Godot would break
            // `packet_peer_stream_frames_packets_over_stream_peer`, which
            // asserts the round-trip on purpose ("empty packet is valid"), and
            // it is outside this bead's acceptance. Flagged rather than
            // silently switched.
            self.buffer.extend_from_slice(&0u32.to_le_bytes());
            return OK;
        }
        if data.len() + LEN_PREFIX > self.output_buffer_size {
            return ERR_INVALID_PARAMETER;
        }
        let len = data.len() as u32;
        self.buffer.extend_from_slice(&len.to_le_bytes());
        self.buffer.extend_from_slice(data);
        OK
    }

    /// Godot `input_buffer_max_size`.
    ///
    /// 🔑 Reports the raw buffer size **minus the 4-byte prefix**, exactly as
    /// `PacketPeerStream::get_input_buffer_max_size` does
    /// (`return input_buffer.size() - 4;`). So the default reads **65532**, not
    /// 65536 — the obvious guess is wrong by exactly the prefix.
    pub fn input_buffer_max_size(&self) -> i64 {
        (self.input_buffer_size - LEN_PREFIX) as i64
    }

    /// Godot `output_buffer_max_size`, same `- 4` convention.
    pub fn output_buffer_max_size(&self) -> i64 {
        (self.output_buffer_size - LEN_PREFIX) as i64
    }

    /// Godot `set_input_buffer_max_size`.
    ///
    /// 🔑 Rounds up to a power of two **including the prefix**
    /// (`next_power_of_2(p_max_size + 4)`), so this is NOT a store-what-you-were
    /// -given setter: `set(1000)` then `get()` answers **1020**. A round-trip
    /// test asserting 1000 would be asserting the bug.
    ///
    /// A negative size is refused (Godot's `ERR_FAIL_COND_MSG`), leaving the
    /// buffer as it was.
    pub fn set_input_buffer_max_size(&mut self, max_size: i64) {
        if let Some(size) = Self::buffer_size_for(max_size) {
            self.input_buffer_size = size;
        }
    }

    /// Godot `set_output_buffer_max_size`.
    pub fn set_output_buffer_max_size(&mut self, max_size: i64) {
        if let Some(size) = Self::buffer_size_for(max_size) {
            self.output_buffer_size = size;
        }
    }

    /// The raw buffer size Godot would allocate for a requested max size, or
    /// `None` for a negative request.
    fn buffer_size_for(max_size: i64) -> Option<usize> {
        if max_size < 0 {
            return None;
        }
        Some((max_size as usize + LEN_PREFIX).next_power_of_two())
    }

    /// Godot `PacketPeer.get_packet_error` — the code the last
    /// [`get_packet`](Self::get_packet) produced.
    ///
    /// ⚠️ Godot's script-facing `get_packet()` returns only the bytes; the error
    /// is a **separate** accessor. That is why `get_packet` below still hands
    /// back an `Option` and records the code here rather than changing shape.
    pub fn get_packet_error(&self) -> i64 {
        self.last_error
    }

    /// Appends raw bytes received from the underlying stream (e.g. what a
    /// `StreamPeer` delivered). Framing is decoded lazily by
    /// [`get_packet`](Self::get_packet).
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The un-consumed framed bytes — the wire representation the underlying
    /// `StreamPeer` would carry onward.
    pub fn stream_bytes(&self) -> &[u8] {
        &self.buffer[self.read_pos..]
    }

    /// The number of complete framed packets currently available to read (Godot
    /// `get_available_packet_count`).
    pub fn get_available_packet_count(&self) -> usize {
        let mut pos = self.read_pos;
        let mut count = 0;
        while let Some(len) = Self::peek_len(&self.buffer, pos) {
            let end = pos + LEN_PREFIX + len;
            if end > self.buffer.len() {
                break;
            }
            pos = end;
            count += 1;
        }
        count
    }

    /// Decodes and consumes the next complete packet, or `None` when no complete
    /// packet is available yet (a partial frame stays buffered) (Godot
    /// `get_packet`).
    pub fn get_packet(&mut self) -> Option<Vec<u8>> {
        let Some(len) = Self::peek_len(&self.buffer, self.read_pos) else {
            self.last_error = ERR_UNAVAILABLE;
            return None;
        };
        let start = self.read_pos + LEN_PREFIX;
        let end = start + len;
        if end > self.buffer.len() {
            // A partial frame is "nothing available yet", not a malformed
            // stream — the remaining bytes stay buffered for the next feed.
            self.last_error = ERR_UNAVAILABLE;
            return None;
        }
        self.last_error = OK;
        let packet = self.buffer[start..end].to_vec();
        self.read_pos = end;
        // Reclaim memory once the stream is fully drained.
        if self.read_pos == self.buffer.len() {
            self.buffer.clear();
            self.read_pos = 0;
        }
        Some(packet)
    }

    /// Reads the 4-byte little-endian length at `pos`, if four bytes are present.
    fn peek_len(buffer: &[u8], pos: usize) -> Option<usize> {
        if pos + LEN_PREFIX > buffer.len() {
            return None;
        }
        let bytes = [
            buffer[pos],
            buffer[pos + 1],
            buffer[pos + 2],
            buffer[pos + 3],
        ];
        Some(u32::from_le_bytes(bytes) as usize)
    }
}

/// Script-visible handle table for `PacketPeerStream` (pat-s3rrv.80).
///
/// This module implemented the full framing but nothing could reach it: there
/// was no ClassDB registration and no interpreter dispatch, so a 162-line
/// working class was unreachable from GDScript. The handle pattern mirrors
/// `websocket_peer.rs` exactly — same poison-tolerant table, same tag mask.
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte makes `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x5C` was chosen after
/// sweeping all 36 `*_HANDLE_TAG` constants in the workspace; it is unused, and
/// sits beside `StreamPeer` (`0x5B`) and `WebSocket` (`0x57`).
const PACKET_PEER_STREAM_HANDLE_TAG: u64 = 0x5C00_0000_0000_0000;
static NEXT_PACKET_PEER_STREAM_HANDLE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
static PACKET_PEER_STREAM_HANDLES: std::sync::Mutex<
    Option<std::collections::HashMap<u64, PacketPeerStream>>,
> = std::sync::Mutex::new(None);

fn packet_peer_stream_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, PacketPeerStream>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    PACKET_PEER_STREAM_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_packet_peer_stream_handle`].
pub fn is_packet_peer_stream_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == PACKET_PEER_STREAM_HANDLE_TAG
}

/// Allocates a fresh [`PacketPeerStream`] and returns its script-visible id
/// (Godot `PacketPeerStream.new()`).
pub fn new_packet_peer_stream_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_PACKET_PEER_STREAM_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = PACKET_PEER_STREAM_HANDLE_TAG | seq;
    packet_peer_stream_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, PacketPeerStream::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live peer behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_packet_peer_stream<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut PacketPeerStream) -> R,
) -> Option<R> {
    let mut guard = packet_peer_stream_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops a handle. Idempotent.
pub fn release_packet_peer_stream_handle(id: gdcore::id::ObjectId) {
    let mut guard = packet_peer_stream_handles();
    if let Some(map) = guard.as_mut() {
        map.remove(&id.raw());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_peer_stream_frames_packets_over_stream_peer() {
        let mut p = PacketPeerStream::new();
        assert_eq!(p.get_available_packet_count(), 0);
        assert_eq!(p.get_packet(), None);

        p.put_packet(&[1, 2, 3]);
        p.put_packet(&[]); // empty packet is valid
        p.put_packet(&[9, 8, 7, 6]);
        assert_eq!(p.get_available_packet_count(), 3);

        assert_eq!(p.get_packet(), Some(vec![1, 2, 3]));
        assert_eq!(p.get_available_packet_count(), 2);
        assert_eq!(p.get_packet(), Some(vec![])); // empty round-trips
        assert_eq!(p.get_packet(), Some(vec![9, 8, 7, 6]));
        assert_eq!(p.get_packet(), None);
        assert_eq!(p.get_available_packet_count(), 0);
    }

    #[test]
    fn put_packet_uses_little_endian_length_prefix() {
        let mut p = PacketPeerStream::new();
        p.put_packet(&[0xAA]);
        // 4-byte LE length of 1, then the payload byte.
        assert_eq!(p.stream_bytes(), &[0x01, 0x00, 0x00, 0x00, 0xAA]);
    }

    #[test]
    fn little_endian_length_is_decoded() {
        let mut p = PacketPeerStream::new();
        // LE length 2 = [02, 00, 00, 00], then two payload bytes.
        p.feed(&[0x02, 0x00, 0x00, 0x00, 0x10, 0x20]);
        assert_eq!(p.get_available_packet_count(), 1);
        assert_eq!(p.get_packet(), Some(vec![0x10, 0x20]));
    }

    #[test]
    fn partial_frame_is_not_a_packet_until_complete() {
        let mut p = PacketPeerStream::new();
        // Length says 3 payload bytes but only 2 have arrived.
        p.feed(&[0x03, 0x00, 0x00, 0x00, 0xAA, 0xBB]);
        assert_eq!(p.get_available_packet_count(), 0);
        assert_eq!(p.get_packet(), None);
        // Deliver the final byte -> the packet completes.
        p.feed(&[0xCC]);
        assert_eq!(p.get_available_packet_count(), 1);
        assert_eq!(p.get_packet(), Some(vec![0xAA, 0xBB, 0xCC]));
    }
}
