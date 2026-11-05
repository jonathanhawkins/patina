//! Godot's block-compressed file wrapper (`FileAccessCompressed`,
//! `core/io/file_access_compressed.cpp`).
//!
//! A binary resource saved with `ResourceSaver.FLAG_COMPRESS` — and every
//! binary `.scn` the editor writes by default — is NOT an `RSRC` file on disk:
//! it is an `RSRC` file split into fixed-size blocks, each compressed on its
//! own, behind an `RSCC` header. The official 3D platformer's whole level
//! (`stage/grid_map.scn`) ships this way, and before this module the loader
//! rejected it outright, so the GridMap instanced as an empty `Node`.
//!
//! ## Layout (all little-endian `u32`)
//!
//! ```text
//! magic          "RSCC"
//! mode           Compression::Mode (0 FastLZ, 1 Deflate, 2 Zstd, 3 GZip, 4 Brotli)
//! block_size     uncompressed bytes per block (4096 by default)
//! total_size     uncompressed size of the whole file
//! csize[n]       compressed size of each block, n = total_size / block_size + 1
//! blocks...      the compressed blocks, back to back
//! magic          "RSCC" again at the very end
//! ```
//!
//! Every block but the last decompresses to exactly `block_size` bytes; the
//! last to `total_size % block_size`. ⚠️ `n` is `total/block + 1` even when the
//! total is an exact multiple of the block size — the final block is then an
//! empty one — so `n` must be computed the way Godot does, not as a ceiling.

use gdcore::error::{EngineError, EngineResult};

/// Magic bytes of a `FileAccessCompressed` resource file.
pub const RSCC_MAGIC: &[u8; 4] = b"RSCC";

/// `Compression::Mode` values (`core/io/compression.h`).
const MODE_FASTLZ: u32 = 0;
const MODE_DEFLATE: u32 = 1;
const MODE_ZSTD: u32 = 2;
const MODE_GZIP: u32 = 3;
const MODE_BROTLI: u32 = 4;

/// Refuse to inflate more than this — a corrupt header must not make us
/// allocate gigabytes before the first block fails to decode.
const MAX_TOTAL_SIZE: usize = 1 << 30;

/// Whether `data` starts with the `RSCC` compressed-resource magic.
pub fn is_compressed_resource(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == RSCC_MAGIC
}

/// Decompresses an `RSCC` file into the bytes it wraps (an `RSRC` binary
/// resource, for any file Godot's resource saver wrote).
///
/// Supports the Zstd (the default for resources), Deflate and GZip modes.
/// FastLZ and Brotli return [`EngineError::Unsupported`]; truncated or
/// inconsistent input returns [`EngineError::Parse`] — never a panic.
pub fn decompress_rscc(data: &[u8]) -> EngineResult<Vec<u8>> {
    if !is_compressed_resource(data) {
        return Err(EngineError::Parse(
            "not a compressed resource: missing RSCC magic".into(),
        ));
    }
    let word = |at: usize| -> EngineResult<u32> {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| EngineError::Parse("compressed resource header truncated".into()))
    };
    let mode = word(4)?;
    let block_size = word(8)? as usize;
    let total = word(12)? as usize;
    if block_size == 0 {
        return Err(EngineError::Parse(
            "compressed resource has block size 0 (corrupt)".into(),
        ));
    }
    if total > MAX_TOTAL_SIZE {
        return Err(EngineError::Parse(format!(
            "compressed resource claims {total} bytes, over the {MAX_TOTAL_SIZE} limit"
        )));
    }
    match mode {
        MODE_ZSTD | MODE_DEFLATE | MODE_GZIP => {}
        MODE_FASTLZ | MODE_BROTLI => {
            return Err(EngineError::Unsupported(format!(
                "compressed resource mode {mode} ({}) is not supported",
                if mode == MODE_FASTLZ {
                    "FastLZ"
                } else {
                    "Brotli"
                }
            )))
        }
        other => {
            return Err(EngineError::Parse(format!(
                "compressed resource has unknown compression mode {other}"
            )))
        }
    }

    // Godot's block count, quirk included (see the module docs).
    let block_count = total / block_size + 1;
    let mut sizes = Vec::with_capacity(block_count);
    for i in 0..block_count {
        sizes.push(word(16 + i * 4)? as usize);
    }

    let mut out = Vec::with_capacity(total);
    let mut offset = 16 + block_count * 4;
    for (i, &csize) in sizes.iter().enumerate() {
        let expected = if i + 1 == block_count {
            total % block_size
        } else {
            block_size
        };
        let end = offset
            .checked_add(csize)
            .filter(|&e| e <= data.len())
            .ok_or_else(|| {
                EngineError::Parse(format!(
                    "compressed resource block {i} truncated: needs {csize} byte(s) at offset {offset}"
                ))
            })?;
        let block = &data[offset..end];
        offset = end;
        if expected == 0 {
            continue;
        }
        let decoded = decompress_block(mode, block)?;
        if decoded.len() != expected {
            return Err(EngineError::Parse(format!(
                "compressed resource block {i} decoded to {} byte(s), expected {expected}",
                decoded.len()
            )));
        }
        out.extend_from_slice(&decoded);
    }
    Ok(out)
}

