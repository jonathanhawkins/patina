//! `StreamPeerBuffer` — an in-memory, cursor-based typed byte buffer
//! (pat-u3l8c.82).
//!
//! A pure-`std` port of Godot's `StreamPeerBuffer`: a growable `Vec<u8>` with a
//! single read/write cursor and a big/little-endian toggle. `put_*` methods
//! serialize a value at the cursor (extending the buffer as needed) and advance
//! it; `get_*` methods deserialize at the cursor and advance it, returning `0`
//! / empty on underflow rather than panicking.
//!
//! # Scope / remaining work (pat-u3l8c.82, a net pack)
//!
//! This delivers only the `StreamPeerBuffer` primitive. The pack's other
//! pieces remain open: a shared `StreamPeer` trait unifying this with
//! `StreamPeerTCP`, `Variant`-valued `put_var`/`get_var`, the WebSocket server /
//! `WebSocketMultiplayerPeer`, HTTP gzip + `download_file`, and UDP
//! broadcast/multicast.

/// An in-memory byte buffer with typed accessors and a movable cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamPeerBuffer {
    data: Vec<u8>,
    pos: usize,
    big_endian: bool,
}

impl StreamPeerBuffer {
    /// Creates an empty little-endian buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects big-endian (`true`) or little-endian (`false`) encoding for
    /// subsequent multi-byte `put_*` / `get_*` calls.
    pub fn set_big_endian(&mut self, big_endian: bool) {
        self.big_endian = big_endian;
    }

    /// Whether multi-byte values are encoded big-endian.
    pub fn is_big_endian(&self) -> bool {
        self.big_endian
    }

    /// The current cursor position.
    pub fn get_position(&self) -> usize {
        self.pos
    }

    /// The number of bytes stored.
    pub fn get_size(&self) -> usize {
        self.data.len()
    }

