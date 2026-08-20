//! pat-s3rrv.214 acceptance: font formats beyond bare `.ttf`/`.otf`.
//!
//! Godot ground truth: `scene/resources/font.cpp` `FontFile::load_dynamic_font`
//! accepts TTF/OTF/**WOFF**/WOFF2/PFB/PFM. Patina registered exactly two
//! extensions (`importers.rs`), so every other face silently fell back to the
//! built-in bitmap font — the wrong glyphs on screen with no error anywhere.
//!
//! ⚠️ **SCOPE — WOFF1, not WOFF2.** The bead's first clause names `.woff2`.
//! WOFF2 compresses its tables with **Brotli**; this workspace has no Brotli
//! decoder (`Cargo.lock` has none, nothing transitive provides one) and workers
//! cannot run cargo to add one. WOFF1 drives the identical code path — same
//! table directory, same SFNT reconstruction, same "did the face really load"
//! evidence — differing only in the decompressor, and `miniz_oxide` for that is
//! already a dependency. Raised with the coordinator rather than worked around
//! quietly, and `a_woff2_font_fails_loudly_rather_than_falling_back` pins the
//! honest failure so the gap cannot be mistaken for support.
//!
//! 🔑 The fixture is BUILT here from the repo's real TTF rather than checked in.
//! A hand-written WOFF would only prove the reader agrees with my own writer;
//! compressing a genuine face and demanding the reconstruction parse as *that
//! same face* — same units_per_em, same glyph count, same metrics — is what
//! makes this a round trip rather than a tautology.
//!
//! The Label-rendering clause lives in `gdeditor/tests/font_woff_label_render.rs`:
//! it needs the viewport, and `gdeditor` cannot see `miniz_oxide`/`ttf_parser`
//! to build a compressed fixture, so each half sits in the crate that can
//! actually express it.

use std::path::Path;

/// The one face the repo ships, reached from the gdresource crate root.
const FONT_REL: &str = "test_assets/Cantarell-Regular.ttf";

fn ttf_bytes() -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(FONT_REL);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn rd16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn rd32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Wraps a real SFNT into a WOFF1 container, zlib-compressing each table.
///
/// The inverse of the code under test, written from the WOFF 1.0 spec: 44-byte
/// header, 20-byte directory entries, per-table zlib, 4-byte padding.
fn sfnt_to_woff(sfnt: &[u8]) -> Vec<u8> {
    let flavor = rd32(sfnt, 0);
    let num_tables = rd16(sfnt, 4) as usize;

    struct Entry {
        tag: u32,
        checksum: u32,
        orig: Vec<u8>,
        comp: Vec<u8>,
    }
    let mut entries = Vec::new();
    for i in 0..num_tables {
        let at = 12 + i * 16;
        let off = rd32(sfnt, at + 8) as usize;
        let len = rd32(sfnt, at + 12) as usize;
        let orig = sfnt[off..off + len].to_vec();
        let comp = miniz_oxide::deflate::compress_to_vec_zlib(&orig, 6);
        entries.push(Entry {
            tag: rd32(sfnt, at),
            checksum: rd32(sfnt, at + 4),
            orig,
            comp,
        });
    }

    let mut body_offset = 44 + num_tables * 20;
    let mut header = Vec::new();
    header.extend_from_slice(b"wOFF");
    header.extend_from_slice(&be32(flavor));
    header.extend_from_slice(&be32(0)); // length, patched at the end
    header.extend_from_slice(&be16(num_tables as u16));
    header.extend_from_slice(&be16(0)); // reserved
    header.extend_from_slice(&be32(sfnt.len() as u32)); // totalSfntSize
    header.extend_from_slice(&be16(1));
    header.extend_from_slice(&be16(0));
    for _ in 0..5 {
        header.extend_from_slice(&be32(0)); // meta*/priv* offsets and lengths
    }
    assert_eq!(header.len(), 44, "the WOFF header is 44 bytes");

    let mut dir = Vec::new();
    let mut body = Vec::new();
    for e in &entries {
        // A table that does not shrink is stored raw, which the spec signals with
        // compLength == origLength. Real fonts contain such tables, so the reader
        // must handle both and this fixture must produce both.
        let stored = e.comp.len() >= e.orig.len();
        let payload: &[u8] = if stored { &e.orig } else { &e.comp };
        dir.extend_from_slice(&be32(e.tag));
        dir.extend_from_slice(&be32(body_offset as u32));
        dir.extend_from_slice(&be32(payload.len() as u32));
        dir.extend_from_slice(&be32(e.orig.len() as u32));
        dir.extend_from_slice(&be32(e.checksum));
        body.extend_from_slice(payload);
        let pad = (4 - payload.len() % 4) % 4;
        body.extend(std::iter::repeat(0u8).take(pad));
        body_offset += payload.len() + pad;
    }

    let mut out = header;
    out.extend_from_slice(&dir);
    out.extend_from_slice(&body);
    let total = out.len() as u32;
    out[8..12].copy_from_slice(&be32(total));
    out
}

