//! pat-0q2kx.217 — `PortableCompressedTexture2D`.
//!
//! Godot's `PortableCompressedTexture2D` stores compressed image data **inside
//! the resource itself** rather than referencing a `.ctex` produced by the
//! import pipeline. That is what makes it portable: a `.tres` carrying one is
//! self-contained and loads without the `.godot/imported/` sidecar.
//!
//! The class was absent here entirely — no implementation and no ClassDB entry,
//! so a `.tscn`/`.tres` referencing the type could not load at all.
//!
//! Godot: `doc/classes/PortableCompressedTexture2D.xml`, `scene/resources/
//! portable_compressed_texture.cpp`.

use crate::image::{Image, ImageFormat};
use gdcore::error::{EngineError, EngineResult};

/// Godot's `PortableCompressedTexture2D.CompressionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortableCompressionMode {
    Lossless = 0,
    Lossy = 1,
    BasisUniversal = 2,
    S3tc = 3,
    Etc2 = 4,
    Bptc = 5,
    /// Godot 4 added ASTC as mode 6 — easy to miss if you transcribe the enum
    /// from memory instead of `PortableCompressedTexture2D.xml`.
    Astc = 6,
}

impl PortableCompressionMode {
    /// Maps the script-facing integer onto a mode, as `.tres` stores it.
    pub fn from_i64(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Lossless,
            1 => Self::Lossy,
            2 => Self::BasisUniversal,
            3 => Self::S3tc,
            4 => Self::Etc2,
            5 => Self::Bptc,
            6 => Self::Astc,
            _ => return None,
        })
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Whether this mode is stored byte-exactly.
    ///
    /// Only `LOSSLESS` is: the block-compressed modes (S3TC/ETC2/BPTC),
    /// Basis Universal and WebP-lossy all need encoders this engine does not
    /// have yet — `block_compression.rs` implements BC1/BC3 **decode** only.
    /// See [`PortableCompressedTexture2D::create_from_image`] for what happens
    /// when one of those is requested.
    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Lossless)
    }
}

/// A texture that carries its own compressed payload.
#[derive(Debug, Clone)]
pub struct PortableCompressedTexture2D {
    width: u32,
    height: u32,
    /// The source image's format, so `get_image` hands back what went in
    /// rather than always RGBA8.
    source_format: ImageFormat,
    /// The mode the caller asked for — preserved verbatim so it round-trips
    /// through a `.tres` even when the payload had to fall back (below).
    requested_mode: PortableCompressionMode,
    /// The compressed bytes (PNG).
    data: Vec<u8>,
    keep_compressed_buffer: bool,
}

impl PortableCompressedTexture2D {
    /// Godot's `create_from_image(image, compression_mode, ...)`.
    ///
    /// **`LOSSLESS` is a real PNG** — deflate-compressed, byte-exact, and
    /// readable by any PNG decoder, which is what "portable" is supposed to
    /// mean.
    ///
    /// ⚠️ The other six modes currently store the *same* lossless PNG payload
    /// while recording the requested mode. That is a deliberate,
    /// documented-and-reported fallback rather than silent breakage: there is
    /// no S3TC/ETC2/BPTC/Basis encoder in the engine yet, and a texture that
    /// decodes correctly at the wrong compression is far better than one that
    /// stores garbage. `is_lossless()` tells a caller which it got.
    pub fn create_from_image(
        image: &Image,
        compression_mode: PortableCompressionMode,
    ) -> EngineResult<Self> {
        if image.get_width() == 0 || image.get_height() == 0 {
            return Err(EngineError::Parse(
                "PortableCompressedTexture2D: cannot compress a zero-sized image".into(),
            ));
        }

        // PNG is RGBA8; the source format is recorded so `get_image` restores it.
        let rgba = image.convert(ImageFormat::Rgba8);
        let data = encode_png_rgba8(rgba.get_width(), rgba.get_height(), rgba.get_data())?;

        Ok(Self {
            width: image.get_width(),
            height: image.get_height(),
            source_format: image.get_format(),
            requested_mode: compression_mode,
            data,
            // Godot's property default is `false`: it means "you may free the
            // compressed source after the GPU texture exists", not "discard it
            // now". There is no upload stage here, so the payload is retained
            // and `discard_compressed_buffer` is the explicit stand-in.
            keep_compressed_buffer: false,
        })
    }

    /// Godot's `get_image()` — decodes the stored payload back to an `Image`.
    pub fn get_image(&self) -> EngineResult<Image> {
        if self.data.is_empty() {
            return Err(EngineError::Parse(
                "PortableCompressedTexture2D: the compressed buffer was discarded".into(),
            ));
        }
        let (width, height, rgba) = decode_png_rgba8(&self.data)?;

        let image = Image::create_from_data(width, height, ImageFormat::Rgba8, rgba);
        Ok(if self.source_format == ImageFormat::Rgba8 {
            image
        } else {
            image.convert(self.source_format)
        })
    }

    pub fn get_width(&self) -> u32 {
        self.width
    }

    pub fn get_height(&self) -> u32 {
        self.height
    }