    /// Moves the cursor to `pos` (clamped to the buffer size).
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.data.len());
    }

    /// Empties the buffer and resets the cursor.
    pub fn clear(&mut self) {
        self.data.clear();
        self.pos = 0;
    }

    /// Borrows the underlying bytes.
    pub fn data_array(&self) -> &[u8] {
        &self.data
    }

    // ---- writes -----------------------------------------------------------

    /// Appends/overwrites `bytes` at the cursor, growing the buffer if needed.
    pub fn put_data(&mut self, bytes: &[u8]) {
        let end = self.pos + bytes.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
    }

    /// Writes an unsigned byte.
    pub fn put_u8(&mut self, v: u8) {
        self.put_data(&[v]);
    }

    /// Writes a signed byte.
    pub fn put_8(&mut self, v: i8) {
        self.put_data(&[v as u8]);
    }

    /// Writes an unsigned 16-bit integer.
    pub fn put_u16(&mut self, v: u16) {
        let b = if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        self.put_data(&b);
    }

    /// Writes a signed 16-bit integer.
    pub fn put_16(&mut self, v: i16) {
        self.put_u16(v as u16);
    }

    /// Writes an unsigned 32-bit integer.
    pub fn put_u32(&mut self, v: u32) {
        let b = if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        self.put_data(&b);
    }

    /// Writes a signed 32-bit integer.
    pub fn put_32(&mut self, v: i32) {
        self.put_u32(v as u32);
    }

    /// Writes an unsigned 64-bit integer.
    pub fn put_u64(&mut self, v: u64) {
        let b = if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        self.put_data(&b);
    }

    /// Writes a signed 64-bit integer.
    pub fn put_64(&mut self, v: i64) {
        self.put_u64(v as u64);
    }

    /// Writes a 32-bit float.
    pub fn put_float(&mut self, v: f32) {
        self.put_u32(v.to_bits());
    }

    /// Writes a 64-bit float.
    pub fn put_double(&mut self, v: f64) {
        self.put_u64(v.to_bits());
    }

    /// Writes a UTF-8 string as a `u32` byte-length prefix followed by its bytes.
    pub fn put_utf8_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.put_u32(bytes.len() as u32);
        self.put_data(bytes);
    }

    // ---- reads ------------------------------------------------------------

    /// Reads up to `len` bytes from the cursor (fewer if the buffer is short).
    pub fn get_data(&mut self, len: usize) -> Vec<u8> {
        let end = (self.pos + len).min(self.data.len());
        let out = self.data[self.pos..end].to_vec();
        self.pos = end;
        out
    }

    /// Reads `N` bytes into a fixed array, zero-padding on underflow.
    fn read_array<const N: usize>(&mut self) -> [u8; N] {
        let mut arr = [0u8; N];
        let avail = self.data.len().saturating_sub(self.pos).min(N);
        arr[..avail].copy_from_slice(&self.data[self.pos..self.pos + avail]);
        self.pos += avail;
        arr
    }

    /// Reads an unsigned byte (`0` on underflow).
    pub fn get_u8(&mut self) -> u8 {
        self.read_array::<1>()[0]
    }

    /// Reads a signed byte.
    pub fn get_8(&mut self) -> i8 {
        self.get_u8() as i8
    }

    /// Reads an unsigned 16-bit integer.
    pub fn get_u16(&mut self) -> u16 {
        let a = self.read_array::<2>();
        if self.big_endian {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        }
    }

    /// Reads a signed 16-bit integer.
    pub fn get_16(&mut self) -> i16 {
        self.get_u16() as i16
    }

    /// Reads an unsigned 32-bit integer.
    pub fn get_u32(&mut self) -> u32 {
        let a = self.read_array::<4>();
        if self.big_endian {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        }
    }

    /// Reads a signed 32-bit integer.
    pub fn get_32(&mut self) -> i32 {
        self.get_u32() as i32
    }

    /// Reads an unsigned 64-bit integer.
    pub fn get_u64(&mut self) -> u64 {
        let a = self.read_array::<8>();
        if self.big_endian {
            u64::from_be_bytes(a)
        } else {
            u64::from_le_bytes(a)
        }
    }

    /// Reads a signed 64-bit integer.
    pub fn get_64(&mut self) -> i64 {
        self.get_u64() as i64
    }

    /// Reads a 32-bit float.
    pub fn get_float(&mut self) -> f32 {
        f32::from_bits(self.get_u32())
    }

    /// Reads a 64-bit float.
    pub fn get_double(&mut self) -> f64 {
        f64::from_bits(self.get_u64())
    }

    /// Reads a `u32`-length-prefixed UTF-8 string (lossy on invalid bytes).
    pub fn get_utf8_string(&mut self) -> String {
        let len = self.get_u32() as usize;
        let bytes = self.get_data(len);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_roundtrip_little_endian() {
        let mut b = StreamPeerBuffer::new();
        b.put_u8(0xAB);
        b.put_8(-5);
        b.put_u16(0x1234);
        b.put_16(-1000);
        b.put_u32(0xDEAD_BEEF);
        b.put_32(-123_456);
        b.put_u64(0x0102_0304_0506_0708);
        b.put_64(-9_000_000_000);
        b.seek(0);
        assert_eq!(b.get_u8(), 0xAB);
        assert_eq!(b.get_8(), -5);
        assert_eq!(b.get_u16(), 0x1234);
        assert_eq!(b.get_16(), -1000);
        assert_eq!(b.get_u32(), 0xDEAD_BEEF);
        assert_eq!(b.get_32(), -123_456);
        assert_eq!(b.get_u64(), 0x0102_0304_0506_0708);
        assert_eq!(b.get_64(), -9_000_000_000);
    }

    #[test]
    fn float_roundtrip() {
        let mut b = StreamPeerBuffer::new();
        b.put_float(3.14159);
        b.put_double(2.718281828459045);
        b.seek(0);
        assert!((b.get_float() - 3.14159).abs() < 1e-6);
        assert!((b.get_double() - 2.718281828459045).abs() < 1e-12);
    }

    #[test]
    fn string_and_data_roundtrip() {
        let mut b = StreamPeerBuffer::new();
        b.put_utf8_string("héllo, 世界");
        b.put_data(&[1, 2, 3, 4, 5]);
        b.seek(0);
        assert_eq!(b.get_utf8_string(), "héllo, 世界");
        assert_eq!(b.get_data(5), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn endianness_changes_byte_order_but_not_value() {
        let mut le = StreamPeerBuffer::new();
        le.put_u16(0x0102);
        assert_eq!(le.data_array(), &[0x02, 0x01], "little-endian byte order");

        let mut be = StreamPeerBuffer::new();
        be.set_big_endian(true);
        be.put_u16(0x0102);
        assert_eq!(be.data_array(), &[0x01, 0x02], "big-endian byte order");

        be.seek(0);
        assert_eq!(be.get_u16(), 0x0102, "value roundtrips regardless of order");
    }

    #[test]
    fn overwrite_at_seeked_position() {
        let mut b = StreamPeerBuffer::new();
        b.put_u32(0);
        b.seek(0);
        b.put_u16(0xBEEF); // overwrite the low half
        assert_eq!(b.get_size(), 4, "overwrite must not grow the buffer");
        b.seek(0);
        assert_eq!(b.get_u16(), 0xBEEF);
    }

    #[test]
    fn underflow_reads_return_zero_without_panicking() {
        let mut b = StreamPeerBuffer::new();
        b.put_u8(7);
        b.seek(0);
        assert_eq!(b.get_u8(), 7);
        // Past the end: zero-filled, no panic.
        assert_eq!(b.get_u32(), 0);
        assert_eq!(b.get_data(10), Vec::<u8>::new());
        assert_eq!(b.get_position(), b.get_size());
    }

    #[test]
    fn position_and_size_track_writes() {
        let mut b = StreamPeerBuffer::new();
        assert_eq!(b.get_size(), 0);
        assert_eq!(b.get_position(), 0);
        b.put_u32(1);
        assert_eq!(b.get_position(), 4);
        assert_eq!(b.get_size(), 4);
        b.clear();
        assert_eq!(b.get_size(), 0);
        assert_eq!(b.get_position(), 0);
    }

    #[test]
    fn mixed_sequence_roundtrip_big_endian() {
        let mut b = StreamPeerBuffer::new();
        b.set_big_endian(true);
        b.put_8(-1);
        b.put_float(-0.5);
        b.put_utf8_string("net");
        b.put_u64(42);
        b.seek(0);
        assert_eq!(b.get_8(), -1);
        assert!((b.get_float() + 0.5).abs() < 1e-6);
        assert_eq!(b.get_utf8_string(), "net");
        assert_eq!(b.get_u64(), 42);
    }
}
