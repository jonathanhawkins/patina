//! WOFF (Web Open Font Format) 1.0 → SFNT reconstruction (pat-s3rrv.214).
//!
//! Godot's `FontFile::load_dynamic_font` accepts WOFF alongside TTF/OTF, and on
//! the web target a compressed web font is the normal shipping format. Patina
//! registered only `.ttf`/`.otf`, so a `.woff` face silently fell back to the
//! built-in bitmap font.
//!
//! A WOFF file is an SFNT (the TTF/OTF container) whose tables have each been
//! individually zlib-compressed, wrapped in a 44-byte header and a table
//! directory. Decoding is therefore *reconstruction*, not parsing: rebuild the
//! SFNT byte-for-byte and hand it to the existing `ttf_parser` path, which then
//! needs to know nothing about WOFF.
//!
//! # Scope
//!
//! ⚠️ **WOFF1 only.** WOFF2 uses **Brotli** rather than zlib, and no Brotli
//! implementation exists in this workspace (`Cargo.lock` has none, nothing
//! transitive provides one). The two formats share this file's shape — header,
//! table directory, reconstruct — so when a Brotli dependency lands, WOFF2 slots
//! in as an alternative decompressor plus its own transformed-glyf handling,
//! rather than as a rewrite. [`is_woff2`] exists so callers can tell the two
//! apart and produce an honest "unsupported" instead of a corrupt face.

use gdcore::error::{EngineError, EngineResult};

/// `wOFF` — the WOFF1 magic.
const WOFF1_SIGNATURE: u32 = 0x774F_4646;
/// `wOF2` — the WOFF2 magic, recognised only to be rejected clearly.
const WOFF2_SIGNATURE: u32 = 0x774F_4632;

/// Bytes of the WOFF header before the table directory.
const WOFF_HEADER_LEN: usize = 44;
/// Bytes per WOFF table-directory entry.
const WOFF_ENTRY_LEN: usize = 20;
/// Bytes per reconstructed SFNT table record.
const SFNT_RECORD_LEN: usize = 16;
/// Bytes of the SFNT header before the first table record.
const SFNT_HEADER_LEN: usize = 12;

/// An upper bound on a single decompressed table, so a corrupt `origLength`
/// cannot ask the allocator for gigabytes.
///
/// ⚠️ The limit is passed **into** the decoder rather than checked after it:
/// `miniz_oxide` stops at the cap instead of inflating first and being measured
/// afterwards, which is the difference between rejecting a zip bomb and
/// materialising one. Same reasoning as `http_request_node::decompress_body`.
const MAX_TABLE_LEN: usize = 64 * 1024 * 1024;

fn be_u16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be_u32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Whether `data` starts with the WOFF1 magic.
pub fn is_woff1(data: &[u8]) -> bool {
    data.len() >= 4 && be_u32(data, 0) == WOFF1_SIGNATURE
}

/// Whether `data` starts with the WOFF2 magic.
///
/// Patina cannot decode WOFF2 yet; this exists so a caller can say so plainly
/// rather than failing as though the file were malformed.
pub fn is_woff2(data: &[u8]) -> bool {
    data.len() >= 4 && be_u32(data, 0) == WOFF2_SIGNATURE
}

/// One entry of the WOFF table directory.
struct WoffTable {
    tag: u32,
    offset: usize,
    comp_length: usize,
    orig_length: usize,
    orig_checksum: u32,
}

