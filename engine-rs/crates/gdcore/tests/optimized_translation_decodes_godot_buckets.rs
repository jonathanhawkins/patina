//! pat-s3rrv.213 (part 1) — the binary `OptimizedTranslation` decoder.
//!
//! Ground truth: `upstream/godot/core/string/optimized_translation.{h,cpp}` and
//! `upstream/godot/thirdparty/misc/smaz.c`, both vendored.
//!
//! # Scope — this is one part of a five-clause bead
//!
//! Delivered here: the payload decoder (hash → bucket → SMAZ) as a pure,
//! panic-free module. **Not** delivered, and deliberately not claimed:
//!
//! - ClassDB registration of `Translation` / `OptimizedTranslation`
//!   (acceptance clause 1). `gdobject/src/class_db.rs` was under another
//!   agent's exclusive lease for the whole of this work.
//! - Dispatching `load()` and `patina-runner`'s `load_project_translations` on
//!   the real resource type instead of the file extension (clauses 2 and 4).
//!   `gdcore/src/translation.rs` was also leased.
//! - A fixture produced by **Godot's own** CSV importer. See the note on
//!   evidence below — that is the one thing these tests cannot stand in for.
//!
//! # How these fixtures avoid proving nothing
//!
//! ⚠️ The obvious way to test a decoder is to write the matching encoder and
//! round-trip. That would prove only that my two halves agree with each other —
//! it cannot catch a shared misreading of the format. So nothing here uses an
//! encoder:
//!
//! - The SMAZ vectors are assembled **by hand** from the three rules in
//!   `smaz_decompress` (code byte, `254` = verbatim byte, `255` = verbatim run)
//!   plus codebook indices read straight out of `smaz.c`.
//! - The bucket tables are laid out **by hand** as `u32` words, in the layout
//!   the C `struct Bucket` overlays.
//! - The expected hash values come from the algorithm in the vendored header,
//!   recomputed independently rather than captured from this implementation's
//!   output.
//!
//! What that still does **not** prove is byte-compatibility with a real
//! `.translation` file emitted by Godot 4.6.1. That needs a fixture from the
//! actual importer, which cannot be produced here.

use gdcore::optimized_translation::{
    hash, smaz_decompress, OptimizedTranslation, SMAZ_REVERSE_CODEBOOK,
};

// ---------------------------------------------------------------------------
// The codebook
// ---------------------------------------------------------------------------

/// The table is the part of SMAZ a hand transcription silently corrupts.
///
/// ⚠️ 51 of the 254 entries differ from another entry only in leading/trailing
/// whitespace, so a dropped space does not fail loudly — it mis-decodes only
/// the strings that used the variant that got lost.
#[test]
fn a_the_smaz_codebook_matches_the_vendored_table() {
    assert_eq!(SMAZ_REVERSE_CODEBOOK.len(), 254);

    // Read directly off `smaz.c`'s first line of entries.
    assert_eq!(SMAZ_REVERSE_CODEBOOK[0], " ");
    assert_eq!(SMAZ_REVERSE_CODEBOOK[1], "the");
    assert_eq!(SMAZ_REVERSE_CODEBOOK[2], "e");
    assert_eq!(SMAZ_REVERSE_CODEBOOK[3], "t");
    assert_eq!(SMAZ_REVERSE_CODEBOOK[4], "a");
    assert_eq!(SMAZ_REVERSE_CODEBOOK[5], "of");
    // …and the last, which a truncated copy loses.
    assert_eq!(SMAZ_REVERSE_CODEBOOK[253], ".com");

    // 🔑 The whitespace variants are distinct codes, not duplicates.
    let a_forms: Vec<&&str> = SMAZ_REVERSE_CODEBOOK
        .iter()
        .filter(|e| e.trim() == "a" && !e.is_empty())
        .collect();
    assert!(
        a_forms.len() >= 3,
        "`a` appears in several spaced forms as separate codes; found {a_forms:?}"
    );
}

// ---------------------------------------------------------------------------
// SMAZ
// ---------------------------------------------------------------------------