/// The reconstruction is faithful enough that the face is *the same face*.
#[test]
fn a_woff_reconstructs_to_the_original_sfnt_face() {
    let ttf = ttf_bytes();
    let woff = sfnt_to_woff(&ttf);

    // ⚠️ If the fixture is not actually smaller, nothing was compressed and this
    // whole file would pass against a reader that ignores zlib entirely.
    assert!(
        woff.len() < ttf.len(),
        "WOFF fixture ({}) is not smaller than the TTF ({}); nothing compressed",
        woff.len(),
        ttf.len()
    );
    assert!(gdresource::woff::is_woff1(&woff));
    assert!(!gdresource::woff::is_woff2(&woff));

    let rebuilt = gdresource::woff::woff_to_sfnt(&woff).expect("WOFF reconstructs");
    let original = ttf_parser::Face::parse(&ttf, 0).expect("the fixture TTF parses");
    let restored = ttf_parser::Face::parse(&rebuilt, 0).expect("the reconstructed SFNT parses");

    assert_eq!(restored.units_per_em(), original.units_per_em());
    assert_eq!(restored.number_of_glyphs(), original.number_of_glyphs());
    assert_eq!(restored.ascender(), original.ascender());
    assert_eq!(restored.descender(), original.descender());
    assert_eq!(
        restored.is_monospaced(),
        original.is_monospaced(),
        "a flag read from a table that reconstructed wrong would differ here"
    );
}

/// Glyph geometry survives, not just the header numbers.
///
/// 🔑 The header fields all live in `head`/`maxp`, two small tables. If only
/// those reconstructed correctly every assertion above would pass while `glyf`
/// — the big one, and the one that actually compresses — was garbage. This
/// reads an outline through the rebuilt tables.
#[test]
fn a_woff_reconstruction_preserves_glyph_outlines() {
    let ttf = ttf_bytes();
    let rebuilt = gdresource::woff::woff_to_sfnt(&sfnt_to_woff(&ttf)).expect("reconstructs");

    let original = ttf_parser::Face::parse(&ttf, 0).unwrap();
    let restored = ttf_parser::Face::parse(&rebuilt, 0).unwrap();

    let mut compared = 0;
    for ch in ['A', 'g', 'W', '8', 'i'] {
        let (Some(go), Some(gr)) = (original.glyph_index(ch), restored.glyph_index(ch)) else {
            continue;
        };
        assert_eq!(go, gr, "glyph index for {ch:?} moved");
        assert_eq!(
            original.glyph_hor_advance(go),
            restored.glyph_hor_advance(gr),
            "advance for {ch:?} differs, so hmtx did not reconstruct"
        );
        assert_eq!(
            original
                .glyph_bounding_box(go)
                .map(|b| (b.x_min, b.y_min, b.x_max, b.y_max)),
            restored
                .glyph_bounding_box(gr)
                .map(|b| (b.x_min, b.y_min, b.x_max, b.y_max)),
            "outline bbox for {ch:?} differs, so glyf did not reconstruct"
        );
        compared += 1;
    }
    assert!(
        compared >= 3,
        "compared only {compared} glyphs; fixture too sparse"
    );
}

