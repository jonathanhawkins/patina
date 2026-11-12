//! `StreamPeerGZIP` — a `StreamPeer` that gzip/zlib-transforms the bytes passing
//! through it (pat-s3rrv.236).
//!
//! Godot ground truth: `upstream/godot/core/io/stream_peer_gzip.cpp` and
//! `doc/classes/StreamPeerGZIP.xml`. `start_compression(use_deflate,
//! buffer_size)` / `start_decompression(..)` open a codec, the inherited
//! `StreamPeer` put/get API pushes bytes in and pulls transformed bytes out, and
//! `finish()` flushes the trailing block.
//!
//! # ⚠️ `use_deflate` selects **zlib**, not raw deflate
//!
//! The bead says `use_deflate` produces "a header-less raw deflate stream".
//! It does not. Godot picks the window bits:
//!
//! ```cpp
//! int window_bits = p_is_deflate ? 15 : (15 + 16);
//! ```
//!
//! In zlib, `15` is the **zlib** container (a two-byte header plus an Adler-32
//! trailer) and `15 + 16` is **gzip**. Raw, headerless deflate would be `-15`,
//! which Godot never passes. So `use_deflate = true` emits a zlib stream, and a
//! peer that emitted raw deflate instead would fail to interoperate with Godot
//! while still satisfying a test that only checks "no gzip magic".
//!
//! # ⚠️ Why this lives in `gdresource`
//!
//! The bead's prose says to implement it "next to the existing StreamPeer
//! family" in `gdplatform`. That crate depends on **neither** `miniz_oxide` nor
//! anything carrying a CRC-32, so it would need two new dependencies.
//! `gdresource` already has both — `miniz_oxide` for `zip.rs` and
//! `portable_compressed_texture.rs`, and [`crate::crc32`] — so the peer costs no
//! new edges here. The bead's own `Files:` list names `gdresource`.
//!
//! # ⚠️ Buffered, not incremental
//!
//! Godot streams through zlib and makes output available as input arrives. This
//! implementation buffers: compression transforms on [`finish`](StreamPeerGzip::finish),
//! and decompression decodes once the whole member has been written. That is
//! sufficient for the documented usage (write everything, then read), and it is
//! honest about what it is rather than pretending to stream. A caller that
//! interleaves small writes and reads will see output later than Godot would.

use crate::crc32;

/// Godot `Error` codes, transcribed from `doc/classes/@GlobalScope.xml` and
/// cross-checked against `gdscript-interop/src/global_constants.rs`.
pub const OK: i64 = 0;
/// `_start` when a codec is already open (`ERR_FAIL_COND_V(ctx != nullptr, ..)`).
pub const ERR_ALREADY_IN_USE: i64 = 22;
/// A non-positive `buffer_size`, and a `put_data` with no codec open.
pub const ERR_INVALID_PARAMETER: i64 = 31;
/// `finish()` on a peer that is not compressing.
pub const ERR_UNAVAILABLE: i64 = 2;

/// The gzip member header Godot's GZIP mode writes: magic, CM=8 (deflate),
/// FLG=0, MTIME=0, XFL=0, OS=255 (unknown).
///
/// ⚠️ Byte-identical to `GZIP_HEADER` in `gdscript-interop`'s
/// `PackedByteArray.compress(MODE_GZIP)` (pat-s3rrv.224) **on purpose**, so a
/// buffer compressed by one can be read by the other. That copy predates this
/// one and should be switched to these helpers — `gdscript-interop` already
/// depends on `gdresource` and already calls `gdresource::crc32` from the same
/// function, so the consolidation is a call-site change, not a move.
pub const GZIP_HEADER: [u8; 10] = [0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0xff];

/// The deflate level `zip.rs` and the `PackedByteArray` codec both use.
const DEFLATE_LEVEL: u8 = 6;

