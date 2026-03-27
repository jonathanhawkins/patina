//! Fuzz and property-based tests for the binary `.res` format loader and
//! the PCK archive parser.
//!
//! Tests that `parse_res_header`, `load_res_binary`, `is_res_binary`,
//! `PckArchive::from_bytes`, and `PckResourceLoader::from_bytes` never panic on
//! arbitrary input, and that well-formed inputs parse correctly.

use gdresource::res_loader::{is_res_binary, load_res_binary, parse_res_header, RES_MAGIC};
use gdresource::pck::{PckArchive, PckResourceLoader, PCK_MAGIC};
use proptest::prelude::*;

// ===========================================================================
// Helpers: build valid binary .res headers for property-based tests
// ===========================================================================

/// Build a binary .res header from parameterized fields.
fn build_res_header(
    big_endian: bool,
    use_64bit: bool,
    major: u32,
    minor: u32,
    format_ver: u32,
    resource_type: &[u8],
) -> Vec<u8> {
    let mut data = Vec::with_capacity(128);
    data.extend_from_slice(RES_MAGIC);

    // Endian and 64bit flags always written as LE.
    data.extend_from_slice(&(big_endian as u32).to_le_bytes());
    data.extend_from_slice(&(use_64bit as u32).to_le_bytes());

    let write_u32 = |v: u32| -> [u8; 4] {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };

    data.extend_from_slice(&write_u32(major));
    data.extend_from_slice(&write_u32(minor));
    data.extend_from_slice(&write_u32(format_ver));

    // Resource type: length-prefixed, 4-byte aligned.
    data.extend_from_slice(&write_u32(resource_type.len() as u32));
    data.extend_from_slice(resource_type);
    let padding = (4 - (resource_type.len() % 4)) % 4;
    data.extend(std::iter::repeat(0u8).take(padding));

    data
}

/// Build a minimal valid PCK archive with the given file entries.
///
/// The parser reads path_len bytes exactly (no alignment padding on paths),
/// then 8 bytes offset, 8 bytes size, 16 bytes md5.
fn build_pck_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(PCK_MAGIC);
    buf.extend_from_slice(&2u32.to_le_bytes()); // format version
    buf.extend_from_slice(&4u32.to_le_bytes()); // ver_major
    buf.extend_from_slice(&3u32.to_le_bytes()); // ver_minor
    buf.extend_from_slice(&0u32.to_le_bytes()); // ver_patch
    buf.extend_from_slice(&0u32.to_le_bytes()); // flags

    // Reserved: 16 x u32 = 64 bytes
    buf.extend(std::iter::repeat(0u8).take(64));

    // File count
    buf.extend_from_slice(&(files.len() as u32).to_le_bytes());

    // Calculate where file data starts: after all directory entries.
    // Each entry: 4 (path_len) + path_bytes (no padding) + 8 (offset) + 8 (size) + 16 (md5)
    let dir_start = buf.len();
    let mut dir_size = 0usize;
    for (path, _) in files {
        dir_size += 4 + path.as_bytes().len() + 8 + 8 + 16;
    }

    let data_start = dir_start + dir_size;
    // Align to 64 bytes.
    let data_start_aligned = (data_start + 63) & !63;

    // Write directory entries.
    let mut file_offset = data_start_aligned as u64;
    for (path, content) in files {
        let path_bytes = path.as_bytes();

        buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(path_bytes);

        buf.extend_from_slice(&file_offset.to_le_bytes());
        buf.extend_from_slice(&(content.len() as u64).to_le_bytes());
        buf.extend(std::iter::repeat(0u8).take(16)); // md5 zeroes

        let aligned_size = (content.len() as u64 + 63) & !63;
        file_offset += aligned_size.max(64); // at least 64 byte alignment
    }

    // Pad to data start alignment.
    while buf.len() < data_start_aligned {
        buf.push(0);
    }

    // Write file data (each aligned to 64 bytes).
    for (_, content) in files {
        buf.extend_from_slice(content);
        let total = content.len().max(64);
        let aligned_size = (total + 63) & !63;
        let pad = aligned_size - content.len();
        buf.extend(std::iter::repeat(0u8).take(pad));
    }

    buf
}

