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

    // ---- Variant codec (pat-0q2kx.183) ------------------------------------

    /// Writes `value` in Godot's binary Variant encoding at the cursor
    /// (`StreamPeer.put_var`).
    ///
    /// Godot frames the value with a leading 32-bit byte length so `get_var`
    /// knows how much to consume; this mirrors that, using the *stream's* own
    /// endianness for the length so a big-endian peer stays self-consistent.
    /// Returns `false` when the Variant cannot be encoded, leaving the cursor
    /// untouched rather than writing a truncated record.
    pub fn put_var(&mut self, value: &gdvariant::Variant) -> bool {
        match gdvariant::binary::encode_variant(value) {
            Ok(bytes) => {
                self.put_u32(bytes.len() as u32);
                self.put_data(&bytes);
                true
            }
            Err(_) => false,
        }
    }

    /// Reads a Variant written by [`put_var`](Self::put_var)
    /// (`StreamPeer.get_var`).
    ///
    /// Returns `None` — with the cursor restored — when the length prefix is
    /// absent, the payload is short, or the bytes do not decode. A partial read
    /// must not silently advance the cursor into the middle of a record, or
    /// every subsequent `get_*` on the stream reads garbage.
    pub fn get_var(&mut self) -> Option<gdvariant::Variant> {
        let start = self.pos;
        if self.data.len().saturating_sub(self.pos) < 4 {
            return None;
        }
        let len = self.get_u32() as usize;
        if self.data.len().saturating_sub(self.pos) < len {
            self.pos = start;
            return None;
        }
        let bytes = self.get_data(len);
        match gdvariant::binary::decode_variant(&bytes) {
            Ok((v, _)) => Some(v),
            Err(_) => {
                self.pos = start;
                None
            }
        }
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

    // ---- remaining StreamPeer surface (pat-s3rrv.178) ----------------------

    /// Bytes between the cursor and the end of the buffer
    /// (`StreamPeer.get_available_bytes`).
    pub fn get_available_bytes(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Alias for [`is_big_endian`](Self::is_big_endian) under Godot's spelling.
    ///
    /// `extension_api.json` names the getter `is_big_endian_enabled`, so a
    /// script calling it hit an unknown method. The short spelling is kept
    /// because Rust callers already use it.
    pub fn is_big_endian_enabled(&self) -> bool {
        self.big_endian
    }

    /// Replaces the whole buffer and rewinds the cursor
    /// (`StreamPeerBuffer.set_data_array`).
    pub fn set_data_array(&mut self, bytes: &[u8]) {
        self.data = bytes.to_vec();
        self.pos = 0;
    }

    /// Grows or truncates the buffer to `size` bytes
    /// (`StreamPeerBuffer.resize`), zero-filling any growth.
    ///
    /// The cursor is clamped, because leaving it past the end after a
    /// truncation would make the next `put_*` silently re-grow the buffer.
    pub fn resize(&mut self, size: usize) {
        self.data.resize(size, 0);
        self.pos = self.pos.min(self.data.len());
    }

    /// A copy of this buffer, cursor and endianness included
    /// (`StreamPeerBuffer.duplicate`).
    pub fn duplicate(&self) -> Self {
        self.clone()
    }

    /// Writes a half-precision (16-bit) float (`StreamPeer.put_half`).
    pub fn put_half(&mut self, v: f32) {
        self.put_u16(f32_to_f16_bits(v));
    }

    /// Reads a half-precision (16-bit) float (`StreamPeer.get_half`).
    pub fn get_half(&mut self) -> f32 {
        f16_bits_to_f32(self.get_u16())
    }

    /// Writes an ASCII string prefixed with its `u32` byte length
    /// (`StreamPeer.put_string`).
    ///
    /// Godot's `String::ascii()` **truncates** each code point to its low byte
    /// rather than substituting a placeholder, so `é` (U+00E9) is written as
    /// `0xE9`. Mirrored here so a stream written by one engine reads the same
    /// in the other.
    pub fn put_string(&mut self, s: &str) {
        let bytes: Vec<u8> = s.chars().map(|c| (c as u32 & 0xff) as u8).collect();
        self.put_u32(bytes.len() as u32);
        self.put_data(&bytes);
    }

    /// Reads an ASCII string (`StreamPeer.get_string`).
    ///
    /// `bytes = None` mirrors Godot's default of `-1`: read the `u32` length
    /// prefix first. A concrete count reads exactly that many bytes with **no**
    /// prefix, which is how a stream written with bare `put_data` is decoded.
    pub fn get_string(&mut self, bytes: Option<usize>) -> String {
        let len = match bytes {
            Some(n) => n,
            None => self.get_u32() as usize,
        };
        self.get_data(len).into_iter().map(|b| b as char).collect()
    }

    /// Reads a UTF-8 string, with Godot's optional explicit byte count
    /// (`StreamPeer.get_utf8_string`). See [`get_string`](Self::get_string).
    pub fn get_utf8_string_sized(&mut self, bytes: Option<usize>) -> String {
        let len = match bytes {
            Some(n) => n,
            None => self.get_u32() as usize,
        };
        let raw = self.get_data(len);
        String::from_utf8_lossy(&raw).into_owned()
    }

    /// Writes what it can and reports how much (`StreamPeer.put_partial_data`).
    ///
    /// Returns `(error, bytes_written)`. An in-memory buffer never refuses a
    /// write, so this always reports OK and the full length — the method exists
    /// so script code written against a socket peer runs unchanged here.
    pub fn put_partial_data(&mut self, bytes: &[u8]) -> (i64, usize) {
        self.put_data(bytes);
        (0, bytes.len())
    }

    /// Reads up to `len` bytes and reports how many (`get_partial_data`).
    ///
    /// Returns `(error, bytes)`. Short reads are OK with fewer bytes, matching
    /// Godot: the error slot signals a broken stream, not an empty one.
    pub fn get_partial_data(&mut self, len: usize) -> (i64, Vec<u8>) {
        (0, self.get_data(len))
    }
}

// ---- half-precision codec (pat-s3rrv.178) ---------------------------------
//
// Stable Rust has no `f16`, so the IEEE 754 binary16 conversion is done by
// hand. Both directions round-half-to-even and handle subnormals, because a
// truncating shift would make `put_half`/`get_half` lose a bit on values as
// ordinary as 0.1 and drop every value below 2^-14 to zero.

/// Encodes an `f32` as IEEE 754 binary16 bits.
fn f32_to_f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp32 = ((bits >> 23) & 0xff) as i32;
    let man32 = bits & 0x007f_ffff;

    // NaN keeps a non-zero mantissa so it does not decode as infinity.
    if exp32 == 0xff {
        return sign | 0x7c00 | if man32 != 0 { 0x0200 } else { 0 };
    }

    let exp = exp32 - 127 + 15;
    if exp >= 0x1f {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        // Below the smallest subnormal there is nothing left to round to.
        if exp < -10 {
            return sign;
        }
        // Restore the implicit leading 1 and shift into subnormal position.
        let man = man32 | 0x0080_0000;
        let shift = (14 - exp) as u32;
        return sign | round_shift(man, shift) as u16;
    }
    let out = ((exp as u32) << 10) + round_shift(man32, 13);
    // Rounding can carry all the way into the exponent and overflow to Inf.
    if out >= 0x7c00 {
        return sign | 0x7c00;
    }
    sign | out as u16
}