    /// Godot's `get_size()`.
    pub fn get_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The mode this texture was created with.
    pub fn compression_mode(&self) -> PortableCompressionMode {
        self.requested_mode
    }

    /// Whether the stored payload is byte-exact for the requested mode.
    pub fn is_lossless(&self) -> bool {
        self.requested_mode.is_lossless()
    }

    /// Godot's `keep_compressed_buffer` — whether the compressed bytes are
    /// retained after the GPU texture is created. Dropping them saves memory
    /// but makes `get_image()` unavailable, which is exactly Godot's tradeoff.
    pub fn is_keeping_compressed_buffer(&self) -> bool {
        self.keep_compressed_buffer
    }

    pub fn set_keep_compressed_buffer(&mut self, keep: bool) {
        self.keep_compressed_buffer = keep;
    }

    /// Frees the compressed payload.
    ///
    /// Godot drops it right after the GPU texture is created when
    /// `keep_compressed_buffer` is false. This engine has no upload stage yet,
    /// so the free is explicit rather than implicit — `get_image()` stops
    /// working afterwards, exactly as in Godot.
    pub fn discard_compressed_buffer(&mut self) {
        self.data = Vec::new();
    }

    /// The raw compressed payload, empty once the buffer has been dropped.
    pub fn compressed_bytes(&self) -> &[u8] {
        &self.data
    }
}

// ---------------------------------------------------------------------------
// Minimal PNG encoder
// ---------------------------------------------------------------------------

/// Encodes RGBA8 pixels as a PNG.
///
/// Paired with [`decode_png_rgba8`] below. Uses `miniz_oxide`, already in the
/// dependency graph, rather than pulling in a second codec. Every scanline gets
/// filter byte 0 (None), which keeps the encoder trivial and still yields real
/// deflate compression.
fn encode_png_rgba8(width: u32, height: u32, rgba: &[u8]) -> EngineResult<Vec<u8>> {
    use miniz_oxide::deflate::compress_to_vec_zlib;

    let expected = width as usize * height as usize * 4;
    if rgba.len() < expected {
        return Err(EngineError::Parse(format!(
            "PortableCompressedTexture2D: expected {expected} RGBA bytes, got {}",
            rgba.len()
        )));
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR: 8-bit, color type 6 (RGBA), no interlace.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);

    // Raw scanlines, each prefixed with its filter byte.
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity(height as usize * (stride + 1));
    for row in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[row * stride..row * stride + stride]);
    }
    write_chunk(&mut out, b"IDAT", &compress_to_vec_zlib(&raw, 6));
    write_chunk(&mut out, b"IEND", &[]);

    Ok(out)
}