/// Wraps a raw deflate stream in a gzip member: header, body, then the
/// CRC-32 and ISIZE trailer, both little-endian.
///
/// ⚠️ ISIZE is the original length **modulo 2^32**, which is what the `as u32`
/// truncation expresses — gzip has no way to describe a longer member.
pub fn gzip_wrap(deflated: &[u8], original: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(GZIP_HEADER.len() + deflated.len() + 8);
    out.extend_from_slice(&GZIP_HEADER);
    out.extend_from_slice(deflated);
    out.extend_from_slice(&crc32(original).to_le_bytes());
    out.extend_from_slice(&(original.len() as u32).to_le_bytes());
    out
}

/// Skips a gzip member header, honouring the optional FLG fields.
///
/// Returns `None` for anything that is not deflate-compressed gzip. Our own
/// writer emits FLG=0, but a buffer produced elsewhere may carry FNAME or
/// FEXTRA, and blindly skipping ten bytes would feed the filename into the
/// inflater.
pub fn gzip_strip_header(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 0x08 {
        return None;
    }
    let flg = data[3];
    let mut i = 10usize;
    if flg & 0x04 != 0 {
        // FEXTRA: a two-byte length then that many bytes.
        let xlen = u16::from_le_bytes([*data.get(i)?, *data.get(i + 1)?]) as usize;
        i = i.checked_add(2)?.checked_add(xlen)?;
    }
    for bit in [0x08u8, 0x10] {
        if flg & bit != 0 {
            // FNAME / FCOMMENT: NUL-terminated.
            while *data.get(i)? != 0 {
                i += 1;
            }
            i += 1;
        }
    }
    if flg & 0x02 != 0 {
        i = i.checked_add(2)?; // FHCRC
    }
    data.get(i..)
}

/// Which transform an open peer is performing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// No codec open — `start_compression`/`start_decompression` not yet called.
    Idle,
    Compressing,
    Decompressing,
}

/// Godot `StreamPeerGZIP`.
#[derive(Debug, Clone)]
pub struct StreamPeerGzip {
    mode: Mode,
    /// `use_deflate`: true selects the **zlib** container, false gzip.
    use_deflate: bool,
    /// Godot's `buffer_size`; retained so `get_available_bytes` and the ring
    /// sizing match, and because it is observable via a failed `_start`.
    buffer_size: i64,
    /// Bytes written with `put_data` and not yet transformed.
    input: Vec<u8>,
    /// Transformed bytes waiting to be read.
    output: Vec<u8>,
    /// Read cursor into `output`.
    out_pos: usize,
    /// Set once a decompression peer has decoded its member, so a growing input
    /// is not decoded repeatedly.
    decoded: bool,
}

impl Default for StreamPeerGzip {
    fn default() -> Self {
        Self {
            mode: Mode::Idle,
            use_deflate: false,
            buffer_size: DEFAULT_BUFFER_SIZE,
            input: Vec::new(),
            output: Vec::new(),
            out_pos: 0,
            decoded: false,
        }
    }
}

/// `StreamPeerGZIP.xml`'s documented default for both start methods.
pub const DEFAULT_BUFFER_SIZE: i64 = 65535;

impl StreamPeerGzip {
    /// A closed peer. `start_compression`/`start_decompression` opens it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Godot `start_compression(use_deflate, buffer_size)`.
    pub fn start_compression(&mut self, use_deflate: bool, buffer_size: i64) -> i64 {
        self.start(Mode::Compressing, use_deflate, buffer_size)
    }

    /// Godot `start_decompression(use_deflate, buffer_size)`.
    pub fn start_decompression(&mut self, use_deflate: bool, buffer_size: i64) -> i64 {
        self.start(Mode::Decompressing, use_deflate, buffer_size)
    }

    /// Godot `_start`, whose two guards run in this order: already-open first,
    /// then the buffer size.
    ///
    /// ⚠️ The order is observable. Calling `start_compression` twice with a bad
    /// buffer size reports `ERR_ALREADY_IN_USE`, not `ERR_INVALID_PARAMETER`.
    fn start(&mut self, mode: Mode, use_deflate: bool, buffer_size: i64) -> i64 {
        if self.mode != Mode::Idle {
            return ERR_ALREADY_IN_USE;
        }
        if buffer_size <= 0 {
            return ERR_INVALID_PARAMETER;
        }
        self.clear();
        self.mode = mode;
        self.use_deflate = use_deflate;
        self.buffer_size = buffer_size;
        OK
    }