// ===========================================================================
// parse_res_header: must never panic
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Completely random bytes must not panic.
    #[test]
    fn res_header_random_bytes_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..1000)) {
        let _ = parse_res_header(&bytes);
    }

    /// Bytes starting with RSRC magic but random tail must not panic.
    #[test]
    fn res_header_magic_prefix_random_tail(tail in prop::collection::vec(any::<u8>(), 0..500)) {
        let mut data = b"RSRC".to_vec();
        data.extend_from_slice(&tail);
        let _ = parse_res_header(&data);
    }

    /// Valid headers with fuzzed version numbers must not panic.
    #[test]
    fn res_header_fuzzed_versions(
        big_endian in any::<bool>(),
        use_64bit in any::<bool>(),
        major in any::<u32>(),
        minor in any::<u32>(),
        format_ver in any::<u32>(),
    ) {
        let data = build_res_header(big_endian, use_64bit, major, minor, format_ver, b"Resource");
        let result = parse_res_header(&data);
        prop_assert!(result.is_ok(), "valid header should parse: {:?}", result.err());
        let header = result.unwrap();
        prop_assert_eq!(header.big_endian, big_endian);
        prop_assert_eq!(header.use_64bit, use_64bit);
    }

    /// Headers with arbitrary type strings must not panic.
    #[test]
    fn res_header_arbitrary_type_string(
        type_bytes in prop::collection::vec(any::<u8>(), 0..200),
    ) {
        let data = build_res_header(false, false, 4, 3, 5, &type_bytes);
        let result = parse_res_header(&data);
        prop_assert!(result.is_ok(), "header with any type bytes should parse: {:?}", result.err());
    }

    /// Headers with very large type_len field must not panic or OOM.
    #[test]
    fn res_header_huge_type_len(fake_len in 0u32..u32::MAX) {
        let mut data = b"RSRC".to_vec();
        data.extend_from_slice(&0u32.to_le_bytes()); // big_endian = false
        data.extend_from_slice(&0u32.to_le_bytes()); // use_64bit = false
        data.extend_from_slice(&4u32.to_le_bytes()); // major
        data.extend_from_slice(&3u32.to_le_bytes()); // minor
        data.extend_from_slice(&5u32.to_le_bytes()); // format_version
        data.extend_from_slice(&fake_len.to_le_bytes()); // type_len (possibly huge)
        // Only 16 bytes of actual type data follow.
        data.extend_from_slice(b"ShortTypeString!");
        let _ = parse_res_header(&data);
    }

    /// Truncation at every byte offset must not panic.
    #[test]
    fn res_header_truncated_at_every_offset(offset in 0usize..50) {
        let full = build_res_header(false, false, 4, 3, 5, b"Resource");
        let truncated = &full[..offset.min(full.len())];
        let _ = parse_res_header(truncated);
    }
}

// ===========================================================================
// load_res_binary: must never panic
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Random bytes must not panic load_res_binary.
    #[test]
    fn load_res_binary_random_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..500)) {
        let _ = load_res_binary(&bytes, "fuzz://random.res");
    }

    /// Valid headers passed to load_res_binary must return Unsupported, not panic.
    #[test]
    fn load_res_binary_valid_header_returns_unsupported(
        resource_type in "[A-Z][a-zA-Z0-9]{0,30}",
    ) {
        let data = build_res_header(false, false, 4, 3, 5, resource_type.as_bytes());
        let result = load_res_binary(&data, "fuzz://typed.res");
        prop_assert!(result.is_err());
    }
}

// ===========================================================================
// is_res_binary: must never panic
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// is_res_binary must never panic on any input.
    #[test]
    fn is_res_binary_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..100)) {
        let _ = is_res_binary(&bytes);
    }
}

// ===========================================================================
// PckArchive: must never panic
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Completely random bytes must not panic the PCK parser.
    #[test]
    fn pck_archive_random_bytes_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..1000)) {
        let _ = PckArchive::from_bytes(&bytes);
    }

    /// Bytes starting with GDPC magic but random tail must not panic.
    #[test]
    fn pck_archive_magic_prefix_random_tail(tail in prop::collection::vec(any::<u8>(), 0..500)) {
        let mut data = b"GDPC".to_vec();
        data.extend_from_slice(&tail);
        let _ = PckArchive::from_bytes(&data);
    }

    /// PCK header with fuzzed file_count must not panic or OOM.
    #[test]
    fn pck_archive_fuzzed_file_count(count in any::<u32>()) {
        let mut data = Vec::new();
        data.extend_from_slice(PCK_MAGIC);
        data.extend_from_slice(&2u32.to_le_bytes()); // format version
        data.extend_from_slice(&4u32.to_le_bytes()); // ver_major
        data.extend_from_slice(&3u32.to_le_bytes()); // ver_minor
        data.extend_from_slice(&0u32.to_le_bytes()); // ver_patch
        data.extend_from_slice(&0u32.to_le_bytes()); // flags
        data.extend(std::iter::repeat(0u8).take(64)); // reserved
        data.extend_from_slice(&count.to_le_bytes()); // file_count (possibly huge)
        // No actual file entries — just the count.
        let _ = PckArchive::from_bytes(&data);
    }

    /// Truncation at every offset of a valid PCK must not panic.
    #[test]
    fn pck_archive_truncated_at_every_offset(offset in 0usize..200) {
        let full = build_pck_archive(&[("res://icon.png", b"PNG_DATA_HERE")]);
        let truncated = &full[..offset.min(full.len())];
        let _ = PckArchive::from_bytes(truncated);
    }
}