/// Decompresses one block in the given `Compression::Mode`.
fn decompress_block(mode: u32, block: &[u8]) -> EngineResult<Vec<u8>> {
    let err = |what: &str, e: String| EngineError::Parse(format!("{what} block: {e}"));
    match mode {
        MODE_ZSTD => {
            use std::io::Read;
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(block)
                .map_err(|e| err("zstd", e.to_string()))?;
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| err("zstd", e.to_string()))?;
            Ok(out)
        }
        // Godot's DEFLATE mode is zlib-wrapped (`deflateInit` with default
        // window bits), GZIP is the gzip wrapper around the same stream.
        MODE_DEFLATE => miniz_oxide::inflate::decompress_to_vec_zlib(block)
            .map_err(|e| err("deflate", format!("{e:?}"))),
        MODE_GZIP => {
            let body = strip_gzip_header(block)?;
            miniz_oxide::inflate::decompress_to_vec(body).map_err(|e| err("gzip", format!("{e:?}")))
        }
        other => Err(EngineError::Unsupported(format!(
            "compression mode {other} is not supported"
        ))),
    }
}

/// Returns the raw deflate stream inside a gzip member (RFC 1952), skipping
/// the header's optional fields. The 8-byte trailer is ignored by the inflater.
fn strip_gzip_header(data: &[u8]) -> EngineResult<&[u8]> {
    let short = || EngineError::Parse("gzip block header truncated".into());
    if data.len() < 10 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return Err(EngineError::Parse("gzip block has a bad header".into()));
    }
    let flags = data[3];
    let mut pos = 10;
    if flags & 0x04 != 0 {
        let xlen = *data.get(pos).ok_or_else(short)? as usize
            | (*data.get(pos + 1).ok_or_else(short)? as usize) << 8;
        pos += 2 + xlen;
    }
    for bit in [0x08u8, 0x10] {
        if flags & bit != 0 {
            while *data.get(pos).ok_or_else(short)? != 0 {
                pos += 1;
            }
            pos += 1;
        }
    }
    if flags & 0x02 != 0 {
        pos += 2;
    }
    data.get(pos..).ok_or_else(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an RSCC file the way `FileAccessCompressed::_close` does.
    fn build_rscc(payload: &[u8], block_size: usize, mode: u32) -> Vec<u8> {
        let block_count = payload.len() / block_size + 1;
        let mut blocks = Vec::new();
        for i in 0..block_count {
            let start = i * block_size;
            let end = if i + 1 == block_count {
                payload.len()
            } else {
                start + block_size
            };
            let raw = &payload[start..end];
            let c = match mode {
                MODE_ZSTD => ruzstd::encoding::compress_to_vec(
                    raw,
                    ruzstd::encoding::CompressionLevel::Fastest,
                ),
                MODE_DEFLATE => miniz_oxide::deflate::compress_to_vec_zlib(raw, 6),
                _ => unreachable!(),
            };
            blocks.push(c);
        }
        let mut out = RSCC_MAGIC.to_vec();
        out.extend_from_slice(&mode.to_le_bytes());
        out.extend_from_slice(&(block_size as u32).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        for b in &blocks {
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        }
        for b in &blocks {
            out.extend_from_slice(b);
        }
        out.extend_from_slice(RSCC_MAGIC);
        out
    }

    fn payload(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[test]
    fn zstd_multi_block_round_trips() {
        let data = payload(10_000);
        let file = build_rscc(&data, 4096, MODE_ZSTD);
        assert_eq!(decompress_rscc(&file).unwrap(), data);
    }

    #[test]
    fn deflate_round_trips() {
        let data = payload(5000);
        let file = build_rscc(&data, 1024, MODE_DEFLATE);
        assert_eq!(decompress_rscc(&file).unwrap(), data);
    }

    /// Edge case: a total that is an exact multiple of the block size still
    /// carries a trailing EMPTY block, which must be skipped, not decoded.
    #[test]
    fn exact_multiple_of_block_size_has_an_empty_last_block() {
        let data = payload(8192);
        // Three size words (two full blocks + the empty one): a ceiling block
        // count would read only two and start the blocks 4 bytes early.
        let file = build_rscc(&data, 4096, MODE_ZSTD);
        assert_eq!(decompress_rscc(&file).unwrap(), data);
    }

    #[test]
    fn fastlz_is_reported_unsupported() {
        let mut file = RSCC_MAGIC.to_vec();
        for w in [MODE_FASTLZ, 4096, 10] {
            file.extend_from_slice(&w.to_le_bytes());
        }
        let err = decompress_rscc(&file).unwrap_err();
        assert!(matches!(err, EngineError::Unsupported(_)), "{err}");
    }

    #[test]
    fn truncated_block_is_a_parse_error_not_a_panic() {
        let data = payload(10_000);
        let file = build_rscc(&data, 4096, MODE_ZSTD);
        for cut in [3, 15, 20, 40, file.len() / 2, file.len() - 5] {
            assert!(decompress_rscc(&file[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn zero_block_size_is_rejected() {
        let mut file = RSCC_MAGIC.to_vec();
        for w in [MODE_ZSTD, 0, 10] {
            file.extend_from_slice(&w.to_le_bytes());
        }
        assert!(decompress_rscc(&file).is_err());
    }
}