/// Hand-assembled streams exercising all three opcodes.
#[test]
fn b_smaz_decompresses_each_opcode() {
    // Codebook entries: 1 = "the", 0 = " ", 5 = "of".
    let stream = [1u8, 0, 5];
    assert_eq!(smaz_decompress(&stream, 6).as_deref(), Some("the of"));

    // 254 = the next byte verbatim.
    let stream = [254u8, b'Z'];
    assert_eq!(smaz_decompress(&stream, 1).as_deref(), Some("Z"));

    // ⚠️ 255 = a verbatim run whose length byte is stored as `len - 1`.
    // Reading it as a plain length drops the last byte of every run, which is
    // the single easiest thing to get wrong in this codec.
    let stream = [255u8, 2, b'x', b'y', b'z'];
    assert_eq!(smaz_decompress(&stream, 3).as_deref(), Some("xyz"));

    // A run of exactly one byte is encoded with a length byte of 0.
    let stream = [255u8, 0, b'q'];
    assert_eq!(smaz_decompress(&stream, 1).as_deref(), Some("q"));

    // Mixed.
    let stream = [1u8, 254, b'!', 255, 1, b'o', b'k'];
    assert_eq!(smaz_decompress(&stream, 6).as_deref(), Some("the!ok"));
}

/// Malformed input returns `None` instead of panicking or overrunning.
///
/// ⚠️ Load-bearing for the boot path: a truncated `.translation` has to degrade
/// to "no translation" and let the project start, the way an unreadable file
/// already does.
#[test]
fn c_smaz_rejects_truncated_and_overlong_streams() {
    // 254 with no following byte.
    assert_eq!(smaz_decompress(&[254u8], 1), None);
    // 255 with no length byte.
    assert_eq!(smaz_decompress(&[255u8], 1), None);
    // 255 claiming 4 bytes with only 2 present.
    assert_eq!(smaz_decompress(&[255u8, 3, b'a', b'b'], 4), None);
    // Output would exceed the declared uncompressed length.
    assert_eq!(smaz_decompress(&[1u8, 1, 1], 3), None);
}

// ---------------------------------------------------------------------------
// The hash
// ---------------------------------------------------------------------------

/// The bucket hash, pinned to values computed OUTSIDE this implementation.
///
/// ⚠️ These constants were produced by transcribing the C algorithm into Python
/// and running it, not by printing what this module returns. A "reference"
/// helper written in Rust beside the implementation would be the same code
/// twice and could not catch a shared misreading — which is the whole failure
/// mode a ported codec has.
#[test]
fn d_hash_matches_values_computed_from_the_c_algorithm() {
    assert_eq!(
        hash(0, ""),
        0x0100_0193,
        "empty key is just the seeded prime"
    );
    assert_eq!(hash(0, "HELLO"), 0x77A7_6E3D);
    assert_eq!(hash(0, "greeting.key"), 0xDA61_09D9);
    assert_eq!(
        hash(7, "HELLO"),
        0xC59A_DD69,
        "a non-zero seed is used as-is"
    );

    // ⚠️ Seed 0 is special-cased to the FNV prime, so these are the SAME value.
    // A bucket whose `func` is 0 must behave like the default seed rather than
    // like "no seeding".
    assert_eq!(hash(0, "KEY"), 0x027C_ABA0);
    assert_eq!(hash(0, "KEY"), hash(0x0100_0193, "KEY"));

    // 🔑 Hashes UTF-8 BYTES, not chars, and the multiply wraps. Iterating chars
    // would agree for ASCII and silently diverge for every accented key — which
    // is exactly the population a translation catalogue is full of.
    assert_eq!(hash(0, "é"), 0x7FE7_1937);
    assert_eq!(hash(0, "e"), 0x2602_7A0C);
    assert_ne!(hash(0, "é"), hash(0, "e"));
}

// ---------------------------------------------------------------------------
// The bucket lookup
// ---------------------------------------------------------------------------

/// Builds a one-bucket catalogue by hand, in the C `struct Bucket` layout.
///
/// `bucket_table` words: `[size, func, key, str_offset, comp_size, uncomp_size, …]`.
/// Entries are stored raw here (`comp_size == uncomp_size`), so this isolates
/// the lookup from the codec.
fn single_bucket(keys_and_values: &[(&str, &str)], func: u32) -> OptimizedTranslation {
    let mut strings: Vec<u8> = Vec::new();
    let mut bucket = vec![keys_and_values.len() as u32, func];
    for (key, value) in keys_and_values {
        let offset = strings.len() as u32;
        strings.extend_from_slice(value.as_bytes());
        bucket.push(hash(func, key));
        bucket.push(offset);
        bucket.push(value.len() as u32);
        bucket.push(value.len() as u32);
    }
    OptimizedTranslation {
        locale: "fr".to_string(),
        // One slot, so every key lands in bucket 0 — a deliberate collision.
        hash_table: vec![0],
        bucket_table: bucket,
        strings,
    }
}

