//! pat-nzgdu.224: `ZIPPacker` → `ZIPReader` archive roundtrip.
//!
//! Covers the three things Godot's archive classes have to get right:
//!
//! 1. `ZIPReader` / `ZIPPacker` are ClassDB-registered under `RefCounted`, so
//!    script/`.tscn` references to them resolve.
//! 2. Files written through the packer read back byte-identical through the
//!    reader — including a compressible entry (which deflates) and binary data.
//! 3. `APPEND_ADDINZIP` preserves the entries already in the archive instead of
//!    truncating them, which is the whole point of append mode.

use gdobject::class_db::{class_exists, get_class_info, is_parent_class, register_io_classes};
use gdresource::{ZipAppend, ZipPacker, ZipReader};

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(name);
    p
}

fn write_entry(packer: &mut ZipPacker, name: &str, data: &[u8]) {
    packer.start_file(name).expect("start_file should succeed");
    packer.write_file(data).expect("write_file should succeed");
    packer.close_file().expect("close_file should succeed");
}

#[test]
fn zip_packer_reader_roundtrip() {
    // ---- 1. ClassDB registration -----------------------------------------
    register_io_classes();

    for class in ["ZIPReader", "ZIPPacker"] {
        assert!(
            class_exists(class),
            "{class} should be registered in ClassDB"
        );
        let info = get_class_info(class).expect("registered class should have info");
        assert_eq!(
            info.parent_class, "RefCounted",
            "{class} should inherit RefCounted, as in Godot"
        );
        assert!(is_parent_class(class, "RefCounted"));
        assert!(is_parent_class(class, "Object"));
    }

    // The script-facing method surface Godot exposes. Bind the ClassInfo first:
    // borrowing straight off the temporary would drop it while still in use.
    let reader_info = get_class_info("ZIPReader").unwrap();
    let reader_methods: Vec<&str> = reader_info
        .methods
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    for m in ["open", "close", "get_files", "file_exists", "read_file"] {
        assert!(
            reader_methods.contains(&m),
            "ZIPReader should expose {m}(), got {reader_methods:?}"
        );
    }
    let packer_info = get_class_info("ZIPPacker").unwrap();
    let packer_methods: Vec<&str> = packer_info
        .methods
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    for m in ["open", "start_file", "write_file", "close_file", "close"] {
        assert!(
            packer_methods.contains(&m),
            "ZIPPacker should expose {m}(), got {packer_methods:?}"
        );
    }
    // ZIPPacker.compression_level defaults to -1 (zlib's default level).
    assert!(
        packer_info
            .properties
            .iter()
            .any(|p| p.name == "compression_level"),
        "ZIPPacker should expose the compression_level property"
    );

    // ---- 2. Two-file roundtrip -------------------------------------------
    let path = temp_path("pat_nzgdu224_roundtrip.zip");
    let path_str = path.to_str().unwrap();
    let _ = std::fs::remove_file(&path);

    // Repetitive text compresses (deflate path); the bytes exercise a
    // non-UTF-8 payload including NUL.
    let text: Vec<u8> = "the quick brown fox jumps over the lazy dog\n"
        .repeat(64)
        .into_bytes();
    let binary: Vec<u8> = (0u16..=255).map(|b| b as u8).cycle().take(1024).collect();

    let mut packer = ZipPacker::new();
    packer
        .open(path_str, ZipAppend::Create)
        .expect("open should succeed");
    write_entry(&mut packer, "docs/readme.txt", &text);
    write_entry(&mut packer, "data/blob.bin", &binary);
    packer.close().expect("close should succeed");

    let mut reader = ZipReader::new();
    reader.open(path_str).expect("open should succeed");

    let files = reader.get_files();
    assert_eq!(
        files,
        vec!["docs/readme.txt".to_string(), "data/blob.bin".to_string()],
        "get_files() should list both entries in write order"
    );
    assert!(reader.file_exists("docs/readme.txt", true));
    assert!(reader.file_exists("data/blob.bin", true));
    assert!(!reader.file_exists("nope.txt", true));

    assert_eq!(
        reader.read_file("docs/readme.txt", true).unwrap(),
        text,
        "text entry should read back byte-identical"
    );
    assert_eq!(
        reader.read_file("data/blob.bin", true).unwrap(),
        binary,
        "binary entry should read back byte-identical"
    );

    // Reading a missing entry errors rather than returning empty bytes.
    assert!(reader.read_file("nope.txt", true).is_err());

    // The archive really is compressed, not just stored: a 2752-byte
    // repetitive text plus 1024 cycling bytes must not fit in a file that
    // large if deflate ran.
    let archive_len = std::fs::metadata(&path).unwrap().len();
    assert!(
        archive_len < (text.len() + binary.len()) as u64,
        "archive ({archive_len} bytes) should be smaller than its {} bytes of input",
        text.len() + binary.len()
    );

    // ---- 3. Append mode preserves existing entries ------------------------
    let mut appender = ZipPacker::new();
    appender
        .open(path_str, ZipAppend::AddInZip)
        .expect("append-mode open should succeed");
    write_entry(&mut appender, "docs/added.txt", b"appended entry");
    appender.close().expect("close should succeed");

    let mut reader2 = ZipReader::new();
    reader2.open(path_str).expect("reopen should succeed");
    assert_eq!(
        reader2.get_files(),
        vec![
            "docs/readme.txt".to_string(),
            "data/blob.bin".to_string(),
            "docs/added.txt".to_string(),
        ],
        "append mode must preserve the original entries and add the new one"
    );
    // The pre-existing entries still decompress correctly after the rewrite —
    // i.e. append did not corrupt their local-header offsets.
    assert_eq!(reader2.read_file("docs/readme.txt", true).unwrap(), text);
    assert_eq!(reader2.read_file("data/blob.bin", true).unwrap(), binary);
    assert_eq!(
        reader2.read_file("docs/added.txt", true).unwrap(),
        b"appended entry"
    );

    // A fresh Create on the same path replaces the archive wholesale.
    let mut recreate = ZipPacker::new();
    recreate.open(path_str, ZipAppend::Create).unwrap();
    write_entry(&mut recreate, "only.txt", b"replaced");
    recreate.close().unwrap();

    let mut reader3 = ZipReader::new();
    reader3.open(path_str).unwrap();
    assert_eq!(
        reader3.get_files(),
        vec!["only.txt".to_string()],
        "APPEND_CREATE should discard the previous archive"
    );

    let _ = std::fs::remove_file(&path);
}