    /// The `buffer_size` the peer was opened with.
    ///
    /// Godot sizes its ring buffer from this; here it is retained so the value
    /// is observable and so a future incremental implementation has it without
    /// changing the public surface.
    pub fn buffer_size(&self) -> i64 {
        self.buffer_size
    }

    /// Godot `clear()` — closes the codec and drops both buffers.
    pub fn clear(&mut self) {
        self.mode = Mode::Idle;
        self.input.clear();
        self.output.clear();
        self.out_pos = 0;
        self.decoded = false;
    }

    /// `StreamPeer.put_data` — feeds bytes into the transform.
    ///
    /// A decompression peer decodes as soon as the member is complete, so its
    /// output becomes readable without `finish()` — which Godot refuses for a
    /// decompressing peer.
    pub fn put_data(&mut self, data: &[u8]) -> i64 {
        match self.mode {
            Mode::Idle => ERR_INVALID_PARAMETER,
            Mode::Compressing => {
                self.input.extend_from_slice(data);
                OK
            }
            Mode::Decompressing => {
                self.input.extend_from_slice(data);
                self.try_decode();
                OK
            }
        }
    }

    /// Godot `finish()` — flushes the compressor's trailing block.
    ///
    /// ⚠️ `ERR_UNAVAILABLE` for a peer that is closed **or decompressing**, per
    /// `ERR_FAIL_COND_V(!ctx || !compressing, ERR_UNAVAILABLE)`. Returning OK for
    /// a decompressing peer would look harmless and hide a caller's mistake.
    pub fn finish(&mut self) -> i64 {
        if self.mode != Mode::Compressing {
            return ERR_UNAVAILABLE;
        }
        let deflated = if self.use_deflate {
            miniz_oxide::deflate::compress_to_vec_zlib(&self.input, DEFLATE_LEVEL)
        } else {
            let raw = miniz_oxide::deflate::compress_to_vec(&self.input, DEFLATE_LEVEL);
            gzip_wrap(&raw, &self.input)
        };
        self.output.extend_from_slice(&deflated);
        self.input.clear();
        OK
    }

    /// Decodes the accumulated member if it is complete, once.
    fn try_decode(&mut self) {
        if self.decoded {
            return;
        }
        let decoded = if self.use_deflate {
            miniz_oxide::inflate::decompress_to_vec_zlib(&self.input).ok()
        } else {
            gzip_strip_header(&self.input).and_then(|body| {
                // The 8-byte CRC+ISIZE trailer is not part of the deflate stream.
                let end = body.len().saturating_sub(8);
                miniz_oxide::inflate::decompress_to_vec(&body[..end]).ok()
            })
        };
        if let Some(bytes) = decoded {
            self.output.extend_from_slice(&bytes);
            self.decoded = true;
        }
    }

    /// `StreamPeer.get_available_bytes` — transformed bytes ready to read.
    pub fn get_available_bytes(&self) -> usize {
        self.output.len() - self.out_pos
    }

    /// `StreamPeer.get_data(bytes)` — reads exactly `count` transformed bytes.
    ///
    /// Returns `ERR_UNAVAILABLE` with what is there when fewer are available,
    /// rather than blocking, since nothing here can produce more later.
    pub fn get_data(&mut self, count: usize) -> (i64, Vec<u8>) {
        let available = self.get_available_bytes();
        let take = count.min(available);
        let bytes = self.output[self.out_pos..self.out_pos + take].to_vec();
        self.out_pos += take;
        let err = if take < count { ERR_UNAVAILABLE } else { OK };
        (err, bytes)
    }

    /// Reads everything currently available.
    pub fn get_all(&mut self) -> Vec<u8> {
        let n = self.get_available_bytes();
        self.get_data(n).1
    }
}