// ===========================================================================
// PckResourceLoader: must never panic
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Random bytes must not panic PckResourceLoader.
    #[test]
    fn pck_loader_random_bytes_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..500)) {
        let _ = PckResourceLoader::from_bytes(bytes);
    }
}

// ===========================================================================
// Correctness: valid .res headers parse correctly
// ===========================================================================

#[test]
fn res_header_le_standard_godot4() {
    let data = build_res_header(false, false, 4, 3, 5, b"Theme");
    let h = parse_res_header(&data).unwrap();
    assert!(!h.big_endian);
    assert!(!h.use_64bit);
    assert_eq!(h.version_major, 4);
    assert_eq!(h.version_minor, 3);
    assert_eq!(h.format_version, 5);
    assert_eq!(h.resource_type, "Theme");
}

#[test]
fn res_header_be_with_64bit() {
    let data = build_res_header(true, true, 4, 1, 2, b"PackedScene");
    let h = parse_res_header(&data).unwrap();
    assert!(h.big_endian);
    assert!(h.use_64bit);
    assert_eq!(h.resource_type, "PackedScene");
}

#[test]
fn res_header_empty_type_string() {
    let data = build_res_header(false, false, 4, 0, 1, b"");
    let h = parse_res_header(&data).unwrap();
    assert_eq!(h.resource_type, "");
}

#[test]
fn res_header_type_with_null_bytes() {
    let data = build_res_header(false, false, 4, 0, 1, b"Res\0\0\0\0");
    let h = parse_res_header(&data).unwrap();
    // from_utf8_lossy + trim_end_matches('\0') should yield "Res"
    assert_eq!(h.resource_type, "Res");
}

#[test]
fn res_header_non_utf8_type() {
    let data = build_res_header(false, false, 4, 0, 1, &[0xFF, 0xFE, 0x80, 0x90]);
    let h = parse_res_header(&data).unwrap();
    // from_utf8_lossy replaces invalid bytes with replacement char
    assert!(!h.resource_type.is_empty());
}

// ===========================================================================
// Correctness: valid PCK archives parse correctly
// ===========================================================================

#[test]
fn pck_archive_single_file() {
    let data = build_pck_archive(&[("res://test.txt", b"hello world")]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert!(archive.file_paths().contains(&"res://test.txt"));
    let extracted = archive.extract_file(&data, "res://test.txt").unwrap();
    assert_eq!(extracted, b"hello world");
}

#[test]
fn pck_archive_multiple_files() {
    let data = build_pck_archive(&[
        ("res://a.txt", b"aaa"),
        ("res://b.txt", b"bbb"),
        ("res://c.txt", b"ccc"),
    ]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 3);
    assert_eq!(archive.extract_file(&data, "res://a.txt").unwrap(), b"aaa");
    assert_eq!(archive.extract_file(&data, "res://c.txt").unwrap(), b"ccc");
}

#[test]
fn pck_archive_empty() {
    let data = build_pck_archive(&[]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 0);
}

#[test]
fn pck_loader_extracts_files() {
    let data = build_pck_archive(&[("res://icon.png", b"FAKE_PNG_DATA")]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.extract_raw("res://icon.png").unwrap(), b"FAKE_PNG_DATA");
    assert_eq!(loader.file_count(), 1);
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn res_empty_data() {
    assert!(parse_res_header(&[]).is_err());
    assert!(!is_res_binary(&[]));
}

#[test]
fn res_exactly_4_bytes_wrong_magic() {
    assert!(parse_res_header(b"NOPE").is_err());
}

#[test]
fn res_exactly_4_bytes_correct_magic() {
    // Has magic but truncated before endian flags.
    assert!(parse_res_header(b"RSRC").is_err());
}

#[test]
fn pck_exactly_4_bytes_correct_magic() {
    assert!(PckArchive::from_bytes(b"GDPC").is_err());
}

#[test]
fn pck_file_with_empty_path() {
    let data = build_pck_archive(&[("", b"data")]);
    // Should parse without panic, even if path is empty.
    let _ = PckArchive::from_bytes(&data);
}

#[test]
fn pck_file_with_very_long_path() {
    let long_path = format!("res://{}", "a".repeat(1000));
    let data = build_pck_archive(&[(&long_path, b"data")]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 1);
}

#[test]
fn load_res_binary_always_returns_unsupported_for_valid_data() {
    let data = build_res_header(false, false, 4, 3, 5, b"AnimationLibrary");
    let err = load_res_binary(&data, "res://lib.res").unwrap_err().to_string();
    assert!(err.contains("not yet supported"));
    assert!(err.contains("AnimationLibrary"));
}
