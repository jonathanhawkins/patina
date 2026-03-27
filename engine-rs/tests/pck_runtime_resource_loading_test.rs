//! pat-zq6st: Runtime resource loading from PCK archives.
//!
//! Validates:
//! 1. Pack and unpack roundtrip for single and multiple files
//! 2. PCK header parsing (magic, version, file count)
//! 3. File extraction by res:// path
//! 4. Sorted file listing
//! 5. Alignment and offset correctness
//! 6. Empty archives, empty files, large files
//! 7. PckResourceLoader loading .tres and .tscn resources from archive
//! 8. PckResourceLoader raw byte extraction
//! 9. Non-text resources loaded as PackedFile with byte_size metadata
//! 10. Error handling: invalid magic, truncated data, missing files, invalid UTF-8
//! 11. Integration with ResourceCache (deduplication)
//! 12. ClassDB registration of PCKPacker and ProjectSettings.load_resource_pack

use gdresource::pck::{PckArchive, PckPacker, PckResourceLoader, PCK_MAGIC};
use gdresource::loader::ResourceLoader;
use std::sync::Arc;

// ── Pack/unpack roundtrip ────────────────────────────────────────────

#[test]
fn empty_archive_roundtrip() {
    let packer = PckPacker::new();
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 0);
    assert_eq!(archive.header.format_version, 2);
}

#[test]
fn single_file_roundtrip() {
    let mut packer = PckPacker::new();
    packer.add_file("res://hello.txt", b"Hello PCK!".to_vec());
    let data = packer.pack().unwrap();

    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert!(archive.has_file("res://hello.txt"));

    let content = archive.extract_file(&data, "res://hello.txt").unwrap();
    assert_eq!(content, b"Hello PCK!");
}

#[test]
fn multiple_files_roundtrip() {
    let mut packer = PckPacker::new();
    packer.add_file("res://scripts/player.gd", b"extends CharacterBody2D".to_vec());
    packer.add_file("res://scenes/main.tscn", b"[gd_scene]".to_vec());
    packer.add_file("res://icon.png", vec![0x89, 0x50, 0x4E, 0x47]);
    let data = packer.pack().unwrap();

    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 3);

    assert_eq!(
        archive.extract_file(&data, "res://scripts/player.gd").unwrap(),
        b"extends CharacterBody2D"
    );
    assert_eq!(
        archive.extract_file(&data, "res://icon.png").unwrap(),
        &[0x89, 0x50, 0x4E, 0x47]
    );
}

// ── Header parsing ───────────────────────────────────────────────────

#[test]
fn header_version_preserved() {
    let packer = PckPacker::new().with_version(4, 6, 1);
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.header.ver_major, 4);
    assert_eq!(archive.header.ver_minor, 6);
    assert_eq!(archive.header.ver_patch, 1);
}

#[test]
fn custom_version() {
    let packer = PckPacker::new().with_version(5, 0, 0);
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.header.ver_major, 5);
}

#[test]
fn magic_bytes_correct() {
    let data = PckPacker::new().pack().unwrap();
    assert_eq!(&data[0..4], PCK_MAGIC);
}

// ── File listing ─────────────────────────────────────────────────────

#[test]
fn file_paths_sorted() {
    let mut packer = PckPacker::new();
    packer.add_file("res://z.txt", vec![]);
    packer.add_file("res://a.txt", vec![]);
    packer.add_file("res://m.txt", vec![]);
    let data = packer.pack().unwrap();

    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_paths(), vec!["res://a.txt", "res://m.txt", "res://z.txt"]);
}

#[test]
fn has_file_check() {
    let mut packer = PckPacker::new();
    packer.add_file("res://exists.txt", vec![]);
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert!(archive.has_file("res://exists.txt"));
    assert!(!archive.has_file("res://missing.txt"));
}

// ── Alignment ────────────────────────────────────────────────────────