/// `value >> shift`, rounded half-to-even.
fn round_shift(value: u32, shift: u32) -> u32 {
    let quotient = value >> shift;
    let remainder = value & ((1 << shift) - 1);
    let half = 1 << (shift - 1);
    if remainder > half || (remainder == half && quotient & 1 == 1) {
        quotient + 1
    } else {
        quotient
    }
}

/// Decodes IEEE 754 binary16 bits to an `f32`.
fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let man = (bits & 0x03ff) as u32;

    if exp == 0 {
        if man == 0 {
            return f32::from_bits(sign);
        }
        // Subnormal: normalize until the implicit bit is in place. After `s`
        // shifts the value is `1.f * 2^(-14 - s)`, so the biased f32 exponent
        // is `127 - 14 - s`.
        let mut m = man;
        let mut s = 0u32;
        while m & 0x0400 == 0 {
            m <<= 1;
            s += 1;
        }
        return f32::from_bits(sign | ((113 - s) << 23) | ((m & 0x03ff) << 13));
    }
    if exp == 0x1f {
        return f32::from_bits(sign | 0x7f80_0000 | (man << 13));
    }
    f32::from_bits(sign | ((exp + 127 - 15) << 23) | (man << 13))
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

    // ---- pat-s3rrv.178 -----------------------------------------------------

    #[test]
    fn half_roundtrip_matches_ieee_binary16() {
        // Expected bit patterns were taken from the IEEE reference encoding,
        // not from this implementation, so the test is an oracle rather than a
        // restatement of the code.
        for (value, bits, decoded) in [
            (1.0f32, 0x3C00u16, 1.0f32),
            (-2.0, 0xC000, -2.0),
            (0.1, 0x2E66, 0.0999755859375),
            (65504.0, 0x7BFF, 65504.0),
            (0.0, 0x0000, 0.0),
        ] {
            let mut b = StreamPeerBuffer::new();
            b.put_half(value);
            assert_eq!(
                b.data_array(),
                &bits.to_le_bytes()[..],
                "{value} should encode to {bits:#06x}"
            );
            b.seek(0);
            assert_eq!(b.get_half(), decoded, "{value} should decode to {decoded}");
        }
    }

    #[test]
    fn half_rounds_half_to_even_not_toward_zero() {
        // These two are the whole reason the codec does not just shift-and-mask.
        // 2^-25 is exactly half an ulp and must round DOWN to even (zero);
        // 3 * 2^-25 is one and a half ulps and must round UP to 2 ulps. A
        // truncating implementation returns zero for both.
        let mut b = StreamPeerBuffer::new();
        b.put_half(2.0f32.powi(-25));
        b.put_half(3.0 * 2.0f32.powi(-25));
        b.seek(0);
        assert_eq!(b.get_half(), 0.0, "an exact tie rounds to even");
        assert_eq!(
            b.get_half(),
            1.1920928955078125e-07,
            "one and a half ulps rounds up"
        );
    }

    #[test]
    fn half_saturates_to_infinity_rather_than_wrapping() {
        let mut b = StreamPeerBuffer::new();
        b.put_half(65520.0); // just past the largest finite half
        b.put_half(-1.0e30);
        b.seek(0);

        let positive = b.get_half();
        assert!(
            positive.is_infinite() && positive.is_sign_positive(),
            "overflow saturates to +inf, got {positive}"
        );

        let negative = b.get_half();
        assert!(
            negative.is_infinite() && negative.is_sign_negative(),
            "overflow keeps the sign, got {negative}"
        );
    }

    #[test]
    fn subnormal_halves_survive_the_roundtrip() {
        // 2^-24 is the smallest positive half; it is subnormal, so an
        // implementation that ignores the subnormal range flushes it to zero.
        let mut b = StreamPeerBuffer::new();
        b.put_half(2.0f32.powi(-24));
        b.seek(0);
        assert_eq!(b.get_half(), 5.960464477539063e-08);
    }

    #[test]
    fn half_respects_endianness() {
        let mut be = StreamPeerBuffer::new();
        be.set_big_endian(true);
        be.put_half(1.0);
        assert_eq!(be.data_array(), &[0x3C, 0x00], "big-endian half byte order");
        be.seek(0);
        assert_eq!(be.get_half(), 1.0);
    }

    #[test]
    fn put_string_truncates_to_the_low_byte_like_godot() {
        // Godot's String::ascii() truncates rather than substituting, so `é`
        // (U+00E9) goes out as a single 0xE9 byte — one byte, not the two that
        // UTF-8 would use.
        let mut ascii = StreamPeerBuffer::new();
        ascii.put_string("é");
        assert_eq!(ascii.data_array(), &[1, 0, 0, 0, 0xE9]);

        let mut utf8 = StreamPeerBuffer::new();
        utf8.put_utf8_string("é");
        assert_eq!(utf8.data_array(), &[2, 0, 0, 0, 0xC3, 0xA9]);

        ascii.seek(0);
        assert_eq!(ascii.get_string(None), "é");
        utf8.seek(0);
        assert_eq!(utf8.get_utf8_string(), "é");
    }

    #[test]
    fn an_explicit_byte_count_skips_the_length_prefix() {
        let mut b = StreamPeerBuffer::new();
        b.put_data(b"raw"); // written without a prefix
        b.seek(0);
        assert_eq!(b.get_string(Some(3)), "raw");

        b.seek(0);
        assert_eq!(b.get_utf8_string_sized(Some(3)), "raw");

        // Positive control: reading the same bytes as a prefixed string
        // consumes them as a length and returns something else entirely, so
        // the assertions above are not passing by accident.
        b.seek(0);
        assert_ne!(b.get_string(None), "raw");
    }

    #[test]
    fn partial_reads_report_what_was_actually_available() {
        let mut b = StreamPeerBuffer::new();
        b.put_data(&[1, 2, 3]);
        b.seek(0);

        let (err, got) = b.get_partial_data(10);
        assert_eq!(err, 0, "a short read is not an error");
        assert_eq!(got, vec![1, 2, 3], "only the available bytes come back");
        assert_eq!(b.get_available_bytes(), 0);

        let (err, written) = b.put_partial_data(&[9, 9]);
        assert_eq!((err, written), (0, 2), "an in-memory buffer takes it all");
    }

    #[test]
    fn resize_clamps_the_cursor_into_the_buffer() {
        let mut b = StreamPeerBuffer::new();
        b.put_data(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(b.get_position(), 8);

        b.resize(3);
        assert_eq!(b.get_size(), 3);
        assert_eq!(
            b.get_position(),
            3,
            "a cursor left past the end would make the next write re-grow the buffer"
        );

        b.resize(6);
        assert_eq!(b.data_array(), &[1, 2, 3, 0, 0, 0], "growth zero-fills");
    }

    #[test]
    fn duplicate_copies_state_and_is_independent() {
        let mut b = StreamPeerBuffer::new();
        b.set_big_endian(true);
        b.put_u16(0x0102);
        b.seek(1);

        let mut copy = b.duplicate();
        assert_eq!(copy.get_position(), 1, "cursor is copied");
        assert!(copy.is_big_endian_enabled(), "endianness is copied");
        assert_eq!(copy.data_array(), b.data_array());

        copy.seek(0);
        copy.put_u16(0xFFFF);
        assert_eq!(b.data_array(), &[0x01, 0x02], "the original is untouched");
    }

    #[test]
    fn set_data_array_replaces_and_rewinds() {
        let mut b = StreamPeerBuffer::new();
        b.put_data(&[1, 2, 3, 4]);
        b.set_data_array(&[7, 8]);
        assert_eq!(b.data_array(), &[7, 8]);
        assert_eq!(b.get_position(), 0, "the cursor rewinds with the contents");
        assert_eq!(b.get_available_bytes(), 2);
    }

    #[test]
    fn available_bytes_counts_from_the_cursor() {
        let mut b = StreamPeerBuffer::new();
        b.put_data(&[1, 2, 3, 4]);
        assert_eq!(b.get_available_bytes(), 0, "cursor sits at the end");
        b.seek(1);
        assert_eq!(b.get_available_bytes(), 3);
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