/// Reconstructs the SFNT (TTF/OTF) bytes from a WOFF1 file.
///
/// The result is a complete, self-contained font that `ttf_parser` — or any
/// other SFNT consumer — accepts without knowing WOFF exists.
pub fn woff_to_sfnt(data: &[u8]) -> EngineResult<Vec<u8>> {
    if is_woff2(data) {
        return Err(EngineError::Unsupported(
            "woff: WOFF2 uses Brotli compression, which this build has no decoder for; \
             convert the font to .woff or .ttf"
                .to_string(),
        ));
    }
    if !is_woff1(data) {
        return Err(EngineError::Parse(
            "woff: not a WOFF file (bad signature)".to_string(),
        ));
    }
    if data.len() < WOFF_HEADER_LEN {
        return Err(EngineError::Parse("woff: truncated header".to_string()));
    }

    let flavor = be_u32(data, 4);
    let num_tables = be_u16(data, 12) as usize;
    if num_tables == 0 {
        return Err(EngineError::Parse("woff: font has no tables".to_string()));
    }

    let dir_end = WOFF_HEADER_LEN + num_tables * WOFF_ENTRY_LEN;
    if data.len() < dir_end {
        return Err(EngineError::Parse(format!(
            "woff: truncated table directory ({num_tables} tables need {dir_end} bytes, \
             file is {})",
            data.len()
        )));
    }

    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let at = WOFF_HEADER_LEN + i * WOFF_ENTRY_LEN;
        let tag = be_u32(data, at);
        let offset = be_u32(data, at + 4) as usize;
        let comp_length = be_u32(data, at + 8) as usize;
        let orig_length = be_u32(data, at + 12) as usize;
        let orig_checksum = be_u32(data, at + 16);

        // ⚠️ A compressed length ABOVE the original is not merely odd, it is how
        // a malformed file gets the reader to treat compressed bytes as stored
        // and hand `ttf_parser` garbage that still has a plausible length.
        if comp_length > orig_length {
            return Err(EngineError::Parse(format!(
                "woff: table {} claims compressed length {comp_length} > original \
                 {orig_length}",
                tag_name(tag)
            )));
        }
        if orig_length > MAX_TABLE_LEN {
            return Err(EngineError::Parse(format!(
                "woff: table {} declares an implausible length of {orig_length} bytes",
                tag_name(tag)
            )));
        }
        if offset.saturating_add(comp_length) > data.len() {
            return Err(EngineError::Parse(format!(
                "woff: table {} runs past the end of the file",
                tag_name(tag)
            )));
        }
        tables.push(WoffTable {
            tag,
            offset,
            comp_length,
            orig_length,
            orig_checksum,
        });
    }

    // 🔑 SFNT requires its table records sorted by tag, regardless of the order
    // the WOFF directory happened to use. `ttf_parser` binary-searches the
    // directory, so an unsorted one makes tables randomly "missing" rather than
    // failing outright — the worst kind of wrong.
    tables.sort_by_key(|t| t.tag);

    // Lay out the body: every table starts 4-byte aligned.
    let mut body_len = 0usize;
    for t in &tables {
        body_len += align4(t.orig_length);
    }
    let dir_len = SFNT_HEADER_LEN + tables.len() * SFNT_RECORD_LEN;
    let mut out = Vec::with_capacity(dir_len + body_len);

    // -- SFNT header --
    let n = tables.len() as u16;
    // `entrySelector` is floor(log2(numTables)); `searchRange` is 16 * 2^that.
    let entry_selector = (15 - n.leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = n * 16 - search_range;
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    // -- table records, then bodies --
    let mut cursor = dir_len;
    let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(tables.len());
    for t in &tables {
        let raw = &data[t.offset..t.offset + t.comp_length];
        let body = if t.comp_length == t.orig_length {
            // Stored: the spec says an entry whose lengths match is uncompressed,
            // which is what an already-incompressible table gets.
            raw.to_vec()
        } else {
            let inflated = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
                raw,
                t.orig_length.min(MAX_TABLE_LEN),
            )
            .map_err(|e| {
                EngineError::Parse(format!(
                    "woff: inflate failed for table {}: {e:?}",
                    tag_name(t.tag)
                ))
            })?;
            if inflated.len() != t.orig_length {
                return Err(EngineError::Parse(format!(
                    "woff: table {} inflated to {} bytes, header says {}",
                    tag_name(t.tag),
                    inflated.len(),
                    t.orig_length
                )));
            }
            inflated
        };

        out.extend_from_slice(&t.tag.to_be_bytes());
        out.extend_from_slice(&t.orig_checksum.to_be_bytes());
        out.extend_from_slice(&(cursor as u32).to_be_bytes());
        out.extend_from_slice(&(t.orig_length as u32).to_be_bytes());
        cursor += align4(body.len());
        bodies.push(body);
    }
    for body in bodies {
        let pad = align4(body.len()) - body.len();
        out.extend_from_slice(&body);
        out.extend(std::iter::repeat(0u8).take(pad));
    }

    Ok(out)
}

/// Rounds up to the next 4-byte boundary, as SFNT table alignment requires.
fn align4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

/// A table tag rendered for error messages (`0x676C7966` → `glyf`).
fn tag_name(tag: u32) -> String {
    let b = tag.to_be_bytes();
    b.iter()
        .map(|&c| {
            if (0x20..0x7f).contains(&c) {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}