#[test]
fn entry_offsets_aligned_to_64() {
    let mut packer = PckPacker::new();
    packer.add_file("res://small.txt", b"hi".to_vec());
    packer.add_file("res://other.txt", b"there".to_vec());
    let data = packer.pack().unwrap();

    let archive = PckArchive::from_bytes(&data).unwrap();
    for entry in archive.entries.values() {
        assert_eq!(entry.offset % 64, 0, "offset {} not 64-aligned", entry.offset);
    }
}

// ── Edge cases ───────────────────────────────────────────────────────

#[test]
fn empty_file_in_archive() {
    let mut packer = PckPacker::new();
    packer.add_file("res://empty.txt", vec![]);
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    let content = archive.extract_file(&data, "res://empty.txt").unwrap();
    assert!(content.is_empty());
}

#[test]
fn large_file_roundtrip() {
    let big = vec![0xAB_u8; 50_000];
    let mut packer = PckPacker::new();
    packer.add_file("res://big.bin", big.clone());
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    let content = archive.extract_file(&data, "res://big.bin").unwrap();
    assert_eq!(content.len(), 50_000);
    assert!(content.iter().all(|&b| b == 0xAB));
}

#[test]
fn total_data_size_sums_correctly() {
    let mut packer = PckPacker::new();
    packer.add_file("res://a.bin", vec![0u8; 100]);
    packer.add_file("res://b.bin", vec![0u8; 200]);
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.total_data_size(), 300);
}

#[test]
fn entry_metadata_accessible() {
    let mut packer = PckPacker::new();
    packer.add_file("res://script.gd", b"extends Node".to_vec());
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    let entry = archive.get_entry("res://script.gd").unwrap();
    assert_eq!(entry.path, "res://script.gd");
    assert_eq!(entry.size, 12);
    assert_eq!(entry.md5, [0u8; 16]); // not computed
}

// ── Error handling ───────────────────────────────────────────────────

#[test]
fn invalid_magic_rejected() {
    let mut data = PckPacker::new().pack().unwrap();
    data[0] = b'X';
    assert!(PckArchive::from_bytes(&data).is_err());
}

#[test]
fn truncated_data_rejected() {
    assert!(PckArchive::from_bytes(&[0u8; 10]).is_err());
}

#[test]
fn extract_nonexistent_file_returns_none() {
    let data = PckPacker::new().pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert!(archive.extract_file(&data, "res://nope.txt").is_none());
}

// ── PckResourceLoader ────────────────────────────────────────────────

#[test]
fn loader_loads_tres_resource() {
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "FromPCK"
value = 42
"#;
    let mut packer = PckPacker::new();
    packer.add_file("res://item.tres", tres.as_bytes().to_vec());
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let res = loader.load("res://item.tres").unwrap();
    assert_eq!(res.class_name, "Resource");
    assert_eq!(res.path, "res://item.tres");
}

#[test]
fn loader_loads_tscn_resource() {
    let tscn = r#"[gd_scene format=3 uid="uid://test"]

[node name="Root" type="Node2D"]
"#;
    let mut packer = PckPacker::new();
    packer.add_file("res://level.tscn", tscn.as_bytes().to_vec());
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let res = loader.load("res://level.tscn").unwrap();
    assert_eq!(res.path, "res://level.tscn");
}

#[test]
fn loader_non_text_returns_packed_file() {
    let mut packer = PckPacker::new();
    packer.add_file("res://icon.png", vec![0x89, 0x50, 0x4E, 0x47, 0x0D]);
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let res = loader.load("res://icon.png").unwrap();
    assert_eq!(res.class_name, "PackedFile");
    assert_eq!(
        res.get_property("byte_size"),
        Some(&gdvariant::Variant::Int(5))
    );
}

#[test]
fn loader_extract_raw() {
    let content = b"raw binary data here";
    let mut packer = PckPacker::new();
    packer.add_file("res://data.bin", content.to_vec());
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.extract_raw("res://data.bin").unwrap(), content);
    assert!(loader.extract_raw("res://missing.bin").is_none());
}