/// Decodes a PNG produced by [`encode_png_rgba8`] back to RGBA8 bytes.
///
/// Deliberately narrow: it accepts only 8-bit RGBA, non-interlaced, filter-0
/// scanlines — exactly what this module writes. That is not a shortcut, it is
/// the reason the round-trip is provably exact; a general PNG decoder would
/// have to guess at formats this resource never emits.
///
/// ⚠️ `gdresource` **does** contain a full hand-rolled decoder in
/// `texture_import.rs`, and I originally called it. It cannot be used: that
/// file is an **orphaned module** — 25 KB never declared in `lib.rs`, so the
/// compiler has never seen it (`could not find texture_import in the crate
/// root`). Declaring it is real work with its own risk and belongs in its own
/// bead; reported separately rather than smuggled in here.
fn decode_png_rgba8(data: &[u8]) -> EngineResult<(u32, u32, Vec<u8>)> {
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    const MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 8 || data[..8] != MAGIC {
        return Err(EngineError::Parse("not a PNG (bad magic)".into()));
    }

    let mut pos = 8;
    let mut dims: Option<(u32, u32)> = None;
    let mut idat = Vec::new();

    // Walk the chunk list: length | type | data | crc.
    while pos + 8 <= data.len() {
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let kind = &data[pos + 4..pos + 8];
        let body_start = pos + 8;
        if body_start + len > data.len() {
            return Err(EngineError::Parse("PNG chunk truncated".into()));
        }
        let body = &data[body_start..body_start + len];

        match kind {
            b"IHDR" => {
                if len < 13 {
                    return Err(EngineError::Parse("PNG IHDR too short".into()));
                }
                let w = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                let h = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                if body[8] != 8 || body[9] != 6 || body[12] != 0 {
                    return Err(EngineError::Parse(format!(
                        "unsupported PNG: depth {} colour type {} interlace {}",
                        body[8], body[9], body[12]
                    )));
                }
                dims = Some((w, h));
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos = body_start + len + 4;
    }

    let (width, height) = dims.ok_or_else(|| EngineError::Parse("PNG has no IHDR".into()))?;
    let raw = decompress_to_vec_zlib(&idat)
        .map_err(|e| EngineError::Parse(format!("PNG inflate failed: {e:?}")))?;

    let stride = width as usize * 4;
    let expected = height as usize * (stride + 1);
    if raw.len() < expected {
        return Err(EngineError::Parse(format!(
            "PNG data short: expected {expected} bytes, got {}",
            raw.len()
        )));
    }

    let mut out = Vec::with_capacity(height as usize * stride);
    for row in 0..height as usize {
        let start = row * (stride + 1);
        // Filter byte. Only None(0) is emitted by our encoder; anything else
        // means the payload did not come from here and cannot be trusted to
        // decode correctly, so say so rather than returning wrong pixels.
        if raw[start] != 0 {
            return Err(EngineError::Parse(format!(
                "unsupported PNG row filter {} (only filter 0 is written by this encoder)",
                raw[start]
            )));
        }
        out.extend_from_slice(&raw[start + 1..start + 1 + stride]);
    }

    Ok((width, height, out))
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&png_crc32(kind, data).to_be_bytes());
}

/// CRC-32 over the chunk type followed by its data, as PNG specifies.
fn png_crc32(kind: &[u8], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in kind.iter().chain(data.iter()) {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Image {
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let on = (x + y) % 2 == 0;
                data.extend_from_slice(if on {
                    &[200, 30, 90, 255]
                } else {
                    &[10, 240, 60, 128]
                });
            }
        }
        Image::create_from_data(w, h, ImageFormat::Rgba8, data)
    }

    #[test]
    fn lossless_roundtrip_is_byte_exact() {
        let src = checker(7, 5);
        let tex =
            PortableCompressedTexture2D::create_from_image(&src, PortableCompressionMode::Lossless)
                .expect("compresses");
        let back = tex.get_image().expect("decodes");

        assert_eq!(back.get_width(), 7);
        assert_eq!(back.get_height(), 5);
        assert_eq!(
            back.get_data(),
            src.get_data(),
            "LOSSLESS must round-trip byte-for-byte, including the alpha channel",
        );
    }

    #[test]
    fn payload_is_a_real_png_and_actually_smaller() {
        // A large flat image: deflate should crush it, proving the payload is
        // genuinely compressed rather than raw bytes with a header.
        let src = Image::create_from_data(64, 64, ImageFormat::Rgba8, vec![17u8; 64 * 64 * 4]);
        let tex =
            PortableCompressedTexture2D::create_from_image(&src, PortableCompressionMode::Lossless)
                .expect("compresses");

        assert_eq!(
            &tex.compressed_bytes()[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "the payload must be a real PNG",
        );
        assert!(
            tex.compressed_bytes().len() < 64 * 64 * 4 / 4,
            "a flat image must compress hard, got {} bytes",
            tex.compressed_bytes().len(),
        );
    }

    #[test]
    fn zero_sized_image_is_rejected() {
        let empty = Image::create_from_data(0, 0, ImageFormat::Rgba8, Vec::new());
        assert!(
            PortableCompressedTexture2D::create_from_image(
                &empty,
                PortableCompressionMode::Lossless
            )
            .is_err(),
            "a zero-sized image has nothing to compress",
        );
    }

    #[test]
    fn keep_flag_defaults_false_but_the_payload_survives() {
        let src = checker(4, 4);
        let mut tex =
            PortableCompressedTexture2D::create_from_image(&src, PortableCompressionMode::Lossless)
                .expect("compresses");

        // Godot's default for the property is false...
        assert!(!tex.is_keeping_compressed_buffer());
        // ...but that means "free it after upload", so the bytes are still here
        // and get_image() works — which is what the acceptance depends on.
        assert!(!tex.compressed_bytes().is_empty());
        assert!(tex.get_image().is_ok());

        tex.set_keep_compressed_buffer(true);
        assert!(tex.is_keeping_compressed_buffer());

        tex.discard_compressed_buffer();
        assert!(
            tex.compressed_bytes().is_empty(),
            "discarding must actually free the payload, not just flip a flag",
        );
        assert!(
            tex.get_image().is_err(),
            "get_image() must report the discarded buffer rather than decoding \
             an empty slice into a blank image",
        );
    }

    #[test]
    fn mode_round_trips_even_when_the_payload_falls_back() {
        let src = checker(4, 4);
        let tex =
            PortableCompressedTexture2D::create_from_image(&src, PortableCompressionMode::S3tc)
                .expect("compresses");

        assert_eq!(
            tex.compression_mode(),
            PortableCompressionMode::S3tc,
            "the requested mode must be preserved for `.tres` round-tripping",
        );
        assert!(
            !tex.is_lossless(),
            "S3TC must report itself as not byte-exact so callers can tell",
        );
        // …and the image still decodes correctly despite the fallback.
        assert_eq!(tex.get_image().expect("decodes").get_data(), src.get_data());
    }

    #[test]
    fn mode_ints_match_godots_enum() {
        for (value, mode) in [
            (0, PortableCompressionMode::Lossless),
            (1, PortableCompressionMode::Lossy),
            (2, PortableCompressionMode::BasisUniversal),
            (3, PortableCompressionMode::S3tc),
            (4, PortableCompressionMode::Etc2),
            (5, PortableCompressionMode::Bptc),
            (6, PortableCompressionMode::Astc),
        ] {
            assert_eq!(PortableCompressionMode::from_i64(value), Some(mode));
            assert_eq!(mode.as_i64(), value);
        }
        assert_eq!(PortableCompressionMode::from_i64(7), None);
    }
}