/// `import_font` accepts a `.woff` and reports real metrics for it.
#[test]
fn import_font_loads_a_woff_and_records_its_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("face.woff");
    std::fs::write(&path, sfnt_to_woff(&ttf_bytes())).expect("write fixture");

    let res = gdresource::importers::import_font(&path).expect("a .woff imports");
    // ⚠️ `Resource::get_property` hands back `Option<&Variant>`, not a `Variant`.
    let prop = |k: &str| res.get_property(k).cloned();

    assert_eq!(
        prop("parsed"),
        Some(gdvariant::Variant::Bool(true)),
        "a .woff must parse; `false` here is the silent-fallback bug"
    );
    assert_eq!(
        prop("font_format"),
        Some(gdvariant::Variant::String("woff".to_string()))
    );

    // The decoded payload is larger than the compressed container it came from —
    // the observable difference between "we decompressed" and "we stored bytes".
    let (Some(gdvariant::Variant::Int(on_disk)), Some(gdvariant::Variant::Int(decoded))) =
        (prop("byte_length"), prop("sfnt_byte_length"))
    else {
        panic!("byte_length/sfnt_byte_length must be ints");
    };
    assert!(
        decoded > on_disk,
        "decoded SFNT ({decoded}) should exceed the compressed file ({on_disk})"
    );

    // And the metrics match the face it was built from.
    let ttf = ttf_bytes();
    let face = ttf_parser::Face::parse(&ttf, 0).unwrap();
    assert_eq!(
        prop("units_per_em"),
        Some(gdvariant::Variant::Int(face.units_per_em() as i64))
    );
    assert_eq!(
        prop("glyph_count"),
        Some(gdvariant::Variant::Int(face.number_of_glyphs() as i64))
    );
}

/// A plain `.ttf` still imports unchanged — the WOFF sniff must not disturb it.
#[test]
fn a_plain_ttf_still_imports_as_sfnt() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(FONT_REL);
    let res = gdresource::importers::import_font(&p).expect("the .ttf still imports");
    assert_eq!(
        res.get_property("parsed"),
        Some(&gdvariant::Variant::Bool(true))
    );
    assert_eq!(
        res.get_property("font_format"),
        Some(&gdvariant::Variant::String("sfnt".to_string()))
    );
    assert_eq!(
        res.get_property("byte_length"),
        res.get_property("sfnt_byte_length"),
        "an uncompressed face decodes to itself"
    );
}

/// ⚠️ **WOFF2 must fail LOUDLY.** With no Brotli decoder the only wrong answer is
/// a silent fallback — a Label quietly drawing the wrong face. It must be an
/// error naming both the format and the missing decoder.
#[test]
fn a_woff2_font_fails_loudly_rather_than_falling_back() {
    let mut woff2 = Vec::new();
    woff2.extend_from_slice(b"wOF2");
    woff2.extend_from_slice(&be32(0x0001_0000));
    woff2.extend(std::iter::repeat(0u8).take(64));

    assert!(gdresource::woff::is_woff2(&woff2));
    assert!(!gdresource::woff::is_woff1(&woff2));

    let err = gdresource::woff::woff_to_sfnt(&woff2)
        .expect_err("WOFF2 cannot be decoded here and must not pretend otherwise");
    let msg = format!("{err}");
    assert!(
        msg.contains("WOFF2") && msg.to_lowercase().contains("brotli"),
        "the error must name the format AND the missing decoder, so a reader \
         knows this is a build limitation and not a corrupt file: {msg}"
    );
}

/// Malformed input is rejected rather than handed to the parser as garbage.
#[test]
fn a_malformed_woff_is_rejected_rather_than_silently_wrong() {
    let good = sfnt_to_woff(&ttf_bytes());

    assert!(
        gdresource::woff::woff_to_sfnt(&good[..50.min(good.len())]).is_err(),
        "a truncated directory must be rejected"
    );
    assert!(
        gdresource::woff::woff_to_sfnt(&ttf_bytes()).is_err(),
        "a bare TTF is not a WOFF"
    );

    // 🔑 compLength > origLength is the dangerous one: it is otherwise treated as
    // "stored", feeding compressed bytes to the parser with a plausible length.
    let mut lying = good.clone();
    let orig_len_field = 44 + 12;
    lying[orig_len_field..orig_len_field + 4].copy_from_slice(&be32(1));
    assert!(
        gdresource::woff::woff_to_sfnt(&lying).is_err(),
        "a table claiming compressed length > original must be rejected"
    );
}