#[test]
fn loader_has_file_and_count() {
    let mut packer = PckPacker::new();
    packer.add_file("res://a.txt", vec![1]);
    packer.add_file("res://b.txt", vec![2]);
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert!(loader.has_file("res://a.txt"));
    assert!(!loader.has_file("res://c.txt"));
    assert_eq!(loader.file_count(), 2);
}

#[test]
fn loader_file_paths_sorted() {
    let mut packer = PckPacker::new();
    packer.add_file("res://z.gd", vec![]);
    packer.add_file("res://a.gd", vec![]);
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.file_paths(), vec!["res://a.gd", "res://z.gd"]);
}

#[test]
fn loader_missing_file_returns_error() {
    let data = PckPacker::new().pack().unwrap();
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert!(loader.load("res://missing.tres").is_err());
}

#[test]
fn loader_invalid_utf8_tres_returns_error() {
    let mut packer = PckPacker::new();
    packer.add_file("res://bad.tres", vec![0xFF, 0xFE, 0x80]);
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert!(loader.load("res://bad.tres").is_err());
}

#[test]
fn loader_invalid_archive_bytes() {
    assert!(PckResourceLoader::from_bytes(vec![0, 1, 2, 3]).is_err());
}

#[test]
fn loader_header_metadata() {
    let packer = PckPacker::new().with_version(4, 6, 1);
    let data = packer.pack().unwrap();
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.header().ver_major, 4);
    assert_eq!(loader.header().ver_minor, 6);
}

// ── Cache integration ────────────────────────────────────────────────

#[test]
fn loader_with_cache_deduplicates() {
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Cached"
"#;
    let mut packer = PckPacker::new();
    packer.add_file("res://cached.tres", tres.as_bytes().to_vec());
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let mut cache = gdresource::cache::ResourceCache::new(loader);
    let first = cache.load("res://cached.tres").unwrap();
    let second = cache.load("res://cached.tres").unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

// ── Multi-resource workflow ──────────────────────────────────────────

#[test]
fn full_game_archive_workflow() {
    let scene = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]
"#;
    let resource = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Config"
"#;
    let script = b"extends Node2D\nfunc _ready():\n\tprint('hello')";
    let texture = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header

    let mut packer = PckPacker::new().with_version(4, 6, 1);
    packer.add_file("res://main.tscn", scene.as_bytes().to_vec());
    packer.add_file("res://config.tres", resource.as_bytes().to_vec());
    packer.add_file("res://scripts/main.gd", script.to_vec());
    packer.add_file("res://textures/icon.png", texture);
    let data = packer.pack().unwrap();

    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.file_count(), 4);

    // Load scene
    let scene_res = loader.load("res://main.tscn").unwrap();
    assert_eq!(scene_res.path, "res://main.tscn");

    // Load resource
    let config = loader.load("res://config.tres").unwrap();
    assert_eq!(config.class_name, "Resource");

    // Load script as packed file
    let script_res = loader.load("res://scripts/main.gd").unwrap();
    assert_eq!(script_res.class_name, "PackedFile");

    // Raw extraction
    let raw = loader.extract_raw("res://textures/icon.png").unwrap();
    assert_eq!(raw[0], 0x89);
}

// ── ClassDB registration ─────────────────────────────────────────────

#[test]
fn classdb_pck_packer_exists() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("PCKPacker"));
}

#[test]
fn classdb_pck_packer_has_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("PCKPacker", "pck_start"));
    assert!(gdobject::class_db::class_has_method("PCKPacker", "add_file"));
    assert!(gdobject::class_db::class_has_method("PCKPacker", "flush"));
}

#[test]
fn classdb_project_settings_has_load_resource_pack() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("ProjectSettings", "load_resource_pack"));
}
