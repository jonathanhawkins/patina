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

/// A packet peer that frames variable-length packets over a byte stream with
/// Godot's 4-byte little-endian length prefix (Godot `PacketPeerStream`).
#[derive(Debug, Clone, Default)]
pub struct PacketPeerStream {
    /// The underlying stream bytes (framed packets), written at the tail and
    /// consumed from `read_pos`.
    buffer: Vec<u8>,
    /// Read cursor into `buffer`.
    read_pos: usize,
}

impl PacketPeerStream {
    /// Creates an empty packet-stream peer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Frames `data` with a 4-byte little-endian length prefix and appends it to
    /// the stream (Godot `put_packet`).
    pub fn put_packet(&mut self, data: &[u8]) {
        let len = data.len() as u32;
        self.buffer.extend_from_slice(&len.to_le_bytes());
        self.buffer.extend_from_slice(data);
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
        let len = Self::peek_len(&self.buffer, self.read_pos)?;
        let start = self.read_pos + LEN_PREFIX;
        let end = start + len;
        if end > self.buffer.len() {
            return None;
        }
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