/// 🔑 Three keys sharing one first-level slot are told apart by the bucket's
/// second hash — the collision case the acceptance calls for.
#[test]
fn e_get_message_resolves_three_keys_from_one_collision_bucket() {
    let entries = [
        ("GREETING", "Bonjour"),
        ("FAREWELL", "Au revoir"),
        ("YES", "Oui"),
    ];
    let t = single_bucket(&entries, 0x9E37_79B9);

    for (key, expected) in entries {
        assert_eq!(
            t.get_message(key).as_deref(),
            Some(expected),
            "every key in the shared bucket must resolve; {key} did not"
        );
    }

    assert_eq!(
        t.get_message("ABSENT"),
        None,
        "a key that is not in the catalogue must return None — that is what \
         makes tr() fall back to the key itself"
    );
}

/// ⚠️ The second hash must use the bucket's own `func`, not the first seed.
///
/// The control for the test above: with `func` reused as 0 the lookup still
/// finds the bucket, so a build that hashed twice with the default seed would
/// pass every "key resolves" assertion while being wrong for any real file. A
/// bucket whose entries were keyed with a DIFFERENT `func` than the bucket
/// advertises must therefore miss.
#[test]
fn f_a_bucket_keyed_with_the_wrong_func_does_not_match() {
    let mut t = single_bucket(&[("GREETING", "Bonjour")], 0x1234_5678);
    assert_eq!(t.get_message("GREETING").as_deref(), Some("Bonjour"));

    // Advertise a different `func` while leaving the element keys as they were.
    t.bucket_table[1] = 0x8765_4321;
    assert_eq!(
        t.get_message("GREETING"),
        None,
        "the element key is hash(func, text); changing func must stop it matching"
    );
}

/// A compressed entry (`comp_size != uncomp_size`) is run through SMAZ.
#[test]
fn g_a_compressed_entry_is_decompressed_on_read() {
    // "the of" as SMAZ: codes 1 (" the" -> "the"), 0 (" "), 5 ("of").
    let compressed = [1u8, 0, 5];
    let uncompressed_len = "the of".len() as u32;

    let func = 0x55u32;
    let key = "PHRASE";
    let t = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        bucket_table: vec![
            1,
            func,
            hash(func, key),
            0,
            compressed.len() as u32,
            uncompressed_len,
        ],
        strings: compressed.to_vec(),
    };

    assert_eq!(
        t.get_message(key).as_deref(),
        Some("the of"),
        "an entry whose comp_size differs from uncomp_size must be SMAZ-decoded"
    );
}

/// An empty or corrupt catalogue answers `None` rather than panicking.
#[test]
fn h_a_corrupt_catalogue_degrades_instead_of_panicking() {
    let empty = OptimizedTranslation::default();
    assert_eq!(empty.get_message("ANY"), None, "no hash table at all");

    // Slot points past the end of the bucket arena.
    let dangling = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        bucket_table: vec![9_999],
        strings: Vec::new(),
    };
    assert_eq!(dangling.get_message("ANY"), None, "truncated bucket");

    // Entry points past the end of the string arena.
    let func = 3u32;
    let bad_offset = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        bucket_table: vec![1, func, hash(func, "K"), 500, 4, 4],
        strings: b"abc".to_vec(),
    };
    assert_eq!(bad_offset.get_message("K"), None, "offset past the strings");

    // 0xFFFFFFFF is the "nothing hashes here" sentinel, not a bucket index.
    let sentinel = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0xFFFF_FFFF],
        bucket_table: Vec::new(),
        strings: Vec::new(),
    };
    assert_eq!(sentinel.get_message("ANY"), None, "empty slot sentinel");
}

/// 🔑 Godot stores the NUL terminator, and counts it in `uncomp_size`.
///
/// ⚠️ This is the one thing a hand-built fixture gets wrong by default, and a
/// decoder/encoder round-trip could never catch — both halves would simply
/// agree on the wrong length. `generate()` writes `orig_len = src_s.size()`
/// where `src_s` is a `CharString`, and `ustring.h:182` defines `size()` as the
/// buffer length **including** the terminating NUL (`length()` is `size() - 1`).
/// The writer's empty-string case says the same thing outright: `orig_len = 1`
/// with a single `0` byte.
///
/// So a real `.translation` stores `"Oui\0"` with `uncomp_size == 4`. Taking
/// that at face value returns a string with a trailing NUL — which compares
/// unequal to `"Oui"`, renders as a stray glyph, and would have shipped.
#[test]
fn j_a_godot_sized_entry_drops_its_nul_terminator() {
    let func = 0x21u32;
    let key = "YES";

    // Stored exactly as Godot writes it: the bytes INCLUDE the terminator and
    // the length counts it.
    let stored: &[u8] = b"Oui\0";
    assert_eq!(stored.len(), 4, "three characters plus the NUL");

    let t = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        bucket_table: vec![1, func, hash(func, key), 0, 4, 4],
        strings: stored.to_vec(),
    };

    assert_eq!(
        t.get_message(key).as_deref(),
        Some("Oui"),
        "the CharString terminator must not survive into the message"
    );

    // And the same through the compressed path, where Godot's own decoder stops
    // at the NUL via `String::utf8(get_data())`.
    // SMAZ: 254 = verbatim byte. "Oui\0" verbatim is 4 escaped bytes.
    let compressed: Vec<u8> = vec![254, b'O', 254, b'u', 254, b'i', 254, 0];
    let t = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        bucket_table: vec![1, func, hash(func, key), 0, compressed.len() as u32, 4],
        strings: compressed,
    };
    assert_eq!(
        t.get_message(key).as_deref(),
        Some("Oui"),
        "compressed entries carry the terminator too — it is part of the \
         compressed payload, because the ORIGINAL buffer included it"
    );

    // A message with no terminator (as a hand-built fixture would store it) is
    // returned unchanged — stripping must not eat a real character.
    let t = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        bucket_table: vec![1, func, hash(func, key), 0, 3, 3],
        strings: b"Oui".to_vec(),
    };
    assert_eq!(t.get_message(key).as_deref(), Some("Oui"));
}

/// The empty translation, which `generate()` special-cases explicitly.
///
/// ⚠️ Godot does not store zero bytes for an empty message. The writer's `else`
/// branch sets `orig_len = 1` and writes a single `0` byte, so the entry looks
/// like `comp_size == uncomp_size == 1` over one NUL. Decoding that must give
/// `Some("")` — an empty translation the author deliberately set — and NOT
/// `None`, which would make `tr()` fall back to the key and put the raw
/// identifier on screen.
#[test]
fn k_an_empty_message_decodes_to_an_empty_string_not_a_miss() {
    let func = 0x31u32;
    let key = "BLANK";
    let t = OptimizedTranslation {
        locale: "fr".to_string(),
        hash_table: vec![0],
        // orig_len = 1, comp_size = 1, over the single NUL byte the writer emits.
        bucket_table: vec![1, func, hash(func, key), 0, 1, 1],
        strings: vec![0u8],
    };

    assert_eq!(
        t.get_message(key).as_deref(),
        Some(""),
        "an authored empty translation must decode to the empty string, not a \
         lookup miss — a miss would surface the key itself to the user"
    );
}

/// The documented asymmetry: the source keys are gone, the translations are not.
///
/// ⚠️ `get_message_list` / `get_message_count` returning empty and 0 is the
/// specified behaviour, not a stub — the format keys on a hash of the source
/// text and never stores the text. `get_translated_message_list` still works,
/// because the translated strings ARE stored.
#[test]
fn i_message_list_is_empty_but_translated_list_is_not() {
    let t = single_bucket(&[("GREETING", "Bonjour"), ("YES", "Oui")], 0x11);

    assert!(
        t.get_message_list().is_empty(),
        "OptimizedTranslation does not store the source texts"
    );
    assert_eq!(t.get_message_count(), 0);

    let mut translated = t.get_translated_message_list();
    translated.sort();
    assert_eq!(
        translated,
        vec!["Bonjour".to_string(), "Oui".to_string()],
        "the translations themselves are stored and must be listable"
    );
}
